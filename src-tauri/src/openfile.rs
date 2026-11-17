//! Opening a file in its own app, and showing a file in its folder (Finder, File Explorer, the
//! file manager), for the app window only.
//!
//! Library files go through the same checks as the daemon's file API and the note links
//! (`links::locate`): `thoughtsd::library::resolve` (the deny-list such as `.git` and `.claude`, no
//! `..`, no link leading out of the library), the real file still inside the library, and a native
//! question first when the file is in a folder the user marked private. Chat files must be that
//! chat's own. Only kinds in `SAFE_TO_OPEN` are opened; any file that passes the path checks may be
//! shown in its folder, where the user decides what to do with it.

use std::path::{Path, PathBuf};
use tauri_plugin_dialog::MessageDialogKind;
use tauri_plugin_opener::OpenerExt;

use crate::recovery::{arabic, choose, one_line};

/// Kinds the system may open in their own app with one click. Library files arrive from other
/// computers through sync, from imports and from the assistant, and none of them carries the
/// "downloaded from the internet" mark that makes Office start in Protected View, so only kinds
/// that hold no macros or embedded programs are here:
/// - no programs, scripts, shortcuts or web pages;
/// - no macro-enabled Office files (`docm`, `xlsm`…), and none of the older formats that can carry
///   macros or are known for exploits: Word/Excel/PowerPoint 97–2003 (`doc`, `xls`, `ppt`), Rich
///   Text (`rtf`) and the OpenDocument files (`odt`, `ods`, `odp`, which carry Basic macros).
///   Those are offered "Show in Finder" instead, and the user opens them there if they trust them.
///
/// `csv` and `tsv` stay: they are plain text and hold no macros. A spreadsheet app treats cells that
/// start with `=` as formulas, but the old DDE trick that could start a program from one is off by
/// default in current Excel and asks twice even where it is on; the file is also editable and shown
/// in Thoughts itself. Keep `ui/src/lib/openInApp.ts` in step (a test checks it).
pub(crate) const SAFE_TO_OPEN: &[&str] = &["pdf", "docx", "xlsx", "pptx", "csv", "tsv", "txt", "md", "png", "jpg", "jpeg", "gif",
    "webp", "heic", "tiff", "bmp", "mp3", "m4a", "wav", "mp4", "mov", "epub",
    // Every kind of sound a meeting recording may be (`thoughtsd::voice::AUDIO_EXT`): the Meetings tab plays them.
    "aac", "caf", "aiff", "aif"];

fn safe_kind(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    SAFE_TO_OPEN.contains(&ext.as_str())
}

/// Why a file is not opened or shown.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// Not a plain path to a file inside the library (or the chat's folder), or in a folder Thoughts never opens.
    Path,
    /// The file is not there (any more).
    Gone,
    /// A kind that is not opened directly (it may still be shown in its folder).
    Kind,
}

fn words(key: &str) -> (&'static str, &'static str) {
    match key {
        "path" => ("This file can't be opened from Thoughts.", "لا يمكن فتح هذا الملف من Thoughts."),
        "gone" => ("The file is no longer there.", "لم يعد الملف موجوداً."),
        "kind" => (
            "Thoughts doesn't open this kind of file directly. Show it in its folder and open it from there if you trust it.",
            "لا يفتح Thoughts هذا النوع من الملفات مباشرةً. أظهِره في مجلده وافتحه من هناك إن كنت تثق به.",
        ),
        "nolib" => ("No library is open.", "لا توجد مكتبة مفتوحة."),
        "private.open.title" => ("Open a file from a private folder?", "فتح ملف من مجلد خاص؟"),
        "private.open.body" => (
            "“{path}” is in a folder you marked private. It will open in another app, outside Thoughts.",
            "«{path}» في مجلد جعلته خاصاً، وسيُفتح في تطبيق آخر خارج Thoughts.",
        ),
        "private.show.title" => ("Show a file from a private folder?", "إظهار ملف من مجلد خاص؟"),
        "private.show.body" => (
            "“{path}” is in a folder you marked private. Its folder will open outside Thoughts.",
            "«{path}» في مجلد جعلته خاصاً، وسيُفتح مجلده خارج Thoughts.",
        ),
        "open" => ("Open", "فتح"),
        "show" => ("Show", "إظهار"),
        "cancel" => ("Cancel", "إلغاء"),
        _ => ("", ""),
    }
}

fn tx(key: &str) -> &'static str {
    let (en, ar) = words(key);
    if arabic() { ar } else { en }
}

fn say(r: Refused) -> String {
    tx(match r { Refused::Path => "path", Refused::Gone => "gone", Refused::Kind => "kind" }).to_string()
}

