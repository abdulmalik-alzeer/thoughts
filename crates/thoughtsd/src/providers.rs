//! Other providers (round 0.5.0): the small additions that let the two assistant programs answer
//! through a service other than their makers' own, without any gateway in the app.
//!
//! - Subscriptions stay inside the unmodified CLIs: the app never reads, forwards or stores a Claude
//!   or ChatGPT sign-in. Claude Code reaches another service through its own `ANTHROPIC_BASE_URL`
//!   (`claude::Provider::Service`); Codex through its own `model_providers` settings, given per run
//!   on the command line (`codex_args`), with the key only in the run's environment.
//! - A service on this computer that takes no key (Ollama, a self-run LiteLLM without a key) gets a
//!   stand-in key (`NO_KEY`), so neither program falls back to sending the user's sign-in there.
//! - "Load models" asks the service itself for its list (`GET …/v1/models`): only when the user
//!   presses the button, with the saved key, no redirects, a time limit and a size limit; the answer
//!   is untrusted (every name passes `settings::check_cli_value`, the list is bounded).
//! - A key is kept in the system password store (`secrets.rs`), one per program, bound to exactly
//!   one address, and leaves only in the run's environment (never on a command line, in a file the
//!   app writes, in a log or in a prompt).
//!
//! What yields to full access: only the address check (`settings::check_base_url_for`: plain
//! `http://` to a private address on the home network). Key handling, the stand-in key, no
//! redirects and the size and time limits do not.

use crate::claude::KeyStore;
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

/// What a service that takes no key is sent instead of one. It is never a secret: its only job is to
/// be there, so the program does not use the user's own sign-in for that address.
pub const NO_KEY: &str = "thoughts-no-key";

// ---------- the service's own list of models ----------

/// How long the service has to answer "Load models".
pub const MODELS_TIMEOUT: Duration = Duration::from_secs(5);
/// The most of the answer that is read; a longer one is refused.
pub const MODELS_MAX_BYTES: u64 = 1024 * 1024;
/// The most model names kept from one answer.
pub const MODELS_MAX: usize = 500;

/// How the key travels on the model-list request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Auth { None, Bearer, Anthropic }

/// The model-list address of a service at `base`. Claude Code's address is the root its
/// `/v1/messages` hangs from (`https://openrouter.ai/api`); Codex's is the one its `/responses`
/// hangs from, usually ending in `/v1` already (`http://localhost:11434/v1`).
pub fn models_url(base: &str, program: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    let path = if b.to_ascii_lowercase().ends_with("/v1") { format!("{b}/models") } else { format!("{b}/v1/models") };
    // Anthropic's own list (and those copying it) gives 20 at a time unless asked for more.
    if program == "claude" { format!("{path}?limit=1000") } else { path }
}

/// The model names in a service's answer: `{"data":[{"id":…}]}` (the Anthropic and OpenAI shape),
/// else `{"models":[{"id"|"name":…}]}`. A name that is not a plain token is left out; at most
/// `MODELS_MAX`, each once.
pub fn parse_models(body: &[u8]) -> anyhow::Result<Vec<String>> {
    let v: Value = serde_json::from_slice(body).map_err(|_| crate::tr::err("provider.models_bad", &[]))?;
    let list = v.get("data").and_then(Value::as_array).or_else(|| v.get("models").and_then(Value::as_array))
        .ok_or_else(|| crate::tr::err("provider.models_bad", &[]))?;
    let mut out: Vec<String> = vec![];
    for m in list {
        let id = m.get("id").or_else(|| m.get("name")).and_then(Value::as_str).unwrap_or("").trim();
        if id.is_empty() || id.len() > 120 || crate::settings::check_cli_value("model", Some(id)).is_err() { continue; }
        if !out.iter().any(|o| o == id) { out.push(id.to_string()); }
        if out.len() >= MODELS_MAX { break; }
    }
    if out.is_empty() { return Err(crate::tr::err("provider.models_empty", &[])); }
    Ok(out)
}

