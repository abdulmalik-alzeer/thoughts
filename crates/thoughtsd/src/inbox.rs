//! The inbox: quick captures waiting to be filed. Each is one file directly inside the library's
//! `inbox/` folder — saved by Home's quick note, or put there from the phone (an iOS Shortcut that
//! writes a text file into a library that sits in a folder the phone can reach). The app runs no
//! server for the phone: a file that appears in the folder is a capture.
//!
//! Here: the listing (and count) behind Home's badge and the Inbox page, and making the
//! `process-inbox` routine available to a library that was set up before the kit had it, and filing
//! a capture under an item (into its notes, listed in its `## Files`). Making a capture a todo and
//! deleting it use the ordinary capture (with the capture's own date) and delete endpoints, so
//! history and the busy-library rules apply as everywhere else.

use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub const DIR: &str = "inbox";
/// The kit's routine that files captures.
pub const ROUTINE: &str = ".claude/skills/process-inbox/SKILL.md";
/// Longest capture shown whole; a bigger file is listed but only opened as a document.
const MAX_BYTES: u64 = 200 * 1024;
/// Most captures one page lists (the rest are counted).
const MAX_LISTED: usize = 200;

/// Does this file name hold a capture? Text files only; dotfiles are what a cloud folder shows
/// while a file has not finished arriving (`.name.icloud`) or what an editor leaves behind.
fn is_capture(name: &str) -> bool {
    if name.starts_with(['.', '~']) || name.ends_with('~') { return false; }
    let lower = name.to_lowercase();
    ["md", "markdown", "txt"].iter().any(|e| lower.ends_with(&format!(".{e}")))
}

/// The files waiting: `(library path, file)`, plain files directly inside `inbox/` only.
fn files(root: &Path) -> Vec<(String, std::fs::Metadata, std::path::PathBuf)> {
    let dir = root.join(DIR);
    // `inbox` must be a real folder inside the library, not a link leading elsewhere.
    if !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) { return vec![]; }
    let Ok(rd) = std::fs::read_dir(&dir) else { return vec![] };
    rd.flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        let meta = std::fs::symlink_metadata(e.path()).ok()?;
        (meta.is_file() && is_capture(&name)).then(|| (format!("{DIR}/{name}"), meta, e.path()))
    }).collect()
}

/// How many captures are waiting (reads no file).
pub fn count(root: &Path) -> usize { files(root).len() }

/// A capture's text split from its details block: `(title, captured, text)`.
fn split(raw: &str) -> (String, String, String) {
    let raw = raw.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let (mut title, mut captured, mut body) = (String::new(), String::new(), raw.as_str());
    if let Some(rest) = raw.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            if after.is_empty() || after.starts_with('\n') {
                for line in rest[..end].lines() {
                    let Some((k, v)) = line.split_once(':') else { continue };
                    let v = v.trim();
                    let v = match (v.strip_prefix('"').and_then(|x| x.strip_suffix('"')), v.strip_prefix('\'').and_then(|x| x.strip_suffix('\''))) {
                        (Some(q), _) => q.replace("\\\"", "\"").replace("\\\\", "\\"),
                        (None, Some(q)) => q.to_string(),
                        _ => v.to_string(),
                    };
                    match k.trim() { "title" => title = v, "captured" | "date" | "created" => captured = v, _ => {} }
                }
                body = after.trim_start_matches('\n');
            }
        }
    }
    let text = body.trim().to_string();
    if title.is_empty() {
        let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        title = first.trim_start_matches(['#', ' ']).chars().take(80).collect();
    }
    (title, captured, text)
}

/// When it was captured, best guess first: its own `captured:` line, the date in its name, then
/// when the file was last written (a phone's Shortcut often adds nothing else).
fn when(captured: &str, name: &str, meta: &std::fs::Metadata) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(captured) { return d.with_timezone(&chrono::Local); }
    if let Ok(d) = chrono::NaiveDateTime::parse_from_str(captured, "%Y-%m-%d %H:%M:%S").or_else(|_| chrono::NaiveDateTime::parse_from_str(captured, "%Y-%m-%d %H:%M")) {
        if let Some(d) = chrono::Local.from_local_datetime(&d).single() { return d; }
    }
    // A name that starts with a date, then perhaps a time: `2026-11-12-0930-…`, `2026-11-12 09.30.txt`.
    if let Some(m) = name.get(..10).and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) {
        let t: String = name[10..].chars().skip(1).take(5).filter(char::is_ascii_digit).collect();
        let time = chrono::NaiveTime::parse_from_str(&t, "%H%M").unwrap_or_default();
        if let Some(d) = chrono::Local.from_local_datetime(&m.and_time(time)).single() { return d; }
    }
    meta.modified().map(chrono::DateTime::<chrono::Local>::from).unwrap_or_else(|_| chrono::Local::now())
}

