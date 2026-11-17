//! Approving what the assistant asks for, from inside the chat.
//!
//! `claude -p` has nobody to ask when a tool needs permission. Given `--permission-prompt-tool
//! mcp__thoughtsapp__approve` it asks that MCP tool instead, and the daemon is that tool: a small
//! MCP endpoint on the daemon's own port (`POST /mcp/approvals`, streamable HTTP). Each run that may
//! ask gets a token of its own in its private MCP config file; the endpoint answers nothing else.
//!
//! The request (`{tool_name, input, tool_use_id}`) becomes a card in the chat — what the assistant
//! wants to do, in plain words, and why the chat's mode does not let it do so on its own — with
//! Allow once, Always for this chat, and Deny. The tool call waits for the answer (keeping its HTTP
//! response open, with keep-alives) and is refused when nobody answers within `WAIT`, when the run
//! ends, or when the program goes away. The answer goes back as one text block holding
//! `{"behavior":"allow"}` or `{"behavior":"deny","message":…}` (Claude Code's documented contract).
//!
//! Who gets it: ordinary chats only (`wanted`), never in bypass mode and never while web lookups
//! are on, and only when the installed program has `--permission-prompts host`. Everything the app
//! starts by itself keeps its fixed profile, where anything that would ask is refused.
//!
//! "Always for this chat" is remembered here (`chat-approvals.json`), never written into Claude
//! Code's own settings: the next request with the same key is allowed without a card. The key is
//! exact — this command, this file, this connected tool — and deny rules still come first (a
//! private folder, the app's own data and the protective rules are refused before anyone is asked).

use crate::chats::Msg;
use crate::state::AppState;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The MCP server's name in a run's MCP config, and its one tool.
pub const SERVER: &str = "thoughtsapp";
pub const TOOL: &str = "approve";
/// Where the daemon answers it. Outside the API's own token: each run has a token of its own.
pub const MCP_PATH: &str = "/mcp/approvals";
/// How long a request waits for the user. Unanswered, it is refused.
pub const WAIT: Duration = Duration::from_secs(5 * 60);
/// The per-server `timeout` in the run's MCP config. Claude Code gives an HTTP server's requests 60
/// seconds unless this is set, and aborts a call that is silent for 5 minutes; this raises both
/// past `WAIT` for this one server.
const CLIENT_TIMEOUT_MS: u64 = (5 * 60 + 60) * 1000;
/// A comment line on the waiting response this often, so nothing on the way closes it as idle.
const KEEPALIVE: Duration = Duration::from_secs(15);
/// The most requests one run may have waiting at once.
const MAX_WAITING: usize = 4;
/// A tool's input can be large (a whole file to write): room for it.
const BODY_LIMIT: usize = 16 * 1024 * 1024;
/// Text kept per value of a request's details on the card.
const DETAIL_CHARS: usize = 4000;

/// The full name Claude Code knows the tool by.
pub fn prompt_tool() -> String { format!("mcp__{SERVER}__{TOOL}") }

static PORT: AtomicU16 = AtomicU16::new(0);
/// The daemon's port, once it is listening (`lib.rs`). Until then nothing can ask.
pub fn set_port(p: u16) { PORT.store(p, Ordering::Relaxed); }
pub fn port() -> u16 { PORT.load(Ordering::Relaxed) }

/// What the assistant is told (in English, like the rest of its instructions) when it may ask.
pub const NOTE: &str = "Permission requests: when a tool needs the user's permission in this chat, the app shows the request to the user \
    and waits for the answer (they may allow it once, allow it for the rest of this chat, or refuse). When a request is refused, accept \
    that: do not try another way to do the same thing; carry on without it and say in your reply what you could not do. The tool \
    `mcp__thoughtsapp__approve` belongs to the app: never call it yourself.";

/// What the assistant is told when strict private folders close commands in its chat.
pub fn strict_note(asks: bool) -> &'static str {
    if asks {
        "Strict private folders is on: every command needs the user's approval in this chat, because a command can reach a private folder. \
        Prefer the file tools (Read, Edit, Write, Glob, Grep), which do not need it, and ask for a command only when nothing else will do."
    } else {
        "Strict private folders is on: commands are switched off in this chat, because a command can reach a private folder. Use the file \
        tools (Read, Edit, Write, Glob, Grep) instead."
    }
}

// ---------- who may ask, and what a run is started with ----------

/// Does a run of this kind, in this mode and with this profile, put its questions to the user?
/// Ordinary chats only: nobody watches what the app starts by itself. Never in bypass mode (nothing
/// asks there), and never while web lookups are on (a page could make the assistant ask for
/// anything, and its profile closes connected tools, this one included).
pub fn wanted(kind: &str, mode: &str, p: &crate::claude::Profile) -> bool {
    kind == "chat" && mode != crate::settings::BYPASS && p.name == "chat" && !p.web
}

