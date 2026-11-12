use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Version of this file's shape, for future migrations. Server-owned.
    pub schema_version: u32,
    pub onboarded: bool,
    pub user_name: String,
    /// "en" | "ar"
    pub language: String,
    /// "saudi" | "contrast" | "tech"
    pub theme: String,
    /// "system" | "light" | "dark"
    pub mode: String,
    pub library_path: Option<String>,
    pub ai: AiSettings,
    pub sync: SyncSettings,
    /// Keep a history of changes to the library on this computer, so any change can be undone.
    /// On unless the user switches it off; it has nothing to do with where the library is backed up.
    pub keep_history: bool,
    /// module id -> enabled
    pub modules: BTreeMap<String, bool>,
    pub daemon: DaemonSettings,
    pub notifications: bool,
    /// Minutes without activity before a chat gets its mini-wrap.
    pub idle_minutes: u64,
    /// Popup when this many chats are unwrapped.
    pub wrap_nag_threshold: usize,
    /// Delete finished chats this many days after their last activity. 0 = never (the default).
    /// Changed only through `PUT /api/chats/retention` (see `tidy.rs`), never by a general save.
    pub chat_retention_days: u32,
    /// Summarise a chat in the background once it goes quiet or another one starts. Off = a chat's
    /// text is only sent for a summary when the user presses Wrap.
    pub auto_summaries: bool,
    pub projects: Vec<Project>,
    pub last_wrap: Option<chrono::DateTime<chrono::Utc>>,
    pub search: SearchSettings,
    /// Library-relative folders the AI must never read.
    pub private_paths: Vec<String>,
    /// Keep the local-only usage log that suggestions are drawn from. Off = nothing is recorded or suggested.
    pub usage_signals: bool,
    /// What the user does, in their own words (asked at setup). Shapes suggestions and wording.
    pub profession: Option<String>,
    pub accessibility: Accessibility,
    pub calendar: CalendarSettings,
    /// Ask once a week whether a newer version has been published (`release.rs`). Off unless the
    /// user switches it on: it is a request to the internet the app does not otherwise make.
    pub release_check: bool,
    /// The monthly spending guard (see `spend.rs`).
    pub spending: SpendingSettings,
    /// Quick capture from anywhere (the desktop app's keyboard shortcut).
    pub capture: CaptureSettings,
    /// Every library the user has added, the open one (`library_path`) among them. The open one's
    /// own settings are the top-level fields above; each other one's wait in its entry until it is
    /// opened again (see `LibrarySettings`). Server-owned: it changes only through the library
    /// switcher (`libraries.rs`), setup, and choosing another library folder in Settings.
    pub libraries: Vec<Library>,
}

/// What belongs to one library rather than to the app: its backup and history, its private
/// folders, its line of work, its features, its linked folders and when it was last wrapped. The
/// open library's are the top-level fields of `Settings`, where everything reads them.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct LibrarySettings {
    /// Everything but the password store, which belongs to the app (`Settings::sync` keeps it).
    pub sync: SyncSettings,
    pub keep_history: bool,
    pub private_paths: Vec<String>,
    pub profession: Option<String>,
    pub modules: BTreeMap<String, bool>,
    pub projects: Vec<Project>,
    pub last_wrap: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for LibrarySettings {
    fn default() -> Self {
        Self { sync: SyncSettings::default(), keep_history: true, private_paths: vec![], profession: None, modules: BTreeMap::new(), projects: vec![], last_wrap: None }
    }
}

impl LibrarySettings {
    /// What a library starts with when it is added: the kind of backup, the history choice and the
    /// line of work of the library open now (`from`); nothing that only made sense for that folder
    /// (its private folders, its main backup location, its linked folders, its last wrap).
    pub fn for_new(from: &Settings) -> Self {
        let mut sync = SyncSettings { mode: from.sync.mode.clone(), target: from.sync.target.clone(), ..SyncSettings::default() };
        sync.credential_store = from.sync.credential_store.clone();
        Self { sync, keep_history: from.keep_history, profession: from.profession.clone(), ..Self::default() }
    }
}

/// One library in the switcher.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Library {
    /// Stable for as long as the library is listed (see `library_id_for`).
    pub id: String,
    /// The name the user gave it in the switcher. Empty: the folder's own name.
    pub name: String,
    pub path: String,
    /// Its own settings while another library is open. `None` for the open one (the top-level
    /// fields are its settings), and for one never opened yet (it starts from `for_new`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parked: Option<LibrarySettings>,
}

impl Library {
    /// The name to show: the one the user gave it, else the folder's own.
    pub fn display_name(&self) -> String {
        if !self.name.trim().is_empty() { return self.name.trim().to_string(); }
        std::path::Path::new(&self.path).file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| self.path.clone())
    }
}

/// The id a library gets when it is first listed: derived from its folder, so an install that was
/// never saved again since the upgrade sees the same id on every start.
pub fn library_id_for(path: &str) -> String {
    use sha2::{Digest, Sha256};
    let h = format!("{:x}", Sha256::digest(path.as_bytes()));
    format!("lib-{}", &h[..12])
}

/// Are `a` and `b` the same folder (another spelling, a trailing slash, a link, other capitals)?
pub fn same_folder(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b || crate::paths::canonical_lenient(a) == crate::paths::canonical_lenient(b)
}

/// The longest name the user can give a library.
pub const LIBRARY_NAME_MAX: usize = 60;

/// A monthly spending limit the user sets, in US dollars. `None` = no limit. At `warn_percent` of it
/// the app says so; at the limit it stops starting things nobody is watching.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SpendingSettings {
    pub monthly_limit_usd: Option<f64>,
    pub warn_percent: u8,
}

impl Default for SpendingSettings {
    fn default() -> Self { Self { monthly_limit_usd: None, warn_percent: 80 } }
}

impl SpendingSettings {
    pub const MIN_LIMIT: f64 = 1.0;
    pub const MAX_LIMIT: f64 = 100_000.0;
    /// Refuse an amount or warning point that makes no sense; round the amount to whole cents.
    pub fn sanitize(&mut self) -> anyhow::Result<()> {
        if let Some(l) = self.monthly_limit_usd {
            if !l.is_finite() || !(Self::MIN_LIMIT..=Self::MAX_LIMIT).contains(&l) { return Err(crate::tr::err("spend.limit_invalid", &[])); }
            self.monthly_limit_usd = Some((l * 100.0).round() / 100.0);
        }
        if !(50..=95).contains(&self.warn_percent) { return Err(crate::tr::err("spend.limit_invalid", &[])); }
        Ok(())
    }
}

/// The keyboard shortcut that opens the quick capture box from any app. Only the desktop app reads
/// it (it registers the shortcut with the system); `shortcut` is the key combination as the desktop
/// app writes it ("Control+Alt+Space"), empty for the app's own default on this computer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CaptureSettings {
    pub shortcut_on: bool,
    pub shortcut: String,
}

impl Default for CaptureSettings {
    fn default() -> Self { Self { shortcut_on: true, shortcut: String::new() } }
}

impl CaptureSettings {
    /// Key names joined by `+`, nothing else (the desktop app checks the combination itself).
    fn sanitize(&mut self) {
        let ok = self.shortcut.len() <= 80 && self.shortcut.chars().all(|c| c.is_ascii_alphanumeric() || c == '+');
        if !ok { self.shortcut.clear(); }
    }
}

/// Calendar preferences: the first day of the week and the weekend (`None` = from the library's
/// `Week:` line, else the region), Hijri dates beside Gregorian ones, and the time of the reminder
/// in exported calendar events (`None` = no reminder). Days are counted from Sunday = 0.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CalendarSettings {
    pub week_start: Option<u8>,
    pub weekend: Option<Vec<u8>>,
    /// "gregorian" | "both"
    pub dates: String,
    /// "HH:MM"
    pub alarm: Option<String>,
}

impl Default for CalendarSettings {
    fn default() -> Self { Self { week_start: None, weekend: None, dates: "gregorian".into(), alarm: Some("09:00".into()) } }
}

