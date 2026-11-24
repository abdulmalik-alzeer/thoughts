//! Plugin and pack files as real files (0.7.0). A file someone shares is a zip archive:
//! `<name>.thoughts-plugin` for one plugin, `<name>.thoughts-pack` for everything else (Export all,
//! a feature pack, a full pack). At its root is `plugin.json`, what the file says it is, and next to
//! it every customisation file at its own layer path, as an ordinary file (`plugins/calendar.js`,
//! `custom.css`, `workflows/weekly.json`…). Unzipped, it is a folder of code anyone can read and edit.
//!
//! Nothing here decides anything. Reading turns the archive into the same pack value a legacy
//! `.json` pack file is (`packs.rs`), and every check of that path runs on it unchanged: path rules,
//! limits, the fingerprint worked out from the files (the one in `plugin.json` is only compared),
//! minApp and layer format, new / update / older. What reading adds is what only an archive can do
//! wrong: a zip bomb, links and special entries, names that climb out or hide, the same name twice.
//! No assistant takes part in reading, checking, staging or keeping a file.

use crate::packs::{self, MAX_FILES, MAX_FILE_BYTES, MAX_TOTAL_BYTES};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

/// What the file says it is, at the root of the archive.
pub const MANIFEST: &str = "plugin.json";
/// The version of this file format (`formatVersion` in `plugin.json`); a newer one is refused.
pub const FORMAT_VERSION: u64 = 1;
/// The file of one plugin (`format` in `plugin.json` and the extension).
pub const PLUGIN: &str = "thoughts-plugin";
/// Every other file: Export all, a feature pack, a full pack.
pub const PACK: &str = "thoughts-pack";
/// The archive as it arrives, at most (4 MB of text in all, stored uncompressed, fits with room).
pub const MAX_ARCHIVE_BYTES: usize = 5 * 1024 * 1024;
/// Entries in the archive's directory, at most: every file, a folder entry each, and a little more
/// (what a computer's own "Compress" adds, `__MACOSX/…`, is counted too).
pub const MAX_ENTRIES: usize = 2 * MAX_FILES + 16;
/// `plugin.json`, at most (a pack of everything lists its plugins there).
const MAX_MANIFEST_BYTES: usize = MAX_FILE_BYTES;

fn no(key: &str, args: &[(&str, &str)]) -> anyhow::Error { crate::tr::err(key, args) }

/// Is this the start of a zip archive?
pub fn is_archive(bytes: &[u8]) -> bool { bytes.starts_with(b"PK") }

/// The extension a pack is saved under: one plugin, or a pack.
pub fn extension(pack: &Value) -> &'static str { if pack["plugin"].is_object() { PLUGIN } else { PACK } }

/// `plugin.json` for `pack` (whose checked files are `files`): its kind, what its maker says it is,
/// what it needs (`minApp`, `layerFormat`, `apiVersion`) and the fingerprint of its files. A pack of
/// everything also lists its plugins (with which are on), as the legacy file did.
pub fn manifest(pack: &Value, files: &BTreeMap<String, String>) -> Value {
    let plugin = &pack["plugin"];
    let kind = if pack["kind"] == "feature" { "feature" } else { "full" };
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let mut m = json!({
        "format": extension(pack), "formatVersion": FORMAT_VERSION, "kind": kind,
        "name": text(&pack["name"]), "description": text(&pack["description"]),
        "minApp": plugin["minApp"].as_str().filter(|s| !s.is_empty()).unwrap_or(crate::bundles::first_app()),
        "layerFormat": plugin["layerFormat"].as_u64().unwrap_or(crate::bundles::FORMAT),
        "apiVersion": pack["apiVersion"].as_u64().unwrap_or(crate::applayer::API_VERSION),
        "appVersion": pack["appVersion"].as_str().unwrap_or(env!("CARGO_PKG_VERSION")),
        "fingerprint": packs::fingerprint(kind, files),
    });
    let id = [plugin["id"].as_str(), pack["id"].as_str()].into_iter().flatten().find(|i| !i.is_empty());
    if let Some(id) = id { m["id"] = json!(id); }
    if let Some(a) = pack["author"].as_str().filter(|s| !s.is_empty()) { m["author"] = json!(a); }
    let version = [pack["packVersion"].as_str(), plugin["version"].as_str()].into_iter().flatten().find(|v| !v.is_empty());
    if let Some(v) = version { m["version"] = json!(v); }
    if let Some(list) = pack["plugins"].as_array() { m["plugins"] = json!(list); }
    m
}