/// Does the installed program (its `--help`) answer permission prompts through a tool? Read from
/// `--permission-prompts host`, whose text names `--permission-prompt-tool` (Claude Code 2.1.259 and
/// later; the tool flag itself is not listed). Older programs keep refusing what would ask.
pub fn supported(help: &str) -> bool {
    use crate::claude::{flag_block, supports};
    supports(help, "--mcp-config")
        && (supports(help, "--permission-prompt-tool") || flag_block(help, "--permission-prompts").contains("--permission-prompt-tool"))
}

/// Do strict private folders apply to a run? When the user switched them on, the library has
/// private folders, and the run is a chat (ordinary or customise) that is not in bypass mode. Runs
/// the app starts by itself have no commands already, except set-up and Reorganize.
pub fn strict_applies(on: bool, private: &[String], kind: &str, mode: &str) -> bool {
    on && matches!(kind, "chat" | "customize") && mode != crate::settings::BYPASS
        && private.iter().any(|p| crate::paths::clean_rel(p).map_or(false, |c| !c.is_empty()))
}

/// The MCP config a run that asks is started with: this daemon's endpoint and the run's token.
pub fn config_json(port: u16, token: &str) -> String {
    json!({"mcpServers": {SERVER: {
        "type": "http",
        "url": format!("http://127.0.0.1:{port}{MCP_PATH}"),
        "headers": {"Authorization": format!("Bearer {token}")},
        "timeout": CLIENT_TIMEOUT_MS,
    }}}).to_string()
}

/// A run's flags (`claude::profile_flags`) for a run that asks: questions go to the prompt tool
/// (`--permission-prompts host`) instead of being refused (`none`), and the tool's server is loaded
/// from `config`. `--mcp-config` takes a list, so a flag always follows it.
pub fn flags(base: Vec<String>, help: &str, config: &Path) -> Vec<std::ffi::OsString> {
    let mut out: Vec<std::ffi::OsString> = vec![];
    let mut i = 0;
    while i < base.len() {
        if base[i] == "--permission-prompts" { i += 2; continue; }
        out.push(base[i].clone().into());
        i += 1;
    }
    if crate::claude::supports(help, "--permission-prompts") { out.push("--permission-prompts".into()); out.push("host".into()); }
    out.push("--mcp-config".into());
    out.push(config.into());
    out.push("--permission-prompt-tool".into());
    out.push(prompt_tool().into());
    out
}

// ---------- the state ----------

/// What a run that asks was started with, for wording its questions.
#[derive(Clone, Debug, Default)]
pub struct RunCtx {
    pub chat_id: String,
    pub mode: String,
    /// Strict private folders apply: every command asks, with the warning.
    pub strict: bool,
    pub cwd: PathBuf,
    pub add_dirs: Vec<PathBuf>,
    pub library: Option<PathBuf>,
    pub private: Vec<String>,
}

struct Run {
    ctx: RunCtx,
    /// Tool uses the program asked about in this run (each is asked about once).
    seen: HashSet<String>,
    /// …and those of them that became a card (their refusal is on the card already).
    carded: HashSet<String>,
}

struct Waiting {
    chat_id: String,
    token: String,
    card: Card,
    /// What "Always for this chat" matches: `(kind, key)`, taken from the request itself (the card's copy is cut short).
    key: Option<(&'static str, String)>,
    tx: tokio::sync::oneshot::Sender<Answer>,
}

/// One "Always for this chat".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Always {
    pub id: String,
    /// `command` (this exact command), `file` (changes to this file), `read` (reading this file or
    /// folder), `tool` (this tool, any input).
    pub kind: String,
    /// The tool it was given for (`Bash`, `Edit`, `mcp__mail__send`…).
    pub tool: String,
    /// What is matched: the command, the absolute path, or the tool's name.
    pub key: String,
    /// How the card named it, for the list in the chat.
    pub what: String,
    pub target: String,
    pub at: DateTime<Utc>,
}

/// The user's answer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Answer { Once, Always, Deny }

/// What the program is told.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict { Allow, Deny(String) }

impl Verdict {
    /// The one text block Claude Code reads.
    pub fn text(&self) -> String {
        match self {
            Verdict::Allow => json!({"behavior": "allow"}).to_string(),
            Verdict::Deny(m) => json!({"behavior": "deny", "message": m}).to_string(),
        }
    }
}

/// The chat's approvals: runs that may ask (by token), requests waiting for the user, and what the
/// user allowed for a whole chat.
pub struct Approvals {
    file: PathBuf,
    inner: std::sync::Mutex<Inner>,
    /// The newest change of `always` already in `file`. Held while writing, so the writes take
    /// turns and an older copy never lands after a newer one (`save`).
    saved: std::sync::Mutex<u64>,
}

#[derive(Default)]
struct Inner {
    runs: HashMap<String, Run>,
    waiting: HashMap<String, Waiting>,
    /// chat id -> what is always allowed there; read from `file` the first time it is needed.
    always: Option<BTreeMap<String, Vec<Always>>>,
    /// Counts the changes to `always`: each copy handed to `save` carries the count it was taken at.
    always_version: u64,
}