/// Ask the service at `base` for its models (blocking: call from `spawn_blocking`). The address must
/// pass the same check it is saved with; the request follows no redirect (a redirect could carry
/// the key to another address), reaches no proxy, has `MODELS_TIMEOUT` and reads at most
/// `MODELS_MAX_BYTES`. A key is sent only as `auth` says and never appears in what is returned.
pub fn fetch_models(base: &str, program: &str, key: Option<&str>, auth: Auth, full: bool) -> anyhow::Result<Vec<String>> {
    crate::settings::check_base_url_for(base, full)?;
    let agent = ureq::AgentBuilder::new().redirects(0).timeout(MODELS_TIMEOUT).build();
    let mut req = agent.get(&models_url(base, program)).set("Accept", "application/json");
    match (auth, key.filter(|k| !k.is_empty() && *k != NO_KEY)) {
        (Auth::Bearer, Some(k)) => req = req.set("Authorization", &format!("Bearer {k}")),
        (Auth::Anthropic, Some(k)) => req = req.set("x-api-key", k).set("anthropic-version", "2023-06-01"),
        _ => {}
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => return Err(status_err(code)),
        Err(ureq::Error::Transport(t)) => {
            let text = t.to_string().to_lowercase();
            crate::log::warn(format!("model list: no answer from the service ({:?})", t.kind()));
            if text.contains("timed out") || text.contains("timeout") { return Err(crate::tr::err("provider.models_timeout", &[])); }
            return Err(crate::tr::err("provider.models_unreachable", &[]));
        }
    };
    if resp.status() != 200 { return Err(status_err(resp.status())); }
    let mut body = Vec::new();
    resp.into_reader().take(MODELS_MAX_BYTES + 1).read_to_end(&mut body).map_err(|_| crate::tr::err("provider.models_unreachable", &[]))?;
    if body.len() as u64 > MODELS_MAX_BYTES { return Err(crate::tr::err("provider.models_too_big", &[])); }
    parse_models(&body)
}

fn status_err(code: u16) -> anyhow::Error {
    match code {
        300..=399 => crate::tr::err("provider.models_redirect", &[]),
        401 | 403 => crate::tr::err("provider.key_refused", &[]),
        404 => crate::tr::err("provider.models_none", &[]),
        _ => crate::tr::err("provider.models_status", &[("status", &code.to_string())]),
    }
}

fn loaded_file() -> PathBuf { crate::paths::home().join("service-models.json") }

fn loaded_key(program: &str, dest: &str) -> String { format!("{program} {dest}") }

