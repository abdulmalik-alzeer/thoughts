//! Suggested next messages: asked for only in ordinary chats, read from the CLI's own event.

use super::*;
use crate::chats::Chat;

/// The event as the CLI defines it (2.1.295, SDK message `prompt_suggestion`).
fn event(text: &str) -> Value {
    json!({"type": "prompt_suggestion", "suggestion": text, "uuid": "6f1c3a9e-1b2d-4c5e-9f00-112233445566", "session_id": "00000000-0000-4000-8000-000000000000"})
}

#[tokio::test]
async fn a_suggestion_is_kept_with_the_chat_and_announced() {
    let state = AppState::for_test();
    let c = Chat::default();
    let id = c.id.clone();
    state.chats.insert(c).await;
    let mut rx = state.events.subscribe();
    assert_eq!(handle_event(&state, &id, &event("  Can you turn that into a checklist?\n")).await, None, "it does not end the run");
    assert_eq!(state.chats.get(&id).await.unwrap().suggestion.as_deref(), Some("Can you turn that into a checklist?"));
    let e = rx.try_recv().unwrap();
    assert_eq!((e["type"].as_str(), e["chatId"].as_str(), e["text"].as_str()), (Some("chat.suggestion"), Some(id.as_str()), Some("Can you turn that into a checklist?")));
    // Nothing to suggest, or no text at all: nothing changes, nothing is said.
    for ev in [event("   "), json!({"type": "prompt_suggestion"}), json!({"type": "prompt_suggestion", "suggestion": 42})] {
        handle_event(&state, &id, &ev).await;
        assert!(rx.try_recv().is_err());
    }
    assert_eq!(state.chats.get(&id).await.unwrap().suggestion.as_deref(), Some("Can you turn that into a checklist?"));
    // A very long one is cut to size.
    handle_event(&state, &id, &event(&"word ".repeat(400))).await;
    assert_eq!(state.chats.get(&id).await.unwrap().suggestion.unwrap().chars().count(), SUGGESTION_MAX_CHARS);
    // Kept in the chat's file, and only while there is one.
    let mut c = state.chats.get(&id).await.unwrap();
    assert!(serde_json::to_value(&c).unwrap()["suggestion"].is_string());
    c.suggestion = None;
    assert!(serde_json::to_value(&c).unwrap().get("suggestion").is_none());
}

#[test]
fn the_flag_is_passed_only_when_asked_for() {
    let args = |on: bool| {
        let o = RunOptions { prompt_suggestions: on, ..Default::default() };
        run_args(&o, "acceptEdits", None, None, &[], None, Passed::Inline("note")).iter().map(|a| a.to_string_lossy().to_string()).collect::<Vec<_>>()
    };
    let on = args(true);
    let at = on.iter().position(|a| a == "--prompt-suggestions").expect("asked for");
    assert_eq!(on[at + 1], "true");
    assert!(!args(false).iter().any(|a| a == "--prompt-suggestions"));
    // The app's own choice: never stored with the chat, never taken from a client.
    let o = RunOptions { prompt_suggestions: true, ..Default::default() };
    assert!(serde_json::to_value(&o).unwrap().get("promptSuggestions").is_none());
    let back: RunOptions = serde_json::from_value(json!({"promptSuggestions": true})).unwrap();
    assert!(!back.prompt_suggestions);
}
