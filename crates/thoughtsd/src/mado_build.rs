//! MADO's build engine (`docs/mado-spec.md`, "Build engine"): the app builds an approved plan by
//! itself, in waves of lanes, each lane in a git worktree of its own under
//! `<thoughts home>/mado/<projectId>/`, on a branch of its own (`mado/build-<n>`).
//!
//! - `build.json` in the project's `mado/` folder plus git are the truth; this file keeps only who
//!   is working right now (`LIVE`) in memory, so a restart resumes from the file (`resume`).
//! - The user's own branch and working tree are never checked out, committed to, reset or merged
//!   into while a build runs. Only `land` touches them, and only when the build is done, into a
//!   clean tree, refusing (and changing nothing) when the two clash.
//! - A task is done when its run ended, its acceptance commands (approved verbatim in the Review
//!   step) pass in its lane's worktree, and the daemon made its commit. A failure is tried once more
//!   with what went wrong; then the build waits for the user. Lanes are merged into the build
//!   branch by the daemon, one at a time; a clash gets one run that settles it, else the build waits.
//! - Before a lane starts, the governor looks at the usage the app already meters (`meter.rs`) and
//!   the spending guard (`spend.rs`), and holds new lanes above the threshold; it goes on by itself.
//!
//! Kept after the fact (the record of choices, see the worker report): `stop` removes every
//! worktree and lane branch and keeps `mado/build-<n>` when lanes were merged into it (else that
//! goes too); a finished build removes its worktrees and lane branches and keeps `mado/build-<n>`
//! for `land`, which deletes it (safe delete) once it is in the user's branch.

