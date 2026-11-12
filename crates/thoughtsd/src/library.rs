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
pub(crate) fn check_new_path(rel: &str) -> anyhow::Result<()> {
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
pub(crate) fn instruction_file(name: &str) -> bool {
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

/// Largest picture that can be pasted or dropped into a note.
pub const PICTURE_MAX_BYTES: usize = 15 * 1024 * 1024;

/// What kind of picture `bytes` hold, judged by their first bytes (never by the name they came with):
/// the extension they are saved under. None for anything that is not a PNG, JPEG, GIF or WebP, which
/// every system's web view shows (SVG is left out: it can carry scripts).
pub fn picture_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") { Some("png") }
    else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) { Some("jpg") }
    else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") { Some("gif") }
    else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" { Some("webp") }
    else { None }
}

/// A picture's name without its extension: letters and digits of any script, `-` and `_`, with
/// spaces and dots turned into `-`, so the link to it needs no escaping. At most 60 characters, never empty.
fn picture_stem(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = match base.rsplit_once('.') { Some((s, _)) if !s.is_empty() => s, _ => base };
    let mut out = String::new();
    for c in stem.chars() {
        if c.is_alphanumeric() || c == '_' || c == '-' { out.push(c); }
        else if (c.is_whitespace() || c == '.') && !out.ends_with('-') { out.push('-'); }
    }
    let out: String = out.trim_matches(['-', '_']).chars().take(60).collect();
    let out = out.trim_end_matches(['-', '_']);
    // Names Windows keeps for devices cannot be files there, whatever the extension.
    let reserved = |s: &str| { let u = s.to_uppercase(); ["CON", "PRN", "AUX", "NUL"].contains(&u.as_str())
        || (u.len() == 4 && (u.starts_with("COM") || u.starts_with("LPT")) && u.as_bytes()[3].is_ascii_digit()) };
    if out.is_empty() { "picture".into() } else if reserved(out) { format!("picture-{out}") } else { out.into() }
}