/// Every capture, newest first: `{path, name, title, text, raw, captured, big}`. `raw` is the file
/// as it is, so a delete can be undone exactly.
pub fn list(root: &Path) -> Vec<Value> {
    let mut out: Vec<(chrono::DateTime<chrono::Local>, Value)> = files(root).into_iter().map(|(path, meta, file)| {
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        let big = meta.len() > MAX_BYTES;
        let raw = if big { String::new() } else { std::fs::read(&file).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default() };
        let (title, captured, text) = split(&raw);
        let at = when(&captured, &name, &meta);
        let title = if title.is_empty() { name.trim_end_matches(|c| c != '.').trim_end_matches('.').to_string() } else { title };
        (at, json!({"path": path, "name": name, "title": title, "text": text, "raw": raw, "captured": at.to_rfc3339(), "big": big}))
    }).collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.0));
    out.into_iter().take(MAX_LISTED).map(|(_, v)| v).collect()
}

/// Is `inbox/` one of the folders the user keeps private? Then the page shows nothing of it.
fn is_private(root: &Path, private: &[String]) -> bool { crate::paths::is_private(root, DIR, private) }

/// Put the `process-inbox` routine into a library that does not have it (one made before the kit
/// shipped it). Never replaces a routine that is there, whoever wrote it, and never writes through a
/// link. The write goes through `trust::renew_after_app_write`, so a library that was trusted stays
/// trusted and one that was not stays asking. The caller holds the library's lock.
/// Returns whether the routine was added.
pub fn ensure_routine(root: &Path) -> anyhow::Result<bool> {
    let file = root.join(ROUTINE);
    if std::fs::symlink_metadata(&file).is_ok() { return Ok(false); }
    if crate::library::awaits_setup(root) { return Ok(false); }
    if crate::library::via_link(root, &file) { anyhow::bail!("the routines folder is a link"); }
    let Some(shipped) = crate::library::STARTER.get_file(ROUTINE) else { return Ok(false) };
    let bytes = shipped.contents();
    let renewed = crate::trust::renew_after_app_write(root, &file, Some(bytes), || {
        // Created only if still absent: never replaces anything that appeared meanwhile.
        if let Some(parent) = file.parent() { std::fs::create_dir_all(parent)?; }
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&file)?;
        f.write_all(bytes)?;
        Ok(())
    })?;
    if !renewed { crate::log::info("inbox: routine added; the library's trust was not renewed (it was not trusted, or something else changed)"); }
    Ok(true)
}

// ---------- filing a capture under an item ----------

/// Is this line the heading of a README's file list (`## Files`, «## الملفات»)? `Some(arabic)`.
fn files_heading(line: &str) -> Option<bool> {
    let h = line.trim_end().strip_prefix("## ")?.trim();
    if h.eq_ignore_ascii_case("files") { Some(false) } else if h == "الملفات" { Some(true) } else { None }
}

/// A link destination for a file name, still a destination however the name is spelled.
fn dest(name: &str) -> String {
    name.chars().map(|c| match c {
        ' ' | '%' | '#' | '?' | '<' | '>' | '(' | ')' | '[' | ']' => format!("%{:02X}", c as u32),
        c if c.is_control() => String::new(),
        c => c.to_string(),
    }).collect()
}

/// The line that lists a filed capture in its item's `## Files`: `- [title](notes/name) — from the inbox, date`.
fn files_line(title: &str, name: &str, date: &str, arabic: bool) -> String {
    let title: String = title.chars().filter(|c| !c.is_control()).take(80).collect();
    let title = title.trim().replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]");
    let title = if title.is_empty() { name.to_string() } else { title };
    if arabic { format!("- [{title}](notes/{}) — من صندوق الوارد، {date}", dest(name)) } else { format!("- [{title}](notes/{}) — from the inbox, {date}", dest(name)) }
}