use crate::chats::{Chat, Msg, RunOptions};
use crate::claude::{self, RunSpec};
// Until the planning core (`mado.rs`) is merged: the same names, from a stand-in.
use crate::mado;
use crate::state::AppState;
use axum::extract::{Path as UrlPath, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::future::BoxFuture;
use mado::{PlanDoc, PlanTask};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How many times a task is tried before the build waits for the user (the first try and one more).
/// (A run has no commands of its own: the daemon checks its work, and a failed check goes back to
/// the same chat up to three times.)
pub const MAX_ATTEMPTS: u32 = 4;
pub const DEFAULT_PARALLEL: u32 = 3;
pub const MAX_PARALLEL: u32 = 6;
/// Longest one task's run may take before it is stopped and counted as failed.
#[cfg(not(test))]
const RUN_LIMIT: Duration = Duration::from_secs(60 * 60);
#[cfg(test)]
const RUN_LIMIT: Duration = Duration::from_secs(30);
/// Longest one acceptance command may take.
const CHECK_LIMIT: Duration = Duration::from_secs(10 * 60);
/// What is kept of a command's output: its start and its end.
const OUTPUT_HEAD: usize = 4 * 1024;
const OUTPUT_TAIL: usize = 12 * 1024;
/// The longest note kept on a task (characters, the end is kept).
const NOTE_MAX: usize = 4000;
/// The largest `mado/` file the engine reads or copies.
const FILE_MAX: u64 = 1024 * 1024;
/// How often a holding governor looks again.
#[cfg(not(test))]
const RECHECK: Duration = Duration::from_secs(60);
#[cfg(test)]
const RECHECK: Duration = Duration::from_millis(50);

// ---------- the shapes (`BuildState` in ui/src/lib/mado/types.ts) ----------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BuildStatus { #[default] Idle, Running, Paused, Waiting, Failed, Done, Stopped,
    /// Round 0.4.0: after the last wave, the started product's screens are looked at (`finish`).
    Checking,
    /// Round 0.4.0: a fresh run reviews the joined product, and `must` findings are fixed (`finish`).
    Reviewing }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus { #[default] Pending, Running, Checking, Done, Failed, Conflict, Skipped }

fn is_zero(n: &u32) -> bool { *n == 0 }

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildTask {
    pub id: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Tries started (daemon bookkeeping, beyond the contract's shape).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub attempts: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildLane {
    pub id: String,
    pub status: TaskStatus,
    pub tasks: Vec<BuildTask>,
    /// The run that settles a clash when this lane is merged (beyond the contract's shape).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_chat_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildWave {
    pub status: TaskStatus,
    pub lanes: Vec<BuildLane>,
    /// Its tasks' checks passed once more in the build's worktree after every lane was merged (beyond the contract's shape).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub checked: bool,
    /// The run that fixes what those checks found (beyond the contract's shape).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_chat_id: Option<String>,
    /// Round 0.4.0: the plan's `verify` commands passed in the build's copy after this wave.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verified: bool,
    /// Round 0.4.0: the run fixing a failed `verify` (the latest one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_fix_chat_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Governor {
    pub max_parallel: u32,
    pub running: u32,
    pub holding: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Default for Governor { fn default() -> Self { Governor { max_parallel: DEFAULT_PARALLEL, running: 0, holding: false, reason: None } } }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildState {
    pub version: u32,
    pub status: BuildStatus,
    pub branch: String,
    pub base: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    pub waves: Vec<BuildWave>,
    pub governor: Governor,
    #[serde(rename = "plan_hash")]
    pub plan_hash: String,
    // Beyond the contract's shape (additive, all optional for the window):
    /// The build's number (`mado/build-<n>`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub number: u32,
    /// The commit of the user's branch the build started from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_commit: String,
    /// What the user should know about the build as a whole (why it waits or failed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// When the finished build was brought into the user's branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landed_at: Option<String>,
    /// Round 0.4.0: the screen check after the last wave (`finish`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screens: Option<finish::BuildScreens>,
    /// Round 0.4.0: the review of the joined product after the last wave (`finish`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<finish::BuildReview>,
}

impl Default for BuildState {
    fn default() -> Self {
        BuildState { version: 1, status: BuildStatus::Idle, branch: String::new(), base: String::new(), started_at: None, waves: vec![],
            governor: Governor::default(), plan_hash: String::new(), number: 0, base_commit: String::new(), message: None, landed_at: None,
            screens: None, review: None }
    }
}

impl BuildState {
    fn lane_mut(&mut self, wi: usize, li: usize) -> Option<&mut BuildLane> { self.waves.get_mut(wi)?.lanes.get_mut(li) }
    fn task_mut(&mut self, wi: usize, li: usize, ti: usize) -> Option<&mut BuildTask> { self.lane_mut(wi, li)?.tasks.get_mut(ti) }
    fn under_way(&self) -> bool { matches!(self.status, BuildStatus::Running | BuildStatus::Paused | BuildStatus::Waiting | BuildStatus::Checking | BuildStatus::Reviewing) }
    /// Working by itself right now (a wave, the screen check, the review).
    fn active(&self) -> bool { matches!(self.status, BuildStatus::Running | BuildStatus::Checking | BuildStatus::Reviewing) }
}

// ---------- where things are ----------

/// A project id the engine may use as a folder name.
fn valid_pid(pid: &str) -> bool { !pid.is_empty() && pid.len() <= 64 && pid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') }

/// `<thoughts home>/mado/<projectId>`: everything the engine keeps outside the project.
fn engine_dir(pid: &str) -> anyhow::Result<PathBuf> {
    if !valid_pid(pid) { return Err(crate::tr::err("not_found.project", &[])); }
    Ok(crate::paths::home().join("mado").join(pid))
}
fn build_wt(pid: &str) -> anyhow::Result<PathBuf> { Ok(engine_dir(pid)?.join("build")) }
fn lane_wt(pid: &str, wi: usize, li: usize) -> anyhow::Result<PathBuf> { Ok(engine_dir(pid)?.join(format!("lane-{}-{}", wi + 1, li + 1))) }
fn lane_branch(n: u32, wi: usize, li: usize) -> String { format!("mado/build-{n}-lane-{}-{}", wi + 1, li + 1) }
/// The plan as it was approved, kept for the build's whole life (the project's copy may change).
fn snapshot_file(pid: &str) -> anyhow::Result<PathBuf> { Ok(engine_dir(pid)?.join("plan.json")) }

/// `<project>/mado`, a real folder (never a link out of the project).
fn mado_dir(project: &Path) -> anyhow::Result<PathBuf> {
    let d = project.join("mado");
    match std::fs::symlink_metadata(&d) {
        Ok(m) if m.is_dir() => Ok(d),
        _ => Err(crate::tr::err("mado_build.no_mado", &[])),
    }
}

/// A file the engine reads from `<project>/mado/` (`rel` is always one of its own names): `None`
/// when absent; refused when it is a link, not a plain file, or too large.
fn read_mado(project: &Path, rel: &str) -> anyhow::Result<Option<Vec<u8>>> {
    let p = mado_dir(project)?.join(rel);
    match std::fs::symlink_metadata(&p) {
        Err(_) => Ok(None),
        Ok(m) if m.is_file() && m.len() <= FILE_MAX => Ok(Some(std::fs::read(&p)?)),
        Ok(_) => Err(crate::tr::err("mado_build.bad_file", &[("file", rel)])),
    }
}

pub fn load_build(project: &Path) -> anyhow::Result<Option<BuildState>> {
    match read_mado(project, "build.json")? {
        None => Ok(None),
        Some(b) => Ok(Some(serde_json::from_slice(&b).map_err(|_| crate::tr::err("mado_build.bad_file", &[("file", "build.json")]))?)),
    }
}

fn save_build(project: &Path, b: &BuildState) -> anyhow::Result<()> {
    let p = mado_dir(project)?.join("build.json");
    if std::fs::symlink_metadata(&p).map_or(false, |m| !m.is_file()) { return Err(crate::tr::err("mado_build.bad_file", &[("file", "build.json")])); }
    crate::paths::write_atomic(&p, serde_json::to_string_pretty(b)?.as_bytes(), false)?;
    Ok(())
}

// ---------- who is working right now ----------

type Locks = std::sync::Mutex<std::collections::BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>;
/// One writer of a project's `build.json` at a time.
static STATE_LOCKS: Locks = std::sync::Mutex::new(std::collections::BTreeMap::new());
/// One change to a project's worktrees and branches at a time.
static GIT_LOCKS: Locks = std::sync::Mutex::new(std::collections::BTreeMap::new());
/// One merge into a project's build branch at a time (held through a clash's fix run).
static MERGE_LOCKS: Locks = std::sync::Mutex::new(std::collections::BTreeMap::new());
/// Lanes with a worker, per project (`"<wave>-<lane>"`).
static LIVE: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
/// Projects whose governor will look again shortly.
static RECHECKING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn lock_of(map: &'static Locks, pid: &str) -> Arc<tokio::sync::Mutex<()>> {
    map.lock().unwrap_or_else(|e| e.into_inner()).entry(pid.to_string()).or_default().clone()
}
fn key(wi: usize, li: usize) -> String { format!("{wi}-{li}") }
fn wave_key(wi: usize) -> String { format!("wave-{wi}") }
fn live_of(pid: &str) -> HashSet<String> { LIVE.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|(p, _)| p == pid).map(|(_, k)| k.clone()).collect() }
fn live_add(pid: &str, k: &str) { LIVE.lock().unwrap_or_else(|e| e.into_inner()).push((pid.to_string(), k.to_string())); }
fn live_remove(pid: &str, k: &str) {
    let mut l = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = l.iter().position(|(p, x)| p == pid && x == k) { l.remove(i); }
}

/// Change `build.json` under its lock. `None` when there is no build.
async fn update<T>(pid: &str, project: &Path, f: impl FnOnce(&mut BuildState) -> T) -> anyhow::Result<Option<T>> {
    let l = lock_of(&STATE_LOCKS, pid);
    let _g = l.lock().await;
    let Some(mut b) = load_build(project)? else { return Ok(None) };
    let r = f(&mut b);
    save_build(project, &b)?;
    Ok(Some(r))
}

fn tr(key: &str, args: &[(&str, &str)]) -> String { crate::tr::text(key, args) }

/// The end of `text`, at most `NOTE_MAX` characters.
fn clip_note(text: &str) -> String {
    let n = text.chars().count();
    if n <= NOTE_MAX { return text.to_string(); }
    format!("…{}", text.chars().skip(n - NOTE_MAX).collect::<String>())
}

async fn full(state: &AppState) -> bool { state.settings.read().await.full_access() }

// ---------- git ----------

async fn git(state: &AppState, dir: &Path, args: &[&str]) -> anyhow::Result<String> { crate::git::git(state, dir, args).await }
async fn git_ok(state: &AppState, dir: &Path, args: &[&str]) -> bool { crate::git::git_output(state, dir, args).await.map_or(false, |(ok, _)| ok) }
fn s(p: &Path) -> String { p.to_string_lossy().to_string() }

/// Refused when the repository's own config makes git start programs (filters, merge drivers, diff
/// converters, editors, pagers, credential helpers, signing): they would run with the engine's git
/// commands. A guard: the caller lets it yield to full access.
async fn no_git_programs(state: &AppState, project: &Path) -> anyhow::Result<()> {
    let keys = crate::git::program_keys(state, project).await?;
    if keys.is_empty() { return Ok(()); }
    let mut shown: Vec<String> = keys.iter().take(6).map(|k| k.chars().take(80).collect()).collect();
    if keys.len() > 6 { shown.push("…".into()); }
    Err(crate::tr::err("mado_build.git_programs", &[("keys", &shown.join(", "))]))
}

/// Is `e` the refusal of `no_git_programs`?
fn is_git_programs(e: &anyhow::Error) -> bool {
    let shown = crate::tr::classify(e).message;
    let head = |ar: bool| crate::tr::text_in(ar, "mado_build.git_programs", &[]).split('{').next().unwrap_or_default().to_string();
    shown.starts_with(&head(false)) || shown.starts_with(&head(true))
}

/// `no_git_programs` for `dir` (the project or one of the engine's worktrees), right before the
/// engine's own git changes files there; it yields to full access.
async fn git_safe(state: &AppState, dir: &Path) -> anyhow::Result<()> {
    if full(state).await { return Ok(()); }
    no_git_programs(state, dir).await
}

/// Changes in the project's working tree, the app's own `mado/` folder aside.
async fn dirty_files(state: &AppState, dir: &Path) -> anyhow::Result<Vec<String>> {
    let out = git(state, dir, &["status", "--porcelain=v1", "-z"]).await?;
    let mut files = vec![];
    let mut it = out.split('\0');
    while let Some(e) = it.next() {
        if e.len() < 4 { continue; }
        let (xy, path) = (&e[..2], &e[3..]);
        if xy.starts_with('R') || xy.starts_with('C') { it.next(); }
        if path == "mado" || path == "mado/" || path.starts_with("mado/") { continue; }
        files.push(path.to_string());
    }
    Ok(files)
}

/// The branch names `mado/build-<n>…` already in the repository: the next free number.
async fn next_number(state: &AppState, project: &Path, previous: u32) -> u32 {
    let refs = git(state, project, &["for-each-ref", "--format=%(refname:short)", "refs/heads/mado/"]).await.unwrap_or_default();
    let used = refs.lines().filter_map(|l| l.trim().strip_prefix("mado/build-")).filter_map(|r| r.split('-').next()?.parse::<u32>().ok()).max().unwrap_or(0);
    used.max(previous) + 1
}

/// Remove a worktree the engine made (only ever under `<home>/mado/<projectId>/`).
async fn remove_worktree(state: &AppState, project: &Path, pid: &str, wt: &Path) {
    let Ok(root) = engine_dir(pid) else { return };
    if !wt.starts_with(&root) || wt == root { return; }
    // Its trust record (made for every run in it) goes with it, by the name it was recorded under.
    crate::log::ok_or_warn("mado build: could not drop a removed worktree's trust record", crate::trust::forget(wt));
    if std::fs::symlink_metadata(wt).is_ok() {
        if let Err(e) = git(state, project, &["worktree", "remove", "--force", &s(wt)]).await {
            crate::log::warn(format!("mado build: git could not remove a worktree ({e}); removing the folder"));
        }
        if std::fs::symlink_metadata(wt).is_ok() { crate::log::ok_or_warn("mado build: could not remove a worktree folder", std::fs::remove_dir_all(wt)); }
    }
    let _ = std::fs::remove_file(setup_marker(wt));
    let _ = git(state, project, &["worktree", "prune"]).await;
    // And any record an earlier version left for a worktree of this project that is gone.
    crate::log::ok_or_warn("mado build: could not drop old worktrees' trust records", crate::trust::forget_missing_under(&root));
}

/// Where the engine notes that a worktree's set-up commands ran (beside it, never inside it).
fn setup_marker(wt: &Path) -> PathBuf {
    wt.with_file_name(format!("{}.setup", wt.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()))
}

/// Delete a branch the engine made (only ever `mado/…`).
async fn delete_branch(state: &AppState, project: &Path, branch: &str, force: bool) -> bool {
    if !branch.starts_with("mado/build-") { return false; }
    git(state, project, &["branch", "-q", if force { "-D" } else { "-d" }, branch]).await.is_ok()
}

/// Put a worktree of the engine's back to its last commit, then copy the plan's documents in again.
async fn reset_wt(state: &AppState, project: &Path, wt: &Path) {
    if !wt.join(".git").exists() { return; }
    if let Err(e) = git_safe(state, wt).await { crate::log::warn(format!("mado build: a worktree was not reset: {e}")); return; }
    if git_ok(state, wt, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).await { let _ = git(state, wt, &["merge", "--abort"]).await; }
    let _ = git(state, wt, &["reset", "-q", "--hard", "HEAD"]).await;
    let _ = git(state, wt, &["clean", "-fdq", "-e", "/mado/"]).await;
    crate::log::ok_or_warn("mado build: the plan's documents were not copied", copy_docs(project, wt, full(state).await));
}

/// The documents of the plan, copied into a worktree's `mado/` for the runs to read (they are
/// never part of the build's commits). Refused while `mado/` holds a link or a shared file (unless
/// full access is on): nothing is copied from behind one.
fn copy_docs(project: &Path, wt: &Path, full: bool) -> anyhow::Result<()> {
    let Ok(src) = mado_dir(project) else { return Ok(()) };
    mado::require_plain(&src, full)?;
    let dst = wt.join("mado");
    // A `mado` in the repository that is a link (or a file) is left alone: nothing is written through it.
    match std::fs::symlink_metadata(&dst) {
        Ok(m) if !m.is_dir() => return Ok(()),
        Err(_) => if std::fs::create_dir(&dst).is_err() { return Ok(()) },
        _ => {}
    }
    let mut names: Vec<String> = ["brief.md", "spec.md", "tech.md", "design.md", "plan.json", "flow.graph.json", "architecture.graph.json", "schema.graph.json",
        "design/tokens.json", "design/styles.json", "conventions.md"].iter().map(|n| n.to_string()).collect();
    if let Ok(rd) = std::fs::read_dir(src.join("design/screens")) {
        for e in rd.flatten().take(400) {
            let n = e.file_name().to_string_lossy().to_string();
            if n.ends_with(".html") && !n.starts_with('.') && !n.contains(['/', '\\']) { names.push(format!("design/screens/{n}")); }
        }
    }
    for rel in names {
        let from = src.join(&rel);
        let Ok(m) = std::fs::symlink_metadata(&from) else { continue };
        if !m.is_file() || m.len() > FILE_MAX { continue; }
        let to = dst.join(&rel);
        if let Some(parent) = to.parent() {
            // Every folder on the way is a real folder inside `dst`.
            if std::fs::create_dir_all(parent).is_err() || !std::fs::symlink_metadata(parent).map_or(false, |m| m.is_dir()) { continue; }
            if crate::paths::canonical(parent).map_or(true, |p| !p.starts_with(crate::paths::canonical(&dst).unwrap_or_default())) { continue; }
        }
        if std::fs::symlink_metadata(&to).map_or(false, |m| !m.is_file()) { continue; }
        let _ = std::fs::copy(&from, &to);
    }
    Ok(())
}

// ---------- may a build start? ----------

/// The task ids the contract allows.
fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty() && b.len() <= 64 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// What can keep a plan from being built: bad ids, unknown dependencies, a cycle.
fn plan_problem(plan: &PlanDoc) -> Option<String> {
    if plan.version != 1 || plan.tasks.is_empty() { return Some("no tasks".into()); }
    let mut seen = HashSet::new();
    for t in &plan.tasks {
        if !valid_id(&t.id) || !seen.insert(t.id.as_str()) { return Some(format!("task id “{}”", t.id.chars().take(70).collect::<String>())); }
    }
    for t in &plan.tasks {
        if let Some(d) = t.depends_on.iter().find(|d| !seen.contains(d.as_str())) { return Some(format!("{} → {}", t.id, d.chars().take(70).collect::<String>())); }
    }
    // A cycle: some tasks never get a level.
    let mut done: HashSet<&str> = HashSet::new();
    loop {
        let before = done.len();
        for t in &plan.tasks { if t.depends_on.iter().all(|d| done.contains(d.as_str())) { done.insert(&t.id); } }
        if done.len() == plan.tasks.len() { return None; }
        if done.len() == before {
            let caught: Vec<&str> = plan.tasks.iter().filter(|t| !done.contains(t.id.as_str())).map(|t| t.id.as_str()).take(5).collect();
            return Some(caught.join(" ↔ "));
        }
    }
}

struct Ready { plan: PlanDoc, bytes: Vec<u8>, hash: String, branch: String, head: String }

/// Everything a build needs before it may start, in the order the user would fix it.
/// `commit`: the build is starting, so what is not committed yet is committed first (`commit_pending`).
/// Without it (the view asking whether a build could start) nothing is changed, and pending changes
/// or a repository with no commit yet are not a reason to refuse: the start takes care of them.
async fn check_ready(state: &AppState, pid: &str, project: &Path, commit: bool) -> anyhow::Result<Ready> {
    // A repository of its own, the project's folder at its top.
    let top = git(state, project, &["rev-parse", "--show-toplevel"]).await.ok().map(|t| PathBuf::from(t.trim()));
    let same = top.as_deref().and_then(|t| crate::paths::canonical(t).ok()).map_or(false, |t| crate::paths::canonical(project).map_or(false, |p| p == t));
    if !same { return Err(crate::tr::err("mado_build.not_repo", &[])); }
    if !full(state).await { crate::trust::require_git(project)?; }
    git_safe(state, project).await?;
    // No link or shared file in `mado/` (a cloned repository can ship `mado/spec.md -> ~/.zshrc`).
    mado::require_plain(&mado_dir(project)?, full(state).await)?;
    // The Review step approved exactly the plan that is there now (its commands are what will run).
    // The approval is the daemon's own record, never anything in the project (`state.json` could be forged).
    let approval = mado::approved_plan(state, pid).await.ok_or_else(|| crate::tr::err("mado_build.not_approved", &[]))?;
    let bytes = read_mado(project, "plan.json")?.ok_or_else(|| crate::tr::err("mado_build.not_approved", &[]))?;
    let hash = mado::plan_hash(&bytes);
    if !mado::approval_matches(&approval, &bytes) { return Err(crate::tr::err("mado_build.plan_changed", &[])); }
    let plan: PlanDoc = serde_json::from_slice(&bytes).map_err(|_| crate::tr::err("mado_build.plan_invalid", &[("why", "plan.json")]))?;
    if let Some(why) = plan_problem(&plan) { return Err(crate::tr::err("mado_build.plan_invalid", &[("why", &why)])); }
    // Every command the build may run (set-up, checks, `verify`, `run.start`) is one the user was shown and approved.
    if plan.all_commands().into_iter().any(|c| !approval.commands.contains(c)) {
        return Err(crate::tr::err("mado_build.plan_changed", &[]));
    }
    let branch = crate::git::current_branch(state, project).await.map_err(|_| crate::tr::err("mado_build.no_branch", &[]))?;
    if branch.starts_with("mado/") { return Err(crate::tr::err("mado_build.no_branch", &[])); }
    if !commit { return Ok(Ready { plan, bytes, hash, branch, head: crate::git::head(state, project).await.unwrap_or_default() }); }
    // The build's first step: the user's pending changes become a commit, so the build starts from
    // everything that is in the folder now (the plan in `mado/` included).
    commit_pending(state, project).await?;
    let head = crate::git::head(state, project).await.ok_or_else(|| crate::tr::err("mado_build.no_commit", &[]))?;
    let dirty = dirty_files(state, project).await?;
    if !dirty.is_empty() {
        let mut shown: Vec<String> = dirty.iter().take(5).cloned().collect();
        if dirty.len() > 5 { shown.push("…".into()); }
        return Err(crate::tr::err("mado_build.dirty", &[("files", &shown.join(", "))]));
    }
    Ok(Ready { plan, bytes, hash, branch, head })
}

/// The message of the commit a build makes of the user's pending changes before it starts.
pub const PENDING_COMMIT: &str = "Before the MADO build";

/// Commits everything that is not committed yet (new files too; what `.gitignore` leaves out stays
/// out, and so does the app's own `mado/` folder, as for `dirty_files`), on the user's own branch. A repository with no commit yet gets its first one, empty if
/// there is nothing in it. Nothing to commit: nothing is done. No hook of the project runs
/// (`git::git` switches them off), and with no git identity on this machine the commit is the app's.
async fn commit_pending(state: &AppState, project: &Path) -> anyhow::Result<()> {
    let first = crate::git::head(state, project).await.is_none();
    if !first && dirty_files(state, project).await?.is_empty() { return Ok(()); }
    // (Not the app's own `mado/` folder: it is never part of the user's history unless they put it there.)
    git(state, project, &["add", "-A", "--", ".", ":(exclude,top)mado"]).await?;
    let mut args = vec!["commit", "-q", "-m", PENDING_COMMIT];
    if first { args.push("--allow-empty"); }
    crate::git::git_ident(state, project, &args).await?;
    Ok(())
}

/// What the Final check says about the project's repository, as `(kind, message key, files)`:
/// `no-repo` (not a repository of its own) and `no-branch` keep a build from starting;
/// `uncommitted` only tells what the build will commit first (up to five names, then `…`).
pub async fn repo_findings(state: &AppState, project: &Path) -> Vec<(&'static str, &'static str, String)> {
    let top = git(state, project, &["rev-parse", "--show-toplevel"]).await.ok().map(|t| PathBuf::from(t.trim()));
    let same = top.as_deref().and_then(|t| crate::paths::canonical(t).ok()).map_or(false, |t| crate::paths::canonical(project).map_or(false, |p| p == t));
    if !same { return vec![("no-repo", "mado_build.not_repo", String::new())]; }
    // The same guards as a build's start, before any git command that reads the working tree
    // (`git status` runs the repository's filters): a repository that is not trusted, or whose own
    // settings make git start programs, is named and nothing more is asked of it.
    let guarded = async { if !full(state).await { crate::trust::require_git(project)?; } git_safe(state, project).await };
    if let Err(e) = guarded.await { return vec![("no-repo", "mado.p.git_unsafe", crate::tr::classify(&e).message)]; }
    let mut out = vec![];
    // (A repository with no commit yet names its branch all the same.)
    let branch = git(state, project, &["symbolic-ref", "--short", "-q", "HEAD"]).await.map(|b| b.trim().to_string()).unwrap_or_default();
    if branch.is_empty() || branch.starts_with("mado/") { out.push(("no-branch", "mado_build.no_branch", String::new())); }
    let dirty = dirty_files(state, project).await.unwrap_or_default();
    if !dirty.is_empty() || crate::git::head(state, project).await.is_none() {
        let mut shown: Vec<String> = dirty.iter().take(5).cloned().collect();
        if dirty.len() > 5 { shown.push("…".into()); }
        out.push(("uncommitted", if shown.is_empty() { "mado.p.first_commit" } else { "mado.p.uncommitted" }, shown.join(", ")));
    }
    out
}

// ---------- start, pause, resume, stop, retry, land ----------

/// The linked project's folder, which must still be trusted (a guard: it yields to full access).
async fn project_of(state: &AppState, pid: &str) -> anyhow::Result<PathBuf> {
    let dir = mado::project_dir(state, pid).await?;
    if !full(state).await { crate::trust::require(&dir)?; }
    Ok(dir)
}

/// `project_of` for the engine itself (picking a build up after a restart, and every step after
/// it): a project linked to any listed library, so a build goes on while another library is open.
/// The same trust check; what the user starts, stops or lands still goes through `project_of`.
async fn engine_project_of(state: &AppState, pid: &str) -> anyhow::Result<PathBuf> {
    let dir = mado::any_project_dir_in(&*state.settings.read().await, pid)?;
    if !full(state).await { crate::trust::require(&dir)?; }
    Ok(dir)
}

pub async fn start(state: &Arc<AppState>, pid: &str, max_parallel: Option<u32>) -> anyhow::Result<BuildState> {
    let project = project_of(state, pid).await?;
    let root = engine_dir(pid)?;
    let b = {
        let l = lock_of(&STATE_LOCKS, pid);
        let _g = l.lock().await;
        let old = load_build(&project)?;
        if old.as_ref().map_or(false, BuildState::under_way) || !live_of(pid).is_empty() { return Err(crate::tr::err("mado_build.busy", &[])); }
        let r = check_ready(state, pid, &project, true).await?;
        let gl = lock_of(&GIT_LOCKS, pid);
        let _gg = gl.lock().await;
        // Whatever an earlier build left behind under the engine's folder goes first.
        if let Some(o) = &old { cleanup(state, &project, pid, o, false).await; }
        remove_worktree(state, &project, pid, &build_wt(pid)?).await;
        let n = next_number(state, &project, old.as_ref().map_or(0, |o| o.number)).await;
        let branch = format!("mado/build-{n}");
        std::fs::create_dir_all(&root)?;
        crate::paths::write_atomic(&snapshot_file(pid)?, &r.bytes, true)?;
        // A new branch at the user's commit, checked out only in the engine's own worktree.
        git(state, &project, &["branch", "--no-track", &branch, &r.head]).await?;
        if let Err(e) = git(state, &project, &["worktree", "add", "-q", &s(&build_wt(pid)?), &branch]).await {
            delete_branch(state, &project, &branch, true).await;
            return Err(e);
        }
        if let Err(e) = git_safe(state, &build_wt(pid)?).await {
            remove_worktree(state, &project, pid, &build_wt(pid)?).await;
            delete_branch(state, &project, &branch, true).await;
            return Err(e);
        }
        crate::log::ok_or_warn("mado build: the plan's documents were not copied", copy_docs(&project, &build_wt(pid)?, full(state).await));
        let waves = mado::waves(&r.plan).into_iter().map(|w| BuildWave {
            status: TaskStatus::Pending, checked: false, fix_chat_id: None, verified: false, gate_fix_chat_id: None,
            lanes: w.lanes.into_iter().map(|l| BuildLane { id: l.id, status: TaskStatus::Pending, fix_chat_id: None,
                tasks: l.tasks.into_iter().map(|id| BuildTask { id, ..Default::default() }).collect() }).collect(),
        }).collect();
        let b = BuildState {
            version: 1, status: BuildStatus::Running, branch, base: r.branch, started_at: Some(chrono::Utc::now().to_rfc3339()), waves,
            governor: Governor { max_parallel: max_parallel.unwrap_or(DEFAULT_PARALLEL).clamp(1, MAX_PARALLEL), ..Default::default() },
            plan_hash: r.hash, number: n, base_commit: r.head, message: None, landed_at: None, screens: None, review: None,
        };
        save_build(&project, &b)?;
        b
    };
    crate::log::info(format!("mado build {}: started ({} wave(s))", b.number, b.waves.len()));
    mado::tell(state, pid, "build").await;
    tick(state.clone(), pid.to_string()).await;
    Ok(load_build(&project)?.unwrap_or(b))
}

pub async fn pause(state: &Arc<AppState>, pid: &str) -> anyhow::Result<BuildState> {
    let project = project_of(state, pid).await?;
    let r = update(pid, &project, |b| if b.active() { b.status = BuildStatus::Paused; true } else { false }).await?
        .ok_or_else(|| crate::tr::err("mado_build.none", &[]))?;
    if !r { return Err(not_now(&project)); }
    // A started product (the screen check) is stopped at once; it starts again on resume.
    finish::end_app(pid).await;
    mado::tell(state, pid, "build").await;
    Ok(load_build(&project)?.unwrap_or_default())
}

/// Paused, failed as a whole, or waiting (after a failed set-up or wave check, which are tried again): go on.
pub async fn resume_build(state: &Arc<AppState>, pid: &str) -> anyhow::Result<BuildState> {
    let project = project_of(state, pid).await?;
    let r = update(pid, &project, |b| if matches!(b.status, BuildStatus::Paused | BuildStatus::Failed | BuildStatus::Waiting) { b.status = BuildStatus::Running; b.message = None; true } else { false }).await?
        .ok_or_else(|| crate::tr::err("mado_build.none", &[]))?;
    if !r { return Err(not_now(&project)); }
    mado::tell(state, pid, "build").await;
    tick(state.clone(), pid.to_string()).await;
    Ok(load_build(&project)?.unwrap_or_default())
}

fn not_now(project: &Path) -> anyhow::Error {
    let status = load_build(project).ok().flatten().map(|b| serde_json::to_value(b.status).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()).unwrap_or_default();
    crate::tr::err("mado_build.not_now", &[("status", &status)])
}

/// Stop the build: its runs are stopped the usual way, its worktrees removed.
pub async fn stop(state: &Arc<AppState>, pid: &str) -> anyhow::Result<BuildState> {
    let project = project_of(state, pid).await?;
    let chats = update(pid, &project, |b| {
        if !b.under_way() && b.status != BuildStatus::Failed { return None; }
        b.status = BuildStatus::Stopped;
        b.governor.holding = false;
        b.governor.reason = None;
        let mut chats = vec![];
        if let Some(r) = &b.review { chats.extend(r.chat_id.iter().chain(r.fix_chat_id.iter()).cloned()); }
        for w in &b.waves { chats.extend(w.fix_chat_id.iter().chain(w.gate_fix_chat_id.iter()).cloned()); for l in &w.lanes {
            if let Some(c) = &l.fix_chat_id { chats.push(c.clone()); }
            for t in &l.tasks { if matches!(t.status, TaskStatus::Running | TaskStatus::Checking) { if let Some(c) = &t.chat_id { chats.push(c.clone()); } } }
        } }
        Some(chats)
    }).await?.ok_or_else(|| crate::tr::err("mado_build.none", &[]))?.ok_or_else(|| not_now(&project))?;
    for c in chats { if let Some(k) = state.runs.lock().await.remove(&c) { let _ = k.send(()); } }
    finish::end_app(pid).await;
    crate::log::info("mado build: stopped by the user");
    mado::tell(state, pid, "build").await;
    if live_of(pid).is_empty() { cleanup_stopped(state, &project, pid).await; }
    Ok(load_build(&project)?.unwrap_or_default())
}

async fn cleanup_stopped(state: &AppState, project: &Path, pid: &str) {
    let Ok(Some(b)) = load_build(project) else { return };
    if b.status != BuildStatus::Stopped { return; }
    let gl = lock_of(&GIT_LOCKS, pid);
    let _g = gl.lock().await;
    cleanup(state, project, pid, &b, true).await;
}

/// Remove the build's worktrees and lane branches. `stopped`: the build branch also goes when
/// nothing was merged into it (it is still the user's commit).
async fn cleanup(state: &AppState, project: &Path, pid: &str, b: &BuildState, stopped: bool) {
    for (wi, w) in b.waves.iter().enumerate() {
        for li in 0..w.lanes.len() {
            if let Ok(wt) = lane_wt(pid, wi, li) { remove_worktree(state, project, pid, &wt).await; }
            if b.number > 0 { delete_branch(state, project, &lane_branch(b.number, wi, li), true).await; }
        }
    }
    if let Ok(wt) = build_wt(pid) { remove_worktree(state, project, pid, &wt).await; }
    // The build's own caches go with its worktrees.
    if let Ok(c) = cache_dir(pid) { if std::fs::symlink_metadata(&c).is_ok() { crate::log::ok_or_warn("mado build: could not remove its caches", std::fs::remove_dir_all(&c)); } }
    if stopped && !b.branch.is_empty() && !b.base_commit.is_empty() {
        let tip = git(state, project, &["rev-parse", "-q", "--verify", &format!("refs/heads/{}", b.branch)]).await.unwrap_or_default();
        if tip.trim() == b.base_commit { delete_branch(state, project, &b.branch, true).await; }
    }
}

/// Try a failed or clashing task again (the build waits for this).
pub async fn retry(state: &Arc<AppState>, pid: &str, task: &str) -> anyhow::Result<BuildState> {
    let project = project_of(state, pid).await?;
    let found = update(pid, &project, |b| {
        if !matches!(b.status, BuildStatus::Waiting | BuildStatus::Failed | BuildStatus::Running | BuildStatus::Paused) { return Err(crate::tr::err("mado_build.retry_bad", &[])); }
        let n = b.number;
        for (wi, w) in b.waves.iter_mut().enumerate() {
            for (li, l) in w.lanes.iter_mut().enumerate() {
                let Some(t) = l.tasks.iter_mut().find(|t| t.id == task) else { continue };
                match t.status {
                    TaskStatus::Failed => { t.status = TaskStatus::Pending; t.attempts = 0; t.chat_id = None; }
                    // Its commit is on the lane's branch: the merge (with one fix run) is tried again.
                    TaskStatus::Conflict => for t in l.tasks.iter_mut().filter(|t| t.status == TaskStatus::Conflict) { t.status = TaskStatus::Done; },
                    _ => return Err(crate::tr::err("mado_build.retry_bad", &[])),
                }
                l.status = TaskStatus::Pending;
                l.fix_chat_id = None;
                w.status = TaskStatus::Running;
                if b.status != BuildStatus::Paused { b.status = BuildStatus::Running; }
                b.message = None;
                return Ok((wi, li, n));
            }
        }
        Err(crate::tr::err("mado_build.no_task", &[]))
    }).await?.ok_or_else(|| crate::tr::err("mado_build.none", &[]))??;
    // A failed task starts again from its lane's last commit (what went wrong is in its note).
    if let Ok(wt) = lane_wt(pid, found.0, found.1) {
        let gl = lock_of(&GIT_LOCKS, pid);
        let _g = gl.lock().await;
        reset_wt(state, &project, &wt).await;
    }
    mado::tell(state, pid, "build").await;
    tick(state.clone(), pid.to_string()).await;
    Ok(load_build(&project)?.unwrap_or_default())
}

/// Bring the finished build into the user's branch: fast-forward when possible, else a merge
/// commit. A clash changes nothing.
pub async fn land(state: &Arc<AppState>, pid: &str) -> anyhow::Result<BuildState> {
    let project = project_of(state, pid).await?;
    let l = lock_of(&STATE_LOCKS, pid);
    let _g = l.lock().await;
    let mut b = load_build(&project)?.ok_or_else(|| crate::tr::err("mado_build.none", &[]))?;
    if b.landed_at.is_some() { return Err(crate::tr::err("mado_build.landed", &[])); }
    if b.status != BuildStatus::Done { return Err(crate::tr::err("mado_build.not_done", &[])); }
    if !full(state).await { crate::trust::require_git(&project)?; }
    git_safe(state, &project).await?;
    let here = crate::git::current_branch(state, &project).await.unwrap_or_default();
    if here != b.base { return Err(crate::tr::err("mado_build.wrong_branch", &[("branch", &b.base)])); }
    let dirty = dirty_files(state, &project).await?;
    if !dirty.is_empty() { return Err(crate::tr::err("mado_build.land_dirty", &[("files", &dirty.iter().take(5).cloned().collect::<Vec<_>>().join(", "))])); }
    let gl = lock_of(&GIT_LOCKS, pid);
    let _gg = gl.lock().await;
    // Asked of git without touching the working tree: would the two clash?
    if let Some(clash) = crate::git::merge_clashes(state, &project, "HEAD", &b.branch).await.filter(|c| !c.is_empty()) {
        return Err(crate::tr::err("mado_build.land_conflict", &[("files", &clash.iter().take(5).cloned().collect::<Vec<_>>().join(", "))]));
    }
    let head = crate::git::head(state, &project).await.unwrap_or_default();
    let ff = git_ok(state, &project, &["merge-base", "--is-ancestor", &head, &b.branch]).await;
    let res = if ff { git(state, &project, &["merge", "-q", "--ff-only", &b.branch]).await }
        else { crate::git::git_ident(state, &project, &["merge", "-q", "--no-ff", "--no-edit", "-m", &format!("MADO: build {}", b.number), &b.branch]).await };
    if let Err(e) = git_safe(state, &project).await {
        crate::log::warn(format!("mado build {}: git settings that start programs appeared while it was brought in", b.number));
        return Err(e);
    }
    if let Err(e) = res {
        let clash = crate::git::unmerged(state, &project).await;
        if git_ok(state, &project, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).await { let _ = git(state, &project, &["merge", "--abort"]).await; }
        crate::log::warn(format!("mado build {}: could not be brought in ({e})", b.number));
        if clash.is_empty() { return Err(crate::tr::err_detail("mado_build.land_failed", &[], e)); }
        return Err(crate::tr::err("mado_build.land_conflict", &[("files", &clash.join(", "))]));
    }
    b.landed_at = Some(chrono::Utc::now().to_rfc3339());
    save_build(&project, &b)?;
    // In the user's branch now: the build branch goes (a safe delete, refused were it not merged).
    delete_branch(state, &project, &b.branch, false).await;
    crate::log::info(format!("mado build {}: brought into {} ({})", b.number, b.base, if ff { "fast-forward" } else { "merge commit" }));
    drop(_gg);
    mado::tell(state, pid, "build").await;
    Ok(b)
}

// ---------- the scheduler ----------

fn wave_status(w: &BuildWave) -> TaskStatus {
    let has = |s: TaskStatus| w.lanes.iter().any(|l| l.status == s);
    if w.lanes.iter().all(|l| l.status == TaskStatus::Done) { TaskStatus::Done }
    else if has(TaskStatus::Running) || (has(TaskStatus::Done) && has(TaskStatus::Pending)) { TaskStatus::Running }
    else if has(TaskStatus::Pending) { TaskStatus::Pending }
    else if has(TaskStatus::Conflict) { TaskStatus::Conflict }
    else { TaskStatus::Failed }
}

/// Look at the build and start what may start. Called after every change; cheap and idempotent.
pub fn tick(state: Arc<AppState>, pid: String) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        if let Err(e) = tick_inner(&state, &pid).await { crate::log::warn(format!("mado build: {e:#}")); }
    })
}

