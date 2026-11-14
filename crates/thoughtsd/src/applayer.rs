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
pub const PERMISSIONS: &[&str] = &["library:read", "library:write", "todos:read", "todos:write", "ideas:read", "ideas:write", "chat:start", "storage",
    "files:save", "notify", "assistant:ask"];
/// What a sandboxed HTML view may declare (views only ever read).
pub const VIEW_PERMISSIONS: &[&str] = &["library:read", "todos:read", "ideas:read"];
/// The permissions that read the library. A `read` list narrows all three to its folders.
pub const READ_PERMISSIONS: &[&str] = &["library:read", "todos:read", "ideas:read"];

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
/// Which declaration is in effect for `sub/file`, the one rule every reader and writer of
/// declarations follows: the item's own `<stem>.json` when it is a regular file (never a link) of at
/// most 64 KB holding a JSON object, else its entry in `manifest.json` (`shared`) when that is an
/// object, else none.
#[derive(Debug, Clone, PartialEq)]
pub enum DeclIn {
    /// The own file, layer-relative (`plugins/hours.json`), with what it says.
    Own(String, Value),
    /// The entry `shared[sub][file]`.
    Manifest(Value),
    None,
}

pub fn decl_in_effect(dir: &Path, sub: &str, file: &str, shared: &Value) -> DeclIn {
    let stem = Path::new(file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let rel = format!("{sub}/{stem}.json");
    let own_path = dir.join(&rel);
    let own = std::fs::symlink_metadata(&own_path).ok().filter(|m| m.is_file() && m.len() <= 64 * 1024)
        .and_then(|_| std::fs::read_to_string(&own_path).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object);
    match own {
        Some(o) => DeclIn::Own(rel, o),
        None if shared[sub][file].is_object() => DeclIn::Manifest(shared[sub][file].clone()),
        None => DeclIn::None,
    }
}

pub fn resolve_decl(dir: &Path, sub: &str, file: &str, shared: &Value, arabic: bool) -> Value {
    let found = decl_in_effect(dir, sub, file, shared);
    let (decl, version) = match &found {
        DeclIn::Own(_, o) => (o.clone(), o["apiVersion"].as_u64().unwrap_or(API_VERSION)),
        DeclIn::Manifest(m) => (m.clone(), shared["apiVersion"].as_u64().unwrap_or(API_VERSION)),
        DeclIn::None => (Value::Null, shared["apiVersion"].as_u64().unwrap_or(API_VERSION)),
    };
    let own = matches!(found, DeclIn::Own(..)).then_some(());
    let allowed = if sub == "views" { VIEW_PERMISSIONS } else { PERMISSIONS };
    let mut permissions: Vec<&str> = decl["permissions"].as_array().into_iter().flatten().filter_map(Value::as_str)
        .filter_map(|p| allowed.iter().find(|a| **a == p).copied()).collect();
    permissions.dedup();
    let write: Vec<String> = if sub == "views" { vec![] } else {
        decl["write"].as_array().into_iter().flatten().filter_map(Value::as_str).filter_map(|w| clean_write_folder(w).ok()).take(20).collect()
    };
    // The only folders it may read in (empty: the whole library but private folders). A folder that
    // cannot be one is dropped; when nothing is left of a list that was given, it reads nothing.
    let read: Option<Vec<String>> = decl["read"].as_array().map(|l| l.iter().filter_map(Value::as_str).filter_map(|w| clean_write_folder(w).ok()).take(20).collect());
    let reads_nothing = read.as_ref().map_or(false, Vec::is_empty);
    let unsupported = version > API_VERSION;
    let mut v = json!({
        "permissions": permissions, "write": write, "read": read.unwrap_or_default(), "readsNothing": reads_nothing, "apiVersion": version,
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
pub fn manifest_lang(dir: &Path, arabic: bool) -> Value { manifest_with(dir, arabic, &switched_off(dir)) }

/// The staged layer for "Try it": what the user switched off stays off. The switched-off list is
/// the live layer's, since only the app writes it and a staged copy of it is never kept.
pub fn manifest_preview(staging: &Path, layer: &Path, arabic: bool) -> Value { manifest_with(staging, arabic, &switched_off(layer)) }

fn manifest_with(dir: &Path, arabic: bool, off: &BTreeSet<String>) -> Value {
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap_or_default();
    let parsed = |p: &str| std::fs::read_to_string(dir.join(p)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    let object = |p: &str| Some(parsed(p)).filter(Value::is_object).unwrap_or(Value::Null);
    let shared = parsed("manifest.json");
    // What the user switched off is not handed to the UI at all: it is listed under `off` only.
    let list = |sub: &str, ext: &str| -> Vec<Value> {
        let mut v: Vec<Value> = std::fs::read_dir(dir.join(sub)).into_iter().flatten().flatten()
            .filter(|e| e.path().extension().map_or(false, |x| x == ext) && !e.file_name().to_string_lossy().starts_with('.'))
            .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
            .filter(|e| !off.contains(&format!("{sub}/{}", e.file_name().to_string_lossy())))
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
        "pages": object("pages.json"),
        "workflows": crate::workflows::load(dir),
        "off": off.iter().filter(|p| dir.join(p).is_file()).collect::<Vec<_>>(),
    })
}

// ---------- switching one add-on or page off, removing it ----------

/// The layer file listing the add-ons and pages the user switched off: `{"off": ["plugins/hours.js"]}`.
pub const ADDONS_FILE: &str = "addons.json";

/// `plugins/<plain name>.js` or `views/<plain name>.html` → `("plugins" | "views", file name)`.
pub fn item_path(path: &str) -> Option<(&'static str, &str)> {
    let (sub, file) = path.split_once('/')?;
    let (sub, ext) = match sub { "plugins" => ("plugins", ".js"), "views" => ("views", ".html"), _ => return None };
    (file.len() > ext.len() && file.ends_with(ext) && crate::layer_api::valid_plugin_name(file)).then_some((sub, file))
}

/// The add-ons and pages switched off in the layer at `dir`.
pub fn switched_off(dir: &Path) -> BTreeSet<String> {
    let v: Value = std::fs::read_to_string(dir.join(ADDONS_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    v["off"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|p| item_path(p).is_some()).map(String::from).collect()
}

fn read_object(path: &Path) -> Option<serde_json::Map<String, Value>> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned())
}

fn write_json(path: &Path, v: &Value) -> anyhow::Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(v)? + "\n")?;
    Ok(())
}

/// Write the switched-off list (keeping anything else the file holds).
fn write_off(dir: &Path, off: &BTreeSet<String>) -> anyhow::Result<()> {
    let mut o = read_object(&dir.join(ADDONS_FILE)).unwrap_or_default();
    o.insert("off".into(), json!(off));
    write_json(&dir.join(ADDONS_FILE), &Value::Object(o))
}

/// A change the app makes to the live layer for the user, outside a review: `edit` changes the
/// files it names, and they (nothing else) become one history entry, marked as `act`. Whatever was
/// in the live layer without a review is set aside first, and if anything fails the layer is put
/// back exactly as it was. Returns the new entry (empty when nothing changed).
async fn app_change(g: Git<'_>, dir: &Path, subject: &str, act: Act, edit: impl FnOnce(&Path) -> anyhow::Result<Vec<String>>) -> anyhow::Result<String> {
    crate::staging::set_aside(g, &Dirs::of_layer(dir)).await?;
    let head = g.rev(dir, "HEAD").await?;
    let done: anyhow::Result<String> = async {
        let touched = edit(dir)?;
        if touched.is_empty() { return Ok(String::new()); }
        let mut add = vec!["add", "-A", "--"];
        add.extend(touched.iter().map(String::as_str));
        g.run(dir, &add).await?;
        if g.run(dir, &["diff", "--cached", "--quiet"]).await.is_ok() { return Ok(String::new()); }
        g.commit_staged(dir, subject, &[]).await?;
        g.rev(dir, "HEAD").await
    }.await;
    match done {
        Ok(hash) => {
            if !hash.is_empty() { mark_act(g, dir, act).await; }
            Ok(hash)
        }
        Err(e) => {
            let _ = g.run(dir, &["reset", "-q", "--hard", &head]).await;
            let _ = g.run(dir, &["clean", "-fdq"]).await;
            crate::log::warn(format!("layer: {subject} failed and was undone: {e}"));
            Err(e)
        }
    }
}

fn known_item<'p>(dir: &Path, path: &'p str) -> anyhow::Result<(&'static str, &'p str)> {
    let item = item_path(path).filter(|_| std::fs::symlink_metadata(dir.join(path)).map_or(false, |m| m.is_file()));
    item.ok_or_else(|| crate::tr::err("addon.gone", &[]))
}

/// Switch one add-on or page off (`on = false`) or back on. It stays in the layer, is simply not
/// loaded, and the switch is one history entry of its own, so it works whatever later changes did
/// and can itself be undone. Returns the new entry (empty when it already was that way).
pub async fn switch_in(g: Git<'_>, dir: &Path, path: &str, on: bool) -> anyhow::Result<String> {
    known_item(dir, path)?;
    let mut off = switched_off(dir);
    if off.contains(path) != on { return Ok(String::new()); }
    let subject = format!("{} {path}", if on { "Switch on" } else { "Switch off" });
    let act = Act { kind: (if on { "on" } else { "off" }).into(), item: path.to_string(), ..Default::default() };
    app_change(g, dir, &subject, act, |dir| {
        if on { off.remove(path); } else { off.insert(path.to_string()); }
        write_off(dir, &off)?;
        Ok(vec![ADDONS_FILE.to_string()])
    }).await
}

/// Take one add-on or page out of the layer: its file, its own declaration, its entry in
/// `manifest.json` and its line in the switched-off list. A history entry of its own, so it works
/// whatever later changes did, and undoing it brings everything back. Its saved data is kept.
pub async fn remove_in(g: Git<'_>, dir: &Path, path: &str) -> anyhow::Result<String> {
    let (sub, file) = known_item(dir, path)?;
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s).to_string();
    let act = Act { kind: "remove".into(), item: path.to_string(), ..Default::default() };
    app_change(g, dir, &format!("Remove {path}"), act, |dir| {
        let mut touched = vec![path.to_string()];
        std::fs::remove_file(dir.join(path))?;
        let decl = format!("{sub}/{stem}.json");
        if std::fs::symlink_metadata(dir.join(&decl)).map_or(false, |m| m.is_file()) {
            std::fs::remove_file(dir.join(&decl))?;
            touched.push(decl);
        }
        if let Some(mut m) = read_object(&dir.join("manifest.json")) {
            if m.get_mut(sub).and_then(Value::as_object_mut).and_then(|s| s.remove(file)).is_some() {
                write_json(&dir.join("manifest.json"), &Value::Object(m))?;
                touched.push("manifest.json".into());
            }
        }
        let mut off = switched_off(dir);
        if off.remove(path) {
            write_off(dir, &off)?;
            touched.push(ADDONS_FILE.into());
        }
        Ok(touched)
    }).await
}

