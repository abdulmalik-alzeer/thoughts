//! Moving Thoughts to another computer (Settings → Maintenance): one file with the chats (and
//! their attachments and unwrapped summaries), the settings, the customisations and what add-ons
//! saved (their records exist nowhere else). The library travels on its own, through sync or a
//! copy of its folder.
//!
//! The file never holds a secret: no provider key, no sync password, no daemon token (those live
//! in the keychain or in files that are not read here), and the settings say "no key saved".
//!
//! Reading one is treated like any file from outside: every part is checked before anything is
//! written; chat ids and attachment names must be plain names, so nothing can land outside the
//! data folder; chats already on this computer are only replaced when the user says so;
//! customisations are staged for the usual review instead of being switched on; and nothing that
//! is a security decision on the other computer (trusted folders, approved automatic tasks,
//! "bypass permissions") comes along.

use crate::chats::{Chat, RunOptions};
use crate::settings::{Project, Settings};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "thoughts-move";
pub const VERSION: u64 = 1;
pub const MAX_CHATS: usize = 20_000;
/// Attachments larger than this in total stay behind (their chats still travel).
pub const MAX_ATTACHMENT_BYTES: usize = 300 * 1024 * 1024;
/// The largest file `import` accepts over the API.
pub const MAX_BODY: usize = 600 * 1024 * 1024;

/// A plain chat id: letters, digits and dashes (what the app creates).
pub fn valid_id(id: &str) -> bool { !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') }

/// An attachment name that is a plain file name and stays one when cleaned.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 120 && !name.contains(['/', '\\']) && crate::library::sanitize_file_name(name) == name
}

/// The attachment `path` of chat `id` under `attachments`, when it really is one (a file directly
/// inside that chat's folder).
fn own_attachment(attachments: &Path, id: &str, path: &str) -> Option<(String, PathBuf)> {
    let p = PathBuf::from(path);
    let name = p.file_name()?.to_string_lossy().to_string();
    let expected = attachments.join(id).join(&name);
    if !valid_name(&name) || crate::paths::canonical_lenient(&p) != crate::paths::canonical_lenient(&expected) { return None; }
    std::fs::symlink_metadata(&expected).ok().filter(|m| m.is_file())?;
    Some((name, expected))
}

/// The settings as they travel: everything except the key flag, which is false (the key itself
/// never leaves the keychain), and only the open library's: the other libraries in the switcher,
/// their folders and their own settings stay on this computer.
pub fn settings_for_export(s: &Settings) -> Value {
    let mut out = s.clone();
    out.ai.claude.has_api_token = false;
    out.libraries.retain(|l| s.is_open_library(Path::new(&l.path)));
    for l in &mut out.libraries { l.parked = None; }
    serde_json::to_value(out).unwrap_or(Value::Null)
}

/// What an export holds, for the confirmation.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported { pub chats: usize, pub attachments: usize, pub attachments_left: usize, pub customisations: usize, pub addon_data: usize }

/// At most this many add-ons' saved data travel in one file (each at most the storage limit).
pub const MAX_ADDON_DATA: usize = 500;

/// Add what add-ons saved on this computer (`home/plugin-data`) to an export.
pub fn add_addon_data(v: &mut Value, info: &mut Exported, home: &Path) {
    let data = crate::layer_api::storage_all(home);
    let data: BTreeMap<String, Value> = data.into_iter().take(MAX_ADDON_DATA).collect();
    info.addon_data = data.len();
    v["addonData"] = json!(data);
}

/// Check the add-on data of a move file: plain add-on names, each a JSON object within the storage limit.
fn read_addon_data(v: &Value) -> Option<BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    match v {
        Value::Null => {}
        Value::Object(m) if m.len() <= MAX_ADDON_DATA => for (name, data) in m {
            let size = serde_json::to_string(data).ok()?.len();
            if !crate::layer_api::valid_plugin_name(name) || !data.is_object() || size > crate::layer_api::STORAGE_MAX_BYTES { return None; }
            out.insert(name.clone(), data.clone());
        },
        _ => return None,
    }
    Some(out)
}

