//! Plan usage without a run: how much of the Claude and ChatGPT plans' limits is used, asked of the
//! assistant programs themselves when the app starts, when its window opens, every few minutes
//! while a window is there and shortly after each run — so the figures are on screen before the
//! first message (`meter.rs` only learned them from a run's output).
//!
//! The two sources, both documented by their makers and both the unmodified programs:
//!
//! - **Claude Code**: its own `/usage` command, run headless (`claude -p /usage --output-format
//!   stream-json`). Claude Code answers it itself, without a request to a model, and attaches the
//!   plan's rows as `usage_report` to the answer (Agent SDK reference, `SDKUsageReport`; Claude Code
//!   2.1.273 and later). Started like every one-shot of the app: no session kept, no tools, none of
//!   the user's hooks or MCP servers.
//! - **Codex**: `codex app-server`, asked `account/read` and `account/rateLimits/read` over its
//!   stdio (app-server reference, "Rate limits (ChatGPT)"). No thread is started, so nothing reaches
//!   Codex's history.
//!
//! What this module never does: read or copy a sign-in token or a credential store, call a service
//! itself, or send anything to a model. Should an installed Claude Code ever hand `/usage` to a
//! model after all (it answers with tokens used and no command named), that is recorded as spent
//! like any run and that version is never asked again (`unsafe_version`).
//!
//! With an API key or another service there is no plan limit to ask for: nothing is started, and
//! the page shows the app's own record of what was used instead (`meter.jsonl`).
//!
//! The last answer per program is kept in `~/.thoughts/usage-watch.json` (when it was asked, when
//! it last worked, why not); the windows themselves stay where they always were
//! (`plan-usage.json`), each with the time it was seen.

use crate::codex_acp::{parse_version, Version};
use crate::meter::{Window, CLAUDE, CODEX};
use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// While a window is there, a known figure is asked again this often (seconds).
pub const EVERY: i64 = 10 * 60;
/// Never twice within this, whatever asks (a window opening, a run ending).
pub const MIN_GAP: i64 = 2 * 60;
/// A run just ended: ask this much later (its own figures arrive first, and runs end in bursts).
pub const AFTER_RUN: i64 = 20;
/// "Nothing to ask here" (no program, not signed in, a key…) is looked at again this often while a
/// window is there, and at once when the window says something changed.
pub const RECHECK: i64 = 60 * 60;
/// A window counts as there for this long after it last asked for the figures.
pub const WINDOW_FOR: i64 = 15 * 60;
/// The first Claude Code whose `/usage` answer carries the plan's rows as data.
pub const CLAUDE_MIN: Version = (2, 1, 273);

/// After `failures` failures in a row: 5, 10, 20, 40, 80 minutes, then every 2 hours.
pub fn backoff(failures: u32) -> i64 { (300_i64 << failures.saturating_sub(1).min(5)).min(2 * 3600) }

/// What is known about asking one program.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Entry {
    /// "" (never asked), "known", "unavailable" (nothing to ask: `reason`) or "failed".
    pub state: String,
    /// Why not: `off`, `missing`, `signedOut`, `notice`, `key`, `service`, `oldProgram`, `noReport`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The last time it was asked (or looked at), and the last time that worked; seconds since 1970.
    pub checked_at: i64,
    pub fetched_at: i64,
    pub failures: u32,
    /// The Claude Code version that handed `/usage` to a model: never asked again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsafe_version: Option<String>,
}

/// Is it time to ask? `forced`: the app started, or a window opened or saw a change. `poke`: a run
/// ended (ask from that time on). `watching`: a window is there.
pub fn due(e: &Entry, now: i64, forced: bool, poke: Option<i64>, watching: bool) -> bool {
    let since = now - e.checked_at;
    let poked = poke.is_some_and(|t| now >= t);
    if e.checked_at == 0 { return forced || watching || poked; }
    match e.state.as_str() {
        // Cheap to look at again (no program is started for the figures), and a sign-in may just have happened.
        "unavailable" => (forced && since >= 15) || (poked && since >= MIN_GAP) || (watching && since >= RECHECK),
        "failed" => since >= backoff(e.failures) && (forced || poked || watching),
        _ => ((forced || poked) && since >= MIN_GAP) || (watching && since >= EVERY),
    }
}

/// What asking came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Known(Vec<(String, Window)>),
    Unavailable(&'static str),
    Failed(String),
    /// Claude Code (this version) gave the question to a model.
    Spent(String),
}

