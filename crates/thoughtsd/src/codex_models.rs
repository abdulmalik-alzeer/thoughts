//! Which models the installed Codex offers, asked of the program itself — never a list kept in the
//! app. Each version of Codex can say it in a different way (or not at all), so this is a ladder,
//! newest way first, and the first rung that names models wins:
//!
//! 1. `codex debug models` (Codex ≥ 0.140 here; listed under `codex debug --help`): the model
//!    catalog as JSON, with each model's efforts and context window. It refreshes the catalog with
//!    the user's own sign-in (a free catalog request, never a model run); if that cannot finish,
//!    `codex debug models --bundled` gives the catalog the program was shipped with (`bundled`).
//! 2. `codex app-server` and its `model/list` request (listed under `codex --help`): the same
//!    models, as the program's own app server gives them to its desktop app. The server is ended as
//!    soon as it has answered. A version without `app-server` in its help is never started with that
//!    word: older versions take an unknown word for a prompt and would open a session.
//! 3. `models_cache.json` in Codex's folder: the catalog newer versions keep on disk (read only).
//!
//! Below the catalog rungs, `options` adds what is known without one: the models named in Codex's
//! `config.toml` (`config`), the models runs have answered with (`codex::Seen`, `seen`) and the
//! user's own picks. Codex 0.27 (and up to at least 0.40) has none of the catalog rungs: there the
//! source is `config`, `seen` or `none`, and the app says so (an update of Codex lists them).
//!
//! What was found is kept in memory and in the app's folder (`codex-models.json`), keyed by the
//! program (its path, size and time), Codex's `config.toml` and `models_cache.json` times; it is
//! asked again in the background when the key changes or the answer is older than `MAX_AGE`, and on
//! demand ("Refresh models"). Every probe has a time limit, and none starts a model run.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// Where a list came from (`modelsSource` in `GET /api/ai/codex/options`).
pub const DEBUG_MODELS: &str = "debugModels";
pub const BUNDLED: &str = "bundled";
pub const APP_SERVER: &str = "appServer";
pub const CACHE: &str = "cache";
pub const CONFIG: &str = "config";
pub const SEEN: &str = "seen";
pub const NONE: &str = "none";

/// A rung's outcome (`modelsTried`).
pub const OK: &str = "ok";
/// This version has no such way to say it.
pub const UNAVAILABLE: &str = "unavailable";
pub const FAILED: &str = "failed";
pub const TIMED_OUT: &str = "timedOut";
pub const MALFORMED: &str = "malformed";
pub const EMPTY: &str = "empty";

/// An answer older than this is asked again (in the background) the next time it is wanted.
pub const MAX_AGE: Duration = Duration::from_secs(12 * 3600);

/// Time limits of the probes.
pub const HELP_LIMIT: Duration = Duration::from_secs(15);
pub const DEBUG_LIMIT: Duration = Duration::from_secs(30);
pub const BUNDLED_LIMIT: Duration = Duration::from_secs(15);
pub const APP_SERVER_LIMIT: Duration = Duration::from_secs(20);

/// One model, as the program described it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Model {
    pub id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    /// The reasoning efforts this model takes, as Codex lists them.
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    /// How many tokens it can take in.
    pub context: Option<u64>,
    /// Codex's own choice when none is made.
    pub is_default: bool,
}

/// What the answer depends on: a different program or changed Codex settings mean asking again.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Key {
    pub bin: PathBuf,
    pub bin_len: u64,
    pub bin_modified: Option<u64>,
    pub config_modified: Option<u64>,
    pub cache_modified: Option<u64>,
}

/// One rung tried, and how it went.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Tried { pub rung: String, pub outcome: String }

/// What the ladder found.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Detected {
    pub key: Key,
    /// `codex --version` as it printed it (`codex-cli 0.27.0`).
    pub version: String,
    /// The catalog rung that named the models, or empty when none could.
    pub source: String,
    pub models: Vec<Model>,
    /// Seconds since 1970.
    pub checked_at: u64,
    pub tried: Vec<Tried>,
}

