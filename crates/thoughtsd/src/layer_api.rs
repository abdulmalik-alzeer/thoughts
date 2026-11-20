//! HTTP handlers for the customisation layer: staging (pending / preview / apply / discard),
//! history and per-change undo, packs, workflows, the usage log, suggestions and layer updates.

use crate::api::ApiErr;
use crate::applayer::{self, Dirs, Git};
use crate::state::AppState;
use crate::{packs, staging, suggestions, updates, usage, workflows};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::routing::{get, post, put};
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
pub(crate) static LAYER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/layer", get(layer))
        .route("/api/layer/history", get(history))
        .route("/api/layer/rollback", post(rollback))
        .route("/api/layer/revert", post(revert))
        .route("/api/layer/undo-last", post(undo_last).get(undo_preview))
        .route("/api/layer/storage/:plugin", get(storage_get).put(storage_put).layer(DefaultBodyLimit::max(STORAGE_MAX_BYTES + 1024)))
        .route("/api/layer/customize", post(customize))
        .route("/api/layer/pending", get(pending))
        .route("/api/layer/pending/apply", post(pending_apply))
        .route("/api/layer/pending/accept", post(pending_accept))
        .route("/api/layer/pending/discard", post(pending_discard))
        .route("/api/layer/pending/continue", post(pending_continue))
        .route("/api/layer/pending/review-again", post(pending_review_again))
        .route("/api/layer/pending/details", get(pending_details))
        .route("/api/layer/drafts", get(draft_list))
        .route("/api/layer/drafts/open", post(draft_open))
        .route("/api/layer/drafts/discard", post(draft_discard))
        .route("/api/layer/export", get(export))
        .route("/api/layer/import", post(import).layer(DefaultBodyLimit::max(8 * 1024 * 1024)))
        .route("/api/layer/pack/inspect", post(pack_inspect).layer(DefaultBodyLimit::max(8 * 1024 * 1024)))
        .route("/api/layer/gallery", get(gallery_list))
        .route("/api/layer/gallery/stage", post(gallery_stage))
        .route("/api/layer/addons", get(addon_list))
        .route("/api/layer/addons/switch", post(addon_switch))
        .route("/api/layer/addons/remove", post(addon_remove))
        .route("/api/layer/addons/permission", post(addon_permission))
        .route("/api/layer/nav", put(nav_edit))
        .route("/api/layer/pages/builtin", post(page_builtin))
        .route("/api/layer/addons/data/restore", post(addon_data_restore))
        .route("/api/layer/addons/data/clear", post(addon_data_clear))
        .route("/api/layer/addons/data/forget", post(addon_data_forget))
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
        // Under the layer lock: the new built-in files are written and recorded in one go, so the
        // first load of the layer never finds them half-way and takes them for outside changes.
        let _g = LAYER_LOCK.lock().await;
        if let Err(e) = updates::check(&st).await { crate::log::warn(format!("layer: update check failed: {e}")); }
    });
    tokio::spawn(workflows::run_loop(state.clone()));
    tokio::spawn(async move {
        let mut rx = state.events.subscribe();
        loop {
            match rx.recv().await {
                // Like every other entry of the local usage log: only while the user keeps it on.
                Ok(e) if e["type"] == "wrap.done" => { if state.settings.read().await.usage_signals { let _ = usage::append(&crate::paths::home(), "wrap.done", chrono::Local::now()); } }
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

/// `usage` = the local usage log is switched on. Off, nothing is remembered and what was is removed:
/// these words only ever fed that log.
fn remember_request(home: &std::path::Path, hash: &str, request: &str, usage: bool) {
    if !usage { let _ = std::fs::remove_file(requests_file(home)); return; }
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

/// `GET /api/layer` — the live layer; `?preview=1&sig=` — the staged one, in the same shape, only while
/// it is exactly the change with that signature (see `staging::preview_manifest`).
async fn layer(State(st): S, Query(q): Query<HashMap<String, String>>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    let ar = arabic(&st).await;
    let preview = q.get("preview").map_or(false, |v| v == "1" || v == "true");
    // Only what the user kept is ever loaded: anything else found in the live layer is taken out
    // first and offered for review like any other change.
    if staging::guard(Git(Some(&st)), &d, ar).await { st.emit(json!({"type": "layer.pending"})); }
    if preview {
        // `sig`: the signature of the change the user chose to try; nothing else is handed out.
        let sig = q.get("sig").map_or("", |s| s.as_str());
        return Ok(Json(staging::preview_manifest(Git(Some(&st)), &d, ar, sig).await?));
    }
    Ok(Json(applayer::manifest_lang(&d.layer, ar)))
}

/// `GET /api/layer/history?limit=&offset=` — `limit` entries (100 unless given, 500 at most) after
/// skipping the `offset` newest.
async fn history(State(st): S, Query(q): Query<HashMap<String, String>>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let number = |k: &str, default: usize| q.get(k).and_then(|v| v.parse::<usize>().ok()).unwrap_or(default);
    Ok(Json(json!(applayer::history_page(Git(Some(&st)), &Dirs::real().layer, number("offset", 0), number("limit", 100)).await)))
}

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

/// What `POST /api/layer/undo-last` would undo right now, without undoing anything:
/// `{exists:false}`, or `{exists:true, hash, summary}`. Files changed outside the app do not count:
/// they are never in use (`staging::set_aside` takes them out before anything is loaded or undone).
pub async fn undo_preview_in(g: Git<'_>, dir: &std::path::Path) -> Value {
    match applayer::last_undoable_in(g, dir).await {
        Some((hash, summary)) => json!({"exists": true, "hash": hash, "summary": summary}),
        None => json!({"exists": false}),
    }
}

/// `GET /api/layer/undo-last` — see `undo_preview_in`. Lets a confirmation name the change before
/// anything moves (the native Recovery menu asks with it).
async fn undo_preview(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    Ok(Json(undo_preview_in(Git(Some(&st)), &Dirs::real().layer).await))
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
    /// The chat's name, when the request is not words of the user's own (a Home suggestion).
    title: String,
}

impl Customize {
    fn request(self) -> String { if self.prompt.trim().is_empty() { self.text } else { self.prompt } }
}

async fn customize(State(st): S, Json(c): Json<Customize>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let title = Some(c.title.trim().to_string()).filter(|t| !t.is_empty());
    let request = c.request();
    let id = crate::api::start_customize(st.clone(), request.clone(), "customize", title).await?;
    // The request's words are kept only for the local usage log; with that log off they are not kept at all.
    if st.settings.read().await.usage_signals { staging::set_request(&Dirs::real(), &request); }
    Ok(Json(json!({"chatId": id})))
}

async fn pending(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    let (d, ar) = (Dirs::real(), arabic(&st).await);
    staging::guard(Git(Some(&st)), &d, ar).await;
    Ok(Json(staging::pending(Git(Some(&st)), &d, ar).await))
}

/// Body: `{signature}` — the user read that the assistant was interrupted and wants to keep the
/// change with that signature anyway.
async fn pending_accept(State(st): S, body: axum::body::Bytes) -> R {
    let _g = LAYER_LOCK.lock().await;
    let body = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    staging::accept_incomplete(Git(Some(&st)), &Dirs::real(), arabic(&st).await, body["signature"].as_str().unwrap_or_default()).await?;
    ok()
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
    // A kept pack's entry remembers what the pack said it was and the fingerprint calculated here.
    if r["origin"] == "import" { mark_kept_pack(Git(Some(&st)), &d, r["hash"].as_str().unwrap_or_default(), &before).await; }
    let approved = workflows::approve_paths(&d.layer, &d.home, &workflows_to_approve(&before, approve))?;
    if !approved.is_empty() { crate::log::info(format!("workflows approved with a kept change: {}", approved.join(", "))); }
    workflows::bind_to_open(&st, &approved).await;
    r["approvedWorkflows"] = json!(approved);
    if r["origin"] == "customize" {
        let request = r["request"].as_str().unwrap_or_default().to_string();
        remember_request(&d.home, r["hash"].as_str().unwrap_or_default(), &request, st.settings.read().await.usage_signals);
        record_outcome(&st, "keep", &request).await;
    }
    // A merge of an app update settles its question only now that the merged file is really kept.
    if r["origin"] == "update" {
        if let Some(path) = r["resolves"].as_str().filter(|p| !p.is_empty()) { let _ = updates::clear_conflict(&d.home, path); }
    }
    // Anything set aside while this change was waiting takes its turn now.
    staging::adopt_held(Git(Some(&st)), &d, ar).await;
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
    staging::adopt_held(Git(Some(&st)), &d, arabic(&st).await).await;
    st.emit(json!({"type": "layer.pending"}));
    ok()
}

/// `POST /api/layer/pending/continue` — "Continue chatting": the user tried the open version and
/// wants changes. It stays as it is for the chat to build on (see `staging::cycle`); the chat gets a
/// line saying so. → `{ok, chatId}`
async fn pending_continue(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    let id = staging::load_meta(&d).and_then(|m| m.chat_id).unwrap_or_default();
    // Only a customise chat that is still there can take the conversation up again.
    if st.chats.get(&id).await.map_or(true, |c| c.kind != "customize") { return Err(crate::tr::err("layer.no_chat", &[]).into()); }
    let chat = staging::cycle::continue_chat(Git(Some(&st)), &d).await?;
    let last_is_note = st.chats.with(&chat, |c| c.messages.last().map_or(false, |m| m.role == "system" && m.tool.as_ref().map_or(false, |t| t["continued"] == true))).await.unwrap_or(false);
    if !last_is_note {
        let mut note = crate::chats::Msg::new("system", &crate::tr::text("layer.continued_note", &[]));
        note.tool = Some(json!({"continued": true}));
        crate::claude::push(&st, &chat, note).await;
    }
    st.emit(json!({"type": "layer.pending"}));
    Ok(Json(json!({"ok": true, "chatId": chat})))
}

/// `POST /api/layer/pending/review-again` — a version set aside for more conversation goes back to
/// the review. → the review.
async fn pending_review_again(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    let (d, ar) = (Dirs::real(), arabic(&st).await);
    staging::cycle::review_again(Git(Some(&st)), &d).await?;
    st.emit(json!({"type": "layer.pending"}));
    Ok(Json(staging::pending(Git(Some(&st)), &d, ar).await))
}

/// `GET /api/layer/pending/details` — what each file of the open change changes, line by line
/// (see `staging::details`).
async fn pending_details(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    Ok(Json(staging::details(Git(Some(&st)), &Dirs::real()).await))
}

/// `GET /api/layer/drafts` — the other changes in progress, set aside (see `staging::drafts_list`).
async fn draft_list() -> R {
    let _g = LAYER_LOCK.lock().await;
    Ok(Json(staging::drafts_list(&Dirs::real())))
}

#[derive(Deserialize)]
struct DraftRef { id: String }

/// `POST /api/layer/drafts/open {id}` — open that change for review; the one that was open is set
/// aside in its place. → the review of the change now open.
async fn draft_open(State(st): S, Json(r): Json<DraftRef>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let (d, ar) = (Dirs::real(), arabic(&st).await);
    staging::open_draft(Git(Some(&st)), &d, &r.id).await?;
    st.emit(json!({"type": "layer.pending"}));
    Ok(Json(staging::pending(Git(Some(&st)), &d, ar).await))
}

/// `POST /api/layer/drafts/discard {id}` — throw away a change that is set aside.
async fn draft_discard(State(st): S, Json(r): Json<DraftRef>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let x = staging::drop_draft(Git(Some(&st)), &Dirs::real(), &r.id).await?;
    if x.meta.origin.is_empty() || x.meta.origin == "customize" { record_outcome(&st, "discard", &x.meta.request).await; }
    st.emit(json!({"type": "layer.pending"}));
    Ok(Json(json!({"ok": true, "origin": x.meta.origin})))
}


/// Record the identity of the pack whose pending change (`pending`, read before it was kept) became entry `hash`.
async fn mark_kept_pack(g: Git<'_>, d: &Dirs, hash: &str, pending: &Value) {
    let p = &pending["pack"];
    let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
    if hash.is_empty() || !p.is_object() { return; }
    applayer::mark_pack(g, &d.layer, hash, applayer::PackMark { name: s("name"), author: s("author"), version: s("version"), fingerprint: s("fingerprint") }).await;
}

/// `GET /api/layer/export?name=&author=&version=&description=&paths=a,b` — the whole layer, or with
/// `paths` (files, or folders ending in `/`) a feature pack of just those. The pack carries what
/// the sharer says it is, and its fingerprint.
async fn export(Query(q): Query<HashMap<String, String>>) -> R {
    let get = |k: &str| q.get(k).map_or("", String::as_str);
    let paths: Option<Vec<String>> = q.get("paths").filter(|p| !p.trim().is_empty()).map(|p| p.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect());
    let who = packs::Identity { name: get("name"), author: get("author"), version: get("version"), description: get("description") };
    Ok(Json(packs::export_as(&Dirs::real().layer, &who, paths.as_deref())?))
}

#[derive(Deserialize)]
struct Inspect { pack: Value }

/// `POST /api/layer/pack/inspect {pack}` — check a pack before anything is staged and say what it
/// is (see `packs::inspect`). Nothing is written.
async fn pack_inspect(State(st): S, Json(i): Json<Inspect>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    let kept = applayer::kept_packs(Git(Some(&st)), &d.layer).await;
    Ok(Json(packs::inspect(&i.pack, &d.layer, &kept)?))
}

// ---------- ready-made packs ----------

/// `GET /api/layer/gallery` — the packs that come with the app, in the UI language, each with
/// whether it is already part of the app exactly as it comes.
async fn gallery_list(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    let ar = arabic(&st).await;
    let layer = Dirs::real().layer;
    Ok(Json(json!(crate::gallery::list(ar, &layer))))
}

#[derive(Deserialize)]
struct GalleryPick { id: String }

/// `POST /api/layer/gallery/stage {id}` — stage a ready-made pack for the usual review (never kept
/// here). → the pending change, like an import.
async fn gallery_stage(State(st): S, Json(p): Json<GalleryPick>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    let ar = arabic(&st).await;
    let (pack, summary) = crate::gallery::pack(&p.id, ar).ok_or_else(|| crate::tr::err("gallery.gone", &[]))?;
    applayer::ensure_in(Git(Some(&st)), &d).await?;
    packs::import_with(Git(Some(&st)), &d, &pack, ar, Some(summary)).await?;
    st.emit(json!({"type": "layer.pending"}));
    Ok(Json(staging::pending(Git(Some(&st)), &d, ar).await))
}

// ---------- one add-on at a time: switch off, remove, take a permission away ----------

/// `GET /api/layer/addons` — `{items: [...], leftover: [...]}`: every add-on and page of the app,
/// on or off, with what it may do and its saved data (`applayer::addons_of` + `data`), and the
/// saved data of add-ons that are no longer in the app.
async fn addon_list(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    let (d, ar) = (Dirs::real(), arabic(&st).await);
    if staging::guard(Git(Some(&st)), &d, ar).await { st.emit(json!({"type": "layer.pending"})); }
    Ok(Json(addons_with_data(&d, ar)))
}

pub fn addons_with_data(d: &Dirs, ar: bool) -> Value {
    let mut items = applayer::addons_of(&d.layer, ar);
    let mut known = std::collections::BTreeSet::new();
    for it in &mut items {
        let name = it["name"].as_str().unwrap_or_default().to_string();
        it["data"] = data_info(&d.home, &name).unwrap_or(Value::Null);
        known.insert(name);
    }
    let leftover: Vec<Value> = stored_names(&d.home).into_iter().filter(|n| !known.contains(n))
        .filter_map(|n| data_info(&d.home, &n).map(|info| json!({"name": n, "label": applayer::item_label(&n), "data": info}))).collect();
    json!({"items": items, "leftover": leftover})
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AddonRef { path: String, on: bool, permission: String }

/// After a change to the live layer made here: the app reloads it.
fn changed(st: &AppState, hash: &str) -> R {
    if !hash.is_empty() { st.emit(json!({"type": "layer.changed"})); }
    Ok(Json(json!({"ok": true, "hash": hash})))
}

/// `POST /api/layer/addons/switch {path, on}` → `{ok, hash}` (the history entry; empty when nothing changed).
async fn addon_switch(State(st): S, Json(a): Json<AddonRef>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let hash = applayer::switch_in(Git(Some(&st)), &Dirs::real().layer, &a.path, a.on).await?;
    crate::log::info(format!("layer: {} switched {}", a.path, if a.on { "on" } else { "off" }));
    changed(&st, &hash)
}

/// `POST /api/layer/addons/remove {path}` → `{ok, hash}`. Its saved data stays.
async fn addon_remove(State(st): S, Json(a): Json<AddonRef>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let hash = applayer::remove_in(Git(Some(&st)), &Dirs::real().layer, &a.path).await?;
    crate::log::info(format!("layer: {} removed", a.path));
    changed(&st, &hash)
}

/// `POST /api/layer/addons/permission {path, permission}` → `{ok, hash}`.
async fn addon_permission(State(st): S, Json(a): Json<AddonRef>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let hash = applayer::withdraw_in(Git(Some(&st)), &Dirs::real().layer, &a.path, &a.permission).await?;
    crate::log::info(format!("layer: {} no longer has {}", a.path, a.permission));
    changed(&st, &hash)
}

/// `PUT /api/layer/nav` with the whole `nav.json` the user arranged in "Edit sidebar" → `{ok, hash}`.
/// Checked exactly as a reviewed change is; `{}` removes the file (the usual sidebar). The user made
/// it here by hand, so it is kept at once, as one history entry they can undo.
async fn nav_edit(State(st): S, Json(nav): Json<Value>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let ar = arabic(&st).await;
    let hash = applayer::nav_in(Git(Some(&st)), &Dirs::real().layer, &nav, ar).await?;
    crate::log::info("layer: the sidebar was edited by hand");
    changed(&st, &hash)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct BuiltinRef { page: String, on: bool }

/// `POST /api/layer/pages/builtin {page, on}` → `{ok, hash}`: `on` keeps the app's own page although
/// a customisation replaces it; `false` uses the replacement again.
async fn page_builtin(State(st): S, Json(b): Json<BuiltinRef>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let hash = applayer::builtin_in(Git(Some(&st)), &Dirs::real().layer, &b.page, b.on).await?;
    changed(&st, &hash)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DataRef { name: String, version: String }

/// `POST /api/layer/addons/data/restore {name, version}` — put an earlier copy back. What was
/// saved until now is kept as a copy first, so this can be reversed the same way.
async fn addon_data_restore(Json(r): Json<DataRef>) -> R {
    storage_restore(&crate::paths::home(), &r.name, &r.version)?;
    ok()
}

/// `POST /api/layer/addons/data/clear {name}` — remove what an add-on saved (a copy is kept with the earlier ones).
async fn addon_data_clear(Json(r): Json<DataRef>) -> R {
    storage_clear(&crate::paths::home(), &r.name)?;
    ok()
}

/// `POST /api/layer/addons/data/forget {name}` — delete, for good, what an add-on that is no longer
/// in the app saved, with its earlier copies. Refused while the add-on is in the app.
async fn addon_data_forget(Json(r): Json<DataRef>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    if applayer::addons_of(&d.layer, false).iter().any(|a| a["name"] == r.name.as_str()) { return Err(crate::tr::err("addon.data_in_use", &[]).into()); }
    storage_forget(&d.home, &r.name)?;
    crate::log::info(format!("layer: saved data of the removed add-on {} deleted", r.name));
    ok()
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

/// Replace plugin `plugin`'s document. Private folder, private file, written atomically. What was
/// saved before is kept as an earlier copy when the newest copy is more than an hour old (see
/// `keep_version`), so one bad save never loses an add-on's records.
pub fn storage_write(home: &std::path::Path, plugin: &str, bytes: &[u8], arabic: bool) -> anyhow::Result<()> {
    let file = storage_file(home, plugin)?;
    if bytes.len() > STORAGE_MAX_BYTES {
        anyhow::bail!("{}", if arabic { "البيانات المحفوظة لهذه الإضافة كبيرة جداً (الحد الأقصى ٢٥٦ كيلوبايت)." } else { "This add-on is trying to store too much (the limit is 256 KB)." });
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("storage must be a JSON object"))?;
    if !v.is_object() { anyhow::bail!("storage must be a JSON object"); }
    let _g = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = home.join("plugin-data");
    crate::paths::create_private_dir(&dir)?;
    // Never write through a link someone planted in the data folder.
    if std::fs::symlink_metadata(&file).map_or(false, |m| !m.is_file()) { anyhow::bail!("storage is not available for this plugin"); }
    keep_version(home, plugin, false, chrono::Local::now())?;
    crate::paths::write_atomic(&file, serde_json::to_string(&v)?.as_bytes(), true)?;
    Ok(())
}

/// Replace what `plugin` saved with `v` from elsewhere (a move file), keeping the current document
/// as an earlier copy first.
pub fn storage_replace(home: &std::path::Path, plugin: &str, v: &Value) -> anyhow::Result<()> {
    let file = storage_file(home, plugin)?;
    let text = serde_json::to_string(v)?;
    if !v.is_object() || text.len() > STORAGE_MAX_BYTES { anyhow::bail!("storage must be a JSON object of at most 256 KB"); }
    let _g = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    crate::paths::create_private_dir(&home.join("plugin-data"))?;
    if std::fs::symlink_metadata(&file).map_or(false, |m| !m.is_file()) { anyhow::bail!("storage is not available for this plugin"); }
    keep_version(home, plugin, true, chrono::Local::now())?;
    crate::paths::write_atomic(&file, text.as_bytes(), true)?;
    Ok(())
}

/// One saved document is written at a time (keeping a copy and writing belong together).
static STORAGE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// How many earlier copies of each add-on's saved data are kept at most: the five newest, and
/// before them one a day.
pub const VERSIONS_KEPT: usize = 12;
const VERSIONS_RECENT: usize = 5;

/// Earlier copies live beside the documents, in a folder no add-on name can reach (names never start with a dot).
fn versions_dir(home: &std::path::Path, plugin: &str) -> std::path::PathBuf { home.join("plugin-data").join(".versions").join(plugin) }

/// A copy's id is when it was taken, local time: `YYYYMMDD-HHMMSS-mmm`.
fn valid_version(id: &str) -> bool {
    id.len() == 19 && id.bytes().enumerate().all(|(i, b)| if i == 8 || i == 15 { b == b'-' } else { b.is_ascii_digit() })
}

/// The kept copies of `plugin`'s data, newest first: `(id, path)`.
fn versions(home: &std::path::Path, plugin: &str) -> Vec<(String, std::path::PathBuf)> {
    let mut v: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(versions_dir(home, plugin)).into_iter().flatten().flatten()
        .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
        .filter_map(|e| { let n = e.file_name().to_string_lossy().to_string(); n.strip_suffix(".json").filter(|id| valid_version(id)).map(|id| (id.to_string(), e.path())) })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v
}

/// Before `plugin`'s document is replaced: keep a copy of it, unless the newest copy is less than
/// an hour old (`force`: always, for a restore, a clear or data from elsewhere) or holds the same.
/// Then thin the copies out: the newest `VERSIONS_RECENT`, then the newest of each earlier day, at
/// most `VERSIONS_KEPT` in all.
fn keep_version(home: &std::path::Path, plugin: &str, force: bool, now: chrono::DateTime<chrono::Local>) -> anyhow::Result<()> {
    let file = storage_file(home, plugin)?;
    if !std::fs::symlink_metadata(&file).map_or(false, |m| m.is_file()) { return Ok(()); }
    let current = std::fs::read(&file)?;
    let list = versions(home, plugin);
    let stamp = |t: chrono::DateTime<chrono::Local>| t.format("%Y%m%d-%H%M%S-%3f").to_string();
    let hour_ago = stamp(now - chrono::Duration::hours(1));
    if let Some((newest, path)) = list.first() {
        if std::fs::read(path).ok().as_deref() == Some(current.as_slice()) { return Ok(()); }
        if !force && *newest > hour_ago { return Ok(()); }
    }
    let dir = versions_dir(home, plugin);
    crate::paths::create_private_dir(&dir)?;
    // Two copies taken within the same millisecond each keep their own name.
    let mut t = now;
    while dir.join(format!("{}.json", stamp(t))).exists() { t += chrono::Duration::milliseconds(1); }
    let id = stamp(t);
    crate::paths::write_atomic(&dir.join(format!("{id}.json")), &current, true)?;
    // Newest first: the first copy met of a day is that day's newest.
    let mut days = std::collections::BTreeSet::new();
    let mut kept = 0usize;
    for (i, (vid, path)) in versions(home, plugin).into_iter().enumerate() {
        let new_day = days.insert(vid[..8].to_string());
        if (i < VERSIONS_RECENT || new_day) && kept < VERSIONS_KEPT { kept += 1; } else { let _ = std::fs::remove_file(path); }
    }
    Ok(())
}

/// What `plugin` saved and its earlier copies: `{bytes, saved, versions: [{id, saved, bytes}]}`
/// (`bytes`/`saved` null when nothing is saved now), or `None` when there is neither.
pub fn data_info(home: &std::path::Path, plugin: &str) -> Option<Value> {
    let file = storage_file(home, plugin).ok()?;
    let when = |m: &std::fs::Metadata| m.modified().ok().map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    let now = std::fs::symlink_metadata(&file).ok().filter(|m| m.is_file());
    let list: Vec<Value> = versions(home, plugin).into_iter().filter_map(|(id, path)| {
        let m = std::fs::symlink_metadata(&path).ok()?;
        Some(json!({"id": id, "saved": when(&m), "bytes": m.len()}))
    }).collect();
    if now.is_none() && list.is_empty() { return None; }
    Some(json!({"bytes": now.as_ref().map(|m| m.len()), "saved": now.as_ref().and_then(when), "versions": list}))
}

/// The names of add-ons that have saved data (or earlier copies of it).
pub fn stored_names(home: &std::path::Path) -> Vec<String> {
    let mut out: std::collections::BTreeSet<String> = std::fs::read_dir(home.join("plugin-data")).into_iter().flatten().flatten()
        .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".json").map(String::from)).filter(|n| valid_plugin_name(n)).collect();
    out.extend(std::fs::read_dir(home.join("plugin-data").join(".versions")).into_iter().flatten().flatten()
        .filter(|e| e.file_type().map_or(false, |t| t.is_dir())).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| valid_plugin_name(n)));
    out.into_iter().collect()
}

/// Put the copy `version` of `plugin`'s data back, keeping what is saved now as a copy first.
pub fn storage_restore(home: &std::path::Path, plugin: &str, version: &str) -> anyhow::Result<()> {
    let file = storage_file(home, plugin)?;
    if !valid_version(version) { return Err(crate::tr::err("addon.version_gone", &[])); }
    let _g = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let src = versions_dir(home, plugin).join(format!("{version}.json"));
    let bytes = std::fs::symlink_metadata(&src).ok().filter(|m| m.is_file() && m.len() as usize <= STORAGE_MAX_BYTES)
        .and_then(|_| std::fs::read(&src).ok()).filter(|b| serde_json::from_slice::<Value>(b).map_or(false, |v| v.is_object()))
        .ok_or_else(|| crate::tr::err("addon.version_gone", &[]))?;
    crate::paths::create_private_dir(&home.join("plugin-data"))?;
    if std::fs::symlink_metadata(&file).map_or(false, |m| !m.is_file()) { anyhow::bail!("storage is not available for this plugin"); }
    keep_version(home, plugin, true, chrono::Local::now())?;
    crate::paths::write_atomic(&file, &bytes, true)?;
    Ok(())
}

/// Remove what `plugin` saved now (a copy is kept with the earlier ones, so it can be put back).
pub fn storage_clear(home: &std::path::Path, plugin: &str) -> anyhow::Result<()> {
    let file = storage_file(home, plugin)?;
    let _g = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !std::fs::symlink_metadata(&file).map_or(false, |m| m.is_file()) { return Ok(()); }
    keep_version(home, plugin, true, chrono::Local::now())?;
    std::fs::remove_file(&file)?;
    Ok(())
}

/// Delete what `plugin` saved and every earlier copy, for good (offered for add-ons no longer in the app).
pub fn storage_forget(home: &std::path::Path, plugin: &str) -> anyhow::Result<()> {
    let file = storage_file(home, plugin)?;
    let _g = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if std::fs::symlink_metadata(&file).is_ok() { std::fs::remove_file(&file)?; }
    let dir = versions_dir(home, plugin);
    if std::fs::symlink_metadata(&dir).map_or(false, |m| m.is_dir()) { std::fs::remove_dir_all(&dir)?; }
    else if std::fs::symlink_metadata(&dir).is_ok() { std::fs::remove_file(&dir)?; }
    Ok(())
}

/// Every add-on's saved document (valid JSON objects within the limit), for a move file.
pub fn storage_all(home: &std::path::Path) -> std::collections::BTreeMap<String, Value> {
    stored_names(home).into_iter().filter(|n| storage_file(home, n).map_or(false, |f| std::fs::symlink_metadata(f).map_or(false, |m| m.is_file())))
        .filter_map(|n| storage_read(home, &n).ok().filter(|v| v.as_object().map_or(false, |o| !o.is_empty())).map(|v| (n, v))).collect()
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
    // Codex cannot be held to a spending limit at all.
    let codex = crate::assistant::primary(&*st.settings.read().await) == crate::assistant::Program::Codex;
    let budget = !codex && crate::claude::has_flag(&st, "--max-budget-usd").await;
    workflows::add_libraries(&st, &mut list).await;
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
    workflows::bind_to_open(&st, &[l.file.clone()]).await;
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

/// The update status, with `merging: true` on a conflict whose assistant merge is being prepared or
/// is waiting for review. It stays a conflict until that merge is kept; if the merge fails, is
/// discarded or is replaced, the question is simply open again.
async fn update_status(State(st): S) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    let p = staging::pending(Git(Some(&st)), &d, arabic(&st).await).await;
    // (While the assistant is still writing there may be nothing staged yet.)
    let mut merging: Vec<String> = staging::load_meta(&d).filter(|m| m.origin == "update" && (p["exists"] == true || p["running"] == true)).map(|m| m.resolves).into_iter().collect();
    // A merge set aside for another change is still the answer to its question.
    merging.extend(staging::merges_set_aside(&d));
    merging.retain(|m| !m.is_empty());
    Ok(Json(updates::explained(updates::status_merging(&d.home, &merging), &d.home, applayer::SHIPPED)))
}

#[derive(Deserialize)]
struct Resolve { path: String, choice: String }

async fn update_resolve(State(st): S, Json(r): Json<Resolve>) -> R {
    let _g = LAYER_LOCK.lock().await;
    let d = Dirs::real();
    if r.choice == "merge" {
        let prompt = updates::merge_prompt(&d, applayer::SHIPPED, &r.path)?;
        let title = if arabic(&st).await { format!("دمج تحديث {}", r.path) } else { format!("Merge update of {}", r.path) };
        let id = crate::api::start_customize(st.clone(), prompt, "update", Some(title)).await?;
        // The conflict stays on record until the merged file is kept (see `pending_apply`).
        staging::set_resolves(&d, &r.path);
        return Ok(Json(json!({"ok": true, "chatId": id, "status": updates::explained(updates::status_merging(&d.home, &[r.path.clone()]), &d.home, applayer::SHIPPED)})));
    }
    updates::resolve(Git(Some(&st)), &d, applayer::SHIPPED, &r.path, &r.choice).await?;
    if r.choice == "new" { st.emit(json!({"type": "layer.changed"})); }
    Ok(Json(json!({"ok": true, "status": updates::explained(updates::status(&d.home), &d.home, applayer::SHIPPED)})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_undo_preview_names_exactly_what_an_undo_would_take_back() {
        let (g, d) = applayer::testutil::layer("undo-preview").await;
        // A fresh layer has only its starting point: nothing to undo.
        assert_eq!(undo_preview_in(g, &d.layer).await, json!({"exists": false}));
        std::fs::write(d.layer.join("custom.css"), "body { color: red }\n").unwrap();
        g.commit_all(&d.layer, "Make text red").await.unwrap();
        std::fs::write(d.layer.join("plugins/a.js"), "export default () => {}\n").unwrap();
        g.commit_all(&d.layer, "Add a plugin").await.unwrap();
        let before = applayer::history_in(g, &d.layer).await;
        let p = undo_preview_in(g, &d.layer).await;
        assert_eq!((p["exists"].clone(), p["summary"].clone()), (json!(true), json!("Add a plugin")));
        assert_eq!(p["hash"], before[0]["hash"]);
        // Looking changes nothing.
        assert_eq!(applayer::history_in(g, &d.layer).await, before);
        assert!(d.layer.join("plugins/a.js").exists());
        // The preview and the undo agree, one change after the other.
        let (hash, summary) = applayer::undo_last_in(g, &d.layer, false).await.unwrap();
        assert_eq!((json!(hash), json!(summary)), (p["hash"].clone(), p["summary"].clone()));
        assert_eq!(undo_preview_in(g, &d.layer).await["summary"], "Make text red", "the undo itself is not offered for undoing");
        // A file changed outside the app is never in use, so it does not change what an undo
        // names, and the undo takes back exactly that change.
        std::fs::write(d.layer.join("custom.css"), "body { display: none }\n").unwrap();
        let p = undo_preview_in(g, &d.layer).await;
        assert_eq!(p["summary"], "Make text red");
        let (hash, summary) = applayer::undo_last_in(g, &d.layer, false).await.unwrap();
        assert_eq!((json!(hash), json!(summary)), (p["hash"].clone(), p["summary"].clone()));
        assert_ne!(std::fs::read_to_string(d.layer.join("custom.css")).unwrap(), "body { display: none }\n", "the outside edit is not in use");
        assert_eq!(undo_preview_in(g, &d.layer).await, json!({"exists": false}));
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    /// The native Recovery menu's "Put it back" undoes the entry its action just added (an undo, or
    /// a "went back" entry). Afterwards the app is as it was, and the history says so: the change is
    /// in effect again and is what the next "Undo last app change" names.
    #[tokio::test]
    async fn putting_back_undoes_exactly_the_entry_just_added() {
        let (g, d) = applayer::testutil::layer("put-back").await;
        std::fs::write(d.layer.join("custom.css"), "body { color: red }\n").unwrap();
        g.commit_all(&d.layer, "Make text red").await.unwrap();
        std::fs::write(d.layer.join("plugins/a.js"), "export default () => {}\n").unwrap();
        g.commit_all(&d.layer, "Add a plugin").await.unwrap();
        let newest = |h: &[Value]| h[0]["hash"].as_str().unwrap().to_string();
        // Undo, then put it back.
        let named = undo_preview_in(g, &d.layer).await;
        applayer::revert_in(g, &d.layer, named["hash"].as_str().unwrap()).await.unwrap();
        assert!(!d.layer.join("plugins/a.js").exists());
        let after = applayer::history_in(g, &d.layer).await;
        applayer::revert_in(g, &d.layer, &newest(&after)).await.unwrap();
        assert!(d.layer.join("plugins/a.js").exists(), "the change is back");
        let p = undo_preview_in(g, &d.layer).await;
        assert_eq!((p["summary"].clone(), p["hash"].clone()), (named["summary"].clone(), named["hash"].clone()), "and it is the one an undo names again");
        // Go back to the starting point, then put it back.
        let start = applayer::history_in(g, &d.layer).await.last().unwrap()["hash"].as_str().unwrap().to_string();
        applayer::rollback_in(g, &d.layer, &start).await.unwrap();
        assert!(!d.layer.join("plugins/a.js").exists());
        assert_ne!(std::fs::read_to_string(d.layer.join("custom.css")).unwrap_or_default(), "body { color: red }\n");
        let after = applayer::history_in(g, &d.layer).await;
        applayer::revert_in(g, &d.layer, &newest(&after)).await.unwrap();
        assert_eq!(std::fs::read_to_string(d.layer.join("custom.css")).unwrap(), "body { color: red }\n");
        assert!(d.layer.join("plugins/a.js").exists());
        assert_eq!(undo_preview_in(g, &d.layer).await["summary"], "Add a plugin");
        std::fs::remove_dir_all(&d.home).unwrap();
    }

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

    /// FEAT-23: one bad save never loses an add-on's records: earlier copies are kept (the newest
    /// few, then one a day), can be put back, and clearing keeps a copy too.
    #[test]
    fn saved_data_keeps_earlier_copies_that_can_be_put_back() {
        use chrono::TimeZone;
        let home = std::env::temp_dir().join(format!("thoughts-versions-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        let at = |day: u32, h: u32, m: u32| chrono::Local.with_ymd_and_hms(2026, 10, day, h, m, 0).unwrap();
        let file = storage_file(&home, "hours.js").unwrap();
        let put = |n: u32, when| { keep_version(&home, "hours.js", false, when).unwrap(); crate::paths::write_atomic(&file, format!(r#"{{"n":{n}}}"#).as_bytes(), true).unwrap(); };
        storage_write(&home, "hours.js", br#"{"n":0}"#, false).unwrap();
        assert!(versions(&home, "hours.js").is_empty(), "nothing before the first save");
        put(1, at(1, 9, 0));
        put(2, at(1, 9, 20));
        assert_eq!(versions(&home, "hours.js").len(), 1, "at most one copy an hour");
        put(3, at(1, 11, 0));
        keep_version(&home, "hours.js", false, at(1, 13, 0)).unwrap();
        keep_version(&home, "hours.js", false, at(1, 15, 0)).unwrap();
        assert_eq!(versions(&home, "hours.js").len(), 3, "the same content is not copied twice");
        for day in 2..=25 { put(day + 10, at(day, 9, 0)); }
        let kept = versions(&home, "hours.js");
        assert_eq!(kept.len(), VERSIONS_KEPT);
        let days: std::collections::BTreeSet<&str> = kept.iter().map(|(id, _)| &id[..8]).collect();
        assert_eq!(days.len(), VERSIONS_KEPT, "the newest few, then one a day");
        assert!(kept[0].0.starts_with("20261025"));
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(versions_dir(&home, "hours.js")).unwrap().permissions().mode() & 0o777, 0o700); }
        // Putting one back keeps what was there as a copy first.
        let (oldest, _) = kept.last().unwrap().clone();
        let before = std::fs::read_to_string(&file).unwrap();
        storage_restore(&home, "hours.js", &oldest).unwrap();
        assert_ne!(std::fs::read_to_string(&file).unwrap(), before);
        assert_eq!(std::fs::read_to_string(&versions(&home, "hours.js")[0].1).unwrap(), before);
        for bad in ["../x", "2026", "20261001-090000-00a", "", "20261001-090000-000/../../x"] { assert!(storage_restore(&home, "hours.js", bad).is_err(), "{bad}"); }
        assert!(storage_restore(&home, "../settings", &oldest).is_err());
        // Clearing removes what is saved now, keeping a copy; the data is still listed (for a removed add-on too).
        storage_clear(&home, "hours.js").unwrap();
        assert!(!file.exists());
        let info = data_info(&home, "hours.js").unwrap();
        assert!(info["bytes"].is_null() && info["versions"].as_array().unwrap().len() == VERSIONS_KEPT);
        assert_eq!(stored_names(&home), ["hours.js"]);
        assert!(data_info(&home, "never.js").is_none());
        assert!(storage_all(&home).is_empty(), "nothing saved now: nothing to move");
        // Deleted for good: the data and every copy, nothing else.
        storage_write(&home, "other.js", br#"{"keep":true}"#, false).unwrap();
        storage_forget(&home, "hours.js").unwrap();
        assert!(data_info(&home, "hours.js").is_none() && !versions_dir(&home, "hours.js").exists());
        assert_eq!(storage_read(&home, "other.js").unwrap()["keep"], true);
        assert!(storage_forget(&home, "..").is_err() && storage_forget(&home, "").is_err());
        assert!(home.join("plugin-data/.versions").is_dir());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn remembers_what_was_asked_for_each_kept_change() {
        let home = std::env::temp_dir().join(format!("thoughts-requests-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(recall_request(&home, "abc123"), None);
        remember_request(&home, "abc123def456", "Show overdue todos on Home", true);
        remember_request(&home, "ffff0000aaaa", "Group todos by date", true);
        remember_request(&home, "", "ignored", true);
        assert_eq!(recall_request(&home, "abc123").as_deref(), Some("Show overdue todos on Home"), "a short hash finds it too");
        assert_eq!(recall_request(&home, "ffff0000aaaa").as_deref(), Some("Group todos by date"));
        assert_eq!(recall_request(&home, ""), None);
        // With the usage log switched off nothing is kept, and what was kept is removed.
        remember_request(&home, "1234abcd5678", "Private wording", false);
        assert!(!requests_file(&home).exists());
        assert_eq!(recall_request(&home, "abc123"), None);
        std::fs::remove_dir_all(home).unwrap();
    }
}