/// Save a picture pasted or dropped into note `note` in an `images` folder beside it, under a name
/// of its own (`receipt.png`, `receipt-2.png`…; never replacing a file). The same path rules as every
/// other library write. Returns the library-relative path and the link to write into the note
/// (relative to the note's folder).
pub fn save_picture(root: &Path, note: &str, name: &str, bytes: &[u8]) -> anyhow::Result<(String, String)> {
    use std::io::Write;
    let note = note.trim().trim_start_matches("./").replace('\\', "/");
    if !note.to_lowercase().ends_with(".md") { return Err(crate::tr::err("invalid.path", &[])); }
    if bytes.len() > PICTURE_MAX_BYTES { return Err(crate::tr::err("picture.too_large", &[])); }
    let Some(ext) = picture_kind(bytes) else { return Err(crate::tr::err("picture.type", &[])) };
    check_new_path(&note)?;
    if !resolve(root, &note)?.is_file() { return Err(crate::tr::err("not_found.file", &[])); }
    let folder = match note.rsplit_once('/') { Some((dir, _)) => format!("{dir}/images"), None => "images".into() };
    let dir = resolve(root, &folder)?;
    // `images` taken by a file: refuse rather than guess.
    if dir.exists() && !dir.is_dir() { return Err(crate::tr::err("invalid.path", &[])); }
    std::fs::create_dir_all(&dir)?;
    let stem = picture_stem(name);
    for n in 1..10_000 {
        let file = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem}-{n}.{ext}") };
        let rel = format!("{folder}/{file}");
        check_new_path(&rel)?;
        let p = resolve(root, &rel)?;
        // create_new: fails rather than replaces if the name is taken (also refuses dangling symlinks).
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(bytes) { drop(f); let _ = std::fs::remove_file(&p); return Err(e.into()); }
                return Ok((rel, format!("images/{file}")));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::bail!("too many pictures named {stem}")
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

// ---------- one item's page (FEAT-17) ----------

/// The folder of an item, `<area>/<item>`, from `<area>/<item>`, `<area>/<item>/` or
/// `<area>/<item>/README.md` — only where `ideas` would list one (never `me/`, `inbox/` or a dot folder).
pub fn item_dir(rel: &str) -> anyhow::Result<String> {
    let rel = rel.trim().replace('\\', "/");
    let rel = rel.trim_matches('/');
    let rel = rel.strip_suffix("/README.md").unwrap_or(rel);
    let parts: Vec<&str> = rel.split('/').collect();
    let ok = parts.len() == 2 && parts.iter().all(|p| !p.trim().is_empty() && *p != "." && *p != "..")
        && !parts[0].starts_with('.') && parts[0] != "me" && parts[0] != "inbox";
    if !ok { return Err(crate::tr::err("not_found.item", &[])); }
    Ok(rel.to_string())
}

/// A file of an item as its page lists it. Markdown files carry what their note details say.
fn item_file(root: &Path, p: &Path, rel: String) -> Value {
    let meta = std::fs::metadata(p).ok();
    let modified = meta.as_ref().and_then(|m| m.modified().ok()).map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let mut v = json!({"path": rel, "name": name, "size": meta.as_ref().map(|m| m.len()).unwrap_or(0), "modified": modified});
    if name.to_lowercase().ends_with(".md") {
        // The note details are at the top: the first part of the file is enough.
        use std::io::Read;
        let mut head = Vec::with_capacity(8192);
        if let Ok(f) = std::fs::File::open(root.join(&rel)) { let _ = f.take(16 * 1024).read_to_end(&mut head); }
        let text = String::from_utf8_lossy(&head);
        if let Some((fm, _)) = note_details(&text) {
            let s = |k: &str| match &fm[k] { Value::Null => Value::Null, Value::String(s) if s.trim().is_empty() => Value::Null, Value::String(s) => json!(s.trim()), o => json!(yaml_date(o)) };
            for k in ["title", "summary", "updated", "date"] { v[k] = s(k); }
            v["attendees"] = match &fm["attendees"] {
                Value::Array(a) => json!(a.iter().map(|x| x.as_str().map(String::from).unwrap_or_else(|| yaml_date(x))).filter(|x| !x.trim().is_empty()).collect::<Vec<_>>()),
                Value::String(s) if !s.trim().is_empty() => json!(s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>()),
                _ => json!([]),
            };
        }
    }
    // A meeting's day: its note details, else the date its name starts with.
    if v["date"].is_null() {
        let d: String = name.chars().take(10).collect();
        if chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").is_ok() { v["date"] = json!(d); }
    }
    v
}

/// Dated lines under a "key dates" heading of an item's front page (either language), soonest
/// first: `{date, text}`. Only full dates (YYYY-MM-DD) count; the rest of the line (or of the table
/// row) is the text.
pub fn key_dates(body: &str) -> Vec<Value> {
    let heading = regex::Regex::new(r"(?i)\b(key|important)\s+dates?\b|\bdates\b|\bdeadlines?\b|التواريخ|تواريخ|المواعيد|مواعيد").unwrap();
    let date = regex::Regex::new(r"\b(\d{4}-\d{2}-\d{2})\b").unwrap();
    let bullet = regex::Regex::new(r"^(?:[-*+]|\d+[.)])\s+").unwrap();
    let mut out: Vec<Value> = vec![];
    let mut level: Option<usize> = None;
    for l in body.lines() {
        let t = l.trim();
        let hashes = t.chars().take_while(|c| *c == '#').count();
        if hashes > 0 && t.chars().nth(hashes) == Some(' ') {
            if level.map_or(false, |lv| hashes <= lv) { level = None; }
            if level.is_none() && heading.is_match(&t[hashes..]) { level = Some(hashes); }
            continue;
        }
        if level.is_none() { continue; }
        let Some(m) = date.captures(t).and_then(|c| c.get(1)) else { continue };
        if chrono::NaiveDate::parse_from_str(m.as_str(), "%Y-%m-%d").is_err() { continue; }
        let tidy = |s: &str| s.replace("**", "").replace('`', "").trim().trim_matches(['-', '—', '–', ':', '·', '|', ' ', '*']).trim().to_string();
        let text = if t.starts_with('|') {
            t.split('|').map(|c| tidy(&c.replace(m.as_str(), ""))).filter(|c| !c.is_empty()).collect::<Vec<_>>().join(" · ")
        } else {
            tidy(&bullet.replace(t, "").replacen(m.as_str(), "", 1))
        };
        out.push(json!({"date": m.as_str(), "text": text.chars().take(200).collect::<String>()}));
        if out.len() >= 50 { break; }
    }
    out.sort_by(|a, b| a["date"].as_str().cmp(&b["date"].as_str()));
    out
}

/// The entries of an item's log (`## YYYY-MM-DD — topic`, then lines), newest first. Anything
/// above the first entry (a title) is left out; an entry without a date keeps its place after the dated ones.
pub fn log_entries(text: &str) -> Vec<Value> {
    // (heading, body lines) per entry, in file order.
    let mut raw: Vec<(String, Vec<&str>)> = vec![];
    for l in text.lines() {
        if let Some(h) = l.strip_prefix("## ") { raw.push((h.trim().to_string(), vec![])); }
        else if let Some((_, body)) = raw.last_mut() { body.push(l.trim_end_matches('\r')); }
    }
    let mut out: Vec<(String, Value)> = raw.into_iter().map(|(heading, body)| {
        let d: String = heading.chars().take(10).collect();
        let date = if chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").is_ok() { d } else { String::new() };
        let topic = if date.is_empty() { heading.clone() } else { heading[date.len()..].trim().trim_start_matches(['—', '–', '-', ':', '·']).trim().to_string() };
        let v = json!({"date": if date.is_empty() { Value::Null } else { json!(date) }, "topic": topic, "heading": heading,
            "body": body.join("\n").trim_matches('\n').to_string()});
        (date, v)
    }).collect();
    // Newest first, whatever order the file is in; undated entries after the dated ones.
    out.sort_by(|a, b| match (a.0.is_empty(), b.0.is_empty()) { (false, false) => b.0.cmp(&a.0), (x, y) => x.cmp(&y) });
    out.into_iter().map(|(_, v)| v).collect()
}

/// Everything an item's page shows, in one read: its note details, front page, key dates, todo
/// list, meeting notes, notes, deliverables (`outputs/`), log, and any other file in its folder.
/// Anything in an `exclude` folder (library-relative) is left out, judged on where each path
/// really leads (a link, another letter case) as well as on its name: an item that is in one is
/// refused, and a file of the item that leads into one is not read.
pub fn item(root: &Path, rel: &str, exclude: &[String]) -> anyhow::Result<Value> {
    let dir = item_dir(rel)?;
    let readme_rel = format!("{dir}/README.md");
    let private = |rel: &str| crate::paths::is_private(root, rel, exclude);
    if private(&dir) || private(&readme_rel) { return Err(crate::tr::err("private", &[("path", &dir)])); }
    let base = resolve(root, &dir)?;
    let readme = resolve(root, &readme_rel)?;
    let raw = match std::fs::read_to_string(&readme) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(crate::tr::err("not_found.item", &[])),
        Err(e) => return Err(e.into()),
    };
    let text = raw.strip_prefix('\u{feff}').unwrap_or(&raw).to_string();
    let details = note_details(&text);
    let fm = details.as_ref().map(|d| d.0.clone()).unwrap_or(Value::Null);
    let s = |k: &str| match &fm[k] { Value::String(s) => s.trim().to_string(), Value::Null => String::new(), o => yaml_date(o) };
    let (area, slug) = dir.split_once('/').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
    // (The body is split off even when the details are not strict YAML.)
    let body = frontmatter(&text).1.to_string();

    let hidden = Hidden::new(root, exclude);
    let (mut notes, mut meetings, mut outputs, mut other) = (vec![], vec![], vec![], vec![]);
    const MAX: usize = 1000;
    for e in WalkDir::new(&base).min_depth(1).into_iter().filter_entry(|e| !skip(e) && !hidden.folder(e)).flatten() {
        if !e.file_type().is_file() || e.file_name().to_string_lossy().starts_with('.') { continue; }
        let Some(r) = rel_of(root, e.path()) else { continue };
        if private(&r) { continue; }
        let Some(inner) = r.strip_prefix(&format!("{dir}/")) else { continue };
        let (first, nested) = match inner.split_once('/') { Some((f, _)) => (f.to_lowercase(), true), None => (inner.to_lowercase(), false) };
        let list = match (first.as_str(), nested) {
            ("notes", true) => &mut notes,
            ("meetings", true) => &mut meetings,
            ("outputs", true) => &mut outputs,
            ("readme.md" | "log.md" | "todo.md" | "log-archive.md", false) => continue,
            _ => &mut other,
        };
        if list.len() < MAX { list.push(item_file(root, e.path(), r)); }
    }
    let day = |v: &Value| v["date"].as_str().or(v["updated"].as_str()).map(|d| d.chars().take(10).collect::<String>())
        .or_else(|| v["modified"].as_str().map(|m| m.chars().take(10).collect())).unwrap_or_default();
    let newest = |a: &Value, b: &Value| day(b).cmp(&day(a)).then_with(|| b["path"].as_str().cmp(&a["path"].as_str()));
    meetings.sort_by(newest);
    notes.sort_by(|a, b| { let u = |v: &Value| v["updated"].as_str().map(String::from).unwrap_or_else(|| day(v)); u(b).cmp(&u(a)).then_with(|| a["path"].as_str().cmp(&b["path"].as_str())) });
    outputs.sort_by(|a, b| b["modified"].as_str().cmp(&a["modified"].as_str()).then_with(|| a["path"].as_str().cmp(&b["path"].as_str())));
    other.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));

    let todo_rel = format!("{dir}/todo.md");
    let todo_text = if private(&todo_rel) { None } else { std::fs::read_to_string(resolve(root, &todo_rel)?).ok() };
    let log_rel = format!("{dir}/log.md");
    let log_text = if private(&log_rel) { None } else { std::fs::read_to_string(resolve(root, &log_rel)?).ok() };
    let archive = format!("{dir}/log-archive.md");
    let archive = (!private(&archive) && resolve(root, &archive)?.is_file()).then_some(archive);

    Ok(json!({
        "path": readme_rel, "dir": dir, "area": area, "slug": slug,
        "title": if s("title").is_empty() { slug.clone() } else { s("title") },
        "status": s("status"), "kind": s("kind"), "summary": s("summary"),
        "created": s("created"), "updated": s("updated"),
        "tags": match &fm["tags"] { Value::Array(a) => json!(a), Value::String(t) if !t.trim().is_empty() => json!(t.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).collect::<Vec<_>>()), _ => json!([]) },
        "details": details.is_some(),
        "detailsUnreadable": details.as_ref().map_or(false, |d| d.1 && s("title").is_empty() && s("status").is_empty() && s("summary").is_empty()),
        "readme": {"content": text, "body": body, "hash": content_hash(raw.as_bytes())},
        "keyDates": key_dates(&body),
        "todo": {"file": todo_rel, "exists": todo_text.is_some(),
                 "groups": todo_text.as_deref().map(crate::todos::groups).unwrap_or_default(),
                 "items": todo_text.as_deref().map(crate::todos::parse).unwrap_or_default()},
        "meetings": meetings, "notes": notes, "outputs": outputs, "other": other,
        "log": {"path": log_rel, "exists": log_text.is_some(), "archive": archive, "entries": log_text.as_deref().map(log_entries).unwrap_or_default()},
    }))
}

/// Characters a file name made from a title leaves out instead of turning them into a gap:
/// Arabic diacritics and tatweel, and combining accents.
fn name_mark(c: char) -> bool {
    matches!(c, '\u{0300}'..='\u{036f}' | '\u{0610}'..='\u{061a}' | '\u{064b}'..='\u{065f}' | '\u{0670}' | '\u{0640}' | '\u{06d6}'..='\u{06ed}')
}
fn arabic_letter(c: char) -> bool {
    matches!(c, '\u{0600}'..='\u{06ff}' | '\u{0750}'..='\u{077f}' | '\u{08a0}'..='\u{08ff}' | '\u{fb50}'..='\u{fdff}' | '\u{fe70}'..='\u{feff}')
}

/// Does this library name its files in Arabic? The `Names:` line under `## About this library`
/// (written at setup) says so; without one, an item whose own folder name is Arabic does.
pub fn arabic_names(root: &Path, folder: &str) -> bool {
    let rules = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap_or_default();
    for l in rules.lines() {
        let l = l.trim().trim_start_matches(['-', '*']).trim().to_lowercase();
        let Some(v) = l.strip_prefix("names:").or_else(|| l.strip_prefix("الأسماء:")) else { continue };
        if v.contains("arab") || v.contains("عرب") { return true; }
        if v.contains("latin") || v.contains("لاتين") { return false; }
    }
    folder.chars().any(arabic_letter)
}