fn read_loaded() -> BTreeMap<String, Vec<String>> {
    std::fs::read_to_string(loaded_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// The models last loaded from the service at `dest` for `program` ("claude" or "codex"); a
/// service's list is never offered for another address. Names are checked again on the way out.
pub fn loaded_models(program: &str, dest: &str) -> Vec<String> {
    if dest.is_empty() { return vec![]; }
    let mut list = read_loaded().remove(&loaded_key(program, dest)).unwrap_or_default();
    list.retain(|m| crate::settings::check_cli_value("model", Some(m)).is_ok() && !m.is_empty());
    list.truncate(MODELS_MAX);
    list
}

fn keep_loaded(program: &str, dest: &str, models: &[String]) {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut map = read_loaded();
    map.insert(loaded_key(program, dest), models.to_vec());
    if let Ok(text) = serde_json::to_string_pretty(&map) {
        crate::log::ok_or_warn("could not keep the service's models", crate::paths::write_atomic(&loaded_file(), text.as_bytes(), true));
    }
}

/// "Load models" for Claude Code: the saved service's own list, kept for the model menu.
/// `{ok, models}`; refused when no service is saved.
pub async fn load_claude_models(state: &AppState) -> anyhow::Result<Value> {
    let c = state.settings.read().await.ai.claude.clone();
    let crate::claude::Provider::Service(url) = crate::claude::provider_of(&c) else { return Err(crate::tr::err("provider.models_no_service", &[])) };
    let key = if c.has_api_token { Some(claude_key().ok_or_else(|| crate::tr::err("provider.key_unreadable", &[]))?) } else { None };
    load(state, "claude", url, key, Auth::Bearer).await
}

/// "Load models" for Codex's other provider.
pub async fn load_codex_models(state: &AppState) -> anyhow::Result<Value> {
    let c = state.settings.read().await.ai.codex.clone();
    let CodexProvider::Service(url) = codex_provider_of(&c) else { return Err(crate::tr::err("provider.models_no_service", &[])) };
    let key = if c.has_api_token { Some(codex_key().ok_or_else(|| crate::tr::err("provider.key_unreadable", &[]))?) } else { None };
    load(state, "codex", url, key, Auth::Bearer).await
}

async fn load(state: &AppState, program: &'static str, url: String, key: Option<String>, auth: Auth) -> anyhow::Result<Value> {
    let full = crate::full_access::on();
    let u = url.clone();
    let models = tokio::task::spawn_blocking(move || fetch_models(&u, program, key.as_deref(), auth, full)).await??;
    // The address may have changed while the service answered: the list is kept for the one asked.
    keep_loaded(program, &url, &models);
    crate::log::info(format!("loaded {} model names from the {program} service", models.len()));
    state.emit(json!({"type": "provider.models", "program": program}));
    Ok(json!({"ok": true, "models": models}))
}

/// The Claude provider key from the password store (unit tests never reach the real one).
fn claude_key() -> Option<String> {
    if cfg!(test) { return Some("test-key".into()); }
    crate::claude::Keychain.get()
}

// ---------- Codex: another OpenAI-compatible provider ----------

/// The id the app's provider has in Codex's `model_providers` (not one of Codex's reserved ids
/// `openai`, `ollama`, `lmstudio`, so it never changes one of those).
pub const CODEX_PROVIDER_ID: &str = "thoughts";
/// The environment variable the key travels in (named by `env_key`); commands Codex runs never get it.
pub const CODEX_KEY_ENV: &str = "THOUGHTS_PROVIDER_KEY";

/// Who answers Codex's runs.
#[derive(Debug, Clone, PartialEq)]
pub enum CodexProvider {
    /// Codex's own set-up: the ChatGPT sign-in or OpenAI key, and the user's `config.toml`.
    Own,
    /// Another OpenAI-compatible service at this address (its Responses API), with its key saved
    /// or, on this computer, none.
    Service(String),
    /// An address without a key (or one that no longer passes the check): nothing runs.
    Incomplete,
}

impl CodexProvider {
    pub fn name(&self) -> &'static str {
        match self { CodexProvider::Own => "own", CodexProvider::Service(_) => "service", CodexProvider::Incomplete => "incomplete" }
    }
}

/// The address Codex's requests go to when it is not its own set-up.
pub fn codex_destination(base_url: Option<&str>) -> Option<&str> { base_url.map(str::trim).filter(|u| !u.is_empty()) }

pub fn codex_provider_of(c: &crate::settings::CodexSettings) -> CodexProvider {
    let full = crate::full_access::on();
    match codex_destination(c.base_url.as_deref()) {
        None => CodexProvider::Own,
        Some(u) if c.has_api_token && crate::settings::check_base_url_for(u, full).is_ok() => CodexProvider::Service(u.to_string()),
        Some(u) if !c.has_api_token && c.keyless && crate::settings::keyless_allowed(u, full) => CodexProvider::Service(u.to_string()),
        Some(_) => CodexProvider::Incomplete,
    }
}

/// The system password store entry of the Codex provider key.
pub struct CodexKeychain;
impl KeyStore for CodexKeychain {
    fn get(&self) -> Option<String> { crate::secrets::get("thoughts-ai", "codex") }
    fn set(&self, key: &str) -> anyhow::Result<()> { crate::secrets::set("thoughts-ai", "codex", key) }
    fn delete(&self) -> anyhow::Result<()> { crate::secrets::delete("thoughts-ai", "codex") }
}

pub(crate) fn codex_key() -> Option<String> {
    if cfg!(test) { return Some("test-codex-key".into()); }
    CodexKeychain.get()
}

/// What a Codex run is pointed at: the address, and the key that goes only into its environment.
#[derive(Clone, PartialEq)]
pub struct CodexService { pub base_url: String, pub key: String }

impl std::fmt::Debug for CodexService {
    // The key is never printed, not even in a test failure.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "CodexService({}, key …)", self.base_url) }
}

/// The service of `c` for a run: `None` for Codex's own set-up; an error for an address that may not
/// run. A keyless service gets `NO_KEY`; a saved key comes from `saved`.
pub fn codex_service_for(c: &crate::settings::CodexSettings, saved: impl FnOnce() -> Option<String>) -> anyhow::Result<Option<CodexService>> {
    match codex_provider_of(c) {
        CodexProvider::Own => Ok(None),
        CodexProvider::Incomplete => Err(crate::tr::err("codex.provider_incomplete", &[])),
        CodexProvider::Service(base_url) => {
            let key = if c.has_api_token { saved().filter(|k| !k.trim().is_empty()).ok_or_else(|| crate::tr::err("provider.key_unreadable", &[]))? }
                else { NO_KEY.to_string() };
            Ok(Some(CodexService { base_url, key }))
        }
    }
}

/// The service a Codex run that is about to start uses (see `codex_service_for`).
pub async fn codex_service(state: &AppState) -> anyhow::Result<Option<CodexService>> {
    let c = state.settings.read().await.ai.codex.clone();
    codex_service_for(&c, codex_key).map_err(|e| { crate::log::warn(format!("Codex not started: {e}")); e })
}

/// The `-c` settings that point one Codex run at `svc` (Codex's own `model_providers`), with the key
/// named only by the variable that carries it. `user_excludes` are the names the user's own
/// `config.toml` already keeps from commands; the key's variable is added to them, and Codex's
/// default excludes (names with KEY, SECRET or TOKEN) are kept on, so commands the run starts never
/// see the key.
pub fn codex_args(svc: &CodexService, user_excludes: &[String]) -> Vec<String> {
    let q = |s: &str| serde_json::to_string(s).unwrap_or_default();
    let p = CODEX_PROVIDER_ID;
    let mut exclude: Vec<String> = user_excludes.iter().filter(|e| !e.is_empty() && *e != CODEX_KEY_ENV).cloned().collect();
    exclude.push(CODEX_KEY_ENV.into());
    vec![
        format!("model_provider={}", q(p)),
        format!("model_providers.{p}.name={}", q("Thoughts: another provider")),
        format!("model_providers.{p}.base_url={}", q(&svc.base_url)),
        format!("model_providers.{p}.wire_api={}", q("responses")),
        format!("model_providers.{p}.env_key={}", q(CODEX_KEY_ENV)),
        "shell_environment_policy.ignore_default_excludes=false".into(),
        format!("shell_environment_policy.exclude={}", serde_json::to_string(&exclude).unwrap_or_default()),
    ]
}

/// The names the user's own Codex `config.toml` keeps from commands (`[shell_environment_policy]
/// exclude`), so the run's setting adds to them instead of replacing them.
pub fn user_excludes(config_toml: &str) -> Vec<String> {
    // A whole file is a table: from toml 0.9 on, parsing a `Value` reads one value, not a document.
    let Ok(v) = config_toml.parse::<toml::Table>() else { return vec![] };
    v.get("shell_environment_policy").and_then(|p| p.get("exclude")).and_then(|e| e.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).filter(|s| s.len() <= 200).map(String::from).take(200).collect()).unwrap_or_default()
}

