//! Claude Code provider: discovery of options from the installed CLI, auth
//! status, and streaming runs via `claude -p --output-format stream-json`.

use crate::chats::{Msg, RunOptions};
use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// PATH from the user's login shell — GUI apps on macOS start with a bare PATH.
pub fn shell_path() -> &'static str {
    static P: OnceLock<String> = OnceLock::new();
    P.get_or_init(|| {
        let base = std::env::var("PATH").unwrap_or_default();
        let mut parts: Vec<String> = vec![];
        #[cfg(unix)]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
            if let Ok(o) = std::process::Command::new(shell).args(["-lc", "printf %s \"$PATH\""]).output() {
                parts.extend(String::from_utf8_lossy(&o.stdout).split(':').map(String::from));
            }
        }
        parts.extend(base.split(if cfg!(windows) { ';' } else { ':' }).map(String::from));
        if let Some(h) = dirs::home_dir() {
            for d in [".local/bin", ".claude/local", ".npm-global/bin", ".cargo/bin", ".bun/bin"] {
                parts.push(h.join(d).to_string_lossy().into());
            }
        }
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
            parts.push(d.into());
        }
        let mut seen = std::collections::HashSet::new();
        parts.retain(|p| !p.is_empty() && seen.insert(p.clone()));
        parts.join(if cfg!(windows) { ";" } else { ":" })
    })
}