impl Approvals {
    /// Approvals that remember "always" in `file`.
    pub fn at(file: PathBuf) -> Self { Self { file, inner: Default::default(), saved: Default::default() } }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> { self.inner.lock().unwrap_or_else(|e| e.into_inner()) }

    fn always_map<'a>(&self, inner: &'a mut Inner) -> &'a mut BTreeMap<String, Vec<Always>> {
        inner.always.get_or_insert_with(|| std::fs::read_to_string(&self.file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default())
    }

    /// A copy of `always` after a change, numbered, for `save` (taken under the lock).
    fn changed(inner: &mut Inner) -> (u64, BTreeMap<String, Vec<Always>>) {
        inner.always_version += 1;
        (inner.always_version, inner.always.clone().unwrap_or_default())
    }

    /// Write copy number `version` of `always`, unless a newer one was written already: two
    /// changes saved at once (a take-back and a new "always") can reach here in either order, and
    /// the older must not put back a rule the user took away.
    fn save(&self, (version, map): (u64, BTreeMap<String, Vec<Always>>)) {
        let mut saved = self.saved.lock().unwrap_or_else(|e| e.into_inner());
        if version <= *saved { return; }
        let text = serde_json::to_string_pretty(&map).unwrap_or_else(|_| "{}".into());
        crate::log::ok_or_warn("could not remember what was allowed for a chat", crate::paths::write_atomic(&self.file, text.as_bytes(), true));
        *saved = version;
    }

    /// What is always allowed in `chat_id`.
    pub fn always(&self, chat_id: &str) -> Vec<Always> {
        let mut g = self.lock();
        self.always_map(&mut g).get(chat_id).cloned().unwrap_or_default()
    }

    /// Take back one "always" of a chat. Returns what remains.
    pub fn forget(&self, chat_id: &str, id: &str) -> Vec<Always> {
        let mut g = self.lock();
        let map = self.always_map(&mut g);
        let left = match map.get_mut(chat_id) {
            Some(list) => { list.retain(|a| a.id != id); list.clone() }
            None => return vec![],
        };
        if left.is_empty() { map.remove(chat_id); }
        let snapshot = Self::changed(&mut g);
        drop(g);
        self.save(snapshot);
        left
    }

    /// A chat was deleted: what was allowed in it goes with it.
    pub fn forget_chat(&self, chat_id: &str) {
        let mut g = self.lock();
        let map = self.always_map(&mut g);
        if map.remove(chat_id).is_none() { return; }
        let snapshot = Approvals::changed(&mut g);
        drop(g);
        self.save(snapshot);
    }

    /// Is a run of `chat_id` able to ask right now?
    pub fn asking(&self, chat_id: &str) -> bool { self.lock().runs.values().any(|r| r.ctx.chat_id == chat_id) }

    /// The tool uses of `chat_id`'s run that were put to the user (their refusals were on a card already).
    pub fn carded(&self, chat_id: &str) -> HashSet<String> {
        self.lock().runs.values().filter(|r| r.ctx.chat_id == chat_id).flat_map(|r| r.carded.iter().cloned()).collect()
    }

    fn knows(&self, token: &str) -> bool { !token.is_empty() && self.lock().runs.contains_key(token) }
}

/// A run that may ask. While it is held, its token opens the endpoint; dropped (the run is over,
/// however it ended), every request of the run still waiting is refused and its card says so.
pub struct Ticket { token: String, state: Arc<AppState> }

impl Ticket {
    pub fn token(&self) -> &str { &self.token }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let ended: Vec<(String, String)> = {
            let mut g = self.state.approvals.lock();
            g.runs.remove(&self.token);
            let ids: Vec<String> = g.waiting.iter().filter(|(_, w)| w.token == self.token).map(|(id, _)| id.clone()).collect();
            ids.into_iter().filter_map(|id| g.waiting.remove(&id).map(|w| (w.chat_id, id))).collect()
        };
        if ended.is_empty() { return; }
        let state = self.state.clone();
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.spawn(async move { for (chat, id) in ended { settle(&state, &chat, &id, "ended").await; } });
        }
    }
}

/// Open a run that may ask.
pub fn open(state: &Arc<AppState>, ctx: RunCtx) -> Ticket {
    use rand::Rng;
    let token: String = rand::thread_rng().sample_iter(&rand::distributions::Alphanumeric).take(40).map(char::from).collect();
    state.approvals.lock().runs.insert(token.clone(), Run { ctx, seen: HashSet::new(), carded: HashSet::new() });
    Ticket { token, state: state.clone() }
}

// ---------- one request ----------

/// What Claude Code asks about.
#[derive(Clone, Debug, Default)]
pub struct Request { pub tool: String, pub input: Value, pub tool_use_id: Option<String> }

impl Request {
    /// From the tool call's arguments (`{tool_name, input, tool_use_id}`).
    pub fn from_args(a: &Value) -> Self {
        let s = |keys: &[&str]| keys.iter().find_map(|k| a[*k].as_str()).map(String::from);
        let input = ["input", "tool_input"].iter().map(|k| &a[*k]).find(|v| !v.is_null()).cloned().unwrap_or(Value::Null);
        Request { tool: s(&["tool_name", "toolName"]).unwrap_or_default(), input, tool_use_id: s(&["tool_use_id", "toolUseId"]).filter(|t| !t.is_empty()) }
    }
}

