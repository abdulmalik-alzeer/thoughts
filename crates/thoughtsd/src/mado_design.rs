//! MADO, the design step: style examples to pick from, the picked style's settings as the user
//! tunes them, and the screen mock-ups an assistant drew (contract: `docs/mado-spec.md`, sections
//! "Design" and "Mock-ups").
//!
//! Everything read here was, or may have been, written by a run, so nothing is trusted: ids follow
//! the contract's pattern, every path is resolved inside `<project>/mado/` (a link leading out is
//! refused), every file has a size limit checked before it is read, style candidates and settings
//! are validated field by field, and a mock-up is never handed out as written but cleaned from an
//! allowlist (`mado_design_clean.rs`). The folder trust check and the link checks yield to full
//! access like every other guard; validation and cleaning do not, because they are what makes the
//! files drawable at all.
//!
//! This module never writes `state.json`: it reads the core's state (`mado::load_state`, which
//! distrusts a state file the daemon did not write) to refuse direct changes while the step is
//! approved, and records the picked style through the core (`mado::set_style`). Paths are resolved
//! by the core's own `mado::resolve_in` / `read_in` / `write_in`.

use std::path::Path;
use std::sync::Arc;

use axum::{extract::{Path as UrlPath, State}, routing::{get, post}, Json, Router};
use serde_json::{json, Value};

use crate::state::AppState;

#[path = "mado_design_clean.rs"]
pub mod clean;
#[path = "mado_design_styles.rs"]
pub mod styles;

use clean::clean;
use crate::mado as core;
use styles::{app_styles, assistant_styles, Tokens};

pub use styles::{font_names, FONTS};

type S = State<Arc<AppState>>;
type R = Result<Json<Value>, crate::api::ApiErr>;

/// The contract's limit for one mock-up.
pub const MAX_MOCK: u64 = 400_000;
/// `design/styles.json` and `design/tokens.json` are small.
const MAX_STYLES: u64 = 256 * 1024;
const MAX_TOKENS: u64 = 16 * 1024;

const STYLES: &str = "design/styles.json";
const TOKENS: &str = "design/tokens.json";

/// One change to a project's design files at a time.
static WRITE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/mado/:project/design/styles", get(list_styles))
        .route("/api/mado/:project/design/style", post(pick))
        .route("/api/mado/:project/design/tokens", get(get_tokens).put(put_tokens))
        .route("/api/mado/:project/design/screens", get(screens))
        .route("/api/mado/:project/design/screens/:id", get(screen))
}

/// Graph, screen and style ids (the contract's pattern).
pub fn valid_id(id: &str) -> bool { crate::mado_validate::id_ok(id) }

fn full() -> bool { crate::full_access::on() }

/// `<project>/mado` of a linked, trusted project (trust yields to full access), which must exist.
async fn mado_dir(state: &AppState, project_id: &str) -> anyhow::Result<std::path::PathBuf> {
    let project = core::project_dir(state, project_id).await?;
    if !full() { crate::trust::require(&project)?; }
    let mado = core::mado_dir_of(&project, full())?;
    if !mado.is_dir() { return Err(crate::tr::err("mado.design.not_started", &[])); }
    Ok(mado)
}

/// A file of the plan folder as text, refused above `max` bytes. `Ok(None)`: no such file.
fn read_text(mado: &Path, rel: &str, max: u64) -> anyhow::Result<Option<String>> {
    Ok(core::read_in(mado, rel, full(), max as usize)?.map(|b| String::from_utf8_lossy(&b).into_owned()))
}

/// Is `rel` an ordinary file really inside the plan folder?
fn has_file(mado: &Path, rel: &str) -> bool {
    core::resolve_in(mado, rel, full()).map_or(false, |p| std::fs::symlink_metadata(&p).map_or(false, |m| m.is_file()))
}

/// The core's state of the project, as the core believes it (`None` before planning started).
fn core_state(project_id: &str, mado: &Path) -> Option<core::MadoState> {
    core::load_state(project_id, mado, full()).ok().flatten()
}

/// The flow's screens, in its order (none while the flow is missing or not valid).
fn flow_screens(mado: &Path) -> Vec<(String, String)> {
    core::read_graph(mado, "flow.graph.json", "flow", full()).ok().flatten().map(|g| core::screens_of(&g)).unwrap_or_default()
}

/// The app's examples first, then the assistant's valid candidates; how many were left out.
/// For a project that is already built (round 0.5.0) the assistant's `current` (the look the
/// product has now, read from its code) comes before everything else.
fn candidates(mado: &Path) -> (Vec<Value>, usize) {
    let mut all = app_styles();
    let dropped = match read_text(mado, STYLES, MAX_STYLES) {
        Ok(Some(text)) => { let (mine, dropped) = assistant_styles(&text); all.extend(mine); dropped }
        Ok(None) => 0,
        // Too large, or not a file inside the folder: nothing of it is shown.
        Err(_) => 1,
    };
    if core::existing_in(mado, full()) {
        if let Some(i) = all.iter().position(|s| s["id"] == CURRENT && s["source"] == "assistant") { let c = all.remove(i); all.insert(0, c); }
    }
    (all, dropped)
}

/// The id of the candidate that is a built project's own look (round 0.5.0).
const CURRENT: &str = "current";

/// The id of the picked style: the core's record (`state.json`, `style`). An id is only ever an id.
fn picked(project_id: &str, mado: &Path) -> Option<String> {
    core_state(project_id, mado)?.style.filter(|id| valid_id(id))
}

