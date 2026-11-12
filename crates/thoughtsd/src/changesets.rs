//! What each assistant run changed in the library, and undoing exactly that (FEAT-06).
//!
//! When a chat or an automatic task starts working in the library, the library as it is at that
//! moment is recorded (`begin`). Only a tree is written into the history's own store: the folder,
//! the branch and the index every other history command uses are left as they are. When the run is
//! over the library is recorded again (`finish`), and the difference is what changed meanwhile.
//! Not all of it need be the run's:
//!
//! - every save the user makes in the app while a run works there is fenced (`fence`): the library
//!   is recorded just before and just after the save, and whatever the save changed is set aside;
//! - what another run working in the same library at the same time wrote with its file tools is set
//!   aside too; when it ran commands (which can change any file), every file this run did not write
//!   with its own file tools is.
//!
//! What is set aside is never counted as the run's: it is not part of the run's history entry, its
//! Undo never touches it, and the chat says that something else changed meanwhile instead of
//! guessing. The run's own changes become its history entry (those files only) and a `changes`
//! message under its answer: what changed, document by document, and Undo. Undo puts back exactly
//! those files, one by one, merging line by line with whatever changed them since; when a later
//! change touched the same lines it refuses and names the documents. Nothing is ever reset as a
//! whole. The undo is itself a history entry, and it can be taken back ("Put them back").
//!
//! Each change set is kept as two commits outside every branch (the library before, and the library
//! before with only the run's changes), under `refs/thoughts/changes/<chat>/<message>`: Sync never
//! sends them, History never lists them, and they outlive anything done to the branch.
//!
//! Needs the history of changes (on unless the user switched it off). What another program changes
//! in the folder while a run works (an editor of the user's own) cannot be told apart from the run's
//! commands, and is counted as the run's.

use crate::chats::Msg;
use crate::git::{git, git_raw, git_with_index};
use crate::state::AppState;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where the recorded states of every change set are kept.
const REFS: &str = "refs/thoughts/changes";
/// Names a chat message keeps (the change view reads the whole list from the history).
const LISTED: usize = 200;
/// Tools that can change any file, wherever it is.
const ANYWHERE_TOOLS: &[&str] = &["Bash", "PowerShell"];
/// File tools, and the field that names the file they change.
const FILE_TOOLS: &[(&str, &str)] = &[("Write", "file_path"), ("Edit", "file_path"), ("MultiEdit", "file_path"), ("NotebookEdit", "notebook_path")];

/// A run being watched.
struct Watch {
    token: String,
    chat: String,
    /// The library, as its real path.
    lib: PathBuf,
    /// Where the run works (its file tools name files relative to it).
    cwd: PathBuf,
    /// The library when the run started (a tree; empty until it has been looked at).
    before: String,
    started: DateTime<Utc>,
    /// Files someone else changed while it worked.
    foreign: BTreeSet<String>,
    /// Something else may have changed any file meanwhile.
    anywhere: bool,
}

static WATCHES: std::sync::Mutex<Vec<Watch>> = std::sync::Mutex::new(Vec::new());
/// Saves in the app still being looked at, per library.
static PENDING: std::sync::Mutex<BTreeMap<PathBuf, usize>> = std::sync::Mutex::new(BTreeMap::new());
/// Libraries whose recorded states were already tidied while the app runs.
static SWEPT: std::sync::Mutex<BTreeSet<PathBuf>> = std::sync::Mutex::new(BTreeSet::new());

fn watches() -> std::sync::MutexGuard<'static, Vec<Watch>> { WATCHES.lock().unwrap_or_else(|e| e.into_inner()) }
fn real(p: &Path) -> PathBuf { crate::paths::canonical_lenient(p) }

/// Is an assistant run being watched in this library right now?
pub fn watched(lib: &Path) -> bool {
    let lib = real(lib);
    watches().iter().any(|w| w.lib == lib)
}

fn pending(lib: &Path) -> usize { PENDING.lock().unwrap_or_else(|e| e.into_inner()).get(lib).copied().unwrap_or(0) }
fn pend(lib: &Path, more: bool) {
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let n = p.entry(lib.to_path_buf()).or_insert(0);
    if more { *n += 1 } else { *n = n.saturating_sub(1) }
    if *n == 0 { p.remove(lib); }
}

/// Set these files aside for every run watched in `lib` (or, with `anywhere`, everything they did
/// not write with their own file tools).
fn set_aside(lib: &Path, files: &[String], anywhere: bool) {
    for w in watches().iter_mut().filter(|w| w.lib == lib) {
        w.foreign.extend(files.iter().cloned());
        w.anywhere |= anywhere;
    }
}

// ---------- states of the library ----------

/// A file of the app's own inside the history's store (an index, a version of a document for a
/// merge), removed when it is no longer needed.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let mut lock = self.0.clone().into_os_string();
        lock.push(".lock");
        let _ = std::fs::remove_file(PathBuf::from(lock));
    }
}

async fn scratch(state: &AppState, dir: &Path, what: &str) -> anyhow::Result<Scratch> {
    let store = git(state, dir, &["rev-parse", "--absolute-git-dir"]).await?;
    Ok(Scratch(PathBuf::from(store.trim()).join(format!("thoughts-{what}-{}", uuid::Uuid::new_v4()))))
}

/// The tree of everything the folder holds right now, exactly as the history would record it (its
/// ignore rules included). Worked out in an index of its own: nothing else is touched.
pub async fn snapshot(state: &AppState, dir: &Path) -> anyhow::Result<String> {
    // `add` runs whatever filters the repository's settings name: only settings that were trusted.
    crate::trust::require_git(dir)?;
    let index = scratch(state, dir, "index").await?;
    let own = PathBuf::from(git(state, dir, &["rev-parse", "--git-path", "index"]).await?.trim());
    let own = if own.is_absolute() { own } else { dir.join(own) };
    // Starting from the history's own index, only the files that changed since are read again.
    if own.is_file() { std::fs::copy(&own, &index.0)?; }
    git_with_index(state, dir, &index.0, &["add", "-A"]).await?;
    Ok(git_with_index(state, dir, &index.0, &["write-tree"]).await?.trim().to_string())
}

/// One file that differs between two states: its mode and object before and after (`None`: not there).
#[derive(Debug, Clone, PartialEq)]
struct Delta { path: String, old: Option<(String, String)>, new: Option<(String, String)> }

/// `git diff-tree -r -z --no-renames` output, one entry per file.
fn parse_raw(out: &str) -> Vec<Delta> {
    let mut found = vec![];
    let mut it = out.split('\0');
    while let Some(head) = it.next() {
        let Some(h) = head.trim_start_matches('\n').strip_prefix(':') else { continue };
        let f: Vec<&str> = h.split_whitespace().collect();
        if f.len() < 5 { continue; }
        let Some(path) = it.next() else { break };
        let side = |mode: &str, oid: &str| (!mode.bytes().all(|b| b == b'0')).then(|| (mode.to_string(), oid.to_string()));
        if !path.is_empty() { found.push(Delta { path: path.to_string(), old: side(f[0], f[2]), new: side(f[1], f[3]) }); }
    }
    found
}

