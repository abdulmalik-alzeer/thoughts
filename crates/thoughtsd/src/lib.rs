//! thoughtsd — the Thoughts session daemon.
//!
//! A small local HTTP + WebSocket server bound to 127.0.0.1 with a random port
//! and bearer token (written to `~/.thoughts/daemon.json`). It owns every AI
//! session process, so chats survive app restarts and updates.

pub mod addons;
pub mod api;
pub mod applayer;
pub mod approvals;
pub mod assistant;
pub mod askpass;
pub mod care_api;
pub mod changes;
pub mod changesets;
pub mod chats;
pub mod consent;
pub mod claude;
pub mod cloud;
pub mod diffs;
pub mod gallery;
pub mod codex;
pub mod git;
pub mod handoff;
pub mod hello;
pub mod inbox;
pub mod history;
pub mod import;
pub mod kit_history;
pub mod health;
pub mod layer_api;
pub mod libraries;
pub mod library;
pub mod links;
pub mod listings;
pub mod log;
pub mod mentions;
pub mod modules;
pub mod packs;
pub mod paths;
pub mod proc;
pub mod routines;
pub mod release;
pub mod removal;
pub mod repair;
pub mod runs;
pub mod run_files;
pub mod search;
pub mod secrets;
pub mod settings;
pub mod spend;
pub mod meter;
pub mod prices;
pub mod shell_guard;
pub mod sshhost;
pub mod staging;
pub mod state;
pub mod suggestions;
pub mod tidy;
pub mod todos;
pub mod tokens;
pub mod tr;
pub mod traces;
pub mod transfer;
pub mod trust;
pub mod updates;
pub mod usage;
pub mod voice;
pub mod workflows;
pub mod wraps;

pub use state::AppState;

use std::sync::Arc;

/// Take the daemon's exclusive lock (`daemon.lock`). `Ok(None)` means another daemon holds it.
/// The lock lives as long as the returned file stays open, and the OS releases it if the process dies.
pub fn acquire_lock(path: &std::path::Path) -> std::io::Result<Option<std::fs::File>> {
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path)?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

/// Run the daemon until shutdown. Returns early if another daemon is alive.
pub async fn run() -> anyhow::Result<()> {
    paths::ensure_dirs()?;
    if let Some(info) = paths::read_daemon_info() {
        if ping(info.port, &info.token).await {
            eprintln!("thoughtsd already running on port {}", info.port);
            return Ok(());
        }
    }
    log::init(&paths::logs_dir());
    log::install_panic_hook();
    // The desktop shell that started this daemon hands over its confirmation key (`consent.rs`).
    consent::take_key_if_asked();
    // Two daemons must never run at once, even if both start in the same instant.
    let _lock = match acquire_lock(&paths::lock_file()) {
        Ok(Some(f)) => Some(f),
        Ok(None) => {
            log::warn("another daemon holds the lock; not starting a second one");
            eprintln!("thoughtsd already running");
            return Ok(());
        }
        // A filesystem without locking should not keep the app from working.
        Err(e) => { log::warn(format!("could not take the daemon lock ({e}); continuing without it")); None }
    };
    // Holding the lock means no other daemon is alive, so a daemon.json still lying here was left
    // by one that died. Remove it now: until this daemon has written its own, nothing should point
    // a client at a port that anyone may have taken since.
    if _lock.is_some() { paths::clear_daemon_info(); }
    log::info(format!("daemon {} starting (pid {}, {})", env!("CARGO_PKG_VERSION"), std::process::id(), std::env::consts::OS));
    // Assistant processes that outlived a crashed daemon are ended before anything new starts.
    let reaped = runs::reap_stale(&paths::runs_file());
    if reaped > 0 { log::warn(format!("ended {reaped} assistant process(es) left over from a previous run")); }
    // …and so are the private files that daemon had handed to its runs.
    claude::clear_run_tmp(&paths::run_tmp_dir());
    // …and the copies of chat attachments made for its runs (`run_files`).
    run_files::clear(&paths::home(), None);
    // Asking the user's shell for its PATH can take a few seconds: do it now, off to the side,
    // rather than inside the first request that needs it.
    tokio::task::spawn_blocking(|| { claude::shell_path(); });
    let state = Arc::new(AppState::load()?);
    log::add_secret(&state.token);
    if state.settings_recovered.load(std::sync::atomic::Ordering::Relaxed) { log::warn("settings were reset to defaults; the damaged file was kept next to settings.json"); }
    if state.chats.unreadable > 0 { log::warn(format!("{} chat file(s) could not be read and were skipped", state.chats.unreadable)); }
    log::ok_or_warn("could not prepare the customisation layer", applayer::ensure(&state).await);
    wraps::recover(&state).await;
    // The start-at-login entry records where the app was when it was switched on: check it still matches.
    { let s = state.settings.read().await.clone(); tokio::task::spawn_blocking(move || api::reconcile_login_item(&s)); }
    // A library set up by an older version still carries a few of the starter kit's one-time files.
    // (Not while a cut-off wrap is waiting: its undo must find the library exactly as it left it.)
    if wraps::interrupted_wrap(&state).await.is_none() {
        let s = state.settings.read().await.clone();
        if let Some(lib) = s.library().filter(|l| s.onboarded && l.is_dir()) {
            let cleared = library::tidy_kit_leftovers(&lib);
            if cleared > 0 { log::info(format!("cleared away {cleared} leftover starter-kit file(s) from the library")); }
            // …and its old session hook, which pulled inside app runs (with the library held, see there).
            claude::refresh_starter_hook(&state, &lib).await;
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    // Runs that ask the user (`approvals.rs`) are pointed at this port.
    approvals::set_port(port);
    paths::write_daemon_info(&paths::DaemonInfo {
        port,
        token: state.token.clone(),
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })?;
    log::info(format!("listening on 127.0.0.1:{port}"));
    tokio::spawn(wraps::idle_loop(state.clone()));
    // The customisation layer's background work (default-file updates, workflows, usage log).
    layer_api::start(state.clone());
    // Every library has a history of changes: one that has none yet (from before 0.0.9) gets it now.
    history::start(state.clone());
    // The weekly "is there a newer version?" check, for users who switched it on.
    tokio::spawn(release::weekly(state.clone()));
    // Old and never-used chats are removed in the background (the old-chat part only when the person asked for it).
    tidy::start(state.clone());
    // Streaming chats are written to disk at most every couple of seconds; this picks up the rest.
    let flusher = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            flusher.chats.flush(false).await;
        }
    });
    let app = api::router(state.clone());
    let shutdown = state.shutdown.clone();
    let server = axum::serve(listener, app).with_graceful_shutdown(async move { shutdown.notified().await });
    // The daemon stops when it is asked to over the API (POST /api/shutdown) or by the system
    // (logout, restart, the app's own last resort: SIGTERM / SIGINT / SIGHUP). Either way it ends
    // the same way, in `shut_down`.
    // (`notify_waiters` only reaches a `notified()` that already exists, so this one is made first.)
    let asked = state.shutdown.notified();
    tokio::pin!(asked);
    let served = tokio::select! {
        r = server => r,
        // Asked over the API: requests in flight get a moment to finish. The window's live event
        // stream never closes by itself, so the wait is bounded.
        _ = async { asked.await; tokio::time::sleep(SHUTDOWN_GRACE).await } => Ok(()),
        // Asked by the system: there may be very little time, so nothing is waited for.
        sig = stop_signal() => { log::info(format!("{sig} received: stopping")); Ok(()) }
    };
    shut_down(&state).await;
    match &served {
        Ok(()) => log::info("daemon stopped"),
        Err(e) => log::error(format!("daemon stopped with an error: {e}")),
    }
    served?;
    Ok(())
}

