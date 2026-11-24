//! Usage: how much of the Claude and Codex plans' limits is used, and the tokens the assistants
//! used with what they would cost at API prices — for the usage ring in a chat, the tile on Home,
//! Settings › Usage and the Usage page.
//!
//! Nothing here asks the providers anything, spends anything or touches the keychain. Every figure
//! comes from what the assistant programs print while they run (and, for the plans' limits, what
//! they answer when `plan_watch.rs` asks them before any run), or (only when the user chose
//! "Everything on this computer") from the session files they keep on this computer, read and
//! never written:
//!
//! - **Plan limits.** Claude Code reports the plan's limit in `rate_limit_event` (status, the window,
//!   when it resets, and in newer versions how much of the 5-hour and weekly windows is used);
//!   Codex reports `rate_limits` (percent used, window length, reset time) with its token counts,
//!   on the run's output (older versions) or in the session file of the run (newer ones; the app
//!   already reads that file to find the run's session). From here they are only known after a run
//!   (`plan_watch.rs` asks without one), so each window keeps the time it was seen ("as of …"). Kept in `~/.thoughts/plan-usage.json`.
//! - **Tokens and cost.** Every run the app starts adds a line to `~/.thoughts/meter.jsonl`: the
//!   tokens per model, and the cost Claude Code reported (`modelUsage.*.costUSD`) or, for Codex,
//!   an estimate from `prices.rs`. A model that is not in the price table has an unknown cost.
//! - **Everything on this computer.** Sessions under Claude's folder (`$CLAUDE_CONFIG_DIR`, else
//!   `~/.claude`, `projects/**.jsonl`) and Codex's (`$CODEX_HOME`, else `~/.codex`,
//!   `sessions/**/rollout-*.jsonl`) are added — terminal use included. Runs the app recorded
//!   itself are counted once.

use crate::prices::{self, Tokens};
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

pub const CLAUDE: &str = "claude";
pub const CODEX: &str = "codex";
/// The longest period the page offers.
pub const MAX_DAYS: i64 = 90;
/// Past this size the file is cut down to its newest records, to well under it (`KEEP_BYTES`), so
/// that it is not rewritten again for a long while.
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const KEEP_BYTES: usize = (MAX_BYTES / 2) as usize;

fn is_zero(n: &u64) -> bool { *n == 0 }

// ---------- what one run used ----------

/// One model's use in one run the app started.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub t: DateTime<Utc>,
    pub provider: String,
    /// The chat the run belonged to; `None` for the app's own background answers (titles, summaries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    /// The assistant program's session, so the same run read from its files is not counted twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub tokens: Tokens,
    /// Of the cache writes, those that last an hour (priced higher).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_write_long: u64,
    /// US dollars: what the program reported, or an estimate (`estimated`); `None` = unknown model.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub estimated: bool,
}

pub fn file(home: &Path) -> PathBuf { home.join("meter.jsonl") }
pub fn plans_file(home: &Path) -> PathBuf { home.join("plan-usage.json") }

static WRITE: Mutex<()> = Mutex::new(());

/// Add records to the file (trimmed to its newest lines when it grows large).
pub fn append(home: &Path, recs: &[Record]) -> anyhow::Result<()> {
    use std::io::Write;
    if recs.is_empty() { return Ok(()); }
    let path = file(home);
    let _one = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    if std::fs::metadata(&path).map_or(false, |m| m.len() > MAX_BYTES) {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        crate::paths::write_atomic(&path, newest(&text, KEEP_BYTES, Utc::now() - chrono::Duration::days(MAX_DAYS)).as_bytes(), true)?;
    }
    let mut out = String::new();
    for r in recs { out.push_str(&serde_json::to_string(r)?); out.push('\n'); }
    let mut opts = std::fs::OpenOptions::new();
    opts.append(true).create(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; opts.mode(0o600); }
    opts.open(&path)?.write_all(out.as_bytes())?;
    Ok(())
}

/// The newest whole lines of `text` that fit in `keep` bytes, without those of records older than
/// `since` (the page never shows them), each line ending in a newline.
fn newest(text: &str, keep: usize, since: DateTime<Utc>) -> String {
    #[derive(Deserialize)]
    struct At { t: DateTime<Utc> }
    let mut kept: Vec<&str> = vec![];
    let mut size = 0;
    for l in text.lines().rev() {
        if l.trim().is_empty() { continue; }
        if size + l.len() + 1 > keep { break; }
        if serde_json::from_str::<At>(l).map_or(false, |a| a.t < since) { break; }
        size += l.len() + 1;
        kept.push(l);
    }
    kept.iter().rev().fold(String::with_capacity(size), |mut s, l| { s.push_str(l); s.push('\n'); s })
}

/// Add records to the file, off the runtime (it is file work, and now and then a rewrite), and
/// tell the window.
async fn keep(state: &AppState, recs: Vec<Record>) {
    let home = crate::paths::home();
    let r = tokio::task::spawn_blocking(move || append(&home, &recs)).await.map_err(anyhow::Error::from).and_then(|r| r);
    if let Err(e) = r { crate::log::warn(format!("usage: could not keep a run's figures: {e}")); }
    changed(state);
}

/// Every record, oldest first (lines that cannot be read are skipped).
pub fn read(home: &Path) -> Vec<Record> {
    std::fs::read_to_string(file(home)).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn u(v: &Value) -> u64 { v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)).unwrap_or(0) }
fn session_of(v: &Value) -> Option<String> {
    v.as_str().filter(|s| (1..=80).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')).map(String::from)
}

/// The records of a Claude Code `result` (or a one-shot answer's JSON): one per model it used.
pub fn claude_records(ev: &Value, chat: Option<&str>, now: DateTime<Utc>) -> Vec<Record> {
    let session = session_of(&ev["session_id"]);
    let base = |model: String, tokens: Tokens, long: u64, cost: Option<f64>| {
        let (cost_usd, estimated) = match cost {
            Some(c) if c.is_finite() && c >= 0.0 => (Some(c), false),
            _ => (prices::cost(&model, &tokens, long, false), true),
        };
        Record { t: now, provider: CLAUDE.into(), chat: chat.map(String::from), session: session.clone(), model, tokens, cache_write_long: long, cost_usd, estimated }
    };
    let mut out = vec![];
    if let Some(models) = ev["modelUsage"].as_object().filter(|m| !m.is_empty()) {
        for (model, mu) in models {
            let tokens = Tokens { input: u(&mu["inputTokens"]), output: u(&mu["outputTokens"]), cache_read: u(&mu["cacheReadInputTokens"]), cache_write: u(&mu["cacheCreationInputTokens"]) };
            let cost = mu["costUSD"].as_f64();
            if tokens.is_empty() && cost.unwrap_or(0.0) <= 0.0 { continue; }
            out.push(base(model.clone(), tokens, 0, cost));
        }
        return out;
    }
    let us = &ev["usage"];
    let tokens = Tokens { input: u(&us["input_tokens"]), output: u(&us["output_tokens"]), cache_read: u(&us["cache_read_input_tokens"]), cache_write: u(&us["cache_creation_input_tokens"]) };
    let cost = ev["total_cost_usd"].as_f64();
    if tokens.is_empty() && cost.unwrap_or(0.0) <= 0.0 { return out; }
    out.push(base(String::new(), tokens, u(&us["cache_creation"]["ephemeral_1h_input_tokens"]), cost));
    out
}

/// Keep what a Claude Code run used (its `result`), and tell the window.
pub async fn record_claude(state: &AppState, chat: Option<&str>, ev: &Value) {
    let recs = claude_records(ev, chat, Utc::now());
    if recs.is_empty() { return; }
    keep(state, recs).await;
    crate::plan_watch::after_run(CLAUDE);
}

fn changed(state: &AppState) { state.emit(json!({"type": "meter.changed"})); }

// ---------- Codex: tokens and limits from its events ----------

/// What one line of Codex's output or session file says about use.
#[derive(Debug, Clone, PartialEq)]
pub enum CodexBit {
    Model(String),
    Session { id: Option<String>, folder: Option<String> },
    /// A request's tokens (`last`) and the session's running total (`total`), when given.
    Count { last: Option<Tokens>, total: Option<Tokens>, limits: Option<Value>, at: Option<DateTime<Utc>> },
    /// A whole turn's tokens (newer `exec --json`, which gives no per-request counts).
    Turn(Tokens),
}

/// Codex's token usage (input counts the cached part too) in the app's four kinds.
fn codex_tokens(v: &Value) -> Option<Tokens> {
    if !v.is_object() { return None; }
    let input = u(&v["input_tokens"]);
    let cached = u(&v["cached_input_tokens"]);
    let written = u(&v["cache_write_input_tokens"]);
    let t = Tokens { input: input.saturating_sub(cached).saturating_sub(written), output: u(&v["output_tokens"]), cache_read: cached, cache_write: written };
    Some(t)
}

fn at_of(v: &Value) -> Option<DateTime<Utc>> { v["timestamp"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc)) }

