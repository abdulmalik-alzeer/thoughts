//! Staging: every customisation is prepared in a separate git worktree
//! (`~/.thoughts/app-layer-staging`, branch `staging`, recreated from the live layer for each
//! request). The live layer changes only when the user applies the pending change.

use crate::applayer::{Dirs, Git};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Meta {
    /// "customize" | "import" | "update"
    pub origin: String,
    pub chat_id: Option<String>,
    /// Files the run changed outside staging (library-relative, or `app-layer/<path>` for the live layer).
    pub outside_changes: Vec<String>,
    /// What the library and the live layer looked like when the run started.
    pub library_before: BTreeMap<String, String>,
    pub layer_before: BTreeMap<String, String>,
    pub created: String,
}

fn meta_file(d: &Dirs) -> PathBuf { d.home.join("layer-pending.json") }

pub fn load_meta(d: &Dirs) -> Option<Meta> {
    serde_json::from_str(&std::fs::read_to_string(meta_file(d)).ok()?).ok()
}

fn save_meta(d: &Dirs, m: &Meta) -> anyhow::Result<()> {
    crate::paths::write_atomic(&meta_file(d), serde_json::to_string_pretty(m)?.as_bytes(), true)?;
    Ok(())
}

fn staged(d: &Dirs) -> bool { d.staging.join(".git").exists() }

async fn chat_running(g: Git<'_>, m: &Meta) -> bool {
    match &m.chat_id { Some(id) => g.chat_running(id).await, None => false }
}

/// Throw the staging copy away (the live layer is not touched).
async fn remove(g: Git<'_>, d: &Dirs) {
    let path = d.staging.to_string_lossy().to_string();
    let _ = g.run(&d.layer, &["worktree", "remove", "--force", &path]).await;
    let _ = std::fs::remove_dir_all(&d.staging);
    let _ = g.run(&d.layer, &["worktree", "prune"]).await;
    let _ = std::fs::remove_file(meta_file(d));
}

/// Start a fresh staging copy from the live layer, replacing any earlier pending change.
pub async fn recreate(g: Git<'_>, d: &Dirs, origin: &str, chat_id: Option<&str>) -> anyhow::Result<()> {
    if let Some(m) = load_meta(d) {
        if chat_running(g, &m).await && m.chat_id.as_deref() != chat_id {
            anyhow::bail!("Another customisation is still being prepared. Wait for it to finish, or stop it first.");
        }
    }
    remove(g, d).await;
    let path = d.staging.to_string_lossy().to_string();
    g.run(&d.layer, &["worktree", "add", "-f", "-B", "staging", &path, "HEAD"]).await?;
    save_meta(d, &Meta { origin: origin.into(), chat_id: chat_id.map(String::from), created: chrono::Utc::now().to_rfc3339(), ..Default::default() })
}

/// An absolute path as a Claude Code permission pattern (`//path`, `//c/path` on Windows).
fn rule_path(p: &Path) -> String {
    let abs = p.to_string_lossy().replace('\\', "/");
    match abs.strip_prefix('/') {
        Some(rest) => format!("//{rest}"),
        None => format!("//{}", abs.replacen(':', "", 1)),
    }
}

/// What a customise run may not do: write to the library or the live layer, plant settings that
/// would run code, or read the daemon's secrets.
pub fn deny_rules(d: &Dirs, library: Option<&Path>) -> Vec<String> {
    let mut deny = vec![];
    for dir in library.into_iter().chain(std::iter::once(d.layer.as_path())) {
        let p = rule_path(dir);
        deny.push(format!("Edit({p})"));
        deny.push(format!("Edit({p}/**)"));
    }
    for rel in [".claude", ".mcp.json", ".git"] {
        deny.push(format!("Edit(./{rel})"));
        deny.push(format!("Edit(./{rel}/**)"));
    }
    // Permission rules have no "everything except": the staging copy lives inside the app's home,
    // so each credential store and private folder there is listed (see `paths::secret_paths_in`).
    let (files, dirs) = crate::paths::secret_paths_in(&d.home);
    for f in &files {
        let p = rule_path(f);
        for rule in [format!("Read({p})"), format!("Read({p}*)"), format!("Edit({p})"), format!("Edit({p}*)")] { deny.push(rule); }
        // Best-effort for the shell (Read/Edit rules do not cover `cat`).
        deny.push(format!("Bash(*{}*)", f.to_string_lossy().replace('\\', "/")));
    }
    for dir in &dirs {
        let p = rule_path(dir);
        for rule in [format!("Read({p})"), format!("Read({p}/**)"), format!("Edit({p})"), format!("Edit({p}/**)")] { deny.push(rule); }
        deny.push(format!("Bash(*{}*)", dir.to_string_lossy().replace('\\', "/")));
    }
    deny
}