/// How long requests in flight get after a shutdown was asked for over the API.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

/// The system asking this process to stop. Resolves with the name of the signal.
#[cfg(unix)]
async fn stop_signal() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};
    // A signal that cannot be listened for is simply never seen.
    async fn got(kind: SignalKind) {
        match signal(kind) { Ok(mut s) => { s.recv().await; } Err(_) => std::future::pending().await }
    }
    tokio::select! {
        _ = got(SignalKind::terminate()) => "SIGTERM",
        _ = got(SignalKind::interrupt()) => "SIGINT",
        _ = got(SignalKind::hangup()) => "SIGHUP",
    }
}

/// The system asking this process to stop (UNTESTED on Windows: a process without a console may never see it).
#[cfg(not(unix))]
async fn stop_signal() -> &'static str {
    if tokio::signal::ctrl_c().await.is_err() { std::future::pending::<()>().await; }
    "Ctrl+C"
}

/// What stopping means, however it was asked for: every assistant run still going is ended
/// together with everything it started, the chats are written out, and the files that say "a
/// daemon is running" are removed. Runs ended here stay marked as running in their chats, so the
/// next start reports them as cut off (and recovers a wrap that was in progress).
async fn shut_down(state: &Arc<AppState>) {
    state.stopping.store(true, std::sync::atomic::Ordering::SeqCst);
    let ended = runs::end_all(&paths::runs_file());
    if ended > 0 { log::warn(format!("ended {ended} assistant run(s) that were still going")); }
    state.chats.flush(true).await;
    paths::clear_daemon_info();
}

/// Whether the daemon that owns `token` is answering on `port`. The token is not sent: the
/// responder has to prove it knows it (see `hello`).
pub async fn ping(port: u16, token: &str) -> bool {
    let token = token.to_string();
    matches!(tokio::task::spawn_blocking(move || hello::probe(port, &token)).await, Ok(hello::Probe::Daemon(_)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_one_daemon_can_hold_the_lock() {
        let d = std::env::temp_dir().join(format!("thoughts-lock-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("daemon.lock");
        let first = super::acquire_lock(&p).unwrap();
        assert!(first.is_some());
        assert!(super::acquire_lock(&p).unwrap().is_none(), "a second daemon must be refused");
        drop(first);
        // Another test may fork a child at this very moment; until that child execs it shares the
        // lock's open file, so the lock can stay held for a few milliseconds after the drop.
        let freed = (0..100).any(|_| super::acquire_lock(&p).unwrap().is_some() || { std::thread::sleep(std::time::Duration::from_millis(20)); false });
        assert!(freed, "the lock is free again once its holder is gone");
        std::fs::remove_dir_all(d).unwrap();
    }
}
