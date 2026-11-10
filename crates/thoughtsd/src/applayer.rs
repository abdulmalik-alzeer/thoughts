//! The customisation layer: a git repo (`~/.thoughts/app-layer`) holding everything that makes this
//! install *yours* — CSS, themes, Home layout, plugins, views, workflows. The AI never writes here
//! directly: it works in a staging copy (see `staging.rs`) and the user applies the result. Every
//! applied change is one history entry that can be undone on its own, and Rollback restores any
//! earlier state as a whole.

use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The contract that teaches the AI how to customise the app. Rewritten into the layer as
/// `README.md` on every start, so a new app version can teach new abilities.
pub const CONTRACT: &str = include_str!("../assets/LAYER.md");

/// Default layer files shipped with the app (besides README.md, which is always the contract).
/// App updates merge these three-way against the user's copies (see `updates.rs`).
pub const SHIPPED: &[(&str, &str)] = &[
    ("CLAUDE.md", "Read README.md first: it is the contract for customising the Thoughts app. Keep changes small and self-contained, and finish by writing SUMMARY.md as the contract describes.\n"),
    ("custom.css", "/* Your overrides. */\n"),
    ("themes.json", "[]\n"),
    ("home.json", "{\n  \"chips\": [],\n  \"hidden\": [],\n  \"order\": []\n}\n"),
    ("manifest.json", "{\n  \"apiVersion\": 1,\n  \"plugins\": {}\n}\n"),
];

/// Never part of the layer, wherever a working copy lives.
const EXCLUDE: &str = "SUMMARY.md\n.claude/\n.mcp.json\n";

/// The folders the layer machinery works in. Tests point these at a temp dir.
#[derive(Clone, Debug)]
pub struct Dirs {
    /// `~/.thoughts`
    pub home: PathBuf,
    /// The live layer (branch `main`).
    pub layer: PathBuf,
    /// The staging worktree (branch `staging`).
    pub staging: PathBuf,
}

impl Dirs {
    pub fn real() -> Self { Self::at(&crate::paths::home()) }
    pub fn at(home: &Path) -> Self {
        Self { home: home.to_path_buf(), layer: home.join("app-layer"), staging: home.join("app-layer-staging") }
    }
}

pub fn valid_hash(hash: &str) -> bool {
    (4..=64).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Runs git for the layer: through the daemon's hardened `git::git` in the app, and through a
/// plain equivalent in unit tests (which have no daemon state).
#[derive(Clone, Copy)]
pub struct Git<'a>(pub Option<&'a AppState>);

impl<'a> Git<'a> {
    pub async fn run(&self, dir: &Path, args: &[&str]) -> anyhow::Result<String> {
        match self.0 {
            Some(state) => crate::git::git(state, dir, args).await,
            None => plain_git(dir, args).await,
        }
    }

    /// Commit what is already staged in the index (falling back to an app identity when git has none).
    pub async fn commit_staged(&self, dir: &Path, msg: &str, only: &[&str]) -> anyhow::Result<()> {
        let mut args = vec!["commit", "-q", "-m", msg];
        if !only.is_empty() { args.push("--"); args.extend(only); }
        if let Err(e) = self.run(dir, &args).await {
            if !e.to_string().contains("tell me who you are") { return Err(e); }
            let mut with_id = vec!["-c", "user.name=Thoughts", "-c", "user.email=thoughts@localhost"];
            with_id.extend(&args);
            self.run(dir, &with_id).await?;
        }
        Ok(())
    }

    /// Commit everything in `dir`. Returns whether there was anything to commit.
    pub async fn commit_all(&self, dir: &Path, msg: &str) -> anyhow::Result<bool> {
        self.run(dir, &["add", "-A"]).await?;
        if self.run(dir, &["diff", "--cached", "--quiet"]).await.is_ok() { return Ok(false); }
        self.commit_staged(dir, msg, &[]).await?;
        Ok(true)
    }

    pub async fn chat_running(&self, chat_id: &str) -> bool {
        match self.0 { Some(state) => state.chats.get(chat_id).await.map_or(false, |c| c.running), None => false }
    }
}

#[cfg(not(test))]
async fn plain_git(_dir: &Path, _args: &[&str]) -> anyhow::Result<String> { anyhow::bail!("git is not available here") }

#[cfg(test)]
async fn plain_git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = tokio::process::Command::new(crate::claude::find_binary(&None, "git").unwrap_or_else(|| "git".into()))
        .current_dir(dir).env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_TERMINAL_PROMPT", "0")
        .args(["-c", "core.hooksPath=/dev/null", "-c", "user.name=Test", "-c", "user.email=test@localhost", "-c", "commit.gpgSign=false"])
        .args(args).stdin(std::process::Stdio::null()).output().await?;
    if !out.status.success() { anyhow::bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()); }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn write_exclude(layer: &Path) {
    let info = layer.join(".git/info");
    let _ = std::fs::create_dir_all(&info);
    let _ = std::fs::write(info.join("exclude"), EXCLUDE);
}