/// The card a request becomes in the chat.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub tool: String,
    /// `command`, `change`, `open`, `look`, `connected`, `tool`.
    pub what: String,
    /// The command, the file (relative to the library or the chat's folder when inside it), or the tool.
    pub target: String,
    /// Why the chat's mode does not let it through on its own: `strict`, `command`, `planCommand`,
    /// `change`, `plan`, `outside`, `read`, `connected`, `other`.
    pub why: String,
    pub mode: String,
    pub strict: bool,
    /// The private folders, when the warning is about them.
    pub private: Vec<String>,
    /// What "Always for this chat" would cover (`command`, `file`, `read`, `tool`); `None`: only once.
    pub always: Option<String>,
    /// The request as the program sent it, long values cut, characters that do not show written out.
    pub detail: Value,
    pub tool_use_id: Option<String>,
    /// The request holds characters that do not show or that turn text around (`hidden_in`): what is
    /// shown could differ from what would run. `target` and `detail` have them written out as
    /// `[U+202E]`; the card warns, and "Always" is not offered.
    pub hidden: bool,
}

/// Is `c` a character a reader would not see, or one that changes the order text is drawn in? The
/// list the add-on checks use (`staging::validate::is_hidden_char`), plus control characters other
/// than a line break or a tab — and here with no exceptions: in a command or a path even a single
/// direction mark can make what is shown differ from what runs.
fn hidden_char(c: char) -> bool {
    crate::staging::validate::is_hidden_char(c) || (c.is_control() && c != '\n' && c != '\t')
}

/// Does any text in `v` (keys too) hold such a character?
pub fn hidden_in(v: &Value) -> bool {
    match v {
        Value::String(s) => s.chars().any(hidden_char),
        Value::Array(a) => a.iter().any(hidden_in),
        Value::Object(o) => o.iter().any(|(k, x)| k.chars().any(hidden_char) || hidden_in(x)),
        _ => false,
    }
}

/// `s` with every such character written out (`[U+202E]`), so that what is read is what is there.
/// Nothing else is changed or left out; a long run of them is shortened to the first few and a count.
pub fn reveal(s: &str) -> String {
    const SHOWN: usize = 8;
    let (mut out, mut run) = (String::with_capacity(s.len()), 0usize);
    for c in s.chars() {
        if hidden_char(c) {
            run += 1;
            if run <= SHOWN { out.push_str(&format!("[U+{:04X}]", c as u32)); }
            continue;
        }
        if run > SHOWN { out.push_str(&format!("[+{}]", run - SHOWN)); }
        run = 0;
        out.push(c);
    }
    if run > SHOWN { out.push_str(&format!("[+{}]", run - SHOWN)); }
    out
}

const SHELL: [&str; 2] = ["Bash", "PowerShell"];
const CHANGE: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];
const READ: [&str; 5] = ["Read", "NotebookRead", "Glob", "Grep", "LS"];

/// The path a file tool names, as given.
fn named_path(tool: &str, input: &Value) -> Option<String> {
    let keys: &[&str] = if CHANGE.contains(&tool) || tool == "Read" || tool == "NotebookRead" { &["file_path", "notebook_path"] } else if READ.contains(&tool) { &["path"] } else { return None };
    keys.iter().find_map(|k| input[*k].as_str()).filter(|s| !s.is_empty()).map(String::from)
}

fn absolute(raw: &str, ctx: &RunCtx) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() { p.to_path_buf() } else { ctx.cwd.join(p) }
}