/// Can the program be asked at all?
#[derive(Debug, Clone, PartialEq)]
pub enum Facts { Ask, No(&'static str) }

/// What the watch needs from the world; the tests stand in for it.
pub trait World: Send + Sync {
    fn facts(&self, provider: &'static str, e: &Entry) -> impl std::future::Future<Output = Facts> + Send;
    fn fetch(&self, provider: &'static str) -> impl std::future::Future<Output = Outcome> + Send;
    /// The plan's windows as just reported: they replace what was known.
    fn store(&self, provider: &'static str, windows: Vec<(String, Window)>);
    fn changed(&self);
}

/// What is known, and what is asked for next.
#[derive(Default)]
pub struct Watch {
    file: Option<PathBuf>,
    entries: BTreeMap<String, Entry>,
    busy: HashSet<&'static str>,
    force: HashSet<&'static str>,
    poke: HashMap<&'static str, i64>,
    /// When a window last asked for the figures.
    seen: i64,
}

impl Watch {
    pub fn load(file: PathBuf) -> Self {
        let entries = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Watch { file: Some(file), entries, ..Default::default() }
    }
    fn save(&self) {
        let Some(f) = &self.file else { return };
        let _ = crate::paths::write_atomic(f, serde_json::to_string_pretty(&self.entries).unwrap_or_default().as_bytes(), true);
    }
    pub fn entry(&self, p: &str) -> Entry { self.entries.get(p).cloned().unwrap_or_default() }
    /// A window is there (and, with `refresh`, wants fresh figures for both programs).
    pub fn window(&mut self, now: i64, refresh: bool) { self.seen = now; if refresh { self.force.extend([CLAUDE, CODEX]); } }
    pub fn ran(&mut self, p: &'static str, now: i64) { self.poke.entry(p).or_insert(now + AFTER_RUN); }
    pub fn json(&self) -> Value {
        let one = |p: &'static str| { let e = self.entry(p); json!({"state": e.state, "reason": e.reason, "checkedAt": e.checked_at, "fetchedAt": e.fetched_at,
            // Being asked right now, or about to be for the first time.
            "busy": self.busy.contains(p) || e.checked_at == 0}) };
        json!({CLAUDE: one(CLAUDE), CODEX: one(CODEX)})
    }
}

/// Ask `p` when it is due. `true` when it was asked (or looked at).
pub async fn tick_one<W: World>(w: &Mutex<Watch>, world: &W, p: &'static str, now: i64) -> bool {
    let before = {
        let mut g = w.lock().unwrap_or_else(|e| e.into_inner());
        let e = g.entry(p);
        if g.busy.contains(p) || !due(&e, now, g.force.contains(p), g.poke.get(p).copied(), now - g.seen <= WINDOW_FOR && g.seen > 0) { return false; }
        g.busy.insert(p);
        g.force.remove(p);
        g.poke.remove(p);
        e
    };
    let out = match world.facts(p, &before).await { Facts::No(r) => Outcome::Unavailable(r), Facts::Ask => world.fetch(p).await };
    let mut e = before.clone();
    e.checked_at = now;
    match out {
        Outcome::Known(windows) => { e.state = "known".into(); e.reason = None; e.failures = 0; e.fetched_at = now; world.store(p, windows); }
        Outcome::Unavailable(r) => { e.state = "unavailable".into(); e.reason = Some(r.into()); e.failures = 0; }
        Outcome::Spent(version) => { e.state = "unavailable".into(); e.reason = Some("noReport".into()); e.failures = 0; e.unsafe_version = Some(version); }
        Outcome::Failed(why) => {
            e.state = "failed".into(); e.reason = None; e.failures = e.failures.saturating_add(1);
            // Quietly: once in the log when it starts failing, not on every try.
            if e.failures == 1 { crate::log::info(format!("usage: could not ask {p} for the plan's usage ({why}); trying again later")); }
        }
    }
    let mut g = w.lock().unwrap_or_else(|e| e.into_inner());
    g.busy.remove(p);
    g.entries.insert(p.to_string(), e);
    g.save();
    drop(g);
    world.changed();
    true
}

// ---------- Claude Code: its `/usage` answer ----------

fn slug(s: &str) -> String {
    let t: String = s.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(24).collect();
    t.trim_matches('_').to_string()
}

/// The plan's rows of a `usage_report` (`rate_limits.limits`) as windows, under the names the rest
/// of the app uses: the session row is the 5-hour window, the weekly rows the 7-day ones.
pub fn claude_rows(limits: &[Value], now: i64) -> Vec<(String, Window)> {
    let mut out: Vec<(String, Window)> = vec![];
    for r in limits {
        let Some(pct) = r["percent"].as_f64().filter(|p| p.is_finite()) else { continue };
        let scope = ["model", "surface"].iter().find_map(|k| r["scope"][*k]["display_name"].as_str()).map(str::trim).filter(|s| !s.is_empty());
        let kind = r["kind"].as_str().unwrap_or("");
        let weekly = kind.starts_with("weekly") || r["group"] == "weekly";
        let key = match (kind, scope) {
            ("session", _) => "five_hour".to_string(),
            ("weekly_all", _) => "seven_day".to_string(),
            (_, Some(s)) if weekly => format!("seven_day_{}", slug(s)),
            (k, _) if !slug(k).is_empty() => slug(k),
            _ => continue,
        };
        if out.iter().any(|(k, _)| *k == key) { continue; }
        let resets_at = r["resets_at"].as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp());
        let status = if pct >= 100.0 { "reached" } else if matches!(r["severity"].as_str(), Some("warning" | "critical")) { "warning" } else { "ok" };
        let minutes = if key == "five_hour" { Some(300) } else if weekly { Some(10080) } else { None };
        out.push((key, Window { percent: Some(pct.clamp(0.0, 100.0)), resets_at, minutes, status: Some(status.into()), seen_at: now,
            label: scope.filter(|_| kind != "session" && kind != "weekly_all").map(|s| s.chars().take(40).collect()) }));
    }
    out
}

/// Did this `result` use a model? A command Claude Code answers itself names the command
/// (`local_command`) and costs nothing.
fn spent(result: &Value) -> bool {
    if result.get("local_command").is_some_and(|c| !c.is_null()) { return false; }
    result["total_cost_usd"].as_f64().unwrap_or(0.0) > 0.0 || result["usage"]["output_tokens"].as_u64().unwrap_or(0) > 0
}

/// What `claude -p /usage --output-format stream-json` printed. With the outcome, the `result` to
/// record when a model answered.
pub fn read_claude(stdout: &str, version: &str, now: i64) -> (Outcome, Option<Value>) {
    let (mut report, mut result) = (None, None);
    for l in stdout.lines() {
        if !l.contains("\"usage_report\"") && !l.contains("\"result\"") { continue; }
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        if v["type"] == "assistant" && v["usage_report"].is_object() { report = Some(v["usage_report"].clone()); }
        if v["type"] == "result" { result = Some(v); }
    }
    if let Some(r) = result.as_ref().filter(|r| spent(r)) { return (Outcome::Spent(version.to_string()), Some(r.clone())); }
    if let Some(rep) = report {
        // `null`: Claude Code could not get the plan's usage; no rows: the plan has no meters.
        let rows = rep["rate_limits"]["limits"].as_array().map(|l| claude_rows(l, now)).unwrap_or_default();
        return (if rows.is_empty() { Outcome::Unavailable("noReport") } else { Outcome::Known(rows) }, None);
    }
    match result {
        Some(r) if r["is_error"] == true && crate::claude::is_auth_failure(r["result"].as_str().unwrap_or("")) => (Outcome::Unavailable("signedOut"), None),
        Some(r) if r["is_error"] == true => (Outcome::Failed(r["result"].as_str().unwrap_or("error").chars().take(200).collect()), None),
        // Answered, but with words only: a sign-in that carries no plan (or cannot read it).
        Some(_) => (Outcome::Unavailable("noReport"), None),
        None => (Outcome::Failed("no answer".into()), None),
    }
}

/// The command line that asks Claude Code: nothing kept, no tools, nothing of the user's set-up
/// started (`--disable-slash-commands` is left out on purpose: `/usage` is one).
pub fn claude_args(help: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-p", "/usage", "--output-format", "stream-json", "--verbose", "--no-session-persistence", "--tools", "", "--permission-mode", crate::settings::DEFAULT_PERMISSION_MODE]
        .iter().map(|s| s.to_string()).collect();
    a.extend(crate::claude::one_shot_guard(help).into_iter().filter(|f| f != "--disable-slash-commands"));
    a
}

/// Can Claude Code be asked? (`status` is `claude::status`.)
pub fn claude_facts(on: bool, noticed: bool, status: &Value, help: &str, e: &Entry) -> Facts {
    if !on { return Facts::No("off"); }
    if status["installed"] != true { return Facts::No("missing"); }
    match status["provider"].as_str() { Some("anthropicKey") => return Facts::No("key"), Some("service") => return Facts::No("service"), Some("incomplete") => return Facts::No("signedOut"), _ => {} }
    if status["loggedIn"] != true { return Facts::No("signedOut"); }
    // A sign-in to the Console pays by use: no plan. (Named by the program itself, when it says.)
    let how = status["auth"]["authMethod"].as_str().unwrap_or("").to_lowercase();
    if how.contains("api") || how.contains("console") { return Facts::No("key"); }
    if !noticed { return Facts::No("notice"); }
    let version = status["version"].as_str().unwrap_or("");
    if parse_version(version).is_none_or(|v| v < CLAUDE_MIN) || !crate::claude::supports(help, "--no-session-persistence") { return Facts::No("oldProgram"); }
    if e.unsafe_version.as_deref() == Some(version) { return Facts::No("noReport"); }
    Facts::Ask
}

async fn ask_claude(state: &AppState) -> Outcome {
    let Some(bin) = crate::claude::binary(state).await else { return Outcome::Unavailable("missing") };
    let (Ok(help), Ok(env)) = (crate::claude::run_help(state).await, crate::claude::provider_env(state).await) else { return Outcome::Failed("could not prepare".into()) };
    let version = crate::claude::version(state).await;
    let mut c = crate::claude::cmd(&bin);
    c.current_dir(crate::paths::home()).args(claude_args(&help));
    env.apply(&mut c);
    c.stdin(std::process::Stdio::null());
    let out = match crate::proc::output_timeout(&mut c, Duration::from_secs(45)).await { Ok(o) => o, Err(e) => return Outcome::Failed(e.to_string()) };
    let (outcome, used) = read_claude(&String::from_utf8_lossy(&out.stdout), &version, chrono::Utc::now().timestamp());
    if let Some(r) = used {
        // It cost something after all: counted like any run, said once, and not asked again.
        crate::log::warn(format!("usage: Claude Code {version} gave /usage to a model; its plan usage is not asked for again on this version"));
        state.add_usage(r["total_cost_usd"].as_f64().unwrap_or(0.0)).await;
        crate::meter::record_claude(state, None, &r).await;
    }
    outcome
}

// ---------- Codex: `account/rateLimits/read` of its app-server ----------

fn codex_window(w: &Value) -> Value {
    if !w.is_object() { return Value::Null; }
    json!({"used_percent": w["usedPercent"], "window_minutes": w["windowDurationMins"], "resets_at": w["resetsAt"]})
}

/// The answers to `account/read` and `account/rateLimits/read` (each the whole JSON-RPC message).
pub fn read_codex(account: &Value, limits: &Value, now: chrono::DateTime<chrono::Utc>) -> Outcome {
    let acc = &account["result"]["account"];
    match acc["type"].as_str() {
        Some("chatgpt") => {}
        Some("apiKey") => return Outcome::Unavailable("key"),
        // Another provider's own credentials (a cloud account…): paid by use there.
        Some(_) => return Outcome::Unavailable("service"),
        None if account.get("result").is_some() => return Outcome::Unavailable("signedOut"),
        None => return Outcome::Failed(account["error"]["message"].as_str().unwrap_or("no account answer").chars().take(200).collect()),
    }
    let r = &limits["result"];
    if !r.is_object() { return Outcome::Failed(limits["error"]["message"].as_str().unwrap_or("no answer").chars().take(200).collect()); }
    // Several limits can be reported; Codex's own plan limit is the one shown.
    let bucket = [&r["rateLimitsByLimitId"]["codex"], &r["rateLimits"]].into_iter().find(|b| b.is_object()).cloned().unwrap_or(Value::Null);
    if bucket["limitId"].as_str().is_some_and(|id| !id.eq_ignore_ascii_case("codex")) { return Outcome::Unavailable("noReport"); }
    let windows = crate::meter::codex_windows(&json!({"primary": codex_window(&bucket["primary"]), "secondary": codex_window(&bucket["secondary"])}), now);
    if windows.is_empty() { Outcome::Unavailable("noReport") } else { Outcome::Known(windows) }
}

/// Can Codex be asked? (`status` is `codex::status`, `help` its `--help`.)
pub fn codex_facts(on: bool, noticed: bool, status: &Value, help: &str) -> Facts {
    if !on { return Facts::No("off"); }
    if status["installed"] != true { return Facts::No("missing"); }
    match status["provider"].as_str() { Some("service") => return Facts::No("service"), Some("incomplete") => return Facts::No("signedOut"), _ => {} }
    if status["loggedIn"] != true { return Facts::No("signedOut"); }
    if status["method"].as_str().is_some_and(|m| m != "chatgpt") { return Facts::No("key"); }
    if !noticed { return Facts::No("notice"); }
    // An older Codex would take the word for a prompt and start its terminal screen.
    if !crate::codex::subcommands(help).iter().any(|c| c == "app-server") { return Facts::No("oldProgram"); }
    Facts::Ask
}

/// Start `codex app-server`, shake hands, ask the two questions, end it. Nothing else is sent: no
/// thread, no turn.
pub async fn ask_codex(bin: &Path) -> Outcome {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut c = crate::codex::cmd(bin);
    c.arg("app-server").current_dir(crate::paths::home());
    c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    let mut child = match c.spawn() { Ok(c) => c, Err(e) => return Outcome::Failed(e.to_string()) };
    let pid = child.id();
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return Outcome::Failed("no pipes".into()) };
    let talk = async {
        let mut lines = BufReader::new(stdout).lines();
        let send = |v: Value| { let mut s = v.to_string(); s.push('\n'); s };
        let hello = send(json!({"method": "initialize", "id": 0, "params": {"clientInfo": {"name": "thoughts", "title": "Thoughts", "version": env!("CARGO_PKG_VERSION")}}}));
        stdin.write_all(hello.as_bytes()).await?;
        let (mut account, mut limits) = (None, None);
        while let Some(l) = lines.next_line().await? {
            let Ok(v) = serde_json::from_str::<Value>(&l) else { continue };
            // Only answers matter; what the server announces by itself is not ours to act on.
            match v["id"].as_i64().filter(|_| v.get("method").is_none()) {
                Some(0) => {
                    if v.get("error").is_some() { return Ok((v, Value::Null)); }
                    let rest = send(json!({"method": "initialized", "params": {}})) + &send(json!({"method": "account/read", "id": 1, "params": {"refreshToken": false}}))
                        + &send(json!({"method": "account/rateLimits/read", "id": 2}));
                    stdin.write_all(rest.as_bytes()).await?;
                }
                Some(1) => account = Some(v),
                Some(2) => limits = Some(v),
                _ => {}
            }
            if account.is_some() && limits.is_some() { break; }
        }
        Ok::<_, std::io::Error>((account.unwrap_or(Value::Null), limits.unwrap_or(Value::Null)))
    };
    let got = tokio::time::timeout(Duration::from_secs(30), talk).await;
    if let Some(pid) = pid { crate::proc::end_tree(pid).await; }
    let _ = child.kill().await;
    match got {
        Ok(Ok((account, limits))) => read_codex(&account, &limits, chrono::Utc::now()),
        Ok(Err(e)) => Outcome::Failed(e.to_string()),
        Err(_) => Outcome::Failed("no answer in time".into()),
    }
}

// ---------- the daemon's own watch ----------

struct Real<'a>(&'a Arc<AppState>);

