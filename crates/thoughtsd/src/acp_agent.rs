//! Running the ACP adapters (`acp_install.rs`) with the ACP client (`acp.rs`): starting one with the
//! user's own program and provider, reading what it offers (`probe`, cached per program and
//! provider), and driving one turn of an ordinary Claude Code chat (`open_chat` + `drive`).

use crate::acp::{self, Catalog, Conn, Incoming};
use crate::assistant::Program;
use crate::claude::{Ended, Followed};
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::mpsc::UnboundedReceiver;

/// The adapter for `program`, ready to start: the machine's Node.js running the pinned adapter,
/// pointed at the user's own installed program and the provider chosen in Settings.
pub async fn adapter_command(state: &AppState, program: Program, entry: &Path) -> anyhow::Result<Command> {
    let node = crate::claude::find_binary(&None, "node").ok_or_else(|| anyhow::anyhow!("Node.js was not found"))?;
    let mut c = crate::proc::command(&node);
    c.arg(entry);
    c.env("PATH", crate::claude::shell_path());
    c.env(crate::claude::APP_RUN_ENV, "1");
    for (name, value) in crate::claude::quiet_env(crate::claude::sends_reports()) { c.env(name, value); }
    match program {
        Program::Claude => {
            let bin = crate::claude::binary(state).await.ok_or_else(|| anyhow::anyhow!("Claude Code CLI not found"))?;
            c.env("CLAUDE_CODE_EXECUTABLE", bin);
            crate::claude::provider_env(state).await?.apply(&mut c);
        }
        Program::Codex => {
            let bin = crate::codex::binary(state).await.ok_or_else(|| anyhow::anyhow!("Codex not found"))?;
            c.env("CODEX_PATH", bin).env("NO_BROWSER", "1");
            if let Some(svc) = crate::providers::codex_service(state).await? {
                let text = std::fs::read_to_string(crate::codex::codex_home().join("config.toml")).unwrap_or_default();
                c.env("CODEX_CONFIG", crate::providers::codex_config_json(&svc, &crate::providers::user_excludes(&text)).to_string());
                crate::providers::apply_codex_env(&mut c, Some(&svc));
            }
        }
    }
    c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    c.kill_on_drop(true);
    crate::proc::own_group(&mut c);
    Ok(c)
}

/// Start `c` and say hello over ACP.
pub async fn connect(c: Command) -> anyhow::Result<(Child, Arc<Conn>, UnboundedReceiver<Incoming>)> {
    connect_told(c).await.map(|(child, conn, rx, _)| (child, conn, rx))
}

/// `connect`, with what the agent answered to `initialize` (its capabilities).
pub async fn connect_told(mut c: Command) -> anyhow::Result<(Child, Arc<Conn>, UnboundedReceiver<Incoming>, Value)> {
    let mut child = c.spawn()?;
    let (Some(out), Some(inp)) = (child.stdout.take(), child.stdin.take()) else { anyhow::bail!("the adapter has no connection") };
    let (conn, rx) = Conn::start(out, inp);
    match tokio::time::timeout(std::time::Duration::from_secs(60), acp::initialize(&conn)).await {
        Ok(Ok(told)) => Ok((child, conn, rx, told)),
        Ok(Err(e)) => { end(&mut child).await; Err(e) }
        Err(_) => { end(&mut child).await; anyhow::bail!("the adapter did not answer") }
    }
}

pub async fn end(child: &mut Child) {
    if let Some(pid) = child.id() { crate::proc::end_tree(pid).await; }
    let _ = child.kill().await;
}

// ---------- what an agent offers ----------

fn cache() -> &'static std::sync::Mutex<(HashMap<String, Catalog>, HashSet<String>)> {
    static C: std::sync::OnceLock<std::sync::Mutex<(HashMap<String, Catalog>, HashSet<String>)>> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

/// Whose catalog: the program and where its requests go (another provider has other models).
async fn key(state: &AppState, program: Program) -> String {
    let s = state.settings.read().await;
    match program {
        Program::Claude => format!("claude|{}", crate::claude::destination(s.ai.claude.base_url.as_deref()).unwrap_or("")),
        Program::Codex => format!("codex|{}", crate::providers::codex_destination(s.ai.codex.base_url.as_deref()).unwrap_or("")),
    }
}