impl Detected {
    /// Did every catalog rung say this version has no way to list models (an old Codex)?
    pub fn too_old(&self) -> bool { self.source.is_empty() && !self.tried.is_empty() && self.tried.iter().all(|t| t.outcome == UNAVAILABLE) }
    pub fn model(&self, id: &str) -> Option<&Model> { self.models.iter().find(|m| m.id == id) }
}

fn secs(t: SystemTime) -> u64 { t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }
fn mtime(p: &Path) -> Option<u64> { std::fs::metadata(p).and_then(|m| m.modified()).ok().map(secs) }
fn now() -> u64 { secs(SystemTime::now()) }

fn valid_id(id: &str) -> bool { !id.trim().is_empty() && crate::settings::check_cli_value("model", Some(id)).is_ok() }

// ---------- reading what the program says ----------

/// The catalog of `codex debug models` and of `models_cache.json`: `{"models": [{"slug", "display_name",
/// "description", "visibility", "priority", "supported_reasoning_levels": [{"effort"}],
/// "default_reasoning_level", "context_window"}…]}`. Models Codex hides from its own picker
/// (`visibility: "hide"`) are left out; the rest in Codex's order (`priority`). `Err` names what was wrong.
pub fn parse_catalog(text: &str) -> Result<Vec<Model>, &'static str> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|_| MALFORMED)?;
    let list = v.get("models").and_then(Value::as_array).ok_or(MALFORMED)?;
    let mut out: Vec<(i64, Model)> = vec![];
    for m in list {
        let Some(id) = m["slug"].as_str().filter(|s| valid_id(s)) else { continue };
        let vis = m["visibility"].as_str().unwrap_or("list");
        if vis == "hide" || vis == "none" { continue; }
        if out.iter().any(|(_, o)| o.id == id) { continue; }
        let efforts: Vec<String> = m["supported_reasoning_levels"].as_array().into_iter().flatten()
            .filter_map(|e| e["effort"].as_str().or(e.as_str())).filter(|e| valid_id(e)).map(String::from).collect();
        out.push((m["priority"].as_i64().unwrap_or(i64::MAX), Model {
            id: id.to_string(),
            label: m["display_name"].as_str().filter(|s| !s.trim().is_empty()).map(String::from),
            description: m["description"].as_str().filter(|s| !s.trim().is_empty()).map(String::from),
            efforts,
            default_effort: m["default_reasoning_level"].as_str().filter(|e| valid_id(e)).map(String::from),
            context: m["context_window"].as_u64().filter(|c| *c > 0),
            is_default: false,
        }));
    }
    out.sort_by_key(|(p, _)| *p);
    let mut models: Vec<Model> = out.into_iter().map(|(_, m)| m).collect();
    if models.is_empty() { return Err(EMPTY); }
    models[0].is_default = true;
    Ok(models)
}

/// One page of the app server's `model/list` answer (its `result`): `{"data": [{"id", "model",
/// "displayName", "description", "hidden", "isDefault", "supportedReasoningEfforts":
/// [{"reasoningEffort"}], "defaultReasoningEffort"}…], "nextCursor"}`. Hidden models are left out.
pub fn parse_model_page(result: &Value) -> Result<(Vec<Model>, Option<String>), &'static str> {
    let list = result.get("data").and_then(Value::as_array).ok_or(MALFORMED)?;
    let mut out = vec![];
    for m in list {
        if m["hidden"].as_bool().unwrap_or(false) { continue; }
        let Some(id) = m["model"].as_str().or(m["id"].as_str()).filter(|s| valid_id(s)) else { continue };
        let efforts = m["supportedReasoningEfforts"].as_array().into_iter().flatten()
            .filter_map(|e| e["reasoningEffort"].as_str().or(e.as_str())).filter(|e| valid_id(e)).map(String::from).collect();
        out.push(Model {
            id: id.to_string(),
            label: m["displayName"].as_str().filter(|s| !s.trim().is_empty()).map(String::from),
            description: m["description"].as_str().filter(|s| !s.trim().is_empty()).map(String::from),
            efforts,
            default_effort: m["defaultReasoningEffort"].as_str().filter(|e| valid_id(e)).map(String::from),
            context: None,
            is_default: m["isDefault"].as_bool().unwrap_or(false),
        });
    }
    Ok((out, result["nextCursor"].as_str().filter(|c| !c.is_empty()).map(String::from)))
}