/// Take one permission away from an add-on or page (and, with `library:write`, its write folders),
/// wherever its declaration lives. The add-on may stop working in the parts that needed it.
pub async fn withdraw_in(g: Git<'_>, dir: &Path, path: &str, permission: &str) -> anyhow::Result<String> {
    let (sub, file) = known_item(dir, path)?;
    let shared_of = |dir: &Path| std::fs::read_to_string(dir.join("manifest.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    let granted = |dir: &Path| resolve_decl(dir, sub, file, &shared_of(dir), false)["permissions"].as_array().map_or(false, |a| a.iter().any(|p| p == permission));
    if !granted(dir) { return Err(crate::tr::err("addon.no_permission", &[])); }
    let act = Act { kind: "permission".into(), item: path.to_string(), detail: permission.to_string() };
    app_change(g, dir, &format!("Take away {permission} from {path}"), act, |dir| {
        let take = |decl: &mut serde_json::Map<String, Value>| {
            if let Some(Value::Array(list)) = decl.get_mut("permissions") { list.retain(|p| p != permission); }
            if permission == "library:write" { decl.remove("write"); }
        };
        // Exactly the declaration that is in effect (`decl_in_effect`, the rule `resolve_decl` reads by).
        let touched = match decl_in_effect(dir, sub, file, &shared_of(dir)) {
            DeclIn::Own(rel, Value::Object(mut decl)) => {
                take(&mut decl);
                write_json(&dir.join(&rel), &Value::Object(decl))?;
                rel
            }
            DeclIn::Manifest(_) => {
                let mut m = read_object(&dir.join("manifest.json")).ok_or_else(|| crate::tr::err("addon.no_permission", &[]))?;
                let decl = m.get_mut(sub).and_then(Value::as_object_mut).and_then(|s| s.get_mut(file)).and_then(Value::as_object_mut)
                    .ok_or_else(|| crate::tr::err("addon.no_permission", &[]))?;
                take(decl);
                write_json(&dir.join("manifest.json"), &Value::Object(m))?;
                "manifest.json".to_string()
            }
            _ => return Err(crate::tr::err("addon.no_permission", &[])),
        };
        // Never report success while the permission is still granted (the change is then undone).
        if granted(dir) { anyhow::bail!("the permission {permission} is still granted to {path} after it was taken away"); }
        Ok(vec![touched])
    }).await
}

/// Every add-on and page in the layer at `dir`, on or off: `{path, name, kind, on, permissions,
/// write, description, unsupported, message?}`, plugins first, by name.
pub fn addons_of(dir: &Path, arabic: bool) -> Vec<Value> {
    let shared = std::fs::read_to_string(dir.join("manifest.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    let off = switched_off(dir);
    let mut out = vec![];
    for (sub, ext, kind) in [("plugins", "js", "plugin"), ("views", "html", "view")] {
        let mut names: Vec<String> = std::fs::read_dir(dir.join(sub)).into_iter().flatten().flatten()
            .filter(|e| e.file_type().map_or(false, |t| t.is_file()) && e.path().extension().map_or(false, |x| x == ext))
            .map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| !n.starts_with('.')).collect();
        names.sort();
        for name in names {
            let path = format!("{sub}/{name}");
            let mut v = resolve_decl(dir, sub, &name, &shared, arabic);
            v["path"] = json!(path);
            v["name"] = json!(name);
            v["label"] = json!(item_label(&path));
            v["kind"] = json!(kind);
            v["on"] = json!(!off.contains(&path));
            // Only a plain name can be switched off, removed or keep saved data.
            v["manageable"] = json!(item_path(&path).is_some());
            out.push(v);
        }
    }
    out
}

// ---------- history ----------

const GUIDE_SUBJECT: &str = "Update customisation guide";
const DEFAULTS_SUBJECT: &str = "Update built-in defaults";
const BACK_SUBJECT: &str = "Went back to an earlier version";

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
    /// Entries the app wrote for something the user did outside a review (see `Act`).
    acts: BTreeMap<String, Act>,
    /// Kept pack imports: what the pack said it was, and the fingerprint the app calculated.
    packs: BTreeMap<String, PackMark>,
}

/// What an entry the app wrote for the user stands for: `back` (went back to an earlier version,
/// `item` = its short id), `off` / `on` / `remove` (an add-on or page, `item` = its file) and
/// `permission` (`detail` = the permission taken away from `item`). Its title is written from this
/// in the language of the moment, never read out of the stored text.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Act { pub kind: String, pub item: String, pub detail: String }

/// A kept pack: its own claims (name, author, version) and the fingerprint the app calculated.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct PackMark { pub name: String, pub author: String, pub version: String, pub fingerprint: String }

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
        // "Went back to an earlier version (<short id of an older entry>)": only its title is read
        // from this (it stays an ordinary entry that can be undone).
        if let Some(short) = subject.strip_prefix(&format!("{BACK_SUBJECT} (")).and_then(|r| r.strip_suffix(')')) {
            if valid_hash(short) && recs[i + 1..].iter().any(|o| o[0].starts_with(short)) {
                m.acts.insert(hash.clone(), Act { kind: "back".into(), item: short.to_string(), ..Default::default() });
                continue;
            }
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

/// What an entry is, finer than `kind_of`, for History to group and name it: `start`, `guide`,
/// `defaults`, `undo`, `back`, `off`, `on`, `remove`, `permission`, `pack` or `change`.
fn entry_of(marks: &Marks, hash: &str, parents: &str, stored: &str) -> &'static str {
    match kind_of(marks, hash, parents) {
        "start" => return "start",
        "system" => return if stored.starts_with(DEFAULTS_SUBJECT) { "defaults" } else { "guide" },
        "undo" => return "undo",
        _ => {}
    }
    match marks.acts.get(hash).map(|a| a.kind.as_str()) {
        Some("back") => "back",
        Some("off") => "off",
        Some("on") => "on",
        Some("remove") => "remove",
        Some("permission") => "permission",
        _ if marks.packs.contains_key(hash) => "pack",
        _ => "change",
    }
}

/// An add-on's or page's name as people know it: its file name without `.js` / `.html`.
pub fn item_label(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    show_hidden(file.strip_suffix(".js").or_else(|| file.strip_suffix(".html")).unwrap_or(file), false)
}

/// A permission in a few plain words (for History; the add-on list uses the UI's longer wording).
pub fn permission_words(p: &str, ar: bool) -> &'static str {
    match (p, ar) {
        ("library:read", false) => "read your notes", ("library:read", true) => "قراءة ملاحظاتك",
        ("library:write", false) => "write in your library", ("library:write", true) => "الكتابة في مكتبتك",
        ("todos:read", false) => "read your todos", ("todos:read", true) => "قراءة مهامك",
        ("todos:write", false) => "change your todos", ("todos:write", true) => "تعديل مهامك",
        ("ideas:read", false) => "read your ideas", ("ideas:read", true) => "قراءة أفكارك",
        ("ideas:write", false) => "change the status of your ideas", ("ideas:write", true) => "تغيير حالة أفكارك",
        ("chat:start", false) => "start chats", ("chat:start", true) => "بدء المحادثات",
        ("storage", false) => "keep its own saved data", ("storage", true) => "حفظ بياناتها الخاصة",
        (_, false) => "a permission", (_, true) => "صلاحية",
    }
}

/// An entry's title in the UI language. The app's own entries are named from what the app recorded
/// about them, so they read in the language of the moment; every other title is shown as stored
/// (the words of the change's summary).
fn title_of(marks: &Marks, hash: &str, parents: &str, raw: &str, ar: bool, depth: u8) -> String {
    let stored = clean_subject(raw);
    let act = marks.acts.get(hash).cloned().unwrap_or_default();
    let page = act.item.starts_with("views/");
    let name = item_label(&act.item);
    match entry_of(marks, hash, parents, &stored) {
        "start" => (if ar { "نقطة البداية" } else { "Starting point" }).into(),
        "guide" => (if ar { "تحديث دليل التخصيص" } else { "Update customisation guide" }).into(),
        "defaults" => {
            let v = stored.strip_prefix(&format!("{DEFAULTS_SUBJECT} to version ")).unwrap_or("").trim();
            if ar { format!("تحديث الملفات الافتراضية المرفقة بالتطبيق إلى الإصدار {v}") } else { format!("{DEFAULTS_SUBJECT} to version {v}") }
        }
        "undo" => {
            let inner = stored.strip_prefix("Undo: ").unwrap_or(&stored).to_string();
            // (An undo is never of the starting point, so the entry it undid has a parent.)
            let of = marks.undo.get(hash).cloned().unwrap_or_default();
            let title = if depth < 8 { title_of(marks, &of, "-", &inner, ar, depth + 1) } else { inner };
            if ar { format!("تراجع: {title}") } else { format!("Undo: {title}") }
        }
        "back" => if ar { format!("العودة إلى إصدار سابق ({})", act.item) } else { format!("{BACK_SUBJECT} ({})", act.item) },
        "off" => match (ar, page) {
            (true, true) => format!("إيقاف الصفحة «{name}»"), (true, false) => format!("إيقاف الإضافة «{name}»"),
            (false, true) => format!("Switched off the page \u{201c}{name}\u{201d}"), (false, false) => format!("Switched off the add-on \u{201c}{name}\u{201d}"),
        },
        "on" => match (ar, page) {
            (true, true) => format!("تشغيل الصفحة «{name}»"), (true, false) => format!("تشغيل الإضافة «{name}»"),
            (false, true) => format!("Switched on the page \u{201c}{name}\u{201d}"), (false, false) => format!("Switched on the add-on \u{201c}{name}\u{201d}"),
        },
        "remove" => match (ar, page) {
            (true, true) => format!("إزالة الصفحة «{name}»"), (true, false) => format!("إزالة الإضافة «{name}»"),
            (false, true) => format!("Removed the page \u{201c}{name}\u{201d}"), (false, false) => format!("Removed the add-on \u{201c}{name}\u{201d}"),
        },
        "permission" => {
            let what = permission_words(&act.detail, ar);
            if ar { format!("سحب صلاحية من «{name}»: {what}") } else { format!("Took a permission away from \u{201c}{name}\u{201d}: {what}") }
        }
        "pack" => {
            let n = marks.packs.get(hash).map(|p| p.name.clone()).unwrap_or_default();
            if ar { format!("إضافة الحزمة «{n}»") } else { format!("Added the pack \u{201c}{n}\u{201d}") }
        }
        _ => stored,
    }
}

/// `[{hash, short, date, summary, files:[path], kind, entry, undoable, undone, pack?}]`, newest
/// first (`subject` = `summary`, kept for older UIs). `kind` is `start` (the first entry), `system`
/// (the app's own housekeeping), `undo` or `change`; only the last two can be undone on their own.
/// `entry` names it more finely (see `entry_of`), and the app's own entries are titled in the UI
/// language. `pack`: for a kept pack, what it said it was and the fingerprint the app calculated.
pub async fn history(state: &AppState) -> Vec<Value> { history_in(Git(Some(state)), &Dirs::real().layer).await }

pub async fn history_in(g: Git<'_>, dir: &Path) -> Vec<Value> { history_page(g, dir, 0, 100).await }

/// `limit` entries of the history after skipping the `offset` newest.
pub async fn history_page(g: Git<'_>, dir: &Path, offset: usize, limit: usize) -> Vec<Value> {
    history_page_in(g, dir, offset, limit, crate::tr::arabic()).await
}

pub async fn history_page_in(g: Git<'_>, dir: &Path, offset: usize, limit: usize, ar: bool) -> Vec<Value> {
    let (skip, max) = (format!("--skip={offset}"), format!("--max-count={}", limit.clamp(1, 500)));
    let range = [skip.as_str(), max.as_str()];
    let recs = log_records(g, dir, &range, &["%H", "%h", "%cI", "%P", "%s"]).await;
    if recs.is_empty() { return vec![]; }
    let known: BTreeSet<&str> = recs.iter().map(|p| p[0].as_str()).collect();
    let mut files = files_of(g, dir, &range, &known).await;
    let marks = load_marks(g, dir).await;
    recs.iter().map(|p| {
        let kind = kind_of(&marks, &p[0], &p[3]);
        let summary = title_of(&marks, &p[0], &p[3], &p[4], ar, 0);
        let mut v = json!({"hash": p[0], "short": p[1], "summary": summary, "subject": summary, "date": p[2], "files": files.remove(&p[0]).unwrap_or_default(),
            "kind": kind, "entry": entry_of(&marks, &p[0], &p[3], &clean_subject(&p[4])), "undoable": kind == "change" || kind == "undo", "undone": !marks.in_effect(&p[0])});
        if let Some(pk) = marks.packs.get(&p[0]) {
            v["pack"] = json!(pk);
            v["pack"]["code"] = json!(crate::packs::fingerprint_code(&pk.fingerprint));
        }
        v
    }).collect()
}

/// Record what the pack kept as entry `hash` said it was (see `PackMark`).
pub async fn mark_pack(g: Git<'_>, dir: &Path, hash: &str, pack: PackMark) {
    if !full_hash(hash) { return; }
    let mut m = load_marks(g, dir).await;
    m.packs.insert(hash.to_string(), pack);
    save_marks(dir, &m);
}

/// The packs kept so far (newest last): `(entry, pack)`.
pub async fn kept_packs(g: Git<'_>, dir: &Path) -> Vec<(String, PackMark)> {
    let m = load_marks(g, dir).await;
    let order: Vec<String> = log_records(g, dir, &["--reverse"], &["%H", "%s"]).await.into_iter().map(|mut p| p.remove(0)).collect();
    order.into_iter().filter_map(|h| m.packs.get(&h).map(|p| (h.clone(), p.clone()))).collect()
}

async fn mark_act(g: Git<'_>, dir: &Path, act: Act) {
    let Ok(head) = g.rev(dir, "HEAD").await else { return };
    let mut m = load_marks(g, dir).await;
    m.acts.insert(head, act);
    save_marks(dir, &m);
}

fn bad_entry() -> anyhow::Error { crate::tr::err("layer.bad_entry", &[]) }

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
    if g.commit_all(dir, &format!("{BACK_SUBJECT} ({short})")).await? {
        mark_act(g, dir, Act { kind: "back".into(), item: short, ..Default::default() }).await;
    }
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
        return Err(crate::tr::err("layer.undo_builds_on", &[]));
    }
    if g.run(dir, &["diff", "--cached", "--quiet"]).await.is_ok() {
        let _ = g.run(dir, &["revert", "--quit"]).await;
        return Err(crate::tr::err("layer.already_undone", &[]));
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
        .map(|p| (p[0].clone(), title_of(&marks, &p[0], &p[1], &p[2], crate::tr::arabic(), 0)))
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

    /// FEAT-22: one add-on is switched off, back on, loses a permission or is removed, although a
    /// later change built on the change that added it (so undoing that change is refused). Each is
    /// one history entry that can itself be undone.
    #[tokio::test]
    async fn one_add_on_can_be_switched_off_or_removed_whatever_came_after() {
        let (st, d) = testutil::layer("addon-off").await;
        let w = |p: &str, t: &str| std::fs::write(d.layer.join(p), t).unwrap();
        w("plugins/hours.js", "export default () => { /* hours */ }\n");
        w("plugins/hours.json", r#"{"apiVersion":1,"permissions":["storage","todos:read"]}"#);
        w("views/board.html", "<p>board</p>");
        st.commit_all(&d.layer, "Add hours and a board").await.unwrap();
        let added = st.rev(&d.layer, "HEAD").await.unwrap();
        w("plugins/hours.js", "export default () => { /* hours, v2 */ }\n");
        st.commit_all(&d.layer, "Improve hours").await.unwrap();
        assert!(revert_in(st, &d.layer, &added).await.unwrap_err().to_string().contains("later changes build on it"));
        let names = |m: &Value, k: &str| m[k].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();

        // Off: not handed to the app at all, still in the layer, listed as off.
        let off = switch_in(st, &d.layer, "plugins/hours.js", false).await.unwrap();
        assert!(!off.is_empty());
        let m = manifest_of(&d.layer);
        assert!(names(&m, "plugins").is_empty() && names(&m, "views") == ["board.html"]);
        assert_eq!(m["off"], json!(["plugins/hours.js"]));
        assert!(d.layer.join("plugins/hours.js").exists());
        assert_eq!(switch_in(st, &d.layer, "plugins/hours.js", false).await.unwrap(), "", "already off: no entry");
        let a = addons_of(&d.layer, false);
        assert_eq!((a[0]["path"].as_str(), a[0]["on"].as_bool(), a[1]["on"].as_bool()), (Some("plugins/hours.js"), Some(false), Some(true)));
        let h = history_page_in(st, &d.layer, 0, 10, false).await;
        assert_eq!((h[0]["entry"].as_str(), h[0]["summary"].as_str(), h[0]["undoable"].as_bool()), (Some("off"), Some("Switched off the add-on \u{201c}hours\u{201d}"), Some(true)));
        assert_eq!(history_page_in(st, &d.layer, 0, 1, true).await[0]["summary"], "إيقاف الإضافة «hours»");
        // Undoing the switch (here: the native "Undo last app change") turns it back on.
        let (undone, title) = undo_last_in(st, &d.layer, false).await.unwrap();
        assert_eq!((undone.as_str(), title.as_str()), (off.as_str(), "Switched off the add-on \u{201c}hours\u{201d}"));
        assert_eq!(names(&manifest_of(&d.layer), "plugins"), ["hours.js"]);
        assert_eq!(history_page_in(st, &d.layer, 0, 1, false).await[0]["summary"], "Undo: Switched off the add-on \u{201c}hours\u{201d}");
        assert_eq!(history_page_in(st, &d.layer, 0, 1, true).await[0]["summary"], "تراجع: إيقاف الإضافة «hours»");
        // Off, then on again by hand.
        switch_in(st, &d.layer, "views/board.html", false).await.unwrap();
        assert!(names(&manifest_of(&d.layer), "views").is_empty());
        switch_in(st, &d.layer, "views/board.html", true).await.unwrap();
        assert_eq!(names(&manifest_of(&d.layer), "views"), ["board.html"]);
        assert_eq!(history_page_in(st, &d.layer, 0, 1, false).await[0]["summary"], "Switched on the page \u{201c}board\u{201d}");

        // A permission taken away, from wherever the declaration lives.
        withdraw_in(st, &d.layer, "plugins/hours.js", "todos:read").await.unwrap();
        assert_eq!(manifest_of(&d.layer)["plugins"][0]["permissions"], json!(["storage"]));
        assert!(withdraw_in(st, &d.layer, "plugins/hours.js", "todos:read").await.is_err(), "it has it no more");
        assert_eq!(history_page_in(st, &d.layer, 0, 1, false).await[0]["summary"], "Took a permission away from \u{201c}hours\u{201d}: read your todos");
        w("manifest.json", r#"{"apiVersion":1,"plugins":{"old.js":{"permissions":["library:write","todos:read"],"write":["clients/"]}}}"#);
        w("plugins/old.js", "export default () => {}\n");
        st.commit_all(&d.layer, "Add an old-style add-on").await.unwrap();
        withdraw_in(st, &d.layer, "plugins/old.js", "library:write").await.unwrap();
        let old = manifest_of(&d.layer)["plugins"].as_array().unwrap().iter().find(|p| p["name"] == "old.js").unwrap().clone();
        assert_eq!((old["permissions"].clone(), old["write"].clone()), (json!(["todos:read"]), json!([])), "its write folders go with it");

        // Removed: every trace of it in the layer, in one entry; its saved data is not touched.
        switch_in(st, &d.layer, "plugins/old.js", false).await.unwrap();
        crate::layer_api::storage_write(&d.home, "old.js", br#"{"x":1}"#, false).unwrap();
        let removed = remove_in(st, &d.layer, "plugins/old.js").await.unwrap();
        assert!(!d.layer.join("plugins/old.js").exists());
        let man: Value = serde_json::from_str(&std::fs::read_to_string(d.layer.join("manifest.json")).unwrap()).unwrap();
        assert!(man["plugins"].get("old.js").is_none());
        assert!(switched_off(&d.layer).is_empty());
        assert_eq!(crate::layer_api::storage_read(&d.home, "old.js").unwrap()["x"], 1);
        remove_in(st, &d.layer, "plugins/hours.js").await.unwrap();
        assert!(!d.layer.join("plugins/hours.json").exists());
        assert_eq!(history_page_in(st, &d.layer, 0, 1, true).await[0]["summary"], "إزالة الإضافة «hours»");
        // The remove of "old" is undone on its own although "hours" was removed after it.
        revert_in(st, &d.layer, &removed).await.unwrap();
        assert!(d.layer.join("plugins/old.js").exists());
        assert_eq!(switched_off(&d.layer), BTreeSet::from(["plugins/old.js".to_string()]), "back as it was: off");
        // Nothing outside the layer's add-ons can be reached this way.
        for bad in ["plugins/../manifest.json", "custom.css", "plugins/gone.js", "plugins/a b.js", "views/x.js", "plugins/hours.json"] {
            assert!(switch_in(st, &d.layer, bad, false).await.is_err() && remove_in(st, &d.layer, bad).await.is_err(), "{bad}");
        }
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    /// Only the app's own switch puts an add-on back on: a pack cannot carry the switched-off list,
    /// a change prepared in the staging copy cannot keep an edit of it (nor show it in "Try it"),
    /// and an edit made outside the app is set aside.
    #[tokio::test]
    async fn only_the_app_switches_an_add_on_back_on() {
        let (st, d) = testutil::layer("addon-onlyapp").await;
        std::fs::write(d.layer.join("plugins/a.js"), "export default () => {}\n").unwrap();
        st.commit_all(&d.layer, "Add a").await.unwrap();
        switch_in(st, &d.layer, "plugins/a.js", false).await.unwrap();
        let off = || switched_off(&d.layer).contains("plugins/a.js");
        let pack = json!({"format": "thoughts-pack", "version": 1, "kind": "feature", "name": "x", "files": {ADDONS_FILE: "{\"off\": []}\n", "plugins/b.js": "export default () => {}\n"}});
        assert!(crate::packs::check_pack(&pack).is_err());
        assert!(crate::packs::check_path(ADDONS_FILE).is_err());
        // Prepared in staging (as an assistant run could): not part of the change, never kept, not tried.
        crate::staging::recreate(st, &d, "customize", None).await.unwrap();
        std::fs::write(d.staging.join(ADDONS_FILE), "{\"off\": []}\n").unwrap();
        std::fs::write(d.staging.join("plugins/b.js"), "export default () => {}\n").unwrap();
        let p = crate::staging::pending(st, &d, false).await;
        assert_eq!(p["files"], json!([{"path": "plugins/b.js", "status": "added"}]));
        assert_eq!(p["ignored"], json!([ADDONS_FILE]));
        let tried = manifest_preview(&d.staging, &d.layer, false);
        assert_eq!(tried["plugins"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect::<Vec<_>>(), ["b.js"]);
        crate::staging::apply(st, &d, false, p["signature"].as_str()).await.unwrap();
        assert!(off(), "still off after the change was kept");
        // Edited by hand in the live layer: taken out before anything loads.
        std::fs::write(d.layer.join(ADDONS_FILE), "{\"off\": []}\n").unwrap();
        crate::staging::guard(st, &d, false).await;
        assert!(off());
        assert!(!manifest_of(&d.layer)["plugins"].as_array().unwrap().iter().any(|x| x["name"] == "a.js"));
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    /// Taking a permission away edits exactly the declaration in effect (the one `resolve_decl`
    /// reads), and never reports success while the permission is still granted.
    #[tokio::test]
    async fn a_permission_is_taken_from_the_declaration_in_effect() {
        let (st, d) = testutil::layer("addon-decl").await;
        let w = |p: &str, t: &str| std::fs::write(d.layer.join(p), t).unwrap();
        let granted = |name: &str| manifest_of(&d.layer)["plugins"].as_array().unwrap().iter().find(|x| x["name"] == name).unwrap()["permissions"].clone();
        w("manifest.json", r#"{"apiVersion":1,"plugins":{"big.js":{"permissions":["todos:read"]},"odd.js":{"permissions":["todos:read"]},"linked.js":{"permissions":["todos:read"]},"shared.js":{"permissions":["todos:read","storage"]}}}"#);
        for n in ["big", "odd", "linked", "shared"] { w(&format!("plugins/{n}.js"), "export default () => {}\n"); }
        // Own files that are not in effect: too large, not an object, a link.
        let big = format!(r#"{{"apiVersion":1,"permissions":["storage"],"description":"{}"}}"#, "x".repeat(70 * 1024));
        w("plugins/big.json", &big);
        w("plugins/odd.json", r#"["todos:read"]"#);
        let outside = d.home.join("outside.json");
        std::fs::write(&outside, r#"{"apiVersion":1,"permissions":["storage"]}"#).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, d.layer.join("plugins/linked.json")).unwrap();
        st.commit_all(&d.layer, "Add four").await.unwrap();
        for n in ["big.js", "odd.js", "linked.js"] {
            if n == "linked.js" && !cfg!(unix) { continue; }
            assert!(matches!(decl_in_effect(&d.layer, "plugins", n, &serde_json::from_str(&std::fs::read_to_string(d.layer.join("manifest.json")).unwrap()).unwrap()), DeclIn::Manifest(_)), "{n}");
            assert_eq!(granted(n), json!(["todos:read"]), "{n}: the manifest entry is in effect");
            withdraw_in(st, &d.layer, &format!("plugins/{n}"), "todos:read").await.unwrap();
            assert_eq!(granted(n), json!([]), "{n}: taken away where it was granted");
        }
        assert_eq!(std::fs::read_to_string(d.layer.join("plugins/big.json")).unwrap(), big, "the file not in effect is left alone");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), r#"{"apiVersion":1,"permissions":["storage"]}"#, "nothing is written through a link");
        // A manifest entry shared with others: only this add-on's entry changes.
        withdraw_in(st, &d.layer, "plugins/shared.js", "storage").await.unwrap();
        assert_eq!(granted("shared.js"), json!(["todos:read"]));
        // A permission granted twice is gone entirely.
        w("plugins/twice.js", "export default () => {}\n");
        w("plugins/twice.json", r#"{"apiVersion":1,"permissions":["storage","todos:read","storage"]}"#);
        st.commit_all(&d.layer, "Add twice").await.unwrap();
        withdraw_in(st, &d.layer, "plugins/twice.js", "storage").await.unwrap();
        assert_eq!(granted("twice.js"), json!(["todos:read"]));
        // Nothing declared: refused, nothing recorded.
        w("plugins/bare.js", "export default () => {}\n");
        st.commit_all(&d.layer, "Add bare").await.unwrap();
        let before = history_in(st, &d.layer).await.len();
        assert!(withdraw_in(st, &d.layer, "plugins/bare.js", "storage").await.is_err());
        assert_eq!(history_in(st, &d.layer).await.len(), before);
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    /// UX-30: the app's own entries are named in the language of the moment.
    #[tokio::test]
    async fn the_apps_own_entries_speak_the_users_language() {
        let (st, d) = testutil::layer("lang").await;
        std::fs::write(d.layer.join("README.md"), "old").unwrap();
        st.commit_all(&d.layer, "old guide").await.unwrap();
        ensure_in(st, &d).await.unwrap();
        std::fs::write(d.layer.join("custom.css"), "body{}\n").unwrap();
        st.commit_all(&d.layer, "Make it calm").await.unwrap();
        let start = history_in(st, &d.layer).await.last().unwrap()["hash"].as_str().unwrap().to_string();
        rollback_in(st, &d.layer, &start).await.unwrap();
        let en = history_page_in(st, &d.layer, 0, 10, false).await;
        let ar = history_page_in(st, &d.layer, 0, 10, true).await;
        let short = &start[..7];
        assert!(en[0]["summary"].as_str().unwrap().starts_with("Went back to an earlier version (") && en[0]["entry"] == "back");
        assert!(ar[0]["summary"].as_str().unwrap().starts_with("العودة إلى إصدار سابق (") && ar[0]["summary"].as_str().unwrap().contains(short));
        assert_eq!((ar[1]["summary"].as_str(), ar[1]["entry"].as_str()), (Some("Make it calm"), Some("change")), "the user's own words stay");
        assert_eq!((ar[2]["summary"].as_str(), ar[2]["entry"].as_str()), (Some("تحديث دليل التخصيص"), Some("guide")));
        assert_eq!((ar.last().unwrap()["summary"].as_str(), ar.last().unwrap()["entry"].as_str()), (Some("نقطة البداية"), Some("start")));
        // The same, read back from a history whose record was lost.
        std::fs::remove_file(marks_file(&d.layer)).unwrap();
        assert_eq!(history_page_in(st, &d.layer, 0, 10, true).await, ar);
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
        w("plugins/reader.js", "export default () => {}\n");
        w("plugins/reader.json", r#"{"apiVersion":1,"permissions":["library:read"],"read":["clients/","../x"]}"#);
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
        // `read`: cleaned like `write`; none listed is the whole library.
        assert_eq!((p("reader.js")["read"].clone(), p("reader.js")["readsNothing"].clone()), (json!(["clients"]), json!(false)));
        assert_eq!((p("bare.js")["read"].clone(), p("bare.js")["readsNothing"].clone()), (json!([]), json!(false)));
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
