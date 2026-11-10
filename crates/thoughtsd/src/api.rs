//! HTTP + WebSocket API. Every route requires the bearer token from daemon.json.

use crate::chats::{Chat, Msg, RunOptions};
use crate::claude::{self, RunSpec};
use crate::settings::{Project, Settings};
use crate::state::AppState;
use crate::{applayer, git, library, modules, paths, todos, updates, wraps};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

macro_rules! bail { ($($t:tt)*) => { return Err(anyhow::anyhow!($($t)*).into()) } }

type S = State<Arc<AppState>>;
type R = Result<Json<Value>, ApiErr>;

pub struct ApiErr(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for ApiErr {
    fn from(e: E) -> Self { ApiErr(e.into()) }
}
impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(json!({"error": self.0.to_string()}))).into_response()
    }
}
fn ok() -> R { Ok(Json(json!({"ok": true}))) }

pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/api/ping", get(|| async { Json(json!({"ok": true, "version": env!("CARGO_PKG_VERSION")})) }))
        .route("/api/status", get(status))
        .route("/api/shutdown", post(shutdown))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/onboarding", post(onboarding))
        .route("/api/modules", get(|| async { Json(json!(modules::list())) }))
        .route("/api/modules/:id/enable", post(enable_module))
        .route("/api/ai/claude/options", get(claude_options))
        .route("/api/ai/claude/token", post(save_ai_token))
        .route("/api/ai/claude/login", post(claude_login))
        .route("/api/git/install", post(git_install))
        .route("/api/library/trust", post(trust_library))
        // library
        .route("/api/library/ideas", get(ideas))
        .route("/api/library/tree", get(tree))
        .route("/api/library/file", get(read_file).put(write_file))
        .route("/api/library/search", get(search))
        .route("/api/library/stats", get(stats))
        .route("/api/todos", get(todo_list))
        .route("/api/todos/:op", post(todo_op))
        // chats
        .route("/api/chats", get(chat_list).post(chat_create))
        .route("/api/chats/:id", get(chat_get).delete(chat_delete).patch(chat_patch))
        .route("/api/chats/:id/send", post(chat_send))
        .route("/api/chats/:id/stop", post(chat_stop))
        // wrap & reorganize
        .route("/api/wrap", get(wrap_status).post(wrap_start))
        .route("/api/wrap/:id/apply", post(wrap_apply))
        .route("/api/reorganize", post(reorg_start))
        .route("/api/reorganize/:id", get(reorg_diff))
        .route("/api/reorganize/:id/:op", post(reorg_finish))
        // git
        .route("/api/git", get(git_status))
        .route("/api/git/sync", post(git_sync))
        .route("/api/git/remotes", post(remote_add))
        .route("/api/git/remotes/:name", delete(remote_remove))
        .route("/api/git/credentials", post(save_cred))
        // projects
        .route("/api/projects", get(project_list).post(project_add))
        .route("/api/projects/:id", delete(project_remove))
        // app layer & updates & extensions
        .route("/api/layer", get(|| async { Json(applayer::manifest().await) }))
        .route("/api/layer/history", get(layer_history))
        .route("/api/layer/rollback", post(layer_rollback))
        .route("/api/layer/customize", post(customize))
        .route("/api/updates", get(|| async { Json(json!(updates::list())) }))
        .route("/api/updates/apply", post(update_apply))
        .route("/api/extensions", get(extensions))
        .route("/api/extensions/search/:op", post(search_ext))
        .route("/api/events", get(events))
        .layer(middleware::from_fn_with_state(state.clone(), auth))
        // Outermost: answer CORS preflights before auth. The bearer token is the real guard.
        .layer(tower_http::cors::CorsLayer::permissive());
    api.with_state(state)
}