/// Build the file. Only ordinary chats travel (Wrap, setup and customise runs belong to this
/// computer's history), and only the open library's: another library's chats, like its folder and
/// settings, stay here. `layer` is the customisation layer.
pub fn export(settings: &Settings, chats: Vec<Chat>, attachments_dir: &Path, layer: &Path) -> (Value, Exported) {
    let mut info = Exported::default();
    let mut files: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut total = 0usize;
    let mut out_chats = vec![];
    let open = crate::wraps::Scope::of(settings.library());
    for mut c in chats.into_iter().filter(|c| c.kind == "chat" && valid_id(&c.id) && open.holds(c.library.as_deref())) {
        // Which folder it belonged to here means nothing on the other computer: it joins the library open there.
        c.library = None;
        c.running = false;
        c.interrupted = false;
        // The assistant's own session lives on this computer only; the next message replays the chat instead.
        c.session_id = None;
        for m in &mut c.messages {
            let mut kept = vec![];
            for a in &m.attachments {
                let Some((name, path)) = own_attachment(attachments_dir, &c.id, a) else { continue };
                if files.get(&c.id).map_or(false, |f| f.contains_key(&name)) { kept.push(name); continue; }
                match std::fs::read(&path) {
                    Ok(bytes) if total + bytes.len() <= MAX_ATTACHMENT_BYTES => {
                        total += bytes.len();
                        files.entry(c.id.clone()).or_default().insert(name.clone(), base64::engine::general_purpose::STANDARD.encode(bytes));
                        info.attachments += 1;
                        kept.push(name);
                    }
                    _ => info.attachments_left += 1,
                }
            }
            // In the file, an attachment is named by its file name only.
            m.attachments = kept;
        }
        out_chats.push(c);
    }
    info.chats = out_chats.len();
    let pack = crate::packs::export(layer, "Moved from another computer");
    let pack = if pack["files"].as_object().map_or(false, |f| !f.is_empty()) {
        info.customisations = pack["files"].as_object().map_or(0, |f| f.len());
        pack
    } else { Value::Null };
    (json!({
        "format": FORMAT, "version": VERSION, "appVersion": env!("CARGO_PKG_VERSION"),
        "createdAt": chrono::Utc::now(), "platform": std::env::consts::OS,
        "settings": settings_for_export(settings),
        "chats": out_chats,
        "attachments": files,
        "customisations": pack,
    }), info)
}

/// A checked file, ready to import.
#[derive(Debug)]
pub struct Bundle {
    pub settings: Option<Settings>,
    pub chats: Vec<Chat>,
    /// chat id -> file name -> bytes
    pub attachments: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    pub customisations: Option<Value>,
    /// add-on name -> what it saved
    pub addon_data: BTreeMap<String, Value>,
    pub app_version: String,
    pub created_at: Option<String>,
}

