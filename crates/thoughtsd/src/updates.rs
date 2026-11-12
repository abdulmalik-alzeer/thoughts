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

/// `{from, to, applied:[path], conflicts:[{path, from}], kept:[{path, from}], removed:[path],
/// downgrade}` — the outcome of the last app update. `conflicts` stay listed until the user decides
/// about each one. `kept` are the built-in files whose new version the user turned down ("Keep
/// mine"): the choice can be revisited (use the new version, or merge) for as long as it applies.
pub fn status(home: &Path) -> Value {
    let mut v = std::fs::read_to_string(status_file(home)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v["conflicts"].is_array())
        .unwrap_or_else(|| json!({"from": env!("CARGO_PKG_VERSION"), "to": env!("CARGO_PKG_VERSION"), "applied": [], "conflicts": []}));
    if !v["removed"].is_array() { v["removed"] = json!([]); }
    if !v["kept"].is_array() { v["kept"] = json!([]); }
    if !v["downgrade"].is_boolean() { v["downgrade"] = json!(false); }
    v
}

/// `status`, with `merging: true` on each conflict (or kept file) in `merging`: the user chose "let
/// the assistant merge" for that file, and the merged version is being prepared or is waiting for
/// review. The question itself stays on record until that version is kept.
pub fn status_merging(home: &Path, merging: &[String]) -> Value {
    let mut v = status(home);
    for key in ["conflicts", "kept"] {
        if let Some(list) = v[key].as_array_mut() {
            for c in list.iter_mut().filter(|c| merging.iter().any(|m| c["path"] == m.as_str())) { c["merging"] = json!(true); }
        }
    }
    v
}

/// Each open question and each kept file of `status`, with what the new built-in version changes
/// compared with the one the user's copy started from (`changes`, in the review's terms, see
/// `staging::effects`; `null` when that cannot be said for this file).
pub fn explained(mut v: Value, home: &Path, shipped: &[(&str, &str)]) -> Value {
    let to = v["to"].as_str().unwrap_or_default().to_string();
    for key in ["conflicts", "kept"] {
        let Some(list) = v[key].as_array_mut() else { continue };
        for c in list.iter_mut() {
            let path = c["path"].as_str().unwrap_or_default().to_string();
            let from = c["from"].as_str().unwrap_or_default().to_string();
            let old = std::fs::read_to_string(base_dir(home, &from).join(&path)).ok();
            let changes = shipped_text(&path, shipped)
                .and_then(|new| crate::staging::effects::file_effects(&path, old.as_deref(), Some(&new)));
            c["changes"] = json!(changes);
            if c["to"].is_null() { c["to"] = json!(to); }
        }
    }
    v
}