async fn auth(State(st): S, req: Request, next: Next) -> Response {
    let header = req.headers().get("authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
    let q = req.uri().query().unwrap_or("");
    let token_ok = header == format!("Bearer {}", st.token)
        || q.split('&').any(|kv| kv == format!("token={}", st.token));
    if !token_ok { return (StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    next.run(req).await
}

async fn lib(st: &AppState) -> anyhow::Result<std::path::PathBuf> {
    st.settings.read().await.library().ok_or_else(|| anyhow::anyhow!("no library configured"))
}

// ---------- status & settings ----------

async fn status(State(st): S) -> R {
    let s = st.settings.read().await.clone();
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"), "onboarded": s.onboarded,
        "library": s.library_path, "claude": claude::status(&st).await,
        "git": git_tool_status().await,
        "platform": std::env::consts::OS,
        "home": dirs::home_dir(),
    })))
}

/// Is a working git available? On macOS /usr/bin/git is a stub until the command-line tools
/// are installed, so check xcode-select first (running the stub would pop a dialog).
async fn git_tool_status() -> Value {
    use tokio::process::Command;
    #[cfg(target_os = "macos")]
    {
        let clt = Command::new("xcode-select").arg("-p").output().await.map(|o| o.status.success()).unwrap_or(false);
        let brew = ["/opt/homebrew/bin/git", "/usr/local/bin/git"].iter().find(|p| std::path::Path::new(p).exists());
        if !clt && brew.is_none() { return json!({"installed": false}); }
    }
    match Command::new("git").arg("--version").env("PATH", claude::shell_path()).output().await {
        Ok(o) if o.status.success() => json!({"installed": true,
            "version": String::from_utf8_lossy(&o.stdout).trim().trim_start_matches("git version ").to_string()}),
        _ => json!({"installed": false}),
    }
}

#[derive(Deserialize)]
struct TrustReq { path: String }
/// Record the user's explicit decision to trust a library folder that contains runnable settings.
async fn trust_library(Json(t): Json<TrustReq>) -> R {
    crate::trust::trust(std::path::Path::new(&t.path))?;
    ok()
}

/// Starts the OS installer for git (Apple's command-line tools on macOS).
async fn git_install() -> R {
    #[cfg(target_os = "macos")]
    { tokio::process::Command::new("xcode-select").arg("--install").spawn()?; }
    ok()
}

async fn shutdown(State(st): S) -> R {
    let n = st.shutdown.clone();
    tokio::spawn(async move { tokio::time::sleep(std::time::Duration::from_millis(200)).await; n.notify_waiters(); });
    ok()
}

async fn get_settings(State(st): S) -> R { Ok(Json(serde_json::to_value(&*st.settings.read().await)?)) }

fn cloud_folder(p: &str) -> bool {
    ["Mobile Documents", "Library/CloudStorage", "Dropbox", "OneDrive", "Google Drive", "iCloud"].iter().any(|k| p.contains(k))
}

fn validate(s: &Settings) -> anyhow::Result<()> {
    if let Some(p) = &s.library_path {
        if s.git_enabled() && cloud_folder(p) {
            bail!("This library is inside a cloud-synced folder. Git and iCloud/Dropbox on the same folder corrupt repositories — choose \"Folder sync\" or move the library.");
        }
    }
    Ok(())
}

async fn put_settings(State(st): S, Json(mut new): Json<Settings>) -> R {
    validate(&new)?;
    let old = st.settings.read().await.clone();
    if new.library_path != old.library_path {
        if let Some(p) = &new.library_path { crate::trust::require(std::path::Path::new(p))?; }
    }
    // Server-owned fields the UI may not overwrite.
    new.last_wrap = old.last_wrap;
    new.ai.claude.has_api_token = old.ai.claude.has_api_token;
    if new.daemon.run_at_login != old.daemon.run_at_login { login_item(new.daemon.run_at_login)?; }
    new.save()?;
    *st.settings.write().await = new.clone();
    st.emit(json!({"type": "settings.changed"}));
    Ok(Json(serde_json::to_value(new)?))
}

/// Start the daemon at login (macOS LaunchAgent; other platforms in milestone 2).
fn login_item(enable: bool) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let plist = dirs::home_dir().unwrap().join("Library/LaunchAgents/app.thoughts.daemon.plist");
        if enable {
            let exe = std::env::current_exe()?;
            std::fs::create_dir_all(plist.parent().unwrap())?;
            std::fs::write(&plist, format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>app.thoughts.daemon</string>
<key>ProgramArguments</key><array><string>{}</string><string>--daemon</string></array>
<key>RunAtLoad</key><true/>
</dict></plist>
"#, exe.display()))?;
        } else {
            let _ = std::fs::remove_file(plist);
        }
    }
    let _ = enable;
    Ok(())
}