/// Check every part of a file before anything is written. A file with any part that is not what
/// the app writes is refused whole (`MOVE_BAD::<what>`).
pub fn read(v: &Value) -> anyhow::Result<Bundle> {
    let bad = |what: &str| anyhow::anyhow!("MOVE_BAD::{what}");
    if v["format"] != FORMAT { return Err(bad("format")); }
    match v["version"].as_u64() { Some(VERSION) => {}, Some(n) if n > VERSION => return Err(bad("newer")), _ => return Err(bad("format")) }
    let settings = match &v["settings"] {
        Value::Null => None,
        s if s.is_object() => Some(serde_json::from_value::<Settings>(crate::settings::migrate(s.clone())).map_err(|_| bad("settings"))?),
        _ => return Err(bad("settings")),
    };
    let raw_chats = match &v["chats"] { Value::Null => &[][..], Value::Array(a) => a.as_slice(), _ => return Err(bad("chats")) };
    if raw_chats.len() > MAX_CHATS { return Err(bad("chats")); }
    let mut chats = vec![];
    let mut seen = BTreeSet::new();
    for c in raw_chats {
        let mut c: Chat = serde_json::from_value(c.clone()).map_err(|_| bad("chats"))?;
        // Ids are made in lower case. One that differs from another only in letter case would name
        // the same files on a computer whose disk ignores case: it is the same id (two in one file are refused).
        c.id = c.id.to_ascii_lowercase();
        if !valid_id(&c.id) || !seen.insert(c.id.clone()) { return Err(bad("chats")); }
        if c.kind != "chat" { continue; }
        for m in &c.messages {
            if m.attachments.iter().any(|a| !valid_name(a)) { return Err(bad("attachments")); }
        }
        c.options = safe_options(c.options);
        chats.push(c);
    }
    let mut attachments = BTreeMap::new();
    let mut total = 0usize;
    match &v["attachments"] {
        Value::Null => {}
        Value::Object(per_chat) => {
            for (id, files) in per_chat {
                let id = &id.to_ascii_lowercase();
                if !valid_id(id) || !seen.contains(id) || attachments.contains_key(id) { return Err(bad("attachments")); }
                let files = files.as_object().ok_or_else(|| bad("attachments"))?;
                let mut out = BTreeMap::new();
                for (name, data) in files {
                    if !valid_name(name) { return Err(bad("attachments")); }
                    let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_str().ok_or_else(|| bad("attachments"))?).map_err(|_| bad("attachments"))?;
                    total += bytes.len();
                    if total > MAX_ATTACHMENT_BYTES { return Err(bad("attachments")); }
                    out.insert(name.clone(), bytes);
                }
                attachments.insert(id.clone(), out);
            }
        }
        _ => return Err(bad("attachments")),
    }
    let customisations = match &v["customisations"] {
        Value::Null => None,
        p => { crate::packs::check_pack(p).map_err(|_| bad("customisations"))?; Some(p.clone()) }
    };
    let addon_data = read_addon_data(&v["addonData"]).ok_or_else(|| bad("addon-data"))?;
    Ok(Bundle {
        settings, chats, attachments, customisations, addon_data,
        app_version: v["appVersion"].as_str().unwrap_or("").chars().filter(|c| c.is_ascii_alphanumeric() || ".-+".contains(*c)).take(30).collect(),
        created_at: v["createdAt"].as_str().map(|s| s.chars().take(40).collect()),
    })
}

/// Run options from another computer: plain values only, and never "bypass permissions" (that
/// is only ever switched on here, after the warning).
pub fn safe_options(o: RunOptions) -> RunOptions {
    let ok = |what: &str, v: &Option<String>| crate::settings::check_cli_value(what, v.as_deref()).is_ok();
    RunOptions {
        model: o.model.clone().filter(|_| ok("model", &o.model)),
        effort: o.effort.clone().filter(|_| ok("effort", &o.effort)),
        permission_mode: o.permission_mode.filter(|m| m != crate::settings::BYPASS && crate::settings::check_permission_mode(Some(m)).is_ok()),
        // Web lookups are switched on here, after their warning, like bypass.
        web: false,
        max_budget_usd: None,
        // Decided when a message is sent (Settings), never carried over.
        prompt_suggestions: false,
        fork_session: false,
    }
}

