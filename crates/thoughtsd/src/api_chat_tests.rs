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

fn b64(bytes: &[u8]) -> String { base64::engine::general_purpose::STANDARD.encode(bytes) }
fn att(name: &str, bytes: &[u8]) -> Attachment { Attachment { name: name.into(), data: b64(bytes) } }
fn say(text: &str) -> Json<SendBody> { Json(SendBody { text: text.into(), ..Default::default() }) }

/// Stand in for the assistant: remember each prompt it is given, and answer with `ok`.
fn record_prompts(st: &AppState, ok: bool) -> Arc<std::sync::Mutex<Vec<String>>> {
    let prompts = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let p = prompts.clone();
    t::assistant(st, move |info| { let p = p.clone(); async move { p.lock().unwrap().push(info.prompt); ok } });
    prompts
}

#[test]
fn attachments_are_checked_before_anything_is_written() {
    let err = |list: &[Attachment], max_file, max_total| crate::tr::classify_in(false, &decode_attachments(list, max_file, max_total).unwrap_err());
    // Names are reduced to a safe file name; data URLs and bare base64 both work.
    let ok = decode_attachments(&[att("a.txt", b"12345"), Attachment { name: "../../b.txt".into(), data: format!("data:text/plain;base64,{}", b64(b"123")) }], 5, 8).unwrap();
    assert_eq!(ok, [("a.txt".to_string(), b"12345".to_vec()), ("b.txt".to_string(), b"123".to_vec())]);
    // One file over the limit for a file: named, in plain words.
    let e = err(&[att("small.txt", b"1"), att("big.bin", b"123456")], 5, 100);
    assert_eq!((e.status, e.code, e.message.as_str()), (413, "too_large", "big.bin is larger than 25 MB."));
    // Files that are fine one by one but too much together.
    let e = err(&[att("a", b"12345"), att("b", b"1234")], 5, 8);
    assert_eq!((e.status, e.code), (413, "too_large"));
    assert_eq!(e.message, crate::tr::text_in(false, "chat.files_too_large", &[]));
    // Something that is not a file's bytes at all.
    let e = err(&[Attachment { name: "x.png".into(), data: "data:image/png;base64,@@not base64@@".into() }], 100, 100);
    assert_eq!((e.status, e.message.as_str()), (422, "x.png could not be read."));
    // The real limits are the ones the composer shows, and the request may be large enough to carry them.
    assert_eq!((ATTACHMENT_MAX_BYTES, ATTACHMENTS_MAX_BYTES), (25 * 1024 * 1024, 50 * 1024 * 1024));
    assert!(SEND_BODY_LIMIT > ATTACHMENTS_MAX_BYTES / 3 * 4 + 1024 * 1024);
}

