//! Ordinary Codex chats (kind `chat`) over Codex's ACP adapter (`acp_install::CODEX`, codex-acp),
//! the way Claude Code's chats run over theirs (`acp_agent.rs`). Everything about the run is decided
//! first, as for `codex exec` (`codex::launch`: trust, strict private folders, plain passwords, the
//! sandbox, the kept files); only then is it handed to the adapter here, and only when every one of
//! those guards travels (see `docs/acp.md` for the table):
//! - the sandbox is the adapter's mode of the same name (`read-only`, `workspace-write`, and
//!   `agent-full-access` for Bypass), set over ACP and checked: a mode it does not offer sends the
//!   chat back to the command line, and a switch mid-turn to one that does more ends the turn
//!   (`acp::mode_within`). The writable roots are the session's additional folders, which the
//!   adapter adds to the sandbox of every turn;
//! - every `-c` setting `codex exec` gets (`codex::exec_args`: no MCP servers or notifier where the
//!   profile says so, no history, no analytics, no network for commands, web search, model,
//!   effort, another provider) goes as the adapter's session config (`CODEX_CONFIG`); the provider's
//!   key only ever in the environment (`providers::apply_codex_env`);
//! - Codex asks for permission only to leave its sandbox (a command outside it, a file outside the
//!   writable folders, the network): `codex exec` never does, so every such request is refused and
//!   listed under the answer like what the sandbox refused (`refusal`);
//! - what has no checked equivalent stays on `codex exec` (`Stay`): Bypass on macOS, whose run
//!   `codex exec` wraps in the app's own sandbox (`codex::outer_sandbox`); a folder with Codex
//!   settings of its own (the adapter marks the folders it works in trusted, so Codex would load
//!   them, which `codex exec` does not); `ai.classicChats`; and anything but an ordinary chat.
//! An adapter that is not installed (it is then installed in the background), no Node.js, a Codex
//! older than the adapter is made for (`codex_fits`), or an adapter that fails before the turn is
//! sent: the chat goes on through `codex exec`, as before, and the log says why.

use crate::acp::{self, Conn};
use crate::assistant::Program;
use crate::codex::{After, Ev, Track};
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

/// Why a Codex chat's turn runs through `codex exec` this time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stay {
    /// `ai.classicChats`: the user keeps the command line.
    Classic,
    /// Only ordinary chats run over ACP.
    NotChat,
    /// Bypass on macOS: `codex exec` runs inside the app's own sandbox, which has no checked
    /// equivalent around the adapter and its `codex app-server`.
    OuterSandbox,
    /// A folder the run works in (or one above it) has Codex settings of its own.
    OwnSettings,
    NotInstalled,
    NoNode,
    /// The installed Codex is older than the one the adapter is made for (or its version is unknown).
    TooOld,
    /// The installed Codex has no `app-server`, which the adapter drives.
    NoAppServer,
    /// The adapter could not start, or open the session, before the turn.
    Failed,
}

impl Stay {
    pub fn name(self) -> &'static str {
        match self {
            Stay::Classic => "classic", Stay::NotChat => "notChat", Stay::OuterSandbox => "outerSandbox", Stay::OwnSettings => "ownSettings",
            Stay::NotInstalled => "notInstalled", Stay::NoNode => "noNode", Stay::TooOld => "codexTooOld", Stay::NoAppServer => "noAppServer", Stay::Failed => "failed",
        }
    }

    /// For the log.
    pub fn why(self) -> &'static str {
        match self {
            Stay::Classic => "the command line is chosen for chats (ai.classicChats)",
            Stay::NotChat => "only ordinary chats run over ACP",
            Stay::OuterSandbox => "a Bypass chat on macOS runs inside the app's own sandbox, which only the command line has",
            Stay::OwnSettings => "a folder of the chat has Codex settings of its own, which the adapter would load",
            Stay::NotInstalled => "the ACP adapter is not installed yet",
            Stay::NoNode => "Node.js was not found",
            Stay::TooOld => "the installed Codex is older than the ACP adapter needs",
            Stay::NoAppServer => "the installed Codex has no app-server",
            Stay::Failed => "the ACP adapter could not open the chat",
        }
    }
}

