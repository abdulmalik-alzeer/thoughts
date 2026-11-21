//! What the design step needs from the planning core (`mado.rs`), kept here under the names agreed
//! for the round so the design step could be built and tested while the core was being written.
//!
//! For whoever merges the branches: `project_dir`, `mado_dir`, `tell` and `screens` are meant to be
//! pointed at the core's own functions of the same names (or this file kept, if the core has none).
//! Everything else in this file is the design step's own careful reading of files a run may have
//! written, and stays.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::state::AppState;

/// Graph, screen and style ids (the contract's pattern).
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty() && b.len() <= 64 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// The linked project's folder. The id must be one of the linked projects, and the folder must be
/// trusted (a linked folder can carry its own hooks and settings) unless full access is on.
pub async fn project_dir(state: &Arc<AppState>, project_id: &str) -> anyhow::Result<PathBuf> {
    let path = {
        let s = state.settings.read().await;
        s.projects.iter().find(|p| p.id == project_id).map(|p| PathBuf::from(&p.path))
    }.ok_or_else(|| crate::tr::err("not_found.project", &[]))?;
    let real = crate::paths::canonical(&path).map_err(|_| crate::tr::err("not_found.folder", &[("path", &path.display().to_string())]))?;
    if !real.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", &path.display().to_string())])); }
    if !crate::full_access::on() { crate::trust::require(&real)?; }
    Ok(real)
}

/// `<project>/mado`, which must exist and really be inside the project (not a link leading out).
pub async fn mado_dir(state: &Arc<AppState>, project_id: &str) -> anyhow::Result<PathBuf> {
    let project = project_dir(state, project_id).await?;
    let dir = project.join("mado");
    let real = crate::paths::canonical(&dir).map_err(|_| crate::tr::err("mado.design.not_started", &[]))?;
    if !real.is_dir() { return Err(crate::tr::err("mado.design.not_started", &[])); }
    if !crate::full_access::on() && !real.starts_with(&project) { return Err(crate::tr::err("mado.design.outside", &[])); }
    Ok(real)
}

/// Tell the window that something of this project's plan changed. It fetches again; the event
/// carries nothing to trust.
pub fn tell(state: &AppState, project_id: &str, what: &str) {
    state.emit(json!({"type": "mado", "projectId": project_id, "what": what}));
}

/// The file `rel` (forward slashes, no `..`) inside `mado`, only when it is an ordinary file that
/// really lies inside the folder: a link, or a path through a linked folder, is refused.
/// `Ok(None)`: there is no such file. Under full access the link check yields, like every guard.
pub fn inside(mado: &Path, rel: &str) -> anyhow::Result<Option<PathBuf>> {
    if rel.is_empty() || rel.split('/').any(|c| c.is_empty() || c == "." || c == ".." || c.contains('\\') || c.contains('\0')) {
        return Err(crate::tr::err("invalid.path", &[]));
    }
    let path = mado.join(rel);
    let meta = match std::fs::symlink_metadata(&path) { Ok(m) => m, Err(_) => return Ok(None) };
    let full = crate::full_access::on();
    if !full && meta.file_type().is_symlink() { return Err(crate::tr::err("mado.design.outside", &[])); }
    let real = match crate::paths::canonical(&path) { Ok(p) => p, Err(_) => return Ok(None) };
    if !full && !real.starts_with(mado) { return Err(crate::tr::err("mado.design.outside", &[])); }
    if !real.is_file() { return Ok(None); }
    Ok(Some(real))
}

/// Read a file of the plan folder as text, refusing anything longer than `max` bytes before
/// reading it. `Ok(None)`: no such file.
pub fn read_text(mado: &Path, rel: &str, max: u64) -> anyhow::Result<Option<String>> {
    let Some(path) = inside(mado, rel)? else { return Ok(None) };
    if std::fs::metadata(&path)?.len() > max { return Err(crate::tr::err("mado.design.too_large", &[])); }
    let bytes = std::fs::read(&path)?;
    if bytes.len() as u64 > max { return Err(crate::tr::err("mado.design.too_large", &[])); }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Where a file of the plan folder is written: its folder is made if needed and must really lie
/// inside `mado`; an existing link at the file's place is refused.
pub fn place_for_write(mado: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    if rel.is_empty() || rel.split('/').any(|c| c.is_empty() || c == "." || c == ".." || c.contains('\\') || c.contains('\0')) {
        return Err(crate::tr::err("invalid.path", &[]));
    }
    let path = mado.join(rel);
    let parent = path.parent().ok_or_else(|| crate::tr::err("invalid.path", &[]))?;
    std::fs::create_dir_all(parent)?;
    let full = crate::full_access::on();
    let real_parent = crate::paths::canonical(parent)?;
    if !full && !real_parent.starts_with(mado) { return Err(crate::tr::err("mado.design.outside", &[])); }
    if !full && std::fs::symlink_metadata(&path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err(crate::tr::err("mado.design.outside", &[]));
    }
    Ok(real_parent.join(path.file_name().ok_or_else(|| crate::tr::err("invalid.path", &[]))?))
}

/// A doc may be this long (the contract's limit for documents, used for the JSON files too).
pub const MAX_JSON: u64 = 1024 * 1024;

/// `state.json` as written by the core, or `Null`. Read only: the design step never writes it.
pub fn state_json(mado: &Path) -> Value {
    read_text(mado, "state.json", MAX_JSON).ok().flatten().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

/// Is the design step approved (and so closed to direct changes until it is reopened)?
pub fn design_approved(mado: &Path) -> bool {
    state_json(mado).pointer("/steps/design/status").and_then(Value::as_str) == Some("approved")
}

/// One screen of the flow.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen { pub id: String, pub label: String }

/// The flow graph's `screen` nodes, in the file's order. Nodes with an id outside the pattern are
/// left out; a missing or unreadable graph is simply no screens. At most the contract's 400 nodes.
pub fn screens(mado: &Path) -> Vec<Screen> {
    let Some(text) = read_text(mado, "flow.graph.json", MAX_JSON).ok().flatten() else { return vec![] };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else { return vec![] };
    let mut seen = std::collections::HashSet::new();
    doc.get("nodes").and_then(Value::as_array).map(|nodes| nodes.iter().take(400).filter_map(|n| {
        if n.get("kind").and_then(Value::as_str) != Some("screen") { return None; }
        let id = n.get("id").and_then(Value::as_str).filter(|id| valid_id(id))?;
        if !seen.insert(id.to_string()) { return None; }
        let label: String = n.get("label").and_then(Value::as_str).unwrap_or(id).chars().filter(|c| !c.is_control()).take(120).collect();
        Some(Screen { id: id.to_string(), label: if label.trim().is_empty() { id.to_string() } else { label } })
    }).collect()).unwrap_or_default()
}
