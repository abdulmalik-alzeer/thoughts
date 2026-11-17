//! How a run that cannot resume its session is recognised (see `error_msg`, `is_session_lost`),
//! and how the answer being written is kept for a view that opens in the middle of it.

use super::*;
use crate::chats::Chat;

/// What `claude -p --output-format stream-json --resume <id>` prints (CLI 2.1.295) when it has no
/// such session: one `result` event, no `result` text, the reason under `errors`. Nothing is sent
/// to the model and nothing is charged.
const LOST: &str = r#"{"type":"result","subtype":"error_during_execution","duration_ms":0,"duration_api_ms":0,"is_error":true,"num_turns":0,"stop_reason":null,"session_id":"00000000-0000-4000-8000-000000000000","total_cost_usd":0,"usage":{"input_tokens":0,"output_tokens":0},"modelUsage":{},"permission_denials":[],"uuid":"bf63669b-0ec5-4786-895f-e3d8c1130136","errors":["No conversation found with session ID: 00000000-0000-4000-8000-000000000000"],"result_index":0}"#;

async fn chat(state: &AppState) -> String {
    let c = Chat { session_id: Some("00000000-0000-4000-8000-000000000000".into()), ..Default::default() };
    let id = c.id.clone();
    state.chats.insert(c).await;
    id
}

#[tokio::test]
async fn a_session_the_cli_no_longer_has_is_marked_as_such() {
    let state = AppState::for_test();
    let id = chat(&state).await;
    let ev: Value = serde_json::from_str(LOST).unwrap();
    assert_eq!(handle_event(&state, &id, &ev).await, Some(false));
    let c = state.chats.get(&id).await.unwrap();
    let last = c.messages.last().unwrap();
    assert_eq!((last.role.as_str(), last.tool.as_ref().unwrap()["code"].as_str()), ("error", Some(SESSION_LOST)));
    // Plain words on screen; the CLI's own sentence is kept as the detail.
    assert_eq!(last.text, crate::tr::text_in(false, "assistant.session_lost", &[]));
    assert!(last.tool.as_ref().unwrap()["detail"].as_str().unwrap().starts_with("No conversation found with session ID"));
    assert_eq!(c.messages.len(), 1);

    // Older CLIs say it on stderr with no result event: the same marking.
    let m = error_msg(&state, "No conversation found with session ID: 0b8f1c2e-3a4d-4e5f-8a9b-0c1d2e3f4a5b").await;
    assert_eq!(m.tool.unwrap()["code"], SESSION_LOST);
    assert!(!is_session_lost("Credit balance is too low") && !is_session_lost(""));
}

#[tokio::test]
async fn a_failed_run_shows_the_reason_the_cli_listed() {
    let state = AppState::for_test();
    let id = chat(&state).await;
    let ev = json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "session_id": "s", "errors": ["The model is overloaded", "Try again later"]});
    assert_eq!(handle_event(&state, &id, &ev).await, Some(false));
    // What it means is said plainly; the assistant's own words are kept as detail.
    let c = state.chats.get(&id).await.unwrap();
    let last = c.messages.last().unwrap();
    assert_eq!(last.text, crate::tr::text("assistant.overloaded", &[]));
    let tool = last.tool.as_ref().unwrap();
    assert_eq!((tool["code"].as_str(), tool["reason"].as_str()), (Some(ASSISTANT_ERROR), Some("overloaded")));
    assert_eq!(tool["detail"], "The model is overloaded\nTry again later");
    // With neither a text nor a list, the kind of ending is all there is to go on.
    let ev = json!({"type": "result", "subtype": "error_max_turns", "is_error": true});
    handle_event(&state, &id, &ev).await;
    let last = state.chats.get(&id).await.unwrap().messages.last().unwrap().clone();
    assert_eq!(last.text, crate::tr::text("assistant.turn_limit", &[]));
    assert_eq!(last.tool.unwrap()["detail"], "Run ended: error_max_turns");
    // A result text still wins.
    let ev = json!({"type": "result", "subtype": "success", "is_error": true, "result": "Credit balance is too low", "errors": ["ignored"]});
    handle_event(&state, &id, &ev).await;
    let last = state.chats.get(&id).await.unwrap().messages.last().unwrap().clone();
    assert_eq!(last.text, crate::tr::text("assistant.credit", &[]));
    assert_eq!(last.tool.unwrap()["detail"], "Credit balance is too low");
}