/// What `program` offers, as last read. Missing (or `refresh`): read again in the background —
/// installing the adapter first if needed — and `acp.options` is announced when it is in.
pub async fn catalog(state: &Arc<AppState>, program: Program, refresh: bool) -> Option<Catalog> {
    let k = key(state, program).await;
    let have = cache().lock().unwrap_or_else(|e| e.into_inner()).0.get(&k).cloned();
    if have.is_some() && !refresh { return have; }
    if cache().lock().unwrap_or_else(|e| e.into_inner()).1.insert(k.clone()) {
        let st = state.clone();
        tokio::spawn(async move {
            match probe(&st, program).await {
                Ok(c) => {
                    cache().lock().unwrap_or_else(|e| e.into_inner()).0.insert(k.clone(), c);
                    st.emit(json!({"type": "acp.options", "program": program.name()}));
                }
                Err(e) => crate::log::warn(format!("could not read what {} offers over ACP: {e}", program.label())),
            }
            cache().lock().unwrap_or_else(|e| e.into_inner()).1.remove(&k);
        });
    }
    have
}

/// Ask the adapter what a session would offer, in an empty folder of the app's own, without keeping
/// a session (`session/options`; `session/new` where the adapter has no such method).
pub async fn probe(state: &AppState, program: Program) -> anyhow::Result<Catalog> {
    // The Codex adapter starts `codex app-server`, which an older Codex does not have (0.2x would take
    // the word for a prompt): it is not started at all for a Codex older than the one it is made for.
    let entry = match acp_ready(program) { Some(e) => e, None => crate::acp_install::install(program).await? };
    if program == Program::Codex {
        if let Err(stay) = crate::codex_acp::codex_fits(state, &entry).await { anyhow::bail!("{}", stay.why()); }
    }
    let dir = crate::paths::run_tmp_dir().join(format!("acp-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let res = async {
        let (mut child, conn, mut rx) = connect(adapter_command(state, program, &entry).await?).await?;
        // Nothing it asks while being probed is answered with a yes.
        let c2 = conn.clone();
        let refuse = tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                if let Incoming::Request { id, method, .. } = m {
                    let _ = if method == "session/request_permission" { c2.respond(&id, acp::permission_answer(None)).await }
                        else { c2.respond_error(&id, -32601, "not offered by this client").await };
                }
            }
        });
        let params = json!({"cwd": dir, "mcpServers": []});
        let ask = async {
            match conn.request("session/options", params.clone()).await {
                Ok(r) => Ok(r),
                Err(_) => conn.request("session/new", params.clone()).await,
            }
        };
        let r = tokio::time::timeout(std::time::Duration::from_secs(90), ask).await;
        end(&mut child).await;
        refuse.abort();
        match r { Ok(r) => Ok(acp::catalog(&r?)), Err(_) => anyhow::bail!("the adapter did not answer in time") }
    }.await;
    let _ = std::fs::remove_dir_all(&dir);
    res
}

fn acp_ready(program: Program) -> Option<PathBuf> { crate::acp_install::ready(program) }

// ---------- one turn of an ordinary chat ----------

/// What an ordinary Claude Code chat's turn is started with: everything `claude::start_run` decided.
pub struct ChatSpec {
    pub cwd: PathBuf,
    pub add_dirs: Vec<PathBuf>,
    pub resume: Option<String>,
    pub fork: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The permission mode, already checked (`check_bypass`, `bypass.chat_only`, full access).
    pub mode: String,
    /// The private file holding the run's rules and command-check hooks.
    pub rules: Option<PathBuf>,
    pub system: String,
    pub budget: Option<f64>,
    pub prompt: String,
    /// The profile's guards that the command line passes as flags (`claude::profile_flags`):
    /// which of Claude Code's setting sources load (`None`: all of them; `Some("")`: none), and
    /// whether no MCP server may start (`--strict-mcp-config`).
    pub setting_sources: Option<String>,
    pub strict_mcp: bool,
}

