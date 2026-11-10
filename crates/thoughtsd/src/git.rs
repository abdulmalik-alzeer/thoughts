//! Git via the system `git` binary. Credentials are served through GIT_ASKPASS
//! (this same executable in askpass mode, reading the OS keychain) or a
//! `git-credentials` file outside the library.

use crate::state::AppState;
use serde_json::{json, Value};
use std::path::Path;

pub async fn git(state: &AppState, dir: &Path, args: &[&str]) -> anyhow::Result<String> {
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
               "credential.helper="] {
        c.args(["-c", kv]);
    }
    // On Linux a machine without a keyring keeps passwords in the private file instead (see `save_credential`).
    let file_fallback = cfg!(target_os = "linux") && crate::paths::git_credentials_file().exists();
    if store == "file" || file_fallback {
        let f = crate::paths::git_credentials_file();
        c.args(["-c", &format!("credential.helper=store --file={}", f.display())]);
    }
    if store != "file" {
        if let Ok(exe) = std::env::current_exe() { c.env("GIT_ASKPASS", exe).env("THOUGHTS_ASKPASS", "1"); }
    }
    c.args(args).stdin(std::process::Stdio::null());
    let out = c.output().await?;
    if !out.status.success() {
        anyhow::bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
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
        _ => anyhow::bail!("the library is not on a branch"),
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
        anyhow::bail!("the library is on \"{branch}\", which the app keeps for its own use and never syncs");
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
    let branch = syncable_branch(state, dir).await?;
    let committed = commit_all(state, dir, &format!("sync: {}", chrono::Local::now().format("%Y-%m-%d %H:%M"))).await?;
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

/// Finish a sync whose pull conflicted. The local state is always kept first as a branch
/// `backup/<YYYYMMDD-HHMMSS>`; then the remote is merged (a merge, not a rebase, so "mine" really
/// is the local side) keeping `strategy`'s side wherever the two disagree, and the result is pushed.
pub async fn resolve(state: &AppState, dir: &Path, strategy: &str) -> anyhow::Result<Value> {
    let side = match strategy {
        "mine" => "ours",
        "theirs" => "theirs",
        _ => anyhow::bail!("choose which version to keep: mine or theirs"),
    };
    let _g = state.git_lock.lock().await;
    let branch = syncable_branch(state, dir).await?;
    let committed = commit_all(state, dir, &format!("sync: {}", chrono::Local::now().format("%Y-%m-%d %H:%M"))).await?;
    let rs = remotes(state, dir).await;
    let primary = rs.first().and_then(|r| r["name"].as_str()).ok_or_else(|| anyhow::anyhow!("no backup location is set up for this library"))?.to_string();
    if head(state, dir).await.is_none() { anyhow::bail!("there is nothing in this library's history yet"); }

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut backup = format!("backup/{stamp}");
    let mut n = 1;
    while git(state, dir, &["rev-parse", "--verify", "-q", &format!("refs/heads/{backup}")]).await.is_ok() {
        n += 1;
        backup = format!("backup/{stamp}-{n}");
    }
    git(state, dir, &["branch", "--", &backup]).await?;
    crate::log::info(format!("sync: resolving conflict keeping {strategy}; local state kept as {backup}"));

    let mut log = vec![format!("backup {backup}")];
    match git(state, dir, &["fetch", "--", &primary, &branch]).await {
        Ok(_) => {
            if let Err(e) = git_ident(state, dir, &["merge", "--no-edit", "-X", side, "FETCH_HEAD"]).await {
                // What -X cannot settle (one side deleted or renamed the file): take the chosen side whole.
                let files = unmerged(state, dir).await;
                if files.is_empty() {
                    let _ = git(state, dir, &["merge", "--abort"]).await;
                    anyhow::bail!("could not combine the two versions: {e}");
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
        Err(e) if e.to_string().contains("couldn't find remote ref") => log.push(format!("{primary}: new branch")),
        Err(e) => anyhow::bail!("pull failed: {e}"),
    }
    push_all(state, dir, &rs, &branch, &mut log).await;
    crate::log::info(format!("sync: {}", log.join(", ")));
    Ok(json!({"committed": committed, "backup": backup, "strategy": strategy, "log": log}))
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
        let r = keyring::Entry::new("thoughts-git", host).and_then(|e| e.set_password(&v));
        #[cfg(target_os = "linux")]
        if let Err(e) = &r {
            // No working keyring on this machine: keep it in the private file git reads directly.
            crate::log::warn(format!("system keyring unavailable ({e}); saving the sync password in a private file instead"));
            return save_credential_file(&crate::paths::git_credentials_file(), host, username, secret);
        }
        r?;
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
