//! Customisation packs: the layer as one shareable JSON file. Importing a pack never applies it —
//! it is validated and staged as a pending change the user previews first.

use crate::applayer::{Dirs, Git};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const MAX_FILES: usize = 200;
pub const MAX_FILE_BYTES: usize = 512 * 1024;
pub const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;

const TOP_FILES: &[&str] = &["custom.css", "themes.json", "home.json", "manifest.json", "todos.json", "nav.json"];
const TOP_DIRS: &[&str] = &["plugins", "views", "workflows"];

/// Is `path` a place a customisation may live? Relative, forward slashes, no `..`, no dotfiles,
/// and only under the names the layer defines.
pub fn check_path(path: &str) -> anyhow::Result<()> {
    let bad = |why: &str| anyhow::anyhow!("{why}: {path}");
    if path.is_empty() || path.len() > 200 { return Err(bad("invalid file path")); }
    if path.starts_with('/') || path.starts_with('~') || path.contains('\\') || path.contains(':') || Path::new(path).is_absolute() {
        return Err(bad("file paths must be relative"));
    }
    // Nothing in a file name may be invisible or reorder how the name reads (`…sj.txt` that is really a `.js`).
    if path.chars().any(|c| c.is_control() || crate::staging::validate::is_hidden_char(c)) { return Err(bad("invalid characters in file path")); }
    let parts: Vec<&str> = path.split('/').collect();
    for p in &parts {
        if p.is_empty() || *p == "." || *p == ".." { return Err(bad("file paths may not contain '..' or empty parts")); }
        if p.starts_with('.') { return Err(bad("hidden files are not allowed")); }
        if p.trim() != *p { return Err(bad("invalid file path")); }
    }
    let ok = match parts.len() {
        1 => TOP_FILES.contains(&parts[0]),
        n => TOP_DIRS.contains(&parts[0]) && n <= 6,
    };
    if !ok { return Err(bad("not a customisation file (allowed: custom.css, themes.json, home.json, nav.json, todos.json, manifest.json, plugins/, views/, workflows/)")); }
    Ok(())
}

/// Validate a pack and return its files.
pub fn check_pack(pack: &Value) -> anyhow::Result<BTreeMap<String, String>> {
    if pack["format"] != "thoughts-pack" { anyhow::bail!("this file is not a Thoughts customisation pack"); }
    if pack["version"] != 1 { anyhow::bail!("this pack was made by a newer version of Thoughts"); }
    // Older packs carry no apiVersion: they are version 1.
    if !pack["apiVersion"].is_null() && pack["apiVersion"].as_u64().map_or(true, |v| v == 0 || v > crate::applayer::API_VERSION) {
        anyhow::bail!("this pack needs a newer version of Thoughts (it uses a newer plugin API)");
    }
    if !pack["kind"].is_null() && pack["kind"] != "full" && pack["kind"] != "feature" { anyhow::bail!("this pack is of a kind this version of Thoughts does not know"); }
    let files = pack["files"].as_object().ok_or_else(|| anyhow::anyhow!("the pack has no files"))?;
    if files.is_empty() { anyhow::bail!("the pack has no files"); }
    if files.len() > MAX_FILES { anyhow::bail!("the pack has too many files (limit {MAX_FILES})"); }
    let mut out = BTreeMap::new();
    let mut total = 0usize;
    for (path, v) in files {
        check_path(path)?;
        let text = v.as_str().ok_or_else(|| anyhow::anyhow!("{path} is not text"))?;
        if text.len() > MAX_FILE_BYTES { anyhow::bail!("{path} is too large (limit 512 KB)"); }
        total += text.len();
        if total > MAX_TOTAL_BYTES { anyhow::bail!("the pack is too large (limit 4 MB)"); }
        out.insert(path.clone(), text.to_string());
    }
    Ok(out)
}

/// Every customisation file in `dir` (text files in allowed places only; links are never followed).
pub fn collect(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut total = 0usize;
    for e in walkdir::WalkDir::new(dir).follow_links(false).max_depth(6).sort_by_file_name().into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.')).flatten()
    {
        if !e.file_type().is_file() { continue; }
        let Ok(rel) = e.path().strip_prefix(dir) else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if check_path(&rel).is_err() { continue; }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        if text.len() > MAX_FILE_BYTES || total + text.len() > MAX_TOTAL_BYTES || out.len() >= MAX_FILES { continue; }
        total += text.len();
        out.insert(rel, text);
    }
    out
}

