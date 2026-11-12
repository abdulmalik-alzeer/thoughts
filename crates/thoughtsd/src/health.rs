//! The start-up self-check (Settings → Maintenance, and a notice when the app opens): problems that
//! would otherwise surface later as unrelated errors, each with a fix the user can run in one click.
//!
//! Every issue is an id the window translates (`id`, plus an optional count `n`), and `fix` names
//! what the button does. Fixes that only the service can make are run by `fix`; the others
//! (choose a folder, enter a key, install git) are handled by the window.

use crate::chats::Chat;
use crate::settings::Settings;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A lock the history tool left behind is only treated as stale after this long.
pub const STALE_LOCK: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Issue {
    pub id: &'static str,
    /// What the button does (see the module comment), if anything can be done from here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<&'static str>,
    /// A count, for issues about several things.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<usize>,
}

fn issue(id: &'static str, fix: &'static str) -> Issue { Issue { id, fix: Some(fix), n: None } }

/// Folders a cloud service keeps in step with other computers.
pub fn in_cloud_folder(p: &str) -> bool {
    ["Mobile Documents", "Library/CloudStorage", "Dropbox", "OneDrive", "Google Drive", "iCloud"].iter().any(|k| p.contains(k))
}

/// The folder git keeps the library's history in: its own `.git` folder, or the one the app keeps
/// for it outside the library (`git::history_store`).
pub fn git_dir(library: &Path) -> Option<PathBuf> {
    match crate::git::repo_of(library) {
        crate::git::Repo::Own => { let g = library.join(".git"); std::fs::symlink_metadata(&g).ok().filter(|m| m.is_dir()).map(|_| g) }
        crate::git::Repo::Kept(store) => Some(store),
        crate::git::Repo::Missing => None,
    }
}

/// A combine (rebase or merge) that stopped halfway and was never finished or undone.
pub fn unfinished_step(git_dir: &Path) -> Option<&'static str> {
    if git_dir.join("rebase-merge").is_dir() || git_dir.join("rebase-apply").is_dir() { return Some("rebase"); }
    if git_dir.join("MERGE_HEAD").is_file() { return Some("merge"); }
    None
}

/// The history tool's lock file, when it was left behind by a program that is long gone.
pub fn stale_lock(git_dir: &Path, now: SystemTime) -> Option<PathBuf> {
    let lock = git_dir.join("index.lock");
    let m = std::fs::symlink_metadata(&lock).ok().filter(|m| m.is_file())?;
    let age = now.duration_since(m.modified().ok()?).unwrap_or_default();
    (age >= STALE_LOCK).then_some(lock)
}

/// Chat files in `dir` that cannot be read.
pub fn unreadable_chats(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "json") && std::fs::symlink_metadata(p).map_or(false, |m| m.is_file()))
        .filter(|p| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Chat>(&t).ok()).is_none())
        .collect();
    out.sort();
    out
}

/// Move unreadable chat files into `aside` (never overwriting anything there). Returns how many moved.
pub fn set_aside(files: &[PathBuf], aside: &Path) -> anyhow::Result<usize> {
    if files.is_empty() { return Ok(0); }
    crate::paths::create_private_dir(aside)?;
    let mut moved = 0;
    for f in files {
        let Some(name) = f.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
        let mut target = aside.join(&name);
        let mut i = 2;
        while std::fs::symlink_metadata(&target).is_ok() {
            target = aside.join(format!("{} ({i}).json", name.trim_end_matches(".json")));
            i += 1;
        }
        std::fs::rename(f, &target)?;
        moved += 1;
    }
    Ok(moved)
}

/// What can be told from the files alone (no programs run), for one settings state.
pub struct Facts<'a> {
    pub settings: &'a Settings,
    pub chats_dir: &'a Path,
    /// Chat files the service could not read when it started.
    pub unreadable_at_start: usize,
    /// Is a working history tool installed? (`None`: not asked, because sync is off.)
    pub git_installed: Option<bool>,
    /// Is the saved provider key readable? (`None`: there is none, or this copy may not look.)
    pub key_readable: Option<bool>,
    /// What the start-at-login entry needs, if anything (`"write"` / `"remove"`).
    pub login_item: Option<&'static str>,
    pub now: SystemTime,
}

