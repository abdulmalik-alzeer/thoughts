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

/// PATH as the user's own terminal has it — GUI apps on macOS start with a bare PATH — followed
/// by the folders programs are commonly installed in.
pub fn shell_path() -> &'static str {
    static P: OnceLock<String> = OnceLock::new();
    P.get_or_init(|| {
        let base = std::env::var("PATH").unwrap_or_default();
        let mut parts: Vec<String> = vec![];
        #[cfg(unix)]
        parts.extend(terminal_path(&std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()), &[]));
        parts.extend(base.split(if cfg!(windows) { ';' } else { ':' }).map(String::from));
        if let Some(h) = dirs::home_dir() {
            for d in [".local/bin", ".claude/local", ".npm-global/bin", ".cargo/bin", ".bun/bin"] {
                parts.push(h.join(d).to_string_lossy().into());
            }
            // Should the shell not have told us (a start-up file that hangs or fails): where version
            // managers keep what they install.
            let nvm = std::env::var_os("NVM_DIR").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| h.join(".nvm"));
            for d in version_manager_dirs(&h, &nvm) { parts.push(d.to_string_lossy().into()); }
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

/// How long the user's shell gets to say what its PATH is.
#[cfg(unix)]
const SHELL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);
/// Printed on both sides of the PATH, because start-up files may print things of their own.
#[cfg(unix)]
const PATH_MARK: &str = "__THOUGHTS_PATH__";

/// The PATH `shell` sets up for a terminal window. nvm, fnm, asdf and their like add their folders
/// in the start-up file of *interactive* shells (`~/.zshrc`, `~/.bashrc`), which a plain login
/// shell (`-lc`) never reads — so the shell is first asked as an interactive login shell, and as a
/// plain login shell only when that fails or takes too long. `env` is added to the shell's
/// environment (tests point it at start-up files of their own).
#[cfg(unix)]
fn terminal_path(shell: &str, env: &[(&str, &str)]) -> Vec<String> {
    for flags in ["-ilc", "-lc"] {
        let mut c = crate::proc::std_command(shell);
        c.args([flags, &format!("printf '%s%s%s' {PATH_MARK} \"$PATH\" {PATH_MARK}")]);
        for (k, v) in env { c.env(k, v); }
        let found = crate::proc::std_output_timeout(&mut c, SHELL_TIMEOUT).ok().and_then(|o| marked_path(&String::from_utf8_lossy(&o.stdout)));
        if let Some(p) = found { return p; }
    }
    vec![]
}

/// The PATH between the two marks in what the shell printed, whatever else is around it.
#[cfg(unix)]
fn marked_path(out: &str) -> Option<Vec<String>> {
    let start = out.find(PATH_MARK)? + PATH_MARK.len();
    let end = start + out[start..].find(PATH_MARK)?;
    let dirs: Vec<String> = out[start..end].split(':').map(str::trim).filter(|d| !d.is_empty()).map(String::from).collect();
    (!dirs.is_empty()).then_some(dirs)
}

/// Folders (that exist) where version managers keep the programs they install: nvm's default Node
/// version, and the shim folders of fnm, asdf, mise, Volta, nodenv and n.
fn version_manager_dirs(home: &Path, nvm: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = nvm_bin(nvm).into_iter().collect();
    for d in [".local/share/fnm/aliases/default/bin", "Library/Application Support/fnm/aliases/default/bin", ".fnm/aliases/default/bin",
              ".asdf/shims", ".local/share/mise/shims", ".volta/bin", ".nodenv/shims", "n/bin"] {
        out.push(home.join(d));
    }
    out.retain(|d| d.is_dir());
    out
}

/// The `bin` folder of the Node version nvm uses by default: the installed version its `default`
/// alias names, or else the newest one installed.
fn nvm_bin(nvm: &Path) -> Option<PathBuf> {
    let number = |name: &str| -> Option<Vec<u64>> { name.trim().trim_start_matches('v').split('.').map(|n| n.parse().ok()).collect() };
    let mut versions: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(nvm.join("versions/node")).ok()?.flatten()
        .filter_map(|e| Some((number(&e.file_name().to_string_lossy())?, e.path()))).collect();
    versions.sort();
    // The alias may be a whole version ("v20.11.1"), part of one ("20"), or a name ("lts/*", "node") — a name means the newest.
    let wanted = std::fs::read_to_string(nvm.join("alias/default")).ok().and_then(|a| number(&a));
    let pick = wanted.and_then(|w| versions.iter().rev().find(|(v, _)| v.starts_with(&w))).or(versions.last())?;
    Some(pick.1.join("bin"))
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

/// How long a "signed in" answer from the CLI is trusted before a chat asks it again.
const AUTH_TTL: std::time::Duration = std::time::Duration::from_secs(30);

/// What `claude auth status --json` prints, or `Null` when it cannot be asked. Never hangs.
async fn auth_status(bin: &Path) -> Value {
    let mut c = cmd(bin);
    c.args(["auth", "status", "--json"]);
    crate::proc::output_timeout(&mut c, std::time::Duration::from_secs(15)).await.ok()
        .and_then(|o| serde_json::from_slice(&o.stdout).ok()).unwrap_or(Value::Null)
}

/// `{installed, version, loggedIn, authFailed, auth}`. `loggedIn` is what the CLI says, unless the
/// last run was refused for lack of a valid sign-in (`authFailed`): the CLI only looks at what is
/// stored on this machine, so an expired or revoked sign-in still reads as "logged in" there.
pub async fn status(state: &AppState) -> Value {
    let Some(bin) = binary(state).await else {
        return json!({"installed": false});
    };
    let version = version(state).await;
    let auth = auth_status(&bin).await;
    // `provider` says who answers: "account" (the Claude sign-in), "anthropicKey", "service" (another
    // address with its own key) or "incomplete" (an address without a key: nothing runs).
    let provider = provider_of(&state.settings.read().await.ai.claude);
    let failed = state.auth_failed.load(std::sync::atomic::Ordering::Relaxed);
    let logged_in = (auth.get("loggedIn").and_then(Value::as_bool).unwrap_or(false) || provider.uses_key()) && !failed;
    json!({"installed": true, "path": bin, "version": version, "loggedIn": logged_in, "authFailed": failed, "auth": auth, "provider": provider.name()})
}

/// Does this error text from the CLI mean "not signed in" (never signed in, expired, revoked, bad key)?
pub fn is_auth_failure(text: &str) -> bool {
    let t = text.to_lowercase();
    ["not logged in", "please run /login", "run /login", "invalid api key", "invalid x-api-key", "authentication_error",
     "authentication_failed", "oauth token", "invalid bearer token", "invalid authentication credentials"].iter().any(|k| t.contains(k))
}

/// Who answers the assistant's requests, as set up under Settings › Assistant.
#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    /// The user's own Claude sign-in (what `claude auth login` stored).
    Account,
    /// An Anthropic API key, sent to Anthropic itself.
    AnthropicKey,
    /// Another Anthropic-compatible service at this address, with the key saved for it.
    Service(String),
    /// An address with no key saved for it (or one that is not a valid address). Nothing may run:
    /// with only an address, Claude Code sends the user's Claude sign-in to that address.
    Incomplete,
}

impl Provider {
    pub fn name(&self) -> &'static str {
        match self { Provider::Account => "account", Provider::AnthropicKey => "anthropicKey", Provider::Service(_) => "service", Provider::Incomplete => "incomplete" }
    }
    /// Does it pay with a key of its own (then the Claude account sign-in is beside the point)?
    pub fn uses_key(&self) -> bool { matches!(self, Provider::AnthropicKey | Provider::Service(_)) }
}

/// The address requests go to, when it is not Anthropic's own: `None` for no address and for
/// Anthropic's address alike. A saved key belongs to exactly one such destination.
pub fn destination(base_url: Option<&str>) -> Option<&str> {
    base_url.map(str::trim).filter(|u| !u.is_empty() && !crate::settings::is_anthropic_url(u))
}

pub fn provider_of(c: &crate::settings::ClaudeSettings) -> Provider {
    match (destination(c.base_url.as_deref()), c.has_api_token) {
        (None, false) => Provider::Account,
        (None, true) => Provider::AnthropicKey,
        (Some(u), true) if crate::settings::check_base_url(u).is_ok() => Provider::Service(u.to_string()),
        (Some(_), _) => Provider::Incomplete,
    }
}

