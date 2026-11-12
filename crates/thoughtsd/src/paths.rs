use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `~/.thoughts`, overridable with `THOUGHTS_HOME`.
#[cfg(not(test))]
pub fn home() -> PathBuf {
    if let Ok(h) = std::env::var("THOUGHTS_HOME") {
        return PathBuf::from(h);
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".thoughts")
}

/// Under test the home is always a throwaway temp folder (one per test process), so no test can
/// ever read or write the real `~/.thoughts`.
#[cfg(test)]
pub fn home() -> PathBuf {
    static H: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    H.get_or_init(|| {
        let d = std::env::temp_dir().join(format!("thoughts-test-home-{}", uuid::Uuid::new_v4()));
        for sub in ["chats", "attachments", "extensions", "logs"] { std::fs::create_dir_all(d.join(sub)).unwrap(); }
        d
    }).clone()
}

pub fn chats_dir() -> PathBuf { home().join("chats") }
pub fn attachments_dir() -> PathBuf { home().join("attachments") }
pub fn app_layer_dir() -> PathBuf { home().join("app-layer") }
pub fn extensions_dir() -> PathBuf { home().join("extensions") }
pub fn settings_file() -> PathBuf { home().join("settings.json") }
pub fn daemon_file() -> PathBuf { home().join("daemon.json") }
pub fn git_credentials_file() -> PathBuf { home().join("git-credentials") }
pub fn usage_file() -> PathBuf { home().join("usage.json") }
pub fn logs_dir() -> PathBuf { home().join("logs") }
/// PIDs of assistant processes started by this daemon (cleaned up after a crash).
pub fn runs_file() -> PathBuf { home().join("runs.json") }
/// Held exclusively for the daemon's lifetime so two daemons can never run.
pub fn lock_file() -> PathBuf { home().join("daemon.lock") }
/// Fallback secret store (0600) for systems without a working keyring.
pub fn secrets_file() -> PathBuf { home().join("secrets.json") }
pub fn trusted_file() -> PathBuf { home().join("trusted.json") }
/// The staging copy of the customisation layer (a git worktree of the app layer).
pub fn app_layer_staging_dir() -> PathBuf { home().join("app-layer-staging") }
/// Histories of libraries that carry no repository of their own (see `git::history_store`): kept
/// here, outside the library, so a cloud drive that syncs the library never sees them.
pub fn history_dir() -> PathBuf { home().join("history") }
/// When each library was last backed up to an online location, and where (see `git::note_backup`).
pub fn backup_state_file() -> PathBuf { home().join("backup-state.json") }

/// Everything under the app's home (`home`) that a customise run must never read or change: every
/// file and folder the daemon keeps there (credentials, token, settings, chats, workflow approvals,
/// usage, pending state…) except the staging worktree it works in and the live layer (which the
/// caller protects separately). Known names are always listed, even before they exist, and anything
/// else found in the folder is added, so a new state file is protected by default.
pub fn secret_paths_in(home: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    const FILES: &[&str] = &["daemon.json", "daemon.lock", "settings.json", "settings.json.bak", "git-credentials", "trusted.json", "secrets.json",
        "runs.json", "usage.json", "usage-events.jsonl", "layer-pending.json", "workflows-approved.json",
        "workflows-state.json", "suggestions.json", "base-manifest.json", "layer-requests.json", "backup-state.json"];
    const DIRS: &[&str] = &["chats", "attachments", "logs", "base", "backups", "extensions", "reorganize-worktree", "run-tmp", "plugin-data", "history"];
    const OPEN: &[&str] = &["app-layer-staging", "app-layer"];
    let mut files: Vec<PathBuf> = FILES.iter().map(|f| home.join(f)).collect();
    let mut dirs: Vec<PathBuf> = DIRS.iter().map(|d| home.join(d)).collect();
    if let Ok(rd) = std::fs::read_dir(home) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if OPEN.contains(&name.as_str()) { continue; }
            let p = e.path();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let list = if is_dir { &mut dirs } else { &mut files };
            if !list.contains(&p) { list.push(p); }
        }
    }
    (files, dirs)
}

/// Short-lived files handed to the assistant program on systems where they cannot be passed as arguments.
pub fn run_tmp_dir() -> PathBuf { home().join("run-tmp") }

/// The program to start for the daemon, autostart entries and git's password prompt. Inside an
/// AppImage the running binary lives in a mount that disappears when the app quits, so the AppImage
/// file itself (`$APPIMAGE`) is what must be recorded.
pub fn app_exe() -> std::io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(p) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_absolute() && p.is_file()) { return Ok(p); }
    std::env::current_exe()
}

