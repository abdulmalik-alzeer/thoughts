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
    if denied(rel) { return Err(crate::tr::err("protected", &[])); }
    safe_join(root, rel).map_err(|e| if e.downcast_ref::<std::io::Error>().is_some() { e } else { crate::tr::err("invalid.path", &[]) })
}

/// A path the user (or an add-on) wants to create: every part a sensible file or folder name.
fn check_new_path(rel: &str) -> anyhow::Result<()> {
    let bad = || crate::tr::err("invalid.path", &[]);
    let rel = rel.trim();
    if rel.is_empty() || rel.len() > 400 || rel.starts_with(['/', '\\', '~']) || Path::new(rel).is_absolute() { return Err(bad()); }
    for part in rel.split(['/', '\\']) {
        let ok = !part.is_empty() && part != "." && part != ".." && part.len() <= 200
            && !part.ends_with(['.', ' ']) && !part.starts_with(' ')
            && !part.chars().any(|c| c.is_control() || "<>:\"|?*".contains(c));
        if !ok { return Err(bad()); }
    }
    Ok(())
}

/// Create a new file; never replaces one that is there. Returns the content's hash.
pub fn create(root: &Path, rel: &str, content: &str) -> anyhow::Result<String> {
    use std::io::Write;
    check_new_path(rel)?;
    let p = resolve(root, rel)?;
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    // create_new: fails rather than replaces if the name is taken (and refuses dangling symlinks).
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(content.as_bytes()) { drop(f); let _ = std::fs::remove_file(&p); return Err(e.into()); }
            Ok(content_hash(content.as_bytes()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(crate::tr::err("exists", &[("path", rel)])),
        Err(e) => Err(e.into()),
    }
}

/// Move or rename a file or folder inside the library. Never replaces something that is there.
pub fn rename(root: &Path, from: &str, to: &str) -> anyhow::Result<()> {
    check_new_path(to)?;
    let (src, dst) = (resolve(root, from)?, resolve(root, to)?);
    if from.trim().trim_matches(['/', '\\']).is_empty() { return Err(crate::tr::err("invalid.path", &[])); }
    let meta = std::fs::symlink_metadata(&src).map_err(|_| crate::tr::err("not_found.file", &[]))?;
    // A folder cannot be moved into itself.
    if meta.is_dir() && crate::paths::inside_ci(&dst, &src) && crate::paths::canonical_lenient(&dst) != crate::paths::canonical_lenient(&src) {
        return Err(crate::tr::err("invalid.path", &[]));
    }
    // Changing only the letter case of a name is a rename, not a clash with itself.
    let same_file = dst.exists() && crate::paths::canonical(&dst).ok() == crate::paths::canonical(&src).ok();
    if std::fs::symlink_metadata(&dst).is_ok() && !same_file { return Err(crate::tr::err("exists", &[("path", to)])); }
    if let Some(parent) = dst.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::rename(&src, &dst)?;
    Ok(())
}

/// Delete a file, or a folder that is already empty. (With history on, the caller records the
/// library first, so a deleted file can be brought back.)
pub fn remove(root: &Path, rel: &str) -> anyhow::Result<()> {
    if rel.trim().trim_matches(['/', '\\']).is_empty() { return Err(crate::tr::err("invalid.path", &[])); }
    let p = resolve(root, rel)?;
    let meta = std::fs::symlink_metadata(&p).map_err(|_| crate::tr::err("not_found.file", &[]))?;
    if meta.is_dir() {
        if std::fs::read_dir(&p)?.next().is_some() { return Err(crate::tr::err("folder.not_empty", &[])); }
        std::fs::remove_dir(&p)?;
    } else {
        std::fs::remove_file(&p)?;
    }
    Ok(())
}

/// Set `status:` (and today's date as `updated:`) in the note details at the top of an idea's
/// README, leaving every other line — and the file's line endings — exactly as they were.
pub fn set_idea_status(root: &Path, rel: &str, status: &str, today: chrono::NaiveDate) -> anyhow::Result<()> {
    let status = status.trim();
    let ok = (1..=30).contains(&status.chars().count()) && status.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ' ');
    if !ok { anyhow::bail!("{}", if crate::tr::arabic() { "هذه ليست حالة صالحة." } else { "That is not a valid status." }); }
    let p = resolve(root, rel)?;
    let text = std::fs::read_to_string(&p)?;
    let mut lines: Vec<String> = text.split_inclusive('\n').map(String::from).collect();
    let end_of = |l: &str| if l.ends_with("\r\n") { "\r\n" } else { "\n" };
    let is_fence = |l: &str| l.trim_end_matches(['\r', '\n']) == "---";
    let close = if lines.first().map_or(false, |l| is_fence(l)) { lines.iter().skip(1).position(|l| is_fence(l)).map(|i| i + 1) } else { None };
    let Some(close) = close else { return Err(crate::tr::err("not_found.file", &[])) };
    let eol = end_of(&lines[0]).to_string();
    let set = |lines: &mut Vec<String>, close: &mut usize, key: &str, value: &str, add: bool| {
        match lines[1..*close].iter().position(|l| l.starts_with(&format!("{key}:"))) {
            Some(i) => lines[i + 1] = format!("{key}: {value}{eol}"),
            None if add => { lines.insert(*close, format!("{key}: {value}{eol}")); *close += 1; }
            None => {}
        }
    };
    let mut close = close;
    set(&mut lines, &mut close, "status", status, true);
    set(&mut lines, &mut close, "updated", &today.format("%Y-%m-%d").to_string(), false);
    crate::paths::write_atomic(&p, lines.concat().as_bytes(), false)?;
    Ok(())
}