/// Remove whatever Claude settings sit in the staging copy, so nothing in it can run code or
/// loosen permissions. The run's own deny rules (`deny_rules`) are handed to the run directly
/// (`RunSpec::extra_deny`), never written into the folder.
fn harden(d: &Dirs, _library: Option<&Path>) -> anyhow::Result<()> {
    let claude = d.staging.join(".claude");
    match std::fs::symlink_metadata(&claude) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(&claude)?,
        Ok(_) => std::fs::remove_file(&claude)?,
        Err(_) => {}
    }
    let _ = std::fs::remove_file(d.staging.join(".mcp.json"));
    Ok(())
}

/// Every uncommitted path in a git folder with a cheap content stamp, so a second edit to an
/// already-changed file still shows up. Empty when `dir` is not a git repo.
pub async fn snapshot(g: Git<'_>, dir: &Path) -> BTreeMap<String, String> {
    let out = g.run(dir, &["status", "--porcelain", "-uall", "-z"]).await.unwrap_or_default();
    let mut map = BTreeMap::new();
    let mut it = out.split('\0');
    while let Some(entry) = it.next() {
        if entry.len() < 4 { continue; }
        let (xy, path) = (&entry[..2], &entry[3..]);
        if xy.contains('R') || xy.contains('C') { it.next(); }
        let stamp = std::fs::metadata(dir.join(path)).ok().map(|m| {
            let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |x| x.as_nanos());
            format!("{}:{t}", m.len())
        }).unwrap_or_default();
        map.insert(path.to_string(), format!("{xy}:{stamp}"));
    }
    map
}

/// Paths whose state differs between two snapshots.
pub fn snapshot_diff(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> = after.iter().filter(|(k, v)| before.get(*k) != Some(v)).map(|(k, _)| k.clone()).collect();
    out.extend(before.keys().filter(|k| !after.contains_key(*k)).cloned());
    out.sort(); out.dedup();
    out
}

/// Get staging ready for a run of customise chat `chat_id` and return the folder the run works in.
/// A follow-up message in the chat that owns the pending change keeps building on it; anything
/// else starts from the live layer again.
pub async fn prepare_run(state: &AppState, chat_id: &str) -> anyhow::Result<PathBuf> {
    let d = Dirs::real();
    let g = Git(Some(state));
    crate::applayer::ensure_in(g, &d).await?;
    let reuse = staged(&d) && load_meta(&d).map_or(false, |m| m.chat_id.as_deref() == Some(chat_id));
    if !reuse { recreate(g, &d, "customize", Some(chat_id)).await?; }
    let library = state.settings.read().await.library();
    // The staging copy is the app's own folder (exempt from the trust check like the layer itself);
    // it carries no settings, and the caller passes `deny_rules` to the run.
    harden(&d, library.as_deref())?;
    let mut m = load_meta(&d).unwrap_or_default();
    m.library_before = match &library { Some(l) => snapshot(g, l).await, None => BTreeMap::new() };
    m.layer_before = snapshot(g, &d.layer).await;
    save_meta(&d, &m)?;
    Ok(d.staging)
}

/// After a customise run: record anything it changed outside staging. Nothing is applied here.
pub async fn after_run(state: &AppState) {
    let d = Dirs::real();
    let g = Git(Some(state));
    let Some(mut m) = load_meta(&d) else { return };
    let mut outside = vec![];
    if let Some(l) = state.settings.read().await.library() {
        outside.extend(snapshot_diff(&m.library_before, &snapshot(g, &l).await));
    }
    outside.extend(snapshot_diff(&m.layer_before, &snapshot(g, &d.layer).await).into_iter().map(|p| format!("app-layer/{p}")));
    if !outside.is_empty() {
        crate::log::warn(format!("layer: a customise run changed files outside staging: {}", outside.join(", ")));
    }
    m.outside_changes.extend(outside);
    m.outside_changes.sort(); m.outside_changes.dedup();
    let _ = save_meta(&d, &m);
    state.emit(json!({"type": "layer.pending"}));
}

