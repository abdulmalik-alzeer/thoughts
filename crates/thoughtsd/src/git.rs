//! Git via the system `git` binary. Credentials are served through GIT_ASKPASS
//! (this same executable in askpass mode, asking the daemon, which reads the OS keychain) or a
//! `git-credentials` file outside the library.
//!
//! Every library has a history. A library that is a repository itself (it has its own `.git`,
//! the usual arrangement for one that syncs with an online location) keeps it there. Any other
//! library — "this computer only", or one inside iCloud Drive or Dropbox — has its history kept by
//! the app under `~/.thoughts/history/`, outside the folder, and nothing is added to the folder
//! itself (see `Repo`). Either way every command below works on exactly that repository and never
//! on one further up the folder tree.

use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Longest a git command that only touches this machine may run.
pub const LOCAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
/// Longest a git command that talks to a remote may run.
pub const NETWORK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Where the git subcommand sits in `args` (after any leading `-c key=value` pairs).
fn subcommand_at(args: &[&str]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-c" { i += 2; continue; }
        return Some(i);
    }
    None
}

/// The git subcommand in `args` (skipping leading `-c key=value` pairs).
fn subcommand<'a>(args: &[&'a str]) -> &'a str { subcommand_at(args).map_or("", |i| args[i]) }

/// How long `args` may take: commands that reach a remote get longer than local ones.
pub fn timeout_for(args: &[&str]) -> std::time::Duration {
    if matches!(subcommand(args), "push" | "pull" | "fetch" | "clone" | "ls-remote") { NETWORK_TIMEOUT } else { LOCAL_TIMEOUT }
}

/// `credential.helper` value for the private password file. The value is read by a shell, so the
/// path is quoted, and written with forward slashes (which Git for Windows needs).
pub fn credential_helper(file: &Path) -> String {
    let p = file.to_string_lossy().replace('\\', "/");
    let mut quoted = String::new();
    for c in p.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') { quoted.push('\\'); }
        quoted.push(c);
    }
    format!("store --file=\"{quoted}\"")
}

/// Where git finds the repository of a folder the app works in.
#[derive(Debug, Clone, PartialEq)]
pub enum Repo {
    /// The folder is a repository itself: it has its own `.git` (a library set up the usual git
    /// way, the customisation layer, a working copy).
    Own,
    /// The app keeps the folder's history outside it, in this folder (see `history_store`).
    Kept(PathBuf),
    /// Nothing has been recorded for the folder yet.
    Missing,
}

/// Where the app keeps the history of a folder that carries no repository of its own:
/// `<app home>/history/<folder name>-<digest of its full path>`. Outside the folder on purpose: a
/// cloud drive that syncs the library (iCloud, Dropbox…) must never see git's own files, which it
/// would damage, and a library that sits inside someone else's repository must not become part of
/// that repository's history.
pub fn history_store(dir: &Path) -> PathBuf {
    use sha2::Digest;
    let real = crate::paths::canonical_lenient(dir);
    let name: String = real.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        .chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(32).collect();
    let hex: String = sha2::Sha256::digest(real.to_string_lossy().as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
    crate::paths::history_dir().join(format!("{}-{hex}", if name.is_empty() { "library" } else { name.as_str() }))
}

/// The library that was in folder `from` is now in folder `to` (moved or renamed, and chosen again):
/// the history the app keeps for it, which is filed under the folder's place, goes with it, and so
/// does what is known about its backups. (A library's own `.git` moved with the folder by itself.)
/// Nothing changes when `to` already has a history, or when `from` had none kept by the app.
/// Returns whether a history was moved.
pub async fn follow_move(state: &AppState, from: &Path, to: &Path) -> anyhow::Result<bool> {
    let _g = state.git_lock.lock().await;
    let (old, new) = (history_store(from), history_store(to));
    if old == new || !old.join("HEAD").is_file() || repo_of(to) != Repo::Missing || std::fs::symlink_metadata(&new).is_ok() { return Ok(false); }
    std::fs::rename(&old, &new)?;
    let noted = backup_state().get(&backup_key(from)).and_then(Value::as_object).cloned().filter(|e| !e.is_empty());
    if let Some(e) = noted { change_backup_state(to, |n| if n.is_empty() { *n = e; }); }
    crate::log::info("history: the library's folder moved; its history went with it");
    Ok(true)
}

/// Which repository belongs to `dir`: its own when it has one, else the one the app keeps for it.
pub fn repo_of(dir: &Path) -> Repo {
    if std::fs::symlink_metadata(dir.join(".git")).is_ok() { return Repo::Own; }
    let store = history_store(dir);
    if store.join("HEAD").is_file() { Repo::Kept(store) } else { Repo::Missing }
}

/// The repository `dir` sits inside without being one itself (a notes vault, a dotfiles folder…):
/// the nearest folder above it that has a `.git`. The app never touches that repository; it only
/// needs to know, so that it does not plant a second repository in the middle of it.
pub fn enclosing_repo(dir: &Path) -> Option<PathBuf> {
    let real = crate::paths::canonical_lenient(dir);
    real.ancestors().skip(1).find(|a| std::fs::symlink_metadata(a.join(".git")).is_ok()).map(Path::to_path_buf)
}

/// Files a history the app keeps never records: what the system and cloud drives scatter around.
/// (A library's own `.gitignore` files are honoured as well.)
const KEPT_EXCLUDE: &str = "# Written by Thoughts.\n.DS_Store\nThumbs.db\ndesktop.ini\n.*.icloud\n.dropbox\n.dropbox.cache/\n.dropbox.attr\n.~lock.*\n~$*\n.*.tmp\n";

/// The first line of the rule that keeps meeting recordings (the sound files in any `meetings/`
/// folder, FEAT-55) out of a library's history, and so out of every online backup: only the
/// transcript note beside a recording is recorded. Found by this line, so it is added only once.
const RECORDINGS_MARK: &str = "# Thoughts: meeting recordings stay on this computer (never in the history, never sent to a backup).";

fn recordings_rule() -> String {
    let mut s = format!("{RECORDINGS_MARK}\n");
    for ext in crate::voice::AUDIO_EXT { s.push_str(&format!("**/meetings/*.{ext}\n")); }
    s
}

/// Make sure the history of `dir` leaves meeting recordings out. The rule goes into that history's
/// own list of files to leave out (`info/exclude`, inside the repository: never a `.gitignore`, which
/// is the user's and would travel with the library), for the history the app keeps and for a
/// repository of the library's own alike. It only stops files from being added: a recording that
/// some earlier version already recorded stays recorded. Called before anything is added to the
/// history; once the rule is there this is one read of a small file.
pub async fn keep_recordings_out(state: &AppState, dir: &Path) {
    let file = match repo_of(dir) {
        Repo::Kept(store) => store.join("info/exclude"),
        // The usual repository; a `.git` file (a linked working copy) names where git keeps it.
        Repo::Own if dir.join(".git").is_dir() => dir.join(".git/info/exclude"),
        Repo::Own => match git(state, dir, &["rev-parse", "--git-path", "info/exclude"]).await {
            Ok(p) if !p.trim().is_empty() => { let p = PathBuf::from(p.trim()); if p.is_absolute() { p } else { dir.join(p) } }
            _ => return,
        },
        Repo::Missing => return,
    };
    let had = std::fs::read_to_string(&file).unwrap_or_default();
    if had.lines().any(|l| l.trim() == RECORDINGS_MARK) { return; }
    let sep = if had.is_empty() || had.ends_with('\n') { "" } else { "\n" };
    crate::log::ok_or_warn("history: could not keep meeting recordings out",
        file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&file, format!("{had}{sep}{}", recordings_rule()))));
}

#[cfg(test)]
thread_local! {
    /// Set by a test to behave, on its own thread only, like a computer that has no git.
    pub(crate) static TEST_NO_GIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Set by a test to behave, on its own thread only, like a computer where git was never told
    /// who the user is and cannot guess it (no settings of the user's, no name made up).
    pub(crate) static TEST_NO_IDENTITY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Is there a git that can really run here? On a Mac without Apple's command-line tools
/// `/usr/bin/git` is only a placeholder: running it does nothing useful and makes macOS ask to
/// install the developer tools — every time. So the app looks first and, when git is not there,
/// says so plainly (`git.missing`) instead of running the placeholder. A yes is remembered while
/// the app runs; a no is looked at again after a few seconds, so installing git is noticed.
pub fn usable() -> bool {
    #[cfg(test)]
    if TEST_NO_GIT.with(|c| c.get()) { return false; }
    #[cfg(target_os = "macos")]
    {
        use std::time::{Duration, Instant};
        static SEEN: std::sync::Mutex<Option<(bool, Instant)>> = std::sync::Mutex::new(None);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        match *seen {
            Some((true, _)) => return true,
            Some((false, at)) if at.elapsed() < Duration::from_secs(5) => return false,
            _ => {}
        }
        // A git from somewhere else (Homebrew, MacPorts…) is the real thing; Apple's needs its tools installed.
        let own = ["/opt/homebrew/bin/git", "/usr/local/bin/git"].iter().any(|p| Path::new(p).exists())
            || crate::claude::find_binary(&None, "git").map_or(false, |p| p != Path::new("/usr/bin/git"));
        let yes = own || crate::proc::std_output_timeout(crate::proc::std_command("xcode-select").arg("-p"), Duration::from_secs(15)).map_or(false, |o| o.status.success());
        *seen = Some((yes, Instant::now()));
        yes
    }
    #[cfg(not(target_os = "macos"))]
    { true }
}

/// Run git in `dir` and return its raw output. Hardened against a library's own configuration, never
/// waits for a terminal, and never runs longer than `timeout_for(args)`: a stalled command is killed
/// (with any helper it started) and reported as a plain error, so nothing stays locked behind it.
pub async fn git_raw(state: &AppState, dir: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    if !dir.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", &dir.display().to_string())])); }
    run(state, dir, repo_of(dir), args).await
}

/// How git treats line endings in `repo` (`core.autocrlf`; `None` leaves it to git's settings).
/// A history the app keeps is only ever written and read by the app: every file is recorded and
/// given back exactly as it was, Windows line endings included, whatever this computer's git
/// settings say. A repository of the folder's own may have been checked out by Git for Windows,
/// which turns line-ending conversion on in its system settings (ignored here, see `run`): there
/// "input" keeps such a library from looking entirely changed. It compares and saves with plain
/// line endings and never rewrites a file on disk — but a file saved with Windows line endings
/// then comes back from that history with plain ones.
fn line_endings(repo: &Repo, windows: bool) -> Option<&'static str> {
    match repo {
        Repo::Kept(_) => Some("false"),
        Repo::Own | Repo::Missing => windows.then_some("input"),
    }
}

