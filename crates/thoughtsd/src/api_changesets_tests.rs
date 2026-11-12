//! Tests for what an answer changed in the library, through the chat and library endpoints (FEAT-06).

use super::*;
use crate::wraps::testutil as t;

fn shown(r: R) -> crate::tr::UserErr {
    match r { Ok(v) => panic!("expected an error, got {}", v.0), Err(e) => crate::tr::classify_in(false, &e.0) }
}
fn no_query() -> Query<HashMap<String, String>> { Query(HashMap::new()) }
fn save(path: &str, content: &str) -> Json<WriteFile> { Json(WriteFile { path: path.into(), content: content.into(), base_hash: None }) }
fn changes_msg(c: &Chat) -> Option<Msg> { c.messages.iter().find(|m| m.role == "changes").cloned() }
/// The newest history entries: (subject, files).
fn top(lib: &std::path::Path, n: usize) -> Vec<(String, Vec<String>)> {
    t::sh(lib, &["-c", "core.quotePath=false", "log", &format!("-{n}"), "--name-only", "--format=%x1e%s"]).split('\x1e').filter(|r| !r.trim().is_empty())
        .map(|r| { let mut l = r.lines().filter(|l| !l.is_empty()); (l.next().unwrap().to_string(), l.map(String::from).collect()) }).collect()
}
fn op(st: &Arc<AppState>, chat: &str, msg: &str, what: &str) -> impl std::future::Future<Output = R> {
    run_changes_op(State(st.clone()), Path((chat.to_string(), msg.to_string(), what.to_string())))
}

