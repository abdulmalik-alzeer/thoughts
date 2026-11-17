//! Tests for editing a sent message and branching a chat (`chat_edit`, `chat_branch`, `branch.rs`).

use super::*;
use crate::wraps::testutil as t;

const SESSION: &str = "5d0c3a1e-2b4f-4c6d-8e9f-0a1b2c3d4e5f";
const FORK_HELP: &str = "Options:\n  -p, --print                 Print response and exit\n  -r, --resume [value]        Resume a conversation\n  --fork-session              When resuming, create a new session ID\n";

/// What the stand-in assistant was asked: the prompt, the session it was to continue, and whether from a copy.
type Asked = Arc<std::sync::Mutex<Vec<(String, Option<String>, bool)>>>;

fn record(st: &Arc<AppState>) -> Asked {
    let asked: Asked = Default::default();
    let (a, weak) = (asked.clone(), Arc::downgrade(st));
    t::assistant(st, move |info| { let (a, st) = (a.clone(), weak.upgrade().unwrap()); async move {
        a.lock().unwrap().push((info.prompt.clone(), info.resume.clone(), info.fork));
        claude::push(&st, &info.chat_id, Msg::new("assistant", "New answer.")).await;
        true
    } });
    asked
}

async fn done(rx: &mut tokio::sync::broadcast::Receiver<Value>, chat: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop { let e = rx.recv().await.unwrap(); if e["type"] == "chat.done" && e["chatId"] == chat { return; } }
    }).await.expect("the run did not finish");
}

/// A chat of two exchanges with a session, the last question with a file attached.
async fn conversation(st: &AppState) -> (String, String) {
    let chat = t::chat(st, "What should the shop sell?").await;
    let dir = paths::attachments_dir().join(&chat);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("prices.txt"), "ten").unwrap();
    let file = dir.join("prices.txt").to_string_lossy().to_string();
    st.chats.update(&chat, |c| {
        c.messages.push(Msg::new("assistant", "Candles."));
        let mut q = Msg::new("user", "And the price?");
        q.attachments = vec![file.clone()];
        q.tool = Some(json!({"mentions": ["ideas/shop/README.md"]}));
        c.messages.push(q);
        c.messages.push(Msg::new("assistant", "Ten riyals."));
        c.session_id = Some(SESSION.into());
        c.cost_usd = 0.5;
    }).await;
    (chat, file)
}

fn ids(c: &Chat) -> Vec<(String, String)> { c.messages.iter().map(|m| (m.role.clone(), m.text.clone())).collect() }

