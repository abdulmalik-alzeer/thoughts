//! Changes made to the library by other programs — another editor, a phone's notes app, a cloud
//! drive bringing in what another computer saved — noticed as they happen.
//!
//! One recursive watch on the open library's folder (`notify`: FSEvents on a Mac, inotify on Linux,
//! ReadDirectoryChanges on Windows; never a poll of each file). What it reports is collected for a
//! moment (`QUIET` without news, `MOST` at the longest) and then:
//!
//! - the open views are told the library changed (the same `library.changed` every save sends), and
//! - when the library keeps a history, what was changed is recorded as an entry of its own,
//!   "Changed outside Thoughts" (`git::subject::outside`), instead of being swept into whatever the
//!   app records next.
//!
//! **The app's own writes are never recorded as someone else's.** Two things keep them apart, and a
//! batch of changes is only recorded when neither says the app was at work:
//!
//! 1. *The app says when it changes the library.* Every save, wrap, sync, undo, import or todo edit
//!    tells the open views so (`library.changed`, `todos.changed`, `history.changed`, `wrap.*`…).
//!    Any such news from the time the batch was collected (a few seconds of slack before it) marks
//!    the batch as the app's: its changes are left for the app's own entry, exactly as before this
//!    watcher existed. So does an assistant run working in the library (its files become the run's
//!    entry), and a library held by a wrap or sync is waited for.
//! 2. *A ledger of what the app wrote* (`note_write`, called by `paths::write_atomic`, which saves
//!    documents, todos and most of the app's other files): a file whose size and time are still the
//!    ones the app gave it is the app's, even if its news came late.
//!
//! The first is what makes it robust: it needs no list of every place the app writes (there are
//! hundreds), and in doubt it does what the app always did. The cost is that a change made elsewhere
//! at the very moment the app saves something goes into the app's next entry instead of its own.
//!
//! Recording takes the library's lock (and waits for edits under way), never while an assistant run
//! works there or a combine of changes stopped halfway; when busy it tries again a little later. It
//! goes through `git add`, so the library's own `.gitignore` and the history's list of what to leave
//! out are respected; private folders are recorded like every other save records them. Git's own
//! folder, the app's search index, iCloud placeholders and editors' temporary and swap files are
//! never looked at (`ignored`), and a file iCloud only moved to the cloud (`evicted`) is not taken for
//! a deleted one.

use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// A batch is recorded once nothing new arrived for this long…
pub const QUIET: Duration = Duration::from_secs(2);
/// …or once it has been collecting for this long, whatever keeps arriving.
pub const MOST: Duration = Duration::from_secs(30);
/// News from the app this long before a batch began still marks it as the app's.
pub const SLACK: Duration = Duration::from_secs(3);
/// A busy library is tried again after this long…
const RETRY: Duration = Duration::from_secs(3);
/// …this many times (five minutes), and then the batch is left for the app's next entry.
const MAX_TRIES: u32 = 100;
/// More paths than this are recorded by their folders (one `git add` each).
pub const MAX_SPECS: usize = 200;
/// How long the ledger remembers a write of the app's.
const LEDGER_TTL: Duration = Duration::from_secs(120);

// ---------- what is never looked at ----------

/// Folders whose insides are never looked at: git's own, the app's search index, and the folders
/// the file list leaves out too (`library::SKIP`).
const SKIP_DIRS: &[&str] = &[".git", ".thoughts-search", "node_modules", "target", ".claude",
    ".Trash", ".Trashes", ".Spotlight-V100", ".fseventsd", ".DocumentRevisions-V100", ".TemporaryItems",
    "$RECYCLE.BIN", "System Volume Information", ".dropbox.cache", ".stversions"];

