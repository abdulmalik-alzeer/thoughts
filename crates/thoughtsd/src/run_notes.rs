//! The extra notes a run is told on top of its system text, one per feature (`claude::start_run`
//! joins them for Claude Code and folds them into `append_system` for Codex). A feature that has
//! nothing to say returns `None`, and then the run's text is exactly what it was without it.

use std::path::Path;

use crate::state::AppState;

pub async fn all(state: &AppState, kind: &str, prompt: &str, library: Option<&Path>) -> Vec<String> {
    let mut notes = vec![];
    // slides
    if let Some(n) = crate::slides::run_note().await { notes.push(n); }
    // meaning
    if let Some(n) = crate::meaning::run_note(state, kind, prompt, library).await { notes.push(n); }
    notes
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn with_nothing_to_say_a_run_is_told_nothing_more() {
        let st = crate::state::AppState::for_test();
        assert!(super::all(&st, "chat", "hi", None).await.is_empty());
        assert!(crate::keys::problems(std::path::Path::new("."), false).is_empty());
        assert!(!crate::keys::act_title(false).is_empty() && !crate::keys::act_title(true).is_empty());
    }
}
