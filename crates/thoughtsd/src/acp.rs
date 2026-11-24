//! The Agent Client Protocol (ACP, agentclientprotocol.com) client: the daemon talks to the official
//! adapters (`acp_install.rs`) over their stdio, newline-delimited JSON-RPC 2.0.
//!
//! What goes over ACP (see `docs/acp.md`):
//! - reading what an agent offers — its models, effort levels and permission modes — from what the
//!   adapter reports (`session/options`, else `session/new`), never from a list in the app (`probe`);
//! - Claude Code's ordinary chats (`claude::start_run`, kind `chat`): the same rules file, hooks,
//!   system text and guards as the command-line path, handed to the adapter as SDK options; the
//!   adapter's permission requests become the app's approval cards (`approvals::ask`).
//!
//! The protocol is small and its unstable parts (session config options, resume, fork) are what this
//! needs, so it is spoken here directly over tokio with `serde_json` rather than through the
//! `agent-client-protocol` crate, whose connection runs on its own executor and keeps these
//! methods behind unstable features.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

/// The ACP version this client speaks.
pub const PROTOCOL_VERSION: u64 = 1;
/// The longest line read from an agent.
const LINE_MAX: usize = 16 * 1024 * 1024;

/// Something the agent sent that is not an answer to us.
#[derive(Debug, Clone)]
pub enum Incoming {
    Notification { method: String, params: Value },
    Request { id: Value, method: String, params: Value },
}

type Pending = Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<Result<Value, Value>>>>>;

/// One JSON-RPC connection to an agent.
pub struct Conn {
    out: Mutex<Box<dyn AsyncWrite + Send + Unpin>>,
    pending: Pending,
    next: AtomicU64,
}

impl Conn {
    /// Start talking over `read`/`write`. What the agent sends of its own goes to the receiver;
    /// it closes when the agent's output ends.
    pub fn start(read: impl AsyncRead + Send + Unpin + 'static, write: impl AsyncWrite + Send + Unpin + 'static) -> (Arc<Conn>, mpsc::UnboundedReceiver<Incoming>) {
        let pending: Pending = Default::default();
        let (tx, rx) = mpsc::unbounded_channel();
        let p = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::with_capacity(64 * 1024, read);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match tokio::io::AsyncReadExt::take(&mut lines, LINE_MAX as u64).read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let Ok(v) = serde_json::from_slice::<Value>(&buf) else { continue };
                let method = v["method"].as_str().map(String::from);
                match (method, v.get("id").filter(|i| !i.is_null())) {
                    (Some(m), Some(id)) => { let _ = tx.send(Incoming::Request { id: id.clone(), method: m, params: v["params"].clone() }); }
                    (Some(m), None) => { let _ = tx.send(Incoming::Notification { method: m, params: v["params"].clone() }); }
                    (None, Some(id)) => {
                        let Some(n) = id.as_u64() else { continue };
                        let waiter = p.lock().unwrap_or_else(|e| e.into_inner()).remove(&n);
                        if let Some(w) = waiter {
                            let _ = w.send(match v.get("error") { Some(e) if !e.is_null() => Err(e.clone()), _ => Ok(v["result"].clone()) });
                        }
                    }
                    _ => {}
                }
            }
            // The agent is gone: nobody waits for an answer that cannot come.
            p.lock().unwrap_or_else(|e| e.into_inner()).clear();
        });
        (Arc::new(Conn { out: Mutex::new(Box::new(write)), pending, next: AtomicU64::new(1) }), rx)
    }

    async fn send(&self, v: &Value) -> anyhow::Result<()> {
        let mut line = serde_json::to_vec(v)?;
        line.push(b'\n');
        let mut out = self.out.lock().await;
        out.write_all(&line).await?;
        out.flush().await?;
        Ok(())
    }

    /// A request, and its answer (an error answer is an `Err` with the agent's message).
    pub async fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).await?;
        match rx.await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => anyhow::bail!("{}", error_text(&e)),
            Err(_) => anyhow::bail!("the agent ended before answering {method}"),
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> anyhow::Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params})).await
    }

    pub async fn respond(&self, id: &Value, result: Value) -> anyhow::Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result})).await
    }

    pub async fn respond_error(&self, id: &Value, code: i64, message: &str) -> anyhow::Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})).await
    }
}

