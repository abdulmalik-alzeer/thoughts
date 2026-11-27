//! Round 0.4.0 of the build engine (`docs/mado-spec.md`, "Round 0.4.0"): what checks the joined
//! product, not only each task.
//!
//! - The `verify` gate: after every wave is merged, the plan's `verify` commands run in the build's
//!   copy (the same runner, environment and confinement as every check). A failure goes to a fix run
//!   there with what it printed, up to `MAX_GATE_FIXES` times; then the build waits for the user.
//! - Locked files: once a `contracts` or `tests` task is done, a later change to what it owns is undone
//!   and sent back to the assistant like a failed check.
//! - After the last wave (only for a plan that has the round's fields, so a 0.3.x plan ends as before):
//!   the screen check (`checking`, only with `plan.run`): the daemon starts `run.start` in the build's
//!   copy, confined, waits for `run.url`, asks each screen's address itself, and offers the addresses to
//!   the window for `LOOK_WAIT` (`GET/POST /build/screens`, one nonce); the started program is always
//!   stopped. Then the review (`reviewing`): a fresh run that only reads, its findings parsed
//!   defensively, and up to two rounds of fixing the `must` findings (fix → `verify` → review again).
//!   Whatever is left stays in `review`; the build ends `done` all the same.
//! - `GET /build/guide`: how to run the result (the plan's start command and address, the project's README).
//!
//! Nothing a run or the project wrote is trusted here: the plan is the approved snapshot (and its
//! address and paths are checked again before use), what the window posts and what the reviewer
//! answers are size-limited and cleaned, and every text handed to a run goes in as a data block.

use super::*;
use crate::mado_validate::{screen_path_ok, url_port};
use mado::PlanRun;
use std::time::Instant;

/// The key of the finishing worker in `LIVE`.
pub const FINISH: &str = "finish";
/// How many fix runs a failing `verify` gets before the build waits.
pub const MAX_GATE_FIXES: u32 = 3;
/// How many reviews at most: the first, and one after each of two rounds of fixes.
pub const MAX_REVIEWS: u32 = 3;
/// How long the started product has to answer at its address.
#[cfg(not(test))]
const ANSWER_WAIT: Duration = Duration::from_secs(60);
#[cfg(test)]
const ANSWER_WAIT: Duration = Duration::from_secs(15);
/// How long a window has to look at the screens.
#[cfg(not(test))]
pub(super) const LOOK_WAIT: Duration = Duration::from_secs(120);
#[cfg(test)]
pub(super) const LOOK_WAIT: Duration = Duration::from_secs(4);
/// How long the daemon's own request for one address may take.
const FETCH_LIMIT: Duration = Duration::from_secs(10);
/// The largest `mado/conventions.md` read, and what of it a run is given (characters).
const CONVENTIONS_READ: usize = 256 * 1024;
const CONVENTIONS_MAX: usize = 40_000;
/// The project's README in the guide: what is read, and what is shown (characters).
const README_READ: u64 = 256 * 1024;
pub const README_MAX: usize = 8000;
/// What the window may post about one screen (`ScreenResult` in types.ts).
pub const TEXT_MAX: usize = 4000;
pub const FIELDS_MAX: usize = 60;
pub const ERRORS_MAX: usize = 20;
const LINE_MAX: usize = 500;
/// The largest body `POST /build/screens` takes.
const POST_MAX: usize = 4 * 1024 * 1024;
/// What the reviewer may report.
pub const FINDINGS_MAX: usize = 50;
const FINDING_TEXT_MAX: usize = 1000;
const WHERE_MAX: usize = 200;
/// How much of the screen results and of the comments goes into the reviewer's brief (characters).
const SCREENS_DATA_MAX: usize = 60_000;
const COMMENTS_MAX: usize = 100;

// ---------- the shapes (`BuildScreens`, `BuildReview` in ui/src/lib/mado/types.ts) ----------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScreensStatus { Waiting, Running, Done, Skipped }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildScreens {
    pub status: ScreensStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<u32>,
}

impl BuildScreens {
    fn finished(&self) -> bool { matches!(self.status, ScreensStatus::Done | ScreensStatus::Skipped) }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReviewStatus { Running, Passed, Changes, Failed }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity { Must, Should }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Finding {
    pub severity: Severity,
    #[serde(rename = "where", default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildReview {
    pub status: ReviewStatus,
    /// The number of the review under way or last made (1 to `MAX_REVIEWS`).
    pub round: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_chat_id: Option<String>,
    #[serde(default)]
    pub findings: Vec<Finding>,
    /// Nothing more will be done (beyond the contract's shape).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub finished: bool,
    /// Why the review or its fix did not go as planned, in words for the user (beyond the contract's shape).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// What one screen showed when the window looked (`ScreenResult` in types.ts), cleaned (`clean_results`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ScreenResult {
    pub id: String,
    pub path: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub errors: Vec<String>,
    pub text: String,
    pub fields: Vec<ScreenField>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ScreenField { pub label: String, pub value: String }

/// What is known about one screen after the check: the daemon's own answer, and what the window saw.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ScreenSeen {
    pub id: String,
    pub path: String,
    /// The status code the daemon's own request got (`None`: no answer).
    pub status: Option<u16>,
    pub looked: Option<ScreenResult>,
}

impl ScreenSeen {
    fn ok(&self) -> bool { self.status.is_some_and(|s| s < 400) && self.looked.as_ref().is_none_or(|l| l.ok && l.errors.is_empty()) }
}

// ---------- which phase comes next ----------

pub fn has_verify(plan: &PlanDoc) -> bool { plan.verify.iter().flatten().any(|c| !c.trim().is_empty()) }

/// A plan with any of round 0.4.0's fields: only such a plan gets the phases after the last wave
/// (a 0.3.x plan builds exactly as before).
pub fn is_round4(plan: &PlanDoc) -> bool { has_verify(plan) || plan.run.is_some() || plan.tasks.iter().any(|t| t.kind.is_some()) }

/// What comes after the last wave, while it is not finished: the screen check (with `plan.run`), then
/// the review. `None`: the build is done.
pub fn next_phase(b: &BuildState, plan: &PlanDoc) -> Option<BuildStatus> {
    if plan.run.is_some() && !b.screens.as_ref().is_some_and(BuildScreens::finished) { return Some(BuildStatus::Checking); }
    if is_round4(plan) && !b.review.as_ref().is_some_and(|r| r.finished) { return Some(BuildStatus::Reviewing); }
    None
}

// ---------- texts from the project ----------

/// `text` without control characters (line breaks kept, tabs as spaces) and without the invisible
/// format characters that can make a line read differently from what it holds (bidi overrides,
/// zero-width characters), at most `max` characters.
pub fn clean_text(text: &str, max: usize) -> String {
    text.chars().map(|c| if c == '\t' { ' ' } else { c }).filter(|c| *c == '\n' || !(c.is_control() || invisible(*c))).take(max).collect()
}

/// `clean_text` on one line: line breaks and runs of spaces become one space.
pub fn clean_line(text: &str, max: usize) -> String {
    let t: String = text.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).filter(|c| !(c.is_control() || invisible(*c))).collect();
    t.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

fn invisible(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{feff}' | '\u{061c}' | '\u{00ad}')
}

/// A file the engine reads from the project: a plain file of its own (not a link, not hard-linked
/// to a file elsewhere, unless full access is on), at most `max` bytes of it.
fn read_plain(p: &Path, full: bool, max: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let m = std::fs::symlink_metadata(p).ok()?;
    if m.file_type().is_symlink() && !full { return None; }
    let m = std::fs::metadata(p).ok()?;
    if !m.is_file() { return None; }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.nlink() > 1 && !full { return None; }
    }
    let mut out = vec![];
    std::fs::File::open(p).ok()?.take(max).read_to_end(&mut out).ok()?;
    Some(out)
}