/// The models named in Codex's `config.toml`: the top-level `model` and each profile's
/// (`[profiles.<name>]`). Read line by line, as the file's simple `key = "value"` lines are.
pub fn config_models(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut section = String::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') { section = l.trim_matches(|c| c == '[' || c == ']').trim().to_string(); continue; }
        let top = section.is_empty();
        let profile = section.starts_with("profiles.") && section.matches('.').count() == 1
            || section.starts_with("profiles.\"") && section.ends_with('"');
        if !(top || profile) { continue; }
        let Some((k, v)) = l.split_once('=') else { continue };
        if k.trim() != "model" { continue; }
        let v = v.split('#').next().unwrap_or("").trim();
        let v = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\''))).unwrap_or("");
        if valid_id(v) && !out.iter().any(|o| o == v) { out.push(v.to_string()); }
    }
    out
}

/// The efforts of all the models, each once, in the order the catalog gives them.
pub fn all_efforts(models: &[Model]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for e in models.iter().flat_map(|m| m.efforts.iter()) { if !out.contains(e) { out.push(e.clone()); } }
    out
}

// ---------- asking the program ----------

#[cfg(test)]
tokio::task_local! {
    /// Under test: a rung whose stand-in hangs on purpose, and the shorter limit it gets.
    pub static CUT: (&'static str, Duration);
}

/// A probe's time limit: `normal`, under test too, so a busy machine cannot make a quick answer
/// look like a hang. A test that makes one rung hang on purpose cuts that rung's limit (`CUT`),
/// and only that one, so it does not wait the full time.
fn lim(rung: &str, normal: Duration) -> Duration {
    #[cfg(test)]
    if let Ok((cut, short)) = CUT.try_with(|c| *c) { if !rung.is_empty() && cut == rung { return short; } }
    let _ = rung;
    normal
}

/// `home` is the user's own Codex folder: their sign-in and settings decide which models they have.
fn probe_cmd(bin: &Path, home: &Path, dir: &Path) -> tokio::process::Command {
    let mut c = crate::codex::cmd(bin);
    c.current_dir(dir).env("CODEX_HOME", home);
    c
}

/// A fresh, empty folder for a probe to run in (Codex reads settings from the folder it runs in).
fn probe_dir() -> Option<PathBuf> {
    let d = std::env::temp_dir().join(format!("thoughts-codex-models-{}", uuid::Uuid::new_v4().simple()));
    crate::paths::create_private_dir(&d).ok().map(|_| d)
}

/// Run one probe. `rung` names the rung it asks for models (empty for the help and version probes).
async fn run(bin: &Path, home: &Path, rung: &str, args: &[&str], limit: Duration) -> Result<String, &'static str> {
    let Some(dir) = probe_dir() else { return Err(FAILED) };
    let mut c = probe_cmd(bin, home, &dir);
    c.args(args);
    let out = crate::proc::output_timeout(&mut c, lim(rung, limit)).await;
    let _ = std::fs::remove_dir_all(&dir);
    match out {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).to_string()),
        Ok(_) => Err(FAILED),
        Err(e) if crate::proc::timed_out(&e) => Err(TIMED_OUT),
        Err(_) => Err(FAILED),
    }
}