/// A short file-name part from a title, as the library's rules name things: lowercase words joined
/// by hyphens, in Arabic or Latin letters (`arabic`), without diacritics or punctuation, at most six words.
pub fn name_from_title(title: &str, arabic: bool) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if name_mark(c) { continue; }
        let keep = c.is_alphanumeric() && (arabic || !arabic_letter(c));
        if keep { out.extend(c.to_lowercase()); } else if !out.ends_with('-') { out.push('-'); }
    }
    let mut name = String::new();
    for w in out.split('-').filter(|w| !w.is_empty()).take(6) {
        if name.chars().count() + w.chars().count() > 60 { break; }
        if !name.is_empty() { name.push('-'); }
        name.push_str(w);
    }
    name
}

/// Start a meeting note for an item, the way the meeting-notes routine files one:
/// `<item>/meetings/YYYY-MM-DD-short-topic.md` with the note details `title, date, attendees,
/// updated, summary` and the three sections, headings in the app's language. Never replaces a file:
/// a second note of the same name and day gets `-2`, `-3`… Returns the new file.
pub fn new_meeting(root: &Path, rel: &str, title: &str, date: chrono::NaiveDate, today: chrono::NaiveDate, arabic: bool) -> anyhow::Result<String> {
    let dir = item_dir(rel)?;
    if !resolve(root, &format!("{dir}/README.md"))?.is_file() { return Err(crate::tr::err("not_found.item", &[])); }
    let folder = dir.rsplit('/').next().unwrap_or(&dir);
    let names_ar = arabic_names(root, folder);
    let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
    let title = if title.is_empty() { (if arabic { "اجتماع" } else { "Meeting" }).to_string() } else { title };
    let topic = name_from_title(&title, names_ar);
    let topic = if topic.is_empty() { (if names_ar { "اجتماع" } else { "meeting" }).to_string() } else { topic };
    let quoted = title.replace('\\', "\\\\").replace('"', "\\\"");
    let (discussed, decisions, actions) = if arabic { ("ما نوقش", "القرارات", "بنود العمل") } else { ("Discussed", "Decisions", "Action items") };
    let content = format!("---\ntitle: \"{quoted}\"\ndate: {d}\nattendees: []\nupdated: {u}\nsummary: \n---\n\n# {title}\n\n## {discussed}\n\n\n## {decisions}\n\n\n## {actions}\n\n",
        d = date.format("%Y-%m-%d"), u = today.format("%Y-%m-%d"));
    let stem = format!("{dir}/meetings/{}-{topic}", date.format("%Y-%m-%d"));
    for n in 1..100 {
        let path = if n == 1 { format!("{stem}.md") } else { format!("{stem}-{n}.md") };
        match create(root, &path, &content) {
            Ok(_) => return Ok(path),
            Err(e) if crate::tr::classify(&e).code == "exists" => continue,
            Err(e) => return Err(e),
        }
    }
    Err(crate::tr::err("exists", &[("path", &stem)]))
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
    pub(crate) fn folder(&self, e: &walkdir::DirEntry) -> bool { e.file_type().is_dir() && self.dir(e.path()) }

    /// Is the folder at `p` one of the hidden ones (or inside one)? For a list of files that was
    /// walked already: ask per folder, not per file.
    pub(crate) fn dir(&self, p: &Path) -> bool {
        if self.real.is_empty() { return false; }
        let p = Self::norm(p);
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

/// Move the kit's presets from the top of a library to `.claude/presets/`: out of the user's way,
/// still there if they change their line of work. A preset already installed there is kept and the
/// copy at the top removed. Only the kit's own file names are touched; anything else in a folder
/// called `presets` stays where it is. Returns whether anything was moved or removed.
/// Does the way from `root` down to `path` (not counting `path` itself) pass through a link? The
/// app never moves, writes or removes one of the kit's files by way of a link: where a link leads
/// is not the library.
pub(crate) fn via_link(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else { return true };
    let mut parts: Vec<_> = rel.components().collect();
    parts.pop();
    let mut at = root.to_path_buf();
    parts.into_iter().any(|c| { at.push(c); std::fs::symlink_metadata(&at).map_or(false, |m| m.file_type().is_symlink()) })
}

pub fn install_presets(root: &Path) -> bool {
    let (top, home) = (root.join("presets"), root.join(".claude/presets"));
    let is = |p: &Path, dir: bool| std::fs::symlink_metadata(p).map_or(false, |m| if dir { m.is_dir() } else { m.is_file() });
    // A real folder only: a link named `presets` is somebody else's business.
    let Some(kit) = STARTER.get_dir("presets").filter(|_| is(&top, true)) else { return false };
    let (mut changed, mut dirs, mut stack) = (false, vec![], vec![kit]);
    while let Some(d) = stack.pop() {
        stack.extend(d.dirs());
        dirs.push(d.path().to_path_buf());
        for f in d.files() {
            let Ok(rel) = f.path().strip_prefix("presets") else { continue };
            let (from, to) = (top.join(rel), home.join(rel));
            if !is(&from, false) || via_link(root, &from) || via_link(root, &to) { continue; }
            let done = if std::fs::symlink_metadata(&to).is_ok() { std::fs::remove_file(&from) } else {
                to.parent().map_or(Ok(()), std::fs::create_dir_all)
                    // A rename can fail across volumes: copy, then remove.
                    .and_then(|_| std::fs::rename(&from, &to).or_else(|_| std::fs::copy(&from, &to).and_then(|_| std::fs::remove_file(&from))))
            };
            match done {
                Ok(()) => changed = true,
                Err(e) => crate::log::warn(format!("could not tidy the preset {}: {e}", rel.display())),
            }
        }
    }
    // Deepest first; a folder that still holds something of the user's is not empty and stays.
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs { let _ = std::fs::remove_dir(root.join(d)); }
    changed
}

/// What the app clears away once the assistant has finished setting a new library up: the one-time
/// setup routine, both welcome pages (English and Arabic), and the kit's presets folder at the top.
/// The assistant is told to leave all of this to the app: it cannot write under `.claude/` itself.
pub fn finish_setup(root: &Path) {
    let _ = std::fs::remove_dir_all(root.join(SETUP_ROUTINE));
    for page in ["START-HERE.md", "START-HERE.ar.md"] { let _ = std::fs::remove_file(root.join(page)); }
    install_presets(root);
}

/// The welcome pages and presets exactly as the kit shipped them in 0.0.8 (sha256), for
/// `tidy_kit_leftovers`: the kit's current files may have been reworded since.
const KIT_0_0_8: &[(&str, &str)] = &[
    ("START-HERE.md", "8feffc62aa0b0c09b095914af6181414b5dab35695ee19f55c3f4a7a7c842a07"),
    ("START-HERE.ar.md", "b933845d41fa9dc1e04aa806528d802108993e437f224ef27998c16afc5f9ef6"),
    ("presets/accounting-finance.md", "75302339d359a0524a8bd5180e3af83806e600780a309e827b644b2268302dc9"),
    ("presets/ar/accounting-finance.md", "17f79953e8a6441788b416e496176380887a2a9bb920fa2d43d716566a66e7b4"),
    ("presets/ar/consulting.md", "a659e3236d0b07fe7d180681f9fe19a8da0d95a040626491af87182a8e68eeec"),
    ("presets/ar/founder-product.md", "9578b190192e0c1490cc43caccca54a3d972f7931454f8a5edf7859a4dadd57d"),
    ("presets/ar/legal.md", "b182e7d7d1385a939aed06fa1a660d0c997ad346055f92b3b04a3db3aa67c202"),
    ("presets/ar/research-study.md", "4312581180d474d2d4d5421ebef2173608e08dedb6feb6cb9e5382288497bfee"),
    ("presets/ar/teaching.md", "f6765dc36e6d0a2ca8b5bb843d276410c4241015dc3620c908db7803a35930a0"),
    ("presets/consulting.md", "5a63cf17c35eea0fad6fc2c57adb8f0dcdc70fcdb5ffe7f9d080b9f1a7adc883"),
    ("presets/founder-product.md", "32cbbb38c563c2d56e93e954acb0e5dd792c9e264cf4cf8f2216578e7c0d76c5"),
    ("presets/legal.md", "6fa7da4e50188f52390420679a8977cd0053277b7ec51cfcfd62047b0e722abc"),
    ("presets/research-study.md", "67fbbaea82702d80daf542da3cd1779aa89b3f3904f4d0f5a5e7a96b99966267"),
    ("presets/teaching.md", "5724379a2abfdad17ca9abf6ff42dd7511dab2555abe6c98cec2fda15df71e13"),
];

/// Is the library's file at `rel` still exactly what a kit shipped there (this version's, or 0.0.8's)?
fn untouched_kit_file(root: &Path, rel: &str) -> bool {
    let file = root.join(rel);
    if !std::fs::symlink_metadata(&file).map_or(false, |m| m.is_file() && m.len() < 1 << 20) { return false; }
    let Ok(bytes) = std::fs::read(&file) else { return false };
    if STARTER.get_file(rel).map_or(false, |f| f.contents() == bytes) { return true; }
    let hash = content_hash(&bytes);
    KIT_0_0_8.iter().any(|(r, h)| *r == rel && *h == hash)
}

/// A library set up with 0.0.8 or earlier kept the Arabic welcome page at the top and, usually,
/// the kit's whole presets folder (that version's setup could not clear them away). Finish that
/// job: remove those files where they are still exactly what the kit shipped, and make sure the
/// presets are where the routines expect them, under `.claude/presets/`. A file the user changed,
/// and anything else in a folder called `presets`, is left alone; a library that has not been set
/// up yet is not touched at all. Returns how many files were cleared away.
pub fn tidy_kit_leftovers(root: &Path) -> usize {
    if awaits_setup(root) || !root.join("CLAUDE.md").is_file() { return 0; }
    let mut cleared = 0;
    for page in ["START-HERE.md", "START-HERE.ar.md"] {
        if untouched_kit_file(root, page) && std::fs::remove_file(root.join(page)).is_ok() { cleared += 1; }
    }
    let (mut dirs, mut stack) = (vec![], STARTER.get_dir("presets").into_iter().collect::<Vec<_>>());
    while let Some(d) = stack.pop() {
        stack.extend(d.dirs());
        dirs.push(d.path().to_path_buf());
        for f in d.files() {
            let rel = f.path().to_string_lossy().replace('\\', "/");
            // Installed as the kit ships it today, so it stays one of the app's own files.
            let home = root.join(".claude").join(f.path());
            if !untouched_kit_file(root, &rel) || via_link(root, &root.join(&rel)) || via_link(root, &home) { continue; }
            if std::fs::symlink_metadata(&home).is_err() {
                let put = home.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&home, f.contents()));
                if let Err(e) = put { crate::log::warn(format!("could not install the preset {rel}: {e}")); continue; }
            }
            if std::fs::remove_file(root.join(&rel)).is_ok() { cleared += 1; }
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs { let _ = std::fs::remove_dir(root.join(d)); }
    cleared
}

/// The kit's one-time setup routine. While it is there, the library has not been set up.
pub const SETUP_ROUTINE: &str = ".claude/skills/init-library";

/// Has this library still to be set up? (Its setup routine is removed when setup finishes.)
pub fn awaits_setup(root: &Path) -> bool { root.join(SETUP_ROUTINE).join("SKILL.md").is_file() }

/// Library files containing every word of `q` (see `crate::search`), best first.
pub fn search(root: &Path, q: &str, limit: usize, exclude: &[String]) -> Vec<Value> {
    crate::search::notes(root, &crate::search::terms(q), limit, exclude)
}

/// The library's own machinery rather than the user's notes: the rules (`CLAUDE.md` anywhere), the
/// index and the starter's welcome pages at the top, and a leftover `presets/` folder.
fn system_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name == "CLAUDE.md" || rel == "INDEX.md" || (!rel.contains('/') && name.starts_with("START-HERE")) || rel.starts_with("presets/") || rel.starts_with("inbox/")
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

/// What a folder is, as far as starting a new library in it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderState {
    /// Nothing there yet: it will be created.
    Missing,
    /// A folder with nothing in it (an empty repository counts).
    Empty,
    /// A copy of the starter kit that nobody has changed or set up — what a setup that stopped
    /// half-way leaves behind. Setup can simply run on it again.
    Starter,
    /// A Thoughts library with content of its own.
    Library,
    /// A folder with other things in it.
    Files,
    /// Something that is not a folder.
    NotFolder,
}

impl FolderState {
    pub fn as_str(self) -> &'static str {
        match self { Self::Missing => "missing", Self::Empty => "empty", Self::Starter => "starter", Self::Library => "library", Self::Files => "files", Self::NotFolder => "notFolder" }
    }
    /// May a new library be started here?
    pub fn free(self) -> bool { matches!(self, Self::Missing | Self::Empty | Self::Starter) }
}