/// The system prompt of every customise run.
pub fn system_prompt(d: &Dirs) -> String {
    format!(
        "You are customising the Thoughts desktop app for its user, who is probably not a developer.\n\
         You are working in a STAGING copy of the app's customisation layer: {staging}\n\
         1. Read README.md in that folder first. It is the contract: the layer layout, the Plugin API, the rules.\n\
         2. Write ONLY inside that folder. The user's library is readable for context but read-only for you; never write \
         anywhere else, never run git, never touch `.claude/` or the live layer at {layer}.\n\
         3. Any plugin you add or change must have its permissions declared in manifest.json — the smallest set that works.\n\
         4. Never weaken security, never read or ask for the app's access token, never load code from the internet.\n\
         5. When you are done, write SUMMARY.md in that folder: 2–5 plain sentences for a non-developer, in the user's language, \
         saying what will change and how to try it. No file names unless they help, no jargon. Its first line becomes the title of the change.\n\
         Nothing you write takes effect until the user previews it and presses Apply, so do not tell them it is already live. \
         The user cannot answer interactive tool prompts here — ask questions in plain text, and keep the change small.",
        staging = d.staging.display(), layer = d.layer.display()
    )
}

/// What staging changes relative to the live layer: `(path, status)` for customisation files, plus
/// changed paths that are not customisation files (those are never applied).
async fn changes(g: Git<'_>, d: &Dirs) -> anyhow::Result<(Vec<(String, &'static str)>, Vec<String>)> {
    g.run(&d.staging, &["add", "-A"]).await?;
    let head = g.run(&d.layer, &["rev-parse", "HEAD"]).await?.trim().to_string();
    let out = g.run(&d.staging, &["diff", "--cached", "--name-status", "--no-renames", "-z", &head]).await?;
    let (mut files, mut ignored) = (vec![], vec![]);
    let mut it = out.split('\0');
    while let (Some(st), Some(path)) = (it.next(), it.next()) {
        let status = match st.chars().next() { Some('A') => "added", Some('D') => "deleted", _ => "modified" };
        if crate::packs::check_path(path).is_ok() { files.push((path.to_string(), status)); } else { ignored.push(path.to_string()); }
    }
    Ok((files, ignored))
}

fn read_summary(d: &Dirs) -> String {
    let p = d.staging.join("SUMMARY.md");
    if !std::fs::symlink_metadata(&p).map_or(false, |m| m.is_file() && m.len() < 64 * 1024) { return String::new(); }
    std::fs::read_to_string(p).unwrap_or_default().trim().to_string()
}

/// `{exists:false}` or `{exists:true, summary, files:[{path,status}], pluginChanged, origin, …}`.
pub async fn pending(g: Git<'_>, d: &Dirs, arabic: bool) -> Value {
    let Some(m) = load_meta(d).filter(|_| staged(d)) else { return json!({"exists": false}) };
    let running = chat_running(g, &m).await;
    let Ok((files, ignored)) = changes(g, d).await else { return json!({"exists": false}) };
    if files.is_empty() { return json!({"exists": false, "running": running}); }
    let mut summary = read_summary(d);
    if summary.is_empty() {
        let names = files.iter().map(|(p, _)| p.as_str()).take(6).collect::<Vec<_>>().join(", ");
        summary = if arabic { format!("تغييرات على تخصيص التطبيق\n\nالملفات: {names}") } else { format!("Changes to your app customisation\n\nFiles: {names}") };
    }
    let plugin_changed = files.iter().any(|(p, _)| p.starts_with("plugins/") || p.starts_with("views/") || p == "manifest.json");
    // Workflows run on their own later, so the user is shown exactly what each one would be told to do.
    let workflow_changed = files.iter().any(|(p, _)| p.starts_with("workflows/"));
    let workflows: Vec<Value> = files.iter().filter(|(p, s)| p.starts_with("workflows/") && p.ends_with(".json") && *s != "deleted")
        .filter_map(|(p, _)| staged_workflow(d, p)).collect();
    json!({
        "workflowChanged": workflow_changed, "workflows": workflows,
        "exists": true, "summary": summary,
        "files": files.iter().map(|(p, s)| json!({"path": p, "status": s})).collect::<Vec<_>>(),
        "pluginChanged": plugin_changed, "origin": if m.origin.is_empty() { "customize" } else { &m.origin },
        "outsideChanges": m.outside_changes, "ignored": ignored, "chatId": m.chat_id, "running": running,
    })
}

