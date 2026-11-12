//! The inbox: quick captures waiting to be filed. Each is one file directly inside the library's
//! `inbox/` folder — saved by Home's quick note, or put there from the phone (an iOS Shortcut that
//! writes a text file into a library that sits in a folder the phone can reach). The app runs no
//! server for the phone: a file that appears in the folder is a capture.
//!
//! Here: the listing (and count) behind Home's badge and the Inbox page, and making the
//! `process-inbox` routine available to a library that was set up before the kit had it. Making a
//! capture a todo, filing it under an item and deleting it use the ordinary capture, move and delete
//! endpoints, so history, links and the busy-library rules apply as everywhere else.

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
    if !std::fs::symlink_metadata(&dir).map_or(false, |m| m.is_dir()) { return vec![]; }
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
    out.sort_by(|a, b| b.0.cmp(&a.0));
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

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/inbox", get(api_list))
        .route("/api/inbox/prepare", post(api_prepare))
}

type R = Result<Json<Value>, crate::api::ApiErr>;

/// `GET /api/inbox[?count=1]` → `{count, items}` (`items` left out with `count=1`, for the badge).
async fn api_list(State(st): State<Arc<AppState>>, Query(q): Query<HashMap<String, String>>) -> R {
    let (lib, private) = { let s = st.settings.read().await; (s.library(), s.private_paths.clone()) };
    let Some(lib) = lib.filter(|l| l.is_dir()) else { return Ok(Json(json!({"count": 0, "items": []}))) };
    if is_private(&lib, &private) { return Ok(Json(json!({"count": 0, "items": []}))); }
    if q.get("count").is_some() {
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
    let held = st.library_lock.clone().try_lock_owned().map_err(|_| crate::tr::err("busy", &[]))?;
    let added = ensure_routine(&lib)?;
    if added {
        crate::claude::record_library_locked(&st, &lib, &crate::git::subject::edit("routine process-inbox")).await;
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

    #[tokio::test]
    async fn the_inbox_endpoints_follow_the_library_and_its_private_folders() {
        let (st, lib) = crate::wraps::testutil::library().await;
        crate::wraps::testutil::write(&lib, "inbox/a.md", "hello\n");
        // Waiting captures are not "notes" in the library count, but they are files: search and the tree see them.
        let stats = crate::library::stats(&lib);
        assert_eq!(stats["markdownFiles"].as_u64().unwrap(), stats["notes"].as_u64().unwrap() + 1 + 1);
        assert!(crate::library::search(&lib, "hello", 10, &[]).iter().any(|h| h["path"] == "inbox/a.md"));
        assert!(crate::library::tree(&lib, &[]).to_string().contains("inbox"));
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