/// The SDK options the adapter starts Claude Code with: the rules file, the app's notes on top of
/// Claude Code's own system prompt, and the switch that keeps "bypass permissions" out of reach of
/// a session that may not use it.
pub fn claude_options(s: &ChatSpec) -> Value {
    let bypass = s.mode == crate::settings::BYPASS;
    let mut o = json!({
        "systemPrompt": {"type": "preset", "preset": "claude_code", "append": s.system},
        "allowDangerouslySkipPermissions": bypass,
        "promptSuggestions": false,
    });
    if let Some(r) = &s.rules { o["settings"] = json!(r); }
    if let Some(b) = s.budget.filter(|b| b.is_finite() && *b > 0.0) { o["maxBudgetUsd"] = json!(b); }
    if let Some(m) = s.model.as_ref().filter(|m| !m.is_empty()) { o["model"] = json!(m); }
    if let Some(e) = s.effort.as_ref().filter(|e| !e.is_empty()) { o["effort"] = json!(e); }
    // The adapter starts the SDK with every setting source unless told otherwise.
    if let Some(src) = &s.setting_sources {
        o["settingSources"] = json!(src.split(',').map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>());
    }
    // No connected tools: only the (empty) servers handed over start, and their tools are refused too.
    if s.strict_mcp {
        o["strictMcpConfig"] = json!(true);
        o["mcpServers"] = json!({});
        o["disallowedTools"] = json!(["mcp__*"]);
    }
    o
}

/// Can the ACP path carry every guard of profile `p`? `--restricted` (no own set-up and no commands)
/// has no checked equivalent over the adapter: such a run stays on the command line.
pub fn carries(p: &crate::claude::Profile) -> bool { p.own_setup || p.shell }

/// The request that opens the session of a turn: a new one, the chat's own (`session/resume`), or a
/// copy of it for a branch (`session/fork`).
pub fn open_request(s: &ChatSpec) -> (&'static str, Value) {
    let mut p = json!({"cwd": s.cwd, "mcpServers": [], "additionalDirectories": s.add_dirs,
        "_meta": {"claudeCode": {"options": claude_options(s)}}});
    let method = match &s.resume {
        Some(id) => { p["sessionId"] = json!(id); if s.fork { "session/fork" } else { "session/resume" } }
        None => "session/new",
    };
    (method, p)
}

/// Open the session and put it in the chat's mode, model and effort. The mode must be one the agent
/// offers and must be what it reports afterwards, or the turn does not start.
pub async fn open_chat(conn: &Conn, s: &ChatSpec) -> anyhow::Result<(String, Catalog)> {
    let (method, params) = open_request(s);
    let r = conn.request(method, params).await?;
    let session = r["sessionId"].as_str().map(String::from).or_else(|| s.resume.clone().filter(|_| !s.fork))
        .ok_or_else(|| anyhow::anyhow!("the agent opened no session"))?;
    let cat = acp::catalog(&r);
    settle(conn, &session, &cat, &s.mode, s.model.as_deref(), s.effort.as_deref()).await?;
    Ok((session, cat))
}

/// Put an open session in the chat's mode (which must be offered: else an error, and the turn does
/// not start), model and effort (each only when offered).
pub async fn settle(conn: &Conn, session: &str, cat: &Catalog, mode: &str, model: Option<&str>, effort: Option<&str>) -> anyhow::Result<()> {
    if cat.current_mode.as_deref() != Some(mode) {
        acp::choose(conn, session, cat.mode_option.as_deref(), &cat.modes, mode, true).await
            .map_err(|e| anyhow::anyhow!("the chat's permission mode could not be set: {e}"))?;
    }
    if let Some(m) = model.filter(|m| !m.is_empty() && cat.current_model.as_deref() != Some(*m)) {
        if cat.models.iter().any(|c| c.id == m) { let _ = acp::choose(conn, session, cat.model_option.as_deref(), &cat.models, m, false).await; }
    }
    if let Some(e) = effort.filter(|e| !e.is_empty() && cat.current_effort.as_deref() != Some(*e)) {
        if let Some(o) = cat.effort_option.as_deref() { let _ = acp::choose(conn, session, Some(o), &cat.efforts, e, false).await; }
    }
    Ok(())
}

/// Who answers the agent's permission requests: the app's approval cards for a chat that asks, a
/// refusal otherwise (as `--permission-prompts none` on the command line).
#[derive(Clone)]
pub struct Asker {
    pub token: Option<String>,
    /// The chat's checked permission mode: the session never goes on in a mode that does more.
    pub mode: String,
    /// Full access is on: this guard yields, like every other.
    pub full: bool,
}

async fn answer_permission(state: Arc<AppState>, conn: Arc<Conn>, id: Value, params: Value, asker: Asker) {
    let call = &params["toolCall"];
    let allow = match &asker.token {
        Some(token) => {
            let req = crate::approvals::Request {
                tool: acp::tool_name(call),
                input: if call["rawInput"].is_object() { call["rawInput"].clone() } else { json!({}) },
                tool_use_id: call["toolCallId"].as_str().map(String::from),
            };
            matches!(crate::approvals::ask(&state, token, req, crate::approvals::WAIT).await, crate::approvals::Verdict::Allow)
        }
        None => false,
    };
    // A yes with no safe option to say it with is answered "cancelled", like a no without one.
    let _ = conn.respond(&id, acp::permission_answer(acp::pick_option(&params["options"], allow, &asker.mode, asker.full))).await;
}

