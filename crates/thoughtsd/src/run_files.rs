//! Copies of a chat's attachments for its assistant run.
//!
//! Every run is denied the whole attachments folder (`claude::protective_deny`), so that no rule
//! depends on which chats happen to have files when the run starts. A run reads the files of its
//! own chat from copies made for it when the message is sent, with the Read tool only (the shell is
//! closed to all copies), in a folder of its own under `<home>/chat-files/`. That folder's place is
//! worked out from the chat id (`folder`): one single-digit folder per level, `DEPTH` levels down;
//! the copies are kept there from one run to the next (`copy` refreshes them). Every place a copy of *another* chat could ever be is then one
//! of the 9 other digits at some level along this chat's path — a fixed list that can be closed
//! before any of those folders exist (`closed_for`). Digits have no letter case, so a rule can not
//! be dodged by spelling a name in capitals on a file system that ignores case.
//!
//! The Read tool opens text, pictures and PDFs, not spreadsheets, Word documents or slide decks. For
//! each copy of such a file a text version is written (`as_text`) in `text/` beside the copies'
//! folder, inside the chat's own folder, so the same rules open it to this chat's run and close it
//! to every other; the prompt names it (`listed`). Library files of those kinds that a message
//! refers to get theirs in `library/` there (`library_texts`).

use std::path::{Path, PathBuf};

/// The folder in the app's home holding the copies of every run.
pub const DIR: &str = "chat-files";
/// Levels below `DIR`: the 20 decimal digits of 64 bits of the chat id's fingerprint (two chats
/// share a folder with a chance of one in 2^64).
const DEPTH: usize = 20;
const DIGITS: &str = "0123456789";

pub fn root(home: &Path) -> PathBuf { home.join(DIR) }

fn digits(chat_id: &str) -> String {
    let h = crate::updates::sha(chat_id.as_bytes());
    format!("{:020}", u64::from_str_radix(&h[..16], 16).unwrap_or(0))
}

/// The folder (below `root`) of chat `chat_id`'s copies; `None` for an id that is not a chat id.
/// The copies themselves are in a folder inside it (`copy`).
pub fn folder(home: &Path, chat_id: &str) -> Option<PathBuf> {
    if !crate::chats::valid_id(chat_id) { return None; }
    let mut p = root(home);
    for c in digits(chat_id).chars() { p.push(c.to_string()); }
    Some(p)
}

/// Every folder under `root` that chat `chat_id`'s run must not read, relative to `root` (`3`,
/// `1/7`, …): beside each level of its own path, the other 9. Together they hold every place any
/// other chat's copies can be. `None` for an id that is not a chat id: then all of it is closed.
pub fn closed_for(chat_id: &str) -> Option<Vec<String>> {
    if !crate::chats::valid_id(chat_id) { return None; }
    let mut out = Vec::with_capacity(DEPTH * 9);
    let mut at = String::new();
    for own in digits(chat_id).chars() {
        for c in DIGITS.chars().filter(|c| *c != own) { out.push(format!("{at}{c}")); }
        at.push(own);
        at.push('/');
    }
    Some(out)
}