pub fn codex_line(line: &str) -> Vec<CodexBit> {
    // Most lines are about something else: skip them without parsing.
    if !["token_count", "model", "turn.completed", "session_meta"].iter().any(|k| line.contains(k)) { return vec![]; }
    let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![] };
    let at = at_of(&v);
    let count = |m: &Value| {
        let info = &m["info"];
        let (last, total) = if info.is_object() {
            (codex_tokens(&info["last_token_usage"]), codex_tokens(&info["total_token_usage"]))
        } else if m.get("input_tokens").is_some() {
            (codex_tokens(m), None) // 0.2x: one request per event
        } else { (None, None) };
        let limits = m.get("rate_limits").filter(|r| r.is_object()).cloned();
        CodexBit::Count { last, total, limits, at }
    };
    // `exec --json` of 0.2x–0.3x: {"id":…,"msg":{…}}
    if let Some(m) = v.get("msg").filter(|m| m.is_object()) {
        return match m["type"].as_str().unwrap_or("") {
            "token_count" => vec![count(m)],
            "session_configured" => m["model"].as_str().map(|s| CodexBit::Model(s.into())).into_iter().collect(),
            _ => vec![],
        };
    }
    // Session files: {"timestamp":…,"type":"event_msg"|"turn_context"|"session_meta","payload":{…}}
    let pl = &v["payload"];
    match v["type"].as_str().unwrap_or("") {
        "event_msg" if pl["type"] == "token_count" => return vec![count(pl)],
        "turn_context" => return pl["model"].as_str().map(|s| CodexBit::Model(s.into())).into_iter().collect(),
        "session_meta" => return vec![CodexBit::Session { id: session_of(&pl["id"]), folder: pl["cwd"].as_str().map(folder_name) }],
        "turn.completed" => return codex_tokens(&v["usage"]).filter(|t| !t.is_empty()).map(CodexBit::Turn).into_iter().collect(),
        _ => {}
    }
    // The configuration line `exec --json` printed first (0.2x).
    if v.get("workdir").is_some() || v.get("sandbox").is_some() {
        return v["model"].as_str().map(|s| CodexBit::Model(s.split_whitespace().next().unwrap_or("").to_string())).filter(|b| b != &CodexBit::Model(String::new())).into_iter().collect();
    }
    vec![]
}

/// The last part of a folder path, for "Outside Thoughts · <folder>".
fn folder_name(p: &str) -> String {
    p.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("").chars().take(60).collect()
}

/// Codex's use, gathered line by line: the requests (priced one by one once the model is known),
/// the model, and the newest limits.
#[derive(Default, Debug)]
pub struct CodexRun {
    requests: Vec<(Tokens, bool)>,
    model: Option<String>,
    last_total: Option<Tokens>,
    pub limits: Option<(Value, DateTime<Utc>)>,
    saw_counts: bool,
    pub session: Option<String>,
    pub folder: Option<String>,
}

impl CodexRun {
    pub fn feed(&mut self, line: &str) { for b in codex_line(line) { self.take(b, Utc::now()); } }

    fn take(&mut self, b: CodexBit, now: DateTime<Utc>) {
        match b {
            CodexBit::Model(m) if !m.trim().is_empty() => self.model = Some(m),
            CodexBit::Model(_) => {}
            CodexBit::Session { id, folder } => { if id.is_some() { self.session = id; } if folder.is_some() { self.folder = folder; } }
            CodexBit::Count { last, total, limits, at } => {
                if let Some(l) = limits { self.limits = Some((l, at.unwrap_or(now))); }
                // An event that only repeats the running total (a limits update) is no new request.
                if total.is_some() && total == self.last_total { return; }
                let req = match (last, total, self.last_total) {
                    (Some(l), _, _) => Some(l),
                    (None, Some(t), Some(p)) => Some(Tokens { input: t.input.saturating_sub(p.input), output: t.output.saturating_sub(p.output), cache_read: t.cache_read.saturating_sub(p.cache_read), cache_write: t.cache_write.saturating_sub(p.cache_write) }),
                    (None, Some(t), None) => Some(t),
                    _ => None,
                };
                if total.is_some() { self.last_total = total; }
                if let Some(r) = req.filter(|r| !r.is_empty()) { self.saw_counts = true; self.requests.push((r, true)); }
            }
            CodexBit::Turn(t) => { if !self.saw_counts { self.requests.push((t, false)); } }
        }
    }

    pub fn tokens(&self) -> Tokens { let mut t = Tokens::default(); for (r, _) in &self.requests { t.add(r); } t }

    /// The estimated cost at `model`'s prices; `None` when the model has no price.
    pub fn cost(&self, model: &str) -> Option<f64> {
        let mut sum = 0.0;
        for (r, single) in &self.requests { sum += prices::cost(model, r, 0, *single)?; }
        Some(sum)
    }

    pub fn model(&self) -> Option<&str> { self.model.as_deref() }
}

/// Codex's `rate_limits` as windows. `at` is when they were reported (for "resets in N seconds").
pub fn codex_windows(rl: &Value, at: DateTime<Utc>) -> Vec<(String, Window)> {
    // Several limits can be reported; only Codex's own plan limit is shown.
    if let Some(id) = rl["limit_id"].as_str() { if !id.eq_ignore_ascii_case("codex") { return vec![]; } }
    let mut out = vec![];
    for (slot, fallback) in [("primary", "five_hour"), ("secondary", "seven_day")] {
        let w = &rl[slot];
        let flat = |k: &str| rl[format!("{slot}_{k}")].clone();
        let (pct, mins, reset_at, reset_in) = if w.is_object() {
            (w["used_percent"].as_f64(), w["window_minutes"].as_i64(), w["resets_at"].as_i64(), w["resets_in_seconds"].as_i64().or_else(|| w["reset_after_seconds"].as_i64()))
        } else {
            (flat("used_percent").as_f64(), flat("window_minutes").as_i64(), flat("resets_at").as_i64(), flat("resets_in_seconds").as_i64().or_else(|| flat("reset_after_seconds").as_i64()))
        };
        let Some(pct) = pct.filter(|p| p.is_finite()) else { continue };
        let pct = pct.clamp(0.0, 100.0);
        let key = match mins { Some(300) => "five_hour".to_string(), Some(10080) => "seven_day".to_string(), Some(m) if m > 0 => format!("m{m}"), _ => fallback.to_string() };
        let resets_at = reset_at.map(normalise_epoch).or_else(|| reset_in.map(|s| at.timestamp() + s.max(0)));
        out.push((key, Window { percent: Some(pct), resets_at, minutes: mins.filter(|m| *m > 0), status: Some(if pct >= 100.0 { "reached" } else { "ok" }.into()), seen_at: at.timestamp(), label: None }));
    }
    out
}

/// The model of a Codex session, the use in it and its newest limits, from its session file.
pub fn read_rollout(p: &Path) -> CodexRun {
    use std::io::BufRead;
    let mut run = CodexRun::default();
    let Ok(f) = std::fs::File::open(p) else { return run };
    let now = Utc::now();
    for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
        for b in codex_line(&line) { run.take(b, now); }
    }
    run
}