/// Compare two version strings by their numeric parts (`0.10.0` is newer than `0.9.3`).
pub fn older(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(|c: char| c == '-' || c == '+').next().unwrap_or("").split('.').map(|p| p.trim().parse().unwrap_or(0)).collect()
    };
    let (mut x, mut y) = (parts(a), parts(b));
    let n = x.len().max(y.len());
    x.resize(n, 0); y.resize(n, 0);
    x < y
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
    if from.as_deref() == Some(version) {
        // Back on the version the defaults belong to: an earlier "older app" note no longer applies.
        if status_file(&d.home).exists() {
            let mut st = status(&d.home);
            if st["downgrade"] == true {
                st["downgrade"] = json!(false);
                st["from"] = json!(version);
                st["to"] = json!(version);
                write_status(&d.home, &st)?;
            }
        }
        return Ok(false);
    }
    // What the user has not decided yet survives every later update.
    let had_status = status_file(&d.home).exists();
    let earlier = status(&d.home);
    let open: Vec<Value> = if had_status { earlier["conflicts"].as_array().cloned().unwrap_or_default() } else { vec![] };
    let kept: Vec<Value> = if had_status { earlier["kept"].as_array().cloned().unwrap_or_default() } else { vec![] };
    if let Some(f) = from.as_deref().filter(|f| older(version, f)) {
        // An older app was started on a newer install: touch nothing, remember nothing, just say so.
        let st = json!({"from": f, "to": version, "applied": [], "conflicts": open, "kept": kept, "removed": [], "downgrade": true});
        if !had_status || earlier != st { write_status(&d.home, &st)?; }
        return Ok(false);
    }
    let (mut applied, mut conflicts): (Vec<String>, Vec<Value>) = (vec![], vec![]);
    for (path, new) in shipped {
        let target = d.layer.join(path);
        let current = std::fs::read(&target).ok().map(|b| sha(&b));
        let new_sha = sha(new.as_bytes());
        if let Some(c) = open.iter().find(|c| c["path"] == *path) {
            // Still undecided from an earlier update: the user's file is never replaced under them.
            // It stays a conflict (against the base it started from) unless it now equals the new default.
            if current.as_deref() != Some(new_sha.as_str()) {
                let since = c["from"].as_str().or(earlier["from"].as_str()).unwrap_or_default();
                conflicts.push(json!({"path": path, "from": since}));
            }
            continue;
        }
        let take = match &base {
            // No record of earlier defaults (first run of this mechanism): only fill in what is missing.
            None => current.is_none(),
            Some(b) => match classify(b["files"][*path].as_str(), current.as_deref(), &new_sha) {
                Class::Take => true,
                Class::Conflict => { conflicts.push(json!({"path": path, "from": from.clone().unwrap_or_default()})); false }
                Class::Same | Class::Keep => false,
            },
        };
        if take {
            if let Some(p) = target.parent() { std::fs::create_dir_all(p)?; }
            std::fs::write(&target, new)?;
            applied.push(path.to_string());
        }
    }
    // Defaults the new version no longer ships (removed or renamed): the user's copies stay where they are.
    let removed: Vec<String> = base.as_ref().and_then(|b| b["files"].as_object()).into_iter().flatten().map(|(p, _)| p.clone())
        .filter(|p| !shipped.iter().any(|(s, _)| s == p) && d.layer.join(p).exists()).collect();
    if !applied.is_empty() {
        let files: Vec<&str> = applied.iter().map(String::as_str).collect();
        let mut add = vec!["add", "--"]; add.extend(&files);
        g.run(&d.layer, &add).await?;
        if g.run(&d.layer, &["diff", "--cached", "--quiet"]).await.is_err() {
            g.commit_staged(&d.layer, &format!("Update built-in defaults to version {version}"), &[]).await?;
            // The app's own entry: History never offers to undo it.
            crate::applayer::mark_system_head(g, &d.layer).await;
        }
    }
    // A new version the user turned down earlier stays on offer while it still applies: the file is
    // still shipped, still differs from what this version ships, and is not an open question again.
    let kept: Vec<Value> = kept.into_iter().filter(|k| {
        let path = k["path"].as_str().unwrap_or_default();
        let Some((_, new)) = shipped.iter().find(|(p, _)| *p == path) else { return false };
        !conflicts.iter().any(|c| c["path"] == path) && std::fs::read(d.layer.join(path)).ok().map(|b| sha(&b)) != Some(sha(new.as_bytes()))
    }).collect();
    store_base(&d.home, version, shipped)?;
    if let Some(from) = &from {
        write_status(&d.home, &json!({"from": from, "to": version, "applied": applied, "conflicts": conflicts, "kept": kept, "removed": removed, "downgrade": false}))?;
    }
    Ok(!applied.is_empty())
}

fn shipped_text(path: &str, shipped: &[(&str, &str)]) -> Option<String> {
    shipped.iter().find(|(p, _)| *p == path).map(|(_, t)| t.to_string())
}

/// The status with the question about `path` removed (an open conflict, or a new version the user
/// kept their own file over), and that entry; fails when there is no such question.
fn take_conflict(home: &Path, path: &str) -> anyhow::Result<(Value, Value)> {
    let mut st = status(home);
    let mut found = None;
    for key in ["conflicts", "kept"] {
        let list = st[key].as_array().cloned().unwrap_or_default();
        if let Some(c) = list.iter().find(|c| c["path"] == path) { found.get_or_insert(c.clone()); }
        st[key] = json!(list.into_iter().filter(|c| c["path"] != path).collect::<Vec<_>>());
    }
    let entry = found.ok_or_else(|| anyhow::anyhow!("there is no update conflict for {path}"))?;
    Ok((st, entry))
}