// ---------- onboarding ----------

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Onboarding {
    settings: Settings,
    library_mode: String, // "new" | "existing"
    seeds: Vec<String>,
    remote_url: String,
    remote_username: String,
    remote_token: String,
    api_token: String,
}

async fn onboarding(State(st): S, Json(o): Json<Onboarding>) -> R {
    let mut s = o.settings;
    let path = s.library_path.clone().filter(|p| !p.trim().is_empty()).ok_or_else(|| anyhow::anyhow!("choose a library folder"))?;
    let path = if let Some(rest) = path.strip_prefix("~/") { dirs::home_dir().unwrap().join(rest) } else { path.into() };
    s.library_path = Some(path.to_string_lossy().to_string());
    validate(&s)?;
    let fresh = o.library_mode != "existing";
    if fresh {
        library::create_from_starter(&path)?;
        // Trim rituals for modules that were not chosen (the AI re-adds them on demand).
        let on = |m: &str| s.modules.get(m).copied().unwrap_or(false);
        if !on("me") && !on("checkin") {
            let _ = std::fs::remove_dir_all(path.join(".claude/skills/check-in"));
            let _ = std::fs::remove_dir_all(path.join(".claude/skills/interview-me"));
        }
    } else if !path.join("CLAUDE.md").exists() && !path.join("INDEX.md").exists() {
        bail!("{} doesn't look like a Thoughts library (no CLAUDE.md / INDEX.md)", path.display());
    } else {
        crate::trust::require(&path)?;
    }
    if s.git_enabled() {
        git::init(&st, &path).await?;
        if !o.remote_url.trim().is_empty() {
            let _ = git::git(&st, &path, &["remote", "add", "origin", o.remote_url.trim()]).await;
            if !o.remote_token.is_empty() {
                if let Some(h) = git::host_of(&o.remote_url) { git::save_credential(&st, &h, &o.remote_username, &o.remote_token).await?; }
            }
        }
    }
    if !o.api_token.is_empty() {
        keyring::Entry::new("thoughts-ai", "claude")?.set_password(&o.api_token)?;
        s.ai.claude.has_api_token = true;
    }
    if s.daemon.run_at_login { login_item(true)?; }
    s.onboarded = true;
    s.save()?;
    *st.settings.write().await = s.clone();
    applayer::ensure(&st).await?;

    if !fresh {
        return Ok(Json(json!({"ok": true, "setupChatId": null})));
    }
    let on = |m: &str| s.modules.get(m).copied().unwrap_or(false);
    let mut uses = vec!["Project & business ideas"];
    if on("checkin") { uses.push("Daily briefs / journaling"); }
    if on("todos") { uses.push("Todo lists & tasks"); }
    if on("me") { uses.push("Personal profile"); }
    let mut extra = String::new();
    for m in ["docs", "learning"] {
        if on(m) { extra.push_str(&format!("\n- Also add the {} module: {}", m, modules::get(m).unwrap().add_prompt)); }
    }
    let prompt = format!(
        "Initialize this library by following .claude/skills/init-library/SKILL.md. The user already answered the interview \
         in the Thoughts app — do NOT ask questions, proceed with these answers:\n\
         - Uses: {}\n- Name: {}\n- On their mind: {}\n- Sync: handled by the app\n\
         Do Steps 2 and 5 only. Skip Step 3 (git) and Step 4 (self-destruct + commit): the app deletes init-library and \
         START-HERE.md and makes the commit afterwards. If personal profile was not chosen, me/README.md is still created per the skill.\
         {extra}\nAlso add one line to CLAUDE.md noting the library is used through the Thoughts app (its Wrap button runs the wrap ritual over all unwrapped chats).",
        uses.join(", "), if s.user_name.is_empty() { "friend" } else { &s.user_name },
        if o.seeds.is_empty() { "(nothing yet)".into() } else { o.seeds.join("; ") }
    );
    let mut chat = Chat { title: "Setting up your library".into(), kind: "setup".into(), ..Default::default() };
    chat.messages.push(Msg::new("user", "Set up my library."));
    let id = chat.id.clone();
    st.chats.insert(chat).await;
    let spec = RunSpec { chat_id: id.clone(), prompt, cwd: path.clone(), add_dirs: vec![],
        options: RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() }, resume: None, append_system: None };
    let name = s.user_name.clone();
    let git_on = s.git_enabled();
    claude::start_run(st.clone(), spec, Some(Box::new(move |st: Arc<AppState>, _ok| Box::pin(async move {
        let _ = std::fs::remove_dir_all(path.join(".claude/skills/init-library"));
        let _ = std::fs::remove_file(path.join("START-HERE.md"));
        if git_on {
            let _ = git::commit_all(&st, &path, &format!("init: library configured for {}", if name.is_empty() { "me" } else { &name })).await;
            if !git::remotes(&st, &path).await.is_empty() { let _ = git::sync(&st, &path).await; }
        }
        st.emit(json!({"type": "library.changed"}));
    })))).await?;
    Ok(Json(json!({"ok": true, "setupChatId": id})))
}