/// A version as `x.y.z`.
pub type Version = (u64, u64, u64);

/// The first `x.y.z` in `s` (`codex-cli 0.27.0`, `^0.171.0`).
pub fn parse_version(s: &str) -> Option<Version> {
    let re = regex::Regex::new(r"(\d+)\.(\d+)\.(\d+)").ok()?;
    let c = re.captures(s)?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?))
}

/// The lowest Codex the installed adapter is made for: the lower end of its own dependency on
/// `@openai/codex` (`^0.171.0` → 0.171.0), read from its `package.json` beside `dist/`. Never a
/// number written into the app: it moves with the pin.
pub fn required_codex(entry: &Path) -> Option<Version> {
    let pkg = entry.parent()?.parent()?.join("package.json");
    let meta: Value = serde_json::from_str(&std::fs::read_to_string(pkg).ok()?).ok()?;
    parse_version(meta["dependencies"]["@openai/codex"].as_str()?)
}

/// Can the installed Codex be driven by the adapter at `entry`: Node.js is there, Codex is at least
/// the version the adapter is made for, and it has the `app-server` the adapter starts. (On an older
/// Codex, `codex app-server` would be taken for a prompt.) Asked of the program's own version first,
/// and of its help only after that.
pub async fn codex_fits(state: &AppState, entry: &Path) -> Result<(), Stay> {
    if crate::claude::find_binary(&None, "node").is_none() { return Err(Stay::NoNode); }
    let need = required_codex(entry).ok_or(Stay::TooOld)?;
    let have = parse_version(&crate::codex::version_kept(state).await).ok_or(Stay::TooOld)?;
    if have < need { return Err(Stay::TooOld); }
    if !crate::codex::subcommands(&crate::codex::top_help(state).await).iter().any(|c| c == "app-server") { return Err(Stay::NoAppServer); }
    Ok(())
}

/// Whether ordinary Codex chats can run over ACP on this computer, for Settings:
/// `{on, why, need, have}` (`why`: `Stay::name`; `need`/`have`: the Codex versions, when too old).
pub async fn status(state: &AppState) -> Value {
    if state.settings.read().await.ai.classic_chats { return json!({"on": false, "why": Stay::Classic.name()}); }
    let Some(entry) = crate::acp_install::ready(Program::Codex) else { return json!({"on": false, "why": Stay::NotInstalled.name()}) };
    match codex_fits(state, &entry).await {
        Ok(()) => json!({"on": true}),
        Err(Stay::TooOld) => {
            let v = |x: Option<Version>| x.map(|(a, b, c)| format!("{a}.{b}.{c}"));
            json!({"on": false, "why": Stay::TooOld.name(), "need": v(required_codex(&entry)), "have": v(parse_version(&crate::codex::version_kept(state).await))})
        }
        Err(s) => json!({"on": false, "why": s.name()}),
    }
}

/// The adapter's mode for the sandbox `codex exec` would get (`unrestricted`: full access's
/// "no sandbox and no approvals").
pub fn mode_for(sandbox: &str, unrestricted: bool) -> &'static str {
    if unrestricted { return "agent-full-access"; }
    match sandbox { "read-only" => "read-only", "danger-full-access" => "agent-full-access", _ => "workspace-write" }
}

/// Does `dir` or a folder above it have Codex settings of its own (`.codex/config.toml`)? Codex's own
/// folder (`codex_home`, the user's settings, which every run loads) does not count.
pub fn own_settings(dirs: &[&Path], codex_home: &Path) -> bool {
    let home = crate::paths::canonical(codex_home).unwrap_or_else(|_| codex_home.to_path_buf());
    dirs.iter().any(|d| d.ancestors().any(|a| {
        let c = a.join(".codex");
        if c == codex_home || crate::paths::canonical(&c).is_ok_and(|x| x == home) { return false; }
        std::fs::symlink_metadata(c.join("config.toml")).is_ok()
    }))
}

