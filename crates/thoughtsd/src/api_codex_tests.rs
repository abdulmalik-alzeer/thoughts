//! Tests for the Codex parts of the chat endpoints: which program a new chat gets, moving a chat to
//! the other program, and what its next message passes on.

use super::*;
use crate::assistant::Program;
use crate::wraps::testutil as t;

fn shown(r: R) -> crate::tr::UserErr {
    match r { Ok(v) => panic!("expected an error, got {}", v.0), Err(e) => crate::tr::classify_in(false, &e.0) }
}

/// A stand-in file where the Codex program would be (nothing here ever starts it: runs go to the test hook).
fn fake_codex(st: &AppState) -> impl std::future::Future<Output = ()> + '_ {
    let dir = std::env::temp_dir().join(format!("thoughts-fake-codex-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("codex"), "#!/bin/sh\nexit 1\n").unwrap();
    async move { st.settings.write().await.ai.codex.binary = Some(dir.join("codex").to_string_lossy().into()); }
}

fn patch(provider: &str) -> Json<ChatPatch> { Json(ChatPatch { provider: Some(provider.into()), ..Default::default() }) }

#[tokio::test]
async fn new_chats_take_the_program_in_settings_with_its_own_model() {
    let st = Arc::new(AppState::for_test());
    { let mut s = st.settings.write().await; s.ai.primary = "codex".into(); s.ai.codex.model = Some("gpt-5".into()); s.ai.claude.model = Some("opus".into()); }
    let c = chat_create(State(st.clone()), Json(NewChat { by_user: true, ..Default::default() })).await.unwrap().0;
    assert_eq!((c["provider"].as_str(), c["options"]["model"].as_str()), (Some("codex"), Some("gpt-5")));
    // Asked for by name; a name this version does not know is the one in Settings.
    let c = chat_create(State(st.clone()), Json(NewChat { provider: Some("claude".into()), title: Some("x".into()), ..Default::default() })).await.unwrap().0;
    assert_eq!((c["provider"].as_str(), c["options"]["model"].as_str()), (Some("claude"), Some("opus")));
    let c = chat_create(State(st.clone()), Json(NewChat { provider: Some("gemini".into()), title: Some("y".into()), ..Default::default() })).await.unwrap().0;
    assert_eq!(c["provider"], "codex");
}

#[tokio::test]
async fn a_chat_moves_to_codex_and_its_next_message_carries_the_conversation() {
    let (st, _lib) = t::library().await;
    let chat = t::chat(&st, "What should the shop sell?").await;
    st.chats.update(&chat, |c| { c.messages.push(Msg::new("assistant", "Candles, to start with.")); c.session_id = Some("claude-session-1".into()); c.options.model = Some("opus".into()); }).await;
    // Not installed: said plainly, and nothing changes.
    let e = shown(chat_patch(State(st.clone()), Path(chat.clone()), patch("codex")).await);
    assert_eq!(e.message, crate::tr::text_in(false, "codex.missing", &[]));
    assert_eq!(st.chats.get(&chat).await.unwrap().provider, "claude");
    assert_eq!(shown(chat_patch(State(st.clone()), Path(chat.clone()), patch("gpt")).await).status, 422);
    fake_codex(&st).await;
    let mut rx = st.events.subscribe();
    let v = chat_patch(State(st.clone()), Path(chat.clone()), patch("codex")).await.unwrap().0;
    assert_eq!((v["provider"].as_str(), v["handoff"].as_str()), (Some("codex"), Some("replay")));
    let c = st.chats.get(&chat).await.unwrap();
    // The Claude session stays behind (and with the chat's record, for deleting), and so does its model.
    assert_eq!((c.session_id.as_deref(), c.options.model.as_deref()), (None, None));
    assert!(c.session_ids.contains(&"claude-session-1".to_string()));
    let note = c.messages.last().unwrap();
    assert_eq!((note.role.as_str(), note.text.as_str()), ("system", crate::tr::text_in(false, "handoff.replay", &[("to", "Codex")]).as_str()));
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|e| e["type"] == "chat.message" && e["message"]["tool"]["code"] == "handoff"));
    // The next message passes the whole conversation on, and goes to Codex.
    let seen = Arc::new(std::sync::Mutex::new(Vec::<(String, Program)>::new()));
    let log = seen.clone();
    t::assistant(&st, move |info| { let log = log.clone(); async move {
        // Codex has no way to put a request to the user: its runs never get the approval tool.
        assert!(info.prompt_token.is_none() && info.ask.is_empty(), "a Codex run is never given the approval prompt");
        log.lock().unwrap().push((info.prompt.clone(), info.program)); true
    } });
    chat_send(State(st.clone()), Path(chat.clone()), Json(SendBody { text: "And what else?".into(), ..Default::default() })).await.unwrap();
    for _ in 0..400 { if !seen.lock().unwrap().is_empty() { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
    let (prompt, program) = seen.lock().unwrap()[0].clone();
    assert_eq!(program, Program::Codex);
    assert!(prompt.starts_with("This conversation was moved to you from Claude Code") && prompt.ends_with("And what else?"), "{prompt}");
    assert!(prompt.contains("ASSISTANT: Candles, to start with.") && prompt.matches("And what else?").count() == 1);
    // Moving back and forth: the same chat, only an ordinary chat, never while it answers.
    st.chats.update(&chat, |c| c.running = true).await;
    assert_eq!(shown(chat_patch(State(st.clone()), Path(chat.clone()), patch("claude")).await).status, 409);
    st.chats.update(&chat, |c| c.running = false).await;
    let w = t::wrap_ready(&st, &[chat.clone()]).await;
    assert_eq!(shown(chat_patch(State(st.clone()), Path(w), patch("codex")).await).status, 403);
    // Already there: nothing to do, nothing said.
    let n = st.chats.get(&chat).await.unwrap().messages.len();
    let v = chat_patch(State(st.clone()), Path(chat.clone()), patch("codex")).await.unwrap().0;
    assert!(v["handoff"].is_null() && st.chats.get(&chat).await.unwrap().messages.len() == n);
}

#[tokio::test]
async fn a_codex_chat_asks_codex_whether_it_is_signed_in() {
    let st = Arc::new(AppState::for_test());
    let chat = Chat { provider: "codex".into(), ..Default::default() };
    let id = chat.id.clone();
    st.chats.insert(chat).await;
    // No Codex on this computer: the message is refused up front, in plain words, and not kept.
    let e = shown(chat_send(State(st.clone()), Path(id.clone()), Json(SendBody { text: "hi".into(), ..Default::default() })).await);
    assert_eq!(e.message, crate::tr::text_in(false, "codex.missing", &[]));
    assert!(st.chats.get(&id).await.unwrap().messages.is_empty());
}

/// Customising the app runs on Claude Code only, whichever program answers everything else: on Codex
/// a customise run could read the app's own data. Enforced by the daemon, whatever a client asks for.
#[cfg(unix)]
#[tokio::test]
async fn customising_runs_on_claude_code_whatever_program_is_chosen() {
    let st = Arc::new(AppState::for_test());
    fake_codex(&st).await;
    let bin = std::env::temp_dir().join(format!("thoughts-no-claude-{}", uuid::Uuid::new_v4())).join("claude");
    { let mut s = st.settings.write().await; s.ai.primary = "codex".into(); s.ai.codex.model = Some("gpt-5".into()); s.ai.claude.model = Some("opus".into());
      s.ai.claude.binary = Some(bin.to_string_lossy().into()); }
    // A new customise chat is Claude Code's, with Claude Code's model — also when Codex is asked for by name.
    let mut made = vec![];
    for provider in [None, Some("codex")] {
        let c = chat_create(State(st.clone()), Json(NewChat { kind: Some("customize".into()), provider: provider.map(String::from), ..Default::default() })).await.unwrap().0;
        assert_eq!((c["provider"].as_str(), c["options"]["model"].as_str()), (Some("claude"), Some("opus")), "{provider:?}");
        made.push(c["id"].as_str().unwrap().to_string());
    }
    // Claude Code is not installed: the message is refused up front, saying customising needs it.
    let send = |id: String| chat_send(State(st.clone()), Path(id), Json(SendBody { text: "Add a page for my invoices".into(), ..Default::default() }));
    let e = shown(send(made[0].clone()).await);
    assert_eq!(e.message, crate::tr::text_in(false, "customize.needs_claude", &[]));
    assert!(st.chats.get(&made[0]).await.unwrap().messages.is_empty());
    // Installed but signed out: said the same way, with the code that offers to sign in to Claude Code.
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, "#!/bin/sh\necho '{\"loggedIn\": false}'\n").unwrap();
    { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap(); }
    let e = shown(send(made[1].clone()).await);
    assert_eq!((e.code, e.message), ("signed_out", format!("SIGNED_OUT::{}", crate::tr::text_in(false, "customize.claude_signed_out", &[]))));
    // A customise chat recorded with Codex (by an earlier build) still runs on Claude Code, and is put right.
    let old = Chat { kind: "customize".into(), provider: "codex".into(), ..Default::default() };
    let id = old.id.clone();
    st.chats.insert(old).await;
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Program>::new()));
    let log = seen.clone();
    t::assistant(&st, move |info| { let log = log.clone(); async move { log.lock().unwrap().push(info.program); true } });
    let cwd = bin.parent().unwrap().join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    crate::claude::start_run(st.clone(), crate::claude::RunSpec { chat_id: id.clone(), prompt: "x".into(), cwd, add_dirs: vec![],
        options: RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() }, resume: None, append_system: None, extra_deny: vec![] }, None).await.unwrap();
    for _ in 0..400 { if !seen.lock().unwrap().is_empty() { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
    assert_eq!(seen.lock().unwrap().as_slice(), [Program::Claude]);
    assert_eq!(st.chats.get(&id).await.unwrap().provider, "claude");
    // Moving one to Codex is refused like any chat that is not an ordinary one.
    assert_eq!(shown(chat_patch(State(st.clone()), Path(made[0].clone()), patch("codex")).await).status, 403);
    std::fs::remove_dir_all(bin.parent().unwrap()).unwrap();
}
