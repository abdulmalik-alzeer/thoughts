//! Keeping the library's history of changes going without anyone having to ask for it.
//!
//! Every library has a history unless the user switched it off (`Settings::history_enabled`).
//! Where it lives is `git.rs`'s business (`git::Repo`); this module makes sure it is *there*:
//!
//! - **When the app starts** and **whenever the settings change** (another library chosen, history
//!   switched back on), a library that has no history yet gets one, kept by the app outside the
//!   folder, with the library as it stands as its first entry. That is the whole migration for
//!   libraries from before history was separate from backup: nothing in the library folder is
//!   created, changed or removed.

use crate::state::AppState;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How long the library is left alone after a save before that save is recorded as a version of
/// its own. A burst of saves (or of ticked todos) is one entry; every pause in the work is one.
/// (Under test nothing is recorded by itself, so tests decide what their history holds.)
pub const SAVE_QUIET: Duration = Duration::from_secs(if cfg!(test) { 24 * 3600 } else { 10 });

/// Something was saved in the library at `root` (a document, a todo, a new file): record it in the
/// history once the library has been quiet for `SAVE_QUIET`, so each saved version can be found
/// and restored later instead of being folded into whatever is recorded next.
pub fn record_soon(state: &Arc<AppState>, root: &Path) { record_after(state, root, SAVE_QUIET) }

/// `record_soon` with the quiet time given. A newer save of the same library starts the wait again;
/// while the assistant works there, or the library is held by a wrap or another save, it waits on.
pub fn record_after(state: &Arc<AppState>, root: &Path, quiet: Duration) {
    // Per library, the number of the latest save: only the task of the latest one records.
    static LATEST: std::sync::Mutex<std::collections::BTreeMap<PathBuf, u64>> = std::sync::Mutex::new(std::collections::BTreeMap::new());
    let key = root.to_path_buf();
    let mine = {
        let mut latest = LATEST.lock().unwrap_or_else(|e| e.into_inner());
        let n = latest.entry(key.clone()).or_insert(0);
        *n += 1;
        *n
    };
    let state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(quiet).await;
        if LATEST.lock().unwrap_or_else(|e| e.into_inner()).get(&key) != Some(&mine) { return; }
        if !record_saved(&state, &key).await { record_after(&state, &key, quiet); }
    });
}

/// Record the user's saved changes now, when nothing stands in the way. `false`: not now (the
/// assistant is working in the library, or a wrap or a save holds it), try again later.
async fn record_saved(state: &Arc<AppState>, root: &Path) -> bool {
    // Another library was chosen meanwhile, or the history was switched off: nothing to do.
    let current = state.settings.read().await.library().map(|l| crate::paths::canonical_lenient(&l));
    if current.as_deref() != Some(crate::paths::canonical_lenient(root).as_path()) { return true; }
    // The assistant's half-written files are not the user's version; its own entry follows its work.
    if state.run_active_in(root) { return false; }
    let Ok(_guard) = state.library_lock.clone().try_lock_owned() else { return false };
    let before = crate::git::head(state, root).await;
    crate::claude::record_library_locked(state, root, &crate::git::subject::edit("your changes")).await;
    if crate::git::head(state, root).await != before { state.emit(serde_json::json!({"type": "history.changed"})); }
    true
}

/// Give the configured library a history when it should have one and has none. Returns whether
/// the library has a history afterwards. Never touches a library that is not set up, is missing,
/// has history switched off, or is being changed right now (the next start, settings change or
/// recorded change tries again).
pub async fn ensure_for_library(state: &AppState) -> bool {
    let s = state.settings.read().await.clone();
    if !s.onboarded || !s.history_enabled() { return false; }
    let Some(lib) = s.library().filter(|l| l.is_dir()) else { return false };
    if crate::git::repo_of(&lib) != crate::git::Repo::Missing { return crate::git::is_repo(state, &lib).await; }
    // Half-written files of an assistant at work, or of a wrap being saved, are not a starting point.
    if state.library_busy() || state.run_active_in(&lib) { return false; }
    match crate::git::start_history(state, &lib).await {
        Ok(()) => {
            crate::log::info("history: this library had no history of changes; one has been started");
            state.emit(serde_json::json!({"type": "library.changed"}));
            true
        }
        Err(e) => { crate::log::warn(format!("history: could not start a history for the library ({e})")); false }
    }
}

