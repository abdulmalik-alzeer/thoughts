//! Customisation packs: the layer as one shareable JSON file. Importing a pack never applies it —
//! it is validated and staged as a pending change the user previews first.

use crate::applayer::{Dirs, Git};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const MAX_FILES: usize = 200;
pub const MAX_FILE_BYTES: usize = 512 * 1024;
pub const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;

const TOP_FILES: &[&str] = &["custom.css", "themes.json", "home.json", "manifest.json"];
const TOP_DIRS: &[&str] = &["plugins", "views", "workflows"];

/// Is `path` a place a customisation may live? Relative, forward slashes, no `..`, no dotfiles,
/// and only under the names the layer defines.
pub fn check_path(path: &str) -> anyhow::Result<()> {
    let bad = |why: &str| anyhow::anyhow!("{why}: {path}");
    if path.is_empty() || path.len() > 200 { return Err(bad("invalid file path")); }
    if path.starts_with('/') || path.starts_with('~') || path.contains('\\') || path.contains(':') || Path::new(path).is_absolute() {
        return Err(bad("file paths must be relative"));
    }
    if path.chars().any(char::is_control) { return Err(bad("invalid characters in file path")); }
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
    if !ok { return Err(bad("not a customisation file (allowed: custom.css, themes.json, home.json, manifest.json, plugins/, views/, workflows/)")); }
    Ok(())
}

/// Validate a pack and return its files.
pub fn check_pack(pack: &Value) -> anyhow::Result<BTreeMap<String, String>> {
    if pack["format"] != "thoughts-pack" { anyhow::bail!("this file is not a Thoughts customisation pack"); }
    if pack["version"] != 1 { anyhow::bail!("this pack was made by a newer version of Thoughts"); }
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

pub fn export(dir: &Path, name: &str) -> Value {
    let name: String = name.chars().filter(|c| !c.is_control()).take(80).collect();
    json!({
        "format": "thoughts-pack", "version": 1,
        "name": if name.trim().is_empty() { "My customisations" } else { name.trim() },
        "appVersion": env!("CARGO_PKG_VERSION"),
        "files": collect(dir),
    })
}

/// Stage a pack as a pending change (origin `import`). Nothing is applied.
pub async fn import(g: Git<'_>, d: &Dirs, pack: &Value, arabic: bool) -> anyhow::Result<()> {
    let files = check_pack(pack)?;
    crate::staging::recreate(g, d, "import", None).await?;
    for (path, text) in &files {
        let target = d.staging.join(path);
        if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
        if std::fs::symlink_metadata(&target).map_or(false, |m| m.file_type().is_symlink()) { std::fs::remove_file(&target)?; }
        std::fs::write(&target, text)?;
    }
    let name: String = pack["name"].as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(80).collect();
    let name = if name.trim().is_empty() { "pack".to_string() } else { name.trim().to_string() };
    let summary = if arabic {
        format!("استيراد حزمة التخصيص «{name}»\n\nتحتوي الحزمة على {} ملف/ملفات. جرّبها أولًا، ثم طبّقها إن أعجبتك؛ ويمكنك التراجع عنها لاحقًا من السجل.", files.len())
    } else {
        format!("Import the customisation pack \"{name}\"\n\nThe pack contains {} file(s). Try it first, then apply it if you like it — you can undo it later from History.", files.len())
    };
    std::fs::write(d.staging.join("SUMMARY.md"), summary)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_rules() {
        for ok in ["custom.css", "themes.json", "home.json", "manifest.json", "plugins/a.js", "views/board.html", "workflows/weekly.json", "plugins/lib/util.js"] {
            assert!(check_path(ok).is_ok(), "{ok}");
        }
        for bad in ["", "/etc/passwd", "../x", "plugins/../../x", "plugins/./a.js", ".git/config", "plugins/.hidden.js", ".claude/settings.json",
                    "README.md", "CLAUDE.md", "SUMMARY.md", "other/x.js", "plugins", "custom.css/x", "plugins\\a.js", "C:/x", "~/x", "plugins//a.js", "plugins/a\n.js"] {
            assert!(check_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn pack_limits() {
        let pack = |files: Value| json!({"format": "thoughts-pack", "version": 1, "name": "x", "files": files});
        assert_eq!(check_pack(&pack(json!({"custom.css": "a{}"}))).unwrap().len(), 1);
        assert!(check_pack(&json!({"format": "zip", "version": 1, "files": {"custom.css": ""}})).is_err());
        assert!(check_pack(&json!({"format": "thoughts-pack", "version": 2, "files": {"custom.css": ""}})).is_err());
        assert!(check_pack(&pack(json!({}))).is_err());
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
}