/// `mado/conventions.md` of the project, when there is one (round 0.4.0: written by the tech step),
/// read through the core's readers (inside the real `mado/`, no link on the way), never hard-linked,
/// at most `CONVENTIONS_MAX` characters. It is given to the runs as data.
pub fn conventions(project: &Path, full: bool) -> Option<String> {
    let dir = mado::mado_dir_of(project, full).ok()?;
    let p = mado::resolve_in(&dir, "conventions.md", full).ok()?;
    read_plain(&p, full, 0)?;
    let bytes = match mado::read_in(&dir, "conventions.md", full, CONVENTIONS_READ) {
        Ok(b) => b?,
        Err(e) => { crate::log::warn(format!("mado build: conventions.md was not given to the runs ({})", crate::tr::classify(&e).message)); return None; }
    };
    let text = clean_text(&String::from_utf8_lossy(&bytes), CONVENTIONS_MAX);
    (!text.trim().is_empty()).then_some(text)
}

// ---------- locked files ----------

/// The `owns` patterns locked now: those of every `contracts` or `tests` task of the plan that is
/// done in this build, `except` the tasks asking (a task may change its own files; a lane being
/// merged brings its own tasks' files).
pub fn locked_by(plan: &PlanDoc, b: &BuildState, except: &[&str]) -> Vec<String> {
    let done: HashSet<&str> = b.waves.iter().flat_map(|w| w.lanes.iter()).flat_map(|l| l.tasks.iter()).filter(|t| t.status == TaskStatus::Done).map(|t| t.id.as_str()).collect();
    plan.tasks.iter().filter(|t| t.locks() && done.contains(t.id.as_str()) && !except.contains(&t.id.as_str())).flat_map(|t| t.owns.iter().cloned()).collect()
}

/// In worktree `wt`, the changes (against its last commit) to files under `locked` are undone:
/// changed files are put back, new ones removed. Returns the files that were touched (at most 200).
/// A path in `exempt` is never touched: the exact files (not patterns) the task at hand owns by name
/// in the approved plan, such as the marker a feature's last task writes (`tests/built/F2`) under a
/// folder a tests task owns. Nothing is asked of git when nothing is locked (a 0.3.x plan never gets here).
pub async fn undo_locked(state: &AppState, wt: &Path, locked: &[String], exempt: &[String]) -> anyhow::Result<Vec<String>> {
    if locked.is_empty() { return Ok(vec![]); }
    git_safe(state, wt).await?;
    git(state, wt, &["add", "-A", "--", ".", ":(exclude)mado"]).await?;
    let changed = git(state, wt, &["diff", "--cached", "--name-only", "-z", "--no-renames"]).await?;
    let hit: Vec<String> = changed.split('\0').filter(|f| !f.is_empty() && !exempt.iter().any(|e| e == f) && locked.iter().any(|p| glob_match(p, f))).take(200).map(String::from).collect();
    for f in &hit {
        let spec = format!(":(literal){f}");
        let _ = git(state, wt, &["reset", "-q", "--", &spec]).await;
        if git_ok(state, wt, &["cat-file", "-e", &format!("HEAD:{f}")]).await {
            git(state, wt, &["checkout", "-q", "HEAD", "--", &spec]).await?;
        } else {
            // A new file (git names it relative to the worktree; nothing outside it is removed).
            let p = wt.join(f);
            if !f.split('/').any(|s| s == ".." || s.is_empty()) && std::fs::symlink_metadata(&p).is_ok_and(|m| !m.is_dir()) { let _ = std::fs::remove_file(&p); }
        }
    }
    git_safe(state, wt).await?;
    Ok(hit)
}

/// What a task that changed locked files is told (and what its note says).
pub fn locked_note(files: &[String]) -> String {
    let mut shown: Vec<String> = files.iter().take(10).cloned().collect();
    if files.len() > 10 { shown.push("…".into()); }
    tr("mado_build.locked", &[("files", &shown.join(", "))])
}

// ---------- the governor, for runs after the lanes ----------

/// Wait until the governor lets a new run start (as it holds new lanes), saying why on the build
/// meanwhile. `false`: the build left `while_in` in the meantime (paused, stopped), so nothing starts.
async fn governor_wait(state: &AppState, pid: &str, project: &Path, while_in: &[BuildStatus]) -> bool {
    loop {
        if !while_in.contains(&status_now(project)) { return false; }
        let hold = governor_reason(state, pid).await;
        let changed = update(pid, project, |b| {
            let was = (b.governor.holding, b.governor.reason.clone());
            b.governor.holding = hold.is_some();
            b.governor.reason = hold.clone();
            was != (b.governor.holding, b.governor.reason.clone())
        }).await.ok().flatten().unwrap_or(false);
        if changed { mado::tell(state, pid, "build").await; }
        if hold.is_none() { return true; }
        tokio::time::sleep(RECHECK).await;
    }
}

// ---------- the `verify` gate ----------