async fn tick_inner(state: &Arc<AppState>, pid: &str) -> anyhow::Result<()> {
    let project = engine_project_of(state, pid).await?;
    let mut spawn: Vec<(usize, usize)> = vec![];
    let mut check_wave: Option<usize> = None;
    let (mut recheck, mut finished, mut finishing) = (false, None, false);
    {
        let l = lock_of(&STATE_LOCKS, pid);
        let _g = l.lock().await;
        let Some(mut b) = load_build(&project)? else { return Ok(()) };
        let before = b.clone();
        let live = live_of(pid);
        // The plan as approved: whether waves have a `verify` gate, and what follows the last wave.
        let plan = snapshot(pid, &b.plan_hash).ok();
        let gate = plan.as_ref().map_or(false, finish::has_verify);
        // The screen check or the review was under way and nobody works on it (a restart): it goes on.
        if matches!(b.status, BuildStatus::Checking | BuildStatus::Reviewing) && !live.contains(finish::FINISH) { finishing = true; }
        if b.status == BuildStatus::Running {
            loop {
                let Some(wi) = b.waves.iter().position(|w| w.status != TaskStatus::Done) else {
                    // Every wave is in. What a 0.4.0 plan asks for after it (the screen check, the
                    // review) comes next; a plan without it is done, as before.
                    match plan.as_ref().and_then(|p| finish::next_phase(&b, p)) {
                        Some(phase) => {
                            b.status = phase;
                            b.message = None;
                            if !live.contains(finish::FINISH) { finishing = true; }
                        }
                        None => {
                            b.status = BuildStatus::Done;
                            b.message = None;
                            b.governor.holding = false;
                            b.governor.reason = None;
                            finished = Some(b.clone());
                        }
                    }
                    break;
                };
                let max = b.governor.max_parallel.clamp(1, MAX_PARALLEL) as usize;
                let w = &mut b.waves[wi];
                // A lane marked running with nobody working on it (a restart, a worker gone): it starts again.
                for (li, lane) in w.lanes.iter_mut().enumerate() { if lane.status == TaskStatus::Running && !live.contains(&key(wi, li)) { lane.status = TaskStatus::Pending; } }
                if w.lanes.iter().all(|l| l.status == TaskStatus::Done) {
                    if w.checked && (w.verified || !gate) { w.status = TaskStatus::Done; continue; }
                    // Every lane is in: the wave's checks run once more, together, in the build's
                    // worktree, then the plan's `verify` commands (the gate of a 0.4.0 plan).
                    if !live.contains(&wave_key(wi)) { check_wave = Some(wi); }
                    w.status = TaskStatus::Running;
                    break;
                }
                let pending: Vec<usize> = (0..w.lanes.len()).filter(|li| w.lanes[*li].status == TaskStatus::Pending).collect();
                let active = live.len();
                let mut hold = None;
                if !pending.is_empty() && active < max {
                    hold = governor_reason(state, pid).await;
                    if hold.is_none() {
                        for li in pending.iter().copied().take(max - active) { w.lanes[li].status = TaskStatus::Running; spawn.push((wi, li)); }
                    }
                }
                w.status = wave_status(w);
                recheck = hold.is_some();
                b.governor.holding = hold.is_some();
                b.governor.reason = hold;
                if active == 0 && pending.is_empty() {
                    b.status = BuildStatus::Waiting;
                    if b.message.is_none() { b.message = Some(tr("mado_build.waiting", &[])); }
                    crate::log::warn(format!("mado build {}: waiting for the user", b.number));
                }
                break;
            }
        } else if b.governor.holding && !matches!(b.status, BuildStatus::Checking | BuildStatus::Reviewing) {
            // (While checking or reviewing, the finishing worker holds and releases the governor itself.)
            b.governor.holding = false;
            b.governor.reason = None;
        }
        b.governor.running = (live.len() + spawn.len() + usize::from(check_wave.is_some()) + usize::from(finishing)) as u32;
        for (wi, li) in &spawn { live_add(pid, &key(*wi, *li)); }
        if let Some(wi) = check_wave { live_add(pid, &wave_key(wi)); }
        if finishing { live_add(pid, finish::FINISH); }
        if b != before { save_build(&project, &b)?; }
    }
    if let Some(b) = finished { done_cleanup(state, pid, &project, &b).await; }
    mado::tell(state, pid, "build").await;
    for (wi, li) in spawn { tokio::spawn(lane_worker(state.clone(), pid.to_string(), project.clone(), wi, li)); }
    if let Some(wi) = check_wave { tokio::spawn(wave_worker(state.clone(), pid.to_string(), project.clone(), wi)); }
    if finishing { tokio::spawn(finish::finish_worker(state.clone(), pid.to_string(), project.clone())); }
    if recheck { schedule_recheck(state.clone(), pid.to_string()); }
    Ok(())
}

