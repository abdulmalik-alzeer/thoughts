use super::*;
use crate::settings::CodexSettings;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// An HTTP server on 127.0.0.1 that answers every request with `answer` and keeps what it was sent
/// (the request line and headers). `answer` = None: it never answers.
fn server(answer: Option<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(vec![]));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut c) = conn else { continue };
            let mut r = BufReader::new(c.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" { break; }
                head.push_str(&line);
            }
            s2.lock().unwrap().push(head);
            match &answer {
                Some(a) => { let _ = c.write_all(a.as_bytes()); }
                None => { std::thread::sleep(Duration::from_secs(8)); }
            }
        }
    });
    (addr, seen)
}

fn ok_json(body: &str) -> String {
    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

fn said(e: anyhow::Error) -> String { crate::tr::classify_in(false, &e).message }

#[test]
fn the_model_list_address_hangs_from_the_service_address() {
    assert_eq!(models_url("https://openrouter.ai/api", "claude"), "https://openrouter.ai/api/v1/models?limit=1000");
    assert_eq!(models_url("http://localhost:11434/", "claude"), "http://localhost:11434/v1/models?limit=1000");
    assert_eq!(models_url("http://localhost:11434/v1/", "codex"), "http://localhost:11434/v1/models");
    assert_eq!(models_url("https://openrouter.ai/api/v1", "codex"), "https://openrouter.ai/api/v1/models");
}

#[test]
fn a_service_answer_is_untrusted_names_are_checked_and_bounded() {
    let v = parse_models(br#"{"data":[{"id":"qwen3:32b"},{"id":"--dangerously-skip-permissions"},{"id":"a b"},{"id":"openai/gpt-x"},{"id":"qwen3:32b"},{"nope":1},{"id":""}]}"#).unwrap();
    assert_eq!(v, ["qwen3:32b", "openai/gpt-x"]);
    // Ollama's own shape.
    assert_eq!(parse_models(br#"{"models":[{"name":"llama3"}]}"#).unwrap(), ["llama3"]);
    let many = json!({"data": (0..2000).map(|i| json!({"id": format!("m{i}")})).collect::<Vec<_>>()}).to_string();
    assert_eq!(parse_models(many.as_bytes()).unwrap().len(), MODELS_MAX);
    for bad in [&b"<html>"[..], b"{}", b"{\"data\":5}", b"[]"] { assert!(parse_models(bad).is_err()); }
    assert!(said(parse_models(br#"{"data":[{"id":"--x"}]}"#).unwrap_err()).contains("no models"));
}

#[test]
fn loading_models_sends_the_key_only_as_asked_and_never_the_stand_in() {
    let (addr, seen) = server(Some(ok_json(r#"{"data":[{"id":"m1"}]}"#)));
    assert_eq!(fetch_models(&addr, "codex", Some("sk-secret"), Auth::Bearer, false).unwrap(), ["m1"]);
    assert_eq!(fetch_models(&addr, "codex", Some(NO_KEY), Auth::Bearer, false).unwrap(), ["m1"]);
    assert_eq!(fetch_models(&addr, "claude", None, Auth::Bearer, false).unwrap(), ["m1"]);
    let seen = seen.lock().unwrap().clone();
    assert!(seen[0].starts_with("GET /v1/models HTTP/1.1") && seen[0].to_lowercase().contains("authorization: bearer sk-secret"), "{}", seen[0]);
    for h in &seen[1..] { assert!(!h.to_lowercase().contains("authorization") && !h.contains(NO_KEY), "{h}"); }
    assert!(seen[2].starts_with("GET /v1/models?limit=1000 "), "{}", seen[2]);
}

#[test]
fn a_redirect_is_never_followed_so_the_key_stays_with_its_address() {
    let (other, other_seen) = server(Some(ok_json(r#"{"data":[{"id":"stolen"}]}"#)));
    let (addr, _) = server(Some(format!("HTTP/1.1 302 Found\r\nLocation: {other}/v1/models\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")));
    let e = fetch_models(&addr, "codex", Some("sk-secret"), Auth::Bearer, false).unwrap_err();
    assert!(said(e).contains("another address"));
    std::thread::sleep(Duration::from_millis(100));
    assert!(other_seen.lock().unwrap().is_empty(), "the redirect was followed");
}

#[test]
fn a_slow_a_huge_a_refusing_or_a_missing_list_is_said_plainly() {
    let (slow, _) = server(None);
    let t = std::time::Instant::now();
    assert!(said(fetch_models(&slow, "codex", None, Auth::Bearer, false).unwrap_err()).contains("5 seconds"));
    assert!(t.elapsed() < Duration::from_secs(7));
    let big = format!("{{\"data\":[{{\"id\":\"{}\"}}]}}", "x".repeat(MODELS_MAX_BYTES as usize + 10));
    let (huge, _) = server(Some(ok_json(&big)));
    assert!(said(fetch_models(&huge, "codex", None, Auth::Bearer, false).unwrap_err()).contains("too large"));
    let (no, _) = server(Some("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()));
    assert!(said(fetch_models(&no, "codex", Some("k"), Auth::Bearer, false).unwrap_err()).contains("did not accept"));
    let (none, _) = server(Some("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()));
    assert!(said(fetch_models(&none, "codex", None, Auth::Bearer, false).unwrap_err()).contains("does not list"));
    // Nobody there.
    let gone = { let l = TcpListener::bind("127.0.0.1:0").unwrap(); format!("http://127.0.0.1:{}", l.local_addr().unwrap().port()) };
    assert!(said(fetch_models(&gone, "codex", None, Auth::Bearer, false).unwrap_err()).contains("could not be reached"));
    // The address check comes first, and only it yields to full access.
    assert!(fetch_models("http://example.com", "codex", None, Auth::Bearer, true).is_err());
    assert!(fetch_models("http://user@localhost:1", "codex", None, Auth::Bearer, true).is_err());
}

#[test]
fn the_rules_of_a_provider_change() {
    let ok = |cur, url, key, no_key| decide(cur, url, key, no_key, false).unwrap();
    let refused = |cur, url, key, no_key| said(decide(cur, url, key, no_key, false).unwrap_err());
    // A key with its address.
    assert_eq!(ok((None, false, false), Some("https://a.example"), Some("k"), false), Change { has_api_token: true, keyless: false, drops_old_key: false, moved: true });
    // Another address without its key.
    assert!(refused((Some("https://a.example"), true, false), Some("https://b.example"), None, false).contains("Enter the key"));
    // "No key needed" only on this computer.
    assert!(refused((None, false, false), Some("https://a.example"), None, true).contains("this computer"));
    assert_eq!(ok((None, false, false), Some("http://localhost:11434"), None, true), Change { has_api_token: false, keyless: true, drops_old_key: false, moved: true });
    // Switching a keyed local service to "no key": its key goes.
    assert!(ok((Some("http://localhost:4000"), true, false), Some("http://localhost:4000"), None, true).drops_old_key);
    // A keyless service saved again as it is stays keyless…
    assert!(ok((Some("http://localhost:11434"), false, true), Some("http://localhost:11434"), None, false).keyless);
    // …but moved elsewhere it needs a key again.
    assert!(refused((Some("http://localhost:11434"), false, true), Some("https://b.example"), None, false).contains("Enter the key"));
    // A key typed for a keyless service replaces "no key".
    assert_eq!(ok((Some("http://localhost:11434"), false, true), Some("http://localhost:11434"), Some("k"), false), Change { has_api_token: true, keyless: false, drops_old_key: false, moved: false });
    // Back to the program's own set-up: the key goes.
    assert_eq!(ok((Some("https://a.example"), true, false), None, None, false), Change { has_api_token: false, keyless: false, drops_old_key: true, moved: true });
    // On the home network only with full access, and the key rules hold all the same.
    assert!(decide((None, false, false), Some("http://192.168.1.5:11434/v1"), None, true, true).unwrap().keyless);
    assert!(decide((None, false, false), Some("http://192.168.1.5:11434/v1"), None, true, false).is_err());
}

fn codex(url: Option<&str>, key: bool, keyless: bool) -> CodexSettings {
    CodexSettings { base_url: url.map(String::from), has_api_token: key, keyless, ..Default::default() }
}

#[test]
fn codex_runs_get_the_saved_key_the_stand_in_or_nothing() {
    let key = || Some("sk-codex".to_string());
    assert_eq!(codex_service_for(&codex(None, false, false), key).unwrap(), None);
    assert_eq!(codex_service_for(&codex(Some(" "), false, false), key).unwrap(), None);
    assert_eq!(codex_service_for(&codex(Some("https://openrouter.ai/api/v1"), true, false), key).unwrap(),
        Some(CodexService { base_url: "https://openrouter.ai/api/v1".into(), key: "sk-codex".into() }));
    assert_eq!(codex_service_for(&codex(Some("http://localhost:11434/v1"), false, true), || panic!("no key is read")).unwrap().unwrap().key, NO_KEY);
    // An address alone, a keyless remote address (a file edited by hand), a plain http address elsewhere: nothing runs.
    for c in [codex(Some("https://x.example/v1"), false, false), codex(Some("https://x.example/v1"), false, true), codex(Some("http://x.example/v1"), true, false)] {
        assert_eq!(codex_provider_of(&c), CodexProvider::Incomplete);
        assert_eq!(crate::tr::classify_in(false, &codex_service_for(&c, key).unwrap_err()).code, "provider_incomplete");
    }
    // A key that cannot be read stops the run; it never falls back to the sign-in.
    assert!(codex_service_for(&codex(Some("https://x.example/v1"), true, false), || None).is_err());
    // Debug output never shows the key.
    assert!(!format!("{:?}", CodexService { base_url: "u".into(), key: "sk-codex".into() }).contains("sk-codex"));
}

#[test]
fn a_codex_run_names_the_key_only_by_its_variable_and_keeps_it_from_commands() {
    let svc = CodexService { base_url: "http://localhost:11434/v1".into(), key: "sk-secret".into() };
    let a = codex_args(&svc, &["AWS_*".into()]);
    assert_eq!(a, [
        "model_provider=\"thoughts\"",
        "model_providers.thoughts.name=\"Thoughts: another provider\"",
        "model_providers.thoughts.base_url=\"http://localhost:11434/v1\"",
        "model_providers.thoughts.wire_api=\"responses\"",
        "model_providers.thoughts.env_key=\"THOUGHTS_PROVIDER_KEY\"",
        "shell_environment_policy.ignore_default_excludes=false",
        "shell_environment_policy.exclude=[\"AWS_*\",\"THOUGHTS_PROVIDER_KEY\"]",
    ]);
    assert!(a.iter().all(|x| !x.contains("sk-secret")));
    assert_eq!(user_excludes("[shell_environment_policy]\nexclude = [\"AWS_*\", \"MY_TOKEN\"]\n"), ["AWS_*", "MY_TOKEN"]);
    assert!(user_excludes("not toml [").is_empty() && user_excludes("model = \"x\"").is_empty());
    // The environment: the key in its variable, no inherited OpenAI key or address.
    let mut c = tokio::process::Command::new("codex");
    c.env("OPENAI_API_KEY", "sk-inherited");
    apply_codex_env(&mut c, Some(&svc));
    let envs: Vec<(String, Option<String>)> = c.as_std().get_envs().map(|(k, v)| (k.to_string_lossy().into(), v.map(|v| v.to_string_lossy().into()))).collect();
    assert!(envs.contains(&("THOUGHTS_PROVIDER_KEY".into(), Some("sk-secret".into()))));
    for name in CODEX_UNSET { assert!(envs.contains(&(name.to_string(), None)), "{name} is not removed"); }
    // Codex's own set-up: nothing is touched.
    let mut c = tokio::process::Command::new("codex");
    apply_codex_env(&mut c, None);
    assert_eq!(c.as_std().get_envs().count(), 0);
}

#[test]
fn every_codex_preset_passes_the_address_check_and_says_whether_it_needs_a_key() {
    for p in CODEX_PRESETS {
        assert!(crate::settings::check_base_url(p.base_url).is_ok(), "{}", p.id);
        if p.no_key { assert!(crate::settings::keyless_allowed(p.base_url, false), "{}", p.id); }
    }
}

#[test]
fn a_codex_test_tries_what_was_typed_and_the_saved_key_only_for_its_address() {
    let saved = codex(Some("https://a.example/v1"), true, false);
    let key = || Some("sk-saved".to_string());
    let t = |u: Option<&str>, k: Option<&str>, n: bool| codex_test_service(&saved, Some((u, k, n)), key, false);
    assert_eq!(t(Some("https://a.example/v1"), None, false).unwrap().unwrap().key, "sk-saved");
    assert_eq!(t(Some("https://a.example/v1"), Some("typed"), false).unwrap().unwrap().key, "typed");
    assert!(t(Some("https://b.example/v1"), None, false).is_err(), "the saved key went to another address");
    assert!(t(Some("https://b.example/v1"), None, true).is_err());
    assert_eq!(t(Some("http://localhost:11434/v1"), None, true).unwrap().unwrap().key, NO_KEY);
    assert_eq!(t(None, None, false).unwrap(), None);
    assert_eq!(codex_test_service(&saved, None, key, false).unwrap().unwrap().key, "sk-saved");
}

/// A password store that lives in memory.
#[derive(Default)]
struct MemKeys(std::sync::Mutex<Option<String>>);
impl KeyStore for MemKeys {
    fn get(&self) -> Option<String> { self.0.lock().unwrap().clone() }
    fn set(&self, key: &str) -> anyhow::Result<()> { *self.0.lock().unwrap() = Some(key.into()); Ok(()) }
    fn delete(&self) -> anyhow::Result<()> { *self.0.lock().unwrap() = None; Ok(()) }
}

#[tokio::test]
async fn the_codex_address_and_its_key_are_saved_together_and_never_part() {
    let st = AppState::for_test();
    let keys = MemKeys::default();
    let now = |st: &AppState| { let c = st.settings.try_read().unwrap().ai.codex.clone(); (c.base_url, c.has_api_token, c.keyless) };
    let code = |r: anyhow::Result<CodexSaved>| crate::tr::classify_in(false, &r.unwrap_err()).code;
    assert_eq!(code(set_codex_provider(&st, &keys, Some("https://openrouter.ai/api/v1"), None, false).await), "validation");
    assert_eq!(code(set_codex_provider(&st, &keys, None, Some("k"), false).await), "validation");
    let r = set_codex_provider(&st, &keys, Some(" https://openrouter.ai/api/v1 "), Some(" or-key "), false).await.unwrap();
    assert_eq!(r, CodexSaved { provider: CodexProvider::Service("https://openrouter.ai/api/v1".into()), key_removed: false });
    assert_eq!((now(&st), keys.get().as_deref()), ((Some("https://openrouter.ai/api/v1".into()), true, false), Some("or-key")));
    // Same address, no key typed: kept. Another address without its key: refused, nothing changes.
    set_codex_provider(&st, &keys, Some("https://openrouter.ai/api/v1"), None, false).await.unwrap();
    assert_eq!(code(set_codex_provider(&st, &keys, Some("https://other.example/v1"), None, false).await), "validation");
    assert_eq!(code(set_codex_provider(&st, &keys, Some("http://plain.example/v1"), Some("k"), false).await), "validation");
    assert_eq!(keys.get().as_deref(), Some("or-key"));
    // A service on this computer with no key: the OpenRouter key is removed, not reused.
    let r = set_codex_provider(&st, &keys, Some("http://localhost:11434/v1"), None, true).await.unwrap();
    assert_eq!((r.provider, r.key_removed, keys.get()), (CodexProvider::Service("http://localhost:11434/v1".into()), true, None));
    assert_eq!(now(&st), (Some("http://localhost:11434/v1".into()), false, true));
    // A general settings save can neither move nor drop it.
    let old = st.settings.read().await.clone();
    let mut sent = old.clone();
    sent.ai.codex.base_url = Some("https://evil.example/v1".into());
    sent.ai.codex.has_api_token = true;
    sent.ai.codex.keyless = false;
    sent.sanitize_update(&old).unwrap();
    assert_eq!((sent.ai.codex.base_url.as_deref(), sent.ai.codex.has_api_token, sent.ai.codex.keyless), (Some("http://localhost:11434/v1"), false, true));
    // Back to Codex's own set-up.
    set_codex_provider(&st, &keys, Some("https://openrouter.ai/api/v1"), Some("or-2"), false).await.unwrap();
    clear_codex_provider(&st, &keys).await.unwrap();
    assert_eq!((now(&st), keys.get()), ((None, false, false), None));
    assert_eq!(codex_provider_of(&st.settings.read().await.ai.codex), CodexProvider::Own);
}

#[tokio::test]
async fn a_codex_provider_skips_the_chatgpt_sign_in_and_an_incomplete_one_runs_nothing() {
    let st = AppState::for_test();
    st.settings.write().await.ai.codex = codex(Some("https://x.example/v1"), false, false);
    assert_eq!(crate::tr::classify_in(false, &crate::codex::require_signed_in(&st).await.unwrap_err()).code, "provider_incomplete");
    assert!(codex_service(&st).await.is_err());
    st.settings.write().await.ai.codex = codex(Some("http://localhost:11434/v1"), false, true);
    crate::codex::require_signed_in(&st).await.unwrap();
    assert_eq!(codex_service(&st).await.unwrap().unwrap().key, NO_KEY);
}

#[test]
fn no_access_key_reaches_the_provider_routes() {
    let caller = crate::tokens::Caller { id: "k".into(), scopes: vec![crate::tokens::Scope::Capture, crate::tokens::Scope::Read] };
    for (m, p) in [("PUT", "/api/ai/codex/provider"), ("DELETE", "/api/ai/codex/provider"), ("POST", "/api/ai/codex/provider/test"),
                   ("POST", "/api/ai/codex/provider/models"), ("POST", "/api/ai/claude/provider/models"), ("GET", "/api/ai/codex/presets"),
                   ("PUT", "/api/ai/claude/provider"), ("POST", "/api/ai/claude/provider/test")] {
        let uri: axum::http::Uri = p.parse().unwrap();
        assert!(crate::tokens::admit(&caller, &m.parse().unwrap(), &uri).is_none(), "{m} {p}");
    }
}

#[test]
fn the_loaded_list_belongs_to_one_program_and_one_address() {
    let dest = format!("http://localhost:{}", 40000 + (std::process::id() % 1000));
    keep_loaded("codex", &dest, &["m1".into(), "--bad".into()]);
    assert_eq!(loaded_models("codex", &dest), ["m1"]);
    assert!(loaded_models("claude", &dest).is_empty());
    assert!(loaded_models("codex", "http://localhost:1").is_empty());
    assert!(loaded_models("codex", "").is_empty());
}

/// Rule 14: the real `claude` and `codex` programs, started exactly as the app starts them (its own
/// one-shot paths, its environment, its command line), against a stand-in service that records what
/// arrives. Never run by `cargo test`: started by hand with a throwaway HOME, CODEX_HOME and
/// CLAUDE_CONFIG_DIR holding planted fake sign-ins, sentinel credentials in the environment, and
/// `THOUGHTS_PROOF_PORT` (the stand-in) and `THOUGHTS_PROOF_CLAUDE` / `THOUGHTS_PROOF_CODEX` (the
/// programs). What the stand-in recorded is checked afterwards (report `v050-providers`).
#[tokio::test]
#[ignore]
async fn real_programs_send_no_real_credential_to_another_address() {
    let port = std::env::var("THOUGHTS_PROOF_PORT").unwrap();
    let st = AppState::for_test();
    let base = format!("http://127.0.0.1:{port}");
    {
        let mut s = st.settings.write().await;
        s.ai.claude.binary = Some(std::env::var("THOUGHTS_PROOF_CLAUDE").unwrap());
        s.ai.claude.base_url = Some(base.clone());
        s.ai.claude.keyless = true;
        s.ai.codex.binary = Some(std::env::var("THOUGHTS_PROOF_CODEX").unwrap());
        s.ai.codex.base_url = Some(format!("{base}/v1"));
        s.ai.codex.keyless = true;
    }
    println!("PROOF claude keyless: {:?}", crate::claude::complete_claude(&st, "Reply with OK.", Some("standin-model".into())).await);
    println!("PROOF codex keyless: {:?}", crate::codex::complete(&st, "Reply with OK.", Some("standin-model".into()), false).await);
    // With a saved key (under test: "test-codex-key"), for the commands' environment check.
    { let mut s = st.settings.write().await; s.ai.codex.keyless = false; s.ai.codex.has_api_token = true; }
    println!("PROOF codex keyed: {:?}", crate::codex::complete(&st, "Reply with OK.", Some("standin-model".into()), false).await);
    // "Load models", through the app's own request.
    println!("PROOF models claude: {:?}", load_claude_models(&st).await.map(|v| v["models"].clone()));
    println!("PROOF models codex: {:?}", load_codex_models(&st).await.map(|v| v["models"].clone()));
}