pub fn find_binary(custom: &Option<String>, name: &str) -> Option<PathBuf> {
    if let Some(c) = custom.as_ref().filter(|c| !c.is_empty()) {
        let p = PathBuf::from(c);
        return p.exists().then_some(p);
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    for dir in shell_path().split(sep) {
        for cand in [name.to_string(), format!("{name}.exe"), format!("{name}.cmd")] {
            let p = Path::new(dir).join(&cand);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub async fn binary(state: &AppState) -> Option<PathBuf> {
    let custom = state.settings.read().await.ai.claude.binary.clone();
    find_binary(&custom, "claude")
}

fn cmd(bin: &Path) -> Command {
    let mut c = Command::new(bin);
    c.env("PATH", shell_path());
    c.stdin(std::process::Stdio::null());
    c.kill_on_drop(true);
    c
}

/// `{installed, version, loggedIn, auth}`
pub async fn status(state: &AppState) -> Value {
    let Some(bin) = binary(state).await else {
        return json!({"installed": false});
    };
    let version = cmd(&bin).arg("--version").output().await.ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let auth: Value = cmd(&bin).args(["auth", "status", "--json"]).output().await.ok()
        .and_then(|o| serde_json::from_slice(&o.stdout).ok()).unwrap_or(Value::Null);
    let has_token = state.settings.read().await.ai.claude.has_api_token;
    let logged_in = auth.get("loggedIn").and_then(Value::as_bool).unwrap_or(false) || has_token;
    json!({"installed": true, "path": bin, "version": version, "loggedIn": logged_in, "auth": auth})
}

/// Pull the choices for a flag out of `claude --help`, so the UI never ships a
/// stale static list. Returns models (aliases), efforts and permission modes.
pub async fn options(state: &AppState) -> Value {
    let Some(bin) = binary(state).await else { return json!({}) };
    let help = cmd(&bin).arg("--help").output().await.ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    json!({
        "models": quoted(&flag_block(&help, "--model")),
        "efforts": paren_list(&flag_block(&help, "--effort")),
        "permissionModes": paren_list(&flag_block(&help, "--permission-mode")),
    })
}

/// The help text belonging to one flag: its line plus indented continuation lines.
pub fn flag_block(help: &str, flag: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in help.lines() {
        let t = line.trim_start();
        let is_flag_line = t.starts_with('-') && line.len() - t.len() <= 4;
        if is_flag_line {
            if inside { break; }
            let names = t.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
            inside = names.split([',', ' ']).any(|n| n == flag);
        }
        if inside {
            out.push_str(t);
            out.push(' ');
        }
    }
    out
}

fn paren_list(block: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\(([^()]*)\)").unwrap();
    for cap in re.captures_iter(block) {
        let inner = cap[1].trim_start_matches("choices:").trim();
        let items: Vec<String> = inner.split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|s| !s.is_empty() && !s.contains(' '))
            .collect();
        if items.len() >= 2 {
            return items;
        }
    }
    vec![]
}

fn quoted(block: &str) -> Vec<String> {
    let re = regex::Regex::new(r"'([A-Za-z0-9._\-\[\]]+)'").unwrap();
    re.captures_iter(block).map(|c| c[1].to_string()).collect()
}

async fn provider_env(state: &AppState, c: &mut Command) {
    let s = state.settings.read().await.ai.claude.clone();
    if let Some(url) = s.base_url.filter(|u| !u.is_empty()) {
        c.env("ANTHROPIC_BASE_URL", url);
        if let Ok(tok) = keyring::Entry::new("thoughts-ai", "claude").and_then(|e| e.get_password()) {
            c.env("ANTHROPIC_AUTH_TOKEN", tok);
        }
    }
}

pub struct RunSpec {
    pub chat_id: String,
    pub prompt: String,
    pub cwd: PathBuf,
    pub add_dirs: Vec<PathBuf>,
    pub options: RunOptions,
    pub resume: Option<String>,
    pub append_system: Option<String>,
}

/// Start a streaming run in the background. Events go out on the broadcast
/// channel; the finished chat is persisted. `on_done` runs after completion.
pub async fn start_run(
    state: Arc<AppState>,
    spec: RunSpec,
    on_done: Option<Box<dyn FnOnce(Arc<AppState>, bool) -> futures::future::BoxFuture<'static, ()> + Send>>,
) -> anyhow::Result<()> {
    let bin = binary(&state).await.ok_or_else(|| anyhow::anyhow!("Claude Code CLI not found"))?;
    let mut c = cmd(&bin);
    c.current_dir(&spec.cwd)
        .args(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"]);
    if let Some(r) = &spec.resume { c.args(["--resume", r]); }
    if let Some(m) = spec.options.model.as_ref().filter(|s| !s.is_empty()) { c.args(["--model", m]); }
    if let Some(e) = spec.options.effort.as_ref().filter(|s| !s.is_empty()) { c.args(["--effort", e]); }
    if let Some(p) = spec.options.permission_mode.as_ref().filter(|s| !s.is_empty()) { c.args(["--permission-mode", p]); }
    for d in &spec.add_dirs { c.arg("--add-dir").arg(d); }
    if let Some(sys) = &spec.append_system { c.args(["--append-system-prompt", sys]); }
    provider_env(&state, &mut c).await;
    c.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = c.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let prompt = spec.prompt.clone();
    tokio::spawn(async move {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    });

    let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
    state.runs.lock().await.insert(spec.chat_id.clone(), kill_tx);
    let id = spec.chat_id.clone();
    if let Some(ch) = state.chats.update(&id, |c| c.running = true).await {
        state.emit(json!({"type": "chat.updated", "chat": ch.summary()}));
    }

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut err_lines = BufReader::new(stderr).lines();
        let err_task = tokio::spawn(async move {
            let mut buf = String::new();
            while let Ok(Some(l)) = err_lines.next_line().await {
                if buf.len() < 4000 { buf.push_str(&l); buf.push('\n'); }
            }
            buf
        });
        let mut ok = false;
        let mut got_result = false;
        loop {
            tokio::select! {
                _ = &mut kill_rx => {
                    let _ = child.kill().await;
                    push(&state, &id, Msg::new("system", "Stopped.")).await;
                    break;
                }
                line = lines.next_line() => {
                    let Ok(Some(line)) = line else { break };
                    let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
                    if let Some(r) = handle_event(&state, &id, &ev).await {
                        got_result = true;
                        ok = r;
                    }
                }
            }
        }
        let _ = child.wait().await;
        let stderr_text = err_task.await.unwrap_or_default();
        if !got_result && !stderr_text.trim().is_empty() {
            push(&state, &id, Msg::new("error", stderr_text.trim())).await;
        }
        state.runs.lock().await.remove(&id);
        if let Some(ch) = state.chats.update(&id, |c| { c.running = false; c.updated = chrono::Utc::now(); }).await {
            state.emit(json!({"type": "chat.done", "chatId": id, "ok": ok, "title": ch.title, "chat": ch.summary()}));
        }
        if let Some(f) = on_done { f(state.clone(), ok).await; }
    });
    Ok(())
}

pub async fn push(state: &AppState, chat_id: &str, m: Msg) {
    let mv = serde_json::to_value(&m).unwrap();
    state.chats.update(chat_id, |c| c.messages.push(m)).await;
    state.emit(json!({"type": "chat.message", "chatId": chat_id, "message": mv}));
}

/// Returns Some(success) on the final `result` event.
async fn handle_event(state: &AppState, id: &str, ev: &Value) -> Option<bool> {
    match ev["type"].as_str()? {
        "system" if ev["subtype"] == "init" => {
            if let Some(sid) = ev["session_id"].as_str() {
                let sid = sid.to_string();
                state.chats.update(id, |c| c.session_id = Some(sid)).await;
            }
            None
        }
        "stream_event" => {
            let e = &ev["event"];
            if e["type"] == "content_block_delta" {
                if let Some(t) = e["delta"]["text"].as_str() {
                    state.emit(json!({"type": "chat.delta", "chatId": id, "text": t}));
                } else if let Some(t) = e["delta"]["thinking"].as_str() {
                    state.emit(json!({"type": "chat.thinking", "chatId": id, "text": t}));
                }
            }
            None
        }
        "assistant" => {
            for block in ev["message"]["content"].as_array().into_iter().flatten() {
                match block["type"].as_str() {
                    Some("text") => {
                        let t = block["text"].as_str().unwrap_or("");
                        if !t.trim().is_empty() { push(state, id, Msg::new("assistant", t)).await; }
                    }
                    Some("tool_use") => {
                        let name = block["name"].as_str().unwrap_or("tool");
                        let mut m = Msg::new("tool", tool_label(name, &block["input"]));
                        m.tool = Some(json!({"name": name, "input": block["input"], "id": block["id"]}));
                        push(state, id, m).await;
                    }
                    _ => {}
                }
            }
            None
        }
        "user" => {
            for block in ev["message"]["content"].as_array().into_iter().flatten() {
                if block["type"] == "tool_result" {
                    let text = match &block["content"] {
                        Value::String(s) => s.clone(),
                        Value::Array(a) => a.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join("\n"),
                        _ => String::new(),
                    };
                    let mut short: String = text.chars().take(2000).collect();
                    if text.chars().count() > 2000 { short.push('…'); }
                    let mut m = Msg::new("tool_result", short);
                    m.tool = Some(json!({"toolUseId": block["tool_use_id"], "isError": block["is_error"]}));
                    push(state, id, m).await;
                }
            }
            None
        }
        "result" => {
            let is_err = ev["is_error"].as_bool().unwrap_or(false) || ev["subtype"] != "success";
            let cost = ev["total_cost_usd"].as_f64().unwrap_or(0.0);
            let sid = ev["session_id"].as_str().map(String::from);
            state.chats.update(id, |c| {
                c.cost_usd += cost;
                if sid.is_some() { c.session_id = sid; }
            }).await;
            if is_err {
                let txt = ev["result"].as_str().map(String::from)
                    .unwrap_or_else(|| format!("Run ended: {}", ev["subtype"].as_str().unwrap_or("error")));
                // The CLI often echoes the error as assistant text first; show it once, as an error.
                state.chats.update(id, |c| {
                    if c.messages.last().map_or(false, |m| m.role == "assistant" && m.text.trim() == txt.trim()) { c.messages.pop(); }
                }).await;
                push(state, id, Msg::new("error", txt)).await;
            }
            Some(!is_err)
        }
        _ => None,
    }
}

fn tool_label(name: &str, input: &Value) -> String {
    let arg = ["file_path", "command", "pattern", "path", "url", "description", "prompt"]
        .iter().find_map(|k| input[k].as_str()).unwrap_or("");
    let arg: String = arg.chars().take(140).collect();
    if arg.is_empty() { name.to_string() } else { format!("{name}: {arg}") }
}

/// One-shot, tool-less completion (used for mini-wraps and titles).
pub async fn complete(state: &AppState, prompt: &str, model: Option<String>) -> anyhow::Result<String> {
    let bin = binary(state).await.ok_or_else(|| anyhow::anyhow!("Claude Code CLI not found"))?;
    let mut c = cmd(&bin);
    c.current_dir(crate::paths::home())
        .args(["-p", "--output-format", "json", "--no-session-persistence", "--tools", ""]);
    if let Some(m) = model.filter(|m| !m.is_empty()) { c.args(["--model", &m]); }
    provider_env(state, &mut c).await;
    c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = c.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(prompt.as_bytes()).await?;
    drop(stdin);
    let out = child.wait_with_output().await?;
    let v: Value = serde_json::from_slice(&out.stdout)
        .map_err(|_| anyhow::anyhow!("claude: {}", String::from_utf8_lossy(&out.stderr)))?;
    if v["is_error"].as_bool().unwrap_or(false) {
        anyhow::bail!("{}", v["result"].as_str().unwrap_or("error"));
    }
    Ok(v["result"].as_str().unwrap_or("").trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    const HELP: &str = "Options:\n  --effort <level>                      Effort level for the current session\n                                        (low, medium, high, xhigh, max)\n  --environment <id>   x\n  --model <model>                       Model. Provide\n                                        an alias (e.g.\n                                        'fable', 'opus', or 'sonnet') or full.\n  --permission-mode <mode>              Permission mode\n                                        (choices: \"acceptEdits\", \"auto\",\n                                        \"plan\")\n  --other x\n";
    #[test]
    fn parses_help() {
        assert_eq!(paren_list(&flag_block(HELP, "--effort")), ["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(paren_list(&flag_block(HELP, "--permission-mode")), ["acceptEdits", "auto", "plan"]);
        assert_eq!(quoted(&flag_block(HELP, "--model")), ["fable", "opus", "sonnet"]);
    }
}
