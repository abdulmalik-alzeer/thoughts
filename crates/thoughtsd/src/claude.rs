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
    let dirs: Vec<&Path> = shell_path().split(sep).map(Path::new).collect();
    search(&dirs, &cands, cfg!(windows), |p| p.is_file())
}

/// Look for the first of `cands` in `dirs`. Normally the first folder that has any of them wins.
/// On Windows a real program (`.exe`) anywhere on the path is preferred over a `.cmd` shim in an
/// earlier folder: the shim goes through cmd.exe, which mangles long and multi-line arguments.
pub fn search(dirs: &[&Path], cands: &[String], windows: bool, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    if windows {
        for cand in cands.iter().filter(|c| c.ends_with(".exe")) {
            for dir in dirs { let p = dir.join(cand); if exists(&p) { return Some(p); } }
        }
    }
    for dir in dirs {
        for cand in cands {
            let p = dir.join(cand);
            if exists(&p) { return Some(p); }
        }
    }
    None
}

/// Is this program a Windows batch shim (`claude.cmd` from npm)? Those cannot take multi-line or
/// quote-heavy arguments, so such values are handed over in files instead.
pub fn is_cmd_shim(bin: &Path) -> bool {
    bin.extension().and_then(|e| e.to_str()).map_or(false, |e| matches!(e.to_lowercase().as_str(), "cmd" | "bat"))
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
    // Its own process group, so stopping a run also ends whatever the assistant started (see `proc::kill_tree`).
    crate::proc::own_group(&mut c);
    c
}

