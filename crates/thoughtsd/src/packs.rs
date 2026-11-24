//! Customisation packs: what is shared of the layer, as one value (`format: "thoughts-pack"`, every
//! file as text under `files`). Since 0.7.0 it travels as a zip of the real files (`pack_file.rs`),
//! which is turned into this same value before any check; a legacy `.json` file is this value as it
//! is. Importing a pack never applies it — it is validated and staged as a pending change the user
//! previews first.

use crate::applayer::{Dirs, Git};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const MAX_FILES: usize = 200;
pub const MAX_FILE_BYTES: usize = 512 * 1024;
pub const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;

// (`addons.json`, the switched-off list, is not one: only the app's own switch and remove write it,
// so no pack, review or outside edit can switch an add-on back on.)
const TOP_FILES: &[&str] = &["custom.css", "themes.json", "home.json", "manifest.json", "todos.json", "nav.json", "pages.json", "keys.json"];
pub(crate) const TOP_DIRS: &[&str] = &["plugins", "views", "workflows"];

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
    if !ok { return Err(bad("not a customisation file (allowed: custom.css, themes.json, home.json, nav.json, todos.json, pages.json, keys.json, manifest.json, plugins/, views/, workflows/)")); }
    Ok(())
}

/// A file name from a pack inside one of the app's sentences: one line, anything invisible written out.
pub(crate) fn shown_path(path: &str) -> String { clean_line(path, 120) }

/// Validate a pack and return its files. Refusals are in the UI language.
pub fn check_pack(pack: &Value) -> anyhow::Result<BTreeMap<String, String>> {
    let no = |key: &str, args: &[(&str, &str)]| crate::tr::err(key, args);
    if pack["format"] != "thoughts-pack" { return Err(no("pack.not_pack", &[])); }
    if pack["version"] != 1 { return Err(no("pack.newer", &[])); }
    // Older packs carry no apiVersion: they are version 1.
    if !pack["apiVersion"].is_null() && pack["apiVersion"].as_u64().map_or(true, |v| v == 0 || v > crate::applayer::API_VERSION) {
        return Err(no("pack.newer", &[]));
    }
    if !pack["kind"].is_null() && pack["kind"] != "full" && pack["kind"] != "feature" { return Err(no("pack.newer", &[])); }
    let files = pack["files"].as_object().ok_or_else(|| no("pack.empty", &[]))?;
    if files.is_empty() { return Err(no("pack.empty", &[])); }
    if files.len() > MAX_FILES { return Err(no("pack.too_many", &[("count", &MAX_FILES.to_string())])); }
    let mut out = BTreeMap::new();
    let mut total = 0usize;
    for (path, v) in files {
        if check_path(path).is_err() { return Err(no("pack.bad_file", &[("path", &shown_path(path))])); }
        let text = v.as_str().ok_or_else(|| no("pack.not_text", &[("path", &shown_path(path))]))?;
        if text.len() > MAX_FILE_BYTES { return Err(no("pack.file_too_big", &[("path", &shown_path(path))])); }
        total += text.len();
        if total > MAX_TOTAL_BYTES { return Err(no("pack.too_big", &[])); }
        out.insert(path.clone(), text.to_string());
    }
    Ok(out)
}

/// A pack's fingerprint: the full SHA-256 (64 hex digits) of exactly what it would bring (its kind
/// and every file with its content). Every decision compares this full value. What the pack calls
/// itself plays no part, and a fingerprint written in the file is only compared, never trusted.
pub fn fingerprint(kind: &str, files: &BTreeMap<String, String>) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"thoughts-pack\0");
    h.update(if kind == "feature" { b"feature\0".as_slice() } else { b"full\0".as_slice() });
    for (path, text) in files {
        h.update(path.as_bytes());
        h.update([0u8]);
        h.update((text.len() as u64).to_le_bytes());
        h.update(text.as_bytes());
    }
    format!("{:x}", h.finalize())
}

/// The fingerprint as people compare it by eye or read it out: its first 128 bits, as eight groups
/// of four (`3F9A-12C4-…`). Only for showing; decisions use the full fingerprint.
pub fn fingerprint_code(full: &str) -> String {
    let hex: Vec<char> = full.chars().filter(char::is_ascii_hexdigit).take(32).map(|c| c.to_ascii_uppercase()).collect();
    hex.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-")
}

/// Is `s` a full fingerprint (64 hex digits)?
pub fn full_fingerprint(s: &str) -> bool { s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) }

