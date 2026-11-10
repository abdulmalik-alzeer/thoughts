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

/// The Plugin API version this app implements. Anything declaring a higher one is not loaded.
pub const API_VERSION: u64 = 1;

/// Every permission a plugin may declare.
pub const PERMISSIONS: &[&str] = &["library:read", "library:write", "todos:read", "todos:write", "ideas:read", "ideas:write", "chat:start", "storage"];
/// What a sandboxed HTML view may declare (views only ever read).
pub const VIEW_PERMISSIONS: &[&str] = &["library:read", "todos:read", "ideas:read"];

/// A library-relative folder a plugin may write into: relative, no `..`, never `.claude` or `.git`.
/// Returns the cleaned folder (no trailing slash).
pub fn clean_write_folder(raw: &str) -> anyhow::Result<String> {
    let rel = crate::paths::clean_rel(raw)?;
    if rel.split('/').any(|c| { let c = c.to_lowercase(); c == ".claude" || c == ".git" }) { anyhow::bail!("{raw} is a protected folder"); }
    Ok(rel)
}

pub fn unsupported_message(arabic: bool) -> &'static str {
    if arabic { "صُنع هذا لإصدار أحدث من التطبيق. حدّث التطبيق لاستخدامه." } else { "This was made for a newer version of the app. Update the app to use it." }
}