/// A path as programs and people expect to see it: canonical, but without Windows' `\\?\` prefix.
pub fn canonical(p: &Path) -> std::io::Result<PathBuf> { dunce::canonicalize(p) }

/// The real location of `p` even when its last components do not exist yet: the deepest existing
/// ancestor is resolved (symlinks, true letter case) and the rest is appended.
pub fn canonical_lenient(p: &Path) -> PathBuf {
    let mut rest: Vec<std::ffi::OsString> = vec![];
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(real) = dunce::canonicalize(&cur) {
            let mut out = real;
            for c in rest.iter().rev() { out.push(c); }
            return out;
        }
        match (cur.file_name().map(|n| n.to_os_string()), cur.parent().map(Path::to_path_buf)) {
            (Some(name), Some(parent)) => { rest.push(name); cur = parent; }
            _ => return p.to_path_buf(),
        }
    }
}

/// Is `p` equal to, or inside, `dir`? Both are resolved to their real locations first and compared
/// without regard to letter case, so `Me/Finances`, a symlink to it, or `me/finances/../finances`
/// are all the same folder (volumes on macOS and Windows ignore case; being strict everywhere only
/// ever protects more).
pub fn inside_ci(p: &Path, dir: &Path) -> bool {
    let norm = |x: &Path| canonical_lenient(x).to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    let (p, dir) = (norm(p), norm(dir));
    p == dir || p.starts_with(&format!("{dir}/"))
}

/// Is library-relative `rel` inside one of the private folders, judged on real paths (see `inside_ci`)
/// as well as on the names as written?
pub fn is_private(root: &Path, rel: &str, private: &[String]) -> bool {
    if private.is_empty() { return false; }
    if is_under(rel, private) { return true; }
    let target = root.join(rel.trim_start_matches(['/', '\\']));
    private.iter().any(|d| inside_ci(&target, &root.join(d)))
}

/// Keep only the `keep` newest files in `dir` whose name starts with `prefix` (newest by name, which
/// for timestamped names is newest by time). Returns how many were removed.
pub fn prune_files(dir: &Path, prefix: &str, keep: usize) -> usize {
    let mut names: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with(prefix)).collect();
    names.sort();
    let extra = names.len().saturating_sub(keep);
    names.iter().take(extra).filter(|n| std::fs::remove_file(dir.join(n)).is_ok()).count()
}

/// Where Reorganize works, so the library's own folder never leaves the user's branch.
pub fn reorganize_worktree() -> PathBuf { home().join("reorganize-worktree") }

/// The OS user's home folder, as an error rather than a panic when it cannot be determined.
pub fn user_home() -> anyhow::Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not find your home folder"))
}

/// Expand a leading `~/`.
pub fn expand_tilde(raw: &str) -> anyhow::Result<PathBuf> {
    match raw.strip_prefix("~/") {
        Some(rest) => Ok(user_home()?.join(rest)),
        None => Ok(PathBuf::from(raw)),
    }
}

/// Create `d` (and parents) readable by the owner only, and tighten it if it already exists.
pub fn create_private_dir(d: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(d)?;
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(d)?;
    Ok(())
}

pub fn ensure_dirs() -> std::io::Result<()> {
    for d in [home(), chats_dir(), attachments_dir(), extensions_dir(), logs_dir()] {
        create_private_dir(&d)?;
    }
    Ok(())
}

/// Write `bytes` to `path` atomically: a temp file in the same folder, then a rename, so a crash
/// never leaves a half-written file. `private` files are created 0600 (never chmod-after).
pub fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    use std::io::Write;
    // A library file that is a symlink keeps being one: write through to its target.
    let target = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() && !private => dunce::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let name = target.file_name().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp = dir.join(format!(".{}.{}.tmp", name.to_string_lossy(), uuid::Uuid::new_v4().simple()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Keep an existing shared file's mode; new shared files get the usual 0644 (minus umask).
        let mode = if private { 0o600 } else {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(&target).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o644)
        };
        opts.mode(mode);
    }
    let res = (|| {
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &target)
    })();
    if res.is_err() { let _ = std::fs::remove_file(&tmp); }
    res
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
    let text = serde_json::to_string_pretty(info).map_err(std::io::Error::other)?;
    write_atomic(&daemon_file(), text.as_bytes(), true)
}

pub fn clear_daemon_info() {
    let _ = std::fs::remove_file(daemon_file());
}

