//! Tests for the chat endpoints in `api.rs` (create, delete, send, attachments).

use super::*;
use crate::wraps::testutil as t;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn shown(r: R) -> crate::tr::UserErr {
    match r { Ok(v) => panic!("expected an error, got {}", v.0), Err(e) => crate::tr::classify_in(false, &e.0) }
}

/// Serve the real router on a local port and return it.
async fn serve(st: &Arc<AppState>) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = router(st.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

/// One HTTP request with the test token; returns (status, body).
async fn http(port: u16, method: &str, path: &str, body: &[u8]) -> (u16, String) {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer test-token\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    s.write_all(head.as_bytes()).await.unwrap();
    // The server may answer (and close) before a refused body has been sent in full.
    let _ = s.write_all(body).await;
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out).await;
    let out = String::from_utf8_lossy(&out).to_string();
    let status = out.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default())
}

#[tokio::test]
async fn deleting_a_chat_takes_only_a_real_chat_id() {
    let st = Arc::new(AppState::for_test());
    let home = paths::home();
    let neighbour = format!("api-neighbour-{}", uuid::Uuid::new_v4());
    std::fs::create_dir_all(home.join(&neighbour)).unwrap();
    std::fs::write(home.join(&neighbour).join("keep.txt"), "keep").unwrap();
    let chat = t::chat(&st, "to be deleted").await;
    let files = paths::attachments_dir().join(&chat);
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("a.txt"), "x").unwrap();
    let keep = t::chat(&st, "stays").await;
    let intact = || {
        assert!(home.join(&neighbour).join("keep.txt").exists());
        for sub in ["chats", "attachments", "logs"] { assert!(home.join(sub).is_dir(), "{sub} is gone"); }
    };

    // Called directly, as the router would after decoding the path.
    for id in ["..", "../..", ".", "", "/", &format!("../{neighbour}"), &format!("../../{}", home.file_name().unwrap().to_string_lossy()), "a/b", "a\\b"] {
        let e = shown(chat_delete(State(st.clone()), Path(id.to_string())).await);
        assert_eq!((e.status, e.code), (404, "not_found"), "{id:?}");
        intact();
    }
    // A well-formed id that is not a chat is "not found" too, and removes nothing.
    assert_eq!(shown(chat_delete(State(st.clone()), Path(uuid::Uuid::new_v4().to_string())).await).status, 404);

    // Over HTTP, with the encodings a client could try.
    let port = serve(&st).await;
    for path in ["/api/chats/..%2F", "/api/chats/%2E%2E", "/api/chats/..%2F..", "/api/chats/%2e%2e%2f%2e%2e", &format!("/api/chats/..%2F{neighbour}"),
                 &format!("/api/chats/..%2F..%2F{}", home.file_name().unwrap().to_string_lossy()), "/api/chats/..%5C..", "/api/chats/%2Fetc", "/api/chats/.", "/api/chats/%00"] {
        let (status, _) = http(port, "DELETE", path, b"").await;
        assert!(matches!(status, 400 | 404 | 405), "{path}: {status}");
        intact();
    }
    assert_eq!(st.chats.list().await.len(), 2, "no chat was removed by a bad id");
    assert!(files.join("a.txt").exists());

    // The real id still deletes the chat, its file and its attachments — and nothing else.
    let (status, _) = http(port, "DELETE", &format!("/api/chats/{chat}"), b"").await;
    assert_eq!(status, 200);
    assert!(st.chats.get(&chat).await.is_none() && !files.exists());
    assert!(st.chats.get(&keep).await.is_some());
    let (status, body) = http(port, "DELETE", &format!("/api/chats/{chat}"), b"").await;
    assert_eq!(status, 404, "{body}");
    intact();
    std::fs::remove_dir_all(home.join(&neighbour)).unwrap();
}