/// One line of someone else's text, as it will be shown: no control characters, and anything that
/// would not show (zero-width, direction-changing…) written out.
fn clean_line(text: &str, max: usize) -> String {
    let line: String = text.chars().filter(|c| !c.is_control()).collect();
    crate::staging::validate::show_hidden(&line, false).chars().take(max).collect::<String>().trim().to_string()
}

/// A pack's name inside one of the app's own sentences: short, and unable to close the quotation
/// marks around it and carry on as if the app were speaking.
fn quoted_name(name: &str) -> String {
    let n: String = name.chars().filter(|c| !"\"'`«»“”„‟‹›".contains(*c)).take(60).collect();
    if n.trim().is_empty() { "pack".into() } else { n.trim().to_string() }
}

/// The whole layer as a pack (`kind: "full"`).
pub fn export(dir: &Path, name: &str) -> Value { export_with(dir, name, "", None).unwrap_or_else(|_| json!({})) }

/// A pack of the layer. With `paths` (files, or folders ending in `/`) only those are included and
/// the pack is a `feature` pack that merges into the importer's layer instead of replacing files.
/// A selected plugin or view always travels with its declaration: its own `<name>.json`, or one
/// written from its `manifest.json` entry.
pub fn export_with(dir: &Path, name: &str, description: &str, paths: Option<&[String]>) -> anyhow::Result<Value> {
    let all = collect(dir);
    let (kind, files) = match paths {
        None => ("full", all),
        Some(wanted) => {
            let shared: Value = all.get("manifest.json").and_then(|t| serde_json::from_str(t).ok()).unwrap_or(Value::Null);
            let mut out = BTreeMap::new();
            for w in wanted.iter().map(|w| w.trim()).filter(|w| !w.is_empty()) {
                for (path, text) in all.iter().filter(|(p, _)| p.as_str() == w || (w.ends_with('/') && p.starts_with(w))) {
                    out.insert(path.clone(), text.clone());
                    let (sub, ext) = if path.starts_with("plugins/") { ("plugins", ".js") } else if path.starts_with("views/") { ("views", ".html") } else { continue };
                    let Some(file) = path.strip_prefix(sub).and_then(|f| f.strip_prefix('/')).filter(|f| !f.contains('/') && f.ends_with(ext)) else { continue };
                    let decl_path = format!("{sub}/{}.json", file.trim_end_matches(ext));
                    if let Some(own) = all.get(&decl_path) {
                        out.insert(decl_path, own.clone());
                    } else if let Some(entry) = shared[sub][file].as_object() {
                        let mut decl = entry.clone();
                        decl.entry("apiVersion").or_insert(json!(shared["apiVersion"].as_u64().unwrap_or(crate::applayer::API_VERSION)));
                        out.insert(decl_path, serde_json::to_string_pretty(&decl)? + "\n");
                    }
                }
            }
            if out.is_empty() { anyhow::bail!("nothing to export: none of the chosen files exist"); }
            ("feature", out)
        }
    };
    let name = clean_line(name, 80);
    Ok(json!({
        "format": "thoughts-pack", "version": 1, "apiVersion": crate::applayer::API_VERSION, "kind": kind,
        "name": if name.is_empty() { "My customisations" } else { name.as_str() },
        "description": clean_line(description, 400),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "files": files,
    }))
}

/// Key-wise merge of a pack's JSON into the user's: objects merge per key, lists are unioned (the
/// user's items first, then the pack's new ones), and for a plain value both define, the pack's wins.
pub fn merge_json(mine: &Value, theirs: &Value) -> Value {
    match (mine, theirs) {
        (Value::Object(a), Value::Object(b)) => {
            let mut out = a.clone();
            for (k, v) in b {
                let merged = match a.get(k) { Some(old) => merge_json(old, v), None => v.clone() };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        (Value::Array(a), Value::Array(b)) => {
            let mut out = a.clone();
            for item in b {
                // Things with an `id` (themes) are replaced in place rather than listed twice.
                match item["id"].as_str().and_then(|id| out.iter().position(|x| x["id"] == id)) {
                    Some(i) => out[i] = item.clone(),
                    None => if !out.contains(item) { out.push(item.clone()); },
                }
            }
            Value::Array(out)
        }
        _ => theirs.clone(),
    }
}

/// The pack's name as it appears in the CSS markers (nothing that could end a comment).
fn marker_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if c.is_alphanumeric() || " ._-".contains(c) { c } else { '-' }).take(60).collect();
    if n.trim().is_empty() { "pack".into() } else { n.trim().to_string() }
}

