//! HTTP handlers for the customisation layer: staging (pending / preview / apply / discard),
//! history and per-change undo, packs, workflows, the usage log, suggestions and layer updates.

use crate::api::ApiErr;
use crate::applayer::{self, Dirs, Git};
use crate::state::AppState;
use crate::{packs, staging, suggestions, updates, usage, workflows};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::routing::{get, post};
use chrono::Local;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

type S = State<Arc<AppState>>;
type R = Result<Json<Value>, ApiErr>;
fn ok() -> R { Ok(Json(json!({"ok": true}))) }

/// One layer operation at a time (apply, discard, undo, rollback, import, starting a run).
static LAYER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/layer", get(layer))
        .route("/api/layer/history", get(history))
        .route("/api/layer/rollback", post(rollback))
        .route("/api/layer/revert", post(revert))
        .route("/api/layer/undo-last", post(undo_last))
        .route("/api/layer/storage/:plugin", get(storage_get).put(storage_put).layer(DefaultBodyLimit::max(STORAGE_MAX_BYTES + 1024)))
        .route("/api/layer/customize", post(customize))
        .route("/api/layer/pending", get(pending))
        .route("/api/layer/pending/apply", post(pending_apply))
        .route("/api/layer/pending/discard", post(pending_discard))
        .route("/api/layer/export", get(export))
        .route("/api/layer/import", post(import).layer(DefaultBodyLimit::max(8 * 1024 * 1024)))
        .route("/api/workflows", get(workflow_list))
        .route("/api/workflows/approve", post(workflow_approve))
        .route("/api/workflows/disable", post(workflow_disable))
        .route("/api/workflows/run", post(workflow_run))
        .route("/api/workflows/limits", post(workflow_limits))
        .route("/api/usage", post(usage_event))
        .route("/api/suggestions", get(suggestion_list))
        .route("/api/suggestions/:id/dismiss", post(suggestion_dismiss))
        .route("/api/updates", get(update_status))
        .route("/api/updates/status", get(update_status))
        .route("/api/updates/resolve", post(update_resolve))
}

/// Background work of the layer: default-file updates, the workflow runner, and the local usage log.
pub fn start(state: Arc<AppState>) {
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = updates::check(&st).await { crate::log::warn(format!("layer: update check failed: {e}")); }
    });
    tokio::spawn(workflows::run_loop(state.clone()));
    tokio::spawn(async move {
        let mut rx = state.events.subscribe();
        loop {
            match rx.recv().await {
                Ok(e) if e["type"] == "wrap.done" => { let _ = usage::append(&crate::paths::home(), "wrap.done", chrono::Local::now()); }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return,
            }
        }
    });
}

async fn arabic(st: &AppState) -> bool { st.settings.read().await.language == "ar" }

/// Note in the local usage log how a customisation ended (unless the user switched that log off).
async fn record_outcome(st: &AppState, outcome: &str, request: &str) {
    if !st.settings.read().await.usage_signals { return; }
    usage::record_outcome(&crate::paths::home(), outcome, request, Local::now());
}

/// What the user asked for in each kept change (`hash -> request`), so a later undo can be logged
/// with the same words. Local, small, capped.
fn requests_file(home: &std::path::Path) -> std::path::PathBuf { home.join("layer-requests.json") }

fn remember_request(home: &std::path::Path, hash: &str, request: &str) {
    if hash.is_empty() || request.is_empty() { return; }
    let mut list: Vec<Value> = std::fs::read_to_string(requests_file(home)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    list.push(json!({"hash": hash, "request": request}));
    let keep = list.len().saturating_sub(200);
    if let Ok(t) = serde_json::to_string(&list[keep..]) { let _ = crate::paths::write_atomic(&requests_file(home), t.as_bytes(), true); }
}

fn recall_request(home: &std::path::Path, hash: &str) -> Option<String> {
    let list: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(requests_file(home)).ok()?).ok()?;
    list.iter().rev().find(|e| e["hash"].as_str().map_or(false, |h| !hash.is_empty() && (h.starts_with(hash) || hash.starts_with(h))))
        .and_then(|e| e["request"].as_str().map(String::from))
}

