use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `~/.thoughts`, overridable with `THOUGHTS_HOME`.
pub fn home() -> PathBuf {
    if let Ok(h) = std::env::var("THOUGHTS_HOME") {
        return PathBuf::from(h);
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".thoughts")
}

pub fn chats_dir() -> PathBuf { home().join("chats") }
pub fn attachments_dir() -> PathBuf { home().join("attachments") }
pub fn app_layer_dir() -> PathBuf { home().join("app-layer") }
pub fn extensions_dir() -> PathBuf { home().join("extensions") }
pub fn settings_file() -> PathBuf { home().join("settings.json") }
pub fn daemon_file() -> PathBuf { home().join("daemon.json") }
pub fn git_credentials_file() -> PathBuf { home().join("git-credentials") }

pub fn ensure_dirs() -> std::io::Result<()> {
    for d in [home(), chats_dir(), attachments_dir(), extensions_dir()] {
        std::fs::create_dir_all(d)?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DaemonInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub version: String,
}

pub fn read_daemon_info() -> Option<DaemonInfo> {
    let s = std::fs::read_to_string(daemon_file()).ok()?;
    serde_json::from_str(&s).ok()
}

pub fn write_daemon_info(info: &DaemonInfo) -> std::io::Result<()> {
    let p = daemon_file();
    std::fs::write(&p, serde_json::to_string_pretty(info).unwrap())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn clear_daemon_info() {
    let _ = std::fs::remove_file(daemon_file());
}

/// Join `rel` onto `root`, refusing anything that escapes `root`.
pub fn safe_join(root: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let rel = rel.trim_start_matches('/');
    let mut out = root.to_path_buf();
    for comp in Path::new(rel).components() {
        match comp {
            std::path::Component::Normal(c) => out.push(c),
            std::path::Component::CurDir => {}
            _ => anyhow::bail!("invalid path"),
        }
    }
    // Every component that is a symlink (dangling ones included) must resolve inside root.
    let real_root = root.canonicalize()?;
    let mut cur = root.to_path_buf();
    for comp in out.strip_prefix(root)?.components() {
        cur.push(comp);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                let target = cur.canonicalize().map_err(|_| anyhow::anyhow!("broken symlink in path"))?;
                if !target.starts_with(&real_root) { anyhow::bail!("path escapes the library"); }
            }
            Ok(_) => {}
            Err(_) => break, // rest doesn't exist yet; nothing to follow
        }
    }
    Ok(out)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn blocks_symlink_escape() {
        let d = std::env::temp_dir().join(format!("sj-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("lib/sub")).unwrap();
        let lib = d.join("lib");
        std::os::unix::fs::symlink(&d, lib.join("out")).unwrap();
        std::os::unix::fs::symlink(d.join("nope"), lib.join("dangling")).unwrap();
        assert!(safe_join(&lib, "sub/new.md").is_ok());
        assert!(safe_join(&lib, "out/x.md").is_err());
        assert!(safe_join(&lib, "dangling").is_err());
        assert!(safe_join(&lib, "../x").is_err());
        std::fs::remove_dir_all(d).unwrap();
    }
}