/// The `-c` settings for a run through `svc`, reading the user's own excludes from Codex's folder.
pub fn codex_args_now(svc: &CodexService) -> Vec<String> {
    let text = std::fs::read_to_string(crate::codex::codex_home().join("config.toml")).unwrap_or_default();
    codex_args(svc, &user_excludes(&text))
}

/// The environment of a run through `svc`: the key in `CODEX_KEY_ENV`, and no inherited OpenAI key or
/// address that could outrank or redirect it.
pub fn apply_codex_env(c: &mut tokio::process::Command, svc: Option<&CodexService>) {
    let Some(svc) = svc else { return };
    for name in CODEX_UNSET { c.env_remove(name); }
    c.env(CODEX_KEY_ENV, &svc.key);
}

/// What a run through another provider never inherits from the daemon's environment.
pub const CODEX_UNSET: [&str; 3] = ["OPENAI_API_KEY", "OPENAI_BASE_URL", "CODEX_API_KEY"];

/// What `set_codex_provider` did.
#[derive(Debug, PartialEq)]
pub struct CodexSaved { pub provider: CodexProvider, pub key_removed: bool }

/// How a provider change works out, for either program: what is saved, and whether the key saved for
/// the previous address goes. `cur` = (destination, has key, keyless) as saved.
#[derive(Debug, PartialEq)]
pub struct Change { pub has_api_token: bool, pub keyless: bool, pub drops_old_key: bool, pub moved: bool }