/// Every file that differs between two states (trees or commits).
async fn deltas(state: &AppState, dir: &Path, from: &str, to: &str) -> anyhow::Result<Vec<Delta>> {
    Ok(parse_raw(&git(state, dir, &["diff-tree", "-r", "-z", "--no-renames", "--no-commit-id", from, to]).await?))
}

async fn idx_git(state: &AppState, dir: &Path, index: Option<&Path>, args: &[&str]) -> anyhow::Result<String> {
    match index { Some(i) => git_with_index(state, dir, i, args).await, None => git(state, dir, args).await }
}

/// Make these files in `index` (the history's own when `None`) what they are after `entries`.
async fn put_entries(state: &AppState, dir: &Path, index: Option<&Path>, entries: &[Delta]) -> anyhow::Result<()> {
    // Removals first: a file may take the place of a folder that went.
    let gone: Vec<&str> = entries.iter().filter(|d| d.new.is_none()).map(|d| d.path.as_str()).collect();
    for chunk in gone.chunks(100) {
        let mut args = vec!["update-index", "--force-remove", "--"];
        args.extend_from_slice(chunk);
        idx_git(state, dir, index, &args).await?;
    }
    let put: Vec<String> = entries.iter().filter_map(|d| d.new.as_ref().map(|(m, o)| format!("{m},{o},{}", d.path))).collect();
    for chunk in put.chunks(100) {
        let mut args = vec!["update-index", "--add", "--replace"];
        for c in chunk { args.push("--cacheinfo"); args.push(c); }
        idx_git(state, dir, index, &args).await?;
    }
    Ok(())
}

/// `tree` (or nothing) with `entries` applied, as a tree.
async fn tree_with(state: &AppState, dir: &Path, tree: Option<&str>, entries: &[Delta]) -> anyhow::Result<String> {
    let index = scratch(state, dir, "index").await?;
    match tree {
        Some(t) => { git_with_index(state, dir, &index.0, &["read-tree", t]).await?; }
        None => { git_with_index(state, dir, &index.0, &["read-tree", "--empty"]).await?; }
    }
    put_entries(state, dir, Some(&index.0), entries).await?;
    Ok(git_with_index(state, dir, &index.0, &["write-tree"]).await?.trim().to_string())
}

async fn new_commit(state: &AppState, dir: &Path, tree: &str, parent: Option<&str>, msg: &str) -> anyhow::Result<String> {
    let mut args = vec!["commit-tree", tree];
    if let Some(p) = parent { args.extend(["-p", p]); }
    args.extend(["-m", msg]);
    Ok(crate::git::git_ident(state, dir, &args).await?.trim().to_string())
}

/// The two recorded states of a change set: `before`, and `before` with only `entries` changed.
async fn states(state: &AppState, dir: &Path, before: &str, entries: &[Delta]) -> anyhow::Result<(String, String)> {
    let start = new_commit(state, dir, before, None, "Thoughts: the library before an assistant's changes").await?;
    let tree = tree_with(state, dir, Some(before), entries).await?;
    let end = new_commit(state, dir, &tree, Some(&start), "Thoughts: what the assistant changed").await?;
    Ok((start, end))
}

/// A history entry holding only `entries`, on top of whatever the history holds now (other files'
/// pending changes stay pending). The caller holds `git_lock`. `None`: nothing new to record.
async fn record(state: &AppState, dir: &Path, entries: &[Delta], subject: &str) -> anyhow::Result<Option<String>> {
    if entries.is_empty() { return Ok(None); }
    let head = crate::git::head(state, dir).await;
    let tree = tree_with(state, dir, head.as_deref(), entries).await?;
    if let Some(h) = &head {
        if git(state, dir, &["rev-parse", &format!("{h}^{{tree}}")]).await?.trim() == tree { return Ok(None); }
    }
    let commit = new_commit(state, dir, &tree, head.as_deref(), subject).await?;
    let mut args = vec!["update-ref", "-m", subject, "HEAD", commit.as_str()];
    if let Some(h) = &head { args.push(h); }
    git(state, dir, &args).await?;
    // The history's own index agrees with the new entry for these files, as after any commit.
    crate::log::ok_or_warn("changes: could not bring the history's index up to date", put_entries(state, dir, None, entries).await);
    Ok(Some(commit))
}

// ---------- watching a run ----------

/// The library-relative name of a file a tool named (`None`: not in the library).
fn rel_in(lib: &Path, cwd: &Path, raw: &str) -> Option<String> {
    let p = Path::new(raw.trim());
    if p.as_os_str().is_empty() { return None; }
    let abs = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    let rel = real(&abs).strip_prefix(lib).or_else(|_| abs.strip_prefix(lib)).ok()?.to_path_buf();
    if rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) { return None; }
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// What a run's tool rows since `since` say it wrote: the library files its file tools changed, and
/// whether it used something that can change any file (commands, tools connected to Claude Code).
fn tool_writes(messages: &[Msg], since: DateTime<Utc>, lib: &Path, cwd: &Path) -> (BTreeSet<String>, bool) {
    let mut files = BTreeSet::new();
    let mut anywhere = false;
    for m in messages.iter().filter(|m| m.role == "tool" && m.ts >= since) {
        let Some(t) = &m.tool else { continue };
        let name = t["name"].as_str().unwrap_or("");
        if ANYWHERE_TOOLS.contains(&name) || name.starts_with("mcp__") { anywhere = true; continue; }
        if let Some((_, field)) = FILE_TOOLS.iter().find(|(n, _)| *n == name) {
            if let Some(rel) = t["input"][*field].as_str().and_then(|p| rel_in(lib, cwd, p)) { files.insert(rel); }
        }
    }
    (files, anywhere)
}

/// The run's own changes, and the files set aside (with whether any was set aside only because
/// something else could have changed any file).
fn sort_out(changed: Vec<Delta>, own: &BTreeSet<String>, foreign: &BTreeSet<String>, anywhere: bool) -> (Vec<Delta>, Vec<String>, bool) {
    let (mut mine, mut others, mut unsure) = (vec![], vec![], false);
    for d in changed {
        if foreign.contains(&d.path) { others.push(d.path); }
        else if own.contains(&d.path) || !anywhere { mine.push(d); }
        else { unsure = true; others.push(d.path); }
    }
    (mine, others, unsure)
}

/// A run being watched; when it is dropped without `finish` (the run never started, the app is
/// stopping), the watch is forgotten.
pub struct Watching { token: String }
impl Drop for Watching {
    fn drop(&mut self) { watches().retain(|w| w.token != self.token); }
}