/// Is every folder from `root` down to `dir` a plain folder (not a link), creating the missing ones?
fn plain_path(home: &Path, dir: &Path, create: bool) -> std::io::Result<bool> {
    let root = root(home);
    let Ok(rest) = dir.strip_prefix(&root) else { return Ok(false) };
    let mut at = root.clone();
    for step in std::iter::once(None).chain(rest.components().map(Some)) {
        if let Some(c) = step { at.push(c); }
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_dir() => {}
            Ok(_) => return Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                #[cfg(unix)]
                { use std::os::unix::fs::DirBuilderExt; std::fs::DirBuilder::new().mode(0o700).create(&at)?; }
                #[cfg(not(unix))]
                std::fs::create_dir(&at)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// The folder (inside the chat's `folder`) holding the copies of its attached files. It stays from
/// one run to the next: a send copies only what is new or changed, and removes the copies of files
/// that are no longer attached. Nothing at the end of a run removes it (a run the next message
/// starts at once reads the very same files); it goes with the chat (`clear`, from `chats::remove`)
/// and when the daemon starts.
const COPIES: &str = "files";
/// Start of the name a copy has while it is being written, before it takes its own name.
const PART: &str = ".thoughts-part-";

/// Bring the copies of the files attached to chat `chat_id` up to date and return their folder.
/// Only plain files directly in the chat's own attachments folder are copied: a link, a
/// sub-folder, a file whose real place is elsewhere or one with other names (a hard link) is left
/// out, and so is everything when the chat's folder is itself a link. Each copy keeps the name of
/// the file it copies; a copy whose file is no longer there (or no longer qualifies) is removed.
/// Refused when any folder on the way to the copies is a link. Blocking: call it off the runtime.
pub fn copy(home: &Path, chat_id: &str) -> std::io::Result<PathBuf> {
    let bad = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, what.to_string());
    let mine = folder(home, chat_id).ok_or_else(|| bad("not a chat id"))?;
    let dest = mine.join(COPIES);
    if !plain_path(home, &dest, true)? { return Err(bad("a folder on the way to the copies is a link")); }
    let mut kept: std::collections::HashSet<std::ffi::OsString> = Default::default();
    let att = home.join("attachments");
    let src = att.join(chat_id);
    let plain_dir = |p: &Path| std::fs::symlink_metadata(p).map_or(false, |m| m.is_dir());
    if plain_dir(&att) && plain_dir(&src) {
        let real = crate::paths::canonical(&src)?;
        for e in std::fs::read_dir(&src)?.flatten() {
            let path = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            if !meta.file_type().is_file() { continue; }
            if crate::paths::canonical(&path).ok().and_then(|r| r.parent().map(|p| p == real)) != Some(true) { continue; }
            let to = dest.join(e.file_name());
            if !up_to_date(&to, &meta) {
                if let Err(err) = copy_one(&path, &meta, &dest, &to) {
                    crate::log::warn(format!("an attached file could not be copied for the assistant: {err}"));
                    continue;
                }
            }
            kept.insert(e.file_name());
        }
    }
    // Whatever else is there goes: copies of files no longer attached, half-written ones.
    for e in std::fs::read_dir(&dest)?.flatten() {
        if kept.contains(&e.file_name()) { continue; }
        let p = e.path();
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => { let _ = std::fs::remove_dir_all(&p); }
            Ok(_) => { let _ = std::fs::remove_file(&p); }
            Err(_) => {}
        }
    }
    texts(home, &mine, &dest, &kept);
    Ok(dest)
}

/// The folder (inside the chat's `folder`, beside `COPIES`) of the text versions of copied office files.
const TEXTS: &str = "text";
/// The folder (inside the chat's `folder`) of the text versions of library files referred to in the chat.
const LIBRARY: &str = "library";
/// End of a text version's name, after the file's whole name (`report.xlsx.txt`).
const TEXT_EXT: &str = ".txt";
/// End of the name of the note left when a file could not be turned into text (it holds the reason),
/// so that the same file is not tried again with every message.
const FAILED_EXT: &str = ".failed";
/// Longest all the text versions of one message may take together; what is left is tried with the next message.
const TEXTS_TIME: std::time::Duration = std::time::Duration::from_secs(45);

fn with_ext(name: &std::ffi::OsStr, ext: &str) -> std::ffi::OsString { let mut n = name.to_os_string(); n.push(ext); n }

/// Does `to` carry the time of change of the file looked at (`meta`): made from that very content?
fn same_time(to: &Path, meta: &std::fs::Metadata) -> bool {
    let Ok(have) = std::fs::symlink_metadata(to) else { return false };
    have.file_type().is_file() && matches!((have.modified(), meta.modified()), (Ok(a), Ok(b)) if a == b)
}

/// Write `bytes` as `to` in `dir` (under a passing name first), read-only, with the time of change `meta` has.
fn write_one(dir: &Path, to: &Path, bytes: &[u8], meta: &std::fs::Metadata) -> std::io::Result<()> {
    use std::io::Write;
    let part = dir.join(format!("{PART}{}", uuid::Uuid::new_v4().simple()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut out = o.open(&part)?;
    let written = out.write_all(bytes).and_then(|_| match meta.modified() { Ok(t) => out.set_modified(t), Err(_) => Ok(()) });
    drop(out);
    #[cfg(unix)]
    let written = written.and_then(|_| { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o400)) });
    let placed = written.and_then(|_| std::fs::rename(&part, to));
    if placed.is_err() { let _ = std::fs::remove_file(&part); }
    placed
}