/// `git_raw` on a repository named by the caller (creating one is the only time it is not `repo_of`).
async fn run(state: &AppState, dir: &Path, repo: Repo, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let out = run_with(state, dir, repo, args, None).await?;
    if !out.status.success() {
        anyhow::bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

/// `git` reading and writing its own index file `index` instead of the repository's: a snapshot of
/// the folder can be taken this way while the repository's index is damaged or in the middle of a
/// merge, without changing it.
pub async fn git_with_index(state: &AppState, dir: &Path, index: &Path, args: &[&str]) -> anyhow::Result<String> {
    if !dir.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", &dir.display().to_string())])); }
    let out = run_with(state, dir, repo_of(dir), args, Some(index)).await?;
    if !out.status.success() {
        anyhow::bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// `git` for a command whose answer is in its output whether or not it succeeds (`merge-tree`
/// lists the clashing files and exits with 1): `(succeeded, output)`.
pub async fn git_output(state: &AppState, dir: &Path, args: &[&str]) -> anyhow::Result<(bool, String)> {
    if !dir.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", &dir.display().to_string())])); }
    let out = run_with(state, dir, repo_of(dir), args, None).await?;
    Ok((out.status.success(), String::from_utf8_lossy(&out.stdout).to_string()))
}

async fn run_with(state: &AppState, dir: &Path, repo: Repo, args: &[&str], index: Option<&Path>) -> anyhow::Result<std::process::Output> {
    run_opts(state, dir, repo, args, index, false, true).await
}

/// Longest a background check (`git_quiet`) may run.
pub const QUIET_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

/// `git` for work nobody asked for right now (the background check for changes from other
/// computers, `incoming.rs`): it can never ask anything. SSH runs in batch mode (no passphrase or host
/// question), no other program's prompt is used, and it gives up sooner. By default no password prompt
/// helper is given (so the system's password store is not read, which could show a dialog of its own). A sign-in that is needed
/// simply makes it fail. A password kept in the private file is still used: reading a file shows nothing.
/// With `saved_sign_in`, the app's own saved answer is given through the prompt helper as a Sync gives
/// it (`askpass`, which only ever answers from what is saved and shows nothing) — for a caller that
/// knows reading it cannot make the system ask either (see `incoming::may_use_saved_sign_in`).
pub async fn git_quiet(state: &AppState, dir: &Path, args: &[&str], saved_sign_in: bool) -> anyhow::Result<String> {
    if !dir.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", &dir.display().to_string())])); }
    let out = run_opts(state, dir, repo_of(dir), args, None, true, saved_sign_in).await?;
    if !out.status.success() {
        anyhow::bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Make `c` a command that never stops to ask: SSH in batch mode (later `-c` values win), no prompt
/// helper of SSH's, no interactive credential manager. (`GIT_ASKPASS` is taken away too; `run_opts`
/// gives the app's own back only when `askpass_given` says so.)
fn make_quiet(c: &mut tokio::process::Command) {
    c.args(["-c", "core.sshCommand=ssh -o BatchMode=yes -o ConnectTimeout=20"]);
    for var in ["GIT_ASKPASS", "SSH_ASKPASS"] { c.env_remove(var); }
    c.env("SSH_ASKPASS_REQUIRE", "never").env("GCM_INTERACTIVE", "never");
}

/// Is the app's prompt helper given to this command? Not with the private password file (git reads
/// it itself), and for a quiet command only when the caller allows the saved sign-in.
pub fn askpass_given(store: &str, quiet: bool, saved_sign_in: bool) -> bool { store != "file" && (!quiet || saved_sign_in) }

async fn run_opts(state: &AppState, dir: &Path, repo: Repo, args: &[&str], index: Option<&Path>, quiet: bool, saved_sign_in: bool) -> anyhow::Result<std::process::Output> {
    if !usable() { return Err(crate::tr::err("git.missing", &[])); }
    let store = state.settings.read().await.sync.credential_store.clone();
    let mut c = crate::proc::command(crate::claude::find_binary(&None, "git").unwrap_or_else(|| "git".into()));
    c.current_dir(dir).env("PATH", crate::claude::shell_path()).env("GIT_TERMINAL_PROMPT", "0");
    // Which repository a command works on is decided here and nowhere else — never by whatever the
    // app itself was started with, and never by searching upwards: a library inside another
    // repository (a vault, a dotfiles folder) is not that repository, so the search stops at `dir`.
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR", "GIT_OBJECT_DIRECTORY", "GIT_CEILING_DIRECTORIES"] { c.env_remove(var); }
    let autocrlf = line_endings(&repo, cfg!(windows));
    match repo {
        Repo::Kept(kept) => { c.env("GIT_DIR", kept).env("GIT_WORK_TREE", dir); }
        Repo::Own | Repo::Missing => if let Some(parent) = crate::paths::canonical_lenient(dir).parent() { c.env("GIT_CEILING_DIRECTORIES", parent); },
    }
    if let Some(index) = index { c.env("GIT_INDEX_FILE", index); }
    // Never run code a library's own git config could smuggle in (hooks, fsmonitor, ssh command).
    #[cfg(windows)] let no_hooks = "NUL";
    #[cfg(not(windows))] let no_hooks = "/dev/null";
    c.env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_ATTR_NOSYSTEM", "1");
    for kv in ["core.fsmonitor=false", &format!("core.hooksPath={no_hooks}"), "core.sshCommand=ssh",
               "core.askPass=", "core.pager=cat", "core.editor=true", "sequence.editor=true",
               "commit.gpgSign=false", "tag.gpgSign=false", "gpg.program=gpg", "protocol.ext.allow=never", "core.gitProxy=",
               "credential.helper=",
               // File names are shown as they are. Without this git prints every name that is not
               // plain ASCII (Arabic, accented letters…) as a quoted string of octal escapes.
               "core.quotePath=false",
               // A transfer that crawls below 1 kB/s for 30 seconds is given up on rather than left hanging.
               "http.lowSpeedLimit=1000", "http.lowSpeedTime=30"] {
        c.args(["-c", kv]);
    }
    if quiet { make_quiet(&mut c); }
    #[cfg(test)]
    if TEST_NO_IDENTITY.with(|c| c.get()) {
        c.env("GIT_CONFIG_GLOBAL", no_hooks).args(["-c", "user.useConfigOnly=true"]);
        for var in ["GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL", "GIT_COMMITTER_NAME", "GIT_COMMITTER_EMAIL", "EMAIL"] { c.env_remove(var); }
    }
    if let Some(v) = autocrlf {
        c.args(["-c", &format!("core.autocrlf={v}")]);
    }
    // On Linux a machine without a keyring keeps passwords in the private file instead (see `save_credential`).
    let file_fallback = cfg!(target_os = "linux") && crate::paths::git_credentials_file().exists();
    if store == "file" || file_fallback {
        c.args(["-c", &format!("credential.helper={}", credential_helper(&crate::paths::git_credentials_file()))]);
    }
    // The prompt helper is answered only while this command runs, and only with this command's
    // code (see `askpass`); the code is forgotten when `ticket` goes, however the command ends.
    let ticket = crate::askpass::Ticket::new();
    if askpass_given(&store, quiet, saved_sign_in) {
        if let Ok(exe) = crate::paths::app_exe() {
            c.env("GIT_ASKPASS", exe).env(crate::askpass::CODE_VAR, ticket.code())
                .env(crate::askpass::PORT_VAR, crate::approvals::port().to_string());
        }
    }
    // A comparison is always made by git itself, never handed to a program named in some
    // configuration. (Blanking `diff.external` instead does not work: git then tries to start a
    // program with no name, and every `git diff` fails.)
    match subcommand_at(args).filter(|i| args[*i] == "diff") {
        Some(i) => { c.args(&args[..=i]).arg("--no-ext-diff").args(&args[i + 1..]); }
        None => { c.args(args); }
    }
    c.stdin(std::process::Stdio::null());
    let limit = if quiet { QUIET_TIMEOUT } else { timeout_for(args) };
    let out = match crate::proc::output_timeout(&mut c, limit).await {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut && quiet => {
            crate::log::warn(format!("git {} (a background check) did not finish within {}s and was stopped", subcommand(args), limit.as_secs()));
            return Err(crate::tr::err("git.timeout", &[]));
        }
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            crate::log::error(format!("git {} did not finish within {}s and was stopped", subcommand(args), limit.as_secs()));
            return Err(crate::tr::err("git.timeout", &[]));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(crate::tr::err("git.missing", &[])),
        Err(e) => return Err(e.into()),
    };
    drop(ticket);
    Ok(out)
}

/// Is `key` (as `git config --list` prints it, any case) one that makes git start a program of the
/// configuration's choosing: a filter, a merge driver, a diff converter, an editor or pager, a
/// credential helper, a signing program and the like? `value` lets git-lfs's own standard filter through.
pub fn runs_program(key: &str, value: &str) -> bool {
    let k = key.to_ascii_lowercase();
    let parts: Vec<&str> = k.split('.').collect();
    let (first, last) = (parts[0], *parts.last().unwrap_or(&""));
    let sub = parts.len() >= 3;
    if first == "filter" && sub && matches!(last, "clean" | "smudge" | "process") {
        // `git lfs install --local` writes exactly these.
        let lfs = parts.len() == 3 && parts[1] == "lfs" && matches!((last, value.trim()), ("clean", "git-lfs clean -- %f") | ("smudge", "git-lfs smudge -- %f") | ("process", "git-lfs filter-process"));
        return !lfs;
    }
    if first == "merge" && sub && last == "driver" { return true; }
    if first == "diff" && ((sub && matches!(last, "textconv" | "command")) || k == "diff.external") { return true; }
    if first == "credential" && last == "helper" { return !value.trim().is_empty(); }
    if first == "gpg" && last == "program" { return true; }
    if first == "pager" { return true; }
    if first == "remote" && sub && matches!(last, "uploadpack" | "receivepack") { return true; }
    let off = matches!(value.trim().to_ascii_lowercase().as_str(), "false" | "no" | "off" | "0" | "");
    match k.as_str() {
        "commit.gpgsign" | "tag.gpgsign" | "rebase.gpgsign" | "push.gpgsign" | "core.fsmonitor" => !off,
        "core.editor" | "sequence.editor" | "core.pager" | "core.sshcommand" | "core.hookspath" | "core.askpass" | "core.gitproxy"
        | "core.alternaterefscommand" | "uploadpack.packobjectshook" | "sendemail.smtpserver" => true,
        _ => false,
    }
}

/// The settings of the repository's own configuration (`.git/config`, read from `dir`, a worktree's
/// too) that make git start a program of its choosing (`runs_program`), or that make git read
/// settings from somewhere else, which differ by worktree and branch and could not be checked here
/// (`include.path`, `includeIf.<condition>.path`, `extensions.worktreeConfig`, a `config.worktree`
/// file of any worktree). In order, each once. Nothing is followed: what git would include is refused, not read.
pub async fn program_keys(state: &AppState, dir: &Path) -> anyhow::Result<Vec<String>> {
    let out = git(state, dir, &["config", "--local", "--no-includes", "--list", "-z"]).await?;
    let mut keys: Vec<String> = vec![];
    let mut add = |k: &str| if !keys.iter().any(|x| x.eq_ignore_ascii_case(k)) { keys.push(k.to_string()); };
    for rec in out.split('\0').filter(|r| !r.is_empty()) {
        let (k, v) = rec.split_once('\n').unwrap_or((rec, ""));
        let l = k.to_ascii_lowercase();
        let elsewhere = l == "include.path" || (l.starts_with("includeif.") && l.ends_with(".path")) || l == "extensions.worktreeconfig";
        if elsewhere || runs_program(k, v) { add(k); }
    }
    // A worktree's own settings file, whether or not the extension is on now.
    if let Ok(common) = git(state, dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).await {
        let common = std::path::PathBuf::from(common.trim());
        let mut files = vec![common.join("config.worktree")];
        if let Ok(rd) = std::fs::read_dir(common.join("worktrees")) { files.extend(rd.flatten().take(1000).map(|e| e.path().join("config.worktree"))); }
        if files.iter().any(|f| std::fs::symlink_metadata(f).is_ok()) { add("config.worktree"); }
    }
    Ok(keys)
}

pub async fn git(state: &AppState, dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    Ok(String::from_utf8_lossy(&git_raw(state, dir, args).await?).to_string())
}

/// Like `git`, for commands that create commits (commit, merge, pull): when this machine has no
/// git identity configured, they run as the app. A commit or a merge refuses before it changes
/// anything, so it is simply run again as the app. A pull that replays the library's changes on
/// top of the other devices' would stop halfway instead, and could not be run again: for it the
/// question is asked first.
pub async fn git_ident(state: &AppState, dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let mut as_app = vec!["-c", "user.name=Thoughts", "-c", "user.email=thoughts@localhost"];
    as_app.extend_from_slice(args);
    if !matches!(subcommand(args), "commit" | "merge") && git(state, dir, &["var", "GIT_COMMITTER_IDENT"]).await.is_err() {
        return git(state, dir, &as_app).await;
    }
    match git(state, dir, args).await {
        Err(e) if e.to_string().contains("tell me who you are") => git(state, dir, &as_app).await,
        r => r,
    }
}

/// Does `dir` have a repository — its own, or the history the app keeps for it? Never true merely
/// because a folder further up is a repository.
pub async fn is_repo(state: &AppState, dir: &Path) -> bool {
    git(state, dir, &["rev-parse", "--git-dir"]).await.is_ok()
}

/// Make `dir` a repository of its own (`.git` inside it), unless it already has a history. Only
/// for a library the user asked to sync the usual git way; everything else gets `ensure_history`.
pub async fn init(state: &AppState, dir: &Path) -> anyhow::Result<()> {
    if !is_repo(state, dir).await {
        git(state, dir, &["init", "-b", "main"]).await?;
    }
    Ok(())
}

/// Give `dir` a history when it has none: an empty one kept by the app (see `history_store`).
/// Nothing in `dir` is created, changed or removed. Returns whether a history was created.
pub async fn ensure_history(state: &AppState, dir: &Path) -> anyhow::Result<bool> {
    if repo_of(dir) != Repo::Missing { return Ok(false); }
    if !dir.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", &dir.display().to_string())])); }
    let _g = state.git_lock.lock().await;
    if repo_of(dir) != Repo::Missing { return Ok(false); }
    let store = history_store(dir);
    crate::paths::create_private_dir(&store)?;
    let kept = Repo::Kept(store.clone());
    run(state, dir, kept.clone(), &["init", "-q", "-b", "main"]).await?;
    // The folder is named on every command instead, so the history never points at a stale place.
    let _ = run(state, dir, kept, &["config", "--unset", "core.worktree"]).await;
    crate::log::ok_or_warn("history: could not write the list of files to leave out",
        std::fs::create_dir_all(store.join("info")).and_then(|_| std::fs::write(store.join("info/exclude"), format!("{KEPT_EXCLUDE}{}", recordings_rule()))));
    crate::log::info("history: created a history for the library, kept by the app");
    Ok(true)
}

/// Make sure `dir` has a history with its starting point recorded: the app-kept one is created
/// when the folder has none, and what is in the folder right now becomes the first entry (so the
/// first real change can be undone). Fails when no history can be kept (git is not installed, the
/// folder's own repository is damaged).
pub async fn start_history(state: &AppState, dir: &Path) -> anyhow::Result<()> {
    ensure_history(state, dir).await?;
    keep_recordings_out(state, dir).await;
    // The first entry is only ever made in a history the app keeps: what goes into a repository
    // the user set up themselves is decided by what they do.
    if matches!(repo_of(dir), Repo::Kept(_)) && head(state, dir).await.is_none() {
        let _g = state.git_lock.lock().await;
        if head(state, dir).await.is_none() && commit_all(state, dir, &subject::start()).await? {
            crate::log::info("history: recorded the library as it is now as the starting point");
        }
    }
    // A plain question that fails with the real reason when there is no usable repository.
    git(state, dir, &["rev-parse", "--git-dir"]).await.map(|_| ())
}

/// Is there a history to record into? (Creates the app-kept one on first use; see `start_history`.)
/// Must not be called while holding `git_lock`.
pub async fn ready(state: &AppState, dir: &Path) -> bool {
    match start_history(state, dir).await {
        Ok(()) => true,
        Err(e) => { crate::log::warn(format!("history: not available ({e})")); false }
    }
}

pub async fn commit_all(state: &AppState, dir: &Path, msg: &str) -> anyhow::Result<bool> {
    // Never into a combine of changes that stopped halfway: the half-done state (clash marks and
    // all) would become an entry of the history. Repair history settles it first.
    if crate::repair::unfinished(dir).is_some() { return Err(crate::tr::err("history.unfinished", &[])); }
    keep_recordings_out(state, dir).await;
    git(state, dir, &["add", "-A"]).await?;
    if git(state, dir, &["diff", "--cached", "--quiet"]).await.is_ok() {
        return Ok(false);
    }
    git_ident(state, dir, &["commit", "-q", "-m", msg]).await?;
    Ok(true)
}

/// Like `commit_all`, for only `paths` (library-relative files or folders, ones just removed too):
/// every other change is left for an entry of its own. A path that cannot be added (one that is in
/// neither the folder nor the history, or one the history leaves out) is passed over. Returns whether
/// an entry was made.
pub async fn commit_paths(state: &AppState, dir: &Path, msg: &str, paths: &[String]) -> anyhow::Result<bool> {
    if crate::repair::unfinished(dir).is_some() { return Err(crate::tr::err("history.unfinished", &[])); }
    keep_recordings_out(state, dir).await;
    let mut specs: Vec<String> = vec![];
    for p in paths {
        let p = p.trim().replace('\\', "/");
        let p = p.trim_matches('/');
        if p.is_empty() { continue; }
        let spec = format!(":(top,literal){p}");
        if !specs.contains(&spec) && git(state, dir, &["add", "-A", "--", &spec]).await.is_ok() { specs.push(spec); }
    }
    if specs.is_empty() { return Ok(false); }
    // Exactly the files whose change was just added, so the entry never takes in anything else.
    let mut changed = vec!["diff", "--cached", "--name-only", "-z", "--no-renames", "--"];
    changed.extend(specs.iter().map(String::as_str));
    let names: Vec<String> = git(state, dir, &changed).await?.split('\0').filter(|n| !n.is_empty()).map(|n| format!(":(top,literal){n}")).collect();
    if names.is_empty() { return Ok(false); }
    let mut commit = vec!["commit", "-q", "-m", msg, "--"];
    commit.extend(names.iter().map(String::as_str));
    git_ident(state, dir, &commit).await?;
    Ok(true)
}

/// The commit HEAD points at, or None in a repository without commits.
pub async fn head(state: &AppState, dir: &Path) -> Option<String> {
    git(state, dir, &["rev-parse", "--verify", "-q", "HEAD"]).await.ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub async fn current_branch(state: &AppState, dir: &Path) -> anyhow::Result<String> {
    // symbolic-ref also answers in a repository that has no commits yet.
    match git(state, dir, &["symbolic-ref", "--short", "-q", "HEAD"]).await {
        Ok(b) if !b.trim().is_empty() => Ok(b.trim().to_string()),
        _ => Err(crate::tr::err("git.not_branch", &[])),
    }
}

/// Branches the app creates for its own bookkeeping. They never leave this machine.
pub fn internal_branch(name: &str) -> bool {
    name == "staging" || name.starts_with("reorganize/") || name.starts_with("backup/") || name.starts_with("wrap/")
}

/// Files with unresolved conflicts in a stopped merge or rebase.
pub async fn unmerged(state: &AppState, dir: &Path) -> Vec<String> {
    git(state, dir, &["diff", "--name-only", "--diff-filter=U", "-z"]).await.unwrap_or_default()
        .split('\0').filter(|f| !f.is_empty()).map(String::from).collect()
}

/// The main location among `names` (in the order they were added): the one the user chose
/// (`sync.mainLocation`), while it still exists; otherwise the first one added. Never a matter of
/// how the names happen to sort.
pub fn pick_main<'a>(names: &[&'a str], chosen: Option<&str>) -> Option<&'a str> {
    chosen.and_then(|c| names.iter().copied().find(|n| *n == c)).or_else(|| names.first().copied())
}

/// Is `url` a folder rather than an address on a network: a backup kept in a folder on this
/// computer, an external drive or a network share (see `add_folder_location`)?
pub fn is_folder_url(url: &str) -> bool {
    let b = url.as_bytes();
    url.starts_with('/') || url.starts_with("\\\\") || url.starts_with("file://")
        || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

/// What kind of place a location's address names: `folder` (a drive or folder, see
/// `is_folder_url`), `ssh` (`ssh://…` or `git@host:path`, which signs in with this computer's key)
/// or `online` (an `https://` address, signing in with a password or access token — or `http://`,
/// which only a library's own settings can name and which is never sent a password).
pub fn location_kind(url: &str) -> &'static str {
    if is_folder_url(url) { "folder" }
    else if url.starts_with("ssh://") || (!url.contains("://") && url.contains(':')) { "ssh" }
    else { "online" }
}

/// The folder a `folder` location's backup is kept in.
fn folder_path(url: &str) -> PathBuf { PathBuf::from(url.strip_prefix("file://").unwrap_or(url)) }

/// Is the backup kept in the folder `url` there right now? Not when its drive is not plugged in,
/// its network share is not connected, or the copy was moved away. Never waits long: a network
/// share that cannot be reached can take a minute to say so.
pub async fn folder_present(url: &str) -> bool {
    let path = folder_path(url);
    let look = tokio::task::spawn_blocking(move || path.join("HEAD").is_file() && path.join("objects").is_dir());
    matches!(tokio::time::timeout(std::time::Duration::from_secs(5), look).await, Ok(Ok(true)))
}

/// The online locations, `[{name, url, main, kind}]`, in the order they were added. `main` marks
/// the one a sync receives from (see `pick_main`); every location is sent to. `kind`: see `location_kind`.
pub async fn remotes(state: &AppState, dir: &Path) -> Vec<Value> {
    // The configuration lists them in the order they were added; `git remote` would sort by name.
    let out = git(state, dir, &["config", "--local", "--get-regexp", r"^remote\..*\.url$"]).await.unwrap_or_default();
    let mut list: Vec<(String, String)> = vec![];
    for l in out.lines() {
        let Some((key, url)) = l.split_once(' ') else { continue };
        let Some(name) = key.strip_prefix("remote.").and_then(|k| k.strip_suffix(".url")) else { continue };
        if !list.iter().any(|(n, _)| n == name) { list.push((name.to_string(), url.trim().to_string())); }
    }
    // That library's own choice (a Wrap may sync a library after another one was opened).
    let chosen = state.settings.read().await.sync_of(dir).main_location.clone();
    let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
    let main = pick_main(&names, chosen.as_deref()).map(String::from);
    list.iter().map(|(n, u)| json!({"name": n, "url": u, "main": Some(n) == main.as_ref(), "kind": location_kind(u)})).collect()
}

/// The address and kind of the location `name` in a list from `remotes`.
fn location_of<'a>(rs: &'a [Value], name: &str) -> (&'a str, &'a str) {
    let r = rs.iter().find(|r| r["name"] == name);
    (r.and_then(|r| r["url"].as_str()).unwrap_or(""), r.and_then(|r| r["kind"].as_str()).unwrap_or("online"))
}

/// The name of the main location in a list from `remotes`.
fn main_of(rs: &[Value]) -> Option<String> {
    rs.iter().find(|r| r["main"] == true).or(rs.first()).and_then(|r| r["name"].as_str()).map(String::from)
}

pub async fn status(state: &AppState, dir: &Path) -> Value {
    if !is_repo(state, dir).await { return json!({"repo": false}); }
    let branch = git(state, dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await.unwrap_or_default().trim().to_string();
    let dirty = git(state, dir, &["status", "--porcelain"]).await.unwrap_or_default().lines().count();
    let ab = git(state, dir, &["rev-list", "--left-right", "--count", "@{u}...HEAD"]).await.ok();
    let (behind, ahead) = ab.as_deref().and_then(|s| {
        let mut it = s.split_whitespace();
        Some((it.next()?.parse::<u32>().ok()?, it.next()?.parse::<u32>().ok()?))
    }).unwrap_or((0, 0));
    let last = git(state, dir, &["log", "-1", "--format=%h %s (%cr)"]).await.unwrap_or_default().trim().to_string();
    json!({"repo": true, "branch": branch, "dirty": dirty, "ahead": ahead, "behind": behind,
           "last": last, "remotes": remotes(state, dir).await,
           // Is the history kept by the app, outside the library folder?
           "kept": matches!(repo_of(dir), Repo::Kept(_))})
}

/// The branch a sync works on. The app's own bookkeeping branches are never pushed.
async fn syncable_branch(state: &AppState, dir: &Path) -> anyhow::Result<String> {
    let branch = current_branch(state, dir).await?;
    if internal_branch(&branch) {
        return Err(crate::tr::err("git.internal_branch", &[("branch", &branch)]));
    }
    Ok(branch)
}

/// Send `branch` to every location. Returns, per location, `None` when it was sent, or why not
/// (a key of `tr`'s table, see `why_not_sent`). A drive that is not plugged in is not tried, and a
/// library that keeps files with Git LFS is sent nowhere (`lfs_in_use`).
async fn push_all(state: &AppState, dir: &Path, rs: &[Value], branch: &str, log: &mut Vec<String>) -> Vec<(String, Option<&'static str>)> {
    if lfs_in_use(state, dir).await {
        crate::log::warn("sync: the library keeps files with Git LFS, which the app does not send; nothing was sent");
        return rs.iter().filter_map(|r| r["name"].as_str()).map(|n| { log.push(format!("{n}: not sent (Git LFS)")); (n.to_string(), Some("sync.why.lfs")) }).collect();
    }
    let mut sent = vec![];
    // The main location first, and only it becomes what the branch follows: "to send" and "to
    // receive" are counted against it, whatever was added or sent to afterwards.
    let main = main_of(rs);
    let mut order: Vec<&Value> = rs.iter().collect();
    order.sort_by_key(|r| r["name"].as_str().map(String::from) != main);
    for r in order {
        let name = r["name"].as_str().unwrap_or_default();
        let kind = r["kind"].as_str().unwrap_or("online");
        if kind == "folder" && !folder_present(r["url"].as_str().unwrap_or("")).await {
            log.push(format!("{name}: not connected"));
            sent.push((name.to_string(), Some("sync.why.unplugged")));
            continue;
        }
        let args: &[&str] = if Some(name) == main.as_deref() { &["push", "-u", "--", name, branch] } else { &["push", "--", name, branch] };
        match git(state, dir, args).await {
            Ok(_) => { log.push(format!("pushed {name}")); sent.push((name.to_string(), None)); }
            Err(e) => {
                crate::log::warn(format!("sync: push to {name} failed: {e}"));
                log.push(format!("push {name} failed"));
                sent.push((name.to_string(), Some(why_not_sent(&e, kind))));
            }
        }
    }
    sent
}

/// Does this library keep files with Git LFS? A file its rules give to LFS is then recorded as a
/// placeholder, and the file itself is uploaded by a step git runs before sending (a hook) — which
/// the app never runs (see `run_with`). A location would get placeholders for files that exist only
/// on this computer: GitHub refuses that, other hosts take it silently. True when the LFS filter is
/// set up as this library's git sees it and one of its attribute rules hands files to it.
pub async fn lfs_in_use(state: &AppState, dir: &Path) -> bool {
    let mut set = false;
    for key in ["filter.lfs.process", "filter.lfs.clean"] {
        set |= git(state, dir, &["config", "--get", key]).await.map_or(false, |v| !v.trim().is_empty());
    }
    if !set { return false; }
    let mut rules: Vec<PathBuf> = vec![];
    let listed = git(state, dir, &["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", ":(glob)**/.gitattributes"]).await.unwrap_or_default();
    rules.extend(listed.split('\0').filter(|f| !f.is_empty()).map(|f| dir.join(f)));
    if let Ok(p) = git(state, dir, &["rev-parse", "--git-path", "info/attributes"]).await {
        let p = PathBuf::from(p.trim());
        rules.push(if p.is_absolute() { p } else { dir.join(p) });
    }
    if let Ok(p) = git(state, dir, &["config", "--type=path", "--get", "core.attributesFile"]).await {
        if !p.trim().is_empty() { rules.push(PathBuf::from(p.trim())); }
    }
    rules.iter().any(|f| std::fs::read_to_string(f).map_or(false, |t| t.lines().any(|l| !l.trim_start().starts_with('#') && l.contains("filter=lfs"))))
}

/// The plain reason a location of `kind` (see `location_kind`) did not take the library, or could
/// not be reached, as a key of `tr`'s table.
fn why_not_sent(e: &anyhow::Error, kind: &str) -> &'static str {
    let m = e.to_string().to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    // SSH says these before anything else: the server's key, not a sign-in, is what stopped it.
    if has(&["remote host identification has changed", "host key for", "has changed and you have requested strict checking"]) { return "sync.why.host_changed"; }
    if has(&["host key verification failed", "no ecdsa host key is known", "no ed25519 host key is known", "no rsa host key is known"]) { return "sync.why.host_key"; }
    if kind == "folder" {
        if has(&["no space left", "disk full", "not enough space"]) { return "sync.why.full"; }
        if has(&["read-only file system", "permission denied", "access is denied", "operation not permitted"]) { return "sync.why.no_write"; }
        if has(&["does not appear to be a git repository", "not a git repository", "no such file or directory", "could not read from remote"]) { return "sync.why.unplugged"; }
    }
    match crate::tr::classify_in(false, e).code {
        // An SSH location has no password: its key is what the server turned down.
        "sign_in" => return if kind == "ssh" { "sync.why.ssh_key" } else { "sync.why.sign_in" },
        "offline" => return "sync.why.offline",
        "timeout" => return "sync.why.timeout",
        _ => {}
    }
    if has(&["repository not found", "does not appear to be a git repository", "returned error: 404"]) { return "sync.why.not_found"; }
    if has(&["non-fast-forward", "fetch first", "(stale info)", "tip of your current branch is behind"]) { "sync.why.rejected" }
    else if has(&["file size limit", "too large", "exceeds", "larger than"]) { "sync.why.too_large" }
    else { "sync.why.refused" }
}

/// What is known about each library's backups: `{<library folder>: {at, location, locations:
/// {<name>: {ok, tried, why}}}}`. `at`/`location`: the last time the library really reached a
/// location, and which. Per location, `ok` is the last time it took the library, `tried` the last
/// attempt, and `why` why that attempt failed (a key of `tr`'s table; null when it worked).
fn backup_state() -> serde_json::Map<String, Value> {
    std::fs::read_to_string(crate::paths::backup_state_file()).ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

fn backup_key(dir: &Path) -> String { crate::paths::canonical_lenient(dir).to_string_lossy().to_string() }

/// Change what is noted about the library in `dir` (see `backup_state`), one writer at a time.
fn change_backup_state(dir: &Path, change: impl FnOnce(&mut serde_json::Map<String, Value>)) {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = backup_state();
    let mut entry = all.get(&backup_key(dir)).and_then(Value::as_object).cloned().unwrap_or_default();
    change(&mut entry);
    all.insert(backup_key(dir), Value::Object(entry));
    crate::log::ok_or_warn("sync: could not note the time of the backup",
        serde_json::to_string_pretty(&all).map_err(std::io::Error::other).and_then(|t| crate::paths::write_atomic(&crate::paths::backup_state_file(), t.as_bytes(), true)));
}

/// Note how an attempt to reach each location in `sent` went just now: `None` when it took the
/// library whole, otherwise why not.
fn note_attempts(dir: &Path, sent: &[(String, Option<&'static str>)]) {
    if sent.is_empty() { return; }
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    change_backup_state(dir, |entry| {
        if let Some((name, _)) = sent.iter().find(|(_, why)| why.is_none()) {
            entry.insert("at".into(), json!(now));
            entry.insert("location".into(), json!(name));
        }
        let mut locs = entry.get("locations").and_then(Value::as_object).cloned().unwrap_or_default();
        for (name, why) in sent {
            let mut l = locs.get(name).and_then(Value::as_object).cloned().unwrap_or_default();
            l.insert("tried".into(), json!(now));
            l.insert("why".into(), json!(why));
            if why.is_none() { l.insert("ok".into(), json!(now)); }
            locs.insert(name.clone(), Value::Object(l));
        }
        entry.insert("locations".into(), Value::Object(locs));
    });
}

/// Forget what was noted about a location that was removed from the library in `dir`.
pub fn forget_location_state(dir: &Path, name: &str) {
    change_backup_state(dir, |entry| {
        if let Some(locs) = entry.get_mut("locations").and_then(Value::as_object_mut) { locs.remove(name); }
    });
}

/// The location `name` lets this computer in again (its sign-in or the server's key was put
/// right): its last failure no longer stands, and the next sync says how it goes.
pub fn clear_failure(dir: &Path, name: &str) {
    change_backup_state(dir, |entry| {
        if let Some(l) = entry.get_mut("locations").and_then(Value::as_object_mut).and_then(|m| m.get_mut(name)).and_then(Value::as_object_mut) {
            l.insert("why".into(), Value::Null);
        }
    });
}

/// `{at, location}`: the last time the library in `dir` really reached a location. Null when it
/// never has (or the note of it is gone).
pub fn last_backup(dir: &Path) -> Value {
    match backup_state().get(&backup_key(dir)) {
        Some(e) if e["at"].is_string() => json!({"at": e["at"], "location": e["location"]}),
        _ => Value::Null,
    }
}

/// After how many days without a backup, while changes wait to be sent, the backup counts as
/// overdue (`backup_health`).
pub const STALE_DAYS: i64 = 7;

/// How the backup of the library in `dir` stands, for the window to show plainly:
/// `{state, staleDays, locations: [{name, kind, main, at, tried, why, whyKey, unsent, present}]}`.
/// Per location: when it last took the library (`at`), the last attempt and why it failed, how many
/// recorded changes it does not have yet (`unsent`, plus the changes not recorded yet), and for a
/// drive whether it is plugged in. `state`: `none` (no location), `failed` (the last attempt at a
/// location failed), `stale` (changes have waited more than `STALE_DAYS` days, or a location never
/// received the library), `waiting` (changes not sent yet) or `ok`.
pub async fn backup_health(state: &AppState, dir: &Path) -> Value {
    let rs = remotes(state, dir).await;
    let noted = backup_state().get(&backup_key(dir)).and_then(|e| e.get("locations")).cloned().unwrap_or(Value::Null);
    let branch = current_branch(state, dir).await.ok();
    let has_head = head(state, dir).await.is_some();
    let dirty = git(state, dir, &["status", "--porcelain"]).await.unwrap_or_default().lines().count() as u64;
    let now = chrono::Utc::now();
    let when = |v: &Value| v.as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&chrono::Utc));
    let mut overall = if rs.is_empty() { "none" } else { "ok" };
    let rank = |s: &str| ["ok", "waiting", "stale", "failed", "none"].iter().position(|x| *x == s).unwrap_or(0);
    let mut out = vec![];
    for r in &rs {
        let name = r["name"].as_str().unwrap_or_default();
        let url = r["url"].as_str().unwrap_or_default();
        let n = &noted[name];
        // What this location does not have: the changes after what it last received (all of them
        // when it never received anything), and those not recorded yet.
        let unsent = if !has_head { 0 } else {
            let tracking = branch.as_ref().map(|b| format!("refs/remotes/{name}/{b}"));
            let known = match &tracking { Some(t) => git(state, dir, &["rev-parse", "--verify", "-q", t]).await.is_ok(), None => false };
            let range = if known { format!("{}..HEAD", tracking.unwrap_or_default()) } else { "HEAD".into() };
            git(state, dir, &["rev-list", "--count", &range]).await.ok().and_then(|c| c.trim().parse::<u64>().ok()).unwrap_or(0)
        } + dirty;
        let ok = when(&n["ok"]);
        // The last attempt's reason stands until an attempt works (or it was put right, `clear_failure`).
        let why_key = n["why"].as_str();
        let present = if r["kind"] == "folder" { Some(folder_present(url).await) } else { None };
        let this = if why_key.is_some() { "failed" }
            else if unsent > 0 && ok.map_or(true, |t| (now - t).num_days() >= STALE_DAYS) { "stale" }
            else if unsent > 0 { "waiting" } else { "ok" };
        if rank(this) > rank(overall) { overall = this; }
        out.push(json!({"name": name, "kind": r["kind"], "main": r["main"], "at": n["ok"], "tried": n["tried"],
            "whyKey": why_key, "why": why_key.map(|k| crate::tr::text(k, &[])), "unsent": unsent, "present": present, "state": this}));
    }
    json!({"state": overall, "staleDays": STALE_DAYS, "locations": out})
}

/// Close a sync. What happened at each location is noted, and unless every location took the
/// library the sync fails, naming each one and saying why — a sync is never reported as done
/// while something was not sent. Returns `[{name, sent: true}]`.
fn sync_outcome(dir: &Path, sent: &[(String, Option<&'static str>)]) -> anyhow::Result<Vec<Value>> {
    note_attempts(dir, sent);
    let failed: Vec<&(String, Option<&'static str>)> = sent.iter().filter(|(_, why)| why.is_some()).collect();
    if failed.is_empty() { return Ok(sent.iter().map(|(name, _)| json!({"name": name, "sent": true})).collect()); }
    let ar = crate::tr::arabic();
    let ok: Vec<&str> = sent.iter().filter(|(_, why)| why.is_none()).map(|(n, _)| n.as_str()).collect();
    let mut parts = vec![];
    let quoted: Vec<String> = ok.iter().map(|n| if ar { format!("«{n}»") } else { format!("\"{n}\"") }).collect();
    if !ok.is_empty() { parts.push(crate::tr::text_in(ar, "sync.sent_to", &[("names", &quoted.join(if ar { "، " } else { ", " }))])); }
    for (name, why) in failed {
        let why = crate::tr::text_in(ar, why.unwrap_or("sync.why.refused"), &[]);
        parts.push(crate::tr::text_in(ar, "sync.not_sent_to", &[("name", name), ("why", &why)]));
    }
    Err(crate::tr::UserErr { code: "sync_failed", status: 409, message: parts.join(" ") }.into())
}

/// Receiving from the main location `name` failed with `e`: noted as that location's last attempt,
/// and said plainly: the reason when it is a known one, otherwise that it answered with an error
/// (git's own words are kept for "Show details").
fn not_received(dir: &Path, name: &str, kind: &str, e: anyhow::Error) -> anyhow::Error {
    let why = why_not_sent(&e, kind);
    note_attempts(dir, &[(name.to_string(), Some(why))]);
    crate::log::warn(format!("sync: receiving from {name} failed: {e}"));
    // Sending-side reasons say nothing about receiving.
    let why = if matches!(why, "sync.why.refused" | "sync.why.rejected" | "sync.why.too_large") { "sync.why.receive" } else { why };
    let why = crate::tr::text(why, &[]);
    crate::tr::err_detail("sync.not_received", &[("name", name), ("why", &why)], format!("{e:#}"))
}

/// Fetch `branch` from the location `name`: the commit it is at there, or `None` when the location
/// does not have that branch yet. Talks to the network: called without holding the files.
async fn fetch_tip(state: &AppState, dir: &Path, name: &str, branch: &str) -> anyhow::Result<Option<String>> {
    match git(state, dir, &["fetch", "-q", "--", name, branch]).await {
        Ok(_) => Ok(Some(git(state, dir, &["rev-parse", "--verify", "-q", "FETCH_HEAD^{commit}"]).await?.trim().to_string())),
        Err(e) if e.to_string().contains("couldn't find remote ref") => Ok(None),
        Err(e) => Err(e),
    }
}

async fn is_ancestor(state: &AppState, dir: &Path, a: &str, b: &str) -> bool {
    git(state, dir, &["merge-base", "--is-ancestor", a, b]).await.is_ok()
}

/// May the changes recorded here that `theirs` does not have be replayed on top of it (a rebase)?
/// Not when one of them combines two sides (replaying it would undo how a clash was settled, and
/// ask about the same documents again), and not when any location already took one of them: that
/// location would hold the old ones and refuse the replayed ones for good, since the app never
/// overwrites what a location has. In doubt, no: combining (a merge) is always safe.
async fn may_replay(state: &AppState, dir: &Path, theirs: &str) -> bool {
    async fn count(state: &AppState, dir: &Path, args: &[&str]) -> Option<u64> {
        git(state, dir, args).await.ok().and_then(|c| c.trim().parse::<u64>().ok())
    }
    let range = format!("{theirs}..HEAD");
    let all = count(state, dir, &["rev-list", "--count", &range]).await;
    let merges = count(state, dir, &["rev-list", "--count", "--merges", &range]).await;
    // Every location's branches as this computer last saw them (a push updates them too).
    let sent_nowhere = count(state, dir, &["rev-list", "--count", &range, "--not", "--remotes"]).await;
    matches!((all, merges, sent_nowhere), (Some(a), Some(0), Some(u)) if a == u)
}

/// Do this library's history and `theirs` share a beginning? Not when the library folder was copied
/// here without its history (the one the app keeps stays on the other computer) and then given the
/// location that computer sends to: each side then began on its own.
async fn shares_start(state: &AppState, dir: &Path, theirs: &str) -> bool {
    git(state, dir, &["merge-base", "HEAD", theirs]).await.is_ok()
}

/// Sync stops before anything is changed: the location `name` holds a library this one did not come from.
fn unrelated_location(name: &str) -> anyhow::Error {
    crate::log::warn(format!("sync: {name} holds a history that shares nothing with this library's; asking how to join them"));
    crate::tr::err("sync.unrelated", &[("name", name)])
}

/// Does what the location `name` holds share a beginning with this library? `None` when that cannot
/// be told: the location cannot be reached, it holds nothing yet, or nothing is recorded here yet.
/// Talks to the network.
pub async fn location_related(state: &AppState, dir: &Path, name: &str) -> Option<bool> {
    let branch = current_branch(state, dir).await.ok()?;
    head(state, dir).await?;
    let _g = state.git_lock.lock().await;
    let tip = fetch_tip(state, dir, name, &branch).await.ok()??;
    Some(shares_start(state, dir, &tip).await)
}

/// Commit local changes, take in what the main location has (fetch, then rebase onto it — or merge
/// it, when a rebase would rewrite changes a location already took or a clash already settled, see
/// `may_replay`), send to all. A pull that conflicts is undone and reported as `CONFLICT::<file;file;…>` (see `resolve`).
/// Fails (`no_location`) when there is nowhere to send the library, and (`sync_failed`, naming the
/// location and the reason) when any location did not take it: `Ok` means it is backed up.
///
/// The library's files are held (`AppState::sync_files`) only while git changes them — recording
/// them, and putting what arrived together with them. While the sync waits on the network the
/// files are free, and an edit made meanwhile is recorded before anything that arrived is put in.
/// A main location that is a drive not plugged in is not received from; the others are still sent to.
pub async fn sync(state: &AppState, dir: &Path) -> anyhow::Result<Value> {
    let (branch, mut committed, rs) = {
        let _files = state.sync_files.write().await;
        let _g = state.git_lock.lock().await;
        crate::trust::require_git(dir)?;
        let branch = syncable_branch(state, dir).await?;
        // Edits not recorded yet are the user's own: they are recorded as such, not as something that arrived.
        let committed = commit_all(state, dir, &subject::edit("your changes")).await?;
        (branch, committed, remotes(state, dir).await)
    };
    // Nothing leaves this computer without a location: that is not a sync that worked.
    if rs.is_empty() { return Err(crate::tr::err("sync.no_location", &[])); }
    let before = head(state, dir).await;
    let mut log = vec![];
    if let Some(primary) = main_of(&rs).as_deref() {
        let (url, kind) = location_of(&rs, primary);
        if kind == "folder" && !folder_present(url).await {
            log.push(format!("{primary}: not connected"));
        } else if before.is_none() {
            // A library with nothing recorded yet takes the location's history as it is.
            let _files = state.sync_files.write().await;
            let _g = state.git_lock.lock().await;
            match git_ident(state, dir, &["pull", "--rebase", "--autostash", "--", primary, &branch]).await {
                Ok(_) => log.push(format!("pulled {primary}")),
                Err(e) if e.to_string().contains("couldn't find remote ref") => log.push(format!("{primary}: new branch")),
                Err(e) => return Err(not_received(dir, primary, kind, e)),
            }
        } else {
            let fetched = { let _g = state.git_lock.lock().await; fetch_tip(state, dir, primary, &branch).await };
            match fetched {
                Ok(Some(theirs)) => {
                    let _files = state.sync_files.write().await;
                    let _g = state.git_lock.lock().await;
                    // What was saved while the fetch ran is recorded first, like everything before it.
                    committed |= commit_all(state, dir, &subject::edit("your changes")).await?;
                    // A library this one did not come from is never mixed in: the person chooses how (`join_unrelated`).
                    if !shares_start(state, dir, &theirs).await { return Err(unrelated_location(primary)); }
                    // Changes recorded here are replayed on top of what arrived when nothing else has
                    // them yet; otherwise the two are combined, and what was recorded stays as it is.
                    let replay = may_replay(state, dir, &theirs).await;
                    let took = if is_ancestor(state, dir, &theirs, "HEAD").await { Ok(String::new()) }
                        else if replay { git_ident(state, dir, &["rebase", "--autostash", &theirs]).await }
                        else { git_ident(state, dir, &["merge", "--no-edit", "-m", &subject::combined(), &theirs]).await };
                    if let Err(e) = took {
                        // Read the conflict list before undoing the half-finished rebase or merge.
                        let files = unmerged(state, dir).await;
                        let conflict = !files.is_empty() || e.to_string().contains("CONFLICT");
                        if let Err(e2) = git(state, dir, &[if replay { "rebase" } else { "merge" }, "--abort"]).await {
                            if conflict { crate::log::error(format!("sync: could not undo the conflicting pull: {e2}")); }
                        }
                        if conflict {
                            // A rebase stops at the first change that clashes, so its list can be short: the
                            // documents that clash when the two sides are combined (as `resolve` will) are
                            // the ones to ask about.
                            let files = match merge_clashes(state, dir, "HEAD", &theirs).await { Some(all) if !all.is_empty() => all, _ => files };
                            crate::log::warn(format!("sync: pull from {primary} conflicts in {} file(s)", files.len()));
                            anyhow::bail!("CONFLICT::{}", files.join(";"));
                        }
                        crate::log::warn(format!("sync: pull from {primary} failed: {e}"));
                        return Err(crate::tr::err_detail("sync.not_received", &[("name", primary), ("why", &crate::tr::text("sync.why.receive", &[]))], format!("{e:#}")));
                    }
                    log.push(format!("pulled {primary}"));
                }
                Ok(None) => log.push(format!("{primary}: new branch")),
                Err(e) => return Err(not_received(dir, primary, kind, e)),
            }
        }
    }
    // Did anything arrive from the other devices?
    let received = head(state, dir).await != before;
    let sent = { let _g = state.git_lock.lock().await; push_all(state, dir, &rs, &branch, &mut log).await };
    crate::log::info(format!("sync: {}", log.join(", ")));
    let locations = sync_outcome(dir, &sent)?;
    Ok(json!({"committed": committed, "log": log, "received": received, "locations": locations, "lastBackup": last_backup(dir)}))
}

/// How many `backup/…` branches are kept; older ones are removed when a new one is made.
pub const KEEP_BACKUPS: usize = 10;

/// Delete all but the newest `KEEP_BACKUPS` backup branches (their names are timestamps).
pub async fn prune_backups(state: &AppState, dir: &Path) -> usize {
    let out = git(state, dir, &["for-each-ref", "--format=%(refname:short)", "refs/heads/backup/"]).await.unwrap_or_default();
    let mut names: Vec<&str> = out.lines().map(str::trim).filter(|n| n.starts_with("backup/")).collect();
    names.sort();
    let extra = names.len().saturating_sub(KEEP_BACKUPS);
    let mut removed = 0;
    for name in names.into_iter().take(extra) {
        if git(state, dir, &["branch", "-q", "-D", "--", name]).await.is_ok() { removed += 1; }
    }
    removed
}

/// Keep the current local state as a new branch `backup/<YYYYMMDD-HHMMSS>` and return its name.
async fn make_backup(state: &AppState, dir: &Path) -> anyhow::Result<String> { make_backup_at(state, dir, "HEAD").await }

/// Keep commit `at` as a new branch `backup/<YYYYMMDD-HHMMSS>` and return its name.
pub async fn make_backup_at(state: &AppState, dir: &Path, at: &str) -> anyhow::Result<String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut backup = format!("backup/{stamp}");
    let mut n = 1;
    while git(state, dir, &["rev-parse", "--verify", "-q", &format!("refs/heads/{backup}")]).await.is_ok() {
        n += 1;
        backup = format!("backup/{stamp}-{n}");
    }
    git(state, dir, &["branch", "--", &backup, at]).await?;
    prune_backups(state, dir).await;
    Ok(backup)
}

/// The documents that clash when `a` and `b` are combined, or None when this git cannot tell
/// without combining them (older than 2.38).
pub async fn merge_clashes(state: &AppState, dir: &Path, a: &str, b: &str) -> Option<Vec<String>> {
    let (clean, out) = git_output(state, dir, &["merge-tree", "--write-tree", "--name-only", "--no-messages", "-z", a, b]).await.ok()?;
    // The new tree's id comes first; then, when they clash, each clashing name.
    let mut it = out.split('\0');
    let tree = it.next()?.trim();
    if tree.len() < 40 || !tree.bytes().all(|c| c.is_ascii_hexdigit()) { return None; }
    if clean { return Some(vec![]); }
    let mut names: Vec<String> = vec![];
    for n in it.map(str::trim_end).filter(|n| !n.is_empty()) {
        if !names.iter().any(|x| x == n) { names.push(n.to_string()); }
    }
    Some(names)
}

/// How one clashing document can be settled: keep `mine`, take `theirs`, or keep `both` (mine
/// where it is, theirs beside it as a second copy named for the other device).
pub const CHOICES: &[&str] = &["mine", "theirs", "both"];

/// Finish a sync whose pull conflicted, with one choice for every clashing document (see
/// `resolve_each`).
pub async fn resolve(state: &AppState, dir: &Path, strategy: &str) -> anyhow::Result<Value> {
    resolve_each(state, dir, Some(strategy), &std::collections::HashMap::new()).await
}

/// Finish a sync whose pull conflicted: the remote is merged (a merge, not a rebase, so "mine"
/// really is the local side) and each clashing document is settled as `choices` says (`{path:
/// mine|theirs|both}`), or as `default` for one not named there; then the result is pushed. Where
/// the two sides changed different parts of a document both changes are kept: mine or theirs only
/// decides where they disagree. A clashing document with no choice at all changes nothing and is
/// reported again with every other one (`CONFLICT::<file;file;…>`), so the person is asked about
/// all of them. Whenever the two sides really clash, the local state is first kept as a branch
/// `backup/<YYYYMMDD-HHMMSS>`; when there is nothing to settle (the call was not needed) no backup
/// is made and this is an ordinary sync.
pub async fn resolve_each(state: &AppState, dir: &Path, default: Option<&str>, choices: &std::collections::HashMap<String, String>) -> anyhow::Result<Value> {
    if default.map_or(false, |d| !CHOICES.contains(&d)) || choices.values().any(|c| !CHOICES.contains(&c.as_str())) {
        return Err(crate::tr::err("git.strategy", &[]));
    }
    // As in `sync`: the files are held while git changes them, and free while it waits on the network.
    let (branch, mut committed, rs, primary) = {
        let _files = state.sync_files.write().await;
        let _g = state.git_lock.lock().await;
        crate::trust::require_git(dir)?;
        let branch = syncable_branch(state, dir).await?;
        let committed = commit_all(state, dir, &subject::edit("your changes")).await?;
        let rs = remotes(state, dir).await;
        let primary = main_of(&rs).ok_or_else(|| crate::tr::err("git.no_remote", &[]))?;
        if head(state, dir).await.is_none() { return Err(crate::tr::err("history.empty", &[])); }
        (branch, committed, rs, primary)
    };
    let (url, kind) = location_of(&rs, &primary);
    if kind == "folder" && !folder_present(url).await {
        return Err(not_received(dir, &primary, kind, anyhow::anyhow!("not a git repository: the drive is not connected")));
    }

    let mut log = vec![];
    let mut backup: Option<String> = None;
    let mut settled: Vec<Value> = vec![];
    let how = default.unwrap_or("each");
    let fetched = { let _g = state.git_lock.lock().await; fetch_tip(state, dir, &primary, &branch).await };
    match fetched {
        Ok(Some(theirs)) => {
            let _files = state.sync_files.write().await;
            let _g = state.git_lock.lock().await;
            committed |= commit_all(state, dir, &subject::edit("your changes")).await?;
            if !shares_start(state, dir, &theirs).await { return Err(unrelated_location(&primary)); }
            if is_ancestor(state, dir, &theirs, "HEAD").await {
                log.push(format!("{primary}: nothing new"));
            } else {
                // Only a real clash needs a safety copy: a fast-forward or a clean merge loses nothing.
                let clean = is_ancestor(state, dir, "HEAD", &theirs).await
                    || git(state, dir, &["merge-tree", "--write-tree", "HEAD", &theirs]).await.is_ok();
                if !clean {
                    let b = make_backup(state, dir).await?;
                    crate::log::info(format!("sync: resolving conflict ({how}); local state kept as {b}"));
                    log.push(format!("backup {b}"));
                    backup = Some(b);
                }
                let msg = subject::combined();
                if let Err(e) = git_ident(state, dir, &["merge", "--no-edit", "-m", &msg, &theirs]).await {
                    let mut files = unmerged(state, dir).await;
                    if files.is_empty() {
                        let _ = git(state, dir, &["merge", "--abort"]).await;
                        return Err(crate::tr::err_detail("history.combine_failed", &[], format!("{e:#}")));
                    }
                    // A merge that looked clean but was not (an older git without merge-tree): back up now.
                    if backup.is_none() {
                        let _ = git(state, dir, &["merge", "--abort"]).await;
                        let b = make_backup(state, dir).await?;
                        log.push(format!("backup {b}"));
                        backup = Some(b);
                        let _ = git_ident(state, dir, &["merge", "--no-edit", "-m", &msg, &theirs]).await;
                        files = unmerged(state, dir).await;
                    }
                    let choice_of = |f: &str| choices.get(f).map(String::as_str).or(default);
                    // Nothing is decided for the person: every clashing document needs its choice.
                    if files.iter().any(|f| choice_of(f).is_none()) {
                        let _ = git(state, dir, &["merge", "--abort"]).await;
                        crate::log::info(format!("sync: {} clashing file(s), not all of them chosen for; asking again", files.len()));
                        return Err(crate::tr::err("conflict", &[("files", &files.join(";"))]));
                    }
                    let done = async {
                        for f in &files {
                            let choice = choice_of(f).unwrap_or("mine");
                            let copy = settle(state, dir, f, choice).await?;
                            settled.push(json!({"path": f, "choice": choice, "copy": copy}));
                        }
                        git(state, dir, &["add", "-A"]).await?;
                        git_ident(state, dir, &["commit", "-q", "-m", &msg]).await
                    }.await;
                    if let Err(e) = done {
                        let _ = git(state, dir, &["merge", "--abort"]).await;
                        return Err(crate::tr::err_detail("history.combine_failed", &[], format!("{e:#}")));
                    }
                }
                log.push(format!("merged {primary} ({how})"));
            }
        }
        Ok(None) => log.push(format!("{primary}: new branch")),
        Err(e) => return Err(not_received(dir, &primary, kind, e)),
    }
    let sent = { let _g = state.git_lock.lock().await; push_all(state, dir, &rs, &branch, &mut log).await };
    crate::log::info(format!("sync: {}", log.join(", ")));
    let locations = sync_outcome(dir, &sent)?;
    Ok(json!({"committed": committed, "backup": backup, "strategy": default, "settled": settled, "log": log, "locations": locations, "lastBackup": last_backup(dir)}))
}

/// How a library can be joined, once, with a main location holding one it did not come from (see
/// `join_unrelated`).
pub const JOINS: &[&str] = &["combine", "theirs"];

/// Join this library, once, with what its main location holds when the two did not come from one
/// another — typically the library folder was copied to this computer without its history and then
/// given the location the other computer sends to. `sync` never mixes the two (`unrelated`); the
/// person chooses `how`, and this computer's state is first kept as a `backup/<stamp>` branch:
/// - `combine` (the safe default): every document here stays, the location's are added; a document
///   both have with different contents keeps this computer's version in place and the location's
///   beside it (`other_copy_name`). Both histories are kept.
/// - `theirs`: the location's history becomes the library's; the folder's documents are recorded on
///   top of it as the person's changes (where they differ, the location's version stays in the
///   history), and documents only the location has are put in the folder. Nothing is removed.
///
/// Then the result is sent to every location. When the two turn out to share a beginning after all,
/// or the location holds nothing yet, this is an ordinary `sync`.
pub async fn join_unrelated(state: &AppState, dir: &Path, how: &str) -> anyhow::Result<Value> {
    if !JOINS.contains(&how) { return Err(crate::tr::err("git.strategy", &[])); }
    let (branch, mut committed, rs, primary) = {
        let _files = state.sync_files.write().await;
        let _g = state.git_lock.lock().await;
        crate::trust::require_git(dir)?;
        let branch = syncable_branch(state, dir).await?;
        let committed = commit_all(state, dir, &subject::edit("your changes")).await?;
        let rs = remotes(state, dir).await;
        let primary = main_of(&rs).ok_or_else(|| crate::tr::err("git.no_remote", &[]))?;
        if head(state, dir).await.is_none() { return Err(crate::tr::err("history.empty", &[])); }
        (branch, committed, rs, primary)
    };
    let (url, kind) = location_of(&rs, &primary);
    if kind == "folder" && !folder_present(url).await {
        return Err(not_received(dir, &primary, kind, anyhow::anyhow!("not a git repository: the drive is not connected")));
    }
    let fetched = { let _g = state.git_lock.lock().await; fetch_tip(state, dir, &primary, &branch).await };
    let theirs = match fetched {
        Ok(Some(t)) => t,
        Ok(None) => return sync(state, dir).await,
        Err(e) => return Err(not_received(dir, &primary, kind, e)),
    };
    let mut log = vec![];
    let mut copies: Vec<String> = vec![];
    let backup = {
        let _files = state.sync_files.write().await;
        let _g = state.git_lock.lock().await;
        committed |= commit_all(state, dir, &subject::edit("your changes")).await?;
        if shares_start(state, dir, &theirs).await { None } else {
            let b = make_backup(state, dir).await?;
            crate::log::info(format!("sync: joining an unrelated history from {primary} ({how}); local state kept as {b}"));
            log.push(format!("backup {b}"));
            let joined = if how == "combine" { join_both(state, dir, &theirs, &mut copies).await } else { start_from(state, dir, &theirs, &b).await };
            if let Err(e) = joined {
                crate::log::warn(format!("sync: joining {primary} failed: {e:#}"));
                return Err(crate::tr::err_detail("history.combine_failed", &[], format!("{e:#}")));
            }
            log.push(format!("joined {primary} ({how})"));
            Some(b)
        }
    };
    // Already one library: nothing to join, an ordinary sync (with nothing held here).
    let Some(backup) = backup else { return sync(state, dir).await };
    let sent = { let _g = state.git_lock.lock().await; push_all(state, dir, &rs, &branch, &mut log).await };
    crate::log::info(format!("sync: {}", log.join(", ")));
    let locations = sync_outcome(dir, &sent)?;
    Ok(json!({"committed": committed, "received": true, "backup": backup, "how": how, "copies": copies, "log": log, "locations": locations, "lastBackup": last_backup(dir)}))
}

/// `join_unrelated`'s `combine`: the two histories merged; every document both have with different
/// contents is kept twice (`settle` with `both`). Anything that fails is undone.
async fn join_both(state: &AppState, dir: &Path, theirs: &str, copies: &mut Vec<String>) -> anyhow::Result<()> {
    let msg = subject::combined();
    let Err(e) = git_ident(state, dir, &["merge", "--allow-unrelated-histories", "--no-edit", "-m", &msg, theirs]).await else { return Ok(()) };
    let files = unmerged(state, dir).await;
    let done = async {
        if files.is_empty() { return Err(e); }
        for f in &files {
            if let Some(copy) = settle(state, dir, f, "both").await? { copies.push(copy); }
        }
        git(state, dir, &["add", "-A"]).await?;
        git_ident(state, dir, &["commit", "-q", "-m", &msg]).await.map(|_| ())
    }.await;
    if done.is_err() { let _ = git(state, dir, &["merge", "--abort"]).await; }
    done
}

/// `join_unrelated`'s `theirs`: the library's history becomes `theirs`, the folder is left as it is
/// apart from the documents only `theirs` has, which are put in it, and what differs is recorded as
/// the person's changes. On failure the history goes back to `backup` (the folder keeps whatever
/// documents were already put in: nothing is ever taken out).
async fn start_from(state: &AppState, dir: &Path, theirs: &str, backup: &str) -> anyhow::Result<()> {
    let done = async {
        // The history and its index move to theirs; no file in the folder is touched.
        git(state, dir, &["reset", "-q", theirs]).await?;
        let missing = git(state, dir, &["ls-files", "-z", "--deleted"]).await?;
        let specs: Vec<String> = missing.split('\0').filter(|f| !f.is_empty()).map(|f| format!(":(top,literal){f}")).collect();
        for chunk in specs.chunks(200) {
            let mut args = vec!["checkout", "-q", "--"];
            args.extend(chunk.iter().map(String::as_str));
            git(state, dir, &args).await?;
        }
        commit_all(state, dir, &subject::edit("your changes")).await.map(|_| ())
    }.await;
    if done.is_err() { let _ = git(state, dir, &["reset", "-q", backup]).await; }
    done
}

/// The blob of each side of clashing document `f` in a stopped merge: `[base, mine, theirs]`, None
/// where that side does not have it (it was added on both sides, or deleted on one).
async fn sides_of(state: &AppState, dir: &Path, f: &str) -> anyhow::Result<[Option<String>; 3]> {
    let spec = format!(":(top,literal){f}");
    let out = git(state, dir, &["ls-files", "-u", "-z", "--", &spec]).await?;
    let mut sides: [Option<String>; 3] = [None, None, None];
    for entry in out.split('\0').filter(|e| !e.is_empty()) {
        // `<mode> <blob> <stage>\t<path>`
        let Some((meta, _)) = entry.split_once('\t') else { continue };
        let p: Vec<&str> = meta.split(' ').collect();
        if let (Some(blob), Some(Ok(stage))) = (p.get(1), p.get(2).map(|s| s.parse::<usize>())) {
            if (1..=3).contains(&stage) { sides[stage - 1] = Some(blob.to_string()); }
        }
    }
    Ok(sides)
}

/// Settle clashing document `f` of a stopped merge as `choice` (see `CHOICES`). Returns the name of
/// the second copy, when both were kept.
async fn settle(state: &AppState, dir: &Path, f: &str, choice: &str) -> anyhow::Result<Option<String>> {
    let [base, mine, theirs] = sides_of(state, dir, f).await?;
    let blob = |id: String| async move { git_raw(state, dir, &["cat-file", "blob", &id]).await };
    let spec = format!(":(top,literal){f}");
    let put = |rel: String, bytes: Vec<u8>| async move {
        let target = crate::library::resolve(dir, &rel)?;
        if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
        crate::paths::write_atomic(&target, &bytes, false)?;
        git(state, dir, &["add", "--", &format!(":(top,literal){rel}")]).await.map(|_| ())
    };
    let keep = match choice { "theirs" => theirs.clone(), _ => mine.clone() };
    match (choice, mine, theirs) {
        // Both kept, and both sides have it: mine stays, theirs is put beside it.
        ("both", Some(m), Some(t)) => {
            let copy = other_copy_name(f, &crate::tr::text("sync.other_copy", &[]), |c| std::fs::symlink_metadata(dir.join(c)).is_ok());
            put(f.to_string(), blob(m).await?).await?;
            put(copy.clone(), blob(t).await?).await?;
            return Ok(Some(copy));
        }
        // Only one side still has it: keeping both is keeping that one.
        ("both", Some(x), None) | ("both", None, Some(x)) => put(f.to_string(), blob(x).await?).await?,
        (_, Some(m), Some(t)) => {
            let (m, t) = (blob(m).await?, blob(t).await?);
            let whole = if choice == "theirs" { t.clone() } else { m.clone() };
            let combined = match base { Some(b) => combine(state, dir, &blob(b).await?, &m, &t, choice == "theirs").await.unwrap_or(whole), None => whole };
            put(f.to_string(), combined).await?;
        }
        // The chosen side removed it.
        _ if keep.is_none() => { git(state, dir, &["rm", "-q", "-f", "--ignore-unmatch", "--", &spec]).await?; }
        _ => put(f.to_string(), blob(keep.unwrap_or_default()).await?).await?,
    }
    Ok(None)
}

/// The two sides of a document combined line by line from their common `base`: every change only one
/// side made is kept, and where both changed the same lines `theirs` (or mine) wins. None for a
/// document that is not text, or when git cannot combine it.
async fn combine(state: &AppState, dir: &Path, base: &[u8], mine: &[u8], theirs: &[u8], prefer_theirs: bool) -> Option<Vec<u8>> {
    let binary = |b: &[u8]| b.iter().take(8000).any(|c| *c == 0);
    if binary(base) || binary(mine) || binary(theirs) { return None; }
    let tmp = std::env::temp_dir().join(format!("thoughts-combine-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).ok()?;
    let files = [tmp.join("mine"), tmp.join("base"), tmp.join("theirs")];
    let wrote = std::fs::write(&files[0], mine).and(std::fs::write(&files[1], base)).and(std::fs::write(&files[2], theirs));
    let side = if prefer_theirs { "--theirs" } else { "--ours" };
    let (a, b, c) = (files[0].to_string_lossy().to_string(), files[1].to_string_lossy().to_string(), files[2].to_string_lossy().to_string());
    let out = if wrote.is_ok() { git_raw(state, dir, &["merge-file", "-p", side, &a, &b, &c]).await.ok() } else { None };
    let _ = std::fs::remove_dir_all(&tmp);
    out
}

/// The name for the other device's copy of `f`, kept beside it: `plan (other device).md`, then
/// `plan (other device 2).md`… — the first one `taken` says is free.
pub fn other_copy_name(f: &str, label: &str, taken: impl Fn(&str) -> bool) -> String {
    let (folder, name) = match f.rsplit_once('/') { Some((d, n)) => (format!("{d}/"), n), None => (String::new(), f) };
    let (stem, ext) = match name.rfind('.') { Some(i) if i > 0 => name.split_at(i), _ => (name, "") };
    let mut n = 1;
    loop {
        let candidate = if n == 1 { format!("{folder}{stem} ({label}){ext}") } else { format!("{folder}{stem} ({label} {n}){ext}") };
        if !taken(&candidate) || n > 1000 { return candidate; }
        n += 1;
    }
}

// ---------- library history ----------

/// Subjects of the commits the daemon itself writes. `commit_kind` reads them back.
pub mod subject {
    /// The user's own pending edits, recorded before something else changes the library.
    pub fn edit(why: &str) -> String { format!("edit: {why}") }
    pub fn wrap(chats: usize) -> String { format!("wrap: {} — {chats} chat(s)", chrono::Local::now().format("%Y-%m-%d")) }
    pub fn sync() -> String { format!("sync: {}", chrono::Local::now().format("%Y-%m-%d %H:%M")) }
    /// Changes from another device combined with the ones made here (the merge a clash is settled with).
    pub fn combined() -> String { "sync: changes from another device combined with yours".into() }
    /// Something the assistant did (a chat, a workflow, a feature being added, a reorganize).
    pub fn assistant(what: &str) -> String {
        let what: String = what.chars().filter(|c| !c.is_control()).take(80).collect();
        format!("assistant: {}", what.trim())
    }
    /// A file or folder moved or renamed in the app (with the links that were rewritten for it).
    pub fn moved(from: &str, to: &str) -> String { edit(&format!("moved {from} to {to}")) }
    /// For a file `at` that a change with this subject added: the name it had before, when that
    /// change is a move made in the app (`moved`) of the file itself or of a folder holding it.
    pub fn moved_from(subject: &str, at: &str) -> Option<String> {
        let rest = subject.strip_prefix("edit: moved ")?;
        let clean = |p: &str| p.trim().trim_start_matches("./").trim_matches('/').to_string();
        // The new name is known, so the split is exact even when a name contains " to ".
        (1..rest.len()).filter(|&i| rest.is_char_boundary(i) && rest[i..].starts_with(" to ")).find_map(|i| {
            let (from, to) = (clean(&rest[..i]), clean(&rest[i + 4..]));
            if from.is_empty() || to.is_empty() { return None; }
            if at == to { return Some(from); }
            at.strip_prefix(&format!("{to}/")).map(|inner| format!("{from}/{inner}"))
        })
    }
    pub fn restore(path: &str, short: &str) -> String { format!("restore: {path} (from {short})") }
    pub fn undo(of: &str) -> String { format!("undo: {of}") }
    /// The first entry of a history the app starts for a library that already has content.
    pub fn start() -> String { "start: the library as it was when its history began".into() }
    /// Files changed by another program (another editor, a phone, a cloud drive), recorded as they
    /// were found (`watch.rs`). History shows it as "Changed outside Thoughts".
    pub fn outside() -> String { "outside: changed outside Thoughts".into() }
}

/// What kind of change a commit is, from the subject line the daemon gave it:
/// `wrap` | `sync` | `edit` | `assistant` | `start` | `outside` | `other`.
pub fn commit_kind(subject: &str) -> &'static str {
    let s = subject.trim();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| s.starts_with(p));
    if starts(&["wrap: "]) { "wrap" }
    else if starts(&["outside: "]) { "outside" }
    else if starts(&["start: "]) { "start" }
    else if starts(&["sync: ", "Merge ", "sync:"]) { "sync" }
    else if starts(&["assistant: ", "module: ", "init: ", "reorganize: "]) { "assistant" }
    else if starts(&["edit: ", "restore: ", "undo: ", "snapshot before "]) { "edit" }
    else { "other" }
}

/// A full or abbreviated commit id, and nothing git could read as an option or a range.
pub fn valid_hash(hash: &str) -> bool {
    (7..=64).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The newest `limit` entries of the history (see `history_page`).
pub async fn history(state: &AppState, dir: &Path, path: Option<&str>, limit: usize) -> anyhow::Result<Vec<Value>> {
    history_page(state, dir, path, 0, limit).await
}

/// One page of the history: `[{hash, short, date, subject, kind, files:[path], first}]`, newest
/// first, after leaving out the newest `skip` entries — so however long the history is, all of it
/// can be reached. With `path`, only the changes to that one file, followed back through renames
/// and moves; each entry then also carries `path`, the name the file had at that point (the name
/// to read that version by, see `file_at`).
pub async fn history_page(state: &AppState, dir: &Path, path: Option<&str>, skip: usize, limit: usize) -> anyhow::Result<Vec<Value>> {
    // A repository without commits has no history yet; that is an empty list, not an error.
    if head(state, dir).await.is_none() { return Ok(vec![]); }
    let limit = limit.clamp(1, 500);
    if let Some(path) = path {
        // Following one file, git counts `--skip` in changes of any file — and a rename it skipped
        // over is a rename it never follows. So there the page is cut here, from the newest `skip + limit`.
        return Ok(file_history(state, dir, path, skip + limit).await?.into_iter().skip(skip).collect());
    }
    let (n, skip_arg) = (format!("-{limit}"), format!("--skip={skip}"));
    let out = git(state, dir, &["log", n.as_str(), "-z", LOG_FORMAT, skip_arg.as_str(), "--name-only", "--no-renames"]).await?;
    Ok(log_records(&out).into_iter().map(|(mut entry, names)| { entry["files"] = json!(names); entry }).collect())
}

/// The header of each entry `log_records` reads.
const LOG_FORMAT: &str = "--format=%x1e%H%x1f%h%x1f%s%x1f%cI%x1f%P";

/// The entries of `git log -z` run with `LOG_FORMAT`: `{hash, short, subject, date, kind, first}`,
/// each with what followed its header (file names, or with `--name-status` a status and then the
/// name, or the old and the new name).
fn log_records(out: &str) -> Vec<(Value, Vec<&str>)> {
    out.split('\x1e').filter_map(|rec| {
        // With -z the header and every name end in a NUL (so any name is read back exactly).
        let mut fields = rec.split('\0');
        let p: Vec<_> = fields.next()?.trim_matches('\n').split('\x1f').collect();
        if p.len() != 5 { return None; }
        let names: Vec<&str> = fields.map(|f| f.trim_start_matches('\n')).filter(|f| !f.is_empty()).collect();
        // `first`: where the history begins (nothing earlier exists, so it cannot be undone).
        // `merge`: it combined the changes of two devices (it cannot be undone on its own either).
        let parents = p[4].split_whitespace().count();
        Some((json!({"hash": p[0], "short": p[1], "subject": p[2], "date": p[3], "kind": commit_kind(p[2]), "first": parents == 0, "merge": parents > 1}), names))
    }).collect()
}

/// The newest `want` changes to one file, newest first, each with `files: [name]` and `path`: the
/// name the file had at that point. The file is followed back through every rename and move: the
/// ones git recognises by their content, and the ones made in the app (`subject::moved`), which are
/// followed even when the move also changed much of a short note (its links, for instance).
async fn file_history(state: &AppState, dir: &Path, path: &str, want: usize) -> anyhow::Result<Vec<Value>> {
    let mut found: Vec<Value> = vec![];
    let mut name = path.to_string();
    // Where the part of the history still to read ends: everything (`None`), or what came before a move.
    let mut before: Option<String> = None;
    // A move leads to an earlier name, at an earlier change: this ends. (The bound is only a guard.)
    for _ in 0..1000 {
        let n = format!("-{}", want - found.len());
        let spec = format!(":(top,literal){name}");
        let mut args = vec!["log", n.as_str(), "-z", LOG_FORMAT, "--follow", "-M", "--name-status"];
        if let Some(rev) = &before { args.push(rev.as_str()); }
        args.extend(["--", spec.as_str()]);
        let out = git(state, dir, &args).await?;
        // The name changes, going back, wherever git saw a rename.
        let mut now = name.clone();
        let mut moved_from: Option<(String, String)> = None;
        for (mut entry, names) in log_records(&out) {
            let status = names.first().copied().unwrap_or("");
            let renamed = (status.starts_with('R') || status.starts_with('C')) && names.len() >= 3;
            let at = if renamed { names[2].to_string() } else { names.get(1).map(|n| n.to_string()).unwrap_or_else(|| now.clone()) };
            let added = status.starts_with('A');
            entry["files"] = json!([at]);
            entry["path"] = json!(at);
            // Added here by a move made in the app: older versions are under the name it came from.
            let app_move = if added && entry["first"] != true { subject::moved_from(entry["subject"].as_str().unwrap_or(""), &at) } else { None };
            let hash = entry["hash"].as_str().unwrap_or_default().to_string();
            found.push(entry);
            if renamed { now = names[1].to_string(); }
            if let Some(old) = app_move { moved_from = Some((hash, old)); break; }
        }
        let Some((hash, old)) = moved_from else { break };
        if found.len() >= want || !valid_hash(&hash) { break; }
        before = Some(format!("{hash}^"));
        name = old;
    }
    Ok(found)
}

/// The bytes of library file `rel` as it was at commit `hash`.
pub async fn file_at(state: &AppState, dir: &Path, rel: &str, hash: &str) -> anyhow::Result<Vec<u8>> {
    if !valid_hash(hash) { return Err(crate::tr::err("history.bad_entry", &[])); }
    let rel = rel.trim_start_matches("./").trim_start_matches('/').replace('\\', "/");
    git_raw(state, dir, &["show", &format!("{hash}:{rel}")]).await.map_err(|e| {
        if e.downcast_ref::<crate::tr::UserErr>().is_some() { e } else { crate::tr::err("not_found.version", &[]) }
    })
}

/// Put one file back as it was at `hash`, as a new history entry. Anything not yet recorded is
/// recorded first, so the version being replaced can itself be restored.
pub async fn restore_file(state: &AppState, dir: &Path, rel: &str, hash: &str) -> anyhow::Result<Value> {
    restore_file_from(state, dir, rel, hash, None).await
}

/// `restore_file`, for a version from before the file was renamed or moved: `was` is the name it
/// had at `hash` (the `path` of that history entry). The text goes back into `rel`, where the
/// file is now.
pub async fn restore_file_from(state: &AppState, dir: &Path, rel: &str, hash: &str, was: Option<&str>) -> anyhow::Result<Value> {
    let _g = state.git_lock.lock().await;
    crate::trust::require_git(dir)?;
    let mut bytes = file_at(state, dir, was.unwrap_or(rel), hash).await?;
    // A version from before a move has its links written from where the note was then: they are
    // pointed again at what they meant, from where it is now.
    if let Some(was) = was.filter(|w| *w != rel && crate::links::is_markdown(rel)) {
        if let Ok(text) = String::from_utf8(bytes.clone()) {
            let exists = |p: &str| crate::library::resolve(dir, p).map_or(false, |a| std::fs::symlink_metadata(a).is_ok());
            let (relinked, n) = crate::links::relink_moved(&text, was, rel, &exists);
            if n > 0 { crate::log::info(format!("history: {n} link(s) of the restored version pointed again from its new place")); bytes = relinked.into_bytes(); }
        }
    }
    commit_all(state, dir, &subject::edit(&format!("changes before restoring {rel}"))).await?;
    let target = crate::library::resolve(dir, rel)?;
    if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
    crate::paths::write_atomic(&target, &bytes, false)?;
    let short: String = hash.chars().take(7).collect();
    let committed = commit_all(state, dir, &subject::restore(rel, &short)).await?;
    crate::log::info(format!("history: restored a file from {short} (new entry: {committed})"));
    Ok(json!({"ok": true, "changed": committed, "hash": head(state, dir).await}))
}

/// Undo one whole change, as a new history entry. Fails with `CONFLICT::<file;file;…>` (and leaves
/// the library exactly as it was) when later changes build on it.
pub async fn undo_commit(state: &AppState, dir: &Path, hash: &str) -> anyhow::Result<Value> {
    if !valid_hash(hash) { return Err(crate::tr::err("history.bad_entry", &[])); }
    let _g = state.git_lock.lock().await;
    crate::trust::require_git(dir)?;
    let commit = format!("{hash}^{{commit}}");
    let parents = git(state, dir, &["rev-list", "--parents", "-n", "1", "--end-of-options", &commit]).await
        .map_err(|_| crate::tr::err("history.bad_entry", &[]))?;
    match parents.split_whitespace().count() {
        // The very first entry is the library as it was when its history began: undoing it would
        // mean removing every file it lists.
        0 | 1 => return Err(crate::tr::err("history.first", &[])),
        2 => {}
        _ => return Err(crate::tr::err("history.merge", &[])),
    }
    let subject = git(state, dir, &["log", "-1", "--format=%s", "--end-of-options", &commit]).await?.trim().to_string();
    commit_all(state, dir, &subject::edit("changes before undoing an earlier change")).await?;
    if let Err(e) = git(state, dir, &["revert", "--no-commit", "--end-of-options", hash]).await {
        let files = unmerged(state, dir).await;
        let _ = git(state, dir, &["revert", "--abort"]).await;
        // Everything was recorded just above, so this only clears the half-applied undo.
        let _ = git(state, dir, &["reset", "-q", "--hard", "HEAD"]).await;
        crate::log::warn(format!("history: undo of {hash} clashes with later changes in {} file(s): {e}", files.len()));
        if files.is_empty() { return Err(crate::tr::err_detail("history.undo_failed", &[], format!("{e:#}"))); }
        return Err(crate::tr::err("conflict", &[("files", &files.join(";"))]));
    }
    if git(state, dir, &["diff", "--cached", "--quiet"]).await.is_ok() {
        let _ = git(state, dir, &["revert", "--quit"]).await;
        return Err(crate::tr::err("history.nothing", &[]));
    }
    let done = git_ident(state, dir, &["commit", "-q", "-m", &subject::undo(&subject)]).await;
    let _ = git(state, dir, &["revert", "--quit"]).await;
    done?;
    crate::log::info(format!("history: undid {hash}"));
    Ok(json!({"ok": true, "hash": head(state, dir).await}))
}

/// Paths `git status` reports as untracked (new since the last commit), from `--porcelain -z` output.
pub fn untracked_from_status(porcelain_z: &str) -> Vec<String> {
    let mut out = vec![];
    let mut it = porcelain_z.split('\0');
    while let Some(entry) = it.next() {
        if entry.len() < 4 { continue; }
        let (xy, path) = entry.split_at(3);
        if xy.starts_with("??") { out.push(path.to_string()); }
        // Renames and copies carry their source path as an extra field.
        if xy.starts_with('R') || xy.starts_with('C') { it.next(); }
    }
    out
}

/// Put the working tree back exactly as it was at commit `snapshot`: every file that changed since
/// gets its recorded version again and files created since are removed — one by one, never by
/// resetting the folder, and never anything git was ignoring. Returns the created paths that were removed.
pub async fn rollback_to(state: &AppState, dir: &Path, snapshot: &str) -> anyhow::Result<Vec<String>> {
    if snapshot.len() < 7 || !snapshot.bytes().all(|b| b.is_ascii_hexdigit()) { anyhow::bail!("invalid snapshot"); }
    let full = git(state, dir, &["rev-parse", "--verify", "-q", &format!("{snapshot}^{{commit}}")]).await?;
    crate::changesets::restore_all(state, dir, full.trim()).await
}

pub async fn log(state: &AppState, dir: &Path, n: usize) -> Vec<Value> {
    let out = git(state, dir, &["log", &format!("-{n}"), "--format=%H%x1f%h%x1f%s%x1f%cI"]).await.unwrap_or_default();
    out.lines().filter_map(|l| {
        let p: Vec<_> = l.split('\x1f').collect();
        (p.len() == 4).then(|| json!({"hash": p[0], "short": p[1], "subject": p[2], "date": p[3]}))
    }).collect()
}

fn save_credential_file(f: &Path, host: &str, username: &str, secret: &str) -> anyhow::Result<()> {
    let existing = std::fs::read_to_string(f).unwrap_or_default();
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    let mut lines: Vec<String> = existing.lines().filter(|l| !l.ends_with(&format!("@{host}"))).map(String::from).collect();
    lines.push(format!("https://{}:{}@{host}", enc(username), enc(secret)));
    // Private from the moment it exists, and never half-written.
    crate::paths::write_atomic(f, (lines.join("\n") + "\n").as_bytes(), true)?;
    Ok(())
}

/// Store a credential for `host` (e.g. github.com).
pub async fn save_credential(state: &AppState, host: &str, username: &str, secret: &str) -> anyhow::Result<()> {
    let store = state.settings.read().await.sync.credential_store.clone();
    if store == "file" {
        save_credential_file(&crate::paths::git_credentials_file(), host, username, secret)?;
    } else {
        let v = json!({"username": username, "secret": secret}).to_string();
        // The keyring, or (Linux without one) the private secrets file — the password prompt helper
        // reads from the same place (see `askpass`).
        crate::secrets::set("thoughts-git", host, &v)?;
    }
    Ok(())
}

/// What a sync password for the address `url` is saved under: its host, with the port when the
/// address names one (`git.example.com:3000`) — exactly the host git asks for, so the keyring
/// entry, the line of the credentials file and the password prompt helper (`askpass`) all agree.
/// `None` for an address without `scheme://` (`git@host:path` signs in with a key, not a password).
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    // Only the part before the path: an `@` or `:` further on is not about the host.
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host).trim_end_matches(':');
    (!host.is_empty()).then(|| host.to_string())
}

/// The sign-in saved for `host` (see `host_of`): the username it was saved with (empty when only an
/// access token was given) and where it is kept, `keychain` or `file`. Never the password itself.
pub fn saved_sign_in(host: &str) -> Option<(String, &'static str)> {
    for k in crate::askpass::keys_for(host) {
        if let Some(raw) = crate::secrets::get("thoughts-git", &k) {
            let v: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
            return Some((v["username"].as_str().unwrap_or_default().to_string(), "keychain"));
        }
    }
    file_sign_in(&crate::paths::git_credentials_file(), host).map(|u| (u, "file"))
}

/// The username of the line for `host` in the credentials file (`https://user:pass@host`).
fn file_sign_in(f: &Path, host: &str) -> Option<String> {
    let text = std::fs::read_to_string(f).ok()?;
    let decode = |s: &str| {
        let b = s.as_bytes();
        let mut out = vec![];
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) { out.push(v); i += 3; continue; }
            }
            out.push(b[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).to_string()
    };
    text.lines().find_map(|l| {
        let (cred, h) = l.trim().strip_prefix("https://")?.rsplit_once('@')?;
        (h.trim_end_matches('/') == host).then(|| decode(cred.split(':').next().unwrap_or_default()))
    })
}

/// Take out every line of the credentials file that signs in to `host`.
fn forget_credential_file(f: &Path, host: &str) -> anyhow::Result<()> {
    let Ok(existing) = std::fs::read_to_string(f) else { return Ok(()) };
    let lines: Vec<&str> = existing.lines().filter(|l| !l.trim_end_matches('/').ends_with(&format!("@{host}"))).collect();
    if lines.len() == existing.lines().count() { return Ok(()); }
    let text = if lines.is_empty() { String::new() } else { lines.join("\n") + "\n" };
    crate::paths::write_atomic(f, text.as_bytes(), true)?;
    Ok(())
}

/// Forget the sign-in saved for `host`, wherever it is kept: the system keychain, the private
/// password file and, for the usual port 443, the entry saved under the bare host — unless one of
/// the hosts in `keep` still signs in with that one.
pub fn forget_sign_in(host: &str, keep: &[String]) -> anyhow::Result<()> {
    let needed: std::collections::BTreeSet<String> = keep.iter().filter(|h| *h != host).flat_map(|h| crate::askpass::keys_for(h)).collect();
    for k in crate::askpass::keys_for(host) {
        if k != host && needed.contains(&k) { continue; }
        crate::secrets::delete("thoughts-git", &k)?;
        forget_credential_file(&crate::paths::git_credentials_file(), &k)?;
    }
    Ok(())
}

/// Ask the location `name` whether it lets this computer in, without sending or receiving
/// anything: `None` when it does, otherwise why not (a key of `tr`'s table).
pub async fn check_location(state: &AppState, dir: &Path, name: &str, kind: &str) -> Option<&'static str> {
    if kind == "folder" {
        let url = remotes(state, dir).await.iter().find(|r| r["name"] == name).and_then(|r| r["url"].as_str().map(String::from)).unwrap_or_default();
        if !folder_present(&url).await { return Some("sync.why.unplugged"); }
    }
    match git(state, dir, &["ls-remote", "-q", "--heads", name]).await {
        Ok(_) => None,
        Err(e) => Some(why_not_sent(&e, kind)),
    }
}

/// A name for the backup kept in a folder: the library folder's own name, cut down to what every
/// drive accepts (FAT and exFAT drives, Windows, network shares).
fn backup_folder_name(library: &Path) -> String {
    let raw = library.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let clean: String = raw.chars().filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c)).take(60).collect();
    let clean = clean.trim().trim_matches('.').trim().to_string();
    format!("{}.git", if clean.is_empty() { "Thoughts" } else { &clean })
}

/// Does `path` hold a backup the way the app keeps one in a folder (a bare repository)?
fn is_bare_repo(path: &Path) -> bool {
    path.join("HEAD").is_file() && path.join("objects").is_dir() && path.join("refs").is_dir() && !path.join(".git").exists()
}

/// Make the folder `folder` (on an external drive, a network share or elsewhere on this computer)
/// a backup location named `name` of the library in `dir`. The backup is kept inside it as one
/// folder, `<library name>.git` (a bare repository), created when it is not there yet. One that is
/// already there is used again when it is an earlier backup of this same library — after a
/// reinstall, or on a new computer whose library is still empty, where the next Sync brings the
/// library back from it. A backup of a different library is never mixed in. Returns its path.
pub async fn add_folder_location(state: &AppState, dir: &Path, name: &str, folder: &Path) -> anyhow::Result<PathBuf> {
    let shown = folder.display().to_string();
    if !folder.is_absolute() || !folder.is_dir() { return Err(crate::tr::err("backup.folder_missing", &[("path", &shown)])); }
    let real = crate::paths::canonical_lenient(folder);
    if crate::paths::inside_ci(&real, dir) { return Err(crate::tr::err("backup.folder_in_library", &[])); }
    if let Some(service) = crate::cloud::service_of(&real) {
        let service = if service.is_empty() { crate::tr::text("backup.some_drive", &[]) } else { service.to_string() };
        return Err(crate::tr::err("backup.folder_in_cloud", &[("service", &service)]));
    }
    // The user may have chosen the backup itself rather than the folder holding it.
    let repo = if is_bare_repo(&real) { real.clone() } else { real.join(backup_folder_name(dir)) };
    if crate::paths::inside_ci(&repo, dir) { return Err(crate::tr::err("backup.folder_in_library", &[])); }
    let repo_s = repo.to_string_lossy().to_string();
    let _g = state.git_lock.lock().await;
    if repo.exists() {
        if !is_bare_repo(&repo) {
            return Err(crate::tr::err("backup.folder_taken", &[("name", &repo.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())]));
        }
        // An earlier backup is only taken on when it is this library's: it shares its history (or
        // one of the two is still empty).
        let theirs = git(state, dir, &["ls-remote", "-q", "--heads", &repo_s]).await.map_err(|_| crate::tr::err("backup.folder_unreadable", &[]))?;
        if !theirs.trim().is_empty() && head(state, dir).await.is_some() {
            let branch = current_branch(state, dir).await?;
            let related = match fetch_tip(state, dir, &repo_s, &branch).await {
                Ok(Some(tip)) => git(state, dir, &["merge-base", "HEAD", &tip]).await.is_ok(),
                _ => false,
            };
            if !related { return Err(crate::tr::err("backup.folder_other_library", &[])); }
        }
    } else {
        git(state, &real, &["init", "--bare", "-q", "-b", "main", &repo_s]).await.map_err(|e| {
            crate::log::warn(format!("backup: could not create a backup in {shown}: {e}"));
            crate::tr::err("backup.folder_unwritable", &[])
        })?;
    }
    git(state, dir, &["remote", "add", "--", name, &repo_s]).await?;
    crate::log::info("backup: a folder was added as a backup location");
    Ok(repo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Plain git for arranging test repositories (the code under test goes through `git`).
    fn sh(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git").current_dir(dir)
            .args(["-c", "user.name=Tester", "-c", "user.email=tester@example.com", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
            .args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-git-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn read(dir: &Path, rel: &str) -> String { std::fs::read_to_string(dir.join(rel)).unwrap() }

    #[test]
    fn a_quiet_command_can_never_ask_anything() {
        let mut c = crate::proc::command("git");
        c.env("SSH_ASKPASS", "/some/helper").env("GIT_ASKPASS", "/other/helper");
        make_quiet(&mut c);
        let envs: std::collections::HashMap<String, Option<String>> = c.as_std().get_envs()
            .map(|(k, v)| (k.to_string_lossy().to_string(), v.map(|v| v.to_string_lossy().to_string()))).collect();
        assert_eq!(envs.get("SSH_ASKPASS"), Some(&None), "SSH's prompt helper is taken away");
        assert_eq!(envs.get("GIT_ASKPASS"), Some(&None));
        assert_eq!(envs.get("SSH_ASKPASS_REQUIRE"), Some(&Some("never".into())));
        assert_eq!(envs.get("GCM_INTERACTIVE"), Some(&Some("never".into())));
        let args: Vec<String> = c.as_std().get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.iter().any(|a| a.starts_with("core.sshCommand=") && a.contains("BatchMode=yes")), "{args:?}");
        // The app's own prompt helper: always for a Sync, for a quiet command only when allowed, never with the file.
        assert!(askpass_given("keychain", false, false));
        assert!(!askpass_given("keychain", true, false));
        assert!(askpass_given("keychain", true, true));
        assert!(!askpass_given("file", false, true) && !askpass_given("file", true, true));
    }

    /// A bare "cloud" repository and two clones of it: `mine` (the library) and `other` (another device).
    /// Both have changed notes.md differently; `other` has already pushed, and also added b.md.
    fn diverged() -> (PathBuf, PathBuf, PathBuf) {
        let root = tmp("sync");
        let (remote, mine, other) = (root.join("remote.git"), root.join("mine"), root.join("other"));
        std::fs::create_dir_all(&remote).unwrap();
        sh(&remote, &["init", "-q", "--bare", "-b", "main"]);
        sh(&root, &["clone", "-q", remote.to_str().unwrap(), "mine"]);
        for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com")] { sh(&mine, &["config", k, v]); }
        write(&mine, "notes.md", "line one\n");
        write(&mine, "keep.md", "untouched\n");
        sh(&mine, &["add", "-A"]);
        sh(&mine, &["commit", "-q", "-m", "first"]);
        sh(&mine, &["push", "-q", "-u", "origin", "HEAD:main"]);
        sh(&root, &["clone", "-q", remote.to_str().unwrap(), "other"]);
        write(&other, "notes.md", "their line\n");
        write(&other, "b.md", "from the other device\n");
        sh(&other, &["add", "-A"]);
        sh(&other, &["commit", "-q", "-m", "theirs"]);
        sh(&other, &["push", "-q", "origin", "HEAD:main"]);
        write(&mine, "notes.md", "my line\n");
        (root, mine, remote)
    }

    #[tokio::test]
    async fn sync_without_conflict_pulls_and_pushes() {
        let (root, mine, remote) = diverged();
        let st = AppState::load().unwrap();
        // Take their version first so there is nothing to conflict with, then change another file.
        sh(&mine, &["checkout", "-q", "--", "notes.md"]);
        write(&mine, "keep.md", "changed here\n");
        let v = sync(&st, &mine).await.unwrap();
        assert_eq!(v["committed"], true);
        // Edits not recorded yet are the person's own, not something that arrived.
        assert_eq!(sh(&mine, &["log", "-1", "--format=%s"]), "edit: your changes");
        assert_eq!(read(&mine, "notes.md"), "their line\n");
        assert_eq!(read(&mine, "b.md"), "from the other device\n");
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&mine, &["rev-parse", "HEAD"]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_sync_that_rebases_works_on_a_computer_where_git_does_not_know_the_user() {
        // Most computers that are not a developer's (and the Linux and Windows test machines): git
        // was never told a name and cannot make one up from the computer's name.
        TEST_NO_IDENTITY.with(|c| c.set(true));
        let (root, mine, remote) = diverged();
        for k in ["user.name", "user.email"] { sh(&mine, &["config", "--unset", k]); }
        let st = AppState::load().unwrap();
        sh(&mine, &["checkout", "-q", "--", "notes.md"]);
        write(&mine, "keep.md", "changed here\n");
        let v = sync(&st, &mine).await.unwrap();
        assert_eq!(v["committed"], true);
        assert_eq!((read(&mine, "notes.md").as_str(), read(&mine, "b.md").as_str()), ("their line\n", "from the other device\n"));
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&mine, &["rev-parse", "HEAD"]));
        // Recorded and replayed on top of theirs as the app, with nothing half-done left behind.
        assert_eq!(sh(&mine, &["log", "-1", "--format=%an <%ae> / %cn <%ce>"]), "Thoughts <thoughts@localhost> / Thoughts <thoughts@localhost>");
        assert!(!mine.join(".git/CHERRY_PICK_HEAD").exists() && !mine.join(".git/rebase-merge").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_sync_only_succeeds_when_the_library_really_reached_every_location() {
        let (root, mine, remote) = diverged();
        let st = AppState::load().unwrap();
        sh(&mine, &["checkout", "-q", "--", "notes.md"]);
        assert_eq!(last_backup(&mine), Value::Null);
        // One location, and it takes the library: that is a backup, and its time is noted.
        let v = sync(&st, &mine).await.unwrap();
        assert_eq!((v["received"].as_bool(), &v["locations"]), (Some(true), &json!([{"name": "origin", "sent": true}])));
        assert_eq!(v["lastBackup"]["location"], "origin");
        let noted = last_backup(&mine);
        assert!(noted["at"].as_str().unwrap().ends_with('Z'));
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&mine, &["rev-parse", "HEAD"]));
        // A second location that holds other work refuses the library. The sync says so, by name
        // and with the reason, instead of "synced" — and says where it did arrive.
        let elsewhere = root.join("zeta.git");
        std::fs::create_dir_all(&elsewhere).unwrap();
        sh(&elsewhere, &["init", "-q", "--bare", "-b", "main"]);
        sh(&root, &["clone", "-q", elsewhere.to_str().unwrap(), "elsewhere-work"]);
        let work = root.join("elsewhere-work");
        write(&work, "unrelated.md", "x\n");
        sh(&work, &["add", "-A"]);
        sh(&work, &["commit", "-q", "-m", "unrelated"]);
        sh(&work, &["push", "-q", "origin", "HEAD:main"]);
        sh(&mine, &["remote", "add", "zeta", elsewhere.to_str().unwrap()]);
        write(&mine, "keep.md", "changed here\n");
        let before = sh(&elsewhere, &["rev-parse", "main"]);
        let err = sync(&st, &mine).await.unwrap_err();
        let shown = crate::tr::classify_in(false, &err);
        assert_eq!((shown.code, shown.status), ("sync_failed", 409));
        assert_eq!(shown.message, "Your library was sent to \"origin\". It was not sent to \"zeta\": it holds changes that this computer does not have, so it did not accept yours.");
        assert_eq!(sh(&elsewhere, &["rev-parse", "main"]), before);
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&mine, &["rev-parse", "HEAD"]), "the location that works still got it");
        assert_eq!(last_backup(&mine)["location"], "origin");
        // No location at all: nothing was backed up, and that is what the answer says.
        sh(&mine, &["remote", "remove", "zeta"]);
        sh(&mine, &["remote", "remove", "origin"]);
        write(&mine, "keep.md", "changed again\n");
        let err = sync(&st, &mine).await.unwrap_err();
        assert_eq!(crate::tr::classify_in(false, &err).code, "no_location");
        assert_eq!(sh(&mine, &["status", "--porcelain"]), "", "the change is still recorded in the history");
        assert!(resolve(&st, &mine, "mine").await.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn adding_a_second_location_does_not_change_where_sync_receives_from() {
        // `origin` holds work from another device (see `diverged`) that this computer has not received yet.
        let (root, mine, remote) = diverged();
        let st = AppState::for_test();
        sh(&mine, &["checkout", "-q", "--", "notes.md"]);
        // A second location is added. Its name ("backup", what the app calls it) sorts before "origin".
        let extra = root.join("backup.git");
        std::fs::create_dir_all(&extra).unwrap();
        sh(&extra, &["init", "-q", "--bare", "-b", "main"]);
        sh(&mine, &["remote", "add", "backup", extra.to_str().unwrap()]);
        let listed = |rs: &[Value]| rs.iter().map(|r| (r["name"].as_str().unwrap().to_string(), r["main"] == true)).collect::<Vec<_>>();
        assert_eq!(listed(&remotes(&st, &mine).await), [("origin".to_string(), true), ("backup".to_string(), false)], "in the order added; the first is the main one");
        // Sync still receives from origin, and sends to both.
        let v = sync(&st, &mine).await.unwrap();
        assert_eq!(v["received"], true);
        assert_eq!(read(&mine, "b.md"), "from the other device\n");
        let here = sh(&mine, &["rev-parse", "HEAD"]);
        assert_eq!((sh(&remote, &["rev-parse", "main"]), sh(&extra, &["rev-parse", "main"])), (here.clone(), here));
        // What the branch follows is the main location, so "to send / to receive" are counted against it.
        assert_eq!(sh(&mine, &["config", "branch.main.remote"]), "origin");
        let s = status(&st, &mine).await;
        assert_eq!((s["ahead"].as_u64(), s["behind"].as_u64()), (Some(0), Some(0)));

        // "Make main" is an explicit choice. From then on changes are received from that location.
        st.settings.write().await.sync.main_location = Some("backup".into());
        assert_eq!(listed(&remotes(&st, &mine).await), [("origin".to_string(), false), ("backup".to_string(), true)]);
        sh(&root, &["clone", "-q", extra.to_str().unwrap(), "third"]);
        let third = root.join("third");
        write(&third, "c.md", "sent to the backup location\n");
        sh(&third, &["add", "-A"]);
        sh(&third, &["commit", "-q", "-m", "third"]);
        sh(&third, &["push", "-q", "origin", "HEAD:main"]);
        sync(&st, &mine).await.unwrap();
        assert_eq!(read(&mine, "c.md"), "sent to the backup location\n");
        assert_eq!(sh(&mine, &["config", "branch.main.remote"]), "backup");
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&mine, &["rev-parse", "HEAD"]), "origin is still sent to");
        // The chosen location is removed: the first one added is the main one again.
        sh(&mine, &["remote", "remove", "backup"]);
        assert_eq!(listed(&remotes(&st, &mine).await), [("origin".to_string(), true)]);
        assert!(sync(&st, &mine).await.is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_main_location_is_chosen_or_the_first_added_never_by_name() {
        assert_eq!(pick_main(&["origin", "backup"], None), Some("origin"));
        assert_eq!(pick_main(&["github", "backup"], None), Some("github"));
        assert_eq!(pick_main(&["origin", "backup"], Some("backup")), Some("backup"));
        assert_eq!(pick_main(&["origin", "backup"], Some("gone")), Some("origin"));
        assert_eq!(pick_main(&[], Some("origin")), None);
    }

    /// Git LFS turns large files into placeholders when they are recorded and sends the files
    /// themselves from a step the app never runs: the library is not sent rather than sent hollow.
    #[tokio::test]
    async fn a_library_that_keeps_files_with_git_lfs_is_never_sent_without_them() {
        let (root, mine, remote) = diverged();
        let st = AppState::for_test();
        sh(&mine, &["checkout", "-q", "--", "notes.md"]);
        write(&mine, ".gitattributes", "# large files\n*.pdf filter=lfs diff=lfs merge=lfs -text\n");
        write(&mine, "big.pdf", "%PDF the whole file\n");
        sh(&mine, &["add", "-A"]);
        sh(&mine, &["commit", "-q", "-m", "a large file"]);
        // The filter is normally set up in this computer's own settings (`git lfs install`); here
        // in the library's, which is what its git sees just the same. Empty means not set up.
        sh(&mine, &["config", "filter.lfs.process", ""]);
        sh(&mine, &["config", "filter.lfs.clean", ""]);
        assert!(!lfs_in_use(&st, &mine).await, "rules alone, without the filter, record files whole");
        sh(&mine, &["config", "filter.lfs.clean", "git-lfs clean -- %f"]);
        assert!(lfs_in_use(&st, &mine).await);
        let before = sh(&remote, &["rev-parse", "main"]);
        let rs = remotes(&st, &mine).await;
        let mut log = vec![];
        let sent = push_all(&st, &mine, &rs, "main", &mut log).await;
        assert_eq!(sent, [("origin".to_string(), Some("sync.why.lfs"))]);
        assert_eq!(sh(&remote, &["rev-parse", "main"]), before, "nothing was sent");
        let shown = crate::tr::classify_in(false, &sync_outcome(&mine, &sent).unwrap_err());
        assert_eq!(shown.code, "sync_failed");
        assert!(shown.message.starts_with("It was not sent to \"origin\": this library keeps some files with Git LFS"), "{}", shown.message);
        assert_eq!(backup_health(&st, &mine).await["locations"][0]["whyKey"], "sync.why.lfs");
        // A rule that is only a comment hands nothing to LFS.
        write(&mine, ".gitattributes", "# *.pdf filter=lfs\n");
        assert!(!lfs_in_use(&st, &mine).await);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_location_that_cannot_be_written_to_is_not_a_backup() {
        use std::os::unix::fs::PermissionsExt;
        let (root, mine, remote) = diverged();
        let st = AppState::load().unwrap();
        sh(&mine, &["checkout", "-q", "--", "notes.md"]);
        write(&mine, "keep.md", "changed here\n");
        // Reading from the location works (so changes are received), writing to it does not.
        let lock = |mode: u32| for d in ["objects", "refs/heads", "."] { std::fs::set_permissions(remote.join(d), std::fs::Permissions::from_mode(mode)).unwrap(); };
        lock(0o555);
        let before = sh(&remote, &["rev-parse", "main"]);
        let err = sync(&st, &mine).await.unwrap_err();
        lock(0o755);
        let shown = crate::tr::classify_in(false, &err);
        assert_eq!(shown.code, "sync_failed");
        assert!(shown.message.starts_with("It was not sent to \"origin\":"), "{}", shown.message);
        assert_eq!(sh(&remote, &["rev-parse", "main"]), before);
        assert_eq!(last_backup(&mine), Value::Null, "no backup time is noted for a sync that sent nothing");
        // What was received is in the library all the same.
        assert_eq!(read(&mine, "b.md"), "from the other device\n");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn why_a_location_did_not_take_the_library() {
        let why = |raw: &str| why_not_sent(&anyhow::anyhow!("git push -u -- origin main: {raw}"), "online");
        assert_eq!(why("remote: Invalid username or password.\nfatal: Authentication failed for 'https://github.com/a/b.git/'"), "sync.why.sign_in");
        assert_eq!(why("fatal: could not read Username for 'https://github.com': terminal prompts disabled"), "sync.why.sign_in");
        assert_eq!(why("fatal: unable to access 'https://x/': Could not resolve host: x"), "sync.why.offline");
        assert_eq!(why("! [rejected] main -> main (fetch first)\nerror: failed to push some refs"), "sync.why.rejected");
        assert_eq!(why("! [rejected] main -> main (non-fast-forward)"), "sync.why.rejected");
        assert_eq!(why("remote: error: File big.pdf is 150.00 MB; this exceeds GitHub's file size limit of 100.00 MB"), "sync.why.too_large");
        assert_eq!(why("remote: error: GH006: Protected branch update failed for refs/heads/main."), "sync.why.refused");
        assert_eq!(why_not_sent(&crate::tr::err("git.timeout", &[]), "online"), "sync.why.timeout");
        // An SSH location: the server's key comes first, and a refused key is not about a password.
        let ssh = |raw: &str| why_not_sent(&anyhow::anyhow!("git push -u -- origin main: {raw}"), "ssh");
        assert_eq!(ssh("Host key verification failed.\nfatal: Could not read from remote repository."), "sync.why.host_key");
        assert_eq!(ssh("@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@\nHost key verification failed."), "sync.why.host_changed");
        assert_eq!(ssh("git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository."), "sync.why.ssh_key");
        // A drive or folder.
        let drive = |raw: &str| why_not_sent(&anyhow::anyhow!("git push -u -- usb main: {raw}"), "folder");
        assert_eq!(drive("fatal: '/Volumes/Backup/notes.git' does not appear to be a git repository\nfatal: Could not read from remote repository."), "sync.why.unplugged");
        assert_eq!(drive("error: unable to create temporary object directory: No space left on device"), "sync.why.full");
        assert_eq!(drive("error: insufficient permission for adding an object to repository database ./objects\nPermission denied"), "sync.why.no_write");
        // An online address that is not there (or that the account cannot see).
        assert_eq!(why("remote: Repository not found.\nfatal: repository 'https://github.com/a/b.git/' not found"), "sync.why.not_found");
        // Every reason has its sentence in both languages.
        for key in ["sync.why.sign_in", "sync.why.offline", "sync.why.timeout", "sync.why.rejected", "sync.why.too_large", "sync.why.refused",
                    "sync.why.unplugged", "sync.why.full", "sync.why.no_write", "sync.why.host_key", "sync.why.host_changed", "sync.why.ssh_key",
                    "sync.why.not_found", "sync.why.receive", "sync.why.lfs"] {
            assert!(!crate::tr::text_in(false, key, &[]).contains("unexpected") && crate::tr::text_in(true, key, &[]) != crate::tr::text_in(false, key, &[]), "{key}");
        }
    }

    #[tokio::test]
    async fn conflicting_sync_is_undone_and_reported() {
        let (root, mine, remote) = diverged();
        let st = AppState::load().unwrap();
        let remote_before = sh(&remote, &["rev-parse", "main"]);
        let err = sync(&st, &mine).await.unwrap_err().to_string();
        assert_eq!(err, "CONFLICT::notes.md");
        // Back on the branch with the local version intact, nothing half-finished, nothing pushed.
        assert_eq!(sh(&mine, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
        assert_eq!(read(&mine, "notes.md"), "my line\n");
        assert_eq!(sh(&mine, &["status", "--porcelain"]), "");
        assert!(!mine.join(".git/rebase-merge").exists() && !mine.join(".git/rebase-apply").exists());
        assert_eq!(sh(&remote, &["rev-parse", "main"]), remote_before);
        std::fs::remove_dir_all(root).unwrap();
    }

    async fn resolves(strategy: &str, expect: &str) {
        let (root, mine, remote) = diverged();
        let st = AppState::load().unwrap();
        assert!(sync(&st, &mine).await.unwrap_err().to_string().starts_with("CONFLICT::"));
        let local_before = sh(&mine, &["rev-parse", "HEAD"]);
        let v = resolve(&st, &mine, strategy).await.unwrap();
        assert_eq!(read(&mine, "notes.md"), expect);
        // What did not conflict is kept from both sides.
        assert_eq!(read(&mine, "b.md"), "from the other device\n");
        assert_eq!(read(&mine, "keep.md"), "untouched\n");
        // The backup branch holds the local state from before, and stays on this machine.
        let backup = v["backup"].as_str().unwrap();
        assert!(backup.starts_with("backup/"));
        assert_eq!(sh(&mine, &["rev-parse", backup]), local_before);
        assert_eq!(sh(&remote, &["branch", "--list"]).replace('*', "").trim(), "main");
        // The sync completed.
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&mine, &["rev-parse", "HEAD"]));
        assert_eq!(sh(&mine, &["status", "--porcelain"]), "");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn resolve_keeping_mine() { resolves("mine", "my line\n").await }

    #[tokio::test]
    async fn resolve_keeping_theirs() { resolves("theirs", "their line\n").await }

    #[tokio::test]
    async fn resolve_handles_a_file_deleted_on_one_side() {
        let (root, mine, _remote) = diverged();
        let st = AppState::load().unwrap();
        // Locally the file is deleted; the other device edited it.
        std::fs::remove_file(mine.join("notes.md")).unwrap();
        assert!(sync(&st, &mine).await.unwrap_err().to_string().starts_with("CONFLICT::"));
        resolve(&st, &mine, "theirs").await.unwrap();
        assert_eq!(read(&mine, "notes.md"), "their line\n");
        assert_eq!(sh(&mine, &["status", "--porcelain"]), "");
        assert!(resolve(&st, &mine, "neither").await.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// FEAT-40: each clashing document settled its own way, with nothing chosen for the person.
    #[tokio::test]
    async fn each_clashing_document_is_settled_as_chosen() {
        let (root, mine, remote) = diverged();
        let st = AppState::for_test();
        // A second clash, in a document the rebase never reaches (it stops at the first one), and a
        // third with changes to different parts on each side.
        let other = root.join("other");
        write(&other, "plan.md", "intro\ntheir middle\nend\n");
        write(&other, "INDEX.md", "- a\n- their line\n- c\n- d\n- e\n- f\n- g\n- h\n");
        sh(&other, &["add", "-A"]);
        sh(&other, &["commit", "-q", "-m", "theirs again"]);
        sh(&other, &["push", "-q", "origin", "HEAD:main"]);
        sh(&mine, &["commit", "-q", "-am", "my notes"]);
        write(&mine, "plan.md", "intro\nmy middle\nend\n");
        write(&mine, "INDEX.md", "- a\n- my line\n- c\n- d\n- e\n- f\n- g\n- my last\n");
        sh(&mine, &["add", "-A"]);
        sh(&mine, &["commit", "-q", "-m", "my plan"]);
        let err = sync(&st, &mine).await.unwrap_err().to_string();
        let mut asked: Vec<&str> = err.strip_prefix("CONFLICT::").unwrap().split(';').collect();
        asked.sort();
        assert_eq!(asked, ["INDEX.md", "notes.md", "plan.md"], "every document that clashes is asked about at once");
        // A choice is missing: nothing is changed, and all of them are asked about again.
        let some: std::collections::HashMap<String, String> = [("notes.md".to_string(), "theirs".to_string())].into();
        let before = sh(&mine, &["rev-parse", "HEAD"]);
        let again = resolve_each(&st, &mine, None, &some).await.unwrap_err().to_string();
        assert!(again.starts_with("CONFLICT::") && again.contains("plan.md") && again.contains("INDEX.md"), "{again}");
        assert_eq!(sh(&mine, &["rev-parse", "HEAD"]), before);
        assert_eq!(sh(&mine, &["status", "--porcelain"]), "");
        assert!(!mine.join(".git/MERGE_HEAD").exists());
        // Every one chosen: theirs, both, mine.
        let all: std::collections::HashMap<String, String> = [("notes.md", "theirs"), ("plan.md", "both"), ("INDEX.md", "mine")]
            .iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let bad: std::collections::HashMap<String, String> = [("notes.md".to_string(), "everything".to_string())].into();
        assert!(resolve_each(&st, &mine, None, &bad).await.is_err());
        let v = resolve_each(&st, &mine, None, &all).await.unwrap();
        assert_eq!(read(&mine, "notes.md"), "their line\n");
        assert_eq!(read(&mine, "plan.md"), "intro\nmy middle\nend\n");
        assert_eq!(read(&mine, "plan (other device).md"), "intro\ntheir middle\nend\n");
        // Added on both sides: mine, whole.
        assert_eq!(read(&mine, "INDEX.md"), "- a\n- my line\n- c\n- d\n- e\n- f\n- g\n- my last\n");
        let copy = v["settled"].as_array().unwrap().iter().find(|s| s["path"] == "plan.md").unwrap();
        assert_eq!((copy["choice"].as_str(), copy["copy"].as_str()), (Some("both"), Some("plan (other device).md")));
        assert_eq!(read(&mine, "b.md"), "from the other device\n");
        assert!(v["backup"].as_str().unwrap().starts_with("backup/"));
        assert_eq!(sh(&mine, &["rev-parse", v["backup"].as_str().unwrap()]), before);
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&mine, &["rev-parse", "HEAD"]));
        assert_eq!(sh(&mine, &["status", "--porcelain"]), "");
        // The combining entry says what it is, and is marked as one in the history.
        let newest = &history(&st, &mine, None, 1).await.unwrap()[0];
        assert_eq!((newest["subject"].as_str(), newest["merge"].as_bool(), newest["kind"].as_str()), (Some(subject::combined().as_str()), Some(true), Some("sync")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn mine_keeps_the_other_sides_changes_elsewhere_in_the_same_document() {
        let (root, mine, _remote) = diverged();
        let st = AppState::for_test();
        let other = root.join("other");
        write(&other, "long.md", "1\n2\n3\n4\n5\n6\n7\n8\n");
        sh(&other, &["add", "-A"]);
        sh(&other, &["commit", "-q", "-m", "long"]);
        sh(&other, &["push", "-q", "origin", "HEAD:main"]);
        sh(&mine, &["checkout", "-q", "--", "notes.md"]);
        assert!(sync(&st, &mine).await.is_ok());
        write(&other, "long.md", "1\nTHEIRS\n3\n4\n5\n6\n7\nTHEIR END\n");
        sh(&other, &["commit", "-q", "-am", "theirs"]);
        sh(&other, &["push", "-q", "origin", "HEAD:main"]);
        write(&mine, "long.md", "1\nMINE\n3\n4\n5\n6\n7\n8\n");
        assert!(sync(&st, &mine).await.unwrap_err().to_string().starts_with("CONFLICT::long.md"));
        let v = resolve(&st, &mine, "mine").await.unwrap();
        assert_eq!(read(&mine, "long.md"), "1\nMINE\n3\n4\n5\n6\n7\nTHEIR END\n");
        assert_eq!(v["strategy"], "mine");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_version_from_before_a_move_comes_back_with_its_links_pointing_from_the_new_place() {
        let d = dunce::canonicalize(tmp("relink")).unwrap();
        sh(&d, &["init", "-q", "-b", "main"]);
        for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com")] { sh(&d, &["config", k, v]); }
        let st = AppState::for_test();
        write(&d, "INDEX.md", "# Index\n");
        write(&d, "ideas/todo.md", "- [ ] a\n");
        write(&d, "ideas/plan.md", "# Plan\n[index](../INDEX.md) [todo](todo.md) [web](https://example.com)\n");
        commit_all(&st, &d, &subject::edit("your changes")).await.unwrap();
        let old = head(&st, &d).await.unwrap();
        // Moved one folder deeper in the app (its links rewritten by the move), then edited.
        std::fs::create_dir_all(d.join("ideas/later")).unwrap();
        std::fs::remove_file(d.join("ideas/plan.md")).unwrap();
        write(&d, "ideas/later/plan.md", "# Plan\n[index](../../INDEX.md) [todo](../todo.md) [web](https://example.com)\nnewer\n");
        commit_all(&st, &d, &subject::moved("ideas/plan.md", "ideas/later/plan.md")).await.unwrap();
        restore_file_from(&st, &d, "ideas/later/plan.md", &old, Some("ideas/plan.md")).await.unwrap();
        assert_eq!(read(&d, "ideas/later/plan.md"), "# Plan\n[index](../../INDEX.md) [todo](../todo.md) [web](https://example.com)\n");
        // A version from where it is now is put back as it was, byte for byte.
        let moved = sh(&d, &["rev-parse", "HEAD~1"]);
        restore_file_from(&st, &d, "ideas/later/plan.md", &moved, Some("ideas/later/plan.md")).await.unwrap();
        assert!(read(&d, "ideas/later/plan.md").ends_with("newer\n"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_second_copy_is_named_for_the_other_device() {
        let taken = |c: &str| ["notes/plan (other device).md", "README (other device)"].contains(&c);
        assert_eq!(other_copy_name("notes/plan.md", "other device", taken), "notes/plan (other device 2).md");
        assert_eq!(other_copy_name("TODO.md", "الجهاز الآخر", taken), "TODO (الجهاز الآخر).md");
        assert_eq!(other_copy_name("README", "other device", taken), "README (other device 2)");
        assert_eq!(other_copy_name("a.b/.hidden", "other device", taken), "a.b/.hidden (other device)");
    }

    #[tokio::test]
    async fn internal_branches_are_never_synced() {
        for (b, internal) in [("reorganize/20260101-000000", true), ("backup/20260101-000000", true), ("staging", true), ("wrap/0b6c2c6e-3f43-4a51-9d2e-6c1d8f2f0a11", true), ("main", false), ("backups", false), ("my/staging", false)] {
            assert_eq!(internal_branch(b), internal, "{b}");
        }
        let (root, mine, remote) = diverged();
        let st = AppState::load().unwrap();
        let before = sh(&remote, &["rev-parse", "main"]);
        sh(&mine, &["checkout", "-q", "-b", "backup/20260101-000000"]);
        assert!(sync(&st, &mine).await.unwrap_err().to_string().contains("never syncs"));
        assert!(resolve(&st, &mine, "mine").await.is_err());
        assert_eq!(sh(&remote, &["branch", "--list"]).replace('*', "").trim(), "main");
        assert_eq!(sh(&remote, &["rev-parse", "main"]), before);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rollback_restores_the_snapshot_and_removes_only_new_files() {
        let d = tmp("rollback");
        sh(&d, &["init", "-q", "-b", "main"]);
        write(&d, ".gitignore", "private/\n");
        write(&d, "ideas/a/README.md", "original\n");
        write(&d, "TODO.md", "- [ ] one\n");
        write(&d, "private/keys.txt", "ignored, must survive\n");
        let st = AppState::load().unwrap();
        assert!(commit_all(&st, &d, "snapshot before wrap").await.unwrap());
        let snap = head(&st, &d).await.unwrap();
        // A wrap that died halfway: edits, a deletion, new files (one with a space), a new folder.
        write(&d, "ideas/a/README.md", "half rewritten");
        std::fs::remove_file(d.join("TODO.md")).unwrap();
        write(&d, "ideas/b/README.md", "new idea\n");
        write(&d, "ideas/a/notes/new note.md", "new\n");
        write(&d, "private/more.txt", "also ignored\n");
        let mut created = rollback_to(&st, &d, &snap).await.unwrap();
        created.sort();
        assert_eq!(created, ["ideas/a/notes/new note.md", "ideas/b/README.md"]);
        assert_eq!(read(&d, "ideas/a/README.md"), "original\n");
        assert_eq!(read(&d, "TODO.md"), "- [ ] one\n");
        assert!(!d.join("ideas/b/README.md").exists() && !d.join("ideas/a/notes/new note.md").exists());
        assert_eq!(read(&d, "private/keys.txt"), "ignored, must survive\n");
        assert_eq!(read(&d, "private/more.txt"), "also ignored\n");
        assert_eq!(sh(&d, &["status", "--porcelain"]), "");
        assert_eq!(head(&st, &d).await.unwrap(), snap);
        assert!(rollback_to(&st, &d, "HEAD; rm -rf").await.is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[tokio::test]
    async fn resolve_without_a_real_clash_makes_no_backup() {
        let (root, mine, remote) = diverged();
        let st = AppState::load().unwrap();
        // The other device changed notes.md; here only keep.md changed: both merge cleanly.
        sh(&mine, &["checkout", "-q", "--", "notes.md"]);
        write(&mine, "keep.md", "changed here\n");
        let v = resolve(&st, &mine, "mine").await.unwrap();
        assert!(v["backup"].is_null(), "{v}");
        assert_eq!(sh(&mine, &["branch", "--list", "backup/*"]), "");
        assert_eq!(read(&mine, "notes.md"), "their line\n");
        assert_eq!(read(&mine, "keep.md"), "changed here\n");
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&mine, &["rev-parse", "HEAD"]));
        // Called again with nothing at all to settle: still no backup.
        let v = resolve(&st, &mine, "theirs").await.unwrap();
        assert!(v["backup"].is_null());
        assert_eq!(sh(&mine, &["branch", "--list", "backup/*"]), "");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn only_the_newest_backups_are_kept() {
        let (root, mine, _remote) = diverged();
        let st = AppState::load().unwrap();
        for i in 1..=14 { sh(&mine, &["branch", &format!("backup/202601{i:02}-000000")]); }
        sh(&mine, &["branch", "backups-of-mine"]);
        assert_eq!(prune_backups(&st, &mine).await, 4);
        let left = sh(&mine, &["branch", "--list", "backup/*", "--format=%(refname:short)"]);
        let left: Vec<&str> = left.lines().collect();
        assert_eq!(left.len(), KEEP_BACKUPS);
        assert_eq!(left[0], "backup/20260105-000000");
        assert_eq!(sh(&mine, &["branch", "--list", "backups-of-mine", "--format=%(refname:short)"]), "backups-of-mine");
        // A real clash adds one and the list stays capped.
        assert!(sync(&st, &mine).await.unwrap_err().to_string().starts_with("CONFLICT::"));
        let v = resolve(&st, &mine, "mine").await.unwrap();
        assert!(v["backup"].as_str().unwrap().starts_with("backup/"));
        assert_eq!(sh(&mine, &["branch", "--list", "backup/*"]).lines().count(), KEEP_BACKUPS);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn history_lists_kinds_and_single_files_can_be_restored_or_changes_undone() {
        let d = dunce::canonicalize(tmp("history")).unwrap();
        sh(&d, &["init", "-q", "-b", "main"]);
        for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com")] { sh(&d, &["config", k, v]); }
        let st = AppState::load().unwrap();
        assert!(history(&st, &d, None, 50).await.unwrap().is_empty());
        write(&d, "notes/plan.md", "version one\n");
        write(&d, "TODO.md", "- [ ] a\n");
        commit_all(&st, &d, &subject::edit("your changes")).await.unwrap();
        write(&d, "notes/plan.md", "version two\n");
        commit_all(&st, &d, &subject::assistant("Plan the launch")).await.unwrap();
        write(&d, "TODO.md", "- [ ] a\n- [ ] b\n");
        commit_all(&st, &d, &subject::wrap(2)).await.unwrap();
        write(&d, "other.md", "x\n");
        commit_all(&st, &d, &subject::sync()).await.unwrap();
        sh(&d, &["commit", "-q", "--allow-empty", "-m", "typed by hand"]);

        let all = history(&st, &d, None, 50).await.unwrap();
        let kinds: Vec<&str> = all.iter().map(|h| h["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["other", "sync", "wrap", "assistant", "edit"]);
        assert_eq!(all[2]["files"], json!(["TODO.md"]));
        assert_eq!(all[4]["files"], json!(["TODO.md", "notes/plan.md"]));
        assert!(all[0]["hash"].as_str().unwrap().len() >= 40 && all[0]["date"].as_str().unwrap().contains('T'));
        assert_eq!(history(&st, &d, None, 2).await.unwrap().len(), 2);
        // One file's own history.
        let plan = history(&st, &d, Some("notes/plan.md"), 50).await.unwrap();
        assert_eq!(plan.iter().map(|h| h["kind"].as_str().unwrap()).collect::<Vec<_>>(), ["assistant", "edit"]);
        let first = plan[1]["hash"].as_str().unwrap().to_string();
        assert_eq!(file_at(&st, &d, "notes/plan.md", &first).await.unwrap(), b"version one\n");
        assert!(file_at(&st, &d, "notes/missing.md", &first).await.is_err());
        assert!(file_at(&st, &d, "notes/plan.md", "HEAD~1").await.is_err());
        assert!(file_at(&st, &d, "notes/plan.md", "--output=/tmp/x").await.is_err());

        // Restore: the old text is back as a new entry; what was not yet recorded is recorded first.
        write(&d, "notes/plan.md", "version three, not recorded yet\n");
        let v = restore_file(&st, &d, "notes/plan.md", &first).await.unwrap();
        assert_eq!(v["changed"], true);
        assert_eq!(read(&d, "notes/plan.md"), "version one\n");
        let plan = history(&st, &d, Some("notes/plan.md"), 50).await.unwrap();
        assert!(plan[0]["subject"].as_str().unwrap().starts_with("restore: notes/plan.md"));
        assert_eq!(plan[0]["kind"], "edit");
        let unrecorded = plan[1]["hash"].as_str().unwrap();
        assert_eq!(file_at(&st, &d, "notes/plan.md", unrecorded).await.unwrap(), b"version three, not recorded yet\n");
        assert!(restore_file(&st, &d, ".git/config", &first).await.is_err());

        // Undo a whole change: the wrap's task disappears, everything else stays.
        let wrap = all[2]["hash"].as_str().unwrap();
        undo_commit(&st, &d, wrap).await.unwrap();
        assert_eq!(read(&d, "TODO.md"), "- [ ] a\n");
        assert_eq!(read(&d, "other.md"), "x\n");
        let newest = &history(&st, &d, None, 1).await.unwrap()[0];
        assert!(newest["subject"].as_str().unwrap().starts_with("undo: wrap: "));
        assert!(undo_commit(&st, &d, "not-a-hash").await.is_err());
        assert!(undo_commit(&st, &d, "0123456789abcdef").await.is_err());

        // A change that later ones build on cannot be undone alone: reported, and nothing is changed.
        write(&d, "notes/plan.md", "rewritten again\n");
        commit_all(&st, &d, "later").await.unwrap();
        let before = sh(&d, &["rev-parse", "HEAD"]);
        let assistant = all[3]["hash"].as_str().unwrap();
        let err = undo_commit(&st, &d, assistant).await.unwrap_err().to_string();
        assert_eq!(err, "CONFLICT::notes/plan.md");
        assert_eq!(read(&d, "notes/plan.md"), "rewritten again\n");
        assert_eq!(sh(&d, &["status", "--porcelain"]), "");
        assert_eq!(sh(&d, &["rev-parse", "HEAD"]), before);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn commit_kinds_timeouts_and_helper_quoting() {
        for (s, kind) in [("wrap: 2026-11-10 — 3 chat(s)", "wrap"), ("sync: 2026-11-10 09:00", "sync"), ("Merge branch 'main' of github.com:a/b", "sync"),
                          ("assistant: Plan the launch", "assistant"), ("module: docs", "assistant"), ("init: library configured for Sara", "assistant"),
                          ("reorganize: proposed structure", "assistant"), ("edit: your changes", "edit"), ("snapshot before wrap", "edit"),
                          ("restore: a.md (from 1234567)", "edit"), ("undo: wrap: x", "edit"), ("typed by hand", "other"), ("", "other")] {
            assert_eq!(commit_kind(s), kind, "{s}");
        }
        assert_eq!(commit_kind(&subject::assistant("a\nb")), "assistant");
        assert!(!subject::assistant(&"x".repeat(500)).contains('\n') && subject::assistant(&"x".repeat(500)).len() < 100);
        for net in [&["push", "-u", "--", "origin", "main"][..], &["-c", "user.name=T", "-c", "user.email=t@x", "pull", "--rebase"], &["fetch", "--", "origin"], &["clone", "--depth", "1"]] {
            assert_eq!(timeout_for(net), NETWORK_TIMEOUT, "{net:?}");
        }
        for local in [&["status", "--porcelain"][..], &["commit", "-q", "-m", "push"], &["log", "-1"], &["-c", "x=y", "merge", "fetch"]] {
            assert_eq!(timeout_for(local), LOCAL_TIMEOUT, "{local:?}");
        }
        assert_eq!(credential_helper(Path::new("/home/a b/.thoughts/git-credentials")), "store --file=\"/home/a b/.thoughts/git-credentials\"");
        assert_eq!(credential_helper(Path::new(r"C:\Users\Sara Ali\.thoughts\git-credentials")), "store --file=\"C:/Users/Sara Ali/.thoughts/git-credentials\"");
        assert_eq!(credential_helper(Path::new("/x/$HOME`id`\"")), "store --file=\"/x/\\$HOME\\`id\\`\\\"\"");
        assert!(valid_hash("0123abc") && !valid_hash("012") && !valid_hash("HEAD") && !valid_hash("-n1") && !valid_hash("abc..def"));
    }

    #[tokio::test]
    async fn a_missing_folder_is_a_plain_error() {
        let st = AppState::load().unwrap();
        let gone = std::env::temp_dir().join(format!("thoughts-git-gone-{}", uuid::Uuid::new_v4()));
        let err = git(&st, &gone, &["status"]).await.unwrap_err();
        let shown = crate::tr::classify_in(false, &err);
        assert_eq!(shown.status, 404);
        assert!(shown.message.contains(&gone.display().to_string()) && !shown.message.contains("os error"));
    }

    #[tokio::test]
    async fn names_that_are_not_ascii_are_listed_as_they_are() {
        let d = dunce::canonicalize(tmp("names")).unwrap();
        sh(&d, &["init", "-q", "-b", "main"]);
        let st = AppState::load().unwrap();
        write(&d, "أفكار/متجر/خطة العمل.md", "one\n");
        write(&d, "café/naïve.md", "x\n");
        commit_all(&st, &d, &subject::edit("your changes")).await.unwrap();
        let h = history(&st, &d, None, 10).await.unwrap();
        assert_eq!(h[0]["files"], json!(["café/naïve.md", "أفكار/متجر/خطة العمل.md"]));
        // The name a history entry gives is the name the file can be read back by.
        let hash = h[0]["hash"].as_str().unwrap();
        assert_eq!(file_at(&st, &d, "أفكار/متجر/خطة العمل.md", hash).await.unwrap(), b"one\n");
        assert_eq!(history(&st, &d, Some("أفكار/متجر/خطة العمل.md"), 10).await.unwrap().len(), 1);
        // The reorganize review reads names from these two commands.
        write(&d, "أفكار/متجر/خطة العمل.md", "two\n");
        commit_all(&st, &d, &subject::edit("your changes")).await.unwrap();
        for args in [&["diff", "--stat", "HEAD~1", "HEAD", "--"][..], &["diff", "HEAD~1", "HEAD", "--"], &["status", "--porcelain"]] {
            let out = git(&st, &d, args).await.unwrap();
            assert!(!out.contains("\\330") && !out.contains("\\303"), "{args:?}: {out}");
        }
        assert!(git(&st, &d, &["diff", "HEAD~1", "HEAD", "--"]).await.unwrap().contains("أفكار/متجر/خطة العمل.md"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[tokio::test]
    async fn a_library_inside_another_repository_never_touches_that_repository() {
        // A notes vault (or a dotfiles folder) that is a repository, with the library in a sub-folder.
        let vault = dunce::canonicalize(tmp("vault")).unwrap();
        sh(&vault, &["init", "-q", "-b", "main"]);
        for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com")] { sh(&vault, &["config", k, v]); }
        write(&vault, "dotfiles.txt", "mine\n");
        write(&vault, "thoughts/INDEX.md", "# Index\n");
        sh(&vault, &["add", "-A"]);
        sh(&vault, &["commit", "-q", "-m", "vault"]);
        // Work in progress elsewhere in the vault, which is none of the app's business.
        write(&vault, "dotfiles.txt", "edited, not committed\n");
        write(&vault, "scratch.txt", "new\n");
        let vault_head = sh(&vault, &["rev-parse", "HEAD"]);
        let lib = vault.join("thoughts");
        let st = AppState::load().unwrap();

        // The vault's repository is not the library's: nothing is read from it or written to it.
        assert!(!is_repo(&st, &lib).await);
        assert_eq!(status(&st, &lib).await["repo"], false);
        assert_eq!(enclosing_repo(&lib), Some(vault.clone()));
        assert_eq!(enclosing_repo(&vault), None);
        assert!(head(&st, &lib).await.is_none() && remotes(&st, &lib).await.is_empty());
        assert!(commit_all(&st, &lib, "sync: must not reach the vault").await.is_err());
        assert!(sync(&st, &lib).await.is_err());
        assert!(rollback_to(&st, &lib, &vault_head).await.is_err());
        assert_eq!(sh(&vault, &["rev-parse", "HEAD"]), vault_head);
        assert_eq!(sh(&vault, &["diff", "--cached", "--name-only"]), "", "nothing of the vault was staged");
        assert_eq!(read(&vault, "dotfiles.txt"), "edited, not committed\n");

        // The library gets a history of its own, kept by the app, that knows only the library's files.
        start_history(&st, &lib).await.unwrap();
        let Repo::Kept(store) = repo_of(&lib) else { panic!("expected a history kept by the app") };
        assert!(is_repo(&st, &lib).await && !lib.join(".git").exists());
        let h = history(&st, &lib, None, 10).await.unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0]["files"], json!(["INDEX.md"]));
        // A wrap that fails halfway puts the library back — and only the library.
        let snap = head(&st, &lib).await.unwrap();
        write(&lib, "INDEX.md", "half rewritten");
        write(&lib, "ideas/new/README.md", "new idea\n");
        let created = rollback_to(&st, &lib, &snap).await.unwrap();
        assert_eq!(created, ["ideas/new/README.md"]);
        assert_eq!(read(&lib, "INDEX.md"), "# Index\n");
        assert_eq!(read(&vault, "dotfiles.txt"), "edited, not committed\n");
        assert_eq!(read(&vault, "scratch.txt"), "new\n");
        assert_eq!(sh(&vault, &["rev-parse", "HEAD"]), vault_head);
        assert_eq!(sh(&vault, &["log", "--oneline"]).lines().count(), 1);
        assert_eq!(sh(&vault, &["diff", "--cached", "--name-only"]), "");
        std::fs::remove_dir_all(store).unwrap();
        std::fs::remove_dir_all(vault).unwrap();
    }

    #[tokio::test]
    async fn a_history_kept_by_the_app_syncs_restores_and_undoes_like_any_other() {
        // Nobody ever tells the history the app keeps who the user is: on a computer whose git
        // does not know either, it must still sync (this is how the Linux and Windows test machines are).
        TEST_NO_IDENTITY.with(|c| c.set(true));
        let root = dunce::canonicalize(tmp("kept")).unwrap();
        let (remote, lib, other) = (root.join("remote.git"), root.join("My Library"), root.join("other"));
        std::fs::create_dir_all(&remote).unwrap();
        sh(&remote, &["init", "-q", "--bare", "-b", "main"]);
        write(&lib, "notes.md", "one\n");
        write(&lib, "keep.md", "untouched\n");
        let st = AppState::load().unwrap();
        // Where the history goes depends on the folder alone, and is the same however it is spelled.
        assert_eq!(repo_of(&lib), Repo::Missing);
        assert_eq!(history_store(&lib), history_store(&lib.join("../My Library/.")));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&lib, root.join("shortcut")).unwrap();
            assert_eq!(history_store(&lib), history_store(&root.join("shortcut")));
        }
        assert_ne!(history_store(&lib), history_store(&other));
        assert!(history_store(&lib).file_name().unwrap().to_string_lossy().starts_with("MyLibrary-"));
        assert!(ensure_history(&st, &lib).await.unwrap());
        assert!(!ensure_history(&st, &lib).await.unwrap(), "a second call finds it there");
        let Repo::Kept(store) = repo_of(&lib) else { panic!("expected a history kept by the app") };
        assert!(head(&st, &lib).await.is_none());
        start_history(&st, &lib).await.unwrap();
        let first = head(&st, &lib).await.unwrap();
        start_history(&st, &lib).await.unwrap();
        assert_eq!(head(&st, &lib).await.unwrap(), first, "the starting point is recorded once");
        assert_eq!(status(&st, &lib).await["kept"], true);
        // Nothing of git's is ever put in the library folder.
        let names = |d: &Path| { let mut v: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect(); v.sort(); v };
        assert_eq!(names(&lib), ["keep.md", "notes.md"]);

        // An online location works from that history exactly as from a repository in the folder.
        git(&st, &lib, &["remote", "add", "--", "origin", remote.to_str().unwrap()]).await.unwrap();
        sync(&st, &lib).await.unwrap();
        assert_eq!(sh(&remote, &["rev-parse", "main"]), first);
        sh(&root, &["clone", "-q", remote.to_str().unwrap(), "other"]);
        write(&other, "notes.md", "their line\n");
        write(&other, "b.md", "from the other device\n");
        sh(&other, &["add", "-A"]);
        sh(&other, &["commit", "-q", "-m", "theirs"]);
        sh(&other, &["push", "-q", "origin", "HEAD:main"]);
        write(&lib, "keep.md", "changed here\n");
        sync(&st, &lib).await.unwrap();
        assert_eq!((read(&lib, "notes.md").as_str(), read(&lib, "b.md").as_str()), ("their line\n", "from the other device\n"));
        assert_eq!(sh(&remote, &["rev-parse", "main"]), head(&st, &lib).await.unwrap());
        // A clash is reported and undone, then settled, the same way too.
        write(&other, "notes.md", "theirs again\n");
        sh(&other, &["pull", "-q", "origin", "main"]);
        sh(&other, &["commit", "-q", "-am", "theirs again"]);
        sh(&other, &["push", "-q", "origin", "HEAD:main"]);
        write(&lib, "notes.md", "mine\n");
        assert_eq!(sync(&st, &lib).await.unwrap_err().to_string(), "CONFLICT::notes.md");
        assert_eq!(read(&lib, "notes.md"), "mine\n");
        let v = resolve(&st, &lib, "mine").await.unwrap();
        assert!(v["backup"].as_str().unwrap().starts_with("backup/"));
        assert_eq!(read(&lib, "notes.md"), "mine\n");
        assert_eq!(sh(&remote, &["rev-parse", "main"]), head(&st, &lib).await.unwrap());

        // One file back to an earlier version; one whole change undone.
        assert_eq!(file_at(&st, &lib, "notes.md", &first).await.unwrap(), b"one\n");
        restore_file(&st, &lib, "notes.md", &first).await.unwrap();
        assert_eq!(read(&lib, "notes.md"), "one\n");
        let newest = history(&st, &lib, None, 1).await.unwrap()[0]["hash"].as_str().unwrap().to_string();
        undo_commit(&st, &lib, &newest).await.unwrap();
        assert_eq!(read(&lib, "notes.md"), "mine\n");
        assert_eq!(git(&st, &lib, &["status", "--porcelain"]).await.unwrap(), "");
        assert_eq!(names(&lib), ["b.md", "keep.md", "notes.md"]);
        std::fs::remove_dir_all(store).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_history_kept_by_the_app_gives_back_every_file_exactly_line_endings_and_all() {
        // Kept exactly on every computer; only a repository of the library's own on Windows
        // saves plain line endings (it may have been checked out by Git for Windows).
        for windows in [false, true] { assert_eq!(line_endings(&Repo::Kept(PathBuf::from("h")), windows), Some("false")); }
        assert_eq!((line_endings(&Repo::Own, true), line_endings(&Repo::Own, false)), (Some("input"), None));
        TEST_NO_IDENTITY.with(|c| c.set(true));
        let lib = dunce::canonicalize(tmp("eol")).unwrap();
        let notepad = "made in Notepad\r\nsecond line\r\n";
        write(&lib, "notes/windows.md", notepad);
        write(&lib, "plain.md", "plain\n");
        let st = AppState::load().unwrap();
        start_history(&st, &lib).await.unwrap();
        let Repo::Kept(store) = repo_of(&lib) else { panic!("expected a history kept by the app") };
        let first = head(&st, &lib).await.unwrap();
        assert_eq!(file_at(&st, &lib, "notes/windows.md", &first).await.unwrap(), notepad.as_bytes());
        // Changed, and that change undone: the file is back byte for byte, and nothing looks changed.
        write(&lib, "notes/windows.md", "rewritten\n");
        write(&lib, "plain.md", "rewritten\r\n");
        commit_all(&st, &lib, &subject::edit("your changes")).await.unwrap();
        let newest = history(&st, &lib, None, 1).await.unwrap()[0]["hash"].as_str().unwrap().to_string();
        undo_commit(&st, &lib, &newest).await.unwrap();
        assert_eq!((read(&lib, "notes/windows.md").as_str(), read(&lib, "plain.md").as_str()), (notepad, "plain\n"));
        assert_eq!(git(&st, &lib, &["status", "--porcelain"]).await.unwrap(), "");
        // One file put back from an earlier version: the same.
        write(&lib, "notes/windows.md", "again\n");
        restore_file(&st, &lib, "notes/windows.md", &first).await.unwrap();
        assert_eq!(read(&lib, "notes/windows.md"), notepad);
        std::fs::remove_dir_all(store).unwrap();
        std::fs::remove_dir_all(lib).unwrap();
    }

    #[test]
    fn untracked_paths_are_parsed_from_status() {
        let z = "?? new file.md\0 M changed.md\0R  renamed.md\0old ?? name.md\0?? dir/x.md\0A  staged.md\0";
        assert_eq!(untracked_from_status(z), ["new file.md", "dir/x.md"]);
    }

    #[test]
    fn credentials_file_is_private_and_replaces_per_host() {
        let d = tmp("cred");
        let f = d.join("git-credentials");
        save_credential_file(&f, "github.com", "me", "p@ss/1").unwrap();
        save_credential_file(&f, "gitlab.com", "me", "two").unwrap();
        save_credential_file(&f, "github.com", "me", "new").unwrap();
        assert_eq!(read(&d, "git-credentials"), "https://me:two@gitlab.com\nhttps://me:new@github.com\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_saved_password_is_keyed_by_host_and_port_the_way_git_looks_it_up() {
        for (url, host) in [("https://git.example.com:3000/sara/notes.git", Some("git.example.com:3000")), ("https://sara:secret@git.example.com:3000/x.git", Some("git.example.com:3000")),
                            ("https://github.com/sara/notes.git", Some("github.com")), ("https://gitlab.com/group/@scope/notes.git", Some("gitlab.com")),
                            ("ssh://git@git.example.com:2222/sara/notes.git", Some("git.example.com:2222")), ("https://git.example.com:/x", Some("git.example.com")),
                            ("git@github.com:sara/notes.git", None), ("https://", None)] {
            assert_eq!(host_of(url).as_deref(), host, "{url}");
        }
        // git itself finds the line saved for a self-hosted server on its own port — the address's
        // host and port — and a line saved without the port is not used for it. Only this file is
        // consulted: no configuration of this computer, no keychain, no prompt.
        let d = tmp("cred-port");
        let f = d.join("git-credentials");
        let url = "https://git.example.com:3000/sara/notes.git";
        let fill = |url: &str| {
            let mut c = std::process::Command::new("git");
            c.env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_default()).env("HOME", &d)
                .env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", d.join("none")).env("GIT_TERMINAL_PROMPT", "0")
                .args(["-c", "credential.helper=", "-c", &format!("credential.helper={}", credential_helper(&f)), "credential", "fill"])
                .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
            let mut child = c.spawn().unwrap();
            use std::io::Write;
            child.stdin.take().unwrap().write_all(format!("url={url}\n\n").as_bytes()).unwrap();
            let o = child.wait_with_output().unwrap();
            String::from_utf8_lossy(&o.stdout).lines().find_map(|l| l.strip_prefix("password=").map(String::from))
        };
        save_credential_file(&f, &host_of(url).unwrap(), "sara", "token-3000").unwrap();
        assert_eq!(fill(url).as_deref(), Some("token-3000"));
        std::fs::write(&f, "https://sara:bare@git.example.com\n").unwrap();
        assert_eq!(fill(url), None, "the port is part of what git matches");
        assert_eq!(fill("https://git.example.com/sara/notes.git").as_deref(), Some("bare"));
        std::fs::remove_dir_all(d).unwrap();
    }

    // ---------- drives, backup health, saved sign-ins (hist-b) ----------

    /// A library with one recorded change, as its own repository, and an empty folder standing in
    /// for an external drive.
    fn library_and_drive(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = tmp(name);
        let (lib, drive) = (root.join("Notes"), root.join("Backup Drive"));
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::create_dir_all(&drive).unwrap();
        sh(&lib, &["init", "-q", "-b", "main"]);
        for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com")] { sh(&lib, &["config", k, v]); }
        write(&lib, "notes.md", "first\n");
        sh(&lib, &["add", "-A"]);
        sh(&lib, &["commit", "-q", "-m", "first"]);
        (root, dunce::canonicalize(lib).unwrap(), dunce::canonicalize(drive).unwrap())
    }

    #[tokio::test]
    async fn a_drive_that_is_not_plugged_in_is_said_plainly_and_the_other_locations_still_get_the_library() {
        let (root, lib, drive) = library_and_drive("drive");
        let st = AppState::for_test();
        let repo = add_folder_location(&st, &lib, "usb", &drive).await.unwrap();
        assert_eq!(repo, drive.join("Notes.git"));
        assert_eq!(remotes(&st, &lib).await[0]["kind"], "folder");
        assert_eq!(backup_health(&st, &lib).await["state"], "stale", "never backed up");
        sync(&st, &lib).await.unwrap();
        assert_eq!(sh(&repo, &["rev-parse", "main"]), sh(&lib, &["rev-parse", "HEAD"]));
        let h = backup_health(&st, &lib).await;
        assert_eq!((h["state"].clone(), h["locations"][0]["present"].clone(), h["locations"][0]["unsent"].clone()), (json!("ok"), json!(true), json!(0)));
        // A second location, online in real life (a folder here, which stays connected).
        let other = root.join("other.git");
        std::fs::create_dir_all(&other).unwrap();
        sh(&other, &["init", "-q", "--bare", "-b", "main"]);
        sh(&lib, &["remote", "add", "online", other.to_str().unwrap()]);

        // The drive is unplugged: its folder is gone.
        let away = root.join("unplugged");
        std::fs::rename(&drive, &away).unwrap();
        write(&lib, "notes.md", "second\n");
        let err = sync(&st, &lib).await.unwrap_err();
        let shown = crate::tr::classify_in(false, &err);
        assert_eq!(shown.code, "sync_failed");
        assert!(shown.message.contains("\"online\"") && shown.message.contains("not sent to \"usb\": the drive or folder is not connected"), "{}", shown.message);
        assert_eq!(sh(&other, &["rev-parse", "main"]), sh(&lib, &["rev-parse", "HEAD"]), "the main location was a drive away: the other one still got the library");
        let h = backup_health(&st, &lib).await;
        let usb = h["locations"].as_array().unwrap().iter().find(|l| l["name"] == "usb").unwrap().clone();
        assert_eq!((h["state"].clone(), usb["whyKey"].clone(), usb["present"].clone(), usb["unsent"].clone()), (json!("failed"), json!("sync.why.unplugged"), json!(false), json!(1)));
        assert!(usb["why"].as_str().unwrap().starts_with("the drive or folder is not connected"));
        // Plugged in again: everything goes through.
        std::fs::rename(&away, &drive).unwrap();
        sync(&st, &lib).await.unwrap();
        assert_eq!(sh(&repo, &["rev-parse", "main"]), sh(&lib, &["rev-parse", "HEAD"]));
        assert_eq!(backup_health(&st, &lib).await["state"], "ok");
        // Removed and added again later: the backup already there is used again.
        sh(&lib, &["remote", "remove", "usb"]);
        forget_location_state(&lib, "usb");
        assert_eq!(add_folder_location(&st, &lib, "usb", &drive).await.unwrap(), repo);
        // Choosing the backup folder itself works as well.
        sh(&lib, &["remote", "remove", "usb"]);
        assert_eq!(add_folder_location(&st, &lib, "usb", &repo).await.unwrap(), repo);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A change that reached a second location while the main one was away is never replayed as a
    /// new change: that location would refuse the new one for good (the app never overwrites).
    #[tokio::test]
    async fn changes_a_second_location_already_has_are_combined_not_rewritten() {
        let (root, lib, drive) = library_and_drive("sent-twice");
        let st = AppState::for_test();
        let repo = add_folder_location(&st, &lib, "usb", &drive).await.unwrap();
        sync(&st, &lib).await.unwrap();
        let other = root.join("other.git");
        std::fs::create_dir_all(&other).unwrap();
        sh(&other, &["init", "-q", "--bare", "-b", "main"]);
        sh(&lib, &["remote", "add", "online", other.to_str().unwrap()]);
        sync(&st, &lib).await.unwrap();
        // The drive is away: a change made here reaches only the second location.
        let away = root.join("unplugged");
        std::fs::rename(&drive, &away).unwrap();
        write(&lib, "notes.md", "second\n");
        assert_eq!(crate::tr::classify_in(false, &sync(&st, &lib).await.unwrap_err()).code, "sync_failed");
        let sent = sh(&lib, &["rev-parse", "HEAD"]);
        assert_eq!(sh(&other, &["rev-parse", "main"]), sent);
        // Meanwhile another computer sent its own change to the drive.
        std::fs::rename(&away, &drive).unwrap();
        sh(&root, &["clone", "-q", repo.to_str().unwrap(), "laptop"]);
        let laptop = root.join("laptop");
        write(&laptop, "laptop.md", "from the laptop\n");
        sh(&laptop, &["add", "-A"]);
        sh(&laptop, &["commit", "-q", "-m", "laptop"]);
        sh(&laptop, &["push", "-q", "origin", "HEAD:main"]);
        // Plugged in again: both locations take the library, and the change they had stays as it was.
        let v = sync(&st, &lib).await.unwrap();
        assert_eq!(v["received"], true);
        assert_eq!((read(&lib, "laptop.md").as_str(), read(&lib, "notes.md").as_str()), ("from the laptop\n", "second\n"));
        let here = sh(&lib, &["rev-parse", "HEAD"]);
        sh(&lib, &["merge-base", "--is-ancestor", &sent, "HEAD"]);
        assert_eq!((sh(&repo, &["rev-parse", "main"]), sh(&other, &["rev-parse", "main"])), (here.clone(), here));
        // And the syncs after it go on as usual.
        write(&lib, "notes.md", "third\n");
        sync(&st, &lib).await.unwrap();
        assert_eq!(sh(&other, &["rev-parse", "main"]), sh(&lib, &["rev-parse", "HEAD"]));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The library folder was copied to this computer without its history (the history the app
    /// keeps stays behind), and given the online location the other computer already sends to:
    /// `(root, lib, remote, laptop)`. Both have notes.md (different), same.md (the same), and a
    /// document of their own.
    fn unrelated() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let root = tmp("unrelated");
        let (remote, laptop, lib) = (root.join("remote.git"), root.join("laptop"), root.join("Notes"));
        std::fs::create_dir_all(&remote).unwrap();
        sh(&remote, &["init", "-q", "--bare", "-b", "main"]);
        sh(&root, &["clone", "-q", remote.to_str().unwrap(), "laptop"]);
        write(&laptop, "notes.md", "from the laptop\n");
        write(&laptop, "same.md", "same\n");
        write(&laptop, "only-there.md", "laptop only\n");
        sh(&laptop, &["add", "-A"]);
        sh(&laptop, &["commit", "-q", "-m", "laptop"]);
        sh(&laptop, &["push", "-q", "origin", "HEAD:main"]);
        std::fs::create_dir_all(&lib).unwrap();
        sh(&lib, &["init", "-q", "-b", "main"]);
        for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com")] { sh(&lib, &["config", k, v]); }
        write(&lib, "notes.md", "edited here\n");
        write(&lib, "same.md", "same\n");
        write(&lib, "only-here.md", "here only\n");
        sh(&lib, &["add", "-A"]);
        sh(&lib, &["commit", "-q", "-m", "start"]);
        sh(&lib, &["remote", "add", "origin", remote.to_str().unwrap()]);
        (root, lib, remote, laptop)
    }

    #[tokio::test]
    async fn a_location_holding_a_library_this_one_did_not_come_from_is_never_mixed_in_without_asking() {
        let (root, lib, remote, laptop) = unrelated();
        let st = AppState::for_test();
        let (here, there) = (sh(&lib, &["rev-parse", "HEAD"]), sh(&remote, &["rev-parse", "main"]));
        assert_eq!(location_related(&st, &lib, "origin").await, Some(false));
        // Sync and settling a clash both ask first, and nothing is changed anywhere.
        for e in [sync(&st, &lib).await.unwrap_err(), resolve(&st, &lib, "mine").await.unwrap_err()] {
            let shown = crate::tr::classify_in(false, &e);
            assert_eq!(shown.code, "unrelated", "{}", shown.message);
            assert!(shown.message.contains("\"origin\""), "{}", shown.message);
        }
        assert_eq!((sh(&lib, &["rev-parse", "HEAD"]), sh(&remote, &["rev-parse", "main"])), (here.clone(), there.clone()));
        assert_eq!((read(&lib, "notes.md").as_str(), sh(&lib, &["status", "--porcelain"]).as_str()), ("edited here\n", ""));
        assert!(!lib.join("only-there.md").exists());
        assert!(join_unrelated(&st, &lib, "everything").await.is_err());
        // The safe choice: every document from both, this computer's kept first.
        let v = join_unrelated(&st, &lib, "combine").await.unwrap();
        assert_eq!(sh(&lib, &["rev-parse", v["backup"].as_str().unwrap()]), here);
        assert_eq!(read(&lib, "notes.md"), "edited here\n");
        assert_eq!(read(&lib, "notes (other device).md"), "from the laptop\n");
        assert_eq!(v["copies"], json!(["notes (other device).md"]));
        assert_eq!((read(&lib, "same.md").as_str(), read(&lib, "only-here.md").as_str(), read(&lib, "only-there.md").as_str()), ("same\n", "here only\n", "laptop only\n"));
        assert!(!lib.join("same (other device).md").exists(), "a document that is the same on both is kept once");
        sh(&lib, &["merge-base", "--is-ancestor", &here, "HEAD"]);
        sh(&lib, &["merge-base", "--is-ancestor", &there, "HEAD"]);
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&lib, &["rev-parse", "HEAD"]));
        assert_eq!(sh(&lib, &["status", "--porcelain"]), "");
        assert_eq!(location_related(&st, &lib, "origin").await, Some(true));
        // From now on both computers sync as usual.
        sh(&laptop, &["pull", "-q", "--ff-only", "origin", "main"]);
        assert_eq!(read(&laptop, "only-here.md"), "here only\n");
        write(&lib, "same.md", "changed here later\n");
        sync(&st, &lib).await.unwrap();
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&lib, &["rev-parse", "HEAD"]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_library_can_start_from_its_online_copy_and_keep_every_document_here() {
        let (root, lib, remote, laptop) = unrelated();
        let st = AppState::for_test();
        let (here, there) = (sh(&lib, &["rev-parse", "HEAD"]), sh(&remote, &["rev-parse", "main"]));
        let v = join_unrelated(&st, &lib, "theirs").await.unwrap();
        // This computer's state is kept; the online copy's history is the library's from now on.
        assert_eq!(sh(&lib, &["rev-parse", v["backup"].as_str().unwrap()]), here);
        assert_eq!(sh(&lib, &["rev-parse", "HEAD~1"]), there);
        assert_eq!(sh(&lib, &["log", "-1", "--format=%s"]), "edit: your changes");
        // Where they differ this computer's version is kept (the other stays in the history), and
        // nothing is removed: the documents only the online copy had are put in the folder.
        assert_eq!((read(&lib, "notes.md").as_str(), read(&lib, "only-here.md").as_str(), read(&lib, "only-there.md").as_str()), ("edited here\n", "here only\n", "laptop only\n"));
        assert_eq!(sh(&lib, &["show", &format!("{there}:notes.md")]), "from the laptop");
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&lib, &["rev-parse", "HEAD"]));
        assert_eq!(sh(&lib, &["status", "--porcelain"]), "");
        sh(&laptop, &["pull", "-q", "--ff-only", "origin", "main"]);
        assert_eq!(read(&laptop, "notes.md"), "edited here\n");
        // Asked again once they are one library: an ordinary sync.
        assert!(join_unrelated(&st, &lib, "combine").await.unwrap()["backup"].is_null());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A clash settled here whose result could not be sent is not undone by the next sync, and the
    /// person is not asked about it again.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_settled_clash_that_could_not_be_sent_is_not_asked_about_again() {
        use std::os::unix::fs::PermissionsExt;
        let (root, mine, remote) = diverged();
        let st = AppState::for_test();
        assert!(sync(&st, &mine).await.unwrap_err().to_string().starts_with("CONFLICT::"));
        let lock = |mode: u32| for d in ["objects", "refs/heads", "."] { std::fs::set_permissions(remote.join(d), std::fs::Permissions::from_mode(mode)).unwrap(); };
        lock(0o555);
        assert_eq!(crate::tr::classify_in(false, &resolve(&st, &mine, "mine").await.unwrap_err()).code, "sync_failed");
        let settled = sh(&mine, &["rev-parse", "HEAD"]);
        assert_eq!(read(&mine, "notes.md"), "my line\n");
        // Still not taken, with nothing new there: the settled result stays exactly as it is.
        let again = sync(&st, &mine).await.unwrap_err();
        lock(0o755);
        assert_eq!(crate::tr::classify_in(false, &again).code, "sync_failed", "{again}");
        assert_eq!(sh(&mine, &["rev-parse", "HEAD"]), settled);
        // The other device sends something else meanwhile: it is put together with the settled result.
        let other = root.join("other");
        write(&other, "c.md", "later\n");
        sh(&other, &["add", "-A"]);
        sh(&other, &["commit", "-q", "-m", "later"]);
        sh(&other, &["push", "-q", "origin", "HEAD:main"]);
        sync(&st, &mine).await.unwrap();
        assert_eq!((read(&mine, "notes.md").as_str(), read(&mine, "c.md").as_str()), ("my line\n", "later\n"));
        sh(&mine, &["merge-base", "--is-ancestor", &settled, "HEAD"]);
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&mine, &["rev-parse", "HEAD"]));
        assert_eq!(sh(&mine, &["status", "--porcelain"]), "");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_drive_brings_a_library_back_and_never_mixes_in_another_one() {
        let (root, lib, drive) = library_and_drive("restore");
        let st = AppState::for_test();
        add_folder_location(&st, &lib, "usb", &drive).await.unwrap();
        sync(&st, &lib).await.unwrap();
        // A new computer: an empty library folder of the same name, given a history, and the drive.
        let fresh = root.join("new computer").join("Notes");
        std::fs::create_dir_all(&fresh).unwrap();
        init(&st, &fresh).await.unwrap();
        add_folder_location(&st, &fresh, "usb", &drive).await.unwrap();
        let v = sync(&st, &fresh).await.unwrap();
        assert_eq!(v["received"], true);
        assert_eq!(read(&fresh, "notes.md"), "first\n", "the library came back from the drive");
        // A different library that happens to have the same name is refused.
        let stranger = root.join("someone else").join("Notes");
        std::fs::create_dir_all(&stranger).unwrap();
        sh(&stranger, &["init", "-q", "-b", "main"]);
        write(&stranger, "theirs.md", "not yours\n");
        sh(&stranger, &["add", "-A"]);
        sh(&stranger, &["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-q", "-m", "theirs"]);
        let e = crate::tr::classify_in(false, &add_folder_location(&st, &stranger, "usb", &drive).await.unwrap_err());
        assert!(e.message.contains("a different library"), "{}", e.message);
        assert!(sh(&stranger, &["remote"]).is_empty(), "nothing was added");
        // Something in the way that is not a backup is not touched.
        let busy = root.join("busy drive");
        std::fs::create_dir_all(busy.join("Notes.git")).unwrap();
        write(&busy, "Notes.git/photo.jpg", "x");
        assert!(add_folder_location(&st, &lib, "two", &busy).await.is_err());
        assert_eq!(read(&busy, "Notes.git/photo.jpg"), "x");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn backup_health_says_when_changes_have_waited_too_long() {
        let (root, lib, drive) = library_and_drive("health");
        let st = AppState::for_test();
        assert_eq!(backup_health(&st, &lib).await["state"], "none");
        add_folder_location(&st, &lib, "usb", &drive).await.unwrap();
        sync(&st, &lib).await.unwrap();
        // A change recorded after the backup: waiting, not yet a warning.
        write(&lib, "notes.md", "changed\n");
        commit_all(&st, &lib, "edit: x").await.unwrap();
        let h = backup_health(&st, &lib).await;
        assert_eq!((h["state"].clone(), h["locations"][0]["unsent"].clone()), (json!("waiting"), json!(1)));
        // The last backup was more than a week ago: overdue.
        let old = (chrono::Utc::now() - chrono::Duration::days(STALE_DAYS + 1)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        change_backup_state(&lib, |e| { e["locations"]["usb"]["ok"] = json!(old); e["locations"]["usb"]["tried"] = json!(old); });
        assert_eq!(backup_health(&st, &lib).await["state"], "stale");
        // Nothing to send, however old the backup: fine.
        sync(&st, &lib).await.unwrap();
        change_backup_state(&lib, |e| { e["locations"]["usb"]["ok"] = json!(old); e["locations"]["usb"]["tried"] = json!(old); });
        assert_eq!(backup_health(&st, &lib).await["state"], "ok");
        // A failure that was put right (a sign-in updated, a server trusted) no longer counts.
        note_attempts(&lib, &[("usb".to_string(), Some("sync.why.sign_in"))]);
        assert_eq!(backup_health(&st, &lib).await["state"], "failed");
        clear_failure(&lib, "usb");
        assert_eq!(backup_health(&st, &lib).await["state"], "ok");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn meeting_recordings_never_enter_a_history_but_their_transcripts_do() {
        let st = AppState::for_test();
        // A history the app keeps, and a library with a repository of its own (and a .gitignore of the user's).
        let kept = tmp("rec-kept");
        let own = tmp("rec-own");
        sh(&own, &["init", "-q"]);
        write(&own, ".gitignore", "drafts/\n");
        for d in [&kept, &own] {
            write(d, "clients/acme/meetings/2026-11-12-call.m4a", "sound");
            write(d, "clients/acme/meetings/2026-11-12-call.md", "# Call\n");
            write(d, "meetings/2026-11-13-all-hands.wav", "sound");
            write(d, "clients/acme/outputs/jingle.mp3", "a file the user put there");
            start_history(&st, d).await.unwrap();
            commit_all(&st, d, "edit: test").await.unwrap();
            let files = git(&st, d, &["ls-files"]).await.unwrap();
            assert!(files.contains("clients/acme/meetings/2026-11-12-call.md") && files.contains("clients/acme/outputs/jingle.mp3"), "{files}");
            assert!(!files.contains(".m4a") && !files.contains(".wav"), "{files}");
            // A recording made later is left out too; the rule is written once.
            write(d, "clients/acme/meetings/2026-11-14-review.m4a", "sound");
            commit_all(&st, d, "edit: later").await.unwrap();
            assert!(!git(&st, d, &["ls-files"]).await.unwrap().contains("review.m4a"));
            assert_eq!(git(&st, d, &["status", "--porcelain"]).await.unwrap(), "", "nothing is left waiting to be recorded");
        }
        let Repo::Kept(store) = repo_of(&kept) else { panic!("the history should be kept by the app") };
        assert_eq!(std::fs::read_to_string(store.join("info/exclude")).unwrap().matches(RECORDINGS_MARK).count(), 1);
        let rule = std::fs::read_to_string(own.join(".git/info/exclude")).unwrap();
        assert_eq!(rule.matches(RECORDINGS_MARK).count(), 1, "{rule}");
        // The user's own list of files to leave out is not touched, nor is one made.
        assert_eq!(read(&own, ".gitignore"), "drafts/\n");
        assert!(!kept.join(".gitignore").exists() && !kept.join(".git").exists());
        for d in [kept, own, store] { std::fs::remove_dir_all(d).unwrap(); }
    }

    #[test]
    fn a_sign_in_in_the_password_file_is_read_and_forgotten_by_its_site() {
        let d = tmp("credfile");
        let f = d.join("git-credentials");
        save_credential_file(&f, "git.example.com:3000", "sara@firm.com", "p@ss/1").unwrap();
        save_credential_file(&f, "github.com", "me", "two").unwrap();
        assert_eq!(file_sign_in(&f, "git.example.com:3000").as_deref(), Some("sara@firm.com"));
        assert_eq!(file_sign_in(&f, "git.example.com"), None);
        forget_credential_file(&f, "git.example.com:3000").unwrap();
        assert_eq!(file_sign_in(&f, "git.example.com:3000"), None);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "https://me:two@github.com\n");
        forget_credential_file(&f, "github.com").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn locations_are_told_apart_by_their_address() {
        for (url, kind) in [("https://github.com/a/b.git", "online"), ("http://github.com/a/b.git", "online"), ("ssh://git@host/x.git", "ssh"),
                            ("git@github.com:a/b.git", "ssh"), ("/Volumes/Backup/Notes.git", "folder"), ("D:\\Backups\\Notes.git", "folder"),
                            ("D:/Backups/Notes.git", "folder"), ("\\\\server\\share\\Notes.git", "folder")] {
            assert_eq!(location_kind(url), kind, "{url}");
        }
        assert_eq!(backup_folder_name(Path::new("/Users/sara/My: Notes?")), "My Notes.git");
        assert_eq!(backup_folder_name(Path::new("/Users/sara/ملاحظاتي")), "ملاحظاتي.git");
        assert_eq!(backup_folder_name(Path::new("/")), "Thoughts.git");
    }
}