/// Which guard keeps this turn on the command line, if any (the checks that need no program).
pub fn stays(kind: &str, classic: bool, outer: bool, full: bool, own: bool) -> Option<Stay> {
    if kind != "chat" { return Some(Stay::NotChat); }
    if classic { return Some(Stay::Classic); }
    if outer { return Some(Stay::OuterSandbox); }
    if own && !full { return Some(Stay::OwnSettings); }
    None
}

/// The `-c` settings of the same run's `codex exec` command line (`args`, from `codex::exec_args`)
/// as the session config the adapter hands Codex (`CODEX_CONFIG`): dotted keys, as Codex reads
/// overrides, each value as `-c` reads it (JSON, else text). Left out: how a session is resumed
/// (ACP's `session/resume` does that) and the writable folders (the adapter puts the session's
/// additional folders into every turn's sandbox). Another provider goes as a table (`service`),
/// which the adapter also needs to start Codex with it; its key is never in here.
pub fn session_config(args: &[OsString], service: Option<&crate::providers::CodexService>) -> Value {
    let mut out = serde_json::Map::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a != "-c" { continue; }
        let Some(kv) = it.next().map(|v| v.to_string_lossy().into_owned()) else { break };
        let Some((k, v)) = kv.split_once('=') else { continue };
        if k == "experimental_resume" || k == "sandbox_workspace_write.writable_roots" || k == "model_provider" || k.starts_with("model_providers.") { continue; }
        out.insert(k.to_string(), serde_json::from_str(v).unwrap_or_else(|_| json!(v)));
    }
    if let Some(svc) = service {
        let table = crate::providers::codex_config_json(svc, &[]);
        for k in ["model_provider", "model_providers"] { out.insert(k.into(), table[k].clone()); }
    }
    Value::Object(out)
}

/// The request that opens the turn's session: a new one, or the chat's own (`session/resume`).
/// The folders besides its own that the run may change are the session's additional folders.
pub fn open_request(cwd: &Path, writable: &[PathBuf], resume: Option<&str>) -> (&'static str, Value) {
    let mut p = json!({"cwd": cwd, "mcpServers": [], "additionalDirectories": writable});
    match resume {
        Some(id) => { p["sessionId"] = json!(id); ("session/resume", p) }
        None => ("session/new", p),
    }
}

/// The answer to a permission request from Codex: a no, from an allowlist of its plain refusals
/// ("continue without it" first). `None` answers "cancelled", which Codex reads as a no as well.
pub fn refusal(options: &Value) -> Option<String> {
    const NO: [&str; 3] = ["decline", "reject_permissions", "cancel"];
    let list: Vec<&Value> = options.as_array().into_iter().flatten().collect();
    NO.iter().find_map(|id| list.iter().find(|o| o["optionId"] == *id && o["kind"].as_str().is_some_and(|k| k.starts_with("reject"))))
        .and_then(|o| o["optionId"].as_str().map(String::from))
}

/// The turn's content: the prompt (with the app's notes in front, as `codex exec` gets it on stdin)
/// and the attached pictures, where the adapter takes pictures.
pub fn prompt_blocks(prompt: &str, pictures: &[PathBuf]) -> Vec<Value> {
    use base64::Engine;
    let mut out = vec![json!({"type": "text", "text": prompt})];
    for p in pictures {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let mime = match ext.as_str() { "jpg" | "jpeg" => "image/jpeg", "gif" => "image/gif", "webp" => "image/webp", _ => "image/png" };
        if let Ok(bytes) = std::fs::read(p) {
            out.push(json!({"type": "image", "mimeType": mime, "data": base64::engine::general_purpose::STANDARD.encode(bytes)}));
        }
    }
    out
}

// ---------- updates as the events the chat already shows ----------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind { Command, Change, Search, Tool }

/// One thing to show: a piece of streamed text or thinking (as Claude Code's `stream_event`, which
/// `claude::handle_event` shows), or one of the events Codex's command-line runs report (`codex::handle`).
#[derive(Debug, Clone, PartialEq)]
pub enum Step { Stream(Value), Ev(Ev) }