/// Settle the question about `path` by keeping the user's file (`keep`, which can be revisited
/// later) or taking the new default (`new`).
pub async fn resolve(g: Git<'_>, d: &Dirs, shipped: &[(&str, &str)], path: &str, choice: &str) -> anyhow::Result<()> {
    let (mut st, entry) = take_conflict(&d.home, path)?;
    match choice {
        "keep" => {
            if shipped_text(path, shipped).is_none() { anyhow::bail!("{path} is not a built-in file"); }
            let mut kept = st["kept"].as_array().cloned().unwrap_or_default();
            kept.push(json!({"path": path, "from": entry["from"].as_str().or(st["from"].as_str()).unwrap_or_default(), "to": st["to"]}));
            st["kept"] = json!(kept);
        }
        "new" => {
            let text = shipped_text(path, shipped).ok_or_else(|| anyhow::anyhow!("{path} is not a built-in file"))?;
            // Anything in the live layer that was never reviewed is taken out first, not recorded along.
            crate::staging::set_aside(g, d).await?;
            std::fs::write(d.layer.join(path), text)?;
            g.run(&d.layer, &["add", "--", path]).await?;
            if g.run(&d.layer, &["diff", "--cached", "--quiet"]).await.is_err() {
                g.commit_staged(&d.layer, &format!("Use the new built-in version of {path}"), &[]).await?;
            }
        }
        _ => anyhow::bail!("choice must be keep, new or merge"),
    }
    write_status(&d.home, &st)
}

/// Forget the question about `path`: the merged version the assistant prepared has been kept.
/// (Until then it stays listed, so a merge that fails, is stopped, is discarded or is replaced by
/// another request leaves the question open and "use the new version" still possible.)
pub fn clear_conflict(home: &Path, path: &str) -> anyhow::Result<()> {
    write_status(home, &take_conflict(home, path)?.0)
}