/// Ask the app server (`codex app-server`, JSON lines on stdin and stdout) for `model/list`, every
/// page, then end it.
pub async fn app_server_models(bin: &Path, home: &Path, limit: Duration) -> Result<Vec<Model>, &'static str> {
    let Some(dir) = probe_dir() else { return Err(FAILED) };
    let mut c = probe_cmd(bin, home, &dir);
    c.arg("app-server").args(["-c", "mcp_servers={}"]);
    if !crate::claude::sends_reports() { c.args(["-c", "analytics.enabled=false"]); }
    c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    let mut child = match c.spawn() { Ok(ch) => ch, Err(_) => { let _ = std::fs::remove_dir_all(&dir); return Err(FAILED) } };
    let pid = child.id();
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        if let Some(p) = pid { crate::proc::end_tree(p).await; }
        let _ = child.kill().await;
        let _ = std::fs::remove_dir_all(&dir);
        return Err(FAILED);
    };
    let talk = async {
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        let send = |v: Value| { let mut s = v.to_string(); s.push('\n'); s };
        // The answer to request `id` (other lines, notifications and logs, are passed over).
        async fn answer(lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>, id: u64) -> Result<Value, &'static str> {
            loop {
                let Ok(Some(l)) = lines.next_line().await else { return Err(FAILED) };
                let Ok(v) = serde_json::from_str::<Value>(&l) else { continue };
                if v["id"].as_u64() != Some(id) { continue; }
                if v.get("error").is_some() { return Err(FAILED); }
                return v.get("result").cloned().ok_or(MALFORMED);
            }
        }
        let init = send(json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "thoughts", "title": "Thoughts", "version": env!("CARGO_PKG_VERSION")}}}));
        stdin.write_all(init.as_bytes()).await.map_err(|_| FAILED)?;
        answer(&mut lines, 1).await?;
        stdin.write_all(send(json!({"method": "initialized"})).as_bytes()).await.map_err(|_| FAILED)?;
        let mut all: Vec<Model> = vec![];
        let mut cursor: Option<String> = None;
        for page in 0..10u64 {
            let id = 2 + page;
            let req = send(json!({"id": id, "method": "model/list", "params": {"limit": 100, "cursor": cursor}}));
            stdin.write_all(req.as_bytes()).await.map_err(|_| FAILED)?;
            let (models, next) = parse_model_page(&answer(&mut lines, id).await?)?;
            for m in models { if !all.iter().any(|a| a.id == m.id) { all.push(m); } }
            match next { Some(n) if Some(&n) != cursor.as_ref() => cursor = Some(n), _ => break }
        }
        Ok::<_, &'static str>(all)
    };
    let r = tokio::time::timeout(lim(APP_SERVER, limit), talk).await.unwrap_or(Err(TIMED_OUT));
    if let Some(p) = pid { crate::proc::end_tree(p).await; }
    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(&dir);
    match r {
        Ok(v) if v.is_empty() => Err(EMPTY),
        Ok(mut v) => { if !v.iter().any(|m| m.is_default) { v[0].is_default = true; } Ok(v) }
        Err(e) => Err(e),
    }
}

/// The catalog files newer versions keep in Codex's folder (`models_cache.json`, and one per
/// client, `models_cache.<client>.json`), newest first.
fn cache_files(home: &Path) -> Vec<PathBuf> {
    let mut v: Vec<(u64, PathBuf)> = std::fs::read_dir(home).into_iter().flatten().flatten()
        .filter(|e| { let n = e.file_name().to_string_lossy().to_string(); n.starts_with("models_cache") && n.ends_with(".json") })
        .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
        .map(|e| (mtime(&e.path()).unwrap_or(0), e.path())).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, p)| p).collect()
}

pub fn key_for(bin: &Path, home: &Path) -> Key {
    let meta = std::fs::metadata(bin).ok();
    Key {
        bin: bin.to_path_buf(),
        bin_len: meta.as_ref().map_or(0, |m| m.len()),
        bin_modified: meta.and_then(|m| m.modified().ok()).map(secs),
        config_modified: mtime(&home.join("config.toml")),
        cache_modified: cache_files(home).first().and_then(|p| mtime(p)),
    }
}