/// A chat or automatic task is about to work in `lib` (from `cwd`): record the library as it is.
/// `None` when there is no history to record into.
pub async fn begin(state: &AppState, chat: &str, lib: &Path, cwd: &Path) -> Option<Watching> {
    if !state.settings.read().await.history_enabled() || !crate::git::ready(state, lib).await { return None; }
    let lib = real(lib);
    sweep(state, &lib).await;
    let token = uuid::Uuid::new_v4().to_string();
    // Watched from before the library is looked at, so a save made meanwhile is set aside too.
    watches().push(Watch { token: token.clone(), chat: chat.into(), lib: lib.clone(), cwd: real(cwd), before: String::new(),
        started: Utc::now(), foreign: BTreeSet::new(), anywhere: false });
    let watching = Watching { token: token.clone() };
    match snapshot(state, &lib).await {
        Ok(tree) => { if let Some(w) = watches().iter_mut().find(|w| w.token == token) { w.before = tree; } Some(watching) }
        Err(e) => { crate::log::warn(format!("changes: run {chat} is not watched: the library could not be recorded ({e})")); None }
    }
}

/// Once per library while the app runs: forget the recorded states of chats that were deleted.
async fn sweep(state: &AppState, lib: &Path) {
    if !SWEPT.lock().unwrap_or_else(|e| e.into_inner()).insert(lib.to_path_buf()) { return; }
    let Ok(out) = git(state, lib, &["for-each-ref", "--format=%(refname)", REFS]).await else { return };
    let prefix = format!("{REFS}/");
    for name in out.lines() {
        let Some(chat) = name.strip_prefix(&prefix).and_then(|r| r.split('/').next()) else { continue };
        if state.chats.with(chat, |_| ()).await.is_none() { let _ = git(state, lib, &["update-ref", "-d", name]).await; }
    }
}

/// A save the user makes in the app while an assistant run works in the library. Hold it for the
/// length of the save: what the save changed is set aside for every run watched there.
pub struct Fence { state: Arc<AppState>, lib: PathBuf, before: Option<String> }

/// Fence a save about to be made in the configured library (`None`: no run is watched there).
pub async fn fence(state: &Arc<AppState>) -> Option<Fence> {
    let lib = real(&state.settings.read().await.library()?);
    if !watched(&lib) { return None; }
    pend(&lib, true);
    let before = match snapshot(state, &lib).await {
        Ok(t) => Some(t),
        Err(e) => {
            crate::log::warn(format!("changes: a save during an assistant run could not be looked at ({e})"));
            set_aside(&lib, &[], true);
            pend(&lib, false);
            None
        }
    };
    Some(Fence { state: state.clone(), lib, before })
}

impl Drop for Fence {
    fn drop(&mut self) {
        let Some(before) = self.before.take() else { return };
        let (state, lib) = (self.state.clone(), self.lib.clone());
        let Ok(rt) = tokio::runtime::Handle::try_current() else { set_aside(&lib, &[], true); pend(&lib, false); return };
        rt.spawn(async move {
            let seen = async { let after = snapshot(&state, &lib).await?; deltas(&state, &lib, &before, &after).await }.await;
            match seen {
                Ok(d) => set_aside(&lib, &d.into_iter().map(|d| d.path).collect::<Vec<_>>(), false),
                Err(e) => { crate::log::warn(format!("changes: what a save changed could not be seen ({e})")); set_aside(&lib, &[], true); }
            }
            pend(&lib, false);
        });
    }
}