/// Put a pack's CSS into the user's `custom.css` inside a marked block, replacing an earlier block
/// of the same pack and leaving every other line alone.
pub fn merge_css(mine: &str, pack_css: &str, pack_name: &str) -> String {
    let name = marker_name(pack_name);
    let (open, close) = (format!("/* pack:{name} */"), format!("/* end pack:{name} */"));
    let block = format!("{open}\n{}\n{close}\n", pack_css.trim_end());
    if let Some(start) = mine.find(&open) {
        if let Some(end) = mine[start..].find(&close).map(|i| start + i + close.len()) {
            let rest = mine[end..].strip_prefix("\r\n").or_else(|| mine[end..].strip_prefix('\n')).unwrap_or(&mine[end..]);
            return format!("{}{block}{rest}", &mine[..start]);
        }
    }
    let mut out = mine.to_string();
    if !out.is_empty() && !out.ends_with('\n') { out.push('\n'); }
    if !out.trim().is_empty() { out.push('\n'); }
    out + &block
}

/// What a feature pack's file becomes once merged with the user's copy (`None` = they have none).
fn merged_file(path: &str, mine: Option<&str>, theirs: &str, pack_name: &str) -> anyhow::Result<String> {
    if path == "custom.css" { return Ok(merge_css(mine.unwrap_or(""), theirs, pack_name)); }
    let top_json = !path.contains('/') && path.ends_with(".json");
    let Some(mine) = mine.filter(|_| top_json) else { return Ok(theirs.to_string()) };
    let theirs_v: Value = serde_json::from_str(theirs).map_err(|_| anyhow::anyhow!("{path} in the pack is not valid JSON"))?;
    // A damaged file of the user's is never silently replaced by a merge.
    let mine_v: Value = serde_json::from_str(mine).map_err(|_| anyhow::anyhow!("your {path} is not valid JSON, so the pack cannot be merged into it; fix or remove it first"))?;
    if std::mem::discriminant(&mine_v) != std::mem::discriminant(&theirs_v) { anyhow::bail!("{path} in the pack does not have the same shape as yours"); }
    Ok(serde_json::to_string_pretty(&merge_json(&mine_v, &theirs_v))? + "\n")
}

