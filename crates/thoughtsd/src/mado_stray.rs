//! MADO: the backstop behind a planning run's deny rules. A run of kind `mado` may write only
//! `mado/`; what changed elsewhere in the project while it ran is noted before and after it
//! (cheaply: `git status` in a repository, a bounded walk of sizes and times outside one) and
//! told to the user — in the step's chat and as `strayed` on the step, which keeps the step from
//! being approved until the user dismisses it. Nothing is ever put back: the user decides.

use crate::state::AppState;
use std::collections::BTreeMap;
use std::path::Path;

/// The most entries the walk outside git looks at.
pub const WALK_CAP: usize = 20_000;
/// The most paths kept in a step's `strayed` list.
pub const LIST_CAP: usize = 200;
/// Folders never looked into (the app's usual list: `library::SKIP`, `watch::SKIP_DIRS`).
const SKIP: &[&str] = &[".git", "node_modules", "target", ".thoughts-search", ".Trash", ".Trashes", ".Spotlight-V100", ".fseventsd"];

/// How the project stood: project-relative path → a stamp of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot { pub files: BTreeMap<String, String>, pub capped: bool, pub git: bool }

fn stamp(p: &Path) -> String {
    match std::fs::symlink_metadata(p) {
        Ok(m) => format!("{}:{}", m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos())),
        Err(_) => "gone".into(),
    }
}

fn in_mado(rel: &str) -> bool { rel.split('/').next().map_or(false, |f| f.eq_ignore_ascii_case(crate::mado::DIR)) }

/// The walk outside git: every file under `dir` but `mado/` and the skipped folders, at most `cap` entries.
pub fn walk(dir: &Path, cap: usize) -> Snapshot {
    let mut s = Snapshot::default();
    let mut seen = 0usize;
    let it = walkdir::WalkDir::new(dir).follow_links(false).into_iter().filter_entry(|e| {
        if e.depth() == 0 { return true; }
        let name = e.file_name().to_string_lossy();
        !(e.file_type().is_dir() && (SKIP.contains(&name.as_ref()) || (e.depth() == 1 && name.eq_ignore_ascii_case(crate::mado::DIR))))
    });
    for e in it.flatten() {
        if e.depth() == 0 { continue; }
        seen += 1;
        if seen > cap { s.capped = true; break; }
        if e.file_type().is_dir() { continue; }
        let Ok(rel) = e.path().strip_prefix(dir) else { continue };
        s.files.insert(rel.to_string_lossy().replace('\\', "/"), stamp(e.path()));
    }
    s
}

/// In a git repository: every path `git status` lists (relative to the project), with its status
/// and a stamp, so a second edit to a file already changed shows too. `None` outside a repository.
async fn git_status(state: &AppState, dir: &Path) -> Option<Snapshot> {
    let prefix = crate::git::git(state, dir, &["rev-parse", "--show-prefix"]).await.ok()?.trim().to_string();
    let top = crate::git::git(state, dir, &["rev-parse", "--show-toplevel"]).await.ok()?.trim().to_string();
    let out = crate::git::git(state, dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."]).await.ok()?;
    let top = Path::new(&top);
    let mut s = Snapshot { git: true, ..Default::default() };
    let mut it = out.split('\0');
    while let Some(entry) = it.next() {
        if entry.len() < 4 { continue; }
        let (xy, path) = (&entry[..2], &entry[3..]);
        if xy.contains('R') || xy.contains('C') { it.next(); }
        let Some(rel) = path.strip_prefix(prefix.as_str()) else { continue };
        if in_mado(rel) { continue; }
        s.files.insert(rel.to_string(), format!("{xy}:{}", stamp(&top.join(path))));
    }
    Some(s)
}

/// How the project stands now.
pub async fn snapshot(state: &AppState, dir: &Path) -> Snapshot {
    if let Some(s) = git_status(state, dir).await { return s; }
    let d = dir.to_path_buf();
    tokio::task::spawn_blocking(move || walk(&d, WALK_CAP)).await.unwrap_or_default()
}

/// What changed between two snapshots: added, removed or different, in order.
pub fn changed(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut out: Vec<String> = after.files.iter().filter(|(k, v)| before.files.get(*k) != Some(*v)).map(|(k, _)| k.clone()).collect();
    // Gone since: listed before, not now. (In git: a change that was undone again reads the same way.)
    for k in before.files.keys().filter(|k| !after.files.contains_key(*k)) { out.push(k.clone()); }
    out.sort();
    out.dedup();
    out
}

/// What each `mado` run found when it started (kept in memory: a run cut off by a restart is not checked).
fn kept() -> &'static std::sync::Mutex<std::collections::HashMap<String, Snapshot>> {
    static K: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Snapshot>>> = std::sync::OnceLock::new();
    K.get_or_init(Default::default)
}

pub async fn before_run(state: &AppState, chat_id: &str, dir: &Path) {
    let s = snapshot(state, dir).await;
    kept().lock().unwrap_or_else(|e| e.into_inner()).insert(chat_id.to_string(), s);
}

/// After the run: what changed outside `mado/` (`None` when the run was not watched), and whether
/// the walk stopped at its cap.
pub async fn after_run(state: &AppState, chat_id: &str, dir: &Path) -> Option<(Vec<String>, bool)> {
    let before = kept().lock().unwrap_or_else(|e| e.into_inner()).remove(chat_id)?;
    let after = snapshot(state, dir).await;
    let capped = before.capped || after.capped;
    // A repository made or removed meanwhile: the two cannot be compared, which is said as such.
    if before.git != after.git { return Some((vec![], true)); }
    let mut list = changed(&before, &after);
    list.truncate(LIST_CAP);
    Some((list, capped))
}