/// Files an operating system drops into folders by itself.
pub(crate) fn os_dropping(name: &str) -> bool { matches!(name, ".DS_Store" | "Thumbs.db" | "desktop.ini") || name.starts_with("._") }

pub fn folder_state(dest: &Path) -> FolderState {
    let Ok(meta) = std::fs::metadata(dest) else {
        // A dangling link is something, just not a folder.
        return if std::fs::symlink_metadata(dest).is_ok() { FolderState::NotFolder } else { FolderState::Missing };
    };
    if !meta.is_dir() { return FolderState::NotFolder; }
    let Ok(rd) = std::fs::read_dir(dest) else { return FolderState::Files };
    let empty = rd.flatten().all(|e| { let n = e.file_name(); let n = n.to_string_lossy(); os_dropping(&n) || n == ".git" });
    if empty { return FolderState::Empty; }
    if untouched_starter(dest) { return FolderState::Starter; }
    if dest.join("CLAUDE.md").is_file() || dest.join("INDEX.md").is_file() { return FolderState::Library; }
    FolderState::Files
}

/// Is `dest` a copy of the starter kit with nothing added and nothing changed? Files the kit ships
/// may be missing (setup removes the routines of features that were not chosen); a `.git` folder
/// at the top is not looked into.
fn untouched_starter(dest: &Path) -> bool {
    if !dest.join("CLAUDE.md").is_file() { return false; }
    let walk = WalkDir::new(dest).min_depth(1).follow_links(false).into_iter().filter_entry(|e| !(e.depth() == 1 && e.file_name() == ".git"));
    for (seen, e) in walk.enumerate() {
        // The kit is a few dozen files: anything much bigger is not a copy of it.
        let Ok(e) = e else { return false };
        if seen > 2000 { return false; }
        let Some(rel) = rel_of(dest, e.path()) else { return false };
        let kind = e.file_type();
        if kind.is_dir() {
            if STARTER.get_dir(&rel).is_none() { return false; }
        } else if kind.is_file() {
            if os_dropping(&e.file_name().to_string_lossy()) { continue; }
            let Some(f) = STARTER.get_file(&rel) else { return false };
            let same = e.metadata().map_or(false, |m| m.len() == f.contents().len() as u64) && std::fs::read(e.path()).map_or(false, |b| b == f.contents());
            if !same { return false; }
        } else {
            return false; // a link, or something stranger
        }
    }
    true
}

/// May a new library be started in `dest`? Yes when nothing is there, the folder is empty, or it
/// holds an untouched copy of the starter kit. Otherwise the error says what is in the way.
pub fn check_new_library(dest: &Path) -> anyhow::Result<FolderState> {
    let (state, path) = (folder_state(dest), dest.display().to_string());
    match state {
        FolderState::Library => Err(crate::tr::err("library.exists", &[("path", &path)])),
        FolderState::Files => Err(crate::tr::err("library.not_empty", &[("path", &path)])),
        FolderState::NotFolder => Err(crate::tr::err("library.not_folder", &[("path", &path)])),
        free => Ok(free),
    }
}

