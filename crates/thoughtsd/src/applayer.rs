//! The customisation layer: a git repo (`~/.thoughts/app-layer`) holding everything that makes this
//! install *yours* — CSS, themes, Home layout, plugins, views, workflows. The AI never writes here
//! directly: it works in a staging copy (see `staging.rs`) and the user applies the result. Every
//! applied change is one history entry that can be undone on its own, and Rollback restores any
//! earlier state as a whole.

use crate::staging::validate::show_hidden;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
const EXCLUDE: &str = "SUMMARY.md\n.claude/\n.mcp.json\n.DS_Store\nThumbs.db\ndesktop.ini\n";

/// The folders the layer machinery works in. Tests point these at a temp dir.
#[derive(Clone, Debug)]
pub struct Dirs {
    /// `~/.thoughts`
    pub home: PathBuf,
    /// The live layer (branch `main`).
    pub layer: PathBuf,
    /// The staging worktree (branch `staging`).
    pub staging: PathBuf,
    /// Files found changed in the live layer without a review, waiting to be reviewed (see `staging::set_aside`).
    pub held: PathBuf,
}

impl Dirs {
    pub fn real() -> Self { Self::at(&crate::paths::home()) }
    pub fn at(home: &Path) -> Self {
        Self { home: home.to_path_buf(), layer: home.join("app-layer"), staging: home.join("app-layer-staging"), held: home.join("app-layer-held") }
    }
    /// The folders that belong to the live layer at `layer` (`<home>/app-layer`).
    pub fn of_layer(layer: &Path) -> Self { Self::at(layer.parent().unwrap_or(layer)) }
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

    /// Was this chat's last run cut off by the app stopping?
    pub async fn chat_interrupted(&self, chat_id: &str) -> bool {
        match self.0 { Some(state) => state.chats.get(chat_id).await.map_or(false, |c| c.interrupted), None => false }
    }

    /// Run a git command that records something, as the app itself (the staging copy's own
    /// bookkeeping never carries the user's name, and works on a machine with no git identity).
    pub async fn run_as_app(&self, dir: &Path, args: &[&str]) -> anyhow::Result<String> {
        let mut all = vec!["-c", "user.name=Thoughts", "-c", "user.email=thoughts@localhost"];
        all.extend(args);
        self.run(dir, &all).await
    }

