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
#[derive(Clone, Debug)]
pub struct Loaded {
    /// File name inside `workflows/`, e.g. `weekly-review.json`.
    pub file: String,
    /// sha256 of the file's bytes: approval is given to exactly this content.
    pub hash: String,
    pub workflow: Workflow,
}

pub fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// Valid workflows in `<layer>/workflows/*.json`, by name (first file wins on a duplicate name).
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
        if valid(&w) && !out.iter().any(|x| x.workflow.name == w.name) { out.push(Loaded { file, hash: content_hash(&bytes), workflow: w }); }
    }
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
pub fn approved_workflows(layer: &Path, home: &Path) -> Vec<Workflow> {
    let a = load_approved(home);
    load_files(layer).into_iter().filter(|l| is_approved(&a, l)).map(|l| l.workflow).collect()
}

fn find(layer: &Path, name: &str) -> anyhow::Result<Loaded> {
    load_files(layer).into_iter().find(|l| l.workflow.name == name || l.file == name)
        .ok_or_else(|| anyhow::anyhow!("no such workflow"))
}

/// The user allows workflow `name` (as it is right now) to run on its own.
pub fn approve(layer: &Path, home: &Path, name: &str) -> anyhow::Result<()> {
    let l = find(layer, name)?;
    let mut a = load_approved(home);
    a.insert(l.file, l.hash);
    save_approved(home, &a)
}