/// A build just done: its worktrees and lane branches go (its branch stays, for `land`).
async fn done_cleanup(state: &AppState, pid: &str, project: &Path, b: &BuildState) {
    crate::log::info(format!("mado build {}: done", b.number));
    let gl = lock_of(&GIT_LOCKS, pid);
    let _g = gl.lock().await;
    cleanup(state, project, pid, b, false).await;
}

fn schedule_recheck(state: Arc<AppState>, pid: String) {
    {
        let mut r = RECHECKING.lock().unwrap_or_else(|e| e.into_inner());
        if r.contains(&pid) { return; }
        r.push(pid.clone());
    }
    tokio::spawn(async move {
        tokio::time::sleep(RECHECK).await;
        RECHECKING.lock().unwrap_or_else(|e| e.into_inner()).retain(|p| *p != pid);
        tick(state, pid).await;
    });
}

// ---------- the governor ----------

#[cfg(test)]
static TEST_USAGE: std::sync::Mutex<Vec<(String, f64)>> = std::sync::Mutex::new(Vec::new());

/// How much of the plan's current usage window is used, as last reported (`meter.rs`): the
/// highest share among the short windows (five hours or less) that have not reset yet.
fn usage_percent(pid: &str) -> Option<f64> {
    #[cfg(test)]
    if let Some(v) = TEST_USAGE.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(p, _)| p == pid).map(|(_, v)| *v) { return Some(v); }
    #[cfg(not(test))]
    let _ = pid;
    let plans = crate::meter::plans_json(chrono::Utc::now().timestamp());
    [crate::meter::CLAUDE, crate::meter::CODEX].iter().flat_map(|p| plans[*p]["windows"].as_array().cloned().unwrap_or_default())
        .filter(|w| w["hasReset"] != true && (w["id"] == "five_hour" || w["minutes"].as_i64().map_or(false, |m| m <= 300)))
        .filter_map(|w| w["percent"].as_f64()).fold(None, |a: Option<f64>, p| Some(a.map_or(p, |a| a.max(p))))
}

/// Why new lanes are held right now, in words for the user (`None`: they may start).
async fn governor_reason(state: &AppState, pid: &str) -> Option<String> {
    if crate::spend::reached(state).await { return Some(tr("mado_build.hold_spend", &[])); }
    if crate::spend::plan_limit_json(state)["reached"] == true { return Some(tr("mado_build.hold_plan", &[])); }
    let limit = state.settings.read().await.spending.build_hold();
    let pct = usage_percent(pid)?;
    (pct >= f64::from(limit)).then(|| tr("mado_build.hold_usage", &[("percent", &format!("{}", pct.floor() as i64)), ("limit", &limit.to_string())]))
}

// ---------- a lane ----------

enum Next { Stop, Merge, Stuck, Task(usize, BuildTask) }
enum TaskEnd { Done, Failed, Stopped }

async fn lane_worker(state: Arc<AppState>, pid: String, project: PathBuf, wi: usize, li: usize) {
    let r = lane_inner(&state, &pid, &project, wi, li).await;
    if let Err(e) = r {
        crate::log::error(format!("mado build: a lane stopped with an error: {e:#}"));
        let text = clip_note(&crate::tr::classify(&e).message);
        // Git settings that start programs: nothing more is done until the user has removed them.
        let unsafe_git = is_git_programs(&e);
        let _ = update(&pid, &project, |b| {
            if let Some(l) = b.lane_mut(wi, li) { l.status = if unsafe_git { TaskStatus::Pending } else { TaskStatus::Failed }; }
            if b.status == BuildStatus::Running { b.status = if unsafe_git { BuildStatus::Waiting } else { BuildStatus::Failed }; b.message = Some(text); }
        }).await;
    }
    live_remove(&pid, &key(wi, li));
    let stopped = load_build(&project).ok().flatten().map_or(false, |b| b.status == BuildStatus::Stopped);
    if stopped {
        if live_of(&pid).is_empty() { cleanup_stopped(&state, &project, &pid).await; }
        mado::tell(&state, &pid, "build").await;
    } else {
        tick(state, pid).await;
    }
}

/// The plan the build was started with, exactly as approved.
fn snapshot(pid: &str, hash: &str) -> anyhow::Result<PlanDoc> {
    let bytes = std::fs::read(snapshot_file(pid)?)?;
    if mado::plan_hash(&bytes) != hash { anyhow::bail!("the build's copy of the plan is not the approved one"); }
    Ok(serde_json::from_slice(&bytes)?)
}

async fn ensure_lane_wt(state: &AppState, project: &Path, pid: &str, b: &BuildState, wi: usize, li: usize) -> anyhow::Result<PathBuf> {
    let wt = lane_wt(pid, wi, li)?;
    let gl = lock_of(&GIT_LOCKS, pid);
    let _g = gl.lock().await;
    if wt.join(".git").exists() { return Ok(wt); }
    remove_worktree(state, project, pid, &wt).await;
    git_safe(state, project).await?;
    let br = lane_branch(b.number, wi, li);
    if git_ok(state, project, &["rev-parse", "-q", "--verify", &format!("refs/heads/{br}")]).await {
        git(state, project, &["worktree", "add", "-q", &s(&wt), &br]).await?;
    } else {
        git(state, project, &["worktree", "add", "-q", "-b", &br, &s(&wt), &b.branch]).await?;
    }
    git_safe(state, &wt).await?;
    copy_docs(project, &wt, full(state).await)?;
    Ok(wt)
}

async fn lane_inner(state: &Arc<AppState>, pid: &str, project: &Path, wi: usize, li: usize) -> anyhow::Result<()> {
    let b = load_build(project)?.ok_or_else(|| crate::tr::err("mado_build.none", &[]))?;
    let plan = snapshot(pid, &b.plan_hash)?;
    let wt = ensure_lane_wt(state, project, pid, &b, wi, li).await?;
    if let Err(why) = ensure_setup(state, pid, project, &b.plan_hash, &plan, &wt).await {
        hold_waiting(state, pid, project, Some((wi, li)), &why).await?;
        return Ok(());
    }
    loop {
        let next = update(pid, project, |b| {
            let running = b.status == BuildStatus::Running;
            let Some(l) = b.lane_mut(wi, li) else { return Next::Stop };
            if !running { if l.status == TaskStatus::Running { l.status = TaskStatus::Pending; } return Next::Stop; }
            match l.tasks.iter().position(|t| t.status != TaskStatus::Done) {
                None => Next::Merge,
                Some(ti) if matches!(l.tasks[ti].status, TaskStatus::Failed | TaskStatus::Conflict) => { l.status = l.tasks[ti].status; Next::Stuck }
                Some(ti) => Next::Task(ti, l.tasks[ti].clone()),
            }
        }).await?.unwrap_or(Next::Stop);
        match next {
            Next::Stop | Next::Stuck => return Ok(()),
            Next::Merge => break,
            Next::Task(ti, t) => {
                let task = plan.tasks.iter().find(|p| p.id == t.id).cloned().ok_or_else(|| anyhow::anyhow!("task {} is not in the plan", t.id))?;
                match run_task(state, pid, project, &b, &plan, &wt, (wi, li, ti), &task).await? {
                    TaskEnd::Done => continue,
                    TaskEnd::Failed => {
                        update(pid, project, |b| if let Some(l) = b.lane_mut(wi, li) { l.status = TaskStatus::Failed; }).await?;
                        return Ok(());
                    }
                    TaskEnd::Stopped => {
                        update(pid, project, |b| if let Some(t) = b.task_mut(wi, li, ti) {
                            if matches!(t.status, TaskStatus::Running | TaskStatus::Checking) { t.status = TaskStatus::Pending; t.attempts = t.attempts.saturating_sub(1); }
                        }).await?;
                        return Ok(());
                    }
                }
            }
        }
    }
    let end = merge_lane(state, pid, project, &b, &plan, wi, li).await?;
    update(pid, project, |b| {
        let stopped = b.status == BuildStatus::Stopped;
        if let Some(l) = b.lane_mut(wi, li) {
            l.status = match end { LaneEnd::Merged => TaskStatus::Done, LaneEnd::Conflict => TaskStatus::Conflict, LaneEnd::Stopped => if stopped { l.status } else { TaskStatus::Pending } };
        }
    }).await?;
    Ok(())
}

/// The build's status right now.
fn status_now(project: &Path) -> BuildStatus { load_build(project).ok().flatten().map_or(BuildStatus::Stopped, |b| b.status) }

#[allow(clippy::too_many_arguments)]
async fn run_task(state: &Arc<AppState>, pid: &str, project: &Path, b: &BuildState, plan: &PlanDoc, wt: &Path, at: (usize, usize, usize), task: &PlanTask) -> anyhow::Result<TaskEnd> {
    let (wi, li, ti) = at;
    loop {
        // What went wrong the last time (a try of this run, or the one before the user's retry).
        let (attempts, failure) = update(pid, project, |b| b.task_mut(wi, li, ti).map(|t| (t.attempts, t.note.clone()))).await?.flatten().unwrap_or((MAX_ATTEMPTS, None));
        if status_now(project) == BuildStatus::Stopped { return Ok(TaskEnd::Stopped); }
        if attempts >= MAX_ATTEMPTS {
            update(pid, project, |b| if let Some(t) = b.task_mut(wi, li, ti) { t.status = TaskStatus::Failed; }).await?;
            crate::log::warn(format!("mado build {}: task {} failed {} times", b.number, task.id, MAX_ATTEMPTS));
            mado::tell(state, pid, "build").await;
            return Ok(TaskEnd::Failed);
        }
        // A try after a failed one in this lane goes on in the same chat (its session knows the work);
        // the first try, and one after the user's retry, start a chat of their own.
        let earlier = if attempts > 0 { update(pid, project, |b| b.task_mut(wi, li, ti).and_then(|t| t.chat_id.clone())).await?.flatten() } else { None };
        let session = match &earlier { Some(c) => state.chats.with(c, |c| c.session_id.clone()).await.flatten(), None => None };
        let (chat, brief, resume) = match (earlier, session, failure.as_deref().filter(|f| !f.trim().is_empty())) {
            (Some(c), Some(sid), Some(f)) => (c, retry_brief(task, f), Some(sid)),
            _ => (new_chat(state, pid, &task.id, &format!("{} — {}", tr("mado_build.chat_title", &[]), one_line(&task.title, 80))).await,
                task_brief(task, plan, failure.as_deref(), finish::conventions(project, full(state).await).as_deref()), None),
        };
        update(pid, project, |b| if let Some(t) = b.task_mut(wi, li, ti) { t.attempts += 1; t.status = TaskStatus::Running; t.chat_id = Some(chat.clone()); t.commit = None; }).await?;
        mado::tell(state, pid, "build").await;
        let ended = run_and_wait(state, project, pid, &chat, wt, &brief, resume, deny_for(project, pid, b, wt)).await;
        if status_now(project) == BuildStatus::Stopped { return Ok(TaskEnd::Stopped); }
        if let Err(why) = ended { note_fail(pid, project, at, &why).await?; continue; }
        // The run must have left its worktree on its lane's branch.
        let head = git(state, wt, &["symbolic-ref", "-q", "HEAD"]).await.unwrap_or_default();
        if head.trim() != format!("refs/heads/{}", lane_branch(b.number, wi, li)) { note_fail(pid, project, at, &tr("mado_build.left_branch", &[])).await?; reset_lane_head(state, wt, &lane_branch(b.number, wi, li)).await; continue; }
        update(pid, project, |b| if let Some(t) = b.task_mut(wi, li, ti) { t.status = TaskStatus::Checking; }).await?;
        mado::tell(state, pid, "build").await;
        match run_checks(state, pid, &b.plan_hash, &task.acceptance, wt, project).await {
            Ok(()) => {}
            Err(CheckErr::Failed(why)) => { note_fail(pid, project, at, &why).await?; continue; }
            // Not an approved command: no further try, the build waits for the user.
            Err(CheckErr::Unapproved(why)) => {
                update(pid, project, |b| if let Some(t) = b.task_mut(wi, li, ti) { t.note = Some(why.clone()); t.status = TaskStatus::Failed; t.attempts = MAX_ATTEMPTS; }).await?;
                mado::tell(state, pid, "build").await;
                return Ok(TaskEnd::Failed);
            }
        }
        if status_now(project) == BuildStatus::Stopped { return Ok(TaskEnd::Stopped); }
        // Files of a `contracts` or `tests` task that is done are locked: a later task may not change
        // them. Its changes to them are undone, and it goes back to the assistant like a failed check.
        let locked = load_build(project)?.map(|b| finish::locked_by(plan, &b, &[&task.id])).unwrap_or_default();
        let named: Vec<String> = task.owns.iter().filter(|o| !o.contains(['*', '?', '['])).map(|o| o.trim().trim_start_matches("./").to_string()).collect();
        let touched = finish::undo_locked(state, wt, &locked, &named).await?;
        if !touched.is_empty() {
            note_fail(pid, project, at, &finish::locked_note(&touched)).await?;
            mado::tell(state, pid, "build").await;
            continue;
        }
        let (sha, changed) = commit_task(state, wt, b.number, task).await?;
        let outside: Vec<String> = changed.iter().filter(|f| !task.owns.iter().any(|p| glob_match(p, f))).take(10).cloned().collect();
        let note = if changed.is_empty() { Some(tr("mado_build.no_change", &[])) }
            else if !outside.is_empty() { Some(tr("mado_build.outside", &[("files", &outside.join(", "))])) } else { None };
        update(pid, project, |b| if let Some(t) = b.task_mut(wi, li, ti) { t.status = TaskStatus::Done; t.commit = Some(sha.clone()); t.note = note; }).await?;
        crate::log::info(format!("mado build {}: task {} done ({})", b.number, task.id, &sha[..sha.len().min(10)]));
        mado::tell(state, pid, "build").await;
        return Ok(TaskEnd::Done);
    }
}

