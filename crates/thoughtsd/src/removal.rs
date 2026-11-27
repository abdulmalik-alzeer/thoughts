//! "Remove Thoughts from this computer" (Settings → Maintenance).
//!
//! What it may delete is fixed here, by name: the app's own files and folders directly inside the
//! data folder it was started with (`paths::home()`). Never a parent, never a pattern, never the
//! library or a linked folder. Anything else found in the data folder is left where it is and
//! reported, and the folder itself is only removed when it is empty afterwards.
//!
//! Saved passwords go through the small [`Keyring`] interface, so tests use a fake and never reach
//! the system keychain. A copy of the service started with its own `THOUGHTS_HOME` (a sandbox, a
//! developer build) shares the keychain and the login item with the real app: it never touches them.

use std::path::{Path, PathBuf};

/// Files the app keeps directly in its data folder.
pub const DATA_FILES: &[&str] = &[
    "settings.json", "settings.json.bak", "daemon.json", "daemon.lock", "git-credentials", "secrets.json",
    "trusted.json", "runs.json", "usage.json", "usage-events.jsonl", "layer-pending.json", "layer-requests.json",
    "workflows-approved.json", "workflows-state.json", "suggestions.json", "base-manifest.json",
    "update-status.json", "release-check.json", "backup-state.json", "calendar-export.json",
    // Access keys for other programs, and the port those programs read (`tokens.rs`).
    "tokens.json", "port",
    // Kept by the desktop app: customisations switched off from its Recovery menu.
    "safe-mode",
    // What the user allowed in chats, which library each automatic task belongs to, a change being
    // drafted, and what the assistant programs said about their models.
    "chat-approvals.json", "workflows-library.json", "layer-drafts.json", "models-seen.json", "codex-seen.json", "context-windows.json",
];
/// Folders the app keeps directly in its data folder.
pub const DATA_DIRS: &[&str] = &[
    "chats", "attachments", "logs", "base", "backups", "extensions", "reorganize-worktree", "run-tmp",
    "plugin-data", "app-layer", "app-layer-staging", "app-layer-held", "chats-set-aside", "archify",
    // Earlier versions of libraries that carry no repository of their own (`git::history_store`):
    // the notes themselves are in the library and stay.
    "history",
    // A Wrap's working copies of the library (the library keeps its own files).
    "wrap-copies",
];

/// Where saved passwords live. The real one is the system keychain (`SystemKeyring`).
pub trait Keyring: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Option<String>;
    /// Missing is fine; anything else is an error.
    fn delete(&self, service: &str, account: &str) -> anyhow::Result<()>;
    /// Remove every entry of `service` the system can find by its name alone, whatever the account
    /// (a sync password saved for an address that is no longer in the library). Returns how many.
    fn delete_all(&self, _service: &str) -> usize { 0 }
}

/// The system keychain (Keychain on macOS, Credential Manager on Windows, the Secret Service on
/// Linux, with the private-file fallback there).
pub struct SystemKeyring;

impl Keyring for SystemKeyring {
    fn get(&self, service: &str, account: &str) -> Option<String> { crate::secrets::get(service, account) }
    fn delete(&self, service: &str, account: &str) -> anyhow::Result<()> { crate::secrets::delete(service, account) }
    fn delete_all(&self, service: &str) -> usize {
        // Only the app's own two service names are ever passed here.
        if !SERVICES.contains(&service) { return 0; }
        #[cfg(target_os = "macos")]
        {
            let mut n = 0;
            while n < 50 {
                let done = crate::proc::std_command("security").args(["delete-generic-password", "-s", service])
                    .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
                if !done.is_ok_and(|s| s.success()) { break; }
                n += 1;
            }
            n
        }
        #[cfg(not(target_os = "macos"))]
        { 0 }
    }
}

/// The keychain service names the app uses: the AI provider key and the sync passwords.
pub const SERVICES: &[&str] = &["thoughts-ai", "thoughts-git"];

/// May this copy of the service touch what it shares with the real app (keychain, login item)?
/// Only when it runs on the default data folder.
pub fn shares_with_real_app() -> bool { cfg!(not(test)) && std::env::var_os("THOUGHTS_HOME").is_none() }

/// What a removal of the data folder would do.
#[derive(Debug, Clone)]
pub struct Plan {
    pub home: PathBuf,
    /// The app's own entries that exist now (from `DATA_FILES` / `DATA_DIRS`).
    pub items: Vec<PathBuf>,
    /// Names in the data folder the app does not recognise: left alone.
    pub left: Vec<String>,
    /// Why the data folder cannot be removed (`"link"`, `"library-inside"`, `"folder-inside"`,
    /// `"inside-library"`, `"unsafe"`), if it cannot.
    pub blocked: Option<&'static str>,
}