/// `GET /api/layer` — the live layer; `?preview=1` — the staged one, in the same shape.
async fn layer(State(st): S, Query(q): Query<HashMap<String, String>>) -> R {
    let d = Dirs::real();
    let ar = arabic(&st).await;
    let preview = q.get("preview").map_or(false, |v| v == "1" || v == "true");
    if preview {
        if !d.staging.join(".git").exists() { return Err(anyhow::anyhow!("there is no pending change to preview").into()); }
        return Ok(Json(applayer::manifest_lang(&d.staging, ar)));
    }
    Ok(Json(applayer::manifest_lang(&d.layer, ar)))
}

async fn history(State(st): S) -> R { Ok(Json(json!(applayer::history(&st).await))) }

#[derive(Deserialize)]
struct Commit { hash: String }

async fn rollback(State(st): S, Json(c): Json<Commit>) -> R {
    let _g = LAYER_LOCK.lock().await;
    applayer::rollback(&st, &c.hash).await?;
    ok()
}

async fn revert(State(st): S, Json(c): Json<Commit>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    let subject = applayer::subject_in(Git(Some(&st)), &d.layer, &c.hash).await;
    applayer::revert(&st, &c.hash).await?;
    record_outcome(&st, "undo", &recall_request(&d.home, &c.hash).unwrap_or(subject)).await;
    ok()
}

/// `POST /api/layer/undo-last` — undo the most recent change that is still in effect.
async fn undo_last(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    let (hash, subject) = applayer::undo_last_in(Git(Some(&st)), &d.layer, arabic(&st).await).await?;
    st.emit(json!({"type": "layer.changed"}));
    record_outcome(&st, "undo", &recall_request(&d.home, &hash).unwrap_or_else(|| subject.clone())).await;
    Ok(Json(json!({"ok": true, "hash": hash, "summary": subject})))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Customize {
    prompt: String,
    /// The older name of the same field. It is a field of its own, not a serde alias: a client that
    /// sends both names in one request would otherwise be refused with "duplicate field `prompt`".
    text: String,
}

impl Customize {
    fn request(self) -> String { if self.prompt.trim().is_empty() { self.text } else { self.prompt } }
}

async fn customize(State(st): S, Json(c): Json<Customize>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let request = c.request();
    let id = crate::api::start_customize(st.clone(), request.clone(), "customize", None).await?;
    staging::set_request(&Dirs::real(), &request);
    Ok(Json(json!({"chatId": id})))
}

async fn pending(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    Ok(Json(staging::pending(Git(Some(&st)), &Dirs::real(), arabic(&st).await).await))
}

/// Which workflow files a kept change may approve: only a change the user made through the
/// assistant and reviewed (`customize`), and only when they explicitly allowed it. Imported packs
/// and app updates never approve a workflow on their own.
fn workflows_to_approve(pending: &Value, approve: bool) -> Vec<String> {
    if !approve || pending["origin"] != "customize" { return vec![]; }
    pending["files"].as_array().into_iter().flatten()
        .filter(|f| f["status"] != "deleted").filter_map(|f| f["path"].as_str())
        .filter(|p| p.starts_with("workflows/")).map(String::from).collect()
}

/// Body: `{signature, approveWorkflows?}`. `signature` is the one `GET /api/layer/pending` gave
/// for the change the user reviewed; if staging no longer matches it the apply fails with `CHANGED::`.
async fn pending_apply(State(st): S, body: axum::body::Bytes) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    let ar = arabic(&st).await;
    let body = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    let approve = body["approveWorkflows"] == true;
    // A keep that does not say what was reviewed is treated like a review of something else.
    let Some(signature) = body["signature"].as_str().filter(|s| !s.is_empty()) else { return Err(anyhow::anyhow!(staging::changed_error(ar)).into()) };
    let before = staging::pending(Git(Some(&st)), &d, ar).await;
    let mut r = staging::apply(Git(Some(&st)), &d, ar, Some(signature)).await?;
    let approved = workflows::approve_paths(&d.layer, &d.home, &workflows_to_approve(&before, approve))?;
    if !approved.is_empty() { crate::log::info(format!("workflows approved with a kept change: {}", approved.join(", "))); }
    r["approvedWorkflows"] = json!(approved);
    if r["origin"] == "customize" {
        let request = r["request"].as_str().unwrap_or_default().to_string();
        remember_request(&d.home, r["hash"].as_str().unwrap_or_default(), &request);
        record_outcome(&st, "keep", &request).await;
    }
    st.emit(json!({"type": "layer.changed"}));
    st.emit(json!({"type": "layer.pending"}));
    Ok(Json(r))
}