/// The pack value a `plugin.json` and the files beside it stand for: exactly what a legacy file of
/// the same pack would hold, so the same checks decide. Values are passed on as they are (not
/// cleaned here) for those checks to judge.
fn to_pack(m: &Value, files: BTreeMap<String, String>) -> Value {
    let mut pack = json!({"format": "thoughts-pack", "version": 1, "kind": m["kind"], "files": files});
    for (from, to) in [("name", "name"), ("description", "description"), ("appVersion", "appVersion"), ("apiVersion", "apiVersion"), ("fingerprint", "fingerprint"), ("id", "id"), ("author", "author"), ("version", "packVersion"), ("plugins", "plugins")] {
        if !m[from].is_null() { pack[to] = m[from].clone(); }
    }
    // A plugin's own lines (what it needs, and its id) are read from `plugin` (`bundles::pack_plugin`).
    if m["kind"] == "feature" {
        let mut p = json!({"name": m["name"], "version": m["version"]});
        for k in ["id", "minApp", "layerFormat"] { if !m[k].is_null() { p[k] = m[k].clone(); } }
        pack["plugin"] = p;
    }
    pack
}

/// The archive for `pack`: `plugin.json`, then every file at its own path, compressed. The same
/// pack always gives the same bytes (fixed dates and permissions).
pub fn write(pack: &Value) -> anyhow::Result<Vec<u8>> {
    use zip::write::SimpleFileOptions;
    let files = packs::check_pack(pack)?;
    let text = serde_json::to_string_pretty(&manifest(pack, &files))? + "\n";
    if text.len() > MAX_MANIFEST_BYTES { return Err(no("pack.too_big", &[])); }
    let opt = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default()).unix_permissions(0o644);
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file(MANIFEST, opt)?;
    z.write_all(text.as_bytes())?;
    for (path, body) in &files {
        z.start_file(path.as_str(), opt)?;
        z.write_all(body.as_bytes())?;
    }
    Ok(z.finish()?.into_inner())
}

/// What an export route answers: the file as base64 (`file`), the extension it is saved under, and
/// what the dialog shows afterwards (its name, kind and fingerprint).
pub fn answer(pack: &Value) -> anyhow::Result<Value> {
    let bytes = write(pack)?;
    let files = packs::check_pack(pack)?;
    let kind = if pack["kind"] == "feature" { "feature" } else { "full" };
    Ok(json!({"format": extension(pack), "extension": extension(pack), "name": pack["name"], "kind": kind,
        "fingerprint": packs::fingerprint(kind, &files), "bytes": bytes.len(),
        "file": base64::engine::general_purpose::STANDARD.encode(bytes)}))
}

/// Things a computer adds when it zips a folder, never part of a customisation: skipped, still
/// counted against the limits.
fn junk(name: &str) -> bool {
    name.starts_with("__MACOSX/") || name.trim_end_matches('/').rsplit('/').next() == Some(".DS_Store")
}

/// A name in the archive that must never be followed, whatever else it is: absolute, climbing out,
/// with a backslash, a drive or anything invisible.
fn climbs(name: &str) -> bool {
    name.starts_with('/') || name.contains('\\') || name.contains(':')
        || name.chars().any(|c| c.is_control() || crate::staging::validate::is_hidden_char(c))
        || name.trim_end_matches('/').split('/').any(|p| p.is_empty() || p == "." || p == "..")
}

/// How many entries the archive's own directory says it has (its end record, or the zip64 one it
/// points to). The zip reader keeps one entry per name, so the very same name written twice would
/// otherwise pass as one, while unzipping on a computer gives the other copy: the counts must agree.
fn directory_count(bytes: &[u8]) -> Option<u64> {
    let le = |at: usize, n: usize| -> Option<u64> { bytes.get(at..at + n).map(|b| b.iter().rev().fold(0u64, |v, x| (v << 8) | *x as u64)) };
    let from = bytes.len().saturating_sub(22 + 65535);
    let end = (from..=bytes.len().saturating_sub(22)).rev()
        .find(|&i| bytes[i..].starts_with(b"PK\x05\x06") && le(i + 20, 2).map(|c| i + 22 + c as usize) == Some(bytes.len()))?;
    let count = le(end + 10, 2)?;
    if count != 0xffff { return Some(count); }
    let locator = end.checked_sub(20).filter(|&l| bytes[l..].starts_with(b"PK\x06\x07"))?;
    let at = usize::try_from(le(locator + 8, 8)?).ok()?;
    if !bytes.get(at..)?.starts_with(b"PK\x06\x06") { return None; }
    le(at + 32, 8)
}

