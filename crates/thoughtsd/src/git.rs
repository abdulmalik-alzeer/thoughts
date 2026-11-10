//! Git via the system `git` binary. Credentials are served through GIT_ASKPASS
//! (this same executable in askpass mode, reading the OS keychain) or a
//! `git-credentials` file outside the library.

use crate::state::AppState;
use serde_json::{json, Value};
use std::path::Path;

/// Longest a git command that only touches this machine may run.
pub const LOCAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
/// Longest a git command that talks to a remote may run.
pub const NETWORK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// The git subcommand in `args` (skipping leading `-c key=value` pairs).
fn subcommand<'a>(args: &[&'a str]) -> &'a str {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == "-c" { it.next(); continue; }
        return a;
    }
    ""
}

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

/// Run git in `dir` and return its raw output. Hardened against a library's own configuration, never
/// waits for a terminal, and never runs longer than `timeout_for(args)`: a stalled command is killed
/// (with any helper it started) and reported as a plain error, so nothing stays locked behind it.
pub async fn git_raw(state: &AppState, dir: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    if !dir.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", &dir.display().to_string())])); }
    let store = state.settings.read().await.sync.credential_store.clone();
    let mut c = crate::proc::command(crate::claude::find_binary(&None, "git").unwrap_or_else(|| "git".into()));
    c.current_dir(dir).env("PATH", crate::claude::shell_path()).env("GIT_TERMINAL_PROMPT", "0");
    // Never run code a library's own git config could smuggle in (hooks, fsmonitor, ssh command).
    #[cfg(windows)] let no_hooks = "NUL";
    #[cfg(not(windows))] let no_hooks = "/dev/null";
    c.env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_ATTR_NOSYSTEM", "1");
    for kv in ["core.fsmonitor=false", &format!("core.hooksPath={no_hooks}"), "core.sshCommand=ssh",
               "core.askPass=", "core.pager=cat", "core.editor=true", "diff.external=",
               "commit.gpgSign=false", "tag.gpgSign=false", "gpg.program=gpg", "protocol.ext.allow=never", "core.gitProxy=",
               "credential.helper=",
               // A transfer that crawls below 1 kB/s for 30 seconds is given up on rather than left hanging.
               "http.lowSpeedLimit=1000", "http.lowSpeedTime=30"] {
        c.args(["-c", kv]);
    }
    // Git for Windows turns line-ending conversion on in its system settings, which this command
    // ignores (see above). Without the line below a library that was cloned with that conversion
    // would look entirely changed here. "input" compares and saves with plain line endings and
    // never rewrites the files on disk.
    #[cfg(windows)]
    c.args(["-c", "core.autocrlf=input"]);
    // On Linux a machine without a keyring keeps passwords in the private file instead (see `save_credential`).
    let file_fallback = cfg!(target_os = "linux") && crate::paths::git_credentials_file().exists();
    if store == "file" || file_fallback {
        c.args(["-c", &format!("credential.helper={}", credential_helper(&crate::paths::git_credentials_file()))]);
    }
    if store != "file" {
        if let Ok(exe) = crate::paths::app_exe() { c.env("GIT_ASKPASS", exe).env("THOUGHTS_ASKPASS", "1"); }
    }
    c.args(args).stdin(std::process::Stdio::null());
    let limit = timeout_for(args);
    let out = match crate::proc::output_timeout(&mut c, limit).await {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            crate::log::error(format!("git {} did not finish within {}s and was stopped", subcommand(args), limit.as_secs()));
            return Err(crate::tr::err("git.timeout", &[]));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(crate::tr::err("git.missing", &[])),
        Err(e) => return Err(e.into()),
    };
    if !out.status.success() {
        anyhow::bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

pub async fn git(state: &AppState, dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    Ok(String::from_utf8_lossy(&git_raw(state, dir, args).await?).to_string())
}

/// Like `git`, for commands that create commits (commit, merge, rebase): when this machine has no
/// git identity configured, retry as the app.
pub async fn git_ident(state: &AppState, dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    match git(state, dir, args).await {
        Err(e) if e.to_string().contains("tell me who you are") => {
            let mut a = vec!["-c", "user.name=Thoughts", "-c", "user.email=thoughts@localhost"];
            a.extend_from_slice(args);
            git(state, dir, &a).await
        }
        r => r,
    }
}

pub async fn is_repo(state: &AppState, dir: &Path) -> bool {
    git(state, dir, &["rev-parse", "--git-dir"]).await.is_ok()
}

pub async fn init(state: &AppState, dir: &Path) -> anyhow::Result<()> {
    if !is_repo(state, dir).await {
        git(state, dir, &["init", "-b", "main"]).await?;
    }
    Ok(())
}

pub async fn commit_all(state: &AppState, dir: &Path, msg: &str) -> anyhow::Result<bool> {
    git(state, dir, &["add", "-A"]).await?;
    if git(state, dir, &["diff", "--cached", "--quiet"]).await.is_ok() {
        return Ok(false);
    }
    git_ident(state, dir, &["commit", "-q", "-m", msg]).await?;
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
    name == "staging" || name.starts_with("reorganize/") || name.starts_with("backup/")
}

/// Files with unresolved conflicts in a stopped merge or rebase.
pub async fn unmerged(state: &AppState, dir: &Path) -> Vec<String> {
    git(state, dir, &["diff", "--name-only", "--diff-filter=U", "-z"]).await.unwrap_or_default()
        .split('\0').filter(|f| !f.is_empty()).map(String::from).collect()
}

pub async fn remotes(state: &AppState, dir: &Path) -> Vec<Value> {
    let out = git(state, dir, &["remote", "-v"]).await.unwrap_or_default();
    let mut seen = std::collections::BTreeMap::new();
    for l in out.lines() {
        let p: Vec<_> = l.split_whitespace().collect();
        if p.len() >= 2 { seen.entry(p[0].to_string()).or_insert(p[1].to_string()); }
    }
    seen.into_iter().map(|(n, u)| json!({"name": n, "url": u})).collect()
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
           "last": last, "remotes": remotes(state, dir).await})
}