    /// The commit `rev` names in `dir`, in full.
    pub async fn rev(&self, dir: &Path, rev: &str) -> anyhow::Result<String> {
        Ok(self.run(dir, &["rev-parse", "--verify", "-q", "--end-of-options", &format!("{rev}^{{commit}}")]).await?.trim().to_string())
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
            g.commit_staged(dir, GUIDE_SUBJECT, &["README.md"]).await?;
            mark_system_head(g, dir).await;
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
        "description": decl["description"].as_str().map(|d| show_hidden(&d.chars().take(300).collect::<String>(), false)).unwrap_or_default(),
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

// ---------- history ----------

const GUIDE_SUBJECT: &str = "Update customisation guide";
const DEFAULTS_SUBJECT: &str = "Update built-in defaults";

/// What the app itself knows about its history entries: which ones are undos (and of what), and
/// which are its own housekeeping. Kept beside the history, inside the layer's `.git` folder where
/// no customisation can write, and never read out of the entries' text, which a pack or a summary
/// decides.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
struct Marks {
    /// Undo entry → the entry it undid (full hashes).
    undo: BTreeMap<String, String>,
    /// The app's own entries: refreshed guide, refreshed built-in defaults.
    system: BTreeSet<String>,
}

impl Marks {
    /// Is the change `hash` made still part of the layer? Not when an undo of it is itself still in effect.
    fn in_effect(&self, hash: &str) -> bool {
        !self.undo.iter().any(|(undo, of)| of == hash && self.in_effect(undo))
    }
}

fn marks_file(dir: &Path) -> PathBuf { dir.join(".git").join("thoughts-history.json") }

fn save_marks(dir: &Path, m: &Marks) {
    if let Ok(t) = serde_json::to_string(m) { let _ = crate::paths::write_atomic(&marks_file(dir), t.as_bytes(), true); }
}

async fn load_marks(g: Git<'_>, dir: &Path) -> Marks {
    if let Some(m) = std::fs::read_to_string(marks_file(dir)).ok().and_then(|t| serde_json::from_str::<Marks>(&t).ok()) { return m; }
    let m = seed_marks(g, dir).await;
    save_marks(dir, &m);
    m
}

fn full_hash(s: &str) -> bool { (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit()) }

/// `git log` as records of exactly `fields.len()` fields. Records end with a NUL, which no entry
/// can contain, and only the last field may be free text: nothing an entry says can add, hide or
/// reshape a record.
async fn log_records(g: Git<'_>, dir: &Path, range: &[&str], fields: &[&str]) -> Vec<Vec<String>> {
    let format = format!("--format={}", fields.join("%x1f"));
    let mut args = vec!["log", "-z", format.as_str()];
    args.extend(range);
    let out = g.run(dir, &args).await.unwrap_or_default();
    out.split('\0').filter(|r| !r.is_empty())
        .map(|r| r.splitn(fields.len(), '\x1f').map(String::from).collect::<Vec<_>>())
        .filter(|p| p.len() == fields.len() && full_hash(&p[0])).collect()
}

/// The files each of the entries `known` touched.
async fn files_of(g: Git<'_>, dir: &Path, range: &[&str], known: &BTreeSet<&str>) -> HashMap<String, Vec<String>> {
    let mut args = vec!["log", "-z", "--name-only", "--no-renames", "--format=%x1e%H"];
    args.extend(range);
    let out = g.run(dir, &args).await.unwrap_or_default();
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let mut current: Option<String> = None;
    for token in out.split('\0') {
        let token = token.strip_prefix('\n').unwrap_or(token);
        if let Some(h) = token.strip_prefix('\x1e').filter(|h| known.contains(h)) {
            map.entry(h.to_string()).or_default();
            current = Some(h.to_string());
        } else if !token.is_empty() {
            if let Some(list) = current.as_ref().and_then(|c| map.get_mut(c)) { list.push(token.to_string()); }
        }
    }
    map
}

/// The marks for a history that has none yet (made by an app version from before they were kept, or
/// whose record was lost). Only entries of the exact shape the app itself writes are taken: an undo
/// is `Undo: <the subject of X>` with `Reverts: <X>` as its whole body, where X is an older entry
/// (a kept change always repeats its title in its body, so its text cannot take that shape); a
/// housekeeping entry has the app's subject and touches only the app's files.
async fn seed_marks(g: Git<'_>, dir: &Path) -> Marks {
    let mut m = Marks::default();
    let recs = log_records(g, dir, &[], &["%H", "%s"]).await;
    let bodies: HashMap<String, String> = log_records(g, dir, &[], &["%H", "%b"]).await.into_iter().map(|mut p| (p.remove(0), p.remove(0))).collect();
    let known: BTreeSet<&str> = recs.iter().map(|p| p[0].as_str()).collect();
    let files = files_of(g, dir, &[], &known).await;
    for (i, p) in recs.iter().enumerate() {
        let (hash, subject) = (&p[0], p[1].trim());
        let touched = files.get(hash).map(Vec::as_slice).unwrap_or_default();
        if subject == GUIDE_SUBJECT && touched == ["README.md"] { m.system.insert(hash.clone()); continue; }
        if subject.starts_with(&format!("{DEFAULTS_SUBJECT} to version ")) && !touched.is_empty() && touched.iter().all(|f| SHIPPED.iter().any(|(s, _)| s == f)) {
            m.system.insert(hash.clone());
            continue;
        }
        let Some(of) = bodies.get(hash).and_then(|b| b.trim_end_matches(['\n', '\r']).strip_prefix("Reverts: ")).filter(|h| full_hash(h)) else { continue };
        let older = recs[i + 1..].iter().find(|o| o[0] == of);
        if older.map_or(false, |o| subject == format!("Undo: {}", o[1].trim()) || subject == format!("Undo: {}", clean_subject(&o[1]))) {
            m.undo.insert(hash.clone(), of.to_string());
        }
    }
    m
}

/// Record that the newest entry is the app's own housekeeping (never something the user undoes).
pub async fn mark_system_head(g: Git<'_>, dir: &Path) {
    let Ok(head) = g.rev(dir, "HEAD").await else { return };
    let mut m = load_marks(g, dir).await;
    if m.system.insert(head) { save_marks(dir, &m); }
}

/// A history entry's title as it is shown: one line, nothing invisible in it.
pub fn clean_subject(raw: &str) -> String {
    let line: String = raw.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    show_hidden(line.trim(), false)
}

fn kind_of(marks: &Marks, hash: &str, parents: &str) -> &'static str {
    if parents.trim().is_empty() { "start" }
    else if marks.system.contains(hash) { "system" }
    else if marks.undo.contains_key(hash) { "undo" }
    else { "change" }
}

/// `[{hash, short, date, summary, files:[path], kind, undoable, undone}]`, newest first (`subject` =
/// `summary`, kept for older UIs). `kind` is `start` (the first entry), `system` (the app's own
/// housekeeping), `undo` or `change`; only the last two can be undone on their own.
pub async fn history(state: &AppState) -> Vec<Value> { history_in(Git(Some(state)), &Dirs::real().layer).await }

pub async fn history_in(g: Git<'_>, dir: &Path) -> Vec<Value> { history_page(g, dir, 0, 100).await }

/// `limit` entries of the history after skipping the `offset` newest.
pub async fn history_page(g: Git<'_>, dir: &Path, offset: usize, limit: usize) -> Vec<Value> {
    let (skip, max) = (format!("--skip={offset}"), format!("--max-count={}", limit.clamp(1, 500)));
    let range = [skip.as_str(), max.as_str()];
    let recs = log_records(g, dir, &range, &["%H", "%h", "%cI", "%P", "%s"]).await;
    if recs.is_empty() { return vec![]; }
    let known: BTreeSet<&str> = recs.iter().map(|p| p[0].as_str()).collect();
    let mut files = files_of(g, dir, &range, &known).await;
    let marks = load_marks(g, dir).await;
    recs.iter().map(|p| {
        let kind = kind_of(&marks, &p[0], &p[3]);
        let summary = clean_subject(&p[4]);
        json!({"hash": p[0], "short": p[1], "summary": summary, "subject": summary, "date": p[2], "files": files.remove(&p[0]).unwrap_or_default(),
            "kind": kind, "undoable": kind == "change" || kind == "undo", "undone": !marks.in_effect(&p[0])})
    }).collect()
}

fn bad_entry() -> anyhow::Error { anyhow::anyhow!("bad history entry") }

/// `hash` as an entry of this layer's own history (never a commit of the staging copy, which lives
/// in the same repository and has not been reviewed).
async fn own_entry(g: Git<'_>, dir: &Path, hash: &str) -> anyhow::Result<String> {
    if !valid_hash(hash) { return Err(bad_entry()); }
    let full = g.rev(dir, hash).await.map_err(|_| bad_entry())?;
    if !full_hash(&full) || g.run(dir, &["merge-base", "--is-ancestor", &full, "HEAD"]).await.is_err() { return Err(bad_entry()); }
    Ok(full)
}

/// Restore the whole layer to `hash` as a *new* history entry, so rollback itself is undoable.
pub async fn rollback(state: &AppState, hash: &str) -> anyhow::Result<()> {
    rollback_in(Git(Some(state)), &Dirs::real().layer, hash).await?;
    state.emit(json!({"type": "layer.changed"}));
    Ok(())
}

pub async fn rollback_in(g: Git<'_>, dir: &Path, hash: &str) -> anyhow::Result<()> {
    if !valid_hash(hash) { return Err(bad_entry()); }
    // Recovery must work on a damaged layer too: what cannot be set aside is simply overwritten.
    let _ = crate::staging::set_aside(g, &Dirs::of_layer(dir)).await;
    let full = own_entry(g, dir, hash).await?;
    let short = g.run(dir, &["rev-parse", "--short", "--end-of-options", &full]).await?.trim().to_string();
    g.run(dir, &["read-tree", "-u", "--reset", "--end-of-options", &full]).await?;
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
    if !valid_hash(hash) { return Err(bad_entry()); }
    crate::staging::set_aside(g, &Dirs::of_layer(dir)).await?;
    let full = own_entry(g, dir, hash).await?;
    let rec = log_records(g, dir, &["-1", &full], &["%H", "%P", "%s"]).await.into_iter().next().ok_or_else(bad_entry)?;
    let mut marks = load_marks(g, dir).await;
    let ar = crate::tr::arabic();
    match kind_of(&marks, &full, &rec[1]) {
        "start" => anyhow::bail!("{}", if ar { "لا يمكن التراجع عن نقطة البداية. للعودة إليها استخدم «العودة إلى هذه النقطة»." }
            else { "The starting point cannot be undone. To return to it, use \u{201c}Go back to this point\u{201d}." }),
        "system" => anyhow::bail!("{}", if ar { "هذا الإدخال تحديث أجراه التطبيق لملفاته، ولا يمكن التراجع عنه." }
            else { "This entry is the app keeping its own files up to date. It cannot be undone." }),
        _ => {}
    }
    let subject = clean_subject(&rec[2]);
    if let Err(e) = g.run(dir, &["revert", "--no-commit", "--end-of-options", &full]).await {
        let _ = g.run(dir, &["revert", "--abort"]).await;
        let _ = g.run(dir, &["reset", "-q", "--hard", "HEAD"]).await;
        crate::log::warn(format!("layer: revert of {hash} failed: {e}"));
        anyhow::bail!("This change cannot be undone on its own because later changes build on it. Use Rollback to go back to an earlier version instead.");
    }
    if g.run(dir, &["diff", "--cached", "--quiet"]).await.is_ok() {
        let _ = g.run(dir, &["revert", "--quit"]).await;
        anyhow::bail!("This change is already undone.");
    }
    g.commit_staged(dir, &format!("Undo: {subject}\n\nReverts: {full}"), &[]).await?;
    let _ = g.run(dir, &["revert", "--quit"]).await;
    if let Ok(head) = g.rev(dir, "HEAD").await {
        marks.undo.insert(head, full);
        save_marks(dir, &marks);
    }
    Ok(())
}

/// The subject of history entry `hash` (empty when unknown).
pub async fn subject_in(g: Git<'_>, dir: &Path, hash: &str) -> String {
    let Ok(full) = own_entry(g, dir, hash).await else { return String::new() };
    log_records(g, dir, &["-1", &full], &["%H", "%s"]).await.into_iter().next().map(|p| clean_subject(&p[1])).unwrap_or_default()
}

/// The newest change the user can still undo: not the starting point, not the app's own
/// housekeeping (guide and built-in default updates), not an undo, and not already undone.
pub async fn last_undoable_in(g: Git<'_>, dir: &Path) -> Option<(String, String)> {
    let marks = load_marks(g, dir).await;
    log_records(g, dir, &["-200"], &["%H", "%P", "%s"]).await.into_iter()
        .find(|p| kind_of(&marks, &p[0], &p[1]) == "change" && marks.in_effect(&p[0]))
        .map(|p| (p[0].clone(), clean_subject(&p[2])))
}

/// Undo the most recent change that is still in effect. Returns `(hash, subject)` of what was undone.
pub async fn undo_last_in(g: Git<'_>, dir: &Path, arabic: bool) -> anyhow::Result<(String, String)> {
    crate::staging::set_aside(g, &Dirs::of_layer(dir)).await?;
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
    async fn history_is_read_whatever_the_entries_say() {
        let (st, d) = testutil::layer("histtext").await;
        let commit = |file: &'static str, msg: String| { let (st, d) = (st, d.clone()); async move {
            std::fs::write(d.layer.join(file), format!("/* {} */\n", uuid::Uuid::new_v4())).unwrap();
            st.commit_all(&d.layer, &msg).await.unwrap();
            st.rev(&d.layer, "HEAD").await.unwrap()
        } };
        let first = commit("custom.css", "Make text red".into()).await;
        // A title carrying the very characters the history used to be split on: still listed, cleaned.
        let odd = commit("themes.json", "Hid\u{1f}den \u{1e}title\u{202e}".into()).await;
        // A body shaped like the app's own undo note (what a pack's description or a summary could say).
        let pack = commit("home.json", format!("Import the pack\n\nImport the pack\n\nChecked by the Thoughts team.\nReverts: {first}\nReverts: 0")).await;
        let h = history_in(st, &d.layer).await;
        assert_eq!(h.len(), 4);
        assert_eq!((h[0]["hash"].as_str(), h[0]["kind"].as_str(), h[0]["undoable"].as_bool()), (Some(pack.as_str()), Some("change"), Some(true)));
        assert_eq!((h[1]["hash"].as_str(), h[1]["summary"].as_str()), (Some(odd.as_str()), Some("Hid den  title[U+202E]")));
        assert_eq!(h[1]["files"], json!(["themes.json"]));
        assert_eq!((h[2]["hash"].as_str(), h[2]["undone"].as_bool()), (Some(first.as_str()), Some(false)), "free text cannot mark another change as undone");
        assert_eq!((h[3]["kind"].as_str(), h[3]["undoable"].as_bool()), (Some("start"), Some(false)));
        // Undo-last takes the newest change, whatever its text claims to be.
        let (undone, _) = undo_last_in(st, &d.layer, false).await.unwrap();
        assert_eq!(undone, pack);
        let h = history_in(st, &d.layer).await;
        assert_eq!((h[0]["kind"].as_str(), h[1]["undone"].as_bool(), h[0]["undone"].as_bool()), (Some("undo"), Some(true), Some(false)));
        // The next one is the odd-titled change, not the one the pack's text pointed at.
        assert_eq!(undo_last_in(st, &d.layer, false).await.unwrap().0, odd);
        // Undoing an undo brings the change back, and it can then be undone again.
        let redo = h[0]["hash"].as_str().unwrap().to_string();
        revert_in(st, &d.layer, &redo).await.unwrap();
        assert_eq!(last_undoable_in(st, &d.layer).await.unwrap().0, pack);
        // A history from before the app kept its own record (or whose record was lost) is read once,
        // and only entries of the exact shape the app writes count as undos.
        std::fs::remove_file(marks_file(&d.layer)).unwrap();
        let h = history_in(st, &d.layer).await;
        let kinds: Vec<&str> = h.iter().map(|e| e["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["undo", "undo", "undo", "change", "change", "change", "start"]);
        assert_eq!(last_undoable_in(st, &d.layer).await.unwrap().0, pack);
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn the_starting_point_and_the_apps_own_entries_cannot_be_undone() {
        let (st, d) = testutil::layer("noundo").await;
        std::fs::write(d.layer.join("README.md"), "old").unwrap();
        st.commit_all(&d.layer, "old guide").await.unwrap();
        ensure_in(st, &d).await.unwrap();
        std::fs::write(d.layer.join("plugins/a.js"), "export default () => {}\n").unwrap();
        st.commit_all(&d.layer, "Add a plugin").await.unwrap();
        let h = history_in(st, &d.layer).await;
        let (guide, root) = (&h[1], h.last().unwrap());
        assert_eq!((guide["summary"].as_str(), guide["kind"].as_str(), guide["undoable"].as_bool()), (Some("Update customisation guide"), Some("system"), Some(false)));
        assert_eq!((root["kind"].as_str(), root["undoable"].as_bool()), (Some("start"), Some(false)));
        // Undoing the first entry used to succeed and delete the starting files.
        let e = revert_in(st, &d.layer, root["hash"].as_str().unwrap()).await.unwrap_err().to_string();
        assert!(e.contains("starting point"), "{e}");
        assert!(revert_in(st, &d.layer, guide["hash"].as_str().unwrap()).await.unwrap_err().to_string().contains("own files"));
        for kept in ["custom.css", "home.json", "themes.json", "manifest.json", "CLAUDE.md"] { assert!(d.layer.join(kept).exists(), "{kept}"); }
        assert_eq!(history_in(st, &d.layer).await.len(), h.len(), "nothing was recorded");
        // A user's change with the app's subject is still the user's: it stays undoable.
        std::fs::write(d.layer.join("custom.css"), "body{}\n").unwrap();
        st.commit_all(&d.layer, "Update customisation guide").await.unwrap();
        assert_eq!(history_in(st, &d.layer).await[0]["kind"], "change");
        assert_eq!(last_undoable_in(st, &d.layer).await.unwrap().1, "Update customisation guide");
        // Only entries of this history are accepted: not a commit of the staging copy beside it.
        crate::staging::recreate(st, &d, "customize", None).await.unwrap();
        std::fs::write(d.staging.join("plugins/unreviewed.js"), "export default () => {}\n").unwrap();
        st.run(&d.staging, &["add", "-A"]).await.unwrap();
        st.run_as_app(&d.staging, &["commit", "-q", "-m", "pending change"]).await.unwrap();
        let staged = st.rev(&d.staging, "HEAD").await.unwrap();
        assert!(rollback_in(st, &d.layer, &staged).await.is_err() && revert_in(st, &d.layer, &staged).await.is_err());
        assert!(!d.layer.join("plugins/unreviewed.js").exists());
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn history_goes_back_as_far_as_it_exists() {
        let (st, d) = testutil::layer("paging").await;
        for i in 1..=7 {
            std::fs::write(d.layer.join("custom.css"), format!("/* {i} */\n")).unwrap();
            st.commit_all(&d.layer, &format!("Change {i}")).await.unwrap();
        }
        let titles = |v: Vec<Value>| v.iter().map(|e| e["summary"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(titles(history_page(st, &d.layer, 0, 3).await), ["Change 7", "Change 6", "Change 5"]);
        assert_eq!(titles(history_page(st, &d.layer, 3, 3).await), ["Change 4", "Change 3", "Change 2"]);
        assert_eq!(titles(history_page(st, &d.layer, 6, 3).await), ["Change 1", "Starting point"]);
        assert!(history_page(st, &d.layer, 8, 3).await.is_empty());
        assert_eq!(history_page(st, &d.layer, 6, 3).await[1]["files"].as_array().unwrap().len(), 9, "the first entry lists the starting files");
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
