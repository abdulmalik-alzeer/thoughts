//! Workflows: small recurring jobs the user (through the AI) defines in the layer as
//! `workflows/*.json`. The daemon runs each due one as a chat of kind `workflow` inside the
//! library — one at a time, never while a wrap is running, trust-checked like any other run.

use crate::chats::{Chat, Msg, RunOptions};
use crate::claude::{self, RunSpec};
use crate::state::AppState;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Trigger {
    /// `every`: "day" | "week" | "weekdays" | "month"; `at`: "HH:MM" local time;
    /// `weekday`: 0 (Sunday) – 6 (Saturday), weekly only; `day`: 1–28 or "last", monthly only;
    /// `days`: which weekdays count for "weekdays" (default Monday–Friday).
    Schedule {
        every: String, at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")] weekday: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")] day: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")] days: Option<Vec<u32>>,
    },
    /// `on`: "wrap.done" | "app.start"
    Event { on: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Workflow {
    pub name: String,
    pub trigger: Trigger,
    pub prompt: String,
}

fn parse_at(at: &str) -> Option<NaiveTime> {
    if at.len() != 5 { return None; }
    NaiveTime::parse_from_str(at, "%H:%M").ok()
}

/// Day of the month a monthly schedule fires on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MonthDay { On(u32), Last }

fn month_day(day: &Option<Value>) -> Option<MonthDay> {
    match day.as_ref()? {
        Value::String(s) if s == "last" => Some(MonthDay::Last),
        v => v.as_u64().filter(|d| (1..=28).contains(d)).map(|d| MonthDay::On(d as u32)),
    }
}

/// Does a schedule fire on `day`? `None` for a schedule the runner does not understand.
fn fits(trigger: &Trigger, day: NaiveDate) -> Option<bool> {
    let Trigger::Schedule { every, weekday, day: month, days, .. } = trigger else { return None };
    let wd = day.weekday().num_days_from_sunday();
    Some(match every.as_str() {
        "day" => true,
        "week" => wd == weekday.unwrap_or(0),
        "weekdays" => match days { Some(d) => d.contains(&wd), None => (1..=5).contains(&wd) },
        "month" => match month_day(month)? {
            MonthDay::On(n) => day.day() == n,
            // The last day of a month is the one whose tomorrow is the 1st.
            MonthDay::Last => day.succ_opt().map_or(true, |t| t.day() == 1),
        },
        _ => return None,
    })
}

/// Is this definition one the runner understands?
pub fn valid(w: &Workflow) -> bool {
    if w.name.trim().is_empty() || w.name.chars().count() > 80 || w.name.chars().any(char::is_control) { return false; }
    if w.prompt.trim().is_empty() || w.prompt.len() > 20_000 { return false; }
    match &w.trigger {
        Trigger::Schedule { every, at, weekday, day, days } => parse_at(at).is_some() && match every.as_str() {
            "day" => true,
            "week" => weekday.map_or(true, |d| d <= 6),
            "weekdays" => days.as_ref().map_or(true, |d| !d.is_empty() && d.len() <= 7 && d.iter().all(|x| *x <= 6)),
            "month" => month_day(day).is_some(),
            _ => false,
        },
        Trigger::Event { on } => on == "wrap.done" || on == "app.start",
    }
}

/// A valid workflow together with the file it came from and that file's content hash.
/// The file is the workflow's identity (the name is only what it is called); the hash is its content.
#[derive(Clone, Debug)]
pub struct Loaded {
    /// File name inside `workflows/`, e.g. `weekly-review.json`.
    pub file: String,
    /// sha256 of the file's bytes: approval is given to exactly this content.
    pub hash: String,
    pub workflow: Workflow,
    /// Another workflow file uses the same name: the list says so, so the two can be told apart.
    pub duplicate_name: bool,
}

pub fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// Valid workflows in `<layer>/workflows/*.json`, in file order. Two files may carry the same
/// name: neither hides the other (each is its own workflow, approved on its own).
pub fn load_files(layer: &Path) -> Vec<Loaded> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(layer.join("workflows")).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "json") && p.is_file()).collect();
    files.sort();
    let mut out: Vec<Loaded> = vec![];
    for f in files.into_iter().take(50) {
        let Ok(bytes) = std::fs::read(&f) else { continue };
        let Some(mut v) = serde_json::from_slice::<Value>(&bytes).ok() else { continue };
        if v["name"].as_str().map_or(true, |n| n.trim().is_empty()) {
            if let Some(o) = v.as_object_mut() { o.insert("name".into(), json!(f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())); }
        }
        let Ok(w) = serde_json::from_value::<Workflow>(v) else { continue };
        let Some(file) = f.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
        if valid(&w) { out.push(Loaded { file, hash: content_hash(&bytes), workflow: w, duplicate_name: false }); }
    }
    let names: Vec<String> = out.iter().map(|l| l.workflow.name.clone()).collect();
    for l in &mut out { l.duplicate_name = names.iter().filter(|n| **n == l.workflow.name).count() > 1; }
    out
}

pub fn load(layer: &Path) -> Vec<Workflow> {
    load_files(layer).into_iter().map(|l| l.workflow).collect()
}

/// Workflows run unattended, so each one runs only after the user has approved it — and only the
/// exact content they approved: file name -> sha256, kept outside the layer where no run can write.
pub type Approved = BTreeMap<String, String>;

fn approved_file(home: &Path) -> PathBuf { home.join("workflows-approved.json") }

