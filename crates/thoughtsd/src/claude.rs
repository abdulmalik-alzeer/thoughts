//! Claude Code provider: discovery of options from the installed CLI, auth
//! status, and streaming runs via `claude -p --output-format stream-json`.

use crate::chats::{Msg, RunOptions};
use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// How long a one-shot completion (mini-wrap, title) may take before it is abandoned.
pub const COMPLETE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// PATH from the user's login shell — GUI apps on macOS start with a bare PATH.
pub fn shell_path() -> &'static str {
    static P: OnceLock<String> = OnceLock::new();
    P.get_or_init(|| {
        let base = std::env::var("PATH").unwrap_or_default();
        let mut parts: Vec<String> = vec![];
        #[cfg(unix)]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
            if let Ok(o) = crate::proc::std_command(shell).args(["-lc", "printf %s \"$PATH\""]).output() {
                parts.extend(String::from_utf8_lossy(&o.stdout).split(':').map(String::from));
            }
        }
        parts.extend(base.split(if cfg!(windows) { ';' } else { ':' }).map(String::from));
        if let Some(h) = dirs::home_dir() {
            for d in [".local/bin", ".claude/local", ".npm-global/bin", ".cargo/bin", ".bun/bin"] {
                parts.push(h.join(d).to_string_lossy().into());
            }
        }
        #[cfg(not(windows))]
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
            parts.push(d.into());
        }
        // Where npm, the native installer and Git for Windows put their programs. UNTESTED on Windows.
        #[cfg(windows)]
        for (var, subs) in [("APPDATA", &["npm"][..]), ("LOCALAPPDATA", &["Programs\\claude", "Programs\\Git\\cmd", "Microsoft\\WinGet\\Links", "AnthropicClaude"][..]),
                            ("ProgramFiles", &["Git\\cmd", "nodejs"][..])] {
            if let Ok(base) = std::env::var(var) {
                for sub in subs { parts.push(Path::new(&base).join(sub).to_string_lossy().into()); }
            }
        }
        let mut seen = std::collections::HashSet::new();
        parts.retain(|p| !p.is_empty() && seen.insert(p.clone()));
        parts.join(if cfg!(windows) { ";" } else { ":" })
    })
}