/// Normalise a library-relative folder path: forward slashes, no leading `./`, no trailing slash.
/// Refuses absolute paths, `..`, and characters that cannot be expressed safely in a permission rule.
pub fn clean_rel(rel: &str) -> anyhow::Result<String> {
    let rel = rel.trim().replace('\\', "/");
    if rel.starts_with('/') || rel.starts_with('~') || Path::new(&rel).is_absolute() { anyhow::bail!("path must be relative to the library: {rel}"); }
    if rel.chars().any(|c| c.is_control() || "()*?[]{}\"'`$!:".contains(c)) { anyhow::bail!("path contains unsupported characters: {rel}"); }
    let mut parts = vec![];
    for comp in rel.split('/') {
        match comp.trim() {
            "" | "." => {}
            ".." => anyhow::bail!("path may not contain '..': {rel}"),
            c => parts.push(c.to_string()),
        }
    }
    if parts.is_empty() { anyhow::bail!("empty path"); }
    Ok(parts.join("/"))
}

/// Is library-relative `rel` equal to, or inside, one of `dirs` (already cleaned with `clean_rel`)?
pub fn is_under(rel: &str, dirs: &[String]) -> bool {
    let rel = rel.replace('\\', "/");
    let rel = rel.trim_start_matches("./").trim_start_matches('/').to_lowercase();
    dirs.iter().any(|d| { let d = d.to_lowercase(); rel == d || rel.starts_with(&format!("{d}/")) })
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
    let real_root = dunce::canonicalize(root)?;
    let mut cur = root.to_path_buf();
    for comp in out.strip_prefix(root)?.components() {
        cur.push(comp);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                let target = dunce::canonicalize(&cur).map_err(|_| anyhow::anyhow!("broken symlink in path"))?;
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

    #[test]
    fn atomic_write_is_private_from_creation() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("wa-{}", uuid::Uuid::new_v4()));
        create_private_dir(&d.join("sub")).unwrap();
        assert_eq!(std::fs::metadata(d.join("sub")).unwrap().permissions().mode() & 0o777, 0o700);
        let f = d.join("sub/secret.json");
        write_atomic(&f, b"one", true).unwrap();
        write_atomic(&f, b"two", true).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "two");
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        // No temp files left behind.
        assert_eq!(std::fs::read_dir(d.join("sub")).unwrap().count(), 1);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn private_folders_match_whatever_the_spelling() {
        let d = std::env::temp_dir().join(format!("pv-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("lib/me/finances")).unwrap();
        std::fs::create_dir_all(d.join("lib/ideas")).unwrap();
        std::fs::write(d.join("lib/me/finances/tax.md"), "x").unwrap();
        let lib = d.join("lib");
        std::os::unix::fs::symlink(lib.join("me/finances"), lib.join("ideas/money")).unwrap();
        let private = vec!["me/finances".to_string()];
        for hidden in ["me/finances/tax.md", "ME/Finances/tax.md", "me/finances", "me/./finances/new.md", "ideas/money/tax.md", "ideas/money", "me/finances/not/yet/there.md"] {
            assert!(is_private(&lib, hidden, &private), "{hidden}");
        }
        for open in ["me/profile.md", "ideas/a.md", "me/finances-old/x.md", "me"] { assert!(!is_private(&lib, open, &private), "{open}"); }
        assert!(!is_private(&lib, "me/finances/tax.md", &[]));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn old_files_are_pruned_by_name() {
        let d = std::env::temp_dir().join(format!("pr-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        for i in 1..=8 { std::fs::write(d.join(format!("settings.json.corrupt-2026010{i}-000000")), "x").unwrap(); }
        std::fs::write(d.join("settings.json"), "{}").unwrap();
        assert_eq!(prune_files(&d, "settings.json.corrupt-", 5), 3);
        let mut left: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left.len(), 6);
        assert_eq!(left[0], "settings.json");
        assert_eq!(left[1], "settings.json.corrupt-20260104-000000");
        assert_eq!(prune_files(&d, "settings.json.corrupt-", 5), 0);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn clean_rel_rejects_escapes() {
        assert_eq!(clean_rel(" ./me/finances/ ").unwrap(), "me/finances");
        for bad in ["../x", "a/../b", "/etc", "~/x", "a/**", "x)y", ""] { assert!(clean_rel(bad).is_err(), "{bad}"); }
        let dirs = vec!["me/finances".to_string()];
        assert!(is_under("me/finances/tax.md", &dirs) && is_under("Me/Finances", &dirs));
        assert!(!is_under("me/finances-old/x.md", &dirs));
    }
}