/// Make sure `dir` holds the text version of the office file `src` (or the note saying why there
/// is none) under `name`, made from the content `src` has now. Returns the two names it may have.
/// Nothing is made once `until` has passed. Never fails: a file that cannot be read or turned into
/// text just has no text version.
fn text_of(dir: &Path, src: &Path, name: &std::ffi::OsStr, until: std::time::Instant) -> [std::ffi::OsString; 2] {
    let names = [with_ext(name, TEXT_EXT), with_ext(name, FAILED_EXT)];
    let (ok, failed) = (dir.join(&names[0]), dir.join(&names[1]));
    let Ok(meta) = std::fs::symlink_metadata(src) else { return names };
    if !meta.file_type().is_file() || same_time(&ok, &meta) || same_time(&failed, &meta) { return names; }
    let now = std::time::Instant::now();
    if now >= until { return names; }
    let made = if meta.len() > crate::as_text::IN_MAX { Err("the file is too big".to_string()) } else {
        match std::fs::read(src) {
            Ok(bytes) => crate::as_text::convert_by(&name.to_string_lossy(), &bytes, until.min(now + crate::as_text::TIME_MAX)),
            Err(_) => Err("the file could not be opened".to_string()),
        }
    };
    let (to, other, bytes) = match &made { Ok(t) => (&ok, &failed, t.as_bytes()), Err(why) => (&failed, &ok, why.as_bytes()) };
    let _ = std::fs::remove_file(other);
    if let Err(e) = write_one(dir, to, bytes, &meta) { crate::log::warn(format!("the text version of an attached file could not be written: {e}")); }
    names
}

/// Remove from `dir` everything whose name is not in `keep`.
fn sweep(dir: &Path, keep: &std::collections::HashSet<std::ffi::OsString>) {
    let Ok(list) = std::fs::read_dir(dir) else { return };
    for e in list.flatten() {
        if keep.contains(&e.file_name()) { continue; }
        let p = e.path();
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => { let _ = std::fs::remove_dir_all(&p); }
            Ok(_) => { let _ = std::fs::remove_file(&p); }
            Err(_) => {}
        }
    }
}

/// Bring the text versions of the copies in `dest` (named in `kept`) up to date, in `mine/text`:
/// made for a new or changed copy, kept for an unchanged one, removed with the copy they are of.
fn texts(home: &Path, mine: &Path, dest: &Path, kept: &std::collections::HashSet<std::ffi::OsString>) {
    let dir = mine.join(TEXTS);
    let office: Vec<&std::ffi::OsString> = kept.iter().filter(|n| crate::as_text::convertible(&n.to_string_lossy())).collect();
    if office.is_empty() {
        // Nothing to have a text version of: no folder either (only ever removed as a plain folder of this chat's).
        if std::fs::symlink_metadata(&dir).is_ok() && plain_path(home, mine, false).unwrap_or(false) {
            match std::fs::symlink_metadata(&dir) { Ok(m) if m.is_dir() => { let _ = std::fs::remove_dir_all(&dir); } Ok(_) => { let _ = std::fs::remove_file(&dir); } Err(_) => {} }
        }
        return;
    }
    if !plain_path(home, &dir, true).unwrap_or(false) { crate::log::warn("the folder for text versions of attached files is not a plain folder: none were made".to_string()); return; }
    let until = std::time::Instant::now() + TEXTS_TIME;
    let mut keep: std::collections::HashSet<std::ffi::OsString> = Default::default();
    for name in office { keep.extend(text_of(&dir, &dest.join(name), name, until)); }
    sweep(&dir, &keep);
}

/// The name a library file's text version has: a fingerprint of its whole path, then its own name.
fn library_name(file: &Path) -> std::ffi::OsString {
    let mut n = std::ffi::OsString::from(format!("{}-", &crate::updates::sha(file.to_string_lossy().as_bytes())[..16]));
    n.push(file.file_name().unwrap_or_default());
    n
}