/// The rules of `claude::set_provider_with`, for an address that has no "own" spelling:
/// - a new key is saved for exactly the address given with it;
/// - "no key needed" only for an address on this computer (`provider.key_remote` otherwise);
/// - another address without its key is refused (`provider.key_needed`), and nothing changes;
/// - a key is removed when its address goes or the service is saved as taking none.
pub fn decide(cur: (Option<&str>, bool, bool), url: Option<&str>, key: Option<&str>, no_key: bool, full: bool) -> anyhow::Result<Change> {
    let (cur_dest, cur_has, cur_keyless) = cur;
    let moved = cur_dest != url;
    let keyless = url.is_some() && key.is_none() && (no_key || (cur_keyless && !moved));
    if keyless && !url.is_some_and(|u| crate::settings::keyless_allowed(u, full)) { return Err(crate::tr::err("provider.key_remote", &[])); }
    let keeps_key = cur_has && !moved && !keyless;
    if url.is_some() && key.is_none() && !keeps_key && !keyless { return Err(crate::tr::err("provider.key_needed", &[])); }
    Ok(Change { has_api_token: url.is_some() && (key.is_some() || keeps_key), keyless, drops_old_key: key.is_none() && cur_has && (moved || keyless || url.is_none()), moved })
}

/// Point Codex at another provider (`base_url` + `key`, or `no_key` for a service on this computer),
/// or with a blank address back to its own set-up. Same order as `claude::set_provider`: the key is
/// stored before the settings say it is there, taken out again if they cannot be saved, and a key of
/// a previous address goes only once that is on disk.
pub async fn set_codex_provider(state: &AppState, store: &dyn KeyStore, base_url: Option<&str>, key: Option<&str>, no_key: bool) -> anyhow::Result<CodexSaved> {
    if crate::settings::read_only() { return Err(crate::tr::err("newer_settings", &[])); }
    let full = crate::full_access::on();
    let url = base_url.map(str::trim).filter(|u| !u.is_empty());
    if let Some(u) = url { crate::settings::check_base_url_for(u, full)?; }
    let key = key.map(str::trim).filter(|k| !k.is_empty());
    if url.is_none() && key.is_some() { return Err(crate::tr::err("codex.provider_no_address", &[])); }
    let mut s = state.settings.write().await;
    let cur = s.ai.codex.clone();
    let ch = decide((codex_destination(cur.base_url.as_deref()), cur.has_api_token, cur.keyless), url, key, no_key, full)?;
    let mut next = s.clone();
    next.ai.codex.base_url = url.map(String::from);
    next.ai.codex.has_api_token = ch.has_api_token;
    next.ai.codex.keyless = ch.keyless;
    if let Some(k) = key { store.set(k)?; }
    if let Err(e) = next.save() {
        if key.is_some() && (ch.moved || !cur.has_api_token) { crate::log::ok_or_warn("could not remove the Codex provider key after a failed save", store.delete()); }
        return Err(e.into());
    }
    if ch.drops_old_key { crate::log::ok_or_warn("could not remove the saved Codex provider key", store.delete()); }
    let provider = codex_provider_of(&next.ai.codex);
    *s = next;
    drop(s);
    state.codex.auth_failed.store(false, std::sync::atomic::Ordering::Relaxed);
    crate::log::info(format!("Codex provider set to {}", provider.name()));
    Ok(CodexSaved { provider, key_removed: ch.drops_old_key })
}

/// Codex back to its own set-up: forget the address and the key.
pub async fn clear_codex_provider(state: &AppState, store: &dyn KeyStore) -> anyhow::Result<()> {
    if crate::settings::read_only() { return Err(crate::tr::err("newer_settings", &[])); }
    let mut s = state.settings.write().await;
    let mut next = s.clone();
    next.ai.codex.base_url = None;
    next.ai.codex.has_api_token = false;
    next.ai.codex.keyless = false;
    next.save()?;
    *s = next;
    drop(s);
    crate::log::ok_or_warn("could not remove the saved Codex provider key", store.delete());
    crate::log::info("Codex provider removed; using Codex's own set-up");
    Ok(())
}