/// What went wrong with a try of a task, kept for the user and for the next try.
async fn note_fail(pid: &str, project: &Path, at: (usize, usize, usize), why: &str) -> anyhow::Result<()> {
    update(pid, project, |b| if let Some(t) = b.task_mut(at.0, at.1, at.2) { t.note = Some(clip_note(why)); t.status = TaskStatus::Running; }).await?;
    Ok(())
}

/// A run that switched its worktree to another branch: back onto the lane's (its changes stay).
async fn reset_lane_head(state: &AppState, wt: &Path, branch: &str) {
    if let Err(e) = git_safe(state, wt).await { crate::log::warn(format!("mado build: a worktree was not put back on its branch: {e}")); return; }
    let _ = git(state, wt, &["checkout", "-q", "-f", branch]).await;
}

fn one_line(s: &str, n: usize) -> String { s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect() }

/// A chat of kind `mado-build` for task `task_id`: its `meta.mado` tells `mado::prepare_run` whose
/// approved acceptance commands the run may use.
async fn new_chat(state: &AppState, pid: &str, task_id: &str, title: &str) -> String {
    new_chat_with(state, pid, json!({"mado": {"projectId": pid, "taskId": task_id}}), title, false).await
}

/// A chat of kind `mado-build` with `meta`; `read_only`: a run that may only read (the review).
/// Its model and effort are the project's (`mado::run_options`), recorded with the chat.
async fn new_chat_with(state: &AppState, pid: &str, meta: Value, title: &str, read_only: bool) -> String {
    let (model, effort) = options_for(state, pid).await;
    let c = Chat { kind: "mado-build".into(), title: title.to_string(), project_id: Some(pid.to_string()),
        options: RunOptions { permission_mode: Some(mode_of(read_only).into()), model, effort, ..Default::default() },
        meta, ..Default::default() };
    let id = c.id.clone();
    state.chats.insert(c).await;
    id
}

/// What a run of the build may not change, on top of the app's own rules: the user's project
/// folder, the build's other worktrees, the copied plan documents, git's own files, and the git
/// commands that change branches or history. (Like every rule, they do not travel under full access.)
fn deny_for(project: &Path, pid: &str, b: &BuildState, wt: &Path) -> Vec<String> {
    let mut d = vec![];
    let mut closed: Vec<PathBuf> = vec![project.to_path_buf()];
    if let Ok(bw) = build_wt(pid) { closed.push(bw); }
    for (wi, w) in b.waves.iter().enumerate() { for li in 0..w.lanes.len() { if let Ok(p) = lane_wt(pid, wi, li) { closed.push(p); } } }
    for p in closed.into_iter().filter(|p| p != wt) {
        let r = claude::rule_path(&p);
        d.push(format!("Edit({r})"));
        d.push(format!("Edit({r}/**)"));
    }
    let own = claude::rule_path(wt);
    for r in [format!("Edit({own}/mado)"), format!("Edit({own}/mado/**)"), format!("Edit({own}/.git)")] { d.push(r); }
    for g in mado::BUILD_GIT_DENY {
        d.push(format!("Bash(git {g}:*)"));
    }
    d
}

/// The permission mode of a build's run: edits are accepted (they are confined by the deny rules
/// and the worktree), except for a run that only reads (the review), which plans: Claude Code's
/// read-only mode, kept even under full access (`full_access::mode_for`).
fn mode_of(read_only: bool) -> &'static str { if read_only { "plan" } else { "acceptEdits" } }

#[cfg(test)]
static TEST_OPTIONS: std::sync::Mutex<Vec<(String, (Option<String>, Option<String>))>> = std::sync::Mutex::new(Vec::new());

/// The model and effort of every run of this project's build (`mado::options_for_run`: the project's
/// choice, else the app's default; never a Claude Code choice for another program).
async fn options_for(state: &AppState, pid: &str) -> (Option<String>, Option<String>) {
    #[cfg(test)]
    {
        let under_other = { let s = state.settings.read().await; s.full_access() && crate::assistant::primary(&s) != crate::assistant::Program::Claude };
        if !under_other {
            if let Some(o) = TEST_OPTIONS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(p, _)| p == pid).map(|(_, o)| o.clone()) { return o; }
        }
    }
    mado::options_for_run(state, pid).await
}

const SYSTEM: &str = "This run is one task of a build the Thoughts app runs by itself from a plan the user approved. Nobody is watching it: \
do not ask questions, do the work the task describes, and end with a short plain-language summary of what you did. \
Work only in the current folder, by reading and editing files: you have no shell and run no commands. The app runs the checks and commits your work itself.";

fn task_brief(task: &PlanTask, plan: &PlanDoc, failure: Option<&str>, conventions: Option<&str>) -> String {
    // The task's words come from plan.json: quoted as data, apart from the app's own instructions.
    let mut out = format!("Task {}: {}\n\nWhat the task is (from the plan):\n{}\n", task.id,
        crate::mado_prompts::inline(&one_line(&task.title, 200)), crate::mado_prompts::block(&task.description.chars().take(8000).collect::<String>()));
    let list = |v: &[String]| v.iter().map(|x| format!("- {}", crate::mado_prompts::inline(&one_line(x, 400)))).collect::<Vec<_>>().join("\n");
    out.push_str(&format!("\nFiles you own:\n{}\n", if task.owns.is_empty() { "- (none named)".into() } else { list(&task.owns) }));
    if !task.acceptance.is_empty() { out.push_str(&format!("\nWhen you are done, these commands must pass when run from this folder:\n{}\n", list(&task.acceptance))); }
    let deps: Vec<String> = task.depends_on.iter().filter_map(|d| plan.tasks.iter().find(|t| &t.id == d)).map(|t| format!("{} ({})", t.id, crate::mado_prompts::inline(&one_line(&t.title, 100)))).collect();
    if !deps.is_empty() { out.push_str(&format!("\nIt builds on work already done: {}.\n", deps.join(", "))); }
    out.push_str("\nRead first, for context (copies of the plan's documents; do not change them): mado/brief.md, mado/spec.md, mado/tech.md, mado/design.md, mado/plan.json, and mado/design/ for the look.\n");
    if let Some(c) = conventions { out.push_str(&prompts::conventions(c)); }
    if task.locks() { out.push_str(prompts::LOCKING_TASK); }
    out.push_str("\nStay inside the files you own; if you must touch another file, say so in your final message.\n");
    out.push_str("You cannot run commands in this task. When you finish, the app runs the commands above itself, in this folder; \
        if one fails it comes back to you in this chat with the command and what it printed.\n");
    out.push_str("Commit nothing: the app commits your work after the commands pass.\n");
    if let Some(f) = failure.filter(|f| !f.trim().is_empty()) {
        out.push_str(&format!("\nThe previous try of this task did not succeed. What went wrong (what the app and the commands printed):\n{}\nFix that this time.\n", crate::mado_prompts::block(&clip_note(f))));
    }
    out.push_str(&format!("\n{}\n", crate::mado_prompts::DATA_NOTE));
    out
}

/// What a try after a failed one is told, in the same chat.
fn retry_brief(task: &PlanTask, failure: &str) -> String {
    format!("Task {}: the app ran the checks after your work, and this did not succeed (what the app and the command printed):\n{}\n\
        Fix it in the files. You still cannot run commands: the app runs the checks again when you finish.\n\n{}\n",
        task.id, crate::mado_prompts::block(&clip_note(failure)), crate::mado_prompts::DATA_NOTE)
}

/// Start a run in `cwd` and wait for it to end. `Err` carries what went wrong, for the user and the next try.
#[allow(clippy::too_many_arguments)]
async fn run_and_wait(state: &Arc<AppState>, project: &Path, pid: &str, chat: &str, cwd: &Path, brief: &str, resume: Option<String>, extra_deny: Vec<String>) -> Result<(), String> {
    run_and_wait_as(state, project, pid, chat, cwd, brief, resume, extra_deny, false, SYSTEM).await
}

/// `run_and_wait` with the run's mode (`read_only`: `mode_of`) and its instructions.
#[allow(clippy::too_many_arguments)]
async fn run_and_wait_as(state: &Arc<AppState>, project: &Path, pid: &str, chat: &str, cwd: &Path, brief: &str, resume: Option<String>, extra_deny: Vec<String>, read_only: bool, system: &str) -> Result<(), String> {
    if !full(state).await {
        // The worktree is a checkout of the trusted project: its assistant settings must be ones the user already saw.
        if let Err(e) = crate::trust::require(project) { return Err(crate::tr::classify(&e).message); }
        let differing = crate::trust::differing_assistant_files(project, cwd);
        if !differing.is_empty() { return Err(tr("mado_build.untrusted_copy", &[("files", &differing.join(", "))])); }
        if let Err(e) = crate::trust::trust(cwd) { return Err(crate::tr::classify(&e).message); }
    }
    state.chats.update(chat, |c| c.messages.push(Msg::new("user", brief))).await;
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    let (model, effort) = options_for(state, pid).await;
    let spec = RunSpec {
        chat_id: chat.to_string(), prompt: brief.to_string(), cwd: cwd.to_path_buf(), add_dirs: vec![],
        options: RunOptions { permission_mode: Some(mode_of(read_only).into()), model, effort, ..Default::default() }, resume,
        append_system: Some(system.into()), extra_deny,
    };
    let done: claude::OnDone = Box::new(move |_st, ok| Box::pin(async move { let _ = tx.send(ok); }));
    if let Err(e) = claude::start_run(state.clone(), spec, Some(done)).await {
        let text = crate::tr::classify(&e).message;
        claude::push(state, chat, Msg::new("error", text.clone())).await;
        return Err(tr("mado_build.run_failed", &[("why", &text)]));
    }
    let ok = match tokio::time::timeout(RUN_LIMIT, rx).await {
        Ok(r) => r.unwrap_or(false),
        Err(_) => {
            if let Some(k) = state.runs.lock().await.remove(chat) { let _ = k.send(()); }
            return Err(tr("mado_build.too_long", &[("minutes", &(RUN_LIMIT.as_secs() / 60).to_string())]));
        }
    };
    if ok { return Ok(()); }
    let why = state.chats.with(chat, |c| c.messages.iter().rev().take(4).find(|m| m.role == "error").map(|m| m.text.clone())).await.flatten().unwrap_or_default();
    Err(tr("mado_build.run_failed", &[("why", &one_line(&why, 400))]))
}

// ---------- acceptance commands ----------

/// Why a check did not pass: it failed (worth another try), or it was refused because it is not
/// a command the user approved for this build's plan, or it could not be run confined (no try can
/// change that: the build waits).
pub enum CheckErr { Failed(String), Unapproved(String) }

impl CheckErr { fn text(self) -> String { match self { CheckErr::Failed(t) | CheckErr::Unapproved(t) => t } } }

/// Is `cmd` one the user approved for the plan this build runs (hash `hash`), and is the project's
/// plan still that one? Asked again right before every command (correctness: it holds under full access too).
async fn approved_now(state: &AppState, pid: &str, project: &Path, hash: &str, cmd: &str) -> bool {
    let Some(a) = mado::approved_plan(state, pid).await else { return false };
    let Ok(Some(bytes)) = read_mado(project, "plan.json") else { return false };
    a.plan_hash == hash && mado::approval_matches(&a, &bytes) && a.commands.iter().any(|c| c == cmd)
}

/// Each command, in order, from `cwd`; the first that fails ends it, with what it printed.
async fn run_checks(state: &AppState, pid: &str, hash: &str, cmds: &[String], cwd: &Path, project: &Path) -> Result<(), CheckErr> {
    let full = full(state).await;
    let mut how = confinement(state).await;
    how.cache = cache_dir(pid).unwrap_or_default();
    for cmd in cmds.iter().filter(|c| !c.trim().is_empty()) {
        if status_now(project) == BuildStatus::Stopped { return Err(CheckErr::Failed(tr("mado_build.stopped_note", &[]))); }
        if !approved_now(state, pid, project, hash, cmd).await {
            crate::log::warn("mado build: a check was refused, it is not an approved command of this plan");
            return Err(CheckErr::Unapproved(tr("mado_build.check_unapproved", &[("cmd", &one_line(cmd, 300))])));
        }
        run_check(cmd, cwd, project, full, &how).await?;
    }
    Ok(())
}

/// The user's shell, as a terminal would start it for one command: `(program, arguments)`.
fn shell_argv(cmd: &str) -> (String, Vec<String>) {
    #[cfg(windows)]
    { ("cmd".into(), vec!["/C".into(), cmd.into()]) }
    #[cfg(not(windows))]
    {
        let sh = std::env::var("SHELL").ok().filter(|s| Path::new(s).is_absolute() && Path::new(s).exists()).unwrap_or_else(|| "/bin/sh".into());
        (sh, vec!["-c".into(), cmd.into()])
    }
}

// ---------- what a command the daemon runs may see and change ----------

/// The variables a command the daemon runs for a build keeps from the daemon's environment (and `LC_*`).
/// Everything else (keys, tokens, the app's and the assistant's own settings) is left out.
const ENV_KEEP: &[&str] = &["PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "TMPDIR", "TERM",
    "CARGO_HOME", "RUSTUP_HOME", "GOPATH", "GOROOT", "JAVA_HOME", "NVM_DIR", "PNPM_HOME", "VOLTA_HOME", "PYENV_ROOT"];

/// May a variable of this name reach a command a build runs?
pub fn env_kept(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    let secret = ["TOKEN", "KEY", "SECRET", "PASSWORD"].iter().any(|w| up.contains(w));
    let own = ["ANTHROPIC_", "CLAUDE", "THOUGHTS_"].iter().any(|p| up.starts_with(p));
    !secret && !own && (ENV_KEEP.contains(&name) || name.starts_with("LC_"))
}