/// Create the layer on first run, and on every start refresh the embedded contract (README.md).
pub async fn ensure(state: &AppState) -> anyhow::Result<()> { ensure_in(Git(Some(state)), &Dirs::real()).await }

pub async fn ensure_in(g: Git<'_>, d: &Dirs) -> anyhow::Result<()> {
    let dir = &d.layer;
    if !dir.join(".git").exists() {
        for sub in ["plugins", "views", "workflows"] {
            std::fs::create_dir_all(dir.join(sub))?;
            std::fs::write(dir.join(sub).join(".gitkeep"), "")?;
        }
        std::fs::write(dir.join("README.md"), CONTRACT)?;
        for (p, text) in SHIPPED { std::fs::write(dir.join(p), text)?; }
        g.run(dir, &["init", "-q", "-b", "main"]).await?;
        write_exclude(dir);
        g.commit_all(dir, "Starting point").await?;
        return Ok(());
    }
    write_exclude(dir);
    for sub in ["plugins", "views", "workflows"] { let _ = std::fs::create_dir_all(dir.join(sub)); }
    if std::fs::read_to_string(dir.join("README.md")).ok().as_deref() != Some(CONTRACT) {
        std::fs::write(dir.join("README.md"), CONTRACT)?;
        g.run(dir, &["add", "--", "README.md"]).await?;
        if g.run(dir, &["diff", "--cached", "--quiet", "--", "README.md"]).await.is_err() {
            // Only the guide goes into this commit, whatever else is lying around.
            g.commit_staged(dir, "Update customisation guide", &["README.md"]).await?;
        }
    }
    Ok(())
}

/// What the UI loads: the live layer.
pub async fn manifest() -> Value { manifest_of(&Dirs::real().layer) }