/// The run of `chat` is over. What it changed (and only that) becomes its history entry, named
/// `subject`, and a `changes` message under its answer. Returns false when the run was not watched
/// or its changes could not be worked out: the caller then records the library the usual way.
pub async fn finish(state: &AppState, chat: &str, subject: &str) -> bool {
    let Some((lib, cwd, started)) = watches().iter().find(|w| w.chat == chat).map(|w| (w.lib.clone(), w.cwd.clone(), w.started)) else { return false };
    // Saves made in the app meanwhile are looked at first (a few seconds at most).
    for _ in 0..500 {
        if pending(&lib) == 0 { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let own = state.chats.with(chat, |c| tool_writes(&c.messages, started, &lib, &cwd).0).await.unwrap_or_default();
    // Other runs at work in the same library meanwhile: what they wrote is not this run's, and what
    // this run wrote is not theirs.
    let others: Vec<(String, PathBuf, DateTime<Utc>)> = watches().iter().filter(|w| w.lib == lib && w.chat != chat).map(|w| (w.chat.clone(), w.cwd.clone(), w.started)).collect();
    let (mut theirs, mut anywhere_else) = (BTreeSet::new(), false);
    let mut for_them = vec![];
    for (other, ocwd, since) in &others {
        if let Some((f, a)) = state.chats.with(other, |c| tool_writes(&c.messages, started, &lib, ocwd)).await { theirs.extend(f); anywhere_else |= a; }
        for_them.push((other.clone(), state.chats.with(chat, |c| tool_writes(&c.messages, *since, &lib, &cwd)).await.unwrap_or_default()));
    }
    let w = {
        let mut ws = watches();
        for (other, (f, a)) in for_them {
            if let Some(o) = ws.iter_mut().find(|o| o.chat == other && o.lib == lib) { o.foreign.extend(f); o.anywhere |= a; }
        }
        let Some(i) = ws.iter().position(|w| w.chat == chat) else { return false };
        ws.remove(i)
    };
    if w.before.is_empty() { return false; }
    let changed = match snapshot(state, &lib).await {
        Ok(after) => deltas(state, &lib, &w.before, &after).await,
        Err(e) => Err(e),
    };
    let changed = match changed {
        Ok(c) => c,
        Err(e) => { crate::log::warn(format!("changes: what run {chat} changed could not be worked out ({e})")); return false; }
    };
    if changed.is_empty() { return true; }
    let mut foreign = w.foreign;
    foreign.extend(theirs);
    let (mine, others, unsure) = sort_out(changed, &own, &foreign, w.anywhere || anywhere_else);
    if !others.is_empty() {
        crate::log::info(format!("changes: run {chat}: {} file(s) changed meanwhile by the user or another chat were set aside", others.len()));
    }
    if mine.is_empty() { return true; }
    let (start, end) = match states(state, &lib, &w.before, &mine).await {
        Ok(s) => s,
        Err(e) => { crate::log::warn(format!("changes: what run {chat} changed could not be recorded ({e})")); return false; }
    };
    let history = record_run(state, &lib, &mine, subject).await;
    let names = names(state, &lib, &start, &end).await.unwrap_or_else(|_| mine.iter().map(|d| crate::changes::Entry {
        path: d.path.clone(), from: None, change: match (&d.old, &d.new) { (None, _) => "added", (_, None) => "deleted", _ => "edited" } }).collect());
    let m = message(&lib, subject, &start, &end, &names, &others, unsure, history.as_deref());
    crate::log::ok_or_warn("changes: could not keep what the run changed", git(state, &lib, &["update-ref", &format!("{REFS}/{chat}/{}", m.id), &end]).await);
    crate::log::info(format!("changes: run {chat} changed {} file(s){}", names.len(), if history.is_some() { "" } else { " (not recorded in the history now)" }));
    under_answer(state, chat, m).await;
    true
}

/// The run's history entry, when nothing stands in the way (as `claude::record_library`).
async fn record_run(state: &AppState, lib: &Path, mine: &[Delta], subject: &str) -> Option<String> {
    if !state.settings.read().await.history_enabled() || state.library_busy() { return None; }
    if crate::wraps::interrupted_wrap(state).await.is_some() { return None; }
    let _g = state.git_lock.lock().await;
    if let Err(e) = crate::trust::require_git(lib) { crate::log::warn(format!("history: not recording ({e})")); return None; }
    match record(state, lib, mine, subject).await {
        Ok(h) => h,
        Err(e) => { crate::log::warn(format!("history: could not record what the assistant changed ({e})")); None }
    }
}

/// The documents that differ between two recorded states, moves recognised, for a person to read.
async fn names(state: &AppState, dir: &Path, from: &str, to: &str) -> anyhow::Result<Vec<crate::changes::Entry>> {
    let out = git(state, dir, &["diff", "--name-status", "-z", "-M", "--no-color", "--no-relative", from, to]).await?;
    Ok(crate::changes::parse_name_status(&out))
}

fn message(lib: &Path, subject: &str, start: &str, end: &str, names: &[crate::changes::Entry], others: &[String], unsure: bool, history: Option<&str>) -> Msg {
    let mut counts = json!({"added": 0, "edited": 0, "moved": 0, "deleted": 0});
    for e in names { counts[e.change] = json!(counts[e.change].as_u64().unwrap_or(0) + 1); }
    let mut list = names.iter().take(10).map(|e| e.path.as_str()).collect::<Vec<_>>().join(", ");
    if names.len() > 10 { list.push_str(", …"); }
    // The text is for anything that shows messages as plain text; the chat words it itself.
    let mut m = Msg::new("changes", crate::tr::text("changes.summary", &[("files", &list)]));
    m.tool = Some(json!({
        "code": "changes", "library": lib.to_string_lossy(), "subject": subject, "before": start, "after": end,
        "files": names.iter().take(LISTED).map(|e| json!({"path": e.path, "from": e.from, "change": e.change})).collect::<Vec<_>>(),
        "total": names.len(), "counts": counts,
        "others": others.iter().take(LISTED).collect::<Vec<_>>(), "othersTotal": others.len(), "othersUnsure": unsure,
        "history": history, "undone": Value::Null,
    }));
    m
}

/// Under the answer — and before an error, which must stay the last message of a run that failed.
async fn under_answer(state: &AppState, chat: &str, m: Msg) {
    let mv = serde_json::to_value(&m).unwrap_or(Value::Null);
    state.chats.update(chat, |c| {
        let at = c.messages.len() - usize::from(c.messages.last().map_or(false, |x| x.role == "error"));
        c.messages.insert(at, m);
    }).await;
    state.emit(json!({"type": "chat.message", "chatId": chat, "message": mv}));
}

// ---------- looking back, and undoing ----------

/// The change set of message `msg` in chat `chat`.
async fn change_set(state: &AppState, lib: &Path, chat: &str, msg: &str) -> anyhow::Result<Value> {
    let t = state.chats.with(chat, |c| c.messages.iter().find(|m| m.id == msg && m.role == "changes").and_then(|m| m.tool.clone())).await.flatten()
        .filter(|t| t["code"] == "changes").ok_or_else(|| crate::tr::err("changes.not_found", &[]))?;
    if real(Path::new(t["library"].as_str().unwrap_or(""))) != real(lib) { return Err(crate::tr::err("changes.other_library", &[])); }
    Ok(t)
}

/// Both recorded states are still there.
async fn present(state: &AppState, lib: &Path, points: &[&str]) -> anyhow::Result<()> {
    for p in points {
        if !crate::git::valid_hash(p) || git(state, lib, &["cat-file", "-e", &format!("{p}^{{commit}}")]).await.is_err() {
            return Err(crate::tr::err("changes.gone", &[]));
        }
    }
    Ok(())
}

/// `GET` view of what an answer changed: `changes::between` for its own changes, plus what was set aside.
pub async fn view(state: &AppState, lib: &Path, chat: &str, msg: &str) -> anyhow::Result<Value> {
    let t = change_set(state, lib, chat, msg).await?;
    let (start, end) = (t["before"].as_str().unwrap_or(""), t["after"].as_str().unwrap_or(""));
    present(state, lib, &[start, end]).await?;
    let mut v = crate::changes::between(state, lib, start, end).await?;
    for k in ["others", "othersTotal", "othersUnsure"] { v[k] = t[k].clone(); }
    v["undone"] = json!(!t["undone"].is_null());
    Ok(v)
}

/// What putting one file back takes.
enum Step { Remove, Write(Vec<u8>, bool) }

/// Where a file may be written or removed: inside the library, never through a link to elsewhere,
/// never inside `.git`.
fn target(lib: &Path, rel: &str) -> Option<PathBuf> {
    let parts: Vec<std::path::Component> = Path::new(rel).components().collect();
    if parts.is_empty() || parts.iter().any(|c| !matches!(c, std::path::Component::Normal(_))) { return None; }
    if parts.iter().any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git")) { return None; }
    let mut at = lib.to_path_buf();
    for c in &parts[..parts.len() - 1] {
        at.push(c);
        if std::fs::symlink_metadata(&at).map_or(false, |m| m.file_type().is_symlink()) { return None; }
    }
    Some(lib.join(rel))
}

fn regular(side: &(String, String)) -> bool { side.0 == "100644" || side.0 == "100755" }

async fn blob(state: &AppState, lib: &Path, oid: &str) -> anyhow::Result<Vec<u8>> { git_raw(state, lib, &["cat-file", "blob", oid]).await }

/// `cur` with the change from `base` to `target` made in it, line by line; `None` when a change
/// since touched the same lines (or the file is not text).
async fn merged(state: &AppState, lib: &Path, cur: &str, base: &str, target: &str) -> anyhow::Result<Option<Vec<u8>>> {
    let mut files = vec![];
    for (what, oid) in [("mine", cur), ("base", base), ("theirs", target)] {
        let f = scratch(state, lib, what).await?;
        std::fs::write(&f.0, blob(state, lib, oid).await?)?;
        files.push(f);
    }
    let p: Vec<String> = files.iter().map(|f| f.0.to_string_lossy().to_string()).collect();
    Ok(git_raw(state, lib, &["merge-file", "-p", "-q", &p[0], &p[1], &p[2]]).await.ok())
}

/// What taking the library from state `base` back to state `goal` takes, for the files that differ
/// between the two, given the library as it is now (`now`). Refused, naming the documents, when a
/// later change touched the same lines (or a file cannot be put back here).
async fn plan(state: &AppState, lib: &Path, base: &str, goal: &str, now: &str) -> anyhow::Result<Vec<(String, Step)>> {
    let since: BTreeMap<String, Delta> = deltas(state, lib, base, now).await?.into_iter().map(|d| (d.path.clone(), d)).collect();
    let (mut steps, mut clash, mut special) = (vec![], vec![], vec![]);
    for d in deltas(state, lib, base, goal).await? {
        // d.old: as the change left it; d.new: as it is to be; cur: as it is now.
        let cur = since.get(&d.path).map_or_else(|| d.old.clone(), |s| s.new.clone());
        if cur == d.new { continue; }
        let Some(_) = target(lib, &d.path) else { special.push(d.path); continue };
        let step = if cur == d.old {
            match &d.new {
                None => Step::Remove,
                Some(n) if regular(n) => Step::Write(blob(state, lib, &n.1).await?, n.0 == "100755"),
                Some(_) => { special.push(d.path); continue; }
            }
        } else {
            match (&d.old, &d.new, &cur) {
                (Some(b), Some(g), Some(c)) if regular(b) && regular(g) && regular(c) => match merged(state, lib, &c.1, &b.1, &g.1).await? {
                    Some(bytes) => Step::Write(bytes, c.0 == "100755"),
                    None => { clash.push(d.path); continue; }
                },
                _ => { clash.push(d.path); continue; }
            }
        };
        steps.push((d.path, step));
    }
    if !clash.is_empty() { return Err(crate::tr::err("changes.clash", &[("files", &clash.join(", "))])); }
    if !special.is_empty() { return Err(crate::tr::err("changes.special", &[("files", &special.join(", "))])); }
    Ok(steps)
}

/// Is the folder `dir` (which exists) really inside the library, links resolved?
fn inside(lib: &Path, dir: &Path) -> anyhow::Result<()> {
    if dunce::canonicalize(dir)?.starts_with(real(lib)) { Ok(()) } else { anyhow::bail!("{} is not inside the library", dir.display()) }
}

/// Write `bytes` as the library file `rel`, never through a link: a link found there is removed
/// (its target is never touched) and the file takes its place. Only inside the library, checked on
/// the real path. The new file is written beside it, given its mode, then renamed over the old name,
/// so no link is ever followed, for the contents or for the mode.
fn put_file(lib: &Path, rel: &str, bytes: &[u8], exec: bool) -> anyhow::Result<()> {
    use std::io::Write;
    let p = target(lib, rel).ok_or_else(|| anyhow::anyhow!("{rel} is reached through a link"))?;
    let parent = p.parent().ok_or_else(|| anyhow::anyhow!("{rel} has no folder"))?.to_path_buf();
    std::fs::create_dir_all(&parent)?;
    inside(lib, &parent)?;
    #[allow(unused_mut)]
    let mut mode: Option<u32> = None;
    match std::fs::symlink_metadata(&p) {
        Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(&p)?,
        Ok(m) if m.is_dir() => anyhow::bail!("{rel} is a folder now"),
        #[cfg(unix)]
        Ok(m) => { use std::os::unix::fs::PermissionsExt; mode = Some(m.permissions().mode()); }
        #[cfg(not(unix))]
        Ok(_) => {}
        Err(_) => {}
    }
    let tmp = parent.join(format!(".thoughts-{}.tmp", uuid::Uuid::new_v4()));
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    let done = (|| -> std::io::Result<()> {
        f.write_all(bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let base = mode.unwrap_or(0o644) & 0o7777;
            f.set_permissions(std::fs::Permissions::from_mode(if exec { base | 0o111 } else { base & !0o111 }))?;
        }
        #[cfg(not(unix))]
        let _ = (exec, mode);
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &p)
    })();
    if done.is_err() { let _ = std::fs::remove_file(&tmp); }
    Ok(done?)
}

/// Remove the library file `rel` (a link is removed itself, never what it points at), and the
/// folders that held only it.
fn remove_file_at(lib: &Path, rel: &str) -> anyhow::Result<()> {
    let p = target(lib, rel).ok_or_else(|| anyhow::anyhow!("{rel} is reached through a link"))?;
    let Some(parent) = p.parent().filter(|d| d.is_dir()) else { return Ok(()) };
    inside(lib, parent)?;
    if std::fs::symlink_metadata(&p).map_or(false, |m| !m.is_dir()) { std::fs::remove_file(&p)?; }
    // Folders that existed only for it go too (never one that still holds anything).
    let mut cur = Path::new(rel).parent();
    while let Some(r) = cur.filter(|r| !r.as_os_str().is_empty()) {
        if std::fs::remove_dir(lib.join(r)).is_err() { break; }
        cur = r.parent();
    }
    Ok(())
}

/// Write the planned files: removals first (a file may take the place of a folder that goes).
fn carry_out(lib: &Path, steps: &[(String, Step)]) -> anyhow::Result<()> {
    for (rel, _) in steps.iter().filter(|(_, s)| matches!(s, Step::Remove)) { remove_file_at(lib, rel)?; }
    for (rel, step) in steps {
        if let Step::Write(bytes, exec) = step { put_file(lib, rel, bytes, *exec)?; }
    }
    Ok(())
}

/// Put every file of the folder back as it was at commit `point`, one by one: a file changed since
/// gets its recorded version again, a file created since is removed — never anything the history
/// ignores, and never by resetting the folder as a whole. The history's index agrees afterwards.
/// Returns the files created since that were removed. (`git::rollback_to` for a cut-off Wrap.)
pub async fn restore_all(state: &AppState, dir: &Path, point: &str) -> anyhow::Result<Vec<String>> {
    if !crate::git::valid_hash(point) { anyhow::bail!("invalid snapshot"); }
    let now = snapshot(state, dir).await?;
    // old: as it is now; new: as it was at `point`.
    let back = deltas(state, dir, &now, point).await?;
    let (mut steps, mut created, mut special) = (vec![], vec![], vec![]);
    for d in &back {
        if target(dir, &d.path).is_none() { anyhow::bail!("{} cannot be put back: it is reached through a link", d.path); }
        match &d.new {
            None => { created.push(d.path.clone()); steps.push((d.path.clone(), Step::Remove)); }
            Some(n) if regular(n) => steps.push((d.path.clone(), Step::Write(blob(state, dir, &n.1).await?, n.0 == "100755"))),
            // A link or another special item: git itself puts that one file back.
            Some(_) => special.push(d.path.clone()),
        }
    }
    carry_out(dir, &steps)?;
    for p in &special {
        // Whatever is there now goes first (a link is removed itself); git then writes the recorded item.
        remove_file_at(dir, p)?;
        git(state, dir, &["checkout", "-q", point, "--", &format!(":(top,literal){p}")]).await?;
    }
    put_entries(state, dir, None, &back).await?;
    Ok(created)
}

/// Take back what an answer changed (`back`: put it back again after it was taken back). The caller
/// holds the library (no run at work in it, nothing else changing it). Only the answer's own files
/// are touched, each merged with what changed it since; refused when a later change touched the
/// same lines. Recorded as a history entry. Returns `{ok, message, hash}`.
pub async fn undo(state: &AppState, lib: &Path, chat: &str, msg: &str, back: bool) -> anyhow::Result<Value> {
    let t = change_set(state, lib, chat, msg).await?;
    if t["undone"].is_null() == back { return Err(crate::tr::err(if back { "changes.not_undone" } else { "changes.already" }, &[])); }
    // Taking the change back goes from its result to where it started; putting it back, from where the undo left it to where the undo started.
    let point = |v: &Value| v.as_str().unwrap_or("").to_string();
    let (base, goal) = if back { (point(&t["undone"]["after"]), point(&t["undone"]["before"])) } else { (point(&t["after"]), point(&t["before"])) };
    let subject = t["subject"].as_str().unwrap_or("assistant").to_string();
    let _g = state.git_lock.lock().await;
    crate::trust::require_git(lib)?;
    present(state, lib, &[&base, &goal]).await?;
    let now = snapshot(state, lib).await?;
    let steps = plan(state, lib, &base, &goal, &now).await?;
    if steps.is_empty() { return Err(crate::tr::err("history.nothing", &[])); }
    // What the user had not recorded yet is recorded first, so this entry holds the undo alone.
    crate::git::commit_all(state, lib, &crate::git::subject::edit("changes before undoing an assistant's changes")).await?;
    carry_out(lib, &steps)?;
    let after = snapshot(state, lib).await?;
    let touched: BTreeSet<&str> = steps.iter().map(|(p, _)| p.as_str()).collect();
    let made: Vec<Delta> = deltas(state, lib, &now, &after).await?.into_iter().filter(|d| touched.contains(d.path.as_str())).collect();
    let what = subject.strip_prefix("assistant: ").unwrap_or(&subject);
    let entry = if back { crate::git::subject::edit(&format!("put back what the assistant changed ({what})")) } else { crate::git::subject::undo(&subject) };
    let hash = record(state, lib, &made, &entry).await?;
    let undone = if back {
        let _ = git(state, lib, &["update-ref", "-d", &format!("{REFS}/{chat}/{msg}-undo")]).await;
        Value::Null
    } else {
        let (start, end) = states(state, lib, &now, &made).await?;
        crate::log::ok_or_warn("changes: could not keep the undo", git(state, lib, &["update-ref", &format!("{REFS}/{chat}/{msg}-undo"), &end]).await);
        json!({"before": start, "after": end, "at": Utc::now(), "history": hash})
    };
    drop(_g);
    let mut updated: Option<Value> = None;
    state.chats.update(chat, |c| if let Some(m) = c.messages.iter_mut().find(|m| m.id == msg) {
        if let Some(t) = m.tool.as_mut() { t["undone"] = undone; }
        updated = serde_json::to_value(&*m).ok();
    }).await;
    crate::log::info(format!("changes: {} what an answer in chat {chat} changed ({} file(s))", if back { "put back" } else { "undid" }, made.len()));
    if let Some(m) = &updated { state.emit(json!({"type": "chat.message", "chatId": chat, "message": m})); }
    for e in ["library.changed", "todos.changed", "history.changed"] { state.emit(json!({"type": e})); }
    Ok(json!({"ok": true, "message": updated, "hash": hash}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wraps::testutil as t;

    /// A chat whose run is watched from now on in the test library.
    async fn watched_chat(st: &Arc<AppState>, lib: &Path, title: &str) -> (String, Watching) {
        let chat = t::chat(st, title).await;
        let w = begin(st, &chat, lib, lib).await.expect("watched");
        (chat, w)
    }
    /// A tool row in the chat, as the stream would add it.
    async fn tool(st: &AppState, chat: &str, name: &str, input: Value) {
        let mut m = Msg::new("tool", name);
        m.tool = Some(json!({"name": name, "input": input}));
        st.chats.update(chat, |c| c.messages.push(m)).await;
    }
    async fn changes(st: &AppState, chat: &str) -> Option<Value> {
        st.chats.with(chat, |c| c.messages.iter().find(|m| m.role == "changes").and_then(|m| m.tool.clone())).await.flatten()
    }
    async fn msg_id(st: &AppState, chat: &str) -> String {
        st.chats.with(chat, |c| c.messages.iter().find(|m| m.role == "changes").map(|m| m.id.clone())).await.flatten().unwrap()
    }
    fn paths(v: &Value, key: &str) -> Vec<String> {
        v[key].as_array().unwrap().iter().map(|f| f.get("path").unwrap_or(f).as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn the_history_s_list_of_changed_files_is_read_exactly() {
        let out = ":100644 100644 aaaaaaa bbbbbbb M\0TODO.md\0:000000 100644 0000000 ccccccc A\0ملاحظات/ب, ج.md\0:100644 000000 ddddddd 0000000 D\0old.md\0";
        assert_eq!(parse_raw(out), vec![
            Delta { path: "TODO.md".into(), old: Some(("100644".into(), "aaaaaaa".into())), new: Some(("100644".into(), "bbbbbbb".into())) },
            Delta { path: "ملاحظات/ب, ج.md".into(), old: None, new: Some(("100644".into(), "ccccccc".into())) },
            Delta { path: "old.md".into(), old: Some(("100644".into(), "ddddddd".into())), new: None },
        ]);
        assert!(parse_raw("").is_empty());
    }

    #[test]
    fn what_was_written_by_someone_else_is_set_aside_and_never_guessed() {
        let d = |p: &str| Delta { path: p.into(), old: None, new: Some(("100644".into(), "1".into())) };
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<BTreeSet<String>>();
        // Only the user's save is someone else's.
        let (mine, others, unsure) = sort_out(vec![d("a.md"), d("b.md")], &set(&[]), &set(&["b.md"]), false);
        assert_eq!((mine.iter().map(|d| d.path.as_str()).collect::<Vec<_>>(), others, unsure), (vec!["a.md"], vec!["b.md".to_string()], false));
        // Another chat ran commands meanwhile: only what this run wrote with its own file tools is its own.
        let (mine, others, unsure) = sort_out(vec![d("a.md"), d("b.md"), d("c.md")], &set(&["a.md", "c.md"]), &set(&["c.md"]), true);
        assert_eq!((mine.iter().map(|d| d.path.as_str()).collect::<Vec<_>>(), others, unsure), (vec!["a.md"], vec!["b.md".to_string(), "c.md".to_string()], true));
    }

    #[test]
    fn tool_rows_name_library_files_and_commands_can_change_any() {
        let lib = Path::new("/home/u/Library");
        let since = Utc::now() - chrono::Duration::seconds(5);
        let row = |name: &str, input: Value| { let mut m = Msg::new("tool", name); m.tool = Some(json!({"name": name, "input": input})); m };
        let mut old = row("Write", json!({"file_path": "/home/u/Library/old.md"}));
        old.ts = since - chrono::Duration::seconds(60);
        let msgs = vec![old, row("Edit", json!({"file_path": "/home/u/Library/notes/a.md"})), row("Write", json!({"file_path": "notes/b.md"})),
            row("Write", json!({"file_path": "/elsewhere/c.md"})), row("NotebookEdit", json!({"notebook_path": "/home/u/Library/n.ipynb"})), row("Read", json!({"file_path": "/home/u/Library/r.md"}))];
        let (files, anywhere) = tool_writes(&msgs, since, lib, lib);
        assert_eq!(files.into_iter().collect::<Vec<_>>(), ["n.ipynb", "notes/a.md", "notes/b.md"]);
        assert!(!anywhere);
        assert!(tool_writes(&[row("Bash", json!({"command": "mv a b"}))], since, lib, lib).1);
        assert!(tool_writes(&[row("mcp__files__write", json!({}))], since, lib, lib).1);
    }

    #[tokio::test]
    async fn a_run_that_only_deletes_is_undone_and_one_that_changes_nothing_says_nothing() {
        let (st, lib) = t::library().await;
        let (quiet, _w) = watched_chat(&st, &lib, "Look only").await;
        let head = t::sh(&lib, &["rev-parse", "HEAD"]);
        assert!(finish(&st, &quiet, "assistant: Look only").await);
        assert!(changes(&st, &quiet).await.is_none());
        assert_eq!(t::sh(&lib, &["rev-parse", "HEAD"]), head);

        let (chat, _w) = watched_chat(&st, &lib, "Clear out").await;
        std::fs::remove_file(lib.join("ideas/shop/README.md")).unwrap();
        std::fs::remove_dir(lib.join("ideas/shop")).unwrap();
        assert!(finish(&st, &chat, "assistant: Clear out").await);
        let c = changes(&st, &chat).await.unwrap();
        assert_eq!(c["files"], json!([{"path": "ideas/shop/README.md", "from": null, "change": "deleted"}]));
        assert_eq!(t::sh(&lib, &["log", "-1", "--format=%s"]), "assistant: Clear out");
        let r = undo(&st, &lib, &chat, &msg_id(&st, &chat).await, false).await.unwrap();
        assert_eq!(t::read(&lib, "ideas/shop/README.md"), "---\ntitle: Shop\nstatus: seed\n---\nOriginal idea.\n");
        assert!(r["hash"].is_string());
        assert!(t::sh(&lib, &["status", "--porcelain"]).is_empty(), "the undo is recorded, and the history's index agrees");
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn arabic_names_and_a_later_edit_in_other_lines_survive_an_undo() {
        let (st, lib) = t::library().await;
        t::write(&lib, "أفكار/متجر صغير.md", "سطر أول\nسطر ثانٍ\nسطر ثالث\nسطر رابع\nسطر خامس\n");
        t::sh(&lib, &["add", "-A"]);
        t::sh(&lib, &["commit", "-q", "-m", "arabic"]);
        let (chat, _w) = watched_chat(&st, &lib, "عدّل الفكرة").await;
        t::write(&lib, "أفكار/متجر صغير.md", "سطر أول — معدّل\nسطر ثانٍ\nسطر ثالث\nسطر رابع\nسطر خامس\n");
        t::write(&lib, "ملاحظات/جديدة.md", "ملاحظة جديدة\n");
        assert!(finish(&st, &chat, "assistant: عدّل الفكرة").await);
        let c = changes(&st, &chat).await.unwrap();
        assert_eq!(paths(&c, "files"), ["أفكار/متجر صغير.md", "ملاحظات/جديدة.md"]);
        assert!(c["files"].as_array().unwrap().iter().any(|f| f["change"] == "added"));
        // The user edits another line of the same note afterwards.
        t::write(&lib, "أفكار/متجر صغير.md", "سطر أول — معدّل\nسطر ثانٍ\nسطر ثالث\nسطر رابع\nسطر خامس — كتبته أنا\n");
        let id = msg_id(&st, &chat).await;
        let v = view(&st, &lib, &chat, &id).await.unwrap();
        assert_eq!(v["files"][0]["hunks"][0]["lines"][1], json!(["+", "سطر أول — معدّل"]));
        undo(&st, &lib, &chat, &id, false).await.unwrap();
        assert_eq!(t::read(&lib, "أفكار/متجر صغير.md"), "سطر أول\nسطر ثانٍ\nسطر ثالث\nسطر رابع\nسطر خامس — كتبته أنا\n", "only the answer's line went back");
        assert!(!lib.join("ملاحظات").exists());
        // Putting them back keeps the user's line too.
        undo(&st, &lib, &chat, &id, true).await.unwrap();
        assert_eq!(t::read(&lib, "أفكار/متجر صغير.md"), "سطر أول — معدّل\nسطر ثانٍ\nسطر ثالث\nسطر رابع\nسطر خامس — كتبته أنا\n");
        assert_eq!(t::read(&lib, "ملاحظات/جديدة.md"), "ملاحظة جديدة\n");
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn two_chats_at_work_together_each_keep_only_their_own_changes() {
        let (st, lib) = t::library().await;
        let (a, _wa) = watched_chat(&st, &lib, "First").await;
        let (b, _wb) = watched_chat(&st, &lib, "Second").await;
        // Each writes with its file tools; the second also runs a command, which could change anything.
        tool(&st, &a, "Write", json!({"file_path": lib.join("a.md").to_string_lossy()})).await;
        t::write(&lib, "a.md", "from the first chat\n");
        tool(&st, &b, "Edit", json!({"file_path": lib.join("TODO.md").to_string_lossy()})).await;
        t::write(&lib, "TODO.md", "# Todo\n\n- [ ] call the bank\n- [ ] from the second chat\n");
        tool(&st, &b, "Bash", json!({"command": "echo x > b.md"})).await;
        t::write(&lib, "b.md", "made by a command\n");
        assert!(finish(&st, &a, "assistant: First").await);
        let ca = changes(&st, &a).await.unwrap();
        assert_eq!(paths(&ca, "files"), ["a.md"]);
        assert_eq!(ca["others"], json!(["TODO.md", "b.md"]));
        assert_eq!(ca["othersUnsure"], true);
        assert!(finish(&st, &b, "assistant: Second").await);
        let cb = changes(&st, &b).await.unwrap();
        assert_eq!(paths(&cb, "files"), ["TODO.md", "b.md"]);
        assert_eq!(cb["others"], json!(["a.md"]), "the first chat's file is said, and not counted");
        let h = t::sh(&lib, &["log", "-2", "--name-only", "--format=%s"]);
        assert_eq!(h.lines().filter(|l| !l.is_empty()).collect::<Vec<_>>(), ["assistant: Second", "TODO.md", "b.md", "assistant: First", "a.md"]);
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn a_save_fenced_during_a_run_is_the_user_s_and_a_run_not_finished_is_forgotten() {
        let (st, lib) = t::library().await;
        let (chat, w) = watched_chat(&st, &lib, "Plan").await;
        t::write(&lib, "notes/plan.md", "the run's\n");
        {
            let _save = fence(&st).await.expect("a run is watched");
            t::write(&lib, "README.md", "the user's\n");
        }
        assert!(finish(&st, &chat, "assistant: Plan").await);
        let c = changes(&st, &chat).await.unwrap();
        assert_eq!((paths(&c, "files"), c["others"].clone(), c["othersUnsure"].clone()), (vec!["notes/plan.md".to_string()], json!(["README.md"]), json!(false)));
        drop(w);
        assert!(!watched(&lib) && fence(&st).await.is_none(), "nothing is fenced once no run is watched");
        // A run that never finishes (its start failed, the app stopped) leaves no watch behind.
        let (other, w) = watched_chat(&st, &lib, "Never").await;
        drop(w);
        assert!(!finish(&st, &other, "assistant: Never").await);
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_note_turned_into_a_link_to_a_file_elsewhere_is_put_back_in_the_library_and_the_file_elsewhere_is_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let (st, lib) = t::library().await;
        let outside = lib.parent().unwrap().join("dotfile");
        std::fs::write(&outside, "export SECRET=1\n").unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o600)).unwrap();
        let untouched = |why: &str| {
            assert_eq!(std::fs::read_to_string(&outside).unwrap(), "export SECRET=1\n", "{why}");
            assert_eq!(std::fs::metadata(&outside).unwrap().permissions().mode() & 0o777, 0o600, "{why}");
        };
        // The run replaced a note with a link to a file outside the library.
        let (chat, _w) = watched_chat(&st, &lib, "Link it").await;
        std::fs::remove_file(lib.join("TODO.md")).unwrap();
        std::os::unix::fs::symlink(&outside, lib.join("TODO.md")).unwrap();
        assert!(finish(&st, &chat, "assistant: Link it").await);
        undo(&st, &lib, &chat, &msg_id(&st, &chat).await, false).await.unwrap();
        untouched("undo");
        let m = std::fs::symlink_metadata(lib.join("TODO.md")).unwrap();
        assert!(m.is_file() && !m.file_type().is_symlink(), "a plain note again");
        assert_eq!(t::read(&lib, "TODO.md"), "# Todo\n\n- [ ] call the bank\n");

        // After an answer edited a note, something else replaced it with such a link: Undo refuses.
        let (chat, _w) = watched_chat(&st, &lib, "Edit").await;
        t::write(&lib, "README.md", "edited by the answer\n");
        assert!(finish(&st, &chat, "assistant: Edit").await);
        std::fs::remove_file(lib.join("README.md")).unwrap();
        std::os::unix::fs::symlink(&outside, lib.join("README.md")).unwrap();
        assert!(undo(&st, &lib, &chat, &msg_id(&st, &chat).await, false).await.is_err());
        untouched("refused undo");

        // Putting the whole folder back as it was (a cut-off Wrap) never writes through it either.
        std::fs::remove_file(lib.join("README.md")).unwrap();
        t::write(&lib, "README.md", "my library\n");
        t::sh(&lib, &["add", "-A"]);
        t::sh(&lib, &["commit", "-q", "-m", "snapshot before wrap"]);
        let snap = t::sh(&lib, &["rev-parse", "HEAD"]);
        std::fs::remove_file(lib.join("README.md")).unwrap();
        std::os::unix::fs::symlink(&outside, lib.join("README.md")).unwrap();
        crate::git::rollback_to(&st, &lib, &snap).await.unwrap();
        untouched("rollback");
        assert!(!std::fs::symlink_metadata(lib.join("README.md")).unwrap().file_type().is_symlink());
        assert_eq!(t::read(&lib, "README.md"), "my library\n");
        assert_eq!(t::sh(&lib, &["status", "--porcelain"]), "");
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn undo_never_writes_through_a_link_and_is_refused_when_the_same_lines_changed_again() {
        let (st, lib) = t::library().await;
        let outside = lib.parent().unwrap().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let (chat, _w) = watched_chat(&st, &lib, "Ideas").await;
        std::fs::remove_file(lib.join("ideas/shop/README.md")).unwrap();
        t::write(&lib, "TODO.md", "# Todo\n\n- [x] call the bank\n");
        assert!(finish(&st, &chat, "assistant: Ideas").await);
        let id = msg_id(&st, &chat).await;
        // The folder became a link to somewhere else: the file is never written there.
        std::fs::remove_dir(lib.join("ideas/shop")).unwrap();
        std::os::unix::fs::symlink(&outside, lib.join("ideas/shop")).unwrap();
        let e = crate::tr::classify_in(false, &undo(&st, &lib, &chat, &id, false).await.unwrap_err());
        assert_eq!(e.code, "conflict");
        assert!(e.message.contains("ideas/shop/README.md"), "{}", e.message);
        assert!(!outside.join("README.md").exists());
        assert_eq!(t::read(&lib, "TODO.md"), "# Todo\n\n- [x] call the bank\n", "nothing was changed");
        std::fs::remove_file(lib.join("ideas/shop")).unwrap();
        // The same line changed again by the user: refused, naming the document.
        t::write(&lib, "TODO.md", "# Todo\n\n- [x] call the bank today\n");
        let e = crate::tr::classify_in(false, &undo(&st, &lib, &chat, &id, false).await.unwrap_err());
        assert!(e.message.contains("TODO.md") && !e.message.contains("README"), "{}", e.message);
        assert_eq!(t::read(&lib, "TODO.md"), "# Todo\n\n- [x] call the bank today\n");
        // Another library is not this one's.
        let (_st2, lib2) = t::library().await;
        assert_eq!(crate::tr::classify_in(false, &undo(&st, &lib2, &chat, &id, false).await.unwrap_err()).code, "conflict");
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
        std::fs::remove_dir_all(lib2.parent().unwrap()).unwrap();
    }
}