/// The service a Codex connection test tries: what was typed (`url`, `token`, `no_key`), with the saved
/// key only for the saved address; else the saved set-up. `None` = Codex's own set-up.
pub fn codex_test_service(saved: &crate::settings::CodexSettings, typed: Option<(Option<&str>, Option<&str>, bool)>, saved_key: impl FnOnce() -> Option<String>, full: bool) -> anyhow::Result<Option<CodexService>> {
    let Some((url, token, no_key)) = typed else { return codex_service_for(saved, saved_key) };
    let url = url.map(str::trim).filter(|u| !u.is_empty());
    let Some(u) = url else { return Ok(None) };
    crate::settings::check_base_url_for(u, full)?;
    let token = token.map(str::trim).filter(|k| !k.is_empty()).map(String::from);
    let same = codex_destination(saved.base_url.as_deref()) == Some(u);
    let key = if let Some(t) = token { t }
        else if no_key || (saved.keyless && same) {
            if !crate::settings::keyless_allowed(u, full) { return Err(crate::tr::err("provider.key_remote", &[])); }
            NO_KEY.to_string()
        }
        else if saved.has_api_token && same { saved_key().ok_or_else(|| crate::tr::err("provider.key_unreadable", &[]))? }
        else { return Err(crate::tr::err("provider.key_needed", &[])) };
    Ok(Some(CodexService { base_url: u.to_string(), key }))
}

/// Try Codex through the typed (or saved) provider with one very short question. `{ok}` or
/// `{ok:false, refused, message}` in plain words, the key never in the message.
pub async fn test_codex_provider(state: &AppState, typed: Option<(Option<&str>, Option<&str>, bool)>) -> Value {
    let saved = state.settings.read().await.ai.codex.clone();
    let svc = match codex_test_service(&saved, typed, codex_key, crate::full_access::on()) {
        Ok(Some(s)) => s,
        Ok(None) => return crate::claude::test_result(Err(crate::tr::err("codex.provider_no_address", &[]))),
        Err(e) => return crate::claude::test_result(Err(e)),
    };
    let key = svc.key.clone();
    let r = crate::codex::complete_through(state, "Reply with the single word OK.", saved.model.clone(), true, Some(svc)).await
        .and_then(|a| if a.trim().is_empty() { anyhow::bail!("the service answered with nothing") } else { Ok(()) })
        .map_err(|e| if key.len() >= 4 && key != NO_KEY { anyhow::anyhow!(e.to_string().replace(&key, "…")) } else { e });
    crate::claude::test_result(r)
}

/// A service the Settings page offers ready-made (round 0.7.2): the user picks it and pastes a key,
/// the address is ours. Kept to the few big ones whose own documentation says which of the two
/// programs they work with; everything else is the page's one "another service" form (address, key,
/// model), and an address saved before that is no longer here simply shows as that form.
/// No model lists here: those come from the service ("Load models").
pub struct Service {
    pub id: &'static str,
    /// The address Claude Code is pointed at (`ANTHROPIC_BASE_URL`; the service speaks Anthropic's
    /// format there). `Some("")` is Anthropic itself: a key and no address. `None`: not for Claude Code.
    pub claude: Option<&'static str>,
    /// The address Codex's `model_providers.thoughts.base_url` gets (the service answers OpenAI's
    /// Responses API there, the only one Codex still speaks). `None`: not for Codex.
    pub codex: Option<&'static str>,
    /// Runs on this computer and takes no key (it gets the stand-in `NO_KEY`).
    pub no_key: bool,
    /// Where the person gets a key (a page in their browser).
    pub keys_url: &'static str,
}

/// Each fact was read in the service's own documentation on 2026-11-24 (round 0.7.2 report,
/// `providers.md`, has the pages). Names and one-line descriptions are the window's (`i18n/providers72.ts`).
pub const SERVICES: &[Service] = &[
    // Codex's own configuration reference shows a custom provider at an api.openai.com address with a key from `env_key`.
    Service { id: "openai", claude: None, codex: Some("https://api.openai.com/v1"), no_key: false, keys_url: "https://platform.openai.com/api-keys" },
    // A key and no address: Claude Code's own `ANTHROPIC_API_KEY`.
    Service { id: "anthropic", claude: Some(""), codex: None, no_key: false, keys_url: "https://console.anthropic.com/settings/keys" },
    // openrouter.ai/docs/guides/coding-agents/codex-cli (base_url https://openrouter.ai/api/v1) and
    // …/guides/claude-code-integration (ANTHROPIC_BASE_URL https://openrouter.ai/api, ANTHROPIC_AUTH_TOKEN).
    Service { id: "openrouter", claude: Some("https://openrouter.ai/api"), codex: Some("https://openrouter.ai/api/v1"), no_key: false, keys_url: "https://openrouter.ai/keys" },
    // api-docs.deepseek.com/quick_start/agent_integrations/codex (base_url https://api.deepseek.com/, wire_api
    // "responses") and …/guides/anthropic_api (https://api.deepseek.com/anthropic).
    Service { id: "deepseek", claude: Some("https://api.deepseek.com/anthropic"), codex: Some("https://api.deepseek.com"), no_key: false, keys_url: "https://platform.deepseek.com/api_keys" },
    // docs.ollama.com/integrations/codex (http://localhost:11434/v1/, wire_api "responses") and
    // …/api/anthropic-compatibility (ANTHROPIC_BASE_URL http://localhost:11434, the token "required but ignored").
    Service { id: "ollama", claude: Some("http://localhost:11434"), codex: Some("http://localhost:11434/v1"), no_key: true, keys_url: "" },
];