/// An agent's error as one line of text.
pub fn error_text(e: &Value) -> String {
    let msg = e["message"].as_str().unwrap_or("the agent reported an error");
    match e["data"].as_str().or_else(|| e["data"]["details"].as_str()) {
        Some(d) if !d.is_empty() && d != msg => format!("{msg}: {d}"),
        _ => msg.to_string(),
    }
}

/// `initialize`: what this client can do is nothing on the file system or terminal of its own —
/// the agent uses its own tools, under the rules the run is started with.
pub async fn initialize(conn: &Conn) -> anyhow::Result<Value> {
    let r = conn.request("initialize", json!({
        "protocolVersion": PROTOCOL_VERSION,
        "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
        "clientInfo": {"name": "thoughts", "version": env!("CARGO_PKG_VERSION")},
    })).await?;
    if r["protocolVersion"].as_u64().map_or(false, |v| v < PROTOCOL_VERSION) {
        anyhow::bail!("the agent speaks an older version of ACP ({})", r["protocolVersion"]);
    }
    Ok(r)
}

// ---------- what an agent offers ----------

/// One choice an agent offers: its id and the name it gives it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Choice { pub id: String, pub name: String }

/// The models, effort levels and permission modes an agent reported, each with the config option
/// that sets it (when it reported them as config options).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub models: Vec<Choice>,
    pub efforts: Vec<Choice>,
    pub modes: Vec<Choice>,
    pub current_model: Option<String>,
    pub current_effort: Option<String>,
    pub current_mode: Option<String>,
    #[serde(skip)] pub model_option: Option<String>,
    #[serde(skip)] pub effort_option: Option<String>,
    #[serde(skip)] pub mode_option: Option<String>,
}

fn choices(options: &Value) -> Vec<Choice> {
    let mut out = vec![];
    for o in options.as_array().into_iter().flatten() {
        // A grouped list: `{group, name, options: [...]}`.
        if o["options"].is_array() { out.extend(choices(&o["options"])); continue; }
        let Some(id) = o["value"].as_str().or_else(|| o["id"].as_str()).or_else(|| o["modelId"].as_str()) else { continue };
        let name = o["name"].as_str().filter(|n| !n.trim().is_empty()).unwrap_or(id);
        if !out.iter().any(|c: &Choice| c.id == id) { out.push(Choice { id: id.into(), name: name.into() }); }
    }
    out
}

/// The catalog in a `session/new` (or `session/options`, `resume`, `fork`) answer. Config options
/// win over the older `modes` / `models` fields, which are read where they are all there is.
pub fn catalog(r: &Value) -> Catalog {
    let mut c = Catalog::default();
    for o in r["configOptions"].as_array().into_iter().flatten() {
        let (id, cat) = (o["id"].as_str().unwrap_or(""), o["category"].as_str().unwrap_or(""));
        let cur = o["currentValue"].as_str().map(String::from);
        let list = choices(&o["options"]);
        if list.is_empty() { continue; }
        let which = match (cat, id) {
            ("mode", _) | (_, "mode") => Some(0),
            ("model", _) | (_, "model") => Some(1),
            ("thought_level", _) | (_, "effort") | (_, "reasoning_effort") => Some(2),
            _ => None,
        };
        match which {
            Some(0) if c.mode_option.is_none() => { c.modes = list; c.current_mode = cur; c.mode_option = Some(id.into()); }
            Some(1) if c.model_option.is_none() => { c.models = list; c.current_model = cur; c.model_option = Some(id.into()); }
            Some(2) if c.effort_option.is_none() => { c.efforts = list; c.current_effort = cur; c.effort_option = Some(id.into()); }
            _ => {}
        }
    }
    if c.modes.is_empty() {
        c.modes = choices(&r["modes"]["availableModes"]);
        c.current_mode = r["modes"]["currentModeId"].as_str().map(String::from);
    }
    if c.models.is_empty() {
        c.models = choices(&r["models"]["availableModels"]);
        c.current_model = r["models"]["currentModelId"].as_str().map(String::from);
    }
    c
}

