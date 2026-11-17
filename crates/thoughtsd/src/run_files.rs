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
    Ok(dest)
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
