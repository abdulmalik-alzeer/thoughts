//! The official ACP adapters (`acp.rs`), installed on demand into the app's own data folder.
//!
//! Each adapter is pinned to one version (`PINS`): it is installed with the machine's npm into
//! `<home>/acp/<name>-<version>/`, without install scripts and without the adapters' own copies of
//! the assistant programs (optional dependencies are left out). The adapter runs the user's own
//! installed program instead (`CLAUDE_CODE_EXECUTABLE`, `CODEX_PATH`), so the sign-in stays inside
//! the unmodified CLI. Nothing of it is in the repository or the app bundle.

use crate::assistant::Program;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// An adapter: its npm package, the pinned version and the entry point inside it.
pub struct Pin { pub package: &'static str, pub version: &'static str, pub entry: &'static str }

/// Raise a version here (and only here) after checking it against the app.
pub const CLAUDE: Pin = Pin { package: "@agentclientprotocol/claude-agent-acp", version: "0.110.0", entry: "dist/index.js" };
pub const CODEX: Pin = Pin { package: "@agentclientprotocol/codex-acp", version: "2.10.0", entry: "dist/index.js" };

pub fn pin(p: Program) -> &'static Pin { match p { Program::Claude => &CLAUDE, Program::Codex => &CODEX } }

/// Where the adapter for `p` is installed.
pub fn dir(p: Program) -> PathBuf {
    let pin = pin(p);
    crate::paths::home().join("acp").join(format!("{}-{}", pin.package.rsplit('/').next().unwrap_or(pin.package), pin.version))
}

/// The installed entry point, when the pinned version is there (its own package.json says so).
pub fn entry_in(dir: &Path, pin: &Pin) -> Option<PathBuf> {
    let pkg = dir.join("node_modules").join(pin.package);
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(pkg.join("package.json")).ok()?).ok()?;
    if meta["version"].as_str() != Some(pin.version) { return None; }
    let e = pkg.join(pin.entry);
    e.is_file().then_some(e)
}

pub fn ready(p: Program) -> Option<PathBuf> { entry_in(&dir(p), pin(p)) }

/// The npm command line that installs `pin` into `prefix`.
pub fn install_args(pin: &Pin, prefix: &Path) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = ["install", "--no-save", "--package-lock=false", "--ignore-scripts", "--omit=optional",
        "--no-audit", "--no-fund", "--loglevel=error", "--prefix"].iter().map(Into::into).collect();
    a.push(prefix.into());
    a.push(format!("{}@{}", pin.package, pin.version).into());
    a
}

static INSTALLING: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
fn slot(p: Program) -> &'static AtomicBool { &INSTALLING[match p { Program::Claude => 0, Program::Codex => 1 }] }

/// Install the adapter for `p` now (into a fresh folder beside its place, moved there when done).
pub async fn install(p: Program) -> anyhow::Result<PathBuf> {
    if let Some(e) = ready(p) { return Ok(e); }
    let npm = crate::claude::find_binary(&None, "npm").ok_or_else(|| anyhow::anyhow!("npm was not found (Node.js is needed for the ACP adapters)"))?;
    let (target, pin) = (dir(p), pin(p));
    let parent = target.parent().map(Path::to_path_buf).unwrap_or_else(crate::paths::home);
    std::fs::create_dir_all(&parent)?;
    let staging = parent.join(format!(".installing-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&staging)?;
    let mut c = crate::proc::command(&npm);
    c.env("PATH", crate::claude::shell_path()).args(install_args(pin, &staging)).current_dir(&staging)
        .stdin(std::process::Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(std::time::Duration::from_secs(600), c.output()).await;
    let ok = matches!(&out, Ok(Ok(o)) if o.status.success()) && entry_in(&staging, pin).is_some();
    if !ok {
        let why = match out { Ok(Ok(o)) => String::from_utf8_lossy(&o.stderr).chars().take(400).collect(), Ok(Err(e)) => e.to_string(), Err(_) => "timed out".into() };
        let _ = std::fs::remove_dir_all(&staging);
        anyhow::bail!("could not install {}@{}: {why}", pin.package, pin.version);
    }
    if target.exists() { let _ = std::fs::remove_dir_all(&target); }
    std::fs::rename(&staging, &target)?;
    crate::log::info(format!("installed the ACP adapter {}@{}", pin.package, pin.version));
    ready(p).ok_or_else(|| anyhow::anyhow!("the adapter is not where it was installed"))
}

/// Install in the background (once at a time); `acp.installed` is announced when it is done.
pub fn ensure_in_background(state: Arc<crate::state::AppState>, p: Program) {
    // Tests never install anything with the machine's npm.
    if cfg!(test) || ready(p).is_some() || slot(p).swap(true, Ordering::SeqCst) { return; }
    tokio::spawn(async move {
        match install(p).await {
            Ok(_) => state.emit(serde_json::json!({"type": "acp.installed", "program": p.name()})),
            Err(e) => crate::log::warn(format!("ACP adapter for {}: {e}", p.label())),
        }
        slot(p).store(false, Ordering::SeqCst);
    });
}