/// What a pack says it is, each claim one short clean line: `(name, author, version, description)`.
pub fn claims(pack: &Value) -> (String, String, String, String) {
    let s = |k: &str, max: usize| clean_line(pack[k].as_str().unwrap_or(""), max);
    (s("name", 80), s("author", 80), s("packVersion", 20), s("description", 400))
}

/// A pack checked before anything is staged, for the import dialog: what it says it is (its
/// maker's words), its fingerprint as calculated here (`fingerprint` in full, `code` to show),
/// whether the fingerprint written in it matches in full (`null` when it has none), how many of its
/// files differ from the layer at `layer`, and the newest earlier pack of the same name kept here
/// (`same`: exactly this pack, by full fingerprint), and `plugin`: its plugin lines compared with
/// the plugins here. Refusals are in the UI language.
pub fn inspect(pack: &Value, layer: &Path, kept: &[(String, crate::applayer::PackMark)]) -> anyhow::Result<Value> {
    let files = check_pack(pack)?;
    let kind = if pack["kind"] == "feature" { "feature" } else { "full" };
    let print = fingerprint(kind, &files);
    let claimed = pack["fingerprint"].as_str().map(|f| clean_line(f, 80));
    let (name, author, version, description) = claims(pack);
    let mine = collect(layer);
    let changes = files.iter().filter(|(p, t)| mine.get(p.as_str()) != Some(*t)).count();
    let earlier = kept.iter().rev().find(|(_, p)| !name.is_empty() && p.name == name).map(|(h, p)| json!({
        "entry": h, "version": p.version, "author": p.author, "code": fingerprint_code(&p.fingerprint),
        "same": full_fingerprint(&p.fingerprint) && p.fingerprint.eq_ignore_ascii_case(&print),
    }));
    Ok(json!({
        "name": name, "author": author, "version": version, "description": description, "kind": kind,
        "files": files.keys().collect::<Vec<_>>(), "fingerprint": print, "code": fingerprint_code(&print),
        "claimed": claimed.as_deref().map(fingerprint_code),
        "matches": claimed.as_ref().map(|c| full_fingerprint(c) && c.eq_ignore_ascii_case(&print)),
        "changes": changes, "earlier": earlier, "appVersion": clean_line(pack["appVersion"].as_str().unwrap_or(""), 30),
        // What it says of itself as a plugin, and how that compares with the plugins here (`bundles::inspect_plugin`).
        "plugin": crate::bundles::inspect_plugin(pack, layer)?,
    }))
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
pub(crate) fn clean_line(text: &str, max: usize) -> String {
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

/// What the person sharing a pack says it is.
#[derive(Default, Clone, Copy)]
pub struct Identity<'a> { pub name: &'a str, pub author: &'a str, pub version: &'a str, pub description: &'a str }

pub fn export_with(dir: &Path, name: &str, description: &str, paths: Option<&[String]>) -> anyhow::Result<Value> {
    export_as(dir, &Identity { name, description, ..Default::default() }, paths)
}

/// A pack of the layer. With `paths` (files, or folders ending in `/`) only those are included and
/// the pack is a `feature` pack that merges into the importer's layer instead of replacing files.
/// A selected plugin or view always travels with its declaration: its own `<name>.json`, or one
/// written from its `manifest.json` entry. The pack carries its maker's name, author and version,
/// and the fingerprint of what it holds (see `fingerprint`).
pub fn export_as(dir: &Path, who: &Identity, paths: Option<&[String]>) -> anyhow::Result<Value> {
    let (kind, files) = match paths {
        None => ("full", collect(dir)),
        Some(wanted) => {
            let out = feature_files(dir, wanted)?;
            if out.is_empty() { return Err(crate::tr::err("pack.nothing", &[])); }
            ("feature", out)
        }
    };
    let mut pack = pack_json(kind, files, who);
    // A pack of everything says which plugins it holds, so they are plugins again where it is imported
    // (their names and claims are checked there like everything else a pack says).
    let plugins = if kind == "full" { crate::bundles::plugins_list(dir) } else { vec![] };
    if !plugins.is_empty() { pack["plugins"] = json!(plugins); }
    Ok(pack)
}

/// The files of the layer at `dir` that `wanted` names (files, or folders ending in `/`), each
/// add-on and page with its declaration (see `export_as`). Empty when none of them is there.
pub(crate) fn feature_files(dir: &Path, wanted: &[String]) -> anyhow::Result<BTreeMap<String, String>> {
    let all = collect(dir);
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
    Ok(out)
}

/// A pack of `files`, with what its maker says it is and the fingerprint of what it holds.
pub(crate) fn pack_json(kind: &str, files: BTreeMap<String, String>, who: &Identity) -> Value {
    let name = clean_line(who.name, 80);
    let mut pack = json!({
        "format": "thoughts-pack", "version": 1, "apiVersion": crate::applayer::API_VERSION, "kind": kind,
        "name": if name.is_empty() { "My customisations" } else { name.as_str() },
        "description": clean_line(who.description, 400),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "fingerprint": fingerprint(kind, &files),
        "files": files,
    });
    for (key, value, max) in [("author", who.author, 80), ("packVersion", who.version, 20)] {
        let v = clean_line(value, max);
        if !v.is_empty() { pack[key] = json!(v); }
    }
    pack
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
pub(crate) fn marker_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if c.is_alphanumeric() || " ._-".contains(c) { c } else { '-' }).take(60).collect();
    if n.trim().is_empty() { "pack".into() } else { n.trim().to_string() }
}

/// What marks a pack's block in `custom.css`: its name and, when the pack has one, its `id` (a short
/// code written into the pack when it is exported). Two packs that merely share a name then keep
/// separate blocks, while a newer export of the same pack still replaces its own. A pack without an
/// id (made before 0.0.9) is keyed by its name alone, as before.
pub fn css_block_key(name: &str, id: Option<&str>) -> String {
    let id: String = id.unwrap_or("").chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect();
    if id.len() < 6 { name.to_string() } else { format!("{} {id}", name.chars().take(40).collect::<String>().trim()) }
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
    let shown = shown_path(path);
    let theirs_v: Value = serde_json::from_str(theirs).map_err(|_| crate::tr::err("pack.bad_json", &[("path", &shown)]))?;
    // A damaged file of the user's is never silently replaced by a merge.
    let mine_v: Value = serde_json::from_str(mine).map_err(|_| crate::tr::err("pack.yours_broken", &[("path", &shown)]))?;
    if std::mem::discriminant(&mine_v) != std::mem::discriminant(&theirs_v) { return Err(crate::tr::err("pack.shape", &[("path", &shown)])); }
    Ok(serde_json::to_string_pretty(&merge_json(&mine_v, &theirs_v))? + "\n")
}

/// Would importing `pack` into the layer at `layer` change nothing (it is already there, exactly)?
pub fn already_in(pack: &Value, layer: &Path) -> bool {
    let Ok(files) = check_pack(pack) else { return false };
    let feature = pack["kind"] == "feature";
    let name = clean_line(pack["name"].as_str().unwrap_or(""), 80);
    let name = if name.is_empty() { "pack".to_string() } else { name };
    let css_key = css_block_key(&name, pack["id"].as_str());
    files.iter().all(|(path, text)| {
        let mine = std::fs::read_to_string(layer.join(path)).ok();
        let out = if feature { merged_file(path, mine.as_deref(), text, &css_key).ok() } else { Some(text.clone()) };
        out.is_some() && out == mine
    })
}

/// Stage a pack as a pending change (origin `import`). Nothing is applied. A `full` pack replaces
/// the files it names; a `feature` pack merges into what the user already has.
pub async fn import(g: Git<'_>, d: &Dirs, pack: &Value, arabic: bool) -> anyhow::Result<()> { import_with(g, d, pack, arabic, None).await }

/// `import`, with the app's own description of the change in place of the usual one (for the
/// ready-made packs that come with the app, whose words are the app's).
pub async fn import_with(g: Git<'_>, d: &Dirs, pack: &Value, arabic: bool, own_summary: Option<String>) -> anyhow::Result<()> {
    import_as(g, d, pack, arabic, own_summary, false).await
}

/// `import_with`. A feature pack is staged as a plugin: one made for a newer Thoughts is refused,
/// one in an older layer format is brought up to date first, and when it is a newer version of a
/// plugin that is already here, that version's things are taken out of the staging copy before the
/// merge, so what the new version dropped goes away (`as_new`: stage it as another plugin instead).
pub async fn import_as(g: Git<'_>, d: &Dirs, pack: &Value, arabic: bool, own_summary: Option<String>, as_new: bool) -> anyhow::Result<()> {
    let received = check_pack(pack)?;
    let feature = pack["kind"] == "feature";
    // The fingerprint is of the pack as it came, whatever a format update then does to its files.
    let print = fingerprint(if feature { "feature" } else { "full" }, &received);
    let plugin = if feature { Some(crate::bundles::check_compatible(pack)?) } else { None };
    let mut files = received.clone();
    let mut updated = false;
    if let Some(p) = &plugin {
        updated = crate::bundles::migrate_files(&mut files, p.layer_format).map_err(|e| crate::tr::err_detail("plugin.migrate_failed", &[], e))?;
        // What a format update wrote is checked like the pack itself was.
        if updated { files = check_pack(&json!({"format": "thoughts-pack", "version": 1, "files": files}))?; }
    }
    let (_, author, version, _) = claims(pack);
    let name = clean_line(pack["name"].as_str().unwrap_or(""), 80);
    let name = if name.is_empty() { "pack".to_string() } else { name };
    let css_key = css_block_key(&name, pack["id"].as_str());
    // The change that was waiting for review is only gone once this pack has really been staged.
    let waiting = crate::staging::put_aside(g, d).await;
    if let Err(e) = crate::staging::recreate(g, d, "import", None).await {
        if let Some(w) = waiting { crate::staging::put_back(g, d, w).await; }
        return Err(e);
    }
    let mut replaced: Vec<&str> = vec![];
    let mut record: Option<Value> = if feature { None } else { crate::bundles::staged_all(pack) };
    let result: anyhow::Result<()> = (|| {
        if let Some(p) = &plugin {
            let origin = if own_summary.is_some() { "ready" } else { "pack" };
            record = Some(crate::bundles::before_merge(d, p, &files, &css_key, origin, &print, as_new, updated)?);
        }
        for (path, text) in &files {
            let target = d.staging.join(path);
            if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
            if std::fs::symlink_metadata(&target).map_or(false, |m| m.file_type().is_symlink()) { std::fs::remove_file(&target)?; }
            let mine = std::fs::read_to_string(&target).ok();
            let out = if feature { merged_file(path, mine.as_deref(), text, &css_key)? } else { text.clone() };
            if out.len() > MAX_FILE_BYTES { return Err(crate::tr::err("pack.grows_too_big", &[("path", &shown_path(path))])); }
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
    if let Some(w) = waiting { crate::staging::let_go(g, d, w).await; }
    // What the pack says about itself is kept apart and shown as the pack's words. It never becomes
    // part of the app's own description of the change, nor of the history entry.
    // (A ready-made pack's description is the app's own, part of `own_summary`: no quotation.)
    let pack_words = if own_summary.is_some() { String::new() } else { clean_line(pack["description"].as_str().unwrap_or(""), 400) };
    crate::staging::set_pack(d, &name, &pack_words);
    crate::staging::set_pack_identity(d, &author, &version, &print);
    crate::staging::set_bundle_pack(d, record);
    let name = quoted_name(&name);
    let mut summary = match (own_summary, arabic, feature) {
        (Some(s), _, _) => s,
        (None, true, true) => format!("إضافة حزمة «{name}» إلى تخصيصاتك\n\nتضيف الحزمة {} وتُدمج مع ما لديك دون أن تستبدل إعداداتك الأخرى. جرّبها أولاً، ثم اعتمدها إن أعجبتك؛ ويمكنك التراجع عنها لاحقاً من السجل.", files_ar(files.len(), false)),
        (None, true, false) => format!("استيراد حزمة التخصيص «{name}»\n\nتحتوي الحزمة على {} تحلّ محلّ الملفات المقابلة لديك. جرّبها أولاً، ثم اعتمدها إن أعجبتك؛ ويمكنك التراجع عنها لاحقاً من السجل.", files_ar(files.len(), true)),
        (None, false, true) => format!("Add the pack \"{name}\" to your customisations\n\nThe pack brings {} and is merged into what you already have; your other settings stay as they are. Try it first, then keep it if you like it — you can undo it later from History.", files_en(files.len())),
        (None, false, false) => format!("Import the customisation pack \"{name}\"\n\nThe pack contains {}, which replace the matching files you have now. Try it first, then keep it if you like it — you can undo it later from History.", files_en(files.len())),
    };
    if !replaced.is_empty() {
        let list = replaced.join(", ");
        summary.push_str(&if arabic { format!("\n\nتنبيه: لديك ملفات بالاسم نفسه ستُستبدل بنسخة الحزمة: {list}") } else { format!("\n\nNote: you already have files with the same names, and the pack's versions will replace them: {list}") });
    }
    std::fs::write(d.staging.join("SUMMARY.md"), summary)?;
    Ok(())
}

/// "1 file", "3 files".
fn files_en(n: usize) -> String { if n == 1 { "1 file".into() } else { format!("{n} files") } }

/// A number of files in Arabic, with the noun in the form the number takes (`after_preposition`:
/// after «على», where one file is «ملف واحد» rather than «ملفاً واحداً»).
fn files_ar(n: usize, after_preposition: bool) -> String {
    match (n, n % 100) {
        (1, _) => if after_preposition { "ملف واحد".into() } else { "ملفاً واحداً".into() },
        (2, _) => "ملفين".into(),
        (_, 3..=10) => format!("{n} ملفات"),
        (_, 11..=99) => format!("{n} ملفاً"),
        _ => format!("{n} ملف"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_counts_read_naturally_in_both_languages() {
        assert_eq!((files_en(1), files_en(3)), ("1 file".to_string(), "3 files".to_string()));
        let ar: Vec<String> = [1, 2, 3, 10, 11, 99, 100, 103, 111].iter().map(|n| files_ar(*n, false)).collect();
        assert_eq!(ar, ["ملفاً واحداً", "ملفين", "3 ملفات", "10 ملفات", "11 ملفاً", "99 ملفاً", "100 ملف", "103 ملفات", "111 ملفاً"]);
        assert_eq!(files_ar(1, true), "ملف واحد");
    }

    #[test]
    fn path_rules() {
        for ok in ["custom.css", "themes.json", "home.json", "manifest.json", "todos.json", "nav.json", "keys.json", "plugins/a.json", "views/board.json", "plugins/a.js", "views/board.html", "workflows/weekly.json", "plugins/lib/util.js"] {
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

    /// FEAT-24: a pack says who made it and which version it is, and carries a fingerprint of what
    /// it holds. The importer calculates the fingerprint itself, sees whether the file's matches,
    /// and the kept entry remembers all of it.
    #[tokio::test]
    async fn a_pack_has_an_identity_and_a_fingerprint_the_importer_calculates() {
        let (st, d) = crate::applayer::testutil::layer("identity").await;
        std::fs::write(d.layer.join("plugins/board.js"), "export default () => {}\n").unwrap();
        let who = Identity { name: "Kanban", author: "Sara\u{202e}", version: "1.2", description: "A board." };
        let pack = export_as(&d.layer, &who, Some(&["plugins/board.js".to_string()])).unwrap();
        assert_eq!((&pack["author"], &pack["packVersion"]), (&json!("Sara[U+202E]"), &json!("1.2")));
        let print = pack["fingerprint"].as_str().unwrap().to_string();
        assert!(full_fingerprint(&print), "the full SHA-256: {print}");
        let code = fingerprint_code(&print);
        assert!(regex::Regex::new(r"^([0-9A-F]{4}-){7}[0-9A-F]{4}$").unwrap().is_match(&code), "128 bits to compare by eye: {code}");
        // The same content under another name has the same fingerprint; other content another one.
        let renamed = export_as(&d.layer, &Identity { name: "Something else", ..who }, Some(&["plugins/board.js".to_string()])).unwrap();
        assert_eq!(renamed["fingerprint"], pack["fingerprint"]);
        assert_ne!(export_as(&d.layer, &who, None).unwrap()["fingerprint"], pack["fingerprint"], "the whole layer is another pack");

        // Someone else looks at it before importing.
        let (st2, d2) = crate::applayer::testutil::layer("identity-in").await;
        let i = inspect(&pack, &d2.layer, &[]).unwrap();
        assert_eq!((&i["name"], &i["author"], &i["version"], &i["fingerprint"], &i["matches"]), (&json!("Kanban"), &json!("Sara[U+202E]"), &json!("1.2"), &json!(print), &json!(true)));
        assert_eq!(i["code"], json!(code));
        assert_eq!(i["changes"], 1);
        // A file changed after it was made: the fingerprint written in it no longer matches.
        let mut tampered = pack.clone();
        tampered["files"]["plugins/board.js"] = json!("export default () => { steal() }\n");
        let t = inspect(&tampered, &d2.layer, &[]).unwrap();
        assert_eq!((t["matches"].clone(), t["claimed"].clone()), (json!(false), json!(code)));
        // Only the full fingerprint counts: a file claiming just the short code it shows does not match.
        let mut short = pack.clone();
        short["fingerprint"] = json!(code);
        assert_eq!(inspect(&short, &d2.layer, &[]).unwrap()["matches"], false);
        assert_ne!(t["fingerprint"], json!(print));
        let mut bare = pack.clone();
        bare.as_object_mut().unwrap().remove("fingerprint");
        assert!(inspect(&bare, &d2.layer, &[]).unwrap()["matches"].is_null(), "an older pack has none to compare");
        assert!(inspect(&json!({"format": "zip"}), &d2.layer, &[]).unwrap_err().to_string().contains("not a Thoughts customisation pack"));

        // Imported and kept: the entry remembers the identity, and the next import of the same name knows the earlier one.
        import(st2, &d2, &pack, false).await.unwrap();
        let p = crate::staging::pending(st2, &d2, false).await;
        assert_eq!((&p["pack"]["author"], &p["pack"]["version"], &p["pack"]["fingerprint"]), (&json!("Sara[U+202E]"), &json!("1.2"), &json!(print)));
        let kept = crate::staging::apply(st2, &d2, false, p["signature"].as_str()).await.unwrap();
        let hash = kept["hash"].as_str().unwrap();
        crate::applayer::mark_pack(st2, &d2.layer, hash, crate::applayer::PackMark { name: "Kanban".into(), author: "Sara".into(), version: "1.2".into(), fingerprint: print.clone() }).await;
        let h = crate::applayer::history_page_in(st2, &d2.layer, 0, 1, false).await;
        assert_eq!((h[0]["entry"].as_str(), h[0]["summary"].as_str(), h[0]["pack"]["fingerprint"].as_str()), (Some("pack"), Some("Added the pack \u{201c}Kanban\u{201d}"), Some(print.as_str())));
        assert_eq!(crate::applayer::history_page_in(st2, &d2.layer, 0, 1, true).await[0]["summary"], "إضافة الحزمة «Kanban»");
        let kept_packs = crate::applayer::kept_packs(st2, &d2.layer).await;
        let again = inspect(&pack, &d2.layer, &kept_packs).unwrap();
        assert_eq!((again["earlier"]["version"].as_str(), again["earlier"]["same"].as_bool(), again["changes"].as_u64()), (Some("1.2"), Some(true), Some(0)));
        // Another pack under the same name is not "the same", whatever its first digits.
        let other = inspect(&tampered, &d2.layer, &kept_packs).unwrap();
        assert_eq!(other["earlier"]["same"], false);
        assert!(already_in(&pack, &d2.layer));
        let _ = st;
        std::fs::remove_dir_all(&d.home).unwrap();
        std::fs::remove_dir_all(&d2.home).unwrap();
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
        // Two packs that share a name keep separate blocks when they carry ids; a newer export of the
        // same pack (same id) replaces its own block; a pack without an id is keyed by its name alone.
        let (board, theme) = (css_block_key("My pack", Some("k3j9x0ab12cd")), css_block_key("My pack", Some("zz81mmq04tty")));
        assert_ne!(board, theme);
        let css = merge_css(&merge_css("", ".board { gap: 8px }", &board), ".t { color: red }", &theme);
        assert!(css.contains(".board { gap: 8px }") && css.contains(".t { color: red }"), "{css}");
        let css = merge_css(&css, ".board { gap: 12px }", &board);
        assert!(css.contains("gap: 12px") && !css.contains("gap: 8px") && css.contains(".t { color: red }"), "{css}");
        assert_eq!(css.matches("/* pack:My pack ").count(), 2);
        assert_eq!(css_block_key("My pack", None), "My pack");
        assert_eq!(css_block_key("My pack", Some("x*/")), "My pack", "an id too short to tell packs apart is ignored");
        assert_eq!(css_block_key("My pack", Some("abc*/ def123")), "My pack abcdef123", "only letters and digits of an id are used");
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
        assert_eq!(p["pack"], json!({"name": "Kanban", "description": "A board of your items by status.", "author": "", "version": "",
            "fingerprint": fingerprint("feature", &check_pack(&pack).unwrap())}));
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
        assert!(import(st2, &d2, &pack, false).await.unwrap_err().to_string().contains("Your home.json is damaged"));
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
