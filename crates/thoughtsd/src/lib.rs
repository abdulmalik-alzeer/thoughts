//! thoughtsd — the Thoughts session daemon.
//!
//! A small local HTTP + WebSocket server bound to 127.0.0.1 with a random port
//! and bearer token (written to `~/.thoughts/daemon.json`). It owns every AI
//! session process, so chats survive app restarts and updates.

pub mod api;
pub mod applayer;
pub mod askpass;
pub mod chats;
pub mod claude;
pub mod git;
pub mod layer_api;
pub mod library;
pub mod log;
pub mod modules;
pub mod packs;
pub mod paths;
pub mod proc;
pub mod runs;
pub mod secrets;
pub mod settings;
pub mod staging;
pub mod state;
pub mod suggestions;
pub mod todos;
pub mod tr;
pub mod trust;
pub mod updates;
pub mod usage;
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
    log::info(format!("daemon {} starting (pid {}, {})", env!("CARGO_PKG_VERSION"), std::process::id(), std::env::consts::OS));
    // Assistant processes that outlived a crashed daemon are ended before anything new starts.
    let reaped = runs::reap_stale(&paths::runs_file());
    if reaped > 0 { log::warn(format!("ended {reaped} assistant process(es) left over from a previous run")); }
    let state = Arc::new(AppState::load()?);
    log::add_secret(&state.token);
    if state.settings_recovered.load(std::sync::atomic::Ordering::Relaxed) { log::warn("settings were reset to defaults; the damaged file was kept next to settings.json"); }
    if state.chats.unreadable > 0 { log::warn(format!("{} chat file(s) could not be read and were skipped", state.chats.unreadable)); }
    log::ok_or_warn("could not prepare the customisation layer", applayer::ensure(&state).await);
    wraps::recover(&state).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
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
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(async move { shutdown.notified().await })
        .await;
    state.chats.flush(true).await;
    paths::clear_daemon_info();
    match &served {
        Ok(()) => log::info("daemon stopped"),
        Err(e) => log::error(format!("daemon stopped with an error: {e}")),
    }
    served?;
    Ok(())
}

pub async fn ping(port: u16, token: &str) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Ok(Ok(mut s)) = tokio::time::timeout(
        std::time::Duration::from_millis(800),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    else {
        return false;
    };
    let req = format!(
        "GET /api/ping HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    if s.write_all(req.as_bytes()).await.is_err() {
        return false;
    }
    let mut buf = vec![0u8; 256];
    matches!(tokio::time::timeout(std::time::Duration::from_millis(800), s.read(&mut buf)).await,
        Ok(Ok(n)) if n > 12 && buf[..n].starts_with(b"HTTP/1.1 200"))
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
        assert!(super::acquire_lock(&p).unwrap().is_some(), "the lock is free again once its holder is gone");
        std::fs::remove_dir_all(d).unwrap();
    }
}