#[tokio::test]
async fn an_answer_that_edits_two_notes_while_the_user_saves_another_keeps_its_own_changes_apart() {
    let (st, lib) = t::library().await;
    let chat = t::chat(&st, "Tidy my notes").await;
    st.chats.update(&chat, |c| c.options.permission_mode = Some("acceptEdits".into())).await;
    let gate = Arc::new(tokio::sync::Notify::new());
    let (g, lib2) = (gate.clone(), lib.clone());
    t::assistant(&st, move |_| { let (g, lib) = (g.clone(), lib2.clone()); async move {
        t::write(&lib, "TODO.md", "# Todo\n\n- [ ] call the bank\n- [ ] order the sign\n");
        t::write(&lib, "notes/plan.md", "# Plan\n\nOpen in May.\n");
        g.notified().await;
        true
    } });
    let mut rx = st.events.subscribe();
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "go".into(), ..Default::default() })).await.unwrap();
    for _ in 0..500 { if lib.join("notes/plan.md").exists() { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
    // Meanwhile the user saves another note in the app.
    write_file(State(st.clone()), no_query(), save("README.md", "my library, renamed by me\n")).await.unwrap();
    gate.notify_one();
    t::event(&mut rx, &st, "chat.done").await;

    let c = st.chats.get(&chat).await.unwrap();
    let m = changes_msg(&c).expect("a list of what the answer changed");
    let tool = m.tool.clone().unwrap();
    let files: Vec<&str> = tool["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(files, ["TODO.md", "notes/plan.md"]);
    assert_eq!(tool["counts"], json!({"added": 1, "edited": 1, "moved": 0, "deleted": 0}));
    assert_eq!(tool["others"], json!(["README.md"]), "the user's save is said, and not counted");
    // Its history entry holds its two files only; the user's save is still the user's, waiting.
    let h = top(&lib, 2);
    assert!(h[0].0.starts_with("assistant: "), "{h:?}");
    assert_eq!(h[0].1, ["TODO.md", "notes/plan.md"]);
    assert!(t::sh(&lib, &["status", "--porcelain"]).contains("README.md"));
    // The view shows the changed lines, document by document.
    let v = run_changes(State(st.clone()), Path((chat.clone(), m.id.clone()))).await.unwrap().0;
    assert_eq!(v["total"], 2);
    let todo = v["files"].as_array().unwrap().iter().find(|f| f["path"] == "TODO.md").unwrap().clone();
    assert!(todo["hunks"][0]["lines"].as_array().unwrap().contains(&json!(["+", "- [ ] order the sign"])));
    assert_eq!(v["others"], json!(["README.md"]));

    // Undo takes back those two files and nothing else.
    let r = op(&st, &chat, &m.id, "undo").await.unwrap().0;
    assert_eq!(t::read(&lib, "TODO.md"), "# Todo\n\n- [ ] call the bank\n");
    assert!(!lib.join("notes/plan.md").exists() && !lib.join("notes").exists());
    assert_eq!(t::read(&lib, "README.md"), "my library, renamed by me\n");
    assert!(!r["message"]["tool"]["undone"].is_null());
    let h = top(&lib, 2);
    assert_eq!(h[0].0, format!("undo: {}", tool["subject"].as_str().unwrap()));
    assert_eq!(h[0].1, ["TODO.md", "notes/plan.md"]);
    assert_eq!((h[1].0.as_str(), h[1].1.as_slice()), ("edit: changes before undoing an assistant's changes", ["README.md".to_string()].as_slice()));
    assert_eq!(shown(op(&st, &chat, &m.id, "undo").await).code, "conflict", "undone once");
    // …and is itself undone with "Put them back".
    op(&st, &chat, &m.id, "redo").await.unwrap();
    assert_eq!(t::read(&lib, "TODO.md"), "# Todo\n\n- [ ] call the bank\n- [ ] order the sign\n");
    assert_eq!(t::read(&lib, "notes/plan.md"), "# Plan\n\nOpen in May.\n");
    assert!(st.chats.get(&chat).await.unwrap().messages.iter().find(|x| x.id == m.id).unwrap().tool.as_ref().unwrap()["undone"].is_null());
    assert!(top(&lib, 1)[0].0.starts_with("edit: put back what the assistant changed"));
    // A later change to the same lines: Undo refuses, names the document and changes nothing.
    write_file(State(st.clone()), no_query(), save("TODO.md", "# Todo\n\n- [ ] call the bank\n- [x] order the sign\n")).await.unwrap();
    let e = shown(op(&st, &chat, &m.id, "undo").await);
    assert_eq!((e.status, e.code), (409, "conflict"));
    assert!(e.message.contains("TODO.md") && e.message.contains("same lines"), "{}", e.message);
    assert_eq!(t::read(&lib, "TODO.md"), "# Todo\n\n- [ ] call the bank\n- [x] order the sign\n");
    assert!(lib.join("notes/plan.md").exists());
    // Never on an add-on's behalf: the changes show documents' contents.
    assert!(!plugin_scope_allowed(&format!("/api/chats/{chat}/changes/{}", m.id), "scope=plugin"));
    assert!(!plugin_scope_allowed(&format!("/api/chats/{chat}/changes/{}/undo", m.id), "scope=plugin"));
    assert!(plugin_scope_allowed(&format!("/api/chats/{chat}/changes/{}", m.id), ""));
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn nothing_is_undone_while_the_assistant_works_and_an_answer_that_changed_nothing_lists_nothing() {
    let (st, lib) = t::library().await;
    let chat = t::chat(&st, "Just a question").await;
    st.chats.update(&chat, |c| c.options.permission_mode = Some("acceptEdits".into())).await;
    let before = t::sh(&lib, &["rev-parse", "HEAD"]);
    t::assistant(&st, |_| async { true });
    let mut rx = st.events.subscribe();
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "what is on my list?".into(), ..Default::default() })).await.unwrap();
    t::event(&mut rx, &st, "chat.done").await;
    assert!(changes_msg(&st.chats.get(&chat).await.unwrap()).is_none());
    assert_eq!(t::sh(&lib, &["rev-parse", "HEAD"]), before, "no empty history entry");

    // A second chat changes a note; while a third is at work, its Undo waits.
    let other = t::chat(&st, "Write it down").await;
    let gate = Arc::new(tokio::sync::Notify::new());
    let (g, lib2) = (gate.clone(), lib.clone());
    t::assistant(&st, move |info| { let (g, lib) = (g.clone(), lib2.clone()); async move {
        if info.prompt.contains("WRITE-X") { t::write(&lib, "notes/x.md", "x\n"); } else { g.notified().await; }
        true
    } });
    chat_send(State(st.clone()), Path(other.clone()), Json(SendBody { text: "WRITE-X please".into(), ..Default::default() })).await.unwrap();
    t::event(&mut rx, &st, "chat.done").await;
    let m = changes_msg(&st.chats.get(&other).await.unwrap()).unwrap();
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "wait".into(), ..Default::default() })).await.unwrap();
    for _ in 0..500 { if st.run_active_in(&lib) { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
    assert_eq!(shown(op(&st, &other, &m.id, "undo").await).code, "busy");
    assert!(lib.join("notes/x.md").exists());
    gate.notify_one();
    t::event(&mut rx, &st, "chat.done").await;
    op(&st, &other, &m.id, "undo").await.unwrap();
    assert!(!lib.join("notes/x.md").exists());
    assert_eq!(shown(op(&st, &other, "not-a-message", "undo").await).status, 404);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}