impl CodexRun {
    /// Take from the run's session file what its output did not say (newer versions name no model
    /// and report no limits there, and count whole turns rather than requests).
    fn fill_from(&mut self, f: CodexRun) {
        if !self.saw_counts && f.saw_counts && self.tokens().total() == f.tokens().total() { self.requests = f.requests; self.saw_counts = true; }
        if self.model.is_none() { self.model = f.model; }
        if self.limits.is_none() { self.limits = f.limits; }
    }
}

/// Keep what a Codex run used, and the plan's limits it reported. Gives the estimated cost, which
/// the caller adds to the month's spending (`AppState::add_usage`): the record here is for the
/// usage page, the month's figure for the spending limit, and neither is ever added to the other.
async fn keep_codex(state: &AppState, chat: Option<&str>, session: Option<String>, mut run: CodexRun, chosen: Option<String>) -> Option<f64> {
    if let Some((rl, at)) = run.limits.take() { update_plans(state, CODEX, codex_windows(&rl, at)); }
    let tokens = run.tokens();
    if tokens.is_empty() { return None; }
    let model = run.model.clone().or(chosen).unwrap_or_default();
    let cost_usd = run.cost(&model);
    let rec = Record { t: Utc::now(), provider: CODEX.into(), chat: chat.map(String::from), session, model, tokens, cache_write_long: 0, cost_usd, estimated: true };
    keep(state, vec![rec]).await;
    crate::plan_watch::after_run(CODEX);
    cost_usd
}

/// After a Codex run the app started (a chat, a wrap…). `chosen` is the model the chat asked for.
pub async fn record_codex(state: &AppState, chat: Option<&str>, session: Option<String>, mut run: CodexRun, chosen: Option<String>) {
    if let (true, Some(sid)) = (run.limits.is_none() || run.model.is_none() || !run.saw_counts, session.clone()) {
        let home = crate::codex::codex_home();
        let from_file = tokio::task::spawn_blocking(move || crate::codex::session_file(&home, &sid).map(|p| read_rollout(&p))).await.ok().flatten();
        if let Some(f) = from_file { run.fill_from(f); }
    }
    if let Some(c) = keep_codex(state, chat, session, run, chosen).await { state.add_usage(c).await; }
}

/// A one-shot Codex answer: its whole output, and its session file read before it was removed.
pub async fn record_codex_one_shot(state: &AppState, stdout: &str, chosen: Option<String>, file: Option<CodexRun>) {
    let mut run = CodexRun::default();
    for l in stdout.lines() { run.feed(l); }
    if let Some(f) = file { run.fill_from(f); }
    if let Some(c) = keep_codex(state, None, None, run, chosen).await { state.add_usage(c).await; }
}

// ---------- plan limits ----------

/// One window of a plan's limit (the 5-hour one, the weekly one…) as last reported.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Window {
    /// Share used, 0–100, when the program said.
    pub percent: Option<f64>,
    /// When it resets, seconds since 1970.
    pub resets_at: Option<i64>,
    pub minutes: Option<i64>,
    /// "ok", "warning" (close to the limit) or "reached".
    pub status: Option<String>,
    /// When this was reported, seconds since 1970.
    pub seen_at: i64,
    /// What a window for one model or one product is for, in the service's own words ("Opus").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

type Plans = BTreeMap<String, BTreeMap<String, Window>>;

static PLANS: Mutex<Option<Plans>> = Mutex::new(None);

fn normalise_epoch(t: i64) -> i64 { if t > 100_000_000_000 { t / 1000 } else { t } }

/// Claude Code's `rate_limit_info` as windows.
pub fn claude_windows(info: &Value, now: i64) -> Vec<(String, Window)> {
    let pct = |v: &Value| v.as_f64().filter(|f| f.is_finite()).map(|f| if f <= 1.5 { f * 100.0 } else { f }.clamp(0.0, 100.0));
    let epoch = |v: &Value| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).map(normalise_epoch);
    let mut out: BTreeMap<String, Window> = BTreeMap::new();
    if let Some(ws) = info["unifiedWindows"].as_object() {
        for (k, w) in ws {
            if k.is_empty() || k.len() > 40 { continue; }
            let p = pct(&w["utilization"]);
            out.insert(k.clone(), Window { percent: p, resets_at: epoch(&w["resetsAt"]), minutes: None, status: p.map(|p| if p >= 100.0 { "reached" } else { "ok" }.into()), seen_at: now, label: None });
        }
    }
    let status = match info["status"].as_str() { Some("rejected") => Some("reached"), Some("allowed_warning") => Some("warning"), Some("allowed") => Some("ok"), _ => None };
    if let Some(k) = info["rateLimitType"].as_str().filter(|k| !k.is_empty() && k.len() <= 40) {
        let w = out.entry(k.to_string()).or_insert_with(|| Window { seen_at: now, ..Default::default() });
        if let Some(p) = pct(&info["utilization"]) { w.percent = Some(p); }
        if let Some(r) = epoch(&info["resetsAt"]) { w.resets_at = Some(r); }
        if let Some(s) = status { w.status = Some(s.into()); }
        if status == Some("reached") && w.percent.is_none() { w.percent = Some(100.0); }
    }
    for (k, w) in out.iter_mut() { if w.minutes.is_none() { w.minutes = match k.as_str() { "five_hour" => Some(300), k if k.starts_with("seven_day") => Some(10080), _ => None }; } }
    out.into_iter().collect()
}

/// Fold newly reported windows into what is known: an older report never replaces a newer one, and
/// a report that gives only a status keeps the share last seen for the same window period.
pub fn merge(known: &mut BTreeMap<String, Window>, new: Vec<(String, Window)>) -> bool {
    let mut changed = false;
    for (k, mut w) in new {
        if let Some(old) = known.get(&k) {
            if old.seen_at > w.seen_at { continue; }
            if w.percent.is_none() && old.resets_at == w.resets_at { w.percent = old.percent; }
            if w.resets_at.is_none() && old.resets_at.map_or(false, |r| r > w.seen_at) { w.resets_at = old.resets_at; }
        }
        if known.get(&k) != Some(&w) { known.insert(k, w); changed = true; }
    }
    changed
}

fn with_plans<T>(f: impl FnOnce(&mut Plans) -> T) -> T {
    let mut g = PLANS.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(std::fs::read_to_string(plans_file(&crate::paths::home())).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default());
    }
    f(g.as_mut().unwrap())
}

pub fn update_plans(state: &AppState, provider: &str, windows: Vec<(String, Window)>) {
    if windows.is_empty() { return; }
    let saved = with_plans(|p| {
        let known = p.entry(provider.to_string()).or_default();
        if !merge(known, windows) { return None; }
        Some(serde_json::to_string_pretty(&*p).unwrap_or_default())
    });
    if let Some(text) = saved {
        let _ = crate::paths::write_atomic(&plans_file(&crate::paths::home()), text.as_bytes(), true);
        changed(state);
    }
}

/// A whole report of `provider`'s plan (`plan_watch.rs` asked the program itself): it replaces what
/// was known, so a window the plan no longer has does not linger with an old figure.
pub fn set_plans(provider: &str, windows: Vec<(String, Window)>) {
    let text = with_plans(|p| {
        p.insert(provider.to_string(), windows.into_iter().collect());
        serde_json::to_string_pretty(&*p).unwrap_or_default()
    });
    let _ = crate::paths::write_atomic(&plans_file(&crate::paths::home()), text.as_bytes(), true);
}

/// A `rate_limit_event` of Claude Code's output (the user's own Claude plan).
pub fn note_claude_limit(state: &AppState, ev: &Value) {
    update_plans(state, CLAUDE, claude_windows(&ev["rate_limit_info"], Utc::now().timestamp()));
}

const ORDER: &[&str] = &["five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet"];