impl CalendarSettings {
    /// The reminder time as `(hour, minute)`, when there is a valid one.
    pub fn alarm_time(&self) -> Option<(u32, u32)> {
        let t = chrono::NaiveTime::parse_from_str(self.alarm.as_deref()?.trim(), "%H:%M").ok()?;
        use chrono::Timelike;
        Some((t.hour(), t.minute()))
    }
    fn sanitize(&mut self) {
        self.week_start = self.week_start.filter(|d| *d < 7);
        self.weekend = self.weekend.take().map(|w| { let mut w: Vec<u8> = w.into_iter().filter(|d| *d < 7).collect(); w.sort(); w.dedup(); w.truncate(6); w });
        if !["gregorian", "both"].contains(&self.dates.as_str()) { self.dates = "gregorian".into(); }
        if self.alarm.is_some() && self.alarm_time().is_none() { self.alarm = None; }
        if let Some((h, m)) = self.alarm_time() { self.alarm = Some(format!("{h:02}:{m:02}")); }
    }
}

/// How the window is shown, for people who need it larger, calmer or clearer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Accessibility {
    /// Size of everything in the window, in percent (`ZOOM_MIN`..=`ZOOM_MAX`).
    pub zoom: u16,
    /// Switch animations off even when the system does not ask for it.
    pub reduce_motion: bool,
    pub underline_links: bool,
    /// A thicker keyboard-focus outline.
    pub strong_focus: bool,
    /// More room between lines, words and letters.
    pub text_spacing: bool,
}

pub const ZOOM_MIN: u16 = 50;
pub const ZOOM_MAX: u16 = 200;

impl Default for Accessibility {
    fn default() -> Self { Self { zoom: 100, reduce_motion: false, underline_links: false, strong_focus: false, text_spacing: false } }
}

pub const SCHEMA_VERSION: u32 = 1;
/// The smart-search add-on address that earlier versions filled in by themselves. Nobody connected
/// to the app controls that account, so a saved copy of it is cleared: the address is only ever one
/// the user typed in.
const RETIRED_EXTENSION_REPO: &str = "https://github.com/thoughts-app/thoughts-search.git";
/// The only permission modes the app ever passes to the CLI, under the names Claude Code gives them.
/// `bypassPermissions` is special: it is only ever switched on after the user confirmed it in a
/// dialog (see `bypass_needs_confirm`), and only for ordinary chats.
pub const PERMISSION_MODES: &[&str] = &["acceptEdits", "plan", BYPASS];
/// Claude Code's "skip all permission checks" mode.
pub const BYPASS: &str = "bypassPermissions";
/// What a client sends along (`?confirm=…` on settings, `confirmBypass` on a chat) once the user said yes.
pub const BYPASS_CONFIRM: &str = BYPASS;

/// Does going from `old` to `new` switch "bypass permissions" on? That step needs the user's yes.
pub fn bypass_needs_confirm(old: Option<&str>, new: Option<&str>) -> bool {
    new == Some(BYPASS) && old != Some(BYPASS)
}

/// The mode used wherever none is stored or asked for: the assistant may edit notes.
pub const DEFAULT_PERMISSION_MODE: &str = "acceptEdits";

/// Allow-list for a chat/run permission mode. `None` / empty means "not said": the app's own
/// default (`DEFAULT_PERMISSION_MODE`), never Claude Code's. The old `default` ("ask before
/// changes") is still accepted from files and clients written before it was removed, and is always
/// treated as `plan` (see `effective_permission_mode`): the app has no way to answer the CLI's
/// permission questions, so asking only ever meant silently skipping the change.
pub fn check_permission_mode(mode: Option<&str>) -> anyhow::Result<()> {
    match mode {
        None | Some("") | Some("default") => Ok(()),
        Some(m) if PERMISSION_MODES.contains(&m) => Ok(()),
        Some(m) => anyhow::bail!("permission mode \"{m}\" is not allowed (use acceptEdits, plan or bypassPermissions)"),
    }
}

/// The mode actually used for a stored or requested one. It is always a real mode, because a run
/// started without one takes whatever the user's (or the folder's) own Claude Code settings say —
/// "bypass permissions" included. Nothing said becomes accept-edits; `default` becomes `plan`.
pub fn effective_permission_mode(mode: Option<&str>) -> &str {
    match mode {
        None | Some("") => DEFAULT_PERMISSION_MODE,
        Some("default") => "plan",
        Some(m) => m,
    }
}

fn normalize_permission_mode(mode: &mut Option<String>) {
    let real = effective_permission_mode(mode.as_deref()).to_string();
    if mode.as_deref() != Some(real.as_str()) { *mode = Some(real); }
}

/// Set once at startup when settings.json was written by a newer version of the app: from then on
/// nothing saves settings, so the newer file is never rewritten with fields this version drops.
static READ_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn set_read_only(on: bool) { READ_ONLY.store(on, std::sync::atomic::Ordering::Relaxed); }
pub fn read_only() -> bool { READ_ONLY.load(std::sync::atomic::Ordering::Relaxed) }

/// How many damaged settings files are kept beside settings.json.
pub const KEEP_CORRUPT: usize = 5;

/// The outcome of reading settings.json at startup.
pub struct Loaded {
    pub settings: Settings,
    /// The file was damaged, no usable backup existed, and the app started from defaults.
    pub reset: bool,
    /// The file was damaged or missing and the backup copy was put back.
    pub restored: bool,
    /// The file comes from a newer version of the app: use it, never rewrite it.
    pub newer: bool,
}

/// A value handed to the CLI as a flag argument (model, effort, session id): plain token, never a flag.
pub fn check_cli_value(what: &str, v: Option<&str>) -> anyhow::Result<()> {
    let Some(v) = v.filter(|v| !v.is_empty()) else { return Ok(()) };
    let ok = v.len() <= 120 && !v.starts_with('-')
        && v.chars().all(|c| c.is_ascii_alphanumeric() || "._-:/@[]".contains(c));
    if !ok { anyhow::bail!("invalid {what}"); }
    Ok(())
}

/// `https://…` anywhere, or plain `http://` only for this machine (localhost / 127.0.0.1).
pub fn check_base_url(url: &str) -> anyhow::Result<()> { base_url_host(url).map(|_| ()) }

/// The host of a provider address that passes validation. The refusals are worded for the person
/// (`tr.rs`), in both languages.
fn base_url_host(url: &str) -> anyhow::Result<&str> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) { return Err(crate::tr::err("provider.url_blank", &[])); }
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") { (true, r) }
        else if let Some(r) = url.strip_prefix("http://") { (false, r) }
        else { return Err(crate::tr::err("provider.url_scheme", &[])) };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') { return Err(crate::tr::err("provider.url_login", &[])); }
    let host = authority.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map_or(authority, |(h, _)| h);
    if host.is_empty() { return Err(crate::tr::err("provider.url_host", &[])); }
    if !https && host != "localhost" && host != "127.0.0.1" { return Err(crate::tr::err("provider.url_http", &[])); }
    Ok(host)
}

/// Is this Anthropic's own API address (with or without a path)? Naming it is the same as naming
/// no address at all: requests go where Claude Code sends them anyway.
pub fn is_anthropic_url(url: &str) -> bool {
    url.starts_with("https://") && base_url_host(url).map_or(false, |h| h.eq_ignore_ascii_case("api.anthropic.com"))
}

