//! MADO: the backstop behind a planning run's deny rules. A run of kind `mado` may write only
//! `mado/`; what changed elsewhere in the project while it ran is noted before and after it
//! (cheaply: `git status` in a repository, a bounded walk of sizes and times outside one) and
//! told to the user — in the step's chat and as `strayed` on the step, which keeps the step from
//! being approved until the user dismisses it. Nothing is ever put back: the user decides.
//!
//! The "before" is kept on disk beside the approval record (`<thoughts home>/mado/<projectId>/
//! stray-before-<chatId>.json`), so a run cut off by a restart of the daemon is still checked:
//! on the next start (`mado::check_cut_off_runs`).

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
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot { pub files: BTreeMap<String, String>, pub capped: bool, pub git: bool }

fn stamp(p: &Path) -> String {
    match std::fs::symlink_metadata(p) {
        Ok(m) => format!("{}:{}", m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos())),
        Err(_) => "gone".into(),
    }
}

fn in_mado(rel: &str) -> bool { rel.split('/').next().is_some_and(|f| f.eq_ignore_ascii_case(crate::mado::DIR)) }

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

/// The most a kept "before" may weigh when it is read back (a full walk is far less).
const MAX_KEPT: u64 = 64 * 1024 * 1024;
const PREFIX: &str = "stray-before-";

/// Where the "before" of the run of `chat_id` on project `project_id` is kept. `None` for an id
/// that could name anything else.
fn kept_file(project_id: &str, chat_id: &str) -> Option<std::path::PathBuf> {
    let ok = !chat_id.is_empty() && chat_id.len() <= 64 && chat_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !ok { return None; }
    Some(crate::mado::record_dir(project_id)?.join(format!("{PREFIX}{chat_id}.json")))
}

/// What a run's check comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// Compared: what changed outside `mado/`, and whether a walk stopped at its cap.
    Checked(Vec<String>, bool),
    /// The run was watched, but its "before" is missing, unreadable, too large or not the one
    /// recorded: nothing can be said, so the step waits as it would for real strays.
    Unchecked,
}

/// What a watched run records in its chat (`meta.mado.strayCheck`): the SHA-256 of its kept
/// "before" (empty when it could not be kept). Its absence means the run was not watched.
pub const META_KEY: &str = "strayCheck";

/// Read back (and remove) what the run found when it started, against the hash its chat recorded.
/// `expected`: `None` = the run was not watched (nothing to check; any file is dropped unread).
/// Fails closed: a watched run whose file is missing, odd, too large, unreadable or another one
/// than recorded is `Err(())`.
fn take(project_id: &str, chat_id: &str, expected: Option<&str>) -> Option<Result<Snapshot, ()>> {
    let f = kept_file(project_id, chat_id);
    let read = || -> Result<Snapshot, ()> {
        let f = f.as_ref().ok_or(())?;
        let m = std::fs::symlink_metadata(f).map_err(|_| ())?;
        if !m.is_file() || m.len() > MAX_KEPT { return Err(()); }
        let bytes = std::fs::read(f).map_err(|_| ())?;
        let want = expected.filter(|h| !h.is_empty()).ok_or(())?;
        if crate::mado_review::plan_hash(&bytes) != want { return Err(()); }
        serde_json::from_slice(&bytes).map_err(|_| ())
    };
    let out = expected.map(|_| read());
    if let Some(f) = &f {
        if std::fs::symlink_metadata(f).is_ok() { crate::log::ok_or_warn("mado: could not remove a run's kept snapshot", std::fs::remove_file(f)); }
    }
    out
}

/// Drop what was kept for a run that will never be checked (its chat is gone).
pub fn discard(project_id: &str, chat_id: &str) { let _ = take(project_id, chat_id, None); }

/// Every run whose "before" is still kept: (project id, chat id). At the start of the daemon these
/// are the runs a restart cut off.
pub fn kept_runs() -> Vec<(String, String)> {
    let root = crate::paths::home().join(crate::mado::DIR);
    let mut out = vec![];
    for p in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let pid = p.file_name().to_string_lossy().to_string();
        if crate::mado::record_dir(&pid).is_none() { continue; }
        for f in std::fs::read_dir(p.path()).into_iter().flatten().flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            let Some(chat) = name.strip_prefix(PREFIX).and_then(|n| n.strip_suffix(".json")) else { continue };
            if kept_file(&pid, chat).is_some() { out.push((pid.clone(), chat.to_string())); }
        }
    }
    out.sort();
    out
}

/// Note how the project stands before a run, on disk. Returns the SHA-256 of what was kept, for the
/// run's chat (`META_KEY`); empty when it could not be kept, which makes the check fail closed.
pub async fn before_run(state: &AppState, project_id: &str, chat_id: &str, dir: &Path) -> String {
    let s = snapshot(state, dir).await;
    let Some(f) = kept_file(project_id, chat_id) else { return String::new() };
    let write = || -> anyhow::Result<String> {
        let bytes = serde_json::to_vec(&s)?;
        if let Some(d) = f.parent() { crate::paths::create_private_dir(d)?; }
        crate::paths::write_atomic(&f, &bytes, true)?;
        Ok(crate::mado_review::plan_hash(&bytes))
    };
    crate::log::ok_or_warn("mado: could not keep what the project looked like before a run", write()).unwrap_or_default()
}

/// After the run: what changed outside `mado/` (`None` when the run was not watched, `expected`
/// being the hash its chat recorded, see `take`).
pub async fn after_run(state: &AppState, project_id: &str, chat_id: &str, dir: &Path, expected: Option<&str>) -> Option<Found> {
    let Ok(before) = take(project_id, chat_id, expected)? else { return Some(Found::Unchecked) };
    let after = snapshot(state, dir).await;
    let capped = before.capped || after.capped;
    // A repository made or removed meanwhile: the two cannot be compared, which is said as such.
    if before.git != after.git { return Some(Found::Checked(vec![], true)); }
    let mut list = changed(&before, &after);
    list.truncate(LIST_CAP);
    Some(Found::Checked(list, capped))
}