/// Make the session use `value` for the config option `option` (or, for the mode without one,
/// `session/set_mode`). A value the agent did not offer is never sent.
pub async fn choose(conn: &Conn, session: &str, option: Option<&str>, offered: &[Choice], value: &str, is_mode: bool) -> anyhow::Result<()> {
    if !offered.iter().any(|c| c.id == value) { anyhow::bail!("the agent does not offer \"{value}\""); }
    match option {
        Some(o) => { conn.request("session/set_config_option", json!({"sessionId": session, "configId": o, "value": value})).await?; }
        None if is_mode => { conn.request("session/set_mode", json!({"sessionId": session, "modeId": value})).await?; }
        None => { conn.request("session/set_model", json!({"sessionId": session, "modelId": value})).await?; }
    }
    Ok(())
}

// ---------- permission requests ----------

/// How much a permission mode lets the agent do without asking, least first. A mode not listed is
/// unknown and counts as no mode at all (never "within" another). Claude Code's adapter names its
/// modes as Claude Code does; Codex's (`codex_acp`) by its sandbox: `read-only` (nothing changes),
/// `workspace-write` (the folders it works in), `agent` (the same, with a model approving what it
/// asks), `agent-full-access` (no sandbox). Its permission profiles (`permission-profile:…`) are unknown.
pub fn strictness(mode: &str) -> Option<u8> {
    match mode {
        "plan" | "read-only" => Some(0),
        "default" => Some(1),
        "acceptEdits" | "workspace-write" => Some(2),
        crate::settings::AUTO | "agent" => Some(3),
        crate::settings::BYPASS | "agent-full-access" => Some(4),
        _ => None,
    }
}

/// May a session checked for mode `checked` be in mode `now`? Only the same mode or a stricter one.
pub fn mode_within(checked: &str, now: &str) -> bool {
    matches!((strictness(checked), strictness(now)), (Some(c), Some(n)) if n <= c)
}

/// The permission mode an option switches the session to, as the agent names it (the mode ids
/// themselves, or Claude Code's adapter's "leave plan mode" choices).
fn option_mode(id: &str) -> Option<&'static str> {
    Some(match id {
        "default" | "exit-plan-default" => "default",
        "acceptEdits" | "exit-plan-accept-edits" | "exit-plan-clear-accept-edits" => "acceptEdits",
        "auto" | "exit-plan-auto" | "exit-plan-clear-auto" => crate::settings::AUTO,
        "bypassPermissions" | "exit-plan-bypass" | "exit-plan-clear-bypass" => crate::settings::BYPASS,
        "plan" => "plan",
        _ => return None,
    })
}

/// The plain answers that change no mode: a yes (once, or with the rule the agent proposes) or a no.
const PLAIN_ALLOW: &[&str] = &["allow-once", "allow_once", "allow", "allow-always", "allow_always", "always", "allow-with-updates", "allow-skill-exact", "allow-skill-prefix"];
const PLAIN_REJECT: &[&str] = &["reject", "reject-once", "reject_once", "reject-always", "reject_always", "no", "deny"];

/// The option to answer a permission request with. `allow` picks a one-time (else standing) yes,
/// `false` a one-time (else standing) no. Only options on an allowlist are ever picked: the plain
/// answers (`PLAIN_ALLOW` / `PLAIN_REJECT`), and options that switch the mode only when the new mode
/// is the chat's checked mode `checked` or a stricter one (`mode_within`). With full access on
/// (`full`) every option is allowed, as every other guard yields. `None`: answer "cancelled".
pub fn pick_option(options: &Value, allow: bool, checked: &str, full: bool) -> Option<String> {
    let list: Vec<&Value> = options.as_array().into_iter().flatten().collect();
    let allowed = |o: &Value| {
        let id = o["optionId"].as_str().unwrap_or("");
        if full { return !id.is_empty(); }
        match option_mode(id) {
            Some(m) => mode_within(checked, m),
            None => if allow { PLAIN_ALLOW.contains(&id) } else { PLAIN_REJECT.contains(&id) },
        }
    };
    let kinds: &[&str] = if allow { &["allow_once", "allow_always"] } else { &["reject_once", "reject_always"] };
    for k in kinds {
        if let Some(o) = list.iter().find(|o| o["kind"] == *k && allowed(o)) { return o["optionId"].as_str().map(String::from); }
    }
    None
}

/// The answer to `session/request_permission`.
pub fn permission_answer(option: Option<String>) -> Value {
    match option {
        Some(id) => json!({"outcome": {"outcome": "selected", "optionId": id}}),
        None => json!({"outcome": {"outcome": "cancelled"}}),
    }
}