/// Stage a pack as a pending change (origin `import`). Nothing is applied. A `full` pack replaces
/// the files it names; a `feature` pack merges into what the user already has.
pub async fn import(g: Git<'_>, d: &Dirs, pack: &Value, arabic: bool) -> anyhow::Result<()> {
    let files = check_pack(pack)?;
    let feature = pack["kind"] == "feature";
    let name = clean_line(pack["name"].as_str().unwrap_or(""), 80);
    let name = if name.is_empty() { "pack".to_string() } else { name };
    // The change that was waiting for review is only gone once this pack has really been staged.
    let waiting = crate::staging::put_aside(g, d).await;
    if let Err(e) = crate::staging::recreate(g, d, "import", None).await {
        if let Some(w) = waiting { crate::staging::put_back(g, d, w).await; }
        return Err(e);
    }
    let mut replaced: Vec<&str> = vec![];
    let result: anyhow::Result<()> = (|| {
        for (path, text) in &files {
            let target = d.staging.join(path);
            if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
            if std::fs::symlink_metadata(&target).map_or(false, |m| m.file_type().is_symlink()) { std::fs::remove_file(&target)?; }
            let mine = std::fs::read_to_string(&target).ok();
            let out = if feature { merged_file(path, mine.as_deref(), text, &name)? } else { text.clone() };
            if out.len() > MAX_FILE_BYTES { anyhow::bail!("{path} would become too large (limit 512 KB)"); }
            if feature && path.contains('/') && mine.as_deref().map_or(false, |m| m != out) { replaced.push(path.as_str()); }
            std::fs::write(&target, out)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        // A pack that cannot be merged leaves no half-staged change behind, and costs nothing that was waiting.
        match waiting {
            Some(w) => crate::staging::put_back(g, d, w).await,
            None => { let _ = crate::staging::discard(g, d).await; }
        }
        return Err(e);
    }
    // What the pack says about itself is kept apart and shown as the pack's words. It never becomes
    // part of the app's own description of the change, nor of the history entry.
    crate::staging::set_pack(d, &name, &clean_line(pack["description"].as_str().unwrap_or(""), 400));
    let name = quoted_name(&name);
    let mut summary = match (arabic, feature) {
        (true, true) => format!("إضافة حزمة «{name}» إلى تخصيصاتك\n\nتضيف الحزمة {} ملف/ملفات وتُدمج مع ما لديك دون أن تستبدل إعداداتك الأخرى. جرّبها أولًا، ثم أبقِها إن أعجبتك؛ ويمكنك التراجع عنها لاحقًا من السجل.", files.len()),
        (true, false) => format!("استيراد حزمة التخصيص «{name}»\n\nتحتوي الحزمة على {} ملف/ملفات تحلّ محلّ الملفات المقابلة لديك. جرّبها أولًا، ثم أبقِها إن أعجبتك؛ ويمكنك التراجع عنها لاحقًا من السجل.", files.len()),
        (false, true) => format!("Add the pack \"{name}\" to your customisations\n\nThe pack brings {} file(s) and is merged into what you already have; your other settings stay as they are. Try it first, then keep it if you like it — you can undo it later from History.", files.len()),
        (false, false) => format!("Import the customisation pack \"{name}\"\n\nThe pack contains {} file(s), which replace the matching files you have now. Try it first, then keep it if you like it — you can undo it later from History.", files.len()),
    };
    if !replaced.is_empty() {
        let list = replaced.join(", ");
        summary.push_str(&if arabic { format!("\n\nتنبيه: لديك ملفات بالاسم نفسه ستُستبدل بنسخة الحزمة: {list}") } else { format!("\n\nNote: you already have files with the same names, and the pack's versions will replace them: {list}") });
    }
    std::fs::write(d.staging.join("SUMMARY.md"), summary)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_rules() {
        for ok in ["custom.css", "themes.json", "home.json", "manifest.json", "todos.json", "nav.json", "plugins/a.json", "views/board.json", "plugins/a.js", "views/board.html", "workflows/weekly.json", "plugins/lib/util.js"] {
            assert!(check_path(ok).is_ok(), "{ok}");
        }
        for bad in ["", "/etc/passwd", "../x", "plugins/../../x", "plugins/./a.js", ".git/config", "plugins/.hidden.js", ".claude/settings.json",
                    "README.md", "CLAUDE.md", "SUMMARY.md", "other/x.js", "plugins", "custom.css/x", "plugins\\a.js", "C:/x", "~/x", "plugins//a.js", "plugins/a\n.js"] {
            assert!(check_path(bad).is_err(), "{bad}");
        }
        // Nothing invisible in a name: a zero-width character, or `evil\u{202e}sj.txt`, which reads "eviltxt.js".
        for bad in ["plugins/a\u{200b}.js", "plugins/evil\u{202e}sj.txt", "workflows/w\u{2066}.json", "plugins/lib/x\u{feff}.js", "views/a\u{e0041}.html"] {
            assert!(check_path(bad).is_err(), "{bad:?}");
        }
        assert!(check_path("workflows/مراجعة.json").is_ok());
        assert_eq!(clean_line("Calm\u{202e} theme\n\u{200b}", 80), "Calm[U+202E] theme[U+200B]");
    }

    #[test]
    fn pack_limits() {
        let pack = |files: Value| json!({"format": "thoughts-pack", "version": 1, "name": "x", "files": files});
        assert_eq!(check_pack(&pack(json!({"custom.css": "a{}"}))).unwrap().len(), 1);
        assert!(check_pack(&json!({"format": "zip", "version": 1, "files": {"custom.css": ""}})).is_err());
        assert!(check_pack(&json!({"format": "thoughts-pack", "version": 2, "files": {"custom.css": ""}})).is_err());
        assert!(check_pack(&pack(json!({}))).is_err());
        let with = |k: &str, v: Value| { let mut p = pack(json!({"custom.css": "a{}"})); p[k] = v; check_pack(&p) };
        assert!(with("apiVersion", json!(1)).is_ok() && with("kind", json!("feature")).is_ok() && with("kind", json!("full")).is_ok());
        assert!(with("apiVersion", json!(2)).is_err(), "a pack for a newer plugin API is not applied");
        assert!(with("apiVersion", json!("1")).is_err() && with("kind", json!("theme")).is_err());
        assert!(check_pack(&pack(json!({"custom.css": 5}))).is_err());
        assert!(check_pack(&pack(json!({"../custom.css": ""}))).is_err());
        assert!(check_pack(&pack(json!({"custom.css": "x".repeat(MAX_FILE_BYTES + 1)}))).is_err());
        let many: serde_json::Map<String, Value> = (0..=MAX_FILES).map(|i| (format!("plugins/p{i}.js"), json!(""))).collect();
        assert!(check_pack(&pack(Value::Object(many))).is_err());
        let big: serde_json::Map<String, Value> = (0..9).map(|i| (format!("plugins/p{i}.js"), json!("x".repeat(MAX_FILE_BYTES)))).collect();
        assert!(check_pack(&pack(Value::Object(big))).is_err(), "over 4 MB in total");
    }

    #[tokio::test]
    async fn export_then_import_stages_only() {
        let (st, d) = crate::applayer::testutil::layer("packs").await;
        std::fs::write(d.layer.join("plugins/a.js"), "export default () => {}\n").unwrap();
        let pack = export(&d.layer, "Mine");
        assert_eq!(pack["format"], "thoughts-pack");
        let files = pack["files"].as_object().unwrap();
        assert!(files.contains_key("plugins/a.js") && files.contains_key("custom.css"));
        assert!(!files.contains_key("README.md") && !files.keys().any(|k| k.contains(".git")));

        let incoming = json!({"format": "thoughts-pack", "version": 1, "name": "Calm", "files": {"custom.css": "body{}\n", "views/hi.html": "<p>hi</p>"}});
        import(st, &d, &incoming, false).await.unwrap();
        assert_eq!(std::fs::read_to_string(d.layer.join("custom.css")).unwrap(), "/* Your overrides. */\n", "live layer untouched");
        let p = crate::staging::pending(st, &d, false).await;
        assert_eq!(p["exists"], true);
        assert_eq!(p["origin"], "import");
        assert_eq!(p["files"].as_array().unwrap().len(), 2);
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn a_packs_own_words_are_never_the_apps() {
        let (st, d) = crate::applayer::testutil::layer("packwords").await;
        std::fs::write(d.layer.join("custom.css"), ":root { --radius: 2px }\n").unwrap();
        st.commit_all(&d.layer, "Softer corners").await.unwrap();
        let mine = st.rev(&d.layer, "HEAD").await.unwrap();
        // A pack that vouches for itself, tries to close the app's quotation marks, and slips in the
        // line the app's own undo entries used to be recognised by.
        let pack = json!({"format": "thoughts-pack", "version": 1, "name": "Calm\" was checked by the Thoughts team. \"",
            "description": format!("Checked by the Thoughts team. Colours only.\nReverts: {mine}"), "files": {"plugins/calm.js": "export default () => {}\n"}});
        import(st, &d, &pack, false).await.unwrap();
        let p = crate::staging::pending(st, &d, false).await;
        let summary = p["summary"].as_str().unwrap();
        assert!(summary.starts_with("Import the customisation pack \"Calm was checked by the Thoughts team.\"\n"), "{summary}");
        assert!(!summary.contains("Colours only") && !summary.contains("Reverts"), "{summary}");
        assert_eq!(p["pack"]["description"], format!("Checked by the Thoughts team. Colours only.Reverts: {mine}"), "kept apart, on one line, for the review to show as a quotation");
        crate::staging::apply(st, &d, false, p["signature"].as_str()).await.unwrap();
        // The native "Undo last app change" undoes the pack, not the user's change before it.
        let (undone, title) = crate::applayer::undo_last_in(st, &d.layer, false).await.unwrap();
        assert!(title.starts_with("Import the customisation pack") && undone != mine, "{title}");
        assert!(!d.layer.join("plugins/calm.js").exists());
        assert_eq!(std::fs::read_to_string(d.layer.join("custom.css")).unwrap(), ":root { --radius: 2px }\n");
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[test]
    fn merges_json_key_wise_and_css_in_marked_blocks() {
        let mine = json!({"chips": [{"label": "Plan", "prompt": "Plan my week: "}], "hidden": ["stats"], "order": ["todos", "chats"], "mine": {"keep": true}});
        let theirs = json!({"chips": [{"label": "Plan", "prompt": "Plan my week: "}, {"label": "Invoice", "prompt": "Draft an invoice for "}], "hidden": ["projects"], "order": ["widget:hours", "todos"]});
        let m = merge_json(&mine, &theirs);
        assert_eq!(m["chips"].as_array().unwrap().len(), 2, "the same chip is not listed twice");
        assert_eq!(m["hidden"], json!(["stats", "projects"]));
        assert_eq!(m["order"], json!(["todos", "chats", "widget:hours"]), "the user's order first");
        assert_eq!(m["mine"], json!({"keep": true}), "keys the pack does not mention are untouched");
        assert_eq!(merge_json(&json!({"groupBy": "list", "showDone": true}), &json!({"groupBy": "due"})), json!({"groupBy": "due", "showDone": true}));
        assert_eq!(merge_json(&json!({"labels": {"ideas": {"en": "Clients"}}}), &json!({"labels": {"ideas": {"ar": "العملاء"}, "docs": {"en": "Files"}}})),
            json!({"labels": {"ideas": {"en": "Clients", "ar": "العملاء"}, "docs": {"en": "Files"}}}));
        assert_eq!(merge_json(&json!([{"id": "sand", "name": "Sand"}, {"id": "sea", "name": "Sea"}]), &json!([{"id": "sand", "name": "Sand 2"}, {"id": "ink", "name": "Ink"}])),
            json!([{"id": "sand", "name": "Sand 2"}, {"id": "sea", "name": "Sea"}, {"id": "ink", "name": "Ink"}]));

        let css = merge_css("/* Your overrides. */\n:root { --radius: 2px }\n", ".board { gap: 8px }\n", "Kanban */ board");
        assert_eq!(css, "/* Your overrides. */\n:root { --radius: 2px }\n\n/* pack:Kanban -- board */\n.board { gap: 8px }\n/* end pack:Kanban -- board */\n");
        // A newer version of the same pack replaces its own block only; other packs and the user's lines stay.
        let css = merge_css(&(css + "\nbody { margin: 0 }\n"), ".other { color: red }", "Other");
        let again = merge_css(&css, ".board { gap: 12px }", "Kanban */ board");
        assert!(again.contains("gap: 12px") && !again.contains("gap: 8px"));
        assert!(again.starts_with("/* Your overrides. */\n:root { --radius: 2px }\n") && again.contains("body { margin: 0 }") && again.contains(".other { color: red }"));
        assert_eq!(again.matches("/* pack:Kanban -- board */").count(), 1);
        assert_eq!(merge_css("", "a{}", ""), "/* pack:pack */\na{}\n/* end pack:pack */\n");
    }

    #[tokio::test]
    async fn feature_pack_exports_a_subset_and_merges_on_import() {
        let (st, d) = crate::applayer::testutil::layer("featurepack").await;
        let w = |p: &str, t: &str| std::fs::write(d.layer.join(p), t).unwrap();
        // Someone's layer with two plugins; only the board is shared.
        w("manifest.json", r#"{"apiVersion":1,"plugins":{"board.js":{"permissions":["ideas:read","ideas:write"]}}}"#);
        w("plugins/board.js", "export default () => { /* board */ }\n");
        w("plugins/hours.js", "export default () => { /* hours */ }\n");
        w("plugins/hours.json", r#"{"apiVersion":1,"permissions":["storage"]}"#);
        w("views/cal.html", "<p>cal</p>");
        w("views/cal.json", r#"{"apiVersion":1,"permissions":["todos:read"]}"#);
        w("home.json", r#"{"chips":[{"label":"Board","prompt":"Open my board"}],"order":["widget:board"]}"#);
        w("custom.css", ".board { gap: 8px }\n");
        let paths: Vec<String> = ["plugins/board.js", "views/", "home.json", "custom.css", "plugins/missing.js"].iter().map(|s| s.to_string()).collect();
        let pack = export_with(&d.layer, "Kanban", "A board of your items by status.", Some(&paths)).unwrap();
        assert_eq!((&pack["kind"], &pack["apiVersion"], &pack["description"]), (&json!("feature"), &json!(1), &json!("A board of your items by status.")));
        let files = pack["files"].as_object().unwrap();
        assert_eq!(files.keys().map(String::as_str).collect::<Vec<_>>(), ["custom.css", "home.json", "plugins/board.js", "plugins/board.json", "views/cal.html", "views/cal.json"]);
        assert_eq!(serde_json::from_str::<Value>(files["plugins/board.json"].as_str().unwrap()).unwrap(), json!({"apiVersion": 1, "permissions": ["ideas:read", "ideas:write"]}),
            "permissions that lived in manifest.json travel as the plugin's own file");
        assert!(export_with(&d.layer, "x", "", Some(&["plugins/none.js".to_string()])).is_err());
        assert_eq!(export(&d.layer, "")["kind"], "full");
        assert_eq!(export(&d.layer, "")["name"], "My customisations");
        std::fs::remove_dir_all(&d.home).unwrap();

        // Someone else imports it into a layer they have already made their own.
        let (st2, d2) = crate::applayer::testutil::layer("featureimport").await;
        let w = |p: &str, t: &str| std::fs::write(d2.layer.join(p), t).unwrap();
        w("home.json", r#"{"chips":[{"label":"Plan","prompt":"Plan my week: "}],"hidden":["stats"],"order":["todos"]}"#);
        w("custom.css", "/* mine */\n:root { --accent: #0a5c8a }\n");
        w("plugins/mine.js", "export default () => { /* mine */ }\n");
        w("todos.json", r#"{"groupBy":"due"}"#);
        st2.commit_all(&d2.layer, "my layer").await.unwrap();
        import(st2, &d2, &pack, false).await.unwrap();
        let p = crate::staging::pending(st2, &d2, false).await;
        assert_eq!((&p["origin"], &p["problems"]), (&json!("import"), &json!([])));
        // What the pack says about itself is the pack's, not the app's: apart from the app's summary.
        assert!(!p["summary"].as_str().unwrap().contains("A board of your items by status."));
        assert_eq!(p["pack"], json!({"name": "Kanban", "description": "A board of your items by status."}));
        let status = |path: &str| p["files"].as_array().unwrap().iter().find(|f| f["path"] == path).map(|f| f["status"].as_str().unwrap().to_string());
        assert_eq!(status("plugins/board.js").as_deref(), Some("added"));
        assert_eq!(status("home.json").as_deref(), Some("modified"));
        assert_eq!(status("plugins/mine.js"), None);
        assert_eq!(status("todos.json"), None, "files the pack does not carry are not touched");
        assert_eq!(status("manifest.json"), None, "no shared manifest is overwritten");
        let home: Value = serde_json::from_str(&std::fs::read_to_string(d2.staging.join("home.json")).unwrap()).unwrap();
        assert_eq!(home, json!({"chips": [{"label": "Plan", "prompt": "Plan my week: "}, {"label": "Board", "prompt": "Open my board"}], "hidden": ["stats"], "order": ["todos", "widget:board"]}));
        let css = std::fs::read_to_string(d2.staging.join("custom.css")).unwrap();
        assert_eq!(css, "/* mine */\n:root { --accent: #0a5c8a }\n\n/* pack:Kanban */\n.board { gap: 8px }\n/* end pack:Kanban */\n");
        assert_eq!(std::fs::read_to_string(d2.layer.join("custom.css")).unwrap(), "/* mine */\n:root { --accent: #0a5c8a }\n", "live layer untouched until kept");
        crate::staging::apply(st2, &d2, false, p["signature"].as_str()).await.unwrap();
        let m = crate::applayer::manifest_of(&d2.layer);
        let names: Vec<&str> = m["plugins"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["board.js", "mine.js"]);
        assert_eq!(m["plugins"][0]["permissions"], json!(["ideas:read", "ideas:write"]));

        // Importing it again changes nothing; a damaged file of the user's stops the merge instead of being replaced.
        import(st2, &d2, &pack, false).await.unwrap();
        assert_eq!(crate::staging::pending(st2, &d2, false).await["exists"], false, "already merged: no duplicate chips, no second CSS block");
        std::fs::write(d2.layer.join("home.json"), "{broken").unwrap();
        st2.commit_all(&d2.layer, "oops").await.unwrap();
        assert!(import(st2, &d2, &pack, false).await.unwrap_err().to_string().contains("your home.json is not valid JSON"));
        assert!(!d2.staging.exists(), "nothing half-staged is left behind");
        // The same files as a full pack replace instead.
        let mut full = pack.clone();
        full["kind"] = json!("full");
        import(st2, &d2, &full, true).await.unwrap();
        assert_eq!(std::fs::read_to_string(d2.staging.join("custom.css")).unwrap(), ".board { gap: 8px }\n");
        assert!(std::fs::read_to_string(d2.staging.join("SUMMARY.md")).unwrap().contains("تحلّ محلّ"));
        let _ = st;
        std::fs::remove_dir_all(&d2.home).unwrap();
    }
}