async fn pending_discard(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    let meta = staging::load_meta(&d);
    let had_change = staging::pending(Git(Some(&st)), &d, false).await["exists"] == true;
    staging::discard(Git(Some(&st)), &d).await?;
    if let Some(m) = meta.filter(|m| had_change && (m.origin.is_empty() || m.origin == "customize")) { record_outcome(&st, "discard", &m.request).await; }
    st.emit(json!({"type": "layer.pending"}));
    ok()
}

/// `GET /api/layer/export?name=&description=&paths=a,b` — the whole layer, or with `paths` (files,
/// or folders ending in `/`) a feature pack of just those.
async fn export(Query(q): Query<HashMap<String, String>>) -> R {
    let get = |k: &str| q.get(k).map_or("", String::as_str);
    let paths: Option<Vec<String>> = q.get("paths").filter(|p| !p.trim().is_empty()).map(|p| p.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect());
    Ok(Json(packs::export_with(&Dirs::real().layer, get("name"), get("description"), paths.as_deref())?))
}

#[derive(Deserialize)]
struct Import { pack: Value }

async fn import(State(st): S, Json(i): Json<Import>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    applayer::ensure_in(Git(Some(&st)), &d).await?;
    let ar = arabic(&st).await;
    packs::import(Git(Some(&st)), &d, &i.pack, ar).await?;
    st.emit(json!({"type": "layer.pending"}));
    Ok(Json(staging::pending(Git(Some(&st)), &d, ar).await))
}

// ---------- plugin storage ----------

/// One JSON document per plugin, at most this large.
pub const STORAGE_MAX_BYTES: usize = 256 * 1024;

/// Plugin names are file names: letters, digits, dot, dash, underscore (never only dots).
pub fn valid_plugin_name(name: &str) -> bool {
    (1..=60).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) && !name.starts_with('.')
}

fn storage_file(home: &std::path::Path, plugin: &str) -> anyhow::Result<std::path::PathBuf> {
    if !valid_plugin_name(plugin) { anyhow::bail!("invalid plugin name"); }
    Ok(home.join("plugin-data").join(format!("{plugin}.json")))
}

/// What plugin `plugin` has stored (`{}` when nothing yet, or when its file is damaged).
pub fn storage_read(home: &std::path::Path, plugin: &str) -> anyhow::Result<Value> {
    let file = storage_file(home, plugin)?;
    let stored = std::fs::symlink_metadata(&file).ok().filter(|m| m.is_file() && m.len() as usize <= STORAGE_MAX_BYTES)
        .and_then(|_| std::fs::read_to_string(&file).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object);
    Ok(stored.unwrap_or_else(|| json!({})))
}