/// After wave `wi` is merged (and its tasks' checks passed together): the plan's `verify` commands in
/// the build's copy. A failure gets a fix run there with what it printed (the same chat each time, its
/// session resumed), up to `MAX_GATE_FIXES` times; still failing, the fixes are undone and the build
/// waits. Paused or stopped meanwhile: nothing more is done (it is asked again on resume).
pub async fn verify_gate(state: &Arc<AppState>, pid: &str, project: &Path, b: &BuildState, plan: &PlanDoc, wi: usize, bwt: &Path) -> anyhow::Result<()> {
    if !has_verify(plan) { return Ok(()); }
    let cmds = plan.verify.clone().unwrap_or_default();
    let running = [BuildStatus::Running];
    let mut chat: Option<String> = None;
    let mut undone: Vec<String> = vec![];
    for tries in 0..=MAX_GATE_FIXES {
        if status_now(project) != BuildStatus::Running { return Ok(()); }
        let out = match run_checks(state, pid, &b.plan_hash, &cmds, bwt, project).await {
            Ok(()) => return gate_passed(state, pid, project, wi, bwt).await,
            Err(CheckErr::Unapproved(why)) => return hold_waiting(state, pid, project, None, &why).await,
            Err(CheckErr::Failed(out)) => out,
        };
        if status_now(project) != BuildStatus::Running { reset_wt(state, project, bwt).await; return Ok(()); }
        if tries == MAX_GATE_FIXES {
            reset_wt(state, project, bwt).await;
            crate::log::warn(format!("mado build {}: wave {} fails its verify commands after {MAX_GATE_FIXES} fixes; waiting", b.number, wi + 1));
            return hold_waiting(state, pid, project, None, &tr("mado_build.gate_failed", &[("tries", &MAX_GATE_FIXES.to_string()), ("output", &out)])).await;
        }
        if !governor_wait(state, pid, project, &running).await { reset_wt(state, project, bwt).await; return Ok(()); }
        let session = match &chat { Some(c) => state.chats.with(c, |c| c.session_id.clone()).await.flatten(), None => None };
        let (id, brief, resume) = match (&chat, session) {
            (Some(c), Some(sid)) => (c.clone(), prompts::gate_retry(&out, &undone), Some(sid)),
            _ => {
                let id = new_chat_with(state, pid, json!({"mado": {"projectId": pid, "phase": "verify", "wave": wi + 1}}), &format!("{} {}", tr("mado_build.chat_title_gate", &[]), wi + 1), false).await;
                let ids: Vec<String> = b.waves.get(wi).map(|w| w.lanes.iter().flat_map(|l| l.tasks.iter().map(|t| t.id.clone())).collect()).unwrap_or_default();
                let around = graph_around(state, pid, project, &owns_of(plan, &ids)).await;
                (id, prompts::gate_fix(&out, conventions(project, full(state).await).as_deref(), around.as_deref()), None)
            }
        };
        chat = Some(id.clone());
        update(pid, project, |b| if let Some(w) = b.waves.get_mut(wi) { w.gate_fix_chat_id = Some(id.clone()); }).await?;
        mado::tell(state, pid, "build").await;
        crate::log::warn(format!("mado build {}: wave {} fails its verify commands; fix run {}", b.number, wi + 1, tries + 1));
        let ended = run_and_wait(state, project, pid, &id, bwt, &brief, resume, deny_for(project, pid, b, bwt)).await;
        if status_now(project) == BuildStatus::Stopped { reset_wt(state, project, bwt).await; return Ok(()); }
        if let Err(why) = ended { crate::log::warn(format!("mado build: a verify fix run ended without finishing ({})", one_line(&why, 200))); }
        // A fix may not change what a `contracts` or `tests` task agreed on either.
        let locked = locked_by(plan, &load_build(project)?.unwrap_or_default(), &[]);
        undone = undo_locked(state, bwt, &locked, &[]).await?;
    }
    Ok(())
}

/// `verify` passed: whatever the fix runs changed is committed, and the wave is marked verified.
async fn gate_passed(state: &AppState, pid: &str, project: &Path, wi: usize, bwt: &Path) -> anyhow::Result<()> {
    commit_fixes(state, bwt, &format!("MADO: wave {}, fixed after the verify commands", wi + 1)).await?;
    update(pid, project, |b| if let Some(w) = b.waves.get_mut(wi) { w.verified = true; }).await?;
    crate::log::info(format!("mado build: wave {} passed its verify commands", wi + 1));
    Ok(())
}

/// Commit what changed in the build's copy (its copied documents aside), when anything did.
async fn commit_fixes(state: &AppState, bwt: &Path, msg: &str) -> anyhow::Result<()> {
    git_safe(state, bwt).await?;
    git(state, bwt, &["add", "-A", "--", ".", ":(exclude)mado"]).await?;
    if !git_ok(state, bwt, &["diff", "--cached", "--quiet"]).await {
        crate::git::git_ident(state, bwt, &["commit", "-q", "-m", msg]).await?;
        git_safe(state, bwt).await?;
    }
    Ok(())
}

// ---------- the finishing worker ----------

/// Runs what follows the last wave, phase by phase, until the build is done or leaves `checking` /
/// `reviewing` (paused, stopped, waiting). The started product is stopped whatever happens.
pub async fn finish_worker(state: Arc<AppState>, pid: String, project: PathBuf) {
    let r = finish_inner(&state, &pid, &project).await;
    end_app(&pid).await;
    if let Err(e) = r {
        crate::log::error(format!("mado build: the steps after the last wave stopped with an error: {e:#}"));
        let _ = hold_waiting(&state, &pid, &project, None, &crate::tr::classify(&e).message).await;
    }
    live_remove(&pid, FINISH);
    if status_now(&project) == BuildStatus::Stopped {
        if live_of(&pid).is_empty() { cleanup_stopped(&state, &project, &pid).await; }
        mado::tell(&state, &pid, "build").await;
    } else {
        tick(state, pid).await;
    }
}

async fn finish_inner(state: &Arc<AppState>, pid: &str, project: &Path) -> anyhow::Result<()> {
    // Each step finishes a phase, changes the review's round, or the build leaves these phases; the
    // bound is only a backstop.
    for _ in 0..(4 * MAX_REVIEWS + 8) {
        let b = load_build(project)?.ok_or_else(|| crate::tr::err("mado_build.none", &[]))?;
        if !matches!(b.status, BuildStatus::Checking | BuildStatus::Reviewing) { return Ok(()); }
        let plan = snapshot(pid, &b.plan_hash)?;
        let Some(phase) = next_phase(&b, &plan) else {
            let done = update(pid, project, |b| {
                if !matches!(b.status, BuildStatus::Checking | BuildStatus::Reviewing) { return None; }
                b.status = BuildStatus::Done;
                b.message = None;
                b.governor.holding = false;
                b.governor.reason = None;
                Some(b.clone())
            }).await?.flatten();
            if let Some(b) = done { done_cleanup(state, pid, project, &b).await; mado::tell(state, pid, "build").await; }
            return Ok(());
        };
        let moved = update(pid, project, |b| if matches!(b.status, BuildStatus::Checking | BuildStatus::Reviewing) { b.status = phase; true } else { false }).await?.unwrap_or(false);
        if !moved { return Ok(()); }
        mado::tell(state, pid, "build").await;
        let bwt = ensure_build_wt(state, project, pid, &b).await?;
        if let Err(why) = ensure_setup(state, pid, project, &b.plan_hash, &plan, &bwt).await { return hold_waiting(state, pid, project, None, &why).await; }
        match phase {
            BuildStatus::Checking => screen_check(state, pid, project, &b, plan.run.as_ref().expect("a screen check has a run"), &bwt).await?,
            _ => review_step(state, pid, project, &b, &plan, &bwt).await?,
        }
    }
    anyhow::bail!("the steps after the last wave did not settle")
}