pub fn check(f: &Facts) -> Vec<Issue> {
    let s = f.settings;
    let mut out = vec![];
    let library = s.library().filter(|_| s.library_path.as_deref().map_or(false, |p| !p.trim().is_empty()));
    match &library {
        Some(l) if !l.is_dir() => out.push(issue("library-missing", "choose-library")),
        Some(l) => {
            // An online backup made with git inside a folder a cloud drive also copies. (A drive
            // only ever receives what this computer sends, so it does not fight the cloud drive.)
            if s.sync_enabled() && s.sync.target != "drive" && in_cloud_folder(&l.to_string_lossy()) { out.push(issue("library-cloud-git", "use-folder-sync")); }
            if s.history_enabled() {
                if let Some(g) = git_dir(l) {
                    if unfinished_step(&g).is_some() { out.push(issue("history-unfinished", "finish-history")); }
                    else if stale_lock(&g, f.now).is_some() { out.push(issue("history-locked", "unlock-history")); }
                }
            }
        }
        None => {}
    }
    if s.history_enabled() && f.git_installed == Some(false) { out.push(issue("git-missing", "install-git")); }
    if f.unreadable_at_start > 0 {
        let n = unreadable_chats(f.chats_dir).len();
        if n > 0 { out.push(Issue { id: "chats-unreadable", fix: Some("set-aside-chats"), n: Some(n) }); }
    }
    if let Some(what) = f.login_item {
        out.push(issue(if what == "write" { "login-item-stale" } else { "login-item-leftover" }, "repair-login-item"));
    }
    if s.ai.claude.has_api_token && f.key_readable == Some(false) { out.push(issue("key-unreadable", "enter-key")); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-health-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn facts<'a>(s: &'a Settings, chats: &'a Path) -> Facts<'a> {
        Facts { settings: s, chats_dir: chats, unreadable_at_start: 0, git_installed: Some(true), key_readable: None, login_item: None, now: SystemTime::now() }
    }

    fn ids(v: &[Issue]) -> Vec<&'static str> { v.iter().map(|i| i.id).collect() }

    #[test]
    fn a_healthy_setup_has_nothing_to_report() {
        let d = tmp();
        std::fs::create_dir_all(d.join("lib/.git")).unwrap();
        let mut s = Settings::default();
        s.library_path = Some(d.join("lib").to_string_lossy().into());
        assert!(check(&facts(&s, &d)).is_empty());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn each_problem_is_named_with_its_fix() {
        let d = tmp();
        let mut s = Settings::default();
        s.library_path = Some(d.join("gone").to_string_lossy().into());
        assert_eq!(check(&facts(&s, &d)), [issue("library-missing", "choose-library")]);

        // History sync in a cloud folder, stuck halfway through combining changes.
        let lib = d.join("Library/CloudStorage/Dropbox/Notes");
        std::fs::create_dir_all(lib.join(".git/rebase-merge")).unwrap();
        s.library_path = Some(lib.to_string_lossy().into());
        assert_eq!(ids(&check(&facts(&s, &d))), ["library-cloud-git", "history-unfinished"]);
        let mut to_drive = s.clone();
        to_drive.sync.target = "drive".into();
        assert_eq!(ids(&check(&facts(&to_drive, &d))), ["history-unfinished"], "a drive does not fight the cloud drive");
        // Folder sync in the same place is fine; the history kept for it still counts.
        s.sync.mode = "folder".into();
        assert_eq!(ids(&check(&facts(&s, &d))), ["history-unfinished"]);
        // With no history kept at all, history problems do not apply.
        s.keep_history = false;
        s.sync.mode = "none".into();
        assert!(check(&facts(&s, &d)).is_empty());
        s.keep_history = true;
        s.sync.mode = "git-manual".into();
        std::fs::remove_dir_all(lib.join(".git/rebase-merge")).unwrap();
        std::fs::write(lib.join(".git/MERGE_HEAD"), "abc").unwrap();
        assert!(ids(&check(&facts(&s, &d))).contains(&"history-unfinished"));
        std::fs::remove_file(lib.join(".git/MERGE_HEAD")).unwrap();

        // A lock file: only stale after a while.
        std::fs::write(lib.join(".git/index.lock"), "").unwrap();
        assert!(!ids(&check(&facts(&s, &d))).contains(&"history-locked"));
        let later = Facts { now: SystemTime::now() + STALE_LOCK + Duration::from_secs(1), ..facts(&s, &d) };
        assert!(ids(&check(&later)).contains(&"history-locked"));

        // No git while history sync is on; the key cannot be read; the login entry is wrong.
        let mut f = facts(&s, &d);
        f.git_installed = Some(false);
        f.key_readable = Some(false);
        f.login_item = Some("write");
        assert_eq!(ids(&check(&f)), ["library-cloud-git", "git-missing", "login-item-stale"], "no key saved: nothing to read");
        let mut with_key = s.clone();
        with_key.ai.claude.has_api_token = true;
        let mut f = facts(&with_key, &d);
        f.key_readable = Some(false);
        f.login_item = Some("remove");
        assert_eq!(ids(&check(&f)), ["library-cloud-git", "login-item-leftover", "key-unreadable"]);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn unreadable_chats_can_be_set_aside_without_losing_them() {
        let d = tmp();
        let chats = d.join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        std::fs::write(chats.join("good.json"), serde_json::to_string(&Chat::default()).unwrap()).unwrap();
        std::fs::write(chats.join("bad.json"), "{\"messages\": [").unwrap();
        std::fs::write(chats.join("notes.txt"), "not a chat").unwrap();
        let s = Settings::default();
        let mut f = facts(&s, &chats);
        assert!(check(&f).is_empty(), "nothing was unreadable at start: no scan");
        f.unreadable_at_start = 1;
        let found = check(&f);
        assert_eq!(found, [Issue { id: "chats-unreadable", fix: Some("set-aside-chats"), n: Some(1) }]);
        let aside = d.join("chats-set-aside");
        std::fs::create_dir_all(&aside).unwrap();
        std::fs::write(aside.join("bad.json"), "an earlier one").unwrap();
        assert_eq!(set_aside(&unreadable_chats(&chats), &aside).unwrap(), 1);
        assert!(check(&f).is_empty());
        assert_eq!(std::fs::read_to_string(aside.join("bad.json")).unwrap(), "an earlier one");
        assert_eq!(std::fs::read_to_string(aside.join("bad (2).json")).unwrap(), "{\"messages\": [");
        assert!(chats.join("good.json").is_file() && chats.join("notes.txt").is_file());
        std::fs::remove_dir_all(d).unwrap();
    }
}