fn plan_json(windows: &BTreeMap<String, Window>, now: i64) -> Value {
    if windows.is_empty() { return Value::Null; }
    let mut ws: Vec<(&String, &Window)> = windows.iter().collect();
    let rank = |k: &str| ORDER.iter().position(|o| *o == k).unwrap_or(ORDER.len());
    ws.sort_by(|a, b| rank(a.0).cmp(&rank(b.0)).then(a.0.cmp(b.0)));
    let list: Vec<Value> = ws.iter().map(|(k, w)| {
        // A window whose reset time has passed says nothing about now.
        let reset = w.resets_at.map_or(false, |r| r <= now);
        json!({"id": k, "percent": if reset { None } else { w.percent }, "resetsAt": w.resets_at, "minutes": w.minutes,
            "status": if reset { None } else { w.status.clone() }, "seenAt": w.seen_at, "hasReset": reset, "label": w.label})
    }).collect();
    json!({"windows": list, "seenAt": ws.iter().map(|w| w.1.seen_at).max()})
}

pub fn plans_json(now: i64) -> Value {
    with_plans(|p| json!({
        CLAUDE: p.get(CLAUDE).map_or(Value::Null, |w| plan_json(w, now)),
        CODEX: p.get(CODEX).map_or(Value::Null, |w| plan_json(w, now)),
    }))
}

// ---------- everything on this computer ----------

/// Where Claude Code keeps its sessions (`$CLAUDE_CONFIG_DIR`, else `~/.claude`). Tests get a
/// folder of their own that does not exist.
pub fn claude_home() -> PathBuf {
    if cfg!(test) { return std::env::temp_dir().join(format!("thoughts-test-no-claude-{}", std::process::id())); }
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude"))).unwrap_or_default()
}

/// Use read from a session file.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub t: DateTime<Utc>,
    pub provider: &'static str,
    pub chat: Option<String>,
    pub session: Option<String>,
    pub folder: Option<String>,
    pub outside: bool,
    pub model: String,
    pub tokens: Tokens,
    pub cost: Option<f64>,
    pub estimated: bool,
    /// Claude's message id: the same message copied into two files counts once.
    pub key: Option<String>,
}

/// The `*.jsonl` files under `root` (to `depth` folders down, links never followed) changed since `since`.
fn jsonl_files(root: &Path, depth: usize, since: SystemTime, name_ok: fn(&str) -> bool) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut out = vec![];
    if !std::fs::symlink_metadata(root).map_or(false, |m| m.is_dir()) { return out; }
    let mut dirs = vec![(root.to_path_buf(), 0usize)];
    while let Some((d, lv)) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() { if lv < depth { dirs.push((e.path(), lv + 1)); } continue; }
            if !ft.is_file() { continue; }
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".jsonl") || !name_ok(&name) { continue; }
            let Ok(m) = e.metadata() else { continue };
            let mt = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if mt + Duration::from_secs(60) >= since { out.push((e.path(), m.len(), mt)); }
            if out.len() >= 20_000 { return out; }
        }
    }
    out
}

/// The assistant messages of one Claude Code session file (each message once, its last line).
pub fn parse_claude_file(text_lines: impl Iterator<Item = String>) -> Vec<Item> {
    let mut by_key: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Item> = vec![];
    for line in text_lines {
        if !line.contains("\"usage\"") || !line.contains("\"assistant\"") { continue; }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["type"] != "assistant" { continue; }
        let m = &v["message"];
        let us = &m["usage"];
        if !us.is_object() { continue; }
        let model = m["model"].as_str().unwrap_or("").to_string();
        if model.starts_with('<') { continue; } // "<synthetic>": the program's own placeholder, no request
        let Some(t) = at_of(&v) else { continue };
        let tokens = Tokens { input: u(&us["input_tokens"]), output: u(&us["output_tokens"]), cache_read: u(&us["cache_read_input_tokens"]), cache_write: u(&us["cache_creation_input_tokens"]) };
        if tokens.is_empty() { continue; }
        let long = u(&us["cache_creation"]["ephemeral_1h_input_tokens"]);
        let (cost, estimated) = match v["costUSD"].as_f64().filter(|c| c.is_finite() && *c >= 0.0) {
            Some(c) => (Some(c), false),
            None => (prices::cost(&model, &tokens, long, true), true),
        };
        let key = m["id"].as_str().map(|id| format!("{id}:{}", v["requestId"].as_str().unwrap_or("")));
        let item = Item { t, provider: CLAUDE, chat: None, session: session_of(&v["sessionId"]), folder: v["cwd"].as_str().map(folder_name), outside: true, model, tokens, cost, estimated, key: key.clone() };
        match key.as_ref().and_then(|k| by_key.get(k)) {
            Some(&i) => out[i] = item,
            None => { if let Some(k) = key { by_key.insert(k, out.len()); } out.push(item); }
        }
    }
    out
}

/// The requests of one Codex session file, and its newest limits.
pub fn parse_codex_file(text_lines: impl Iterator<Item = String>, name_session: Option<String>) -> (Vec<Item>, Option<(Value, DateTime<Utc>)>) {
    let mut run = CodexRun::default();
    let mut stamps: Vec<DateTime<Utc>> = vec![];
    let mut fallback_t = None;
    for line in text_lines {
        for b in codex_line(&line) {
            let at = if let CodexBit::Count { at, .. } = &b { *at } else { None };
            let before = run.requests.len();
            run.take(b, Utc::now());
            if let Some(a) = at { fallback_t = Some(a); }
            if run.requests.len() > before { stamps.push(at.or(fallback_t).unwrap_or_else(Utc::now)); }
        }
    }
    let model = run.model.clone().unwrap_or_default();
    let session = run.session.clone().or(name_session);
    let items = run.requests.iter().zip(stamps).map(|((t, single), at)| Item {
        t: at, provider: CODEX, chat: None, session: session.clone(), folder: run.folder.clone(), outside: true, model: model.clone(),
        tokens: *t, cost: prices::cost(&model, t, 0, *single), estimated: true, key: None,
    }).collect();
    (items, run.limits)
}

struct Cached { len: u64, modified: SystemTime, items: Vec<Item>, limits: Option<(Value, DateTime<Utc>)> }
static SCAN: Mutex<Option<HashMap<PathBuf, Cached>>> = Mutex::new(None);

/// What reading the two folders found.
#[derive(Debug, Default)]
pub struct Scan { pub items: Vec<Item>, pub files: usize, pub unreadable: usize, pub codex_limits: Option<(Value, DateTime<Utc>)>, pub claude_found: bool, pub codex_found: bool }

/// Read (only what changed since the last time) the session files written since `since`.
pub fn scan(claude: &Path, codex: &Path, since: SystemTime) -> Scan {
    use std::io::BufRead;
    let mut g = SCAN.lock().unwrap_or_else(|e| e.into_inner());
    let cache = g.get_or_insert_with(HashMap::new);
    let mut s = Scan { claude_found: claude.join("projects").is_dir(), codex_found: codex.join("sessions").is_dir(), ..Default::default() };
    let claude_files = jsonl_files(&claude.join("projects"), 4, since, |_| true).into_iter().map(|f| (f, CLAUDE));
    let codex_files = jsonl_files(&codex.join("sessions"), 4, since, |n| n.starts_with("rollout-")).into_iter().map(|f| (f, CODEX));
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for ((path, len, modified), provider) in claude_files.chain(codex_files) {
        seen.insert(path.clone());
        s.files += 1;
        let fresh = cache.get(&path).map_or(false, |c| c.len == len && c.modified == modified);
        if !fresh {
            let Ok(f) = std::fs::File::open(&path) else { s.unreadable += 1; continue };
            let lines = std::io::BufReader::new(f).lines().map_while(Result::ok);
            let (items, limits) = if provider == CLAUDE { (parse_claude_file(lines), None) } else {
                let from_name = path.file_stem().and_then(|n| n.to_str()).and_then(|n| n.get(n.len().saturating_sub(36)..)).and_then(|n| session_of(&json!(n)));
                parse_codex_file(lines, from_name)
            };
            cache.insert(path.clone(), Cached { len, modified, items, limits });
        }
        let c = &cache[&path];
        s.items.extend(c.items.iter().cloned());
        if let Some((l, at)) = &c.limits { if s.codex_limits.as_ref().map_or(true, |(_, b)| at > b) { s.codex_limits = Some((l.clone(), *at)); } }
    }
    cache.retain(|p, _| seen.contains(p));
    s
}