async fn enable_module(State(st): S, Path(id): Path<String>) -> R {
    let m = modules::get(&id).ok_or_else(|| anyhow::anyhow!("unknown module"))?;
    { let mut s = st.settings.write().await; s.modules.insert(id.clone(), true); s.save()?; }
    if m.add_prompt.is_empty() { return Ok(Json(json!({"ok": true, "chatId": null}))); }
    let path = lib(&st).await?;
    let mut chat = Chat { title: format!("Add module: {}", m.name), kind: "module".into(), ..Default::default() };
    chat.messages.push(Msg::new("user", format!("Add the {} module.", m.name)));
    let cid = chat.id.clone();
    st.chats.insert(chat).await;
    claude::start_run(st.clone(), RunSpec { chat_id: cid.clone(), prompt: modules::add_prompt(m), cwd: path.clone(), add_dirs: vec![],
        options: RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() }, resume: None, append_system: None },
        Some(Box::new(move |st: Arc<AppState>, ok| Box::pin(async move {
            if ok && st.settings.read().await.git_enabled() {
                let _ = git::commit_all(&st, &path, &format!("module: {}", m.id)).await;
            }
            st.emit(json!({"type": "library.changed"}));
        })))).await?;
    Ok(Json(json!({"ok": true, "chatId": cid})))
}

async fn claude_options(State(st): S) -> R { Ok(Json(claude::options(&st).await)) }

#[derive(Deserialize)]
struct Token { token: String }
async fn save_ai_token(State(st): S, Json(t): Json<Token>) -> R {
    let e = keyring::Entry::new("thoughts-ai", "claude")?;
    let mut s = st.settings.write().await;
    if t.token.is_empty() { let _ = e.delete_credential(); s.ai.claude.has_api_token = false; }
    else { e.set_password(&t.token)?; s.ai.claude.has_api_token = true; }
    s.save()?;
    ok()
}

/// Opens Claude Code's browser login flow.
async fn claude_login(State(st): S) -> R {
    let bin = claude::binary(&st).await.ok_or_else(|| anyhow::anyhow!("Claude Code CLI not found"))?;
    let mut c = tokio::process::Command::new(bin);
    c.args(["auth", "login"]).env("PATH", claude::shell_path())
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut child = c.spawn()?;
    tokio::spawn(async move {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(600), child.wait()).await;
        st.emit(json!({"type": "auth.changed"}));
    });
    ok()
}

// ---------- library ----------

async fn ideas(State(st): S) -> R { Ok(Json(json!(library::ideas(&lib(&st).await?)))) }
async fn tree(State(st): S) -> R { Ok(Json(json!(library::tree(&lib(&st).await?)))) }
async fn stats(State(st): S) -> R { Ok(Json(library::stats(&lib(&st).await?))) }

async fn read_file(State(st): S, Query(q): Query<HashMap<String, String>>) -> R {
    Ok(Json(library::read(&lib(&st).await?, q.get("path").map(String::as_str).unwrap_or(""))?))
}