/// File types an add-on may write.
pub const PLUGIN_WRITE_EXT: &[&str] = &["md", "json", "csv", "txt"];

/// May an add-on write (or delete) library file `rel`? Only a text file, and only inside one of
/// the library-relative folders it declared (`allow`) — judged on real paths, whatever the spelling.
pub fn check_plugin_write(root: &Path, rel: &str, allow: &[String]) -> anyhow::Result<()> {
    let ext = rel.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    if !PLUGIN_WRITE_EXT.contains(&ext.as_str()) { return Err(crate::tr::err("plugin.file_type", &[])); }
    let target = resolve(root, rel)?;
    let inside = allow.iter().filter_map(|a| crate::paths::clean_rel(a).ok())
        .any(|a| { let dir = root.join(&a); crate::paths::inside_ci(&target, &dir) && crate::paths::canonical_lenient(&target) != crate::paths::canonical_lenient(&dir) });
    if !inside { return Err(crate::tr::err("plugin.not_allowed", &[("path", rel)])); }
    Ok(())
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
    let name = sanitize_file_name(name);
    if bytes.len() > IMPORT_MAX_BYTES { return Err(crate::tr::err("too_large", &[("name", &name)])); }
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

/// Note details for showing an item: strict YAML first; when the block is not valid YAML (an
/// unquoted "title: Client: Acme", a value starting with `*` or `@`…) a line-by-line `key: value`
/// reading, which is all the item list needs. A leading byte-order mark is ignored. `None` when
/// the file has no note details at all; the flag says the lenient reading was needed.
pub fn note_details(text: &str) -> Option<(Value, bool)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (fm, _) = frontmatter(text);
    if !fm.is_null() { return Some((fm, false)); }
    let rest = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n"))?;
    let yaml = &rest[..rest.find("\n---")?];
    Some((lenient_details(yaml), true))
}