/// One turn: send the prompt, show what comes back in the chat (`claude::handle_event`), answer
/// permission requests, and end it all — the adapter and whatever it started — when the turn is
/// over or Stop is pressed. The outcome reads like the command-line path's (`claude::follow`).
pub async fn drive(state: &Arc<AppState>, chat_id: &str, child: Child, conn: Arc<Conn>, rx: UnboundedReceiver<Incoming>,
    session: String, prompt: String, asker: Asker, kill: &mut tokio::sync::oneshot::Receiver<()>) -> Followed {
    let blocks = vec![json!({"type": "text", "text": prompt})];
    drive_with(state, chat_id, child, conn, rx, session, blocks, asker, kill, View::Claude(Default::default())).await.0
}

/// How a turn's updates reach the chat: Claude Code's as its own `stream-json` events
/// (`acp::Translator` → `claude::handle_event`), Codex's as the events its command-line runs report
/// (`codex_acp::View` → `codex::handle`), so a chat looks the same whichever way it ran.
pub enum View { Claude(acp::Translator), Codex(crate::codex_acp::View) }

impl View {
    async fn start(&mut self, state: &AppState, chat_id: &str, session: &str) {
        match self {
            View::Claude(_) => { crate::claude::handle_event(state, chat_id, &json!({"type": "system", "subtype": "init", "session_id": session})).await; }
            View::Codex(v) => v.start(state, chat_id, session).await,
        }
    }

    async fn update(&mut self, state: &AppState, chat_id: &str, u: &Value) {
        match self {
            View::Claude(tr) => for ev in tr.update(u) { crate::claude::handle_event(state, chat_id, &ev).await; },
            View::Codex(v) => v.update(state, chat_id, u).await,
        }
    }

    fn mode(&self) -> Option<&str> {
        match self { View::Claude(tr) => tr.mode.as_deref(), View::Codex(v) => v.mode.as_deref() }
    }

    /// The end of the turn (`Ok`: the answer to `session/prompt`): `Some(success)` once its result is shown.
    async fn end(&mut self, state: &AppState, chat_id: &str, stop: Result<&Value, String>, session: &str) -> Option<bool> {
        match self {
            View::Claude(tr) => {
                let mut result = None;
                let stop = stop.map(|r| r["stopReason"].as_str().unwrap_or("end_turn").to_string());
                for ev in tr.end(stop.as_deref().map_err(|e| e.clone()), session) {
                    if let Some(r) = crate::claude::handle_event(state, chat_id, &ev).await { result = Some(r); }
                }
                result
            }
            View::Codex(v) => v.end(state, chat_id, stop).await,
        }
    }
}