/// Work out what removing the data folder `home` would delete. `library` and `linked` are the
/// user's folders: nothing that holds or sits inside one of them is ever removed.
pub fn plan(home: &Path, library: Option<&Path>, linked: &[PathBuf]) -> Plan {
    let mut p = Plan { home: home.to_path_buf(), items: vec![], left: vec![], blocked: None };
    let user_home = dirs::home_dir();
    let unsafe_home = home.parent().is_none() || home.file_name().is_none() || !home.is_absolute()
        || user_home.as_deref().is_some_and(|u| crate::paths::canonical_lenient(u) == crate::paths::canonical_lenient(home));
    if unsafe_home { p.blocked = Some("unsafe"); return p; }
    match std::fs::symlink_metadata(home) {
        Ok(m) if m.file_type().is_symlink() => { p.blocked = Some("link"); return p; }
        Ok(m) if m.is_dir() => {}
        _ => return p, // nothing there
    }
    for n in DATA_FILES.iter().chain(DATA_DIRS) {
        let item = home.join(n);
        if std::fs::symlink_metadata(&item).is_ok() { p.items.push(item); }
    }
    if let Ok(rd) = std::fs::read_dir(home) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !DATA_FILES.contains(&name.as_str()) && !DATA_DIRS.contains(&name.as_str()) { p.left.push(name); }
        }
    }
    p.left.sort();
    for user_dir in library.into_iter().chain(linked.iter().map(PathBuf::as_path)) {
        if crate::paths::inside_ci(home, user_dir) { p.blocked = Some("inside-library"); break; }
        if crate::paths::inside_ci(user_dir, home) {
            let is_library = library == Some(user_dir);
            p.blocked = Some(if is_library { "library-inside" } else { "folder-inside" });
            break;
        }
    }
    p
}

/// What happened, for the confirmation screen.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// Names of the app's entries that were removed from the data folder.
    pub removed: Vec<String>,
    /// Entries that could not be removed, with the reason.
    pub failed: Vec<(String, String)>,
    /// Names the app did not recognise, left in the data folder.
    pub left: Vec<String>,
    /// The data folder itself is gone (it was empty afterwards).
    pub folder_removed: bool,
    /// The data folder was kept on purpose.
    pub data_kept: bool,
    /// "removed" | "failed" | "skipped" (a copy with its own data folder) | "kept"
    pub passwords: &'static str,
    pub login_item: &'static str,
    /// Errors from the keychain or the login item, in the service's words.
    pub errors: Vec<String>,
}

/// Remove one of the app's entries without ever following a link: a link is removed, not its target.
fn remove_entry(p: &Path) -> std::io::Result<()> {
    let m = std::fs::symlink_metadata(p)?;
    if m.is_dir() && !m.file_type().is_symlink() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }
}

/// Delete what `plan` lists. Refuses a blocked plan. The data folder is removed only when empty.
pub fn remove_data(plan: &Plan) -> anyhow::Result<Outcome> {
    if let Some(why) = plan.blocked { anyhow::bail!("REMOVE_BLOCKED::{why}"); }
    let mut out = Outcome { left: plan.left.clone(), ..Default::default() };
    for item in &plan.items {
        // Every item is a direct child of the data folder, by construction; checked again here.
        if item.parent() != Some(plan.home.as_path()) { continue; }
        let name = item.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        match remove_entry(item) {
            Ok(()) => out.removed.push(name),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => out.failed.push((name, e.to_string())),
        }
    }
    out.folder_removed = std::fs::remove_dir(&plan.home).is_ok();
    Ok(out)
}

/// Sweep the listed entries again, quietly (something written between the removal and the exit).
pub fn sweep(plan: &Plan) {
    if plan.blocked.is_some() { return; }
    for n in DATA_FILES.iter().chain(DATA_DIRS) { let _ = remove_entry(&plan.home.join(n)); }
    let _ = std::fs::remove_dir(&plan.home);
}

/// The hosts sync passwords may be saved for: the library's backup addresses and the hosts in the
/// file store (`git-credentials`, lines `https://user:pass@host`).
pub fn credential_hosts(remote_urls: &[String], credentials_file: &Path) -> Vec<String> {
    let mut hosts: Vec<String> = remote_urls.iter().filter_map(|u| crate::git::host_of(u)).collect();
    if let Ok(t) = std::fs::read_to_string(credentials_file) {
        hosts.extend(t.lines().filter_map(|l| l.trim().rsplit_once('@').map(|(_, h)| h.trim_end_matches('/').to_string())));
    }
    hosts.retain(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c)));
    hosts.sort();
    hosts.dedup();
    hosts
}