/// Flat `key: value` lines, `key: [a, b]` and `key:` followed by `- item` lines. Values are text.
fn lenient_details(yaml: &str) -> Value {
    let unquote = |v: &str| -> String {
        let v = v.trim();
        for q in ['"', '\''] {
            if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) { return v[1..v.len() - 1].to_string(); }
        }
        v.to_string()
    };
    let mut out = serde_json::Map::new();
    let mut list_key: Option<String> = None;
    for raw in yaml.lines() {
        let l = raw.trim_end_matches('\r');
        if l.trim().is_empty() || l.trim_start().starts_with('#') { continue; }
        if let (Some(k), Some(item)) = (&list_key, l.trim_start().strip_prefix("- ").or_else(|| (l.trim() == "-").then_some(""))) {
            if let Some(Value::Array(a)) = out.get_mut(k) { a.push(Value::String(unquote(item))); }
            continue;
        }
        list_key = None;
        if l.starts_with([' ', '\t']) { continue; }
        let Some((k, v)) = l.split_once(':') else { continue };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') { continue; }
        let v = v.trim();
        let value = if v.is_empty() {
            list_key = Some(k.to_string());
            Value::Array(vec![])
        } else if v.starts_with('[') && v.ends_with(']') {
            Value::Array(v[1..v.len() - 1].split(',').map(unquote).filter(|x| !x.is_empty()).map(Value::String).collect())
        } else {
            Value::String(unquote(v))
        };
        out.entry(k.to_string()).or_insert(value);
    }
    // A key left with an empty list and no items was just empty.
    for v in out.values_mut() { if matches!(v, Value::Array(a) if a.is_empty()) { *v = Value::Null; } }
    if let Some(t) = out.get_mut("tags") { if t.is_null() { *t = json!([]); } }
    Value::Object(out)
}

