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
pub mod library;
pub mod modules;
pub mod paths;
pub mod settings;
pub mod state;
pub mod todos;
pub mod trust;
pub mod updates;
pub mod wraps;

pub use state::AppState;

use std::sync::Arc;

/// Run the daemon until shutdown. Returns early if another daemon is alive.
pub async fn run() -> anyhow::Result<()> {
    paths::ensure_dirs()?;
    if let Some(info) = paths::read_daemon_info() {
        if ping(info.port, &info.token).await {
            eprintln!("thoughtsd already running on port {}", info.port);
            return Ok(());
        }
    }
    let state = Arc::new(AppState::load()?);
    applayer::ensure(&state).await.ok();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    paths::write_daemon_info(&paths::DaemonInfo {
        port,
        token: state.token.clone(),
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })?;
    tokio::spawn(wraps::idle_loop(state.clone()));
    let app = api::router(state.clone());
    let shutdown = state.shutdown.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move { shutdown.notified().await })
        .await?;
    paths::clear_daemon_info();
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