/// What a run's environment must carry (`set`) and must not inherit from the daemon's own (`unset`).
#[derive(Debug, Default, PartialEq)]
pub struct ProviderEnv { pub set: Vec<(&'static str, String)>, pub unset: Vec<&'static str> }

impl ProviderEnv {
    fn apply(&self, c: &mut Command) {
        for name in &self.unset { c.env_remove(name); }
        for (name, value) in &self.set { c.env(name, value); }
    }
}

/// The environment for `provider`, given the key read from the password store.
/// - An Anthropic key travels as `ANTHROPIC_API_KEY` (the `X-Api-Key` header, which is what
///   Anthropic takes for API keys) and only ever to Anthropic: an address or bearer token inherited
///   from the daemon's environment is dropped, since either would redirect or outrank it.
/// - Another service gets its address and the key as `ANTHROPIC_AUTH_TOKEN` (a bearer token, which
///   is what gateways take); an inherited Anthropic key is dropped so it is never sent there.
/// - An address without a key never runs.
pub fn provider_env_for(provider: &Provider, key: Option<String>) -> anyhow::Result<ProviderEnv> {
    let key = key.filter(|k| !k.trim().is_empty());
    match provider {
        Provider::Account => Ok(ProviderEnv::default()),
        Provider::Incomplete => Err(crate::tr::err("provider.incomplete", &[])),
        Provider::AnthropicKey => {
            let key = key.ok_or_else(|| crate::tr::err("provider.key_unreadable", &[]))?;
            Ok(ProviderEnv { set: vec![("ANTHROPIC_API_KEY", key)], unset: vec!["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"] })
        }
        Provider::Service(url) => {
            let key = key.ok_or_else(|| crate::tr::err("provider.key_unreadable", &[]))?;
            Ok(ProviderEnv { set: vec![("ANTHROPIC_BASE_URL", url.clone()), ("ANTHROPIC_AUTH_TOKEN", key)], unset: vec!["ANTHROPIC_API_KEY"] })
        }
    }
}

/// Where the provider key is kept. The real one is the system password store.
pub trait KeyStore: Send + Sync {
    fn get(&self) -> Option<String>;
    fn set(&self, key: &str) -> anyhow::Result<()>;
    fn delete(&self) -> anyhow::Result<()>;
}

/// The system password store (`secrets.rs`).
pub struct Keychain;
impl KeyStore for Keychain {
    fn get(&self) -> Option<String> { crate::secrets::get("thoughts-ai", "claude") }
    fn set(&self, key: &str) -> anyhow::Result<()> { crate::secrets::set("thoughts-ai", "claude", key) }
    fn delete(&self) -> anyhow::Result<()> { crate::secrets::delete("thoughts-ai", "claude") }
}

/// The saved provider key. Unit tests never touch the real password store.
fn saved_key() -> Option<String> {
    if cfg!(test) { return Some("test-key".into()); }
    Keychain.get()
}

/// The provider environment for a run that is about to start, or the reason it may not start.
async fn provider_env(state: &AppState) -> anyhow::Result<ProviderEnv> {
    let provider = provider_of(&state.settings.read().await.ai.claude);
    let key = if provider.uses_key() { saved_key() } else { None };
    provider_env_for(&provider, key).map_err(|e| { crate::log::warn(format!("assistant not started: {e}")); e })
}

/// What `set_provider` did.
#[derive(Debug, PartialEq)]
pub struct ProviderSaved {
    pub provider: Provider,
    /// A key saved for the previous address was removed (it is never sent to another one).
    pub key_removed: bool,
}

/// Set the provider: its address and its key, together. `base_url` blank = Anthropic itself;
/// `key` blank = keep the saved key, which is only possible while the destination stays the same.
/// - A new key is saved for exactly the address given with it.
/// - Changing to another service without giving its key is refused, and nothing changes.
/// - Going back to Anthropic without a key removes a key that belonged to another service.
pub async fn set_provider(state: &AppState, store: &dyn KeyStore, base_url: Option<&str>, key: Option<&str>) -> anyhow::Result<ProviderSaved> {
    if crate::settings::read_only() { return Err(crate::tr::err("newer_settings", &[])); }
    let url = base_url.map(str::trim).filter(|u| !u.is_empty());
    if let Some(u) = url { crate::settings::check_base_url(u)?; }
    let key = key.map(str::trim).filter(|k| !k.is_empty());
    let mut s = state.settings.write().await;
    let cur = s.ai.claude.clone();
    let moved = destination(cur.base_url.as_deref()) != destination(url);
    let keeps_key = cur.has_api_token && !moved;
    if destination(url).is_some() && key.is_none() && !keeps_key { return Err(crate::tr::err("provider.key_needed", &[])); }
    let drops_old_key = key.is_none() && cur.has_api_token && moved;
    // The change is made on a copy, saved, and only then swapped in, so a refused save leaves
    // memory and disk agreeing.
    let mut next = s.clone();
    next.ai.claude.base_url = url.map(String::from);
    next.ai.claude.has_api_token = key.is_some() || keeps_key;
    // A new key goes into the password store before the settings say it is there…
    if let Some(k) = key { store.set(k)?; }
    if let Err(e) = next.save() {
        // …and comes out again when they could not be saved, unless it only replaced the key of the
        // same address: the settings on disk still describe the old address, and a key typed for
        // another one must never be sent there.
        if key.is_some() && (moved || !cur.has_api_token) { crate::log::ok_or_warn("could not remove the provider key after a failed save", store.delete()); }
        return Err(e.into());
    }
    // A key that belonged to the previous address goes only once "no key" is on disk.
    if drops_old_key { crate::log::ok_or_warn("could not remove the saved provider key", store.delete()); }
    let provider = provider_of(&next.ai.claude);
    *s = next;
    drop(s);
    state.forget_auth();
    crate::log::info(format!("assistant provider set to {}", provider.name()));
    Ok(ProviderSaved { provider, key_removed: drops_old_key })
}

/// Back to the Claude account: forget the address and the key.
pub async fn clear_provider(state: &AppState, store: &dyn KeyStore) -> anyhow::Result<()> {
    if crate::settings::read_only() { return Err(crate::tr::err("newer_settings", &[])); }
    let mut s = state.settings.write().await;
    let mut next = s.clone();
    next.ai.claude.base_url = None;
    next.ai.claude.has_api_token = false;
    // "No key, no address" is saved first: if that is refused, the key is still there and still described.
    next.save()?;
    *s = next;
    drop(s);
    crate::log::ok_or_warn("could not remove the saved provider key", store.delete());
    state.forget_auth();
    crate::log::info("assistant provider removed; using the Claude account");
    Ok(())
}

/// What to tell the user when the provider (not the Claude account) refuses the saved key.
pub fn key_refused_text() -> String { crate::tr::text("provider.key_refused", &[]) }

/// The sentence shown when a run is refused for lack of a sign-in, in the user's language.
pub fn signed_out_text() -> String {
    crate::tr::user_err_in(crate::tr::arabic(), "assistant.signed_out", &[]).message.trim_start_matches("SIGNED_OUT::").to_string()
}

/// Ask before a chat message is sent, so a missing sign-in is said plainly up front instead of
/// surfacing as a failed answer. A "yes" is remembered for `AUTH_TTL`; "cannot tell" lets the run go.
pub async fn require_signed_in(state: &AppState) -> anyhow::Result<()> {
    match provider_of(&state.settings.read().await.ai.claude) {
        // An address without a key: said now, before the message is saved, not as a failed answer.
        Provider::Incomplete => return Err(crate::tr::err("provider.incomplete", &[])),
        // Paid with a key: whether the Claude account is signed in does not matter.
        Provider::AnthropicKey | Provider::Service(_) => return Ok(()),
        Provider::Account => {}
    }
    if state.run_hook.lock().unwrap_or_else(|e| e.into_inner()).is_some() { return Ok(()); }
    let fresh = state.auth_ok_at.lock().unwrap_or_else(|e| e.into_inner()).map_or(false, |at| at.elapsed() < AUTH_TTL);
    if fresh { return Ok(()); }
    let bin = binary(state).await.ok_or_else(|| crate::tr::err("assistant.missing", &[]))?;
    match auth_status(&bin).await.get("loggedIn").and_then(Value::as_bool) {
        Some(true) => { *state.auth_ok_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now()); Ok(()) }
        Some(false) => {
            crate::log::warn("chat not started: the assistant is not signed in");
            state.emit(json!({"type": "auth.changed", "loggedIn": false}));
            Err(crate::tr::err("assistant.signed_out", &[]))
        }
        None => Ok(()),
    }
}

/// A run failed with `text`. When that means the credentials were refused, return the plain
/// sentence to show instead of the CLI's own, and the code the chat marks the message with:
/// `signedOut` for the Claude account (remembered, so the app says so everywhere), `providerKey`
/// when the user's own key was refused (that is about the key, not the account).
async fn auth_failure(state: &AppState, text: &str) -> Option<(String, &'static str)> {
    if !is_auth_failure(text) { return None; }
    if provider_of(&state.settings.read().await.ai.claude).uses_key() {
        crate::log::warn("the assistant refused a run: the service did not accept the saved key");
        return Some((key_refused_text(), "providerKey"));
    }
    crate::log::warn("the assistant refused a run: not signed in, or the sign-in has expired");
    state.note_auth_failed();
    Some((signed_out_text(), "signedOut"))
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
        // Only the modes the daemon accepts, in a stable order. The last one (bypassPermissions) is
        // only switched on after the user confirmed it.
        "permissionModes": crate::settings::PERMISSION_MODES,
    })
}