/// Make the text versions of the office files among `files` (library files a message of chat
/// `chat_id` refers to; folders and other kinds are left alone), in the chat's own folder. They
/// stay for the chat's later messages; with more than a hundred, those not asked for now go.
/// Blocking: call it off the runtime. Never fails.
pub fn library_texts(home: &Path, chat_id: &str, files: &[PathBuf]) {
    let office: Vec<&PathBuf> = files.iter().filter(|f| crate::as_text::convertible(&f.to_string_lossy())).collect();
    if office.is_empty() { return; }
    let Some(mine) = folder(home, chat_id) else { return };
    let dir = mine.join(LIBRARY);
    if !plain_path(home, &dir, true).unwrap_or(false) { return; }
    let until = std::time::Instant::now() + TEXTS_TIME;
    let mut keep: std::collections::HashSet<std::ffi::OsString> = Default::default();
    for f in office { keep.extend(text_of(&dir, f, &library_name(f), until)); }
    if std::fs::read_dir(&dir).map_or(0, |d| d.count()) > 200 { sweep(&dir, &keep); }
}

/// What there is to read of an office file instead of the file itself.
#[derive(Debug, PartialEq)]
pub enum Text {
    /// Not an office file: the Read tool opens it as it is (or nothing can).
    NotNeeded,
    /// Its text version.
    Ready(PathBuf),
    /// There is none, and why.
    Failed(String),
}

fn text_in(dir: &Path, name: &std::ffi::OsStr, file: &str) -> Text {
    if !crate::as_text::convertible(file) { return Text::NotNeeded; }
    let ok = dir.join(with_ext(name, TEXT_EXT));
    if std::fs::symlink_metadata(&ok).map_or(false, |m| m.file_type().is_file()) { return Text::Ready(ok); }
    let why = std::fs::read_to_string(dir.join(with_ext(name, FAILED_EXT))).ok().map(|w| w.chars().take(200).collect::<String>().replace(['\n', '\r'], " "));
    Text::Failed(why.filter(|w| !w.trim().is_empty()).unwrap_or_else(|| "no text version could be made".to_string()))
}

/// The text version of `file`: a copy made by `copy` (then it is beside the copies' folder), or
/// any other path (an office file that was not copied has none).
pub fn text_for(file: &Path) -> Text {
    let name = file.file_name().unwrap_or_default();
    match file.parent().filter(|d| d.file_name().map_or(false, |n| n == COPIES)).and_then(Path::parent) {
        Some(mine) => text_in(&mine.join(TEXTS), name, &file.to_string_lossy()),
        None => no_text(file),
    }
}

/// An office file with nowhere a text version of it could be.
fn no_text(file: &Path) -> Text {
    if crate::as_text::convertible(&file.to_string_lossy()) { Text::Failed("no text version could be made".to_string()) } else { Text::NotNeeded }
}

/// The text version of library file `file` made for chat `chat_id` by `library_texts`.
pub fn library_text_for(home: &Path, chat_id: &str, file: &Path) -> Text {
    match folder(home, chat_id) {
        Some(mine) => text_in(&mine.join(LIBRARY), &library_name(file), &file.to_string_lossy()),
        None => no_text(file),
    }
}

/// One file in the prompt's list of files to read: its path and, for an office file, where its
/// text version is (or that there is none, so that the assistant says so instead of guessing).
pub fn listed(path: &str, text: Text) -> String {
    let what = crate::as_text::what(path);
    match text {
        Text::Ready(t) => format!("- {path}\n  ({what}, which the Read tool cannot open: read its text version instead, {})\n", t.display()),
        Text::Failed(why) => format!("- {path}\n  ({what}, which the Read tool cannot open, and no text version of it could be made here: {why}. Say so plainly instead of guessing what is in it.)\n"),
        Text::NotNeeded if crate::as_text::opaque(path) => format!("- {path}\n  (a kind of file that cannot be read here. Say so plainly instead of guessing what is in it.)\n"),
        Text::NotNeeded => format!("- {path}\n"),
    }
}

/// For the prompt's line about files attached earlier (`copies`: their folder): where the text
/// versions of the office files among them are. Empty when there are none.
pub fn earlier_note(copies: &Path) -> String {
    let Some(dir) = copies.parent().map(|m| m.join(TEXTS)) else { return String::new() };
    let any = std::fs::read_dir(&dir).map_or(false, |d| d.flatten().any(|e| e.file_name().to_string_lossy().ends_with(TEXT_EXT)));
    if !any { return String::new(); }
    format!("Spreadsheets, Word documents and slide decks among them cannot be opened by the Read tool: read their text versions in {}, each named after its file with {TEXT_EXT} added.\n", dir.display())
}