/// Climb the ladder once (see the top of this file).
pub async fn detect(bin: &Path, home: &Path) -> Detected {
    let mut d = Detected { key: key_for(bin, home), checked_at: now(), ..Default::default() };
    d.version = run(bin, home, "", &["--version"], HELP_LIMIT).await.map(|s| s.trim().to_string()).unwrap_or_default();
    let top = run(bin, home, "", &["--help"], HELP_LIMIT).await.unwrap_or_default();
    let top_cmds = crate::codex::subcommands(&top);
    let tried = |rung: &str, outcome: &str, d: &mut Detected| d.tried.push(Tried { rung: rung.into(), outcome: outcome.into() });
    // 1. codex debug models (then --bundled)
    let debug = if top_cmds.iter().any(|c| c == "debug") { run(bin, home, "", &["debug", "--help"], HELP_LIMIT).await.unwrap_or_default() } else { String::new() };
    if crate::codex::subcommands(&debug).iter().any(|c| c == "models") {
        match run(bin, home, DEBUG_MODELS, &["debug", "models"], DEBUG_LIMIT).await.and_then(|t| parse_catalog(&t)) {
            Ok(m) => { tried(DEBUG_MODELS, OK, &mut d); d.source = DEBUG_MODELS.into(); d.models = m; return d; }
            Err(e) => tried(DEBUG_MODELS, e, &mut d),
        }
        let help = run(bin, home, "", &["debug", "models", "--help"], HELP_LIMIT).await.unwrap_or_default();
        if crate::codex::has_flag(&help, "--bundled") {
            match run(bin, home, BUNDLED, &["debug", "models", "--bundled"], BUNDLED_LIMIT).await.and_then(|t| parse_catalog(&t)) {
                Ok(m) => { tried(BUNDLED, OK, &mut d); d.source = BUNDLED.into(); d.models = m; return d; }
                Err(e) => tried(BUNDLED, e, &mut d),
            }
        }
    } else { tried(DEBUG_MODELS, UNAVAILABLE, &mut d); }
    // 2. codex app-server, model/list
    if top_cmds.iter().any(|c| c == "app-server") {
        match app_server_models(bin, home, APP_SERVER_LIMIT).await {
            Ok(m) => { tried(APP_SERVER, OK, &mut d); d.source = APP_SERVER.into(); d.models = m; return d; }
            Err(e) => tried(APP_SERVER, e, &mut d),
        }
    } else { tried(APP_SERVER, UNAVAILABLE, &mut d); }
    // 3. the catalog Codex keeps on disk
    let files = cache_files(home);
    if files.is_empty() { tried(CACHE, UNAVAILABLE, &mut d); }
    for f in files {
        match std::fs::read_to_string(&f).map_err(|_| FAILED).and_then(|t| parse_catalog(&t)) {
            Ok(m) => { tried(CACHE, OK, &mut d); d.source = CACHE.into(); d.models = m; return d; }
            Err(e) => tried(CACHE, e, &mut d),
        }
    }
    d
}

// ---------- keeping the answer ----------

static STORE: std::sync::Mutex<Option<Detected>> = std::sync::Mutex::new(None);
static REFRESHING: AtomicBool = AtomicBool::new(false);
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn store_file() -> PathBuf { crate::paths::home().join("codex-models.json") }

/// The last answer (from memory, else from the app's folder), whatever it was for.
pub fn last() -> Option<Detected> {
    let mut g = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() { *g = std::fs::read_to_string(store_file()).ok().and_then(|t| serde_json::from_str(&t).ok()); }
    g.clone()
}

/// The last answer, if it is for this program.
pub fn last_for(bin: &Path) -> Option<Detected> { last().filter(|d| d.key.bin == bin) }

fn keep(d: &Detected) {
    *STORE.lock().unwrap_or_else(|e| e.into_inner()) = Some(d.clone());
    if let Ok(t) = serde_json::to_string_pretty(d) {
        crate::log::ok_or_warn("could not keep the Codex models", crate::paths::write_atomic(&store_file(), t.as_bytes(), true));
    }
}

/// Is `d` still the answer for the program and settings as they are now?
pub fn fresh(d: &Detected, key: &Key) -> bool { d.key == *key && now().saturating_sub(d.checked_at) < MAX_AGE.as_secs() }

pub fn refreshing() -> bool { REFRESHING.load(Ordering::Relaxed) }