/// `readme` with `line` added at the end of its `## Files` list. A README without one gets the list,
/// before `## Related` when it has that section, else at the end. The note details, every other line
/// and the file's line endings stay as they were. `arabic`: the words of a list made here.
fn add_to_files(readme: &str, line: &str, arabic: bool) -> String {
    // A byte-order mark (Notepad) stays in front, and the details behind it are still found.
    let (bom, readme) = match readme.strip_prefix('\u{feff}') { Some(rest) => ("\u{feff}", rest), None => ("", readme) };
    let mut lines: Vec<String> = readme.lines().map(str::to_string).collect();
    let mut fence = false;
    let body = crate::library::frontmatter(readme).1;
    let skip = readme[..readme.len() - body.len()].lines().count();
    let mut heading = None;
    let mut next = None;
    for (i, l) in lines.iter().enumerate().skip(skip) {
        let t = l.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") { fence = !fence; continue; }
        if fence { continue; }
        match heading {
            None if files_heading(l).is_some() => heading = Some(i),
            Some(_) if t.starts_with("# ") || t.starts_with("## ") => { next = Some(i); break; }
            _ => {}
        }
    }
    match heading {
        Some(h) => {
            let end = next.unwrap_or(lines.len());
            match (h + 1..end).rev().find(|&i| !lines[i].trim().is_empty()) {
                Some(last) => lines.insert(last + 1, line.to_string()),
                None => { lines.insert(h + 1, String::new()); lines.insert(h + 2, line.to_string()); }
            }
        }
        None => {
            let related = lines.iter().enumerate().skip(skip).position(|(_, l)| { let h = l.trim(); h.eq_ignore_ascii_case("## related") || h == "## ذات صلة" }).map(|p| p + skip);
            let section = [if arabic { "## الملفات".to_string() } else { "## Files".to_string() }, String::new(), line.to_string(), String::new()];
            match related {
                Some(r) => { for (k, s) in section.into_iter().enumerate() { lines.insert(r + k, s); } }
                None => {
                    while lines.last().is_some_and(|l| l.trim().is_empty()) { lines.pop(); }
                    if !lines.is_empty() { lines.push(String::new()); }
                    lines.extend(section.into_iter().take(3));
                }
            }
        }
    }
    // Every line keeps its own ending (a page partly edited on Windows stays as it was).
    format!("{bom}{}", crate::todos::with_endings(readme, &lines))
}

/// `readme` without the line `line` (as it was added), or `None` when it is not there any more.
fn remove_from_files(readme: &str, line: &str) -> Option<String> {
    let (bom, readme) = match readme.strip_prefix('\u{feff}') { Some(rest) => ("\u{feff}", rest), None => ("", readme) };
    let mut lines: Vec<String> = readme.lines().map(str::to_string).collect();
    let at = lines.iter().position(|l| l.trim_end() == line)?;
    lines.remove(at);
    Some(format!("{bom}{}", crate::todos::with_endings(readme, &lines)))
}

/// Move capture `capture` (`inbox/…`) into the notes of item `item` and list it in the item's
/// `## Files`. → `(where it is now, the line added)`.
fn file_under(root: &Path, capture: &str, item: &str, private: &[String]) -> anyhow::Result<(String, String)> {
    let capture = capture.trim().replace('\\', "/");
    let name = capture.strip_prefix(&format!("{DIR}/")).filter(|n| !n.contains('/') && is_capture(n)).ok_or_else(|| crate::tr::err("invalid.path", &[]))?.to_string();
    let dir = crate::library::item_dir(item)?;
    let readme_rel = format!("{dir}/README.md");
    for p in [DIR, dir.as_str(), readme_rel.as_str()] {
        if crate::paths::is_private(root, p, private) { return Err(crate::tr::err("private", &[("path", p)])); }
    }
    let readme_path = crate::library::resolve(root, &readme_rel)?;
    let readme = std::fs::read(&readme_path).map_err(|_| crate::tr::err("not_found.item", &[]))?;
    // Written back in full below: a page that is not plain text (an old Arabic encoding, a stray
    // byte) is refused rather than damaged.
    let readme = String::from_utf8(readme).map_err(|_| crate::tr::err("inbox.readme_not_text", &[]))?;
    let src = crate::library::resolve(root, &capture)?;
    let meta = std::fs::symlink_metadata(&src).map_err(|_| crate::tr::err("not_found.file", &[]))?;
    if !meta.is_file() { return Err(crate::tr::err("invalid.path", &[])); }
    let raw = if meta.len() > MAX_BYTES { String::new() } else { String::from_utf8_lossy(&std::fs::read(&src)?).into_owned() };
    let (title, captured, _) = split(&raw);
    let date = when(&captured, &name, &meta).format("%Y-%m-%d").to_string();
    // A plain text file becomes a note on the way; a note of that name already there is never replaced.
    let stem = name.strip_suffix(".txt").or_else(|| name.strip_suffix(".TXT")).map(|s| s.to_string());
    let (base, ext) = match &stem { Some(s) => (s.clone(), "md".to_string()), None => match name.rsplit_once('.') { Some((b, e)) => (b.to_string(), e.to_string()), None => (name.clone(), "md".into()) } };
    let mut to = String::new();
    for n in 1..1000 {
        let file = if n == 1 { format!("{base}.{ext}") } else { format!("{base}-{n}.{ext}") };
        let rel = format!("{dir}/notes/{file}");
        if std::fs::symlink_metadata(crate::library::resolve(root, &rel)?).is_err() { to = rel; break; }
    }
    if to.is_empty() { return Err(crate::tr::err("exists", &[("path", &format!("{dir}/notes/{name}"))])); }
    let filed = to.rsplit('/').next().unwrap_or_default().to_string();
    let arabic = readme.lines().find_map(files_heading).unwrap_or_else(crate::tr::arabic);
    let line = files_line(&title, &filed, &date, arabic);
    crate::library::rename(root, &capture, &to)?;
    if let Err(e) = crate::paths::write_atomic(&readme_path, add_to_files(&readme, &line, arabic).as_bytes(), false) {
        // The list could not be written: the capture goes back where it was, so nothing is half done.
        let _ = crate::library::rename(root, &to, &capture);
        return Err(e.into());
    }
    Ok((to, line))
}

