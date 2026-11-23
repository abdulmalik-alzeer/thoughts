//! Search by meaning (round 0.5.0, W3). A stub until that work lands: no routes, no run note.

use std::path::Path;
use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

pub fn routes() -> Router<Arc<AppState>> { Router::new() }

/// What a run is told about searching by meaning (`run_notes::all`). Nothing yet.
pub async fn run_note(_state: &AppState, _kind: &str, _prompt: &str, _library: Option<&Path>) -> Option<String> { None }