// ---------- the started product ----------

/// Products the screen check started, per project: `(projectId, process id)`. The process leads a
/// group of its own, so ending it ends what it started.
static APPS: std::sync::Mutex<Vec<(String, u32)>> = std::sync::Mutex::new(Vec::new());

/// Where the engine notes a product it started (beside the worktrees, closed to the commands), so
/// that a daemon that went away without stopping it has it stopped on the next start.
fn app_file(pid: &str) -> anyhow::Result<PathBuf> { Ok(engine_dir(pid)?.join("app.pid")) }

/// When process `n` started, as `ps` says (`None` where there is no `ps`, or no such process).
fn started_at(n: u32) -> Option<String> {
    #[cfg(unix)]
    {
        let out = crate::proc::std_command("ps").args(["-o", "lstart=", "-p", &n.to_string()]).stderr(std::process::Stdio::null()).output().ok()?;
        let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !t.is_empty()).then_some(t)
    }
    #[cfg(not(unix))]
    { let _ = n; None }
}

fn register_app(pid: &str, n: u32) {
    APPS.lock().unwrap_or_else(|e| e.into_inner()).push((pid.to_string(), n));
    if let Ok(f) = app_file(pid) {
        let record = format!("{n}\n{}\n", started_at(n).unwrap_or_default());
        crate::log::ok_or_warn("mado build: could not note the started product", crate::paths::write_atomic(&f, record.as_bytes(), true));
    }
}

/// Stop the product the screen check started for project `pid`, with everything it started (its
/// process group). Under the registry's lock, so the process cannot have been reaped (and its
/// number reused) while it is ended. Nothing when none runs.
pub async fn end_app(pid: &str) {
    // Nobody is asked to look any more either.
    drop_offer(pid);
    let pid = pid.to_string();
    let _ = tokio::task::spawn_blocking(move || end_app_now(&pid)).await;
}

fn end_app_now(pid: &str) {
    let mut apps = APPS.lock().unwrap_or_else(|e| e.into_inner());
    let (mine, others): (Vec<_>, Vec<_>) = apps.drain(..).partition(|(p, _)| p == pid);
    *apps = others;
    for (_, n) in mine { crate::proc::kill_tree(n); crate::log::info("mado build: the started product was stopped"); }
    if let Ok(f) = app_file(pid) { let _ = std::fs::remove_file(f); }
}

/// At the daemon's exit: every product a screen check started is stopped.
pub fn end_apps() {
    let pids: Vec<String> = APPS.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(p, _)| p.clone()).collect();
    for p in pids { end_app_now(&p); }
}

/// After a restart: a product noted by a daemon that went away is stopped, when the process with
/// that number is still the one that was started (same start time); the note goes either way.
pub async fn end_stale_app(pid: &str) {
    let Ok(f) = app_file(pid) else { return };
    let Ok(text) = std::fs::read_to_string(&f) else { return };
    let mut lines = text.lines();
    let n = lines.next().and_then(|l| l.trim().parse::<u32>().ok());
    let when = lines.next().unwrap_or_default().trim().to_string();
    if let Some(n) = n.filter(|n| *n > 1) {
        let live = APPS.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(p, x)| p == pid && *x == n);
        if live { end_app(pid).await; return; }
        if !when.is_empty() && tokio::task::spawn_blocking(move || started_at(n)).await.ok().flatten().as_deref() == Some(when.as_str()) {
            tokio::task::spawn_blocking(move || crate::proc::kill_tree(n)).await.ok();
            crate::log::warn("mado build: a product started before the restart was stopped");
        }
    }
    let _ = std::fs::remove_file(f);
}

/// The status code `GET path` gets from the local server at `port` (`None`: no answer). The daemon
/// asks only `127.0.0.1` / `::1`, follows no redirect, sends nothing of its own, and reads only the
/// status line. A fragment (`/#/x`) is not sent, as a browser would not.
pub async fn http_status(port: u16, path: &str) -> Option<u16> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let path = path.split('#').next().filter(|p| !p.is_empty()).unwrap_or("/");
    if !screen_path_ok(path) { return None; }
    let ask = |addr: std::net::SocketAddr| async move {
        let mut s = tokio::net::TcpStream::connect(addr).await.ok()?;
        let req = format!("GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nUser-Agent: Thoughts-MADO-check\r\nAccept: text/html,*/*\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).await.ok()?;
        let mut buf = vec![0u8; 1024];
        let mut got = 0;
        while got < buf.len() {
            let n = s.read(&mut buf[got..]).await.ok()?;
            if n == 0 { break; }
            got += n;
            if buf[..got].windows(2).any(|w| w == b"\r\n") { break; }
        }
        let line = String::from_utf8_lossy(&buf[..got]).lines().next().unwrap_or_default().to_string();
        let mut parts = line.split(' ');
        if !parts.next().unwrap_or_default().starts_with("HTTP/") { return None; }
        parts.next()?.parse::<u16>().ok().filter(|c| (100..=599).contains(c))
    };
    for ip in [std::net::IpAddr::from([127, 0, 0, 1]), std::net::IpAddr::from(std::net::Ipv6Addr::LOCALHOST)] {
        if let Ok(Some(code)) = tokio::time::timeout(FETCH_LIMIT, ask(std::net::SocketAddr::new(ip, port))).await { return Some(code); }
    }
    None
}

/// Is the program listening on `port` one of the started product's (its process group `group`)?
/// `None`: it cannot be told here (no `lsof`).
async fn listener_is_ours(port: u16, group: u32) -> Option<bool> {
    #[cfg(unix)]
    {
        let mut c = crate::proc::command("lsof");
        c.args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fp"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let out = crate::proc::output_timeout(&mut c, Duration::from_secs(10)).await.ok()?;
        let listeners: Vec<u32> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.strip_prefix('p')?.trim().parse().ok()).collect();
        if listeners.is_empty() { return Some(false); }
        for n in listeners {
            let mut c = crate::proc::command("ps");
            c.args(["-o", "pgid=", "-p", &n.to_string()]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            let Ok(o) = crate::proc::output_timeout(&mut c, Duration::from_secs(5)).await else { continue };
            if String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok() == Some(group) { return Some(true); }
        }
        Some(false)
    }
    #[cfg(not(unix))]
    { let _ = (port, group); None }
}

// ---------- the screen check ----------

/// What a window is offered while it may look (`GET /build/screens`), per project.
struct Offer { nonce: String, url: String, screens: Vec<mado::PlanScreen>, until: Instant, tx: Option<tokio::sync::oneshot::Sender<Vec<ScreenResult>>> }

static OFFERS: std::sync::Mutex<Vec<(String, Offer)>> = std::sync::Mutex::new(Vec::new());

