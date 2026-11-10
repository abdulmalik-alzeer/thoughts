//! Git via the system `git` binary. Credentials are served through GIT_ASKPASS
//! (this same executable in askpass mode, reading the OS keychain) or a
//! `git-credentials` file outside the library.

use crate::state::AppState;
use serde_json::{json, Value};
use std::path::Path;
use tokio::process::Command;

pub async fn git(state: &AppState, dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let store = state.settings.read().await.sync.credential_store.clone();
    let mut c = Command::new(crate::claude::find_binary(&None, "git").unwrap_or_else(|| "git".into()));
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
    if store == "file" {
        let f = crate::paths::git_credentials_file();
        c.args(["-c", &format!("credential.helper=store --file={}", f.display())]);
    } else if let Ok(exe) = std::env::current_exe() {
        c.env("GIT_ASKPASS", exe).env("THOUGHTS_ASKPASS", "1");
    }
    c.args(args).stdin(std::process::Stdio::null());
    let out = c.output().await?;
    if !out.status.success() {
        anyhow::bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
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
    if let Err(e) = git(state, dir, &["commit", "-q", "-m", msg]).await {
        if !e.to_string().contains("tell me who you are") { return Err(e); }
        // No git identity configured on this machine: commit as the app.
        git(state, dir, &["-c", "user.name=Thoughts", "-c", "user.email=thoughts@localhost", "commit", "-q", "-m", msg]).await?;
    }
    Ok(true)
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

/// Commit local changes, pull --rebase from the first remote, push to all.
pub async fn sync(state: &AppState, dir: &Path) -> anyhow::Result<Value> {
    let _g = state.git_lock.lock().await;
    let committed = commit_all(state, dir, &format!("sync: {}", chrono::Local::now().format("%Y-%m-%d %H:%M"))).await?;
    let rs = remotes(state, dir).await;
    let branch = git(state, dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await?.trim().to_string();
    let mut log = vec![];
    if let Some(primary) = rs.first().and_then(|r| r["name"].as_str()) {
        match git(state, dir, &["pull", "--rebase", "--autostash", primary, &branch]).await {
            Ok(_) => log.push(format!("pulled {primary}")),
            Err(e) if e.to_string().contains("couldn't find remote ref") => log.push(format!("{primary}: new branch")),
            Err(e) => { let _ = git(state, dir, &["rebase", "--abort"]).await; anyhow::bail!("pull failed: {e}") }
        }
    }
    for r in &rs {
        let name = r["name"].as_str().unwrap_or_default();
        match git(state, dir, &["push", "-u", name, &branch]).await {
            Ok(_) => log.push(format!("pushed {name}")),
            Err(e) => log.push(format!("push {name} failed: {e}")),
        }
    }
    Ok(json!({"committed": committed, "log": log}))
}

pub async fn log(state: &AppState, dir: &Path, n: usize) -> Vec<Value> {
    let out = git(state, dir, &["log", &format!("-{n}"), "--format=%H%x1f%h%x1f%s%x1f%cI"]).await.unwrap_or_default();
    out.lines().filter_map(|l| {
        let p: Vec<_> = l.split('\x1f').collect();
        (p.len() == 4).then(|| json!({"hash": p[0], "short": p[1], "subject": p[2], "date": p[3]}))
    }).collect()
}

/// Store a credential for `host` (e.g. github.com).
pub async fn save_credential(state: &AppState, host: &str, username: &str, secret: &str) -> anyhow::Result<()> {
    let store = state.settings.read().await.sync.credential_store.clone();
    if store == "file" {
        let f = crate::paths::git_credentials_file();
        let existing = std::fs::read_to_string(&f).unwrap_or_default();
        let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
        let mut lines: Vec<String> = existing.lines().filter(|l| !l.ends_with(&format!("@{host}"))).map(String::from).collect();
        lines.push(format!("https://{}:{}@{host}", enc(username), enc(secret)));
        std::fs::write(&f, lines.join("\n") + "\n")?;
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600))?; }
    } else {
        let v = json!({"username": username, "secret": secret}).to_string();
        keyring::Entry::new("thoughts-git", host)?.set_password(&v)?;
    }
    Ok(())
}

pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let rest = rest.rsplit('@').next()?;
    Some(rest.split(['/', ':']).next()?.to_string())
}
