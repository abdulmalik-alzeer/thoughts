//! Is a folder kept in sync by a cloud drive (iCloud Drive, Dropbox, OneDrive, Google Drive…)?
//!
//! The answer is given for the folder's real location — symlinks resolved, letter case ignored —
//! against the places those services really sync on this computer. It never depends on what the
//! path happens to be called: `~/Notes/Dropbox-export` is not in Dropbox, and with iCloud's
//! "Desktop & Documents" switched on, plain `~/Documents` is in iCloud Drive.
//!
//! Two things in the app turn on it: a library whose backup is "Cloud folder" should really be in
//! one, and a library a cloud drive already syncs should not be sent to an online location as well.

use std::path::{Path, PathBuf};

/// The service a folder directly inside the home folder belongs to, by its name — the places the
/// classic desktop clients create. (Only whole names; never a word somewhere in a longer name.)
fn home_folder_service(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    let is = |exact: &[&str], starts: &[&str]| exact.contains(&n.as_str()) || starts.iter().any(|s| n.starts_with(s));
    if is(&["dropbox"], &["dropbox ("]) { Some("Dropbox") }
    else if is(&["onedrive"], &["onedrive - ", "onedrive-"]) { Some("OneDrive") }
    else if is(&["google drive", "googledrive", "my drive"], &[]) { Some("Google Drive") }
    else if is(&["iclouddrive", "icloud drive"], &[]) { Some("iCloud Drive") }
    else if is(&["box", "box sync"], &[]) { Some("Box") }
    else if is(&["pcloud drive", "pclouddrive"], &[]) { Some("pCloud") }
    else if is(&["mega", "megasync"], &[]) { Some("MEGA") }
    else if is(&["nextcloud"], &[]) { Some("Nextcloud") }
    else { None }
}

/// The service behind a folder of `~/Library/CloudStorage` (macOS names them `Provider-Account`).
/// A provider this list does not know is still a cloud drive: its name is left empty.
fn cloud_storage_service(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.starts_with("dropbox") { "Dropbox" }
    else if n.starts_with("onedrive") { "OneDrive" }
    else if n.starts_with("googledrive") { "Google Drive" }
    else if n.starts_with("box") { "Box" }
    else if n.starts_with("icloud") { "iCloud Drive" }
    else if n.starts_with("pcloud") { "pCloud" }
    else if n.starts_with("nextcloud") { "Nextcloud" }
    else { "" }
}

/// Every folder on this computer that a cloud drive keeps in sync, with the service's name
/// (empty when the service is one this module has no name for).
/// `home` is the user's home folder; `env` reads an environment variable (Windows names the
/// OneDrive folders there). Only folders that exist are listed.
pub fn sync_roots(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<(PathBuf, &'static str)> {
    let mut roots: Vec<(PathBuf, &'static str)> = vec![];
    let mut add = |p: PathBuf, service: &'static str| { if p.is_dir() && !roots.iter().any(|(r, _)| *r == p) { roots.push((p, service)); } };

    // iCloud Drive on a Mac, and every app's own iCloud container.
    let mobile = home.join("Library/Mobile Documents");
    add(mobile.clone(), "iCloud Drive");
    // "Desktop & Documents Folders" in iCloud: the two folders stay where they are in the home
    // folder, and iCloud Drive holds links to them. That is the only trace in the file system.
    for folder in ["Desktop", "Documents"] {
        let link = mobile.join("com~apple~CloudDocs").join(folder);
        if std::fs::symlink_metadata(&link).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            if let Ok(target) = dunce::canonicalize(&link) { add(target, "iCloud Drive"); }
        }
    }
    // Dropbox, OneDrive, Google Drive, Box… as macOS file providers.
    for e in std::fs::read_dir(home.join("Library/CloudStorage")).into_iter().flatten().flatten() {
        add(e.path(), cloud_storage_service(&e.file_name().to_string_lossy()));
    }
    // The folders the desktop clients create directly in the home folder (all systems).
    for e in std::fs::read_dir(home).into_iter().flatten().flatten() {
        if let Some(service) = home_folder_service(&e.file_name().to_string_lossy()) { add(e.path(), service); }
    }
    // Dropbox records where its folder really is (it can be moved anywhere).
    let mut dropbox_info = vec![home.join(".dropbox/info.json")];
    for var in ["APPDATA", "LOCALAPPDATA"] { if let Some(d) = env(var) { dropbox_info.push(Path::new(&d).join("Dropbox/info.json")); } }
    for file in dropbox_info {
        let Some(v) = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { continue };
        for account in v.as_object().into_iter().flat_map(|o| o.values()) {
            if let Some(p) = account["path"].as_str() { add(PathBuf::from(p), "Dropbox"); }
        }
    }
    // OneDrive on Windows names its folders in the environment.
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] { if let Some(p) = env(var) { add(PathBuf::from(p), "OneDrive"); } }
    // Google Drive's older Mac client mounts a volume.
    add(PathBuf::from("/Volumes/GoogleDrive"), "Google Drive");
    roots
}