/// `drive` for either program: `prompt` is the turn's content blocks (text, and pictures for Codex).
/// The view comes back with what it gathered (Codex's tokens, what its sandbox refused).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn drive_with(state: &Arc<AppState>, chat_id: &str, mut child: Child, conn: Arc<Conn>, mut rx: UnboundedReceiver<Incoming>,
    session: String, prompt: Vec<Value>, asker: Asker, kill: &mut tokio::sync::oneshot::Receiver<()>, mut view: View) -> (Followed, View) {
    let err_text = Arc::new(std::sync::Mutex::new(String::new()));
    let err_task = child.stderr.take().map(|mut e| {
        let t = err_text.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            while let Ok(n) = e.read(&mut buf).await {
                if n == 0 { break; }
                let mut s = t.lock().unwrap_or_else(|e| e.into_inner());
                if s.len() < 4000 { s.push_str(&String::from_utf8_lossy(&buf[..n])); }
            }
        })
    });
    view.start(state, chat_id, &session).await;
    let c2 = conn.clone();
    let sid = session.clone();
    let mut turn = tokio::spawn(async move {
        c2.request("session/prompt", json!({"sessionId": sid, "prompt": prompt})).await
    });
    let ended = loop {
        tokio::select! {
            _ = &mut *kill => {
                let _ = conn.notify("session/cancel", json!({"sessionId": session})).await;
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut turn).await;
                break Ended::Stopped;
            }
            done = &mut turn => {
                // What was sent before the answer is shown first.
                while let Ok(m) = rx.try_recv() { if let Incoming::Notification { method, params } = m { if method == "session/update" { view.update(state, chat_id, &params["update"]).await; } } }
                let stop = match done {
                    Ok(Ok(r)) if r["stopReason"] == "cancelled" => break Ended::Stopped,
                    Ok(Ok(r)) => Ok(r),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                let result = view.end(state, chat_id, stop.as_ref().map_err(|e| e.clone()), &session).await;
                break result.map_or(Ended::NoResult, Ended::Result);
            }
            m = rx.recv() => match m {
                None => break Ended::Lost("the adapter's output ended".into()),
                Some(Incoming::Notification { method, params }) if method == "session/update" => {
                    view.update(state, chat_id, &params["update"]).await;
                    // The session never goes on in a mode that does more than the one checked.
                    if let Some(now) = view.mode().filter(|m| !asker.full && !acp::mode_within(&asker.mode, m)) {
                        crate::log::error(format!("run {chat_id}: the agent switched to mode \"{now}\" (checked: {}); stopped", asker.mode));
                        let _ = conn.notify("session/cancel", json!({"sessionId": session})).await;
                        break Ended::Lost("the agent switched to a permission mode the chat does not use".into());
                    }
                }
                Some(Incoming::Notification { .. }) => {}
                // Codex asks only to leave its sandbox, which `codex exec` never does: always refused,
                // and listed under the answer like what its sandbox refused (`codex_acp::refusal`).
                Some(Incoming::Request { id, method, params }) if method == "session/request_permission" && matches!(view, View::Codex(_)) => {
                    if let View::Codex(v) = &mut view { v.refused(&params["toolCall"]); }
                    let _ = conn.respond(&id, acp::permission_answer(crate::codex_acp::refusal(&params["options"]))).await;
                }
                Some(Incoming::Request { id, method, params }) if method == "session/request_permission" => {
                    tokio::spawn(answer_permission(state.clone(), conn.clone(), id, params, asker.clone()));
                }
                Some(Incoming::Request { id, .. }) => { let _ = conn.respond_error(&id, -32601, "not offered by this client").await; }
            },
        }
    };
    turn.abort();
    end(&mut child).await;
    let exit = child.wait().await;
    if let Some(t) = err_task { let _ = tokio::time::timeout(std::time::Duration::from_secs(3), t).await; }
    let stderr = err_text.lock().unwrap_or_else(|e| e.into_inner()).clone();
    (Followed { ended, exit, stderr }, view)
}

/// What `GET /api/ai/<program>/options` adds from the agent's own report: its models first (with
/// the names it gives them, `modelNames`), its effort levels (`effortNames`) and — for Claude Code,
/// whose chats take its modes directly — the permission modes it offers that the daemon accepts, in
/// its order and under its names (`modeNames`). Nothing is added until the report is in.
pub async fn merge_options(state: &Arc<AppState>, program: Program, out: &mut Value, refresh: bool) {
    if !out.is_object() || out.as_object().map_or(true, |o| o.is_empty()) { return; }
    let Some(c) = catalog(state, program, refresh).await else { return };
    apply_catalog(program, &c, out);
}

pub fn apply_catalog(program: Program, c: &Catalog, out: &mut Value) {
    let names = |l: &[acp::Choice]| l.iter().map(|x| (x.id.clone(), json!(x.name))).collect::<serde_json::Map<String, Value>>();
    let ok = |id: &str| crate::settings::check_cli_value("model", Some(id)).is_ok();
    if !c.models.is_empty() {
        let mut models: Vec<String> = c.models.iter().map(|m| m.id.clone()).filter(|m| ok(m)).collect();
        for m in out["models"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !models.iter().any(|x| x == m) { models.push(m.to_string()); }
        }
        out["models"] = json!(models);
        out["modelNames"] = json!(names(&c.models));
    }
    if !c.efforts.is_empty() {
        out["efforts"] = json!(c.efforts.iter().map(|e| e.id.clone()).filter(|e| ok(e)).collect::<Vec<_>>());
        out["effortNames"] = json!(names(&c.efforts));
    }
    if program == Program::Claude && !c.modes.is_empty() {
        let modes: Vec<&str> = c.modes.iter().map(|m| m.id.as_str()).filter(|m| crate::settings::PERMISSION_MODES.contains(m)).collect();
        if !modes.is_empty() {
            out["permissionModes"] = json!(modes);
            out["modeNames"] = json!(names(&c.modes));
        }
    }
    out["acp"] = json!(true);
}