impl World for Real<'_> {
    async fn facts(&self, p: &'static str, e: &Entry) -> Facts {
        let st = self.0;
        let (on, noticed) = { let s = st.settings.read().await; (s.spending.auto_usage(), s.subscription_risk_ack.is_some()) };
        if !on { return Facts::No("off"); }
        if p == CODEX {
            // Nothing is started to find out that the program is not there.
            if crate::codex::binary(st).await.is_none() { return Facts::No("missing"); }
            codex_facts(on, noticed, &crate::codex::status(st).await, &crate::codex::top_help(st).await)
        } else {
            if crate::claude::binary(st).await.is_none() { return Facts::No("missing"); }
            claude_facts(on, noticed, &crate::claude::status(st).await, &crate::claude::run_help(st).await.unwrap_or_default(), e)
        }
    }
    async fn fetch(&self, p: &'static str) -> Outcome {
        if p == CODEX {
            match crate::codex::binary(self.0).await { Some(bin) => ask_codex(&bin).await, None => Outcome::Unavailable("missing") }
        } else { ask_claude(self.0).await }
    }
    fn store(&self, p: &'static str, windows: Vec<(String, Window)>) { crate::meter::set_plans(p, windows); }
    fn changed(&self) { self.0.emit(json!({"type": "meter.changed"})); }
}