/// The file a request is about, relative to the library when it is really inside it (links resolved).
fn library_rel(abs: &Path, lib: &Path) -> Option<String> {
    let real_lib = crate::paths::canonical(lib).unwrap_or_else(|_| lib.to_path_buf());
    let real = crate::paths::canonical_lenient(abs);
    let rel = real.strip_prefix(&real_lib).ok().or_else(|| abs.strip_prefix(lib).ok())?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Is the file a request names in a private folder (judged on its real path, any letter case)?
fn in_private(abs: &Path, ctx: &RunCtx) -> bool {
    let Some(lib) = &ctx.library else { return false };
    if ctx.private.is_empty() { return false; }
    let private: Vec<String> = ctx.private.iter().filter_map(|p| crate::paths::clean_rel(p).ok()).filter(|p| !p.is_empty()).collect();
    library_rel(abs, lib).map_or(false, |rel| crate::paths::is_private(lib, &rel, &private))
}

fn inside_dirs(abs: &Path, ctx: &RunCtx) -> bool {
    std::iter::once(&ctx.cwd).chain(ctx.add_dirs.iter()).any(|d| crate::paths::inside_ci(abs, d))
}

/// How the user would name a file: relative to the library or the chat's folder, else as given.
fn shown_path(raw: &str, ctx: &RunCtx) -> String {
    let abs = absolute(raw, ctx);
    for root in ctx.library.iter().chain(std::iter::once(&ctx.cwd)) {
        if let Ok(rel) = abs.strip_prefix(root) {
            let r = rel.to_string_lossy().replace('\\', "/");
            if !r.is_empty() { return r; }
        }
    }
    raw.to_string()
}

/// A request as the card shows it in its details: long strings cut down (a whole file being
/// written, say), characters that do not show written out.
fn trimmed(v: &Value) -> Value {
    match v {
        Value::String(s) if s.chars().count() > DETAIL_CHARS => Value::String(format!("{}…", reveal(&s.chars().take(DETAIL_CHARS).collect::<String>()))),
        Value::String(s) => Value::String(reveal(s)),
        Value::Array(a) => Value::Array(a.iter().take(50).map(trimmed).collect()),
        Value::Object(o) => Value::Object(o.iter().take(50).map(|(k, x)| (reveal(k), trimmed(x))).collect()),
        x => x.clone(),
    }
}

/// What "Always for this chat" would match for a request: `(kind, key)`. `None` where only once is offered.
fn always_key(req: &Request, ctx: &RunCtx) -> Option<(&'static str, String)> {
    let t = req.tool.as_str();
    if SHELL.contains(&t) {
        // Strict private folders: every command is looked at, every time.
        if ctx.strict { return None; }
        return req.input["command"].as_str().filter(|c| !c.trim().is_empty()).map(|c| ("command", c.to_string()));
    }
    if CHANGE.contains(&t) || READ.contains(&t) {
        let raw = named_path(t, &req.input)?;
        let abs = absolute(&raw, ctx).to_string_lossy().replace('\\', "/");
        return Some((if CHANGE.contains(&t) { "file" } else { "read" }, abs));
    }
    if t.is_empty() { None } else { Some(("tool", t.to_string())) }
}

/// The card for a request.
pub fn card(req: &Request, ctx: &RunCtx) -> Card {
    let t = req.tool.as_str();
    let plan = ctx.mode == "plan";
    let (what, target, why) = if SHELL.contains(&t) {
        let why = if ctx.strict { "strict" } else if plan { "planCommand" } else { "command" };
        ("command", req.input["command"].as_str().unwrap_or("").to_string(), why)
    } else if CHANGE.contains(&t) || READ.contains(&t) {
        let raw = named_path(t, &req.input).or_else(|| req.input["pattern"].as_str().map(String::from)).unwrap_or_default();
        let outside = !raw.is_empty() && !inside_dirs(&absolute(&raw, ctx), ctx);
        let change = CHANGE.contains(&t);
        let why = if change && plan { "plan" } else if outside { "outside" } else if change { "change" } else { "read" };
        let what = if change { "change" } else if t == "Read" || t == "NotebookRead" { "open" } else { "look" };
        (what, if raw.is_empty() { String::new() } else { shown_path(&raw, ctx) }, why)
    } else if let Some(rest) = t.strip_prefix("mcp__") {
        let mut parts = rest.splitn(2, "__");
        let (server, tool) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        ("connected", if tool.is_empty() { server.to_string() } else { format!("{tool} ({server})") }, "connected")
    } else {
        ("tool", t.to_string(), "other")
    };
    let strict = ctx.strict && what == "command";
    let hidden = hidden_in(&req.input) || t.chars().any(hidden_char);
    Card {
        tool: reveal(t), what: what.into(), target: reveal(&target), why: why.into(), mode: ctx.mode.clone(), strict,
        private: if strict { ctx.private.iter().filter_map(|p| crate::paths::clean_rel(p).ok()).filter(|p| !p.is_empty()).collect() } else { vec![] },
        // Only what can be seen in full can be allowed for good.
        always: if hidden { None } else { always_key(req, ctx).map(|(k, _)| k.to_string()) },
        detail: trimmed(&req.input), tool_use_id: req.tool_use_id.clone(), hidden,
    }
}

const OWN: &str = "This tool belongs to the Thoughts app and only asks the user on Claude Code's behalf. Do not call it.";
const QUESTION: &str = "The user cannot answer questions through this dialog in the Thoughts app. Ask in plain text in your reply instead.";
const PLAN: &str = "The Thoughts app does not approve plans this way. Present the plan in your reply; the user switches the chat out of Plan mode in the app when they want the changes made.";
const WEB: &str = "Web lookups are off in this chat. The user can switch them on for this chat in the app. Do not try to reach the web another way.";
const PRIVATE: &str = "That is inside a folder the user marked private. It is closed to the assistant; do not look for another way in.";
const AGAIN: &str = "This request was already answered.";
const TOO_MANY: &str = "Too many permission requests are waiting for the user at once. Wait for their answers before asking for more.";
const REFUSED: &str = "The user refused this in the app.";
const NO_ANSWER: &str = "Nobody answered the permission request in the app within 5 minutes, so it was refused. Carry on without it and say in your reply what still needs the user's permission.";
const ENDED: &str = "The run ended before the user answered.";

/// What can be decided without asking: requests about the app's own tool, questions and plans the
/// app does not show this way, the web (closed unless the chat's switch is on), anything in a
/// private folder (by its real path), a tool use that was asked about already, and what the user
/// allowed for the whole chat. `None`: the user is asked.
pub fn decide_now(req: &Request, ctx: &RunCtx, asked_before: bool, always: &[Always]) -> Option<Verdict> {
    let t = req.tool.as_str();
    if t.starts_with(&format!("mcp__{SERVER}__")) { return Some(Verdict::Deny(OWN.into())); }
    if t == "AskUserQuestion" { return Some(Verdict::Deny(QUESTION.into())); }
    if t == "ExitPlanMode" { return Some(Verdict::Deny(PLAN.into())); }
    if t == "WebFetch" || t == "WebSearch" { return Some(Verdict::Deny(WEB.into())); }
    if t.is_empty() || asked_before { return Some(Verdict::Deny(AGAIN.into())); }
    if let Some(raw) = named_path(t, &req.input) {
        if in_private(&absolute(&raw, ctx), ctx) { return Some(Verdict::Deny(PRIVATE.into())); }
    }
    let (kind, key) = always_key(req, ctx)?;
    // A command is matched by its exact tool (Bash is not PowerShell); a file by what is done to it.
    let hit = always.iter().any(|a| a.kind == kind && a.key == key && ((kind != "command" && kind != "tool") || a.tool == t));
    hit.then_some(Verdict::Allow)
}

/// Put a request to the user and wait for the answer. Everything that ends the wait refuses: Deny,
/// no answer within `wait`, the run ending, the program going away (this future is dropped).
pub async fn ask(state: &Arc<AppState>, token: &str, req: Request, wait: Duration) -> Verdict {
    let (ctx, asked_before) = {
        let mut g = state.approvals.lock();
        let Some(run) = g.runs.get_mut(token) else { return Verdict::Deny(ENDED.into()) };
        let before = req.tool_use_id.as_ref().map_or(false, |id| !run.seen.insert(id.clone()));
        (run.ctx.clone(), before)
    };
    // A tool use that already has its result ran (or was refused) long ago: not something to ask about.
    let done = match &req.tool_use_id {
        Some(id) => state.chats.with(&ctx.chat_id, |c| c.messages.iter().any(|m| m.role == "tool_result" && m.tool.as_ref().map_or(false, |t| t["toolUseId"] == id.as_str()))).await.unwrap_or(false),
        None => false,
    };
    let always = state.approvals.always(&ctx.chat_id);
    if let Some(v) = decide_now(&req, &ctx, asked_before || done, &always) {
        if let Verdict::Deny(m) = &v { crate::log::info(format!("chat {}: {} refused without asking ({})", ctx.chat_id, req.tool, m.split('.').next().unwrap_or(""))); }
        else { crate::log::info(format!("chat {}: {} allowed (always for this chat)", ctx.chat_id, req.tool)); }
        return v;
    }
    let c = card(&req, &ctx);
    let (tx, rx) = tokio::sync::oneshot::channel::<Answer>();
    let mut m = Msg::new("approval", format!("{} {}: {}", crate::tr::text("approval.asked", &[]), c.tool, c.target));
    let expires = Utc::now() + chrono::Duration::from_std(wait).unwrap_or_else(|_| chrono::Duration::minutes(5));
    m.tool = Some(json!({"code": "approval", "status": "waiting", "card": c, "expiresAt": expires.to_rfc3339()}));
    let id = m.id.clone();
    {
        let mut g = state.approvals.lock();
        if g.waiting.values().filter(|w| w.token == token).count() >= MAX_WAITING { return Verdict::Deny(TOO_MANY.into()); }
        let Some(run) = g.runs.get_mut(token) else { return Verdict::Deny(ENDED.into()) };
        if let Some(t) = &req.tool_use_id { run.carded.insert(t.clone()); }
        // Answerable as soon as the card can be seen: saving the card lets other work run before
        // `push` returns, and nobody can answer it before then (its id is only known from the card).
        let key = if c.hidden { None } else { always_key(&req, &ctx) };
        g.waiting.insert(id.clone(), Waiting { chat_id: ctx.chat_id.clone(), token: token.to_string(), card: c.clone(), key, tx });
    }
    // From here on, a program that gives up on the call (it was stopped, or went away) leaves a card
    // that says so: this future is dropped, and with it the guard. Answered, timed out or ended, the
    // request is no longer waiting by the time this returns, so then the guard finds nothing to do.
    let _gone = Gone { state: state.clone(), chat: ctx.chat_id.clone(), id: id.clone() };
    crate::claude::push(state, &ctx.chat_id, m).await;
    // The run ended while the card was being put in the chat: it may have settled the card before
    // the card was there, so settle it now that it is.
    if !state.approvals.lock().runs.contains_key(token) {
        state.approvals.lock().waiting.remove(&id);
        settle(state, &ctx.chat_id, &id, "ended").await;
        return Verdict::Deny(ENDED.into());
    }
    let title = state.chats.with(&ctx.chat_id, |ch| ch.title.clone()).await.unwrap_or_default();
    state.emit(json!({"type": "approval.asked", "chatId": ctx.chat_id, "id": id, "title": title}));
    crate::log::info(format!("chat {}: asking the user about {} ({}){}", ctx.chat_id, c.tool, c.why, if c.hidden { ", with characters that do not show" } else { "" }));
    let verdict = |a: Result<Answer, tokio::sync::oneshot::error::TryRecvError>| match a {
        Ok(Answer::Deny) => Verdict::Deny(REFUSED.into()),
        Ok(_) => Verdict::Allow,
        // The run ended: its card was settled when it did.
        Err(_) => Verdict::Deny(ENDED.into()),
    };
    let mut rx = rx;
    tokio::select! {
        a = &mut rx => verdict(a.map_err(|_| tokio::sync::oneshot::error::TryRecvError::Closed)),
        _ = tokio::time::sleep(wait) => {
            let still = state.approvals.lock().waiting.remove(&id).is_some();
            if !still {
                // Answered in the very last moment (or the run ended): that counts.
                return verdict(rx.try_recv());
            }
            crate::log::info(format!("chat {}: nobody answered about {} in time; refused", ctx.chat_id, c.tool));
            settle(state, &ctx.chat_id, &id, "timeout").await;
            Verdict::Deny(NO_ANSWER.into())
        }
    }
}

/// Settles a card whose request was dropped before it was answered.
struct Gone { state: Arc<AppState>, chat: String, id: String }

impl Drop for Gone {
    fn drop(&mut self) {
        if self.state.approvals.lock().waiting.remove(&self.id).is_none() { return; }
        let (state, chat, id) = (self.state.clone(), std::mem::take(&mut self.chat), std::mem::take(&mut self.id));
        if let Ok(rt) = tokio::runtime::Handle::try_current() { rt.spawn(async move { settle(&state, &chat, &id, "ended").await; }); }
    }
}

/// The card `id` in `chat_id` now says how it ended: `once`, `always`, `denied`, `timeout`, `ended`.
async fn settle(state: &AppState, chat_id: &str, id: &str, status: &str) {
    let mut shown = None;
    state.chats.update(chat_id, |c| {
        if let Some(m) = c.messages.iter_mut().find(|m| m.id == id) {
            if let Some(t) = m.tool.as_mut() { t["status"] = json!(status); t["answeredAt"] = json!(Utc::now().to_rfc3339()); }
            shown = serde_json::to_value(&*m).ok();
        }
    }).await;
    if let Some(m) = shown { state.emit(json!({"type": "chat.message", "chatId": chat_id, "message": m})); }
}

/// The user answered card `id` of `chat_id`.
pub async fn answer(state: &AppState, chat_id: &str, id: &str, a: Answer) -> anyhow::Result<&'static str> {
    let w = {
        let mut g = state.approvals.lock();
        match g.waiting.get(id) {
            Some(w) if w.chat_id == chat_id => g.waiting.remove(id),
            _ => None,
        }
    };
    let Some(w) = w else { return Err(crate::tr::err("approval.gone", &[])) };
    // "Always" only where the card offered it; otherwise it counts as once.
    let a = if a == Answer::Always && w.key.is_none() { Answer::Once } else { a };
    if let (Answer::Always, Some((kind, key))) = (a, &w.key) {
        let rule = Always { id: uuid::Uuid::new_v4().simple().to_string()[..12].to_string(), kind: kind.to_string(), tool: w.card.tool.clone(),
            key: key.clone(), what: w.card.what.clone(), target: w.card.target.clone(), at: Utc::now() };
        let snapshot = {
            let mut g = state.approvals.lock();
            let map = state.approvals.always_map(&mut g);
            let list = map.entry(chat_id.to_string()).or_default();
            if !list.iter().any(|x| x.kind == rule.kind && x.key == rule.key && x.tool == rule.tool) { list.push(rule); }
            Approvals::changed(&mut g)
        };
        state.approvals.save(snapshot);
        state.emit(json!({"type": "approval.changed", "chatId": chat_id}));
    }
    let status = match a { Answer::Once => "once", Answer::Always => "always", Answer::Deny => "denied" };
    crate::log::info(format!("chat {chat_id}: the user answered {status} about {}", w.card.tool));
    settle(state, chat_id, id, status).await;
    // The program may have gone meanwhile; the card says what the user chose either way.
    let _ = w.tx.send(a);
    Ok(status)
}