/// Does the installed assistant program accept `flag`? Read from its own `--help` (remembered for
/// an hour per program), never assumed.
pub async fn has_flag(state: &AppState, flag: &str) -> bool {
    static HELP: std::sync::Mutex<Option<(PathBuf, std::time::Instant, String)>> = std::sync::Mutex::new(None);
    let Some(bin) = binary(state).await else { return false };
    let cached = HELP.lock().unwrap_or_else(|e| e.into_inner()).clone()
        .filter(|(b, at, _)| *b == bin && at.elapsed() < std::time::Duration::from_secs(3600)).map(|(_, _, h)| h);
    let help = match cached {
        Some(h) => h,
        None => {
            let mut c = cmd(&bin);
            c.arg("--help");
            let h = crate::proc::output_timeout(&mut c, std::time::Duration::from_secs(15)).await.ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
            if !h.is_empty() { *HELP.lock().unwrap_or_else(|e| e.into_inner()) = Some((bin, std::time::Instant::now(), h.clone())); }
            h
        }
    };
    !flag_block(&help, flag).is_empty()
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

/// Stands in for a one-shot answer (`complete`): given the prompt and the model, the text.
/// Set on `AppState::complete_hook` by tests, so summaries can be exercised without the CLI.
pub type CompleteHook = Arc<dyn Fn(String, Option<String>) -> futures::future::BoxFuture<'static, anyhow::Result<String>> + Send + Sync>;

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
            // A deny rule cannot make an exception, so every other chat's folder is named — with as
            // few rules as it takes: nothing in the whole folder can be edited (the two rules below),
            // which leaves reading and the shell to close per folder.
            Some("attachments") => {
                let p = rule_path(dir);
                deny.push(format!("Edit({p})"));
                deny.push(format!("Edit({p}/**)"));
                let others: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten()
                    .filter(|e| e.file_name().to_string_lossy() != chat_id).map(|e| e.path()).collect();
                if others.len() > MAX_CLOSED_FOLDERS {
                    // More folders than the rules file may name (Claude Code ignores a settings file
                    // over 2 MB — and with it every rule): close the whole folder, this chat's included.
                    crate::log::warn(format!("{} chats have attachments: more than can be closed one by one, so no chat's attachments can be read by the assistant", others.len()));
                    closed(dir, &mut deny);
                } else {
                    for o in &others {
                        let p = rule_path(o);
                        deny.push(format!("Read({p})"));
                        deny.push(format!("Read({p}/**)"));
                        deny.push(format!("Bash(*{}*)", o.to_string_lossy().replace('\\', "/")));
                    }
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
    // The live customisation layer changes only through a change the user reviewed and kept: every
    // run may read it, none may write it (a customise run works in the staging copy beside it).
    for dir in crate::paths::write_protected_in(home) {
        let (p, shell) = (rule_path(&dir), dir.to_string_lossy().replace('\\', "/"));
        for rule in [format!("Edit({p})"), format!("Edit({p}/**)"), format!("Bash(*{shell}/*)"), format!("Bash(*{shell} *)"), format!("Bash(*{shell})")] { deny.push(rule); }
    }
    let mut seen = std::collections::HashSet::new();
    deny.retain(|r| seen.insert(r.clone()));
    deny
}

/// The most other chats' attachment folders a run's rules name one by one (about 270 bytes each,
/// far below the 2 MB a settings file may have).
pub const MAX_CLOSED_FOLDERS: usize = 3000;
/// The largest rules document a run is started with. Claude Code does not read a settings file
/// over 2 MB, and a run without its rules must never start.
pub const MAX_RULES_BYTES: usize = 1_500_000;

/// What every run that may not write `.claude/` (all but first-time setup) is told about it. The
/// routines of libraries made before 0.0.9 still say "edit that skill's file"; this is how such a
/// run learns that it cannot, and what to do instead.
pub fn managed_note(kind: &str) -> Option<&'static str> {
    if kind == "setup" { return None; }
    Some("The `.claude` folder here (routines, also called skills, and settings) is kept by the Thoughts app: you can read it, but any \
        attempt to create, edit or delete something inside it is refused. Do not try, and never say such a change was made. When a routine \
        (tune-up or wrap, for example) tells you to edit a skill file, describe the agreed change in one or two plain sentences instead and \
        ask the user to open Settings → Customisations → Routines in the app, choose that routine, and paste your sentences into the box that \
        asks the assistant for a draft; they review the result and keep it there. Changes to CLAUDE.md and to files under `routines/` are \
        yours to make as before.")
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

/// How a long value reaches the assistant program: on its command line, or in a private file.
#[derive(Clone, Copy)]
enum Passed<'a> { Inline(&'a str), File(&'a Path) }

/// Short-lived private files handed to one assistant run (its rules, and on Windows its extra
/// instructions). They are removed when this is dropped: when the run is over, could not start,
/// or is cut off by the daemon stopping.
#[derive(Default)]
struct TempFiles(Vec<PathBuf>);

impl TempFiles {
    fn add(&mut self, dir: &Path, name: &str, text: &str) -> std::io::Result<PathBuf> {
        crate::paths::create_private_dir(dir)?;
        let f = dir.join(format!("{}-{name}", uuid::Uuid::new_v4().simple()));
        crate::paths::write_atomic(&f, text.as_bytes(), true)?;
        self.0.push(f.clone());
        Ok(f)
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) { for f in &self.0 { let _ = std::fs::remove_file(f); } }
}

/// Remove what an earlier daemon left in the run folder (it died before its runs were over).
/// Called at start, before any run of this daemon exists.
pub fn clear_run_tmp(dir: &Path) -> usize {
    std::fs::read_dir(dir).into_iter().flatten().flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| std::fs::remove_file(e.path()).is_ok()).count()
}

const MINUTE: std::time::Duration = std::time::Duration::from_secs(60);
/// A run nobody is watching that prints nothing for this long has stalled.
pub const IDLE_LIMIT: std::time::Duration = std::time::Duration::from_secs(20 * 60);
/// The longest a run nobody is watching may take.
pub const TOTAL_LIMIT: std::time::Duration = std::time::Duration::from_secs(2 * 60 * 60);
/// After its final answer the program should exit by itself; it gets this long.
pub const AFTER_RESULT: std::time::Duration = std::time::Duration::from_secs(2 * 60);
/// A program that closed its output gets this long to exit.
const EXIT_GRACE: std::time::Duration = std::time::Duration::from_secs(20);

/// How long a run may stay silent, and how long it may take altogether, before it is ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunLimits {
    /// No output at all for this long: the program has stalled.
    pub idle: Option<std::time::Duration>,
    /// The longest the run may take from start to finish.
    pub total: Option<std::time::Duration>,
    /// How long the program may linger after its final answer.
    pub after_result: std::time::Duration,
}

/// The limits for a run of this kind of chat.
/// - A chat or a customise run is watched by the person who started it, who has the Stop button.
/// - An automatic task has a time limit of its own (`workflows.rs`); silence still means it stalled.
/// - Everything else the app runs (wrap, reorganize, set-up, adding a feature) holds something up
///   while it runs — the library, most of all — so it may neither stall nor go on for ever.
pub fn run_limits(kind: &str) -> RunLimits {
    match kind {
        "chat" | "customize" => RunLimits { idle: None, total: None, after_result: AFTER_RESULT },
        "workflow" => RunLimits { idle: Some(IDLE_LIMIT), total: None, after_result: AFTER_RESULT },
        _ => RunLimits { idle: Some(IDLE_LIMIT), total: Some(TOTAL_LIMIT), after_result: AFTER_RESULT },
    }
}

impl RunLimits {
    /// The limit a run has gone past, if any. `running` = since it started, `silent` = since its
    /// last output, `result` = its final answer once it has given one.
    fn exceeded(&self, running: std::time::Duration, silent: std::time::Duration, result: Option<bool>) -> Option<Ended> {
        if let Some(ok) = result { return (silent >= self.after_result).then_some(Ended::Result(ok)); }
        if let Some(t) = self.total.filter(|t| running >= *t) { return Some(Ended::TooLong(t)); }
        if let Some(i) = self.idle.filter(|i| silent >= *i) { return Some(Ended::Stalled(i)); }
        None
    }

    /// How long until the limits need looking at again.
    fn next_check(&self, running: std::time::Duration, silent: std::time::Duration, has_result: bool) -> std::time::Duration {
        let mut wait = std::time::Duration::from_secs(3600);
        if has_result { wait = wait.min(self.after_result.saturating_sub(silent)); }
        else {
            if let Some(t) = self.total { wait = wait.min(t.saturating_sub(running)); }
            if let Some(i) = self.idle { wait = wait.min(i.saturating_sub(silent)); }
        }
        wait.max(std::time::Duration::from_millis(10))
    }
}

/// How a run came to its end.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// The program gave its final answer (`true` = it succeeded).
    Result(bool),
    /// It exited without one.
    NoResult,
    /// It was stopped: by the user, or by the app.
    Stopped,
    /// It printed nothing for this long and was ended.
    Stalled(std::time::Duration),
    /// It went on for longer than this and was ended.
    TooLong(std::time::Duration),
    /// Its output could no longer be read, and it was ended.
    Lost(String),
}

/// What `follow` reports.
struct Followed { ended: Ended, exit: std::io::Result<std::process::ExitStatus>, stderr: String }