/// The daemon's own watch, read from its file on first use.
fn live() -> &'static Mutex<Watch> {
    static W: std::sync::OnceLock<Mutex<Watch>> = std::sync::OnceLock::new();
    W.get_or_init(|| Mutex::new(Watch::load(crate::paths::home().join("usage-watch.json"))))
}

fn now() -> i64 { chrono::Utc::now().timestamp() }

fn check_now(state: &Arc<AppState>) {
    for p in [CLAUDE, CODEX] {
        let st = state.clone();
        tokio::spawn(async move { tick_one(live(), &Real(&st), p, now()).await; });
    }
}

/// Ask once now (the app just started), then keep looking every few seconds whether something is
/// due. Looking costs nothing; a program is only started when `due` says so.
pub fn start(state: Arc<AppState>) {
    live().lock().unwrap_or_else(|e| e.into_inner()).force.extend([CLAUDE, CODEX]);
    tokio::spawn(async move {
        loop {
            check_now(&state);
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}

/// A run of `provider` just ended: its plan's figures have moved.
pub fn after_run(provider: &str) {
    if cfg!(test) { return; }
    let p = if provider == CODEX { CODEX } else { CLAUDE };
    live().lock().unwrap_or_else(|e| e.into_inner()).ran(p, now());
}

/// What the window shows beside the plans' windows: per program, whether it could be asked.
pub fn watch_json() -> Value {
    if cfg!(test) { return Watch::default().json(); }
    live().lock().unwrap_or_else(|e| e.into_inner()).json()
}

type S = State<Arc<AppState>>;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/meter/plans", get(plans_get)).route("/api/meter/refresh", post(refresh))
}

fn answer() -> Json<Value> { Json(json!({"plans": crate::meter::plans_json(now()), "watch": watch_json()})) }

/// The plans' windows and whether each program could be asked. Asking for them says a window is there.
async fn plans_get(State(_): S) -> Json<Value> {
    if !cfg!(test) { live().lock().unwrap_or_else(|e| e.into_inner()).window(now(), false); }
    answer()
}

/// The window opened, or saw the set-up change: fresh figures, as far as `due` allows.
async fn refresh(State(st): S) -> Json<Value> {
    if !cfg!(test) {
        live().lock().unwrap_or_else(|e| e.into_inner()).window(now(), true);
        check_now(&st);
    }
    answer()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The `usage_report` of the Agent SDK reference (`SDKUsageReport`), on the answer to `/usage`.
    const CLAUDE_USAGE: &str = include_str!("../tests/samples/claude-usage.jsonl");
    /// The app-server reference's example answers ("Check auth state", "Rate limits (ChatGPT)").
    const CODEX_LIMITS: &str = include_str!("../tests/samples/codex-rate-limits.json");

    #[test]
    fn claude_codes_usage_answer_becomes_the_plans_windows() {
        let now = 1_800_000_000;
        let (out, used) = read_claude(CLAUDE_USAGE, "2.1.295 (Claude Code)", now);
        assert!(used.is_none(), "answered by Claude Code itself: nothing to record");
        let Outcome::Known(w) = out else { panic!("{out:?}") };
        let by: BTreeMap<_, _> = w.iter().cloned().collect();
        assert_eq!(w.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["five_hour", "seven_day", "seven_day_opus"], "in the server's order");
        let five = &by["five_hour"];
        assert_eq!((five.percent, five.minutes, five.status.as_deref(), five.seen_at), (Some(23.5), Some(300), Some("ok"), now));
        assert_eq!(five.resets_at, Some(chrono::DateTime::parse_from_rfc3339("2026-11-24T18:00:00Z").unwrap().timestamp()));
        assert_eq!(by["seven_day"].status.as_deref(), Some("warning"), "the server's own reading of the row");
        assert_eq!((by["seven_day_opus"].label.as_deref(), by["seven_day_opus"].percent, by["seven_day_opus"].status.as_deref()), (Some("Opus"), Some(100.0), Some("reached")));
        assert!(five.label.is_none());
    }

    #[test]
    fn an_answer_without_rows_is_nothing_to_show_and_never_a_number() {
        let line = |report: Value| format!("{}\n{}\n", json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "…"}]}, "usage_report": report}),
            json!({"type": "result", "subtype": "success", "is_error": false, "local_command": "usage", "total_cost_usd": 0, "usage": {"output_tokens": 0}}));
        // Claude Code could not get the plan's usage / the plan has no meters / a row without a share.
        for report in [json!({"session": {}, "rate_limits": null}), json!({"session": {}, "rate_limits": {"limits": null}}), json!({"session": {}, "rate_limits": {"limits": []}}),
            json!({"session": {}, "rate_limits": {"limits": [{"kind": "session", "group": "session", "resets_at": null}]}})] {
            assert_eq!(read_claude(&line(report.clone()), "2.1.295", 0).0, Outcome::Unavailable("noReport"), "{report}");
        }
        // Words only (an API-key session, an older program).
        let text_only = format!("{}\n{}\n", json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "Total cost: $0.00"}]}}), json!({"type": "result", "is_error": false, "local_command": "usage"}));
        assert_eq!(read_claude(&text_only, "2.1.295", 0).0, Outcome::Unavailable("noReport"));
        let signed_out = json!({"type": "result", "is_error": true, "result": "Not logged in · Please run /login"}).to_string();
        assert_eq!(read_claude(&signed_out, "2.1.295", 0).0, Outcome::Unavailable("signedOut"));
        assert!(matches!(read_claude(&json!({"type": "result", "is_error": true, "result": "API Error: 503"}).to_string(), "2.1.295", 0).0, Outcome::Failed(_)));
        assert!(matches!(read_claude("", "2.1.295", 0).0, Outcome::Failed(_)), "no answer at all is a failure, to be tried again later");
        assert!(matches!(read_claude("not json\n{\"type\":\"system\"}\n", "2.1.295", 0).0, Outcome::Failed(_)));
    }

    #[test]
    fn a_program_that_gave_the_question_to_a_model_is_caught() {
        let out = format!("{}\n{}\n", json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "I can't check that."}]}}),
            json!({"type": "result", "is_error": false, "total_cost_usd": 0.0021, "usage": {"input_tokens": 900, "output_tokens": 12}, "session_id": "s-1"}));
        let (o, used) = read_claude(&out, "2.1.200 (Claude Code)", 0);
        assert_eq!(o, Outcome::Spent("2.1.200 (Claude Code)".into()));
        assert_eq!(used.unwrap()["total_cost_usd"], 0.0021, "what it cost is recorded like any run");
        // The command named, nothing spent: Claude Code answered itself.
        assert!(!spent(&json!({"local_command": "usage", "total_cost_usd": 0, "usage": {"output_tokens": 0}})));
        assert!(!spent(&json!({"total_cost_usd": 0, "usage": {"output_tokens": 0}})));
        assert!(spent(&json!({"total_cost_usd": 0, "usage": {"output_tokens": 3}})));
    }

    const HELP: &str = "Options:\n  --no-session-persistence              Disable session persistence\n  --safe-mode                           Start with all customizations off\n  --strict-mcp-config                   Only use MCP servers from --mcp-config\n  --disable-slash-commands              Disable all skills\n  --disallowedTools, --disallowed-tools <tools...>\n      Comma or space-separated list\n  --permission-prompts <mode>           none\n";

    #[test]
    fn claude_code_is_asked_without_a_session_tools_or_the_users_set_up() {
        let a = claude_args(HELP);
        assert_eq!(&a[..2], ["-p", "/usage"], "the command, right after -p");
        for want in ["--no-session-persistence", "--safe-mode", "--strict-mcp-config", "--permission-prompts"] { assert!(a.contains(&want.to_string()), "{want}"); }
        assert_eq!(a[a.iter().position(|x| x == "--tools").unwrap() + 1], "", "no tools");
        assert!(!a.contains(&"--disable-slash-commands".to_string()), "/usage is a command: switching commands off would switch it off");
        assert!(!a.iter().any(|x| x == "--model" || x == "--resume" || x == "--continue"));
    }

    #[test]
    fn claude_code_is_only_asked_on_a_plan_that_is_signed_in_and_new_enough() {
        let ok = json!({"installed": true, "version": "2.1.295 (Claude Code)", "loggedIn": true, "provider": "account", "auth": {"loggedIn": true, "authMethod": "claude.ai"}});
        let e = Entry::default();
        assert_eq!(claude_facts(true, true, &ok, HELP, &e), Facts::Ask);
        assert_eq!(claude_facts(false, true, &ok, HELP, &e), Facts::No("off"));
        assert_eq!(claude_facts(true, false, &ok, HELP, &e), Facts::No("notice"), "not before the notice about using a subscription here was read");
        let with = |k: &str, v: Value| { let mut s = ok.clone(); s[k] = v; s };
        assert_eq!(claude_facts(true, true, &json!({"installed": false}), HELP, &e), Facts::No("missing"));
        assert_eq!(claude_facts(true, true, &with("loggedIn", json!(false)), HELP, &e), Facts::No("signedOut"));
        assert_eq!(claude_facts(true, true, &with("provider", json!("anthropicKey")), HELP, &e), Facts::No("key"));
        assert_eq!(claude_facts(true, true, &with("provider", json!("service")), HELP, &e), Facts::No("service"));
        assert_eq!(claude_facts(true, true, &with("auth", json!({"authMethod": "console_api_key"})), HELP, &e), Facts::No("key"));
        assert_eq!(claude_facts(true, true, &with("version", json!("2.1.272 (Claude Code)")), HELP, &e), Facts::No("oldProgram"));
        assert_eq!(claude_facts(true, true, &with("version", json!("")), HELP, &e), Facts::No("oldProgram"), "a version that cannot be read is not trusted");
        assert_eq!(claude_facts(true, true, &ok, "Options:\n  --safe-mode   x\n", &e), Facts::No("oldProgram"), "it would keep a session");
        let burnt = Entry { unsafe_version: Some("2.1.295 (Claude Code)".into()), ..Default::default() };
        assert_eq!(claude_facts(true, true, &ok, HELP, &burnt), Facts::No("noReport"));
        assert_eq!(claude_facts(true, true, &with("version", json!("2.1.296 (Claude Code)")), HELP, &burnt), Facts::Ask, "a newer program is asked again");
    }

    #[test]
    fn codexs_rate_limits_become_the_plans_windows() {
        let v: Value = serde_json::from_str(CODEX_LIMITS).unwrap();
        let at = chrono::DateTime::from_timestamp(1_730_940_000, 0).unwrap();
        let Outcome::Known(w) = read_codex(&v["accountChatgpt"], &v["rateLimits"], at) else { panic!() };
        assert_eq!(w.len(), 2);
        assert_eq!((w[0].0.as_str(), w[0].1.percent, w[0].1.resets_at, w[0].1.minutes, w[0].1.seen_at), ("five_hour", Some(25.0), Some(1_730_947_200), Some(300), 1_730_940_000));
        assert_eq!((w[1].0.as_str(), w[1].1.percent, w[1].1.status.as_deref()), ("seven_day", Some(100.0), Some("reached")));
        // The documentation's own example: one window, of a length with no name of its own.
        let Outcome::Known(doc) = read_codex(&v["accountChatgpt"], &v["rateLimitsDoc"], at) else { panic!() };
        assert_eq!((doc.len(), doc[0].0.as_str(), doc[0].1.percent), (1, "m15", Some(25.0)), "the other limit (codex_other) is not Codex's plan limit");
        assert_eq!(read_codex(&v["accountApiKey"], &v["rateLimits"], at), Outcome::Unavailable("key"));
        assert_eq!(read_codex(&v["accountNone"], &Value::Null, at), Outcome::Unavailable("signedOut"));
        assert_eq!(read_codex(&json!({"id": 1, "result": {"account": {"type": "amazonBedrock"}}}), &Value::Null, at), Outcome::Unavailable("service"));
        assert!(matches!(read_codex(&v["accountChatgpt"], &json!({"id": 2, "error": {"code": -32600, "message": "chatgpt authentication required"}}), at), Outcome::Failed(_)));
        assert!(matches!(read_codex(&Value::Null, &Value::Null, at), Outcome::Failed(_)));
        assert_eq!(read_codex(&v["accountChatgpt"], &json!({"id": 2, "result": {"rateLimits": {"limitId": "codex", "primary": null, "secondary": null}}}), at), Outcome::Unavailable("noReport"));
    }

    #[test]
    fn codex_is_only_asked_on_a_chatgpt_sign_in_with_an_app_server() {
        let help = "Commands:\n  exec        Run Codex non-interactively\n  app-server  Run the app server\n  help        Print this message\n\nOptions:\n";
        let ok = json!({"installed": true, "loggedIn": true, "method": "chatgpt", "provider": "own"});
        assert_eq!(codex_facts(true, true, &ok, help), Facts::Ask);
        assert_eq!(codex_facts(false, true, &ok, help), Facts::No("off"));
        assert_eq!(codex_facts(true, false, &ok, help), Facts::No("notice"));
        assert_eq!(codex_facts(true, true, &ok, "Commands:\n  exec   Run\n  proto  Run\n"), Facts::No("oldProgram"), "0.27 has no app-server");
        assert_eq!(codex_facts(true, true, &json!({"installed": false}), help), Facts::No("missing"));
        assert_eq!(codex_facts(true, true, &json!({"installed": true, "loggedIn": false, "provider": "own"}), help), Facts::No("signedOut"));
        assert_eq!(codex_facts(true, true, &json!({"installed": true, "loggedIn": true, "method": "apiKey", "provider": "own"}), help), Facts::No("key"));
        assert_eq!(codex_facts(true, true, &json!({"installed": true, "loggedIn": true, "provider": "service"}), help), Facts::No("service"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn codexs_app_server_is_asked_two_questions_and_nothing_else() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("thoughts-plan-watch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let (bin, said) = (d.join("codex"), d.join("said.jsonl"));
        // A stand-in app-server: it keeps every line it was sent, and answers by the method named.
        std::fs::write(&bin, format!(r#"#!/bin/sh
[ "$1" = "app-server" ] || exit 9
while IFS= read -r line; do
  printf '%s\n' "$line" >> '{}'
  case "$line" in
    *'"initialize"'*) echo '{{"id":0,"result":{{"userAgent":"stand-in"}}}}' ;;
    *'"account/read"'*) echo '{{"method":"account/updated","params":{{"authMode":"chatgpt"}}}}'; echo '{{"id":1,"result":{{"account":{{"type":"chatgpt","email":null,"planType":"pro"}},"requiresOpenaiAuth":true}}}}' ;;
    *'"account/rateLimits/read"'*) echo '{{"id":2,"result":{{"rateLimits":{{"limitId":"codex","primary":{{"usedPercent":12,"windowDurationMins":300,"resetsAt":1900000000}},"secondary":{{"usedPercent":40,"windowDurationMins":10080,"resetsAt":1900500000}}}}}}}}' ;;
  esac
done
"#, said.display())).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let Outcome::Known(w) = ask_codex(&bin).await else { panic!("no windows") };
        assert_eq!(w.iter().map(|(k, w)| (k.as_str(), w.percent)).collect::<Vec<_>>(), [("five_hour", Some(12.0)), ("seven_day", Some(40.0))]);
        let sent: Vec<Value> = std::fs::read_to_string(&said).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(sent.iter().map(|v| v["method"].as_str().unwrap()).collect::<Vec<_>>(), ["initialize", "initialized", "account/read", "account/rateLimits/read"],
            "the handshake and the two questions: no thread, no turn, no prompt");
        assert_eq!(sent[2]["params"]["refreshToken"], false);
        // A program that says nothing is given up on, and one that is not there is a failure, not a crash.
        assert!(matches!(ask_codex(&d.join("nothing-here")).await, Outcome::Failed(_)));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_schedule_asks_seldom_and_backs_off() {
        let never = Entry::default();
        assert!(due(&never, 1000, true, None, false), "at start");
        assert!(due(&never, 1000, false, None, true) && !due(&never, 1000, false, None, false));
        let known = Entry { state: "known".into(), checked_at: 1000, fetched_at: 1000, ..Default::default() };
        // Nobody looking: never by itself, however long ago.
        assert!(!due(&known, 1000 + 86_400, false, None, false));
        // A window is there: every ten minutes.
        assert!(!due(&known, 1000 + EVERY - 1, false, None, true) && due(&known, 1000 + EVERY, false, None, true));
        // A window opening or a run ending: at once, but never twice within two minutes.
        assert!(!due(&known, 1000 + MIN_GAP - 1, true, None, true) && due(&known, 1000 + MIN_GAP, true, None, false));
        assert!(!due(&known, 1000 + MIN_GAP, false, Some(1000 + MIN_GAP + 5), false), "the run's own figures first");
        assert!(due(&known, 1000 + MIN_GAP + 5, false, Some(1000 + MIN_GAP + 5), false));
        // Failing: 5, 10, 20, 40, 80 minutes, then every two hours, whoever asks.
        assert_eq!((1..=8).map(backoff).collect::<Vec<_>>(), [300, 600, 1200, 2400, 4800, 7200, 7200, 7200]);
        let failed = |n| Entry { state: "failed".into(), checked_at: 1000, failures: n, ..Default::default() };
        assert!(!due(&failed(1), 1000 + 299, true, Some(0), true) && due(&failed(1), 1000 + 300, false, None, true));
        assert!(!due(&failed(3), 1000 + 1199, true, None, true) && due(&failed(3), 1000 + 1200, true, None, false));
        assert!(!due(&failed(1), 1000 + 9999, false, None, false), "and not at all when nobody is looking");
        // Nothing to ask: looked at again when the window says something changed, else hourly.
        let none = Entry { state: "unavailable".into(), reason: Some("missing".into()), checked_at: 1000, ..Default::default() };
        assert!(!due(&none, 1000 + 14, true, None, true) && due(&none, 1000 + 15, true, None, false));
        assert!(!due(&none, 1000 + RECHECK - 1, false, None, true) && due(&none, 1000 + RECHECK, false, None, true));
        assert!(!due(&none, 1000 + 86_400, false, None, false));
    }

    /// A world where the tests decide what is set up and what asking gives, and count the asking.
    #[derive(Default)]
    struct Fake {
        facts: Mutex<HashMap<&'static str, Facts>>,
        answers: Mutex<HashMap<&'static str, Outcome>>,
        fetched: AtomicUsize,
        stored: Mutex<Vec<(&'static str, Vec<(String, Window)>)>>,
        changes: AtomicUsize,
    }
    impl World for Fake {
        async fn facts(&self, p: &'static str, _: &Entry) -> Facts { self.facts.lock().unwrap().get(p).cloned().unwrap_or(Facts::No("missing")) }
        async fn fetch(&self, p: &'static str) -> Outcome { self.fetched.fetch_add(1, Ordering::SeqCst); self.answers.lock().unwrap().get(p).cloned().unwrap_or(Outcome::Failed("down".into())) }
        fn store(&self, p: &'static str, w: Vec<(String, Window)>) { self.stored.lock().unwrap().push((p, w)); }
        fn changed(&self) { self.changes.fetch_add(1, Ordering::SeqCst); }
    }
    fn win(pct: f64, at: i64) -> Vec<(String, Window)> { vec![("five_hour".into(), Window { percent: Some(pct), seen_at: at, ..Default::default() })] }

    #[tokio::test]
    async fn nothing_is_asked_when_nothing_is_set_up() {
        let d = std::env::temp_dir().join(format!("thoughts-plan-watch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let w = Mutex::new(Watch::load(d.join("usage-watch.json")));
        let fake = Fake::default();
        w.lock().unwrap().force.extend([CLAUDE, CODEX]);
        // The app starts, a window opens and stays for a day, runs end: no program is ever started.
        for now in (1000..1000 + 86_400).step_by(10) {
            if now % 300 == 0 { w.lock().unwrap().window(now, now % 3600 == 0); }
            for p in [CLAUDE, CODEX] { tick_one(&w, &fake, p, now).await; }
        }
        assert_eq!(fake.fetched.load(Ordering::SeqCst), 0);
        assert!(fake.stored.lock().unwrap().is_empty());
        let j = w.lock().unwrap().json();
        assert_eq!((j["claude"]["state"].as_str(), j["claude"]["reason"].as_str(), j["codex"]["reason"].as_str()), (Some("unavailable"), Some("missing"), Some("missing")));
        assert_eq!(j["claude"]["busy"], false);
        // And for every other reason there is nothing to ask.
        for reason in ["off", "signedOut", "notice", "key", "service", "oldProgram"] {
            fake.facts.lock().unwrap().insert(CLAUDE, Facts::No(reason));
            w.lock().unwrap().window(200_000, true);
            assert!(tick_one(&w, &fake, CLAUDE, 200_000).await, "{reason}");
            assert_eq!(w.lock().unwrap().entry(CLAUDE).reason.as_deref(), Some(reason));
            w.lock().unwrap().entries.clear();
        }
        assert_eq!(fake.fetched.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn the_watch_asks_keeps_what_it_learned_and_backs_off_quietly() {
        let d = std::env::temp_dir().join(format!("thoughts-plan-watch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let file = d.join("usage-watch.json");
        let w = Mutex::new(Watch::load(file.clone()));
        let fake = Fake::default();
        fake.facts.lock().unwrap().insert(CLAUDE, Facts::Ask);
        fake.answers.lock().unwrap().insert(CLAUDE, Outcome::Known(win(10.0, 1000)));
        assert_eq!(w.lock().unwrap().json()["claude"]["busy"], true, "never asked yet: shown as loading, not as empty");
        // The app starts: asked once, before anything was sent.
        w.lock().unwrap().force.extend([CLAUDE, CODEX]);
        assert!(tick_one(&w, &fake, CLAUDE, 1000).await);
        assert!(tick_one(&w, &fake, CODEX, 1000).await, "looked at: not set up");
        assert_eq!(fake.fetched.load(Ordering::SeqCst), 1);
        assert_eq!(fake.stored.lock().unwrap().len(), 1);
        assert_eq!(fake.changes.load(Ordering::SeqCst), 2, "the window is told");
        // With no window there, nothing more happens.
        for now in (1010..9000).step_by(10) { assert!(!tick_one(&w, &fake, CLAUDE, now).await); }
        // A window is there: every ten minutes, not more.
        let mut asked = 0;
        for now in (9000..9000 + 3600).step_by(10) {
            if now % 300 == 0 { w.lock().unwrap().window(now, false); }
            if tick_one(&w, &fake, CLAUDE, now).await { asked += 1; }
        }
        assert_eq!(asked, 6, "an hour with a window open");
        // A run ends: asked again soon after, once, however many runs ended.
        let t = w.lock().unwrap().entry(CLAUDE).checked_at + MIN_GAP;
        for _ in 0..5 { w.lock().unwrap().ran(CLAUDE, t); }
        assert!(!tick_one(&w, &fake, CLAUDE, t + AFTER_RUN - 1).await);
        assert!(tick_one(&w, &fake, CLAUDE, t + AFTER_RUN).await);
        assert!(!tick_one(&w, &fake, CLAUDE, t + AFTER_RUN + 10).await);
        // Kept on disk: the next start knows when the figures are from.
        let again = Watch::load(file.clone());
        assert_eq!((again.entry(CLAUDE).state.as_str(), again.entry(CLAUDE).fetched_at), ("known", t + AFTER_RUN));
        assert_eq!(again.json()["claude"]["busy"], false);
        // It starts failing: the last good time stays, the tries thin out, nothing is stored.
        fake.answers.lock().unwrap().insert(CLAUDE, Outcome::Failed("offline".into()));
        let stored = fake.stored.lock().unwrap().len();
        let (mut now, mut tries) = (t + AFTER_RUN + EVERY, vec![]);
        let start = now;
        while now < start + 6 * 3600 {
            w.lock().unwrap().window(now, true);
            if tick_one(&w, &fake, CLAUDE, now).await { tries.push(now - start); }
            now += 10;
        }
        assert_eq!(tries, [0, 300, 900, 2100, 4500, 9300, 16500], "5, 10, 20, 40, 80, 120 minutes apart");
        let e = w.lock().unwrap().entry(CLAUDE);
        assert_eq!((e.state.as_str(), e.fetched_at, e.failures), ("failed", t + AFTER_RUN, 7));
        assert_eq!(fake.stored.lock().unwrap().len(), stored, "a failure never replaces a figure");
        // It works again: back to normal at once.
        fake.answers.lock().unwrap().insert(CLAUDE, Outcome::Known(win(55.0, now)));
        let at = e.checked_at + backoff(7);
        w.lock().unwrap().window(at, true);
        assert!(tick_one(&w, &fake, CLAUDE, at).await);
        let e = w.lock().unwrap().entry(CLAUDE);
        assert_eq!((e.failures, e.fetched_at), (0, at));
        // A program that gave the question to a model is remembered (the real world then never asks that version again).
        fake.answers.lock().unwrap().insert(CLAUDE, Outcome::Spent("2.1.200".into()));
        w.lock().unwrap().window(at + EVERY, true);
        assert!(tick_one(&w, &fake, CLAUDE, at + EVERY).await);
        let e = Watch::load(file).entry(CLAUDE);
        assert_eq!((e.state.as_str(), e.reason.as_deref(), e.unsafe_version.as_deref()), ("unavailable", Some("noReport"), Some("2.1.200")));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn the_window_gets_the_windows_and_the_state_of_asking_in_one_answer() {
        let st = Arc::new(AppState::for_test());
        for j in [plans_get(State(st.clone())).await.0, refresh(State(st.clone())).await.0] {
            assert!(j["plans"].is_object() && j["plans"].get("claude").is_some() && j["plans"].get("codex").is_some());
            for p in [CLAUDE, CODEX] { assert_eq!((j["watch"][p]["state"].as_str(), j["watch"][p]["busy"].as_bool()), (Some(""), Some(true)), "never asked: loading"); }
        }
        // A whole report replaces what was known (a window the plan no longer has does not linger).
        let p = "plan-watch-test";
        crate::meter::set_plans(p, vec![("five_hour".into(), Window { percent: Some(1.0), seen_at: 5, ..Default::default() }), ("seven_day_opus".into(), Window { percent: Some(2.0), seen_at: 5, label: Some("Opus".into()), ..Default::default() })]);
        crate::meter::set_plans(p, win(30.0, 9));
        let kept: Value = serde_json::from_str(&std::fs::read_to_string(crate::meter::plans_file(&crate::paths::home())).unwrap()).unwrap();
        assert_eq!(kept[p], json!({"five_hour": {"percent": 30.0, "resetsAt": null, "minutes": null, "status": null, "seenAt": 9}}), "kept on disk with its time, for the next start");
        // Off by default? No: on, until the user switches it off; a save that does not name it keeps it.
        let mut s = crate::settings::SpendingSettings::default();
        assert!(s.auto_usage());
        s.auto_usage = Some(false);
        assert!(!s.auto_usage());
    }

    #[tokio::test]
    async fn one_question_at_a_time() {
        struct Slow(AtomicUsize);
        impl World for Slow {
            async fn facts(&self, _: &'static str, _: &Entry) -> Facts { Facts::Ask }
            async fn fetch(&self, _: &'static str) -> Outcome { self.0.fetch_add(1, Ordering::SeqCst); tokio::time::sleep(Duration::from_millis(80)).await; Outcome::Known(vec![]) }
            fn store(&self, _: &'static str, _: Vec<(String, Window)>) {}
            fn changed(&self) {}
        }
        let w = Mutex::new(Watch::default());
        let slow = Slow(AtomicUsize::new(0));
        w.lock().unwrap().force.insert(CLAUDE);
        let (a, b) = tokio::join!(tick_one(&w, &slow, CLAUDE, 1000), async { tokio::time::sleep(Duration::from_millis(20)).await; w.lock().unwrap().force.insert(CLAUDE); tick_one(&w, &slow, CLAUDE, 1000).await });
        assert!(a && !b);
        assert_eq!(slow.0.load(Ordering::SeqCst), 1);
    }
}