/// Undo `file_under`: note `note` goes back to `capture` in the inbox, and `line` leaves its item's
/// `## Files` (only when it is still there exactly as it was added).
fn unfile(root: &Path, note: &str, capture: &str, line: &str) -> anyhow::Result<()> {
    let note = note.trim().replace('\\', "/");
    let capture = capture.trim().replace('\\', "/");
    capture.strip_prefix(&format!("{DIR}/")).filter(|n| !n.contains('/') && is_capture(n)).ok_or_else(|| crate::tr::err("invalid.path", &[]))?;
    let (dir, _) = note.split_once("/notes/").filter(|(_, f)| !f.is_empty() && !f.contains('/')).ok_or_else(|| crate::tr::err("invalid.path", &[]))?;
    let dir = crate::library::item_dir(dir)?;
    crate::library::rename(root, &note, &capture)?;
    let readme_path = crate::library::resolve(root, &format!("{dir}/README.md"))?;
    if let Ok(text) = std::fs::read_to_string(&readme_path) {
        if let Some(out) = remove_from_files(&text, line.trim_end()) { crate::paths::write_atomic(&readme_path, out.as_bytes(), false)?; }
    }
    Ok(())
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/inbox", get(api_list))
        .route("/api/inbox/prepare", post(api_prepare))
        .route("/api/inbox/file", post(api_file))
        .route("/api/inbox/unfile", post(api_unfile))
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct FileReq { path: String, item: String, to: String, line: String }

/// `POST /api/inbox/file {path, item}` → `{ok, path, line}`: file capture `path` under item `item`
/// (`<area>/<item>`, or its README). It moves into the item's `notes/` (a `.txt` becomes a `.md`;
/// never over a note that is there) and a line linking to it goes into the item's `## Files`, so the
/// assistant finds it there like any other note. With history on it is one entry. `line` is what
/// was added, for Undo.
async fn api_file(State(st): State<Arc<AppState>>, Json(r): Json<FileReq>) -> R {
    let root = crate::api::lib(&st).await?;
    let _edit = crate::api::library_edit(&st).await?;
    let private = st.settings.read().await.private_paths.clone();
    let dir = crate::library::item_dir(&r.item)?;
    let touched = |to: &str| vec![r.path.clone(), to.to_string(), format!("{dir}/README.md")];
    crate::claude::record_app_write(&st, &root, &crate::git::subject::edit(&format!("changes before filing {}", r.path)), &touched(&format!("{dir}/notes")), false).await;
    let (root2, path, item) = (root.clone(), r.path.clone(), r.item.clone());
    let (to, line) = tokio::task::spawn_blocking(move || file_under(&root2, &path, &item, &private)).await.map_err(|e| anyhow::anyhow!("{e}"))??;
    crate::claude::record_app_write(&st, &root, &crate::git::subject::moved(&r.path, &to), &touched(&to), false).await;
    st.emit(json!({"type": "library.changed"}));
    Ok(Json(json!({"ok": true, "path": to, "line": line})))
}

/// `POST /api/inbox/unfile {path, to, line}` → `{ok}`: Undo of `/api/inbox/file`: the note at `path`
/// goes back to `to` in the inbox, and `line` leaves the item's `## Files` (when it is still there).
async fn api_unfile(State(st): State<Arc<AppState>>, Json(r): Json<FileReq>) -> R {
    let root = crate::api::lib(&st).await?;
    let _edit = crate::api::library_edit(&st).await?;
    let private = st.settings.read().await.private_paths.clone();
    if [DIR, r.path.as_str()].iter().any(|p| crate::paths::is_private(&root, p, &private)) { return Err(crate::tr::err("private", &[("path", &r.path)]).into()); }
    let readme = r.path.split_once("/notes/").map(|(d, _)| format!("{d}/README.md")).unwrap_or_default();
    let touched = vec![r.path.clone(), r.to.clone(), readme];
    let (root2, path, to, line) = (root.clone(), r.path.clone(), r.to.clone(), r.line.clone());
    tokio::task::spawn_blocking(move || unfile(&root2, &path, &to, &line)).await.map_err(|e| anyhow::anyhow!("{e}"))??;
    crate::claude::record_app_write(&st, &root, &crate::git::subject::moved(&r.path, &r.to), &touched, false).await;
    st.emit(json!({"type": "library.changed"}));
    Ok(Json(json!({"ok": true})))
}

type R = Result<Json<Value>, crate::api::ApiErr>;

/// `GET /api/inbox[?count=1]` → `{count, items}` (`items` left out with `count=1`, for the badge).
async fn api_list(State(st): State<Arc<AppState>>, Query(q): Query<HashMap<String, String>>) -> R {
    let (lib, private) = { let s = st.settings.read().await; (s.library(), s.private_paths.clone()) };
    let Some(lib) = lib.filter(|l| l.is_dir()) else { return Ok(Json(json!({"count": 0, "items": []}))) };
    if is_private(&lib, &private) { return Ok(Json(json!({"count": 0, "items": []}))); }
    if q.contains_key("count") {
        return Ok(Json(json!({"count": tokio::task::spawn_blocking(move || count(&lib)).await.unwrap_or(0)})));
    }
    let items = tokio::task::spawn_blocking(move || list(&lib)).await.unwrap_or_default();
    Ok(Json(json!({"count": items.len(), "items": items})))
}

/// `POST /api/inbox/prepare` → `{ok, added}`: before "Process with the assistant", make sure the
/// library has the routine that does it.
async fn api_prepare(State(st): State<Arc<AppState>>) -> R {
    let lib = st.settings.read().await.library().ok_or_else(|| crate::tr::err("library.none", &[]))?;
    if !lib.is_dir() { return Err(crate::tr::err("library.missing", &[("path", &lib.display().to_string())]).into()); }
    crate::wraps::require_no_interrupted(&st).await?;
    let held = st.library_lock.of(&lib).try_lock_owned().map_err(|_| crate::tr::err("busy", &[]))?;
    // With the assistant at work in the library, this write is the app's, never the run's (FEAT-06).
    let fenced = crate::changesets::fence(&st).await;
    let added = ensure_routine(&lib)?;
    drop(fenced);
    if added {
        let touched = [ROUTINE.to_string()];
        crate::claude::record_app_write(&st, &lib, &crate::git::subject::edit("routine process-inbox"), &touched, true).await;
        st.emit(json!({"type": "library.changed"}));
    }
    drop(held);
    Ok(Json(json!({"ok": true, "added": added})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-inbox-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("inbox")).unwrap();
        d
    }
    fn put(d: &Path, rel: &str, text: &str) {
        let p = d.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn what_counts_as_a_capture() {
        for ok in ["a.md", "2026-11-12 09.30.TXT", "ملاحظة.md", "x.markdown"] { assert!(is_capture(ok), "{ok}"); }
        for no in [".a.md.icloud", ".DS_Store", "~$a.txt", "a.md~", "photo.jpg", "a", "notes.docx"] { assert!(!is_capture(no), "{no}"); }
    }

    #[test]
    fn captures_from_the_app_and_from_a_phone_are_read_alike() {
        let d = lib();
        // From the app: details block, title and date.
        put(&d, "inbox/2026-11-12-0930-call-sara.md", "---\ntitle: \"Call \\\"Sara\\\"\"\ncaptured: 2026-11-12T09:30:00+03:00\nsource: quick capture\n---\n\nCall Sara about the lease\nbefore Friday\n");
        // From a phone Shortcut: bare text, Windows line endings, no details.
        put(&d, "inbox/2026-11-13 08.15.txt", "\u{feff}# Idea: a shop for dates\r\nsell by weight\r\n");
        // Arabic, with a date only.
        put(&d, "inbox/فكرة.md", "---\ndate: 2026-11-14 10:00\n---\nفكرة عن تطبيق\n");
        // Not captures.
        put(&d, "inbox/.2026-11-15.txt.icloud", "x");
        put(&d, "inbox/photo.jpg", "x");
        put(&d, "inbox/sub/deep.md", "x");
        put(&d, "elsewhere/a.md", "x");
        assert_eq!(count(&d), 3);
        let l = list(&d);
        assert_eq!(l.len(), 3);
        // Newest first: the Arabic one (14th), the phone one (13th), the app one (12th).
        assert_eq!(l[0]["path"], "inbox/فكرة.md");
        assert_eq!((l[0]["title"].as_str(), l[0]["text"].as_str()), (Some("فكرة عن تطبيق"), Some("فكرة عن تطبيق")));
        assert!(l[0]["captured"].as_str().unwrap().starts_with("2026-11-14T10:00"));
        assert_eq!(l[1]["title"], "Idea: a shop for dates");
        assert_eq!(l[1]["text"], "# Idea: a shop for dates\nsell by weight");
        assert!(l[1]["captured"].as_str().unwrap().starts_with("2026-11-13T08:15"));
        assert_eq!(l[2]["title"], "Call \"Sara\"");
        assert_eq!(l[2]["text"], "Call Sara about the lease\nbefore Friday");
        // The whole file is handed back so a delete can be undone exactly.
        assert!(l[2]["raw"].as_str().unwrap().starts_with("---\ntitle:"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_missing_empty_or_linked_inbox_is_simply_empty() {
        let d = lib();
        assert_eq!((count(&d), list(&d).len()), (0, 0));
        std::fs::remove_dir_all(d.join("inbox")).unwrap();
        assert_eq!(count(&d), 0);
        #[cfg(unix)]
        {
            let elsewhere = std::env::temp_dir().join(format!("thoughts-inbox-else-{}", uuid::Uuid::new_v4()));
            put(&elsewhere, "a.md", "secret");
            std::os::unix::fs::symlink(&elsewhere, d.join("inbox")).unwrap();
            assert_eq!(count(&d), 0, "a link is not the library's inbox");
            std::fs::remove_dir_all(elsewhere).unwrap();
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn filing_refuses_an_item_page_that_is_not_plain_text_and_leaves_both_as_they_were() {
        let d = lib();
        put(&d, "inbox/idea.md", "# Idea\nA thought.\n");
        // Saved in an old Arabic encoding (Windows-1256): not valid UTF-8.
        let legacy: &[u8] = b"---\ntitle: \xc7\xe1\xd3\xe6\xe2\n---\n# Shop\n";
        std::fs::create_dir_all(d.join("apps/shop")).unwrap();
        std::fs::write(d.join("apps/shop/README.md"), legacy).unwrap();
        let e = file_under(&d, "inbox/idea.md", "apps/shop", &[]).unwrap_err();
        assert!(e.to_string().contains("Nothing was moved"), "{e}");
        assert_eq!(std::fs::read(d.join("apps/shop/README.md")).unwrap(), legacy);
        assert!(d.join("inbox/idea.md").exists() && !d.join("apps/shop/notes/idea.md").exists());
        // A plain-text page is filed under as usual.
        put(&d, "apps/shop/README.md", "---\ntitle: Shop\n---\n# Shop\n");
        let (to, line) = file_under(&d, "inbox/idea.md", "apps/shop", &[]).unwrap();
        assert_eq!(to, "apps/shop/notes/idea.md");
        assert!(std::fs::read_to_string(d.join("apps/shop/README.md")).unwrap().contains(&line));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_big_file_is_listed_but_not_read_and_the_page_is_capped() {
        let d = lib();
        put(&d, "inbox/big.txt", &"x".repeat(MAX_BYTES as usize + 1));
        let l = list(&d);
        assert_eq!((l[0]["big"].clone(), l[0]["raw"].clone(), l[0]["title"].clone()), (json!(true), json!(""), json!("big")));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_kit_ships_a_routine_that_runs_no_commands() {
        let r = crate::library::STARTER.get_file(ROUTINE).expect("the kit has the process-inbox routine");
        let text = r.contents_utf8().unwrap();
        assert!(text.starts_with("---\nname: process-inbox\ndescription: "));
        assert!(!crate::routines::runs_commands(text));
        let rules = crate::library::STARTER.get_file("CLAUDE.md").unwrap().contents_utf8().unwrap();
        assert!(rules.contains(".claude/skills/process-inbox/SKILL.md"));
    }

    #[test]
    fn an_older_library_gets_the_routine_once_and_it_is_never_replaced() {
        let d = lib();
        put(&d, "CLAUDE.md", "rules\n");
        assert!(ensure_routine(&d).unwrap());
        let shipped = crate::library::STARTER.get_file(ROUTINE).unwrap().contents();
        assert_eq!(std::fs::read(d.join(ROUTINE)).unwrap(), shipped);
        // The user's changed copy stays theirs.
        put(&d, ROUTINE, "---\nname: process-inbox\n---\nMy own way.\n");
        assert!(!ensure_routine(&d).unwrap());
        assert_eq!(std::fs::read_to_string(d.join(ROUTINE)).unwrap(), "---\nname: process-inbox\n---\nMy own way.\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_library_not_yet_set_up_or_with_a_linked_folder_is_left_alone() {
        let d = lib();
        put(&d, ".claude/skills/init-library/SKILL.md", "setup\n");
        assert!(!ensure_routine(&d).unwrap());
        assert!(!d.join(ROUTINE).exists());
        #[cfg(unix)]
        {
            std::fs::remove_dir_all(d.join(".claude/skills/init-library")).unwrap();
            let elsewhere = std::env::temp_dir().join(format!("thoughts-inbox-else-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&elsewhere).unwrap();
            std::os::unix::fs::symlink(&elsewhere, d.join(".claude/skills/process-inbox")).unwrap();
            assert!(ensure_routine(&d).is_err(), "never write through a link");
            std::fs::remove_file(d.join(".claude/skills/process-inbox")).unwrap();
            std::os::unix::fs::symlink(&elsewhere, d.join(".claude/skills/linked")).unwrap();
            std::fs::remove_dir_all(d.join(".claude/skills")).unwrap();
            std::os::unix::fs::symlink(&elsewhere, d.join(".claude/skills")).unwrap();
            assert!(ensure_routine(&d).is_err());
            assert!(!elsewhere.join("process-inbox").exists());
            std::fs::remove_dir_all(elsewhere).unwrap();
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_filed_capture_is_listed_at_the_end_of_its_items_files() {
        let line = files_line("Call [Sara]", "2026-11-13 08.15.md", "2026-11-13", false);
        assert_eq!(line, "- [Call \\[Sara\\]](notes/2026-11-13%2008.15.md) — from the inbox, 2026-11-13");
        // The link is found (and would be kept right by a later move) like any other.
        assert_eq!(crate::links::find_links(&line).len(), 1);
        let readme = "---\ntitle: Acme\n---\n# Acme\n\n## Current state\nOpen.\n\n## Files\n- [fees](notes/fees.md) — the fee table\n\n## Related\n- none\n";
        let out = add_to_files(readme, &line, false);
        assert_eq!(out, readme.replace("the fee table\n", &format!("the fee table\n{line}\n")));
        assert_eq!(remove_from_files(&out, &line).as_deref(), Some(readme));
        assert_eq!(remove_from_files(readme, &line), None, "a line the user already took out is not looked for again");
        // Arabic heading, Windows line endings, an empty list and a code block mentioning ## Files.
        let ar = "\u{feff}# متجر\r\n\r\n```\r\n## Files\r\n```\r\n\r\n## الملفات\r\n\r\n## ذات صلة\r\n";
        let line_ar = files_line("فكرة", "فكرة.md", "2026-11-14", true);
        assert_eq!(line_ar, "- [فكرة](notes/فكرة.md) — من صندوق الوارد، 2026-11-14");
        assert_eq!(add_to_files(ar, &line_ar, true), ar.replace("## الملفات\r\n", &format!("## الملفات\r\n\r\n{line_ar}\r\n")));
        // No list yet: one is made before Related, or at the end.
        let bare = "---\ntitle: X\n---\n# X\n\n## Related\n- y\n";
        assert_eq!(add_to_files(bare, "- [a](notes/a.md)", false), "---\ntitle: X\n---\n# X\n\n## Files\n\n- [a](notes/a.md)\n\n## Related\n- y\n");
        assert_eq!(add_to_files("# X\nText.", "- [a](notes/a.md)", true), "# X\nText.\n\n## الملفات\n\n- [a](notes/a.md)");

    }

    #[test]
    fn each_line_of_the_page_keeps_its_own_ending_and_a_byte_order_mark_stays() {
        // Partly edited on Windows: only some lines end in CR LF. Nothing but the new line changes.
        let mixed = "\u{feff}---\r\ntitle: A\n---\n# A\r\n\n## Files\n- [x](notes/x.md)\r\n\n## Related\n- y\r\n";
        let line = "- [b](notes/b.md)";
        let out = add_to_files(mixed, line, false);
        assert_eq!(out, mixed.replace("- [x](notes/x.md)\r\n", "- [x](notes/x.md)\r\n- [b](notes/b.md)\n"));
        assert_eq!(remove_from_files(&out, line).as_deref(), Some(mixed), "taking it out again gives the page back byte for byte");
        // The mark does not hide the details: a list in front of them is never made.
        let bom = "\u{feff}---\ntitle: A\n---\n# A\n";
        assert_eq!(add_to_files(bom, line, false), "\u{feff}---\ntitle: A\n---\n# A\n\n## Files\n\n- [b](notes/b.md)\n");
    }

    #[tokio::test]
    async fn filing_a_capture_moves_it_into_the_items_notes_and_lists_it_and_undo_puts_both_back() {
        let (st, lib) = crate::wraps::testutil::library().await;
        let t = crate::wraps::testutil::write;
        t(&lib, "clients/acme/README.md", "---\ntitle: Acme\n---\n# Acme\n\n## Files\n- [fees](notes/fees.md)\n");
        t(&lib, "clients/acme/notes/2026-11-13 08.15.md", "a note already there\n");
        t(&lib, "inbox/2026-11-13 08.15.txt", "Ask about the lease\n");
        let req = |path: &str, item: &str| Json(FileReq { path: path.into(), item: item.into(), ..Default::default() });
        let r = api_file(State(st.clone()), req("inbox/2026-11-13 08.15.txt", "clients/acme/README.md")).await.unwrap().0;
        // Never over the note that was there; the text file became a note.
        assert_eq!(r["path"], "clients/acme/notes/2026-11-13 08.15-2.md");
        assert_eq!(std::fs::read_to_string(lib.join("clients/acme/notes/2026-11-13 08.15-2.md")).unwrap(), "Ask about the lease\n");
        assert_eq!(std::fs::read_to_string(lib.join("clients/acme/notes/2026-11-13 08.15.md")).unwrap(), "a note already there\n");
        assert!(!lib.join("inbox/2026-11-13 08.15.txt").exists());
        let readme = std::fs::read_to_string(lib.join("clients/acme/README.md")).unwrap();
        assert!(readme.ends_with("- [fees](notes/fees.md)\n- [Ask about the lease](notes/2026-11-13%2008.15-2.md) — from the inbox, 2026-11-13\n"), "{readme}");
        // The item's page lists it among its notes, and the move is one history entry.
        let item = crate::library::item(&lib, "clients/acme", &[]).unwrap();
        assert!(item["notes"].as_array().unwrap().iter().any(|n| n["path"] == "clients/acme/notes/2026-11-13 08.15-2.md"));
        let last = crate::wraps::testutil::sh(&lib, &["log", "-1", "--name-status", "--format=%s"]);
        assert!(last.contains("clients/acme/README.md") && last.contains("2026-11-13 08.15-2.md"), "{last}");
        // Undo: back in the inbox as it was, and the line is gone.
        let undo = Json(FileReq { path: r["path"].as_str().unwrap().into(), to: "inbox/2026-11-13 08.15.txt".into(), line: r["line"].as_str().unwrap().into(), ..Default::default() });
        let _ = api_unfile(State(st.clone()), undo).await.unwrap();
        assert_eq!(std::fs::read_to_string(lib.join("inbox/2026-11-13 08.15.txt")).unwrap(), "Ask about the lease\n");
        assert_eq!(std::fs::read_to_string(lib.join("clients/acme/README.md")).unwrap(), "---\ntitle: Acme\n---\n# Acme\n\n## Files\n- [fees](notes/fees.md)\n");
        // Only a capture, only into an item, never into or out of a private folder.
        assert!(api_file(State(st.clone()), req("clients/acme/README.md", "clients/acme")).await.is_err());
        assert!(api_file(State(st.clone()), req("inbox/2026-11-13 08.15.txt", "inbox")).await.is_err());
        st.settings.write().await.private_paths = vec!["Clients".into()];
        assert!(api_file(State(st.clone()), req("inbox/2026-11-13 08.15.txt", "clients/acme")).await.is_err());
        #[cfg(unix)]
        {
            // A link to a private item is as closed as the item.
            std::os::unix::fs::symlink(lib.join("clients"), lib.join("work")).unwrap();
            assert!(api_file(State(st.clone()), req("inbox/2026-11-13 08.15.txt", "work/acme")).await.is_err());
        }
        assert!(lib.join("inbox/2026-11-13 08.15.txt").exists());
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn the_inbox_endpoints_follow_the_library_and_its_private_folders() {
        let (st, lib) = crate::wraps::testutil::library().await;
        crate::wraps::testutil::write(&lib, "inbox/a.md", "hello\n");
        // Waiting captures are not "notes" in the library count, but they are files: search and the tree see them.
        let stats = crate::library::stats(&lib);
        assert_eq!(stats["markdownFiles"].as_u64().unwrap(), stats["notes"].as_u64().unwrap() + 1);
        assert!(crate::library::search(&lib, "hello", 10, &[]).iter().any(|h| h["path"] == "inbox/a.md"));
        assert!(serde_json::json!(crate::library::tree(&lib, &[])).to_string().contains("inbox"));
        let q = |c: bool| Query(if c { HashMap::from([("count".to_string(), "1".to_string())]) } else { HashMap::new() });
        assert_eq!(api_list(State(st.clone()), q(true)).await.unwrap().0, json!({"count": 1}));
        let v = api_list(State(st.clone()), q(false)).await.unwrap().0;
        assert_eq!((v["count"].clone(), v["items"][0]["text"].clone()), (json!(1), json!("hello")));
        st.settings.write().await.private_paths = vec!["inbox".into()];
        assert_eq!(api_list(State(st.clone()), q(false)).await.unwrap().0, json!({"count": 0, "items": []}));
        // Preparing adds the routine once; the library had it only through the kit.
        assert_eq!(api_prepare(State(st.clone())).await.unwrap().0["added"], true);
        assert!(lib.join(ROUTINE).is_file());
        assert_eq!(api_prepare(State(st.clone())).await.unwrap().0["added"], false);
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }
}