/// The declaration of one plugin or view: its own `<stem>.json` beside it, else its entry in
/// `manifest.json` (`plugins` / `views`), else nothing (no permissions).
/// → `{permissions, write, apiVersion, description, unsupported, message?}`.
pub fn resolve_decl(dir: &Path, sub: &str, file: &str, shared: &Value, arabic: bool) -> Value {
    let stem = Path::new(file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let own_path = dir.join(sub).join(format!("{stem}.json"));
    let own = std::fs::symlink_metadata(&own_path).ok().filter(|m| m.is_file() && m.len() <= 64 * 1024)
        .and_then(|_| std::fs::read_to_string(&own_path).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object);
    let (decl, version) = match &own {
        Some(o) => (o.clone(), o["apiVersion"].as_u64().unwrap_or(API_VERSION)),
        None => (shared[sub][file].clone(), shared["apiVersion"].as_u64().unwrap_or(API_VERSION)),
    };
    let allowed = if sub == "views" { VIEW_PERMISSIONS } else { PERMISSIONS };
    let mut permissions: Vec<&str> = decl["permissions"].as_array().into_iter().flatten().filter_map(Value::as_str)
        .filter_map(|p| allowed.iter().find(|a| **a == p).copied()).collect();
    permissions.dedup();
    let write: Vec<String> = if sub == "views" { vec![] } else {
        decl["write"].as_array().into_iter().flatten().filter_map(Value::as_str).filter_map(|w| clean_write_folder(w).ok()).take(20).collect()
    };
    let unsupported = version > API_VERSION;
    let mut v = json!({
        "permissions": permissions, "write": write, "apiVersion": version,
        "description": decl["description"].as_str().map(|d| d.chars().take(300).collect::<String>()).unwrap_or_default(),
        "unsupported": unsupported, "source": if own.is_some() { "file" } else if decl.is_object() { "manifest" } else { "none" },
    });
    if unsupported { v["message"] = json!(unsupported_message(arabic)); }
    v
}

/// What the UI loads: the live layer.
pub async fn manifest() -> Value { manifest_of(&Dirs::real().layer) }

/// The layer found in `dir` (the live layer, or the staging copy for a preview).
pub fn manifest_of(dir: &Path) -> Value { manifest_lang(dir, false) }

/// Same, with user-facing messages in the UI language.
pub fn manifest_lang(dir: &Path, arabic: bool) -> Value {
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap_or_default();
    let parsed = |p: &str| std::fs::read_to_string(dir.join(p)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    let object = |p: &str| Some(parsed(p)).filter(Value::is_object).unwrap_or(Value::Null);
    let shared = parsed("manifest.json");
    let list = |sub: &str, ext: &str| -> Vec<Value> {
        let mut v: Vec<Value> = std::fs::read_dir(dir.join(sub)).into_iter().flatten().flatten()
            .filter(|e| e.path().extension().map_or(false, |x| x == ext) && !e.file_name().to_string_lossy().starts_with('.'))
            .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
            .map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let mut item = resolve_decl(dir, sub, &name, &shared, arabic);
                // Code made for a newer API is listed but never handed to the UI to run.
                item["code"] = json!(if item["unsupported"] == true { String::new() } else { std::fs::read_to_string(e.path()).unwrap_or_default() });
                item["name"] = json!(name);
                item
            })
            .collect();
        v.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        v
    };
    json!({
        "apiVersion": API_VERSION,
        "css": read("custom.css"),
        "themes": serde_json::from_str::<Value>(&read("themes.json")).ok().filter(Value::is_array).unwrap_or(json!([])),
        "plugins": list("plugins", "js"),
        "views": list("views", "html"),
        "manifest": shared,
        "home": parsed("home.json"),
        "todos": object("todos.json"),
        "nav": object("nav.json"),
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
    let full = g.run(dir, &["rev-parse", "--verify", "--end-of-options", &commit]).await?.trim().to_string();
    g.commit_staged(dir, &format!("Undo: {subject}\n\nReverts: {full}"), &[]).await?;
    let _ = g.run(dir, &["revert", "--quit"]).await;
    Ok(())
}

/// The subject of history entry `hash` (empty when unknown).
pub async fn subject_in(g: Git<'_>, dir: &Path, hash: &str) -> String {
    if !valid_hash(hash) { return String::new(); }
    let commit = format!("{hash}^{{commit}}");
    g.run(dir, &["log", "-1", "--format=%s", "--end-of-options", &commit]).await.unwrap_or_default().trim().to_string()
}

/// The newest change the user can still undo: not the starting point, not the app's own
/// housekeeping (guide and built-in default updates), not an undo, and not already undone.
pub async fn last_undoable_in(g: Git<'_>, dir: &Path) -> Option<(String, String)> {
    let out = g.run(dir, &["log", "-200", "--format=%H%x1f%P%x1f%s%x1f%b%x1e"]).await.ok()?;
    let recs: Vec<Vec<&str>> = out.split('\x1e').map(|r| r.trim_matches(|c| c == '\n' || c == '\r').split('\x1f').collect::<Vec<_>>()).filter(|p| p.len() == 4).collect();
    let undone: Vec<&str> = recs.iter().flat_map(|p| p[3].lines()).filter_map(|l| l.trim().strip_prefix("Reverts: ")).collect();
    recs.iter().find(|p| {
        let (hash, parents, subject) = (p[0], p[1], p[2]);
        !parents.trim().is_empty() && !undone.contains(&hash) && !p[3].lines().any(|l| l.trim().starts_with("Reverts: "))
            && subject != "Update customisation guide" && !subject.starts_with("Update built-in defaults")
    }).map(|p| (p[0].to_string(), p[2].to_string()))
}

/// Undo the most recent change that is still in effect. Returns `(hash, subject)` of what was undone.
pub async fn undo_last_in(g: Git<'_>, dir: &Path, arabic: bool) -> anyhow::Result<(String, String)> {
    snapshot(g, dir).await?;
    let Some((hash, subject)) = last_undoable_in(g, dir).await else {
        anyhow::bail!("{}", if arabic { "لا يوجد تغيير للتراجع عنه." } else { "There is no change to undo." });
    };
    revert_in(g, dir, &hash).await?;
    Ok((hash, subject))
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
    #[tokio::test]
    async fn plugins_with_their_own_manifest_revert_independently() {
        let (st, d) = testutil::layer("perplugin").await;
        let add = |n: &str| {
            std::fs::write(d.layer.join(format!("plugins/{n}.js")), format!("export default () => {{ /* {n} */ }}\n")).unwrap();
            std::fs::write(d.layer.join(format!("plugins/{n}.json")), r#"{"apiVersion":1,"permissions":["todos:read"]}"#).unwrap();
        };
        add("a");
        st.commit_all(&d.layer, "Add plugin A").await.unwrap();
        add("b");
        st.commit_all(&d.layer, "Add plugin B").await.unwrap();
        let h = history_in(st, &d.layer).await;
        assert_eq!(h[1]["summary"], "Add plugin A");
        // No shared manifest is edited by either change, so A comes out cleanly while B stays.
        revert_in(st, &d.layer, h[1]["hash"].as_str().unwrap()).await.unwrap();
        assert!(!d.layer.join("plugins/a.js").exists() && !d.layer.join("plugins/a.json").exists());
        let m = manifest_of(&d.layer);
        assert_eq!(m["plugins"].as_array().unwrap().len(), 1);
        assert_eq!(m["plugins"][0]["name"], "b.js");
        assert_eq!(m["plugins"][0]["permissions"], json!(["todos:read"]));
        // Undo-last now picks B (A is already undone, and the undo itself is not a change to undo).
        let (_, subject) = undo_last_in(st, &d.layer, false).await.unwrap();
        assert_eq!(subject, "Add plugin B");
        assert!(!d.layer.join("plugins/b.js").exists());
        assert!(undo_last_in(st, &d.layer, false).await.is_err(), "only the starting point is left");
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn resolves_permissions_write_folders_and_api_version() {
        let (_st, d) = testutil::layer("resolve").await;
        let w = |p: &str, t: &str| std::fs::write(d.layer.join(p), t).unwrap();
        w("manifest.json", r#"{"apiVersion":1,"plugins":{"old.js":{"permissions":["todos:read","nonsense"]},"own.js":{"permissions":["chat:start"]}},"views":{"cal.html":{"permissions":["todos:read","todos:write"]}}}"#);
        w("plugins/old.js", "export default () => {}\n");
        w("plugins/own.js", "export default () => {}\n");
        w("plugins/own.json", r#"{"apiVersion":1,"permissions":["library:write","storage"],"write":["clients/","../x",".git/hooks","Notes/.Claude"],"description":"Intake"}"#);
        w("plugins/future.js", "export default () => { secret() }\n");
        w("plugins/future.json", r#"{"apiVersion":2,"permissions":["storage"]}"#);
        w("plugins/bare.js", "export default () => {}\n");
        w("views/cal.html", "<p>cal</p>");
        w("todos.json", r#"{"groupBy":"due"}"#);
        w("nav.json", "[1,2]");
        let m = manifest_lang(&d.layer, true);
        let p = |n: &str| m["plugins"].as_array().unwrap().iter().find(|x| x["name"] == n).unwrap().clone();
        assert_eq!(p("old.js")["permissions"], json!(["todos:read"]), "manifest.json is the fallback; unknown permissions are dropped");
        assert_eq!(p("own.js")["permissions"], json!(["library:write", "storage"]), "the plugin's own file wins");
        assert_eq!(p("own.js")["write"], json!(["clients"]));
        assert_eq!(p("own.js")["description"], "Intake");
        assert_eq!((p("bare.js")["permissions"].clone(), p("bare.js")["apiVersion"].clone()), (json!([]), json!(1)));
        let f = p("future.js");
        assert_eq!((f["unsupported"].clone(), f["code"].clone(), f["apiVersion"].clone()), (json!(true), json!(""), json!(2)));
        assert!(f["message"].as_str().unwrap().contains("أحدث"));
        assert_eq!(p("own.js")["unsupported"], false);
        assert_eq!(m["views"][0]["permissions"], json!(["todos:read"]), "views only ever read");
        assert_eq!(m["todos"]["groupBy"], "due");
        assert!(m["nav"].is_null(), "not an object: ignored");
        assert_eq!(m["apiVersion"], 1);
        std::fs::remove_dir_all(&d.home).unwrap();
    }
}
