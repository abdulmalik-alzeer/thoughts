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

/// Serve the real router on a local port; one request with the test token → (status, headers, body bytes).
async fn serve(st: &Arc<AppState>) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = router(st.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}
async fn get_raw(port: u16, path: &str) -> (u16, String, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let head = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer test-token\r\nConnection: close\r\n\r\n");
    s.write_all(head.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out).await;
    let at = out.windows(4).position(|w| w == b"\r\n\r\n").map_or(out.len(), |i| i + 4);
    let head = String::from_utf8_lossy(&out[..at]).to_string();
    let status = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, head.to_lowercase(), out[at..].to_vec())
}

#[tokio::test]
async fn a_chats_own_attachments_are_shown_and_nothing_else_is() {
    let st = Arc::new(AppState::for_test());
    let chat = t::chat(&st, "look at this").await;
    let other = t::chat(&st, "another").await;
    let dir = paths::attachments_dir().join(&chat);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("photo (2).png"), b"\x89PNG picture").unwrap();
    std::fs::write(dir.join("صورة.jpg"), b"jpeg").unwrap();
    std::fs::write(dir.join("page.html"), b"<script>alert(1)</script>").unwrap();
    let elsewhere = paths::home().join(format!("outside-{}.txt", uuid::Uuid::new_v4()));
    std::fs::write(&elsewhere, b"not the chat's").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, dir.join("link.txt")).unwrap();
    let odir = paths::attachments_dir().join(&other);
    std::fs::create_dir_all(&odir).unwrap();
    std::fs::write(odir.join("theirs.png"), b"other chat").unwrap();
    let port = serve(&st).await;

    let (status, head, body) = get_raw(port, &format!("/api/chats/{chat}/attachments/photo%20(2).png")).await;
    assert_eq!((status, body.as_slice()), (200, &b"\x89PNG picture"[..]));
    assert!(head.contains("content-type: image/png") && head.contains("nosniff") && head.contains("sandbox"), "{head}");
    let (status, _, body) = get_raw(port, &format!("/api/chats/{chat}/attachments/{}", "%D8%B5%D9%88%D8%B1%D8%A9.jpg")).await;
    assert_eq!((status, body.as_slice()), (200, &b"jpeg"[..]));
    // A page is only ever text: it cannot run.
    let (_, head, _) = get_raw(port, &format!("/api/chats/{chat}/attachments/page.html")).await;
    assert!(head.contains("content-type: text/plain"), "{head}");
    for bad in ["..%2F..%2Fsettings.json", "%2E%2E", "..", ".", "link.txt", "missing.png", "..%5Ctheirs.png", &format!("..%2F{other}%2Ftheirs.png"), "%00.png"] {
        let (status, _, body) = get_raw(port, &format!("/api/chats/{chat}/attachments/{bad}")).await;
        assert!(matches!(status, 400 | 404), "{bad}: {status}");
        assert!(!body.windows(5).any(|w| w == b"other" || w == b"not t"), "{bad} leaked a file");
    }
    // Only a real chat's folder.
    for id in ["..", "not-a-chat", &format!("{chat}%2F..")] {
        let (status, _, _) = get_raw(port, &format!("/api/chats/{id}/attachments/theirs.png")).await;
        assert!(matches!(status, 400 | 404), "{id}: {status}");
    }
    std::fs::remove_file(&elsewhere).unwrap();
}

#[tokio::test]
async fn the_mention_list_comes_by_title_and_leaves_private_folders_out() {
    let (st, lib) = t::library().await;
    t::write(&lib, "clients/acme/README.md", "---\ntitle: Acme Trading\n---\n");
    t::write(&lib, "diary/today.md", "# Today\n");
    st.settings.write().await.private_paths = vec!["diary".into()];
    let v = mention_list(State(st.clone())).await.unwrap().0;
    let list = v.as_array().unwrap();
    assert!(list.iter().any(|x| x["path"] == "clients/acme/README.md" && x["title"] == "Acme Trading" && x["kind"] == "idea"));
    assert!(list.iter().any(|x| x["path"] == "ideas/shop/README.md" && x["title"] == "Shop"));
    assert!(list.iter().all(|x| !x["path"].as_str().unwrap().starts_with("diary")), "{list:?}");
    // A folder can be mentioned like a file; a private one cannot.
    t::assistant(&st, |_| async { true });
    let chat = t::chat(&st, "x").await;
    let mut rx = st.events.subscribe();
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "Look at @Acme Trading".into(), mentions: vec!["clients/acme".into()], ..Default::default() })).await.unwrap();
    t::event(&mut rx, &st, "chat.done").await;
    assert_eq!(shown(chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "and".into(), mentions: vec!["diary".into()], ..Default::default() })).await).status, 403);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn sending_clears_the_suggestion_and_only_ordinary_chats_ask_for_one() {
    let (st, lib) = t::library().await;
    t::assistant(&st, |_| async { true });
    let chat = t::chat(&st, "plan the week").await;
    st.chats.update(&chat, |c| c.suggestion = Some("Make it a table".into())).await;
    let mut rx = st.events.subscribe();
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "Make it a table".into(), ..Default::default() })).await.unwrap();
    assert!(st.chats.get(&chat).await.unwrap().suggestion.is_none(), "gone once the user has said what comes next");
    t::event(&mut rx, &st, "chat.done").await;
    // The choice travels with the run's options; it is off unless switched on in Settings.
    assert!(!st.chats.get(&chat).await.unwrap().options.prompt_suggestions);
    st.settings.write().await.ai.claude.suggest_next = true;
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "again".into(), ..Default::default() })).await.unwrap();
    t::event(&mut rx, &st, "chat.done").await;
    assert!(st.chats.get(&chat).await.unwrap().options.prompt_suggestions);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}