/// What `GET /api/ai/services` returns: the ready-made services, in the order the page shows them.
pub fn services_json() -> Value {
    json!(SERVICES.iter().map(|p| json!({"id": p.id, "claude": p.claude, "codex": p.codex, "noKey": p.no_key, "keysUrl": p.keys_url})).collect::<Vec<_>>())
}

// ---------- the routes Settings uses (the app's own key only: `tokens::ROUTES` names none of them) ----------

type R = Result<axum::Json<Value>, crate::api::ApiErr>;
type S = axum::extract::State<std::sync::Arc<AppState>>;

pub fn routes() -> axum::Router<std::sync::Arc<AppState>> {
    use axum::routing::{get, post, put};
    axum::Router::new()
        .route("/api/ai/claude/provider/models", post(claude_models))
        .route("/api/ai/codex/provider", put(save_codex).delete(remove_codex))
        .route("/api/ai/codex/provider/test", post(test_codex))
        .route("/api/ai/codex/provider/models", post(codex_models))
        .route("/api/ai/services", get(|| async { axum::Json(services_json()) }))
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Body { base_url: Option<String>, token: Option<String>, no_key: bool }

/// "Load models" for Claude Code's service. Asked only when the user presses the button.
async fn claude_models(axum::extract::State(st): S) -> R {
    let r = load_claude_models(&st).await?;
    st.emit(json!({"type": "settings.changed"}));
    Ok(axum::Json(r))
}

async fn codex_models(axum::extract::State(st): S) -> R { Ok(axum::Json(load_codex_models(&st).await?)) }

/// Point Codex at another provider: its address with its key (or "no key needed"), saved together.
async fn save_codex(axum::extract::State(st): S, axum::Json(b): axum::Json<Body>) -> R {
    let saved = set_codex_provider(&st, &CodexKeychain, b.base_url.as_deref(), b.token.as_deref(), b.no_key).await?;
    st.emit(json!({"type": "settings.changed"}));
    Ok(axum::Json(json!({"ok": true, "provider": saved.provider.name(), "keyRemoved": saved.key_removed})))
}

async fn remove_codex(axum::extract::State(st): S) -> R {
    clear_codex_provider(&st, &CodexKeychain).await?;
    st.emit(json!({"type": "settings.changed"}));
    Ok(axum::Json(json!({"ok": true, "provider": "own"})))
}

/// With a body, tries what was typed and not saved yet; without, the saved provider.
async fn test_codex(axum::extract::State(st): S, body: Option<axum::Json<Body>>) -> R {
    let typed = body.filter(|b| b.base_url.is_some() || b.token.is_some() || b.no_key);
    Ok(axum::Json(test_codex_provider(&st, typed.as_ref().map(|b| (b.base_url.as_deref(), b.token.as_deref(), b.no_key))).await))
}

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;

/// `codex_args` as the JSON object Codex's ACP adapter merges into its session config
/// (`CODEX_CONFIG`): the same provider, the same key variable, the same excludes.
pub fn codex_config_json(svc: &CodexService, user_excludes: &[String]) -> Value {
    let mut exclude: Vec<String> = user_excludes.iter().filter(|e| !e.is_empty() && *e != CODEX_KEY_ENV).cloned().collect();
    exclude.push(CODEX_KEY_ENV.into());
    serde_json::json!({
        "model_provider": CODEX_PROVIDER_ID,
        "model_providers": {CODEX_PROVIDER_ID: {"name": "Thoughts: another provider", "base_url": svc.base_url, "wire_api": "responses", "env_key": CODEX_KEY_ENV}},
        "shell_environment_policy": {"ignore_default_excludes": false, "exclude": exclude},
    })
}