/// Is the library-relative `rel` something no one wrote as a change of their own: a file inside a
/// folder above, a system or cloud-drive leftover, an iCloud placeholder, or an editor's temporary,
/// lock or swap file (the editor's real save follows it)?
pub fn ignored(rel: &str) -> bool {
    let rel = rel.trim_matches('/');
    if rel.is_empty() { return true; }
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.iter().any(|p| SKIP_DIRS.contains(p) || p.starts_with(".git")) { return true; }
    // Obsidian rewrites where its panes are on every click; that is not a change to the notes.
    if parts.len() >= 2 && parts[parts.len() - 2] == ".obsidian" && parts[parts.len() - 1].starts_with("workspace") { return true; }
    let name = parts[parts.len() - 1];
    let lower = name.to_lowercase();
    matches!(lower.as_str(), ".ds_store" | "thumbs.db" | "desktop.ini" | "4913" | ".dropbox" | ".dropbox.attr" | "icon\r")
        || lower.ends_with(".icloud")
        || lower.ends_with(".tmp") || lower.ends_with(".temp")
        || lower.ends_with(".swp") || lower.ends_with(".swo") || lower.ends_with(".swx") || lower.ends_with(".swn")
        || lower.ends_with('~') || lower.starts_with(".#") || (lower.starts_with('#') && lower.ends_with('#'))
        || lower.starts_with(".~lock.") || lower.starts_with("~$")
        || lower.ends_with(".crdownload") || lower.ends_with(".part") || lower.ends_with(".download")
        || lower.contains(".sb-") || lower.starts_with(".goutputstream-") || lower.starts_with(".syncthing.")
}

/// Did iCloud only move `rel` to the cloud ("Optimise Mac Storage")? Then the file is gone from the
/// disk but a placeholder `.<name>.icloud` stands beside it: the note was not deleted.
pub fn evicted(rel: &str, exists: impl Fn(&str) -> bool) -> bool {
    if exists(rel) { return false; }
    let (dir, name) = match rel.rsplit_once('/') { Some((d, n)) => (format!("{d}/"), n), None => (String::new(), rel) };
    exists(&format!("{dir}.{name}.icloud"))
}

// ---------- collecting ----------

/// The changes reported since the last batch was handled.
#[derive(Debug, Clone)]
pub struct Batch {
    pub paths: BTreeSet<String>,
    pub first: Instant,
    pub last: Instant,
    /// The app was at work in the library while this was collected (an assistant run).
    pub tainted: bool,
    /// The system lost track of what changed (too much at once): only a refresh is safe.
    pub rescan: bool,
}

impl Batch {
    pub fn new(now: Instant) -> Self { Batch { paths: BTreeSet::new(), first: now, last: now, tainted: false, rescan: false } }
    pub fn add(&mut self, rel: String, now: Instant) { self.paths.insert(rel); self.last = now; }
    /// Is it time to handle it: quiet for `QUIET`, or collecting for `MOST`?
    pub fn due(&self, now: Instant) -> bool { now.duration_since(self.last) >= QUIET || now.duration_since(self.first) >= MOST }
    /// When it will be due if nothing more arrives.
    pub fn due_at(&self) -> Instant { (self.last + QUIET).min(self.first + MOST) }
    /// Did news of the app's own at `app` (the latest) come while this was collected?
    pub fn apps(&self, app: Option<Instant>) -> bool { self.tainted || app.map_or(false, |t| t + SLACK >= self.first) }
}

/// At most `max` paths to record `paths` by: the paths themselves, or, for a big batch (a cloud drive
/// bringing in a whole folder), the folders they are in, as near the top as needed.
pub fn coalesce(paths: &BTreeSet<String>, max: usize) -> Vec<String> {
    let mut depth = paths.iter().map(|p| p.split('/').count()).max().unwrap_or(0);
    loop {
        let cut: BTreeSet<String> = paths.iter().map(|p| p.split('/').take(depth.max(1)).collect::<Vec<_>>().join("/")).collect();
        // A path whose folder is also listed is covered by it.
        let kept: Vec<String> = cut.iter().filter(|p| !cut.iter().any(|q| q != *p && p.starts_with(&format!("{q}/")))).cloned().collect();
        if kept.len() <= max || depth <= 1 { return kept; }
        depth -= 1;
    }
}

