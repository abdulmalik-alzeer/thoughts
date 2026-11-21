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
//! This module never writes `state.json`. It reads it to refuse direct changes while the step is
//! approved. Which style was picked is kept in `design/picked.json` (see `picked`).

use std::path::Path;
use std::sync::Arc;

use axum::{extract::{Path as UrlPath, State}, routing::{get, post}, Json, Router};
use serde_json::{json, Value};

use crate::state::AppState;

#[path = "mado_design_clean.rs"]
pub mod clean;
#[path = "mado_design_shim.rs"]
pub mod shim;
#[path = "mado_design_styles.rs"]
pub mod styles;

use clean::clean;
use shim::{self as core, valid_id};
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
const PICKED: &str = "design/picked.json";

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

/// The app's examples first, then the assistant's valid candidates; how many were left out.
fn candidates(mado: &Path) -> (Vec<Value>, usize) {
    let mut all = app_styles();
    let dropped = match core::read_text(mado, STYLES, MAX_STYLES) {
        Ok(Some(text)) => { let (mine, dropped) = assistant_styles(&text); all.extend(mine); dropped }
        Ok(None) => 0,
        // Too large, or not a file inside the folder: nothing of it is shown.
        Err(_) => 1,
    };
    (all, dropped)
}

/// The id of the picked style: the core's record (`state.json`, `style`) when it keeps one,
/// otherwise the design step's own note beside the settings. An id is only ever an id.
fn picked(mado: &Path) -> Option<String> {
    let from_state = core::state_json(mado).get("style").and_then(Value::as_str).map(str::to_string);
    from_state.or_else(|| {
        let text = core::read_text(mado, PICKED, 1024).ok().flatten()?;
        serde_json::from_str::<Value>(&text).ok()?.get("id")?.as_str().map(str::to_string)
    }).filter(|id| valid_id(id))
}

fn write_json(mado: &Path, rel: &str, v: &Value) -> anyhow::Result<()> {
    let path = core::place_for_write(mado, rel)?;
    let mut text = serde_json::to_string_pretty(v)?;
    text.push('\n');
    crate::paths::write_atomic(&path, text.as_bytes(), false)?;
    Ok(())
}

/// Direct changes are closed while the step is approved: the user reopens it first, so that what
/// was approved is what gets built.
fn require_open(mado: &Path) -> anyhow::Result<()> {
    if core::design_approved(mado) { return Err(crate::tr::err("mado.design.approved", &[])); }
    Ok(())
}

/// `GET /design/styles` → `{styles: StyleCandidate[], picked?: id, dropped: n}`.
async fn list_styles(State(st): S, UrlPath(project): UrlPath<String>) -> R {
    let mado = core::mado_dir(&st, &project).await?;
    let (styles, dropped) = candidates(&mado);
    let mut out = json!({"styles": styles, "dropped": dropped});
    if let Some(id) = picked(&mado) { out["picked"] = json!(id); }
    Ok(Json(out))
}

/// `POST /design/style {id}`: pick a style; its settings become `design/tokens.json`.
async fn pick(State(st): S, UrlPath(project): UrlPath<String>, Json(body): Json<Value>) -> R {
    let id = body.get("id").and_then(Value::as_str).filter(|id| valid_id(id)).ok_or_else(|| crate::tr::err("mado.design.bad_id", &[]))?.to_string();
    let mado = core::mado_dir(&st, &project).await?;
    let _g = WRITE.lock().await;
    require_open(&mado)?;
    let (all, _) = candidates(&mado);
    let style = all.into_iter().find(|s| s["id"] == id.as_str()).ok_or_else(|| crate::tr::err("mado.design.no_style", &[]))?;
    write_json(&mado, TOKENS, &style["tokens"])?;
    write_json(&mado, PICKED, &json!({"version": 1, "id": id}))?;
    core::tell(&st, &project, "design");
    Ok(Json(json!({"picked": id, "tokens": style["tokens"]})))
}

/// `GET /design/tokens` → `DesignTokens`, or `null` while no style has been picked.
async fn get_tokens(State(st): S, UrlPath(project): UrlPath<String>) -> R {
    let mado = core::mado_dir(&st, &project).await?;
    let Some(text) = core::read_text(&mado, TOKENS, MAX_TOKENS)? else { return Ok(Json(Value::Null)) };
    let tokens = serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()).and_then(|v| Tokens::parse(&v))
        .map_err(|why| crate::tr::err_detail("mado.design.saved_bad", &[], why))?;
    Ok(Json(tokens.json()))
}

/// `PUT /design/tokens DesignTokens`: the user's own change, saved at once without a run.
async fn put_tokens(State(st): S, UrlPath(project): UrlPath<String>, Json(body): Json<Value>) -> R {
    let tokens = Tokens::parse(&body).map_err(|why| crate::tr::err_detail("mado.design.bad_tokens", &[], why))?;
    let mado = core::mado_dir(&st, &project).await?;
    let _g = WRITE.lock().await;
    require_open(&mado)?;
    write_json(&mado, TOKENS, &tokens.json())?;
    core::tell(&st, &project, "design");
    Ok(Json(tokens.json()))
}

fn mock_path(id: &str) -> String { format!("design/screens/{id}.html") }

/// `GET /design/screens` → `[{id, label, hasMock}]`, the flow's screens in its order.
async fn screens(State(st): S, UrlPath(project): UrlPath<String>) -> R {
    let mado = core::mado_dir(&st, &project).await?;
    let list: Vec<Value> = core::screens(&mado).into_iter().map(|s| {
        let has = matches!(core::inside(&mado, &mock_path(&s.id)), Ok(Some(_)));
        json!({"id": s.id, "label": s.label, "hasMock": has})
    }).collect();
    Ok(Json(json!(list)))
}

/// `GET /design/screens/:id` → `{html}`: the mock-up, cleaned. Never the file as written.
async fn screen(State(st): S, UrlPath((project, id)): UrlPath<(String, String)>) -> R {
    if !valid_id(&id) { return Err(crate::tr::err("mado.design.bad_id", &[]).into()); }
    let mado = core::mado_dir(&st, &project).await?;
    let raw = core::read_text(&mado, &mock_path(&id), MAX_MOCK)?.ok_or_else(|| crate::tr::err("mado.design.no_mock", &[]))?;
    // Parsing a large page takes a moment: not on the threads that answer everything else.
    let html = tokio::task::spawn_blocking(move || clean(&raw)).await?;
    Ok(Json(json!({"html": html})))
}

#[cfg(test)]
#[path = "mado_design_tests.rs"]
mod tests;