// ---------- the API the chat uses ----------

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/chats/:id/approvals", get(list))
        .route("/api/chats/:id/approvals/:aid", post(reply))
        .route("/api/chats/:id/always/:rid", delete(take_back))
}

type ApiResult = Result<Json<Value>, crate::api::ApiErr>;

/// `GET /api/chats/:id/approvals` → `{always: [...], waiting: [ids]}`.
async fn list(State(st): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    if st.chats.meta(&id).await.is_none() { return Err(crate::tr::err("not_found.chat", &[]).into()); }
    let waiting: Vec<String> = st.approvals.lock().waiting.iter().filter(|(_, w)| w.chat_id == id).map(|(k, _)| k.clone()).collect();
    Ok(Json(json!({"always": st.approvals.always(&id), "waiting": waiting})))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ReplyBody { answer: String }

/// `POST /api/chats/:id/approvals/:aid {answer: "once" | "always" | "deny"}` → `{status}`.
async fn reply(State(st): State<Arc<AppState>>, UrlPath((id, aid)): UrlPath<(String, String)>, Json(b): Json<ReplyBody>) -> ApiResult {
    let a = match b.answer.as_str() { "once" => Answer::Once, "always" => Answer::Always, "deny" => Answer::Deny, _ => return Err(crate::tr::err("approval.bad_answer", &[]).into()) };
    let status = answer(&st, &id, &aid, a).await?;
    Ok(Json(json!({"ok": true, "status": status})))
}

/// `DELETE /api/chats/:id/always/:rid` → `{always: [...]}` (what remains allowed).
async fn take_back(State(st): State<Arc<AppState>>, UrlPath((id, rid)): UrlPath<(String, String)>) -> ApiResult {
    let left = st.approvals.forget(&id, &rid);
    st.emit(json!({"type": "approval.changed", "chatId": id}));
    Ok(Json(json!({"always": left})))
}

// ---------- the MCP endpoint the assistant program calls ----------

/// The endpoint, mounted outside the API's token check: it has its own (one per run).
pub fn mcp_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(MCP_PATH, post(mcp_post).get(not_here).delete(not_here))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
}