#[test]
fn a_failed_run_is_told_in_plain_words() {
    for (raw, key) in [
        ("API Error: 529 {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}", "assistant.overloaded"),
        ("API Error: 500 Internal server error", "assistant.overloaded"),
        ("API Error: 429 rate_limit_error", "assistant.rate_limit"),
        ("API Error: Connection error.", "assistant.offline"),
        ("Error: getaddrinfo ENOTFOUND api.anthropic.com", "assistant.offline"),
        ("Run ended: error_max_turns", "assistant.turn_limit"),
        ("Run ended: error_max_budget_usd", "assistant.budget"),
        ("Prompt is too long", "assistant.too_long"),
        ("Credit balance is too low", "assistant.credit"),
        ("Run ended: error_during_execution", "assistant.failed"),
        ("something nobody has seen before", "assistant.failed"),
    ] {
        assert_eq!(run_problem(raw), key, "{raw}");
        // Every reason has its sentence in both languages, and none is the raw text.
        assert!(crate::tr::text_in(true, key, &[]) != crate::tr::text_in(false, key, &[]) && !crate::tr::text_in(false, key, &[]).contains("Run ended"), "{key}");
    }
}

#[test]
fn an_automatic_task_that_hit_its_budget_is_still_known_by_its_detail() {
    // The chat says it in plain words; the outcome is read from the words behind them.
    let shown = format!("{} {}", crate::tr::text_in(true, "assistant.budget", &[]), "Run ended: error_max_budget_usd");
    assert_eq!(crate::workflows::outcome(false, false, false, false, 0.1, 2.0, &shown), "spendLimit");
}

#[tokio::test]
async fn pieces_of_an_answer_are_numbered_and_kept_until_the_message_is_complete() {
    let state = AppState::for_test();
    let id = chat(&state).await;
    state.chats.update(&id, |c| c.running = true).await;
    let mut rx = state.events.subscribe();
    let delta = |d: Value| json!({"type": "stream_event", "event": {"type": "content_block_delta", "delta": d}});
    handle_event(&state, &id, &delta(json!({"type": "thinking_delta", "thinking": "Looking at the notes"}))).await;
    handle_event(&state, &id, &delta(json!({"type": "text_delta", "text": "The shop "}))).await;
    handle_event(&state, &id, &delta(json!({"type": "text_delta", "text": "should sell candles."}))).await;
    let events: Vec<Value> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    let pieces: Vec<(&str, Option<u64>)> = events.iter().filter(|e| e["type"] == "chat.delta").map(|e| (e["text"].as_str().unwrap(), e["seq"].as_u64())).collect();
    assert_eq!(pieces, [("The shop ", Some(1)), ("should sell candles.", Some(2))]);
    // Reasoning is numbered too (on its own count), so a view refreshing mid-answer keeps what it adds.
    let thought: Vec<Option<u64>> = events.iter().filter(|e| e["type"] == "chat.thinking").map(|e| e["seq"].as_u64()).collect();
    assert_eq!(thought, [Some(1)]);
    let p = state.chats.partial(&id).unwrap();
    assert_eq!((p.text.as_str(), p.seq, p.thinking.as_str()), ("The shop should sell candles.", 2, "Looking at the notes"));
    // The completed message takes the place of the pieces.
    let whole = json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "The shop should sell candles."}]}});
    handle_event(&state, &id, &whole).await;
    let p = state.chats.partial(&id).unwrap();
    assert_eq!((p.text.as_str(), p.seq, p.thinking.as_str()), ("", 2, ""));
    assert_eq!(state.chats.get(&id).await.unwrap().messages.last().unwrap().text, "The shop should sell candles.");
}