/// Stop workflow `name` from running (its file stays; approving it again re-enables it).
pub fn disable(layer: &Path, home: &Path, name: &str) -> anyhow::Result<()> {
    let l = find(layer, name)?;
    let mut a = load_approved(home);
    if a.remove(&l.file).is_some() { save_approved(home, &a)?; }
    Ok(())
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

/// A run missed while the app was closed or the computer slept is caught up only this long after its time.
pub const CATCH_UP_HOURS: i64 = 24;
/// A failed run is tried once more after this many minutes.
pub const RETRY_MINUTES: i64 = 10;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Seen {
    pub last_run: Option<NaiveDateTime>,
    /// When the runner first met this workflow: a schedule counts from here, so a new workflow
    /// never fires for a time that had already passed when it was created.
    pub first_seen: Option<NaiveDateTime>,
    /// The chat of the run that is in progress (or was, when the daemon last looked).
    pub chat_id: Option<String>,
    /// A failed run is tried again at this time…
    pub retry_at: Option<NaiveDateTime>,
    /// …but only once per run.
    pub retried: bool,
}

pub type State = BTreeMap<String, Seen>;

fn state_file(home: &Path) -> PathBuf { home.join("workflows-state.json") }

pub fn load_state(home: &Path) -> State {
    std::fs::read_to_string(state_file(home)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_state(home: &Path, s: &State) {
    if let Ok(t) = serde_json::to_string_pretty(s) { let _ = crate::paths::write_atomic(&state_file(home), t.as_bytes(), true); }
}

/// The scheduled time `w` has to run for right now, if any: the latest time its schedule fired,
/// when that is after its last run (or after the runner first met it) and no more than a day ago.
/// However long the app was closed this is at most one run — older missed times are skipped.
pub fn missed(w: &Workflow, seen: Option<&Seen>, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let base = seen.and_then(|s| s.last_run.or(s.first_seen))?;
    let slot = prev_run(&w.trigger, now)?;
    (slot > base && now - slot <= Duration::hours(CATCH_UP_HOURS)).then_some(slot)
}

/// When `w` should next run given what the runner remembers about it.
pub fn due_at(w: &Workflow, seen: Option<&Seen>, now: NaiveDateTime) -> Option<NaiveDateTime> {
    if let Some(t) = seen.and_then(|s| s.retry_at) { return Some(t); }
    missed(w, seen, now).or_else(|| next_run(&w.trigger, now))
}

/// Names of schedule workflows that are due at `now`, and of failed ones whose retry time has come.
pub fn due(workflows: &[Workflow], state: &State, now: NaiveDateTime) -> Vec<String> {
    workflows.iter().filter(|w| {
        let seen = state.get(&w.name);
        missed(w, seen, now).is_some() || seen.and_then(|s| s.retry_at).map_or(false, |t| t <= now)
    }).map(|w| w.name.clone()).collect()
}

/// A run of `seen`'s workflow ended. A failure earns one retry; a success (or a second failure) clears it.
pub fn note_result(seen: &mut Seen, ok: bool, now: NaiveDateTime) {
    seen.chat_id = None;
    if !ok && !seen.retried {
        seen.retried = true;
        seen.retry_at = Some(now + Duration::minutes(RETRY_MINUTES));
    } else {
        seen.retry_at = None;
    }
}

/// A run of `seen`'s workflow is starting at `now`. A retry keeps its mark, so it is not retried again.
pub fn note_start(seen: &mut Seen, now: NaiveDateTime) {
    let retry = seen.retry_at.map_or(false, |t| t <= now);
    seen.retried = retry;
    seen.retry_at = None;
    seen.last_run = Some(now);
}

fn local_iso(t: NaiveDateTime) -> Value {
    json!(t.and_local_timezone(Local).earliest().map(|d| d.to_rfc3339()))
}

/// `[{name, file, trigger, prompt, approved, lastRun, nextRun}]` — `nextRun` only for approved ones.
pub fn status(layer: &Path, home: &Path, now: NaiveDateTime) -> Vec<Value> {
    let state = load_state(home);
    let approved = load_approved(home);
    load_files(layer).iter().map(|l| {
        let w = &l.workflow;
        let seen = state.get(&w.name);
        let ok = is_approved(&approved, l);
        json!({
            "name": w.name, "file": l.file, "trigger": w.trigger, "prompt": w.prompt, "approved": ok,
            "lastRun": seen.and_then(|s| s.last_run).map(local_iso),
            "nextRun": if ok { due_at(w, seen, now).map(local_iso) } else { None },
        })
    }).collect()
}

/// Something that must not be interrupted, or another workflow, is running.
async fn busy(state: &AppState) -> bool {
    state.library_busy() || state.chats.list().await.iter().any(|c| c.running && (c.kind == "workflow" || c.kind == "wrap" || c.kind == "reorganize"))
}

async fn start(state: &Arc<AppState>, w: &Workflow) -> anyhow::Result<String> {
    let lib = state.settings.read().await.library().ok_or_else(|| anyhow::anyhow!("no library configured"))?;
    let mut chat = Chat { kind: "workflow".into(), title: w.name.clone(), ..Default::default() };
    chat.options = RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() };
    chat.messages.push(Msg::new("user", &w.prompt));
    let id = chat.id.clone();
    state.chats.insert(chat).await;
    let spec = RunSpec {
        chat_id: id.clone(), prompt: w.prompt.clone(), cwd: lib, add_dirs: vec![],
        options: RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() }, resume: None,
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

/// The runner: checks schedules twice a minute, listens for event triggers, starts one workflow at a time.
pub async fn run_loop(state: Arc<AppState>) {
    let layer = crate::paths::app_layer_dir();
    let home = crate::paths::home();
    let mut rx = state.events.subscribe();
    let mut queue: VecDeque<String> = approved_workflows(&layer, &home).into_iter()
        .filter(|w| w.trigger == Trigger::Event { on: "app.start".into() }).map(|w| w.name).collect();
    // Runs that ended since the last pass: (chat id, finished without an error).
    let mut finished: Vec<(String, bool)> = vec![];
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            ev = rx.recv() => match ev {
                Ok(e) if e["type"] == "wrap.done" => {
                    for w in approved_workflows(&layer, &home) {
                        if w.trigger == (Trigger::Event { on: "wrap.done".into() }) && !queue.contains(&w.name) { queue.push_back(w.name); }
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
        // Only workflows the user approved, in exactly the form they approved, are ever started.
        prune_approved(&layer, &home);
        let workflows = approved_workflows(&layer, &home);
        let now = Local::now().naive_local();
        let mut st = load_state(&home);
        let mut changed = false;
        for w in &workflows {
            let seen = st.entry(w.name.clone()).or_default();
            if seen.first_seen.is_none() { seen.first_seen = Some(now); changed = true; }
        }
        let before = st.len();
        st.retain(|name, _| workflows.iter().any(|w| &w.name == name));
        changed |= st.len() != before;
        // Settle the runs that ended: a failed one gets a single retry ten minutes later.
        for (name, seen) in st.iter_mut() {
            let Some(chat) = seen.chat_id.clone() else { continue };
            let result = match finished.iter().find(|(id, _)| *id == chat) {
                Some((_, ok)) => Some(*ok),
                // No word of it and it is not running: the daemon restarted under it. Not retried blindly.
                None if !state.chats.get(&chat).await.map_or(false, |c| c.running) => Some(true),
                None => None,
            };
            let Some(ok) = result else { continue };
            let failed = !ok && !stopped_by_user(&state, &chat).await;
            note_result(seen, !failed, now);
            if seen.retry_at.is_some() { crate::log::warn(format!("workflow \"{name}\" failed; it will be tried once more in {RETRY_MINUTES} minutes")); }
            changed = true;
        }
        finished.clear();
        for name in due(&workflows, &st, now) { if !queue.contains(&name) { queue.push_back(name); } }
        queue.retain(|name| workflows.iter().any(|w| &w.name == name));
        if !queue.is_empty() && !busy(&state).await {
            if let Some(w) = queue.pop_front().and_then(|name| workflows.iter().find(|w| w.name == name)) {
                // Recorded before the run, so a workflow that fails is not retried in a loop.
                let seen = st.entry(w.name.clone()).or_default();
                note_start(seen, now);
                changed = true;
                match start(&state, w).await {
                    Ok(id) => seen.chat_id = Some(id),
                    Err(e) => {
                        crate::log::warn(format!("workflow \"{}\" could not start: {e}", w.name));
                        note_result(seen, false, now);
                    }
                }
            }
        }
        if changed { save_state(&home, &st); }
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
        let ws = [w.clone()];
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
        assert_eq!(missed(&m, Some(&s), at(2026, 3, 1, 17, 0)), Some(at(2026, 2, 28, 18, 0)), "opened the next morning: still within a day");
        assert_eq!(missed(&m, Some(&s), at(2026, 3, 1, 18, 1)), None, "more than a day late: skipped");
        assert_eq!(due_at(&m, Some(&s), at(2026, 3, 1, 18, 1)), Some(at(2026, 3, 31, 18, 0)), "waits for the next month end");
        // Once it has run for that time it is not due again.
        let ran = seen(at(2026, 1, 15, 0, 0), Some(at(2026, 3, 1, 17, 0)));
        assert_eq!(missed(&m, Some(&ran), at(2026, 3, 1, 17, 30)), None);
        // Weekdays: Friday's run missed, opened on Saturday morning → caught up; on Sunday → skipped.
        let wd = wf(sched("weekdays", "17:00", None, None, None));
        let s = seen(at(2026, 11, 1, 0, 0), Some(at(2026, 11, 12, 17, 0)));
        assert!(missed(&wd, Some(&s), at(2026, 11, 14, 10, 0)).is_some());
        assert!(missed(&wd, Some(&s), at(2026, 11, 15, 10, 0)).is_none());
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
        assert_eq!(due(&[w.clone()], &st, t0), ["W"]);
        let seen = st.get_mut("W").unwrap();
        note_start(seen, t0);
        seen.chat_id = Some("c1".into());
        note_result(seen, false, at(2026, 11, 10, 9, 2));
        assert_eq!((seen.retry_at, seen.retried, seen.chat_id.clone()), (Some(at(2026, 11, 10, 9, 12)), true, None));
        assert!(due(&[w.clone()], &st, at(2026, 11, 10, 9, 11)).is_empty(), "not before ten minutes have passed");
        assert_eq!(due_at(&w, st.get("W"), at(2026, 11, 10, 9, 11)), Some(at(2026, 11, 10, 9, 12)));
        assert_eq!(due(&[w.clone()], &st, at(2026, 11, 10, 9, 12)), ["W"]);
        // The retry fails too: no third attempt, the next run is tomorrow's.
        let seen = st.get_mut("W").unwrap();
        note_start(seen, at(2026, 11, 10, 9, 12));
        assert!(seen.retried && seen.retry_at.is_none());
        note_result(seen, false, at(2026, 11, 10, 9, 14));
        assert_eq!(seen.retry_at, None);
        assert!(due(&[w.clone()], &st, at(2026, 11, 10, 12, 0)).is_empty());
        assert_eq!(due_at(&w, st.get("W"), at(2026, 11, 10, 12, 0)), Some(at(2026, 11, 11, 9, 0)));
        // Tomorrow's run starts with a clean slate and, if it fails, earns its own retry.
        let seen = st.get_mut("W").unwrap();
        note_start(seen, at(2026, 11, 11, 9, 0));
        assert!(!seen.retried);
        note_result(seen, true, at(2026, 11, 11, 9, 5));
        assert_eq!(seen.retry_at, None);
    }

    #[test]
    fn only_approved_content_runs() {
        let d = std::env::temp_dir().join(format!("thoughts-wf-approve-{}", uuid::Uuid::new_v4()));
        let (layer, home) = (d.join("layer"), d.join("home"));
        std::fs::create_dir_all(layer.join("workflows")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let file = layer.join("workflows/weekly.json");
        let body = |prompt: &str| format!(r#"{{"name":"Weekly review","trigger":{{"type":"event","on":"wrap.done"}},"prompt":"{prompt}"}}"#);
        std::fs::write(&file, body("Review my week.")).unwrap();
        std::fs::write(layer.join("workflows/other.json"), r#"{"trigger":{"type":"event","on":"app.start"},"prompt":"Tidy."}"#).unwrap();
        let names = || approved_workflows(&layer, &home).into_iter().map(|w| w.name).collect::<Vec<_>>();
        let flag = |name: &str| status(&layer, &home, at(2026, 11, 10, 8, 0)).into_iter().find(|s| s["name"] == name).unwrap()["approved"].clone();

        // Nothing runs until the user says so.
        assert!(names().is_empty());
        assert_eq!(flag("Weekly review"), false);
        approve(&layer, &home, "Weekly review").unwrap();
        assert_eq!(names(), ["Weekly review"]);
        assert_eq!(flag("Weekly review"), true);
        assert_eq!(flag("other"), false);
        assert_eq!(load_approved(&home).keys().collect::<Vec<_>>(), ["weekly.json"]);

        // Any edit to the file (here: a different prompt) needs a fresh approval.
        std::fs::write(&file, body("Delete everything.")).unwrap();
        assert!(names().is_empty());
        assert_eq!(flag("Weekly review"), false);
        std::fs::write(&file, body("Review my week.")).unwrap();
        assert_eq!(names(), ["Weekly review"], "the approved content is still approved");

        disable(&layer, &home, "Weekly review").unwrap();
        assert!(names().is_empty());
        assert!(approve(&layer, &home, "No such").is_err() && disable(&layer, &home, "No such").is_err());

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
        let ws = load(&d);
        assert_eq!(ws.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(), ["Weekly review", "b-after-wrap"]);
        // Not approved yet: listed, but with no next run.
        assert!(status(&d, &d, at(2026, 11, 10, 8, 0))[0]["nextRun"].is_null());
        approve(&d, &d, "Weekly review").unwrap();
        let s = status(&d, &d, at(2026, 11, 10, 8, 0));
        assert_eq!(s[0]["trigger"]["weekday"], 5);
        assert!(s[0]["nextRun"].as_str().unwrap().starts_with("2026-11-13T16:00"));
        assert!(s[0]["lastRun"].is_null() && s[1]["nextRun"].is_null());
        std::fs::remove_dir_all(d).unwrap();
    }
}
