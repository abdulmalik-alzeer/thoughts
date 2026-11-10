//! Read-side views over the markdown library: ideas, file tree, search, stats.

use crate::paths::safe_join;
use serde_json::{json, Value};
use std::path::Path;
use walkdir::WalkDir;

const SKIP: &[&str] = &[".git", "node_modules", ".thoughts-search", "target"];

fn skip(e: &walkdir::DirEntry) -> bool {
    e.file_name().to_str().map_or(false, |n| SKIP.contains(&n) || (e.depth() > 0 && denied_component(n)))
}

/// Names the file API never touches: git internals and config (`.git`, `.gitignore`, `.gitmodules`,
/// `.github`… — anything starting with `.git`), Claude Code project config (`.claude`) and MCP
/// server definitions (`.mcp.json`). Editing any of these could make the machine run programs.
/// Case-insensitive, and tolerant of the trailing dots/spaces some filesystems ignore.
fn denied_component(name: &str) -> bool {
    let n = name.trim_end_matches(['.', ' ']).to_lowercase();
    n.starts_with(".git") || n.starts_with("git~") || n == ".claude" || n == ".mcp.json"
        || name.to_lowercase().starts_with(".git")
}

/// Is library-relative `rel` off-limits for the file API?
pub fn denied(rel: &str) -> bool {
    rel.split(['/', '\\']).any(denied_component)
}

/// The one way a client-supplied path becomes a real path inside the library:
/// deny-list first, then `safe_join` (no `..`, no symlink escapes).
pub fn resolve(root: &Path, rel: &str) -> anyhow::Result<std::path::PathBuf> {
    if denied(rel) { anyhow::bail!("this file is protected and cannot be opened or changed from the app"); }
    safe_join(root, rel)
}

/// Like `resolve`, for paths handed to the AI: must also be plainly relative.
pub fn resolve_mention(root: &Path, rel: &str) -> anyhow::Result<std::path::PathBuf> {
    if rel.trim().is_empty() || rel.starts_with(['/', '\\', '~']) || Path::new(rel).is_absolute() || rel.contains(['\n', '\r']) {
        anyhow::bail!("invalid file reference");
    }
    resolve(root, rel)
}

fn rel_of(root: &Path, p: &Path) -> Option<String> {
    Some(p.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/"))
}

/// Turn an uploaded file name into a safe basename: last path component only, no control
/// characters, no leading dots (so never a dotfile), never empty.
pub fn sanitize_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().filter(|c| !c.is_control() && !"<>:\"|?*".contains(*c)).collect();
    let clean = clean.trim().trim_start_matches('.').trim_end_matches(['.', ' ']).trim();
    let clean: String = clean.chars().take(120).collect();
    if clean.is_empty() || denied_component(&clean) { "file".into() } else { clean }
}

pub const IMPORT_MAX_BYTES: usize = 25 * 1024 * 1024;