/// The environment of a command the daemon runs for a build: the kept part of its own, the PATH a
/// terminal would have, `CI=1`, and every package tool's cache pointed at the build's own `cache`
/// folder (never the user's caches, which hold code that runs later).
pub fn command_env(cache: &Path) -> Vec<(String, String)> {
    let at = |sub: &str| cache.join(sub).to_string_lossy().to_string();
    let own: Vec<(&str, String)> = vec![
        ("npm_config_cache", at("npm")), ("npm_config_update_notifier", "false".into()), ("YARN_CACHE_FOLDER", at("yarn")),
        ("PNPM_STORE_DIR", at("pnpm")), ("npm_config_store_dir", at("pnpm")), ("BUN_INSTALL_CACHE_DIR", at("bun")),
        ("CARGO_HOME", at("cargo")), ("GOPATH", at("go")), ("GOMODCACHE", at("go/pkg/mod")), ("GOCACHE", at("go/build")),
        ("GRADLE_USER_HOME", at("gradle")), ("MAVEN_OPTS", format!("-Dmaven.repo.local={}", at("m2"))), ("PIP_CACHE_DIR", at("pip")),
        ("COMPOSER_HOME", at("composer")), ("COMPOSER_CACHE_DIR", at("composer/cache")), ("NUGET_PACKAGES", at("nuget")), ("XDG_CACHE_HOME", at("xdg")),
        // git reads none of the user's own settings (they are closed to the commands, and could name programs).
        ("GIT_CONFIG_GLOBAL", "/dev/null".into()), ("GIT_CONFIG_NOSYSTEM", "1".into()),
    ];
    let mut env: Vec<(String, String)> = std::env::vars().filter(|(k, _)| env_kept(k) && k != "PATH" && !own.iter().any(|(o, _)| o == k)).collect();
    env.push(("PATH".into(), claude::shell_path().to_string()));
    env.push(("CI".into(), "1".into()));
    env.extend(own.into_iter().map(|(k, v)| (k.to_string(), v)));
    env
}

/// The build's own download caches (`<thoughts home>/mado/<projectId>/cache`), removed with its worktrees.
fn cache_dir(pid: &str) -> anyhow::Result<PathBuf> { Ok(engine_dir(pid)?.join("cache")) }

/// How a build's own commands are kept in: `confined` is whether they run under macOS's sandbox.
#[derive(Clone, Debug, PartialEq)]
pub struct Confinement { pub confined: bool, /// The daemon's own port, closed to the commands (0: not known).
    pub port: u16, /// The build's own caches (`cache_dir`).
    pub cache: PathBuf }

/// Whether the build's commands run confined now: on macOS, while the setting is on and full access is off.
pub async fn confinement(state: &AppState) -> Confinement {
    let s = state.settings.read().await;
    Confinement { confined: cfg!(target_os = "macos") && !s.full_access() && s.spending.confine_commands(), port: crate::approvals::port(), cache: PathBuf::new() }
}

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// `p` as the sandbox sees it: links resolved (`/tmp` is `/private/tmp`); the nearest folder that
/// exists, resolved, with the rest put back when it does not exist yet.
fn real(p: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(p) { return c; }
    match (p.parent(), p.file_name()) { (Some(par), Some(n)) if par != p => real(par).join(n), _ => p.to_path_buf() }
}

/// `p` with its folders resolved and its last part as it is (never followed: it may be a link someone planted).
fn in_place(p: &Path) -> PathBuf {
    match (p.parent(), p.file_name()) { (Some(par), Some(n)) => real(par).join(n), _ => p.to_path_buf() }
}

/// `p` as a regular expression of the sandbox's (`#"…"`), every special character escaped.
fn sb_regex(p: &Path) -> String {
    p.to_string_lossy().chars().map(|c| if ".[]{}()*+?^$|\\".contains(c) { format!("\\{c}") } else if c == '"' { "\\x22".into() } else { c.to_string() }).collect()
}

/// Are `wt` and `cache` real folders, not links (checked before every command a build runs confined)?
fn real_dirs(wt: &Path, cache: &Path) -> bool {
    [wt, cache].iter().all(|p| std::fs::symlink_metadata(p).map_or(false, |m| m.file_type().is_dir()))
}

fn sb_str(p: &Path) -> String {
    let t = p.to_string_lossy();
    format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Inside the user's home, what stays closed to reading even where a toolchain on the PATH is let back in.
const SECRETS_IN_HOME: &[&str] = &[".claude", ".claude.json", ".ssh", ".aws", ".azure", ".config/gcloud", ".kube", ".netrc", ".config/gh", ".docker", ".npmrc",
    ".yarnrc", ".yarnrc.yml", ".pypirc", ".gem/credentials", ".cargo/credentials", ".cargo/credentials.toml", ".gnupg", ".git-credentials", ".gitconfig", ".config/git",
    "Library/Keychains", "Library/Cookies", "Library/Mail", "Library/Messages", "Documents", "Desktop", "Downloads", "Pictures", "Library/Mobile Documents",
    ".composer/auth.json", ".config/composer/auth.json", ".m2/settings.xml", ".gradle/gradle.properties"];

/// The folders of the PATH inside the user's home, with the installation each belongs to (`…/v22/bin`
/// → `…/v22`, never a folder right inside the home such as `~/.cargo`), as the sandbox sees them.
fn toolchains(home: &Path) -> Vec<PathBuf> {
    let home = real(home);
    let mut out = vec![];
    for d in claude::shell_path().split(':').filter(|d| Path::new(d).is_absolute()) {
        let d = real(Path::new(d));
        if !d.starts_with(&home) || d == home { continue; }
        let parent = d.parent().map(Path::to_path_buf);
        let root = match (d.file_name().map(|n| n == "bin").unwrap_or(false), parent) {
            (true, Some(p)) if p.parent().map_or(false, |pp| pp != home) && p != home => p,
            _ => d,
        };
        if !out.contains(&root) { out.push(root); }
    }
    out
}

/// The sandbox profile for a build's command run in `wt` (a worktree of the engine, under
/// `<thoughts home>/mado/<projectId>/`), with the build's own caches in `cache`. Everything is
/// allowed but:
/// - writing files: only in `wt`, `cache`, the temporary folders and the null devices; never in the
///   app's own folder, the user's project (its `.git` with it), the user's home or the PATH;
/// - reading the user's home and the app's own folder: only `wt`, `cache`, the project's `.git` (the
///   worktree's history lives there), `RUSTUP_HOME`'s toolchains and the toolchains on the PATH, and
///   never the places that hold credentials or the user's files (`SECRETS_IN_HOME`);
/// - connecting to the daemon's own port (`port`, when known), and reaching the keychain.
/// The last rule that matches wins, so the order matters.
pub fn sandbox_profile(wt: &Path, project: &Path, cache: &Path, user_home: Option<&Path>, port: u16) -> String {
    let q = |p: &Path| sb_str(&real(p));
    let home = crate::paths::home();
    let mut out = String::from("(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write*");
    let mut tmp: Vec<PathBuf> = vec![PathBuf::from("/private/tmp"), PathBuf::from("/private/var/folders"), std::env::temp_dir()];
    if let Ok(t) = std::env::var("TMPDIR") { if Path::new(&t).is_absolute() { tmp.push(PathBuf::from(t)); } }
    for p in &tmp { out.push_str(&format!(" (subpath {})", q(p))); }
    out.push_str(" (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/tty\") (literal \"/dev/dtracehelper\") (regex #\"^/dev/fd/\") (regex #\"^/dev/ttys[0-9]+$\"))\n");
    let mut closed = vec![home.clone(), project.to_path_buf(), project.join(".git")];
    closed.extend(claude::shell_path().split(':').filter(|d| Path::new(d).is_absolute()).map(PathBuf::from));
    if let Some(h) = user_home { closed.push(h.to_path_buf()); }
    out.push_str("(deny file-write*");
    for p in &closed { out.push_str(&format!(" (subpath {})", q(p))); }
    out.push_str(")\n");
    // Reading: the user's home and the app's folder are closed, then the few things a build needs are let back in.
    out.push_str("(deny file-read*");
    for p in [Some(home.as_path()), user_home].into_iter().flatten() { out.push_str(&format!(" (subpath {})", q(p))); }
    out.push_str(")\n");
    let mut back: Vec<PathBuf> = vec![project.join(".git")];
    if let Some(h) = user_home {
        back.extend(toolchains(h));
        back.push(std::env::var("RUSTUP_HOME").ok().filter(|r| Path::new(r).is_absolute()).map(PathBuf::from).unwrap_or_else(|| h.join(".rustup")));
    }
    // (Whether a file is there, and its size, may be asked anywhere in them: no names are listed and nothing is read.)
    out.push_str("(allow file-read-metadata");
    for p in [Some(home.as_path()), user_home].into_iter().flatten() { out.push_str(&format!(" (subpath {})", q(p))); }
    out.push_str(")\n(allow file-read*");
    for p in &back { out.push_str(&format!(" (subpath {})", q(p))); }
    out.push_str(")\n");
    // The worktree's and the cache's contents (never the folders themselves, nor the folder they are in:
    // they cannot be removed, renamed or swapped for a link). Their places come from the engine's own
    // folder, resolved; their last part is not followed.
    let (wt_at, cache_at) = (in_place(wt), in_place(cache));
    out.push_str(&format!("(allow file-read* file-write* (regex #\"^{}/\") (regex #\"^{}/\"))\n", sb_regex(&wt_at), sb_regex(&cache_at)));
    out.push_str(&format!("(allow file-read* (literal {}) (literal {}))\n", sb_str(&wt_at), sb_str(&cache_at)));
    let mut fixed = vec![wt_at.clone(), cache_at.clone(), wt_at.join(".git")];
    if let Some(p) = wt_at.parent() { fixed.push(p.to_path_buf()); }
    if let Some(p) = cache_at.parent() { if !fixed.iter().any(|f| f == p) { fixed.push(p.to_path_buf()); } }
    out.push_str("(deny file-write*");
    for p in &fixed { out.push_str(&format!(" (literal {})", sb_str(p))); }
    out.push_str(")\n");
    // Last, so they win whatever the paths above resolve to: credentials and the user's own files.
    if let Some(h) = user_home {
        let mut secrets: Vec<PathBuf> = SECRETS_IN_HOME.iter().map(|c| h.join(c)).collect();
        if let Ok(c) = std::env::var("CLAUDE_CONFIG_DIR") { if Path::new(&c).is_absolute() { secrets.push(PathBuf::from(c)); } }
        out.push_str("(deny file-read* file-write*");
        for p in &secrets { out.push_str(&format!(" (subpath {})", q(p))); }
        out.push_str(")\n");
    }
    // The app's records (the approval, the plan the build was started with) beside the worktrees.
    out.push_str(&format!("(deny file-read* file-write* (regex #\"^{}/[^/]+\\.(json|sha256|setup|pid)$\"))\n", sb_regex(&real(&crate::paths::home()))));
    if let Some(p) = wt_at.parent() { out.push_str(&format!("(deny file-read* file-write* (regex #\"^{}/[^/]+\\.(json|sha256|setup|pid)$\"))\n", sb_regex(p))); }
    if port != 0 { out.push_str(&format!("(deny network-outbound (remote ip \"localhost:{port}\"))\n")); }
    // The keychain (the assistant's key is kept there). TLS checks do not need it (tried: git and curl over https).
    out.push_str("(deny mach-lookup (global-name \"com.apple.SecurityServer\") (global-name \"com.apple.securityd.xpc\"))\n");
    out
}

/// `sandbox_profile` for the code graph tool (`code_graph`), which the daemon runs on the project
/// itself rather than in a worktree: it may also read `read` (the project and the tool's own
/// installation, both often inside the user's home), writes nowhere but inside `wt` (not even the
/// temporary folders), and reaches no network address at all. The credentials of the home stay
/// closed, except one that holds a folder of `read` (a project in `~/Documents` is read, nothing else there).
pub fn graph_profile(wt: &Path, project: &Path, cache: &Path, user_home: Option<&Path>, port: u16, read: &[PathBuf]) -> String {
    let q = |p: &Path| sb_str(&real(p));
    let mut out = sandbox_profile(wt, project, cache, user_home, port);
    let mut tmp: Vec<PathBuf> = vec![PathBuf::from("/private/tmp"), PathBuf::from("/private/var/folders"), std::env::temp_dir()];
    if let Ok(t) = std::env::var("TMPDIR") { if Path::new(&t).is_absolute() { tmp.push(PathBuf::from(t)); } }
    out.push_str("(deny file-write*");
    for p in &tmp { out.push_str(&format!(" (subpath {})", q(p))); }
    out.push_str(")\n");
    let read: Vec<PathBuf> = read.iter().map(|p| real(p)).collect();
    if !read.is_empty() {
        out.push_str("(allow file-read*");
        for p in &read { out.push_str(&format!(" (subpath {})", sb_str(p))); }
        out.push_str(")\n");
    }
    out.push_str(&format!("(allow file-read* file-write* (regex #\"^{}/\"))\n", sb_regex(&in_place(wt))));
    if let Some(h) = user_home {
        let mut secrets: Vec<PathBuf> = SECRETS_IN_HOME.iter().map(|c| real(&h.join(c))).collect();
        if let Ok(c) = std::env::var("CLAUDE_CONFIG_DIR") { if Path::new(&c).is_absolute() { secrets.push(real(Path::new(&c))); } }
        secrets.retain(|s| !read.iter().any(|r| r.starts_with(s)));
        if !secrets.is_empty() {
            out.push_str("(deny file-read* file-write*");
            for p in &secrets { out.push_str(&format!(" (subpath {})", sb_str(p))); }
            out.push_str(")\n");
        }
    }
    out.push_str("(deny network-outbound (remote ip \"*:*\"))\n(deny network-bind (local ip \"*:*\"))\n");
    out
}

/// Read all of `r`, keeping its start and its end.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(r: Option<R>) -> (Vec<u8>, Vec<u8>, bool) {
    use tokio::io::AsyncReadExt;
    let (mut head, mut tail, mut cut) = (vec![], std::collections::VecDeque::new(), false);
    let Some(mut r) = r else { return (head, vec![], false) };
    let mut buf = [0u8; 8192];
    loop {
        let n = match r.read(&mut buf).await { Ok(0) | Err(_) => break, Ok(n) => n };
        for &byte in &buf[..n] {
            if head.len() < OUTPUT_HEAD { head.push(byte); continue; }
            tail.push_back(byte);
            if tail.len() > OUTPUT_TAIL { tail.pop_front(); cut = true; }
        }
    }
    (head, tail.into_iter().collect(), cut)
}

fn shown_output(parts: (Vec<u8>, Vec<u8>, bool)) -> String {
    let (head, tail, cut) = parts;
    let mut s = String::from_utf8_lossy(&head).to_string();
    if cut { s.push_str("\n…\n"); }
    s.push_str(&String::from_utf8_lossy(&tail));
    s
}

