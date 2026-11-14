//! Copies of a chat's attachments for its assistant run.
//!
//! Every run is denied the whole attachments folder (`claude::protective_deny`), so that no rule
//! depends on which chats happen to have files when the run starts. A run reads the files of its
//! own chat from copies made for it when the message is sent, in a folder of its own under
//! `<home>/chat-files/`. That folder's place is worked out from the chat id (`folder`): one
//! single-letter folder per level, `DEPTH` levels down. Every place a copy of *another* chat could
//! ever be is then one of the 15 other letters at some level along this chat's path — a fixed list
//! that can be closed before any of those folders exist (`closed_for`).

use std::path::{Path, PathBuf};

/// The folder in the app's home holding the copies of every run.
pub const DIR: &str = "chat-files";
/// Levels below `DIR`: 16 hex digits of the chat id's fingerprint (two chats share a folder with a
/// chance of one in 2^64).
const DEPTH: usize = 16;
const HEX: &str = "0123456789abcdef";

pub fn root(home: &Path) -> PathBuf { home.join(DIR) }

fn digits(chat_id: &str) -> String { crate::updates::sha(chat_id.as_bytes())[..DEPTH].to_string() }

/// Where the copies for chat `chat_id` go; `None` for an id that is not a chat id.
pub fn folder(home: &Path, chat_id: &str) -> Option<PathBuf> {
    if !crate::chats::valid_id(chat_id) { return None; }
    let mut p = root(home);
    for c in digits(chat_id).chars() { p.push(c.to_string()); }
    Some(p)
}

/// Every folder under `DIR` that chat `chat_id`'s run must not read: beside each level of its own
/// path, the other 15. Together they hold every place any other chat's copies can be. For an id
/// that is not a chat id: the whole folder.
pub fn closed_for(home: &Path, chat_id: &str) -> Vec<PathBuf> {
    if !crate::chats::valid_id(chat_id) { return vec![root(home)]; }
    let mut out = Vec::with_capacity(DEPTH * 15);
    let mut at = root(home);
    for own in digits(chat_id).chars() {
        for c in HEX.chars().filter(|c| *c != own) { out.push(at.join(c.to_string())); }
        at.push(own.to_string());
    }
    out
}

/// Copy the files attached to chat `chat_id` into its folder (emptied first) and return that
/// folder. Only plain files directly in the chat's own attachments folder are copied: a link, a
/// sub-folder, a file whose real place is elsewhere or one with other names (a hard link) is left
/// out, and so is everything when the chat's folder is itself a link. Each copy keeps the name of
/// the file it copies.
pub fn copy(home: &Path, chat_id: &str) -> std::io::Result<PathBuf> {
    let dest = folder(home, chat_id).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a chat id"))?;
    clear(home, Some(chat_id));
    crate::paths::create_private_dir(&dest)?;
    let att = home.join("attachments");
    let src = att.join(chat_id);
    let plain_dir = |p: &Path| std::fs::symlink_metadata(p).map_or(false, |m| m.is_dir());
    if !plain_dir(&att) || !plain_dir(&src) { return Ok(dest); }
    let real = crate::paths::canonical(&src)?;
    for e in std::fs::read_dir(&src)?.flatten() {
        let path = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if !meta.file_type().is_file() { continue; }
        if crate::paths::canonical(&path).ok().and_then(|r| r.parent().map(|p| p == real)) != Some(true) { continue; }
        if let Err(err) = copy_one(&path, &meta, &dest.join(e.file_name())) {
            crate::log::warn(format!("an attached file could not be copied for the assistant: {err}"));
        }
    }
    Ok(dest)
}

/// Copy one attached file, only if what is opened is the very file looked at (`meta`): not a link
/// put in its place since, and not a file with a second name somewhere else. The copy is read-only.
fn copy_one(from: &Path, meta: &std::fs::Metadata, to: &Path) -> std::io::Result<()> {
    let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "not the attached file");
    let mut f = std::fs::File::open(from)?;
    let opened = f.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (opened.dev(), opened.ino()) != (meta.dev(), meta.ino()) || opened.nlink() != 1 { return Err(bad()); }
    }
    if !opened.is_file() || opened.len() != meta.len() { return Err(bad()); }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut out = o.open(to)?;
    if let Err(e) = std::io::copy(&mut f, &mut out) { drop(out); let _ = std::fs::remove_file(to); return Err(e); }
    drop(out);
    // (Read-only on disk only where that does not keep the folder from being removed afterwards.)
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o400))?; }
    Ok(())
}

/// Remove the copies made for `chat_id`'s run (when it ends or never started), or with `None` all
/// of them (when the daemon starts).
pub fn clear(home: &Path, chat_id: Option<&str>) {
    let target = match chat_id {
        Some(id) => match folder(home, id) { Some(f) => f, None => return },
        None => root(home),
    };
    match std::fs::symlink_metadata(&target) {
        Ok(m) if m.is_dir() => { let _ = std::fs::remove_dir_all(&target); }
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
        assert_eq!(Some(dir.clone()), folder(&home, "c-1"));
        assert!(dir.starts_with(root(&home)) && !dir.starts_with(home.join("attachments")));
        let names = |d: &Path| { let mut n: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect(); n.sort(); n };
        // Only this chat's own plain files: no link, no hard link, no sub-folder, nothing of another chat.
        assert_eq!(names(&dir), ["notes.txt", "shot.png"]);
        assert_eq!(std::fs::read(dir.join("notes.txt")).unwrap(), b"blue");
        #[cfg(unix)]
        assert!(std::fs::metadata(dir.join("notes.txt")).unwrap().permissions().readonly());
        // Another chat's copies, made at the same time, are untouched — and every place they could be is closed to this run.
        assert_eq!(names(&other), ["theirs.png"]);
        let closed = closed_for(&home, "c-1");
        assert_eq!(closed.len(), DEPTH * 15);
        assert!(closed.iter().any(|c| other.starts_with(c)), "another chat's copies are not closed");
        assert!(!closed.iter().any(|c| dir.starts_with(c)), "its own copies are closed");
        // …also for chats that have no copies yet: whatever their folder, it is under a closed one.
        for i in 0..200 { let f = folder(&home, &format!("later-{i}")).unwrap(); assert!(closed.iter().any(|c| f.starts_with(c))); }
        assert_eq!(closed_for(&home, "../c-2"), vec![root(&home)]);
        // An id that is not one (it could name a path) copies nothing.
        assert!(copy(&home, "../c-2").is_err() && folder(&home, "..").is_none());
        // Copying again starts from an empty folder; a chat without files gets an empty one.
        std::fs::remove_file(home.join("attachments/c-1/notes.txt")).unwrap();
        assert_eq!(names(&copy(&home, "c-1").unwrap()), ["shot.png"]);
        assert!(names(&copy(&home, "c-9").unwrap()).is_empty());
        // A chat's attachments folder that is a link is not followed.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(home.join("attachments/c-2"), home.join("attachments/c-4")).unwrap();
            assert!(names(&copy(&home, "c-4").unwrap()).is_empty());
        }
        // Clean-up: when a run ends only its own copies go; when the daemon starts, everything.
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