#[tokio::test]
async fn attachments_of_several_megabytes_arrive_and_never_replace_each_other() {
    let (st, lib) = t::library().await;
    let prompts = record_prompts(&st, true);
    let chat = t::chat(&st, "compare").await;
    let port = serve(&st).await;
    // Two pasted screenshots arrive under one name. The first is 3 MB: more than fits in a request by default.
    let big: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let small = b"the second screenshot".to_vec();
    let body = json!({"text": "compare these", "attachments": [
        {"name": "image.png", "data": format!("data:image/png;base64,{}", b64(&big))},
        {"name": "image.png", "data": b64(&small)}]}).to_string();
    let mut rx = st.events.subscribe();
    let (status, text) = http(port, "POST", &format!("/api/chats/{chat}/send"), body.as_bytes()).await;
    assert_eq!(status, 200, "{text}");
    t::event(&mut rx, &st, "chat.done").await;
    let dir = paths::attachments_dir().join(&chat);
    let (one, two) = (dir.join("image.png"), dir.join("image (2).png"));
    assert_eq!(std::fs::read(&one).unwrap(), big);
    assert_eq!(std::fs::read(&two).unwrap(), small);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&two).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let sent = st.chats.get(&chat).await.unwrap().messages.last().cloned().unwrap();
    assert_eq!(sent.attachments, [one.to_string_lossy().to_string(), two.to_string_lossy().to_string()]);
    // The run is pointed at copies made for it (the attachments folder is closed to every run), gone once it is over.
    let prompt = prompts.lock().unwrap().last().cloned().unwrap();
    let copies = crate::run_files::folder(&paths::home(), &chat).unwrap();
    assert!(!prompt.contains(&*one.to_string_lossy()) && !prompt.contains(&*two.to_string_lossy()), "{prompt}");
    let names = |prompt: &str, name: &str| prompt.lines().filter(|l| l.starts_with(&format!("- {}", copies.display())) && l.ends_with(&format!("/{name}"))).count();
    assert_eq!((names(&prompt, "image.png"), names(&prompt, "image (2).png")), (1, 1), "{prompt}");
    assert!(prompt.contains("Read tool"));
    // They stay after the run (the next message's run may be reading them already) and go with the chat.
    assert!(copies.exists(), "the copies went at the end of the run");

    // A later message with the same file name gets a file of its own; the earlier ones are untouched.
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "and this".into(), attachments: vec![att("image.png", b"third")], ..Default::default() })).await.unwrap();
    t::event(&mut rx, &st, "chat.done").await;
    assert_eq!(std::fs::read(dir.join("image (3).png")).unwrap(), b"third");
    // The earlier files are copied for this run too, and it is told where.
    let prompt = prompts.lock().unwrap().last().cloned().unwrap();
    assert!(names(&prompt, "image (3).png") == 1 && prompt.contains(&format!("copies in {}", copies.display())), "{prompt}");
    assert_eq!(std::fs::read(&one).unwrap(), big);
    assert_eq!(std::fs::read(&two).unwrap(), small);
    // A name without an extension, and one that differs only in letter case.
    for (name, bytes) in [("notes", &b"1"[..]), ("notes", b"2"), ("Image.PNG", b"fourth")] { save_attachment(&dir, name, bytes).unwrap(); }
    assert_eq!(std::fs::read(dir.join("notes (2)")).unwrap(), b"2");
    assert_eq!(std::fs::read(&one).unwrap(), big, "a name in other letter case replaced the first file");

    // A message with a file that cannot be accepted leaves nothing behind: no file, no message.
    let before = (std::fs::read_dir(&dir).unwrap().count(), st.chats.meta(&chat).await.unwrap().message_count);
    let bad = Json(SendBody { text: "two files".into(), attachments: vec![att("fine.txt", b"ok"), Attachment { name: "broken.png".into(), data: "@@".into() }], ..Default::default() });
    assert_eq!(shown(chat_send(State(st.clone()), Path(chat.clone()), bad).await).status, 422);
    assert_eq!((std::fs::read_dir(&dir).unwrap().count(), st.chats.meta(&chat).await.unwrap().message_count), before);
    // Deleting the chat removes the copies with it.
    assert!(copies.exists() && st.chats.remove(&chat).await.is_some());
    assert!(!copies.exists(), "the copies outlived their chat");
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn a_request_too_large_to_take_is_refused_in_plain_words() {
    let st = Arc::new(AppState::for_test());
    let chat = t::chat(&st, "x").await;
    let port = serve(&st).await;
    // One byte more than a send may carry, written in pieces.
    let len = SEND_BODY_LIMIT + 1;
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let head = format!("POST /api/chats/{chat}/send HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer test-token\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n");
    s.write_all(head.as_bytes()).await.unwrap();
    let piece = vec![b'a'; 1024 * 1024];
    let mut left = len;
    while left > 0 {
        let n = left.min(piece.len());
        if s.write_all(&piece[..n]).await.is_err() { break; }
        left -= n;
    }
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out).await;
    let out = String::from_utf8_lossy(&out).to_string();
    assert!(out.starts_with("HTTP/1.1 413"), "{}", out.chars().take(200).collect::<String>());
    let body: Value = serde_json::from_str(out.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["code"], "too_large");
    assert_eq!(body["error"], crate::tr::text_in(false, "chat.files_too_large", &[]));
    // Anything else wrong with the body is still answered as before.
    let (status, _) = http(port, "POST", &format!("/api/chats/{chat}/send"), b"{not json").await;
    assert!((400..500).contains(&status) && status != 413, "{status}");
    assert_eq!(st.chats.meta(&chat).await.unwrap().message_count, 1);
}