enum Look { Done(Vec<ScreenSeen>, Option<String>), Skipped(String, Vec<ScreenSeen>), Wait(String), Left }

/// The screen check, start to end: the product is started, asked for, looked at and stopped; the
/// result is kept on the build (`screens`) and beside it for the review (`screens.json`).
async fn screen_check(state: &Arc<AppState>, pid: &str, project: &Path, b: &BuildState, run: &PlanRun, bwt: &Path) -> anyhow::Result<()> {
    update(pid, project, |b| b.screens = Some(BuildScreens { status: ScreensStatus::Running, reason: None, count: None, failed: None })).await?;
    mado::tell(state, pid, "build").await;
    let r = look(state, pid, project, b, run, bwt).await;
    end_app(pid).await;
    let (status, reason, seen) = match r {
        Look::Left => return Ok(()),
        Look::Wait(why) => {
            update(pid, project, |b| if let Some(s) = &mut b.screens { s.status = ScreensStatus::Running; }).await?;
            return hold_waiting(state, pid, project, None, &why).await;
        }
        Look::Done(seen, None) => (ScreensStatus::Done, None, seen),
        Look::Done(seen, Some(why)) | Look::Skipped(why, seen) => (ScreensStatus::Skipped, Some(why), seen),
    };
    if let Ok(f) = engine_dir(pid).map(|d| d.join("screens.json")) {
        crate::log::ok_or_warn("mado build: the screen results were not kept", crate::paths::write_atomic(&f, &serde_json::to_vec(&seen)?, true));
    }
    let failed = seen.iter().filter(|s| !s.ok()).count() as u32;
    let count = seen.len() as u32;
    crate::log::info(format!("mado build {}: screen check {} ({count} screen(s), {failed} failed)", b.number, if status == ScreensStatus::Done { "done" } else { "skipped" }));
    update(pid, project, |b| b.screens = Some(BuildScreens { status, reason: reason.clone(), count: Some(count), failed: Some(failed) })).await?;
    mado::tell(state, pid, "build").await;
    Ok(())
}

fn still_checking(project: &Path) -> bool { status_now(project) == BuildStatus::Checking }

async fn look(state: &Arc<AppState>, pid: &str, project: &Path, b: &BuildState, run: &PlanRun, bwt: &Path) -> Look {
    let skip = |key: &str, args: &[(&str, &str)]| Look::Skipped(tr(key, args), vec![]);
    // The address is checked again (the plan was validated when approved; this is the snapshot):
    // only this machine, never the app's own port.
    let Some(port) = url_port(&run.url) else { return skip("mado_build.screens_bad_url", &[]) };
    if port == crate::approvals::port() { return skip("mado_build.screens_bad_url", &[]); }
    let screens: Vec<mado::PlanScreen> = run.screens.iter().filter(|s| crate::mado_validate::id_ok(&s.id) && screen_path_ok(&s.path)).take(crate::mado_validate::MAX_SCREENS).cloned().collect();
    // Something already answers there: it would be looked at in the product's place.
    if http_status(port, "/").await.is_some() { return skip("mado_build.screens_port_taken", &[("url", &run.url)]); }
    if !approved_now(state, pid, project, &b.plan_hash, &run.start).await {
        return Look::Wait(tr("mado_build.check_unapproved", &[("cmd", &one_line(&run.start, 300))]));
    }
    let full = full(state).await;
    let mut how = confinement(state).await;
    how.cache = cache_dir(pid).unwrap_or_default();
    let mut c = match command_for(&run.start, bwt, project, full, &how) {
        Ok(c) => c,
        Err(CheckErr::Unapproved(why)) => return Look::Wait(why),
        Err(CheckErr::Failed(why)) => return Look::Skipped(why, vec![]),
    };
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(_) if how.confined => return Look::Wait(tr("mado_build.no_sandbox", &[("cmd", &one_line(&run.start, 300))])),
        Err(e) => return skip("mado_build.screens_start_failed", &[("output", &e.to_string())]),
    };
    let Some(n) = child.id() else { return skip("mado_build.screens_start_failed", &[("output", "")]) };
    register_app(pid, n);
    crate::log::info(format!("mado build {}: started the product for the screen check", b.number));
    let (out, err) = (child.stdout.take(), child.stderr.take());
    let printed = tokio::spawn(async move { let (o, e) = tokio::join!(read_capped(out), read_capped(err)); format!("{}\n{}", shown_output(o), shown_output(e)) });
    let r = look_at(state, pid, project, run, port, n, &screens, &mut child).await;
    // Stopped, whatever happened (and reaped only after: its number is not reused meanwhile).
    end_app(pid).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    match r {
        Err(Some(key)) => {
            let output = tokio::time::timeout(Duration::from_secs(2), printed).await.ok().and_then(Result::ok).unwrap_or_default();
            let tail: String = { let t = output.trim(); let n = t.chars().count(); t.chars().skip(n.saturating_sub(1500)).collect() };
            Look::Skipped(tr(key, &[("url", &run.url), ("output", &clean_text(&tail, 1500)), ("seconds", &ANSWER_WAIT.as_secs().to_string())]), vec![])
        }
        Err(None) => { printed.abort(); Look::Left }
        Ok(l) => { printed.abort(); l }
    }
}

/// With the product started (process `n`): wait for its address, ask each screen's, offer them to a
/// window. `Err(Some(key))`: skipped for that reason (the program's output goes with it); `Err(None)`:
/// the build left the screen check meanwhile.
#[allow(clippy::too_many_arguments)]
async fn look_at(state: &AppState, pid: &str, project: &Path, run: &PlanRun, port: u16, n: u32, screens: &[mado::PlanScreen], child: &mut tokio::process::Child) -> Result<Look, Option<&'static str>> {
    let until = Instant::now() + ANSWER_WAIT;
    loop {
        if !still_checking(project) { return Err(None); }
        if matches!(child.try_wait(), Ok(Some(_))) { return Err(Some("mado_build.screens_exited")); }
        if http_status(port, "/").await.is_some() { break; }
        if Instant::now() > until { return Err(Some("mado_build.screens_no_answer")); }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    // The address must be answered by the started product, not by something else on this machine.
    match listener_is_ours(port, n).await {
        Some(true) => {}
        Some(false) => return Err(Some("mado_build.screens_not_ours")),
        None => if !full(state).await { return Err(Some("mado_build.screens_cannot_tell")) },
    }
    let mut seen = vec![];
    for s in screens { seen.push(ScreenSeen { id: s.id.clone(), path: s.path.clone(), status: http_status(port, &s.path).await, looked: None }); }
    if screens.is_empty() { return Ok(Look::Done(seen, None)); }
    // A window looks (the desktop shell's hidden window), when one is there to be asked.
    if state.events.receiver_count() == 0 { return Ok(Look::Done(seen, Some(tr("mado_build.screens_no_window", &[])))); }
    let nonce = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    {
        let mut o = OFFERS.lock().unwrap_or_else(|e| e.into_inner());
        o.retain(|(p, _)| p != pid);
        o.push((pid.to_string(), Offer { nonce, url: run.url.clone(), screens: screens.to_vec(), until: Instant::now() + LOOK_WAIT, tx: Some(tx) }));
    }
    let _ = update(pid, project, |b| if let Some(s) = &mut b.screens { s.status = ScreensStatus::Waiting; }).await;
    mado::tell(state, pid, "build").await;
    let until = Instant::now() + LOOK_WAIT;
    let got = loop {
        if !still_checking(project) { drop_offer(pid); return Err(None); }
        if Instant::now() > until { break None; }
        match tokio::time::timeout(Duration::from_millis(250), &mut rx).await {
            Ok(Ok(r)) => break Some(r),
            Ok(Err(_)) => break None,
            Err(_) => {}
        }
    };
    drop_offer(pid);
    let _ = update(pid, project, |b| if let Some(s) = &mut b.screens { s.status = ScreensStatus::Running; }).await;
    let Some(results) = got else { return Ok(Look::Done(seen, Some(tr("mado_build.screens_timeout", &[("seconds", &LOOK_WAIT.as_secs().to_string())])))) };
    if results.is_empty() { return Ok(Look::Done(seen, Some(tr("mado_build.screens_not_looked", &[])))); }
    for s in seen.iter_mut() { s.looked = results.iter().find(|r| r.id == s.id).cloned(); }
    Ok(Look::Done(seen, None))
}