/// The branch a sync works on. The app's own bookkeeping branches are never pushed.
async fn syncable_branch(state: &AppState, dir: &Path) -> anyhow::Result<String> {
    let branch = current_branch(state, dir).await?;
    if internal_branch(&branch) {
        return Err(crate::tr::err("git.internal_branch", &[("branch", &branch)]));
    }
    Ok(branch)
}

async fn push_all(state: &AppState, dir: &Path, rs: &[Value], branch: &str, log: &mut Vec<String>) {
    for r in rs {
        let name = r["name"].as_str().unwrap_or_default();
        match git(state, dir, &["push", "-u", "--", name, branch]).await {
            Ok(_) => log.push(format!("pushed {name}")),
            Err(e) => {
                crate::log::warn(format!("sync: push to {name} failed: {e}"));
                log.push(format!("push {name} failed: {e}"));
            }
        }
    }
}

/// Commit local changes, pull --rebase from the first remote, push to all.
/// A pull that conflicts is undone and reported as `CONFLICT::<file;file;…>` (see `resolve`).
pub async fn sync(state: &AppState, dir: &Path) -> anyhow::Result<Value> {
    let _g = state.git_lock.lock().await;
    crate::trust::require_git(dir)?;
    let branch = syncable_branch(state, dir).await?;
    let committed = commit_all(state, dir, &subject::sync()).await?;
    let rs = remotes(state, dir).await;
    let mut log = vec![];
    if let Some(primary) = rs.first().and_then(|r| r["name"].as_str()) {
        match git_ident(state, dir, &["pull", "--rebase", "--autostash", "--", primary, &branch]).await {
            Ok(_) => log.push(format!("pulled {primary}")),
            Err(e) if e.to_string().contains("couldn't find remote ref") => log.push(format!("{primary}: new branch")),
            Err(e) => {
                // Read the conflict list before undoing the half-finished rebase.
                let files = unmerged(state, dir).await;
                let conflict = !files.is_empty() || e.to_string().contains("CONFLICT");
                if let Err(e2) = git(state, dir, &["rebase", "--abort"]).await {
                    if conflict { crate::log::error(format!("sync: could not undo the conflicting pull: {e2}")); }
                }
                if conflict {
                    crate::log::warn(format!("sync: pull from {primary} conflicts in {} file(s)", files.len()));
                    anyhow::bail!("CONFLICT::{}", files.join(";"));
                }
                crate::log::warn(format!("sync: pull from {primary} failed: {e}"));
                anyhow::bail!("pull failed: {e}")
            }
        }
    }
    push_all(state, dir, &rs, &branch, &mut log).await;
    crate::log::info(format!("sync: {}", log.join(", ")));
    Ok(json!({"committed": committed, "log": log}))
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
async fn make_backup(state: &AppState, dir: &Path) -> anyhow::Result<String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut backup = format!("backup/{stamp}");
    let mut n = 1;
    while git(state, dir, &["rev-parse", "--verify", "-q", &format!("refs/heads/{backup}")]).await.is_ok() {
        n += 1;
        backup = format!("backup/{stamp}-{n}");
    }
    git(state, dir, &["branch", "--", &backup]).await?;
    prune_backups(state, dir).await;
    Ok(backup)
}