/// Turns Codex's `session/update`s into what a Codex chat shows when it runs through `codex exec`
/// (`codex::Ev`, handled by `codex::handle` into the same messages), with the text streaming in as
/// it is written. Keeps what the run reports (`track`), its usage, and the mode the agent says it is in.
#[derive(Default)]
pub struct View {
    text: String,
    calls: HashMap<String, (Kind, bool)>,
    pub mode: Option<String>,
    pub(crate) track: Track,
    /// The `usage` of the turn's answer.
    pub usage: Value,
    model: Option<String>,
}

fn s(v: &Value) -> String { v.as_str().unwrap_or("").to_string() }

fn kind_of(u: &Value) -> Kind {
    if u["rawInput"]["type"] == "webSearch" { return Kind::Search; }
    match u["kind"].as_str().unwrap_or("") {
        "execute" | "read" | "search" => Kind::Command,
        "edit" | "delete" | "move" => Kind::Change,
        "fetch" => Kind::Search,
        _ => Kind::Tool,
    }
}

/// The text of a tool call's result: its content, else its raw output when that is text.
fn output_of(u: &Value) -> String {
    let mut t: Vec<String> = u["content"].as_array().into_iter().flatten().filter_map(|c| c["content"]["text"].as_str().map(String::from)).collect();
    if t.is_empty() { if let Some(o) = u["rawOutput"].as_str() { t.push(o.to_string()); } }
    t.join("\n")
}

fn files_of(u: &Value) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let paths = u["locations"].as_array().into_iter().flatten().map(|l| s(&l["path"]))
        .chain(u["content"].as_array().into_iter().flatten().filter(|c| c["type"] == "diff").map(|c| s(&c["path"])));
    for p in paths { if !p.is_empty() && !out.contains(&p) { out.push(p); } }
    out
}

fn command_of(u: &Value) -> String {
    let c = crate::codex::command_text(&u["rawInput"]["command"]);
    if c.is_empty() { s(&u["title"]) } else { c }
}

impl View {
    pub fn new(model: Option<String>) -> View { View { model, ..Default::default() } }

    fn flush(&mut self, out: &mut Vec<Step>) {
        let t = std::mem::take(&mut self.text);
        if !t.trim().is_empty() { out.push(Step::Ev(Ev::Message(t))); }
    }

    fn begin(&mut self, u: &Value, out: &mut Vec<Step>) {
        let id = s(&u["toolCallId"]);
        if self.calls.contains_key(&id) { return; }
        self.flush(out);
        let k = kind_of(u);
        self.calls.insert(id.clone(), (k, false));
        out.push(Step::Ev(match k {
            Kind::Command => Ev::Command { id, command: command_of(u) },
            Kind::Change => Ev::Change { id, files: files_of(u) },
            Kind::Search => Ev::Search { id, query: u["rawInput"]["query"].as_str().map(String::from).unwrap_or_else(|| s(&u["title"])) },
            Kind::Tool => Ev::Tool { id, name: s(&u["title"]) },
        }));
    }

    fn finish(&mut self, u: &Value, out: &mut Vec<Step>) {
        let id = s(&u["toolCallId"]);
        let status = u["status"].as_str().unwrap_or("");
        if status != "completed" && status != "failed" { return; }
        let Some((k, done)) = self.calls.get_mut(&id) else { return };
        if *done { return; }
        *done = true;
        let ok = status == "completed";
        let output = output_of(u);
        match *k {
            Kind::Command => {
                // A command that failed without saying how may have been refused by the sandbox.
                let exit = u["rawOutput"]["exit_code"].as_i64().or(ok.then_some(0));
                out.push(Step::Ev(Ev::CommandDone { id, command: None, output, exit }));
            }
            Kind::Change => out.push(Step::Ev(Ev::ChangeDone { id, ok, output })),
            Kind::Search => {}
            Kind::Tool => out.push(Step::Ev(Ev::ToolDone { id, ok, output })),
        }
    }