/// `{path, name, trigger, prompt, valid}` for a staged workflow file, taken verbatim from the file.
fn staged_workflow(d: &Dirs, path: &str) -> Option<Value> {
    let p = d.staging.join(path);
    if !std::fs::symlink_metadata(&p).map_or(false, |m| m.is_file() && m.len() < 256 * 1024) { return None; }
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).ok()?).unwrap_or(Value::Null);
    let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let name = v["name"].as_str().filter(|n| !n.trim().is_empty()).map(String::from).unwrap_or(stem);
    let valid = serde_json::from_value::<crate::workflows::Workflow>(json!({"name": name, "trigger": v["trigger"], "prompt": v["prompt"]}))
        .map_or(false, |w| crate::workflows::valid(&w));
    Some(json!({"path": path, "name": name, "trigger": v["trigger"], "prompt": v["prompt"].as_str().unwrap_or_default(), "valid": valid}))
}

/// First line of the summary as a one-line title.
pub fn subject_of(summary: &str) -> String {
    let line = summary.lines().map(|l| l.trim().trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("Customisation");
    let mut s: String = line.chars().take(100).collect();
    if line.chars().count() > 100 { s.push('…'); }
    s
}

/// Bring the pending change into the live layer as one history entry, then clear staging.
pub async fn apply(g: Git<'_>, d: &Dirs, arabic: bool) -> anyhow::Result<Value> {
    let m = load_meta(d).filter(|_| staged(d)).ok_or_else(|| anyhow::anyhow!("there is no pending change"))?;
    if chat_running(g, &m).await { anyhow::bail!("The assistant is still working on this change. Wait for it to finish, or stop it first."); }
    let p = pending(g, d, arabic).await;
    if p["exists"] != true { anyhow::bail!("there is no pending change"); }
    let (files, _) = changes(g, d).await?;
    // Read everything first, so a bad file stops the apply before the live layer is touched.
    let mut writes: Vec<(String, Option<Vec<u8>>)> = vec![];
    let mut total = 0usize;
    for (path, status) in &files {
        if *status == "deleted" { writes.push((path.clone(), None)); continue; }
        let src = d.staging.join(path);
        let meta = std::fs::symlink_metadata(&src)?;
        if !meta.is_file() { anyhow::bail!("{path} is not a regular file"); }
        if meta.len() as usize > crate::packs::MAX_FILE_BYTES { anyhow::bail!("{path} is too large (limit 512 KB)"); }
        total += meta.len() as usize;
        if total > crate::packs::MAX_TOTAL_BYTES { anyhow::bail!("the change is too large (limit 4 MB)"); }
        writes.push((path.clone(), Some(std::fs::read(&src)?)));
    }
    if writes.len() > crate::packs::MAX_FILES { anyhow::bail!("the change touches too many files"); }
    // Hand edits made straight in the live layer get their own entry first, so this one stays clean.
    g.commit_all(&d.layer, "Snapshot of manual changes").await?;
    for (path, bytes) in &writes {
        let target = d.layer.join(path);
        match bytes {
            None => { let _ = std::fs::remove_file(&target); }
            Some(b) => {
                if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
                if std::fs::symlink_metadata(&target).map_or(false, |m| !m.is_file()) { anyhow::bail!("{path} is not a regular file in the app layer"); }
                std::fs::write(&target, b)?;
            }
        }
    }
    let summary = p["summary"].as_str().unwrap_or_default();
    let subject = subject_of(summary);
    let msg = if summary.lines().count() > 1 { format!("{subject}\n\n{summary}") } else { subject.clone() };
    g.commit_all(&d.layer, &msg).await?;
    let hash = g.run(&d.layer, &["rev-parse", "HEAD"]).await?.trim().to_string();
    remove(g, d).await;
    Ok(json!({"ok": true, "hash": hash, "summary": subject, "pluginChanged": p["pluginChanged"]}))
}

pub async fn discard(g: Git<'_>, d: &Dirs) -> anyhow::Result<()> {
    if let Some(m) = load_meta(d) {
        if chat_running(g, &m).await { anyhow::bail!("The assistant is still working on this change. Stop it first."); }
    }
    remove(g, d).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applayer::{history_in, testutil};

    #[tokio::test]
    async fn stage_preview_apply() {
        let (st, d) = testutil::layer("apply").await;
        assert_eq!(pending(st, &d, false).await, json!({"exists": false}));
        recreate(st, &d, "customize", Some("chat-1")).await.unwrap();
        assert_eq!(pending(st, &d, false).await["exists"], false, "nothing changed yet");
        harden(&d, Some(Path::new("/home/u/Library"))).unwrap();
        // The "AI" works in staging.
        std::fs::write(d.staging.join("custom.css"), ":root { --radius: 2px }\n").unwrap();
        std::fs::write(d.staging.join("plugins/due.js"), "export default (t) => {}\n").unwrap();
        std::fs::write(d.staging.join("manifest.json"), r#"{"apiVersion":1,"plugins":{"due.js":{"permissions":["todos:read"]}}}"#).unwrap();
        std::fs::remove_file(d.staging.join("themes.json")).unwrap();
        std::fs::write(d.staging.join("README.md"), "tampered").unwrap();
        std::fs::write(d.staging.join("notes.txt"), "stray").unwrap();
        std::fs::write(d.staging.join("SUMMARY.md"), "# Show what's due on Home\n\nA small box on Home lists overdue todos.\n").unwrap();

        let p = pending(st, &d, false).await;
        assert_eq!(p["exists"], true);
        assert_eq!(p["origin"], "customize");
        assert_eq!(p["pluginChanged"], true);
        assert_eq!(p["workflowChanged"], false);
        assert_eq!(p["workflows"], json!([]));
        assert!(p["summary"].as_str().unwrap().starts_with("# Show what's due"));
        let files: Vec<(String, String)> = p["files"].as_array().unwrap().iter()
            .map(|f| (f["path"].as_str().unwrap().to_string(), f["status"].as_str().unwrap().to_string())).collect();
        assert_eq!(files, [("custom.css".into(), "modified".into()), ("manifest.json".into(), "modified".into()),
                           ("plugins/due.js".into(), "added".into()), ("themes.json".into(), "deleted".into())]);
        let ignored = p["ignored"].as_array().unwrap();
        assert!(ignored.contains(&json!("README.md")) && ignored.contains(&json!("notes.txt")));
        assert!(!ignored.iter().any(|x| x.as_str().unwrap().contains(".claude")), "the app's own settings are never part of a change");

        // Preview reads staging; the live layer is untouched until apply.
        assert!(crate::applayer::manifest_of(&d.staging)["css"].as_str().unwrap().contains("--radius"));
        assert!(!crate::applayer::manifest_of(&d.layer)["css"].as_str().unwrap().contains("--radius"));
        let before = history_in(st, &d.layer).await.len();

        let r = apply(st, &d, false).await.unwrap();
        assert_eq!(r["summary"], "Show what's due on Home");
        let h = history_in(st, &d.layer).await;
        assert_eq!(h.len(), before + 1, "one history entry");
        assert_eq!(h[0]["summary"], "Show what's due on Home");
        assert!(d.layer.join("plugins/due.js").exists() && !d.layer.join("themes.json").exists());
        assert_eq!(std::fs::read_to_string(d.layer.join("README.md")).unwrap(), crate::applayer::CONTRACT, "the guide cannot be changed by a run");
        assert!(!d.layer.join("notes.txt").exists() && !d.layer.join("SUMMARY.md").exists() && !d.layer.join(".claude").exists());
        assert!(!d.staging.exists());
        assert_eq!(pending(st, &d, false).await["exists"], false);
        assert!(apply(st, &d, false).await.is_err());
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn discard_leaves_the_layer_alone_and_restaging_starts_clean() {
        let (st, d) = testutil::layer("discard").await;
        recreate(st, &d, "customize", None).await.unwrap();
        std::fs::write(d.staging.join("custom.css"), "x{}\n").unwrap();
        assert_eq!(pending(st, &d, true).await["exists"], true);
        assert!(pending(st, &d, true).await["summary"].as_str().unwrap().contains("custom.css"), "fallback summary");
        // A new request replaces the old pending change.
        recreate(st, &d, "update", None).await.unwrap();
        assert_eq!(pending(st, &d, false).await["exists"], false);
        std::fs::write(d.staging.join("views/a.html"), "<p>a</p>").unwrap();
        assert_eq!(pending(st, &d, false).await["origin"], "update");
        let before = history_in(st, &d.layer).await.len();
        discard(st, &d).await.unwrap();
        assert!(!d.staging.exists() && load_meta(&d).is_none());
        assert_eq!(history_in(st, &d.layer).await.len(), before);
        assert!(!d.layer.join("views/a.html").exists());
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn notices_changes_outside_staging() {
        let (st, d) = testutil::layer("outside").await;
        let before = snapshot(st, &d.layer).await;
        assert!(before.is_empty());
        std::fs::write(d.layer.join("custom.css"), "sneaky{}\n").unwrap();
        std::fs::write(d.layer.join("plugins/new.js"), "").unwrap();
        let after = snapshot(st, &d.layer).await;
        assert_eq!(snapshot_diff(&before, &after), ["custom.css", "plugins/new.js"]);
        assert!(snapshot_diff(&after, &after).is_empty());
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn staged_workflows_are_shown_verbatim() {
        let (st, d) = testutil::layer("workflows").await;
        recreate(st, &d, "customize", Some("chat-1")).await.unwrap();
        std::fs::create_dir_all(d.staging.join("workflows")).unwrap();
        std::fs::write(d.staging.join("workflows/weekly.json"),
            r#"{"name":"Weekly review","trigger":{"type":"schedule","every":"week","at":"16:00","weekday":5},"prompt":"Review my week and list what slipped."}"#).unwrap();
        std::fs::write(d.staging.join("workflows/broken.json"), "{not json").unwrap();
        let p = pending(st, &d, false).await;
        assert_eq!(p["exists"], true);
        assert_eq!(p["workflowChanged"], true);
        assert_eq!(p["pluginChanged"], false);
        let ws = p["workflows"].as_array().unwrap();
        assert_eq!(ws.len(), 2);
        let w = ws.iter().find(|w| w["path"] == "workflows/weekly.json").unwrap();
        assert_eq!(w["name"], "Weekly review");
        assert_eq!(w["prompt"], "Review my week and list what slipped.");
        assert_eq!(w["trigger"]["weekday"], 5);
        assert_eq!(w["valid"], true);
        let b = ws.iter().find(|w| w["path"] == "workflows/broken.json").unwrap();
        assert_eq!((&b["name"], &b["valid"], &b["prompt"]), (&json!("broken"), &json!(false), &json!("")));
    }

    #[test]
    fn deny_rules_protect_library_layer_and_secrets() {
        let d = Dirs::at(Path::new("/home/u/.thoughts"));
        let rules = deny_rules(&d, Some(Path::new("/home/u/Library")));
        for want in ["Edit(//home/u/Library/**)", "Edit(//home/u/.thoughts/app-layer/**)", "Edit(./.claude/**)", "Read(//home/u/.thoughts/daemon.json)",
                     "Read(//home/u/.thoughts/secrets.json)", "Read(//home/u/.thoughts/git-credentials)", "Read(//home/u/.thoughts/trusted.json)",
                     "Read(//home/u/.thoughts/settings.json*)", "Read(//home/u/.thoughts/chats/**)", "Bash(*/home/u/.thoughts/secrets.json*)"] {
            assert!(rules.iter().any(|r| r == want), "missing {want}");
        }
        assert!(!rules.iter().any(|r| r.contains("app-layer-staging")));
        assert_eq!(subject_of("\n## تغيير الألوان \nالتفاصيل"), "تغيير الألوان");
        assert!(system_prompt(&d).contains("SUMMARY.md"));
    }
}