/// Finish a sync whose pull conflicted: the remote is merged (a merge, not a rebase, so "mine"
/// really is the local side) keeping `strategy`'s side wherever the two disagree, and the result is
/// pushed. Whenever the two sides really clash, the local state is first kept as a branch
/// `backup/<YYYYMMDD-HHMMSS>`; when there is nothing to settle (the call was not needed) no backup
/// is made and this is an ordinary sync.
pub async fn resolve(state: &AppState, dir: &Path, strategy: &str) -> anyhow::Result<Value> {
    let side = match strategy {
        "mine" => "ours",
        "theirs" => "theirs",
        _ => return Err(crate::tr::err("git.strategy", &[])),
    };
    let _g = state.git_lock.lock().await;
    crate::trust::require_git(dir)?;
    let branch = syncable_branch(state, dir).await?;
    let committed = commit_all(state, dir, &subject::sync()).await?;
    let rs = remotes(state, dir).await;
    let primary = rs.first().and_then(|r| r["name"].as_str()).ok_or_else(|| crate::tr::err("git.no_remote", &[]))?.to_string();
    if head(state, dir).await.is_none() { anyhow::bail!("there is nothing in this library's history yet"); }

    let mut log = vec![];
    let mut backup: Option<String> = None;
    match git(state, dir, &["fetch", "--", &primary, &branch]).await {
        Ok(_) => {
            let ancestor = |a: &'static str, b: &'static str| async move { git(state, dir, &["merge-base", "--is-ancestor", a, b]).await.is_ok() };
            if ancestor("FETCH_HEAD", "HEAD").await {
                log.push(format!("{primary}: nothing new"));
            } else {
                // Only a real clash needs a safety copy: a fast-forward or a clean merge loses nothing.
                let clean = ancestor("HEAD", "FETCH_HEAD").await
                    || git(state, dir, &["merge-tree", "--write-tree", "HEAD", "FETCH_HEAD"]).await.is_ok();
                if !clean {
                    let b = make_backup(state, dir).await?;
                    crate::log::info(format!("sync: resolving conflict keeping {strategy}; local state kept as {b}"));
                    log.push(format!("backup {b}"));
                    backup = Some(b);
                }
                if let Err(e) = git_ident(state, dir, &["merge", "--no-edit", "-X", side, "FETCH_HEAD"]).await {
                    // What -X cannot settle (one side deleted or renamed the file): take the chosen side whole.
                    let files = unmerged(state, dir).await;
                    if files.is_empty() {
                        let _ = git(state, dir, &["merge", "--abort"]).await;
                        anyhow::bail!("could not combine the two versions: {e}");
                    }
                    // A merge that looked clean but was not (an older git without merge-tree): back up now.
                    if backup.is_none() {
                        let _ = git(state, dir, &["merge", "--abort"]).await;
                        let b = make_backup(state, dir).await?;
                        log.push(format!("backup {b}"));
                        backup = Some(b);
                        let _ = git_ident(state, dir, &["merge", "--no-edit", "-X", side, "FETCH_HEAD"]).await;
                    }
                    for f in &files {
                        if git(state, dir, &["checkout", &format!("--{side}"), "--", f]).await.is_err() {
                            let _ = git(state, dir, &["rm", "-q", "-f", "--", f]).await;
                        }
                    }
                    let done = async {
                        git(state, dir, &["add", "-A"]).await?;
                        git_ident(state, dir, &["commit", "-q", "--no-edit"]).await
                    }.await;
                    if let Err(e) = done {
                        let _ = git(state, dir, &["merge", "--abort"]).await;
                        anyhow::bail!("could not combine the two versions: {e}");
                    }
                }
                log.push(format!("merged {primary} keeping {strategy}"));
            }
        }
        Err(e) if e.to_string().contains("couldn't find remote ref") => log.push(format!("{primary}: new branch")),
        Err(e) => anyhow::bail!("pull failed: {e}"),
    }
    push_all(state, dir, &rs, &branch, &mut log).await;
    crate::log::info(format!("sync: {}", log.join(", ")));
    Ok(json!({"committed": committed, "backup": backup, "strategy": strategy, "log": log}))
}

