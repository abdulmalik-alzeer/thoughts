//! Workflows: small recurring jobs the user (through the AI) defines in the layer as
//! `workflows/*.json`. The daemon runs each due one as a chat of kind `workflow` inside the
//! library — one at a time, never while a wrap is running, trust-checked like any other run.

use crate::chats::{Chat, Msg, RunOptions};
use crate::claude::{self, RunSpec};
use crate::state::AppState;
use chrono::{Datelike, Local, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Trigger {
    /// `every`: "day" | "week"; `at`: "HH:MM" local time; `weekday`: 0 (Sunday) – 6 (Saturday), weekly only.
    Schedule { every: String, at: String, #[serde(default, skip_serializing_if = "Option::is_none")] weekday: Option<u32> },
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

/// Is this definition one the runner understands?
pub fn valid(w: &Workflow) -> bool {
    if w.name.trim().is_empty() || w.name.chars().count() > 80 || w.name.chars().any(char::is_control) { return false; }
    if w.prompt.trim().is_empty() || w.prompt.len() > 20_000 { return false; }
    match &w.trigger {
        Trigger::Schedule { every, at, weekday } => parse_at(at).is_some() && match every.as_str() {
            "day" => true,
            "week" => weekday.map_or(true, |d| d <= 6),
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
    let Trigger::Schedule { every, at, weekday } = trigger else { return None };
    let time = parse_at(at)?;
    let mut day = after.date();
    // At most eight days ahead covers every daily and weekly case.
    for _ in 0..9 {
        let fits = match every.as_str() {
            "day" => true,
            "week" => day.weekday().num_days_from_sunday() == weekday.unwrap_or(0),
            _ => return None,
        };
        let candidate = day.and_time(time);
        if fits && candidate > after { return Some(candidate); }
        day = day.succ_opt()?;
    }
    None
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Seen {
    pub last_run: Option<NaiveDateTime>,
    /// When the runner first met this workflow: a schedule counts from here, so a new workflow
    /// never fires for a time that had already passed when it was created.
    pub first_seen: Option<NaiveDateTime>,
}

pub type State = BTreeMap<String, Seen>;

fn state_file(home: &Path) -> PathBuf { home.join("workflows-state.json") }

pub fn load_state(home: &Path) -> State {
    std::fs::read_to_string(state_file(home)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_state(home: &Path, s: &State) {
    if let Ok(t) = serde_json::to_string_pretty(s) { let _ = crate::paths::write_atomic(&state_file(home), t.as_bytes(), true); }
}

/// When `w` should next run given what the runner remembers about it.
pub fn due_at(w: &Workflow, seen: Option<&Seen>, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let base = seen.and_then(|s| s.last_run.or(s.first_seen)).unwrap_or(now);
    next_run(&w.trigger, base)
}

/// Names of schedule workflows that are due at `now`. A machine that was off for a week runs each once.
pub fn due(workflows: &[Workflow], state: &State, now: NaiveDateTime) -> Vec<String> {
    workflows.iter().filter(|w| due_at(w, state.get(&w.name), now).map_or(false, |t| t <= now)).map(|w| w.name.clone()).collect()
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

async fn start(state: &Arc<AppState>, w: &Workflow) -> anyhow::Result<()> {
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
    Ok(())
}

/// The runner: checks schedules twice a minute, listens for event triggers, starts one workflow at a time.
pub async fn run_loop(state: Arc<AppState>) {
    let layer = crate::paths::app_layer_dir();
    let home = crate::paths::home();
    let mut rx = state.events.subscribe();
    let mut queue: VecDeque<String> = approved_workflows(&layer, &home).into_iter()
        .filter(|w| w.trigger == Trigger::Event { on: "app.start".into() }).map(|w| w.name).collect();
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
        for name in due(&workflows, &st, now) { if !queue.contains(&name) { queue.push_back(name); } }
        queue.retain(|name| workflows.iter().any(|w| &w.name == name));
        if !queue.is_empty() && !busy(&state).await {
            if let Some(w) = queue.pop_front().and_then(|name| workflows.iter().find(|w| w.name == name)) {
                // Recorded before the run, so a workflow that fails is not retried in a loop.
                st.entry(w.name.clone()).or_default().last_run = Some(now);
                changed = true;
                if let Err(e) = start(&state, w).await { crate::log::warn(format!("workflow \"{}\" could not start: {e}", w.name)); }
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
    fn daily(t: &str) -> Trigger { Trigger::Schedule { every: "day".into(), at: t.into(), weekday: None } }
    fn weekly(t: &str, wd: u32) -> Trigger { Trigger::Schedule { every: "week".into(), at: t.into(), weekday: Some(wd) } }

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
        st.insert("Morning".into(), Seen { first_seen: Some(at(2026, 11, 10, 12, 0)), last_run: None });
        assert!(due(&ws, &st, at(2026, 11, 10, 18, 0)).is_empty(), "09:00 had already passed when it was created");
        assert_eq!(due(&ws, &st, at(2026, 11, 11, 9, 0)), ["Morning"]);
        // After a week off it runs once, then waits for tomorrow.
        assert_eq!(due(&ws, &st, at(2026, 11, 18, 10, 0)), ["Morning"]);
        st.get_mut("Morning").unwrap().last_run = Some(at(2026, 11, 18, 10, 0));
        assert!(due(&ws, &st, at(2026, 11, 18, 23, 0)).is_empty());
        assert_eq!(due_at(&w, st.get("Morning"), at(2026, 11, 18, 23, 0)), Some(at(2026, 11, 19, 9, 0)));
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