/// Follow a started assistant program to its end. Every line it prints goes to `on_line`, which
/// answers `Some(success)` for the final result. And it does end: when `kill` fires (the Stop
/// button), when it goes past `limits`, when its output can no longer be read, or when it has
/// closed its output and still does not exit. The kill switch is honoured the whole time,
/// including after the output is over — the process (and everything it started) is always gone
/// when this returns.
async fn follow<F, Fut>(child: &mut tokio::process::Child, kill: &mut tokio::sync::oneshot::Receiver<()>, limits: RunLimits, mut on_line: F) -> Followed
where F: FnMut(String) -> Fut, Fut: std::future::Future<Output = Option<bool>> {
    let pid = child.id();
    // What it says on stderr is collected on the side, and can be read even if that stream never closes.
    let err_text = Arc::new(std::sync::Mutex::new(String::new()));
    let mut err_task = child.stderr.take().map(|e| {
        let text = err_text.clone();
        tokio::spawn(async move {
            let (mut reader, mut raw) = (BufReader::new(e), Vec::new());
            while matches!(reader.read_until(b'\n', &mut raw).await, Ok(n) if n > 0) {
                let mut t = text.lock().unwrap_or_else(|e| e.into_inner());
                if t.len() < 4000 { t.push_str(String::from_utf8_lossy(&raw).trim_end()); t.push('\n'); }
                raw.clear();
            }
        })
    });
    let (started, mut last_output, mut result, mut past_limit) = (std::time::Instant::now(), std::time::Instant::now(), None, false);
    let mut ended = match child.stdout.take() {
        None => Ended::Lost("the program has no output to read".into()),
        Some(out) => {
            // Bytes, not text: one line that is not valid UTF-8 must not end the run.
            let (mut reader, mut raw) = (BufReader::new(out), Vec::new());
            loop {
                let wait = limits.next_check(started.elapsed(), last_output.elapsed(), result.is_some());
                tokio::select! {
                    _ = &mut *kill => break Ended::Stopped,
                    n = reader.read_until(b'\n', &mut raw) => match n {
                        Ok(0) => break result.map_or(Ended::NoResult, Ended::Result),
                        Ok(_) => {
                            last_output = std::time::Instant::now();
                            let line = String::from_utf8_lossy(&raw).trim().to_string();
                            raw.clear();
                            if !line.is_empty() { if let Some(r) = on_line(line).await { result = Some(r); } }
                        }
                        Err(e) => break Ended::Lost(e.to_string()),
                    },
                    _ = tokio::time::sleep(wait) => {
                        if let Some(why) = limits.exceeded(started.elapsed(), last_output.elapsed(), result) { past_limit = true; break why; }
                    }
                }
            }
        }
    };
    // The output is over. A program that finished exits at once; wait for that — but it can still
    // be stopped, and it does not get for ever.
    let mut exit = None;
    if !past_limit && matches!(ended, Ended::Result(_) | Ended::NoResult) {
        tokio::select! {
            e = child.wait() => exit = Some(e),
            _ = &mut *kill => { if result.is_none() { ended = Ended::Stopped; } }
            _ = tokio::time::sleep(EXIT_GRACE) => {}
        }
    }
    let exit = match exit {
        Some(e) => e,
        None => {
            // The whole group: the assistant's own children (shells, tools) must not live on.
            if let Some(pid) = pid { crate::proc::kill_tree(pid); }
            let _ = child.kill().await;
            child.wait().await
        }
    };
    // A helper that still holds stderr open must not hold the run open.
    if let Some(t) = err_task.as_mut() {
        if tokio::time::timeout(std::time::Duration::from_secs(3), &mut *t).await.is_err() { t.abort(); }
    }
    let stderr = err_text.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Followed { ended, exit, stderr }
}

/// How long one sign-in attempt may stay open.
pub const LOGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Look after a sign-in attempt (`claude auth login`, waiting for the browser). There is only ever
/// one: starting another ends the one before it, and an attempt nobody finishes is ended after
/// `limit`. Only the program itself is ended, never what it opened (the browser).
pub fn watch_login(state: Arc<AppState>, mut child: tokio::process::Child, limit: std::time::Duration) {
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let previous = state.login.lock().unwrap_or_else(|e| e.into_inner()).replace(stop);
    if let Some(p) = previous { let _ = p.send(()); }
    tokio::spawn(async move {
        let replaced = tokio::select! {
            _ = child.wait() => false,
            _ = tokio::time::sleep(limit) => false,
            _ = stopped => true,
        };
        // Ended and reaped, whichever way it went (a no-op when it has already exited).
        let _ = child.kill().await;
        // A newer attempt is in charge now: it will say how things stand when it is over.
        if replaced { return; }
        state.forget_auth();
        state.emit(json!({"type": "auth.changed"}));
    });
}

/// The command line of one assistant run (everything after the program name). Built apart from
/// starting the program, so what a run is started with can be checked without starting one.
/// `mode` is always passed: a run without `--permission-mode` takes the default from the user's
/// (or the folder's) own Claude Code settings, which can be "bypass permissions".
/// `budget` is the spending ceiling, already checked against what the installed program accepts.
/// `rules` is the private file holding the run's deny rules: they are never put on the command line,
/// where their size would count against the system's limit for arguments (and anyone could read them).
fn run_args(opts: &RunOptions, mode: &str, budget: Option<&str>, resume: Option<&str>, add_dirs: &[PathBuf], rules: Option<&Path>, system: Passed) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = ["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"].iter().map(Into::into).collect();
    if let Some(f) = rules { a.push("--settings".into()); a.push(f.into()); }
    if let Some(r) = resume { a.push("--resume".into()); a.push(r.into()); }
    if let Some(m) = opts.model.as_ref().filter(|s| !s.is_empty()) { a.push("--model".into()); a.push(m.into()); }
    if let Some(e) = opts.effort.as_ref().filter(|s| !s.is_empty()) { a.push("--effort".into()); a.push(e.into()); }
    a.push("--permission-mode".into());
    a.push(mode.into());
    if let Some(b) = budget { a.push("--max-budget-usd".into()); a.push(b.into()); }
    for d in add_dirs { a.push("--add-dir".into()); a.push(d.into()); }
    match system {
        Passed::Inline(text) => { a.push("--append-system-prompt".into()); a.push(text.into()); }
        Passed::File(f) => { a.push("--append-system-prompt-file".into()); a.push(f.into()); }
    }
    a
}

/// The mode a one-shot completion is started with. It has no built-in tools, but the same rule
/// holds: never leave the choice to Claude Code's own default.
const ONE_SHOT_MODE: &str = crate::settings::DEFAULT_PERMISSION_MODE;

/// The command line of a one-shot, tool-less completion (see `complete`).
fn one_shot_args(model: Option<&str>, note: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-p", "--output-format", "json", "--no-session-persistence", "--tools", "", "--permission-mode", ONE_SHOT_MODE].iter().map(|s| s.to_string()).collect();
    if let Some(m) = model.filter(|m| !m.is_empty()) { a.push("--model".into()); a.push(m.into()); }
    a.push("--append-system-prompt".into());
    a.push(note.into());
    a
}

type OnDone = Box<dyn FnOnce(Arc<AppState>, bool) -> futures::future::BoxFuture<'static, ()> + Send>;

/// Record what is in the library as one history entry, when history is on and nothing forbids it
/// (an interrupted wrap must keep its undo point; git settings must still be the trusted ones).
/// `wait` = wait for other git work to finish; otherwise skip when git is busy.
pub(crate) async fn record_library(state: &AppState, lib: &Path, subject: &str, wait: bool) {
    if !state.settings.read().await.git_enabled() || state.library_busy() { return; }
    if crate::wraps::interrupted_wrap(state).await.is_some() { return; }
    // Every library has a history: its own repository, or the one the app keeps for it.
    if !crate::git::ready(state, lib).await { return; }
    let _g = if wait { state.git_lock.lock().await } else { match state.git_lock.try_lock() { Ok(g) => g, Err(_) => return } };
    if let Err(e) = crate::trust::require_git(lib) { crate::log::warn(format!("history: not recording ({e})")); return; }
    crate::log::ok_or_warn("history: could not record the library", crate::git::commit_all(state, lib, subject).await);
}