/// The tool a permission request (or tool call) is about, as Claude Code names it.
pub fn tool_name(call: &Value) -> String {
    call["_meta"]["claudeCode"]["toolName"].as_str()
        .or_else(|| call["title"].as_str())
        .unwrap_or("tool").to_string()
}

// ---------- updates as the stream the chat already shows ----------

/// Turns `session/update` notifications into the events of Claude Code's `stream-json` output, which
/// `claude::handle_event` already shows in the chat: text as it streams and as a message, tool calls
/// and their results, and the final result.
#[derive(Default)]
pub struct Translator {
    text: String,
    calls: HashMap<String, bool>,
    pub cost: f64,
    pub mode: Option<String>,
}

impl Translator {
    fn flush(&mut self, out: &mut Vec<Value>) {
        let t = std::mem::take(&mut self.text);
        if !t.trim().is_empty() { out.push(json!({"type": "assistant", "message": {"content": [{"type": "text", "text": t}]}})); }
    }

    pub fn update(&mut self, u: &Value) -> Vec<Value> {
        let mut out = vec![];
        match u["sessionUpdate"].as_str().unwrap_or("") {
            "agent_message_chunk" => if let Some(t) = u["content"]["text"].as_str() {
                self.text.push_str(t);
                out.push(json!({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": t}}}));
            },
            "agent_thought_chunk" => if let Some(t) = u["content"]["text"].as_str() {
                out.push(json!({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "thinking_delta", "thinking": t}}}));
            },
            "tool_call" => {
                let id = u["toolCallId"].as_str().unwrap_or("").to_string();
                if !self.calls.contains_key(&id) {
                    self.flush(&mut out);
                    self.calls.insert(id.clone(), false);
                    let input = if u["rawInput"].is_object() { u["rawInput"].clone() } else { json!({}) };
                    out.push(json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": id, "name": tool_name(u), "input": input}]}}));
                }
                out.extend(self.result_of(u));
            }
            "tool_call_update" => out.extend(self.result_of(u)),
            "current_mode_update" => self.mode = u["currentModeId"].as_str().map(String::from),
            // The mode as a config option: whatever the agent now says its value is.
            "config_option_update" => {
                let opts = if u["configOptions"].is_array() { u["configOptions"].clone() } else { json!([u["configOption"].clone()]) };
                for o in opts.as_array().into_iter().flatten() {
                    if o["category"] == "mode" || o["id"] == "mode" {
                        self.mode = Some(o["currentValue"].as_str().unwrap_or("").to_string());
                    }
                }
            }
            "usage_update" => if let Some(a) = u["cost"]["amount"].as_f64() { self.cost = a; },
            _ => {}
        }
        out
    }

    fn result_of(&mut self, u: &Value) -> Option<Value> {
        let id = u["toolCallId"].as_str()?.to_string();
        let status = u["status"].as_str()?;
        if status != "completed" && status != "failed" { return None; }
        if self.calls.get(&id) == Some(&true) { return None; }
        self.calls.insert(id.clone(), true);
        let mut text: Vec<String> = vec![];
        for c in u["content"].as_array().into_iter().flatten() {
            if let Some(t) = c["content"]["text"].as_str() { text.push(t.to_string()); }
            if c["type"] == "diff" { text.push(format!("Changed {}", c["path"].as_str().unwrap_or(""))); }
        }
        if text.is_empty() { if let Some(s) = u["rawOutput"].as_str() { text.push(s.to_string()); } }
        Some(json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": text.join("\n"), "is_error": status == "failed"}]}}))
    }

    /// The end of a turn: what was still being written, and the final `result`.
    pub fn end(&mut self, stop: Result<&str, String>, session: &str) -> Vec<Value> {
        let mut out = vec![];
        self.flush(&mut out);
        let (ok, text) = match stop {
            Ok("refusal") => (false, Some("The model declined to answer.".to_string())),
            Ok(_) => (true, None),
            Err(e) => (false, Some(e)),
        };
        let mut r = json!({"type": "result", "subtype": if ok { "success" } else { "error_during_execution" }, "is_error": !ok,
            "session_id": session, "total_cost_usd": self.cost});
        if let Some(t) = text { r["result"] = json!(t); }
        out.push(r);
        out
    }
}

#[cfg(test)]
#[path = "acp_tests.rs"]
mod tests;