/// The assistant program's version line, or empty when it is not installed.
pub async fn version(state: &AppState) -> String {
    let Some(bin) = binary(state).await else { return String::new() };
    let mut c = cmd(&bin);
    c.arg("--version");
    crate::proc::output_timeout(&mut c, std::time::Duration::from_secs(20)).await.ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// `{installed, version, loggedIn, auth}`
pub async fn status(state: &AppState) -> Value {
    let Some(bin) = binary(state).await else {
        return json!({"installed": false});
    };
    let version = version(state).await;
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

/// What a stand-in for the assistant program is told about the run it replaces.
#[derive(Clone, Debug)]
pub struct RunInfo {
    pub chat_id: String,
    pub cwd: PathBuf,
    pub prompt: String,
    /// The permission mode the program would have been started with.
    pub permission_mode: Option<String>,
    /// Every deny rule the run would have been started with.
    pub deny: Vec<String>,
}

/// Stands in for the assistant program: does the "work" of a run and says whether it succeeded.
/// Set on `AppState::run_hook` by tests, so the wrap / chat machinery can be exercised without the CLI.
pub type RunHook = Arc<dyn Fn(RunInfo) -> futures::future::BoxFuture<'static, bool> + Send + Sync>;

/// An absolute path as a Claude Code permission pattern (`//path`, `//c/path` on Windows).
pub fn rule_path(p: &Path) -> String {
    let abs = p.to_string_lossy().replace('\\', "/");
    match abs.strip_prefix('/') {
        Some(rest) => format!("//{rest}"),
        None => format!("//{}", abs.replacen(':', "", 1)),
    }
}

/// Deny rules every assistant run gets, whatever started it (a chat, a wrap, a workflow, a
/// reorganize, a feature being added):
/// - no editing of what could make this machine run programs later — `.claude/`, `.mcp.json` and
///   `.git/` in the folder it works in and in every added folder (reading stays allowed: routines
///   live in `.claude/skills`). `allow_claude_edit` lifts the `.claude/` part for the one run that
///   sets a brand-new library up;
/// - no reading or editing of the app's own data folder (`home`): sign-in details, settings, other
///   chats and their attachments. This chat's own attachments stay readable, and so do the
///   customisation layer and the search add-on.
pub fn protective_deny(cwd: &Path, add_dirs: &[PathBuf], home: &Path, chat_id: &str, allow_claude_edit: bool) -> Vec<String> {
    let mut deny: Vec<String> = vec![];
    let mut names = vec![".mcp.json", ".git"];
    if !allow_claude_edit { names.insert(0, ".claude"); }
    let add = |prefix: &str, deny: &mut Vec<String>| {
        for name in &names {
            deny.push(format!("Edit({prefix}/{name})"));
            deny.push(format!("Edit({prefix}/{name}/**)"));
            // The same names further down (a `.claude` folder inside a sub-folder is read too).
            if *name != ".git" { deny.push(format!("Edit({prefix}/**/{name})")); deny.push(format!("Edit({prefix}/**/{name}/**)")); }
        }
    };
    add(".", &mut deny);
    for dir in std::iter::once(cwd).chain(add_dirs.iter().map(PathBuf::as_path)) { add(&rule_path(dir), &mut deny); }

    let working_in = |p: &Path| std::iter::once(cwd).chain(add_dirs.iter().map(PathBuf::as_path)).any(|d| d.starts_with(p));
    let (files, dirs) = crate::paths::secret_paths_in(home);
    for f in &files {
        let p = rule_path(f);
        for rule in [format!("Read({p})"), format!("Read({p}*)"), format!("Edit({p})"), format!("Edit({p}*)")] { deny.push(rule); }
        // Best-effort for the shell (Read/Edit rules do not cover `cat`).
        deny.push(format!("Bash(*{}*)", f.to_string_lossy().replace('\\', "/")));
    }
    let closed = |dir: &Path, deny: &mut Vec<String>| {
        let p = rule_path(dir);
        for rule in [format!("Read({p})"), format!("Read({p}/**)"), format!("Edit({p})"), format!("Edit({p}/**)")] { deny.push(rule); }
        deny.push(format!("Bash(*{}*)", dir.to_string_lossy().replace('\\', "/")));
    };
    for dir in &dirs {
        // The folder the run itself works in (the reorganize working copy) is of course open to it.
        if working_in(dir) { continue; }
        match dir.file_name().and_then(|n| n.to_str()) {
            // Files the user attached to *this* chat must stay readable; everyone else's are closed.
            Some("attachments") => {
                let p = rule_path(dir);
                deny.push(format!("Edit({p})"));
                deny.push(format!("Edit({p}/**)"));
                for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                    if e.file_name().to_string_lossy() != chat_id { closed(&e.path(), &mut deny); }
                }
            }
            // The search add-on is a tool the assistant may use, but not rewrite.
            Some("extensions") => {
                let p = rule_path(dir);
                deny.push(format!("Edit({p})"));
                deny.push(format!("Edit({p}/**)"));
            }
            _ => closed(dir, &mut deny),
        }
    }
    let mut seen = std::collections::HashSet::new();
    deny.retain(|r| seen.insert(r.clone()));
    deny
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
    // Permission rules compare letter by letter, but macOS and Windows volumes do not: the same
    // folder answers to `me/finances`, `Me/Finances`… Rules are written for the name as configured,
    // as it is really spelled on disk, and in lower case. (Other mixed spellings are not covered.)
    let mut spellings: Vec<String> = vec![];
    for raw in private {
        let rel = crate::paths::clean_rel(raw)?;
        let mut forms = vec![rel.clone(), rel.to_lowercase()];
        if let Some(lib) = library {
            if let (Ok(real_lib), Ok(real)) = (crate::paths::canonical(lib), crate::paths::canonical(&lib.join(&rel))) {
                if let Ok(on_disk) = real.strip_prefix(&real_lib) {
                    let on_disk = on_disk.to_string_lossy().replace('\\', "/");
                    if !on_disk.is_empty() && crate::paths::clean_rel(&on_disk).is_ok() { forms.push(on_disk); }
                }
            }
        }
        for f in forms { if !spellings.contains(&f) { spellings.push(f); } }
    }
    for rel in spellings {
        let mut pats = vec![];
        if let Some(lib) = library {
            if lib == cwd { pats.push(format!("./{rel}")); }
            // Absolute paths in permission rules are written with a leading `//` (`//c/…` on Windows).
            pats.push(rule_path(&lib.join(&rel)));
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

type OnDone = Box<dyn FnOnce(Arc<AppState>, bool) -> futures::future::BoxFuture<'static, ()> + Send>;

/// Record what is in the library as one history entry, when history is on and nothing forbids it
/// (an interrupted wrap must keep its undo point; git settings must still be the trusted ones).
/// `wait` = wait for other git work to finish; otherwise skip when git is busy.
pub(crate) async fn record_library(state: &AppState, lib: &Path, subject: &str, wait: bool) {
    if !state.settings.read().await.git_enabled() || state.library_busy() { return; }
    if crate::wraps::interrupted_wrap(state).await.is_some() { return; }
    let _g = if wait { state.git_lock.lock().await } else { match state.git_lock.try_lock() { Ok(g) => g, Err(_) => return } };
    if !lib.join(".git").exists() || !crate::git::is_repo(state, lib).await { return; }
    if let Err(e) = crate::trust::require_git(lib) { crate::log::warn(format!("history: not recording ({e})")); return; }
    crate::log::ok_or_warn("history: could not record the library", crate::git::commit_all(state, lib, subject).await);
}

/// Like `record_library`, for a caller that already holds the library lock.
pub(crate) async fn record_library_locked(state: &AppState, lib: &Path, subject: &str) {
    if !state.settings.read().await.git_enabled() { return; }
    if crate::wraps::interrupted_wrap(state).await.is_some() { return; }
    let _g = state.git_lock.lock().await;
    if !lib.join(".git").exists() || !crate::git::is_repo(state, lib).await { return; }
    if let Err(e) = crate::trust::require_git(lib) { crate::log::warn(format!("history: not recording ({e})")); return; }
    crate::log::ok_or_warn("history: could not record the library", crate::git::commit_all(state, lib, subject).await);
}

/// Everything that happens once a run is over, however it ended.
async fn finish(state: Arc<AppState>, id: String, ok: bool, record: Option<PathBuf>, on_done: Option<OnDone>) {
    state.runs.lock().await.remove(&id);
    state.run_dirs.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    let done = state.chats.update(&id, |c| { c.running = false; c.updated = chrono::Utc::now(); }).await;
    // What the assistant changed in the library becomes its own history entry, apart from the user's edits.
    if let (Some(lib), Some(ch)) = (&record, &done) {
        let what = if ch.kind == "workflow" { format!("workflow {}", ch.title) } else { ch.title.clone() };
        record_library(&state, lib, &crate::git::subject::assistant(&what), true).await;
        state.emit(json!({"type": "library.changed"}));
    }
    if let Some(ch) = done {
        state.emit(json!({"type": "chat.done", "chatId": id, "ok": ok, "title": ch.title, "chat": ch.summary()}));
    }
    if let Some(f) = on_done { f(state.clone(), ok).await; }
}

/// Start a streaming run in the background. Events go out on the broadcast
/// channel; the finished chat is persisted. `on_done` runs after completion.
pub async fn start_run(
    state: Arc<AppState>,
    spec: RunSpec,
    on_done: Option<OnDone>,
) -> anyhow::Result<()> {
    // The one gate every AI run passes through (chats, wrap, reorganize, modules, customize).
    crate::settings::check_permission_mode(spec.options.permission_mode.as_deref())?;
    crate::settings::check_cli_value("model", spec.options.model.as_deref())?;
    crate::settings::check_cli_value("effort", spec.options.effort.as_deref())?;
    crate::settings::check_cli_value("session", spec.resume.as_deref())?;
    let settings = state.settings.read().await.clone();
    let library = settings.library();
    // Nothing runs in a folder the user has not trusted. The app layer is the app's own folder.
    // Its staging copy likewise: the app creates it, strips any settings from it, and runs there
    // only under the deny rules in `extra_deny`.
    let (layer, staging) = (crate::paths::app_layer_dir(), crate::paths::app_layer_staging_dir());
    let reorg = crate::paths::reorganize_worktree();
    for dir in std::iter::once(&spec.cwd).chain(spec.add_dirs.iter()) {
        if *dir == layer || *dir == staging { continue; }
        if *dir == reorg {
            // A second checkout of the library. It holds the *recorded* state, which is only as
            // trusted as the folder on disk if its assistant files are the very same ones.
            let lib = library.as_deref().ok_or_else(|| crate::tr::err("library.none", &[]))?;
            crate::trust::require(lib)?;
            let differing = crate::trust::differing_assistant_files(lib, dir);
            if !differing.is_empty() {
                crate::log::error(format!("reorganize: refused — recorded assistant files differ from the ones on disk: {}", differing.join(", ")));
                return Err(crate::tr::err("reorganize.untrusted", &[("files", &differing.join(", "))]));
            }
            continue;
        }
        crate::trust::require(dir)?;
    }
    let kind = state.chats.with(&spec.chat_id, |c| c.kind.clone()).await.unwrap_or_default();
    let mut deny = private_deny_settings(library.as_deref(), &spec.cwd, &settings.private_paths)?;
    // Reorganize works in a second checkout of the library: private folders are off-limits there too.
    if spec.cwd == reorg {
        deny = merge_deny(deny, private_deny_settings(Some(&spec.cwd), &spec.cwd, &settings.private_paths)?);
    }
    // Only the run that sets up a brand-new library may write its `.claude` folder (it installs the presets).
    let protective = protective_deny(&spec.cwd, &spec.add_dirs, &crate::paths::home(), &spec.chat_id, kind == "setup");
    deny = merge_deny(deny, Some(json!({"permissions": {"deny": protective}}).to_string()));
    if !spec.extra_deny.is_empty() {
        deny = merge_deny(deny, Some(json!({"permissions": {"deny": spec.extra_deny}}).to_string()));
    }
    // "Ask before changes" is gone: a stored `default` means `plan`.
    let mode = crate::settings::effective_permission_mode(spec.options.permission_mode.as_deref()).map(String::from);
    // A chat or workflow working in the library: the user's own pending edits are recorded first, so
    // the history shows what the assistant changed separately (and each can be undone on its own).
    let record = library.clone().filter(|l| *l == spec.cwd && matches!(kind.as_str(), "chat" | "workflow") && mode.as_deref() != Some("plan"));
    if let Some(lib) = &record {
        record_library(&state, lib, &crate::git::subject::edit("your changes"), false).await;
    }
    let lang = language_note(&settings.language);
    let sys = match &spec.append_system { Some(s) => format!("{s}\n\n{lang}"), None => lang.to_string() };
    let stopped = if settings.language == "ar" { "تم الإيقاف." } else { "Stopped." };
    let id = spec.chat_id.clone();

    let hook = state.run_hook.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(hook) = hook {
        let info = RunInfo { chat_id: id.clone(), cwd: spec.cwd.clone(), prompt: spec.prompt.clone(), permission_mode: mode.clone(),
            deny: deny.as_deref().and_then(|d| serde_json::from_str::<Value>(d).ok())
                .and_then(|v| v["permissions"]["deny"].as_array().cloned()).unwrap_or_default()
                .iter().filter_map(|r| r.as_str().map(String::from)).collect() };
        let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
        state.runs.lock().await.insert(id.clone(), kill_tx);
        state.run_dirs.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), spec.cwd.clone());
        state.ack_interrupted(&id).await;
        if let Some(ch) = state.chats.update(&id, |c| c.running = true).await {
            state.emit(json!({"type": "chat.updated", "chat": ch.summary()}));
        }
        tokio::spawn(async move {
            let ok = tokio::select! {
                _ = &mut kill_rx => { push(&state, &id, Msg::new("system", stopped)).await; false }
                ok = hook(info) => ok,
            };
            finish(state, id, ok, record, on_done).await;
        });
        return Ok(());
    }

    let bin = binary(&state).await.ok_or_else(|| anyhow::anyhow!("Claude Code CLI not found"))?;
    let mut c = cmd(&bin);
    c.current_dir(&spec.cwd)
        .args(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"]);
    // npm's `claude.cmd` on Windows goes through cmd.exe, which cannot carry multi-line or
    // quote-heavy arguments: the rules and the extra instructions travel as files instead.
    let mut temp_files: Vec<PathBuf> = vec![];
    let via_files = cfg!(windows) && is_cmd_shim(&bin);
    let mut as_file = |name: &str, text: &str| -> std::io::Result<PathBuf> {
        let dir = crate::paths::run_tmp_dir();
        crate::paths::create_private_dir(&dir)?;
        let f = dir.join(format!("{}-{name}", uuid::Uuid::new_v4().simple()));
        crate::paths::write_atomic(&f, text.as_bytes(), true)?;
        temp_files.push(f.clone());
        Ok(f)
    };
    if let Some(d) = &deny {
        if via_files { c.arg("--settings").arg(as_file("settings.json", d)?); } else { c.args(["--settings", d]); }
    }
    if let Some(r) = &spec.resume { c.args(["--resume", r]); }
    if let Some(m) = spec.options.model.as_ref().filter(|s| !s.is_empty()) { c.args(["--model", m]); }
    if let Some(e) = spec.options.effort.as_ref().filter(|s| !s.is_empty()) { c.args(["--effort", e]); }
    if let Some(p) = &mode { c.args(["--permission-mode", p]); }
    for d in &spec.add_dirs { c.arg("--add-dir").arg(d); }
    if via_files { c.arg("--append-system-prompt-file").arg(as_file("system.txt", &sys)?); } else { c.args(["--append-system-prompt", &sys]); }
    provider_env(&state, &mut c).await;
    c.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = c.spawn().map_err(|e| {
        crate::log::error(format!("run {}: could not start the assistant: {e}", spec.chat_id));
        for f in &temp_files { let _ = std::fs::remove_file(f); }
        e
    })?;
    let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        anyhow::bail!("could not connect to the assistant process");
    };
    // Remember the process, so a daemon that dies uncleanly can be cleaned up after.
    let pid = child.id();
    crate::runs::record(pid, &spec.chat_id);
    let started = std::time::Instant::now();
    crate::log::info(format!("run {} started (pid {}, mode {}, in {})", spec.chat_id, pid.unwrap_or(0),
        mode.as_deref().unwrap_or("default"), spec.cwd.display()));
    let prompt = spec.prompt.clone();
    tokio::spawn(async move {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    });

    let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
    state.runs.lock().await.insert(spec.chat_id.clone(), kill_tx);
    state.run_dirs.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), spec.cwd.clone());
    state.ack_interrupted(&id).await;
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
                    // The whole group: the assistant's own children (shells, tools) must not live on.
                    if let Some(pid) = pid { crate::proc::kill_tree(pid); }
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
        for f in &temp_files { let _ = std::fs::remove_file(f); }
        let stderr_text = err_task.await.unwrap_or_default();
        if !got_result && !stderr_text.trim().is_empty() {
            push(&state, &id, Msg::new("error", stderr_text.trim())).await;
        }
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
        finish(state, id, ok, record, on_done).await;
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
            if let Some(pid) = pid { crate::proc::kill_tree(pid); }
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
    fn private_folders_are_denied_in_every_spelling_we_know() {
        let d = std::env::temp_dir().join(format!("thoughts-deny-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("Me/Finances")).unwrap();
        let lib = crate::paths::canonical(&d).unwrap();
        // Configured in lower case, spelled with capitals on disk.
        let v = private_deny_settings(Some(&lib), &lib, &["me/finances".into()]).unwrap().unwrap();
        assert!(v.contains("Read(./me/finances/**)"));
        if lib.join("me/finances").exists() {
            // A volume that ignores case: the real spelling is covered as well.
            assert!(v.contains("Read(./Me/Finances/**)"), "{v}");
        }
        let v = private_deny_settings(Some(&lib), &lib, &["Me/Finances".into()]).unwrap().unwrap();
        assert!(v.contains("Read(./Me/Finances/**)") && v.contains("Read(./me/finances/**)"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn every_run_is_kept_out_of_runnable_config_and_the_apps_own_data() {
        let home = std::env::temp_dir().join(format!("thoughts-protect-{}", uuid::Uuid::new_v4()));
        for sub in ["chats", "attachments/this-chat", "attachments/other-chat", "extensions/search", "app-layer", "reorganize-worktree"] {
            std::fs::create_dir_all(home.join(sub)).unwrap();
        }
        std::fs::write(home.join("something-new.json"), "{}").unwrap();
        let (lib, proj) = (Path::new("/home/u/Library"), PathBuf::from("/home/u/proj"));
        let h = rule_path(&home);
        let deny = protective_deny(lib, &[proj.clone()], &home, "this-chat", false);
        let has = |r: &str| deny.iter().any(|d| d == r);
        for want in ["Edit(./.claude/**)", "Edit(./.mcp.json)", "Edit(./.git/**)", "Edit(//home/u/Library/.claude/**)", "Edit(//home/u/Library/.git/**)",
                     "Edit(//home/u/proj/.claude/**)", "Edit(//home/u/proj/.mcp.json)", "Edit(//home/u/Library/**/.claude/**)"] {
            assert!(has(want), "missing {want}");
        }
        // Routines stay readable (a wrap reads .claude/skills): nothing denies Read inside the library.
        assert!(!deny.iter().any(|d| d.starts_with("Read(") && (d.contains("/home/u/Library") || d.starts_with("Read(./"))), "{deny:?}");
        // The app's own data: secrets, settings, other chats — including a file this version has never heard of.
        for want in [format!("Read({h}/daemon.json)"), format!("Edit({h}/settings.json)"), format!("Read({h}/git-credentials)"),
                     format!("Read({h}/secrets.json)"), format!("Read({h}/chats/**)"), format!("Read({h}/something-new.json)"),
                     format!("Read({h}/attachments/other-chat/**)"), format!("Edit({h}/attachments/**)"), format!("Edit({h}/extensions/**)")] {
            assert!(has(&want), "missing {want}");
        }
        // This chat's own attachments, the search add-on and the customisation layer stay readable.
        for open in [format!("Read({h}/attachments/this-chat/**)"), format!("Read({h}/attachments/**)"), format!("Read({h}/extensions/**)"), format!("Read({h}/app-layer/**)")] {
            assert!(!has(&open), "{open} must not be denied");
        }
        // The reorganize run works inside the app's folder: its own working copy is open to it.
        let wt = home.join("reorganize-worktree");
        let deny = protective_deny(&wt, &[], &home, "c", false);
        assert!(!deny.iter().any(|d| d == &format!("Read({h}/reorganize-worktree/**)")));
        assert!(deny.iter().any(|d| d == &format!("Edit({h}/reorganize-worktree/.claude/**)")));
        // Setting up a brand-new library may write its .claude folder — and nothing else on the list.
        let deny = protective_deny(lib, &[], &home, "c", true);
        assert!(!deny.iter().any(|d| d.contains(".claude")), "{deny:?}");
        assert!(deny.iter().any(|d| d == "Edit(./.git/**)") && deny.iter().any(|d| d == "Edit(./.mcp.json)"));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn windows_prefers_a_real_program_and_spots_the_shim() {
        let dirs = [Path::new("/npm"), Path::new("/native")];
        let cands = candidates("claude", true, ".COM;.EXE;.BAT;.CMD");
        let both = |p: &Path| p == Path::new("/npm/claude.cmd") || p == Path::new("/native/claude.exe");
        assert_eq!(search(&dirs, &cands, true, both), Some(PathBuf::from("/native/claude.exe")));
        assert_eq!(search(&dirs, &cands, true, |p| p == Path::new("/npm/claude.cmd")), Some(PathBuf::from("/npm/claude.cmd")));
        // Elsewhere the first folder on the path wins, as always.
        let unix = candidates("claude", false, "");
        assert_eq!(search(&dirs, &unix, false, |p| p == Path::new("/npm/claude") || p == Path::new("/native/claude")), Some(PathBuf::from("/npm/claude")));
        assert!(is_cmd_shim(Path::new(r"C:\Users\a\AppData\Roaming\npm\claude.CMD")) && is_cmd_shim(Path::new("claude.bat")));
        assert!(!is_cmd_shim(Path::new("/usr/local/bin/claude")) && !is_cmd_shim(Path::new("claude.exe")));
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
