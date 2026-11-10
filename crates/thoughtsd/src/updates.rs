//! App updates of the customisation layer. The default layer files are embedded in the binary.
//! The daemon remembers the defaults it last gave this install (`~/.thoughts/base/<version>/…`
//! plus `base-manifest.json`), so when a new app version ships different defaults it can tell,
//! per file, who changed what:
//!   - the user left it alone        → take the new default silently
//!   - only the user changed it      → keep the user's file
//!   - both changed it               → a conflict the user resolves (keep / new / AI merge)
//! The user's library is never touched by updates.

use crate::applayer::{Dirs, Git};
use crate::state::AppState;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn sha(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Class {
    /// The user's file already equals the new default.
    Same,
    /// The default did not change (or only the user changed the file): keep the user's file.
    Keep,
    /// The user never changed it: take the new default.
    Take,
    /// Both changed it.
    Conflict,
}

/// Three-way classification of one shipped file by content hash (`None` = the file does not exist).
pub fn classify(base: Option<&str>, current: Option<&str>, new: &str) -> Class {
    if current == Some(new) { Class::Same }
    else if base == Some(new) { Class::Keep }
    else if current == base { Class::Take }
    else { Class::Conflict }
}

fn manifest_file(home: &Path) -> PathBuf { home.join("base-manifest.json") }
fn status_file(home: &Path) -> PathBuf { home.join("update-status.json") }
fn base_dir(home: &Path, version: &str) -> PathBuf {
    let safe: String = version.chars().filter(|c| c.is_ascii_alphanumeric() || ".-+_".contains(*c)).collect();
    home.join("base").join(if safe.is_empty() { "unknown".into() } else { safe })
}

fn store_base(home: &Path, version: &str, shipped: &[(&str, &str)]) -> anyhow::Result<()> {
    let dir = base_dir(home, version);
    let mut files = BTreeMap::new();
    for (path, text) in shipped {
        let target = dir.join(path);
        if let Some(p) = target.parent() { std::fs::create_dir_all(p)?; }
        std::fs::write(&target, text)?;
        files.insert(path.to_string(), sha(text.as_bytes()));
    }
    crate::paths::write_atomic(&manifest_file(home), serde_json::to_string_pretty(&json!({"version": version, "files": files}))?.as_bytes(), true)?;
    Ok(())
}

fn write_status(home: &Path, v: &Value) -> anyhow::Result<()> {
    crate::paths::write_atomic(&status_file(home), serde_json::to_string_pretty(v)?.as_bytes(), true)?;
    Ok(())
}

/// `{from, to, applied:[path], conflicts:[{path}]}` — the outcome of the last app update.
pub fn status(home: &Path) -> Value {
    std::fs::read_to_string(status_file(home)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v["conflicts"].is_array())
        .unwrap_or_else(|| json!({"from": env!("CARGO_PKG_VERSION"), "to": env!("CARGO_PKG_VERSION"), "applied": [], "conflicts": []}))
}

/// Run on every daemon start.
pub async fn check(state: &AppState) -> anyhow::Result<()> {
    let changed = check_with(Git(Some(state)), &Dirs::real(), env!("CARGO_PKG_VERSION"), crate::applayer::SHIPPED).await?;
    if changed { state.emit(json!({"type": "layer.changed"})); }
    Ok(())
}

/// Bring the layer's default files up to `version`. Returns whether the live layer changed.
pub async fn check_with(g: Git<'_>, d: &Dirs, version: &str, shipped: &[(&str, &str)]) -> anyhow::Result<bool> {
    let base: Option<Value> = std::fs::read_to_string(manifest_file(&d.home)).ok().and_then(|t| serde_json::from_str(&t).ok());
    let from = base.as_ref().and_then(|b| b["version"].as_str()).map(String::from);
    if from.as_deref() == Some(version) { return Ok(false); }
    let (mut applied, mut conflicts): (Vec<String>, Vec<Value>) = (vec![], vec![]);
    for (path, new) in shipped {
        let target = d.layer.join(path);
        let current = std::fs::read(&target).ok().map(|b| sha(&b));
        let take = match &base {
            // No record of earlier defaults (first run of this mechanism): only fill in what is missing.
            None => current.is_none(),
            Some(b) => match classify(b["files"][*path].as_str(), current.as_deref(), &sha(new.as_bytes())) {
                Class::Take => true,
                Class::Conflict => { conflicts.push(json!({"path": path})); false }
                Class::Same | Class::Keep => false,
            },
        };
        if take {
            if let Some(p) = target.parent() { std::fs::create_dir_all(p)?; }
            std::fs::write(&target, new)?;
            applied.push(path.to_string());
        }
    }
    if !applied.is_empty() {
        let files: Vec<&str> = applied.iter().map(String::as_str).collect();
        let mut add = vec!["add", "--"]; add.extend(&files);
        g.run(&d.layer, &add).await?;
        if g.run(&d.layer, &["diff", "--cached", "--quiet"]).await.is_err() {
            g.commit_staged(&d.layer, &format!("Update built-in defaults to version {version}"), &[]).await?;
        }
    }
    store_base(&d.home, version, shipped)?;
    if let Some(from) = &from {
        write_status(&d.home, &json!({"from": from, "to": version, "applied": applied, "conflicts": conflicts}))?;
    }
    Ok(!applied.is_empty())
}

fn shipped_text(path: &str, shipped: &[(&str, &str)]) -> Option<String> {
    shipped.iter().find(|(p, _)| *p == path).map(|(_, t)| t.to_string())
}

/// The status with the conflict on `path` removed; fails when there is no such conflict.
fn take_conflict(home: &Path, path: &str) -> anyhow::Result<Value> {
    let mut st = status(home);
    let list = st["conflicts"].as_array().cloned().unwrap_or_default();
    if !list.iter().any(|c| c["path"] == path) { anyhow::bail!("there is no update conflict for {path}"); }
    st["conflicts"] = json!(list.into_iter().filter(|c| c["path"] != path).collect::<Vec<_>>());
    Ok(st)
}

/// Resolve a conflict by keeping the user's file (`keep`) or taking the new default (`new`).
pub async fn resolve(g: Git<'_>, d: &Dirs, shipped: &[(&str, &str)], path: &str, choice: &str) -> anyhow::Result<()> {
    let st = take_conflict(&d.home, path)?;
    match choice {
        "keep" => {}
        "new" => {
            let text = shipped_text(path, shipped).ok_or_else(|| anyhow::anyhow!("{path} is not a built-in file"))?;
            g.commit_all(&d.layer, "Snapshot of manual changes").await?;
            std::fs::write(d.layer.join(path), text)?;
            g.commit_all(&d.layer, &format!("Use the new built-in version of {path}")).await?;
        }
        _ => anyhow::bail!("choice must be keep, new or merge"),
    }
    write_status(&d.home, &st)
}

/// Forget the conflict on `path`: once an AI merge has started it lives on as a pending change
/// (origin `update`) that the user applies or discards.
pub fn clear_conflict(home: &Path, path: &str) -> anyhow::Result<()> {
    write_status(home, &take_conflict(home, path)?)
}

/// The customise prompt for an AI merge of one conflicting file.
pub fn merge_prompt(d: &Dirs, shipped: &[(&str, &str)], path: &str) -> anyhow::Result<String> {
    let st = take_conflict(&d.home, path)?;
    let new = shipped_text(path, shipped).ok_or_else(|| anyhow::anyhow!("{path} is not a built-in file"))?;
    let from = st["from"].as_str().unwrap_or_default().to_string();
    let old = std::fs::read_to_string(base_dir(&d.home, &from).join(path)).unwrap_or_default();
    Ok(format!(
        "A new version of the app ships a new built-in `{path}`, and I have also changed my own copy. Merge the two into `{path}` \
         in the staging folder: keep everything I customised, and bring in what the new built-in version adds or fixes. \
         If both changed the same thing, keep mine. In the summary, say in plain words what you kept and what you brought in.\n\n\
         The built-in version my copy started from (app {from}):\n````\n{old}\n````\n\n\
         The new built-in version (app {to}):\n````\n{new}\n````\n\n\
         My current copy is the `{path}` already in the staging folder.",
        to = st["to"].as_str().unwrap_or_default()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applayer::testutil;

    #[test]
    fn three_way_classifier() {
        let (a, b, c) = ("a", "b", "c");
        assert_eq!(classify(Some(a), Some(a), b), Class::Take, "user unchanged, upstream changed");
        assert_eq!(classify(Some(a), Some(c), a), Class::Keep, "user changed, upstream unchanged");
        assert_eq!(classify(Some(a), Some(c), b), Class::Conflict, "both changed");
        assert_eq!(classify(Some(a), Some(b), b), Class::Same, "both made the same change");
        assert_eq!(classify(None, None, b), Class::Take, "a new built-in file");
        assert_eq!(classify(None, Some(c), b), Class::Conflict, "user already has a different file by that name");
        assert_eq!(classify(Some(a), None, b), Class::Conflict, "user deleted it, upstream changed it");
        assert_eq!(classify(Some(a), None, a), Class::Keep, "user deleted it, upstream unchanged");
    }

    #[tokio::test]
    async fn update_takes_keeps_and_reports_conflicts() {
        let (st, d) = testutil::layer("updates").await;
        let v1: &[(&str, &str)] = &[("custom.css", "/* v1 */\n"), ("themes.json", "[]\n"), ("home.json", "{}\n")];
        for (p, t) in v1 { std::fs::write(d.layer.join(p), t).unwrap(); }
        std::fs::remove_file(d.layer.join("manifest.json")).unwrap();
        st.commit_all(&d.layer, "v1 defaults").await.unwrap();

        // First run: record the base, fill in only what is missing, no status to report.
        let first: &[(&str, &str)] = &[v1[0], v1[1], v1[2], ("manifest.json", "{\"apiVersion\":1}\n")];
        assert!(check_with(st, &d, "1.0.0", first).await.unwrap());
        assert!(d.layer.join("manifest.json").exists());
        assert_eq!(status(&d.home)["conflicts"], json!([]));
        assert!(!check_with(st, &d, "1.0.0", first).await.unwrap(), "same version: nothing to do");

        // The user customises two files; the app then ships new defaults for three.
        std::fs::write(d.layer.join("custom.css"), "/* mine */\n").unwrap();
        std::fs::write(d.layer.join("themes.json"), "[{\"id\":\"x\",\"name\":\"X\"}]\n").unwrap();
        st.commit_all(&d.layer, "my changes").await.unwrap();
        let v2: &[(&str, &str)] = &[("custom.css", "/* v2 */\n"), ("themes.json", "[]\n"), ("home.json", "{\"chips\":[]}\n"), ("manifest.json", "{\"apiVersion\":1}\n")];
        assert!(check_with(st, &d, "2.0.0", v2).await.unwrap());
        let s = status(&d.home);
        assert_eq!((s["from"].as_str(), s["to"].as_str()), (Some("1.0.0"), Some("2.0.0")));
        assert_eq!(s["applied"], json!(["home.json"]), "untouched by the user: new default taken");
        assert_eq!(s["conflicts"], json!([{"path": "custom.css"}]), "changed by both");
        assert_eq!(std::fs::read_to_string(d.layer.join("themes.json")).unwrap(), "[{\"id\":\"x\",\"name\":\"X\"}]\n", "changed only by the user: kept");
        assert_eq!(std::fs::read_to_string(d.layer.join("custom.css")).unwrap(), "/* mine */\n");
        assert_eq!(std::fs::read_to_string(d.layer.join("home.json")).unwrap(), "{\"chips\":[]}\n");
        assert_eq!(crate::applayer::history_in(st, &d.layer).await[0]["summary"], "Update built-in defaults to version 2.0.0");
        assert_eq!(std::fs::read_to_string(d.home.join("base/1.0.0/custom.css")).unwrap(), "/* v1 */\n");

        // Merge prompt carries both built-in versions; resolving clears the conflict.
        let prompt = merge_prompt(&d, v2, "custom.css").unwrap();
        assert!(prompt.contains("/* v1 */") && prompt.contains("/* v2 */"));
        assert_eq!(status(&d.home)["conflicts"], json!([{"path": "custom.css"}]), "still listed until the merge run has started");
        clear_conflict(&d.home, "custom.css").unwrap();
        assert_eq!(status(&d.home)["conflicts"], json!([]));
        assert!(resolve(st, &d, v2, "custom.css", "new").await.is_err(), "already resolved");
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn resolve_new_replaces_and_keep_leaves() {
        let (st, d) = testutil::layer("resolve").await;
        let v2: &[(&str, &str)] = &[("custom.css", "/* v2 */\n"), ("themes.json", "[1]\n")];
        std::fs::write(d.layer.join("custom.css"), "/* mine */\n").unwrap();
        write_status(&d.home, &json!({"from": "1", "to": "2", "applied": [], "conflicts": [{"path": "custom.css"}, {"path": "themes.json"}]})).unwrap();
        assert!(resolve(st, &d, v2, "custom.css", "wipe").await.is_err());
        assert!(resolve(st, &d, v2, "../x", "new").await.is_err());
        resolve(st, &d, v2, "custom.css", "new").await.unwrap();
        assert_eq!(std::fs::read_to_string(d.layer.join("custom.css")).unwrap(), "/* v2 */\n");
        resolve(st, &d, v2, "themes.json", "keep").await.unwrap();
        assert_eq!(std::fs::read_to_string(d.layer.join("themes.json")).unwrap(), "[]\n");
        assert_eq!(status(&d.home)["conflicts"], json!([]));
        std::fs::remove_dir_all(&d.home).unwrap();
    }
}