/// The library-relative path of `abs`, a path the system reported, when it is inside the library
/// (its folder as named, or as it really is: FSEvents reports `/private/var/…` for `/var/…`).
pub fn rel_of(roots: &[PathBuf], abs: &Path) -> Option<String> {
    roots.iter().find_map(|r| abs.strip_prefix(r).ok()).map(|p| p.to_string_lossy().replace('\\', "/")).filter(|p| !p.is_empty())
}

/// Is `event` news of the app's own that the library changed (and not this watcher's)?
pub fn from_app(event: &Value) -> bool {
    if event["outside"] == true { return false; }
    let t = event["type"].as_str().unwrap_or("");
    ["library.", "todos.", "history.", "wrap.", "reorganize.", "workflows.", "automations.", "inbox."].iter().any(|p| t.starts_with(p))
        || matches!(t, "chat.done" | "setup.finished")
}

// ---------- the ledger of the app's own writes ----------

/// A file as it was left: its size and modification time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stamp { pub len: u64, pub modified: Option<SystemTime> }

impl Stamp {
    pub fn of(p: &Path) -> Option<Stamp> { std::fs::metadata(p).ok().map(|m| Stamp { len: m.len(), modified: m.modified().ok() }) }
}

/// What the app wrote lately: real path → (how it left the file, when).
#[derive(Default)]
pub struct Ledger { written: HashMap<PathBuf, (Stamp, Instant)> }

impl Ledger {
    pub fn note(&mut self, path: PathBuf, stamp: Stamp, now: Instant) {
        if self.written.len() > 4096 { self.written.retain(|_, (_, at)| now.duration_since(*at) < LEDGER_TTL); }
        self.written.insert(path, (stamp, now));
    }
    /// Is `path` still exactly as the app wrote it, lately?
    pub fn confirms(&self, path: &Path, now: Option<Stamp>, at: Instant) -> bool {
        match (self.written.get(path), now) {
            (Some((s, when)), Some(now)) => at.duration_since(*when) < LEDGER_TTL && *s == now && s.modified.is_some(),
            _ => false,
        }
    }
}

fn ledger() -> std::sync::MutexGuard<'static, Ledger> {
    static L: std::sync::OnceLock<std::sync::Mutex<Ledger>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// The app just wrote the file at `path` (see `paths::write_atomic`).
pub fn note_write(path: &Path) {
    let Some(stamp) = Stamp::of(path) else { return };
    let real = crate::paths::canonical_lenient(path);
    ledger().note(real, stamp, Instant::now());
}

/// Is the file at `abs` the app's own latest write?
fn app_wrote(abs: &Path) -> bool {
    let real = crate::paths::canonical_lenient(abs);
    ledger().confirms(&real, Stamp::of(&real), Instant::now())
}

// ---------- handling a batch ----------

/// What became of a batch.
#[derive(Debug, PartialEq)]
enum Handled {
    /// Nothing anyone else changed.
    Nothing,
    /// The views were told; the history was left as it is (the app's own changes, no history kept…).
    Refreshed,
    /// Recorded as an entry of its own.
    Recorded,
    /// The library is busy: try again a little later.
    Later,
}