/// No stream of server messages and no sessions to end: the endpoint only answers requests.
async fn not_here() -> Response { (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response() }

/// The protocol versions this endpoint speaks (it uses nothing that differs between them).
const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

fn rpc_ok(id: &Value, result: Value) -> Value { json!({"jsonrpc": "2.0", "id": id, "result": result}) }
fn rpc_err(id: &Value, code: i64, message: &str) -> Value { json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}) }

/// The answer to one JSON-RPC message that needs no waiting. `None` for a notification (or a
/// response to a request this server never makes): 202, nothing to say.
pub fn reply_to(msg: &Value) -> Option<Value> {
    let id = msg.get("id").filter(|v| !v.is_null())?;
    let method = msg["method"].as_str()?;
    Some(match method {
        "initialize" => {
            let asked = msg["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(VERSIONS[1]);
            rpc_ok(id, json!({"protocolVersion": version, "capabilities": {"tools": {}},
                "serverInfo": {"name": "thoughts-approvals", "version": env!("CARGO_PKG_VERSION")}}))
        }
        "ping" => rpc_ok(id, json!({})),
        "tools/list" => rpc_ok(id, json!({"tools": [{
            "name": TOOL,
            "description": "Used by Claude Code itself to ask the person using the Thoughts app for permission. Not for the assistant to call.",
            "inputSchema": {"type": "object", "properties": {
                "tool_name": {"type": "string"}, "input": {"type": "object"}, "tool_use_id": {"type": "string"}
            }, "required": ["tool_name", "input"]},
        }]})),
        "tools/call" => return None,
        _ => rpc_err(id, -32601, "method not found"),
    })
}

async fn mcp_post(State(st): State<Arc<AppState>>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    // A web page has no business here (the program sends no Origin).
    if headers.contains_key(header::ORIGIN) { return (StatusCode::FORBIDDEN, "origin not allowed").into_response(); }
    let token = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer ")).unwrap_or("").to_string();
    if !st.approvals.knows(&token) { return (StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(rpc_err(&Value::Null, -32700, "parse error"))).into_response(),
    };
    if !msg.is_object() { return (StatusCode::BAD_REQUEST, Json(rpc_err(&Value::Null, -32600, "one message per request"))).into_response(); }
    let is_call = msg["method"] == "tools/call" && msg.get("id").map_or(false, |v| !v.is_null());
    if !is_call {
        return match reply_to(&msg) { Some(v) => Json(v).into_response(), None => StatusCode::ACCEPTED.into_response() };
    }
    let id = msg["id"].clone();
    if msg["params"]["name"] != TOOL {
        return Json(rpc_ok(&id, json!({"content": [{"type": "text", "text": "unknown tool"}], "isError": true}))).into_response();
    }
    let req = Request::from_args(&msg["params"]["arguments"]);
    let streams = headers.get(header::ACCEPT).and_then(|h| h.to_str().ok()).map_or(false, |a| a.contains("text/event-stream"));
    let work = async move {
        let v = ask(&st, &token, req, WAIT).await;
        rpc_ok(&id, json!({"content": [{"type": "text", "text": v.text()}]}))
    };
    if !streams { return Json(work.await).into_response(); }
    // The answer can take minutes: the response starts now, a comment line keeps it alive, and the
    // result follows as the stream's one event.
    let stream = futures::stream::once(async move { Ok::<_, std::convert::Infallible>(Event::default().event("message").data(work.await.to_string())) });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEPALIVE).text("waiting for the user")).into_response()
}

#[cfg(test)]
#[path = "approvals_tests.rs"]
mod tests;