/// The real path of library file `rel` under `root`, and whether it is in a private folder: a plain
/// relative path, through `library::resolve`, a regular file whose real location is in the library.
pub(crate) fn library_file(root: &Path, rel: &str, private: &[String]) -> Result<(PathBuf, bool), Refused> {
    let p = Path::new(rel);
    if rel.is_empty() || p.is_absolute() || p.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err(Refused::Path);
    }
    let joined = thoughtsd::library::resolve(root, rel).map_err(|_| Refused::Path)?;
    if !joined.is_file() {
        return Err(Refused::Gone);
    }
    // dunce: plain `C:\…` paths on Windows — the shell cannot open `\\?\` verbatim paths.
    let (Ok(real_root), Ok(real)) = (dunce::canonicalize(root), dunce::canonicalize(&joined)) else { return Err(Refused::Gone) };
    if !real.starts_with(&real_root) {
        return Err(Refused::Path);
    }
    Ok((real, thoughtsd::paths::is_private(root, rel, private)))
}

/// The real path of file `name` sent with chat `chat_id`: a plain file name, a regular file (not a
/// link) directly in that chat's own folder under `attachments`, whose real location is inside it.
pub(crate) fn chat_file(attachments: &Path, chat_id: &str, name: &str) -> Result<PathBuf, Refused> {
    if !thoughtsd::chats::valid_id(chat_id) {
        return Err(Refused::Path);
    }
    let plain = !name.is_empty() && name.len() <= 400 && !name.starts_with('.') && name != ".."
        && !name.chars().any(|c| matches!(c, '/' | '\\' | ':') || c.is_control());
    if !plain {
        return Err(Refused::Path);
    }
    let base = dunce::canonicalize(attachments).map_err(|_| Refused::Gone)?;
    let dir = attachments.join(chat_id);
    if !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
        return Err(Refused::Gone);
    }
    let file = dir.join(name);
    if !std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_file()) {
        return Err(Refused::Gone);
    }
    let real = dunce::canonicalize(&file).map_err(|_| Refused::Gone)?;
    if real.parent() != Some(base.join(chat_id).as_path()) {
        return Err(Refused::Path);
    }
    Ok(real)
}

/// `chat_file`, when it may also be opened in its own app (`SAFE_TO_OPEN`).
pub(crate) fn chat_attachment(attachments: &Path, chat_id: &str, name: &str) -> Result<PathBuf, Refused> {
    if !safe_kind(Path::new(name)) {
        return Err(Refused::Kind);
    }
    chat_file(attachments, chat_id, name)
}

/// The library file `rel`, checked, after the user said yes when it is in a private folder.
/// `Ok(None)`: the user said no.
fn checked_library_file(app: &tauri::AppHandle, rel: &str, show: bool) -> Result<Option<PathBuf>, String> {
    let settings = thoughtsd::settings::Settings::load();
    let root = settings.library().ok_or_else(|| tx("nolib").to_string())?;
    let (file, private) = library_file(&root, rel, &settings.private_paths).map_err(say)?;
    if !show && !safe_kind(&file) {
        return Err(say(Refused::Kind));
    }
    if private {
        let (title, body, yes) = if show { ("private.show.title", "private.show.body", "show") } else { ("private.open.title", "private.open.body", "open") };
        let body = tx(body).replace("{path}", &one_line(rel, 120));
        if choose(app, MessageDialogKind::Warning, tx(title), &body, &[tx(yes), tx("cancel")]) != 0 {
            return Ok(None);
        }
    }
    Ok(Some(file))
}