/// Read a plugin or pack archive into the pack value it stands for (see the module comment).
/// Refusals are in the UI language and nothing is written anywhere.
pub fn read(bytes: &[u8]) -> anyhow::Result<Value> {
    if bytes.len() > MAX_ARCHIVE_BYTES { return Err(no("pack.archive_too_big", &[])); }
    let damaged = || no("pack.archive_damaged", &[]);
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| damaged())?;
    if zip.len() > MAX_ENTRIES { return Err(no("pack.too_many", &[("count", &MAX_FILES.to_string())])); }
    if directory_count(bytes) != Some(zip.len() as u64) { return Err(damaged()); }
    // 1. Every entry by its directory record, nothing unpacked yet.
    let (mut entries, mut dirs): (Vec<(usize, String)>, Vec<String>) = (vec![], vec![]);
    for i in 0..zip.len() {
        let f = zip.by_index_raw(i).map_err(|_| damaged())?;
        // One name only: the bytes as written, read as UTF-8, and no other name carried beside them.
        let name = std::str::from_utf8(f.name_raw()).ok().filter(|n| f.name().ok().as_deref() == Some(*n)).ok_or_else(|| no("pack.archive_bad_name", &[]))?.to_string();
        let shown = packs::shown_path(&name);
        if climbs(&name) { return Err(no("pack.bad_file", &[("path", &shown)])); }
        if f.encrypted() { return Err(no("pack.archive_locked", &[])); }
        if !matches!(f.compression(), zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated) { return Err(no("pack.archive_method", &[])); }
        // Only plain files and folders: no link, device, pipe or socket, and a folder is named as one.
        let dir = name.ends_with('/');
        let mode = f.unix_mode().map_or(0, |m| m & 0o170000);
        let plain = mode == 0 || (mode == 0o100000 && !dir) || (mode == 0o040000 && dir);
        if f.is_symlink() || !plain { return Err(no("pack.archive_odd_entry", &[("path", &shown)])); }
        if junk(&name) { continue; }
        if dir { dirs.push(name) } else { entries.push((i, name)) }
    }
    // 2. A folder zipped as a whole (a computer's own "Compress"): one folder around everything,
    //    with `plugin.json` inside it, is that folder's contents.
    let prefix = if entries.iter().any(|(_, n)| n == MANIFEST) { String::new() } else {
        let top = |n: &str| n.split('/').next().unwrap_or_default().to_string();
        let tops: BTreeSet<String> = entries.iter().map(|(_, n)| top(n)).chain(dirs.iter().map(|d| top(d))).collect();
        match tops.iter().next() {
            Some(one) if tops.len() == 1 && !one.starts_with('.') && entries.iter().any(|(_, n)| *n == format!("{one}/{MANIFEST}")) => format!("{one}/"),
            _ => return Err(no("pack.archive_no_manifest", &[])),
        }
    };
    let inside = |n: &str| n.strip_prefix(prefix.as_str()).map(str::to_string).ok_or_else(|| no("pack.bad_file", &[("path", &packs::shown_path(n))]));
    for d in &dirs {
        let rel = inside(d)?;
        let rel = rel.trim_end_matches('/');
        if !(rel.is_empty() || packs::TOP_DIRS.contains(&rel) || packs::check_path(rel).is_ok()) { return Err(no("pack.bad_file", &[("path", &packs::shown_path(d))])); }
    }
    // 3. Each file once (also when names differ only in capitals, which one disk would make one
    //    file), each in a place a customisation may live.
    let (mut seen, mut folded) = (BTreeSet::new(), BTreeSet::new());
    let mut manifest_at = None;
    let mut wanted = vec![];
    for (i, name) in &entries {
        let rel = inside(name)?;
        if !seen.insert(rel.clone()) || !folded.insert(rel.to_lowercase()) { return Err(no("pack.archive_duplicate", &[("path", &packs::shown_path(&rel))])); }
        if rel == MANIFEST { manifest_at = Some(*i); continue; }
        if packs::check_path(&rel).is_err() { return Err(no("pack.bad_file", &[("path", &packs::shown_path(&rel))])); }
        wanted.push((*i, rel));
    }
    let manifest_at = manifest_at.ok_or_else(|| no("pack.archive_no_manifest", &[]))?;
    if wanted.is_empty() { return Err(no("pack.empty", &[])); }
    if wanted.len() > MAX_FILES { return Err(no("pack.too_many", &[("count", &MAX_FILES.to_string())])); }
    // 4. Unpacked, counting what really comes out (never what the archive says it holds).
    let text_of = |zip: &mut zip::ZipArchive<Cursor<&[u8]>>, i: usize, rel: &str, limit: usize| -> anyhow::Result<String> {
        let f = zip.by_index(i).map_err(|_| damaged())?;
        let big = || no("pack.file_too_big", &[("path", &packs::shown_path(rel))]);
        if f.size() > limit as u64 { return Err(big()); }
        let mut buf = Vec::new();
        f.take(limit as u64 + 1).read_to_end(&mut buf).map_err(|_| damaged())?;
        if buf.len() > limit { return Err(big()); }
        String::from_utf8(buf).map_err(|_| no("pack.not_text", &[("path", &packs::shown_path(rel))]))
    };
    let m: Value = serde_json::from_str(&text_of(&mut zip, manifest_at, MANIFEST, MAX_MANIFEST_BYTES)?).map_err(|_| no("pack.not_pack", &[]))?;
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for (i, rel) in &wanted {
        let text = text_of(&mut zip, *i, rel, MAX_FILE_BYTES)?;
        total += text.len();
        if total > MAX_TOTAL_BYTES { return Err(no("pack.too_big", &[])); }
        files.insert(rel.clone(), text);
    }
    // 5. What `plugin.json` says, then the pack's own checks.
    if !m.is_object() || (m["format"] != PLUGIN && m["format"] != PACK) { return Err(no("pack.not_pack", &[])); }
    match m["formatVersion"].as_u64() {
        Some(FORMAT_VERSION) => {}
        Some(v) if v > FORMAT_VERSION => return Err(no("pack.newer", &[])),
        _ => return Err(no("pack.not_pack", &[])),
    }
    if !m["layerFormat"].is_null() && m["layerFormat"].as_u64().is_none() { return Err(no("pack.newer", &[])); }
    // A pack of everything is never a plugin, so `bundles::check_compatible` does not see it: what
    // it needs is checked here.
    if m["kind"] == "full" {
        if let Some(v) = m["minApp"].as_str().filter(|v| crate::bundles::needs_newer_app(v)) { return Err(no("plugin.newer_app", &[("version", &packs::clean_line(v, 20))])); }
        if m["layerFormat"].as_u64().map_or(false, |f| f > crate::bundles::FORMAT) { return Err(no("pack.newer", &[])); }
    }
    let pack = to_pack(&m, files);
    packs::check_pack(&pack)?;
    Ok(pack)
}

/// A file as it was chosen: an archive, or a legacy `.json` pack or plugin file (still read as it is).
pub fn open(bytes: &[u8]) -> anyhow::Result<Value> {
    if is_archive(bytes) { return read(bytes); }
    if bytes.len() > MAX_ARCHIVE_BYTES { return Err(no("pack.archive_too_big", &[])); }
    let text = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    serde_json::from_slice::<Value>(text).ok().filter(Value::is_object).ok_or_else(|| no("pack.not_pack", &[]))
}

/// The pack of a request: `file` (the chosen file's bytes, base64) when given, else a pack value
/// sent as it is (`pack`, what older windows send).
pub fn received(pack: Option<Value>, file: Option<&str>) -> anyhow::Result<Value> {
    match (file, pack) {
        (Some(b64), _) => {
            if b64.len() > MAX_ARCHIVE_BYTES / 3 * 4 + 16 { return Err(no("pack.archive_too_big", &[])); }
            let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| no("pack.not_pack", &[]))?;
            open(&bytes)
        }
        (None, Some(p)) => Ok(p),
        (None, None) => Err(no("pack.not_pack", &[])),
    }
}

#[cfg(test)]
#[path = "pack_file_tests.rs"]
mod tests;
