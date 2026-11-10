//! HTTP handlers for the customisation layer: staging (pending / preview / apply / discard),
//! history and per-change undo, packs, workflows, the usage log, suggestions and layer updates.

use crate::api::ApiErr;
use crate::applayer::{self, Dirs, Git};
use crate::state::AppState;
use crate::{packs, staging, suggestions, updates, usage, workflows};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::routing::{get, post};
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
        .route("/api/layer/customize", post(customize))
        .route("/api/layer/pending", get(pending))
        .route("/api/layer/pending/apply", post(pending_apply))
        .route("/api/layer/pending/discard", post(pending_discard))
        .route("/api/layer/export", get(export))
        .route("/api/layer/import", post(import).layer(DefaultBodyLimit::max(8 * 1024 * 1024)))
        .route("/api/workflows", get(workflow_list))
        .route("/api/workflows/approve", post(workflow_approve))
        .route("/api/workflows/disable", post(workflow_disable))
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

/// `GET /api/layer` — the live layer; `?preview=1` — the staged one, in the same shape.
async fn layer(Query(q): Query<HashMap<String, String>>) -> R {
    let d = Dirs::real();
    let preview = q.get("preview").map_or(false, |v| v == "1" || v == "true");
    if preview {
        if !d.staging.join(".git").exists() { return Err(anyhow::anyhow!("there is no pending change to preview").into()); }
        return Ok(Json(applayer::manifest_of(&d.staging)));
    }
    Ok(Json(applayer::manifest_of(&d.layer)))
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
    applayer::revert(&st, &c.hash).await?;
    ok()
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Customize {
    /// `text` is the older name of the same field.
    #[serde(alias = "text")]
    prompt: String,
}

async fn customize(State(st): S, Json(c): Json<Customize>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let id = crate::api::start_customize(st.clone(), c.prompt, "customize", None).await?;
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

/// Body (optional): `{approveWorkflows: true}`.
async fn pending_apply(State(st): S, body: axum::body::Bytes) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    let approve = serde_json::from_slice::<Value>(&body).ok().map_or(false, |v| v["approveWorkflows"] == true);
    let before = staging::pending(Git(Some(&st)), &d, arabic(&st).await).await;
    let mut r = staging::apply(Git(Some(&st)), &d, arabic(&st).await).await?;
    let approved = workflows::approve_paths(&d.layer, &d.home, &workflows_to_approve(&before, approve))?;
    if !approved.is_empty() { crate::log::info(format!("workflows approved with a kept change: {}", approved.join(", "))); }
    r["approvedWorkflows"] = json!(approved);
    st.emit(json!({"type": "layer.changed"}));
    st.emit(json!({"type": "layer.pending"}));
    Ok(Json(r))
}

async fn pending_discard(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    staging::discard(Git(Some(&st)), &Dirs::real()).await?;
    st.emit(json!({"type": "layer.pending"}));
    ok()
}

async fn export(Query(q): Query<HashMap<String, String>>) -> R {
    Ok(Json(packs::export(&Dirs::real().layer, q.get("name").map_or("", String::as_str))))
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

async fn workflow_list() -> R {
    Ok(Json(json!(workflows::status(&crate::paths::app_layer_dir(), &crate::paths::home(), chrono::Local::now().naive_local()))))
}

#[derive(Deserialize)]
struct WorkflowName { name: String }

/// Allow a workflow to run on its own, in exactly its current form.
async fn workflow_approve(State(st): S, Json(w): Json<WorkflowName>) -> R {
    workflows::approve(&crate::paths::app_layer_dir(), &crate::paths::home(), &w.name)?;
    crate::log::info(format!("workflow \"{}\" approved", w.name));
    st.emit(json!({"type": "workflows.changed"}));
    ok()
}

/// Stop a workflow from running. Its file stays; approving it again turns it back on.
async fn workflow_disable(State(st): S, Json(w): Json<WorkflowName>) -> R {
    workflows::disable(&crate::paths::app_layer_dir(), &crate::paths::home(), &w.name)?;
    crate::log::info(format!("workflow \"{}\" disabled", w.name));
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
    let dismissed = suggestions::dismissed(&home);
    Ok(Json(json!(suggestions::compute(&suggestions::Input {
        events: &events, now, overdue, enabled_views: &enabled, chips: &chips,
        has_weekly_workflow: has_weekly, dismissed: &dismissed, arabic: s.language == "ar",
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
}