/// Replace plugin `plugin`'s document. Private folder, private file, written atomically.
pub fn storage_write(home: &std::path::Path, plugin: &str, bytes: &[u8], arabic: bool) -> anyhow::Result<()> {
    let file = storage_file(home, plugin)?;
    if bytes.len() > STORAGE_MAX_BYTES {
        anyhow::bail!("{}", if arabic { "البيانات المحفوظة لهذه الإضافة كبيرة جدًا (الحد الأقصى ٢٥٦ كيلوبايت)." } else { "This add-on is trying to store too much (the limit is 256 KB)." });
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("storage must be a JSON object"))?;
    if !v.is_object() { anyhow::bail!("storage must be a JSON object"); }
    let dir = home.join("plugin-data");
    crate::paths::create_private_dir(&dir)?;
    // Never write through a link someone planted in the data folder.
    if std::fs::symlink_metadata(&file).map_or(false, |m| !m.is_file()) { anyhow::bail!("storage is not available for this plugin"); }
    crate::paths::write_atomic(&file, serde_json::to_string(&v)?.as_bytes(), true)?;
    Ok(())
}

/// `GET /api/layer/storage/<plugin>` → the plugin's JSON object (`{}` when empty).
async fn storage_get(Path(plugin): Path<String>) -> R {
    Ok(Json(storage_read(&crate::paths::home(), &plugin)?))
}

/// `PUT /api/layer/storage/<plugin>` with the JSON object as the body → `{ok:true}`.
async fn storage_put(State(st): S, Path(plugin): Path<String>, body: axum::body::Bytes) -> R {
    storage_write(&crate::paths::home(), &plugin, &body, arabic(&st).await)?;
    ok()
}

/// Each automatic task with what it does, how it last ended and its limits. `budgetSupported`:
/// whether the installed assistant can be held to a spending limit (read from its own help).
async fn workflow_list(State(st): S) -> R {
    let mut list = workflows::status(&crate::paths::app_layer_dir(), &crate::paths::home(), chrono::Local::now().naive_local());
    let budget = crate::claude::has_flag(&st, "--max-budget-usd").await;
    for w in &mut list {
        w["budgetSupported"] = json!(budget);
        // A past run links to its chat only while that chat still exists.
        for key in ["last", "history"] {
            let rows: Vec<&mut Value> = match w.get_mut(key) { Some(Value::Array(a)) => a.iter_mut().collect(), Some(v) if v.is_object() => vec![v], _ => vec![] };
            for r in rows {
                let exists = match r["chatId"].as_str() { Some(id) => st.chats.with(id, |_| ()).await.is_some(), None => false };
                r["chatExists"] = json!(exists);
            }
        }
    }
    Ok(Json(json!(list)))
}

/// Which task: its file (its identity) and, for approve and run, the content hash the user was shown.
#[derive(Deserialize, Default)]
#[serde(default)]
struct WorkflowRef { file: String, hash: String }

/// Allow the task the user was shown to run on its own, in exactly that form.
async fn workflow_approve(State(st): S, Json(w): Json<WorkflowRef>) -> R {
    let l = workflows::approve(&crate::paths::app_layer_dir(), &crate::paths::home(), &w.file, &w.hash)?;
    crate::log::info(format!("workflow \"{}\" ({}) approved", l.workflow.name, l.file));
    st.emit(json!({"type": "workflows.changed"}));
    ok()
}

/// Stop a workflow from running. Its file stays; approving it again turns it back on.
async fn workflow_disable(State(st): S, Json(w): Json<WorkflowRef>) -> R {
    let l = workflows::disable(&crate::paths::app_layer_dir(), &crate::paths::home(), &w.file)?;
    crate::log::info(format!("workflow \"{}\" ({}) disabled", l.workflow.name, l.file));
    st.emit(json!({"type": "workflows.changed"}));
    ok()
}