fn write_json(mado: &Path, rel: &str, v: &Value) -> anyhow::Result<()> {
    let mut text = serde_json::to_string_pretty(v)?;
    text.push('\n');
    core::write_in(mado, rel, text.as_bytes(), full())
}

/// Direct changes are closed while the step is approved: the user reopens it first, so that what
/// was approved is what gets built.
fn require_open(project_id: &str, mado: &Path) -> anyhow::Result<()> {
    let approved = core_state(project_id, mado).and_then(|s| s.steps.get("design").map(|e| e.status == "approved")).unwrap_or(false);
    if approved { return Err(crate::tr::err("mado.design.approved", &[])); }
    Ok(())
}

/// `GET /design/styles` → `{styles: StyleCandidate[], picked?: id, dropped: n, current?: "current"}`.
async fn list_styles(State(st): S, UrlPath(project): UrlPath<String>) -> R {
    let mado = mado_dir(&st, &project).await?;
    let (styles, dropped) = candidates(&mado);
    // Round 0.5.0: the candidate that is the product's look today, for the window to offer first.
    let current = core::existing_in(&mado, full()) && styles.first().map_or(false, |s| s["id"] == CURRENT);
    let mut out = json!({"styles": styles, "dropped": dropped});
    if let Some(id) = picked(&project, &mado) { out["picked"] = json!(id); }
    if current { out["current"] = json!(CURRENT); }
    Ok(Json(out))
}

/// `POST /design/style {id}`: pick a style; its settings become `design/tokens.json`.
async fn pick(State(st): S, UrlPath(project): UrlPath<String>, Json(body): Json<Value>) -> R {
    let id = body.get("id").and_then(Value::as_str).filter(|id| valid_id(id)).ok_or_else(|| crate::tr::err("mado.design.bad_id", &[]))?.to_string();
    let mado = mado_dir(&st, &project).await?;
    let _g = WRITE.lock().await;
    require_open(&project, &mado)?;
    let (all, _) = candidates(&mado);
    let style = all.into_iter().find(|s| s["id"] == id.as_str()).ok_or_else(|| crate::tr::err("mado.design.no_style", &[]))?;
    write_json(&mado, TOKENS, &style["tokens"])?;
    core::set_style(&st, &project, &id).await?;
    core::tell(&st, &project, "design").await;
    Ok(Json(json!({"picked": id, "tokens": style["tokens"]})))
}

/// `GET /design/tokens` → `DesignTokens`, or `null` while no style has been picked.
async fn get_tokens(State(st): S, UrlPath(project): UrlPath<String>) -> R {
    let mado = mado_dir(&st, &project).await?;
    let Some(text) = read_text(&mado, TOKENS, MAX_TOKENS)? else { return Ok(Json(Value::Null)) };
    let tokens = serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()).and_then(|v| Tokens::parse(&v))
        .map_err(|why| crate::tr::err_detail("mado.design.saved_bad", &[], why))?;
    Ok(Json(tokens.json()))
}

/// `PUT /design/tokens DesignTokens`: the user's own change, saved at once without a run.
async fn put_tokens(State(st): S, UrlPath(project): UrlPath<String>, Json(body): Json<Value>) -> R {
    let tokens = Tokens::parse(&body).map_err(|why| crate::tr::err_detail("mado.design.bad_tokens", &[], why))?;
    let mado = mado_dir(&st, &project).await?;
    let _g = WRITE.lock().await;
    require_open(&project, &mado)?;
    write_json(&mado, TOKENS, &tokens.json())?;
    core::tell(&st, &project, "design").await;
    Ok(Json(tokens.json()))
}

/// `GET /design/styles` as the tests ask it.
#[cfg(test)]
pub(crate) async fn tests_list_styles(st: &Arc<AppState>, project: &str) -> Value { list_styles(State(st.clone()), UrlPath(project.to_string())).await.unwrap().0 }

fn mock_path(id: &str) -> String { format!("design/screens/{id}.html") }

/// `GET /design/screens` → `[{id, label, hasMock}]`, the flow's screens in its order.
async fn screens(State(st): S, UrlPath(project): UrlPath<String>) -> R {
    let mado = mado_dir(&st, &project).await?;
    let list: Vec<Value> = flow_screens(&mado).into_iter().map(|(id, label)| {
        let has = has_file(&mado, &mock_path(&id));
        json!({"id": id, "label": label, "hasMock": has})
    }).collect();
    Ok(Json(json!(list)))
}

/// `GET /design/screens/:id` → `{html}`: the mock-up, cleaned. Never the file as written.
async fn screen(State(st): S, UrlPath((project, id)): UrlPath<(String, String)>) -> R {
    if !valid_id(&id) { return Err(crate::tr::err("mado.design.bad_id", &[]).into()); }
    let mado = mado_dir(&st, &project).await?;
    let raw = read_text(&mado, &mock_path(&id), MAX_MOCK)?.ok_or_else(|| crate::tr::err("mado.design.no_mock", &[]))?;
    // Parsing a large page takes a moment: not on the threads that answer everything else.
    // A page that passes a limit while being read is not shown at all.
    let html = tokio::task::spawn_blocking(move || clean(&raw)).await?.map_err(|why| crate::tr::err_detail("mado.design.too_complex", &[], format!("{why:?}")))?;
    Ok(Json(json!({"html": html})))
}

#[cfg(test)]
#[path = "mado_design_tests.rs"]
mod tests;
