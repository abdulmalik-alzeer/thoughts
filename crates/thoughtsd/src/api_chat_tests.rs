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