/// The layer found in `dir` (the live layer, or the staging copy for a preview).
pub fn manifest_of(dir: &Path) -> Value {
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap_or_default();
    let parsed = |p: &str| std::fs::read_to_string(dir.join(p)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    let list = |sub: &str, ext: &str| -> Vec<Value> {
        let mut v: Vec<Value> = std::fs::read_dir(dir.join(sub)).into_iter().flatten().flatten()
            .filter(|e| e.path().extension().map_or(false, |x| x == ext) && !e.file_name().to_string_lossy().starts_with('.'))
            .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
            .map(|e| json!({"name": e.file_name().to_string_lossy(), "code": std::fs::read_to_string(e.path()).unwrap_or_default()}))
            .collect();
        v.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        v
    };
    json!({
        "css": read("custom.css"),
        "themes": serde_json::from_str::<Value>(&read("themes.json")).ok().filter(Value::is_array).unwrap_or(json!([])),
        "plugins": list("plugins", "js"),
        "views": list("views", "html"),
        "manifest": parsed("manifest.json"),
        "home": parsed("home.json"),
        "workflows": crate::workflows::load(dir),
    })
}

/// `[{hash, short, date, summary, files:[path]}]`, newest first (`subject` = `summary`, kept for older UIs).
pub async fn history(state: &AppState) -> Vec<Value> { history_in(Git(Some(state)), &Dirs::real().layer).await }

pub async fn history_in(g: Git<'_>, dir: &Path) -> Vec<Value> {
    let out = g.run(dir, &["log", "-100", "--name-only", "--no-renames", "--format=%x1e%H%x1f%h%x1f%s%x1f%cI"]).await.unwrap_or_default();
    out.split('\x1e').filter_map(|rec| {
        let mut lines = rec.lines();
        let p: Vec<_> = lines.next()?.split('\x1f').collect();
        if p.len() != 4 { return None; }
        let files: Vec<&str> = lines.map(str::trim).filter(|l| !l.is_empty()).collect();
        Some(json!({"hash": p[0], "short": p[1], "summary": p[2], "subject": p[2], "date": p[3], "files": files}))
    }).collect()
}

/// A stray edit made straight in the live layer is kept as its own history entry before anything else moves.
async fn snapshot(g: Git<'_>, dir: &Path) -> anyhow::Result<()> {
    g.commit_all(dir, "Snapshot of manual changes").await?;
    Ok(())
}

/// Restore the whole layer to `hash` as a *new* history entry, so rollback itself is undoable.
pub async fn rollback(state: &AppState, hash: &str) -> anyhow::Result<()> {
    rollback_in(Git(Some(state)), &Dirs::real().layer, hash).await?;
    state.emit(json!({"type": "layer.changed"}));
    Ok(())
}

pub async fn rollback_in(g: Git<'_>, dir: &Path, hash: &str) -> anyhow::Result<()> {
    if !valid_hash(hash) { anyhow::bail!("bad history entry"); }
    snapshot(g, dir).await?;
    let commit = format!("{hash}^{{commit}}");
    let short = g.run(dir, &["rev-parse", "--verify", "--short", "--end-of-options", &commit]).await?.trim().to_string();
    g.run(dir, &["read-tree", "-u", "--reset", "--end-of-options", hash]).await?;
    g.commit_all(dir, &format!("Went back to an earlier version ({short})")).await?;
    Ok(())
}

/// Undo one change, leaving everything made before and after it in place.
pub async fn revert(state: &AppState, hash: &str) -> anyhow::Result<()> {
    revert_in(Git(Some(state)), &Dirs::real().layer, hash).await?;
    state.emit(json!({"type": "layer.changed"}));
    Ok(())
}

pub async fn revert_in(g: Git<'_>, dir: &Path, hash: &str) -> anyhow::Result<()> {
    if !valid_hash(hash) { anyhow::bail!("bad history entry"); }
    snapshot(g, dir).await?;
    let commit = format!("{hash}^{{commit}}");
    let subject = g.run(dir, &["log", "-1", "--format=%s", "--end-of-options", &commit]).await?.trim().to_string();
    if let Err(e) = g.run(dir, &["revert", "--no-commit", "--end-of-options", hash]).await {
        let _ = g.run(dir, &["revert", "--abort"]).await;
        let _ = g.run(dir, &["reset", "-q", "--hard", "HEAD"]).await;
        crate::log::warn(format!("layer: revert of {hash} failed: {e}"));
        anyhow::bail!("This change cannot be undone on its own because later changes build on it. Use Rollback to go back to an earlier version instead.");
    }
    if g.run(dir, &["diff", "--cached", "--quiet"]).await.is_ok() {
        let _ = g.run(dir, &["revert", "--quit"]).await;
        anyhow::bail!("This change is already undone.");
    }
    g.commit_staged(dir, &format!("Undo: {subject}"), &[]).await?;
    let _ = g.run(dir, &["revert", "--quit"]).await;
    Ok(())
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// A fresh temp "~/.thoughts" with an initialised layer.
    pub async fn layer(name: &str) -> (Git<'static>, Dirs) {
        let home = std::env::temp_dir().join(format!("thoughts-layer-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        let st = Git(None);
        let d = Dirs::at(&home);
        ensure_in(st, &d).await.unwrap();
        (st, d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn creates_layer_and_refreshes_contract() {
        let (st, d) = testutil::layer("ensure").await;
        assert_eq!(std::fs::read_to_string(d.layer.join("README.md")).unwrap(), CONTRACT);
        let m = manifest_of(&d.layer);
        assert_eq!(m["manifest"]["apiVersion"], 1);
        assert!(m["home"]["chips"].is_array() && m["workflows"].is_array());
        // A stale guide is replaced in a commit of its own; an unchanged one adds nothing.
        std::fs::write(d.layer.join("README.md"), "old").unwrap();
        st.commit_all(&d.layer, "old guide").await.unwrap();
        ensure_in(st, &d).await.unwrap();
        let h = history_in(st, &d.layer).await;
        assert_eq!(h[0]["summary"], "Update customisation guide");
        assert_eq!(h[0]["files"], json!(["README.md"]));
        ensure_in(st, &d).await.unwrap();
        assert_eq!(history_in(st, &d.layer).await.len(), h.len());
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn reverts_one_change_and_keeps_later_ones() {
        let (st, d) = testutil::layer("revert").await;
        std::fs::write(d.layer.join("custom.css"), "body { color: red }\n").unwrap();
        st.commit_all(&d.layer, "Make text red").await.unwrap();
        std::fs::write(d.layer.join("plugins/a.js"), "export default () => {}\n").unwrap();
        st.commit_all(&d.layer, "Add a plugin").await.unwrap();
        let h = history_in(st, &d.layer).await;
        assert_eq!(h[0]["summary"], "Add a plugin");
        assert_eq!(h[1]["files"], json!(["custom.css"]));
        revert_in(st, &d.layer, h[1]["hash"].as_str().unwrap()).await.unwrap();
        assert_eq!(std::fs::read_to_string(d.layer.join("custom.css")).unwrap(), "/* Your overrides. */\n");
        assert!(d.layer.join("plugins/a.js").exists(), "the later change survives");
        assert_eq!(history_in(st, &d.layer).await[0]["summary"], "Undo: Make text red");
        assert!(revert_in(st, &d.layer, "--upload-pack=x").await.is_err());
        // Whole-layer rollback still works, and is itself a new entry.
        let first = history_in(st, &d.layer).await.last().unwrap()["hash"].as_str().unwrap().to_string();
        rollback_in(st, &d.layer, &first).await.unwrap();
        assert!(!d.layer.join("plugins/a.js").exists());
        assert!(rollback_in(st, &d.layer, "HEAD~1").await.is_err());
        std::fs::remove_dir_all(&d.home).unwrap();
    }
}