/// This computer's settings with the personal choices from `from`. What belongs to this computer
/// stays: the library and its sync, linked folders (mapped separately), the assistant program,
/// start at login, enabled features (they belong to the library). Private folders are added to,
/// never removed. The provider address only comes along when no key is saved here (a key saved
/// for one address must never be sent to another). "Bypass permissions" as the default is never
/// taken over.
pub fn merged_settings(cur: &Settings, from: &Settings) -> Settings {
    let mut n = cur.clone();
    n.user_name = from.user_name.clone();
    n.language = from.language.clone();
    n.theme = from.theme.clone();
    n.mode = from.mode.clone();
    n.notifications = from.notifications;
    n.idle_minutes = from.idle_minutes;
    n.wrap_nag_threshold = from.wrap_nag_threshold;
    n.usage_signals = from.usage_signals;
    n.profession = from.profession.clone();
    n.accessibility = from.accessibility.clone();
    n.release_check = from.release_check;
    n.assistant_reports = from.assistant_reports;
    n.auto_summaries = from.auto_summaries;
    if from.show_cost.is_some() { n.show_cost = from.show_cost; }
    n.calendar = from.calendar.clone();
    n.daemon.keep_running = from.daemon.keep_running;
    let (c, f) = (&mut n.ai.claude, &from.ai.claude);
    c.model = f.model.clone();
    c.effort = f.effort.clone();
    c.mini_wrap_model = f.mini_wrap_model.clone();
    c.suggest_next = f.suggest_next;
    if f.permission_mode.as_deref() != Some(crate::settings::BYPASS) { c.permission_mode = f.permission_mode.clone(); }
    if !cur.ai.claude.has_api_token { c.base_url = f.base_url.clone(); }
    for p in &from.private_paths { if !n.private_paths.contains(p) { n.private_paths.push(p.clone()); } }
    n.last_wrap = match (cur.last_wrap, from.last_wrap) { (Some(a), Some(b)) => Some(a.max(b)), (a, b) => a.or(b) };
    n
}

/// A linked folder named in the file, and whether the same folder exists on this computer.
pub fn projects_in(b: &Bundle) -> Vec<Project> {
    let used: BTreeSet<&str> = b.chats.iter().filter_map(|c| c.project_id.as_deref()).collect();
    b.settings.as_ref().map(|s| s.projects.iter().filter(|p| used.contains(p.id.as_str()) && valid_id(&p.id)).cloned().collect()).unwrap_or_default()
}

/// Rewrite a chat for this computer: its linked folder mapped (`None` = the library), attachments
/// pointing at their new place, the run state cleared.
pub fn localised(mut c: Chat, project: Option<String>, attachments_dir: &Path, present: &BTreeSet<String>) -> Chat {
    c.project_id = project;
    c.running = false;
    c.interrupted = false;
    c.session_id = None;
    c.options = safe_options(c.options);
    let dir = attachments_dir.join(&c.id);
    for m in &mut c.messages {
        m.attachments = m.attachments.iter().filter(|n| present.contains(*n)).map(|n| dir.join(n).to_string_lossy().to_string()).collect();
    }
    c
}

/// Write one chat's attachments into `attachments_dir/<id>/`, privately. Returns the names written.
pub fn write_attachments(attachments_dir: &Path, id: &str, files: &BTreeMap<String, Vec<u8>>) -> anyhow::Result<BTreeSet<String>> {
    if !valid_id(id) { anyhow::bail!("MOVE_BAD::attachments"); }
    let dir = attachments_dir.join(id);
    if !files.is_empty() { crate::paths::create_private_dir(&dir)?; }
    let mut out = BTreeSet::new();
    for (name, bytes) in files {
        if !valid_name(name) { anyhow::bail!("MOVE_BAD::attachments"); }
        crate::paths::write_atomic(&dir.join(name), bytes, true)?;
        out.insert(name.clone());
    }
    Ok(out)
}

/// One chat's imported attachments, written beside where they go and not swapped in yet.
pub struct Staged { dir: Option<PathBuf>, names: BTreeSet<String> }