/// Copy the embedded starter kit into `dest`: a folder that is missing, empty, or already an
/// untouched copy of the kit (so a setup that failed late can be tried again on the same folder).
pub fn create_from_starter(dest: &Path) -> anyhow::Result<()> {
    check_new_library(dest)?;
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

    /// FEAT-17: one item's page in one read — a full client folder.
    #[test]
    fn an_items_page_has_all_its_parts() {
        let d = tmp();
        write(&d, "clients/acme/README.md", "---\ntitle: Acme Trading\nkind: client\nstatus: active\narea: clients\ntags: [vat, monthly]\ncreated: 2026-01-05\nupdated: 2026-11-02\nsummary: Monthly books and VAT\n---\n\n# Acme Trading\n\n\
            ## Key dates\n\n| Date | What | Source |\n|---|---|---|\n| 2026-12-31 | **Year end** | Engagement letter |\n\n- 2026-11-30 — VAT return due\n- not dated yet\n\n\
            ## Current state\nBooks closed to the end of October (2026-10-31).\n").unwrap();
        write(&d, "clients/acme/todo.md", "## Now\n- [ ] Send the draft @due(2026-11-20)\n  - with the notes\n## Done\n- [x] Book the review\n").unwrap();
        write(&d, "clients/acme/log.md", "# Log\n\n## 2026-10-01 — Kick-off\n- Decided: monthly close\n\n## 2026-11-02 — October close\n- Learned: invoices arrive late\n- Next: chase the supplier\n").unwrap();
        write(&d, "clients/acme/meetings/2026-10-01-kick-off.md", "---\ntitle: Kick-off\ndate: 2026-10-01\nattendees: [Sara (CFO), Omar]\nupdated: 2026-10-01\nsummary: Monthly close agreed\n---\n## Discussed\n").unwrap();
        write(&d, "clients/acme/meetings/2026-11-02-october-review.md", "Rough notes, no details yet.\n").unwrap();
        write(&d, "clients/acme/notes/fees.md", "---\ntitle: Fees\nupdated: 2026-09-01\nsummary: What we charge\n---\nText\n").unwrap();
        std::fs::write(d.join("clients/acme/notes/scan.png"), [0x89, b'P']).unwrap();
        std::fs::create_dir_all(d.join("clients/acme/outputs/letters")).unwrap();
        std::fs::write(d.join("clients/acme/outputs/2026-10-report.pdf"), b"%PDF").unwrap();
        std::fs::write(d.join("clients/acme/outputs/letters/2026-11-01-reminder.docx"), b"PK").unwrap();
        std::fs::write(d.join("clients/acme/contract.pdf"), b"%PDF").unwrap();
        std::fs::write(d.join("clients/acme/.DS_Store"), b"x").unwrap();
        let paths = |v: &Value| v.as_array().unwrap().iter().map(|x| x["path"].as_str().unwrap().to_string()).collect::<Vec<_>>();

        let v = item(&d, "clients/acme/README.md", &[]).unwrap();
        assert_eq!((v["dir"].as_str(), v["area"].as_str(), v["slug"].as_str()), (Some("clients/acme"), Some("clients"), Some("acme")));
        assert_eq!((v["title"].as_str(), v["status"].as_str(), v["kind"].as_str()), (Some("Acme Trading"), Some("active"), Some("client")));
        assert_eq!((v["created"].as_str(), v["updated"].as_str(), v["summary"].as_str()), (Some("2026-01-05"), Some("2026-11-02"), Some("Monthly books and VAT")));
        assert_eq!(v["tags"], json!(["vat", "monthly"]));
        assert!(v["readme"]["body"].as_str().unwrap().starts_with("# Acme Trading"));
        // Key dates come from their own section only, soonest first.
        assert_eq!(v["keyDates"], json!([{"date": "2026-11-30", "text": "VAT return due"}, {"date": "2026-12-31", "text": "Year end · Engagement letter"}]));
        assert_eq!(v["todo"]["file"], "clients/acme/todo.md");
        assert_eq!(v["todo"]["exists"], true);
        assert_eq!(v["todo"]["groups"], json!(["Now", "Done"]));
        assert_eq!((v["todo"]["items"][0]["line"].as_u64(), v["todo"]["items"][0]["due"].as_str()), (Some(1), Some("2026-11-20")));
        assert_eq!(v["todo"]["items"][1]["done"], true);
        // Meetings newest first; the date comes from the note details or the file name.
        assert_eq!(paths(&v["meetings"]), ["clients/acme/meetings/2026-11-02-october-review.md", "clients/acme/meetings/2026-10-01-kick-off.md"]);
        assert_eq!((v["meetings"][0]["date"].as_str(), v["meetings"][0]["title"].as_str()), (Some("2026-11-02"), None));
        assert_eq!(v["meetings"][1]["title"], "Kick-off");
        assert_eq!(v["meetings"][1]["attendees"], json!(["Sara (CFO)", "Omar"]));
        assert_eq!(paths(&v["notes"]).len(), 2);
        assert!(v["notes"].as_array().unwrap().iter().any(|n| n["title"] == "Fees" && n["summary"] == "What we charge"));
        assert_eq!(paths(&v["outputs"]).len(), 2);
        assert!(paths(&v["outputs"]).contains(&"clients/acme/outputs/letters/2026-11-01-reminder.docx".to_string()));
        assert_eq!(paths(&v["other"]), ["clients/acme/contract.pdf"]);
        // The log newest first, whatever order it is written in.
        assert_eq!(v["log"]["exists"], true);
        assert_eq!(v["log"]["entries"][0]["date"], "2026-11-02");
        assert_eq!(v["log"]["entries"][0]["topic"], "October close");
        assert_eq!(v["log"]["entries"][0]["body"], "- Learned: invoices arrive late\n- Next: chase the supplier");
        assert_eq!(v["log"]["entries"][1]["topic"], "Kick-off");
        assert!(v["log"]["archive"].is_null());
        // The same item by its folder; anything that is not an item is refused.
        assert_eq!(item(&d, "clients/acme", &[]).unwrap()["path"], "clients/acme/README.md");
        assert_eq!(item(&d, "/clients/acme/", &[]).unwrap()["title"], "Acme Trading");
        for bad in ["", "clients", "clients/acme/notes", "clients/acme/notes/fees.md", "me/x", "inbox/x", ".claude/x", "../x/y", "clients/../x", "clients/missing"] {
            assert!(item(&d, bad, &[]).is_err(), "{bad:?}");
        }
        // Private folders are left out the way the file tree leaves them out.
        write(&d, "clients/acme/notes/salaries/2026.md", "secret").unwrap();
        assert_eq!(paths(&item(&d, "clients/acme", &[]).unwrap()["notes"]).len(), 3);
        assert_eq!(paths(&item(&d, "clients/acme", &["clients/acme/notes/salaries".into()]).unwrap()["notes"]).len(), 2);
        for private in ["clients/acme", "Clients", "clients/acme/README.md"] {
            let err = item(&d, "clients/acme", &[private.to_string()]).unwrap_err();
            assert_eq!(crate::tr::classify(&err).code, "private", "{private}");
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    /// FEAT-17: nothing of a private folder reaches the item's page through a link inside an item
    /// that is not private, or under another letter case.
    #[cfg(unix)]
    #[test]
    fn links_from_an_item_into_a_private_folder_are_not_followed() {
        let d = tmp();
        let link = |to: &str, at: &str| std::os::unix::fs::symlink(to, d.join(at)).unwrap();
        write(&d, "me/finances/README.md", "---\ntitle: Money\n---\nsalary 9000\n").unwrap();
        write(&d, "me/finances/secret.md", "---\ntitle: Salary\nsummary: salary 9000\n---\nsalary 9000\n").unwrap();
        write(&d, "me/finances/todo.md", "- [ ] pay the tax\n").unwrap();
        write(&d, "me/finances/log.md", "## 2026-01-01 — salary 9000\n").unwrap();
        write(&d, "me/finances/2026/jan.md", "---\ntitle: January\n---\nsalary 9000\n").unwrap();
        // An ordinary client whose notes, todo list and log lead into the private folder.
        write(&d, "clients/beta/README.md", "---\ntitle: Beta\n---\nBody\n").unwrap();
        write(&d, "clients/beta/notes/own.md", "---\ntitle: Own note\n---\n").unwrap();
        link("../../../me/finances/secret.md", "clients/beta/notes/link.md");
        link("../../../me/finances/2026", "clients/beta/notes/year");
        link("../../me/finances/todo.md", "clients/beta/todo.md");
        link("../../me/finances/log.md", "clients/beta/log.md");
        // A client folder that is itself a link to the private folder.
        link("../me/finances", "clients/gamma");
        for private in [vec!["me/finances".to_string()], vec!["Me/Finances".to_string()], vec!["me".to_string()]] {
            let v = item(&d, "clients/beta", &private).unwrap();
            let text = v.to_string();
            assert!(!text.contains("salary") && !text.contains("pay the tax") && !text.contains("January"), "{private:?}: {text}");
            assert_eq!(v["notes"].as_array().unwrap().iter().map(|n| n["path"].as_str().unwrap()).collect::<Vec<_>>(), ["clients/beta/notes/own.md"]);
            assert_eq!((v["todo"]["exists"].as_bool(), v["todo"]["items"].as_array().unwrap().len()), (Some(false), 0));
            assert_eq!((v["log"]["exists"].as_bool(), v["log"]["entries"].as_array().unwrap().len()), (Some(false), 0));
            for path in ["clients/gamma", "clients/gamma/README.md", "CLIENTS/GAMMA"] {
                let err = item(&d, path, &private).unwrap_err();
                assert_eq!(crate::tr::classify(&err).code, if path.starts_with("CLIENTS") && !d.join("CLIENTS").exists() { "not_found" } else { "private" }, "{path}");
            }
        }
        // The user's own page (nothing excluded) reads the item's own list and log, as the Todos page does;
        // links to single files inside notes are still not listed (the file tree leaves them out too).
        let v = item(&d, "clients/beta", &[]).unwrap();
        assert_eq!(v["todo"]["items"][0]["title"], "pay the tax");
        assert!(!v["notes"].as_array().unwrap().iter().any(|n| n["path"] == "clients/beta/notes/link.md"));
        std::fs::remove_dir_all(d).unwrap();
    }

    /// FEAT-17: a new idea with only its front page, and a front page without note details.
    #[test]
    fn a_seed_item_with_only_a_readme_has_empty_parts() {
        let d = tmp();
        write(&d, "ideas/shop/README.md", "---\ntitle: Shop\nstatus: seed\n---\nJust an idea.\n").unwrap();
        let v = item(&d, "ideas/shop", &[]).unwrap();
        assert_eq!((v["title"].as_str(), v["status"].as_str(), v["details"].as_bool()), (Some("Shop"), Some("seed"), Some(true)));
        assert_eq!(v["readme"]["body"], "Just an idea.\n");
        assert_eq!(v["todo"], json!({"file": "ideas/shop/todo.md", "exists": false, "groups": [], "items": []}));
        for part in ["meetings", "notes", "outputs", "other", "keyDates"] { assert_eq!(v[part], json!([]), "{part}"); }
        assert_eq!(v["log"], json!({"path": "ideas/shop/log.md", "exists": false, "archive": null, "entries": []}));
        assert_eq!(v["tags"], json!([]));
        // No note details at all: still a page, named after its folder.
        write(&d, "ideas/plain/README.md", "# Plain page\n").unwrap();
        let v = item(&d, "ideas/plain/README.md", &[]).unwrap();
        assert_eq!((v["title"].as_str(), v["details"].as_bool(), v["status"].as_str()), (Some("plain"), Some(false), Some("")));
        // Details a strict reader rejects: still read, and the body starts after them.
        write(&d, "ideas/loose/README.md", "---\ntitle: Client: Acme\nstatus: active\n---\nBody here\n").unwrap();
        let v = item(&d, "ideas/loose", &[]).unwrap();
        assert_eq!((v["title"].as_str(), v["readme"]["body"].as_str()), (Some("Client: Acme"), Some("Body here\n")));
        // An older log moved to the archive is pointed to.
        write(&d, "ideas/shop/log-archive.md", "## 2025-01-01 — Old\n").unwrap();
        assert_eq!(item(&d, "ideas/shop", &[]).unwrap()["log"]["archive"], "ideas/shop/log-archive.md");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// FEAT-17: Arabic folder and file names, Arabic headings, and meeting notes named in either script.
    #[test]
    fn items_with_arabic_names_and_new_meeting_notes() {
        let d = tmp();
        write(&d, "العملاء/شركة-الأفق/README.md", "---\ntitle: شركة الأفق\nkind: client\nstatus: active\n---\n\n## التواريخ المهمة\n- 2026-12-15 — موعد تقديم الإقرار\n\n## الوضع الحالي\nتم إقفال أكتوبر.\n").unwrap();
        write(&d, "العملاء/شركة-الأفق/meetings/2026-11-01-اجتماع-الانطلاق.md", "---\ntitle: اجتماع الانطلاق\ndate: 2026-11-01\nattendees: [سارة]\n---\n").unwrap();
        write(&d, "العملاء/شركة-الأفق/todo.md", "- [ ] إرسال التقرير @due(2026-11-20)\n").unwrap();
        write(&d, "العملاء/شركة-الأفق/notes/خطة.md", "---\ntitle: الخطة\n---\n").unwrap();
        write(&d, "العملاء/شركة-الأفق/log.md", "## 2026-11-01 — بداية العمل\n- تقرر: إقفال شهري\n").unwrap();
        std::fs::create_dir_all(d.join("العملاء/شركة-الأفق/outputs")).unwrap();
        std::fs::write(d.join("العملاء/شركة-الأفق/outputs/تقرير-أكتوبر.pdf"), b"%PDF").unwrap();
        let v = item(&d, "العملاء/شركة-الأفق", &[]).unwrap();
        assert_eq!((v["area"].as_str(), v["slug"].as_str(), v["title"].as_str()), (Some("العملاء"), Some("شركة-الأفق"), Some("شركة الأفق")));
        assert_eq!(v["keyDates"], json!([{"date": "2026-12-15", "text": "موعد تقديم الإقرار"}]));
        assert_eq!(v["meetings"][0]["title"], "اجتماع الانطلاق");
        assert_eq!(v["meetings"][0]["attendees"], json!(["سارة"]));
        assert_eq!(v["todo"]["items"][0]["title"], "إرسال التقرير");
        assert_eq!(v["notes"][0]["path"], "العملاء/شركة-الأفق/notes/خطة.md");
        assert_eq!(v["outputs"][0]["name"], "تقرير-أكتوبر.pdf");
        assert_eq!(v["log"]["entries"][0]["topic"], "بداية العمل");

        // A new meeting note: Arabic words without diacritics, as the folder names are Arabic.
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let p = new_meeting(&d, "العملاء/شركة-الأفق/README.md", "  مراجعةُ  الحسابات: الشهريّة!  ", day, day, true).unwrap();
        assert_eq!(p, "العملاء/شركة-الأفق/meetings/2026-11-12-مراجعة-الحسابات-الشهرية.md");
        let text = std::fs::read_to_string(d.join(&p)).unwrap();
        assert!(text.starts_with("---\ntitle: \"مراجعةُ الحسابات: الشهريّة!\"\ndate: 2026-11-12\nattendees: []\nupdated: 2026-11-12\nsummary: \n---\n\n# مراجعةُ الحسابات: الشهريّة!\n\n## ما نوقش\n"), "{text}");
        assert!(text.contains("\n## القرارات\n") && text.contains("\n## بنود العمل\n"));
        let (fm, lenient) = note_details(&text).unwrap();
        assert!(!lenient && fm["title"] == "مراجعةُ الحسابات: الشهريّة!" && fm["date"] == "2026-11-12");
        // Never replaces one: the same name on the same day gets a number.
        assert_eq!(new_meeting(&d, "العملاء/شركة-الأفق", "مراجعة الحسابات الشهرية", day, day, true).unwrap(), "العملاء/شركة-الأفق/meetings/2026-11-12-مراجعة-الحسابات-الشهرية-2.md");
        assert_eq!(item(&d, "العملاء/شركة-الأفق", &[]).unwrap()["meetings"].as_array().unwrap().len(), 3);

        // Latin names (from the folder, or from the library's rules); English headings; an empty title.
        write(&d, "clients/acme/README.md", "---\ntitle: Acme\n---\n").unwrap();
        let earlier = chrono::NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        let p = new_meeting(&d, "clients/acme", "Kick-off: Sara & Omar (VAT)", earlier, day, false).unwrap();
        assert_eq!(p, "clients/acme/meetings/2026-11-03-kick-off-sara-omar-vat.md");
        let text = std::fs::read_to_string(d.join(&p)).unwrap();
        assert!(text.contains("date: 2026-11-03\n") && text.contains("updated: 2026-11-12\n") && text.contains("\n## Discussed\n") && text.contains("\n## Action items\n"));
        assert_eq!(new_meeting(&d, "clients/acme", "  ", day, day, false).unwrap(), "clients/acme/meetings/2026-11-12-meeting.md");
        assert!(std::fs::read_to_string(d.join("clients/acme/meetings/2026-11-12-meeting.md")).unwrap().contains("title: \"Meeting\"\n"));
        // An Arabic title in a library with Latin names: the name falls back to "meeting".
        write(&d, "CLAUDE.md", "# Rules\n\n## About this library\n- Names: Latin letters — folder and file names follow this everywhere\n").unwrap();
        assert_eq!(new_meeting(&d, "clients/acme", "اجتماع مع سارة", day, day, true).unwrap(), "clients/acme/meetings/2026-11-12-meeting-2.md");
        // The rules saying Arabic win over a Latin folder name.
        write(&d, "CLAUDE.md", "## About this library\n- Names: Arabic — folder and file names follow this everywhere\n").unwrap();
        assert_eq!(new_meeting(&d, "clients/acme", "اجتماع مع سارة", day, day, true).unwrap(), "clients/acme/meetings/2026-11-12-اجتماع-مع-سارة.md");
        // Not an item: refused, nothing written.
        for bad in ["clients/missing", "clients", ".claude/x", "me/x", "../x/y"] { assert!(new_meeting(&d, bad, "x", day, day, false).is_err(), "{bad}"); }
        assert!(!d.join("clients/missing").exists());
        assert_eq!(name_from_title("Q3 — review, part 2/3", false), "q3-review-part-2-3");
        assert_eq!(name_from_title("one two three four five six seven", false), "one-two-three-four-five-six");
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

    fn kit_files(root: &Path, under: &str) -> Vec<String> {
        let mut out: Vec<String> = WalkDir::new(root.join(under)).into_iter().flatten().filter(|e| e.file_type().is_file())
            .filter_map(|e| rel_of(root, e.path())).collect();
        out.sort();
        out
    }

    #[test]
    fn setup_leaves_no_welcome_pages_setup_routine_or_presets_at_the_top() {
        let d = tmp();
        create_from_starter(&d).unwrap();
        assert!(awaits_setup(&d));
        let shipped = kit_files(&d, "presets");
        assert!(shipped.len() >= 12 && shipped.iter().any(|p| p.starts_with("presets/ar/")), "{shipped:?}");
        assert!(d.join("START-HERE.md").exists() && d.join("START-HERE.ar.md").exists());
        finish_setup(&d);
        assert!(!awaits_setup(&d) && !d.join(SETUP_ROUTINE).exists());
        assert!(!d.join("START-HERE.md").exists() && !d.join("START-HERE.ar.md").exists(), "both welcome pages go");
        assert!(!d.join("presets").exists(), "nothing of the kit's presets stays at the top");
        // Every preset is now under .claude/presets, byte for byte, in the same sub-folders.
        let installed = kit_files(&d, ".claude/presets");
        assert_eq!(installed, shipped.iter().map(|p| format!(".claude/{p}")).collect::<Vec<_>>());
        for p in &shipped {
            assert_eq!(std::fs::read(d.join(".claude").join(p)).unwrap(), STARTER.get_file(p).unwrap().contents(), "{p}");
        }
        // The rest of the kit is untouched, and a second pass changes nothing.
        assert!(d.join("CLAUDE.md").exists() && d.join(".claude/skills/wrap/SKILL.md").exists());
        finish_setup(&d);
        assert_eq!(kit_files(&d, ".claude/presets"), installed);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tidying_presets_never_touches_what_is_not_the_kits() {
        // The assistant already installed them (the routine run in a terminal): the top copy goes.
        let d = tmp();
        create_from_starter(&d).unwrap();
        std::fs::create_dir_all(d.join(".claude/presets")).unwrap();
        std::fs::write(d.join(".claude/presets/legal.md"), "installed earlier").unwrap();
        // The user keeps something of their own in a folder with the same name.
        std::fs::write(d.join("presets/my-own-template.md"), "mine").unwrap();
        std::fs::create_dir_all(d.join("presets/drafts")).unwrap();
        std::fs::write(d.join("presets/drafts/x.md"), "mine too").unwrap();
        assert!(install_presets(&d));
        assert_eq!(std::fs::read_to_string(d.join(".claude/presets/legal.md")).unwrap(), "installed earlier", "an installed preset is never replaced");
        assert!(d.join(".claude/presets/ar/legal.md").exists() && !d.join("presets/legal.md").exists() && !d.join("presets/ar").exists());
        assert_eq!(kit_files(&d, "presets"), ["presets/drafts/x.md", "presets/my-own-template.md"]);
        assert!(!install_presets(&d), "nothing left to do");
        std::fs::remove_dir_all(&d).unwrap();
        // No presets folder, or a link in its place: nothing happens.
        let d = tmp();
        assert!(!install_presets(&d));
        #[cfg(unix)]
        {
            let elsewhere = tmp();
            std::fs::write(elsewhere.join("legal.md"), "not ours").unwrap();
            std::os::unix::fs::symlink(&elsewhere, d.join("presets")).unwrap();
            assert!(!install_presets(&d));
            assert!(elsewhere.join("legal.md").exists() && !d.join(".claude").exists());
            std::fs::remove_dir_all(&elsewhere).unwrap();
            // A link where the presets are installed, or inside the kit's folder: nothing is moved
            // or written through it, at setup or when an older library is tidied.
            for tidy in [false, true] {
                let (lib, outside) = (tmp(), tmp());
                create_from_starter(&lib).unwrap();
                if tidy { std::fs::remove_dir_all(lib.join(SETUP_ROUTINE)).unwrap(); }
                std::fs::create_dir_all(lib.join(".claude")).unwrap();
                std::os::unix::fs::symlink(&outside, lib.join(".claude/presets")).unwrap();
                if tidy { tidy_kit_leftovers(&lib); } else { install_presets(&lib); }
                assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0, "nothing lands where the link leads");
                assert!(lib.join("presets/legal.md").exists() && lib.join("presets/ar/legal.md").exists());
                std::fs::remove_file(lib.join(".claude/presets")).unwrap();
                // The Arabic presets reached through a link stay where they are, the rest is tidied.
                std::fs::rename(lib.join("presets/ar"), outside.join("ar")).unwrap();
                std::os::unix::fs::symlink(outside.join("ar"), lib.join("presets/ar")).unwrap();
                if tidy { tidy_kit_leftovers(&lib); } else { install_presets(&lib); }
                assert!(outside.join("ar/legal.md").exists() && !lib.join(".claude/presets/ar").exists());
                assert!(lib.join(".claude/presets/legal.md").exists() && !lib.join("presets/legal.md").exists());
                std::fs::remove_dir_all(lib).unwrap();
                std::fs::remove_dir_all(outside).unwrap();
            }
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn leftovers_of_an_older_setup_are_cleared_but_nothing_of_the_users() {
        // What 0.0.8 left in every new library: the Arabic welcome page and the presets at the top.
        let d = tmp();
        create_from_starter(&d).unwrap();
        // Not set up yet: hands off.
        assert_eq!(tidy_kit_leftovers(&d), 0);
        assert!(d.join("START-HERE.ar.md").exists() && d.join("presets/legal.md").exists());
        std::fs::remove_dir_all(d.join(SETUP_ROUTINE)).unwrap();
        std::fs::remove_file(d.join("START-HERE.md")).unwrap();
        // The user rewrote one preset and keeps a file of their own beside the kit's.
        std::fs::write(d.join("presets/teaching.md"), "my own teaching notes").unwrap();
        std::fs::write(d.join("presets/price-list.md"), "mine").unwrap();
        let shipped = STARTER.get_dir("presets").unwrap().files().count() + STARTER.get_dir("presets/ar").unwrap().files().count();
        assert_eq!(tidy_kit_leftovers(&d), 1 + shipped - 1, "the Arabic welcome page and every untouched preset");
        assert!(!d.join("START-HERE.ar.md").exists() && !d.join("presets/legal.md").exists() && !d.join("presets/ar").exists());
        assert_eq!(std::fs::read_to_string(d.join("presets/teaching.md")).unwrap(), "my own teaching notes");
        assert_eq!(std::fs::read_to_string(d.join("presets/price-list.md")).unwrap(), "mine");
        assert_eq!(std::fs::read(d.join(".claude/presets/ar/legal.md")).unwrap(), STARTER.get_file("presets/ar/legal.md").unwrap().contents());
        assert!(!d.join(".claude/presets/teaching.md").exists(), "a preset the user changed is not installed in their name");
        // The app's own files only: the folder still needs no trust decision, and nothing is left to do.
        assert!(crate::trust::findings(&d).is_empty() && crate::trust::require(&d).is_ok());
        assert_eq!(tidy_kit_leftovers(&d), 0);
        // A welcome page the user edited stays; so does everything in a folder that never was a library.
        std::fs::write(d.join("START-HERE.md"), "my own start page").unwrap();
        assert_eq!(tidy_kit_leftovers(&d), 0);
        assert!(d.join("START-HERE.md").exists());
        std::fs::remove_dir_all(&d).unwrap();
        let other = tmp();
        std::fs::create_dir_all(other.join("presets")).unwrap();
        std::fs::write(other.join("presets/legal.md"), STARTER.get_file("presets/legal.md").unwrap().contents()).unwrap();
        assert_eq!(tidy_kit_leftovers(&other), 0);
        assert!(other.join("presets/legal.md").exists());
        // The 0.0.8 list names real kit files, each once.
        for (rel, hash) in KIT_0_0_8 {
            assert!(STARTER.get_file(rel).is_some() && hash.len() == 64, "{rel}");
            assert_eq!(KIT_0_0_8.iter().filter(|(r, _)| r == rel).count(), 1, "{rel}");
        }
        std::fs::remove_dir_all(other).unwrap();
    }

    #[test]
    fn a_new_library_goes_into_a_free_folder_or_an_untouched_copy_of_the_kit() {
        let d = tmp();
        let lib = d.join("Thoughts");
        assert_eq!(folder_state(&lib), FolderState::Missing);
        create_from_starter(&lib).unwrap();
        assert_eq!(folder_state(&lib), FolderState::Starter);
        // A setup that stopped half-way: routines of features not chosen were removed, history was
        // started, the system left its own droppings. Trying again on the same folder works.
        std::fs::remove_dir_all(lib.join(".claude/skills/check-in")).unwrap();
        std::fs::create_dir_all(lib.join(".git/objects")).unwrap();
        std::fs::write(lib.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(lib.join(".DS_Store"), [0u8; 4]).unwrap();
        assert_eq!(check_new_library(&lib).unwrap(), FolderState::Starter);
        create_from_starter(&lib).unwrap();
        assert!(lib.join(".claude/skills/check-in/SKILL.md").exists(), "the kit is whole again");
        // Once anything in it has changed, or something was added, it is a library of its own.
        std::fs::write(lib.join("INDEX.md"), "# My index\n").unwrap();
        assert_eq!(folder_state(&lib), FolderState::Library);
        let err = create_from_starter(&lib).unwrap_err();
        assert_eq!(crate::tr::classify(&err).code, "library_exists");
        assert_eq!(std::fs::read_to_string(lib.join("INDEX.md")).unwrap(), "# My index\n", "nothing was overwritten");
        create_from_starter(&d.join("again")).unwrap();
        std::fs::write(d.join("again/notes.md"), "mine").unwrap();
        assert_eq!(folder_state(&d.join("again")), FolderState::Library);
        // A set-up library (routine and welcome pages gone, presets moved) is a library too.
        create_from_starter(&d.join("done")).unwrap();
        finish_setup(&d.join("done"));
        assert_eq!(folder_state(&d.join("done")), FolderState::Library);
        // Other folders: empty ones are free (an empty repository too), anything else is refused untouched.
        std::fs::create_dir_all(d.join("empty/.git")).unwrap();
        std::fs::write(d.join("empty/.DS_Store"), [0u8; 4]).unwrap();
        assert_eq!(check_new_library(&d.join("empty")).unwrap(), FolderState::Empty);
        std::fs::create_dir_all(d.join("photos")).unwrap();
        std::fs::write(d.join("photos/a.jpg"), [1u8; 4]).unwrap();
        assert_eq!(folder_state(&d.join("photos")), FolderState::Files);
        assert_eq!(crate::tr::classify(&create_from_starter(&d.join("photos")).unwrap_err()).code, "validation");
        assert_eq!(std::fs::read_dir(d.join("photos")).unwrap().count(), 1);
        assert_eq!(folder_state(&d.join("photos/a.jpg")), FolderState::NotFolder);
        assert!(create_from_starter(&d.join("photos/a.jpg")).is_err());
        #[cfg(unix)]
        {
            // A link inside is not the kit's.
            create_from_starter(&d.join("linked")).unwrap();
            std::os::unix::fs::symlink(d.join("photos"), d.join("linked/more")).unwrap();
            assert_eq!(folder_state(&d.join("linked")), FolderState::Library);
        }
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

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn pictures_are_saved_beside_their_note_under_names_of_their_own() {
        let d = tmp();
        write(&d, "clients/acme/notes/visit.md", "# Visit\n").unwrap();
        write(&d, "plan.md", "x").unwrap();
        let save = |note: &str, name: &str, bytes: &[u8]| save_picture(&d, note, name, bytes);
        assert_eq!(save("clients/acme/notes/visit.md", "image.png", PNG).unwrap(),
                   ("clients/acme/notes/images/image.png".to_string(), "images/image.png".to_string()));
        // Never over an existing picture.
        assert_eq!(save("clients/acme/notes/visit.md", "image.png", PNG).unwrap().1, "images/image-2.png");
        assert_eq!(std::fs::read(d.join("clients/acme/notes/images/image.png")).unwrap(), PNG);
        // A note at the top of the library.
        assert_eq!(save("plan.md", "x.png", PNG).unwrap(), ("images/x.png".to_string(), "images/x.png".to_string()));
        // The kind comes from the content, not from the name it arrived with.
        assert_eq!(save("plan.md", "evil.svg", PNG).unwrap().1, "images/evil.png");
        assert_eq!(save("plan.md", "photo", b"\xFF\xD8\xFF\xE0rest").unwrap().1, "images/photo.jpg");
        assert_eq!(save("plan.md", "anim.gif", b"GIF89a....").unwrap().1, "images/anim.gif");
        assert_eq!(save("plan.md", "w.webp", b"RIFF\0\0\0\0WEBPVP8 ").unwrap().1, "images/w.webp");
        // Names that would need escaping in a link are tidied; Arabic stays.
        assert_eq!(save("plan.md", "My receipt (March) v1.2.png", PNG).unwrap().1, "images/My-receipt-March-v1-2.png");
        assert_eq!(save("plan.md", "إيصال الشهر.png", PNG).unwrap().1, "images/إيصال-الشهر.png");
        assert_eq!(save("plan.md", "../../.git/x.png", PNG).unwrap().1, "images/x-2.png");
        assert_eq!(save("plan.md", ".claude.png", PNG).unwrap().1, "images/claude.png");
        assert_eq!(save("plan.md", "", PNG).unwrap().1, "images/picture.png");
        assert_eq!(save("plan.md", "\u{202e}gnp\u{0}.exe", PNG).unwrap().1, "images/gnp.png");
        assert_eq!(save("plan.md", "con.png", PNG).unwrap().1, "images/picture-con.png");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn pictures_are_refused_when_they_are_not_pictures_or_have_nowhere_safe_to_go() {
        let d = tmp();
        write(&d, "notes/a.md", "x").unwrap();
        write(&d, "notes/data.csv", "x").unwrap();
        std::fs::create_dir_all(d.join(".claude")).unwrap();
        std::fs::write(d.join(".claude/n.md"), "x").unwrap();
        let code = |r: anyhow::Result<(String, String)>| r.unwrap_err().downcast::<crate::tr::UserErr>().map(|e| e.code).unwrap_or("other");
        // Not a picture (an SVG, a web page, a program), or too large.
        for bytes in [&b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"[..], &b"<html>"[..], &b"MZ\x90\0"[..], &b""[..]] {
            assert_eq!(code(save_picture(&d, "notes/a.md", "x.png", bytes)), "validation");
        }
        let mut big = PNG.to_vec();
        big.resize(PICTURE_MAX_BYTES + 1, 0);
        assert_eq!(code(save_picture(&d, "notes/a.md", "x.png", &big)), "too_large");
        // Only beside a note that is there, never in protected folders or outside the library.
        assert!(save_picture(&d, "notes/data.csv", "x.png", PNG).is_err());
        assert!(save_picture(&d, "notes/gone.md", "x.png", PNG).is_err());
        assert!(save_picture(&d, ".claude/n.md", "x.png", PNG).is_err());
        assert!(save_picture(&d, "../outside.md", "x.png", PNG).is_err());
        assert!(save_picture(&d, "/etc/x.md", "x.png", PNG).is_err());
        assert!(!d.join(".claude/images").exists());
        // An `images` that is a file is left alone.
        write(&d, "other/b.md", "x").unwrap();
        std::fs::write(d.join("other/images"), "not a folder").unwrap();
        assert!(save_picture(&d, "other/b.md", "x.png", PNG).is_err());
        assert_eq!(std::fs::read_to_string(d.join("other/images")).unwrap(), "not a folder");
        #[cfg(unix)]
        {
            // An `images` link that leads out of the library is not followed.
            let out = tmp();
            write(&d, "linked/c.md", "x").unwrap();
            std::os::unix::fs::symlink(&out, d.join("linked/images")).unwrap();
            assert!(save_picture(&d, "linked/c.md", "x.png", PNG).is_err());
            assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
            std::fs::remove_dir_all(out).unwrap();
        }
        std::fs::remove_dir_all(d).unwrap();
    }
}