/// One acceptance command, run by the daemon with the user's shell, time-limited, its output kept
/// within bounds, with a cleared environment (`command_env`) and, when `how.confined`, under macOS's
/// sandbox (`sandbox_profile`). `Err` says what failed, with the end of what it printed;
/// `CheckErr::Unapproved` when it could not be run confined at all (no try can change that).
pub async fn run_check(cmd: &str, cwd: &Path, project: &Path, full: bool, how: &Confinement) -> Result<(), CheckErr> {
    let mut c = command_for(cmd, cwd, project, full, how)?;
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let unconfined = || CheckErr::Unapproved(tr("mado_build.no_sandbox", &[("cmd", &one_line(cmd, 300))]));
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(_) if how.confined => return Err(unconfined()),
        Err(e) => return Err(CheckErr::Failed(tr("mado_build.check_failed", &[("cmd", &one_line(cmd, 300)), ("output", &e.to_string())]))),
    };
    let (out, err) = (child.stdout.take(), child.stderr.take());
    let run = async {
        let (o, e) = tokio::join!(read_capped(out), read_capped(err));
        (o, e, child.wait().await)
    };
    match tokio::time::timeout(CHECK_LIMIT, run).await {
        Ok((o, e, status)) => {
            if status.as_ref().map_or(false, |s| s.success()) { return Ok(()); }
            let mut text = shown_output(o);
            let e = shown_output(e);
            // The sandbox itself refused to start (its own words, before the command ran).
            if how.confined && e.starts_with("sandbox-exec:") { crate::log::warn(format!("mado build: the sandbox did not start: {}", one_line(&e, 300))); return Err(unconfined()); }
            if !e.trim().is_empty() { if !text.is_empty() { text.push('\n'); } text.push_str(&e); }
            let code = status.ok().and_then(|s| s.code()).map(|c| c.to_string()).unwrap_or_else(|| "?".into());
            Err(CheckErr::Failed(tr("mado_build.check_failed", &[("cmd", &one_line(cmd, 300)), ("output", &format!("(exit {code})\n{}", text.trim_end()))])))
        }
        Err(_) => {
            if let Some(id) = child.id() { crate::proc::end_tree(id).await; }
            let _ = child.kill().await;
            Err(CheckErr::Failed(tr("mado_build.check_timeout", &[("cmd", &one_line(cmd, 300)), ("minutes", &(CHECK_LIMIT.as_secs() / 60).to_string())])))
        }
    }
}

/// The process of one of the build's own commands, not started yet: the user's shell from `cwd`,
/// the cleared environment, its own process group, and (when `how.confined`) macOS's sandbox. Its
/// standard streams are the caller's to set. Refused like `run_check` refuses.
pub(crate) fn command_for(cmd: &str, cwd: &Path, project: &Path, full: bool, how: &Confinement) -> Result<tokio::process::Command, CheckErr> {
    // The same check every command of a run gets (`shell_guard`): not the app's own folder. A guard: it yields to full access.
    if !full && crate::shell_guard::blocked(cmd, &s(cwd), &crate::paths::home()) { return Err(CheckErr::Failed(tr("mado_build.check_refused", &[("cmd", &one_line(cmd, 300))]))); }
    let (prog, args) = shell_argv(cmd);
    let unconfined = || CheckErr::Unapproved(tr("mado_build.no_sandbox", &[("cmd", &one_line(cmd, 300))]));
    let mut c = if how.confined {
        if !Path::new(SANDBOX_EXEC).is_file() { return Err(unconfined()); }
        // The worktree and the cache are the engine's real folders, never links (a command could have planted one).
        if !how.cache.as_os_str().is_empty() && std::fs::symlink_metadata(&how.cache).is_err() { let _ = std::fs::create_dir_all(&how.cache); }
        if how.cache.as_os_str().is_empty() || !real_dirs(cwd, &how.cache) {
            crate::log::warn("mado build: a worktree or the build's cache is not a plain folder; the command was not run");
            return Err(CheckErr::Unapproved(tr("mado_build.not_plain_dirs", &[("cmd", &one_line(cmd, 300))])));
        }
        let profile = sandbox_profile(cwd, project, &how.cache, crate::paths::user_home().ok().as_deref(), how.port);
        let mut c = crate::proc::command(SANDBOX_EXEC);
        c.arg("-p").arg(profile).arg(&prog).args(&args);
        c
    } else {
        let mut c = crate::proc::command(&prog);
        c.args(&args);
        c
    };
    if !how.confined && !how.cache.as_os_str().is_empty() { let _ = std::fs::create_dir_all(&how.cache); }
    c.env_clear().envs(command_env(&how.cache)).current_dir(cwd);
    crate::proc::own_group(&mut c);
    Ok(c)
}

// ---------- commits and merges ----------

/// Does a project-relative `path` fall under an `owns` pattern (`src/auth/**`, `src/*.ts`, a file, a folder)?
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let p = pattern.trim().trim_start_matches("./");
    if p.is_empty() { return false; }
    if !p.contains(['*', '?', '[']) { let p = p.trim_end_matches('/'); return path == p || path.starts_with(&format!("{p}/")); }
    fn m(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (None, _) => false,
            (Some(b'*'), _) if p.get(1) == Some(&b'*') => {
                let rest = if p.get(2) == Some(&b'/') { &p[3..] } else { &p[2..] };
                (0..=s.len()).any(|i| (i == 0 || s[i - 1] == b'/' || p.get(2) != Some(&b'/')) && m(rest, &s[i..]))
            }
            (Some(b'*'), _) => (0..=s.len()).take_while(|i| *i == 0 || s[i - 1] != b'/').any(|i| m(&p[1..], &s[i..])),
            (Some(b'?'), Some(c)) if *c != b'/' => m(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a == b => m(&p[1..], &s[1..]),
            _ => false,
        }
    }
    m(p.as_bytes(), path.as_bytes())
}

/// Commit what the task changed in its lane's worktree (the copied documents aside). Returns the
/// commit and the files it holds. A task that needed no change still gets its (empty) commit.
async fn commit_task(state: &AppState, wt: &Path, n: u32, task: &PlanTask) -> anyhow::Result<(String, Vec<String>)> {
    git_safe(state, wt).await?;
    git(state, wt, &["add", "-A", "--", ".", ":(exclude)mado"]).await?;
    let changed: Vec<String> = git(state, wt, &["diff", "--cached", "--name-only", "-z", "--no-renames"]).await?.split('\0').filter(|f| !f.is_empty()).map(String::from).collect();
    let msg = format!("MADO: {}\n\nTask {} of build {}, made by the Thoughts app.\n\nMado-Task: {}", one_line(&task.title, 100), task.id, n, task.id);
    crate::git::git_ident(state, wt, &["commit", "-q", "--allow-empty", "-m", &msg]).await?;
    // And again after it: settings that appeared meanwhile stop the build here.
    git_safe(state, wt).await?;
    let sha = git(state, wt, &["rev-parse", "HEAD"]).await?.trim().to_string();
    Ok((sha, changed))
}

enum LaneEnd { Merged, Conflict, Stopped }

/// Does a file still hold git's clash marks?
fn has_marks(p: &Path) -> bool {
    let Ok(m) = std::fs::symlink_metadata(p) else { return false };
    if !m.is_file() || m.len() > 8 * FILE_MAX { return false; }
    let t = String::from_utf8_lossy(&std::fs::read(p).unwrap_or_default()).to_string();
    // (Not `=======` alone: that is also how Markdown underlines a heading.)
    t.lines().any(|l| l.starts_with("<<<<<<< ") || l.starts_with(">>>>>>> "))
}

/// The build's own worktree, made again if it is gone.
async fn ensure_build_wt(state: &AppState, project: &Path, pid: &str, b: &BuildState) -> anyhow::Result<PathBuf> {
    let bwt = build_wt(pid)?;
    if !bwt.join(".git").exists() {
        let gl = lock_of(&GIT_LOCKS, pid);
        let _g = gl.lock().await;
        remove_worktree(state, project, pid, &bwt).await;
        git_safe(state, project).await?;
        git(state, project, &["worktree", "add", "-q", &s(&bwt), &b.branch]).await?;
        git_safe(state, &bwt).await?;
        copy_docs(project, &bwt, full(state).await)?;
    }
    Ok(bwt)
}

/// The plan's set-up commands (`npm ci`…), once per fresh worktree, before any task or check there.
/// They are approved commands like the checks, and go through the same re-check before each one.
async fn ensure_setup(state: &AppState, pid: &str, project: &Path, hash: &str, plan: &PlanDoc, wt: &Path) -> Result<(), String> {
    if plan.setup.iter().flatten().all(|c| c.trim().is_empty()) { return Ok(()); }
    let marker = setup_marker(wt);
    if std::fs::read_to_string(&marker).ok().as_deref() == Some(hash) { return Ok(()); }
    run_checks(state, pid, hash, plan.setup.as_deref().unwrap_or(&[]), wt, project).await.map_err(|e| tr("mado_build.setup_failed", &[("output", &e.text())]))?;
    crate::log::ok_or_warn("mado build: could not note that a worktree was set up", std::fs::write(&marker, hash));
    Ok(())
}

/// The build waits for the user, saying why; `lane` (when one) is put down to start again on resume.
async fn hold_waiting(state: &AppState, pid: &str, project: &Path, lane: Option<(usize, usize)>, why: &str) -> anyhow::Result<()> {
    update(pid, project, |b| {
        if let Some(l) = lane.and_then(|(wi, li)| b.lane_mut(wi, li)) { if l.status == TaskStatus::Running { l.status = TaskStatus::Pending; } }
        if b.active() { b.status = BuildStatus::Waiting; b.message = Some(clip_note(why)); }
    }).await?;
    crate::log::warn("mado build: waiting for the user (set-up, a wave's checks, or the steps after the last wave)");
    mado::tell(state, pid, "build").await;
    Ok(())
}

async fn wave_worker(state: Arc<AppState>, pid: String, project: PathBuf, wi: usize) {
    if let Err(e) = wave_inner(&state, &pid, &project, wi).await {
        crate::log::error(format!("mado build: a wave's checks stopped with an error: {e:#}"));
        let _ = hold_waiting(&state, &pid, &project, None, &crate::tr::classify(&e).message).await;
    }
    live_remove(&pid, &wave_key(wi));
    if status_now(&project) == BuildStatus::Stopped {
        if live_of(&pid).is_empty() { cleanup_stopped(&state, &project, &pid).await; }
        mado::tell(&state, &pid, "build").await;
    } else {
        tick(state, pid).await;
    }
}

/// The wave's checks in the build's worktree: the first failure as (task, what went wrong).
async fn wave_checks(state: &AppState, pid: &str, project: &Path, hash: &str, plan: &PlanDoc, ids: &[String], bwt: &Path) -> Result<(), (String, CheckErr)> {
    for id in ids {
        let Some(t) = plan.tasks.iter().find(|t| &t.id == id) else { continue };
        run_checks(state, pid, hash, &t.acceptance, bwt, project).await.map_err(|e| (id.clone(), e))?;
    }
    Ok(())
}

/// After every lane of wave `wi` is merged: its tasks' checks once more, together. A failure gets
/// one fix run in the build's worktree with what failed; still failing, the fix is undone and the build waits.
async fn wave_inner(state: &Arc<AppState>, pid: &str, project: &Path, wi: usize) -> anyhow::Result<()> {
    let b = load_build(project)?.ok_or_else(|| crate::tr::err("mado_build.none", &[]))?;
    if b.status != BuildStatus::Running { return Ok(()); }
    let plan = snapshot(pid, &b.plan_hash)?;
    let ml = lock_of(&MERGE_LOCKS, pid);
    let _m = ml.lock().await;
    let bwt = ensure_build_wt(state, project, pid, &b).await?;
    if let Err(why) = ensure_setup(state, pid, project, &b.plan_hash, &plan, &bwt).await { return hold_waiting(state, pid, project, None, &why).await; }
    // Checked before (the build waited on its `verify` gate, or was paused there): only the gate again.
    if b.waves[wi].checked { return finish::verify_gate(state, pid, project, &b, &plan, wi, &bwt).await; }
    let ids: Vec<String> = b.waves[wi].lanes.iter().flat_map(|l| l.tasks.iter().map(|t| t.id.clone())).collect();
    let (task, out) = match wave_checks(state, pid, project, &b.plan_hash, &plan, &ids, &bwt).await {
        Ok(()) => { mark_checked(pid, project, wi).await?; return finish::verify_gate(state, pid, project, &b, &plan, wi, &bwt).await; }
        Err((_, CheckErr::Unapproved(why))) => return hold_waiting(state, pid, project, None, &why).await,
        Err((task, CheckErr::Failed(out))) => (task, out),
    };
    if status_now(project) == BuildStatus::Stopped { return Ok(()); }
    crate::log::warn(format!("mado build {}: wave {} checks fail once put together; one fix run", b.number, wi + 1));
    let fix = new_chat(state, pid, &task, &format!("{} — {}", tr("mado_build.chat_title_wave", &[]), wi + 1)).await;
    update(pid, project, |b| if let Some(w) = b.waves.get_mut(wi) { w.fix_chat_id = Some(fix.clone()); }).await?;
    mado::tell(state, pid, "build").await;
    let brief = format!("This folder holds the build so far: the work of several tasks, just put together. One of their checks now fails here (what it printed):\n{}\n\
        Fix the code so that every task's checks pass, keeping the work of every task. Stay with the files that need it. \
        Commit nothing: the app runs the checks again and commits the result.\n\n{}", crate::mado_prompts::block(&clip_note(&out)), crate::mado_prompts::DATA_NOTE);
    let ended = run_and_wait(state, project, pid, &fix, &bwt, &brief, None, deny_for(project, pid, &b, &bwt)).await;
    if status_now(project) == BuildStatus::Stopped { reset_wt(state, project, &bwt).await; return Ok(()); }
    // The fix may not change what a done `contracts` or `tests` task agreed on: that is undone, and
    // the checks below run on what is left.
    let locked = finish::locked_by(&plan, &load_build(project)?.unwrap_or_default(), &[]);
    let undone = finish::undo_locked(state, &bwt, &locked, &[]).await?;
    if !undone.is_empty() { crate::log::warn(format!("mado build: a wave's fix changed {} locked file(s); they were put back", undone.len())); }
    let again = match ended { Err(why) => Err(why), Ok(()) => wave_checks(state, pid, project, &b.plan_hash, &plan, &ids, &bwt).await.map_err(|(_, e)| e.text()) };
    if let Err(why) = again {
        reset_wt(state, project, &bwt).await;
        return hold_waiting(state, pid, project, None, &tr("mado_build.wave_failed", &[("output", &why)])).await;
    }
    git_safe(state, &bwt).await?;
    git(state, &bwt, &["add", "-A", "--", ".", ":(exclude)mado"]).await?;
    if !git_ok(state, &bwt, &["diff", "--cached", "--quiet"]).await {
        crate::git::git_ident(state, &bwt, &["commit", "-q", "-m", &format!("MADO: wave {}, fixed after its checks", wi + 1)]).await?;
        git_safe(state, &bwt).await?;
    }
    mark_checked(pid, project, wi).await?;
    finish::verify_gate(state, pid, project, &b, &plan, wi, &bwt).await
}

async fn mark_checked(pid: &str, project: &Path, wi: usize) -> anyhow::Result<()> {
    update(pid, project, |b| if let Some(w) = b.waves.get_mut(wi) { w.checked = true; }).await?;
    crate::log::info(format!("mado build: wave {} checked once put together", wi + 1));
    Ok(())
}