fn drop_offer(pid: &str) { OFFERS.lock().unwrap_or_else(|e| e.into_inner()).retain(|(p, _)| p != pid); }

/// What a window posted, cleaned: only the screens it was offered (each once, with the path it was
/// offered), every text without control or invisible characters and within its bounds.
pub fn clean_results(posted: &[Value], offered: &[mado::PlanScreen]) -> Vec<ScreenResult> {
    let mut out: Vec<ScreenResult> = vec![];
    for v in posted.iter().take(offered.len().saturating_mul(2).max(1)) {
        let Some(id) = v["id"].as_str() else { continue };
        let Some(s) = offered.iter().find(|s| s.id == id) else { continue };
        if v["path"].as_str() != Some(s.path.as_str()) || out.iter().any(|r| r.id == id) { continue; }
        let strings = |x: &Value, n: usize, max: usize| -> Vec<String> {
            x.as_array().map(|a| a.iter().filter_map(Value::as_str).map(|t| clean_line(t, max)).filter(|t| !t.is_empty()).take(n).collect()).unwrap_or_default()
        };
        let fields = v["fields"].as_array().map(|a| a.iter().filter(|f| f.is_object()).take(FIELDS_MAX).map(|f| ScreenField {
            label: clean_line(f["label"].as_str().unwrap_or_default(), 200), value: clean_line(f["value"].as_str().unwrap_or_default(), LINE_MAX) }).collect()).unwrap_or_default();
        out.push(ScreenResult {
            id: s.id.clone(), path: s.path.clone(), ok: v["ok"].as_bool().unwrap_or(false),
            status: v["status"].as_u64().filter(|c| (100..=599).contains(c)).map(|c| c as u16),
            errors: strings(&v["errors"], ERRORS_MAX, LINE_MAX), text: clean_text(v["text"].as_str().unwrap_or_default(), TEXT_MAX), fields,
        });
    }
    out
}

/// Constant-time equality of two short secrets.
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `GET /api/mado/:projectId/build/screens` → `{url, screens: [{id, path}], nonce}` while the screen
/// check waits for a window to look, else 409.
pub async fn api_screens_get(State(st): State<Arc<AppState>>, UrlPath(pid): UrlPath<String>) -> R {
    project_of(&st, &pid).await?;
    engine_dir(&pid)?;
    let o = OFFERS.lock().unwrap_or_else(|e| e.into_inner());
    match o.iter().find(|(p, o)| p == &pid && o.until > Instant::now() && o.tx.is_some()) {
        Some((_, o)) => Ok(Json(json!({"url": o.url, "screens": o.screens, "nonce": o.nonce}))),
        None => Err(crate::tr::err("mado_build.screens_none", &[]).into()),
    }
}

/// `POST /api/mado/:projectId/build/screens {nonce, results: ScreenResult[]}`: what the window saw,
/// taken once, for the nonce it was given; everything in it is cleaned (`clean_results`).
pub async fn api_screens_post(State(st): State<Arc<AppState>>, UrlPath(pid): UrlPath<String>, body: axum::body::Bytes) -> R {
    project_of(&st, &pid).await?;
    engine_dir(&pid)?;
    if body.len() > POST_MAX { return Err(crate::tr::err("mado_build.bad_request", &[]).into()); }
    let v: Value = serde_json::from_slice(&body).map_err(|_| crate::tr::err("mado_build.bad_request", &[]))?;
    let (Some(nonce), Some(results)) = (v["nonce"].as_str(), v["results"].as_array()) else { return Err(crate::tr::err("mado_build.bad_request", &[]).into()) };
    let mut offers = OFFERS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(i) = offers.iter().position(|(p, o)| p == &pid && o.until > Instant::now() && o.tx.is_some()) else { return Err(crate::tr::err("mado_build.screens_none", &[]).into()) };
    if !same_secret(nonce, &offers[i].1.nonce) { return Err(crate::tr::err("mado_build.screens_nonce", &[]).into()); }
    let (_, mut o) = offers.remove(i);
    let clean = clean_results(results, &o.screens);
    let n = clean.len();
    if let Some(tx) = o.tx.take() { let _ = tx.send(clean); }
    Ok(Json(json!({"ok": true, "taken": n})))
}

// ---------- the review ----------

/// The last answer of a run's chat that holds findings, and those findings (`parse_findings`).
async fn findings_of(state: &AppState, chat: &str) -> Option<Vec<Finding>> {
    let answers: Vec<String> = state.chats.with(chat, |c| c.messages.iter().rev().filter(|m| m.role == "assistant").take(6).map(|m| m.text.clone()).collect()).await.unwrap_or_default();
    answers.iter().find_map(|t| parse_findings(t))
}