// ---------- library history ----------

/// Subjects of the commits the daemon itself writes. `commit_kind` reads them back.
pub mod subject {
    /// The user's own pending edits, recorded before something else changes the library.
    pub fn edit(why: &str) -> String { format!("edit: {why}") }
    pub fn wrap(chats: usize) -> String { format!("wrap: {} — {chats} chat(s)", chrono::Local::now().format("%Y-%m-%d")) }
    pub fn sync() -> String { format!("sync: {}", chrono::Local::now().format("%Y-%m-%d %H:%M")) }
    /// Something the assistant did (a chat, a workflow, a feature being added, a reorganize).
    pub fn assistant(what: &str) -> String {
        let what: String = what.chars().filter(|c| !c.is_control()).take(80).collect();
        format!("assistant: {}", what.trim())
    }
    pub fn restore(path: &str, short: &str) -> String { format!("restore: {path} (from {short})") }
    pub fn undo(of: &str) -> String { format!("undo: {of}") }
}

/// What kind of change a commit is, from the subject line the daemon gave it:
/// `wrap` | `sync` | `edit` | `assistant` | `other`.
pub fn commit_kind(subject: &str) -> &'static str {
    let s = subject.trim();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| s.starts_with(p));
    if starts(&["wrap: "]) { "wrap" }
    else if starts(&["sync: ", "Merge ", "sync:"]) { "sync" }
    else if starts(&["assistant: ", "module: ", "init: ", "reorganize: "]) { "assistant" }
    else if starts(&["edit: ", "restore: ", "undo: ", "snapshot before "]) { "edit" }
    else { "other" }
}