/// Write one chat's attachments into a new private folder of their own beside `attachments_dir/<id>`
/// (whatever is there is not touched). A write that fails (a full disk) removes what it wrote.
pub fn stage_attachments(attachments_dir: &Path, id: &str, files: &BTreeMap<String, Vec<u8>>) -> anyhow::Result<Staged> {
    if !valid_id(id) || files.keys().any(|n| !valid_name(n)) { anyhow::bail!("MOVE_BAD::attachments"); }
    if files.is_empty() { return Ok(Staged { dir: None, names: BTreeSet::new() }); }
    // Not a chat id (it starts with a dot): nothing takes it for a chat's folder.
    let dir = attachments_dir.join(format!(".import-{id}-{}", uuid::Uuid::new_v4().simple()));
    let written = (|| -> anyhow::Result<()> {
        crate::paths::create_private_dir(&dir)?;
        for (name, bytes) in files { crate::paths::write_atomic(&dir.join(name), bytes, true)?; }
        Ok(())
    })();
    if let Err(e) = written { let _ = std::fs::remove_dir_all(&dir); return Err(e); }
    Ok(Staged { dir: Some(dir), names: files.keys().cloned().collect() })
}

/// Put staged attachments in place as `attachments_dir/<id>`. What is there already is removed
/// only when `replacing` (the person chose to replace the chat of that very id here); otherwise
/// anything there — another chat's files, under the same name in other letter case on a disk that
/// ignores case — is never touched, and this fails. Returns the names now there.
pub fn place_attachments(attachments_dir: &Path, id: &str, staged: Staged, replacing: bool) -> anyhow::Result<BTreeSet<String>> {
    if !valid_id(id) { anyhow::bail!("MOVE_BAD::attachments"); }
    let target = attachments_dir.join(id);
    match std::fs::symlink_metadata(&target) {
        // Only a real folder is emptied; a link placed there goes, never what it points at.
        Ok(m) if replacing && m.is_dir() => std::fs::remove_dir_all(&target)?,
        Ok(_) if replacing => std::fs::remove_file(&target)?,
        Ok(_) => {
            if let Some(dir) = staged.dir { let _ = std::fs::remove_dir_all(dir); }
            anyhow::bail!("MOVE_BAD::attachments");
        }
        Err(_) => {}
    }
    if let Some(dir) = staged.dir { std::fs::rename(&dir, &target)?; }
    Ok(staged.names)
}