    /// What one `session/update` comes to.
    pub fn steps(&mut self, u: &Value) -> Vec<Step> {
        let mut out = vec![];
        match u["sessionUpdate"].as_str().unwrap_or("") {
            "agent_message_chunk" => if let Some(t) = u["content"]["text"].as_str() {
                self.text.push_str(t);
                out.push(Step::Stream(json!({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": t}}})));
            },
            "agent_thought_chunk" => if let Some(t) = u["content"]["text"].as_str() {
                out.push(Step::Stream(json!({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "thinking_delta", "thinking": t}}})));
            },
            "tool_call" => { self.begin(u, &mut out); self.finish(u, &mut out); }
            // A call first reported when it is over is shown with its result.
            "tool_call_update" => { if u["title"].is_string() || u["kind"].is_string() { self.begin(u, &mut out); } self.finish(u, &mut out); }
            "plan" => {
                let steps = u["entries"].as_array().into_iter().flatten().map(|e| (s(&e["content"]), e["status"] == "completed")).collect();
                out.push(Step::Ev(Ev::Plan(crate::codex::plan_text(steps))));
            }
            "current_mode_update" => self.mode = u["currentModeId"].as_str().map(String::from),
            "config_option_update" => {
                let opts = if u["configOptions"].is_array() { u["configOptions"].clone() } else { json!([u["configOption"].clone()]) };
                for o in opts.as_array().into_iter().flatten() {
                    if o["category"] == "mode" || o["id"] == "mode" { self.mode = Some(s(&o["currentValue"])); }
                }
            }
            "usage_update" => if let Some(n) = u["size"].as_u64().filter(|n| *n > 0) { out.push(Step::Ev(Ev::Context(n))); },
            _ => {}
        }
        out
    }

    /// The end of the turn (`Ok`: the answer to `session/prompt`).
    pub fn end_steps(&mut self, stop: Result<&Value, String>) -> Vec<Step> {
        let mut out = vec![];
        self.flush(&mut out);
        match stop {
            Ok(r) if r["stopReason"] == "refusal" => { out.push(Step::Ev(Ev::Error("The model declined to answer.".into()))); out.push(Step::Ev(Ev::Done(false))); }
            Ok(r) => {
                self.usage = r["usage"].clone();
                if let Some(n) = self.usage["totalTokens"].as_u64().filter(|n| *n > 0) { out.push(Step::Ev(Ev::Usage(n))); }
                out.push(Step::Ev(Ev::Done(true)));
            }
            Err(e) => { out.push(Step::Ev(Ev::Error(e))); out.push(Step::Ev(Ev::Done(false))); }
        }
        out
    }

    async fn show(&mut self, state: &AppState, chat_id: &str, steps: Vec<Step>) -> Option<bool> {
        let mut result = None;
        for st in steps {
            match st {
                Step::Stream(ev) => { crate::claude::handle_event(state, chat_id, &ev).await; }
                Step::Ev(ev) => if let Some(ok) = crate::codex::handle(state, chat_id, ev, &mut self.track).await { result = Some(ok); },
            }
        }
        result
    }

    pub(crate) async fn start(&mut self, state: &AppState, chat_id: &str, session: &str) {
        let first = vec![Step::Ev(Ev::Session(session.to_string())), Step::Ev(Ev::Config { model: self.model.clone(), sandbox: None })];
        self.show(state, chat_id, first).await;
    }

    pub(crate) async fn update(&mut self, state: &AppState, chat_id: &str, u: &Value) {
        let steps = self.steps(u);
        self.show(state, chat_id, steps).await;
    }

    pub(crate) async fn end(&mut self, state: &AppState, chat_id: &str, stop: Result<&Value, String>) -> Option<bool> {
        let steps = self.end_steps(stop);
        self.show(state, chat_id, steps).await
    }

    /// A permission request that was refused, listed under the answer like what the sandbox refused.
    pub fn refused(&mut self, call: &Value) {
        let (tool, what) = if kind_of(call) == Kind::Change { ("Change", "change") } else { ("Command", "command") };
        let target = match what { "change" => files_of(call).join(", "), _ => command_of(call) };
        let item = json!({"tool": tool, "what": what, "target": crate::codex::short(&target, 160), "why": "codexSandbox"});
        if !self.track.refused.contains(&item) { self.track.refused.push(item); }
    }
}

/// What the turn used, as Codex's own runs report it (`meter::CodexRun`): the answer's `usage`
/// (input without the cached part) as one turn.
pub fn meter_of(usage: &Value) -> crate::meter::CodexRun {
    let mut run = crate::meter::CodexRun::default();
    let n = |k: &str| usage[k].as_u64().unwrap_or(0);
    if n("totalTokens") > 0 {
        let line = json!({"type": "turn.completed", "usage": {"input_tokens": n("inputTokens") + n("cachedReadTokens") + n("cachedWriteTokens"),
            "cached_input_tokens": n("cachedReadTokens"), "cache_write_input_tokens": n("cachedWriteTokens"), "output_tokens": n("outputTokens")}});
        run.feed(&line.to_string());
    }
    run
}

// ---------- a turn ----------

/// What `codex::launch` decided for the run, for the ACP path.
pub struct Try<'a> {
    pub spec: &'a crate::claude::RunSpec,
    pub kind: &'a str,
    pub profile: &'a crate::claude::Profile,
    pub sandbox: &'a str,
    /// Full access in Bypass: no sandbox and no approvals.
    pub unrestricted: bool,
    /// `codex exec` would run inside the app's own sandbox.
    pub outer: bool,
    pub full: bool,
    pub writable: &'a [PathBuf],
    pub effort: Option<&'a str>,
    /// The command line `codex exec` would get, for its settings.
    pub args: &'a [OsString],
    pub service: Option<&'a crate::providers::CodexService>,
    /// The copies of the attached pictures.
    pub images: &'a [PathBuf],
    /// The app's notes and the prompt, as `codex exec` would read them.
    pub prompt: &'a str,
}

/// Run the turn over the adapter when every guard travels there. `Ok(None)`: it was started (its
/// end runs `codex::after_run`); `Ok(Some(after))`: it stays on `codex exec`, nothing was sent.
pub(crate) async fn start(state: &Arc<AppState>, t: Try<'_>, after: After) -> anyhow::Result<Option<After>> {
    let id = &t.spec.chat_id;
    let classic = state.settings.read().await.ai.classic_chats;
    // (Looking up the folders is file work: off the runtime.)
    let own = t.kind == "chat" && !classic && !t.full && {
        let dirs: Vec<PathBuf> = std::iter::once(t.spec.cwd.clone()).chain(t.writable.iter().cloned()).collect();
        let home = crate::codex::codex_home();
        tokio::task::spawn_blocking(move || own_settings(&dirs.iter().map(PathBuf::as_path).collect::<Vec<_>>(), &home)).await.unwrap_or(true)
    };
    if let Some(stay) = stays(t.kind, classic, t.outer, t.full, own) {
        if stay != Stay::NotChat && stay != Stay::Classic { crate::log::info(format!("run {id}: Codex chat on the command line: {}", stay.why())); }
        return Ok(Some(after));
    }
    let Some(entry) = crate::acp_install::ready(Program::Codex) else {
        crate::acp_install::ensure_in_background(state.clone(), Program::Codex);
        return Ok(Some(after));
    };
    if let Err(stay) = codex_fits(state, &entry).await {
        crate::log::info(format!("run {id}: Codex chat on the command line: {}", stay.why()));
        return Ok(Some(after));
    }
    let mode = mode_for(t.sandbox, t.unrestricted);
    let mut c = match crate::acp_agent::adapter_command(state, Program::Codex, &entry).await {
        Ok(c) => c,
        Err(e) => { crate::log::warn(format!("run {id}: Codex chat on the command line: {e}")); return Ok(Some(after)); }
    };
    for (k, v) in adapter_env(&t, mode) { c.env(k, v); }
    c.current_dir(&t.spec.cwd);
    go(state, t, after, c).await
}

/// What the adapter is started with on top of `acp_agent::adapter_command` (which points it at the
/// user's Codex and puts another provider's key in the environment, and only there): the run's
/// settings as its session config, and its mode from the very start (never the adapter's default,
/// `agent`, where a model approves what Codex asks).
pub fn adapter_env(t: &Try<'_>, mode: &str) -> Vec<(&'static str, String)> {
    vec![("CODEX_CONFIG", session_config(t.args, t.service).to_string()), ("INITIAL_AGENT_MODE", mode.to_string())]
}

/// `start` with the adapter command ready (tests hand in a stand-in).
pub(crate) async fn go(state: &Arc<AppState>, t: Try<'_>, after: After, c: tokio::process::Command) -> anyhow::Result<Option<After>> {
    let id = t.spec.chat_id.clone();
    let mode = mode_for(t.sandbox, t.unrestricted);
    let back = |e: String| { crate::log::warn(format!("run {id}: Codex chat on the command line: {} ({e})", Stay::Failed.why())); };
    let (mut child, conn, rx, told) = match crate::acp_agent::connect_told(c).await {
        Ok(x) => x,
        Err(e) => { back(e.to_string()); return Ok(Some(after)); }
    };
    // Pictures go only where the adapter takes them; a chat with pictures it cannot take runs as before.
    let takes_pictures = told["agentCapabilities"]["promptCapabilities"]["image"].as_bool().unwrap_or(false);
    if !t.images.is_empty() && !takes_pictures {
        crate::acp_agent::end(&mut child).await;
        back("it takes no pictures".into());
        return Ok(Some(after));
    }
    let opened = open(&conn, &t, mode).await;
    let (session, model) = match opened {
        Ok(x) => x,
        Err(e) => { crate::acp_agent::end(&mut child).await; back(e.to_string()); return Ok(Some(after)); }
    };
    let blocks = prompt_blocks(t.prompt, t.images);
    // One run per chat: if another got in meanwhile, this one goes.
    let mut kill_rx = match crate::claude::take_switch(state, &id).await {
        Ok(rx) => rx,
        Err(e) => { crate::acp_agent::end(&mut child).await; return Err(e); }
    };
    if !crate::full_access::still_full(state, &id, t.full).await {
        crate::acp_agent::end(&mut child).await;
        return Err(crate::tr::err("full_access.off", &[]));
    }
    let pid = child.id();
    crate::runs::record(pid, &id);
    crate::log::info(format!("run {id} started on Codex over ACP (pid {}, mode {mode}, profile {}, in {})", pid.unwrap_or(0), t.profile.name, t.spec.cwd.display()));
    crate::codex::running(state, &id, &t.spec.cwd).await;
    let asker = crate::acp_agent::Asker { token: None, mode: mode.to_string(), full: t.full };
    let state = state.clone();
    tokio::spawn(async move {
        let guard = crate::claude::RunGuard::new(&state, &id);
        let started = (Instant::now(), SystemTime::now());
        let view = crate::acp_agent::View::Codex(View::new(model));
        let (f, view) = crate::acp_agent::drive_with(&state, &id, child, conn, rx, session, blocks, asker, &mut kill_rx, view).await;
        drop(kill_rx);
        let crate::acp_agent::View::Codex(v) = view else { unreachable!("the view stays Codex's") };
        let run = meter_of(&v.usage);
        crate::codex::after_run(state, id, f, v.track, run, after, pid, started, guard).await;
    });
    Ok(None)
}

/// Open the turn's session in the run's mode (which the adapter must offer and the session then be
/// in), model and effort. Gives the session and the model it runs with.
async fn open(conn: &Conn, t: &Try<'_>, mode: &str) -> anyhow::Result<(String, Option<String>)> {
    let (method, params) = open_request(&t.spec.cwd, t.writable, t.spec.resume.as_deref());
    let r = conn.request(method, params).await?;
    let session = r["sessionId"].as_str().map(String::from).or_else(|| t.spec.resume.clone())
        .filter(|s| crate::settings::check_cli_value("session", Some(s)).is_ok())
        .ok_or_else(|| anyhow::anyhow!("the agent opened no session"))?;
    let cat = acp::catalog(&r);
    let model = t.spec.options.model.as_deref();
    crate::acp_agent::settle(conn, &session, &cat, mode, model, t.effort).await?;
    let model = model.filter(|m| cat.models.iter().any(|c| c.id == *m)).map(String::from).or(cat.current_model.clone());
    Ok((session, model))
}

#[cfg(test)]
#[path = "codex_acp_tests.rs"]
mod tests;