#[derive(Deserialize)]
struct WriteFile { path: String, content: String }
async fn write_file(State(st): S, Json(w): Json<WriteFile>) -> R {
    library::write(&lib(&st).await?, &w.path, &w.content)?;
    st.emit(json!({"type": "library.changed"}));
    ok()
}

async fn search(State(st): S, Query(q): Query<HashMap<String, String>>) -> R {
    Ok(Json(json!(library::search(&lib(&st).await?, q.get("q").map(String::as_str).unwrap_or(""), 60))))
}

async fn todo_list(State(st): S) -> R { Ok(Json(json!(todos::list(&lib(&st).await?)))) }

#[derive(Deserialize, Default)]
#[serde(default)]
struct TodoOp { file: String, line: usize, text: String, new_text: String }
async fn todo_op(State(st): S, Path(op): Path<String>, Json(t): Json<TodoOp>) -> R {
    let root = lib(&st).await?;
    let file = if t.file.is_empty() { todos::files(&root).into_iter().next().unwrap_or_else(|| "TODO.md".into()) } else { t.file.clone() };
    if !(file == "TODO.md" || file.ends_with("/TODO.md") || file.ends_with("/todo.md")) { bail!("not a todo file"); }
    match op.as_str() {
        "toggle" => todos::toggle(&root, &file, t.line, &t.text)?,
        "add" => todos::add(&root, &file, &t.text)?,
        "edit" => todos::edit(&root, &file, t.line, &t.text, &t.new_text)?,
        "delete" => todos::delete(&root, &file, t.line, &t.text)?,
        _ => bail!("unknown op"),
    }
    st.emit(json!({"type": "todos.changed"}));
    ok()
}

// ---------- chats ----------