/// The service that syncs `path` (its name may be empty, see `sync_roots`), given the sync roots
/// of the machine. `path` need not exist yet: the part of it that does is resolved.
pub fn service_in(path: &Path, roots: &[(PathBuf, &'static str)]) -> Option<&'static str> {
    roots.iter().find(|(root, _)| crate::paths::inside_ci(path, root)).map(|(_, service)| *service)
}

/// The cloud drive that keeps `path` in sync on this computer, if any.
pub fn service_of(path: &Path) -> Option<&'static str> {
    let home = dirs::home_dir()?;
    service_in(path, &sync_roots(&home, &|k| std::env::var(k).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = dunce::canonicalize(std::env::temp_dir()).unwrap().join(format!("thoughts-cloud-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn dirs_in(home: &Path, names: &[&str]) { for n in names { std::fs::create_dir_all(home.join(n)).unwrap(); } }
    const NO_ENV: &dyn Fn(&str) -> Option<String> = &|_| None;

    #[test]
    fn a_folder_is_judged_by_where_it_really_is_not_by_its_name() {
        let home = tmp();
        dirs_in(&home, &["Library/Mobile Documents/com~apple~CloudDocs/Thoughts", "Library/CloudStorage/Dropbox-Personal", "Library/CloudStorage/OneDrive-Contoso",
            "Library/CloudStorage/GoogleDrive-me@example.com/My Drive", "Notes/Dropbox-export", "Notes/iCloud backup", "Documents/Thoughts", "Desktop", "OneDrive - Contoso", "Projects/Google Drive notes"]);
        let roots = sync_roots(&home, NO_ENV);
        let at = |rel: &str| service_in(&home.join(rel), &roots);
        // Really inside a cloud drive.
        assert_eq!(at("Library/Mobile Documents/com~apple~CloudDocs/Thoughts"), Some("iCloud Drive"));
        assert_eq!(at("Library/CloudStorage/Dropbox-Personal/Notes"), Some("Dropbox"));
        assert_eq!(at("Library/CloudStorage/OneDrive-Contoso"), Some("OneDrive"));
        assert_eq!(at("Library/CloudStorage/GoogleDrive-me@example.com/My Drive/Thoughts"), Some("Google Drive"));
        assert_eq!(at("OneDrive - Contoso/Thoughts"), Some("OneDrive"));
        assert_eq!(at("library/mobile documents/com~apple~clouddocs/thoughts"), Some("iCloud Drive"), "letter case does not matter");
        // A cloud word in the name is not a cloud drive.
        for plain in ["Notes/Dropbox-export", "Notes/iCloud backup", "Projects/Google Drive notes", "Thoughts", "Documents/Thoughts", "Desktop/Thoughts"] {
            assert_eq!(at(plain), None, "{plain}");
        }
        // A folder that does not exist yet (a new library) is judged by the folder it will be in.
        assert_eq!(at("Library/CloudStorage/Dropbox-Personal/New/Thoughts"), Some("Dropbox"));
        assert_eq!(at("Notes/New/Thoughts"), None);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn icloud_desktop_and_documents_and_links_into_a_drive_are_recognised() {
        let home = tmp();
        dirs_in(&home, &["Library/Mobile Documents/com~apple~CloudDocs", "Documents/Thoughts", "Desktop", "Notes"]);
        // Before "Desktop & Documents" is switched on, Documents is an ordinary folder.
        assert_eq!(service_in(&home.join("Documents/Thoughts"), &sync_roots(&home, NO_ENV)), None);
        // Switched on: iCloud Drive holds links to the two folders, which stay in the home folder.
        for f in ["Desktop", "Documents"] { std::os::unix::fs::symlink(home.join(f), home.join("Library/Mobile Documents/com~apple~CloudDocs").join(f)).unwrap(); }
        let roots = sync_roots(&home, NO_ENV);
        assert_eq!(service_in(&home.join("Documents/Thoughts"), &roots), Some("iCloud Drive"));
        assert_eq!(service_in(&home.join("Desktop/New library"), &roots), Some("iCloud Drive"));
        assert_eq!(service_in(&home.join("Notes"), &roots), None);
        // A shortcut elsewhere that leads into the drive is in the drive.
        std::os::unix::fs::symlink(home.join("Library/Mobile Documents/com~apple~CloudDocs"), home.join("Notes/cloud")).unwrap();
        assert_eq!(service_in(&home.join("Notes/cloud/Thoughts"), &roots), Some("iCloud Drive"));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn dropbox_and_onedrive_say_where_their_folders_are() {
        let home = tmp();
        dirs_in(&home, &[".dropbox", "Work/Shared Dropbox", "Elsewhere/OD", "AppData/Dropbox", "Moved/Box"]);
        std::fs::write(home.join(".dropbox/info.json"), serde_json::json!({"business": {"path": home.join("Work/Shared Dropbox"), "host": 1}}).to_string()).unwrap();
        std::fs::write(home.join("AppData/Dropbox/info.json"), serde_json::json!({"personal": {"path": home.join("Moved/Box")}}).to_string()).unwrap();
        let env = |k: &str| match k {
            "OneDriveCommercial" => Some(home.join("Elsewhere/OD").to_string_lossy().to_string()),
            "APPDATA" => Some(home.join("AppData").to_string_lossy().to_string()),
            "OneDrive" => Some(home.join("gone").to_string_lossy().to_string()),
            _ => None,
        };
        let roots = sync_roots(&home, &env);
        assert_eq!(service_in(&home.join("Work/Shared Dropbox/Thoughts"), &roots), Some("Dropbox"));
        assert_eq!(service_in(&home.join("Moved/Box/Thoughts"), &roots), Some("Dropbox"));
        assert_eq!(service_in(&home.join("Elsewhere/OD/Thoughts"), &roots), Some("OneDrive"));
        assert_eq!(service_in(&home.join("Work/Thoughts"), &roots), None);
        assert!(!roots.iter().any(|(r, _)| r.ends_with("gone")), "a folder that is not there is not a sync root");
        // A damaged record is ignored, not fatal.
        std::fs::write(home.join(".dropbox/info.json"), "{not json").unwrap();
        assert_eq!(service_in(&home.join("Work/Shared Dropbox/Thoughts"), &sync_roots(&home, &|_| None)), None);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn folder_names_are_matched_whole() {
        for (name, service) in [("Dropbox", Some("Dropbox")), ("Dropbox (Personal)", Some("Dropbox")), ("OneDrive", Some("OneDrive")), ("OneDrive - Contoso", Some("OneDrive")),
                                ("Google Drive", Some("Google Drive")), ("iCloudDrive", Some("iCloud Drive")), ("Box", Some("Box")), ("Nextcloud", Some("Nextcloud")),
                                ("Dropbox-export", None), ("My Dropbox notes", None), ("iCloud", None), ("Boxes", None), ("Documents", None), ("OneDriveBackup", None)] {
            assert_eq!(home_folder_service(name), service, "{name}");
        }
        assert_eq!(cloud_storage_service("Dropbox-Personal"), "Dropbox");
        assert_eq!(cloud_storage_service("SomethingNew-Account"), "", "an unknown provider is still a cloud drive");
    }
}