pub fn load_approved(home: &Path) -> Approved {
    std::fs::read_to_string(approved_file(home)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_approved(home: &Path, a: &Approved) -> anyhow::Result<()> {
    crate::paths::write_atomic(&approved_file(home), serde_json::to_string_pretty(a)?.as_bytes(), true)?;
    Ok(())
}

fn is_approved(a: &Approved, l: &Loaded) -> bool { a.get(&l.file) == Some(&l.hash) }

/// The workflows the runner may start: valid, and approved in their current form.
pub fn approved_workflows(layer: &Path, home: &Path) -> Vec<Loaded> {
    let a = load_approved(home);
    load_files(layer).into_iter().filter(|l| is_approved(&a, l)).collect()
}

/// The workflow in file `file` — looked up by its file only, never by its name.
fn find(layer: &Path, file: &str) -> anyhow::Result<Loaded> {
    load_files(layer).into_iter().find(|l| l.file == file).ok_or_else(|| crate::tr::err("workflow.gone", &[]))
}

/// The workflow the user was shown: file `file` with exactly the content `hash`. Anything else —
/// another file, or this file changed since the list was read — is refused, so they look again.
fn find_seen(layer: &Path, file: &str, hash: &str) -> anyhow::Result<Loaded> {
    let l = find(layer, file)?;
    if hash.is_empty() || l.hash != hash { return Err(crate::tr::err("workflow.changed", &[])); }
    Ok(l)
}

/// The user allows the workflow they were shown (file `file`, content `hash`) to run on its own.
pub fn approve(layer: &Path, home: &Path, file: &str, hash: &str) -> anyhow::Result<Loaded> {
    let l = find_seen(layer, file, hash)?;
    let mut a = load_approved(home);
    a.insert(l.file.clone(), l.hash.clone());
    save_approved(home, &a)?;
    Ok(l)
}

/// Stop the workflow in file `file` from running (its file stays; approving it again re-enables it).
pub fn disable(layer: &Path, home: &Path, file: &str) -> anyhow::Result<Loaded> {
    let l = find(layer, file)?;
    let mut a = load_approved(home);
    if a.remove(&l.file).is_some() { save_approved(home, &a)?; }
    Ok(l)
}

/// Approve the workflows among `paths` (layer-relative, e.g. `workflows/x.json`) as they now are in
/// the live layer. Used when the user keeps a change they reviewed and ticked "allow to run".
/// Returns the approved file names.
pub fn approve_paths(layer: &Path, home: &Path, paths: &[String]) -> anyhow::Result<Vec<String>> {
    let loaded = load_files(layer);
    let mut a = load_approved(home);
    let mut done = vec![];
    for p in paths {
        let Some(file) = p.strip_prefix("workflows/").filter(|f| !f.contains('/')) else { continue };
        if let Some(l) = loaded.iter().find(|l| l.file == file) {
            a.insert(l.file.clone(), l.hash.clone());
            done.push(l.file.clone());
        }
    }
    if !done.is_empty() { save_approved(home, &a)?; }
    Ok(done)
}

/// Forget approvals for files that no longer exist.
fn prune_approved(layer: &Path, home: &Path) {
    let a = load_approved(home);
    if a.is_empty() { return; }
    let dir = layer.join("workflows");
    let kept: Approved = a.iter().filter(|(f, _)| dir.join(f).is_file()).map(|(f, h)| (f.clone(), h.clone())).collect();
    if kept.len() != a.len() { crate::log::ok_or_warn("could not save workflow approvals", save_approved(home, &kept)); }
}

/// The first time a schedule fires strictly after `after` (local wall-clock time). `None` for event triggers.
pub fn next_run(trigger: &Trigger, after: NaiveDateTime) -> Option<NaiveDateTime> {
    let Trigger::Schedule { at, .. } = trigger else { return None };
    let time = parse_at(at)?;
    let mut day = after.date();
    // Two months ahead covers every case, including a monthly schedule seen just after it fired.
    for _ in 0..63 {
        let candidate = day.and_time(time);
        if fits(trigger, day)? && candidate > after { return Some(candidate); }
        day = day.succ_opt()?;
    }
    None
}

/// The last time a schedule fired at or before `now`. `None` for event triggers.
pub fn prev_run(trigger: &Trigger, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let Trigger::Schedule { at, .. } = trigger else { return None };
    let time = parse_at(at)?;
    let mut day = now.date();
    for _ in 0..63 {
        let candidate = day.and_time(time);
        if fits(trigger, day)? && candidate <= now { return Some(candidate); }
        day = day.pred_opt()?;
    }
    None
}

/// A run missed while the app was closed or the computer slept is caught up only this long after
/// its time: a day for daily ones; longer for weekly and monthly ones, so a Friday review or a
/// month-end task missed over a weekend still runs on Monday.
pub fn catch_up(trigger: &Trigger) -> Duration {
    match trigger {
        Trigger::Schedule { every, .. } if every == "week" => Duration::days(3),
        Trigger::Schedule { every, .. } if every == "month" => Duration::days(5),
        _ => Duration::hours(24),
    }
}
/// A failed run is tried once more after this many minutes.
pub const RETRY_MINUTES: i64 = 10;
/// Limits every run gets unless the user set others: how long it may take, and how much it may spend.
pub const DEFAULT_MINUTES: u32 = 20;
pub const DEFAULT_BUDGET_USD: f64 = 2.0;
pub const MAX_MINUTES: u32 = 240;
pub const MIN_BUDGET_USD: f64 = 0.1;
pub const MAX_BUDGET_USD: f64 = 100.0;
/// How many past runs are remembered per workflow.
pub const HISTORY: usize = 10;

/// How one run ended.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct RunResult {
    pub started: Option<NaiveDateTime>,
    pub ended: Option<NaiveDateTime>,
    /// "ok" | "failed" | "stopped" (by the user) | "timeLimit" | "spendLimit" | "interrupted" | "notStarted"
    pub outcome: String,
    pub chat_id: Option<String>,
    /// Started by hand ("Run now"), not by its schedule or event.
    pub manual: bool,
    pub cost_usd: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Seen {
    pub last_run: Option<NaiveDateTime>,
    /// When the runner first met this workflow in its approved form: a schedule counts from here, so
    /// a workflow never fires for a time that had already passed when it was allowed.
    pub first_seen: Option<NaiveDateTime>,
    /// The chat of the run that is in progress (or was, when the daemon last looked).
    pub chat_id: Option<String>,
    /// A failed run is tried again at this time…
    pub retry_at: Option<NaiveDateTime>,
    /// …but only once per run.
    pub retried: bool,
    /// The approved content `first_seen` counts from; a new approval starts the count again.
    /// Empty while the workflow is not approved.
    pub approved_hash: Option<String>,
    /// When the run in progress started (for its time limit).
    pub started_at: Option<NaiveDateTime>,
    /// The run in progress was started by hand.
    pub manual: bool,
    /// The latest scheduled time that was missed for good (too long ago to catch up).
    pub skipped: Option<NaiveDateTime>,
    /// Past runs, newest first.
    pub results: Vec<RunResult>,
    /// The user's own limits for this workflow (the defaults when unset).
    pub minutes: Option<u32>,
    pub budget_usd: Option<f64>,
}

impl Seen {
    pub fn minutes(&self) -> u32 { self.minutes.unwrap_or(DEFAULT_MINUTES) }
    pub fn budget(&self) -> f64 { self.budget_usd.unwrap_or(DEFAULT_BUDGET_USD) }
}

/// What the runner remembers, by workflow file.
pub type State = BTreeMap<String, Seen>;

fn state_file(home: &Path) -> PathBuf { home.join("workflows-state.json") }

pub fn load_state(home: &Path) -> State {
    std::fs::read_to_string(state_file(home)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_state(home: &Path, s: &State) {
    if let Ok(t) = serde_json::to_string_pretty(s) { let _ = crate::paths::write_atomic(&state_file(home), t.as_bytes(), true); }
}

/// Older versions remembered workflows by name. Move such entries to the file that carries the
/// name (the first one, as the old runner did). Returns whether anything moved.
pub fn migrate(st: &mut State, loaded: &[Loaded]) -> bool {
    let mut moved = false;
    for l in loaded {
        if st.contains_key(&l.file) || loaded.iter().any(|x| x.file == l.workflow.name) { continue; }
        if loaded.iter().find(|x| x.workflow.name == l.workflow.name).map(|x| &x.file) != Some(&l.file) { continue; }
        if let Some(s) = st.remove(&l.workflow.name) { st.insert(l.file.clone(), s); moved = true; }
    }
    moved
}

fn base(seen: &Seen) -> Option<NaiveDateTime> {
    match (seen.last_run, seen.first_seen) { (Some(a), Some(b)) => Some(a.max(b)), (a, b) => a.or(b) }
}

/// The scheduled time `w` has to run for right now, if any: the latest time its schedule fired,
/// when that is after its last run (and after it was allowed) and recent enough to catch up.
/// However long the app was closed this is at most one run — older missed times are skipped.
pub fn missed(w: &Workflow, seen: Option<&Seen>, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let base = base(seen?)?;
    let slot = prev_run(&w.trigger, now)?;
    (slot > base && now - slot <= catch_up(&w.trigger)).then_some(slot)
}

/// A scheduled time that passed while the app was closed and is now too long ago to catch up, and
/// that the user has not been told about yet.
pub fn newly_skipped(w: &Workflow, seen: &Seen, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let base = base(seen)?;
    let slot = prev_run(&w.trigger, now)?;
    (slot > base && now - slot > catch_up(&w.trigger) && seen.skipped != Some(slot) && seen.chat_id.is_none()).then_some(slot)
}

/// When `w` should next run given what the runner remembers about it.
pub fn due_at(w: &Workflow, seen: Option<&Seen>, now: NaiveDateTime) -> Option<NaiveDateTime> {
    if let Some(t) = seen.and_then(|s| s.retry_at) { return Some(t); }
    missed(w, seen, now).or_else(|| next_run(&w.trigger, now))
}

/// Files of schedule workflows that are due at `now`, and of failed ones whose retry time has come.
pub fn due(workflows: &[Loaded], state: &State, now: NaiveDateTime) -> Vec<String> {
    workflows.iter().filter(|l| {
        let seen = state.get(&l.file);
        if seen.map_or(false, |s| s.chat_id.is_some()) { return false; }
        missed(&l.workflow, seen, now).is_some() || seen.and_then(|s| s.retry_at).map_or(false, |t| t <= now)
    }).map(|l| l.file.clone()).collect()
}

/// A scheduled run of `seen`'s workflow ended. A failure earns one retry; a success (or a second failure) clears it.
pub fn note_result(seen: &mut Seen, ok: bool, now: NaiveDateTime) {
    seen.chat_id = None;
    if !ok && !seen.retried {
        seen.retried = true;
        seen.retry_at = Some(now + Duration::minutes(RETRY_MINUTES));
    } else {
        seen.retry_at = None;
    }
}

/// A scheduled run of `seen`'s workflow is starting at `now`. A retry keeps its mark, so it is not retried again.
pub fn note_start(seen: &mut Seen, now: NaiveDateTime) {
    let retry = seen.retry_at.map_or(false, |t| t <= now);
    seen.retried = retry;
    seen.retry_at = None;
    seen.last_run = Some(now);
    seen.started_at = Some(now);
    seen.manual = false;
}

/// A run started by hand: it neither counts as the scheduled run nor touches the retry.
pub fn note_manual_start(seen: &mut Seen, chat_id: &str, now: NaiveDateTime) {
    seen.chat_id = Some(chat_id.to_string());
    seen.started_at = Some(now);
    seen.manual = true;
}

/// How a run ended, from what is known about it. `limit` is set when the runner stopped it for
/// running too long; `error` is the last error the chat shows.
pub fn outcome(ok: bool, by_user: bool, limit: bool, interrupted: bool, spent: f64, budget: f64, error: &str) -> &'static str {
    if limit { return "timeLimit"; }
    if ok { return "ok"; }
    if error.to_lowercase().contains("budget") || (budget > 0.0 && spent >= budget) { return "spendLimit"; }
    if interrupted { return "interrupted"; }
    if by_user { return "stopped"; }
    "failed"
}

/// Record how the run in progress ended. Only a scheduled run that simply failed is retried.
pub fn settle(seen: &mut Seen, outcome: &str, cost: f64, now: NaiveDateTime) {
    let chat_id = seen.chat_id.take();
    let manual = std::mem::take(&mut seen.manual);
    let started = seen.started_at.take();
    if !manual { note_result(seen, !matches!(outcome, "failed" | "notStarted"), now); }
    record(seen, RunResult { started, ended: Some(now), outcome: outcome.into(), chat_id, manual, cost_usd: cost });
}

fn record(seen: &mut Seen, r: RunResult) {
    seen.results.insert(0, r);
    seen.results.truncate(HISTORY);
}

/// Has the run in progress gone past its time limit?
pub fn over_time(seen: &Seen, now: NaiveDateTime) -> bool {
    seen.chat_id.is_some() && seen.started_at.map_or(false, |t| now - t >= Duration::minutes(seen.minutes() as i64))
}

/// The user's limits for one workflow: 1–240 minutes, $0.10–$100.
pub fn check_limits(minutes: u32, budget: f64) -> anyhow::Result<()> {
    if !(1..=MAX_MINUTES).contains(&minutes) || !budget.is_finite() || !(MIN_BUDGET_USD..=MAX_BUDGET_USD).contains(&budget) {
        return Err(crate::tr::err("workflow.limits", &[]));
    }
    Ok(())
}

fn local_iso(t: NaiveDateTime) -> Value {
    json!(t.and_local_timezone(Local).earliest().map(|d| d.to_rfc3339()))
}

fn result_json(r: &RunResult) -> Value {
    json!({"started": r.started.map(local_iso), "ended": r.ended.map(local_iso), "outcome": r.outcome, "chatId": r.chat_id, "manual": r.manual, "costUsd": r.cost_usd})
}

/// One entry per workflow file: what it is (`name, file, hash, trigger, prompt, duplicateName`),
/// whether it may run (`approved`), what happened (`lastRun, last, history, running, chatId,
/// skipped, retryAt`), what comes next (`nextRun`, approved ones only) and its `limits`.
/// `hash` is what an approve or "run now" must send back.
pub fn status(layer: &Path, home: &Path, now: NaiveDateTime) -> Vec<Value> {
    let mut state = load_state(home);
    let loaded = load_files(layer);
    migrate(&mut state, &loaded);
    let approved = load_approved(home);
    loaded.iter().map(|l| {
        let w = &l.workflow;
        let seen = state.get(&l.file);
        let ok = is_approved(&approved, l);
        let last = seen.and_then(|s| s.results.first());
        // A skip is news only until something ran after it.
        let skipped = seen.and_then(|s| s.skipped).filter(|t| seen.and_then(|s| s.last_run).map_or(true, |r| r < *t));
        json!({
            "name": w.name, "file": l.file, "hash": l.hash, "trigger": w.trigger, "prompt": w.prompt, "approved": ok,
            "duplicateName": l.duplicate_name,
            "lastRun": seen.and_then(|s| s.last_run.or(last.and_then(|r| r.started))).map(local_iso),
            "nextRun": if ok { due_at(w, seen, now).map(local_iso) } else { None },
            "running": seen.map_or(false, |s| s.chat_id.is_some()),
            "chatId": seen.and_then(|s| s.chat_id.clone()),
            "last": last.map(result_json),
            "history": seen.map(|s| s.results.iter().map(result_json).collect::<Vec<_>>()).unwrap_or_default(),
            "skipped": skipped.map(local_iso),
            "retryAt": if ok { seen.and_then(|s| s.retry_at).map(local_iso) } else { None },
            "limits": {"minutes": seen.map_or(DEFAULT_MINUTES, Seen::minutes), "budgetUsd": seen.map_or(DEFAULT_BUDGET_USD, Seen::budget)},
        })
    }).collect()
}

/// One pass of the runner (or one "run now" / limits change) at a time, so the state file has one writer.
static PASS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Runs the runner stopped for going past their time limit (chat ids), until they are settled.
static LIMITED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn limited(chat: &str) -> bool { LIMITED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|c| c == chat) }

/// Something that must not be interrupted, or another workflow, is running.
async fn busy(state: &AppState) -> bool {
    state.library_busy() || state.chats.list().await.iter().any(|c| c.running && (c.kind == "workflow" || c.kind == "wrap" || c.kind == "reorganize"))
        // A wrap that was cut off: nothing new is written into the library until the user has decided.
        || crate::wraps::interrupted_wrap(state).await.is_some()
}

async fn start(state: &Arc<AppState>, w: &Workflow, budget: f64) -> anyhow::Result<String> {
    let lib = state.settings.read().await.library().ok_or_else(|| anyhow::anyhow!("no library configured"))?;
    let mut chat = Chat { kind: "workflow".into(), title: w.name.clone(), ..Default::default() };
    chat.options = RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() };
    chat.messages.push(Msg::new("user", &w.prompt));
    let id = chat.id.clone();
    state.chats.insert(chat).await;
    let spec = RunSpec {
        chat_id: id.clone(), prompt: w.prompt.clone(), cwd: lib, add_dirs: vec![],
        // The spending limit is passed on only when the installed assistant supports it (`claude::start_run`).
        options: RunOptions { permission_mode: Some("acceptEdits".into()), max_budget_usd: Some(budget), ..Default::default() }, resume: None,
        append_system: Some(format!("This is the automated workflow \"{}\" that the user set up in the Thoughts app. Nobody is watching it run: \
            do not ask questions, do the work described, do not run git, and end with a short plain-language summary of what you did.", w.name)),
        extra_deny: vec![],
    };
    if let Err(e) = claude::start_run(state.clone(), spec, None).await {
        claude::push(state, &id, Msg::new("error", e.to_string())).await;
        return Err(e);
    }
    Ok(id)
}

/// Did the user stop this run themselves? (Then it is not a failure to retry.)
async fn stopped_by_user(state: &AppState, chat_id: &str) -> bool {
    state.chats.get(chat_id).await.map_or(true, |c| c.messages.last().map_or(false, |m| m.role == "system"))
}

/// Tell the app how a run it did not start by hand ended (or that one was skipped), so the user hears of it.
fn announce(state: &AppState, l: &Loaded, outcome: &str, chat_id: Option<&str>, retry: bool, at: Option<NaiveDateTime>) {
    state.emit(json!({"type": "workflow.result", "file": l.file, "hash": l.hash, "name": l.workflow.name, "outcome": outcome,
        "chatId": chat_id, "retry": retry, "at": at.map(local_iso)}));
}

fn money(usd: f64) -> String { format!("${usd:.2}") }

/// "Run now": start the workflow the user was shown (file `file`, content `hash`) once, as a chat
/// they can watch — allowed or not, since they asked for it. Same limits and rules as an
/// unattended run. Returns the chat id.
pub async fn run_now(state: &Arc<AppState>, file: &str, hash: &str) -> anyhow::Result<String> {
    let _g = PASS.lock().await;
    let (layer, home) = (crate::paths::app_layer_dir(), crate::paths::home());
    let l = find_seen(&layer, file, hash)?;
    let mut st = load_state(&home);
    migrate(&mut st, &load_files(&layer));
    if busy(state).await || st.get(&l.file).map_or(false, |s| s.chat_id.is_some()) { return Err(crate::tr::err("workflow.busy", &[])); }
    let budget = st.get(&l.file).map_or(DEFAULT_BUDGET_USD, Seen::budget);
    let id = start(state, &l.workflow, budget).await?;
    note_manual_start(st.entry(l.file.clone()).or_default(), &id, Local::now().naive_local());
    save_state(&home, &st);
    crate::log::info(format!("workflow \"{}\" ({}) started by hand", l.workflow.name, l.file));
    Ok(id)
}

/// The user's time and spending limits for the workflow in file `file`.
pub async fn set_limits(file: &str, minutes: u32, budget: f64) -> anyhow::Result<()> {
    check_limits(minutes, budget)?;
    let _g = PASS.lock().await;
    let (layer, home) = (crate::paths::app_layer_dir(), crate::paths::home());
    let l = find(&layer, file)?;
    let mut st = load_state(&home);
    migrate(&mut st, &load_files(&layer));
    let seen = st.entry(l.file).or_default();
    seen.minutes = Some(minutes);
    seen.budget_usd = Some((budget * 100.0).round() / 100.0);
    save_state(&home, &st);
    Ok(())
}

/// What a finished run's chat says about it: (its error text, cost, cut off by a restart, ended in an error).
async fn chat_facts(state: &AppState, chat: &str) -> (String, f64, bool, bool) {
    state.chats.with(chat, |c| {
        let error = c.messages.iter().rev().take(3).find(|m| m.role == "error").map(|m| m.text.clone()).unwrap_or_default();
        (error, c.cost_usd, c.interrupted, c.messages.last().map_or(false, |m| m.role == "error"))
    }).await.unwrap_or_default()
}

/// The runner: checks schedules twice a minute, listens for event triggers, starts one workflow at a time.
pub async fn run_loop(state: Arc<AppState>) {
    let layer = crate::paths::app_layer_dir();
    let home = crate::paths::home();
    let mut rx = state.events.subscribe();
    let mut queue: VecDeque<String> = approved_workflows(&layer, &home).into_iter()
        .filter(|l| l.workflow.trigger == Trigger::Event { on: "app.start".into() }).map(|l| l.file).collect();
    // Runs that ended since the last pass: (chat id, finished without an error).
    let mut finished: Vec<(String, bool)> = vec![];
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            ev = rx.recv() => match ev {
                Ok(e) if e["type"] == "wrap.done" => {
                    for l in approved_workflows(&layer, &home) {
                        if l.workflow.trigger == (Trigger::Event { on: "wrap.done".into() }) && !queue.contains(&l.file) { queue.push_back(l.file); }
                    }
                }
                Ok(e) if e["type"] == "chat.done" => {
                    let Some(id) = e["chatId"].as_str() else { continue };
                    finished.push((id.to_string(), e["ok"] == true));
                    if finished.len() > 200 { finished.remove(0); }
                    continue;
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            },
        }
        let _g = PASS.lock().await;
        // Only workflows the user approved, in exactly the form they approved, are ever started.
        prune_approved(&layer, &home);
        let loaded = load_files(&layer);
        let approved_list = load_approved(&home);
        let workflows: Vec<Loaded> = loaded.iter().filter(|l| is_approved(&approved_list, l)).cloned().collect();
        let now = Local::now().naive_local();
        let mut st = load_state(&home);
        let mut changed = migrate(&mut st, &loaded);
        for l in &loaded {
            let ok = is_approved(&approved_list, l);
            let seen = st.entry(l.file.clone()).or_default();
            // A new approval starts the schedule's count from now. (`None`: nothing recorded yet,
            // as in an older version's state — its first meeting, if any, is kept. `""`: not approved.)
            if ok && seen.approved_hash.as_deref() != Some(l.hash.as_str()) {
                if seen.approved_hash.is_some() || seen.first_seen.is_none() { seen.first_seen = Some(now); seen.retry_at = None; }
                seen.approved_hash = Some(l.hash.clone());
                changed = true;
            } else if !ok && seen.approved_hash.as_deref().map_or(true, |h| !h.is_empty()) {
                seen.approved_hash = Some(String::new());
                seen.retry_at = None;
                changed = true;
            }
        }
        let before = st.len();
        st.retain(|file, s| s.chat_id.is_some() || loaded.iter().any(|l| &l.file == file));
        changed |= st.len() != before;
        // Settle the runs that ended, and stop the ones that went past their time limit.
        for (file, seen) in st.iter_mut() {
            let Some(chat) = seen.chat_id.clone() else { continue };
            let running = state.chats.with(&chat, |c| c.running).await.unwrap_or(false);
            if running {
                if over_time(seen, now) && !limited(&chat) {
                    LIMITED.lock().unwrap_or_else(|e| e.into_inner()).push(chat.clone());
                    crate::log::warn(format!("workflow {file}: stopped after its time limit of {} minutes", seen.minutes()));
                    claude::push(&state, &chat, Msg::new("error", crate::tr::text("workflow.time_limit", &[("minutes", &seen.minutes().to_string())]))).await;
                    if let Some(k) = state.runs.lock().await.remove(&chat) { let _ = k.send(()); }
                }
                continue;
            }
            let (error, cost, interrupted, ends_in_error) = chat_facts(&state, &chat).await;
            // No word of it and it is not running: the daemon restarted under it, or the event was missed.
            let ok = finished.iter().find(|(id, _)| *id == chat).map_or(!ends_in_error && !interrupted, |(_, ok)| *ok);
            let by_limit = { let mut l = LIMITED.lock().unwrap_or_else(|e| e.into_inner()); let was = l.iter().any(|c| *c == chat); l.retain(|c| *c != chat); was };
            let by_user = !ok && stopped_by_user(&state, &chat).await;
            let out = outcome(ok, by_user, by_limit, interrupted, cost, seen.budget(), &error);
            if out == "spendLimit" {
                claude::push(&state, &chat, Msg::new("error", crate::tr::text("workflow.spend_limit", &[("amount", &money(seen.budget()))]))).await;
            }
            let manual = seen.manual;
            settle(seen, out, cost, now);
            if out == "failed" && seen.retry_at.is_some() { crate::log::warn(format!("workflow {file} failed; it will be tried once more in {RETRY_MINUTES} minutes")); }
            else if out != "ok" { crate::log::warn(format!("workflow {file} ended: {out}")); }
            if out != "ok" && out != "stopped" && !manual {
                if let Some(l) = loaded.iter().find(|l| &l.file == file) { announce(&state, l, out, Some(&chat), seen.retry_at.is_some(), Some(now)); }
            }
            changed = true;
        }
        finished.clear();
        // Scheduled times missed for good: say so once, so a skipped month-end task is not invisible.
        for l in &workflows {
            let Some(seen) = st.get_mut(&l.file) else { continue };
            if let Some(slot) = newly_skipped(&l.workflow, seen, now) {
                seen.skipped = Some(slot);
                changed = true;
                crate::log::warn(format!("workflow {}: the run of {slot} was missed and is too late to catch up", l.file));
                announce(&state, l, "skipped", None, false, Some(slot));
            }
        }
        for file in due(&workflows, &st, now) { if !queue.contains(&file) { queue.push_back(file); } }
        queue.retain(|file| workflows.iter().any(|l| &l.file == file));
        if !queue.is_empty() && !busy(&state).await {
            if let Some(l) = queue.pop_front().and_then(|file| workflows.iter().find(|l| l.file == file)) {
                // Recorded before the run, so a workflow that fails is not retried in a loop.
                let seen = st.entry(l.file.clone()).or_default();
                note_start(seen, now);
                changed = true;
                // Nobody watches an automatic task, so it never starts past the month's spending limit.
                // It counts as missed, not failed: it is not tried again, and the next one comes as scheduled.
                if crate::spend::reached(&state).await {
                    crate::log::warn(format!("workflow {}: not started, this month's spending limit is reached", l.file));
                    seen.chat_id = None;
                    settle(seen, "monthLimit", 0.0, now);
                    announce(&state, l, "monthLimit", None, false, Some(now));
                } else {
                match start(&state, &l.workflow, seen.budget()).await {
                    Ok(id) => seen.chat_id = Some(id),
                    Err(e) => {
                        crate::log::warn(format!("workflow \"{}\" could not start: {e}", l.workflow.name));
                        seen.chat_id = None;
                        settle(seen, "notStarted", 0.0, now);
                        announce(&state, l, "notStarted", None, seen.retry_at.is_some(), Some(now));
                    }
                }
                }
            }
        }
        if changed {
            save_state(&home, &st);
            state.emit(json!({"type": "workflows.changed"}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime { NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap() }
    fn sched(every: &str, t: &str, weekday: Option<u32>, day: Option<Value>, days: Option<Vec<u32>>) -> Trigger { Trigger::Schedule { every: every.into(), at: t.into(), weekday, day, days } }
    fn daily(t: &str) -> Trigger { sched("day", t, None, None, None) }
    fn weekly(t: &str, wd: u32) -> Trigger { sched("week", t, Some(wd), None, None) }
    fn monthly(t: &str, day: Value) -> Trigger { sched("month", t, None, Some(day), None) }
    fn wf(trigger: Trigger) -> Workflow { Workflow { name: "W".into(), trigger, prompt: "x".into() } }
    /// A loaded workflow whose file name is its name, so state keys read naturally in tests.
    fn ld(w: &Workflow) -> Loaded { Loaded { file: w.name.clone(), hash: "h".into(), workflow: w.clone(), duplicate_name: false } }

    #[test]
    fn next_run_daily_and_weekly() {
        // 2026-11-10 is a Tuesday.
        assert_eq!(next_run(&daily("09:00"), at(2026, 11, 10, 8, 0)), Some(at(2026, 11, 10, 9, 0)));
        assert_eq!(next_run(&daily("09:00"), at(2026, 11, 10, 9, 0)), Some(at(2026, 11, 11, 9, 0)), "strictly after");
        assert_eq!(next_run(&daily("23:59"), at(2026, 12, 31, 23, 59)), Some(at(2027, 1, 1, 23, 59)));
        assert_eq!(next_run(&weekly("16:00", 5), at(2026, 11, 10, 8, 0)), Some(at(2026, 11, 13, 16, 0)), "Friday");
        assert_eq!(next_run(&weekly("16:00", 2), at(2026, 11, 10, 17, 0)), Some(at(2026, 11, 17, 16, 0)), "same weekday, time passed");
        assert_eq!(next_run(&weekly("16:00", 0), at(2026, 11, 10, 8, 0)), Some(at(2026, 11, 15, 16, 0)), "Sunday is 0");
        assert_eq!(next_run(&Trigger::Event { on: "wrap.done".into() }, at(2026, 11, 10, 8, 0)), None);
        assert_eq!(next_run(&daily("25:00"), at(2026, 11, 10, 8, 0)), None);
    }

    #[test]
    fn due_counts_from_last_run_or_first_seen() {
        let w = Workflow { name: "Morning".into(), trigger: daily("09:00"), prompt: "x".into() };
        let ws = [ld(&w)];
        let mut st = State::new();
        // Unknown workflow: never due immediately.
        assert!(due(&ws, &st, at(2026, 11, 10, 12, 0)).is_empty());
        st.insert("Morning".into(), Seen { first_seen: Some(at(2026, 11, 10, 12, 0)), ..Default::default() });
        assert!(due(&ws, &st, at(2026, 11, 10, 18, 0)).is_empty(), "09:00 had already passed when it was created");
        assert_eq!(due(&ws, &st, at(2026, 11, 11, 9, 0)), ["Morning"]);
        // After a week off it runs once (for today's 09:00, an hour ago), then waits for tomorrow.
        assert_eq!(due(&ws, &st, at(2026, 11, 18, 10, 0)), ["Morning"]);
        note_start(st.get_mut("Morning").unwrap(), at(2026, 11, 18, 10, 0));
        assert!(due(&ws, &st, at(2026, 11, 18, 23, 0)).is_empty());
        assert_eq!(due_at(&w, st.get("Morning"), at(2026, 11, 18, 23, 0)), Some(at(2026, 11, 19, 9, 0)));
    }

    #[test]
    fn month_and_weekday_schedules() {
        // Day of the month.
        assert_eq!(next_run(&monthly("08:00", json!(1)), at(2026, 11, 10, 8, 0)), Some(at(2026, 12, 1, 8, 0)));
        assert_eq!(next_run(&monthly("08:00", json!(1)), at(2026, 12, 1, 8, 0)), Some(at(2027, 1, 1, 8, 0)), "strictly after, across the year");
        assert_eq!(next_run(&monthly("08:00", json!(28)), at(2026, 2, 1, 0, 0)), Some(at(2026, 2, 28, 8, 0)));
        assert_eq!(next_run(&monthly("08:00", json!(10)), at(2026, 11, 10, 7, 59)), Some(at(2026, 11, 10, 8, 0)));
        // "last" follows the length of each month, leap years included.
        let last = monthly("18:00", json!("last"));
        assert_eq!(next_run(&last, at(2026, 2, 1, 0, 0)), Some(at(2026, 2, 28, 18, 0)));
        assert_eq!(next_run(&last, at(2028, 2, 1, 0, 0)), Some(at(2028, 2, 29, 18, 0)), "leap year");
        assert_eq!(next_run(&last, at(2026, 4, 30, 18, 0)), Some(at(2026, 5, 31, 18, 0)), "30-day month, then a 31-day one");
        assert_eq!(next_run(&last, at(2026, 12, 31, 17, 0)), Some(at(2026, 12, 31, 18, 0)));
        assert_eq!(next_run(&last, at(2026, 12, 31, 18, 0)), Some(at(2027, 1, 31, 18, 0)));
        assert_eq!(prev_run(&last, at(2026, 3, 1, 9, 0)), Some(at(2026, 2, 28, 18, 0)));
        assert_eq!(prev_run(&monthly("08:00", json!(1)), at(2026, 11, 10, 8, 0)), Some(at(2026, 11, 1, 8, 0)));
        // Weekdays: Monday–Friday unless the file lists its own days. 2026-11-13 is a Friday.
        let wd = sched("weekdays", "07:30", None, None, None);
        assert_eq!(next_run(&wd, at(2026, 11, 13, 7, 30)), Some(at(2026, 11, 16, 7, 30)), "skips the weekend");
        assert_eq!(next_run(&wd, at(2026, 11, 12, 9, 0)), Some(at(2026, 11, 13, 7, 30)));
        let sun_thu = sched("weekdays", "07:30", None, None, Some(vec![0, 1, 2, 3, 4]));
        assert_eq!(next_run(&sun_thu, at(2026, 11, 12, 7, 30)), Some(at(2026, 11, 15, 7, 30)), "Thursday → Sunday");
        assert_eq!(prev_run(&wd, at(2026, 11, 15, 12, 0)), Some(at(2026, 11, 13, 7, 30)));
        // What the runner accepts.
        for ok in [monthly("08:00", json!(1)), monthly("08:00", json!(28)), last.clone(), wd.clone(), sun_thu.clone()] { assert!(valid(&wf(ok))); }
        for bad in [monthly("08:00", json!(0)), monthly("08:00", json!(29)), monthly("08:00", json!(31)), monthly("08:00", json!("first")), monthly("08:00", json!(1.5)),
                    sched("month", "08:00", None, None, None), sched("weekdays", "07:30", None, None, Some(vec![])), sched("weekdays", "07:30", None, None, Some(vec![7])),
                    sched("year", "08:00", None, None, None)] {
            assert!(!valid(&wf(bad.clone())), "{bad:?}");
        }
        let parsed: Workflow = serde_json::from_str(r#"{"name":"Invoices","trigger":{"type":"schedule","every":"month","at":"09:00","day":"last"},"prompt":"x"}"#).unwrap();
        assert!(valid(&parsed));
        assert_eq!(serde_json::to_value(&parsed.trigger).unwrap(), json!({"type": "schedule", "every": "month", "at": "09:00", "day": "last"}));
    }

    #[test]
    fn a_missed_run_is_caught_up_once_within_a_day() {
        let seen = |first: NaiveDateTime, last: Option<NaiveDateTime>| Seen { first_seen: Some(first), last_run: last, ..Default::default() };
        // Daily at 09:00, last run on the 10th. The laptop sleeps through the 11th to 14th.
        let w = wf(daily("09:00"));
        let s = seen(at(2026, 11, 1, 0, 0), Some(at(2026, 11, 10, 9, 0)));
        assert_eq!(missed(&w, Some(&s), at(2026, 11, 14, 13, 0)), Some(at(2026, 11, 14, 9, 0)), "one catch-up, for the latest time only");
        // Monthly on the last day at 18:00.
        let m = wf(monthly("18:00", json!("last")));
        let s = seen(at(2026, 1, 15, 0, 0), Some(at(2026, 1, 31, 18, 0)));
        assert_eq!(missed(&m, Some(&s), at(2026, 2, 28, 17, 59)), None, "not yet");
        assert_eq!(missed(&m, Some(&s), at(2026, 2, 28, 18, 0)), Some(at(2026, 2, 28, 18, 0)));
        assert_eq!(missed(&m, Some(&s), at(2026, 3, 1, 17, 0)), Some(at(2026, 2, 28, 18, 0)), "opened the next morning");
        assert_eq!(missed(&m, Some(&s), at(2026, 3, 2, 9, 0)), Some(at(2026, 2, 28, 18, 0)), "a month end missed over a weekend still runs on Monday");
        assert_eq!(newly_skipped(&m.clone(), &s, at(2026, 3, 2, 9, 0)), None, "…so it is not skipped");
        assert_eq!(missed(&m, Some(&s), at(2026, 3, 5, 18, 1)), None, "more than five days late: skipped");
        assert_eq!(newly_skipped(&m, &s, at(2026, 3, 5, 18, 1)), Some(at(2026, 2, 28, 18, 0)), "and the user is told");
        let told = Seen { skipped: Some(at(2026, 2, 28, 18, 0)), ..s.clone() };
        assert_eq!(newly_skipped(&m, &told, at(2026, 3, 6, 9, 0)), None, "only once");
        assert_eq!(due_at(&m, Some(&s), at(2026, 3, 5, 18, 1)), Some(at(2026, 3, 31, 18, 0)), "waits for the next month end");
        // Weekly: Friday's review, opened on Monday morning, still runs; a week later it is skipped.
        let fri = wf(weekly("16:00", 5));
        let s5 = seen(at(2026, 11, 1, 0, 0), Some(at(2026, 11, 6, 16, 0)));
        assert_eq!(missed(&fri, Some(&s5), at(2026, 11, 16, 9, 0)), Some(at(2026, 11, 13, 16, 0)));
        assert_eq!(missed(&fri, Some(&s5), at(2026, 11, 17, 9, 0)), None);
        assert!(newly_skipped(&fri, &s5, at(2026, 11, 17, 9, 0)).is_some());
        // Once it has run for that time it is not due again.
        let ran = seen(at(2026, 1, 15, 0, 0), Some(at(2026, 3, 1, 17, 0)));
        assert_eq!(missed(&m, Some(&ran), at(2026, 3, 1, 17, 30)), None);
        // Weekdays: Friday's run missed, opened on Saturday morning → caught up; on Sunday → skipped.
        let wd = wf(sched("weekdays", "17:00", None, None, None));
        let s = seen(at(2026, 11, 1, 0, 0), Some(at(2026, 11, 12, 17, 0)));
        assert!(missed(&wd, Some(&s), at(2026, 11, 14, 10, 0)).is_some());
        assert!(missed(&wd, Some(&s), at(2026, 11, 15, 10, 0)).is_none());
        // A run in progress is never reported as skipped.
        let busy = Seen { chat_id: Some("c".into()), ..s.clone() };
        assert!(newly_skipped(&wd, &s, at(2026, 11, 15, 10, 0)).is_some() && newly_skipped(&wd, &busy, at(2026, 11, 15, 10, 0)).is_none());
        // A workflow the runner has never met, or an event one, is never "missed".
        assert_eq!(missed(&w, None, at(2026, 11, 14, 13, 0)), None);
        assert_eq!(missed(&wf(Trigger::Event { on: "app.start".into() }), Some(&s), at(2026, 11, 14, 13, 0)), None);
    }

    #[test]
    fn a_failed_run_is_retried_once_after_ten_minutes() {
        let w = wf(daily("09:00"));
        let mut st = State::new();
        st.insert("W".into(), Seen { first_seen: Some(at(2026, 11, 1, 0, 0)), ..Default::default() });
        let t0 = at(2026, 11, 10, 9, 0);
        assert_eq!(due(&[ld(&w)], &st, t0), ["W"]);
        let seen = st.get_mut("W").unwrap();
        note_start(seen, t0);
        seen.chat_id = Some("c1".into());
        note_result(seen, false, at(2026, 11, 10, 9, 2));
        assert_eq!((seen.retry_at, seen.retried, seen.chat_id.clone()), (Some(at(2026, 11, 10, 9, 12)), true, None));
        assert!(due(&[ld(&w)], &st, at(2026, 11, 10, 9, 11)).is_empty(), "not before ten minutes have passed");
        assert_eq!(due_at(&w, st.get("W"), at(2026, 11, 10, 9, 11)), Some(at(2026, 11, 10, 9, 12)));
        assert_eq!(due(&[ld(&w)], &st, at(2026, 11, 10, 9, 12)), ["W"]);
        // The retry fails too: no third attempt, the next run is tomorrow's.
        let seen = st.get_mut("W").unwrap();
        note_start(seen, at(2026, 11, 10, 9, 12));
        assert!(seen.retried && seen.retry_at.is_none());
        note_result(seen, false, at(2026, 11, 10, 9, 14));
        assert_eq!(seen.retry_at, None);
        assert!(due(&[ld(&w)], &st, at(2026, 11, 10, 12, 0)).is_empty());
        assert_eq!(due_at(&w, st.get("W"), at(2026, 11, 10, 12, 0)), Some(at(2026, 11, 11, 9, 0)));
        // Tomorrow's run starts with a clean slate and, if it fails, earns its own retry.
        let seen = st.get_mut("W").unwrap();
        note_start(seen, at(2026, 11, 11, 9, 0));
        assert!(!seen.retried);
        note_result(seen, true, at(2026, 11, 11, 9, 5));
        assert_eq!(seen.retry_at, None);
    }

    fn tmp(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let d = std::env::temp_dir().join(format!("thoughts-wf-{name}-{}", uuid::Uuid::new_v4()));
        let (layer, home) = (d.join("layer"), d.join("home"));
        std::fs::create_dir_all(layer.join("workflows")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        (d, layer, home)
    }
    fn hash_of(layer: &Path, file: &str) -> String { load_files(layer).into_iter().find(|l| l.file == file).unwrap().hash }

    #[test]
    fn only_approved_content_runs() {
        let (d, layer, home) = tmp("approve");
        let file = layer.join("workflows/weekly.json");
        let body = |prompt: &str| format!(r#"{{"name":"Weekly review","trigger":{{"type":"event","on":"wrap.done"}},"prompt":"{prompt}"}}"#);
        std::fs::write(&file, body("Review my week.")).unwrap();
        std::fs::write(layer.join("workflows/other.json"), r#"{"trigger":{"type":"event","on":"app.start"},"prompt":"Tidy."}"#).unwrap();
        let names = || approved_workflows(&layer, &home).into_iter().map(|l| l.workflow.name).collect::<Vec<_>>();
        let flag = |file: &str| status(&layer, &home, at(2026, 11, 10, 8, 0)).into_iter().find(|s| s["file"] == file).unwrap()["approved"].clone();

        // Nothing runs until the user says so.
        assert!(names().is_empty());
        assert_eq!(flag("weekly.json"), false);
        let shown = hash_of(&layer, "weekly.json");
        approve(&layer, &home, "weekly.json", &shown).unwrap();
        assert_eq!(names(), ["Weekly review"]);
        assert_eq!(flag("weekly.json"), true);
        assert_eq!(flag("other.json"), false);
        assert_eq!(load_approved(&home).keys().collect::<Vec<_>>(), ["weekly.json"]);

        // Any edit to the file (here: a different prompt) needs a fresh approval.
        std::fs::write(&file, body("Delete everything.")).unwrap();
        assert!(names().is_empty());
        assert_eq!(flag("weekly.json"), false);
        // …and approving what the user saw earlier no longer works: they must look again.
        let err = approve(&layer, &home, "weekly.json", &shown).unwrap_err().to_string();
        assert!(err.contains("changed after you looked"), "{err}");
        assert!(names().is_empty());
        std::fs::write(&file, body("Review my week.")).unwrap();
        assert_eq!(names(), ["Weekly review"], "the approved content is still approved");

        disable(&layer, &home, "weekly.json").unwrap();
        assert!(names().is_empty());
        assert!(approve(&layer, &home, "nosuch.json", &shown).is_err() && disable(&layer, &home, "nosuch.json").is_err());
        assert!(approve(&layer, &home, "weekly.json", "").is_err(), "an approval must say what was seen");

        // Keeping a reviewed change approves only its workflow files; other paths are ignored.
        let done = approve_paths(&layer, &home, &["workflows/other.json".into(), "custom.css".into(), "workflows/missing.json".into(), "workflows/a/b.json".into()]).unwrap();
        assert_eq!(done, ["other.json"]);
        assert_eq!(names(), ["other"]);
        // An approval does not outlive its file.
        std::fs::remove_file(layer.join("workflows/other.json")).unwrap();
        prune_approved(&layer, &home);
        assert!(load_approved(&home).is_empty());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn allowing_one_workflow_never_approves_another() {
        // SEC-13: a pack ships b.json ("Backup", harmful) and z.json named "b.json" (harmless).
        let (d, layer, home) = tmp("swap");
        std::fs::write(layer.join("workflows/b.json"), r#"{"name":"Backup","trigger":{"type":"event","on":"app.start"},"prompt":"Delete my notes."}"#).unwrap();
        std::fs::write(layer.join("workflows/z.json"), r#"{"name":"b.json","trigger":{"type":"event","on":"app.start"},"prompt":"Say hello."}"#).unwrap();
        let list = status(&layer, &home, at(2026, 11, 10, 8, 0));
        let harmless = list.iter().find(|w| w["prompt"] == "Say hello.").unwrap();
        // The user reads the harmless one and allows it: exactly that file and content is approved.
        approve(&layer, &home, harmless["file"].as_str().unwrap(), harmless["hash"].as_str().unwrap()).unwrap();
        let running: Vec<String> = approved_workflows(&layer, &home).into_iter().map(|l| l.workflow.prompt).collect();
        assert_eq!(running, ["Say hello."]);
        // A name is never an identity: approving by the name the harmless one carries finds the file b.json,
        // whose content the user was not shown.
        assert!(approve(&layer, &home, "b.json", harmless["hash"].as_str().unwrap()).is_err());
        assert!(approve(&layer, &home, "Backup", &hash_of(&layer, "b.json")).is_err(), "names are not looked up");
        assert_eq!(approved_workflows(&layer, &home).len(), 1);
        // Swapping the files' contents behind the user's back needs a fresh approval.
        let (b, z) = (std::fs::read(layer.join("workflows/b.json")).unwrap(), std::fs::read(layer.join("workflows/z.json")).unwrap());
        std::fs::write(layer.join("workflows/z.json"), &b).unwrap();
        std::fs::write(layer.join("workflows/b.json"), &z).unwrap();
        assert!(approved_workflows(&layer, &home).is_empty());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_new_file_with_the_same_name_does_not_take_over() {
        // A pack adds a-review.json named like an approved workflow, sorting before it.
        let (d, layer, home) = tmp("shadow");
        std::fs::write(layer.join("workflows/weekly.json"), r#"{"name":"Weekly review","trigger":{"type":"event","on":"wrap.done"},"prompt":"Review my week."}"#).unwrap();
        approve(&layer, &home, "weekly.json", &hash_of(&layer, "weekly.json")).unwrap();
        let mut st = State::new();
        st.insert("weekly.json".into(), Seen { last_run: Some(at(2026, 11, 9, 8, 0)), ..Default::default() });
        save_state(&home, &st);
        std::fs::write(layer.join("workflows/a-review.json"), r#"{"name":"Weekly review","trigger":{"type":"event","on":"wrap.done"},"prompt":"Send my notes away."}"#).unwrap();
        // The approved one keeps running, and keeps its own history; the newcomer is listed apart, not approved.
        assert_eq!(approved_workflows(&layer, &home).into_iter().map(|l| l.file).collect::<Vec<_>>(), ["weekly.json"]);
        let list = status(&layer, &home, at(2026, 11, 10, 8, 0));
        assert_eq!(list.len(), 2);
        let get = |f: &str| list.iter().find(|w| w["file"] == f).unwrap().clone();
        assert_eq!((get("weekly.json")["approved"].clone(), get("a-review.json")["approved"].clone()), (json!(true), json!(false)));
        assert!(get("weekly.json")["duplicateName"] == true && get("a-review.json")["duplicateName"] == true);
        assert!(get("weekly.json")["lastRun"].is_string() && get("a-review.json")["lastRun"].is_null());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn old_state_by_name_moves_to_the_file() {
        let w = |file: &str, name: &str| Loaded { file: file.into(), hash: "h".into(), duplicate_name: false,
            workflow: Workflow { name: name.into(), trigger: daily("09:00"), prompt: "x".into() } };
        let loaded = [w("a.json", "Morning"), w("b.json", "Morning"), w("c.json", "Evening")];
        let mut st = State::new();
        st.insert("Morning".into(), Seen { last_run: Some(at(2026, 11, 9, 9, 0)), ..Default::default() });
        st.insert("c.json".into(), Seen::default());
        assert!(migrate(&mut st, &loaded));
        assert_eq!(st.keys().collect::<Vec<_>>(), ["a.json", "c.json"], "to the file the old runner used (the first)");
        assert!(st["a.json"].last_run.is_some());
        assert!(!migrate(&mut st, &loaded), "nothing more to move");
    }

    #[test]
    fn how_a_run_ended() {
        // (ok, stopped by the user, stopped at its time limit, cut off by a restart, spent, budget, error)
        assert_eq!(outcome(true, false, false, false, 0.5, 2.0, ""), "ok");
        assert_eq!(outcome(false, false, false, false, 0.5, 2.0, "API Error: 500"), "failed");
        assert_eq!(outcome(false, true, false, false, 0.5, 2.0, ""), "stopped");
        assert_eq!(outcome(false, true, true, false, 0.5, 2.0, ""), "timeLimit", "the runner's stop is not the user's");
        assert_eq!(outcome(false, false, false, false, 0.4, 2.0, "Run ended: error_max_budget_usd"), "spendLimit");
        assert_eq!(outcome(false, false, false, false, 2.01, 2.0, "Run ended: error"), "spendLimit");
        assert_eq!(outcome(false, false, false, true, 0.1, 2.0, ""), "interrupted");
        // Only a scheduled run that simply failed (or could not start) is retried.
        let now = at(2026, 11, 10, 9, 0);
        for (out, retried) in [("failed", true), ("notStarted", true), ("timeLimit", false), ("spendLimit", false), ("stopped", false), ("interrupted", false), ("ok", false)] {
            let mut s = Seen::default();
            note_start(&mut s, now);
            s.chat_id = Some("c".into());
            settle(&mut s, out, 0.3, at(2026, 11, 10, 9, 5));
            assert_eq!(s.retry_at.is_some(), retried, "{out}");
            assert_eq!(s.chat_id, None);
            assert_eq!(s.results[0], RunResult { started: Some(now), ended: Some(at(2026, 11, 10, 9, 5)), outcome: out.into(), chat_id: Some("c".into()), manual: false, cost_usd: 0.3 });
        }
    }

    #[test]
    fn a_run_held_back_by_the_spending_limit_is_missed_not_failed() {
        let mut seen = Seen::default();
        let now = at(2026, 3, 2, 9, 0);
        note_start(&mut seen, now);
        settle(&mut seen, "monthLimit", 0.0, now);
        assert_eq!(seen.results[0].outcome, "monthLimit");
        assert!(seen.retry_at.is_none(), "it is not tried again in ten minutes: the limit still stands");
        assert!(seen.chat_id.is_none());
        assert_eq!(seen.last_run, Some(now), "the schedule moves on to its next time");
    }

    #[test]
    fn run_now_is_apart_from_the_schedule() {
        let w = wf(daily("09:00"));
        let mut st = State::new();
        st.insert("W".into(), Seen { first_seen: Some(at(2026, 11, 1, 0, 0)), last_run: Some(at(2026, 11, 9, 9, 0)), ..Default::default() });
        let s = st.get_mut("W").unwrap();
        note_manual_start(s, "m1", at(2026, 11, 10, 8, 0));
        // While it runs, the scheduled run waits (one run of a workflow at a time)…
        assert!(due(&[ld(&w)], &st, at(2026, 11, 10, 9, 0)).is_empty());
        let s = st.get_mut("W").unwrap();
        settle(s, "failed", 0.1, at(2026, 11, 10, 9, 1));
        // …and a failed run by hand is not retried, nor does it count as today's scheduled run.
        assert_eq!((s.retry_at, s.last_run, s.manual), (None, Some(at(2026, 11, 9, 9, 0)), false));
        assert!(s.results[0].manual);
        assert_eq!(due(&[ld(&w)], &st, at(2026, 11, 10, 9, 2)), ["W"]);
    }

    #[test]
    fn limits_time_and_spending() {
        let mut s = Seen::default();
        assert_eq!((s.minutes(), s.budget()), (DEFAULT_MINUTES, DEFAULT_BUDGET_USD));
        let t0 = at(2026, 11, 10, 9, 0);
        note_start(&mut s, t0);
        assert!(!over_time(&s, t0 + Duration::minutes(30)), "nothing is running");
        s.chat_id = Some("c".into());
        assert!(!over_time(&s, t0 + Duration::minutes(19)));
        assert!(over_time(&s, t0 + Duration::minutes(20)));
        s.minutes = Some(45);
        assert!(!over_time(&s, t0 + Duration::minutes(44)) && over_time(&s, t0 + Duration::minutes(45)));
        assert!(check_limits(1, 0.1).is_ok() && check_limits(240, 100.0).is_ok());
        for (m, b) in [(0, 1.0), (241, 1.0), (10, 0.05), (10, 101.0), (10, f64::NAN), (10, f64::INFINITY)] { assert!(check_limits(m, b).is_err(), "{m} {b}"); }
        // Only the last few runs are remembered.
        for i in 0..15 { record(&mut s, RunResult { outcome: format!("r{i}"), ..Default::default() }); }
        assert_eq!(s.results.len(), HISTORY);
        assert_eq!(s.results[0].outcome, "r14");
    }

    #[test]
    fn loads_only_valid_definitions() {
        let d = std::env::temp_dir().join(format!("thoughts-wf-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("workflows")).unwrap();
        let w = |name: &str, body: &str| std::fs::write(d.join("workflows").join(name), body).unwrap();
        w("a-weekly.json", r#"{"name":"Weekly review","trigger":{"type":"schedule","every":"week","at":"16:00","weekday":5},"prompt":"Review my week."}"#);
        w("b-after-wrap.json", r#"{"trigger":{"type":"event","on":"wrap.done"},"prompt":"Tidy."}"#);
        w("c-bad-time.json", r#"{"name":"Bad","trigger":{"type":"schedule","every":"day","at":"9am"},"prompt":"x"}"#);
        w("d-bad-event.json", r#"{"name":"Bad2","trigger":{"type":"event","on":"chat.message"},"prompt":"x"}"#);
        w("e-dup.json", r#"{"name":"Weekly review","trigger":{"type":"event","on":"app.start"},"prompt":"x"}"#);
        w("f-empty.json", r#"{"name":"Empty","trigger":{"type":"event","on":"app.start"},"prompt":" "}"#);
        w("notes.txt", "not a workflow");
        let ws = load_files(&d);
        assert_eq!(ws.iter().map(|l| (l.file.as_str(), l.workflow.name.as_str(), l.duplicate_name)).collect::<Vec<_>>(),
            [("a-weekly.json", "Weekly review", true), ("b-after-wrap.json", "b-after-wrap", false), ("e-dup.json", "Weekly review", true)]);
        // Not approved yet: listed, but with no next run.
        assert!(status(&d, &d, at(2026, 11, 10, 8, 0))[0]["nextRun"].is_null());
        approve(&d, &d, "a-weekly.json", &hash_of(&d, "a-weekly.json")).unwrap();
        let s = status(&d, &d, at(2026, 11, 10, 8, 0));
        assert_eq!(s[0]["trigger"]["weekday"], 5);
        assert!(s[0]["nextRun"].as_str().unwrap().starts_with("2026-11-13T16:00"));
        assert!(s[0]["lastRun"].is_null() && s[1]["nextRun"].is_null());
        assert_eq!(s[0]["limits"], json!({"minutes": DEFAULT_MINUTES, "budgetUsd": DEFAULT_BUDGET_USD}));
        assert!(s[0]["last"].is_null() && s[0]["history"] == json!([]) && s[0]["running"] == false);
        std::fs::remove_dir_all(d).unwrap();
    }
}