/// Every `<area>/<slug>/README.md` with note details.
pub fn ideas(root: &Path) -> Vec<Value> {
    let mut out = vec![];
    for e in WalkDir::new(root).min_depth(3).max_depth(3).into_iter().filter_entry(|e| !skip(e)).flatten() {
        if e.file_name() != "README.md" { continue; }
        let Ok(rel) = e.path().strip_prefix(root) else { continue };
        let parts: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        if parts.len() != 3 || parts[0].starts_with('.') || parts[0] == "me" || parts[0] == "inbox" { continue; }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let Some((fm, lenient)) = note_details(&text) else { continue };
        let s = |k: &str| fm[k].as_str().map(String::from).unwrap_or_default();
        // The details were not valid and nothing usable could be read from them: still listed, flagged.
        let unreadable = lenient && s("title").is_empty() && s("status").is_empty() && s("summary").is_empty();
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
            "kind": s("kind"),
            "detailsUnreadable": unreadable,
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

/// Content fingerprint the editor sends back when saving, so a save never silently overwrites
/// a file that changed on disk in the meantime.
pub fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

pub fn read(root: &Path, rel: &str) -> anyhow::Result<Value> {
    let p = resolve(root, rel)?;
    let text = std::fs::read_to_string(&p)?;
    let (fm, body) = frontmatter(&text);
    Ok(json!({"path": rel, "content": text, "frontmatter": fm, "body": body, "hash": content_hash(text.as_bytes())}))
}

pub fn write(root: &Path, rel: &str, content: &str) -> anyhow::Result<()> {
    write_checked(root, rel, content, None).map(|_| ())
}

/// Save a file atomically. With `base_hash` (the `hash` the editor got when it loaded the file)
/// the save fails with a `STALE::` error if the file on disk is no longer that version.
/// Returns the new content's hash.
pub fn write_checked(root: &Path, rel: &str, content: &str, base_hash: Option<&str>) -> anyhow::Result<String> {
    let p = resolve(root, rel)?;
    if let Some(base) = base_hash.filter(|b| !b.is_empty()) {
        let current = match std::fs::read(&p) {
            Ok(b) => Some(content_hash(&b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        match current.as_deref() {
            Some(h) if h == base => {}
            Some(_) => return Err(crate::tr::err("stale.changed", &[])),
            None => return Err(crate::tr::err("stale.gone", &[])),
        }
    }
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    crate::paths::write_atomic(&p, content.as_bytes(), false)?;
    Ok(content_hash(content.as_bytes()))
}

/// Largest file `read_raw` serves.
pub const RAW_MAX_BYTES: u64 = 25 * 1024 * 1024;

/// Content type for a file name. Anything a browser could run as a page (HTML, XML) is served as text.
pub fn mime_for(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).as_deref() {
        Some("png") => "image/png", Some("jpg" | "jpeg") => "image/jpeg", Some("gif") => "image/gif",
        Some("webp") => "image/webp", Some("svg") => "image/svg+xml", Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon", Some("avif") => "image/avif", Some("heic") => "image/heic",
        Some("pdf") => "application/pdf", Some("json") => "application/json",
        Some("csv") => "text/csv; charset=utf-8", Some("md" | "markdown") => "text/markdown; charset=utf-8",
        Some("txt" | "log" | "html" | "htm" | "xml" | "yaml" | "yml" | "toml" | "js" | "ts" | "css") => "text/plain; charset=utf-8",
        Some("mp3") => "audio/mpeg", Some("m4a") => "audio/mp4", Some("wav") => "audio/wav", Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4", Some("mov") => "video/quicktime", Some("webm") => "video/webm",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

/// A library file's bytes and content type. Same path rules as `read` (deny-list, no escapes).
pub fn read_raw(root: &Path, rel: &str) -> anyhow::Result<(Vec<u8>, &'static str)> {
    let p = resolve(root, rel)?;
    let meta = std::fs::metadata(&p)?;
    if !meta.is_file() { return Err(crate::tr::err("not_found.file", &[])); }
    if meta.len() > RAW_MAX_BYTES { return Err(crate::tr::err("too_large.show", &[])); }
    Ok((std::fs::read(&p)?, mime_for(rel)))
}

/// After a new library has been set up: the starter's root `presets/` folder is only kept when it
/// has not already been installed under `.claude/presets/`.
pub fn remove_leftover_presets(root: &Path) -> bool {
    let (left, installed) = (root.join("presets"), root.join(".claude/presets"));
    if !installed.is_dir() || !left.is_dir() || std::fs::symlink_metadata(&left).map_or(true, |m| m.file_type().is_symlink()) { return false; }
    match std::fs::remove_dir_all(&left) {
        Ok(()) => true,
        Err(e) => { crate::log::warn(format!("could not remove the leftover presets folder: {e}")); false }
    }
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

/// The library's own machinery rather than the user's notes: the rules (`CLAUDE.md` anywhere), the
/// index and the starter's welcome pages at the top, and a leftover `presets/` folder.
fn system_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name == "CLAUDE.md" || rel == "INDEX.md" || (!rel.contains('/') && name.starts_with("START-HERE")) || rel.starts_with("presets/")
}

/// `markdownFiles` counts every markdown file (what search sizing needs); `notes` leaves out the
/// library's system files, for showing the user how many notes they have.
pub fn stats(root: &Path) -> Value {
    let (mut files, mut notes, mut bytes) = (0usize, 0usize, 0u64);
    for e in WalkDir::new(root).into_iter().filter_entry(|e| !skip(e)).flatten() {
        if e.file_type().is_file() && e.path().extension().map_or(false, |x| x == "md") {
            files += 1;
            bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
            if rel_of(root, e.path()).map_or(false, |r| !system_file(&r)) { notes += 1; }
        }
    }
    json!({"markdownFiles": files, "notes": notes, "bytes": bytes})
}

/// The starter library kit, embedded in the program.
pub static STARTER: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../starter");

/// Copy the embedded starter kit into `dest` (must be empty or missing).
pub fn create_from_starter(dest: &Path) -> anyhow::Result<()> {
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
    fn items_with_loose_note_details_are_still_listed() {
        let d = tmp();
        for (slug, head) in [
            ("acme", "title: Client: Acme Co\nstatus: active\nsummary: Things to do: call bank\ntags: [vat, monthly]\nupdated: 2026-11-02"),
            ("star", "title: *Star* project\nstatus: seed\ntags:\n  - a\n  - \"b\"\nupdated: '2026-11-01'"),
            ("ar", "title: عميل: شركة الأفق\nstatus: active\nkind: client"),
            ("junk", "{{{ not details at all"),
        ] {
            std::fs::create_dir_all(d.join("clients").join(slug)).unwrap();
            std::fs::write(d.join("clients").join(slug).join("README.md"), format!("---\n{head}\n---\n# x\n")).unwrap();
        }
        // A byte-order mark in front of valid details.
        std::fs::create_dir_all(d.join("ideas/bom")).unwrap();
        std::fs::write(d.join("ideas/bom/README.md"), "\u{feff}---\ntitle: With mark\nstatus: seed\n---\n").unwrap();
        // No details at all: not an item.
        std::fs::create_dir_all(d.join("ideas/plain")).unwrap();
        std::fs::write(d.join("ideas/plain/README.md"), "# Just a page\n").unwrap();
        let all = ideas(&d);
        let by = |slug: &str| all.iter().find(|i| i["slug"] == slug).cloned().unwrap_or(Value::Null);
        assert_eq!(all.len(), 5, "{all:?}");
        assert_eq!(by("acme")["title"], "Client: Acme Co");
        assert_eq!(by("acme")["summary"], "Things to do: call bank");
        assert_eq!(by("acme")["tags"], json!(["vat", "monthly"]));
        assert_eq!(by("acme")["updated"], "2026-11-02");
        assert_eq!(by("acme")["detailsUnreadable"], false);
        assert_eq!(by("star")["title"], "*Star* project");
        assert_eq!(by("star")["tags"], json!(["a", "b"]));
        assert_eq!(by("star")["updated"], "2026-11-01");
        assert_eq!(by("ar")["title"], "عميل: شركة الأفق");
        assert_eq!(by("ar")["kind"], "client");
        assert_eq!(by("junk")["title"], "junk");
        assert_eq!(by("junk")["detailsUnreadable"], true);
        assert_eq!(by("bom")["title"], "With mark");
        // The strict reader used elsewhere (the trust check) is unchanged.
        assert!(frontmatter("---\ntitle: Client: Acme\n---\n").0.is_null());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_note_count_leaves_out_system_files() {
        let d = tmp();
        for f in ["CLAUDE.md", "INDEX.md", "START-HERE.md", "START-HERE.ar.md", "presets/legal.md", "TODO.md", "ideas/x/README.md", "ideas/x/CLAUDE.md", "ideas/x/notes/START-HERE.md"] {
            if let Some(p) = d.join(f).parent() { std::fs::create_dir_all(p).unwrap(); }
            std::fs::write(d.join(f), "x").unwrap();
        }
        let s = stats(&d);
        assert_eq!(s["markdownFiles"], 9);
        assert_eq!(s["notes"], 3);
        std::fs::remove_dir_all(d).unwrap();
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
    fn stale_saves_are_refused() {
        let d = tmp();
        write(&d, "notes/a.md", "v1").unwrap();
        let loaded = read(&d, "notes/a.md").unwrap()["hash"].as_str().unwrap().to_string();
        assert_eq!(loaded, content_hash(b"v1"));
        // Saving what was loaded works and gives the next base.
        let next = write_checked(&d, "notes/a.md", "v2", Some(&loaded)).unwrap();
        assert_eq!(next, content_hash(b"v2"));
        // Something else (the assistant, a sync) changes the file: the old base is refused.
        std::fs::write(d.join("notes/a.md"), "changed elsewhere").unwrap();
        let err = write_checked(&d, "notes/a.md", "v3", Some(&next)).unwrap_err().to_string();
        assert!(err.starts_with("STALE::"), "{err}");
        assert_eq!(std::fs::read_to_string(d.join("notes/a.md")).unwrap(), "changed elsewhere");
        // Deleted underneath the editor.
        std::fs::remove_file(d.join("notes/a.md")).unwrap();
        assert!(write_checked(&d, "notes/a.md", "v3", Some(&next)).unwrap_err().to_string().starts_with("STALE::"));
        // No base hash: plain save, as before (new files, older clients).
        write_checked(&d, "notes/a.md", "v4", None).unwrap();
        write_checked(&d, "notes/b.md", "new", Some("")).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("notes/a.md")).unwrap(), "v4");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn files_can_be_created_moved_and_deleted_but_never_overwritten() {
        let d = tmp();
        let hash = create(&d, "clients/acme/notes.md", "# Acme\n").unwrap();
        assert_eq!(hash, content_hash(b"# Acme\n"));
        assert!(create(&d, "clients/acme/notes.md", "again").is_err());
        assert_eq!(std::fs::read_to_string(d.join("clients/acme/notes.md")).unwrap(), "# Acme\n");
        for bad in ["", " ", "/abs.md", "../out.md", "a/../b.md", "a//b.md", ".git/x", ".claude/skills/x/SKILL.md", ".mcp.json", "a/b?.md", "a/trailing. ", "x\ny.md"] {
            assert!(create(&d, bad, "x").is_err(), "{bad:?}");
            assert!(rename(&d, "clients/acme/notes.md", bad).is_err(), "{bad:?}");
        }
        // Move a file, then its folder; names in use are refused.
        rename(&d, "clients/acme/notes.md", "clients/acme/overview.md").unwrap();
        create(&d, "clients/beta/overview.md", "beta").unwrap();
        assert!(rename(&d, "clients/acme/overview.md", "clients/beta/overview.md").is_err());
        assert!(rename(&d, "clients/acme", "clients/beta").is_err());
        assert!(rename(&d, "clients/acme", "clients/acme/inner").is_err());
        assert!(rename(&d, "clients/missing.md", "x.md").is_err());
        assert!(rename(&d, ".git/config", "x.md").is_err());
        rename(&d, "clients/acme", "archive/2026/acme").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("archive/2026/acme/overview.md")).unwrap(), "# Acme\n");
        // A change of letter case only is a rename, on any volume.
        rename(&d, "clients/beta/overview.md", "clients/beta/Overview.md").unwrap();
        assert!(std::fs::read_dir(d.join("clients/beta")).unwrap().flatten().any(|e| e.file_name() == "Overview.md"));
        // Delete: files, and folders only once they are empty.
        assert!(remove(&d, "archive/2026/acme").is_err());
        remove(&d, "archive/2026/acme/overview.md").unwrap();
        remove(&d, "archive/2026/acme").unwrap();
        assert!(!d.join("archive/2026/acme").exists());
        for bad in ["", "/", "nope.md", ".git", ".claude/settings.json", "../x"] { assert!(remove(&d, bad).is_err(), "{bad:?}"); }
        assert!(d.exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn add_ons_write_only_text_files_inside_their_folders() {
        let d = tmp();
        std::fs::create_dir_all(d.join("Billing/2026")).unwrap();
        let allow = vec!["billing/".to_string(), "data".to_string()];
        for ok in ["billing/hours.csv", "Billing/2026/jan.json", "BILLING/notes.MD", "data/new/log.txt"] {
            assert!(check_plugin_write(&d, ok, &allow).is_ok(), "{ok}");
        }
        for bad in ["hours.csv", "billing", "billing-old/x.md", "billing/run.sh", "billing/page.html", "billing/noext", "billing/../x.md",
                    "me/profile.md", ".claude/x.md", "data/.git/x.md"] {
            assert!(check_plugin_write(&d, bad, &allow).is_err(), "{bad}");
        }
        // Nothing declared, or a declaration that is not a plain folder: nothing may be written.
        assert!(check_plugin_write(&d, "billing/hours.csv", &[]).is_err());
        assert!(check_plugin_write(&d, "billing/hours.csv", &["../".to_string(), "/".to_string(), "".to_string()]).is_err());
        #[cfg(unix)]
        {
            // A link out of the allowed folder does not widen it.
            std::fs::create_dir_all(d.join("me")).unwrap();
            std::os::unix::fs::symlink(d.join("me"), d.join("Billing/alias")).unwrap();
            assert!(check_plugin_write(&d, "Billing/alias/profile.md", &allow).is_err());
            // The lower-case spelling reaches the same link only where names ignore case (macOS);
            // on a case-sensitive disk it is simply a new, separate folder inside the allowed one.
            #[cfg(target_os = "macos")]
            assert!(check_plugin_write(&d, "billing/alias/profile.md", &allow).is_err());
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn an_ideas_status_changes_in_place() {
        let d = tmp();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        write(&d, "apps/x/README.md", "---\ntitle: X\nstatus: seed\nupdated: 2026-01-01\n---\n\n## What & why\nstatus: not this one\n").unwrap();
        set_idea_status(&d, "apps/x/README.md", "active", day).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("apps/x/README.md")).unwrap(),
            "---\ntitle: X\nstatus: active\nupdated: 2026-11-10\n---\n\n## What & why\nstatus: not this one\n");
        // No status yet: it is added; Windows line endings are kept.
        write(&d, "apps/y/README.md", "---\r\ntitle: Y\r\n---\r\nBody\r\n").unwrap();
        set_idea_status(&d, "apps/y/README.md", "parked", day).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("apps/y/README.md")).unwrap(), "---\r\ntitle: Y\r\nstatus: parked\r\n---\r\nBody\r\n");
        // Not an idea file, a bad status, a protected path: nothing changes.
        write(&d, "notes.md", "plain\n").unwrap();
        assert!(set_idea_status(&d, "notes.md", "active", day).is_err());
        for bad in ["", "a\nb: c", "x: y", &"z".repeat(40)] { assert!(set_idea_status(&d, "apps/x/README.md", bad, day).is_err(), "{bad:?}"); }
        assert!(set_idea_status(&d, ".claude/x/README.md", "active", day).is_err());
        assert_eq!(std::fs::read_to_string(d.join("notes.md")).unwrap(), "plain\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn raw_files_follow_the_same_path_rules() {
        let d = tmp();
        std::fs::create_dir_all(d.join("imports")).unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("imports/scan.PNG"), [0x89, b'P', b'N', b'G']).unwrap();
        std::fs::write(d.join(".git/config"), "x").unwrap();
        let (bytes, mime) = read_raw(&d, "imports/scan.PNG").unwrap();
        assert_eq!((bytes.as_slice(), mime), (&[0x89, b'P', b'N', b'G'][..], "image/png"));
        assert!(read_raw(&d, ".git/config").is_err());
        assert!(read_raw(&d, "../outside.png").is_err());
        assert!(read_raw(&d, "imports").is_err());
        assert!(read_raw(&d, "imports/missing.pdf").is_err());
        assert_eq!(mime_for("report.pdf"), "application/pdf");
        assert_eq!(mime_for("page.html"), "text/plain; charset=utf-8");
        assert_eq!(mime_for("no-extension"), "application/octet-stream");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn leftover_presets_are_removed_only_when_installed() {
        let d = tmp();
        std::fs::create_dir_all(d.join("presets/accountant")).unwrap();
        assert!(!remove_leftover_presets(&d));
        assert!(d.join("presets").exists());
        std::fs::create_dir_all(d.join(".claude/presets")).unwrap();
        assert!(remove_leftover_presets(&d));
        assert!(!d.join("presets").exists() && d.join(".claude/presets").exists());
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