/// Remove the provider key and every sync password the app saved. Returns the errors, if any.
pub fn remove_secrets(k: &dyn Keyring, hosts: &[String]) -> Vec<String> {
    let mut errors = vec![];
    if let Err(e) = k.delete("thoughts-ai", "claude") { errors.push(format!("{e:#}")); }
    for h in hosts {
        if let Err(e) = k.delete("thoughts-git", h) { errors.push(format!("{e:#}")); }
    }
    for s in SERVICES { k.delete_all(s); }
    errors
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// A keychain in memory.
    #[derive(Default)]
    pub struct FakeKeyring { pub entries: Mutex<BTreeMap<(String, String), String>>, pub refuse: bool }
    impl FakeKeyring {
        pub fn with(entries: &[(&str, &str, &str)]) -> Self {
            Self { entries: Mutex::new(entries.iter().map(|(s, a, v)| ((s.to_string(), a.to_string()), v.to_string())).collect()), refuse: false }
        }
        pub fn left(&self) -> Vec<(String, String)> { self.entries.lock().unwrap().keys().cloned().collect() }
    }
    impl Keyring for FakeKeyring {
        fn get(&self, s: &str, a: &str) -> Option<String> {
            if self.refuse { return None; }
            self.entries.lock().unwrap().get(&(s.into(), a.into())).cloned()
        }
        fn delete(&self, s: &str, a: &str) -> anyhow::Result<()> {
            if self.refuse { anyhow::bail!("access refused"); }
            self.entries.lock().unwrap().remove(&(s.into(), a.into()));
            Ok(())
        }
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-removal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A data folder as the app leaves it, plus something of the user's.
    fn data_folder(root: &Path) -> PathBuf {
        let home = root.join("data");
        for d in ["chats", "attachments/c1", "logs", "app-layer/plugins", "plugin-data"] { std::fs::create_dir_all(home.join(d)).unwrap(); }
        for f in ["settings.json", "settings.json.bak", "daemon.json", "trusted.json", "chats/c1.json", "attachments/c1/a.png", "logs/daemon.log", "app-layer/custom.css"] {
            std::fs::write(home.join(f), "x").unwrap();
        }
        std::fs::write(home.join("settings.json.corrupt-20260101-000000000"), "old").unwrap();
        std::fs::write(home.join("my-own-notes.txt"), "mine").unwrap();
        home
    }

    #[test]
    fn only_the_apps_own_entries_are_removed() {
        let root = tmp();
        let home = data_folder(&root);
        let library = root.join("Library");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(library.join("note.md"), "keep").unwrap();
        let p = plan(&home, Some(&library), &[]);
        assert_eq!(p.blocked, None);
        assert_eq!(p.left, ["my-own-notes.txt", "settings.json.corrupt-20260101-000000000"]);
        assert!(p.items.iter().all(|i| i.parent() == Some(home.as_path())));
        let out = remove_data(&p).unwrap();
        assert!(out.failed.is_empty(), "{:?}", out.failed);
        for gone in ["chats", "attachments", "logs", "app-layer", "plugin-data", "settings.json", "settings.json.bak", "daemon.json", "trusted.json"] {
            assert!(!home.join(gone).exists(), "{gone}");
            assert!(out.removed.contains(&gone.to_string()), "{gone}");
        }
        // What the app does not know is left, so the folder stays.
        assert!(home.join("my-own-notes.txt").is_file() && home.join("settings.json.corrupt-20260101-000000000").is_file());
        assert!(!out.folder_removed && home.is_dir());
        assert_eq!(std::fs::read_to_string(library.join("note.md")).unwrap(), "keep");
        // With nothing else in it, the folder goes too.
        std::fs::remove_file(home.join("my-own-notes.txt")).unwrap();
        std::fs::remove_file(home.join("settings.json.corrupt-20260101-000000000")).unwrap();
        std::fs::write(home.join("settings.json"), "rewritten").unwrap();
        let out = remove_data(&plan(&home, Some(&library), &[])).unwrap();
        assert!(out.folder_removed && !home.exists());
        assert!(root.is_dir() && library.join("note.md").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Everything the service keeps in its data folder goes with "Remove Thoughts from this computer",
    /// and is closed to assistant runs even before it exists (`paths::secret_paths_in`) — but for the
    /// two folders customising works in, which the run rules treat on their own.
    #[test]
    fn every_file_the_app_keeps_is_removed_with_it_and_closed_to_runs() {
        const KEPT_FILES: &[&str] = &["settings.json", "settings.json.bak", "daemon.json", "daemon.lock", "git-credentials", "secrets.json",
            "trusted.json", "runs.json", "usage.json", "usage-events.jsonl", "layer-pending.json", "layer-requests.json", "layer-drafts.json",
            "workflows-approved.json", "workflows-state.json", "workflows-library.json", "suggestions.json", "base-manifest.json",
            "update-status.json", "release-check.json", "backup-state.json", "calendar-export.json", "tokens.json", "port", "safe-mode",
            "chat-approvals.json", "models-seen.json", "codex-seen.json", "context-windows.json"];
        const KEPT_DIRS: &[&str] = &["chats", "attachments", "logs", "base", "backups", "extensions", "reorganize-worktree", "wrap-copies",
            "run-tmp", "plugin-data", "history", "app-layer-held", "chats-set-aside"];
        let home = tmp();
        let (files, dirs) = crate::paths::secret_paths_in(&home);
        for f in KEPT_FILES {
            assert!(DATA_FILES.contains(f), "{f} is left behind by the removal");
            assert!(files.contains(&home.join(f)), "{f} is open to assistant runs before it exists");
        }
        for d in KEPT_DIRS {
            assert!(DATA_DIRS.contains(d), "{d} is left behind by the removal");
            assert!(dirs.contains(&home.join(d)), "{d} is open to assistant runs before it exists");
        }
        for d in ["app-layer", "app-layer-staging"] { assert!(DATA_DIRS.contains(&d), "{d}"); }
        std::fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn links_are_removed_never_followed() {
        let root = tmp();
        let home = data_folder(&root);
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precious.md"), "keep").unwrap();
        // A data folder entry that is a link to the user's own folder.
        std::fs::remove_dir_all(home.join("extensions")).ok();
        std::os::unix::fs::symlink(&outside, home.join("extensions")).unwrap();
        std::os::unix::fs::symlink(&outside, home.join("chats/link")).unwrap();
        let out = remove_data(&plan(&home, None, &[])).unwrap();
        assert!(out.removed.contains(&"extensions".to_string()));
        assert_eq!(std::fs::read_to_string(outside.join("precious.md")).unwrap(), "keep");
        // A data folder that is itself a link is not removed at all.
        let linked_home = root.join("linked-data");
        std::os::unix::fs::symlink(&outside, &linked_home).unwrap();
        let p = plan(&linked_home, None, &[]);
        assert_eq!(p.blocked, Some("link"));
        assert!(remove_data(&p).is_err());
        assert!(outside.join("precious.md").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_library_or_linked_folder_inside_the_data_folder_blocks_removal() {
        let root = tmp();
        let home = data_folder(&root);
        // The library kept inside the data folder (as an app folder name, even).
        let inside = home.join("chats");
        let p = plan(&home, Some(&inside), &[]);
        assert_eq!(p.blocked, Some("library-inside"));
        assert!(remove_data(&p).is_err());
        assert!(home.join("chats/c1.json").is_file(), "nothing removed");
        let p = plan(&home, Some(&root.join("elsewhere")), &[home.join("app-layer")]);
        assert_eq!(p.blocked, Some("folder-inside"));
        // The data folder inside the library.
        let p = plan(&home, Some(&root), &[]);
        assert_eq!(p.blocked, Some("inside-library"));
        // The user's home folder is never a data folder to remove.
        if let Some(h) = dirs::home_dir() { assert_eq!(plan(&h, None, &[]).blocked, Some("unsafe")); }
        assert_eq!(plan(Path::new("/"), None, &[]).blocked, Some("unsafe"));
        assert_eq!(plan(Path::new("relative/data"), None, &[]).blocked, Some("unsafe"));
        // A data folder that does not exist: nothing to do.
        let p = plan(&root.join("missing"), None, &[]);
        assert!(p.blocked.is_none() && p.items.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn saved_passwords_and_the_key_are_removed_through_the_keyring() {
        let root = tmp();
        let creds = root.join("git-credentials");
        std::fs::write(&creds, "https://me:secret@gitlab.example.com\nhttps://x%40y:tok@codeberg.org\n").unwrap();
        let hosts = credential_hosts(&["https://github.com/me/notes.git".into(), "git@github.com:me/notes.git".into(), "not a url".into()], &creds);
        assert_eq!(hosts, ["codeberg.org", "github.com", "gitlab.example.com"]);
        let k = FakeKeyring::with(&[("thoughts-ai", "claude", "sk-1"), ("thoughts-git", "github.com", "{}"),
            ("thoughts-git", "gitlab.example.com", "{}"), ("other-app", "claude", "keep")]);
        assert!(remove_secrets(&k, &hosts).is_empty());
        assert_eq!(k.left(), [("other-app".to_string(), "claude".to_string())]);
        let refusing = FakeKeyring { refuse: true, ..FakeKeyring::with(&[("thoughts-ai", "claude", "sk-1")]) };
        assert_eq!(remove_secrets(&refusing, &[]).len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_test_build_never_shares_the_real_apps_keychain() {
        assert!(!shares_with_real_app());
    }
}