/// Merge a finished lane into the build branch, in the build's worktree. A clash gets one run that
/// settles it; its tasks' commands must pass afterwards, or the merge is undone and the lane waits.
async fn merge_lane(state: &Arc<AppState>, pid: &str, project: &Path, b: &BuildState, plan: &PlanDoc, wi: usize, li: usize) -> anyhow::Result<LaneEnd> {
    let ml = lock_of(&MERGE_LOCKS, pid);
    let _m = ml.lock().await;
    let bwt = ensure_build_wt(state, project, pid, b).await?;
    if let Err(why) = ensure_setup(state, pid, project, &b.plan_hash, plan, &bwt).await {
        hold_waiting(state, pid, project, None, &why).await?;
        return Ok(LaneEnd::Stopped);
    }
    let br = lane_branch(b.number, wi, li);
    let lane = b.waves[wi].lanes[li].clone();
    let names = lane.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ");
    let msg = format!("MADO: wave {} lane {} ({names})", wi + 1, li + 1);
    git_safe(state, &bwt).await?;
    let merged = crate::git::git_ident(state, &bwt, &["merge", "-q", "--no-ff", "--no-edit", "-m", &msg, &br]).await;
    git_safe(state, &bwt).await?;
    let e = match merged { Ok(_) => { drop_lane(state, project, pid, wi, li, &br).await; return Ok(LaneEnd::Merged); } Err(e) => e };
    let clash = crate::git::unmerged(state, &bwt).await;
    if clash.is_empty() {
        if git_ok(state, &bwt, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).await { let _ = git(state, &bwt, &["merge", "--abort"]).await; }
        return Err(e);
    }
    crate::log::warn(format!("mado build {}: lane {} clashes in {} file(s)", b.number, lane.id, clash.len()));
    // Which of the lane's tasks touched the clashing files (the last one when none says).
    let mut hit = vec![];
    for (ti, t) in lane.tasks.iter().enumerate() {
        let Some(c) = &t.commit else { continue };
        let files = git(state, &bwt, &["diff-tree", "--no-commit-id", "--name-only", "-r", "-z", c]).await.unwrap_or_default();
        if files.split('\0').any(|f| clash.iter().any(|x| x == f)) { hit.push(ti); }
    }
    if hit.is_empty() && !lane.tasks.is_empty() { hit.push(lane.tasks.len() - 1); }
    let shown = clash.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
    let first = hit.first().and_then(|ti| lane.tasks.get(*ti)).map(|t| t.id.clone()).unwrap_or_default();
    let fix = new_chat(state, pid, &first, &format!("{} — {}", tr("mado_build.chat_title_fix", &[]), lane.id)).await;
    update(pid, project, |b| {
        if let Some(l) = b.lane_mut(wi, li) {
            l.fix_chat_id = Some(fix.clone());
            for ti in &hit { if let Some(t) = l.tasks.get_mut(*ti) { t.status = TaskStatus::Conflict; t.note = Some(tr("mado_build.conflict", &[("files", &shown)])); } }
        }
    }).await?;
    mado::tell(state, pid, "build").await;
    let titles: Vec<String> = lane.tasks.iter().filter_map(|t| plan.tasks.iter().find(|p| p.id == t.id)).map(|t| format!("{} ({})", t.id, crate::mado_prompts::inline(&one_line(&t.title, 100)))).collect();
    let brief = format!("Two parts of this build changed the same files, and git could not combine them. Files with conflicts:\n{}\n\n\
        Settle every conflict in those files (the lines between <<<<<<<, ======= and >>>>>>>) so that the work of both sides is kept. \
        The side being merged in is: {}. Do not change other files. Commit nothing: the app checks and commits the result.\n\n{}",
        clash.iter().map(|f| format!("- {}", crate::mado_prompts::inline(f))).collect::<Vec<_>>().join("\n"), titles.join("; "), crate::mado_prompts::DATA_NOTE);
    let ended = run_and_wait(state, project, pid, &fix, &bwt, &brief, None, deny_for(project, pid, b, &bwt)).await;
    if status_now(project) == BuildStatus::Stopped { let _ = git(state, &bwt, &["merge", "--abort"]).await; return Ok(LaneEnd::Stopped); }
    if let Err(why) = ended { return fix_failed(state, project, pid, &bwt, wi, li, &why).await; }
    // Settling a clash may not change what a done `contracts` or `tests` task of another lane agreed
    // on (the lane being merged brings its own tasks' files): such a fix fails, and the merge is undone.
    let mine: Vec<&str> = lane.tasks.iter().map(|t| t.id.as_str()).collect();
    let locked = finish::locked_by(plan, &load_build(project)?.unwrap_or_default(), &mine);
    let undone = finish::undo_locked(state, &bwt, &locked, &[]).await?;
    if !undone.is_empty() { return fix_failed(state, project, pid, &bwt, wi, li, &finish::locked_note(&undone)).await; }
    if !git_ok(state, &bwt, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).await || clash.iter().any(|f| has_marks(&bwt.join(f))) {
        return fix_failed(state, project, pid, &bwt, wi, li, &tr("mado_build.fix_failed", &[("files", &shown)])).await;
    }
    let cmds: Vec<String> = lane.tasks.iter().filter_map(|t| plan.tasks.iter().find(|p| p.id == t.id)).flat_map(|t| t.acceptance.clone()).collect();
    if let Err(why) = run_checks(state, pid, &b.plan_hash, &cmds, &bwt, project).await { return fix_failed(state, project, pid, &bwt, wi, li, &why.text()).await; }
    git_safe(state, &bwt).await?;
    git(state, &bwt, &["add", "-A", "--", ".", ":(exclude)mado"]).await?;
    if let Err(e) = crate::git::git_ident(state, &bwt, &["commit", "-q", "--no-edit"]).await { return fix_failed(state, project, pid, &bwt, wi, li, &e.to_string()).await; }
    git_safe(state, &bwt).await?;
    update(pid, project, |b| if let Some(l) = b.lane_mut(wi, li) {
        for t in l.tasks.iter_mut().filter(|t| t.status == TaskStatus::Conflict) { t.status = TaskStatus::Done; t.note = Some(tr("mado_build.settled", &[])); }
    }).await?;
    drop_lane(state, project, pid, wi, li, &br).await;
    Ok(LaneEnd::Merged)
}

/// A lane merged into the build branch: its worktree and branch go.
async fn drop_lane(state: &AppState, project: &Path, pid: &str, wi: usize, li: usize, br: &str) {
    let gl = lock_of(&GIT_LOCKS, pid);
    let _g = gl.lock().await;
    if let Ok(wt) = lane_wt(pid, wi, li) { remove_worktree(state, project, pid, &wt).await; }
    delete_branch(state, project, br, true).await;
}

/// The clash could not be settled: the merge is undone (the build branch is as it was) and the lane waits.
#[allow(clippy::too_many_arguments)]
async fn fix_failed(state: &AppState, project: &Path, pid: &str, bwt: &Path, wi: usize, li: usize, why: &str) -> anyhow::Result<LaneEnd> {
    reset_wt(state, project, bwt).await;
    update(pid, project, |b| if let Some(l) = b.lane_mut(wi, li) {
        for t in l.tasks.iter_mut().filter(|t| t.status == TaskStatus::Conflict) { t.note = Some(clip_note(why)); }
    }).await?;
    Ok(LaneEnd::Conflict)
}

// ---------- after a restart ----------

/// At the daemon's start: every build of the open library's projects that was under way picks up
/// where `build.json` and git say it was. A task that was running starts again from its lane's last commit.
pub fn resume(state: Arc<AppState>) {
    tokio::spawn(async move {
        // Every linked project of every listed library, not only the open one's.
        let ids: Vec<String> = mado::all_linked_projects(&*state.settings.read().await).into_iter().map(|p| p.id).collect();
        for pid in ids { if let Err(e) = recover(&state, &pid).await { crate::log::warn(format!("mado build: could not pick up a build ({e:#})")); } }
    });
}

pub async fn recover(state: &Arc<AppState>, pid: &str) -> anyhow::Result<()> {
    let Ok(project) = engine_project_of(state, pid).await else { return Ok(()) };
    let Ok(Some(b)) = load_build(&project) else { return Ok(()) };
    // A product the screen check started before the daemon went away is stopped first, whatever comes next.
    finish::end_stale_app(pid).await;
    if !matches!(b.status, BuildStatus::Running | BuildStatus::Paused | BuildStatus::Waiting | BuildStatus::Failed | BuildStatus::Checking | BuildStatus::Reviewing) { return Ok(()); }
    // A build goes on only under the approval it was started with (the daemon's own record, never `state.json`).
    let approved = match (mado::approved_plan(state, pid).await, read_mado(&project, "plan.json")) {
        (Some(a), Ok(Some(bytes))) => a.plan_hash == b.plan_hash && mado::approval_matches(&a, &bytes),
        _ => false,
    };
    if !approved && b.active() {
        update(pid, &project, |b| { b.status = BuildStatus::Waiting; b.message = Some(crate::tr::text("mado_build.plan_changed", &[])); }).await?;
        crate::log::warn(format!("mado build {}: not picked up after a restart, its plan is no longer the approved one", b.number));
        mado::tell(state, pid, "build").await;
        return Ok(());
    }
    // Nobody works on this build any more (this is a fresh daemon).
    LIVE.lock().unwrap_or_else(|e| e.into_inner()).retain(|(p, _)| p != pid);
    {
        let gl = lock_of(&GIT_LOCKS, pid);
        let _g = gl.lock().await;
        // A merge, a clash's fix or a wave's fix that was cut off is undone (it is done again).
        if let Ok(bwt) = build_wt(pid) { reset_wt(state, &project, &bwt).await; }
    }
    // Commits the engine made whose record did not reach `build.json` (the daemon stopped in between).
    let mut found: HashMap<String, String> = HashMap::new();
    for (wi, w) in b.waves.iter().enumerate() {
        for (li, l) in w.lanes.iter().enumerate() {
            if l.tasks.iter().all(|t| t.status == TaskStatus::Done) { continue; }
            let br = lane_branch(b.number, wi, li);
            let log = git(state, &project, &["log", "--format=%H%x1f%B%x1e", &format!("{}..{br}", b.branch)]).await.unwrap_or_default();
            for rec in log.split('\x1e') {
                let Some((sha, body)) = rec.trim().split_once('\x1f') else { continue };
                if let Some(id) = body.lines().find_map(|l| l.trim().strip_prefix("Mado-Task: ")) { found.entry(id.trim().to_string()).or_insert_with(|| sha.trim().to_string()); }
            }
            if let Ok(wt) = lane_wt(pid, wi, li) {
                let gl = lock_of(&GIT_LOCKS, pid);
                let _g = gl.lock().await;
                reset_wt(state, &project, &wt).await;
            }
        }
    }
    let status = update(pid, &project, |b| {
        for w in b.waves.iter_mut() {
            for l in w.lanes.iter_mut() {
                for t in l.tasks.iter_mut() {
                    if matches!(t.status, TaskStatus::Running | TaskStatus::Checking | TaskStatus::Pending) {
                        if let Some(sha) = found.get(&t.id) { t.status = TaskStatus::Done; t.commit = Some(sha.clone()); continue; }
                    }
                    if matches!(t.status, TaskStatus::Running | TaskStatus::Checking) { t.status = TaskStatus::Pending; t.attempts = t.attempts.saturating_sub(1); }
                }
                if l.status == TaskStatus::Running { l.status = TaskStatus::Pending; }
            }
        }
        b.governor.running = 0;
        b.status
    }).await?;
    crate::log::info(format!("mado build {}: picked up after a restart", b.number));

    if matches!(status, Some(BuildStatus::Running | BuildStatus::Checking | BuildStatus::Reviewing)) { tick(state.clone(), pid.to_string()).await; } else { mado::tell(state, pid, "build").await; }
    Ok(())
}

// ---------- the API ----------

type R = Result<Json<Value>, crate::api::ApiErr>;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/mado/:projectId/build", get(api_get))
        .route("/api/mado/:projectId/build/start", post(api_start))
        .route("/api/mado/:projectId/build/pause", post(api_pause))
        .route("/api/mado/:projectId/build/resume", post(api_resume))
        .route("/api/mado/:projectId/build/stop", post(api_stop))
        .route("/api/mado/:projectId/build/land", post(api_land))
        .route("/api/mado/:projectId/build/tasks/:id/retry", post(api_retry))
        .route("/api/mado/:projectId/build/screens", get(finish::api_screens_get).post(finish::api_screens_post))
        .route("/api/mado/:projectId/build/guide", get(finish::api_guide))
}

fn shown(b: &BuildState) -> R { Ok(Json(serde_json::to_value(b)?)) }

/// → `BuildState` (an idle one before the first build), plus `blocked`: why a build could not
/// start right now, in words for the user, while none is under way.
async fn api_get(State(st): State<Arc<AppState>>, UrlPath(pid): UrlPath<String>) -> R {
    let project = project_of(&st, &pid).await?;
    engine_dir(&pid)?;
    let mut b = load_build(&project)?.unwrap_or_default();
    b.governor.running = live_of(&pid).len() as u32;
    let mut v = serde_json::to_value(&b)?;
    // Whether the build's own commands run confined (never on other systems than macOS: the window says so).
    v["confined"] = json!(confinement(&st).await.confined);
    if !b.under_way() {
        if let Err(e) = check_ready(&st, &pid, &project, false).await { v["blocked"] = json!(crate::tr::classify(&e).message); }
    }
    Ok(Json(v))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct StartBody { max_parallel: Option<u32> }

async fn api_start(State(st): State<Arc<AppState>>, UrlPath(pid): UrlPath<String>, body: axum::body::Bytes) -> R {
    let b: StartBody = if body.iter().all(u8::is_ascii_whitespace) { StartBody::default() } else { serde_json::from_slice(&body).map_err(|_| crate::tr::err("mado_build.bad_request", &[]))? };
    shown(&start(&st, &pid, b.max_parallel).await?)
}
async fn api_pause(State(st): State<Arc<AppState>>, UrlPath(pid): UrlPath<String>) -> R { shown(&pause(&st, &pid).await?) }
async fn api_resume(State(st): State<Arc<AppState>>, UrlPath(pid): UrlPath<String>) -> R { shown(&resume_build(&st, &pid).await?) }
async fn api_stop(State(st): State<Arc<AppState>>, UrlPath(pid): UrlPath<String>) -> R { shown(&stop(&st, &pid).await?) }
async fn api_land(State(st): State<Arc<AppState>>, UrlPath(pid): UrlPath<String>) -> R { shown(&land(&st, &pid).await?) }
async fn api_retry(State(st): State<Arc<AppState>>, UrlPath((pid, id)): UrlPath<(String, String)>) -> R {
    if !valid_id(&id) { return Err(crate::tr::err("mado_build.no_task", &[]).into()); }
    shown(&retry(&st, &pid, &id).await?)
}

/// Round 0.4.0: the `verify` gate, locked files, and what follows the last wave (the screen check,
/// the review and its fixes), with their routes and the guide.
#[path = "mado_build_finish.rs"]
pub(crate) mod finish;
/// The instructions of the runs `finish` starts (the app's own words; the project's text goes in as data).
#[path = "mado_build_prompts.rs"]
mod prompts;
pub use finish::end_apps;

#[cfg(test)]
#[path = "mado_build_tests.rs"]
mod tests;