#[tokio::test]
async fn editing_the_last_message_replaces_it_and_its_answer_and_sends_it_again() {
    let (st, lib) = t::library().await;
    let (chat, file) = conversation(&st).await;
    let asked = record(&st);
    let mut rx = st.events.subscribe();
    let last = st.chats.get(&chat).await.unwrap().messages[2].id.clone();
    // An earlier message is never edited in place.
    let first = st.chats.get(&chat).await.unwrap().messages[0].id.clone();
    let e = chat_edit(State(st.clone()), Path(chat.clone()), Json(EditBody { message_id: first, text: "x".into(), ..Default::default() })).await;
    assert_eq!(crate::tr::classify_in(false, &e.unwrap_err().0).code, "conflict");
    assert!(chat_edit(State(st.clone()), Path(chat.clone()), Json(EditBody { message_id: last.clone(), text: "  ".into(), ..Default::default() })).await.is_err());

    chat_edit(State(st.clone()), Path(chat.clone()), Json(EditBody { message_id: last.clone(), text: "And the price of soap?".into(),
        keep_attachments: vec!["prices.txt".into(), "/etc/passwd".into()], mentions: vec!["ideas/shop/README.md".into()], ..Default::default() })).await.unwrap();
    done(&mut rx, &chat).await;
    let c = st.chats.get(&chat).await.unwrap();
    assert_eq!(ids(&c), [("user".into(), "What should the shop sell?".into()), ("assistant".into(), "Candles.".into()),
        ("user".into(), "And the price of soap?".into()), ("assistant".into(), "New answer.".into())]);
    // The file goes again (only the message's own, never another one named), and so does the mention.
    assert_eq!(c.messages[2].attachments, [file.clone()]);
    assert!(std::path::Path::new(&file).is_file());
    assert_eq!(c.messages[2].tool.as_ref().unwrap()["mentions"][0], "ideas/shop/README.md");
    // The old session holds the old exchange: left behind (and still known, for deleting), the
    // conversation before the message passed on instead.
    let (prompt, resume, fork) = asked.lock().unwrap()[0].clone();
    assert_eq!((resume, fork), (None, false));
    assert!(prompt.starts_with("This conversation continues an earlier one"), "{prompt}");
    assert!(prompt.contains("USER: What should the shop sell?") && prompt.contains("ASSISTANT: Candles."));
    assert!(!prompt.contains("Ten riyals") && !prompt.contains("And the price?"), "the replaced exchange is not passed on");
    assert!(prompt.contains("And the price of soap?") && prompt.contains("prices.txt"));
    assert!(c.all_sessions().contains(&SESSION.to_string()));
    assert_eq!(c.cost_usd, 0.5, "what was spent stays counted, once");
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn editing_stops_an_answer_being_written_first() {
    let (st, lib) = t::library().await;
    let chat = t::chat(&st, "Write a long essay").await;
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let r = runs.clone();
    t::assistant(&st, move |_| { let r = r.clone(); async move {
        if r.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 { loop { tokio::time::sleep(std::time::Duration::from_millis(5)).await; } }
        true
    } });
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "Write a long essay".into(), ..Default::default() })).await.unwrap();
    let n = |runs: &Arc<std::sync::atomic::AtomicUsize>| runs.load(std::sync::atomic::Ordering::SeqCst);
    for _ in 0..400 { if n(&runs) == 1 { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
    assert!(st.chats.get(&chat).await.unwrap().running, "the first answer is being written");
    let last = st.chats.get(&chat).await.unwrap().messages.iter().rev().find(|m| m.role == "user").unwrap().id.clone();
    chat_edit(State(st.clone()), Path(chat.clone()), Json(EditBody { message_id: last, text: "Write a short one".into(), ..Default::default() })).await.unwrap();
    for _ in 0..400 { if n(&runs) == 2 && !st.chats.get(&chat).await.unwrap().running { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
    let c = st.chats.get(&chat).await.unwrap();
    let users: Vec<&str> = c.messages.iter().filter(|m| m.role == "user").map(|m| m.text.as_str()).collect();
    assert_eq!(users, ["Write a long essay", "Write a short one"], "the message that was being answered is the one replaced");
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 2);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn a_branch_at_the_last_answer_continues_a_copy_of_the_session_and_leaves_the_original_alone() {
    crate::branch::TEST_HELP.with(|h| *h.borrow_mut() = Some(FORK_HELP.into()));
    let (st, lib) = t::library().await;
    let (chat, file) = conversation(&st).await;
    let asked = record(&st);
    let spent = st.month_cost().await;
    let before = st.chats.get(&chat).await.unwrap();
    let at = before.messages[3].id.clone();
    let v = chat_branch(State(st.clone()), Path(chat.clone()), Json(BranchBody { message_id: at.clone(), ..Default::default() })).await.unwrap().0;
    assert_eq!(v["how"], "fork");
    let bid = v["id"].as_str().unwrap().to_string();
    let b = st.chats.get(&bid).await.unwrap();
    assert_eq!(b.title, format!("{} (branch)", before.title));
    assert_eq!(ids(&b), ids(&before));
    assert_eq!((b.session_id.as_deref(), b.cost_usd, b.project_id.clone()), (None, 0.0, before.project_id.clone()));
    assert_eq!(st.month_cost().await, spent, "nothing is counted again");
    assert!(!b.meta_only().is_unwrapped(), "what it carries is the original's to save");
    // Its own copy of the file, in its own folder.
    let copy = b.messages[2].attachments[0].clone();
    assert_ne!(copy, file);
    assert!(copy.contains(&bid) && std::fs::read_to_string(&copy).unwrap() == "ten");
    // The original is as it was, and lists the branch.
    let o = st.chats.get(&chat).await.unwrap();
    assert_eq!((ids(&o), o.session_id.as_deref()), (ids(&before), Some(SESSION)));
    assert_eq!(o.meta["branches"][0]["id"], bid.as_str());

    // Its first message continues a copy of the original's session; it is not passed on again.
    let mut rx = st.events.subscribe();
    chat_send(State(st.clone()), Path(bid.clone()), Json(SendBody { text: "And for soap?".into(), ..Default::default() })).await.unwrap();
    done(&mut rx, &bid).await;
    let (prompt, resume, fork) = asked.lock().unwrap()[0].clone();
    assert_eq!((resume.as_deref(), fork), (Some(SESSION), true));
    // Only the new words (and where this chat's own copies of the earlier files are, same names).
    assert!(prompt.starts_with("And for soap?") && !prompt.contains("USER:"), "{prompt}");
    assert_eq!(ids(&st.chats.get(&chat).await.unwrap()), ids(&before), "the original is not changed by the branch");
    assert!(st.chats.get(&bid).await.unwrap().meta_only().is_unwrapped(), "what is said in the branch is the branch's to save");

    // Deleting the original takes neither the branch nor its files; the branch still says where it came from.
    chat_delete(State(st.clone()), Path(chat.clone())).await.unwrap();
    assert!(std::fs::read_to_string(&copy).is_ok());
    assert_eq!(st.chats.get(&bid).await.unwrap().meta["branch"]["fromTitle"], before.title.as_str());
    crate::branch::TEST_HELP.with(|h| *h.borrow_mut() = None);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn a_branch_from_an_earlier_point_or_an_original_that_moved_on_passes_the_conversation_on_as_text() {
    crate::branch::TEST_HELP.with(|h| *h.borrow_mut() = Some(FORK_HELP.into()));
    let (st, lib) = t::library().await;
    let (chat, _) = conversation(&st).await;
    let asked = record(&st);
    let msgs = st.chats.get(&chat).await.unwrap().messages;
    // An earlier point.
    let v = chat_branch(State(st.clone()), Path(chat.clone()), Json(BranchBody { message_id: msgs[1].id.clone(), ..Default::default() })).await.unwrap().0;
    assert_eq!(v["how"], "replay");
    let early = v["id"].as_str().unwrap().to_string();
    let mut rx = st.events.subscribe();
    chat_send(State(st.clone()), Path(early.clone()), Json(SendBody { text: "What about soap?".into(), ..Default::default() })).await.unwrap();
    done(&mut rx, &early).await;
    let (prompt, resume, fork) = asked.lock().unwrap()[0].clone();
    assert_eq!((resume, fork), (None, false));
    assert!(prompt.starts_with("This conversation continues an earlier one") && prompt.contains("ASSISTANT: Candles.") && !prompt.contains("Ten riyals"), "{prompt}");
    assert!(prompt.ends_with("What about soap?"));

    // At the last answer, but the original goes on before the branch says anything: as text after all.
    let v = chat_branch(State(st.clone()), Path(chat.clone()), Json(BranchBody { message_id: msgs[3].id.clone(), ..Default::default() })).await.unwrap().0;
    assert_eq!(v["how"], "fork");
    let late = v["id"].as_str().unwrap().to_string();
    st.chats.update(&chat, |c| { c.messages.push(Msg::new("user", "More?")); c.messages.push(Msg::new("assistant", "Soap.")); }).await;
    chat_send(State(st.clone()), Path(late.clone()), Json(SendBody { text: "Then?".into(), ..Default::default() })).await.unwrap();
    done(&mut rx, &late).await;
    let (prompt, resume, _) = asked.lock().unwrap()[1].clone();
    assert!(resume.is_none() && prompt.contains("ASSISTANT: Ten riyals.") && !prompt.contains("Soap."));
    assert_eq!(st.chats.get(&late).await.unwrap().meta["branch"]["how"], "replay", "the branch says how it went");
    crate::branch::TEST_HELP.with(|h| *h.borrow_mut() = None);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn editing_an_earlier_message_sends_it_in_a_branch_and_leaves_the_chat_as_it_is() {
    let (st, lib) = t::library().await;
    let (chat, file) = conversation(&st).await;
    let asked = record(&st);
    let before = st.chats.get(&chat).await.unwrap();
    let mut rx = st.events.subscribe();
    let v = chat_branch(State(st.clone()), Path(chat.clone()), Json(BranchBody { message_id: before.messages[0].id.clone(),
        text: Some("What should a bakery sell?".into()), ..Default::default() })).await.unwrap().0;
    let bid = v["id"].as_str().unwrap().to_string();
    done(&mut rx, &bid).await;
    let b = st.chats.get(&bid).await.unwrap();
    assert_eq!(ids(&b), [("user".into(), "What should a bakery sell?".into()), ("assistant".into(), "New answer.".into())]);
    assert_eq!(asked.lock().unwrap()[0].0, "What should a bakery sell?", "nothing came before it");
    assert_eq!(ids(&st.chats.get(&chat).await.unwrap()), ids(&before));

    // The second question edited, its file kept: the branch has its own copy, and the first exchange as text.
    let v = chat_branch(State(st.clone()), Path(chat.clone()), Json(BranchBody { message_id: before.messages[2].id.clone(),
        text: Some("And the price in dollars?".into()), keep_attachments: vec!["prices.txt".into()], ..Default::default() })).await.unwrap().0;
    let bid = v["id"].as_str().unwrap().to_string();
    done(&mut rx, &bid).await;
    let b = st.chats.get(&bid).await.unwrap();
    assert_eq!(b.messages.len(), 4);
    assert_eq!(b.messages[2].text, "And the price in dollars?");
    assert!(b.messages[2].attachments[0].contains(&bid) && b.messages[2].attachments[0] != file);
    let p = asked.lock().unwrap()[1].0.clone();
    assert!(p.contains("USER: What should the shop sell?") && !p.contains("Ten riyals") && p.contains("prices.txt"));

    // A branch that cannot be sent leaves nothing behind.
    let n = st.chats.list().await.len();
    assert!(chat_branch(State(st.clone()), Path(chat.clone()), Json(BranchBody { message_id: before.messages[0].id.clone(), text: Some(" ".into()), ..Default::default() })).await.is_err());
    let bad = BranchBody { message_id: before.messages[0].id.clone(), text: Some("x".into()), options: Some(RunOptions { model: Some("bad model;".into()), ..Default::default() }), ..Default::default() };
    assert!(chat_branch(State(st.clone()), Path(chat.clone()), Json(bad)).await.is_err());
    assert_eq!(st.chats.list().await.len(), n);
    assert_eq!(st.chats.get(&chat).await.unwrap().meta["branches"].as_array().unwrap().len(), 2);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}