/// The reviewer's findings, from the last fenced `json` block of its answer (or the answer itself):
/// `{"findings": [{severity, where?, text}]}`. Anything else is left out: an unknown severity, an
/// empty text, more than `FINDINGS_MAX`; every text is cleaned and bounded. `None`: no such block.
pub fn parse_findings(answer: &str) -> Option<Vec<Finding>> {
    let n = answer.chars().count();
    let tail: String = answer.chars().skip(n.saturating_sub(200_000)).collect();
    let mut candidates: Vec<&str> = vec![];
    if let Some(i) = tail.rfind("```json") {
        let rest = &tail[i + 7..];
        candidates.push(rest.find("```").map_or(rest, |j| &rest[..j]));
    }
    candidates.push(tail.trim());
    if let Some(i) = tail.rfind("{\"findings\"") { candidates.push(&tail[i..]); }
    let v = candidates.iter().find_map(|c| serde_json::from_str::<Value>(c.trim()).ok().filter(|v| v["findings"].is_array() || v.is_array()))?;
    let list = if v.is_array() { v.as_array() } else { v["findings"].as_array() }?;
    Some(list.iter().filter_map(|f| {
        let severity = match f["severity"].as_str()?.trim().to_ascii_lowercase().as_str() { "must" => Severity::Must, "should" => Severity::Should, _ => return None };
        let text = clean_text(f["text"].as_str()?, FINDING_TEXT_MAX).trim().to_string();
        if text.is_empty() { return None; }
        let at = f["where"].as_str().map(|w| clean_line(w, WHERE_MAX)).filter(|w| !w.is_empty());
        Some(Finding { severity, at, text })
    }).take(FINDINGS_MAX).collect())
}

/// The plan's comments as the reviewer sees them (open, sent and resolved; their words as data).
fn comments_text(project: &Path, full: bool) -> String {
    let Ok(dir) = mado::mado_dir_of(project, full) else { return String::new() };
    let Ok(c) = crate::mado_comments::load(&dir, full) else { return String::new() };
    c.comments.iter().filter(|c| matches!(c.status.as_str(), "open" | "sent" | "resolved")).take(COMMENTS_MAX)
        .map(|c| format!("- [{}] {}", clean_line(&c.status, 20), clean_line(&c.body, 600))).collect::<Vec<_>>().join("\n")
}

/// The screen results kept by the screen check, as the reviewer sees them (bounded).
fn screens_text(pid: &str) -> String {
    let Ok(f) = engine_dir(pid).map(|d| d.join("screens.json")) else { return String::new() };
    let Some(bytes) = read_plain(&f, false, 8 * 1024 * 1024) else { return String::new() };
    let Ok(seen) = serde_json::from_slice::<Vec<ScreenSeen>>(&bytes) else { return String::new() };
    let mut out = String::new();
    for (i, s) in seen.iter().enumerate() {
        let one = serde_json::to_string_pretty(s).unwrap_or_default();
        if out.len() + one.len() > SCREENS_DATA_MAX { out.push_str(&format!("\n…and {} more screens not shown", seen.len() - i)); break; }
        out.push_str(&one);
        out.push('\n');
    }
    out
}

/// One step of the review phase, as `build.review` says it stands: a review (the first, again after
/// a cut-off one, or after a round of fixes), a round of fixes for its `must` findings, or nothing
/// more (finished).
async fn review_step(state: &Arc<AppState>, pid: &str, project: &Path, b: &BuildState, plan: &PlanDoc, bwt: &Path) -> anyhow::Result<()> {
    let r = b.review.clone();
    let finish_now = |note: Option<String>| async move {
        update(pid, project, |b| if let Some(r) = &mut b.review { r.finished = true; if note.is_some() { r.note = note; } }).await.map(|_| ())
    };
    match r.as_ref().map(|r| (r.status, r.round)) {
        Some((ReviewStatus::Passed | ReviewStatus::Failed, _)) => finish_now(None).await,
        Some((ReviewStatus::Changes, round)) if round >= MAX_REVIEWS => finish_now(None).await,
        Some((ReviewStatus::Changes, round)) => fix_round(state, pid, project, b, plan, bwt, round).await,
        Some((ReviewStatus::Running, round)) => review_run(state, pid, project, b, bwt, round.max(1)).await,
        None => review_run(state, pid, project, b, bwt, 1).await,
    }
}

/// Round 0.5.0: the files the build changed since it started (its copy against the commit it
/// started from), `mado/` left out; empty when git cannot say (after the git-settings guard).
async fn build_changed(state: &AppState, b: &BuildState, bwt: &Path) -> Vec<String> {
    if b.base_commit.is_empty() || git_safe(state, bwt).await.is_err() { return vec![]; }
    let range = format!("{}..HEAD", b.base_commit);
    git(state, bwt, &["diff", "--name-only", "-z", "--no-renames", &range]).await.unwrap_or_default()
        .split('\0').filter(|f| !f.is_empty() && !f.starts_with("mado/")).take(200).map(String::from).collect()
}

/// Review number `round`: a fresh run with no memory of the build reads the build's copy and reports.
async fn review_run(state: &Arc<AppState>, pid: &str, project: &Path, b: &BuildState, bwt: &Path, round: u32) -> anyhow::Result<()> {
    if round > MAX_REVIEWS {
        update(pid, project, |b| if let Some(r) = &mut b.review { r.finished = true; if r.status == ReviewStatus::Running { r.status = ReviewStatus::Failed; } }).await?;
        return Ok(());
    }
    if !governor_wait(state, pid, project, &[BuildStatus::Reviewing]).await { return Ok(()); }
    let chat = new_chat_with(state, pid, json!({"mado": {"projectId": pid, "phase": "review", "round": round}}), &format!("{} {round}", tr("mado_build.chat_title_review", &[])), true).await;
    let earlier = b.review.as_ref().map(|r| r.findings.clone()).unwrap_or_default();
    update(pid, project, |b| {
        let fix = b.review.as_ref().and_then(|r| r.fix_chat_id.clone());
        b.review = Some(BuildReview { status: ReviewStatus::Running, round, chat_id: Some(chat.clone()), fix_chat_id: fix, findings: earlier.clone(), finished: false, note: None });
    }).await?;
    mado::tell(state, pid, "build").await;
    let full = full(state).await;
    let impact = match has_graph(pid) { true => graph_impact(state, pid, project, &build_changed(state, b, bwt).await).await, false => None };
    let brief = prompts::review(round, conventions(project, full).is_some(), &comments_text(project, full), &screens_text(pid), if round > 1 { &earlier } else { &[] }, impact.as_deref());
    // It only reads: Claude Code's plan mode (`mode_of`), and no edit of its folder either.
    let mut deny = deny_for(project, pid, b, bwt);
    let own = claude::rule_path(bwt);
    deny.extend([format!("Edit({own})"), format!("Edit({own}/**)")]);
    let ended = run_and_wait_as(state, project, pid, &chat, bwt, &brief, None, deny, true, prompts::REVIEW_SYSTEM).await;
    // Whatever it changed anyway (it may not) is undone: the build's copy is as the review found it.
    git_safe(state, bwt).await?;
    if !dirty_files(state, bwt).await.unwrap_or_default().is_empty() {
        crate::log::warn("mado build: the review run changed files; they were put back");
        reset_wt(state, project, bwt).await;
    }
    if status_now(project) != BuildStatus::Reviewing { return Ok(()); }
    let found = match ended { Ok(()) => findings_of(state, &chat).await, Err(_) => None };
    let note = match (&ended, &found) { (Err(why), _) => Some(clip_note(why)), (Ok(()), None) => Some(tr("mado_build.review_unreadable", &[])), _ => None };
    update(pid, project, |b| if let Some(r) = &mut b.review {
        match &found {
            Some(f) => {
                r.status = if f.iter().any(|x| x.severity == Severity::Must) { ReviewStatus::Changes } else { ReviewStatus::Passed };
                r.findings = f.clone();
                r.finished = r.status == ReviewStatus::Passed || round >= MAX_REVIEWS;
            }
            None => { r.status = ReviewStatus::Failed; r.finished = true; r.note = note.clone(); }
        }
    }).await?;
    crate::log::info(format!("mado build {}: review {round}: {}", b.number, found.as_ref().map_or("no findings could be read".to_string(), |f| format!("{} finding(s), {} must", f.len(), f.iter().filter(|x| x.severity == Severity::Must).count()))));
    mado::tell(state, pid, "build").await;
    Ok(())
}