/// Handle a batch for the library at `root`, `app` being the latest news of the app's own.
async fn handle(state: &AppState, root: &Path, b: &Batch, app: Option<Instant>) -> Handled {
    let fresh: Vec<String> = b.paths.iter()
        .filter(|p| !app_wrote(&root.join(p)))
        .filter(|p| !evicted(p, |q| root.join(q).exists()))
        .cloned().collect();
    if fresh.is_empty() && !b.rescan { return Handled::Nothing; }
    let (open, on) = { let s = state.settings.read().await; (s.is_open_library(root), s.history_enabled_in(root)) };
    // The app's own work (or an assistant run's): its own entry records it, as always.
    if !open || !on || b.rescan || fresh.is_empty() || b.apps(app) || state.run_active_in(root) { return Handled::Refreshed; }
    // Never into a combine of changes that stopped halfway (Repair history settles it first).
    if crate::repair::unfinished(root).is_some() { return Handled::Later; }
    let Ok(_lib) = state.library_lock.of(root).try_lock_owned() else { return Handled::Later };
    // Edits already admitted finish first; none starts while this records.
    let Ok(_files) = state.sync_files.try_write() else { return Handled::Later };
    if crate::wraps::interrupted_wrap_in(state, root).await.is_some() { return Handled::Refreshed; }
    if !crate::git::ready(state, root).await { return Handled::Refreshed; }
    let _g = state.git_lock.lock().await;
    if let Err(e) = crate::trust::require_git(root) { crate::log::warn(format!("history: not recording changes made outside the app ({e})")); return Handled::Refreshed; }
    match crate::git::commit_paths(state, root, &crate::git::subject::outside(), &coalesce(&fresh.into_iter().collect(), MAX_SPECS)).await {
        Ok(true) => { crate::log::info("history: recorded changes made outside the app"); Handled::Recorded }
        Ok(false) => Handled::Refreshed,
        Err(e) => { crate::log::warn(format!("history: could not record changes made outside the app ({e})")); Handled::Refreshed }
    }
}

/// Tell the views (and, when an entry was made, History).
fn tell(state: &AppState, how: &Handled) {
    if matches!(how, Handled::Nothing | Handled::Later) { return; }
    state.emit(json!({"type": "library.changed", "outside": true}));
    state.emit(json!({"type": "todos.changed", "outside": true}));
    if *how == Handled::Recorded { state.emit(json!({"type": "history.changed", "outside": true})); }
}

// ---------- the watch ----------

enum Seen { Path(PathBuf), Rescan }

/// The watch on one library folder, and what it reports.
struct Watching {
    root: PathBuf,
    roots: Vec<PathBuf>,
    _watcher: notify::RecommendedWatcher,
    rx: tokio::sync::mpsc::UnboundedReceiver<Seen>,
}

fn watch(root: &Path) -> Option<Watching> {
    use notify::{EventKind, RecursiveMode, Watcher};
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handler = move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if ev.need_rescan() { let _ = tx.send(Seen::Rescan); }
        // Reading a file, or only its access time changing, changes nothing.
        if matches!(ev.kind, EventKind::Access(_) | EventKind::Modify(notify::event::ModifyKind::Metadata(notify::event::MetadataKind::AccessTime))) { return; }
        for p in ev.paths { let _ = tx.send(Seen::Path(p)); }
    };
    let mut w = match notify::recommended_watcher(handler) {
        Ok(w) => w,
        Err(e) => { crate::log::warn(format!("watch: cannot watch the library for changes made elsewhere ({e})")); return None; }
    };
    if let Err(e) = w.watch(root, RecursiveMode::Recursive) {
        crate::log::warn(format!("watch: cannot watch the library for changes made elsewhere ({e})"));
        return None;
    }
    let real = crate::paths::canonical_lenient(root);
    let roots = if real == root { vec![root.to_path_buf()] } else { vec![real, root.to_path_buf()] };
    Some(Watching { root: root.to_path_buf(), roots, _watcher: w, rx })
}

/// Start watching the open library, following it when another one is opened.
pub fn start(state: Arc<AppState>) {
    // Under test only the tests that ask for it run the watch (`run`).
    if cfg!(test) { return; }
    tokio::spawn(run(state));
}

