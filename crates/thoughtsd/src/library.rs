//! Read-side views over the markdown library: ideas, file tree, search, stats.

use crate::paths::safe_join;
use serde_json::{json, Value};
use std::path::Path;
use walkdir::WalkDir;

const SKIP: &[&str] = &[".git", "node_modules", ".thoughts-search", "target"];

fn skip(e: &walkdir::DirEntry) -> bool {
    e.file_name().to_str().map_or(false, |n| SKIP.contains(&n))
}

/// Split `---\nyaml\n---\nbody`.
pub fn frontmatter(text: &str) -> (Value, &str) {
    if let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) {
        if let Some(end) = rest.find("\n---") {
            let yaml = &rest[..end];
            let body = rest[end + 4..].trim_start_matches(['\r', '\n']);
            let v: Value = serde_yaml::from_str::<serde_yaml::Value>(yaml)
                .ok().and_then(|y| serde_json::to_value(y).ok()).unwrap_or(Value::Null);
            return (v, body);
        }
    }
    (Value::Null, text)
}

/// Every `<area>/<slug>/README.md` with frontmatter.
pub fn ideas(root: &Path) -> Vec<Value> {
    let mut out = vec![];
    for e in WalkDir::new(root).min_depth(3).max_depth(3).into_iter().filter_entry(|e| !skip(e)).flatten() {
        if e.file_name() != "README.md" { continue; }
        let rel = e.path().strip_prefix(root).unwrap();
        let parts: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        if parts.len() != 3 || parts[0].starts_with('.') || parts[0] == "me" || parts[0] == "inbox" { continue; }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let (fm, _) = frontmatter(&text);
        if fm.is_null() { continue; }
        let s = |k: &str| fm[k].as_str().map(String::from).unwrap_or_default();
        let todo_open = std::fs::read_to_string(e.path().with_file_name("todo.md"))
            .map(|t| t.lines().filter(|l| l.starts_with("- [ ] ")).count()).unwrap_or(0);
        out.push(json!({
            "path": rel.to_string_lossy().replace('\\', "/"),
            "area": parts[0], "slug": parts[1],
            "title": if s("title").is_empty() { parts[1].clone() } else { s("title") },
            "status": s("status"), "summary": s("summary"),
            "tags": fm["tags"], "level": fm["level"],
            "updated": fm["updated"].as_str().map(String::from).or_else(|| fm["updated"].as_str().map(String::from)).unwrap_or_else(|| yaml_date(&fm["updated"])),
            "openTodos": todo_open,
        }));
    }
    out.sort_by(|a, b| b["updated"].as_str().cmp(&a["updated"].as_str()));
    out
}

fn yaml_date(v: &Value) -> String {
    match v { Value::String(s) => s.clone(), Value::Null => String::new(), o => o.to_string() }
}

/// All markdown and output files, grouped client-side.
pub fn tree(root: &Path) -> Vec<Value> {
    let mut out = vec![];
    for e in WalkDir::new(root).into_iter().filter_entry(|e| !skip(e)).flatten() {
        if !e.file_type().is_file() { continue; }
        let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        let meta = e.metadata().ok();
        out.push(json!({
            "path": rel,
            "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
            "modified": meta.and_then(|m| m.modified().ok())
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()),
        }));
    }
    out
}

pub fn read(root: &Path, rel: &str) -> anyhow::Result<Value> {
    let p = safe_join(root, rel)?;
    let text = std::fs::read_to_string(&p)?;
    let (fm, body) = frontmatter(&text);
    Ok(json!({"path": rel, "content": text, "frontmatter": fm, "body": body}))
}

pub fn write(root: &Path, rel: &str, content: &str) -> anyhow::Result<()> {
    let p = safe_join(root, rel)?;
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(p, content)?;
    Ok(())
}

pub fn search(root: &Path, q: &str, limit: usize) -> Vec<Value> {
    let needle = q.to_lowercase();
    if needle.trim().is_empty() { return vec![]; }
    let mut hits = vec![];
    for e in WalkDir::new(root).into_iter().filter_entry(|e| !skip(e)).flatten() {
        if hits.len() >= limit { break; }
        if !e.file_type().is_file() || e.path().extension().map_or(true, |x| x != "md") { continue; }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        let mut lines = vec![];
        let name_hit = rel.to_lowercase().contains(&needle);
        for (i, l) in text.lines().enumerate() {
            if l.to_lowercase().contains(&needle) {
                lines.push(json!({"line": i + 1, "text": l.chars().take(200).collect::<String>()}));
                if lines.len() >= 3 { break; }
            }
        }
        if name_hit || !lines.is_empty() {
            hits.push(json!({"path": rel, "matches": lines, "nameHit": name_hit}));
        }
    }
    hits.sort_by_key(|h| !h["nameHit"].as_bool().unwrap_or(false));
    hits
}

pub fn stats(root: &Path) -> Value {
    let (mut files, mut bytes) = (0usize, 0u64);
    for e in WalkDir::new(root).into_iter().filter_entry(|e| !skip(e)).flatten() {
        if e.file_type().is_file() && e.path().extension().map_or(false, |x| x == "md") {
            files += 1;
            bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    json!({"markdownFiles": files, "bytes": bytes})
}

/// Copy the embedded starter kit into `dest` (must be empty or missing).
pub fn create_from_starter(dest: &Path) -> anyhow::Result<()> {
    static STARTER: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../starter");
    if dest.exists() && std::fs::read_dir(dest)?.next().is_some() {
        anyhow::bail!("{} is not empty — choose an empty folder or 'use existing library'", dest.display());
    }
    std::fs::create_dir_all(dest)?;
    STARTER.extract(dest)?;
    Ok(())
}