/// The `must` findings of review `round` go to a fix run in the build's copy (edits only); then
/// `verify` runs again. Passing, the fixes are committed and the next review follows; failing (or
/// the run did not finish), they are undone and the review ends with what it found.
#[allow(clippy::too_many_arguments)]
async fn fix_round(state: &Arc<AppState>, pid: &str, project: &Path, b: &BuildState, plan: &PlanDoc, bwt: &Path, round: u32) -> anyhow::Result<()> {
    let musts: Vec<Finding> = b.review.as_ref().map(|r| r.findings.iter().filter(|f| f.severity == Severity::Must).cloned().collect()).unwrap_or_default();
    if musts.is_empty() {
        update(pid, project, |b| if let Some(r) = &mut b.review { r.status = ReviewStatus::Passed; r.finished = true; }).await?;
        return Ok(());
    }
    if !governor_wait(state, pid, project, &[BuildStatus::Reviewing]).await { return Ok(()); }
    let chat = new_chat_with(state, pid, json!({"mado": {"projectId": pid, "phase": "review-fix", "round": round}}), &format!("{} {round}", tr("mado_build.chat_title_review_fix", &[]), ), false).await;
    update(pid, project, |b| if let Some(r) = &mut b.review { r.fix_chat_id = Some(chat.clone()); }).await?;
    mado::tell(state, pid, "build").await;
    let full = full(state).await;
    let named: Vec<String> = musts.iter().filter_map(|f| f.at.as_deref()).map(|w| w.split(':').next().unwrap_or(w).trim().to_string()).collect();
    let around = graph_around(state, pid, project, &named).await;
    let brief = prompts::review_fix(&musts, conventions(project, full).as_deref(), around.as_deref());
    let ended = run_and_wait(state, project, pid, &chat, bwt, &brief, None, deny_for(project, pid, b, bwt)).await;
    if status_now(project) != BuildStatus::Reviewing { reset_wt(state, project, bwt).await; return Ok(()); }
    let locked = locked_by(plan, &load_build(project)?.unwrap_or_default(), &[]);
    let undone = undo_locked(state, bwt, &locked, &[]).await?;
    if !undone.is_empty() { crate::log::warn(format!("mado build: a review fix changed {} locked file(s); they were put back", undone.len())); }
    let failed = match ended {
        Err(why) => Some(why),
        Ok(()) if has_verify(plan) => match run_checks(state, pid, &b.plan_hash, plan.verify.as_deref().unwrap_or(&[]), bwt, project).await {
            Ok(()) => None,
            Err(CheckErr::Unapproved(why)) => { reset_wt(state, project, bwt).await; return hold_waiting(state, pid, project, None, &why).await; }
            Err(CheckErr::Failed(out)) => Some(out),
        },
        Ok(()) => None,
    };
    if let Some(why) = failed {
        reset_wt(state, project, bwt).await;
        crate::log::warn(format!("mado build {}: the fixes of review {round} were undone", b.number));
        update(pid, project, |b| if let Some(r) = &mut b.review { r.finished = true; r.note = Some(tr("mado_build.review_fix_undone", &[("output", &clip_note(&why))])); }).await?;
        mado::tell(state, pid, "build").await;
        return Ok(());
    }
    commit_fixes(state, bwt, &format!("MADO: fixes after review {round}")).await?;
    update(pid, project, |b| if let Some(r) = &mut b.review { r.status = ReviewStatus::Running; r.round = round + 1; r.chat_id = None; }).await?;
    mado::tell(state, pid, "build").await;
    Ok(())
}

// ---------- after the build ----------

/// The plan the guide speaks of: the build's own approved copy while it is there, else the
/// project's plan as it validates now.
fn guide_plan(pid: &str, project: &Path, full: bool) -> Option<PlanDoc> {
    if let Ok(Some(b)) = load_build(project) { if let Ok(p) = snapshot(pid, &b.plan_hash) { return Some(p); } }
    mado::read_plan(&mado::mado_dir_of(project, full).ok()?, full).ok().flatten()
}

/// The project's own `README.md`, for the guide: a plain file of the project's (not a link, not
/// hard-linked, in the project's own folder; links yield to full access), at most `README_MAX`
/// characters of text without control or invisible characters.
pub fn readme(project: &Path, full: bool) -> Option<String> {
    let p = project.join("README.md");
    if !full {
        let parent = crate::paths::canonical(&p).ok()?.parent()?.to_path_buf();
        if crate::paths::canonical(project).ok()? != parent { return None; }
    }
    let bytes = read_plain(&p, full, README_READ)?;
    let text = clean_text(&String::from_utf8_lossy(&bytes), README_MAX);
    (!text.trim().is_empty()).then_some(text)
}

/// `GET /api/mado/:projectId/build/guide` → `{start?, url?, readme?}`: how to run what was built.
pub async fn api_guide(State(st): State<Arc<AppState>>, UrlPath(pid): UrlPath<String>) -> R {
    let project = project_of(&st, &pid).await?;
    engine_dir(&pid)?;
    let full = full(&st).await;
    let mut v = json!({});
    if let Some(run) = guide_plan(&pid, &project, full).and_then(|p| p.run) {
        // Only what the review would have let through: one plain command, a local address.
        if crate::mado_validate::command_ok(&run.start) { v["start"] = json!(run.start.trim()); }
        if url_port(&run.url).is_some() { v["url"] = json!(run.url); }
    }
    if let Some(t) = readme(&project, full) { v["readme"] = json!(t); }
    Ok(Json(v))
}

#[cfg(test)]
pub fn test_register(pid: &str, n: u32) { register_app(pid, n) }
#[cfg(test)]
pub fn test_forget(pid: &str) { APPS.lock().unwrap_or_else(|e| e.into_inner()).retain(|(p, _)| p != pid) }