fn none_if_blank(v: &mut Option<String>) {
    if v.as_deref().map_or(false, |s| s.trim().is_empty()) { *v = None; }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct AiSettings {
    /// Provider used by default: "claude" (codex lands in milestone 2).
    pub primary: String,
    pub secondary: Option<String>,
    pub claude: ClaudeSettings,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ClaudeSettings {
    /// Custom path to the `claude` binary; None = search PATH.
    pub binary: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
    /// Model used for cheap mini-wraps.
    pub mini_wrap_model: Option<String>,
    /// Address of an Anthropic-compatible service; None = Anthropic itself. Server-owned, like
    /// `has_api_token`: the two change together, through `claude::set_provider` only.
    pub base_url: Option<String>,
    /// A key for that address is saved in the password store (never in this file).
    pub has_api_token: bool,
    /// No suggested next message under an answer. (Off by default: the suggestions are on.)
    pub suggestions_off: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct SyncSettings {
    /// Where the library is backed up: "none" (this computer only), "folder" (the library sits in
    /// a cloud folder that keeps the copy), or an online location the app sends to — "git-manual"
    /// (when Sync is pressed) or "git-auto" (after every Wrap). History is a separate switch
    /// (`Settings::keep_history`).
    pub mode: String,
    /// "keychain" | "file"
    pub credential_store: String,
    /// The online location a sync receives changes from, by its short name, when the user picked
    /// one ("Make main"). Unset, or naming a location that is gone: the first one that was added.
    pub main_location: Option<String>,
    /// The user said to send the library to an online location although its folder is one a cloud
    /// drive also syncs ("use it anyway"). Without this that combination is refused.
    pub allow_cloud: bool,
    /// Which of the two backups the app sends to is chosen: "online" ("Online location") or
    /// "drive" ("Drive or folder"). Both are the "git-…" modes; this only decides how Settings
    /// shows the choice and what it offers to add. Every location, of either kind, is sent to.
    pub target: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct DaemonSettings {
    pub run_at_login: bool,
    /// Keep the daemon (and running chats) alive after the window closes.
    pub keep_running: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchSettings {
    pub extension_repo: String,
    pub file_threshold: usize,
    pub prompt_dismissed: bool,
    pub install_offered: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            onboarded: false,
            user_name: String::new(),
            language: "en".into(),
            theme: "saudi".into(),
            mode: "system".into(),
            library_path: None,
            ai: AiSettings::default(),
            sync: SyncSettings::default(),
            keep_history: true,
            modules: BTreeMap::new(),
            daemon: DaemonSettings { run_at_login: false, keep_running: true },
            notifications: true,
            idle_minutes: 15,
            wrap_nag_threshold: 5,
            chat_retention_days: 0,
            auto_summaries: true,
            projects: vec![],
            last_wrap: None,
            search: SearchSettings::default(),
            private_paths: vec![],
            usage_signals: true,
            profession: None,
            accessibility: Accessibility::default(),
            calendar: CalendarSettings::default(),
            release_check: false,
            spending: SpendingSettings::default(),
            capture: CaptureSettings::default(),
            libraries: vec![],
        }
    }
}

impl Default for AiSettings {
    fn default() -> Self {
        Self { primary: "claude".into(), secondary: None, claude: ClaudeSettings::default() }
    }
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self { mode: "git-manual".into(), credential_store: "keychain".into(), main_location: None, allow_cloud: false, target: "online".into() }
    }
}

/// One step per schema version: `MIGRATIONS[n]` turns a version-`n` file into version `n + 1`.
/// Steps work on the raw JSON so they can rename or reshape fields the current struct no longer has.
const MIGRATIONS: &[fn(&mut serde_json::Value)] = &[
    // v0 -> v1: files written before `schemaVersion` existed. Nothing to reshape.
    |_v| {},
];

/// Bring a settings document up to `SCHEMA_VERSION`. Files from a newer app are left as they are.
pub fn migrate(mut v: serde_json::Value) -> serde_json::Value {
    if !v.is_object() { return v; }
    let mut ver = v.get("schemaVersion").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
    while ver < MIGRATIONS.len() {
        MIGRATIONS[ver](&mut v);
        ver += 1;
        v["schemaVersion"] = serde_json::json!(ver);
    }
    v
}

fn parse(text: &str) -> anyhow::Result<Settings> {
    let v: serde_json::Value = serde_json::from_str(text)?;
    if !v.is_object() { anyhow::bail!("not a settings object"); }
    Ok(serde_json::from_value::<Settings>(migrate(v))?.with_defaults())
}

/// The `schemaVersion` a settings document claims, when it can be read at all.
fn version_of(text: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(text).ok().filter(|v| v.is_object())
        .map(|v| v.get("schemaVersion").and_then(|x| x.as_u64()).unwrap_or(0))
}

/// The backup written beside `file` after every successful save.
pub fn backup_file(file: &std::path::Path) -> std::path::PathBuf {
    file.with_file_name(format!("{}.bak", file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "settings.json".into())))
}

impl Settings {
    /// Read-only load (the app shell uses it too): a missing or unreadable file gives the backup,
    /// or defaults, and nothing on disk is touched. The daemon itself starts with `load_full`.
    pub fn load() -> Self {
        let file = crate::paths::settings_file();
        std::fs::read_to_string(&file).ok().and_then(|s| parse(&s).ok())
            .or_else(|| std::fs::read_to_string(backup_file(&file)).ok().and_then(|s| parse(&s).ok()))
            .unwrap_or_else(|| Settings::default().with_defaults())
    }

    /// Load `file`, returning the settings and whether the app had to fall back to defaults.
    pub fn load_or_recover(file: &std::path::Path) -> (Self, bool) {
        let l = Self::load_full(file);
        (l.settings, l.reset)
    }

    /// Load `file` for the daemon.
    /// - A file that exists but cannot be understood is never silently replaced: it is moved aside
    ///   to `settings.json.corrupt-<timestamp>` (the newest few are kept), and the backup written by
    ///   the last successful save is put back. Only when the backup is unusable too does the app
    ///   start from defaults (`reset`).
    /// - A file from a newer version of the app (`schemaVersion` above ours) is used as far as it
    ///   can be read and is never moved, migrated or rewritten (`newer`).
    pub fn load_full(file: &std::path::Path) -> Loaded {
        let bak = backup_file(file);
        let from_backup = || std::fs::read_to_string(&bak).ok().and_then(|t| parse(&t).ok());
        let defaults = || Settings::default().with_defaults();
        let text = match std::fs::read(file) {
            Ok(b) => String::from_utf8_lossy(&b).to_string(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // No settings file: a first run, unless a backup says otherwise (the file was lost).
                return match from_backup() {
                    Some(s) => {
                        crate::log::warn("settings.json was missing; restored it from the backup copy");
                        crate::log::ok_or_warn("could not put the settings backup back", s.write_to(file));
                        Loaded { settings: s, reset: false, restored: true, newer: false }
                    }
                    None => Loaded { settings: defaults(), reset: false, restored: false, newer: false },
                };
            }
            Err(e) => {
                // Unreadable (permissions, disk): keep the file where it is and say so.
                crate::log::error(format!("settings could not be read ({e}); using the backup copy or defaults"));
                return match from_backup() {
                    Some(s) => Loaded { settings: s, reset: false, restored: true, newer: false },
                    None => Loaded { settings: defaults(), reset: true, restored: false, newer: false },
                };
            }
        };
        let newer = version_of(&text).map_or(false, |v| v > SCHEMA_VERSION as u64);
        match parse(&text) {
            Ok(s) => Loaded { settings: s, reset: false, restored: false, newer },
            Err(e) if newer => {
                // Written by a newer app in a shape this one cannot read: leave it exactly as it is.
                crate::log::error(format!("settings were saved by a newer version and could not be read ({e}); they are left untouched"));
                let (settings, reset) = match from_backup() { Some(s) => (s, false), None => (defaults(), true) };
                Loaded { settings, reset, restored: false, newer: true }
            }
            Err(e) => {
                let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "settings.json".into());
                let aside = file.with_file_name(format!("{name}.corrupt-{}", chrono::Local::now().format("%Y%m%d-%H%M%S%3f")));
                match std::fs::rename(file, &aside) {
                    Ok(()) => crate::log::error(format!("settings file was damaged ({e}); kept as {}", aside.display())),
                    Err(e2) => crate::log::error(format!("settings file was damaged ({e}) and could not be moved aside ({e2})")),
                }
                if let Some(dir) = file.parent() { crate::paths::prune_files(dir, &format!("{name}.corrupt-"), KEEP_CORRUPT); }
                match from_backup() {
                    Some(s) => {
                        crate::log::warn("settings restored from the backup copy");
                        crate::log::ok_or_warn("could not put the settings backup back", s.write_to(file));
                        Loaded { settings: s, reset: false, restored: true, newer: false }
                    }
                    None => {
                        crate::log::error("no usable settings backup; started with defaults");
                        Loaded { settings: defaults(), reset: true, restored: false, newer: false }
                    }
                }
            }
        }
    }

    pub(crate) fn with_defaults(mut self) -> Self {
        if self.search.file_threshold == 0 {
            self.search.file_threshold = 1500;
        }
        if self.search.extension_repo.trim() == RETIRED_EXTENSION_REPO {
            self.search.extension_repo.clear();
        }
        if self.schema_version == 0 {
            self.schema_version = SCHEMA_VERSION;
        }
        if self.wrap_nag_threshold == 0 {
            self.wrap_nag_threshold = 5;
        }
        if self.idle_minutes == 0 {
            self.idle_minutes = 15;
        }
        normalize_permission_mode(&mut self.ai.claude.permission_mode);
        let z = &mut self.accessibility.zoom;
        *z = if *z == 0 { 100 } else { (*z).clamp(ZOOM_MIN, ZOOM_MAX) };
        self.list_open_library();
        self
    }

    /// The open library is always in the list, holding no parked settings of its own (its settings
    /// are the top-level ones). This is the whole upgrade for an install with one library: it is
    /// listed, nothing else changes. Entries without a folder or with an id seen before are dropped.
    fn list_open_library(&mut self) {
        let mut seen = std::collections::BTreeSet::new();
        self.libraries.retain(|l| !l.path.trim().is_empty() && !l.id.is_empty() && seen.insert(l.id.clone()));
        let Some(open) = self.library_path.clone().filter(|p| !p.trim().is_empty()) else { return };
        let mut found = false;
        for l in &mut self.libraries {
            if !found && same_folder(std::path::Path::new(&l.path), std::path::Path::new(&open)) {
                found = true;
                l.parked = None;
                l.path = open.clone();
            }
        }
        if !found {
            let mut id = library_id_for(&open);
            while self.libraries.iter().any(|l| l.id == id) { id = format!("lib-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]); }
            self.libraries.insert(0, Library { id, name: String::new(), path: open, parked: None });
        }
    }

    fn write_to(&self, file: &std::path::Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(file, text.as_bytes(), true)
    }

    /// Save to `file`, then keep the same content as `<file>.bak` — the copy `load_full` falls back
    /// to. The backup is written only after the main file, so it always holds a version that was
    /// saved whole.
    pub fn save_to(&self, file: &std::path::Path) -> std::io::Result<()> {
        self.write_to(file)?;
        crate::log::ok_or_warn("could not write the settings backup", self.write_to(&backup_file(file)));
        Ok(())
    }

    pub fn save(&self) -> std::io::Result<()> {
        if read_only() {
            return Err(std::io::Error::other(crate::tr::user_err_in(crate::tr::arabic(), "newer_settings", &[])));
        }
        self.save_to(&crate::paths::settings_file())
    }

    /// Validate settings sent by a client against the current ones, and put back every
    /// server-owned field.
    pub fn sanitize_update(&mut self, old: &Settings) -> anyhow::Result<()> {
        // Server-owned.
        self.schema_version = SCHEMA_VERSION;
        self.last_wrap = old.last_wrap;
        self.projects = old.projects.clone(); // projects change only through the project endpoints
        self.libraries = old.libraries.clone(); // the switcher's list changes only through its own endpoints
        self.chat_retention_days = old.chat_retention_days; // deleting chats automatically is switched only on its own endpoint
        // The provider address and its key belong together and change only through
        // `claude::set_provider`. A general save can therefore never drop a saved key, and never
        // leave an address behind without one.
        self.ai.claude.has_api_token = old.ai.claude.has_api_token;
        self.ai.claude.base_url = old.ai.claude.base_url.clone();

        let c = &mut self.ai.claude;
        for v in [&mut c.binary, &mut c.base_url, &mut c.model, &mut c.effort, &mut c.permission_mode, &mut c.mini_wrap_model] { none_if_blank(v); }
        // A client that names no mode (a partial object, an add-on, a window opened before the
        // update) keeps the one in force: saving something else never changes what the assistant may do.
        if c.permission_mode.is_none() { c.permission_mode = old.ai.claude.permission_mode.clone(); }
        check_permission_mode(c.permission_mode.as_deref())?;
        check_cli_value("model", c.model.as_deref())?;
        check_cli_value("effort", c.effort.as_deref())?;
        check_cli_value("mini-wrap model", c.mini_wrap_model.as_deref())?;
        if let Some(b) = c.binary.as_deref().filter(|b| Some(*b) != old.ai.claude.binary.as_deref()) {
            let p = std::path::Path::new(b);
            let named_claude = p.file_name().and_then(|n| n.to_str()).map_or(false, |n| n.to_lowercase().starts_with("claude"));
            if !p.is_absolute() || !named_claude { anyhow::bail!("the assistant program must be a full path to a file named claude"); }
            if !std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false) { anyhow::bail!("the assistant program was not found at {b}"); }
        }

        self.spending.sanitize()?;

        self.search.extension_repo = self.search.extension_repo.trim().to_string();
        if self.search.extension_repo == RETIRED_EXTENSION_REPO { self.search.extension_repo.clear(); }
        let repo = &self.search.extension_repo;
        if !repo.is_empty() && (!repo.starts_with("https://") || repo.len() < 12 || repo.chars().any(|ch| ch.is_whitespace() || ch.is_control())) {
            anyhow::bail!("the search extension address must start with https://");
        }

        if self.sync.target != "drive" { self.sync.target = "online".into(); }
        // A location's short name, or nothing (the same shape the API accepts when one is added).
        self.sync.main_location = self.sync.main_location.take().map(|n| n.trim().to_string())
            .filter(|n| (1..=40).contains(&n.len()) && !n.starts_with('-') && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)));

        let mut private = vec![];
        for p in &self.private_paths {
            if p.trim().is_empty() { continue; }
            let clean = crate::paths::clean_rel(p)?;
            if !private.contains(&clean) { private.push(clean); }
        }
        self.private_paths = private;

        // One line of plain text, or nothing.
        self.profession = self.profession.take().map(|p| p.chars().filter(|c| !c.is_control()).take(200).collect::<String>().trim().to_string()).filter(|p| !p.is_empty());
        self.user_name = self.user_name.chars().filter(|c| !c.is_control()).take(120).collect::<String>().trim().to_string();
        if !["en", "ar"].contains(&self.language.as_str()) { self.language = old.language.clone(); }
        self.calendar.sanitize();
        self.capture.sanitize();
        // Zero is "unset" for these (a client that sent a partial object): fill them like a loaded file.
        *self = std::mem::take(self).with_defaults();
        Ok(())
    }

    pub fn library(&self) -> Option<std::path::PathBuf> {
        self.library_path.as_ref().map(std::path::PathBuf::from)
    }

    // ----- several libraries -----

    /// The open library's own settings, as they would be parked when another one is opened.
    pub fn own(&self) -> LibrarySettings {
        LibrarySettings {
            sync: self.sync.clone(), keep_history: self.keep_history, private_paths: self.private_paths.clone(),
            profession: self.profession.clone(), modules: self.modules.clone(), projects: self.projects.clone(), last_wrap: self.last_wrap,
        }
    }

    /// Make `l` the open library's settings. Where passwords are kept stays the app's choice.
    pub fn set_own(&mut self, l: LibrarySettings) {
        let store = std::mem::take(&mut self.sync.credential_store);
        self.sync = l.sync;
        self.sync.credential_store = if store.is_empty() { "keychain".into() } else { store };
        self.keep_history = l.keep_history;
        self.private_paths = l.private_paths;
        self.profession = l.profession;
        self.modules = l.modules;
        self.projects = l.projects;
        self.last_wrap = l.last_wrap;
    }

    fn open_index(&self) -> Option<usize> {
        let open = self.library_path.as_deref().filter(|p| !p.trim().is_empty())?;
        self.libraries.iter().position(|l| same_folder(std::path::Path::new(&l.path), std::path::Path::new(open)))
    }

    /// The open library's entry in the switcher.
    pub fn open_library(&self) -> Option<&Library> { self.open_index().map(|i| &self.libraries[i]) }

    /// The open library's id (`None` before there is a library).
    pub fn library_id(&self) -> Option<String> { self.open_library().map(|l| l.id.clone()) }

    /// The listed library whose folder is `lib`.
    pub fn library_at(&self, lib: &std::path::Path) -> Option<&Library> {
        self.libraries.iter().find(|l| same_folder(std::path::Path::new(&l.path), lib))
    }

    /// Is `lib` the open library's folder?
    pub fn is_open_library(&self, lib: &std::path::Path) -> bool {
        self.library_path.as_deref().map_or(false, |p| !p.trim().is_empty() && same_folder(std::path::Path::new(p), lib))
    }

    /// The settings of the library at `lib`: the open one's, another listed one's (what was parked,
    /// or what a new one starts with), or `None` for a folder that is not a library in the list.
    pub fn settings_of(&self, lib: &std::path::Path) -> Option<LibrarySettings> {
        if self.is_open_library(lib) { return Some(self.own()); }
        self.library_at(lib).map(|l| l.parked.clone().unwrap_or_else(|| LibrarySettings::for_new(self)))
    }

    /// The private folders of the library at `lib`; the open library's when `lib` is none of the
    /// listed ones (a linked folder, the app's own folders).
    pub fn private_paths_of(&self, lib: Option<&std::path::Path>) -> Vec<String> {
        lib.and_then(|l| self.settings_of(l)).map_or_else(|| self.private_paths.clone(), |l| l.private_paths)
    }

    /// Is a history kept for the library at `lib`? (`history_enabled`, for any listed library.)
    pub fn history_enabled_in(&self, lib: &std::path::Path) -> bool {
        match self.settings_of(lib) {
            Some(l) => l.keep_history || l.sync.mode.starts_with("git"),
            None => self.history_enabled(),
        }
    }

    /// The backup settings of the library at `lib` (the open one's when it is not listed).
    pub fn sync_of(&self, lib: &std::path::Path) -> SyncSettings {
        self.settings_of(lib).map_or_else(|| self.sync.clone(), |l| l.sync)
    }

    /// Note that the library at `lib` was just wrapped, wherever its settings are kept now.
    pub fn set_last_wrap(&mut self, lib: Option<&std::path::Path>, at: chrono::DateTime<chrono::Utc>) {
        match lib {
            Some(l) if !self.is_open_library(l) => {
                let fresh = LibrarySettings::for_new(self);
                if let Some(e) = self.libraries.iter_mut().find(|e| same_folder(std::path::Path::new(&e.path), l)) {
                    e.parked.get_or_insert(fresh).last_wrap = Some(at);
                }
            }
            _ => self.last_wrap = Some(at),
        }
    }

    /// Open the listed library `id`: the open one's settings are parked in its entry, and `id`'s
    /// own come out of theirs. Nothing on disk changes here (the caller saves).
    pub fn switch_to(&mut self, id: &str) -> anyhow::Result<()> {
        self.list_open_library();
        let to = self.libraries.iter().position(|l| l.id == id).ok_or_else(|| crate::tr::err("libs.not_found", &[]))?;
        let from = self.open_index();
        if from == Some(to) { return Ok(()); }
        let parked = self.libraries[to].parked.take();
        let theirs = parked.unwrap_or_else(|| LibrarySettings::for_new(self));
        if let Some(i) = from { self.libraries[i].parked = Some(self.own()); }
        self.set_own(theirs);
        self.library_path = Some(self.libraries[to].path.clone());
        Ok(())
    }

    /// Open the library in folder `path` — the listed one there, or a new entry — parking the open
    /// one's settings. `own`: the new library's settings when the user just chose them (setup);
    /// otherwise a listed one gets its own back and a new one starts from `for_new`. Returns its id.
    pub fn open_folder(&mut self, path: &str, own: Option<LibrarySettings>) -> String {
        self.list_open_library();
        let id = match self.library_at(std::path::Path::new(path)) {
            Some(l) => l.id.clone(),
            None => {
                let mut id = library_id_for(path);
                while self.libraries.iter().any(|l| l.id == id) { id = format!("lib-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]); }
                self.libraries.push(Library { id: id.clone(), name: String::new(), path: path.to_string(), parked: None });
                id
            }
        };
        // Cannot fail: the id was just found or made.
        let _ = self.switch_to(&id);
        if let Some(o) = own { self.set_own(o); }
        id
    }

    /// The open library is now at `path` (its folder was moved or renamed): same entry, same settings.
    pub fn move_open_library(&mut self, path: &str) {
        self.list_open_library();
        if let Some(i) = self.open_index() { self.libraries[i].path = path.to_string(); }
        self.library_path = Some(path.to_string());
        self.list_open_library();
    }

    /// Give library `id` a name of the user's choosing (empty: its folder's name).
    pub fn rename_library(&mut self, id: &str, name: &str) -> anyhow::Result<()> {
        let clean: String = name.chars().filter(|c| !c.is_control()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
        if clean.chars().count() > LIBRARY_NAME_MAX { return Err(crate::tr::err("libs.name_long", &[("max", &LIBRARY_NAME_MAX.to_string())])); }
        let l = self.libraries.iter_mut().find(|l| l.id == id).ok_or_else(|| crate::tr::err("libs.not_found", &[]))?;
        l.name = clean;
        Ok(())
    }

    /// Take library `id` off the list. Its folder, its chats and its history stay where they are;
    /// only the settings it kept here go. The open library cannot be removed (open another first).
    pub fn remove_library(&mut self, id: &str) -> anyhow::Result<Library> {
        self.list_open_library();
        let i = self.libraries.iter().position(|l| l.id == id).ok_or_else(|| crate::tr::err("libs.not_found", &[]))?;
        if Some(i) == self.open_index() { return Err(crate::tr::err("libs.remove_open", &[])); }
        Ok(self.libraries.remove(i))
    }

    /// Every library folder in the list besides the open one, and every linked folder of any
    /// library: what must never be touched when the app removes its own data.
    pub fn other_folders(&self) -> Vec<std::path::PathBuf> {
        let open = self.open_index();
        let mut out = vec![];
        for (i, l) in self.libraries.iter().enumerate() {
            if Some(i) == open { continue; }
            out.push(std::path::PathBuf::from(&l.path));
            if let Some(p) = &l.parked { out.extend(p.projects.iter().map(|p| std::path::PathBuf::from(&p.path))); }
        }
        out
    }

    /// Is a history of changes kept for the library? It is, for every kind of backup, unless the
    /// user switched it off — and always with an online location, which is sent that very history.
    pub fn history_enabled(&self) -> bool { self.keep_history || self.sync_enabled() }

    /// Is the library backed up to an online location (the modes "git-manual" and "git-auto")?
    pub fn sync_enabled(&self) -> bool { self.sync.mode.starts_with("git") }

    /// The older name of `history_enabled`, from when history existed only together with an
    /// online backup. Everything that records, lists or undoes changes asks this.
    pub fn git_enabled(&self) -> bool { self.history_enabled() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_modes_are_allow_listed() {
        for ok in [None, Some(""), Some("default"), Some("acceptEdits"), Some("plan"), Some("bypassPermissions")] { assert!(check_permission_mode(ok).is_ok(), "{ok:?}"); }
        // "Ask before changes" no longer exists: it is read as "plan" wherever it is still stored.
        assert!(!PERMISSION_MODES.contains(&"default"));
        assert_eq!(effective_permission_mode(Some("default")), "plan");
        assert_eq!(effective_permission_mode(Some("acceptEdits")), "acceptEdits");
        // Nothing said is never "whatever Claude Code would do": it is accept-edits.
        assert_eq!(effective_permission_mode(Some("")), "acceptEdits");
        assert_eq!(effective_permission_mode(None), "acceptEdits");
        assert_eq!(effective_permission_mode(Some(BYPASS)), BYPASS);
        assert_eq!(parse(r#"{"ai":{"claude":{"permissionMode":"default"}}}"#).unwrap().ai.claude.permission_mode.as_deref(), Some("plan"));
        for bad in ["auto", "dontAsk", "manual", "Plan", "bypasspermissions", "plan --dangerously-skip-permissions"] {
            assert!(check_permission_mode(Some(bad)).is_err(), "{bad}");
        }
        // Switching "bypass permissions" on is the one step that needs the user's yes; staying on it does not.
        assert!(bypass_needs_confirm(Some("plan"), Some(BYPASS)) && bypass_needs_confirm(None, Some(BYPASS)));
        assert!(!bypass_needs_confirm(Some(BYPASS), Some(BYPASS)) && !bypass_needs_confirm(Some(BYPASS), Some("plan")));
        assert!(check_cli_value("model", Some("claude-opus-4[1m]")).is_ok());
        assert!(check_cli_value("model", Some("--dangerously-skip-permissions")).is_err());
        assert!(check_cli_value("model", Some("a b")).is_err());
    }

    #[test]
    fn the_default_mode_is_accept_edits_and_a_save_never_changes_it_by_accident() {
        // A fresh install, a file from before the mode was stored, and an explicit null all read as accept-edits.
        assert_eq!(Settings::default().with_defaults().ai.claude.permission_mode.as_deref(), Some("acceptEdits"));
        for file in [r#"{}"#, r#"{"onboarded":true,"ai":{"claude":{}}}"#, r#"{"ai":{"claude":{"permissionMode":null}}}"#, r#"{"ai":{"claude":{"permissionMode":""}}}"#] {
            assert_eq!(parse(file).unwrap().ai.claude.permission_mode.as_deref(), Some("acceptEdits"), "{file}");
        }
        // What the user chose is kept as it is.
        for chosen in ["plan", "acceptEdits", BYPASS] {
            let file = format!(r#"{{"ai":{{"claude":{{"permissionMode":"{chosen}"}}}}}}"#);
            assert_eq!(parse(&file).unwrap().ai.claude.permission_mode.as_deref(), Some(chosen));
        }
        // Saving something unrelated (here: the language) with no mode in the request keeps the mode in force.
        for chosen in ["plan", "acceptEdits", BYPASS] {
            let mut old = Settings::default().with_defaults();
            old.ai.claude.permission_mode = Some(chosen.into());
            for sent in [None, Some(String::new()), Some("  ".to_string())] {
                let mut new = old.clone();
                new.language = "ar".into();
                new.ai.claude.permission_mode = sent;
                new.sanitize_update(&old).unwrap();
                assert_eq!(new.ai.claude.permission_mode.as_deref(), Some(chosen));
                assert_eq!(new.language, "ar");
            }
        }
        // First-run setup sends no mode at all: the result is accept-edits, not "unset" and not plan.
        let old = Settings::default().with_defaults();
        let mut first: Settings = serde_json::from_str(r#"{"language":"ar","ai":{"claude":{"permissionMode":null}}}"#).unwrap();
        first.sanitize_update(&old).unwrap();
        assert_eq!(first.ai.claude.permission_mode.as_deref(), Some("acceptEdits"));
        // An explicit choice in the request is what gets saved.
        let mut new = old.clone();
        new.ai.claude.permission_mode = Some("plan".into());
        new.sanitize_update(&old).unwrap();
        assert_eq!(new.ai.claude.permission_mode.as_deref(), Some("plan"));
    }

    #[test]
    fn base_url_needs_https_unless_local() {
        for ok in ["https://api.example.com", "https://api.example.com:8443/v1", "http://localhost:11434", "http://127.0.0.1/v1"] {
            assert!(check_base_url(ok).is_ok(), "{ok}");
        }
        for bad in ["http://api.example.com", "http://localhost.evil.com", "http://localhost@evil.com", "ftp://x", "https://", "https://a b", "api.example.com"] {
            assert!(check_base_url(bad).is_err(), "{bad}");
        }
        for own in ["https://api.anthropic.com", "https://api.anthropic.com/", "https://API.Anthropic.com/v1", "https://api.anthropic.com:443"] {
            assert!(is_anthropic_url(own), "{own}");
        }
        for other in ["https://api.anthropic.com.evil.example", "https://evil.example/api.anthropic.com", "http://api.anthropic.com", "https://anthropic.com",
                      "https://user@api.anthropic.com", "https://gateway.example.com", ""] {
            assert!(!is_anthropic_url(other), "{other}");
        }
    }

    #[test]
    fn update_keeps_server_owned_fields_and_validates() {
        let mut old = Settings::default();
        old.projects.push(Project { id: "1".into(), name: "p".into(), path: "/tmp/p".into() });
        old.ai.claude.has_api_token = true;
        old.ai.claude.base_url = Some("https://a.example.com".into());

        // Projects cannot be replaced; the key flag cannot be set or cleared.
        let mut new = old.clone();
        new.projects.clear();
        new.schema_version = 99;
        new.private_paths = vec!["me/finances/".into(), "./me/finances".into(), " ".into()];
        new.ai.claude.has_api_token = false;
        new.sanitize_update(&old).unwrap();
        assert!(new.ai.claude.has_api_token);
        assert_eq!(new.projects.len(), 1);
        assert_eq!(new.schema_version, SCHEMA_VERSION);
        assert_eq!(new.private_paths, ["me/finances"]);

        // A general save never moves the provider address, and so never separates it from its key:
        // whatever the request says (another address, none, nonsense), both stay as they were.
        for sent in [Some("https://b.example.com"), None, Some(""), Some("http://evil.example.com"), Some("not an address")] {
            let mut new = old.clone();
            new.ai.claude.base_url = sent.map(String::from);
            new.user_name = "Sara".into();
            new.sanitize_update(&old).unwrap();
            assert_eq!(new.ai.claude.base_url.as_deref(), Some("https://a.example.com"), "{sent:?}");
            assert!(new.ai.claude.has_api_token, "{sent:?}");
            assert_eq!(new.user_name, "Sara");
        }

        let bad = |f: &dyn Fn(&mut Settings)| { let mut n = old.clone(); f(&mut n); n.sanitize_update(&old).is_err() };
        assert!(bad(&|n| n.ai.claude.permission_mode = Some("dontAsk".into())));
        assert!(bad(&|n| n.search.extension_repo = "ssh://evil/repo".into()));
        assert!(bad(&|n| n.search.extension_repo = "--upload-pack=evil".into()));
        assert!(bad(&|n| n.private_paths = vec!["../outside".into()]));
        assert!(bad(&|n| n.ai.claude.binary = Some("claude".into())));
        assert!(bad(&|n| n.ai.claude.binary = Some("/bin/sh".into())));
        assert!(bad(&|n| n.ai.claude.binary = Some("/nonexistent/dir/claude".into())));
    }

    #[test]
    fn the_smart_search_address_is_only_one_the_user_typed() {
        // Nothing by default, and the address earlier versions filled in is cleared.
        assert_eq!(Settings::default().with_defaults().search.extension_repo, "");
        let saved: Settings = serde_json::from_str(&format!(r#"{{"search":{{"extensionRepo":"{RETIRED_EXTENSION_REPO}"}}}}"#)).unwrap();
        assert_eq!(saved.with_defaults().search.extension_repo, "");
        let old = Settings::default().with_defaults();
        let mut new = old.clone();
        new.search.extension_repo = format!(" {RETIRED_EXTENSION_REPO} ");
        new.sanitize_update(&old).unwrap();
        assert_eq!(new.search.extension_repo, "");
        // Empty stays empty; one the user typed is kept.
        let mut new = old.clone();
        new.sanitize_update(&old).unwrap();
        assert_eq!(new.search.extension_repo, "");
        let mut new = old.clone();
        new.search.extension_repo = "https://example.com/me/search.git".into();
        new.sanitize_update(&old).unwrap();
        assert_eq!(new.search.extension_repo, "https://example.com/me/search.git");
    }

    #[test]
    fn binary_must_be_a_real_file_named_claude() {
        let d = std::env::temp_dir().join(format!("thoughts-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("claude-dir")).unwrap();
        std::fs::write(d.join("claude"), "").unwrap();
        let old = Settings::default();
        let set = |p: std::path::PathBuf| { let mut n = old.clone(); n.ai.claude.binary = Some(p.to_string_lossy().into()); n.sanitize_update(&old) };
        assert!(set(d.join("claude")).is_ok());
        assert!(set(d.join("claude-dir")).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn damaged_settings_are_moved_aside_not_overwritten() {
        let d = std::env::temp_dir().join(format!("thoughts-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("settings.json");
        // Missing: defaults, nothing recovered.
        assert!(!Settings::load_or_recover(&f).1);
        // Healthy.
        std::fs::write(&f, r#"{"language":"ar","onboarded":true}"#).unwrap();
        let (s, recovered) = Settings::load_or_recover(&f);
        assert!(!recovered && s.onboarded && s.language == "ar" && s.search.file_threshold == 1500);
        // Damaged: kept under a new name, byte for byte.
        for bad in ["{\"language\":\"ar\",", "[1,2]", "{\"idleMinutes\":\"soon\"}"] {
            std::fs::write(&f, bad).unwrap();
            let (s, recovered) = Settings::load_or_recover(&f);
            assert!(recovered && !s.onboarded);
            assert!(!f.exists());
            let aside: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.path())
                .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("settings.json.corrupt-")).collect();
            assert_eq!(aside.len(), 1);
            assert_eq!(std::fs::read_to_string(&aside[0]).unwrap(), bad);
            std::fs::remove_file(&aside[0]).unwrap();
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_damaged_file_is_replaced_by_the_backup_not_by_defaults() {
        let d = std::env::temp_dir().join(format!("thoughts-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("settings.json");
        let mut s = Settings::default();
        s.onboarded = true;
        s.language = "ar".into();
        s.library_path = Some("/Users/sara/Notes".into());
        s.save_to(&f).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), std::fs::read(backup_file(&f)).unwrap());
        // The main file is cut short by a crash or a full disk.
        std::fs::write(&f, "{\"language\":").unwrap();
        let l = Settings::load_full(&f);
        assert!(l.restored && !l.reset && !l.newer);
        assert!(l.settings.onboarded, "a good backup must never look like a fresh install");
        assert_eq!(l.settings.library_path.as_deref(), Some("/Users/sara/Notes"));
        // The good copy is back in place and the damaged one is kept for inspection.
        assert!(parse(&std::fs::read_to_string(&f).unwrap()).unwrap().onboarded);
        let corrupt = |d: &std::path::Path| std::fs::read_dir(d).unwrap().flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("settings.json.corrupt-")).count();
        assert_eq!(corrupt(&d), 1);
        // Lost altogether: the backup is put back as well.
        std::fs::remove_file(&f).unwrap();
        let l = Settings::load_full(&f);
        assert!(l.restored && l.settings.onboarded && f.exists());
        // Both bad: only now does the app start over, and says so.
        std::fs::write(&f, "garbage").unwrap();
        std::fs::write(backup_file(&f), "also garbage").unwrap();
        let l = Settings::load_full(&f);
        assert!(l.reset && !l.restored && !l.settings.onboarded);
        // At most KEEP_CORRUPT damaged copies pile up.
        for i in 0..8 {
            std::fs::write(d.join(format!("settings.json.corrupt-2020010{i}-000000000")), "x").unwrap();
        }
        std::fs::write(&f, "garbage again").unwrap();
        Settings::load_full(&f);
        assert_eq!(corrupt(&d), KEEP_CORRUPT);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_file_from_a_newer_app_is_used_but_never_touched() {
        let d = std::env::temp_dir().join(format!("thoughts-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("settings.json");
        let newer = r#"{"schemaVersion":99,"onboarded":true,"language":"ar","somethingNew":{"a":1}}"#;
        std::fs::write(&f, newer).unwrap();
        let l = Settings::load_full(&f);
        assert!(l.newer && !l.reset && !l.restored);
        assert!(l.settings.onboarded && l.settings.language == "ar");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), newer);
        // Even one this version cannot read at all stays exactly where it is.
        let unreadable = r#"{"schemaVersion":99,"idleMinutes":{"weekday":15}}"#;
        std::fs::write(&f, unreadable).unwrap();
        let l = Settings::load_full(&f);
        assert!(l.newer);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), unreadable);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "nothing moved aside, no backup written");
        // The current version is not "newer".
        std::fs::write(&f, format!(r#"{{"schemaVersion":{SCHEMA_VERSION}}}"#)).unwrap();
        assert!(!Settings::load_full(&f).newer);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn an_update_fills_unset_values_and_tidies_free_text() {
        let old = Settings::default().with_defaults();
        let mut new: Settings = serde_json::from_str(r#"{"libraryPath":"/x","profession":"  Tax accountant\n(freelance)  ","userName":"Sara\u0007"}"#).unwrap();
        assert_eq!(new.search.file_threshold, 0);
        new.sanitize_update(&old).unwrap();
        assert_eq!(new.search.file_threshold, 1500);
        assert_eq!(new.wrap_nag_threshold, 5);
        assert_eq!(new.profession.as_deref(), Some("Tax accountant(freelance)"));
        assert_eq!(new.user_name, "Sara");
        let mut blank = old.clone();
        blank.profession = Some("   ".into());
        blank.ai.claude.permission_mode = Some("default".into());
        blank.sanitize_update(&old).unwrap();
        assert_eq!(blank.profession, None);
        assert_eq!(blank.ai.claude.permission_mode.as_deref(), Some("plan"));
    }

    #[test]
    fn migrations_run_from_the_files_version() {
        assert_eq!(MIGRATIONS.len() as u32, SCHEMA_VERSION);
        let v = migrate(serde_json::json!({"language": "ar"}));
        assert_eq!(v["schemaVersion"], SCHEMA_VERSION);
        assert_eq!(v["language"], "ar");
        // Already current, or written by a newer app: untouched.
        assert_eq!(migrate(serde_json::json!({"schemaVersion": SCHEMA_VERSION}))["schemaVersion"], SCHEMA_VERSION);
        assert_eq!(migrate(serde_json::json!({"schemaVersion": 99}))["schemaVersion"], 99);
        assert_eq!(parse(r#"{"userName":"Sara"}"#).unwrap().user_name, "Sara");
        assert!(parse("nonsense").is_err());
    }

    #[test]
    fn old_settings_files_get_new_fields() {
        let s: Settings = serde_json::from_str(r#"{"language":"ar"}"#).unwrap();
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        assert!(s.private_paths.is_empty());
        assert!(s.usage_signals, "on unless the user turns it off");
        assert!(!serde_json::from_str::<Settings>(r#"{"usageSignals":false}"#).unwrap().usage_signals);
        assert_eq!(s.accessibility, Accessibility::default());
        assert!(!s.release_check, "the version check is off unless switched on");
        assert!(serde_json::from_str::<Settings>(r#"{"releaseCheck":true}"#).unwrap().release_check);
    }

    #[test]
    fn history_is_on_for_every_kind_of_backup_unless_switched_off() {
        // Files written before the switch existed, whatever backup they chose: history is on.
        for mode in ["none", "folder", "git-manual", "git-auto"] {
            let s = parse(&format!(r#"{{"sync":{{"mode":"{mode}"}}}}"#)).unwrap();
            assert!(s.keep_history && s.history_enabled() && s.git_enabled(), "{mode}");
            assert_eq!(s.sync_enabled(), mode.starts_with("git"), "{mode}");
        }
        assert!(Settings::default().history_enabled());
        // Switching it off is the user's choice and survives a round trip; the backup is untouched.
        for mode in ["none", "folder"] {
            let off = parse(&format!(r#"{{"keepHistory":false,"sync":{{"mode":"{mode}"}}}}"#)).unwrap();
            assert!(!off.history_enabled() && !off.git_enabled() && !off.sync_enabled(), "{mode}");
            assert_eq!(off.sync.mode, mode);
            let again: Settings = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
            assert!(!again.keep_history);
            let mut updated = off.clone();
            updated.sanitize_update(&Settings::default()).unwrap();
            assert!(!updated.keep_history);
        }
        // An online location is sent the history itself, so there the history is always kept.
        let online = parse(r#"{"keepHistory":false,"sync":{"mode":"git-auto"}}"#).unwrap();
        assert!(online.history_enabled() && online.sync_enabled());
        // The main location is a short name or nothing; nothing git could read as an option.
        let main = |name: &str| { let mut s = Settings::default(); s.sync.main_location = Some(name.into()); s.sanitize_update(&Settings::default()).unwrap(); s.sync.main_location };
        assert_eq!(main(" backup-2 ").as_deref(), Some("backup-2"));
        for bad in ["", "-f", "--upload-pack=x", "a b", "a/b", "نسخة"] { assert_eq!(main(bad), None, "{bad}"); }
        assert_eq!(parse(r#"{"sync":{"mode":"git-manual"}}"#).unwrap().sync.main_location, None);
    }

    #[test]
    fn an_install_with_one_library_upgrades_to_a_list_of_one_and_nothing_else_changes() {
        let old = r#"{"schemaVersion":1,"onboarded":true,"language":"ar","libraryPath":"/Users/sara/Notes","privatePaths":["me/finances"],
            "sync":{"mode":"git-auto","mainLocation":"backup"},"profession":"legal","modules":{"docs":true},
            "projects":[{"id":"p1","name":"Site","path":"/Users/sara/site"}],"lastWrap":"2026-01-02T03:04:05Z"}"#;
        let s = parse(old).unwrap();
        assert_eq!(s.libraries.len(), 1);
        let l = &s.libraries[0];
        assert_eq!((l.path.as_str(), l.name.as_str()), ("/Users/sara/Notes", ""));
        assert!(l.parked.is_none(), "the open library's settings stay where everything reads them");
        assert_eq!(l.display_name(), "Notes");
        // The same id on every start, even before anything is saved again.
        assert_eq!(parse(old).unwrap().libraries[0].id, l.id);
        assert_eq!(s.library_id().as_deref(), Some(l.id.as_str()));
        // Every setting is where it was.
        assert_eq!(s.private_paths, ["me/finances"]);
        assert_eq!((s.sync.mode.as_str(), s.sync.main_location.as_deref()), ("git-auto", Some("backup")));
        assert_eq!((s.profession.as_deref(), s.modules.get("docs"), s.projects.len()), (Some("legal"), Some(&true), 1));
        assert!(s.last_wrap.is_some() && s.onboarded && s.language == "ar");
        assert_eq!(s.settings_of(Path::new("/Users/sara/Notes")).unwrap().private_paths, ["me/finances"]);
        // Saved and read again: the same list, the same id.
        let again = parse(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(again.libraries.len(), 1);
        assert_eq!(again.libraries[0].id, l.id);
        // Not set up yet: no library, no list.
        assert!(parse(r#"{"onboarded":false}"#).unwrap().libraries.is_empty());
    }

    use std::path::Path;

    fn with_library(path: &str) -> Settings {
        let mut s = Settings::default();
        s.library_path = Some(path.into());
        s.language = "ar".into();
        s.private_paths = vec!["clients/acme".into()];
        s.sync.mode = "git-auto".into();
        s.sync.main_location = Some("backup".into());
        s.sync.credential_store = "file".into();
        s.profession = Some("legal".into());
        s.modules.insert("docs".into(), true);
        s.projects.push(Project { id: "p1".into(), name: "Acme site".into(), path: "/work/acme-site".into() });
        s.last_wrap = Some(chrono::Utc::now());
        s.with_defaults()
    }

    #[test]
    fn each_library_keeps_its_own_settings_across_switches() {
        let mut s = with_library("/work/Acme");
        let a = s.library_id().unwrap();
        let a_own = s.own();
        // A new library starts with the same kind of backup and line of work, and nothing of A's folder.
        let b = s.open_folder("/work/Personal", None);
        assert_ne!(a, b);
        assert_eq!(s.library_path.as_deref(), Some("/work/Personal"));
        assert_eq!(s.library_id().as_deref(), Some(b.as_str()));
        assert!(s.private_paths.is_empty() && s.projects.is_empty() && s.last_wrap.is_none() && s.sync.main_location.is_none());
        assert_eq!((s.sync.mode.as_str(), s.profession.as_deref(), s.sync.credential_store.as_str()), ("git-auto", Some("legal"), "file"));
        // B gets settings of its own.
        s.private_paths = vec!["diary".into()];
        s.sync.mode = "none".into();
        s.profession = None;
        // A's are where they were left, and come back whole.
        assert_eq!(s.settings_of(Path::new("/work/Acme")).unwrap().private_paths, ["clients/acme"]);
        assert_eq!(s.private_paths_of(Some(Path::new("/work/Acme"))), ["clients/acme"]);
        assert_eq!(s.private_paths_of(Some(Path::new("/work/Personal"))), ["diary"]);
        assert_eq!(s.private_paths_of(Some(Path::new("/elsewhere"))), ["diary"], "a folder that is no library: the open one's");
        assert!(s.history_enabled_in(Path::new("/work/Acme")));
        assert_eq!(s.sync_of(Path::new("/work/Acme")).main_location.as_deref(), Some("backup"));
        s.switch_to(&a).unwrap();
        assert_eq!(s.library_path.as_deref(), Some("/work/Acme"));
        let back = s.own();
        assert_eq!(back.private_paths, a_own.private_paths);
        assert_eq!((back.sync.mode, back.sync.main_location), (a_own.sync.mode, a_own.sync.main_location));
        assert_eq!((back.profession, back.modules, back.projects.len(), back.last_wrap), (a_own.profession, a_own.modules, 1, a_own.last_wrap));
        // …and B's are parked for next time; the app's own choices never moved.
        assert_eq!(s.settings_of(Path::new("/work/Personal")).unwrap().private_paths, ["diary"]);
        assert_eq!((s.language.as_str(), s.sync.credential_store.as_str()), ("ar", "file"));
        // Opening the folder that is already listed opens that library, not a second entry.
        assert_eq!(s.open_folder("/work/Personal/", None), b);
        assert_eq!(s.libraries.len(), 2);
        assert_eq!(s.private_paths, ["diary"]);
        // Unknown ids are refused and change nothing.
        assert!(s.switch_to("lib-nope").is_err());
        assert_eq!(s.library_id().as_deref(), Some(b.as_str()));
    }

    #[test]
    fn libraries_are_named_moved_and_taken_off_the_list_but_never_by_a_general_save() {
        let mut s = with_library("/work/Acme");
        let a = s.library_id().unwrap();
        let b = s.open_folder("/work/Personal", None);
        s.rename_library(&a, "  Acme\u{7}  &   Co ").unwrap();
        assert_eq!(s.libraries.iter().find(|l| l.id == a).unwrap().display_name(), "Acme & Co");
        assert!(s.rename_library(&a, &"x".repeat(LIBRARY_NAME_MAX + 1)).is_err());
        s.rename_library(&b, "").unwrap();
        assert_eq!(s.open_library().unwrap().display_name(), "Personal");
        // A save from a window never changes the list, whatever it sends.
        let mut sent = s.clone();
        sent.libraries.clear();
        sent.sanitize_update(&s).unwrap();
        assert_eq!(sent.libraries.len(), 2);
        // The open library cannot be taken off; another one can, and only its entry goes.
        assert!(s.remove_library(&b).is_err());
        // What the app's own removal must leave alone: the other library and its linked folder.
        assert_eq!(s.other_folders(), [std::path::PathBuf::from("/work/Acme"), std::path::PathBuf::from("/work/acme-site")]);
        let gone = s.remove_library(&a).unwrap();
        assert_eq!(gone.path, "/work/Acme");
        assert_eq!(s.libraries.len(), 1);
        // A library whose folder moved keeps its entry and its settings.
        s.private_paths = vec!["diary".into()];
        s.move_open_library("/Volumes/Disk/Personal");
        assert_eq!((s.library_id().as_deref(), s.libraries.len()), (Some(b.as_str()), 1));
        assert_eq!(s.private_paths, ["diary"]);
        // A wrap of a library that is not open is noted in its own settings.
        let c = s.open_folder("/work/Third", None);
        let when = chrono::Utc::now();
        s.set_last_wrap(Some(Path::new("/Volumes/Disk/Personal")), when);
        assert!(s.last_wrap.is_none(), "the open library was not wrapped");
        assert_eq!(s.settings_of(Path::new("/Volumes/Disk/Personal")).unwrap().last_wrap, Some(when));
        assert_eq!(s.library_id().as_deref(), Some(c.as_str()));
    }

    #[test]
    fn zoom_stays_in_range() {
        let z = |json: &str| parse(json).unwrap().accessibility.zoom;
        assert_eq!(z(r#"{}"#), 100);
        assert_eq!(z(r#"{"accessibility":{"zoom":0}}"#), 100);
        assert_eq!(z(r#"{"accessibility":{"zoom":20}}"#), ZOOM_MIN);
        assert_eq!(z(r#"{"accessibility":{"zoom":900}}"#), ZOOM_MAX);
        assert_eq!(z(r#"{"accessibility":{"zoom":125,"reduceMotion":true}}"#), 125);
        assert!(parse(r#"{"accessibility":{"reduceMotion":true}}"#).unwrap().accessibility.reduce_motion);
    }

    #[test]
    fn the_capture_shortcut_is_on_by_default_and_only_ever_key_names() {
        // On for a fresh install and for a file written before the setting existed.
        for file in [r#"{}"#, r#"{"onboarded":true}"#] {
            let c = parse(file).unwrap().capture;
            assert!(c.shortcut_on && c.shortcut.is_empty(), "{file}");
        }
        let off = parse(r#"{"capture":{"shortcutOn":false,"shortcut":"Control+Alt+KeyN"}}"#).unwrap().capture;
        assert_eq!(off, CaptureSettings { shortcut_on: false, shortcut: "Control+Alt+KeyN".into() });
        // A save keeps a plain combination and drops anything else (the default is used instead).
        let saved = |keys: &str| { let mut s = Settings::default(); s.capture.shortcut = keys.into(); s.sanitize_update(&Settings::default()).unwrap(); s.capture.shortcut };
        assert_eq!(saved("Control+Alt+Super+Space"), "Control+Alt+Super+Space");
        for bad in ["Ctrl Alt Space", "Control+Alt+\u{0}", "Control+<script>", "Control+Alt+مسافة", &"Control+".repeat(20)] {
            assert_eq!(saved(bad), "", "{bad}");
        }
    }
}
