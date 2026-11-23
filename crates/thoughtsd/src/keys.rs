//! Keyboard shortcuts the user sets in the layer's `keys.json` (round 0.5.0, W2).
//! A stub until that work lands: no routes, nothing to check, one History title.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use serde_json::Value;

use crate::state::AppState;

pub fn routes() -> Router<Arc<AppState>> { Router::new() }

/// Everything wrong with the staged `keys.json` in `_dir`, as `{path, message}` (`staging::validate::problems`).
pub fn problems(_dir: &Path, _arabic: bool) -> Vec<Value> { vec![] }

/// The History title of a change the app made to the keyboard shortcuts (`applayer` `Act` kind `keys`).
pub fn act_title(ar: bool) -> String {
    (if ar { "تغيير اختصارات لوحة المفاتيح" } else { "Changed the keyboard shortcuts" }).into()
}