/// Like `record_library`, for a caller that already holds the library lock.
pub(crate) async fn record_library_locked(state: &AppState, lib: &Path, subject: &str) {
    if !state.settings.read().await.git_enabled() { return; }
    if crate::wraps::interrupted_wrap(state).await.is_some() { return; }
    if !crate::git::ready(state, lib).await { return; }
    let _g = state.git_lock.lock().await;
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
    // Who pays for this run. An address without a key stops here: nothing is recorded, nothing starts.
    let provider = provider_env(&state).await?;
    let kind = state.chats.with(&spec.chat_id, |c| c.kind.clone()).await.unwrap_or_default();
    if let Some(lib) = &library {
        // A library that holds half a wrap waits for the user's decision: anything written into it
        // now would be thrown away with the wrap if it is undone. (Finishing that wrap is the exception.)
        if spec.cwd == *lib || (kind != "customize" && spec.add_dirs.contains(lib)) {
            if let Some(cut) = crate::wraps::interrupted_wrap(&state).await.filter(|id| *id != spec.chat_id) {
                return Err(crate::tr::err("wrap.interrupted", &[("id", &cut)]));
            }
        }
        // Whatever works with the library belongs to it (see `wraps::Scope`).
        if kind != "customize" {
            let key = lib.to_string_lossy().to_string();
            state.chats.update(&spec.chat_id, |c| if c.library.is_none() { c.library = Some(key); }).await;
        }
    }
    // Every run gets an explicit mode. A run that names none uses the app's own default (accept
    // edits), never Claude Code's: that one comes from the user's or the folder's own settings and
    // can be "bypass permissions". "Ask before changes" is gone: a stored `default` means `plan`.
    let mode = crate::settings::effective_permission_mode(spec.options.permission_mode.as_deref()).to_string();
    // Skipping every permission check is something the user switches on for a chat of their own —
    // never a wrap, a workflow, a customise run or anything else the app starts itself.
    if mode == crate::settings::BYPASS && kind != "chat" {
        return Err(crate::tr::err("bypass.chat_only", &[]));
    }
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
    // A chat or workflow working in the library: the user's own pending edits are recorded first, so
    // the history shows what the assistant changed separately (and each can be undone on its own).
    let record = library.clone().filter(|l| *l == spec.cwd && matches!(kind.as_str(), "chat" | "workflow") && mode != "plan");
    if let Some(lib) = &record {
        record_library(&state, lib, &crate::git::subject::edit("your changes"), false).await;
    }
    // What the run is told on top of its prompt: the caller's own instructions, that `.claude/` is
    // not its to change (all runs but first-time setup), and the user's language.
    let sys = [spec.append_system.as_deref(), managed_note(&kind), Some(language_note(&settings.language))]
        .into_iter().flatten().collect::<Vec<_>>().join("\n\n");
    let stopped = if settings.language == "ar" { "تم الإيقاف." } else { "Stopped." };
    let id = spec.chat_id.clone();

    let hook = state.run_hook.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(hook) = hook {
        let info = RunInfo { chat_id: id.clone(), cwd: spec.cwd.clone(), prompt: spec.prompt.clone(), permission_mode: Some(mode.clone()),
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
    let budget = match spec.options.max_budget_usd.filter(|b| b.is_finite() && *b > 0.0) {
        Some(b) if has_flag(&state, "--max-budget-usd").await => Some(format!("{b:.2}")),
        Some(_) => { crate::log::warn(format!("run {}: this version of the assistant cannot limit spending; only the time limit applies", spec.chat_id)); None }
        None => None,
    };
    // The rules always travel in a private file, never on the command line: they grow with the
    // library (private folders, other chats' attachments), and one argument may only be so long.
    // npm's `claude.cmd` on Windows goes through cmd.exe, which cannot carry multi-line or
    // quote-heavy arguments at all: there the extra instructions travel as a file as well.
    let mut temp = TempFiles::default();
    let tmp_dir = crate::paths::run_tmp_dir();
    let via_files = cfg!(windows) && is_cmd_shim(&bin);
    let rules_file = match &deny {
        Some(d) if d.len() > MAX_RULES_BYTES => {
            crate::log::error(format!("run {}: not started — its rules are too large to hand over ({} bytes)", spec.chat_id, d.len()));
            anyhow::bail!("the assistant was not started: its safety rules have grown too large to hand over");
        }
        Some(d) => Some(temp.add(&tmp_dir, "settings.json", d)?),
        None => None,
    };
    let sys_file = if via_files { Some(temp.add(&tmp_dir, "system.txt", &sys)?) } else { None };
    let system = match &sys_file { Some(f) => Passed::File(f), None => Passed::Inline(&sys) };
    c.current_dir(&spec.cwd).args(run_args(&spec.options, &mode, budget.as_deref(), spec.resume.as_deref(), &spec.add_dirs, rules_file.as_deref(), system));
    provider.apply(&mut c);
    c.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = c.spawn().map_err(|e| {
        crate::log::error(format!("run {}: could not start the assistant: {e}", spec.chat_id));
        e
    })?;
    let Some(mut stdin) = child.stdin.take() else {
        if let Some(pid) = child.id() { crate::proc::kill_tree(pid); }
        anyhow::bail!("could not connect to the assistant process");
    };
    // Remember the process, so a daemon that dies uncleanly can be cleaned up after.
    let pid = child.id();
    crate::runs::record(pid, &spec.chat_id);
    let started = std::time::Instant::now();
    crate::log::info(format!("run {} started (pid {}, mode {mode}, in {})", spec.chat_id, pid.unwrap_or(0), spec.cwd.display()));
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

    let limits = run_limits(&kind);
    tokio::spawn(async move {
        // Removed when the run is over, however it ends.
        let _temp = temp;
        let (st, cid) = (state.clone(), id.clone());
        let f = follow(&mut child, &mut kill_rx, limits, move |line| {
            let (st, cid) = (st.clone(), cid.clone());
            async move {
                let ev = serde_json::from_str::<Value>(&line).ok()?;
                handle_event(&st, &cid, &ev).await
            }
        }).await;
        // The daemon is stopping (and has ended this process): the run was cut off, not finished.
        // It stays marked as running, so the next start tells the user it was interrupted.
        if state.stopping.load(std::sync::atomic::Ordering::Relaxed) { return; }
        crate::runs::forget(pid);
        let secs = started.elapsed().as_secs();
        let ok = f.ended == Ended::Result(true);
        let mins = |d: &std::time::Duration| (d.as_secs() / MINUTE.as_secs()).max(1).to_string();
        // Ended by the app for going past a limit, or because it could no longer be followed: said
        // in the chat in plain words, and as a notification where nothing else announces the failure.
        let ended_by_app = match &f.ended {
            Ended::Stalled(d) => Some(crate::tr::text("run.stalled", &[("minutes", &mins(d))])),
            Ended::TooLong(d) => Some(crate::tr::text("run.too_long", &[("minutes", &mins(d))])),
            Ended::Lost(_) => Some(crate::tr::text("run.lost", &[])),
            _ => None,
        };
        match (&f.ended, ended_by_app) {
            (_, Some(text)) => {
                crate::log::error(format!("run {id} ended by the app after {secs}s: {:?}", f.ended));
                push(&state, &id, Msg::new("error", &text)).await;
                if !matches!(kind.as_str(), "wrap" | "workflow") { state.notify(&crate::tr::text("run.ended_title", &[]), &text); }
            }
            (Ended::Stopped, _) => push(&state, &id, Msg::new("system", stopped)).await,
            (Ended::NoResult, _) if !f.stderr.trim().is_empty() => push(&state, &id, error_msg(&state, f.stderr.trim()).await).await,
            _ => {}
        }
        if ok {
            state.note_run_ok();
            crate::log::info(format!("run {id} finished ok in {secs}s"));
        } else {
            let status = f.exit.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
            let first: String = f.stderr.lines().next().unwrap_or("").chars().take(300).collect();
            crate::log::warn(format!("run {id} did not finish ({status}, {secs}s, {}){}", match f.ended {
                    Ended::Result(_) => "assistant reported an error", Ended::NoResult => "no result", Ended::Stopped => "stopped",
                    Ended::Stalled(_) => "stalled", Ended::TooLong(_) => "took too long", Ended::Lost(_) => "output lost" },
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
                // Without a `result` text the CLI lists what went wrong under `errors`.
                let listed: Vec<&str> = ev["errors"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let txt = ev["result"].as_str().map(String::from).or_else(|| (!listed.is_empty()).then(|| listed.join("\n")))
                    .unwrap_or_else(|| format!("Run ended: {}", ev["subtype"].as_str().unwrap_or("error")));
                // The CLI often echoes the error as assistant text first; show it once, as an error.
                state.chats.update(id, |c| {
                    if c.messages.last().map_or(false, |m| m.role == "assistant" && m.text.trim() == txt.trim()) { c.messages.pop(); }
                }).await;
                push(state, id, error_msg(state, &txt).await).await;
            }
            Some(!is_err)
        }
        _ => None,
    }
}

/// `tool.code` of the chat message for a run that could not resume its session.
pub const SESSION_LOST: &str = "sessionLost";

/// Did a run fail because the CLI no longer has the session it was asked to resume? (It clears
/// old sessions out, and a session belongs to the folder it ran in.)
pub fn is_session_lost(text: &str) -> bool { text.contains("No conversation found with session ID") }

#[cfg(test)]
#[path = "claude_session_tests.rs"]
mod session_tests;

/// The chat message for a failed run. A missing or expired sign-in is said in plain words and
/// marked (`tool.code = "signedOut"`) so the chat can offer to sign in; the CLI's text is kept as detail.
async fn error_msg(state: &AppState, text: &str) -> Msg {
    // The session a run was to resume is gone. Marked, so that a chat can carry on from its saved
    // conversation instead of failing on every message (`resend_without_session` in api.rs).
    if is_session_lost(text) {
        let mut m = Msg::new("error", crate::tr::text("assistant.session_lost", &[]));
        m.tool = Some(json!({"code": SESSION_LOST, "detail": text.chars().take(600).collect::<String>()}));
        return m;
    }
    match auth_failure(state, text).await {
        Some((plain, code)) => {
            let mut m = Msg::new("error", plain);
            m.tool = Some(json!({"code": code, "detail": text.chars().take(600).collect::<String>()}));
            m
        }
        None => Msg::new("error", text),
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
    let hook = state.complete_hook.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(hook) = hook { return hook(prompt.to_string(), model).await; }
    let provider = provider_env(state).await?;
    let bin = binary(state).await.ok_or_else(|| anyhow::anyhow!("Claude Code CLI not found"))?;
    crate::settings::check_cli_value("model", model.as_deref())?;
    let mut c = cmd(&bin);
    c.current_dir(crate::paths::home())
        .args(one_shot_args(model.as_deref(), language_note(&state.settings.read().await.language)));
    provider.apply(&mut c);
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
        let text = v["result"].as_str().unwrap_or("error");
        auth_failure(state, text).await;
        anyhow::bail!("{text}");
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
        // A flag is used only when the installed program lists it.
        assert!(flag_block(HELP, "--max-budget-usd").is_empty());
        assert!(!flag_block("  --max-budget-usd <amount>   Maximum dollar amount\n", "--max-budget-usd").is_empty());
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
    fn sign_in_failures_are_told_apart_from_other_errors() {
        for auth in ["Invalid API key · Please run /login", "Not logged in · Please run /login", "OAuth token has expired. Please run /login",
                     r#"API Error: 401 {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#] {
            assert!(is_auth_failure(auth), "{auth}");
        }
        for other in ["Credit balance is too low", "API Error: 529 Overloaded", "Run ended: error_max_turns", "permission denied: Edit(./.git/config)", ""] {
            assert!(!is_auth_failure(other), "{other}");
        }
        assert!(!signed_out_text().contains("::") && signed_out_text().contains("Sign in"));
    }

    #[tokio::test]
    async fn a_refused_run_marks_the_app_signed_out_until_a_run_works() {
        let st = AppState::for_test();
        let m = error_msg(&st, "Invalid API key · Please run /login").await;
        assert_eq!(m.tool.as_ref().unwrap()["code"], "signedOut");
        assert!(m.text.contains("not signed in"));
        assert!(st.auth_failed.load(std::sync::atomic::Ordering::Relaxed));
        st.note_run_ok();
        assert!(!st.auth_failed.load(std::sync::atomic::Ordering::Relaxed));
        // Any other failure is shown as it is and changes nothing.
        let m = error_msg(&st, "API Error: 529 Overloaded").await;
        assert!(m.tool.is_none() && m.text.contains("529"));
        assert!(!st.auth_failed.load(std::sync::atomic::Ordering::Relaxed));
        // With the user's own provider key, a refusal is about that key, not the Claude account:
        // said in plain words, and the app does not start asking the user to sign in.
        st.settings.write().await.ai.claude.has_api_token = true;
        let m = error_msg(&st, "Invalid API key").await;
        assert_eq!(m.tool.as_ref().unwrap()["code"], "providerKey");
        assert!(m.text.contains("key") && !m.text.contains("Invalid API key"), "{}", m.text);
        assert!(!st.auth_failed.load(std::sync::atomic::Ordering::Relaxed));
    }

    fn claude_settings(url: Option<&str>, key: bool) -> crate::settings::ClaudeSettings {
        crate::settings::ClaudeSettings { base_url: url.map(String::from), has_api_token: key, ..Default::default() }
    }

    #[test]
    fn a_key_is_sent_the_way_its_destination_takes_it_and_an_address_alone_never_runs() {
        let env = |url: Option<&str>, has: bool| provider_env_for(&provider_of(&claude_settings(url, has)), has.then(|| "sk-test".to_string()));
        let pair = |k: &'static str, v: &str| (k, v.to_string());
        // No address, no key: the Claude sign-in, nothing added and nothing taken away.
        assert_eq!(env(None, false).unwrap(), ProviderEnv::default());
        assert_eq!(env(Some("  "), false).unwrap(), ProviderEnv::default());
        assert_eq!(env(Some("https://api.anthropic.com"), false).unwrap(), ProviderEnv::default());
        // An Anthropic key with no address (or Anthropic's own): X-Api-Key, to Anthropic only.
        for url in [None, Some(""), Some("https://api.anthropic.com"), Some("https://api.anthropic.com/")] {
            let e = env(url, true).unwrap();
            assert_eq!(e.set, [pair("ANTHROPIC_API_KEY", "sk-test")], "{url:?}");
            assert!(e.unset.contains(&"ANTHROPIC_AUTH_TOKEN") && e.unset.contains(&"ANTHROPIC_BASE_URL"), "{url:?}");
        }
        // Another service: its address and a bearer token; an Anthropic key from the environment stays home.
        let e = env(Some("https://gateway.example.com/v1"), true).unwrap();
        assert_eq!(e.set, [pair("ANTHROPIC_BASE_URL", "https://gateway.example.com/v1"), pair("ANTHROPIC_AUTH_TOKEN", "sk-test")]);
        assert_eq!(e.unset, ["ANTHROPIC_API_KEY"]);
        // An address without a key (the user's Claude sign-in would go to it), or one that is not a valid address: no run.
        for (url, has) in [("https://gateway.example.com", false), ("http://localhost:4000", false), ("http://plain.example.com", true), ("gateway.example.com", true)] {
            let p = provider_of(&claude_settings(Some(url), has));
            assert_eq!(p, Provider::Incomplete, "{url}");
            assert!(provider_env_for(&p, Some("sk-test".into())).is_err(), "{url}");
        }
        // The flag says a key is saved but the password store has none: say so, do not run without it.
        assert!(provider_env_for(&Provider::AnthropicKey, None).is_err());
        assert!(provider_env_for(&Provider::Service("https://gateway.example.com".into()), Some("  ".into())).is_err());
        assert_eq!(provider_of(&claude_settings(Some("http://localhost:4000"), true)), Provider::Service("http://localhost:4000".into()));
    }

    #[tokio::test]
    async fn an_address_without_a_key_is_refused_before_anything_starts() {
        let st = Arc::new(AppState::for_test());
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = ran.clone();
        *st.run_hook.lock().unwrap() = Some(Arc::new(move |_info: RunInfo| { flag.store(true, std::sync::atomic::Ordering::SeqCst); Box::pin(async { true }) }));
        st.settings.write().await.ai.claude.base_url = Some("https://gateway.example.com".into());
        let chat = crate::chats::Chat::default();
        st.chats.insert(chat.clone()).await;
        let dir = std::env::temp_dir().join(format!("thoughts-prov-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let spec = || RunSpec { chat_id: chat.id.clone(), prompt: "hi".into(), cwd: dir.clone(), add_dirs: vec![], options: RunOptions::default(), resume: None, append_system: None, extra_deny: vec![] };
        let refused = |e: anyhow::Error| crate::tr::classify_in(false, &e);
        let e = refused(require_signed_in(&st).await.unwrap_err());
        assert_eq!(e.code, "provider_incomplete");
        assert!(e.message.contains("key") && e.message.contains("Settings"), "{}", e.message);
        assert_eq!(refused(start_run(st.clone(), spec(), None).await.unwrap_err()).code, "provider_incomplete");
        assert_eq!(refused(complete(&st, "hi", None).await.unwrap_err()).code, "provider_incomplete");
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst) && !st.chats.get(&chat.id).await.unwrap().running);
        // With the key saved for that address the same run starts, and no sign-in check is made.
        st.settings.write().await.ai.claude.has_api_token = true;
        require_signed_in(&st).await.unwrap();
        start_run(st.clone(), spec(), None).await.unwrap();
        for _ in 0..200 { if ran.load(std::sync::atomic::Ordering::SeqCst) { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
        assert!(ran.load(std::sync::atomic::Ordering::SeqCst));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A password store that lives in memory.
    #[derive(Default)]
    struct MemKeys(std::sync::Mutex<Option<String>>);
    impl KeyStore for MemKeys {
        fn get(&self) -> Option<String> { self.0.lock().unwrap().clone() }
        fn set(&self, key: &str) -> anyhow::Result<()> { *self.0.lock().unwrap() = Some(key.into()); Ok(()) }
        fn delete(&self) -> anyhow::Result<()> { *self.0.lock().unwrap() = None; Ok(()) }
    }

    #[tokio::test]
    async fn the_address_and_its_key_are_saved_together_and_never_part() {
        let st = AppState::for_test();
        let keys = MemKeys::default();
        let now = |st: &AppState| { let c = st.settings.try_read().unwrap().ai.claude.clone(); (c.base_url, c.has_api_token) };
        let code = |r: anyhow::Result<ProviderSaved>| crate::tr::classify_in(false, &r.unwrap_err()).code;
        // A key with no address: an Anthropic key, in use at once.
        let r = set_provider(&st, &keys, None, Some(" sk-ant-1 ")).await.unwrap();
        assert_eq!(r, ProviderSaved { provider: Provider::AnthropicKey, key_removed: false });
        assert_eq!((now(&st), keys.get().as_deref()), ((None, true), Some("sk-ant-1")));
        // Writing Anthropic's own address next to it is the same destination: the key stays.
        let r = set_provider(&st, &keys, Some("https://api.anthropic.com"), None).await.unwrap();
        assert_eq!((r.provider, r.key_removed, keys.get().as_deref()), (Provider::AnthropicKey, false, Some("sk-ant-1")));
        // Another service without its own key: refused, and nothing changes — the Anthropic key is not sent there.
        assert_eq!(code(set_provider(&st, &keys, Some("https://gateway.example.com"), None).await), "validation");
        assert_eq!(code(set_provider(&st, &keys, Some("https://gateway.example.com"), Some("   ")).await), "validation");
        assert_eq!((now(&st), keys.get().as_deref()), ((Some("https://api.anthropic.com".into()), true), Some("sk-ant-1")));
        // Address and key in one step.
        let r = set_provider(&st, &keys, Some(" https://gateway.example.com "), Some("gw-key")).await.unwrap();
        assert_eq!(r.provider, Provider::Service("https://gateway.example.com".into()));
        assert_eq!((now(&st), keys.get().as_deref()), ((Some("https://gateway.example.com".into()), true), Some("gw-key")));
        // The same address again with no key typed: the saved key is kept.
        let r = set_provider(&st, &keys, Some("https://gateway.example.com"), None).await.unwrap();
        assert_eq!((r.key_removed, keys.get().as_deref()), (false, Some("gw-key")));
        // Moving to a third address needs that address's key.
        assert_eq!(code(set_provider(&st, &keys, Some("https://other.example.com"), None).await), "validation");
        // An address that is not allowed is refused whatever comes with it.
        assert!(set_provider(&st, &keys, Some("http://plain.example.com"), Some("k")).await.is_err());
        assert_eq!(keys.get().as_deref(), Some("gw-key"));
        // Back to Anthropic without a key: the gateway's key is removed, not reused.
        let r = set_provider(&st, &keys, None, None).await.unwrap();
        assert_eq!(r, ProviderSaved { provider: Provider::Account, key_removed: true });
        assert_eq!((now(&st), keys.get()), ((None, false), None));
        // Removing everything.
        set_provider(&st, &keys, Some("https://gateway.example.com"), Some("gw-key")).await.unwrap();
        clear_provider(&st, &keys).await.unwrap();
        assert_eq!((now(&st), keys.get()), ((None, false), None));
        assert_eq!(provider_of(&st.settings.read().await.ai.claude), Provider::Account);
    }

    fn flag<'a>(args: &'a [std::ffi::OsString], name: &str) -> Option<&'a str> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.to_str())
    }

    #[test]
    fn every_run_is_started_with_an_explicit_permission_mode() {
        let args = |mode: Option<&str>| {
            let o = RunOptions { permission_mode: mode.map(String::from), ..Default::default() };
            run_args(&o, crate::settings::effective_permission_mode(o.permission_mode.as_deref()), None, None, &[], None, Passed::Inline("note"))
        };
        // Nothing chosen (a fresh install, an old chat, an add-on that sent no options): accept edits,
        // never "whatever Claude Code is set to".
        for unset in [None, Some("")] { assert_eq!(flag(&args(unset), "--permission-mode"), Some("acceptEdits"), "{unset:?}"); }
        assert_eq!(flag(&args(Some("default")), "--permission-mode"), Some("plan"));
        for chosen in ["acceptEdits", "plan", "bypassPermissions"] { assert_eq!(flag(&args(Some(chosen)), "--permission-mode"), Some(chosen)); }
        // The flag is there exactly once, whatever else is passed.
        let o = RunOptions { model: Some("opus".into()), effort: Some("high".into()), ..Default::default() };
        let a = run_args(&o, "plan", Some("1.50"), Some("sess-1"), &[PathBuf::from("/x/lib")], Some(Path::new("/t/rules.json")), Passed::File(Path::new("/t/system.txt")));
        assert_eq!(a.iter().filter(|x| *x == "--permission-mode").count(), 1);
        assert_eq!(flag(&a, "--max-budget-usd"), Some("1.50"));
        assert_eq!((flag(&a, "--model"), flag(&a, "--effort"), flag(&a, "--resume")), (Some("opus"), Some("high"), Some("sess-1")));
        assert_eq!((flag(&a, "--settings"), flag(&a, "--add-dir"), flag(&a, "--append-system-prompt-file")), (Some("/t/rules.json"), Some("/x/lib"), Some("/t/system.txt")));
        assert!(a.starts_with(&["-p".into(), "--output-format".into(), "stream-json".into()]));
        // One-shot completions (titles, mini-wraps) say their mode too.
        let one = one_shot_args(Some("haiku"), "note");
        let at = one.iter().position(|x| x == "--permission-mode").unwrap();
        assert_eq!(one[at + 1], "acceptEdits");
        assert!(one.windows(2).any(|w| w[0] == "--tools" && w[1].is_empty()) && one.windows(2).any(|w| w[0] == "--model" && w[1] == "haiku"));
        assert!(!one_shot_args(None, "note").contains(&"--model".to_string()));
    }

    #[tokio::test]
    async fn a_run_with_no_mode_gets_accept_edits_and_the_app_never_bypasses_on_its_own() {
        let st = Arc::new(AppState::for_test());
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Option<String>>::new()));
        let log = seen.clone();
        *st.run_hook.lock().unwrap() = Some(Arc::new(move |info: RunInfo| {
            log.lock().unwrap().push(info.permission_mode.clone());
            Box::pin(async { true })
        }));
        let dir = std::env::temp_dir().join(format!("thoughts-mode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let spec = |chat: &crate::chats::Chat, mode: Option<&str>| RunSpec { chat_id: chat.id.clone(), prompt: "hi".into(), cwd: dir.clone(), add_dirs: vec![],
            options: RunOptions { permission_mode: mode.map(String::from), ..Default::default() }, resume: None, append_system: None, extra_deny: vec![] };
        let chat = crate::chats::Chat::default();
        st.chats.insert(chat.clone()).await;
        for mode in [None, Some(""), Some("default"), Some("bypassPermissions")] {
            start_run(st.clone(), spec(&chat, mode), None).await.unwrap();
            for _ in 0..200 { if !st.chats.get(&chat.id).await.unwrap().running { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
        }
        assert_eq!(*seen.lock().unwrap(), [Some("acceptEdits".to_string()), Some("acceptEdits".into()), Some("plan".into()), Some("bypassPermissions".into())]);
        // Anything the app starts itself is refused in bypass mode, and gets accept-edits when it names none.
        let wrap = crate::chats::Chat { kind: "wrap".into(), ..Default::default() };
        st.chats.insert(wrap.clone()).await;
        assert!(start_run(st.clone(), spec(&wrap, Some("bypassPermissions")), None).await.is_err());
        start_run(st.clone(), spec(&wrap, None), None).await.unwrap();
        for _ in 0..200 { if seen.lock().unwrap().len() == 5 { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
        assert_eq!(seen.lock().unwrap().last().unwrap().as_deref(), Some("acceptEdits"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_rules_travel_in_a_private_file_that_goes_when_the_run_is_over() {
        let home = std::env::temp_dir().join(format!("thoughts-rules-{}", uuid::Uuid::new_v4()));
        let att = home.join("attachments");
        for i in 0..400 { std::fs::create_dir_all(att.join(format!("3f0c1c5e-6b1a-4c1e-9d3e-{i:012}"))).unwrap(); }
        std::fs::create_dir_all(att.join("this-chat")).unwrap();
        let lib = Path::new("/home/u/Library");
        let deny = protective_deny(lib, &[], &home, "this-chat", false);
        let h = rule_path(&home);
        // Every other chat's folder is closed for reading and for the shell; editing is closed for all of them at once.
        let other = format!("{h}/attachments/3f0c1c5e-6b1a-4c1e-9d3e-000000000007");
        assert!(deny.contains(&format!("Read({other})")) && deny.contains(&format!("Read({other}/**)")));
        assert!(deny.iter().any(|d| d.starts_with("Bash(*") && d.contains("3f0c1c5e-6b1a-4c1e-9d3e-000000000007")));
        assert!(deny.contains(&format!("Edit({h}/attachments/**)")) && !deny.contains(&format!("Edit({other}/**)")));
        assert!(!deny.iter().any(|d| d.contains("this-chat")), "this chat's own files stay readable");
        assert_eq!(deny.iter().filter(|d| d.contains("3f0c1c5e-6b1a-4c1e-9d3e-000000000007")).count(), 3);
        // 400 chats with attachments: far more than one command-line argument may carry on Linux
        // (128 KiB) — which is why the document is handed over as a file, never as an argument.
        let doc = json!({"permissions": {"deny": deny}}).to_string();
        assert!(doc.len() < MAX_RULES_BYTES);
        let tmp = home.join("run-tmp");
        let file = {
            let mut temp = TempFiles::default();
            let f = temp.add(&tmp, "settings.json", &doc).unwrap();
            assert_eq!(std::fs::read_to_string(&f).unwrap(), doc);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
                assert_eq!(std::fs::metadata(&tmp).unwrap().permissions().mode() & 0o777, 0o700);
            }
            let args = run_args(&RunOptions::default(), "acceptEdits", None, None, &[], Some(&f), Passed::Inline("note"));
            assert_eq!(flag(&args, "--settings"), f.to_str());
            assert!(args.iter().all(|a| a.len() < 4096), "nothing long is left on the command line");
            f
        };
        assert!(!file.exists(), "the rules file is removed when the run is over");
        // What a daemon that died left behind is cleared at the next start.
        std::fs::write(tmp.join("left-settings.json"), "{}").unwrap();
        assert_eq!(clear_run_tmp(&tmp), 1);
        assert_eq!(clear_run_tmp(&home.join("no-such-folder")), 0);
        // Past what a rules file can name one by one, the whole folder is closed instead of none of it.
        assert!(MAX_CLOSED_FOLDERS * 300 < MAX_RULES_BYTES);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn runs_that_cannot_write_dot_claude_are_told_so() {
        for kind in ["chat", "wrap", "workflow", "customize", "module", "reorganize", ""] {
            let note = managed_note(kind).unwrap();
            assert!(note.contains(".claude") && note.contains("Routines") && note.contains("refused"), "{kind}");
        }
        // First-time setup is the one run that may write there (it installs the presets).
        assert_eq!(managed_note("setup"), None);
        // The note matches what the rules enforce: `.claude` is closed for every kind but setup.
        let home = std::env::temp_dir().join(format!("thoughts-note-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        let lib = Path::new("/home/u/Library");
        assert!(protective_deny(lib, &[], &home, "c", false).iter().any(|d| d == "Edit(./.claude/**)"));
        assert!(!protective_deny(lib, &[], &home, "c", true).iter().any(|d| d.contains(".claude")));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn limits_depend_on_who_is_watching() {
        let secs = std::time::Duration::from_secs;
        // The user is in front of a chat and has Stop.
        for kind in ["chat", "customize"] { assert_eq!((run_limits(kind).idle, run_limits(kind).total), (None, None), "{kind}"); }
        // Automatic tasks have a time limit of their own; silence still ends them.
        assert_eq!((run_limits("workflow").idle, run_limits("workflow").total), (Some(IDLE_LIMIT), None));
        // Everything else the app runs may neither stall nor go on for ever.
        for kind in ["wrap", "reorganize", "setup", "module"] { assert_eq!((run_limits(kind).idle, run_limits(kind).total), (Some(IDLE_LIMIT), Some(TOTAL_LIMIT)), "{kind}"); }
        let l = RunLimits { idle: Some(secs(60)), total: Some(secs(600)), after_result: secs(30) };
        assert_eq!(l.exceeded(secs(100), secs(59), None), None);
        assert_eq!(l.exceeded(secs(100), secs(60), None), Some(Ended::Stalled(secs(60))));
        assert_eq!(l.exceeded(secs(600), secs(1), None), Some(Ended::TooLong(secs(600))));
        // Once the answer is in, only lingering counts — and it ends as the answer it gave.
        assert_eq!(l.exceeded(secs(900), secs(10), Some(true)), None);
        assert_eq!(l.exceeded(secs(900), secs(30), Some(true)), Some(Ended::Result(true)));
        // A watched chat is never ended for silence or length.
        assert_eq!(run_limits("chat").exceeded(secs(86_400), secs(86_400), None), None);
        assert_eq!(l.next_check(secs(100), secs(20), false), secs(40));
        assert_eq!(l.next_check(secs(590), secs(20), false), secs(10));
        assert_eq!(l.next_check(secs(590), secs(20), true), secs(10));
    }

    /// Start `script` under `sh` the way assistant runs are started (own group, piped).
    #[cfg(unix)]
    fn sh(script: &str) -> tokio::process::Child {
        let mut c = cmd(Path::new("/bin/sh"));
        c.args(["-c", script]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        c.spawn().unwrap()
    }

    #[cfg(unix)]
    fn alive(pid: u32) -> bool {
        std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_run_always_ends_stalled_too_long_stopped_or_unreadable() {
        let ms = std::time::Duration::from_millis;
        let none = RunLimits { idle: None, total: None, after_result: std::time::Duration::from_secs(60) };
        let collect = |seen: Arc<std::sync::Mutex<Vec<String>>>| move |line: String| {
            let seen = seen.clone();
            async move { let done = line.contains("\"result\""); seen.lock().unwrap().push(line); done.then_some(true) }
        };
        let lines = || Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let never = || tokio::sync::oneshot::channel::<()>();

        // An ordinary run: every line arrives, the result is reported, the exit status is its own.
        let (seen, (_keep, mut kill)) = (lines(), never());
        let mut c = sh(r#"echo '{"type":"a"}'; echo; echo '{"type":"result"}'; echo oops >&2"#);
        let f = follow(&mut c, &mut kill, none, collect(seen.clone())).await;
        assert_eq!(f.ended, Ended::Result(true));
        assert!(f.exit.unwrap().success() && f.stderr.trim() == "oops");
        assert_eq!(*seen.lock().unwrap(), [r#"{"type":"a"}"#, r#"{"type":"result"}"#]);

        // No result, and a line that is not valid text in between: the run is read to its end all the same.
        let (seen, (_keep, mut kill)) = (lines(), never());
        let mut c = sh(r#"printf '\377\376 bad\n'; echo after; exit 3"#);
        let f = follow(&mut c, &mut kill, none, collect(seen.clone())).await;
        assert_eq!((f.ended, f.exit.unwrap().code()), (Ended::NoResult, Some(3)));
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert_eq!(seen.lock().unwrap()[1], "after");

        // Silent for longer than allowed: ended, together with what it started.
        let (seen, (_keep, mut kill)) = (lines(), never());
        let mut c = sh("echo start; sleep 41 & sleep 41");
        let pid = c.id().unwrap();
        let began = std::time::Instant::now();
        let f = follow(&mut c, &mut kill, RunLimits { idle: Some(ms(300)), total: None, after_result: ms(300) }, collect(seen.clone())).await;
        assert_eq!(f.ended, Ended::Stalled(ms(300)));
        assert!(began.elapsed() < std::time::Duration::from_secs(10) && !alive(pid));
        assert!(!std::process::Command::new("pgrep").args(["-g", &pid.to_string()]).output().unwrap().status.success(), "its children were left running");

        // Busy but endless: the total limit ends it.
        let (seen, (_keep, mut kill)) = (lines(), never());
        let mut c = sh("while true; do echo tick; sleep 0.05; done");
        let f = follow(&mut c, &mut kill, RunLimits { idle: Some(ms(5000)), total: Some(ms(400)), after_result: ms(300) }, collect(seen.clone())).await;
        assert_eq!(f.ended, Ended::TooLong(ms(400)));
        assert!(!seen.lock().unwrap().is_empty());

        // It gave its answer but never exits: ended after a moment, and the answer stands.
        let (seen, (_keep, mut kill)) = (lines(), never());
        let mut c = sh(r#"echo '{"type":"result"}'; sleep 42"#);
        let f = follow(&mut c, &mut kill, RunLimits { idle: None, total: None, after_result: ms(300) }, collect(seen.clone())).await;
        assert_eq!(f.ended, Ended::Result(true));

        // The Stop button, while it is working…
        let (seen, (stop, mut kill)) = (lines(), never());
        let mut c = sh("echo working; sleep 43");
        let pid = c.id().unwrap();
        tokio::spawn(async move { tokio::time::sleep(ms(200)).await; let _ = stop.send(()); });
        let f = follow(&mut c, &mut kill, none, collect(seen.clone())).await;
        assert_eq!(f.ended, Ended::Stopped);
        assert!(!alive(pid));

        // …and when it has closed its output but is still there: Stop still ends it (it used to be ignored from here on).
        let (seen, (stop, mut kill)) = (lines(), never());
        let mut c = sh("exec >&- 2>&-; sleep 44");
        let pid = c.id().unwrap();
        let began = std::time::Instant::now();
        tokio::spawn(async move { tokio::time::sleep(ms(300)).await; let _ = stop.send(()); });
        let f = follow(&mut c, &mut kill, none, collect(seen.clone())).await;
        assert_eq!(f.ended, Ended::Stopped);
        assert!(began.elapsed() < std::time::Duration::from_secs(10) && !alive(pid));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn only_one_sign_in_attempt_is_ever_open() {
        let st = Arc::new(AppState::for_test());
        let mut events = st.events.subscribe();
        let attempt = || { let mut c = crate::proc::command("sleep"); c.arg("45").kill_on_drop(true); c.spawn().unwrap() };
        let gone = |pid: u32| async move {
            for _ in 0..100 { if !alive(pid) { return true; } tokio::time::sleep(std::time::Duration::from_millis(30)).await; }
            false
        };
        // Three clicks: each ends the attempt before it.
        let (a, b, c) = (attempt(), attempt(), attempt());
        let (pa, pb, pc) = (a.id().unwrap(), b.id().unwrap(), c.id().unwrap());
        watch_login(st.clone(), a, std::time::Duration::from_secs(60));
        watch_login(st.clone(), b, std::time::Duration::from_secs(60));
        assert!(gone(pa).await && alive(pb));
        // The last one is left alone until its time is up, then ended as well — and the app looks at the sign-in again.
        watch_login(st.clone(), c, std::time::Duration::from_millis(400));
        assert!(gone(pb).await);
        assert!(alive(pc));
        assert!(gone(pc).await);
        let ev = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv()).await.unwrap().unwrap();
        assert_eq!(ev["type"], "auth.changed");
        assert!(events.try_recv().is_err(), "an attempt that was replaced says nothing");
    }

    #[cfg(unix)]
    #[test]
    fn version_managers_are_found_through_the_interactive_shell_and_by_folder() {
        // What start-up files print around the value does not matter.
        assert_eq!(marked_path(&format!("Welcome back!\n{PATH_MARK}/a/bin:/b/bin:{PATH_MARK}\nbye")), Some(vec!["/a/bin".to_string(), "/b/bin".into()]));
        assert_eq!(marked_path("no marks at all"), None);
        assert_eq!(marked_path(&format!("{PATH_MARK}{PATH_MARK}")), None);

        let d = std::env::temp_dir().join(format!("thoughts-path-{}", uuid::Uuid::new_v4()));
        let d = crate::paths::canonical_lenient(&d);
        std::fs::create_dir_all(&d).unwrap();
        // nvm puts its folder on the PATH in ~/.zshrc, which only an interactive shell reads. With
        // start-up files of its own (ZDOTDIR), zsh shows the difference: a plain login shell never
        // sees the folder, the way the PATH is asked for now does.
        if Path::new("/bin/zsh").exists() {
            let nvm_bin_dir = d.join("nvm/versions/node/v22.3.0/bin");
            std::fs::write(d.join(".zshrc"), format!("echo 'a greeting printed by the start-up file'\nexport PATH=\"{}:$PATH\"\n", nvm_bin_dir.display())).unwrap();
            let zdot = d.to_string_lossy().to_string();
            let mut plain = crate::proc::std_command("/bin/zsh");
            plain.args(["-lc", "printf %s \"$PATH\""]).env("ZDOTDIR", &zdot);
            let plain = String::from_utf8_lossy(&crate::proc::std_output_timeout(&mut plain, SHELL_TIMEOUT).unwrap().stdout).to_string();
            assert!(!plain.contains(&*nvm_bin_dir.to_string_lossy()), "a login shell that is not interactive does not read .zshrc");
            let found = terminal_path("/bin/zsh", &[("ZDOTDIR", &zdot)]);
            assert_eq!(found.first().map(String::as_str), nvm_bin_dir.to_str(), "{found:?}");
            assert!(found.iter().all(|p| !p.contains("greeting")));
        }
        // A shell that cannot be asked gives nothing (and does not hang).
        assert!(terminal_path("/nonexistent/shell", &[]).is_empty());

        // Should the shell not tell: nvm's default version (or its newest), and the shim folders that exist.
        let (home, nvm) = (d.join("home"), d.join("home/.nvm"));
        for v in ["v18.19.0", "v20.11.1", "v20.9.0", "v22.3.0", "not-a-version"] { std::fs::create_dir_all(nvm.join("versions/node").join(v).join("bin")).unwrap(); }
        assert_eq!(nvm_bin(&nvm), Some(nvm.join("versions/node/v22.3.0/bin")));
        std::fs::create_dir_all(nvm.join("alias")).unwrap();
        for (alias, want) in [("20\n", "v20.11.1"), ("v18.19.0", "v18.19.0"), ("lts/*", "v22.3.0"), ("node", "v22.3.0"), ("16", "v22.3.0")] {
            std::fs::write(nvm.join("alias/default"), alias).unwrap();
            assert_eq!(nvm_bin(&nvm), Some(nvm.join("versions/node").join(want).join("bin")), "{alias}");
        }
        assert_eq!(nvm_bin(&d.join("no-nvm-here")), None);
        std::fs::create_dir_all(home.join(".asdf/shims")).unwrap();
        std::fs::create_dir_all(home.join(".local/share/fnm/aliases/default/bin")).unwrap();
        let dirs = version_manager_dirs(&home, &nvm);
        assert_eq!(dirs, [nvm.join("versions/node/v22.3.0/bin"), home.join(".local/share/fnm/aliases/default/bin"), home.join(".asdf/shims")]);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn language_note_follows_setting() {
        assert!(language_note("ar").contains("Arabic"));
        assert!(language_note("en").contains("English"));
    }
}
