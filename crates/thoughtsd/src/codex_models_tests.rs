//! The models ladder, rung by rung. The samples in `testdata/codex-models/` are what real Codex
//! programs printed on this machine (0.27.0 as installed; 0.40.0, 0.140.0 and 0.171.0 installed in a
//! throwaway folder, never signed in, with an empty Codex folder and every address pointed at
//! nothing), trimmed: the catalog's long instruction texts are cut and three or four models kept.
//! No test starts a real Codex: the stand-ins below answer the way those versions did.

use super::*;

macro_rules! sample { ($f:literal) => { include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/codex-models/", $f)) }; }

const HELP_0_27: &str = sample!("help-0.27.0.txt");
const DEBUG_HELP_0_27: &str = sample!("debug-help-0.27.0.txt");
const HELP_0_40: &str = sample!("help-0.40.0.txt");
const DEBUG_HELP_0_40: &str = sample!("debug-help-0.40.0.txt");
const HELP_0_140: &str = sample!("help-0.140.0.txt");
const DEBUG_HELP_0_140: &str = sample!("debug-help-0.140.0.txt");
const HELP_0_171: &str = sample!("help-0.171.0.txt");
const DEBUG_HELP_0_171: &str = sample!("debug-help-0.171.0.txt");
const DEBUG_MODELS_HELP_0_171: &str = sample!("debug-models-help-0.171.0.txt");
const CATALOG_0_171: &str = sample!("debug-models-0.171.0.json");
const CATALOG_0_140: &str = sample!("debug-models-0.140.0.json");
const APP_SERVER_0_171: &str = sample!("app-server-model-list-0.171.0.jsonl");

fn ids(m: &[Model]) -> Vec<&str> { m.iter().map(|m| m.id.as_str()).collect() }
fn cmds(help: &str) -> Vec<String> { crate::codex::subcommands(help) }

// ---------- what each version offers ----------

#[test]
fn which_versions_have_which_rung() {
    for (v, top, debug) in [("0.27.0", HELP_0_27, DEBUG_HELP_0_27), ("0.40.0", HELP_0_40, DEBUG_HELP_0_40)] {
        assert!(!cmds(top).iter().any(|c| c == "app-server"), "{v} has no app server");
        assert!(cmds(top).iter().any(|c| c == "debug"), "{v} has a debug command");
        assert!(!cmds(debug).iter().any(|c| c == "models"), "{v} has no `debug models`");
    }
    for (v, top, debug) in [("0.140.0", HELP_0_140, DEBUG_HELP_0_140), ("0.171.0", HELP_0_171, DEBUG_HELP_0_171)] {
        assert!(cmds(top).iter().any(|c| c == "app-server"), "{v} has an app server");
        assert!(cmds(debug).iter().any(|c| c == "models"), "{v} has `debug models`");
    }
    assert!(crate::codex::has_flag(DEBUG_MODELS_HELP_0_171, "--bundled"));
}

// ---------- reading the answers ----------

#[test]
fn the_catalog_of_debug_models_gives_names_efforts_and_context() {
    let m = parse_catalog(CATALOG_0_171).unwrap();
    // Codex's own order; the model it hides from its picker is left out.
    assert_eq!(ids(&m), ["gpt-6.2-sol", "gpt-6-luna", "gpt-5.5"]);
    assert_eq!(m[0].label.as_deref(), Some("GPT-6.2-Sol"));
    assert_eq!(m[0].efforts, ["low", "medium", "high", "xhigh", "max", "ultra"]);
    assert_eq!((m[0].default_effort.as_deref(), m[0].context, m[0].is_default), (Some("low"), Some(500000), true));
    assert_eq!((m[2].efforts.len(), m[2].context, m[2].is_default), (4, Some(272000), false));
    assert_eq!(all_efforts(&m), ["low", "medium", "high", "xhigh", "max", "ultra"]);
    let older = parse_catalog(CATALOG_0_140).unwrap();
    assert_eq!(ids(&older), ["gpt-5.5", "gpt-5.4-mini"]);
}

#[test]
fn a_malformed_or_empty_catalog_is_said_as_such() {
    assert_eq!(parse_catalog("Error: not a catalog"), Err(MALFORMED));
    assert_eq!(parse_catalog(""), Err(MALFORMED));
    assert_eq!(parse_catalog(&CATALOG_0_171[..CATALOG_0_171.len() / 2]), Err(MALFORMED), "cut off half way");
    assert_eq!(parse_catalog(r#"{"models": 5}"#), Err(MALFORMED));
    assert_eq!(parse_catalog(r#"{"data": []}"#), Err(MALFORMED));
    assert_eq!(parse_catalog(r#"{"models": []}"#), Err(EMPTY));
    // Names that could not go on a command line, and entries without one, are passed over.
    let m = parse_catalog(r#"{"models": [{"slug": "--oss"}, {"slug": "a b"}, {"display_name": "x"}, {"slug": "ok-1", "supported_reasoning_levels": [{"effort": "-x"}, {"effort": "low"}]}, {"slug": "ok-1"}]}"#).unwrap();
    assert_eq!(ids(&m), ["ok-1"]);
    assert_eq!(m[0].efforts, ["low"]);
    assert_eq!(parse_catalog(r#"{"models": [{"slug": "h", "visibility": "hide"}]}"#), Err(EMPTY));
}

#[test]
fn the_app_servers_model_list_is_read_page_by_page() {
    let line = APP_SERVER_0_171.lines().find(|l| l.contains("\"id\":2")).unwrap();
    let v: Value = serde_json::from_str(line).unwrap();
    let (m, next) = parse_model_page(&v["result"]).unwrap();
    assert_eq!(ids(&m), ["gpt-6.2-sol", "gpt-6-luna", "gpt-5.5"]);
    assert_eq!((m[0].is_default, m[0].efforts.len(), m[0].default_effort.as_deref(), m[0].context), (true, 6, Some("low"), None));
    assert_eq!(next, None);
    let (m, next) = parse_model_page(&json!({"data": [{"id": "a", "model": "a", "hidden": true}, {"id": "b", "model": "b"}], "nextCursor": "p2"})).unwrap();
    assert_eq!((ids(&m), next.as_deref()), (vec!["b"], Some("p2")));
    assert_eq!(parse_model_page(&json!({"models": []})), Err(MALFORMED));
    assert_eq!(parse_model_page(&json!("nope")), Err(MALFORMED));
}

#[test]
fn config_toml_names_its_model_and_its_profiles_models() {
    let text = r#"
model = "gpt-5" # the usual one
model_provider = "openai"
[profiles.fast]
model = 'o4-mini'
[profiles."deep work"]
model = "o3"
[model_providers.ollama]
model = "not-a-profile"
[projects."/Users/x/p"]
trust_level = "trusted"
[profiles.fast.extra]
model = "nested"
[profiles.bad]
model = "--rm"
"#;
    assert_eq!(config_models(text), ["gpt-5", "o4-mini", "o3"]);
    assert!(config_models("[projects.\"/Users/x\"]\ntrust_level = \"trusted\"\n").is_empty(), "the file as it is on this Mac");
    assert!(config_models("model = ").is_empty());
}

// ---------- the ladder, against stand-ins ----------

/// A stand-in `codex` in `dir`: every call is noted in `calls`; `--version`, `--help`, `debug
/// --help`, `debug models --help` print the files of those names; `debug models [--bundled]` and
/// `app-server` run `dm.sh`, `bundled.sh` and `as.sh` (missing: fail).
#[cfg(unix)]
fn stand_in(dir: &Path, version: &str, help: &str, debug_help: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("version.txt"), version).unwrap();
    std::fs::write(dir.join("help.txt"), help).unwrap();
    std::fs::write(dir.join("debug-help.txt"), debug_help).unwrap();
    std::fs::write(dir.join("dm-help.txt"), DEBUG_MODELS_HELP_0_171).unwrap();
    let script = format!(r#"#!/bin/sh
D='{d}'
printf '%s|%s\n' "$*" "$CODEX_HOME" >> "$D/calls"
case "$*" in
  "--version") cat "$D/version.txt"; exit 0;;
  "--help") cat "$D/help.txt"; exit 0;;
  "debug --help") cat "$D/debug-help.txt"; exit 0;;
  "debug models --help") cat "$D/dm-help.txt"; exit 0;;
  "debug models --bundled") [ -f "$D/bundled.sh" ] && exec sh "$D/bundled.sh"; exit 2;;
  "debug models") [ -f "$D/dm.sh" ] && exec sh "$D/dm.sh"; exit 2;;
  app-server*) [ -f "$D/as.sh" ] && exec sh "$D/as.sh"; exit 2;;
esac
echo "a stand-in for an old Codex would have started a session here" > "$D/STARTED"
exit 0
"#, d = dir.display());
    let bin = dir.join("codex");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

#[cfg(unix)]
fn base(name: &str) -> PathBuf { std::env::temp_dir().join(format!("thoughts-codex-models-{name}-{}", uuid::Uuid::new_v4().simple())) }

#[cfg(unix)]
fn calls(dir: &Path) -> Vec<String> { std::fs::read_to_string(dir.join("calls")).unwrap_or_default().lines().map(|l| l.split('|').next().unwrap_or("").to_string()).collect() }

#[cfg(unix)]
fn outcomes(d: &Detected) -> Vec<(&str, &str)> { d.tried.iter().map(|t| (t.rung.as_str(), t.outcome.as_str())).collect() }

#[cfg(unix)]
#[tokio::test]
async fn codex_0_27_has_no_way_to_list_models_and_is_never_started_with_one() {
    let b = base("027");
    let (dir, home) = (b.join("bin"), b.join("home"));
    std::fs::create_dir_all(&home).unwrap();
    let bin = stand_in(&dir, "codex-cli 0.27.0\n", HELP_0_27, DEBUG_HELP_0_27);
    let d = detect(&bin, &home).await;
    assert_eq!((d.source.as_str(), d.models.len(), d.version.as_str()), ("", 0, "codex-cli 0.27.0"));
    assert_eq!(outcomes(&d), [(DEBUG_MODELS, UNAVAILABLE), (APP_SERVER, UNAVAILABLE), (CACHE, UNAVAILABLE)]);
    assert!(d.too_old());
    // Only help and version were asked: an unknown word would have been taken for a prompt.
    assert_eq!(calls(&dir), ["--version", "--help", "debug --help"]);
    assert!(!dir.join("STARTED").exists());
    // Each probe used the user's Codex folder it was given.
    assert!(std::fs::read_to_string(dir.join("calls")).unwrap().lines().all(|l| l.ends_with(&*home.to_string_lossy())));
    std::fs::remove_dir_all(&b).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn debug_models_is_the_first_rung() {
    let b = base("dm");
    let (dir, home) = (b.join("bin"), b.join("home"));
    std::fs::create_dir_all(&home).unwrap();
    let bin = stand_in(&dir, "codex-cli 0.171.0\n", HELP_0_171, DEBUG_HELP_0_171);
    std::fs::write(dir.join("catalog.json"), CATALOG_0_171).unwrap();
    std::fs::write(dir.join("dm.sh"), format!("cat '{}/catalog.json'\n", dir.display())).unwrap();
    std::fs::write(dir.join("as.sh"), "exit 3\n").unwrap();
    let d = detect(&bin, &home).await;
    assert_eq!((d.source.as_str(), ids(&d.models)), (DEBUG_MODELS, vec!["gpt-6.2-sol", "gpt-6-luna", "gpt-5.5"]));
    assert_eq!(outcomes(&d), [(DEBUG_MODELS, OK)]);
    assert!(!d.too_old());
    assert!(!calls(&dir).iter().any(|c| c.starts_with("app-server")), "the app server is not started when the catalog answered");
    std::fs::remove_dir_all(&b).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_catalog_that_cannot_refresh_falls_back_to_the_bundled_one() {
    let b = base("bundled");
    let (dir, home) = (b.join("bin"), b.join("home"));
    std::fs::create_dir_all(&home).unwrap();
    let bin = stand_in(&dir, "codex-cli 0.171.0\n", HELP_0_171, DEBUG_HELP_0_171);
    std::fs::write(dir.join("catalog.json"), CATALOG_0_140).unwrap();
    // The refresh hangs (no network) until its time limit; the bundled catalog answers. Only the
    // hanging rung's limit is cut (to 3 s); the bound below leaves room for a busy machine and is
    // still far short of the hang.
    std::fs::write(dir.join("dm.sh"), "sleep 300\n").unwrap();
    std::fs::write(dir.join("bundled.sh"), format!("cat '{}/catalog.json'\n", dir.display())).unwrap();
    let started = std::time::Instant::now();
    let d = CUT.scope((DEBUG_MODELS, Duration::from_secs(3)), detect(&bin, &home)).await;
    assert!(started.elapsed() < Duration::from_secs(90), "the hanging probe was ended at its limit");
    assert_eq!((d.source.as_str(), ids(&d.models)), (BUNDLED, vec!["gpt-5.5", "gpt-5.4-mini"]));
    assert_eq!(outcomes(&d), [(DEBUG_MODELS, TIMED_OUT), (BUNDLED, OK)]);
    std::fs::remove_dir_all(&b).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn the_app_server_answers_when_debug_models_does_not() {
    let b = base("as");
    let (dir, home) = (b.join("bin"), b.join("home"));
    std::fs::create_dir_all(&home).unwrap();
    let bin = stand_in(&dir, "codex-cli 0.171.0\n", HELP_0_171, DEBUG_HELP_0_171);
    std::fs::write(dir.join("rec.jsonl"), APP_SERVER_0_171).unwrap();
    // Garbage from the catalog, a failing bundled one; then the app server as 0.171.0 answered.
    std::fs::write(dir.join("dm.sh"), "echo 'thread main panicked'\n").unwrap();
    std::fs::write(dir.join("bundled.sh"), "exit 1\n").unwrap();
    std::fs::write(dir.join("as.sh"), format!("D='{}'\nread a; printf '%s\\n' \"$a\" >> \"$D/requests\"; sed -n 1,2p \"$D/rec.jsonl\"\nread b; read c; printf '%s\\n' \"$b\" \"$c\" >> \"$D/requests\"; sed -n 3p \"$D/rec.jsonl\"\nsleep 30\n", dir.display())).unwrap();
    let d = detect(&bin, &home).await;
    assert_eq!((d.source.as_str(), ids(&d.models)), (APP_SERVER, vec!["gpt-6.2-sol", "gpt-6-luna", "gpt-5.5"]));
    assert_eq!(outcomes(&d), [(DEBUG_MODELS, MALFORMED), (BUNDLED, FAILED), (APP_SERVER, OK)]);
    let req: Vec<Value> = std::fs::read_to_string(dir.join("requests")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!((req[0]["method"].as_str(), req[1]["method"].as_str(), req[2]["method"].as_str()), (Some("initialize"), Some("initialized"), Some("model/list")));
    assert_eq!(req[0]["params"]["clientInfo"]["name"], "thoughts");
    assert!(calls(&dir).iter().any(|c| c.starts_with("app-server -c mcp_servers={}")), "{:?}", calls(&dir));
    std::fs::remove_dir_all(&b).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn the_app_server_is_asked_for_every_page() {
    let b = base("pages");
    let (dir, home) = (b.join("bin"), b.join("home"));
    std::fs::create_dir_all(&home).unwrap();
    let bin = stand_in(&dir, "codex-cli 0.150.0\n", HELP_0_171, DEBUG_HELP_0_27);
    let p1 = json!({"id": 2, "result": {"data": [{"id": "m1", "model": "m1", "isDefault": false, "hidden": false, "supportedReasoningEfforts": [{"reasoningEffort": "low", "description": ""}]}], "nextCursor": "c2"}});
    let p2 = json!({"id": 3, "result": {"data": [{"id": "m2", "model": "m2", "isDefault": true, "hidden": false, "supportedReasoningEfforts": []}, {"id": "m1", "model": "m1"}], "nextCursor": null}});
    std::fs::write(dir.join("p1"), format!("{p1}\n")).unwrap();
    std::fs::write(dir.join("p2"), format!("{p2}\n")).unwrap();
    std::fs::write(dir.join("as.sh"), format!("D='{}'\nread a; echo '{{\"id\":1,\"result\":{{}}}}'\nread b; read c; cat \"$D/p1\"\nread d; printf '%s\\n' \"$d\" > \"$D/second\"; cat \"$D/p2\"\nread e\n", dir.display())).unwrap();
    let d = detect(&bin, &home).await;
    assert_eq!(outcomes(&d), [(DEBUG_MODELS, UNAVAILABLE), (APP_SERVER, OK)]);
    assert_eq!(ids(&d.models), ["m1", "m2"]);
    assert!(d.models[1].is_default && !d.models[0].is_default);
    let second: Value = serde_json::from_str(std::fs::read_to_string(dir.join("second")).unwrap().trim()).unwrap();
    assert_eq!((second["id"].as_u64(), second["params"]["cursor"].as_str()), (Some(3), Some("c2")));
    std::fs::remove_dir_all(&b).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn an_app_server_that_hangs_or_refuses_leaves_the_cache_on_disk() {
    for (case, script, outcome) in [
        ("hang", "read a; sleep 300\n".to_string(), TIMED_OUT),
        ("refuse", "read a; echo '{\"id\":1,\"result\":{}}'; read b; read c; echo '{\"id\":2,\"error\":{\"code\":-32600,\"message\":\"no\"}}'\n".into(), FAILED),
        ("bad", "read a; echo '{\"id\":1,\"result\":{}}'; read b; read c; echo '{\"id\":2,\"result\":{\"rows\":[]}}'\n".into(), MALFORMED),
        ("empty", "read a; echo '{\"id\":1,\"result\":{}}'; read b; read c; echo '{\"id\":2,\"result\":{\"data\":[]}}'\n".into(), EMPTY),
        ("gone", "exit 0\n".into(), FAILED),
    ] {
        let b = base(case);
        let (dir, home) = (b.join("bin"), b.join("home"));
        std::fs::create_dir_all(&home).unwrap();
        let bin = stand_in(&dir, "codex-cli 0.150.0\n", HELP_0_171, DEBUG_HELP_0_27);
        std::fs::write(dir.join("as.sh"), script).unwrap();
        // A broken cache file first (newest), then a good one.
        std::fs::write(home.join("models_cache.json"), CATALOG_0_140).unwrap();
        std::fs::write(home.join("models_cache.other.json"), "{not json").unwrap();
        let started = std::time::Instant::now();
        // Only the app server's limit is cut; the bound leaves room for a busy machine and is still
        // far short of the hang.
        let d = CUT.scope((APP_SERVER, Duration::from_secs(3)), detect(&bin, &home)).await;
        assert!(started.elapsed() < Duration::from_secs(90), "{case}");
        assert_eq!(d.source, CACHE, "{case}");
        assert_eq!(ids(&d.models), ["gpt-5.5", "gpt-5.4-mini"], "{case}");
        let o = outcomes(&d);
        assert_eq!(&o[..2], [(DEBUG_MODELS, UNAVAILABLE), (APP_SERVER, outcome)], "{case}");
        assert_eq!(o.last(), Some(&(CACHE, OK)), "{case}");
        std::fs::remove_dir_all(&b).unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn when_every_rung_fails_nothing_is_made_up() {
    let b = base("none");
    let (dir, home) = (b.join("bin"), b.join("home"));
    std::fs::create_dir_all(&home).unwrap();
    let bin = stand_in(&dir, "codex-cli 0.171.0\n", HELP_0_171, DEBUG_HELP_0_171);
    std::fs::write(dir.join("dm.sh"), "exit 1\n").unwrap();
    std::fs::write(dir.join("bundled.sh"), "echo '{\"models\":[]}'\n").unwrap();
    std::fs::write(dir.join("as.sh"), "exit 1\n").unwrap();
    let d = detect(&bin, &home).await;
    assert!(d.source.is_empty() && d.models.is_empty());
    assert_eq!(outcomes(&d), [(DEBUG_MODELS, FAILED), (BUNDLED, EMPTY), (APP_SERVER, FAILED), (CACHE, UNAVAILABLE)]);
    assert!(!d.too_old(), "this version can list models; it did not just now");
    std::fs::remove_dir_all(&b).unwrap();
}

// ---------- what the options say ----------

fn detected(source: &str, models: Vec<Model>, tried: &[(&str, &str)]) -> Detected {
    Detected { source: source.into(), models, version: "codex-cli 0.27.0".into(), checked_at: 1, tried: tried.iter().map(|(r, o)| Tried { rung: (*r).into(), outcome: (*o).into() }).collect(), ..Default::default() }
}

#[test]
fn the_options_list_the_catalog_first_then_what_else_is_known() {
    let d = detected(DEBUG_MODELS, parse_catalog(CATALOG_0_171).unwrap(), &[(DEBUG_MODELS, OK)]);
    let o = options_part(Some(&d), &["o3".into()], &["gpt-5.5".into(), "seen-1".into()], [Some("typed-by-hand".into()), None], false);
    assert_eq!(o["models"], json!(["gpt-6.2-sol", "gpt-6-luna", "gpt-5.5", "o3", "seen-1", "typed-by-hand"]));
    assert_eq!((o["modelsSource"].as_str(), o["modelsDetected"].as_bool(), o["modelsTooOld"].as_bool()), (Some(DEBUG_MODELS), Some(true), Some(false)));
    assert_eq!(o["modelInfo"][0]["efforts"].as_array().unwrap().len(), 6);
    assert_eq!(o["modelInfo"][0]["context"], 500000);
}

#[test]
fn without_a_catalog_the_options_say_where_the_names_came_from() {
    let old = detected("", vec![], &[(DEBUG_MODELS, UNAVAILABLE), (APP_SERVER, UNAVAILABLE), (CACHE, UNAVAILABLE)]);
    // Codex 0.27.0 as installed on this Mac: no model in config.toml, none run yet.
    let o = options_part(Some(&old), &[], &[], [None, None], false);
    assert_eq!((o["models"].clone(), o["modelsSource"].as_str(), o["modelsDetected"].as_bool(), o["modelsTooOld"].as_bool()), (json!([]), Some(NONE), Some(false), Some(true)));
    assert_eq!(o["codexVersion"], "codex-cli 0.27.0");
    // A name typed by hand that then ran is remembered (`codex::Seen`) and offered.
    let o = options_part(Some(&old), &[], &["gpt-5".into()], [None, None], false);
    assert_eq!((o["models"].clone(), o["modelsSource"].as_str()), (json!(["gpt-5"]), Some(SEEN)));
    let o = options_part(Some(&old), &["o3".into()], &["gpt-5".into()], [None, None], false);
    assert_eq!((o["models"].clone(), o["modelsSource"].as_str()), (json!(["o3", "gpt-5"]), Some(CONFIG)));
    // Not looked yet: still being asked.
    let o = options_part(None, &[], &[], [None, None], true);
    assert_eq!((o["modelsSource"].as_str(), o["modelsRefreshing"].as_bool(), o["modelsTooOld"].as_bool()), (Some(NONE), Some(true), Some(false)));
}

#[cfg(unix)]
#[test]
fn the_answer_is_asked_again_when_the_program_or_its_settings_change() {
    let b = base("key");
    let home = b.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let bin = b.join("codex");
    std::fs::write(&bin, "#!/bin/sh\n").unwrap();
    let k = key_for(&bin, &home);
    let mut d = Detected { key: k.clone(), checked_at: now(), ..Default::default() };
    assert!(fresh(&d, &k));
    std::fs::write(home.join("config.toml"), "model = \"o3\"\n").unwrap();
    assert!(!fresh(&d, &key_for(&bin, &home)), "config.toml appeared");
    let k = key_for(&bin, &home);
    d.key = k.clone();
    std::fs::write(&bin, "#!/bin/sh\n# another version\n").unwrap();
    assert!(!fresh(&d, &key_for(&bin, &home)), "the program changed");
    d.key = key_for(&bin, &home);
    d.checked_at = now() - MAX_AGE.as_secs() - 1;
    assert!(!fresh(&d, &d.key.clone()), "too old");
    std::fs::remove_dir_all(&b).unwrap();
}

/// Keeping the answer, the background refresh and "Refresh models" together (one test: they share
/// the kept answer).
#[cfg(unix)]
#[tokio::test]
async fn the_answer_is_kept_refreshed_in_the_background_and_on_demand() {
    let b = base("keep");
    let dir = b.join("bin");
    let bin = stand_in(&dir, "codex-cli 0.171.0\n", HELP_0_171, DEBUG_HELP_0_171);
    std::fs::write(dir.join("catalog.json"), CATALOG_0_171).unwrap();
    std::fs::write(dir.join("dm.sh"), format!("cat '{}/catalog.json'\n", dir.display())).unwrap();
    let (tx, mut rx) = tokio::sync::broadcast::channel(8);
    // Nothing kept for this program: nothing to show yet, and the ladder runs in the background.
    assert!(current_with(&bin, &tx, true).is_none());
    let ev = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.unwrap().unwrap();
    assert_eq!((ev["type"].as_str(), ev["source"].as_str(), ev["count"].as_u64()), (Some("codex.models"), Some(DEBUG_MODELS), Some(3)));
    assert!(!refreshing());
    let kept = current_with(&bin, &tx, true).unwrap();
    assert_eq!(kept.source, DEBUG_MODELS);
    assert!(rx.try_recv().is_err(), "a fresh answer is not asked again");
    // Kept on disk too, for the next start of the app.
    let on_disk: Detected = serde_json::from_str(&std::fs::read_to_string(crate::paths::home().join("codex-models.json")).unwrap()).unwrap();
    assert_eq!(on_disk, kept);
    // What the rest of the app reads from it: efforts per model, all efforts, context windows.
    assert_eq!(efforts_for(&bin, "gpt-5.5"), ["low", "medium", "high", "xhigh"]);
    assert!(efforts_for(&bin, "unknown-model").is_empty());
    assert_eq!(catalog_efforts(&bin), ["low", "medium", "high", "xhigh", "max", "ultra"]);
    assert_eq!((context_of(Some("gpt-6-luna")), context_of(None)), (Some(272000), Some(500000)));
    assert!(efforts_for(&b.join("another-codex"), "gpt-5.5").is_empty(), "another program's answer is not this one's");
    // "Refresh models" asks again even when the answer is fresh.
    let n = calls(&dir).len();
    std::fs::write(dir.join("dm.sh"), "exit 1\n").unwrap();
    let d = refresh(&bin, Some(&tx), true).await;
    assert!(calls(&dir).len() > n);
    assert_eq!(d.tried.first().map(|t| t.outcome.as_str()), Some(FAILED));
    assert_eq!(rx.recv().await.unwrap()["count"], 0);
    std::fs::remove_dir_all(&b).unwrap();
}

/// Against a real Codex, by hand: `THOUGHTS_REAL_CODEX=/path/to/codex cargo test -p thoughtsd
/// real_codex -- --ignored --nocapture`. It uses an empty Codex folder of its own (no sign-in);
/// run it with the proxies pointed at nothing so no request leaves the machine.
#[cfg(unix)]
#[tokio::test]
#[ignore]
async fn real_codex() {
    let Some(bin) = std::env::var_os("THOUGHTS_REAL_CODEX").map(PathBuf::from) else { return };
    let home = base("real");
    std::fs::create_dir_all(&home).unwrap();
    let d = detect(&bin, &home).await;
    println!("version {:?}\nsource {:?}\ntried {:?}\nmodels {:?}", d.version, d.source, outcomes(&d), d.models.iter().map(|m| (&m.id, m.efforts.len(), m.context)).collect::<Vec<_>>());
    if crate::codex::subcommands(&run(&bin, &home, "", &["--help"], HELP_LIMIT).await.unwrap_or_default()).iter().any(|c| c == "app-server") {
        let a = app_server_models(&bin, &home, APP_SERVER_LIMIT).await;
        println!("app server: {:?}", a.map(|m| m.into_iter().map(|m| m.id).collect::<Vec<_>>()));
    }
    std::fs::remove_dir_all(&home).unwrap();
}