/// Save `bytes` as `<library>/imports/<name>` without ever overwriting: `name (2).ext`, `name (3).ext`…
/// Returns the library-relative path.
pub fn import_file(root: &Path, name: &str, bytes: &[u8]) -> anyhow::Result<String> {
    use std::io::Write;
    if bytes.len() > IMPORT_MAX_BYTES { anyhow::bail!("{name} is larger than 25 MB"); }
    let name = sanitize_file_name(name);
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    std::fs::create_dir_all(resolve(root, "imports")?)?;
    for n in 1..10_000 {
        let rel = if n == 1 { format!("imports/{name}") } else { format!("imports/{stem} ({n}){ext}") };
        let p = resolve(root, &rel)?;
        // create_new: fails rather than replaces if the name is taken (also refuses dangling symlinks).
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(bytes) { drop(f); let _ = std::fs::remove_file(&p); return Err(e.into()); }
                return Ok(rel);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::bail!("too many files named {name}")
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
        let Ok(rel) = e.path().strip_prefix(root) else { continue };
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

/// All markdown and output files, grouped client-side. Anything under `exclude`
/// (library-relative folders) is left out.
pub fn tree(root: &Path, exclude: &[String]) -> Vec<Value> {
    let mut out = vec![];
    for e in WalkDir::new(root).into_iter().filter_entry(|e| !skip(e)).flatten() {
        if !e.file_type().is_file() { continue; }
        let Some(rel) = rel_of(root, e.path()) else { continue };
        if crate::paths::is_under(&rel, exclude) { continue; }
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
    let p = resolve(root, rel)?;
    let text = std::fs::read_to_string(&p)?;
    let (fm, body) = frontmatter(&text);
    Ok(json!({"path": rel, "content": text, "frontmatter": fm, "body": body}))
}

pub fn write(root: &Path, rel: &str, content: &str) -> anyhow::Result<()> {
    let p = resolve(root, rel)?;
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    crate::paths::write_atomic(&p, content.as_bytes(), false)?;
    Ok(())
}

pub fn search(root: &Path, q: &str, limit: usize, exclude: &[String]) -> Vec<Value> {
    let needle = q.to_lowercase();
    if needle.trim().is_empty() { return vec![]; }
    let mut hits = vec![];
    for e in WalkDir::new(root).into_iter().filter_entry(|e| !skip(e)).flatten() {
        if hits.len() >= limit { break; }
        if !e.file_type().is_file() || e.path().extension().map_or(true, |x| x != "md") { continue; }
        let Some(rel) = rel_of(root, e.path()) else { continue };
        if crate::paths::is_under(&rel, exclude) { continue; }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-lib-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn protected_paths_are_denied() {
        for bad in [".git/config", ".GIT/hooks/pre-commit", "sub/.git/config", ".gitignore", ".gitmodules", ".github/workflows/x.yml",
                    ".claude/settings.json", ".Claude/settings.local.json", "a/.claude/skills/x/SKILL.md", ".mcp.json", ".MCP.JSON",
                    ".git./config", ".git /config", "x\\.git\\config", "GIT~1/config"] {
            assert!(denied(bad), "{bad} should be denied");
        }
        for ok in ["README.md", "apps/idea/notes/git.md", "docs/claude.md", "me/.notes.md", "mcp.json", "legit/file.md"] {
            assert!(!denied(ok), "{ok} should be allowed");
        }
    }

    #[test]
    fn file_api_refuses_protected_and_escaping_paths() {
        let d = tmp();
        std::fs::create_dir_all(d.join(".claude")).unwrap();
        std::fs::write(d.join(".claude/settings.json"), "{}").unwrap();
        assert!(read(&d, ".claude/settings.json").is_err());
        assert!(write(&d, ".claude/settings.json", "{\"hooks\":{}}").is_err());
        assert!(write(&d, ".git/hooks/pre-commit", "#!/bin/sh").is_err());
        assert!(write(&d, ".mcp.json", "{}").is_err());
        assert!(write(&d, "../escape.md", "x").is_err());
        assert_eq!(std::fs::read_to_string(d.join(".claude/settings.json")).unwrap(), "{}");
        write(&d, "notes/a.md", "hello").unwrap();
        assert_eq!(read(&d, "notes/a.md").unwrap()["content"], "hello");
        assert!(resolve_mention(&d, "notes/a.md").is_ok());
        for bad in ["/etc/passwd", "../x", "notes/../../x", "~/x", ".git/config", ""] { assert!(resolve_mention(&d, bad).is_err(), "{bad}"); }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tree_and_search_leave_out_private_and_protected() {
        let d = tmp();
        write(&d, "me/finances/tax.md", "secret salary").unwrap();
        write(&d, "ideas/a.md", "salary idea").unwrap();
        std::fs::create_dir_all(d.join(".claude")).unwrap();
        std::fs::write(d.join(".claude/x.md"), "salary").unwrap();
        let private = vec!["me/finances".to_string()];
        let paths = |v: Vec<Value>| v.iter().map(|x| x["path"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(paths(tree(&d, &private)), ["ideas/a.md"]);
        assert_eq!(paths(search(&d, "salary", 10, &private)), ["ideas/a.md"]);
        assert_eq!(tree(&d, &[]).len(), 2);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn import_names_are_sanitised() {
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("C:\\Users\\me\\report.pdf"), "report.pdf");
        assert_eq!(sanitize_file_name(".gitignore"), "gitignore");
        assert_eq!(sanitize_file_name("..."), "file");
        assert_eq!(sanitize_file_name(""), "file");
        assert_eq!(sanitize_file_name("a\nb<c>.txt"), "abc.txt");
        assert_eq!(sanitize_file_name("تقرير الربع.pdf"), "تقرير الربع.pdf");
    }

    #[test]
    fn import_never_overwrites() {
        let d = tmp();
        assert_eq!(import_file(&d, "/tmp/x/report.pdf", b"1").unwrap(), "imports/report.pdf");
        assert_eq!(import_file(&d, "report.pdf", b"2").unwrap(), "imports/report (2).pdf");
        assert_eq!(import_file(&d, "report.pdf", b"3").unwrap(), "imports/report (3).pdf");
        assert_eq!(import_file(&d, "notes", b"a").unwrap(), "imports/notes");
        assert_eq!(import_file(&d, "notes", b"b").unwrap(), "imports/notes (2)");
        assert_eq!(std::fs::read(d.join("imports/report.pdf")).unwrap(), b"1");
        assert!(import_file(&d, "big.bin", &vec![0u8; IMPORT_MAX_BYTES + 1]).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }
}
