//! How a run begins and ends: one run per chat, Stop after the answer, what a run leaves behind,
//! a run whose work panics, and output lines too long to keep.

use super::*;

fn ms(n: u64) -> std::time::Duration { std::time::Duration::from_millis(n) }

/// Start `script` under `sh` the way assistant runs are started (own group, piped).
#[cfg(unix)]
fn sh(script: &str) -> tokio::process::Child {
    let mut c = cmd(Path::new("/bin/sh"));
    c.args(["-c", script]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    c.spawn().unwrap()
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

#[cfg(unix)]
fn group_alive(pid: u32) -> bool {
    std::process::Command::new("pgrep").args(["-g", &pid.to_string()]).output().map(|o| o.status.success()).unwrap_or(false)
}

fn is_result(line: String) -> std::future::Ready<Option<bool>> { std::future::ready(line.contains("\"result\"").then_some(true)) }

#[cfg(unix)]
#[tokio::test]
async fn stop_after_the_answer_keeps_the_answer() {
    // It answered and lingers (as the CLI may, for up to two minutes): Stop ends it, and the run
    // still counts as answered — not as stopped, which would drop the user's queued message.
    let (stop, mut kill) = tokio::sync::oneshot::channel::<()>();
    let mut c = sh(r#"echo '{"type":"result"}'; sleep 45"#);
    let pid = c.id().unwrap();
    let began = std::time::Instant::now();
    tokio::spawn(async move { tokio::time::sleep(ms(300)).await; let _ = stop.send(()); });
    let f = follow(&mut c, &mut kill, RunLimits { idle: None, total: None, after_result: std::time::Duration::from_secs(60) }, is_result).await;
    assert_eq!(f.ended, Ended::Result(true));
    assert!(began.elapsed() < std::time::Duration::from_secs(10) && !alive(pid));
}

#[cfg(unix)]
#[tokio::test]
async fn what_a_finished_run_left_running_is_ended() {
    // A command the assistant ran left something going in the background, then the program exited by itself.
    let (_keep, mut kill) = tokio::sync::oneshot::channel::<()>();
    let mut c = sh(r#"(sleep 46 >/dev/null 2>&1 &); echo '{"type":"result"}'"#);
    let pid = c.id().unwrap();
    let f = follow(&mut c, &mut kill, RunLimits { idle: None, total: None, after_result: std::time::Duration::from_secs(60) }, is_result).await;
    assert_eq!(f.ended, Ended::Result(true));
    assert!(f.exit.unwrap().success());
    let mut gone = false;
    for _ in 0..100 { if !group_alive(pid) { gone = true; break; } tokio::time::sleep(ms(20)).await; }
    assert!(gone, "what the run left behind is still running");
}

#[tokio::test]
async fn a_line_over_the_limit_is_skipped_and_reading_goes_on() {
    let long = "x".repeat(20);
    let input = format!("short\n{long}\n{long}{long}\nafter");
    let mut lines = Lines::new(input.as_bytes(), 8);
    let mut seen = vec![];
    loop {
        match lines.next().await.unwrap() {
            Read::Line => { seen.push(String::from_utf8(std::mem::take(&mut lines.raw)).unwrap()); }
            Read::TooLong(n) => seen.push(format!("<{n}>")),
            Read::End => break,
        }
        assert!(lines.raw.capacity() <= 64, "an over-long line was kept in memory");
    }
    assert_eq!(seen, ["short\n", "<21>", "<41>", "after"]);
    // A line that just fits is a line.
    let mut lines = Lines::new(&b"1234567\n"[..], 8);
    assert_eq!(lines.next().await.unwrap(), Read::Line);
    assert_eq!(lines.raw, b"1234567\n");
}

/// A chat whose stand-in assistant waits for `gate` and then answers.
async fn gated_chat(st: &Arc<AppState>, dir: &Path) -> (String, Arc<tokio::sync::Notify>, Arc<std::sync::atomic::AtomicUsize>) {
    let gate = Arc::new(tokio::sync::Notify::new());
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (g, r) = (gate.clone(), runs.clone());
    *st.run_hook.lock().unwrap() = Some(Arc::new(move |_info: RunInfo| {
        let (g, r) = (g.clone(), r.clone());
        Box::pin(async move { r.fetch_add(1, std::sync::atomic::Ordering::SeqCst); g.notified().await; true })
    }));
    std::fs::create_dir_all(dir).unwrap();
    let chat = crate::chats::Chat { kind: "workflow".into(), ..Default::default() };
    st.chats.insert(chat.clone()).await;
    (chat.id, gate, runs)
}

fn spec(id: &str, dir: &Path) -> RunSpec {
    RunSpec { chat_id: id.into(), prompt: "hi".into(), cwd: dir.to_path_buf(), add_dirs: vec![], options: RunOptions::default(),
        resume: None, append_system: None, extra_deny: vec![] }
}

#[tokio::test]
async fn a_second_run_for_a_chat_is_refused_and_the_first_goes_on() {
    let st = Arc::new(AppState::for_test());
    let dir = std::env::temp_dir().join(format!("thoughts-second-{}", uuid::Uuid::new_v4()));
    let (id, gate, runs) = gated_chat(&st, &dir).await;
    let mut rx = st.events.subscribe();
    start_run(st.clone(), spec(&id, &dir), None).await.unwrap();
    for _ in 0..400 { if runs.load(std::sync::atomic::Ordering::SeqCst) > 0 { break; } tokio::time::sleep(ms(5)).await; }
    // Started from somewhere that does not look at `running` (the resend after a lost session, a routine…).
    let second = start_run(st.clone(), spec(&id, &dir), None).await.unwrap_err();
    assert_eq!(crate::tr::classify_in(false, &second).code, "busy");
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1, "a second run was started");
    // The first one was not stopped by it: it answers, and its Stop switch was its own all along.
    assert!(st.runs.lock().await.get(&id).map_or(false, |k| !k.is_closed()));
    gate.notify_one();
    let done = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop { let e = rx.recv().await.unwrap(); if e["type"] == "chat.done" { return e; } }
    }).await.unwrap();
    assert_eq!(done["ok"], true);
    let c = st.chats.get(&id).await.unwrap();
    assert!(!c.running && !c.messages.iter().any(|m| m.role == "system"), "no run was stopped");
    assert!(st.runs.lock().await.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_stopped_run_that_ends_late_leaves_a_newer_run_of_its_chat_alone() {
    let st = Arc::new(AppState::for_test());
    let chat = crate::chats::Chat { running: true, ..Default::default() };
    st.chats.insert(chat.clone()).await;
    // The older run was stopped (its switch taken out); a newer one holds the chat now.
    let (tx, _newer) = tokio::sync::oneshot::channel::<()>();
    st.runs.lock().await.insert(chat.id.clone(), tx);
    st.run_dirs.lock().unwrap().insert(chat.id.clone(), std::env::temp_dir());
    let mut rx = st.events.subscribe();
    finish(st.clone(), chat.id.clone(), false, None, false, None).await;
    assert!(st.runs.lock().await.contains_key(&chat.id) && st.run_dirs.lock().unwrap().contains_key(&chat.id));
    assert!(st.chats.get(&chat.id).await.unwrap().running, "the newer run's chat was marked as finished");
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).all(|e| e["type"] != "chat.done"));
    // Its own switch, already let go of by the run, is cleared as before.
    drop(_newer);
    finish(st.clone(), chat.id.clone(), true, None, false, None).await;
    assert!(st.runs.lock().await.is_empty() && !st.chats.get(&chat.id).await.unwrap().running);
}

#[tokio::test]
async fn a_run_whose_work_panics_does_not_leave_its_chat_running() {
    let st = Arc::new(AppState::for_test());
    let dir = std::env::temp_dir().join(format!("thoughts-panic-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    *st.run_hook.lock().unwrap() = Some(Arc::new(|_info: RunInfo| Box::pin(async { if true { panic!("a bug in the run's work"); } true })));
    let chat = crate::chats::Chat { kind: "workflow".into(), ..Default::default() };
    st.chats.insert(chat.clone()).await;
    let mut rx = st.events.subscribe();
    start_run(st.clone(), spec(&chat.id, &dir), None).await.unwrap();
    let done = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop { let e = rx.recv().await.unwrap(); if e["type"] == "chat.done" { return e; } }
    }).await.expect("the chat was never let go");
    assert_eq!(done["ok"], false);
    let c = st.chats.get(&chat.id).await.unwrap();
    assert!(!c.running && st.runs.lock().await.is_empty() && st.run_dirs.lock().unwrap().is_empty());
    assert_eq!(c.messages.last().unwrap().role, "error", "the chat says it stopped without an answer");
    std::fs::remove_dir_all(dir).unwrap();
}