pub fn find_binary(custom: &Option<String>, name: &str) -> Option<PathBuf> {
    if let Some(c) = custom.as_ref().filter(|c| !c.is_empty()) {
        let p = PathBuf::from(c);
        return (p.is_absolute() && p.is_file()).then_some(p);
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    let cands = candidates(name, cfg!(windows), &std::env::var("PATHEXT").unwrap_or_default());
    for dir in shell_path().split(sep) {
        for cand in &cands {
            let p = Path::new(dir).join(cand);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// File names to look for when searching for program `name`. On Windows a real `.exe` wins over
/// the `.cmd` shim npm installs, then anything else PATHEXT lists; the bare name (a shell script
/// there) is never used. Elsewhere the bare name comes first, as always.
pub fn candidates(name: &str, windows: bool, pathext: &str) -> Vec<String> {
    let mut out: Vec<String> = if windows { vec![format!("{name}.exe"), format!("{name}.cmd")] }
        else { vec![name.to_string(), format!("{name}.exe"), format!("{name}.cmd")] };
    if windows {
        for ext in pathext.split(';').map(|e| e.trim().to_lowercase()).filter(|e| e.starts_with('.') && e.len() > 1) {
            let c = format!("{name}{ext}");
            if !out.contains(&c) { out.push(c); }
        }
    }
    out
}

pub async fn binary(state: &AppState) -> Option<PathBuf> {
    let custom = state.settings.read().await.ai.claude.binary.clone();
    find_binary(&custom, "claude")
}

fn cmd(bin: &Path) -> Command {
    let mut c = crate::proc::command(bin);
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
        // Only the modes the daemon accepts, in a stable order — never bypassPermissions.
        "permissionModes": crate::settings::PERMISSION_MODES,
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
        // Never hand a token to an address that would not pass settings validation.
        if crate::settings::check_base_url(&url).is_err() { return; }
        c.env("ANTHROPIC_BASE_URL", url);
        // The token belongs to the address it was saved for: `has_api_token` is cleared whenever the address changes.
        if !s.has_api_token { return; }
        if let Some(tok) = crate::secrets::get("thoughts-ai", "claude") {
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
    /// Extra permission deny rules for this run (e.g. `Edit(//path/**)`), merged with the private-folder rules.
    pub extra_deny: Vec<String>,
}

/// What every run is told about the user's language.
pub fn language_note(lang: &str) -> &'static str {
    if lang == "ar" {
        "The user's interface language is Arabic. Reply, and write all notes, titles, summaries and todos, in Arabic unless the user writes in another language."
    } else {
        "The user's interface language is English. Reply, and write all notes, titles, summaries and todos, in the language the user writes in (English by default)."
    }
}

/// `--settings` JSON denying the AI any access to the user's private folders. `None` when there are none.
/// Rules are given both relative to the library (when the run starts there) and as absolute paths,
/// so they hold when the library is only an added folder.
pub fn private_deny_settings(library: Option<&Path>, cwd: &Path, private: &[String]) -> anyhow::Result<Option<String>> {
    let mut deny: Vec<String> = vec![];
    for raw in private {
        let rel = crate::paths::clean_rel(raw)?;
        let mut pats = vec![];
        if let Some(lib) = library {
            if lib == cwd { pats.push(format!("./{rel}")); }
            let abs = lib.join(&rel).to_string_lossy().replace('\\', "/");
            // Absolute paths in permission rules are written with a leading `//` (`//c/…` on Windows).
            let abs = match abs.strip_prefix('/') {
                Some(rest) => format!("//{rest}"),
                None => format!("//{}", abs.replacen(':', "", 1)),
            };
            pats.push(abs);
        } else {
            pats.push(format!("./{rel}"));
        }
        for p in pats {
            for tool in ["Read", "Edit"] {
                deny.push(format!("{tool}({p})"));
                deny.push(format!("{tool}({p}/**)"));
            }
        }
        // BEST-EFFORT ONLY: Read/Edit rules do not cover the shell, so also refuse commands that
        // name a private folder. A command that reaches it indirectly (a variable, a glob from the
        // parent folder, a script) is not caught — private folders are a guard against accidents,
        // not a sandbox.
        match library {
            Some(lib) => {
                deny.push(format!("Bash(*{}*)", lib.join(&rel).to_string_lossy().replace('\\', "/")));
                if lib == cwd { for pat in bash_relative_patterns(&rel) { deny.push(pat); } }
            }
            None => for pat in bash_relative_patterns(&rel) { deny.push(pat); },
        }
    }
    if deny.is_empty() { return Ok(None); }
    Ok(Some(json!({"permissions": {"deny": deny}}).to_string()))
}

/// Shell command shapes that mention library-relative folder `rel` from inside the library.
fn bash_relative_patterns(rel: &str) -> Vec<String> {
    vec![format!("Bash(*./{rel}/*)"), format!("Bash(* ./{rel})"), format!("Bash(* {rel}/*)"), format!("Bash(* {rel})")]
}

/// Combine two `--settings` deny documents into one.
fn merge_deny(a: Option<String>, b: Option<String>) -> Option<String> {
    let rules = |s: &Option<String>| -> Vec<Value> {
        s.as_deref().and_then(|t| serde_json::from_str::<Value>(t).ok())
            .and_then(|v| v["permissions"]["deny"].as_array().cloned()).unwrap_or_default()
    };
    let mut all = rules(&a);
    for r in rules(&b) { if !all.contains(&r) { all.push(r); } }
    if all.is_empty() { None } else { Some(json!({"permissions": {"deny": all}}).to_string()) }
}

/// Start a streaming run in the background. Events go out on the broadcast
/// channel; the finished chat is persisted. `on_done` runs after completion.
pub async fn start_run(
    state: Arc<AppState>,
    spec: RunSpec,
    on_done: Option<Box<dyn FnOnce(Arc<AppState>, bool) -> futures::future::BoxFuture<'static, ()> + Send>>,
) -> anyhow::Result<()> {
    // The one gate every AI run passes through (chats, wrap, reorganize, modules, customize).
    crate::settings::check_permission_mode(spec.options.permission_mode.as_deref())?;
    crate::settings::check_cli_value("model", spec.options.model.as_deref())?;
    crate::settings::check_cli_value("effort", spec.options.effort.as_deref())?;
    crate::settings::check_cli_value("session", spec.resume.as_deref())?;
    let settings = state.settings.read().await.clone();
    // Nothing runs in a folder the user has not trusted. The app layer is the app's own folder.
    // Its staging copy likewise: the app creates it, strips any settings from it, and runs there
    // only under the deny rules in `extra_deny`.
    let (layer, staging) = (crate::paths::app_layer_dir(), crate::paths::app_layer_staging_dir());
    for dir in std::iter::once(&spec.cwd).chain(spec.add_dirs.iter()) {
        if *dir != layer && *dir != staging { crate::trust::require(dir)?; }
    }
    let mut deny = private_deny_settings(settings.library().as_deref(), &spec.cwd, &settings.private_paths)?;
    // Reorganize works in a second checkout of the library: private folders are off-limits there too.
    if spec.cwd == crate::paths::reorganize_worktree() {
        deny = merge_deny(deny, private_deny_settings(Some(&spec.cwd), &spec.cwd, &settings.private_paths)?);
    }
    if !spec.extra_deny.is_empty() {
        deny = merge_deny(deny, Some(json!({"permissions": {"deny": spec.extra_deny}}).to_string()));
    }
    let bin = binary(&state).await.ok_or_else(|| anyhow::anyhow!("Claude Code CLI not found"))?;
    let mut c = cmd(&bin);
    c.current_dir(&spec.cwd)
        .args(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"]);
    if let Some(d) = &deny { c.args(["--settings", d]); }
    if let Some(r) = &spec.resume { c.args(["--resume", r]); }
    if let Some(m) = spec.options.model.as_ref().filter(|s| !s.is_empty()) { c.args(["--model", m]); }
    if let Some(e) = spec.options.effort.as_ref().filter(|s| !s.is_empty()) { c.args(["--effort", e]); }
    // "default" means the CLI's own default: pass no flag (newer CLIs no longer accept the word).
    if let Some(p) = spec.options.permission_mode.as_ref().filter(|s| !s.is_empty() && *s != "default") { c.args(["--permission-mode", p]); }
    for d in &spec.add_dirs { c.arg("--add-dir").arg(d); }
    let lang = language_note(&settings.language);
    let sys = match &spec.append_system { Some(s) => format!("{s}\n\n{lang}"), None => lang.to_string() };
    c.args(["--append-system-prompt", &sys]);
    provider_env(&state, &mut c).await;
    c.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = c.spawn().map_err(|e| { crate::log::error(format!("run {}: could not start the assistant: {e}", spec.chat_id)); e })?;
    let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        anyhow::bail!("could not connect to the assistant process");
    };
    // Remember the process, so a daemon that dies uncleanly can be cleaned up after.
    let pid = child.id();
    crate::runs::record(pid, &spec.chat_id);
    let started = std::time::Instant::now();
    crate::log::info(format!("run {} started (pid {}, mode {}, in {})", spec.chat_id, pid.unwrap_or(0),
        spec.options.permission_mode.as_deref().unwrap_or("default"), spec.cwd.display()));
    let prompt = spec.prompt.clone();
    tokio::spawn(async move {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    });

    let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
    state.runs.lock().await.insert(spec.chat_id.clone(), kill_tx);
    let id = spec.chat_id.clone();
    state.ack_interrupted(&id).await;
    let stopped = if settings.language == "ar" { "تم الإيقاف." } else { "Stopped." };
    if let Some(ch) = state.chats.update(&id, |c| c.running = true).await {
        state.emit(json!({"type": "chat.updated", "chat": ch.summary()}));
    }

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
                    push(&state, &id, Msg::new("system", stopped)).await;
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
        let exit = child.wait().await;
        crate::runs::forget(pid);
        let stderr_text = err_task.await.unwrap_or_default();
        if !got_result && !stderr_text.trim().is_empty() {
            push(&state, &id, Msg::new("error", stderr_text.trim())).await;
        }
        state.runs.lock().await.remove(&id);
        let secs = started.elapsed().as_secs();
        if ok {
            state.note_run_ok();
            crate::log::info(format!("run {id} finished ok in {secs}s"));
        } else {
            let status = exit.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
            let first: String = stderr_text.lines().next().unwrap_or("").chars().take(300).collect();
            crate::log::warn(format!("run {id} did not finish ({status}, {secs}s{}){}", if got_result { ", assistant reported an error" } else { ", no result" },
                if first.is_empty() { String::new() } else { format!(": {first}") }));
        }
        if let Some(ch) = state.chats.update(&id, |c| { c.running = false; c.updated = chrono::Utc::now(); }).await {
            state.emit(json!({"type": "chat.done", "chatId": id, "ok": ok, "title": ch.title, "chat": ch.summary()}));
        }
        if let Some(f) = on_done { f(state.clone(), ok).await; }
    });
    Ok(())
}

pub async fn push(state: &AppState, chat_id: &str, m: Msg) {
    let mv = serde_json::to_value(&m).unwrap_or(Value::Null);
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
            state.add_usage(cost).await;
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
    crate::settings::check_cli_value("model", model.as_deref())?;
    let mut c = cmd(&bin);
    c.current_dir(crate::paths::home())
        .args(["-p", "--output-format", "json", "--no-session-persistence", "--tools", ""]);
    if let Some(m) = model.filter(|m| !m.is_empty()) { c.args(["--model", &m]); }
    c.args(["--append-system-prompt", language_note(&state.settings.read().await.language)]);
    provider_env(state, &mut c).await;
    c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = c.spawn()?;
    let pid = child.id();
    crate::runs::record(pid, "one-shot");
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("could not connect to the assistant process"))?;
    // The child is killed when this future is dropped (`kill_on_drop`), so a timeout leaves nothing behind.
    let out = tokio::time::timeout(COMPLETE_TIMEOUT, async {
        stdin.write_all(prompt.as_bytes()).await?;
        drop(stdin);
        child.wait_with_output().await
    }).await;
    crate::runs::forget(pid);
    let out = match out {
        Ok(r) => r?,
        Err(_) => {
            crate::log::warn(format!("one-shot completion timed out after {}s", COMPLETE_TIMEOUT.as_secs()));
            anyhow::bail!("the assistant timed out (no answer within {} seconds)", COMPLETE_TIMEOUT.as_secs())
        }
    };
    let v: Value = serde_json::from_slice(&out.stdout)
        .map_err(|_| anyhow::anyhow!("claude: {}", String::from_utf8_lossy(&out.stderr)))?;
    if v["is_error"].as_bool().unwrap_or(false) {
        anyhow::bail!("{}", v["result"].as_str().unwrap_or("error"));
    }
    state.note_run_ok();
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

    #[test]
    fn private_folders_become_deny_rules() {
        let lib = Path::new("/home/u/Library");
        assert_eq!(private_deny_settings(Some(lib), lib, &[]).unwrap(), None);
        let v: Value = serde_json::from_str(&private_deny_settings(Some(lib), lib, &["me/finances".into()]).unwrap().unwrap()).unwrap();
        let deny: Vec<&str> = v["permissions"]["deny"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        for want in ["Read(./me/finances/**)", "Edit(./me/finances/**)", "Read(//home/u/Library/me/finances/**)", "Edit(//home/u/Library/me/finances)"] {
            assert!(deny.contains(&want), "missing {want} in {deny:?}");
        }
        // Running elsewhere (a project, the app layer): only the absolute form applies.
        let v = private_deny_settings(Some(lib), Path::new("/home/u/proj"), &["me".into()]).unwrap().unwrap();
        assert!(v.contains("Read(//home/u/Library/me/**)") && !v.contains("./me"));
        // Best-effort shell rules: the absolute path always, relative shapes only inside the library.
        assert!(v.contains("Bash(*/home/u/Library/me*)") && !v.contains("Bash(* me"));
        let v = private_deny_settings(Some(lib), lib, &["me/finances".into()]).unwrap().unwrap();
        for want in ["Bash(*/home/u/Library/me/finances*)", "Bash(*./me/finances/*)", "Bash(* me/finances/*)", "Bash(* me/finances)"] { assert!(v.contains(want), "missing {want}"); }
        assert!(private_deny_settings(Some(lib), lib, &["../etc".into()]).is_err());
        assert!(private_deny_settings(Some(lib), lib, &["/etc".into()]).is_err());
    }

    #[test]
    fn deny_rules_merge_without_duplicates() {
        let a = Some(json!({"permissions": {"deny": ["Read(./me)", "Edit(./me)"]}}).to_string());
        let b = Some(json!({"permissions": {"deny": ["Read(./me)", "Read(//wt/me)"]}}).to_string());
        let v: Value = serde_json::from_str(&merge_deny(a.clone(), b).unwrap()).unwrap();
        assert_eq!(v["permissions"]["deny"], json!(["Read(./me)", "Edit(./me)", "Read(//wt/me)"]));
        assert_eq!(merge_deny(a.clone(), None), a);
        assert_eq!(merge_deny(None, None), None);
    }

    #[test]
    fn program_names_per_platform() {
        // Unchanged outside Windows: the bare name first.
        assert_eq!(candidates("claude", false, ".COM;.EXE"), ["claude", "claude.exe", "claude.cmd"]);
        // Windows: a real program before the npm shim, then PATHEXT; never the bare shell script.
        assert_eq!(candidates("claude", true, ".COM;.EXE;.BAT;.CMD;;junk"), ["claude.exe", "claude.cmd", "claude.com", "claude.bat"]);
        assert_eq!(candidates("git", true, ""), ["git.exe", "git.cmd"]);
    }

    #[test]
    fn language_note_follows_setting() {
        assert!(language_note("ar").contains("Arabic"));
        assert!(language_note("en").contains("English"));
    }
}
