//! Read-side views over the markdown library: ideas, file tree, search, stats.

use crate::paths::safe_join;
use serde_json::{json, Value};
use std::path::Path;
use walkdir::WalkDir;

const SKIP: &[&str] = &[".git", "node_modules", ".thoughts-search", "target"];

pub(crate) fn skip(e: &walkdir::DirEntry) -> bool {
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

/// Is `p` (a path `safe_join` placed inside `root`) really in a protected folder, whatever name it
/// was reached by? A link inside the library that leads to `.claude` or `.git`, or a short Windows
/// name such as `CLAUDE~1`, spells none of the protected names — the real location does.
fn denied_real(root: &Path, p: &Path) -> bool {
    let (real_root, real) = (crate::paths::canonical_lenient(root), crate::paths::canonical_lenient(p));
    // `safe_join` has already kept every link inside the library, so the real path is under the
    // real root; should it ever not be, that is refused too.
    real.strip_prefix(&real_root).map_or(true, |rest| denied(&rest.to_string_lossy()))
}

/// The one way a client-supplied path becomes a real path inside the library: deny-list on the
/// name as written, `safe_join` (no `..`, no symlink escapes), then the deny-list again on where
/// the path really leads.
pub fn resolve(root: &Path, rel: &str) -> anyhow::Result<std::path::PathBuf> {
    if denied(rel) { return Err(crate::tr::err("protected", &[])); }
    let p = safe_join(root, rel).map_err(|e| if e.downcast_ref::<std::io::Error>().is_some() { e } else { crate::tr::err("invalid.path", &[]) })?;
    if denied_real(root, &p) { return Err(crate::tr::err("protected", &[])); }
    Ok(p)
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

/// Delete a file, or a folder with everything in it (the user confirmed that). A folder holding
/// something the file API never touches (`.git`, `.claude`, …) is refused as a whole, so nothing
/// protected goes with it; symlinks inside are removed as links, never followed. Never the library
/// itself. (With history on, the caller records the library first, as for `remove`.)
pub fn remove_all(root: &Path, rel: &str) -> anyhow::Result<()> {
    if rel.trim().trim_matches(['/', '\\']).is_empty() { return Err(crate::tr::err("invalid.path", &[])); }
    let p = resolve(root, rel)?;
    let meta = std::fs::symlink_metadata(&p).map_err(|_| crate::tr::err("not_found.file", &[]))?;
    if !meta.is_dir() { return remove(root, rel); }
    if crate::paths::canonical_lenient(&p) == crate::paths::canonical_lenient(root) { return Err(crate::tr::err("invalid.path", &[])); }
    for e in WalkDir::new(&p).min_depth(1).follow_links(false) {
        let e = e?;
        if e.file_name().to_str().map_or(true, denied_component) { return Err(crate::tr::err("folder.protected", &[])); }
    }
    std::fs::remove_dir_all(&p)?;
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

/// Files Claude Code reads as standing instructions as soon as the assistant works in their folder,
/// at any depth: `CLAUDE.md`, `CLAUDE.local.md`, and `AGENTS.md` where there is no `CLAUDE.md`.
/// Whatever the letter case, and without the trailing dots and spaces some filesystems ignore.
fn instruction_file(name: &str) -> bool {
    matches!(name.trim_end_matches(['.', ' ']).to_lowercase().as_str(), "claude.md" | "claude.local.md" | "agents.md")
}

/// May an add-on write (or delete) library file `rel`? Only a text file, and only inside one of
/// the library-relative folders it declared (`allow`) — judged on real paths, whatever the spelling.
pub fn check_plugin_write(root: &Path, rel: &str, allow: &[String]) -> anyhow::Result<()> {
    let ext = rel.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    if !PLUGIN_WRITE_EXT.contains(&ext.as_str()) { return Err(crate::tr::err("plugin.file_type", &[])); }
    let target = resolve(root, rel)?;
    // An add-on's own notes must never become instructions for the assistant: not under that name,
    // and not through a link that leads to such a file.
    let named = |p: &Path| p.file_name().map_or(false, |n| instruction_file(&n.to_string_lossy()));
    if named(&target) || named(&crate::paths::canonical_lenient(&target)) { return Err(crate::tr::err("plugin.instructions", &[])); }
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

/// Every file the file API may touch, as (library-relative path, real path): the same folders the
/// file list walks (no `.git`, no protected names, no build folders).
pub fn walk_files(root: &Path) -> Vec<(String, std::path::PathBuf)> {
    WalkDir::new(root).into_iter().filter_entry(|e| !skip(e)).flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| Some((rel_of(root, e.path())?, e.into_path())))
        .collect()
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
    let name = sanitize_file_name(name);
    if bytes.len() > IMPORT_MAX_BYTES { return Err(crate::tr::err("too_large", &[("name", &name)])); }
    save_numbered(root, "imports", &name, bytes)
}

/// Document types the app itself makes from a note (see `save_output`).
pub const OUTPUT_EXT: &[&str] = &["docx"];

/// Save a document the app generated (a note exported as Word) as `<folder>/<name>` in the library,
/// never overwriting: `name (2).docx`… Only the types in `OUTPUT_EXT`. Returns the library-relative path.
pub fn save_output(root: &Path, folder: &str, name: &str, bytes: &[u8]) -> anyhow::Result<String> {
    let name = sanitize_file_name(name);
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    if !OUTPUT_EXT.contains(&ext.as_str()) { return Err(crate::tr::err("invalid.path", &[])); }
    if bytes.len() > IMPORT_MAX_BYTES { return Err(crate::tr::err("too_large", &[("name", &name)])); }
    let folder = folder.trim().replace('\\', "/");
    let folder = folder.trim_end_matches('/');
    if folder.is_empty() { return save_numbered(root, "", &name, bytes); }
    check_new_path(folder)?;
    save_numbered(root, folder, &name, bytes)
}

/// Write `bytes` as `<folder>/<name>`, or `name (2).ext`, `name (3).ext`… when that is taken.
fn save_numbered(root: &Path, folder: &str, name: &str, bytes: &[u8]) -> anyhow::Result<String> {
    use std::io::Write;
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    let at = |n: &str| if folder.is_empty() { n.to_string() } else { format!("{folder}/{n}") };
    if !folder.is_empty() { std::fs::create_dir_all(resolve(root, folder)?)?; }
    for n in 1..10_000 {
        let rel = if n == 1 { at(name) } else { at(&format!("{stem} ({n}){ext}")) };
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

/// Folders left out of a listing (the user's private folders), recognised by where they really are
/// as well as by name: the same folder under another spelling — letter case, an accent or hamza
/// stored in its other Unicode form, a link that leads to it — is left out just the same.
pub(crate) struct Hidden { real: Vec<String> }

impl Hidden {
    fn norm(p: &Path) -> String { crate::paths::canonical_lenient(p).to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase() }

    pub(crate) fn new(root: &Path, dirs: &[String]) -> Self { Hidden { real: dirs.iter().map(|d| Self::norm(&root.join(d))).collect() } }

    /// Is this folder of a walk one of the hidden ones (or inside one)? Asked once per folder, so
    /// the walk never goes into it; files are not looked up one by one.
    pub(crate) fn folder(&self, e: &walkdir::DirEntry) -> bool {
        if self.real.is_empty() || !e.file_type().is_dir() { return false; }
        let p = Self::norm(e.path());
        self.real.iter().any(|d| p == *d || p.starts_with(&format!("{d}/")))
    }
}

/// All markdown and output files, grouped client-side. Anything under `exclude`
/// (library-relative folders) is left out.
pub fn tree(root: &Path, exclude: &[String]) -> Vec<Value> {
    let mut out = vec![];
    let hidden = Hidden::new(root, exclude);
    for e in WalkDir::new(root).into_iter().filter_entry(|e| !skip(e) && !hidden.folder(e)).flatten() {
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

/// Library files containing every word of `q` (see `crate::search`), best first.
pub fn search(root: &Path, q: &str, limit: usize, exclude: &[String]) -> Vec<Value> {
    crate::search::notes(root, &crate::search::terms(q), limit, exclude)
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
    // Notes are personal: the library folder opens for its owner alone, like the app's own data.
    // (A new folder in the home folder is otherwise readable by every account on a Mac.)
    crate::paths::create_private_dir(dest)?;
    STARTER.extract(dest)?;
    Ok(())
}

/// Can other accounts on this computer get into the library folder? `None` where the question
/// does not apply (Windows keeps a user's folders to that user by its own rules) or cannot be told.
/// A folder is reachable when it lets group or others in *and* every folder above it lets them pass.
pub fn open_to_others(root: &Path) -> Option<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let real = crate::paths::canonical(root).ok()?;
        if std::fs::metadata(&real).ok()?.permissions().mode() & 0o077 == 0 { return Some(false); }
        let passable = |d: &Path| std::fs::metadata(d).map_or(true, |m| m.permissions().mode() & 0o011 != 0);
        Some(real.ancestors().skip(1).all(passable))
    }
    #[cfg(not(unix))]
    { let _ = root; None }
}

/// Close the library folder to every account but its owner. Only the folder's own door changes:
/// with it shut nothing inside can be reached, and no file inside is touched.
pub fn keep_to_owner(root: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(crate::paths::canonical(root)?, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = root;
    Ok(())
}

/// Where a markdown link on `line` points, for every link on it: `[Title](target)` → (Title, target).
fn links_on(line: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find("](") else { break };
        let Some(end) = after[close + 2..].find(')') else { break };
        out.push((after[..close].to_string(), after[close + 2..close + 2 + end].to_string()));
        rest = &after[close + 2 + end..];
    }
    out
}

/// A link target as a library-relative path: no `<…>` wrapper, title or anchor, `%20`-style escapes
/// read back. `None` for anything that is not a file in the library (a web address, an anchor).
fn link_path(target: &str) -> Option<String> {
    let t = target.trim().trim_start_matches('<');
    let t = t.split(['>', '#']).next().unwrap_or("").split(" \"").next().unwrap_or("").trim();
    if t.is_empty() || t.contains("://") || t.starts_with("mailto:") || t.starts_with('/') { return None; }
    let (mut bytes, mut i, raw) = (vec![], 0, t.as_bytes());
    while i < raw.len() {
        let hex = if raw[i] == b'%' { raw.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) } else { None };
        match hex { Some(b) => { bytes.push(b); i += 3; } None => { bytes.push(raw[i]); i += 1; } }
    }
    Some(String::from_utf8_lossy(&bytes).trim_start_matches("./").to_string())
}

/// Is this line of the index an item line that points into `dir`? Gives the item's title when it is.
fn index_line_in(root: &Path, line: &str, dir: &[String]) -> Option<String> {
    let t = line.trim_start();
    if !(t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ")) { return None; }
    links_on(t).into_iter().find(|(_, target)| link_path(target).map_or(false, |p| crate::paths::is_private(root, &p, dir))).map(|(title, _)| title)
}

/// The item lines of the library's index (`INDEX.md`) that point into `dir`, a library-relative
/// folder: the titles they show. An item indexed before its folder was made private still has its
/// title and one-line summary there, where the assistant reads them in most chats.
pub fn index_lines_in(root: &Path, dir: &str) -> Vec<String> {
    let Ok(text) = resolve(root, "INDEX.md").and_then(|p| Ok(std::fs::read_to_string(p)?)) else { return vec![] };
    let dir = [dir.to_string()];
    text.lines().filter_map(|l| index_line_in(root, l, &dir)).collect()
}

/// Take those lines out of the index, in place: every other line, and the file's line endings,
/// stay exactly as they are. Returns how many lines went.
pub fn remove_index_lines(root: &Path, dir: &str) -> anyhow::Result<usize> {
    let p = resolve(root, "INDEX.md")?;
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    let dir = [dir.to_string()];
    let (keep, gone): (Vec<&str>, Vec<&str>) = text.split_inclusive('\n').partition(|l| index_line_in(root, l, &dir).is_none());
    if !gone.is_empty() { crate::paths::write_atomic(&p, keep.concat().as_bytes(), false)?; }
    Ok(gone.len())
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

    /// SEC-17: the protected folders stay closed under any name that leads to them.
    #[cfg(unix)]
    #[test]
    fn protected_folders_are_judged_on_where_a_path_really_leads() {
        let d = tmp();
        std::fs::create_dir_all(d.join(".claude")).unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("notes/real")).unwrap();
        std::fs::write(d.join(".claude/settings.json"), "{}").unwrap();
        std::fs::write(d.join(".git/config"), "[remote \"origin\"]\n\turl = https://user:token@example.com/x.git\n").unwrap();
        std::fs::write(d.join("notes/real/a.md"), "a").unwrap();
        let link = |to: &str, at: &str| std::os::unix::fs::symlink(to, d.join(at)).unwrap();
        link("../.claude", "notes/cfg");
        link("../.git", "notes/x");
        link("../.claude/settings.json", "notes/settings.md");
        link("real", "notes/alias");
        // Reading, writing, creating, moving and deleting through the links are all refused…
        for bad in ["notes/cfg/settings.json", "notes/x/config", "notes/settings.md", "notes/cfg/skills/new/SKILL.md", "notes/cfg", "notes/x"] {
            assert!(resolve(&d, bad).is_err(), "{bad}");
            assert!(read(&d, bad).is_err() && read_raw(&d, bad).is_err(), "{bad}");
            assert!(write(&d, bad, "{\"hooks\":{}}").is_err(), "{bad}");
            assert!(rename(&d, "notes/real/a.md", bad).is_err() && rename(&d, bad, "notes/moved.md").is_err() && remove(&d, bad).is_err(), "{bad}");
        }
        assert!(create(&d, "notes/cfg/hooks.md", "x").is_err());
        assert!(check_plugin_write(&d, "notes/cfg/settings.json", &["notes".to_string()]).is_err());
        assert_eq!(std::fs::read_to_string(d.join(".claude/settings.json")).unwrap(), "{}");
        assert!(!d.join(".claude/skills").exists() && !d.join(".claude/hooks.md").exists());
        // …while an ordinary link inside the library keeps working.
        assert_eq!(read(&d, "notes/alias/a.md").unwrap()["content"], "a");
        write(&d, "notes/alias/b.md", "b").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("notes/real/b.md")).unwrap(), "b");
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
    fn folders_are_deleted_with_their_contents_but_never_protected_ones() {
        let d = tmp();
        create(&d, "clients/acme/README.md", "# Acme").unwrap();
        create(&d, "clients/acme/notes/a.md", "a").unwrap();
        create(&d, "clients/keep.md", "keep").unwrap();
        remove_all(&d, "clients/acme").unwrap();
        assert!(!d.join("clients/acme").exists());
        assert!(d.join("clients/keep.md").exists());
        // A plain file goes the ordinary way.
        remove_all(&d, "clients/keep.md").unwrap();
        assert!(!d.join("clients/keep.md").exists());
        // Anything protected inside stops the whole delete, and nothing is removed.
        create(&d, "work/x/README.md", "x").unwrap();
        std::fs::create_dir_all(d.join("work/x/deep/.git")).unwrap();
        std::fs::write(d.join("work/x/deep/.git/HEAD"), "ref").unwrap();
        assert!(remove_all(&d, "work/x").is_err());
        assert!(d.join("work/x/README.md").exists() && d.join("work/x/deep/.git/HEAD").exists());
        // Never the library itself, nothing outside it, nothing missing.
        for bad in ["", "/", ".", "./", "work/..", "../x", "nope", ".git", ".claude"] { assert!(remove_all(&d, bad).is_err(), "{bad:?}"); }
        assert!(d.join("work/x/README.md").exists());
        // A symlink inside is removed as a link; what it points to stays.
        #[cfg(unix)]
        {
            let outside = tmp();
            std::fs::write(outside.join("precious.md"), "p").unwrap();
            create(&d, "links/README.md", "l").unwrap();
            std::os::unix::fs::symlink(&outside, d.join("links/out")).unwrap();
            remove_all(&d, "links").unwrap();
            assert!(!d.join("links").exists() && outside.join("precious.md").exists());
            std::fs::remove_dir_all(outside).unwrap();
        }
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
        // SEC-43: files the assistant would read as standing instructions are not an add-on's to write,
        // move or delete, at any depth and in any letter case; ordinary notes beside them are.
        for bad in ["billing/CLAUDE.md", "billing/2026/claude.md", "Billing/Claude.Local.md", "data/AGENTS.md", "data/new/CLAUDE.md"] {
            let err = check_plugin_write(&d, bad, &allow).unwrap_err();
            assert_eq!(crate::tr::classify(&err).code, "forbidden", "{bad}");
        }
        for ok in ["billing/claude-notes.md", "billing/CLAUDE.md.txt", "billing/my-CLAUDE.md", "data/agents.json"] {
            assert!(check_plugin_write(&d, ok, &allow).is_ok(), "{ok}");
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
            // A link that leads to an instruction file is that file.
            std::fs::write(d.join("Billing/CLAUDE.md"), "the user's own").unwrap();
            std::os::unix::fs::symlink("CLAUDE.md", d.join("Billing/readme.md")).unwrap();
            assert!(check_plugin_write(&d, "Billing/readme.md", &allow).is_err());
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

    /// SEC-32: a private folder is left out of listings and search whatever its spelling.
    #[cfg(unix)]
    #[test]
    fn private_folders_are_left_out_under_any_spelling() {
        let d = tmp();
        let paths = |v: Vec<Value>| { let mut p: Vec<String> = v.iter().map(|x| x["path"].as_str().unwrap().to_string()).collect(); p.sort(); p };
        write(&d, "me/finances/tax.md", "secret salary").unwrap();
        write(&d, "me/finances/2026/jan.md", "salary slip").unwrap();
        write(&d, "ideas/a.md", "salary idea").unwrap();
        // The private entry is a link's name; the folder it leads to is the private one.
        std::os::unix::fs::symlink("../me/finances", d.join("ideas/money")).unwrap();
        let alias = vec!["ideas/money".to_string()];
        assert!(!crate::paths::is_under("me/finances/tax.md", &alias), "the name alone does not match");
        assert_eq!(paths(tree(&d, &alias)), ["ideas/a.md"]);
        assert_eq!(paths(search(&d, "salary", 10, &alias)), ["ideas/a.md"]);
        // A name with an accent, configured in one Unicode form and stored in the other. Where the
        // disk treats the two as one folder (macOS), it is the private folder.
        let (composed, decomposed) = ("clients/caf\u{e9}", "clients/cafe\u{301}");
        std::fs::create_dir_all(d.join(decomposed)).unwrap();
        std::fs::write(d.join(decomposed).join("deal.md"), "salary terms").unwrap();
        if d.join(composed).join("deal.md").exists() {
            let private = vec![composed.to_string()];
            assert!(!tree(&d, &private).iter().any(|f| f["path"].as_str().unwrap().ends_with("deal.md")), "listed under its other spelling");
            assert!(!search(&d, "salary terms", 10, &private).iter().any(|f| f["path"].as_str().unwrap().ends_with("deal.md")));
        }
        // Nothing private: everything is listed, as before.
        assert_eq!(tree(&d, &[]).len(), 4);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// SEC-14: a new library is its owner's alone, and an older one can be told apart and closed.
    #[cfg(unix)]
    #[test]
    fn a_new_library_is_closed_to_other_accounts() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let base = tmp();
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755)).unwrap();
        let lib = base.join("Thoughts");
        create_from_starter(&lib).unwrap();
        assert_eq!(mode(&lib), 0o700);
        assert!(lib.join("CLAUDE.md").is_file() && lib.join(".claude/skills/wrap/SKILL.md").is_file());
        assert_eq!(open_to_others(&lib), Some(false));
        // An empty folder the user picked is closed as it becomes the library.
        let picked = base.join("Picked");
        std::fs::create_dir(&picked).unwrap();
        std::fs::set_permissions(&picked, std::fs::Permissions::from_mode(0o755)).unwrap();
        create_from_starter(&picked).unwrap();
        assert_eq!(mode(&picked), 0o700);
        // A library from before: open like any folder made the usual way — unless a folder above it is shut.
        let old = base.join("Old");
        std::fs::create_dir(&old).unwrap();
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(old.join("note.md"), "x").unwrap();
        let above_all_open = crate::paths::canonical(&base).unwrap().ancestors().skip(1).all(|d| mode(d) & 0o011 != 0);
        assert_eq!(open_to_others(&old), Some(above_all_open));
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(open_to_others(&old), Some(false), "nobody else gets past the folder above");
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Closing it changes the folder's own door and nothing inside.
        keep_to_owner(&old).unwrap();
        assert_eq!((mode(&old), mode(&old.join("note.md")) & 0o700, open_to_others(&old)), (0o700, 0o600, Some(false)));
        assert_eq!(open_to_others(&base.join("missing")), None);
        std::fs::remove_dir_all(base).unwrap();
    }

    /// SEC-27: the index lines of items in a folder that became private can be found and removed, in place.
    #[test]
    fn index_lines_of_a_private_folder_are_found_and_removed_in_place() {
        let d = tmp();
        let index = "# Index\r\n\r\nLine format: `- [Title](area/folder/README.md) · status`\r\n\r\n## clients\r\n\
            - [Acme audit](clients/acme/README.md) · active · year-end audit · 2026-11-01\r\n\
            - [Beta & Sons](matters/beta/README.md) · active · Beta v. Gamma, hearing 3 Dec · 2026-11-02\r\n\
            * [Delta appeal](./Matters/delta%20appeal/README.md) · parked · appeal against Delta · 2026-10-01\r\n\
            - [Notes on matters](knowledge/matters.md) · done · how we open a matter · 2026-01-01\r\n\
            - See [the site](https://example.com/matters/x) and [Epsilon](<matters/epsilon/README.md> \"Epsilon\")\r\n\
            A paragraph that mentions [Beta](matters/beta/README.md) is not an item line.\r\n";
        std::fs::write(d.join("INDEX.md"), index).unwrap();
        std::fs::create_dir_all(d.join("matters/beta")).unwrap();
        assert_eq!(index_lines_in(&d, "matters"), ["Beta & Sons", "Delta appeal", "Epsilon"]);
        assert_eq!(index_lines_in(&d, "clients/acme"), ["Acme audit"]);
        assert!(index_lines_in(&d, "nothing/here").is_empty() && index_lines_in(&d, "knowledge/matters").is_empty());
        assert_eq!(remove_index_lines(&d, "matters").unwrap(), 3);
        assert_eq!(std::fs::read_to_string(d.join("INDEX.md")).unwrap(), "# Index\r\n\r\nLine format: `- [Title](area/folder/README.md) · status`\r\n\r\n## clients\r\n\
            - [Acme audit](clients/acme/README.md) · active · year-end audit · 2026-11-01\r\n\
            - [Notes on matters](knowledge/matters.md) · done · how we open a matter · 2026-01-01\r\n\
            A paragraph that mentions [Beta](matters/beta/README.md) is not an item line.\r\n");
        // Nothing left to remove: the file is not rewritten; no index at all is not an error.
        assert_eq!(remove_index_lines(&d, "matters").unwrap(), 0);
        std::fs::remove_file(d.join("INDEX.md")).unwrap();
        assert_eq!(remove_index_lines(&d, "matters").unwrap(), 0);
        assert!(index_lines_in(&d, "matters").is_empty());
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
    fn exported_documents_go_where_asked_and_never_overwrite() {
        let root = tmp();
        assert_eq!(save_output(&root, "apps/one/outputs", "Plan.docx", b"a").unwrap(), "apps/one/outputs/Plan.docx");
        assert_eq!(save_output(&root, "apps/one/outputs/", "Plan.docx", b"b").unwrap(), "apps/one/outputs/Plan (2).docx");
        assert_eq!(std::fs::read(root.join("apps/one/outputs/Plan.docx")).unwrap(), b"a");
        assert_eq!(save_output(&root, "", "خطة.docx", b"c").unwrap(), "خطة.docx");
        for (folder, name) in [("apps", "x.html"), ("apps", "x.sh"), ("../out", "x.docx"), (".git", "x.docx"), ("/tmp", "x.docx"), ("a/.claude", "x.docx")] {
            assert!(save_output(&root, folder, name, b"x").is_err(), "{folder}/{name}");
        }
        std::fs::remove_dir_all(&root).unwrap();
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