async fn run(state: Arc<AppState>) {
    let mut events = state.events.subscribe();
    {
        let mut watching: Option<Watching> = None;
        let mut tried_for: Option<PathBuf> = None;
        let mut batch: Option<Batch> = None;
        let mut retry: Option<(Instant, u32)> = None;
        let mut app: Option<Instant> = None;
        loop {
            // Watch the open library (and only it).
            let open = { state.settings.read().await.library() }.filter(|l| l.is_dir());
            if open.as_deref() != watching.as_ref().map(|w| w.root.as_path()) && open != tried_for {
                watching = open.as_deref().and_then(watch);
                tried_for = open.clone();
                batch = None;
                retry = None;
            }
            let wake = retry.map(|(at, _)| at).or_else(|| batch.as_ref().map(Batch::due_at))
                .unwrap_or_else(|| Instant::now() + Duration::from_secs(3600));
            tokio::select! {
                seen = async { match watching.as_mut() { Some(w) => w.rx.recv().await, None => std::future::pending().await } } => {
                    let Some(seen) = seen else { watching = None; continue };
                    let Some(w) = watching.as_ref() else { continue };
                    let now = Instant::now();
                    let b = batch.get_or_insert_with(|| Batch::new(now));
                    match seen {
                        Seen::Rescan => { b.rescan = true; b.last = now; }
                        Seen::Path(p) => match rel_of(&w.roots, &p) {
                            Some(rel) if !ignored(&rel) => b.add(rel, now),
                            _ => {}
                        },
                    }
                    if state.run_active_in(&w.root) { b.tainted = true; }
                    if b.paths.is_empty() && !b.rescan { batch = None; }
                }
                e = events.recv() => match e {
                    Ok(v) => {
                        if from_app(&v) { app = Some(Instant::now()); }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => { app = Some(Instant::now()); }
                    Err(_) => return,
                },
                _ = tokio::time::sleep_until(wake.into()) => {
                    let (Some(w), Some(b)) = (watching.as_ref(), batch.as_ref()) else { retry = None; continue };
                    if retry.is_none() && !b.due(Instant::now()) { continue; }
                    let how = handle(&state, &w.root, b, app).await;
                    if how == Handled::Later {
                        let tries = retry.map_or(1, |(_, n)| n + 1);
                        if tries <= MAX_TRIES { retry = Some((Instant::now() + RETRY, tries)); continue; }
                        tell(&state, &Handled::Refreshed);
                    } else {
                        tell(&state, &how);
                    }
                    batch = None;
                    retry = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_no_one_wrote_as_a_change_is_never_looked_at() {
        for p in [".git/index", ".git/objects/ab/cd", ".github/x.yml", "notes/.DS_Store", ".thoughts-search/db",
                  "ideas/.plan.md.icloud", "ideas/.plan.md.1f2e3d.tmp", "ideas/.plan.md.swp", "ideas/plan.md~",
                  "ideas/.#plan.md", "ideas/#plan.md#", "ideas/4913", "~$report.docx", ".~lock.sheet.ods#",
                  "ideas/plan.md.sb-1a2b3c-xyz", "a/node_modules/x/y.js", ".obsidian/workspace.json",
                  ".obsidian/workspace-mobile.json", "Downloads/file.crdownload", ".claude/settings.json", ""] {
            assert!(ignored(p), "{p}");
        }
        for p in ["ideas/plan.md", "ideas/log.md", "TODO.md", "ملاحظات/فكرة.md", ".obsidian/app.json", "notes/swap.md", "images/a.png", "tmp/notes.md"] {
            assert!(!ignored(p), "{p}");
        }
    }

    #[test]
    fn a_file_icloud_moved_to_the_cloud_is_not_a_deleted_one() {
        let on_disk = |set: &'static [&'static str]| move |p: &str| set.contains(&p);
        assert!(evicted("ideas/plan.md", on_disk(&["ideas/.plan.md.icloud"])));
        assert!(evicted("TODO.md", on_disk(&[".TODO.md.icloud"])));
        assert!(!evicted("ideas/plan.md", on_disk(&[])), "really deleted");
        assert!(!evicted("ideas/plan.md", on_disk(&["ideas/plan.md", "ideas/.plan.md.icloud"])), "there");
    }

    #[test]
    fn a_batch_waits_for_quiet_and_never_longer_than_the_most() {
        let t0 = Instant::now();
        let mut b = Batch::new(t0);
        b.add("a.md".into(), t0);
        assert!(!b.due(t0 + Duration::from_millis(1500)));
        assert!(b.due(t0 + QUIET));
        // Something new keeps arriving every second: handled after MOST all the same.
        let mut at = t0;
        while at < t0 + MOST { at += Duration::from_secs(1); b.add(format!("f{}.md", b.paths.len()), at); }
        assert!(b.due(at));
        assert_eq!(b.due_at(), t0 + MOST);
        // The same file reported over and over is one path.
        let mut c = Batch::new(t0);
        for i in 0..50 { c.add("a.md".into(), t0 + Duration::from_millis(i * 10)); }
        assert_eq!(c.paths.len(), 1);
    }

    #[test]
    fn news_from_the_app_while_a_batch_collects_makes_it_the_apps() {
        let t0 = Instant::now() + Duration::from_secs(60);
        let b = Batch::new(t0);
        assert!(!b.apps(None));
        assert!(!b.apps(Some(t0 - SLACK - Duration::from_secs(1))), "long before");
        assert!(b.apps(Some(t0 - Duration::from_secs(1))), "just before: the write came first");
        assert!(b.apps(Some(t0 + Duration::from_secs(1))));
        let mut run = Batch::new(t0);
        run.tainted = true;
        assert!(run.apps(None));
        // The app's news counts; this watcher's own does not, nor news about chats alone.
        assert!(from_app(&json!({"type": "library.changed"})));
        assert!(from_app(&json!({"type": "todos.changed"})));
        assert!(from_app(&json!({"type": "wrap.done", "chatId": "x"})));
        assert!(from_app(&json!({"type": "chat.done"})));
        assert!(!from_app(&json!({"type": "library.changed", "outside": true})));
        assert!(!from_app(&json!({"type": "chat.message"})));
        assert!(!from_app(&json!({"type": "incoming.changed"})));
    }

    #[test]
    fn a_big_batch_is_recorded_by_its_folders() {
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<BTreeSet<String>>();
        assert_eq!(coalesce(&set(&["b.md", "a/x.md"]), 10), vec!["a/x.md", "b.md"]);
        let many: BTreeSet<String> = (0..500).map(|i| format!("photos/2026/{i}.jpg")).chain(["notes/a.md".to_string()]).collect();
        assert_eq!(coalesce(&many, MAX_SPECS), vec!["notes/a.md", "photos/2026"]);
        let wide: BTreeSet<String> = (0..300).map(|i| format!("d{i}/x.md")).collect();
        assert_eq!(coalesce(&wide, MAX_SPECS).len(), 300, "never coarser than the top level");
        // A folder listed together with files inside it covers them.
        assert_eq!(coalesce(&set(&["a", "a/x.md", "a/y/z.md"]), 10), vec!["a"]);
    }

    #[test]
    fn the_ledger_knows_only_what_the_app_left_as_it_was() {
        let now = Instant::now();
        let t = SystemTime::now();
        let mut l = Ledger::default();
        let p = PathBuf::from("/lib/ideas/plan.md");
        let mine = Stamp { len: 10, modified: Some(t) };
        l.note(p.clone(), mine, now);
        assert!(l.confirms(&p, Some(mine), now + Duration::from_secs(5)));
        // Changed since by someone else: another size or time.
        assert!(!l.confirms(&p, Some(Stamp { len: 11, modified: Some(t) }), now));
        assert!(!l.confirms(&p, Some(Stamp { len: 10, modified: Some(t + Duration::from_secs(1)) }), now));
        // Deleted since, never written by the app, or written long ago.
        assert!(!l.confirms(&p, None, now));
        assert!(!l.confirms(Path::new("/lib/other.md"), Some(mine), now));
        assert!(!l.confirms(&p, Some(mine), now + LEDGER_TTL + Duration::from_secs(1)));
    }

    #[test]
    fn paths_are_taken_relative_to_the_library_however_the_system_names_it() {
        let roots = vec![PathBuf::from("/private/var/lib"), PathBuf::from("/var/lib")];
        assert_eq!(rel_of(&roots, Path::new("/private/var/lib/ideas/a.md")).as_deref(), Some("ideas/a.md"));
        assert_eq!(rel_of(&roots, Path::new("/var/lib/b.md")).as_deref(), Some("b.md"));
        assert_eq!(rel_of(&roots, Path::new("/var/lib")), None);
        assert_eq!(rel_of(&roots, Path::new("/elsewhere/c.md")), None);
    }

    #[test]
    fn the_entry_is_named_plainly_and_history_tells_it_apart() {
        let s = crate::git::subject::outside();
        assert_eq!(s, "outside: changed outside Thoughts");
        assert_eq!(crate::git::commit_kind(&s), "outside");
    }

    /// A library with a history, the app's state open on it.
    async fn library() -> (crate::state::AppState, PathBuf) {
        let lib = std::env::temp_dir().join(format!("thoughts-watch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(lib.join("ideas")).unwrap();
        std::fs::write(lib.join("ideas/plan.md"), "plan\n").unwrap();
        std::fs::write(lib.join("TODO.md"), "- [ ] a\n").unwrap();
        let st = crate::state::AppState::for_test();
        { let mut s = st.settings.write().await; s.library_path = Some(lib.to_string_lossy().to_string()); s.keep_history = true; }
        crate::git::start_history(&st, &lib).await.unwrap();
        (st, lib)
    }

    async fn subjects(st: &AppState, lib: &Path) -> Vec<String> {
        crate::git::git(st, lib, &["log", "--format=%s"]).await.unwrap().lines().map(String::from).collect()
    }

    #[tokio::test]
    async fn a_change_made_elsewhere_is_recorded_on_its_own_and_the_apps_pending_save_is_left_alone() {
        let (st, lib) = library().await;
        // The app saved TODO.md a moment ago (its entry follows after a pause); someone else changed plan.md.
        crate::paths::write_atomic(&lib.join("TODO.md"), b"- [x] a\n", false).unwrap();
        std::fs::write(lib.join("ideas/plan.md"), "plan, edited on the phone\n").unwrap();
        let now = Instant::now();
        let mut b = Batch::new(now);
        b.add("ideas/plan.md".into(), now);
        b.add("TODO.md".into(), now);
        assert_eq!(handle(&st, &lib, &b, None).await, Handled::Recorded);
        let log = subjects(&st, &lib).await;
        assert_eq!(log[0], crate::git::subject::outside());
        let files = crate::git::git(&st, &lib, &["show", "--name-only", "--format=", "HEAD"]).await.unwrap();
        assert_eq!(files.trim(), "ideas/plan.md", "only what changed elsewhere");
        let pending = crate::git::git(&st, &lib, &["status", "--porcelain"]).await.unwrap();
        assert!(pending.contains("TODO.md"), "the app's own save waits for its own entry: {pending}");
    }

    #[tokio::test]
    async fn the_apps_own_work_is_never_recorded_as_someone_elses() {
        let (st, lib) = library().await;
        let n = subjects(&st, &lib).await.len();
        std::fs::write(lib.join("ideas/plan.md"), "written by the app\n").unwrap();
        let now = Instant::now();
        let mut b = Batch::new(now);
        b.add("ideas/plan.md".into(), now);
        // The app said it changed the library while the batch was collected.
        assert_eq!(handle(&st, &lib, &b, Some(now)).await, Handled::Refreshed);
        // An assistant run is working in the library.
        st.run_dirs.lock().unwrap().insert("chat".into(), lib.clone());
        assert_eq!(handle(&st, &lib, &b, None).await, Handled::Refreshed);
        st.run_dirs.lock().unwrap().clear();
        // A run was at work while the batch collected.
        let mut tainted = b.clone();
        tainted.tainted = true;
        assert_eq!(handle(&st, &lib, &tainted, None).await, Handled::Refreshed);
        assert_eq!(subjects(&st, &lib).await.len(), n, "nothing was recorded");
    }

    #[tokio::test]
    async fn a_held_library_is_waited_for() {
        let (st, lib) = library().await;
        std::fs::write(lib.join("ideas/plan.md"), "elsewhere\n").unwrap();
        let now = Instant::now();
        let mut b = Batch::new(now);
        b.add("ideas/plan.md".into(), now);
        let lock = st.library_lock.of(&lib);
        let held = lock.lock().await;
        assert_eq!(handle(&st, &lib, &b, None).await, Handled::Later);
        drop(held);
        // An edit being written holds the files.
        let editing = st.sync_files.clone().read_owned().await;
        assert_eq!(handle(&st, &lib, &b, None).await, Handled::Later);
        drop(editing);
        assert_eq!(handle(&st, &lib, &b, None).await, Handled::Recorded);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_watch_records_an_edit_made_by_another_program_and_tells_the_views() {
        let (st, lib) = library().await;
        let st = Arc::new(st);
        let mut events = st.events.subscribe();
        let task = tokio::spawn(run(st.clone()));
        // The watch is set up when the loop first runs. There is no signal for that, so under load
        // the first edit can come before it: then the edit is made again (a later one is just as
        // much "another program's").
        tokio::time::sleep(Duration::from_millis(800)).await;
        let edit = |n: u32| {
            std::fs::write(lib.join("ideas/plan.md"), format!("plan, edited in another editor ({n})\n")).unwrap();
            std::fs::write(lib.join("ideas/.plan.md.swp"), format!("swap {n}")).unwrap();
        };
        edit(0);
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut again = Instant::now() + Duration::from_secs(10);
        let mut edits = 1;
        let mut told = false;
        // The watch records the entry first and tells the views right after: wait for both before
        // stopping it, or the stop can fall between the two (the race that made this test flaky).
        let recorded = |log: &[String]| log[0] == crate::git::subject::outside();
        loop {
            while let Ok(e) = events.try_recv() { if e["type"] == "library.changed" && e["outside"] == true { told = true; } }
            let log = subjects(&st, &lib).await;
            if (recorded(&log) && told) || Instant::now() >= deadline { break; }
            if !recorded(&log) && Instant::now() >= again {
                edit(edits);
                edits += 1;
                again = Instant::now() + Duration::from_secs(10);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        task.abort();
        assert_eq!(subjects(&st, &lib).await[0], crate::git::subject::outside(), "recorded as its own entry");
        let files = crate::git::git(&st, &lib, &["show", "--name-only", "--format=", "HEAD"]).await.unwrap();
        assert_eq!(files.trim(), "ideas/plan.md");
        assert!(told, "the open views were told");
    }

    #[tokio::test]
    async fn without_a_history_the_views_are_only_told() {
        let (st, lib) = library().await;
        { let mut s = st.settings.write().await; s.keep_history = false; s.sync.mode = "none".into(); }
        std::fs::write(lib.join("ideas/plan.md"), "elsewhere\n").unwrap();
        let now = Instant::now();
        let mut b = Batch::new(now);
        b.add("ideas/plan.md".into(), now);
        let n = subjects(&st, &lib).await.len();
        assert_eq!(handle(&st, &lib, &b, None).await, Handled::Refreshed);
        assert_eq!(subjects(&st, &lib).await.len(), n);
        // Only ignored or untouched paths: nothing at all.
        let mut quiet = Batch::new(now);
        quiet.add("ideas/.x.swp".into(), now);
        quiet.paths.clear();
        assert_eq!(handle(&st, &lib, &quiet, None).await, Handled::Nothing);
    }
}