/// Ask the program now (one ladder at a time; a second caller waits for the first and, unless
/// `force`, takes its answer), keep the answer and tell the windows (`codex.models`).
pub async fn refresh(bin: &Path, events: Option<&tokio::sync::broadcast::Sender<Value>>, force: bool) -> Detected {
    let home = crate::codex::codex_home();
    let _g = ONE_AT_A_TIME.lock().await;
    if !force { if let Some(d) = last_for(bin).filter(|d| fresh(d, &key_for(bin, &home))) { REFRESHING.store(false, Ordering::Relaxed); return d; } }
    REFRESHING.store(true, Ordering::Relaxed);
    let d = detect(bin, &home).await;
    keep(&d);
    REFRESHING.store(false, Ordering::Relaxed);
    let outcome = d.tried.iter().map(|t| format!("{} {}", t.rung, t.outcome)).collect::<Vec<_>>().join(", ");
    crate::log::info(format!("Codex models: {} from {} ({outcome}; {})", d.models.len(), if d.source.is_empty() { NONE } else { &d.source }, d.version));
    if let Some(e) = events { let _ = e.send(json!({"type": "codex.models", "source": d.source, "count": d.models.len()})); }
    d
}

/// The answer for this program, without waiting: what is kept, and a refresh in the background
/// when it is missing or out of date.
pub fn current(bin: &Path, events: &tokio::sync::broadcast::Sender<Value>) -> Option<Detected> {
    // Unit tests of other parts never have their stand-in programs asked behind their back.
    current_with(bin, events, !cfg!(test))
}

pub fn current_with(bin: &Path, events: &tokio::sync::broadcast::Sender<Value>, auto: bool) -> Option<Detected> {
    let key = key_for(bin, &crate::codex::codex_home());
    let d = last_for(bin);
    if auto && !d.as_ref().map_or(false, |d| fresh(d, &key)) && !REFRESHING.swap(true, Ordering::Relaxed) {
        let (bin, events) = (bin.to_path_buf(), events.clone());
        tokio::spawn(async move { refresh(&bin, Some(&events), false).await; });
    }
    // An answer for a different program is not this one's.
    d
}

/// The efforts the kept catalog lists for `model` (none known: empty).
pub fn efforts_for(bin: &Path, model: &str) -> Vec<String> {
    last_for(bin).and_then(|d| d.model(model).map(|m| m.efforts.clone())).unwrap_or_default()
}

/// The efforts of every model of the kept catalog.
pub fn catalog_efforts(bin: &Path) -> Vec<String> { last_for(bin).map(|d| all_efforts(&d.models)).unwrap_or_default() }

/// How much `model` (unnamed: Codex's default) can take in, as the kept catalog says.
pub fn context_of(model: Option<&str>) -> Option<u64> {
    let d = last()?;
    match model { Some(m) => d.model(m).and_then(|m| m.context), None => d.models.iter().find(|m| m.is_default).and_then(|m| m.context) }
}

/// The models part of `GET /api/ai/codex/options`: every name to offer (the catalog first, then
/// `config.toml`'s, the ones runs answered with, the user's picks) and where they came from.
pub fn options_part(d: Option<&Detected>, config: &[String], seen: &[String], picks: [Option<String>; 2], refreshing: bool) -> Value {
    let catalog: Vec<String> = d.map(|d| d.models.iter().map(|m| m.id.clone()).collect()).unwrap_or_default();
    let names = crate::claude::model_choices(catalog, &[config, seen].concat(), picks);
    let detected = d.map_or(false, |d| !d.source.is_empty() && !d.models.is_empty());
    let source = if detected { d.map(|d| d.source.clone()).unwrap_or_default() }
        else if !config.is_empty() { CONFIG.into() } else if !seen.is_empty() { SEEN.into() } else { NONE.into() };
    json!({
        "models": names,
        "modelInfo": d.filter(|_| detected).map(|d| d.models.clone()).unwrap_or_default(),
        "modelsSource": source,
        "modelsDetected": detected,
        "modelsTooOld": d.map_or(false, Detected::too_old),
        "modelsCheckedAt": d.map(|d| d.checked_at),
        "modelsTried": d.map(|d| d.tried.clone()).unwrap_or_default(),
        "modelsRefreshing": refreshing,
        "codexVersion": d.map(|d| d.version.clone()).unwrap_or_default(),
    })
}

#[cfg(test)]
#[path = "codex_models_tests.rs"]
mod tests;