/// The customise prompt for an AI merge of one conflicting (or kept) file.
pub fn merge_prompt(d: &Dirs, shipped: &[(&str, &str)], path: &str) -> anyhow::Result<String> {
    let (st, entry) = take_conflict(&d.home, path)?;
    let new = shipped_text(path, shipped).ok_or_else(|| anyhow::anyhow!("{path} is not a built-in file"))?;
    // A conflict carried over from an earlier update is merged against the base it started from.
    let from = entry["from"].as_str().or(st["from"].as_str()).unwrap_or_default().to_string();
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
        assert_eq!(s["conflicts"], json!([{"path": "custom.css", "from": "1.0.0"}]), "changed by both");
        assert_eq!((&s["removed"], &s["downgrade"]), (&json!([]), &json!(false)));
        assert_eq!(std::fs::read_to_string(d.layer.join("themes.json")).unwrap(), "[{\"id\":\"x\",\"name\":\"X\"}]\n", "changed only by the user: kept");
        assert_eq!(std::fs::read_to_string(d.layer.join("custom.css")).unwrap(), "/* mine */\n");
        assert_eq!(std::fs::read_to_string(d.layer.join("home.json")).unwrap(), "{\"chips\":[]}\n");
        assert_eq!(crate::applayer::history_in(st, &d.layer).await[0]["summary"], "Update built-in defaults to version 2.0.0");
        assert_eq!(std::fs::read_to_string(d.home.join("base/1.0.0/custom.css")).unwrap(), "/* v1 */\n");

        // Merge prompt carries both built-in versions; resolving clears the conflict.
        let prompt = merge_prompt(&d, v2, "custom.css").unwrap();
        assert!(prompt.contains("/* v1 */") && prompt.contains("/* v2 */"));
        assert_eq!(status(&d.home)["conflicts"], json!([{"path": "custom.css", "from": "1.0.0"}]), "still listed while the merge is prepared and reviewed");
        assert_eq!(status_merging(&d.home, &["custom.css".into()])["conflicts"], json!([{"path": "custom.css", "from": "1.0.0", "merging": true}]));
        assert_eq!(status_merging(&d.home, &["home.json".into()])["conflicts"], json!([{"path": "custom.css", "from": "1.0.0"}]));
        // The built-in refresh is the app's own entry: it is not offered for undoing.
        let top = &crate::applayer::history_in(st, &d.layer).await[0];
        assert_eq!((&top["kind"], &top["undoable"]), (&json!("system"), &json!(false)));
        assert!(crate::applayer::revert_in(st, &d.layer, top["hash"].as_str().unwrap()).await.unwrap_err().to_string().contains("cannot be undone"));
        // Once the merged version is kept the conflict is settled.
        clear_conflict(&d.home, "custom.css").unwrap();
        assert_eq!(status(&d.home)["conflicts"], json!([]));
        assert!(resolve(st, &d, v2, "custom.css", "new").await.is_err(), "already resolved");
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn the_update_question_says_what_is_new_and_keep_mine_can_be_revisited() {
        let (st, d) = testutil::layer("explain").await;
        let v1: &[(&str, &str)] = &[("custom.css", "/* v1 */\n"), ("home.json", "{\"chips\":[]}\n")];
        for (p, t) in v1 { std::fs::write(d.layer.join(p), t).unwrap(); }
        st.commit_all(&d.layer, "v1 defaults").await.unwrap();
        check_with(st, &d, "1.0.0", v1).await.unwrap();
        // The user changes both; the new version adds a shortcut and a colour.
        std::fs::write(d.layer.join("custom.css"), "/* mine */\n").unwrap();
        std::fs::write(d.layer.join("home.json"), "{\"chips\":[{\"label\":\"Mine\",\"prompt\":\"m\"}]}\n").unwrap();
        st.commit_all(&d.layer, "mine").await.unwrap();
        let v2: &[(&str, &str)] = &[("custom.css", "/* v2 */\n:root { --accent: #0a5c8a }\n"),
            ("home.json", "{\"chips\":[{\"label\":\"Weekly review\",\"prompt\":\"Review my week\"}]}\n")];
        check_with(st, &d, "2.0.0", v2).await.unwrap();
        // Each question says what the new version brings, compared with the one the user started from.
        let s = explained(status(&d.home), &d.home, v2);
        let home = s["conflicts"].as_array().unwrap().iter().find(|c| c["path"] == "home.json").unwrap().clone();
        assert_eq!(home["changes"], json!([{"area": "home", "op": "chipAdded", "label": "Weekly review", "prompt": "Review my week"}]));
        assert_eq!(home["to"], "2.0.0");
        let css = s["conflicts"].as_array().unwrap().iter().find(|c| c["path"] == "custom.css").unwrap().clone();
        assert_eq!(css["changes"][0]["report"]["kind"], "colours");
        // "Keep mine" is remembered, not forgotten: the new version can still be taken later.
        resolve(st, &d, v2, "home.json", "keep").await.unwrap();
        let s = status(&d.home);
        assert_eq!(s["kept"], json!([{"path": "home.json", "from": "1.0.0", "to": "2.0.0"}]));
        assert!(!s["conflicts"].as_array().unwrap().iter().any(|c| c["path"] == "home.json"));
        assert_eq!(explained(s, &d.home, v2)["kept"][0]["changes"][0]["label"], "Weekly review");
        assert_eq!(status_merging(&d.home, &["home.json".into()])["kept"][0]["merging"], true);
        assert!(merge_prompt(&d, v2, "home.json").unwrap().contains("Review my week"), "a kept file can be merged too");
        // A later version that does not touch it again leaves the offer standing.
        check_with(st, &d, "2.1.0", v2).await.unwrap();
        assert_eq!(status(&d.home)["kept"][0]["path"], "home.json");
        resolve(st, &d, v2, "home.json", "new").await.unwrap();
        assert_eq!(std::fs::read_to_string(d.layer.join("home.json")).unwrap(), v2[1].1);
        assert_eq!(status(&d.home)["kept"], json!([]));
        assert!(resolve(st, &d, v2, "home.json", "new").await.is_err(), "settled");
        // Once the user's own file equals the new default, there is nothing left to offer.
        resolve(st, &d, v2, "custom.css", "keep").await.unwrap();
        std::fs::write(d.layer.join("custom.css"), v2[0].1).unwrap();
        st.commit_all(&d.layer, "by hand").await.unwrap();
        check_with(st, &d, "2.2.0", v2).await.unwrap();
        assert_eq!(status(&d.home)["kept"], json!([]));
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

    #[tokio::test]
    async fn removed_defaults_open_conflicts_and_downgrades() {
        let (st, d) = testutil::layer("updates2").await;
        let read = |p: &str| std::fs::read_to_string(d.layer.join(p)).unwrap();
        let v1: &[(&str, &str)] = &[("custom.css", "/* v1 */\n"), ("home.json", "{}\n"), ("legacy.json", "{\"old\":true}\n"), ("gone.json", "{}\n")];
        for (p, t) in v1 { std::fs::write(d.layer.join(p), t).unwrap(); }
        st.commit_all(&d.layer, "v1 defaults").await.unwrap();
        check_with(st, &d, "1.0.0", v1).await.unwrap();

        // The user edits custom.css and deletes gone.json; v2 changes custom.css and drops both legacy files.
        std::fs::write(d.layer.join("custom.css"), "/* mine */\n").unwrap();
        std::fs::remove_file(d.layer.join("gone.json")).unwrap();
        st.commit_all(&d.layer, "mine").await.unwrap();
        let v2: &[(&str, &str)] = &[("custom.css", "/* v2 */\n"), ("home.json", "{}\n")];
        check_with(st, &d, "2.0.0", v2).await.unwrap();
        let s = status(&d.home);
        assert_eq!(s["removed"], json!(["legacy.json"]), "reported (the one the user still has), never deleted");
        assert_eq!(read("legacy.json"), "{\"old\":true}\n");
        assert_eq!(s["conflicts"], json!([{"path": "custom.css", "from": "1.0.0"}]));

        // v3 ships yet another custom.css and a new home.json while the conflict is still open.
        let v3: &[(&str, &str)] = &[("custom.css", "/* v3 */\n"), ("home.json", "{\"chips\":[]}\n")];
        assert!(check_with(st, &d, "3.0.0", v3).await.unwrap());
        let s = status(&d.home);
        assert_eq!(read("custom.css"), "/* mine */\n", "an undecided file is never overwritten");
        assert_eq!(s["conflicts"], json!([{"path": "custom.css", "from": "1.0.0"}]), "still open, still against the base it started from");
        assert_eq!(s["applied"], json!(["home.json"]));
        let prompt = merge_prompt(&d, v3, "custom.css").unwrap();
        assert!(prompt.contains("/* v1 */") && prompt.contains("/* v3 */") && !prompt.contains("/* v2 */"));

        // Even if the user's copy happens to equal an older default, an open conflict blocks a silent take.
        std::fs::write(d.layer.join("custom.css"), "/* v3 */\n").unwrap();
        st.commit_all(&d.layer, "back to v3 by hand").await.unwrap();
        let v4: &[(&str, &str)] = &[("custom.css", "/* v4 */\n"), ("home.json", "{\"chips\":[]}\n")];
        assert!(!check_with(st, &d, "4.0.0", v4).await.unwrap());
        assert_eq!(read("custom.css"), "/* v3 */\n");
        assert_eq!(status(&d.home)["conflicts"], json!([{"path": "custom.css", "from": "1.0.0"}]));

        // Starting an older app changes nothing at all and says so; the open conflict is kept.
        let before = crate::applayer::history_in(st, &d.layer).await.len();
        assert!(!check_with(st, &d, "3.5.0", v3).await.unwrap());
        let s = status(&d.home);
        assert_eq!((&s["downgrade"], s["from"].as_str(), s["to"].as_str()), (&json!(true), Some("4.0.0"), Some("3.5.0")));
        assert_eq!(s["conflicts"], json!([{"path": "custom.css", "from": "1.0.0"}]));
        assert_eq!(read("home.json"), "{\"chips\":[]}\n");
        assert_eq!(crate::applayer::history_in(st, &d.layer).await.len(), before);
        let base: Value = serde_json::from_str(&std::fs::read_to_string(d.home.join("base-manifest.json")).unwrap()).unwrap();
        assert_eq!(base["version"], "4.0.0", "the record of the newer defaults is kept for when the newer app returns");
        // Back on the newer app: nothing to do, and the note about the older app is cleared.
        assert!(!check_with(st, &d, "4.0.0", v4).await.unwrap());
        assert_eq!(status(&d.home)["downgrade"], false);
        assert_eq!(status(&d.home)["conflicts"], json!([{"path": "custom.css", "from": "1.0.0"}]));
        // Once the user's file equals the new default the conflict resolves itself.
        std::fs::write(d.layer.join("custom.css"), "/* v5 */\n").unwrap();
        check_with(st, &d, "5.0.0", &[("custom.css", "/* v5 */\n"), ("home.json", "{\"chips\":[]}\n")]).await.unwrap();
        let s = status(&d.home);
        assert_eq!((&s["conflicts"], &s["downgrade"]), (&json!([]), &json!(false)));
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[test]
    fn version_order() {
        assert!(older("0.9.3", "0.10.0") && older("1.0.0", "1.0.1") && older("1.2", "1.2.1") && older("0.0.2", "0.0.3-beta"));
        assert!(!older("1.0.0", "1.0.0") && !older("2.0.0", "1.9.9") && !older("1.0.0-rc1", "1.0.0"));
    }
}