/// Is `to` a plain file copied from the file looked at (`meta`): same size and same time of change?
fn up_to_date(to: &Path, meta: &std::fs::Metadata) -> bool {
    let Ok(have) = std::fs::symlink_metadata(to) else { return false };
    have.file_type().is_file() && have.len() == meta.len()
        && matches!((have.modified(), meta.modified()), (Ok(a), Ok(b)) if a == b)
}

/// Copy one attached file, only if what is opened is the very file looked at (`meta`): not a link
/// put in its place since, and not a file with a second name somewhere else. The copy is written
/// under a passing name in `dir` and then takes its place at `to` (a run reading the earlier copy
/// keeps reading that one). It is read-only and carries the file's time of change.
fn copy_one(from: &Path, meta: &std::fs::Metadata, dir: &Path, to: &Path) -> std::io::Result<()> {
    let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "not the attached file");
    let mut f = std::fs::File::open(from)?;
    let opened = f.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (opened.dev(), opened.ino()) != (meta.dev(), meta.ino()) || opened.nlink() != 1 { return Err(bad()); }
    }
    if !opened.is_file() || opened.len() != meta.len() { return Err(bad()); }
    let part = dir.join(format!("{PART}{}", uuid::Uuid::new_v4().simple()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut out = o.open(&part)?;
    let written = std::io::copy(&mut f, &mut out).and_then(|_| match opened.modified() {
        Ok(t) => out.set_modified(t),
        Err(_) => Ok(()),
    });
    drop(out);
    // (Read-only on disk only where that does not keep the folder from being removed afterwards.)
    #[cfg(unix)]
    let written = written.and_then(|_| { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o400)) });
    let placed = written.and_then(|_| std::fs::rename(&part, to));
    if placed.is_err() { let _ = std::fs::remove_file(&part); let _ = std::fs::remove_file(to); }
    placed
}