#[tokio::test]
async fn two_sends_at_once_start_one_run() {
    let (st, lib) = t::library().await;
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let gate = Arc::new(tokio::sync::Notify::new());
    let (r, g) = (runs.clone(), gate.clone());
    t::assistant(&st, move |_| { let (r, g) = (r.clone(), g.clone()); async move {
        r.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        g.notified().await;
        true
    } });
    let chat = t::chat(&st, "first").await;
    let mut rx = st.events.subscribe();
    let (a, b) = tokio::join!(chat_send(State(st.clone()), Path(chat.clone()), say("one")), chat_send(State(st.clone()), Path(chat.clone()), say("two")));
    assert!(a.is_ok() != b.is_ok(), "exactly one of the two is accepted");
    let refused = shown(if a.is_err() { a } else { b });
    assert_eq!((refused.status, refused.code), (409, "busy"));
    // The refused message was not added, and a third try while the first is answered is refused the same way.
    assert_eq!(st.chats.meta(&chat).await.unwrap().message_count, 2);
    assert_eq!(shown(chat_send(State(st.clone()), Path(chat.clone()), say("three")).await).status, 409);
    for _ in 0..500 { if runs.load(std::sync::atomic::Ordering::SeqCst) > 0 { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
    gate.notify_one();
    assert_eq!(t::event(&mut rx, &st, "chat.done").await["ok"], true);
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
    let c = st.chats.get(&chat).await.unwrap();
    assert!(!c.messages.iter().any(|m| m.role == "system"), "no run was stopped");
    // Once it has finished, the chat takes the next message as usual.
    chat_send(State(st.clone()), Path(chat.clone()), say("four")).await.unwrap();
    for _ in 0..500 { if runs.load(std::sync::atomic::Ordering::SeqCst) > 1 { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
    gate.notify_one();
    t::event(&mut rx, &st, "chat.done").await;
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

const OLD_SESSION: &str = "0b8f1c2e-3a4d-4e5f-8a9b-0c1d2e3f4a5b";

/// A chat with one exchange behind it and a session id, as after an earlier run.
async fn chat_with_history(st: &AppState) -> String {
    let chat = t::chat(st, "What should the shop sell?").await;
    st.chats.update(&chat, |c| { c.messages.push(Msg::new("assistant", "Candles, to start with.")); c.session_id = Some(OLD_SESSION.into()); }).await;
    chat
}

/// Collect events until `n` runs of this chat have ended.
async fn until_done(rx: &mut tokio::sync::broadcast::Receiver<Value>, n: usize) -> Vec<Value> {
    let mut seen = vec![];
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while seen.iter().filter(|e: &&Value| e["type"] == "chat.done").count() < n { seen.push(rx.recv().await.unwrap()); }
    }).await.expect("the runs did not finish");
    seen
}

#[tokio::test]
async fn a_chat_whose_session_is_gone_carries_on_from_the_saved_conversation() {
    let (st, lib) = t::library().await;
    let chat = chat_with_history(&st).await;
    let prompts = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (p, weak) = (prompts.clone(), Arc::downgrade(&st));
    t::assistant(&st, move |info| { let (p, st) = (p.clone(), weak.upgrade().unwrap()); async move {
        let n = { let mut all = p.lock().unwrap(); all.push(info.prompt.clone()); all.len() };
        if n == 1 {
            // Asked to resume a session it no longer has, the CLI ends the run at once with this failure.
            let mut m = Msg::new("error", "The assistant could no longer open its earlier session for this conversation.");
            m.tool = Some(json!({"code": claude::SESSION_LOST, "detail": format!("No conversation found with session ID: {OLD_SESSION}")}));
            claude::push(&st, &info.chat_id, m).await;
            return false;
        }
        claude::push(&st, &info.chat_id, Msg::new("assistant", "Soap as well.")).await;
        true
    } });
    let mut rx = st.events.subscribe();
    chat_send(State(st.clone()), Path(chat.clone()), say("And what else?")).await.unwrap();
    let events = until_done(&mut rx, 2).await;

    // Sent once with the session, then once more with the conversation in its place.
    let sent = prompts.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], "And what else?");
    assert!(sent[1].starts_with("Earlier conversation (continue from it):") && sent[1].ends_with("And what else?"), "{}", sent[1]);
    assert!(sent[1].contains("USER: What should the shop sell?") && sent[1].contains("ASSISTANT: Candles, to start with."));
    assert_eq!(sent[1].matches("And what else?").count(), 1, "the new message is not also replayed as history");
    // The chat: the question once, a quiet note in place of the failure, then the answer.
    let c = st.chats.get(&chat).await.unwrap();
    let shape: Vec<(&str, &str)> = c.messages.iter().map(|m| (m.role.as_str(), m.text.as_str())).collect();
    let note = crate::tr::text_in(false, "chat.session_lost", &[]);
    assert_eq!(shape, [("user", "What should the shop sell?"), ("assistant", "Candles, to start with."), ("user", "And what else?"),
        ("system", note.as_str()), ("assistant", "Soap as well.")]);
    assert!(c.session_id.is_none() && !c.running);
    assert_eq!(c.all_sessions(), [OLD_SESSION], "still known, so deleting the chat can clean up after it");
    // The view was told to swap the failure for the note: same message id, new role.
    let messages: Vec<&Value> = events.iter().filter(|e| e["type"] == "chat.message").map(|e| &e["message"]).collect();
    let failure = messages.iter().find(|m| m["role"] == "error").expect("the failure was announced");
    assert!(messages.iter().any(|m| m["id"] == failure["id"] && m["role"] == "system" && m["text"] == note.as_str()));
    let done: Vec<bool> = events.iter().filter(|e| e["type"] == "chat.done").map(|e| e["ok"].as_bool().unwrap()).collect();
    assert_eq!(done, [false, true]);

    // From here on the chat has no session, so every message carries the conversation — and nothing is resent twice.
    chat_send(State(st.clone()), Path(chat.clone()), say("Thanks")).await.unwrap();
    until_done(&mut rx, 1).await;
    let sent = prompts.lock().unwrap().clone();
    assert_eq!(sent.len(), 3);
    assert!(sent[2].starts_with("Earlier conversation") && sent[2].contains("ASSISTANT: Soap as well.") && sent[2].ends_with("Thanks"));
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn any_other_failure_is_shown_and_not_sent_again() {
    let (st, lib) = t::library().await;
    let chat = chat_with_history(&st).await;
    let prompts = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (p, weak) = (prompts.clone(), Arc::downgrade(&st));
    t::assistant(&st, move |info| { let (p, st) = (p.clone(), weak.upgrade().unwrap()); async move {
        p.lock().unwrap().push(info.prompt.clone());
        claude::push(&st, &info.chat_id, Msg::new("error", "Credit balance is too low")).await;
        false
    } });
    let mut rx = st.events.subscribe();
    chat_send(State(st.clone()), Path(chat.clone()), say("And what else?")).await.unwrap();
    until_done(&mut rx, 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    assert_eq!(prompts.lock().unwrap().len(), 1);
    let c = st.chats.get(&chat).await.unwrap();
    assert_eq!((c.messages.last().unwrap().role.as_str(), c.messages.last().unwrap().text.as_str()), ("error", "Credit balance is too low"));
    assert_eq!(c.session_id.as_deref(), Some(OLD_SESSION), "the session is kept: it was not the problem");
    assert!(!c.running);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn a_message_sent_as_a_lost_session_is_resent_starts_one_run_only() {
    let (st, lib) = t::library().await;
    let chat = chat_with_history(&st).await;
    let prompts = record_prompts(&st, true);
    let mut m = Msg::new("error", "The assistant could no longer open its earlier session for this conversation.");
    m.tool = Some(json!({"code": claude::SESSION_LOST}));
    claude::push(&st, &chat, m).await;
    let again = || RunSpec { chat_id: chat.clone(), prompt: "again".into(), cwd: lib.clone(), add_dirs: vec![], options: Default::default(),
        resume: None, append_system: None, extra_deny: vec![] };
    // The user's next message is on its way to a run of its own: the resend stands back, and changes nothing.
    let sending = st.chats.claim_send(&chat).unwrap();
    assert!(!resend_without_session(&st, again(), "chat").await);
    let c = st.chats.get(&chat).await.unwrap();
    assert_eq!((c.session_id.as_deref(), c.messages.last().unwrap().role.as_str()), (Some(OLD_SESSION), "error"));
    // A chat already running again is left alone as well.
    drop(sending);
    st.chats.update(&chat, |c| c.running = true).await;
    assert!(!resend_without_session(&st, again(), "chat").await);
    st.chats.update(&chat, |c| c.running = false).await;
    // With nobody else in the way it goes ahead, once.
    let mut rx = st.events.subscribe();
    assert!(resend_without_session(&st, again(), "chat").await);
    until_done(&mut rx, 1).await;
    assert_eq!(*prompts.lock().unwrap(), ["again"]);
    assert!(!st.chats.get(&chat).await.unwrap().messages.iter().any(|m| m.role == "system" && m.text.starts_with("Stopped")), "no run stopped another");
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn a_view_that_opens_mid_answer_gets_what_was_written_so_far() {
    let (st, lib) = t::library().await;
    let chat = t::chat(&st, "hello").await;
    let get = || async { chat_get(State(st.clone()), Path(chat.clone())).await.unwrap().0 };
    assert!(get().await.get("partial").is_none(), "nothing is being written in a chat at rest");
    let gate = Arc::new(tokio::sync::Notify::new());
    let (g, weak) = (gate.clone(), Arc::downgrade(&st));
    t::assistant(&st, move |info| { let (g, st) = (g.clone(), weak.upgrade().unwrap()); async move {
        st.chats.add_partial(&info.chat_id, "weighing the options", true);
        st.chats.add_partial(&info.chat_id, "The answer ", false);
        st.chats.add_partial(&info.chat_id, "so far", false);
        g.notified().await;
        claude::push(&st, &info.chat_id, Msg::new("assistant", "The answer so far, and the rest.")).await;
        true
    } });
    let mut rx = st.events.subscribe();
    chat_send(State(st.clone()), Path(chat.clone()), say("go")).await.unwrap();
    // Running, but nothing written yet: the view is told so rather than left guessing.
    for _ in 0..500 { if st.chats.partial(&chat).map_or(false, |p| p.seq == 2) { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
    let v = get().await;
    assert_eq!(v["running"], true);
    assert_eq!(v["partial"], json!({"text": "The answer so far", "seq": 2, "thinking": "weighing the options", "thinkingSeq": 1}));
    gate.notify_one();
    t::event(&mut rx, &st, "chat.done").await;
    let v = get().await;
    assert!(v.get("partial").is_none() && v["running"] == false);
    assert_eq!(v["messages"].as_array().unwrap().last().unwrap()["text"], "The answer so far, and the rest.");
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn saving_options_answers_with_what_was_saved_and_lists_mark_bypass() {
    let st = Arc::new(AppState::for_test());
    let chat = Chat::default();
    let id = chat.id.clone();
    st.chats.insert(chat).await;
    let patch = |mode: &str, confirm: bool| Json(ChatPatch { options: Some(RunOptions { permission_mode: Some(mode.into()), model: Some("opus".into()), ..Default::default() }), confirm_bypass: confirm,
        approval: confirm.then(|| crate::consent::test_chat_approval(&id)), ..Default::default() });
    let v = chat_patch(State(st.clone()), Path(id.clone()), patch("plan", false)).await.unwrap().0;
    assert_eq!((v["options"]["permissionMode"].as_str(), v["options"]["model"].as_str(), v["bypass"].as_bool()), (Some("plan"), Some("opus"), Some(false)));
    // Refused without the confirmation: nothing changes, and the list does not mark the chat.
    assert_eq!(shown(chat_patch(State(st.clone()), Path(id.clone()), patch("bypassPermissions", false)).await).code, "confirm_needed");
    assert_eq!(st.chats.meta(&id).await.unwrap().summary()["bypass"], false);
    let v = chat_patch(State(st.clone()), Path(id.clone()), patch("bypassPermissions", true)).await.unwrap().0;
    assert_eq!((v["options"]["permissionMode"].as_str(), v["bypass"].as_bool()), (Some("bypassPermissions"), Some(true)));
    let listed = chat_list(State(st.clone()), Query(Default::default())).await.unwrap().0;
    assert_eq!(listed.as_array().unwrap().iter().find(|c| c["id"] == id.as_str()).unwrap()["bypass"], true);
    // Only an ordinary chat can be in that state, whatever its stored options say.
    let mut wrap = Chat { kind: "wrap".into(), ..Default::default() };
    wrap.options.permission_mode = Some("bypassPermissions".into());
    assert_eq!(wrap.summary()["bypass"], false);
    // A wrap's summary says where it stands; a chat without such a thing says nothing.
    assert_eq!(wrap.summary()["phase"], Value::Null);
    wrap.meta = json!({"phase": "applying"});
    assert_eq!(wrap.summary()["phase"], "applying");
}

#[tokio::test]
async fn pin_and_archive_are_saved_and_sending_brings_an_archived_chat_back() {
    let st = Arc::new(AppState::for_test());
    let id = t::chat(&st, "to keep").await;
    let v = chat_patch(State(st.clone()), Path(id.clone()), Json(ChatPatch { pinned: Some(true), archived: Some(true), ..Default::default() })).await.unwrap().0;
    assert_eq!((v["pinned"].as_bool(), v["archived"].as_bool()), (Some(true), Some(true)));
    // Fields not sent stay as they were; the title the person typed stops the app naming the chat.
    let v = chat_patch(State(st.clone()), Path(id.clone()), Json(ChatPatch { title: Some("Taxes".into()), ..Default::default() })).await.unwrap().0;
    assert_eq!((v["pinned"].as_bool(), v["archived"].as_bool(), v["title"].as_str()), (Some(true), Some(true), Some("Taxes")));
    assert!(!st.chats.get(&id).await.unwrap().title_is_auto());
    let v = chat_patch(State(st.clone()), Path(id.clone()), Json(ChatPatch { archived: Some(false), ..Default::default() })).await.unwrap().0;
    assert_eq!((v["pinned"].as_bool(), v["archived"].as_bool()), (Some(true), Some(false)));
}

#[tokio::test]
async fn new_chat_reuses_the_empty_one_instead_of_piling_up() {
    let st = Arc::new(AppState::for_test());
    let by_user = || Json(NewChat { by_user: true, ..Default::default() });
    let first = chat_create(State(st.clone()), by_user()).await.unwrap().0;
    let again = chat_create(State(st.clone()), by_user()).await.unwrap().0;
    assert_eq!(first["id"], again["id"]);
    assert_eq!(st.chats.list().await.len(), 1);
    // A different group gets its own; one started for the person (not by hand) is always new.
    let other = chat_create(State(st.clone()), Json(NewChat { by_user: true, project_id: Some("p1".into()), ..Default::default() })).await.unwrap().0;
    assert_ne!(first["id"], other["id"]);
    let pre = chat_create(State(st.clone()), Json(NewChat { by_user: false, ..Default::default() })).await.unwrap().0;
    assert_ne!(first["id"], pre["id"]);
    // Once something is said in it, or it is pinned, the next New chat is a new one.
    let id = first["id"].as_str().unwrap().to_string();
    st.chats.update(&id, |c| c.messages.push(crate::chats::Msg::new("user", "hello"))).await;
    let next = chat_create(State(st.clone()), by_user()).await.unwrap().0;
    assert_ne!(next["id"], first["id"]);
    st.chats.update(next["id"].as_str().unwrap(), |c| c.pinned = true).await;
    let after = chat_create(State(st.clone()), by_user()).await.unwrap().0;
    assert_ne!(after["id"], next["id"]);
}

#[tokio::test]
async fn bulk_delete_and_the_clean_up_setting_work_over_http() {
    let st = Arc::new(AppState::for_test());
    let port = serve(&st).await;
    let (a, b, running) = (t::chat(&st, "a").await, t::chat(&st, "b").await, t::chat(&st, "c").await);
    st.chats.update(&running, |c| c.running = true).await;
    let body = json!({"ids": [a, b, running, "../x", uuid::Uuid::new_v4().to_string()]}).to_string();
    let (status, out) = http(port, "POST", "/api/chats/bulk-delete", body.as_bytes()).await;
    assert_eq!(status, 200, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!((v["deleted"].as_u64(), v["skipped"].as_u64()), (Some(2), Some(3)));
    assert_eq!(st.chats.list().await.len(), 1);
    // The clean-up: off by default, what it would do is shown before it is switched on.
    let old = {
        let mut c = Chat::default();
        c.messages.push(crate::chats::Msg::new("user", "old")); c.updated = chrono::Utc::now() - chrono::Duration::days(200); c.wrapped_at = Some(chrono::Utc::now() - chrono::Duration::days(199));
        let id = c.id.clone(); st.chats.insert(c).await; id
    };
    let (status, out) = http(port, "GET", "/api/chats/retention?days=90", b"").await;
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!((status, v["days"].as_u64(), v["preview"]["total"].as_u64(), v["preview"]["chats"].as_u64()), (200, Some(0), Some(1), Some(1)), "{out}");
    let (status, _) = http(port, "PUT", "/api/chats/retention", br#"{"days": 3}"#).await;
    assert_eq!(status, 400);
    assert!(st.chats.get(&old).await.is_some());
    let (status, out) = http(port, "PUT", "/api/chats/retention", br#"{"days": 90}"#).await;
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!((status, v["days"].as_u64(), v["removed"].as_u64()), (200, Some(90), Some(1)), "{out}");
    assert!(st.chats.get(&old).await.is_none());
    // A general settings save cannot switch it on or off.
    let mut s = st.settings.read().await.clone();
    let before = s.clone();
    s.chat_retention_days = 0;
    s.sanitize_update(&before).unwrap();
    assert_eq!(s.chat_retention_days, 90);
}

#[tokio::test]
async fn the_first_message_names_a_chat_at_a_word_and_talking_brings_an_archived_chat_back() {
    let (st, lib) = t::library().await;
    t::assistant(&st, |_| async { true });
    let id = chat_create(State(st.clone()), Json(NewChat { by_user: true, ..Default::default() })).await.unwrap().0["id"].as_str().unwrap().to_string();
    st.chats.update(&id, |c| c.archived = true).await;
    let long = "Please help me plan the migration of our whole accounting system to the new provider by March, with a budget";
    let mut rx = st.events.subscribe();
    chat_send(State(st.clone()), Path(id.clone()), Json(SendBody { text: long.into(), ..Default::default() })).await.unwrap();
    t::event(&mut rx, &st, "chat.done").await;
    let c = st.chats.get(&id).await.unwrap();
    assert!(c.title.ends_with("accounting…"), "{}", c.title);
    assert!(!c.archived);
    // A second message does not rename it.
    chat_send(State(st.clone()), Path(id.clone()), Json(SendBody { text: "and another thing".into(), ..Default::default() })).await.unwrap();
    assert!(st.chats.get(&id).await.unwrap().title.ends_with("accounting…"));
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn moving_a_chat_back_to_the_library_clears_its_folder() {
    // What the chat's "Move to" menu sends: a folder, then null for "Library".
    let body = |v: Value| Json(serde_json::from_value::<ChatPatch>(v).unwrap());
    assert_eq!(serde_json::from_value::<ChatPatch>(json!({"title": "x"})).unwrap().project_id, None, "left out: unchanged");
    let st = Arc::new(AppState::for_test());
    let chat = Chat::default();
    let id = chat.id.clone();
    st.chats.insert(chat).await;
    let v = chat_patch(State(st.clone()), Path(id.clone()), body(json!({"projectId": "p1"}))).await.unwrap().0;
    assert_eq!(v["projectId"], "p1");
    let v = chat_patch(State(st.clone()), Path(id.clone()), body(json!({"title": "Renamed"}))).await.unwrap().0;
    assert_eq!(v["projectId"], "p1", "a patch without projectId leaves the folder alone");
    let v = chat_patch(State(st.clone()), Path(id.clone()), body(json!({"projectId": null}))).await.unwrap().0;
    assert_eq!(v["projectId"], Value::Null);
    assert_eq!(st.chats.meta(&id).await.unwrap().summary()["projectId"], Value::Null);
}

#[tokio::test]
async fn an_attached_office_file_is_named_with_its_text_version() {
    let (st, lib) = t::library().await;
    let prompts = record_prompts(&st, true);
    let chat = t::chat(&st, "the letter").await;
    std::fs::create_dir_all(lib.join("deals")).unwrap();
    std::fs::write(lib.join("deals/offer.rtf"), b"{\\rtf1 We offer 300.\\par}").unwrap();
    let mut rx = st.events.subscribe();
    let body = SendBody { text: "What does the letter say?".into(), mentions: vec!["deals/offer.rtf".into()],
        attachments: vec![att("letter.rtf", b"{\\rtf1 Dear Sara,\\par the total is 950.\\par}"), att("sheet.xlsx", b"not a workbook"), att("notes.txt", b"plain")], ..Default::default() };
    chat_send(State(st.clone()), Path(chat.clone()), Json(body)).await.unwrap();
    t::event(&mut rx, &st, "chat.done").await;
    let prompt = prompts.lock().unwrap()[0].clone();
    let mine = crate::run_files::folder(&paths::home(), &chat).unwrap();
    let (copy, text) = (mine.join("files/letter.rtf"), mine.join("text/letter.rtf.txt"));
    // The file itself and, said to be the one to read, its text version: both inside the chat's own folder of copies.
    assert!(prompt.contains(&format!("- {}\n  (a document, which the Read tool cannot open: read its text version instead, {})\n", copy.display(), text.display())), "{prompt}");
    assert!(std::fs::read_to_string(&text).unwrap().contains("Dear Sara,\nthe total is 950."));
    // One that could not be turned into text is still sent, and said to be unreadable; a plain file is listed as before.
    assert!(prompt.contains(&format!("- {}\n  (a spreadsheet, which the Read tool cannot open, and no text version of it could be made here: the file is not what its name says.", mine.join("files/sheet.xlsx").display())), "{prompt}");
    assert!(prompt.contains(&format!("- {}\n", mine.join("files/notes.txt").display())) && !prompt.contains("notes.txt.txt"), "{prompt}");
    // The library file referred to: where it is, and its text version in the chat's folder.
    let lib_text = prompt.lines().find(|l| l.contains("offer.rtf.txt")).unwrap_or_else(|| panic!("{prompt}"));
    let path = lib_text.rsplit("instead, ").next().unwrap().trim_end_matches(')');
    assert!(std::path::Path::new(path).starts_with(mine.join("library")) && std::fs::read_to_string(path).unwrap().contains("We offer 300."), "{lib_text}");
    assert!(prompt.contains(&format!("- {}\n  (a document", lib.join("deals/offer.rtf").display())) || prompt.contains("deals/offer.rtf\n  (a document"), "{prompt}");
    // The next message is told where the earlier files' text versions are.
    chat_send(State(st.clone()), Path(chat.clone()), say("And the total?")).await.unwrap();
    t::event(&mut rx, &st, "chat.done").await;
    let next = prompts.lock().unwrap()[1].clone();
    assert!(next.contains(&format!("read their text versions in {}, each named after its file with .txt added.", mine.join("text").display())), "{next}");
}
