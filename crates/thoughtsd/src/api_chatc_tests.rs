//! Tests for the composer side of the chat endpoints: a send that cannot start, queued messages,
//! mentions, attachments shown in the chat and suggested next messages.

use super::*;
use crate::wraps::testutil as t;

fn shown(r: R) -> crate::tr::UserErr {
    match r { Ok(v) => panic!("expected an error, got {}", v.0), Err(e) => crate::tr::classify_in(false, &e.0) }
}
fn b64(bytes: &[u8]) -> String { base64::engine::general_purpose::STANDARD.encode(bytes) }

#[tokio::test]
async fn a_message_whose_answer_cannot_start_is_taken_back_with_its_files() {
    let (st, lib) = t::library().await;
    t::assistant(&st, |_| async { true });
    // A new chat with a session id the assistant program would refuse: the run cannot be started at all.
    let c = Chat { session_id: Some("-not a session".into()), library: Some(lib.to_string_lossy().to_string()), ..Default::default() };
    let chat = c.id.clone();
    st.chats.insert(c).await;
    let before = st.chats.get(&chat).await.unwrap();
    let mut rx = st.events.subscribe();
    let body = SendBody { text: "And soap?".into(), attachments: vec![Attachment { name: "list.txt".into(), data: b64(b"soap") }], ..Default::default() };
    assert!(chat_send(State(st.clone()), Path(chat.clone()), Json(body)).await.is_err());

    let after = st.chats.get(&chat).await.unwrap();
    let texts = |c: &Chat| c.messages.iter().map(|m| m.text.clone()).collect::<Vec<_>>();
    assert_eq!(texts(&after), texts(&before), "the message is not left behind");
    assert_eq!((after.title.as_str(), after.dirty, after.updated, after.running), ("New chat", false, before.updated, false), "named after a message it does not hold");
    let dir = paths::attachments_dir().join(&chat);
    assert!(!dir.join("list.txt").exists(), "its file went with it");
    // The view was told: the message it was shown is removed again.
    let mut added = None;
    let mut removed = None;
    while let Ok(e) = rx.try_recv() {
        if e["type"] == "chat.message" { added = Some(e["message"]["id"].clone()); }
        if e["type"] == "chat.message.removed" { removed = Some(e["messageId"].clone()); }
    }
    assert!(added.is_some() && added == removed, "{added:?} {removed:?}");
    // Sent again once it can start, it is there once.
    st.chats.update(&chat, |c| c.session_id = None).await;
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "And soap?".into(), ..Default::default() })).await.unwrap();
    t::event(&mut rx, &st, "chat.done").await;
    let c = st.chats.get(&chat).await.unwrap();
    assert_eq!(c.messages.iter().filter(|m| m.text == "And soap?").count(), 1);
    assert_eq!(c.title, "And soap?");
    assert_eq!(shown(chat_send(State(st.clone()), Path("not-a-chat".into()), Json(SendBody::default())).await).status, 404);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}