/// A full or abbreviated commit id, and nothing git could read as an option or a range.
pub fn valid_hash(hash: &str) -> bool {
    (7..=64).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `[{hash, short, date, subject, kind, files:[path]}]`, newest first; with `path`, only the
/// changes that touched that file.
pub async fn history(state: &AppState, dir: &Path, path: Option<&str>, limit: usize) -> anyhow::Result<Vec<Value>> {
    let n = format!("-{}", limit.clamp(1, 500));
    let mut args = vec!["log", n.as_str(), "--name-only", "--no-renames", "--format=%x1e%H%x1f%h%x1f%s%x1f%cI"];
    let spec = path.map(|p| format!(":(top,literal){p}"));
    if let Some(spec) = &spec { args.push("--"); args.push(spec); }
    // A repository without commits has no history yet; that is an empty list, not an error.
    if head(state, dir).await.is_none() { return Ok(vec![]); }
    let out = git(state, dir, &args).await?;
    Ok(out.split('\x1e').filter_map(|rec| {
        let mut lines = rec.lines();
        let p: Vec<_> = lines.next()?.split('\x1f').collect();
        if p.len() != 4 { return None; }
        let files: Vec<&str> = lines.map(str::trim).filter(|l| !l.is_empty()).collect();
        Some(json!({"hash": p[0], "short": p[1], "subject": p[2], "date": p[3], "kind": commit_kind(p[2]), "files": files}))
    }).collect())
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
    let _g = state.git_lock.lock().await;
    crate::trust::require_git(dir)?;
    let bytes = file_at(state, dir, rel, hash).await?;
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
    if parents.split_whitespace().count() > 2 { return Err(crate::tr::err("history.merge", &[])); }
    let subject = git(state, dir, &["log", "-1", "--format=%s", "--end-of-options", &commit]).await?.trim().to_string();
    commit_all(state, dir, &subject::edit("changes before undoing an earlier change")).await?;
    if let Err(e) = git(state, dir, &["revert", "--no-commit", "--end-of-options", hash]).await {
        let files = unmerged(state, dir).await;
        let _ = git(state, dir, &["revert", "--abort"]).await;
        // Everything was recorded just above, so this only clears the half-applied undo.
        let _ = git(state, dir, &["reset", "-q", "--hard", "HEAD"]).await;
        crate::log::warn(format!("history: undo of {hash} clashes with later changes in {} file(s): {e}", files.len()));
        if files.is_empty() { anyhow::bail!("this change could not be undone: {e}"); }
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

/// Put the working tree back exactly as it was at commit `snapshot`: tracked files are reset, and
/// files created since are removed — only those, never anything git was ignoring. Returns the
/// created paths that were removed.
pub async fn rollback_to(state: &AppState, dir: &Path, snapshot: &str) -> anyhow::Result<Vec<String>> {
    if snapshot.len() < 7 || !snapshot.bytes().all(|b| b.is_ascii_hexdigit()) { anyhow::bail!("invalid snapshot"); }
    let status = git(state, dir, &["status", "--porcelain", "-z", "--untracked-files=all"]).await?;
    let created = untracked_from_status(&status);
    git(state, dir, &["reset", "-q", "--hard", snapshot]).await?;
    // Exact paths from the repository root: no pattern matching, whatever the names contain.
    let specs: Vec<String> = created.iter().map(|p| format!(":(top,literal){p}")).collect();
    for chunk in specs.chunks(100) {
        let mut args = vec!["clean", "-q", "-f", "-d", "--"];
        args.extend(chunk.iter().map(String::as_str));
        git(state, dir, &args).await?;
    }
    // Folders that existed only for those files go too (never one that still holds anything).
    for p in &created {
        let mut cur = Path::new(p).parent();
        while let Some(rel) = cur.filter(|r| !r.as_os_str().is_empty()) {
            if std::fs::remove_dir(dir.join(rel)).is_err() { break; }
            cur = rel.parent();
        }
    }
    Ok(created)
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

pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let rest = rest.rsplit('@').next()?;
    Some(rest.split(['/', ':']).next()?.to_string())
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
        assert_eq!(read(&mine, "notes.md"), "their line\n");
        assert_eq!(read(&mine, "b.md"), "from the other device\n");
        assert_eq!(sh(&remote, &["rev-parse", "main"]), sh(&mine, &["rev-parse", "HEAD"]));
        std::fs::remove_dir_all(root).unwrap();
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
        assert!(resolve(&st, &mine, "both").await.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn internal_branches_are_never_synced() {
        for (b, internal) in [("reorganize/20260101-000000", true), ("backup/20260101-000000", true), ("staging", true), ("main", false), ("backups", false), ("my/staging", false)] {
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
}