/// Where an export is saved: the Downloads folder (the home folder when there is none), under a
/// name that is not taken yet. The file is created here, readable by its owner only.
pub fn save_export(dir: &Path, text: &[u8]) -> anyhow::Result<PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let day = chrono::Local::now().format("%Y-%m-%d");
    for i in 1..=200 {
        let name = if i == 1 { format!("Thoughts move {day}.json") } else { format!("Thoughts move {day} ({i}).json") };
        let path = dir.join(name);
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
        match o.open(&path) {
            Ok(mut f) => { f.write_all(text)?; f.sync_all()?; return Ok(path); }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::bail!("could not find a free file name in {}", dir.display())
}

pub fn export_dir() -> PathBuf {
    dirs::download_dir().filter(|d| d.is_dir()).or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chats::Msg;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-move-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn chat_with_attachment(att: &Path) -> Chat {
        let mut c = Chat { title: "Budget".into(), session_id: Some("sess-1".into()), running: true, project_id: Some("p1".into()), ..Default::default() };
        c.options.permission_mode = Some("bypassPermissions".into());
        let dir = att.join(&c.id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("scan.png"), b"\x89PNG").unwrap();
        let mut m = Msg::new("user", "see attached");
        m.attachments = vec![dir.join("scan.png").to_string_lossy().into(), "/etc/passwd".into()];
        c.messages.push(m);
        c.mini_wrap = Some("unwrapped summary".into());
        c
    }

    #[test]
    fn a_move_file_holds_only_the_open_library_its_folder_settings_and_chats() {
        let d = tmp();
        let (work, personal) = ("/Users/sara/Work Library", "/Users/sara/Diary Library");
        let mut s = Settings::default();
        s.library_path = Some(work.into());
        s.private_paths = vec!["clients/secret".into()];
        let parked = crate::settings::LibrarySettings { private_paths: vec!["therapy".into()], projects: vec![Project { id: "p9".into(), name: "Diary".into(), path: "/Users/sara/Diary Folder".into() }], ..Default::default() };
        s.libraries = vec![
            crate::settings::Library { id: "w".into(), name: "Work".into(), path: work.into(), parked: None },
            crate::settings::Library { id: "p".into(), name: "Personal".into(), path: personal.into(), parked: Some(parked) },
        ];
        let mut mine = Chat { title: "Pricing".into(), ..Default::default() };
        mine.library = Some(work.into());
        let mut theirs = Chat { title: "Dear diary".into(), ..Default::default() };
        theirs.library = Some(personal.into());
        let (v, info) = export(&s, vec![mine.clone(), theirs], &d.join("attachments"), &d.join("app-layer"));
        assert_eq!(info.chats, 1);
        assert_eq!(v["chats"][0]["id"], mine.id.as_str());
        assert!(v["chats"][0]["library"].is_null(), "it joins the library open on the other computer");
        let libs = v["settings"]["libraries"].as_array().unwrap();
        assert_eq!((libs.len(), libs[0]["id"].as_str()), (1, Some("w")));
        assert_eq!(v["settings"]["privatePaths"], json!(["clients/secret"]));
        let text = v.to_string();
        for other in ["Diary Library", "Diary Folder", "therapy", "Dear diary", "Personal"] { assert!(!text.contains(other), "{other} left this computer"); }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn an_export_carries_chats_settings_and_customisations_but_no_secret() {
        let d = tmp();
        let att = d.join("attachments");
        let layer = d.join("app-layer");
        std::fs::create_dir_all(&layer).unwrap();
        std::fs::write(layer.join("custom.css"), "body{}").unwrap();
        let mut s = Settings::default();
        s.ai.claude.has_api_token = true;
        s.user_name = "Sara".into();
        s.projects.push(Project { id: "p1".into(), name: "Client".into(), path: "/Users/sara/Client".into() });
        let wrap = Chat { kind: "wrap".into(), ..Default::default() };
        let (v, info) = export(&s, vec![chat_with_attachment(&att), wrap], &att, &layer);
        assert_eq!((info.chats, info.attachments, info.attachments_left, info.customisations), (1, 1, 0, 1));
        assert_eq!(v["settings"]["ai"]["claude"]["hasApiToken"], false);
        assert_eq!(v["settings"]["userName"], "Sara");
        let c = &v["chats"][0];
        assert!(c["sessionId"].is_null() && c["running"] == false && c["miniWrap"] == "unwrapped summary");
        assert_eq!(c["messages"][0]["attachments"], json!(["scan.png"]), "names only, and only the chat's own files");
        let text = v.to_string();
        for secret in ["sk-", "token\":\"", "password", "test-token"] { assert!(!text.contains(secret), "{secret}"); }
        // It reads back whole.
        let b = read(&v).unwrap();
        assert_eq!(b.chats.len(), 1);
        assert_eq!(b.attachments[&b.chats[0].id]["scan.png"], b"\x89PNG");
        assert_eq!(b.chats[0].options.permission_mode, None, "bypass never travels");
        assert!(b.customisations.is_some() && b.settings.unwrap().user_name == "Sara");
        // And is saved without overwriting anything.
        let out = d.join("Downloads");
        let a = save_export(&out, b"1").unwrap();
        let b2 = save_export(&out, b"2").unwrap();
        assert_ne!(a, b2);
        assert_eq!(std::fs::read(&a).unwrap(), b"1");
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o600); }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_file_that_is_not_what_the_app_writes_is_refused() {
        let ok = || json!({"format": FORMAT, "version": 1, "chats": [{"id": "abc-1", "kind": "chat", "messages": []}], "attachments": {}});
        assert!(read(&ok()).is_ok());
        let code = |v: Value| read(&v).unwrap_err().to_string();
        let with = |f: &dyn Fn(&mut Value)| { let mut v = ok(); f(&mut v); code(v) };
        assert_eq!(with(&|v| v["format"] = json!("thoughts-pack")), "MOVE_BAD::format");
        assert_eq!(with(&|v| v["version"] = json!(2)), "MOVE_BAD::newer");
        // Ids and names that would reach outside the data folder.
        for id in ["../../etc", "a/b", "", "..", "x y"] {
            assert_eq!(with(&|v| v["chats"][0]["id"] = json!(id)), "MOVE_BAD::chats", "{id}");
        }
        assert_eq!(with(&|v| v["chats"] = json!([{"id": "a"}, {"id": "a"}])), "MOVE_BAD::chats");
        for name in ["../x.png", "a/b.png", ".hidden", "C:\\x.png", "con.txt\u{0}"] {
            assert_eq!(with(&|v| v["attachments"] = json!({"abc-1": {name: "AAAA"}})), "MOVE_BAD::attachments", "{name}");
            assert_eq!(with(&|v| v["chats"][0]["messages"] = json!([{"id": "m", "role": "user", "text": "", "ts": "2026-01-01T00:00:00Z", "attachments": [name]}])), "MOVE_BAD::attachments", "{name}");
        }
        assert_eq!(with(&|v| v["attachments"] = json!({"other-chat": {"a.png": "AAAA"}})), "MOVE_BAD::attachments");
        assert_eq!(with(&|v| v["attachments"] = json!({"abc-1": {"a.png": "not base64!"}})), "MOVE_BAD::attachments");
        assert_eq!(with(&|v| v["customisations"] = json!({"format": "thoughts-pack", "version": 1, "files": {"../../.zshrc": "x"}})), "MOVE_BAD::customisations");
        assert_eq!(with(&|v| v["settings"] = json!({"idleMinutes": "soon"})), "MOVE_BAD::settings");
        assert_eq!(code(json!("nonsense")), "MOVE_BAD::format");
        // Other kinds of chat are left out quietly.
        let mut v = ok();
        v["chats"][0]["kind"] = json!("customize");
        assert!(read(&v).unwrap().chats.is_empty());
    }

    #[test]
    fn what_add_ons_saved_travels_and_is_checked_like_the_rest() {
        let d = tmp();
        crate::layer_api::storage_write(&d, "hours.js", br#"{"entries":[{"date":"2026-11-10","client":"Acme","hours":2.5}]}"#, false).unwrap();
        crate::layer_api::storage_write(&d, "empty.js", b"{}", false).unwrap();
        let (mut v, mut info) = export(&Settings::default(), vec![], &d.join("attachments"), &d.join("app-layer"));
        add_addon_data(&mut v, &mut info, &d);
        assert_eq!(info.addon_data, 1, "nothing saved, nothing to carry");
        assert_eq!(v["addonData"]["hours.js"]["entries"][0]["client"], "Acme");
        let b = read(&v).unwrap();
        assert_eq!(b.addon_data.keys().collect::<Vec<_>>(), ["hours.js"]);
        // Names that would reach outside the data folder, and anything but a JSON object of a fitting size.
        let with = |data: Value| { let mut x = v.clone(); x["addonData"] = data; read(&x).map(|_| ()).unwrap_err().to_string() };
        for bad in [json!({"../settings": {}}), json!({".versions": {}}), json!({"a/b.js": {}}), json!({"hours.js": [1]}), json!({"hours.js": "x"}),
                    json!({"hours.js": {"x": "a".repeat(crate::layer_api::STORAGE_MAX_BYTES)}}), json!([1])] {
            assert_eq!(with(bad.clone()), "MOVE_BAD::addon-data", "{bad}");
        }
        // An older file without any is fine.
        let mut old = v.clone();
        old.as_object_mut().unwrap().remove("addonData");
        assert!(read(&old).unwrap().addon_data.is_empty());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn settings_from_another_computer_keep_this_ones_machine_choices() {
        let mut cur = Settings::default();
        cur.library_path = Some("/here/library".into());
        cur.ai.claude.binary = Some("/here/claude".into());
        cur.ai.claude.has_api_token = true;
        cur.ai.claude.base_url = Some("https://here.example.com".into());
        cur.private_paths = vec!["me/finances".into()];
        cur.daemon.run_at_login = true;
        let mut from = Settings::default();
        from.language = "ar".into();
        from.user_name = "Sara".into();
        from.library_path = Some("/there/library".into());
        from.ai.claude.binary = Some("/there/claude".into());
        from.ai.claude.base_url = Some("https://there.example.com".into());
        from.ai.claude.permission_mode = Some("bypassPermissions".into());
        from.private_paths = vec!["clients/acme".into(), "me/finances".into()];
        from.daemon.run_at_login = false;
        from.sync.mode = "none".into();
        cur.show_cost = Some(false);
        assert_eq!(merged_settings(&cur, &from).show_cost, Some(false), "a file that never had the cost choice leaves this one's");
        from.show_cost = Some(true);
        let n = merged_settings(&cur, &from);
        assert_eq!(n.show_cost, Some(true), "the cost choice travels");
        assert_eq!((n.language.as_str(), n.user_name.as_str()), ("ar", "Sara"));
        assert_eq!(n.library_path.as_deref(), Some("/here/library"));
        assert_eq!(n.ai.claude.binary.as_deref(), Some("/here/claude"));
        assert_eq!(n.ai.claude.base_url.as_deref(), Some("https://here.example.com"), "a key is saved for this address");
        assert_eq!(n.ai.claude.permission_mode, cur.ai.claude.permission_mode);
        assert_eq!(n.private_paths, ["me/finances", "clients/acme"]);
        assert!(n.daemon.run_at_login && n.sync.mode == cur.sync.mode && n.ai.claude.has_api_token);
        cur.ai.claude.has_api_token = false;
        assert_eq!(merged_settings(&cur, &from).ai.claude.base_url.as_deref(), Some("https://there.example.com"));
    }

    #[test]
    fn staged_attachments_replace_a_folder_only_when_its_chat_is_replaced() {
        let d = tmp();
        let att = d.join("attachments");
        std::fs::create_dir_all(att.join("c-1")).unwrap();
        std::fs::write(att.join("c-1/keep.txt"), "here").unwrap();
        let mut files = BTreeMap::new();
        files.insert("new.txt".to_string(), b"in".to_vec());
        // Not replacing that chat: what is there is never removed, and nothing is left beside it.
        let staged = stage_attachments(&att, "c-1", &files).unwrap();
        assert!(place_attachments(&att, "c-1", staged, false).is_err());
        assert_eq!(std::fs::read_to_string(att.join("c-1/keep.txt")).unwrap(), "here");
        assert_eq!(std::fs::read_dir(&att).unwrap().count(), 1);
        // Replacing it: swapped whole.
        let staged = stage_attachments(&att, "c-1", &files).unwrap();
        assert_eq!(place_attachments(&att, "c-1", staged, true).unwrap(), BTreeSet::from(["new.txt".to_string()]));
        assert!(!att.join("c-1/keep.txt").exists() && att.join("c-1/new.txt").is_file());
        assert_eq!(std::fs::read_dir(&att).unwrap().count(), 1);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn an_imported_chat_is_made_local() {
        let d = tmp();
        let att = d.join("attachments");
        let c = chat_with_attachment(&d.join("old"));
        let id = c.id.clone();
        let mut files = BTreeMap::new();
        files.insert("scan.png".to_string(), b"img".to_vec());
        let present = write_attachments(&att, &id, &files).unwrap();
        let mut c2 = c.clone();
        c2.messages[0].attachments = vec!["scan.png".into(), "missing.png".into()];
        let l = localised(c2, None, &att, &present);
        assert_eq!(l.messages[0].attachments, [att.join(&id).join("scan.png").to_string_lossy().to_string()]);
        assert!(l.session_id.is_none() && !l.running && l.project_id.is_none() && l.options.permission_mode.is_none());
        assert_eq!(std::fs::read(att.join(&id).join("scan.png")).unwrap(), b"img");
        assert!(write_attachments(&att, "../x", &files).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }
}