/// "Run now": once, as a chat the user can watch. → `{chatId}`
async fn workflow_run(State(st): S, Json(w): Json<WorkflowRef>) -> R {
    let id = workflows::run_now(&st, &w.file, &w.hash).await?;
    st.emit(json!({"type": "workflows.changed"}));
    Ok(Json(json!({"chatId": id})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowLimits { file: String, minutes: u32, budget_usd: f64 }

async fn workflow_limits(State(st): S, Json(w): Json<WorkflowLimits>) -> R {
    workflows::set_limits(&w.file, w.minutes, w.budget_usd).await?;
    st.emit(json!({"type": "workflows.changed"}));
    ok()
}

#[derive(Deserialize)]
struct UsageEvent { event: String }

async fn usage_event(State(st): S, Json(u): Json<UsageEvent>) -> R {
    // The user switched the local usage log off: record nothing.
    if !st.settings.read().await.usage_signals { return ok(); }
    usage::append(&crate::paths::home(), &u.event, chrono::Local::now())?;
    ok()
}

async fn suggestion_list(State(st): S) -> R {
    let s = st.settings.read().await.clone();
    if !s.usage_signals { return Ok(Json(json!([]))); }
    let home = crate::paths::home();
    let layer = crate::paths::app_layer_dir();
    let now = chrono::Local::now();
    let events = usage::read(&home);
    let overdue = s.library().map_or(0, |l| suggestions::overdue_todos(&l, now.date_naive()));
    let enabled: Vec<&str> = suggestions::MODULE_VIEWS.iter()
        .filter(|(module, ..)| s.modules.get(*module).copied().unwrap_or_else(|| crate::modules::get(module).map_or(false, |m| m.default_on)))
        .map(|(_, view, ..)| *view).collect();
    let chips: Vec<String> = applayer::manifest_of(&layer)["home"]["chips"].as_array().into_iter().flatten()
        .filter_map(|c| c["prompt"].as_str().map(String::from)).collect();
    let has_weekly = workflows::load(&layer).iter()
        .any(|w| matches!(&w.trigger, workflows::Trigger::Schedule { every, .. } if every == "week"));
    let nav_hidden: Vec<String> = applayer::manifest_of(&layer)["nav"]["hidden"].as_array().into_iter().flatten()
        .filter_map(|h| h.as_str().map(String::from)).collect();
    let dismissed = suggestions::dismissed(&home);
    Ok(Json(json!(suggestions::compute(&suggestions::Input {
        events: &events, now, overdue, enabled_views: &enabled, chips: &chips,
        has_weekly_workflow: has_weekly, dismissed: &dismissed, arabic: s.language == "ar", nav_hidden: &nav_hidden,
    }))))
}

async fn suggestion_dismiss(Path(id): Path<String>) -> R {
    suggestions::dismiss(&crate::paths::home(), &id)?;
    ok()
}

async fn update_status() -> R { Ok(Json(updates::status(&crate::paths::home()))) }

#[derive(Deserialize)]
struct Resolve { path: String, choice: String }

async fn update_resolve(State(st): S, Json(r): Json<Resolve>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    if r.choice == "merge" {
        let prompt = updates::merge_prompt(&d, applayer::SHIPPED, &r.path)?;
        let title = if arabic(&st).await { format!("دمج تحديث {}", r.path) } else { format!("Merge update of {}", r.path) };
        let id = crate::api::start_customize(st.clone(), prompt, "update", Some(title)).await?;
        updates::clear_conflict(&d.home, &r.path)?;
        return Ok(Json(json!({"ok": true, "chatId": id, "status": updates::status(&d.home)})));
    }
    updates::resolve(Git(Some(&st)), &d, applayer::SHIPPED, &r.path, &r.choice).await?;
    if r.choice == "new" { st.emit(json!({"type": "layer.changed"})); }
    Ok(Json(json!({"ok": true, "status": updates::status(&d.home)})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_customise_request_may_carry_both_names_of_its_text() {
        let both: Customize = serde_json::from_str(r#"{"prompt":"make it green","text":"make it green"}"#).unwrap();
        assert_eq!(both.request(), "make it green");
        let old: Customize = serde_json::from_str(r#"{"text":"older client"}"#).unwrap();
        assert_eq!(old.request(), "older client");
        let new: Customize = serde_json::from_str(r#"{"prompt":"newer client"}"#).unwrap();
        assert_eq!(new.request(), "newer client");
    }

    #[test]
    fn only_a_reviewed_customisation_can_approve_workflows() {
        let pending = |origin: &str| json!({"origin": origin, "files": [
            {"path": "workflows/weekly.json", "status": "added"}, {"path": "workflows/old.json", "status": "deleted"},
            {"path": "custom.css", "status": "modified"}]});
        assert_eq!(workflows_to_approve(&pending("customize"), true), ["workflows/weekly.json"]);
        // Without the explicit tick nothing is approved, and packs / app updates never are.
        assert!(workflows_to_approve(&pending("customize"), false).is_empty());
        assert!(workflows_to_approve(&pending("import"), true).is_empty());
        assert!(workflows_to_approve(&pending("update"), true).is_empty());
        assert!(workflows_to_approve(&json!({"exists": false}), true).is_empty());
    }

    #[test]
    fn plugin_storage_is_one_private_json_object_per_plugin() {
        let home = std::env::temp_dir().join(format!("thoughts-storage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(storage_read(&home, "hours.js").unwrap(), json!({}), "nothing stored yet");
        storage_write(&home, "hours.js", br#"{"entries":[{"date":"2026-11-10","client":"Acme","hours":2.5}]}"#, false).unwrap();
        storage_write(&home, "other", br#"{"x":1}"#, false).unwrap();
        assert_eq!(storage_read(&home, "hours.js").unwrap()["entries"][0]["hours"], 2.5);
        assert_eq!(storage_read(&home, "other").unwrap(), json!({"x": 1}), "each plugin has its own document");
        assert!(home.join("plugin-data/hours.js.json").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(home.join("plugin-data")).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(std::fs::metadata(home.join("plugin-data/hours.js.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // Only a JSON object, only up to 256 KB, only for a plain file name.
        assert!(storage_write(&home, "hours.js", b"[1,2]", false).is_err() && storage_write(&home, "hours.js", b"{oops", false).is_err());
        let big = format!(r#"{{"x":"{}"}}"#, "a".repeat(STORAGE_MAX_BYTES));
        assert!(storage_write(&home, "hours.js", big.as_bytes(), true).unwrap_err().to_string().contains("٢٥٦"));
        assert_eq!(storage_read(&home, "hours.js").unwrap()["entries"][0]["client"], "Acme", "a refused write leaves the old document");
        for bad in ["", "..", ".hidden", "a/b", "a\\b", "../../settings", "name with space", &"x".repeat(61), "ملف"] {
            assert!(!valid_plugin_name(bad), "{bad}");
            assert!(storage_read(&home, bad).is_err() && storage_write(&home, bad, b"{}", false).is_err());
        }
        for good in ["hours.js", "client-intake", "a_b.1", &"x".repeat(60)] { assert!(valid_plugin_name(good), "{good}"); }
        assert!(!home.join("settings.json").exists());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn remembers_what_was_asked_for_each_kept_change() {
        let home = std::env::temp_dir().join(format!("thoughts-requests-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(recall_request(&home, "abc123"), None);
        remember_request(&home, "abc123def456", "Show overdue todos on Home");
        remember_request(&home, "ffff0000aaaa", "Group todos by date");
        remember_request(&home, "", "ignored");
        assert_eq!(recall_request(&home, "abc123").as_deref(), Some("Show overdue todos on Home"), "a short hash finds it too");
        assert_eq!(recall_request(&home, "ffff0000aaaa").as_deref(), Some("Group todos by date"));
        assert_eq!(recall_request(&home, ""), None);
        std::fs::remove_dir_all(home).unwrap();
    }
}