async fn chat_list(State(st): S) -> R {
    Ok(Json(json!(st.chats.list().await.iter().map(|c| c.summary()).collect::<Vec<_>>())))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct NewChat { title: Option<String>, project_id: Option<String>, kind: Option<String>, provider: Option<String> }
async fn chat_create(State(st): S, Json(n): Json<NewChat>) -> R {
    let s = st.settings.read().await.clone();
    let mut c = Chat::default();
    if let Some(t) = n.title { c.title = t; }
    c.project_id = n.project_id;
    if let Some(k) = n.kind.filter(|k| k == "chat" || k == "customize") { c.kind = k; }
    c.provider = n.provider.unwrap_or(s.ai.primary.clone());
    c.options = RunOptions { model: s.ai.claude.model.clone(), effort: s.ai.claude.effort.clone(), permission_mode: s.ai.claude.permission_mode.clone() };
    if c.kind == "customize" { c.title = "Customize the app".into(); }
    let id = c.id.clone();
    let v = serde_json::to_value(&c)?;
    st.chats.insert(c).await;
    wraps::on_new_chat(st.clone(), id);
    Ok(Json(v))
}

async fn chat_get(State(st): S, Path(id): Path<String>) -> R {
    Ok(Json(serde_json::to_value(st.chats.get(&id).await.ok_or_else(|| anyhow::anyhow!("no such chat"))?)?))
}

async fn chat_delete(State(st): S, Path(id): Path<String>) -> R {
    if let Some(k) = st.runs.lock().await.remove(&id) { let _ = k.send(()); }
    st.chats.remove(&id).await;
    ok()
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ChatPatch { title: Option<String>, project_id: Option<Option<String>>, options: Option<RunOptions> }
async fn chat_patch(State(st): S, Path(id): Path<String>, Json(p): Json<ChatPatch>) -> R {
    let c = st.chats.update(&id, |c| {
        if let Some(t) = p.title { c.title = t; }
        if let Some(pid) = p.project_id { c.project_id = pid; }
        if let Some(o) = p.options { c.options = o; }
    }).await.ok_or_else(|| anyhow::anyhow!("no such chat"))?;
    st.emit(json!({"type": "chat.updated", "chat": c.summary()}));
    Ok(Json(c.summary()))
}

#[derive(Deserialize)]
struct Attachment { name: String, data: String }
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SendBody { text: String, attachments: Vec<Attachment>, mentions: Vec<String>, options: Option<RunOptions> }

async fn chat_send(State(st): S, Path(id): Path<String>, Json(m): Json<SendBody>) -> R {
    let chat = st.chats.get(&id).await.ok_or_else(|| anyhow::anyhow!("no such chat"))?;
    if chat.running { bail!("this chat is still running"); }
    if let Some(lib) = st.settings.read().await.library() { crate::trust::require(&lib)?; }
    if chat.provider != "claude" { bail!("Codex support arrives in the next milestone"); }
    let s = st.settings.read().await.clone();
    let library = s.library();
    // Where the session runs: a project folder, the app layer, or the library.
    let (cwd, add_dirs) = match (&chat.kind[..], &chat.project_id) {
        ("customize", _) => (paths::app_layer_dir(), library.clone().into_iter().collect()),
        (_, Some(pid)) => {
            let p = s.projects.iter().find(|p| &p.id == pid).ok_or_else(|| anyhow::anyhow!("project not found"))?;
            (std::path::PathBuf::from(&p.path), library.clone().into_iter().collect())
        }
        _ => (library.clone().ok_or_else(|| anyhow::anyhow!("no library configured"))?, vec![]),
    };
    let mut prompt = m.text.clone();
    let mut saved = vec![];
    if !m.attachments.is_empty() {
        let dir = paths::attachments_dir().join(&id);
        std::fs::create_dir_all(&dir)?;
        for a in &m.attachments {
            let name = std::path::Path::new(&a.name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or("file".into());
            let p = dir.join(&name);
            std::fs::write(&p, base64::engine::general_purpose::STANDARD.decode(a.data.split(',').last().unwrap_or(""))?)?;
            saved.push(p.to_string_lossy().to_string());
        }
        prompt.push_str("\n\nAttached files (read them):\n");
        for p in &saved { prompt.push_str(&format!("- {p}\n")); }
    }
    if !m.mentions.is_empty() {
        prompt.push_str("\n\nReferenced library files (read them):\n");
        for r in &m.mentions {
            let abs = library.as_ref().map(|l| l.join(r).to_string_lossy().to_string()).unwrap_or(r.clone());
            prompt.push_str(&format!("- {abs}\n"));
        }
    }
    // Handoff from another provider / a chat without a session: replay history.
    if chat.session_id.is_none() && chat.messages.iter().any(|x| x.role == "assistant") {
        prompt = format!("Earlier conversation (continue from it):\n\n{}\n\n---\n\n{prompt}", chat.transcript(120_000));
    }
    let first = !chat.messages.iter().any(|x| x.role == "user");
    let mut um = Msg::new("user", &m.text);
    um.attachments = saved;
    um.tool = (!m.mentions.is_empty()).then(|| json!({"mentions": m.mentions}));
    let opts = m.options.unwrap_or(chat.options.clone());
    let updated = st.chats.update(&id, |c| {
        c.messages.push(um.clone());
        c.dirty = true;
        c.mini_wrap = None; // engaged again: the mini-wrap is stale
        c.updated = chrono::Utc::now();
        c.options = opts.clone();
        if first && c.kind == "chat" { c.title = m.text.lines().next().unwrap_or("Chat").chars().take(60).collect(); }
    }).await.unwrap();
    st.emit(json!({"type": "chat.message", "chatId": id, "message": um}));
    st.emit(json!({"type": "chat.updated", "chat": updated.summary()}));
    let append = Some("You are running inside the Thoughts desktop app's chat. The user cannot answer interactive tool prompts or \
        AskUserQuestion dialogs here — ask questions in plain text. Wraps are run by the app's Wrap button; don't wrap unless asked.".to_string());
    let kind = chat.kind.clone();
    let on_done: Option<Box<dyn FnOnce(Arc<AppState>, bool) -> futures::future::BoxFuture<'static, ()> + Send>> = if kind == "customize" {
        let summary: String = m.text.lines().next().unwrap_or("change").chars().take(60).collect();
        Some(Box::new(move |st: Arc<AppState>, _ok| Box::pin(async move {
            let _ = git::commit_all(&st, &paths::app_layer_dir(), &format!("customize: {summary}")).await;
            st.emit(json!({"type": "layer.changed"}));
        })))
    } else { None };
    claude::start_run(st.clone(), RunSpec { chat_id: id, prompt, cwd, add_dirs, options: opts, resume: chat.session_id.clone(), append_system: append }, on_done).await?;
    ok()
}

async fn chat_stop(State(st): S, Path(id): Path<String>) -> R {
    if let Some(k) = st.runs.lock().await.remove(&id) { let _ = k.send(()); }
    ok()
}

// ---------- wrap & reorganize ----------

async fn wrap_status(State(st): S) -> R { Ok(Json(wraps::status(&st).await)) }
async fn wrap_start(State(st): S) -> R { Ok(Json(json!({"chatId": wraps::start(st.clone()).await?}))) }

#[derive(Deserialize, Default)]
#[serde(default)]
struct Note { note: String, instructions: String }
async fn wrap_apply(State(st): S, Path(id): Path<String>, Json(n): Json<Note>) -> R { wraps::apply(st.clone(), &id, &n.note).await?; ok() }
async fn reorg_start(State(st): S, Json(n): Json<Note>) -> R { Ok(Json(json!({"chatId": wraps::reorganize_start(st.clone(), &n.instructions).await?}))) }
async fn reorg_diff(State(st): S, Path(id): Path<String>) -> R { Ok(Json(wraps::reorganize_diff(&st, &id).await?)) }
async fn reorg_finish(State(st): S, Path((id, op)): Path<(String, String)>) -> R {
    wraps::reorganize_finish(&st, &id, op == "merge").await?;
    st.emit(json!({"type": "library.changed"}));
    ok()
}

// ---------- git ----------

async fn git_status(State(st): S) -> R {
    let s = st.settings.read().await.clone();
    let Some(l) = s.library() else { return Ok(Json(json!({"repo": false}))) };
    let mut v = git::status(&st, &l).await;
    v["mode"] = json!(s.sync.mode);
    Ok(Json(v))
}
async fn git_sync(State(st): S) -> R {
    let l = lib(&st).await?;
    if !st.settings.read().await.git_enabled() { bail!("git sync is off for this library"); }
    let v = git::sync(&st, &l).await?;
    st.emit(json!({"type": "library.changed"}));
    Ok(Json(v))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Remote { name: String, url: String, username: String, token: String }
async fn remote_add(State(st): S, Json(r): Json<Remote>) -> R {
    let l = lib(&st).await?;
    git::init(&st, &l).await?;
    let name = if r.name.trim().is_empty() { "origin".to_string() } else { r.name.trim().to_string() };
    git::git(&st, &l, &["remote", "add", &name, r.url.trim()]).await?;
    if !r.token.is_empty() {
        if let Some(h) = git::host_of(&r.url) { git::save_credential(&st, &h, &r.username, &r.token).await?; }
    }
    ok()
}
async fn remote_remove(State(st): S, Path(name): Path<String>) -> R {
    git::git(&st, &lib(&st).await?, &["remote", "remove", &name]).await?;
    ok()
}
#[derive(Deserialize)]
struct Cred { host: String, username: String, token: String }
async fn save_cred(State(st): S, Json(c): Json<Cred>) -> R { git::save_credential(&st, &c.host, &c.username, &c.token).await?; ok() }

// ---------- projects ----------

async fn project_list(State(st): S) -> R { Ok(Json(json!(st.settings.read().await.projects))) }

#[derive(Deserialize, Default)]
#[serde(default)]
struct NewProject { name: String, path: String, inside_library: bool }
async fn project_add(State(st): S, Json(p): Json<NewProject>) -> R {
    let mut s = st.settings.write().await;
    let path = if p.inside_library {
        let slug: String = p.name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
        let dir = s.library().ok_or_else(|| anyhow::anyhow!("no library"))?.join("projects").join(slug.trim_matches('-'));
        std::fs::create_dir_all(&dir)?;
        dir
    } else {
        let raw = p.path.trim();
        let pb = if let Some(rest) = raw.strip_prefix("~/") { dirs::home_dir().unwrap().join(rest) } else { raw.into() };
        if !pb.is_dir() { bail!("folder not found: {}", pb.display()); }
        pb
    };
    let proj = Project { id: uuid::Uuid::new_v4().to_string(), name: if p.name.is_empty() { path.file_name().unwrap().to_string_lossy().into() } else { p.name }, path: path.to_string_lossy().into() };
    s.projects.push(proj.clone());
    s.save()?;
    Ok(Json(serde_json::to_value(proj)?))
}
async fn project_remove(State(st): S, Path(id): Path<String>) -> R {
    let mut s = st.settings.write().await;
    s.projects.retain(|p| p.id != id);
    s.save()?;
    ok()
}

// ---------- app layer, updates, extensions ----------

async fn layer_history(State(st): S) -> R { Ok(Json(json!(applayer::history(&st).await))) }

#[derive(Deserialize)]
struct Commit { hash: String }
async fn layer_rollback(State(st): S, Json(c): Json<Commit>) -> R { applayer::rollback(&st, &c.hash).await?; ok() }

#[derive(Deserialize, Default)]
#[serde(default)]
struct Customize { text: String }
async fn customize(State(st): S, Json(c): Json<Customize>) -> R {
    let mut chat = Chat { kind: "customize".into(), title: "Customize the app".into(), ..Default::default() };
    chat.options.permission_mode = Some("acceptEdits".into());
    let id = chat.id.clone();
    st.chats.insert(chat).await;
    if !c.text.is_empty() {
        let _ = chat_send(State(st.clone()), Path(id.clone()), Json(SendBody { text: c.text, ..Default::default() })).await?;
    }
    Ok(Json(json!({"chatId": id})))
}

#[derive(Deserialize)]
struct ApplyUpdate { pack: String, items: Vec<String> }
async fn update_apply(State(st): S, Json(u): Json<ApplyUpdate>) -> R {
    let (stage, prompt) = updates::prepare(&u.pack, &u.items)?;
    let mut chat = Chat { kind: "customize".into(), title: format!("Update {}", u.pack), ..Default::default() };
    chat.options.permission_mode = Some("acceptEdits".into());
    let id = chat.id.clone();
    st.chats.insert(chat).await;
    let _ = stage;
    let _ = chat_send(State(st.clone()), Path(id.clone()), Json(SendBody { text: prompt, ..Default::default() })).await?;
    Ok(Json(json!({"chatId": id})))
}

async fn extensions(State(st): S) -> R {
    let s = st.settings.read().await.clone();
    let stats = s.library().map(|l| library::stats(&l)).unwrap_or(json!({}));
    let files = stats["markdownFiles"].as_u64().unwrap_or(0) as usize;
    Ok(Json(json!({
        "search": {
            "installed": paths::extensions_dir().join("search/.git").exists(),
            "repo": s.search.extension_repo, "threshold": s.search.file_threshold,
            "files": files, "suggest": files >= s.search.file_threshold && !s.search.prompt_dismissed,
        }
    })))
}

async fn search_ext(State(st): S, Path(op): Path<String>) -> R {
    match op.as_str() {
        "install" => {
            let repo = st.settings.read().await.search.extension_repo.clone();
            let dest = paths::extensions_dir().join("search");
            if !dest.exists() {
                git::git(&st, &paths::extensions_dir(), &["clone", "--depth", "1", &repo, "search"]).await?;
            }
            ok()
        }
        "dismiss" => {
            let mut s = st.settings.write().await;
            s.search.prompt_dismissed = true;
            s.save()?;
            ok()
        }
        _ => Err(anyhow::anyhow!("unknown op").into()),
    }
}

// ---------- events ----------

async fn events(State(st): S, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| pump(st, socket))
}

async fn pump(st: Arc<AppState>, mut socket: WebSocket) {
    let mut rx = st.events.subscribe();
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(v) => if socket.send(Message::Text(v.to_string())).await.is_err() { break },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let _ = socket.send(Message::Text(json!({"type": "resync"}).to_string())).await;
                }
                Err(_) => break,
            },
            msg = socket.recv() => match msg { Some(Ok(_)) => {}, _ => break },
        }
    }
}