/// Remove the copies made for `chat_id`'s runs (when the chat is deleted), or with `None` all of
/// them (when the daemon starts). Nothing is removed through a link.
pub fn clear(home: &Path, chat_id: Option<&str>) {
    let target = match chat_id {
        Some(id) => match folder(home, id) { Some(f) => f, None => return },
        None => root(home),
    };
    match std::fs::symlink_metadata(&target) {
        Ok(m) if m.is_dir() => { if plain_path(home, &target, false).unwrap_or(false) { let _ = std::fs::remove_dir_all(&target); } }
        Ok(_) => { let _ = std::fs::remove_file(&target); }
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn office_files_get_a_text_version_beside_their_copies() {
        let home = std::env::temp_dir().join(format!("thoughts-run-texts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join("attachments/c-1")).unwrap();
        let att = home.join("attachments/c-1");
        std::fs::write(att.join("letter.rtf"), b"{\\rtf1 Dear Sara,\\par the total is 950.\\par}").unwrap();
        std::fs::write(att.join("broken.docx"), b"not a word file").unwrap();
        std::fs::write(att.join("notes.txt"), b"plain").unwrap();
        std::fs::write(att.join("Budget.numbers"), b"apple").unwrap();
        let dir = copy(&home, "c-1").unwrap();
        let mine = folder(&home, "c-1").unwrap();
        let text = mine.join("text");
        let names = |d: &Path| { let mut n: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect(); n.sort(); n };
        // Inside the chat's own folder, so the rule that opens the copies to its run opens these
        // too, and every other chat's are under a folder closed to it. The copies themselves are as before.
        assert_eq!(names(&dir), ["Budget.numbers", "broken.docx", "letter.rtf", "notes.txt"]);
        assert_eq!(names(&text), ["broken.docx.failed", "letter.rtf.txt"]);
        let closed: Vec<PathBuf> = closed_for("c-2").unwrap().iter().map(|r| root(&home).join(r)).collect();
        assert!(closed.iter().any(|c| text.starts_with(c)) && !closed_for("c-1").unwrap().iter().any(|r| text.starts_with(root(&home).join(r))));
        let made = std::fs::read_to_string(text.join("letter.rtf.txt")).unwrap();
        assert!(made.contains("Dear Sara,\nthe total is 950.\n"), "{made}");
        #[cfg(unix)]
        assert!(std::fs::metadata(text.join("letter.rtf.txt")).unwrap().permissions().readonly());
        // What the prompt says of each file.
        assert_eq!(text_for(&dir.join("letter.rtf")), Text::Ready(text.join("letter.rtf.txt")));
        assert!(matches!(text_for(&dir.join("broken.docx")), Text::Failed(why) if why.contains("not what its name says")));
        assert_eq!((text_for(&dir.join("notes.txt")), text_for(&dir.join("Budget.numbers"))), (Text::NotNeeded, Text::NotNeeded));
        // (An office file that was not copied has no text version: said, not guessed.)
        assert!(matches!(text_for(&att.join("letter.rtf")), Text::Failed(_)));
        let p = dir.join("letter.rtf").to_string_lossy().to_string();
        let line = listed(&p, text_for(&dir.join("letter.rtf")));
        assert!(line.starts_with(&format!("- {p}\n  (a document, which the Read tool cannot open: read its text version instead, {})", text.join("letter.rtf.txt").display())), "{line}");
        let line = listed("/x/broken.docx", text_for(&dir.join("broken.docx")));
        assert!(line.contains("no text version of it could be made here: the file is not what its name says. Say so plainly"), "{line}");
        assert_eq!(listed("/x/notes.txt", Text::NotNeeded), "- /x/notes.txt\n");
        assert!(listed("/x/Budget.numbers", Text::NotNeeded).contains("cannot be read here"));
        assert!(earlier_note(&dir).contains(&text.display().to_string()) && earlier_note(&dir).contains(".txt added"));
        // The next message: what is unchanged is not made again (the failed one is not tried again
        // either); a changed file gets a new text version; a file no longer attached loses its own;
        // anything else found there goes.
        #[cfg(unix)]
        let ino = |p: &Path| { use std::os::unix::fs::MetadataExt; std::fs::metadata(p).unwrap().ino() };
        #[cfg(unix)]
        let (was_ok, was_failed) = (ino(&text.join("letter.rtf.txt")), ino(&text.join("broken.docx.failed")));
        std::fs::write(text.join("planted.txt"), b"x").unwrap();
        copy(&home, "c-1").unwrap();
        assert_eq!(names(&text), ["broken.docx.failed", "letter.rtf.txt"]);
        #[cfg(unix)]
        assert_eq!((ino(&text.join("letter.rtf.txt")), ino(&text.join("broken.docx.failed"))), (was_ok, was_failed), "made again without a change");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(att.join("letter.rtf"), b"{\\rtf1 Dear Omar,\\par}").unwrap();
        std::fs::write(att.join("broken.docx"), b"{\\rtf1 still not word}").unwrap();
        std::fs::remove_file(att.join("broken.docx")).unwrap();
        copy(&home, "c-1").unwrap();
        assert_eq!(names(&text), ["letter.rtf.txt"]);
        assert!(std::fs::read_to_string(text.join("letter.rtf.txt")).unwrap().contains("Dear Omar,"));
        // A file that fails now and reads later: the note goes, the text comes.
        std::fs::write(att.join("late.rtf"), b"nothing yet").unwrap();
        copy(&home, "c-1").unwrap();
        assert_eq!(names(&text), ["late.rtf.failed", "letter.rtf.txt"]);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(att.join("late.rtf"), b"{\\rtf1 here now\\par}").unwrap();
        copy(&home, "c-1").unwrap();
        assert_eq!(names(&text), ["late.rtf.txt", "letter.rtf.txt"]);
        // No office file left: no folder, and nothing said about one.
        std::fs::remove_file(att.join("letter.rtf")).unwrap();
        std::fs::remove_file(att.join("late.rtf")).unwrap();
        copy(&home, "c-1").unwrap();
        assert!(!text.exists() && earlier_note(&dir).is_empty());
        // A link where the folder should be: nothing is written through it, and the message's copies are still made.
        #[cfg(unix)]
        {
            let elsewhere = home.join("elsewhere");
            std::fs::create_dir_all(&elsewhere).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &text).unwrap();
            std::fs::write(att.join("letter.rtf"), b"{\\rtf1 again\\par}").unwrap();
            assert_eq!(names(&copy(&home, "c-1").unwrap()), ["Budget.numbers", "letter.rtf", "notes.txt"]);
            assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0, "written through a link");
            assert!(matches!(text_for(&dir.join("letter.rtf")), Text::Failed(_)));
            std::fs::remove_file(&text).unwrap();
        }
        // Library files a message refers to: theirs are in the chat's folder too, named by their whole path.
        let lib = home.join("lib");
        std::fs::create_dir_all(lib.join("a")).unwrap();
        std::fs::create_dir_all(lib.join("b")).unwrap();
        std::fs::write(lib.join("a/plan.rtf"), b"{\\rtf1 plan A\\par}").unwrap();
        std::fs::write(lib.join("b/plan.rtf"), b"{\\rtf1 plan B\\par}").unwrap();
        std::fs::write(lib.join("a/note.md"), b"# hi").unwrap();
        let refs = [lib.join("a/plan.rtf"), lib.join("b/plan.rtf"), lib.join("a/note.md"), lib.join("a"), lib.join("missing.docx")];
        library_texts(&home, "c-1", &refs);
        let (Text::Ready(a), Text::Ready(b)) = (library_text_for(&home, "c-1", &refs[0]), library_text_for(&home, "c-1", &refs[1])) else { panic!("no text version of a library file") };
        assert!(a != b && a.starts_with(mine.join("library")) && std::fs::read_to_string(&a).unwrap().contains("plan A") && std::fs::read_to_string(&b).unwrap().contains("plan B"));
        assert_eq!((library_text_for(&home, "c-1", &refs[2]), library_text_for(&home, "c-1", &refs[3])), (Text::NotNeeded, Text::NotNeeded));
        assert!(matches!(library_text_for(&home, "c-1", &refs[4]), Text::Failed(_)));
        assert!(matches!(library_text_for(&home, "c-2", &refs[0]), Text::Failed(_)), "another chat's text version");
        assert_eq!(std::fs::read(lib.join("a/plan.rtf")).unwrap(), b"{\\rtf1 plan A\\par}");
        library_texts(&home, "../c-2", &refs);
        // They go with the chat.
        clear(&home, Some("c-1"));
        assert!(!mine.exists());
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn a_run_gets_copies_of_its_own_chats_files_only() {
        let home = std::env::temp_dir().join(format!("thoughts-run-files-{}", uuid::Uuid::new_v4()));
        for sub in ["attachments/c-1/sub", "attachments/c-2"] { std::fs::create_dir_all(home.join(sub)).unwrap(); }
        std::fs::write(home.join("attachments/c-1/shot.png"), b"png").unwrap();
        std::fs::write(home.join("attachments/c-1/notes.txt"), b"blue").unwrap();
        std::fs::write(home.join("attachments/c-1/sub/inner.txt"), b"x").unwrap();
        std::fs::write(home.join("attachments/c-2/theirs.png"), b"theirs").unwrap();
        std::fs::write(home.join("secrets.json"), b"{}").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(home.join("secrets.json"), home.join("attachments/c-1/link.json")).unwrap();
            std::os::unix::fs::symlink(home.join("attachments/c-2/theirs.png"), home.join("attachments/c-1/theirs.png")).unwrap();
            std::fs::hard_link(home.join("secrets.json"), home.join("attachments/c-1/hard.json")).unwrap();
        }
        let other = copy(&home, "c-2").unwrap();
        let dir = copy(&home, "c-1").unwrap();
        // In a folder of its own, below the chat's digits-only path.
        assert_eq!(dir.parent(), folder(&home, "c-1").as_deref());
        assert!(dir.starts_with(root(&home)) && !dir.starts_with(home.join("attachments")));
        let rel = folder(&home, "c-1").unwrap().strip_prefix(root(&home)).unwrap().to_string_lossy().replace('\\', "/");
        assert!(rel.bytes().all(|b| b.is_ascii_digit() || b == b'/') && rel.split('/').count() == DEPTH, "{rel}");
        let names = |d: &Path| { let mut n: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect(); n.sort(); n };
        // Only this chat's own plain files: no link, no hard link, no sub-folder, nothing of another chat.
        assert_eq!(names(&dir), ["notes.txt", "shot.png"]);
        assert_eq!(std::fs::read(dir.join("notes.txt")).unwrap(), b"blue");
        #[cfg(unix)]
        assert!(std::fs::metadata(dir.join("notes.txt")).unwrap().permissions().readonly());
        // Another chat's copies, made at the same time, are untouched — and every place they could be is closed to this run.
        assert_eq!(names(&other), ["theirs.png"]);
        let closed: Vec<PathBuf> = closed_for("c-1").unwrap().iter().map(|r| root(&home).join(r)).collect();
        assert_eq!(closed.len(), DEPTH * 9);
        assert!(closed_for("c-1").unwrap().iter().all(|r| r.bytes().all(|b| b.is_ascii_digit() || b == b'/')), "a letter has a case");
        assert!(closed.iter().any(|c| other.starts_with(c)), "another chat's copies are not closed");
        assert!(!closed.iter().any(|c| dir.starts_with(c)), "its own copies are closed");
        // …also for chats that have no copies yet: whatever their folder, it is under a closed one.
        for i in 0..200 { let f = folder(&home, &format!("later-{i}")).unwrap(); assert!(closed.iter().any(|c| f.starts_with(c))); }
        assert_eq!(closed_for("../c-2"), None);
        // An id that is not one (it could name a path) copies nothing.
        assert!(copy(&home, "../c-2").is_err() && folder(&home, "..").is_none());
        // Copying again keeps the copies that are still right (the same file on disk, not copied
        // again), copies a changed file anew, and removes the copy of a file no longer attached and
        // anything else found there. A chat without files gets an empty folder.
        #[cfg(unix)]
        let ino = |p: &Path| { use std::os::unix::fs::MetadataExt; std::fs::metadata(p).unwrap().ino() };
        #[cfg(unix)]
        let shot = ino(&dir.join("shot.png"));
        std::fs::remove_file(home.join("attachments/c-1/notes.txt")).unwrap();
        std::fs::write(home.join("attachments/c-1/later.txt"), b"one").unwrap();
        std::fs::write(dir.join(".thoughts-part-left"), b"half").unwrap();
        std::fs::create_dir_all(dir.join("planted")).unwrap();
        assert_eq!(copy(&home, "c-1").unwrap(), dir);
        assert_eq!(names(&dir), ["later.txt", "shot.png"]);
        #[cfg(unix)]
        assert_eq!(ino(&dir.join("shot.png")), shot, "an unchanged file was copied again");
        std::fs::write(home.join("attachments/c-1/later.txt"), b"two!").unwrap();
        copy(&home, "c-1").unwrap();
        assert_eq!(std::fs::read(dir.join("later.txt")).unwrap(), b"two!");
        #[cfg(unix)]
        assert_eq!(ino(&dir.join("shot.png")), shot);
        std::fs::remove_file(home.join("attachments/c-1/later.txt")).unwrap();
        assert_eq!(names(&copy(&home, "c-1").unwrap()), ["shot.png"]);
        assert!(names(&copy(&home, "c-9").unwrap()).is_empty());
        // A chat's attachments folder that is a link is not followed.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(home.join("attachments/c-2"), home.join("attachments/c-4")).unwrap();
            assert!(names(&copy(&home, "c-4").unwrap()).is_empty());
        }
        // A link anywhere on the way to the copies: refused, and nothing is written through it.
        #[cfg(unix)]
        {
            let elsewhere = home.join("elsewhere");
            std::fs::create_dir_all(&elsewhere).unwrap();
            let first = root(&home).join(&rel[..1]);
            let aside = home.join("aside");
            std::fs::rename(&first, &aside).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &first).unwrap();
            assert!(copy(&home, "c-1").is_err());
            clear(&home, Some("c-1"));
            assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0, "written or removed through a link");
            std::fs::remove_file(&first).unwrap();
            std::fs::rename(&aside, &first).unwrap();
            assert!(copy(&home, "c-1").is_ok());
        }
        // Clean-up: when a chat is deleted only its own copies go; when the daemon starts, everything.
        let dir = copy(&home, "c-1").unwrap();
        clear(&home, Some("c-1"));
        assert!(!dir.exists() && other.exists());
        clear(&home, Some(".."));
        assert!(root(&home).exists());
        clear(&home, None);
        assert!(!root(&home).exists());
        // The originals are untouched throughout.
        assert_eq!(std::fs::read(home.join("attachments/c-1/shot.png")).unwrap(), b"png");
        assert_eq!(std::fs::read(home.join("attachments/c-2/theirs.png")).unwrap(), b"theirs");
        std::fs::remove_dir_all(&home).unwrap();
    }
}