// ---------- the figures ----------

#[derive(Default, Clone)]
struct Sum { tokens: Tokens, cost: f64, unpriced: u64, estimated: bool, runs: u64 }

impl Sum {
    fn add(&mut self, tokens: &Tokens, cost: Option<f64>, estimated: bool) {
        self.tokens.add(tokens);
        match cost { Some(c) => self.cost += c, None => self.unpriced += tokens.total() }
        if estimated && cost.is_some() { self.estimated = true; }
        self.runs += 1;
    }
    fn json(&self) -> Value {
        json!({"tokens": self.tokens, "totalTokens": self.tokens.total(), "costUsd": (self.cost * 10_000.0).round() / 10_000.0,
            "unpricedTokens": self.unpriced, "estimated": self.estimated, "count": self.runs})
    }
}

/// A chat the figures name (its title and kind).
pub struct ChatInfo { pub title: String, pub kind: String }

fn from_record(r: &Record) -> Item {
    Item { t: r.t, provider: if r.provider == CODEX { CODEX } else { CLAUDE }, chat: r.chat.clone(), session: r.session.clone(), folder: None, outside: false,
        model: r.model.clone(), tokens: r.tokens, cost: r.cost_usd, estimated: r.estimated, key: None }
}

/// The app's own records, plus (for "everything") what was read from the folders: a run the app
/// recorded is not counted again from its session file, and a session of a chat (from before the
/// app kept records) is put under that chat.
pub fn combine(records: &[Record], scanned: Vec<Item>, chat_sessions: &HashMap<String, String>) -> Vec<Item> {
    let mut items: Vec<Item> = records.iter().map(from_record).collect();
    let mut first: HashMap<&str, DateTime<Utc>> = HashMap::new();
    for r in records { if let Some(s) = &r.session { let e = first.entry(s.as_str()).or_insert(r.t); if r.t < *e { *e = r.t; } } }
    let mut keys: HashSet<String> = HashSet::new();
    for mut it in scanned {
        if let Some(k) = &it.key { if !keys.insert(k.clone()) { continue; } }
        if let Some(s) = it.session.as_deref() {
            // A record is written when its run ends; the run's messages are earlier.
            if first.get(s).map_or(false, |f| it.t >= *f - chrono::Duration::hours(6)) { continue; }
            if let Some(c) = chat_sessions.get(s) { it.chat = Some(c.clone()); it.outside = false; }
        }
        items.push(it);
    }
    items
}

/// The figures for the page: totals per program, each day, each chat (or folder) and each model.
pub fn summarise(items: &[Item], chats: &HashMap<String, ChatInfo>, since: DateTime<Utc>, now: DateTime<Utc>) -> Value {
    let day = |t: &DateTime<Utc>| t.with_timezone(&Local).date_naive();
    let mut totals: BTreeMap<&str, Sum> = BTreeMap::new();
    let mut days: BTreeMap<NaiveDate, BTreeMap<&str, Sum>> = BTreeMap::new();
    let mut models: BTreeMap<(&str, String), Sum> = BTreeMap::new();
    let mut rows: HashMap<String, (Sum, Value, DateTime<Utc>, HashSet<&str>)> = HashMap::new();
    let (first, last) = (day(&since), day(&now));
    let mut d = first;
    while d <= last { days.entry(d).or_default(); d = d.succ_opt().unwrap_or(last + chrono::Duration::days(1)); }
    for it in items.iter().filter(|i| i.t >= since && i.t <= now + chrono::Duration::minutes(5)) {
        totals.entry(it.provider).or_default().add(&it.tokens, it.cost, it.estimated);
        days.entry(day(&it.t)).or_default().entry(it.provider).or_default().add(&it.tokens, it.cost, it.estimated);
        let mkey = if it.model.is_empty() { String::new() } else { prices::normalise(&it.model) };
        models.entry((it.provider, mkey)).or_default().add(&it.tokens, it.cost, it.estimated);
        let (key, about) = match (&it.chat, it.outside) {
            (Some(c), _) => (format!("chat:{c}"), match chats.get(c) {
                Some(ci) => json!({"kind": "chat", "chatId": c, "title": ci.title, "chatKind": ci.kind}),
                None => json!({"kind": "deleted", "chatId": c}),
            }),
            (None, true) => {
                let f = it.folder.clone().unwrap_or_default();
                (format!("outside:{f}"), json!({"kind": "outside", "folder": f}))
            }
            (None, false) => ("background".to_string(), json!({"kind": "background"})),
        };
        let e = rows.entry(key).or_insert_with(|| (Sum::default(), about, it.t, HashSet::new()));
        e.0.add(&it.tokens, it.cost, it.estimated);
        if it.t > e.2 { e.2 = it.t; }
        e.3.insert(it.provider);
    }
    let mut rows: Vec<_> = rows.into_values().collect();
    rows.sort_by(|a, b| b.0.cost.partial_cmp(&a.0.cost).unwrap_or(std::cmp::Ordering::Equal).then(b.0.tokens.total().cmp(&a.0.tokens.total())));
    let chat_rows: Vec<Value> = rows.into_iter().take(60).map(|(s, mut about, last, provs)| {
        let mut p: Vec<&str> = provs.into_iter().collect();
        p.sort();
        about["providers"] = json!(p);
        about["last"] = json!(last);
        about["usage"] = s.json();
        about
    }).collect();
    let mut model_rows: Vec<(&(&str, String), &Sum)> = models.iter().collect();
    model_rows.sort_by(|a, b| b.1.tokens.total().cmp(&a.1.tokens.total()));
    json!({
        "totals": totals.iter().map(|(k, s)| (k.to_string(), s.json())).collect::<serde_json::Map<_, _>>(),
        "days": days.iter().map(|(d, m)| json!({"day": d.to_string(), "usage": m.iter().map(|(k, s)| (k.to_string(), s.json())).collect::<serde_json::Map<_, _>>()})).collect::<Vec<_>>(),
        "chats": chat_rows,
        "models": model_rows.into_iter().map(|((p, m), s)| json!({"provider": p, "model": m, "priced": s.unpriced == 0, "usage": s.json()})).collect::<Vec<_>>(),
    })
}

// ---------- HTTP ----------

type S = State<Arc<AppState>>;
type R = Result<Json<Value>, crate::api::ApiErr>;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/meter", get(meter_get))
        .route("/api/meter/chat", get(meter_chat))
}

#[derive(Deserialize)]
struct Days { days: Option<i64> }

async fn chat_map(st: &AppState) -> (HashMap<String, ChatInfo>, HashMap<String, String>) {
    let mut info = HashMap::new();
    let mut sessions = HashMap::new();
    for c in st.chats.list().await {
        if let Some(s) = &c.session_id { sessions.insert(s.clone(), c.id.clone()); }
        info.insert(c.id.clone(), ChatInfo { title: c.title.clone(), kind: c.kind.clone() });
    }
    (info, sessions)
}

/// The usage page's figures for the last `days` days (1–90, default 30).
async fn meter_get(State(st): S, Query(q): Query<Days>) -> R {
    let days = q.days.unwrap_or(30).clamp(1, MAX_DAYS);
    let now = Utc::now();
    let since_local = (Local::now() - chrono::Duration::days(days - 1)).date_naive().and_hms_opt(0, 0, 0)
        .and_then(|d| d.and_local_timezone(Local).earliest()).map(|d| d.with_timezone(&Utc)).unwrap_or(now - chrono::Duration::days(days));
    let everything = st.settings.read().await.spending.whole_computer;
    let home = crate::paths::home();
    let records = tokio::task::spawn_blocking(move || read(&home)).await.unwrap_or_default();
    let (chats, sessions) = chat_map(&st).await;
    let mut scan_info = json!({"on": everything});
    let items = if everything {
        let since = SystemTime::from(since_local);
        let s = tokio::task::spawn_blocking(move || scan(&claude_home(), &crate::codex::codex_home(), since)).await.unwrap_or_default();
        if let Some((rl, at)) = &s.codex_limits { update_plans(&st, CODEX, codex_windows(rl, *at)); }
        scan_info = json!({"on": true, "files": s.files, "unreadable": s.unreadable, "claudeFound": s.claude_found, "codexFound": s.codex_found});
        combine(&records, s.items, &sessions)
    } else { combine(&records, vec![], &sessions) };
    let mut out = summarise(&items, &chats, since_local, now);
    out["days_n"] = json!(days);
    out["since"] = json!(since_local);
    out["scope"] = json!(if everything { "computer" } else { "thoughts" });
    out["scan"] = scan_info;
    out["plans"] = plans_json(now.timestamp());
    out["watch"] = crate::plan_watch::watch_json();
    out["firstRecord"] = json!(records.iter().map(|r| r.t).min());
    Ok(Json(out))
}