/// Claude Code's folder under test: always inside the throwaway test home.
fn claude_dir() -> std::path::PathBuf {
    let dir = crate::traces::config_dir().unwrap();
    assert!(dir.starts_with(paths::home()), "tests must never touch the real ~/.claude");
    dir
}
fn transcript_line(sid: &str, slug: &str) -> String { format!("{{\"sessionId\":\"{sid}\",\"type\":\"user\",\"slug\":\"{slug}\"}}\n") }

#[tokio::test]
async fn deleting_a_chat_also_deletes_the_assistants_own_record_of_it() {
    let st = Arc::new(AppState::for_test());
    let config = claude_dir();
    let id = || uuid::Uuid::new_v4().to_string();
    let (first, second, other) = (id(), id(), id());
    let (plan, other_plan) = (format!("wrap-the-user-{}", uuid::Uuid::new_v4().simple()), format!("someone-elses-{}", uuid::Uuid::new_v4().simple()));
    let project = config.join(format!("projects/-Users-sara-{}", uuid::Uuid::new_v4().simple()));
    t::write(&project, &format!("{first}.jsonl"), &transcript_line(&first, &plan));
    t::write(&project, &format!("{second}.jsonl"), "{\"type\":\"user\"}\n");
    t::write(&project, &format!("{other}.jsonl"), &transcript_line(&other, &other_plan));
    t::write(&config, &format!("plans/{plan}.md"), "what the chats said\n");
    t::write(&config, &format!("plans/{other_plan}.md"), "not this chat's\n");

    let chat = t::chat(&st, "a client matter").await;
    // The chat ran twice; the second run was given a new session by the CLI.
    st.chats.update(&chat, |c| c.session_id = Some(first.clone())).await;
    st.chats.update(&chat, |c| c.session_id = Some(second.clone())).await;
    let untouched = t::chat(&st, "another chat").await;
    st.chats.update(&untouched, |c| c.session_id = Some(other.clone())).await;

    chat_delete(State(st.clone()), Path(chat.clone())).await.unwrap();
    assert!(!project.join(format!("{first}.jsonl")).exists() && !project.join(format!("{second}.jsonl")).exists(), "both sessions' transcripts go");
    assert!(!config.join(format!("plans/{plan}.md")).exists(), "the plan file its preview wrote goes");
    assert!(project.join(format!("{other}.jsonl")).exists() && config.join(format!("plans/{other_plan}.md")).exists(), "another chat's records stay");
    assert!(project.is_dir() && config.join("plans").is_dir());
    // A chat that never ran has nothing there, and deleting it is as quiet as before.
    let never_ran = t::chat(&st, "never sent").await;
    chat_delete(State(st.clone()), Path(never_ran)).await.unwrap();
    assert!(project.join(format!("{other}.jsonl")).exists());
}

#[tokio::test]
async fn deleting_a_chat_mid_answer_waits_for_the_assistant_to_stop_writing() {
    let (st, lib) = t::library().await;
    let config = claude_dir();
    let sid = uuid::Uuid::new_v4().to_string();
    let file = config.join(format!("projects/-running-{}/{sid}.jsonl", uuid::Uuid::new_v4().simple()));
    let chat = t::chat(&st, "long answer").await;
    st.chats.update(&chat, |c| c.session_id = Some(sid.clone())).await;
    // The stand-in assistant keeps appending to its transcript until it is stopped.
    let f = file.clone();
    t::assistant(&st, move |_| { let f = f.clone(); async move {
        loop {
            t::write(f.parent().unwrap(), &f.file_name().unwrap().to_string_lossy(), "{\"type\":\"assistant\"}\n");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    } });
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "go".into(), ..Default::default() })).await.unwrap();
    for _ in 0..500 { if file.exists() { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
    assert!(file.exists() && st.run_active_in(&lib));

    chat_delete(State(st.clone()), Path(chat.clone())).await.unwrap();
    assert!(!st.run_active_in(&lib), "the run was stopped first");
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    assert!(!file.exists(), "nothing wrote the transcript back after it was deleted");
    assert!(st.chats.get(&chat).await.is_none());
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}