/// Start the background work: the check at startup, and again after every settings change.
pub fn start(state: Arc<AppState>) {
    let mut events = state.events.subscribe();
    tokio::spawn(async move {
        ensure_for_library(&state).await;
        loop {
            match events.recv().await {
                Ok(e) if e["type"] == "settings.changed" => { ensure_for_library(&state).await; }
                Ok(_) => {}
                // Events were missed while this fell behind: look again rather than guess.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => { ensure_for_library(&state).await; }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;
    use crate::wraps::testutil as t;
    use std::path::{Path, PathBuf};

    /// Every file under `dir` with its content, and every folder: the library exactly as it is on disk.
    fn snapshot(dir: &Path) -> Vec<(String, Option<Vec<u8>>)> {
        let mut out: Vec<(String, Option<Vec<u8>>)> = walkdir::WalkDir::new(dir).min_depth(1).into_iter().flatten().map(|e| {
            let rel = e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
            (rel, e.file_type().is_file().then(|| std::fs::read(e.path()).unwrap()))
        }).collect();
        out.sort();
        out
    }

    /// A library from before 0.0.9 that never had a history: plain files, no repository anywhere.
    async fn plain_library(mode: &str) -> (Arc<AppState>, PathBuf, PathBuf) {
        let root = dunce::canonicalize(std::env::temp_dir()).unwrap().join(format!("thoughts-hist-{}", uuid::Uuid::new_v4()));
        let lib = root.join("My Notes");
        t::write(&lib, "INDEX.md", "# Index\n");
        t::write(&lib, "TODO.md", "- [ ] call the bank\n");
        t::write(&lib, "أفكار/متجر/README.md", "---\ntitle: متجر\n---\nفكرة.\n");
        t::write(&lib, ".DS_Store", "finder");
        let st = Arc::new(AppState::for_test());
        {
            let mut s = st.settings.write().await;
            s.library_path = Some(lib.to_string_lossy().to_string());
            s.onboarded = true;
            s.sync.mode = mode.into();
        }
        (st, lib, root)
    }

    #[tokio::test]
    async fn an_existing_library_without_history_gains_one_and_its_files_are_not_touched() {
        for mode in ["none", "folder"] {
            let (st, lib, root) = plain_library(mode).await;
            let before = snapshot(&lib);
            assert_eq!(git::repo_of(&lib), git::Repo::Missing);
            assert!(ensure_for_library(&st).await, "{mode}");
            // Nothing was added to, changed in or removed from the library: no `.git`, no ignore file.
            assert_eq!(snapshot(&lib), before, "{mode}");
            // The history lives under the app's own folder, and its first entry is the library as it was.
            let git::Repo::Kept(store) = git::repo_of(&lib) else { panic!("the history should be kept by the app") };
            assert!(store.starts_with(crate::paths::history_dir()) && !store.starts_with(&lib));
            let h = git::history(&st, &lib, None, 50).await.unwrap();
            assert_eq!(h.len(), 1);
            assert_eq!((h[0]["kind"].as_str(), h[0]["first"].as_bool()), (Some("start"), Some(true)));
            let mut files: Vec<&str> = h[0]["files"].as_array().unwrap().iter().filter_map(|f| f.as_str()).collect();
            files.sort();
            assert_eq!(files, ["INDEX.md", "TODO.md", "أفكار/متجر/README.md"], "system clutter is left out; Arabic names are readable");
            // Running it again (every start does) changes nothing.
            assert!(ensure_for_library(&st).await);
            assert_eq!(git::history(&st, &lib, None, 50).await.unwrap().len(), 1);
            assert_eq!(snapshot(&lib), before);
            // From here on changes are recorded and can be undone — the point of having a history.
            t::write(&lib, "TODO.md", "- [x] call the bank\n");
            crate::claude::record_library(&st, &lib, &git::subject::edit("your changes"), true).await;
            let h = git::history(&st, &lib, None, 50).await.unwrap();
            assert_eq!(h.len(), 2);
            git::undo_commit(&st, &lib, h[0]["hash"].as_str().unwrap()).await.unwrap();
            assert_eq!(t::read(&lib, "TODO.md"), "- [ ] call the bank\n");
            // The starting point itself cannot be undone: that would delete the whole library.
            let first = h[1]["hash"].as_str().unwrap();
            let err = git::undo_commit(&st, &lib, first).await.unwrap_err();
            assert_eq!(crate::tr::classify_in(false, &err).status, 409);
            assert!(lib.join("INDEX.md").exists());
            assert!(!lib.join(".git").exists());
            std::fs::remove_dir_all(&store).unwrap();
            std::fs::remove_dir_all(&root).unwrap();
        }
    }

    #[tokio::test]
    async fn a_library_that_syncs_with_git_keeps_its_own_repository_untouched() {
        let (st, lib) = t::library().await;
        st.settings.write().await.onboarded = true;
        let head = t::sh(&lib, &["rev-parse", "HEAD"]);
        let config = std::fs::read(lib.join(".git/config")).unwrap();
        assert_eq!(git::repo_of(&lib), git::Repo::Own);
        assert!(ensure_for_library(&st).await);
        // No second history beside it, no extra entry in it, its settings as they were.
        assert_eq!(git::repo_of(&lib), git::Repo::Own);
        assert!(!git::history_store(&lib).exists());
        assert_eq!(t::sh(&lib, &["rev-parse", "HEAD"]), head);
        assert_eq!(std::fs::read(lib.join(".git/config")).unwrap(), config);
        assert_eq!(t::sh(&lib, &["status", "--porcelain"]), "");
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn reorganize_works_on_a_history_kept_by_the_app() {
        let (st, lib, root) = plain_library("none").await;
        assert!(ensure_for_library(&st).await);
        let git::Repo::Kept(store) = git::repo_of(&lib) else { panic!("the history should be kept by the app") };
        let wt = root.join("working copy");
        let (base, branch) = crate::wraps::reorganize_prepare(&st, &lib, &wt).await.unwrap();
        assert_eq!(base, "main");
        // The proposal is made in the working copy; the library stays as it is until it is kept.
        std::fs::rename(wt.join("TODO.md"), wt.join("tasks.md")).unwrap();
        git::commit_all(&st, &wt, "reorganize: proposed structure").await.unwrap();
        assert!(lib.join("TODO.md").exists() && !lib.join("tasks.md").exists());
        let stat = git::git(&st, &lib, &["diff", "--stat", "-M", &format!("{base}...{branch}"), "--"]).await.unwrap();
        assert!(stat.contains("tasks.md"), "{stat}");
        t::write(&lib, "notes/today.md", "written while reviewing\n");
        crate::wraps::reorganize_conclude(&st, &lib, &wt, &branch, true).await.unwrap();
        assert!(lib.join("tasks.md").exists() && !lib.join("TODO.md").exists());
        assert_eq!(t::read(&lib, "notes/today.md"), "written while reviewing\n");
        assert!(!wt.exists() && !lib.join(".git").exists());
        assert_eq!(git::git(&st, &lib, &["status", "--porcelain"]).await.unwrap(), "");
        std::fs::remove_dir_all(&store).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn every_save_becomes_a_version_once_the_editing_pauses() {
        let (st, lib, root) = plain_library("none").await;
        assert!(ensure_for_library(&st).await);
        let git::Repo::Kept(store) = git::repo_of(&lib) else { panic!("the history should be kept by the app") };
        let quiet = Duration::from_millis(60);
        let versions = || async { git::history(&st, &lib, Some("notes/plan.md"), 50).await.unwrap() };
        let wait = || tokio::time::sleep(Duration::from_millis(400));
        let mut events = st.events.subscribe();
        // Saved at 10:00 …
        t::write(&lib, "notes/plan.md", "the ten o'clock text\n");
        record_after(&st, &lib, quiet);
        wait().await;
        assert_eq!(versions().await.len(), 1, "recorded without opening History");
        assert_eq!(events.try_recv().unwrap()["type"], "history.changed");
        // … overwritten and saved again at 11:00: both versions are there to go back to.
        t::write(&lib, "notes/plan.md", "the eleven o'clock text\n");
        record_after(&st, &lib, quiet);
        wait().await;
        let v = versions().await;
        assert_eq!(v.len(), 2);
        assert_eq!(v[0]["subject"], "edit: your changes");
        assert_eq!(git::file_at(&st, &lib, "notes/plan.md", v[1]["hash"].as_str().unwrap()).await.unwrap(), b"the ten o'clock text\n");

        // A burst of saves is one version, recorded after the last of them.
        for n in 1..=5 {
            t::write(&lib, "TODO.md", &format!("- [ ] call the bank\n- [x] task {n}\n"));
            record_after(&st, &lib, Duration::from_millis(150));
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        let before = git::history(&st, &lib, None, 500).await.unwrap().len();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let all = git::history(&st, &lib, None, 500).await.unwrap();
        assert_eq!(all.len(), before + 1);
        assert_eq!(all[0]["files"], serde_json::json!(["TODO.md"]));

        // While a wrap or another save holds the library, or the assistant works in it, the
        // version waits — and is recorded when the way is clear.
        let held = st.library_lock.clone().lock_owned().await;
        st.run_dirs.lock().unwrap().insert("chat".into(), lib.clone());
        t::write(&lib, "notes/plan.md", "typed while a wrap was saving\n");
        record_after(&st, &lib, quiet);
        wait().await;
        assert_eq!(versions().await.len(), 2);
        drop(held);
        wait().await;
        assert_eq!(versions().await.len(), 2, "not while the assistant is at work");
        st.run_dirs.lock().unwrap().clear();
        wait().await;
        assert_eq!(versions().await.len(), 3);

        // Nothing is recorded with the history switched off, or for a library no longer chosen.
        st.settings.write().await.keep_history = false;
        t::write(&lib, "notes/plan.md", "history is off\n");
        record_after(&st, &lib, quiet);
        wait().await;
        st.settings.write().await.keep_history = true;
        assert_eq!(versions().await.len(), 3);
        st.settings.write().await.library_path = Some(root.join("another").to_string_lossy().to_string());
        record_after(&st, &lib, quiet);
        wait().await;
        assert_eq!(versions().await.len(), 3);
        std::fs::remove_dir_all(&store).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn nothing_is_started_when_history_is_off_or_the_library_is_not_ready() {
        // Switched off by the user.
        let (st, lib, root) = plain_library("none").await;
        st.settings.write().await.keep_history = false;
        assert!(!ensure_for_library(&st).await);
        assert_eq!(git::repo_of(&lib), git::Repo::Missing);
        // Setup not finished yet.
        { let mut s = st.settings.write().await; s.keep_history = true; s.onboarded = false; }
        assert!(!ensure_for_library(&st).await);
        // The assistant is in the middle of writing there.
        st.settings.write().await.onboarded = true;
        st.run_dirs.lock().unwrap().insert("chat".into(), lib.clone());
        assert!(!ensure_for_library(&st).await);
        assert_eq!(git::repo_of(&lib), git::Repo::Missing);
        st.run_dirs.lock().unwrap().clear();
        // The folder is gone (a drive that is not connected).
        st.settings.write().await.library_path = Some(root.join("not here").to_string_lossy().to_string());
        assert!(!ensure_for_library(&st).await);
        assert!(!git::history_store(&root.join("not here")).exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