#[derive(Deserialize)]
struct ChatQ { id: String }

/// One chat's figures (for the usage ring), with the plans' limits.
async fn meter_chat(State(st): S, Query(q): Query<ChatQ>) -> R {
    let home = crate::paths::home();
    let records = tokio::task::spawn_blocking(move || read(&home)).await.unwrap_or_default();
    let mut s = Sum::default();
    let mut runs: HashSet<DateTime<Utc>> = HashSet::new();
    for r in records.iter().filter(|r| r.chat.as_deref() == Some(q.id.as_str())) { s.add(&r.tokens, r.cost_usd, r.estimated); runs.insert(r.t); }
    s.runs = runs.len() as u64;
    let chat = st.chats.list().await.into_iter().find(|c| c.id == q.id);
    Ok(Json(json!({
        "usage": s.json(),
        // What the chat itself added up (also from before the app kept these records).
        "chatCostUsd": chat.as_ref().map(|c| c.cost_usd),
        "provider": chat.as_ref().map(|c| c.provider.clone()),
        "plans": plans_json(Utc::now().timestamp()),
        "watch": crate::plan_watch::watch_json(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-meter-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_claude_result_gives_one_record_per_model_with_its_reported_cost() {
        let now = Utc::now();
        let ev = json!({"type": "result", "session_id": "abc-123", "total_cost_usd": 0.5,
            "usage": {"input_tokens": 10, "output_tokens": 20},
            "modelUsage": {
                "claude-opus-4-5-20251101": {"inputTokens": 100, "outputTokens": 200, "cacheReadInputTokens": 3000, "cacheCreationInputTokens": 400, "costUSD": 0.4},
                "claude-haiku-4-5": {"inputTokens": 50, "outputTokens": 5, "cacheReadInputTokens": 0, "cacheCreationInputTokens": 0},
                "claude-unknown-9": {"inputTokens": 0, "outputTokens": 0}
            }});
        let recs = claude_records(&ev, Some("chat-1"), now);
        assert_eq!(recs.len(), 2, "a model that did nothing is left out");
        let opus = recs.iter().find(|r| r.model.starts_with("claude-opus")).unwrap();
        assert_eq!(opus.tokens, Tokens { input: 100, output: 200, cache_read: 3000, cache_write: 400 });
        assert_eq!((opus.cost_usd, opus.estimated), (Some(0.4), false));
        assert_eq!((opus.chat.as_deref(), opus.session.as_deref()), (Some("chat-1"), Some("abc-123")));
        let haiku = recs.iter().find(|r| r.model == "claude-haiku-4-5").unwrap();
        assert!(haiku.estimated && (haiku.cost_usd.unwrap() - (50.0 * 1.0 + 5.0 * 5.0) / 1e6).abs() < 1e-12, "no reported cost: estimated");
        // Without per-model figures: the run's total, model unknown, the reported cost.
        let one = claude_records(&json!({"total_cost_usd": 0.01, "usage": {"input_tokens": 5, "output_tokens": 6, "cache_read_input_tokens": 7}}), None, now);
        assert_eq!(one.len(), 1);
        assert_eq!((one[0].model.as_str(), one[0].cost_usd, one[0].estimated, one[0].tokens.cache_read), ("", Some(0.01), false, 7));
        assert!(claude_records(&json!({"total_cost_usd": 0, "usage": {}}), None, now).is_empty());
    }

    #[test]
    fn records_are_kept_and_read_back() {
        let d = temp();
        let r = Record { t: Utc::now(), provider: CODEX.into(), chat: None, session: Some("s".into()), model: "gpt-5".into(), tokens: Tokens { input: 1, output: 2, cache_read: 3, cache_write: 0 }, cache_write_long: 0, cost_usd: None, estimated: true };
        append(&d, &[r.clone(), r.clone()]).unwrap();
        std::fs::OpenOptions::new().append(true).open(file(&d)).map(|mut f| { use std::io::Write; f.write_all(b"not json\n").unwrap(); }).unwrap();
        let back = read(&d);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0], r);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_large_file_is_cut_to_well_under_its_limit() {
        let d = temp();
        let r = |t: DateTime<Utc>| Record { t, provider: CLAUDE.into(), chat: Some("c".repeat(36)), session: Some("s".repeat(36)), model: "claude-sonnet-4-5".into(), tokens: Tokens { input: 123456, output: 7890, cache_read: 1234567, cache_write: 12345 }, cache_write_long: 0, cost_usd: Some(0.123456), estimated: false };
        // Past the limit with records of the usual size: once cut, the file is at most half the
        // limit, and the next record is added without rewriting it again.
        let line = serde_json::to_string(&r(Utc::now())).unwrap().len() + 1;
        let n = (MAX_BYTES as usize) / line + 50;
        let mut text = String::new();
        for i in 0..n { text.push_str(&serde_json::to_string(&r(Utc::now() - chrono::Duration::seconds((n - i) as i64))).unwrap()); text.push('\n'); }
        std::fs::write(file(&d), &text).unwrap();
        append(&d, &[r(Utc::now())]).unwrap();
        let size = std::fs::metadata(file(&d)).unwrap().len();
        assert!(size <= MAX_BYTES / 2 + 2 * line as u64, "{size}");
        let before = read(&d);
        let ino = |p: &Path| { #[cfg(unix)] { use std::os::unix::fs::MetadataExt; std::fs::metadata(p).unwrap().ino() } #[cfg(not(unix))] { let _ = p; 0 } };
        let first = ino(&file(&d));
        append(&d, &[r(Utc::now())]).unwrap();
        assert_eq!(ino(&file(&d)), first, "rewritten again right after being cut");
        assert_eq!(read(&d).len(), before.len() + 1);
        // Records older than the longest period shown go as well, and a line that cannot be read is kept.
        let old = serde_json::to_string(&r(Utc::now() - chrono::Duration::days(MAX_DAYS + 1))).unwrap();
        let new = serde_json::to_string(&r(Utc::now())).unwrap();
        let cut = newest(&format!("{old}\nnot json\n{new}\n"), 1 << 20, Utc::now() - chrono::Duration::days(MAX_DAYS));
        assert_eq!(cut, format!("not json\n{new}\n"));
        assert_eq!(newest(&format!("{new}\n{new}\n"), new.len() + 1, Utc::now() - chrono::Duration::days(1)), format!("{new}\n"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn codex_output_of_older_versions_gives_requests_model_and_limits() {
        let mut run = CodexRun::default();
        for l in [
            r#"{"model":"gpt-5-codex","sandbox":"read-only","workdir":"/x"}"#,
            r#"{"id":"0","msg":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":50,"reasoning_output_tokens":20,"total_tokens":1050},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":50,"total_tokens":1050}},"rate_limits":{"primary":{"used_percent":12.5,"window_minutes":300,"resets_in_seconds":600},"secondary":{"used_percent":40.0,"window_minutes":10080,"resets_at":1900000000}}}}"#,
            // The same total again (only the limits changed): no new request.
            r#"{"id":"0","msg":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":50,"total_tokens":1050},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":50,"total_tokens":1050}},"rate_limits":{"primary":{"used_percent":13.0,"window_minutes":300}}}}"#,
            // 0.2x: counts at the top of the event, one request each.
            r#"{"id":"0","msg":{"type":"token_count","input_tokens":300,"cached_input_tokens":0,"output_tokens":30,"total_tokens":330}}"#,
            r#"{"id":"0","msg":{"type":"agent_message","message":"hi"}}"#,
        ] { run.feed(l); }
        assert_eq!(run.model(), Some("gpt-5-codex"));
        assert_eq!(run.tokens(), Tokens { input: 200 + 300, output: 80, cache_read: 800, cache_write: 0 });
        let want = (200.0 * 1.25 + 800.0 * 0.125 + 50.0 * 10.0 + 300.0 * 1.25 + 30.0 * 10.0) / 1e6;
        assert!((run.cost("gpt-5-codex").unwrap() - want).abs() < 1e-12);
        assert!(run.cost("gpt-9").is_none(), "an unknown model has no cost");
        let (rl, at) = run.limits.clone().unwrap();
        assert_eq!(rl["primary"]["used_percent"], 13.0, "the newest limits are kept");
        let ws = codex_windows(&rl, at);
        assert_eq!(ws.len(), 1);
        assert_eq!((ws[0].0.as_str(), ws[0].1.percent, ws[0].1.minutes), ("five_hour", Some(13.0), Some(300)));
    }

    #[test]
    fn codex_limits_are_read_in_every_shape_and_others_are_left_out() {
        let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let ws = codex_windows(&json!({"primary": {"used_percent": 100.0, "window_minutes": 300, "resets_in_seconds": 60}, "secondary": {"used_percent": 7.0, "window_minutes": 10080, "resets_at": 1_800_500_000}}), at);
        assert_eq!(ws[0], ("five_hour".into(), Window { percent: Some(100.0), resets_at: Some(1_800_000_060), minutes: Some(300), status: Some("reached".into()), seen_at: 1_800_000_000, label: None }));
        assert_eq!((ws[1].0.as_str(), ws[1].1.resets_at), ("seven_day", Some(1_800_500_000)));
        // The flat shape of some versions, and a window of another length.
        let flat = codex_windows(&json!({"primary_used_percent": 3.0, "primary_window_minutes": 60, "secondary_used_percent": 9.0}), at);
        assert_eq!((flat[0].0.as_str(), flat[1].0.as_str()), ("m60", "seven_day"));
        assert!(codex_windows(&json!({"limit_id": "other-model", "primary": {"used_percent": 5.0}}), at).is_empty());
        assert!(codex_windows(&json!({"primary": {"window_minutes": 300}}), at).is_empty(), "no share, nothing to show");
    }

    #[test]
    fn newer_codex_output_counts_turns_and_a_session_file_counts_requests() {
        let mut run = CodexRun::default();
        run.feed(r#"{"type":"thread.started","thread_id":"t1"}"#);
        run.feed(r#"{"type":"turn.completed","usage":{"input_tokens":24763,"cached_input_tokens":24448,"output_tokens":122}}"#);
        assert_eq!(run.tokens(), Tokens { input: 315, output: 122, cache_read: 24448, cache_write: 0 });
        assert!(run.model().is_none() && run.limits.is_none());

        let file = [
            r#"{"timestamp":"2026-11-10T10:00:00Z","type":"session_meta","payload":{"id":"0199-aaaa","cwd":"/Users/me/Projects/shop"}}"#,
            r#"{"timestamp":"2026-11-10T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5.1-codex","cwd":"/x"}}"#,
            r#"{"timestamp":"2026-11-10T10:00:05Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10},"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":10}},"rate_limits":null}}"#,
            r#"{"timestamp":"2026-11-10T10:00:09Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":300,"cached_input_tokens":100,"output_tokens":30}},"rate_limits":{"primary":{"used_percent":20,"window_minutes":300,"resets_at":1900000000}}}}"#,
            r#"{"timestamp":"2026-11-10T10:00:10Z","type":"response_item","payload":{"type":"message"}}"#,
        ].map(String::from);
        let (items, limits) = parse_codex_file(file.into_iter(), Some("from-name".into()));
        assert_eq!(items.len(), 2);
        assert_eq!((items[0].session.as_deref(), items[0].folder.as_deref(), items[0].model.as_str()), (Some("0199-aaaa"), Some("shop"), "gpt-5.1-codex"));
        assert_eq!(items[1].tokens, Tokens { input: 100, output: 20, cache_read: 100, cache_write: 0 }, "the growth of the total");
        assert_eq!(items[1].t.to_rfc3339(), "2026-11-10T10:00:09+00:00");
        assert!(items.iter().all(|i| i.outside && i.estimated && i.cost.is_some()));
        assert_eq!(limits.unwrap().0["primary"]["used_percent"], 20);
    }

    #[test]
    fn claude_session_files_count_each_message_once() {
        let line = |id: &str, out: u64, model: &str| format!(r#"{{"type":"assistant","timestamp":"2026-11-12T08:00:00Z","sessionId":"sess-1","cwd":"/home/me/notes","requestId":"req_{id}","message":{{"id":"msg_{id}","model":"{model}","usage":{{"input_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":{out},"cache_creation":{{"ephemeral_1h_input_tokens":100}}}}}}}}"#);
        let lines = vec![line("a", 1, "claude-sonnet-4-5-20250929"), line("a", 50, "claude-sonnet-4-5-20250929"), line("b", 5, "<synthetic>"),
            r#"{"type":"user","message":{"content":"hi"}}"#.to_string(), line("c", 7, "claude-new-model")];
        let items = parse_claude_file(lines.into_iter());
        assert_eq!(items.len(), 2, "a message streamed over several lines counts once; the placeholder not at all");
        assert_eq!(items[0].tokens.output, 50, "its last line");
        assert_eq!((items[0].session.as_deref(), items[0].folder.as_deref()), (Some("sess-1"), Some("notes")));
        let want = (10.0 * 3.0 + 100.0 * 6.0 + 1000.0 * 0.30 + 50.0 * 15.0) / 1e6;
        assert!((items[0].cost.unwrap() - want).abs() < 1e-12, "an hour-long cache write at twice the input price");
        assert!(items[1].cost.is_none(), "unknown model: unknown cost");
    }

    #[test]
    fn claude_limits_become_windows_and_merge_with_what_is_known() {
        let now = 1_800_000_000;
        let ws = claude_windows(&json!({"status": "allowed", "rateLimitType": "five_hour", "resetsAt": 1_800_010_000, "utilization": 0.42,
            "unifiedWindows": {"five_hour": {"utilization": 0.4, "resetsAt": 1_800_010_000}, "seven_day": {"utilization": 0.1, "resetsAt": 1_800_400_000}}}), now);
        let m: BTreeMap<String, Window> = ws.into_iter().collect();
        assert_eq!(m["five_hour"].percent, Some(42.0), "the event's own figure wins");
        assert_eq!((m["seven_day"].percent, m["seven_day"].minutes), (Some(10.0), Some(10080)));
        // Only a status (older versions): a warning, no share.
        let w = claude_windows(&json!({"status": "allowed_warning", "rateLimitType": "seven_day", "resetsAt": 1_800_400_000_000_i64}), now + 10);
        assert_eq!((w[0].1.percent, w[0].1.status.as_deref(), w[0].1.resets_at), (None, Some("warning"), Some(1_800_400_000)));
        let mut known = m.clone();
        assert!(merge(&mut known, w));
        assert_eq!(known["seven_day"].percent, Some(10.0), "same window period: the share last seen is kept");
        assert_eq!(known["seven_day"].status.as_deref(), Some("warning"));
        // An older report never replaces a newer one.
        let old = vec![("five_hour".to_string(), Window { percent: Some(1.0), seen_at: now - 100, ..Default::default() })];
        assert!(!merge(&mut known, old));
        // Refused, no share: shown as full.
        let r = claude_windows(&json!({"status": "rejected", "rateLimitType": "five_hour"}), now);
        assert_eq!((r[0].1.percent, r[0].1.status.as_deref()), (Some(100.0), Some("reached")));
        assert!(claude_windows(&json!({"status": "allowed"}), now).is_empty());
        // A window past its reset says nothing about now.
        let j = plan_json(&known, 1_800_020_000);
        assert_eq!(j["windows"][0]["id"], "five_hour");
        assert_eq!((j["windows"][0]["hasReset"].as_bool(), j["windows"][0]["percent"].is_null()), (Some(true), true));
        assert_eq!(j["windows"][1]["percent"], 10.0);
    }

    #[test]
    fn everything_counts_a_run_once_and_puts_sessions_under_their_chat() {
        let t0 = DateTime::parse_from_rfc3339("2026-11-12T10:00:00Z").unwrap().with_timezone(&Utc);
        let rec = Record { t: t0, provider: CLAUDE.into(), chat: Some("c1".into()), session: Some("s1".into()), model: "claude-opus-4-5".into(),
            tokens: Tokens { input: 10, output: 10, cache_read: 0, cache_write: 0 }, cache_write_long: 0, cost_usd: Some(1.0), estimated: false };
        let item = |sess: &str, t: DateTime<Utc>, key: &str| Item { t, provider: CLAUDE, chat: None, session: Some(sess.into()), folder: Some("notes".into()), outside: true,
            model: "claude-opus-4-5".into(), tokens: Tokens { input: 1, output: 1, cache_read: 0, cache_write: 0 }, cost: Some(0.5), estimated: true, key: Some(key.into()) };
        let scanned = vec![
            item("s1", t0 - chrono::Duration::minutes(3), "k1"),  // the recorded run itself
            item("s1", t0 - chrono::Duration::days(3), "k2"),     // the same chat, before records were kept
            item("s2", t0, "k3"), item("s2", t0, "k3"),            // terminal use, copied into two files
            item("s3", t0, "k4"),                                  // a session of a chat the app knows
        ];
        let chats_by_session = HashMap::from([("s3".to_string(), "c3".to_string()), ("s1".to_string(), "c1".to_string())]);
        let all = combine(&[rec], scanned, &chats_by_session);
        assert_eq!(all.len(), 4);
        assert_eq!(all.iter().filter(|i| i.chat.as_deref() == Some("c1")).count(), 2);
        assert!(all.iter().any(|i| i.chat.as_deref() == Some("c3") && !i.outside));
        assert_eq!(all.iter().filter(|i| i.outside).count(), 1);

        let chats = HashMap::from([("c1".to_string(), ChatInfo { title: "Plan".into(), kind: "chat".into() })]);
        let s = summarise(&all, &chats, t0 - chrono::Duration::days(5), t0 + chrono::Duration::hours(1));
        assert_eq!(s["totals"]["claude"]["count"], 4);
        assert!((s["totals"]["claude"]["costUsd"].as_f64().unwrap() - 2.5).abs() < 1e-9);
        assert_eq!(s["totals"]["claude"]["estimated"], true);
        assert_eq!(s["chats"][0]["kind"], "chat");
        assert_eq!(s["chats"][0]["title"], "Plan");
        assert!(s["chats"].as_array().unwrap().iter().any(|r| r["kind"] == "deleted" && r["chatId"] == "c3"));
        assert!(s["chats"].as_array().unwrap().iter().any(|r| r["kind"] == "outside" && r["folder"] == "notes"));
        assert!(s["days"].as_array().unwrap().len() >= 6, "every day of the period, used or not");
        assert_eq!(s["models"][0]["model"], "claude-opus-4-5");
    }

    #[test]
    fn unknown_costs_are_counted_apart_never_as_zero() {
        let now = Utc::now();
        let it = |cost: Option<f64>| Item { t: now, provider: CODEX, chat: None, session: None, folder: None, outside: false, model: "gpt-9".into(),
            tokens: Tokens { input: 100, output: 0, cache_read: 0, cache_write: 0 }, cost, estimated: true, key: None };
        let s = summarise(&[it(None), it(Some(0.25))], &HashMap::new(), now - chrono::Duration::days(1), now);
        assert_eq!(s["totals"]["codex"]["unpricedTokens"], 100);
        assert_eq!(s["totals"]["codex"]["costUsd"], 0.25);
        assert_eq!(s["models"][0]["priced"], false);
        assert_eq!(s["chats"][0]["kind"], "background");
    }

    #[test]
    fn the_folders_are_read_once_and_again_only_when_a_file_changed() {
        let d = temp();
        let (claude, codex) = (d.join("claude"), d.join("codex"));
        std::fs::create_dir_all(claude.join("projects/-x")).unwrap();
        std::fs::create_dir_all(codex.join("sessions/2026/11/12")).unwrap();
        let cl = claude.join("projects/-x/s.jsonl");
        std::fs::write(&cl, r#"{"type":"assistant","timestamp":"2026-11-12T08:00:00Z","sessionId":"s","message":{"id":"m1","model":"claude-haiku-4-5","usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string() + "\n").unwrap();
        std::fs::write(codex.join("sessions/2026/11/12/rollout-2026-11-12T08-00-00-0199aaaa-0000-0000-0000-000000000001.jsonl"),
            r#"{"timestamp":"2026-11-12T08:00:05Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":5,"output_tokens":5}},"rate_limits":{"primary":{"used_percent":3,"window_minutes":300}}}}"#.to_string() + "\n").unwrap();
        std::fs::write(codex.join("sessions/2026/11/12/notes.jsonl"), "{}\n").unwrap();
        let since = SystemTime::now() - Duration::from_secs(3600);
        let s = scan(&claude, &codex, since);
        assert_eq!((s.files, s.items.len(), s.claude_found, s.codex_found), (2, 2, true, true));
        assert_eq!(s.items.iter().find(|i| i.provider == CODEX).unwrap().session.as_deref(), Some("0199aaaa-0000-0000-0000-000000000001"));
        assert!(s.codex_limits.is_some());
        // A link is never followed.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&cl, claude.join("projects/-x/link.jsonl")).unwrap();
            assert_eq!(scan(&claude, &codex, since).files, 2);
        }
        // Grown file: read again.
        use std::io::Write;
        std::fs::OpenOptions::new().append(true).open(&cl).unwrap().write_all(br#"{"type":"assistant","timestamp":"2026-11-12T08:01:00Z","sessionId":"s","message":{"id":"m2","model":"claude-haiku-4-5","usage":{"input_tokens":10,"output_tokens":1}}}
"#).unwrap();
        assert_eq!(scan(&claude, &codex, since).items.len(), 3);
        // Nothing there at all: nothing found, no error.
        let none = scan(&d.join("nope"), &d.join("nope2"), since);
        assert_eq!((none.files, none.claude_found), (0, false));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[tokio::test]
    async fn a_codex_run_counts_once_on_the_usage_page_and_once_in_the_month() {
        let st = AppState::for_test();
        let chat = format!("codex-spend-{}", uuid::Uuid::new_v4());
        let mut run = CodexRun::default();
        run.feed(r#"{"type":"turn.completed","usage":{"input_tokens":200000,"cached_input_tokens":100000,"output_tokens":20000}}"#);
        let est = run.cost("gpt-5.5").expect("a priced model");
        assert!(est > 0.0);
        record_codex(&st, Some(&chat), None, run, Some("gpt-5.5".into())).await;
        // The month's figure (the spending limit) has the estimate once.
        assert!((st.month_cost().await - est).abs() < 1e-9, "{} vs {est}", st.month_cost().await);
        // The usage page reads only the records, never the month's figure: the run is there once.
        let mine: Vec<Record> = read(&crate::paths::home()).into_iter().filter(|r| r.chat.as_deref() == Some(chat.as_str())).collect();
        assert_eq!(mine.len(), 1);
        let page = summarise(&combine(&mine, vec![], &HashMap::new()), &HashMap::new(), Utc::now() - chrono::Duration::days(1), Utc::now());
        let shown = page["totals"]["codex"]["costUsd"].as_f64().unwrap();
        assert!((shown - (est * 10_000.0).round() / 10_000.0).abs() < 1e-9, "{shown} vs {est}");
        // A run with nothing used adds nothing.
        record_codex(&st, Some(&chat), None, CodexRun::default(), Some("gpt-5.5".into())).await;
        assert!((st.month_cost().await - est).abs() < 1e-9);
    }
}