/// Open a document from the library in the user's default app: only from the app window, only
/// regular files inside the library and outside its protected folders, only kinds in
/// `SAFE_TO_OPEN`, and after a native question for a file in a private folder.
/// `false`: the user chose not to.
#[tauri::command]
pub(crate) async fn open_library_file(app: tauri::AppHandle, window: tauri::Window, path: String) -> Result<bool, String> {
    crate::main_only(&window)?;
    let a = app.clone();
    let Some(file) = tauri::async_runtime::spawn_blocking(move || checked_library_file(&a, &path, false)).await.map_err(|e| e.to_string())?? else {
        return Ok(false);
    };
    app.opener().open_path(file.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Show a library file in its folder (Finder, File Explorer, the file manager), with the same
/// checks as opening it, whatever its kind. `false`: the user chose not to.
#[tauri::command]
pub(crate) async fn reveal_library_file(app: tauri::AppHandle, window: tauri::Window, path: String) -> Result<bool, String> {
    crate::main_only(&window)?;
    let a = app.clone();
    let Some(file) = tauri::async_runtime::spawn_blocking(move || checked_library_file(&a, &path, true)).await.map_err(|e| e.to_string())?? else {
        return Ok(false);
    };
    app.opener().reveal_item_in_dir(&file).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Open a file sent in a chat (a Word or Excel file, say) in the user's default app. Only the
/// chat's own files, only from the app window, and only kinds in `SAFE_TO_OPEN`. Chat files are
/// kept outside the library, so no private folder applies.
#[tauri::command]
pub(crate) fn open_chat_attachment(app: tauri::AppHandle, window: tauri::Window, chat_id: String, name: String) -> Result<(), String> {
    crate::main_only(&window)?;
    let file = chat_attachment(&thoughtsd::paths::attachments_dir(), &chat_id, &name).map_err(say)?;
    app.opener().open_path(file.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

/// Show a file sent in a chat in its folder, whatever its kind: only the chat's own files, only
/// from the app window.
#[tauri::command]
pub(crate) fn reveal_chat_attachment(app: tauri::AppHandle, window: tauri::Window, chat_id: String, name: String) -> Result<(), String> {
    crate::main_only(&window)?;
    let file = chat_file(&thoughtsd::paths::attachments_dir(), &chat_id, &name).map_err(say)?;
    app.opener().reveal_item_in_dir(&file).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-{tag}-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn kinds_that_can_carry_macros_or_programs_are_never_opened() {
        for bad in ["doc", "xls", "ppt", "rtf", "odt", "ods", "odp", "docm", "xlsm", "pptm", "dotm", "html", "htm", "svg", "command", "app",
            "exe", "bat", "lnk", "url", "webloc", "jar", "sh", "js", "py", "pkg", "dmg", "iso", "terminal", ""] {
            assert!(!SAFE_TO_OPEN.contains(&bad), "{bad}");
        }
        for good in ["pdf", "docx", "xlsx", "pptx", "csv", "md", "png", "mp3", "m4a"] {
            assert!(SAFE_TO_OPEN.contains(&good), "{good}");
            assert!(safe_kind(Path::new(&format!("a/B.{}", good.to_uppercase()))), "{good}");
        }
    }

    #[test]
    fn every_meeting_recording_can_be_played() {
        // The Meetings tab offers Play for each of these; opening one must not be refused.
        for ext in thoughtsd::voice::AUDIO_EXT {
            assert!(SAFE_TO_OPEN.contains(ext), "{ext}");
        }
    }

    #[test]
    fn the_window_offers_exactly_the_kinds_the_shell_opens() {
        // `ui/src/lib/openInApp.ts` decides which files get "Open in its app" and which get "Show in
        // Finder"; a kind on one list only is a dead button or a missing one.
        let ts = include_str!("../../ui/src/lib/openInApp.ts");
        let start = ts.find("OPEN_IN_APP = [").expect("OPEN_IN_APP list") + "OPEN_IN_APP = [".len();
        let end = start + ts[start..].find(']').unwrap();
        let mut ui: Vec<&str> = ts[start..end].split(',').map(|s| s.trim().trim_matches('\'')).filter(|s| !s.is_empty()).collect();
        let mut rust = SAFE_TO_OPEN.to_vec();
        ui.sort();
        rust.sort();
        assert_eq!(ui, rust);
    }

    #[test]
    fn library_files_are_opened_only_from_open_parts_of_the_library() {
        let root = temp("open-lib");
        for dir in ["ideas/shop/outputs", "me/finances", ".git", ".claude", "مجلد عربي"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for f in ["ideas/shop/outputs/deck.pptx", "me/finances/tax.pdf", ".git/brief.pdf", ".claude/notes.pdf", "مجلد عربي/عقد.docx", "old.doc"] {
            std::fs::write(root.join(f), "x").unwrap();
        }
        let private = vec!["me/finances".to_string()];
        let real = |p: &str| dunce::canonicalize(root.join(p)).unwrap();
        assert_eq!(library_file(&root, "ideas/shop/outputs/deck.pptx", &private), Ok((real("ideas/shop/outputs/deck.pptx"), false)));
        assert_eq!(library_file(&root, "مجلد عربي/عقد.docx", &private), Ok((real("مجلد عربي/عقد.docx"), false)));
        // A private folder is reported (the user is asked first), however it is spelled.
        assert_eq!(library_file(&root, "me/finances/tax.pdf", &private), Ok((real("me/finances/tax.pdf"), true)));
        assert_eq!(library_file(&root, "Me/Finances/tax.pdf", &private).map(|(_, p)| p).unwrap_or(true), true);
        // The folders Thoughts never opens, whatever the file is.
        for bad in [".git/brief.pdf", ".claude/notes.pdf", ".CLAUDE/notes.pdf", ".github/x.pdf"] {
            assert!(library_file(&root, bad, &private).is_err(), "{bad}");
        }
        // Not plain paths inside the library, and things that are not files.
        for bad in ["", "/etc/hosts", "../outside.pdf", "ideas/../me/finances/tax.pdf", "./old.doc", "ideas/shop"] {
            assert!(library_file(&root, bad, &private).is_err(), "{bad}");
        }
        assert_eq!(library_file(&root, "ideas/gone.pdf", &private), Err(Refused::Gone));
        // A legacy Word file passes the path checks (so it can be shown in its folder) but is not a kind to open.
        let (old, _) = library_file(&root, "old.doc", &private).unwrap();
        assert!(!safe_kind(&old));
        #[cfg(unix)]
        {
            // A link inside the library that leads into a protected folder or out of the library.
            let outside = temp("open-lib-outside");
            std::fs::write(outside.join("secret.pdf"), "x").unwrap();
            std::os::unix::fs::symlink(outside.join("secret.pdf"), root.join("ideas/away.pdf")).unwrap();
            std::os::unix::fs::symlink(root.join(".claude"), root.join("ideas/hidden")).unwrap();
            assert!(library_file(&root, "ideas/away.pdf", &private).is_err());
            assert!(library_file(&root, "ideas/hidden/notes.pdf", &private).is_err());
            // A link into a private folder counts as private.
            std::os::unix::fs::symlink(root.join("me/finances"), root.join("ideas/money")).unwrap();
            assert_eq!(library_file(&root, "ideas/money/tax.pdf", &private).map(|(_, p)| p), Ok(true));
            let _ = std::fs::remove_dir_all(&outside);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_a_chats_own_files_are_opened_or_shown() {
        let base = temp("att");
        let att = base.join("attachments");
        let chat = "0b0f6c2e-1d2a-4f8e-9c1e-5c9f2d7e8a10";
        std::fs::create_dir_all(att.join(chat)).unwrap();
        std::fs::create_dir_all(att.join("other-chat")).unwrap();
        for f in ["Budget 2026.xlsx", "عقد.docx", "run.command", "page.html", "macro.docm", "old.doc", "noext"] {
            std::fs::write(att.join(chat).join(f), "x").unwrap();
        }
        std::fs::write(att.join("other-chat").join("theirs.docx"), "x").unwrap();
        std::fs::write(base.join("outside.docx"), "x").unwrap();
        let real = |p: PathBuf| dunce::canonicalize(p).unwrap();
        assert_eq!(chat_attachment(&att, chat, "Budget 2026.xlsx"), Ok(real(att.join(chat).join("Budget 2026.xlsx"))));
        assert_eq!(chat_attachment(&att, chat, "عقد.docx"), Ok(real(att.join(chat).join("عقد.docx"))));
        // Kinds that can run something when opened, and files with no kind at all, are not opened,
        // but may be shown in their folder.
        for kind in ["run.command", "page.html", "macro.docm", "old.doc", "noext"] {
            assert_eq!(chat_attachment(&att, chat, kind), Err(Refused::Kind), "{kind}");
            assert_eq!(chat_file(&att, chat, kind), Ok(real(att.join(chat).join(kind))), "{kind}");
        }
        // Another chat's file, a way out of the folder, a made-up chat, a missing file: neither.
        assert!(chat_attachment(&att, "other-chat", "missing.docx").is_err());
        assert!(chat_file(&att, chat, "theirs.docx").is_err());
        for bad in ["../other-chat/theirs.docx", "..", "../../outside.docx", "a/b.docx", "a\\b.docx", "C:x.docx", ".hidden.docx", ""] {
            assert!(chat_attachment(&att, chat, bad).is_err(), "{bad}");
            assert!(chat_file(&att, chat, bad).is_err(), "{bad}");
        }
        for bad in ["..", "../attachments", "a/b", ""] {
            assert!(chat_file(&att, bad, "theirs.docx").is_err(), "{bad}");
        }
        // A link in the chat's folder that leads elsewhere is not opened or shown, whatever it is named.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("outside.docx"), att.join(chat).join("link.docx")).unwrap();
            assert!(chat_file(&att, chat, "link.docx").is_err());
            std::os::unix::fs::symlink(att.join("other-chat"), att.join("c0ffee")).unwrap();
            assert!(chat_file(&att, "c0ffee", "theirs.docx").is_err());
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn every_message_has_both_languages() {
        for k in ["path", "gone", "kind", "nolib", "private.open.title", "private.open.body", "private.show.title", "private.show.body", "open", "show", "cancel"] {
            let (en, ar) = words(k);
            assert!(!en.is_empty() && !ar.is_empty(), "{k}");
        }
    }
}
