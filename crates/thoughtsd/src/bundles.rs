//! Plugins: named, switchable, exportable customisations built on packs (round 0.5.0, W1).
//! A stub until that work lands: no routes yet.

use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

pub fn routes() -> Router<Arc<AppState>> { Router::new() }
