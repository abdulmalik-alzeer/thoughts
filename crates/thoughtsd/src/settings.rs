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
    /// module id -> enabled
    pub modules: BTreeMap<String, bool>,
    pub daemon: DaemonSettings,
    pub notifications: bool,
    /// Minutes without activity before a chat gets its mini-wrap.
    pub idle_minutes: u64,
    /// Popup when this many chats are unwrapped.
    pub wrap_nag_threshold: usize,
    pub projects: Vec<Project>,
    pub last_wrap: Option<chrono::DateTime<chrono::Utc>>,
    pub search: SearchSettings,
    /// Library-relative folders the AI must never read.
    pub private_paths: Vec<String>,
    /// Keep the local-only usage log that suggestions are drawn from. Off = nothing is recorded or suggested.
    pub usage_signals: bool,
}

pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_EXTENSION_REPO: &str = "https://github.com/thoughts-app/thoughts-search.git";
/// The only permission modes the app ever passes to the CLI (never `bypassPermissions`).
pub const PERMISSION_MODES: &[&str] = &["default", "acceptEdits", "plan"];

/// Allow-list for a chat/run permission mode. `None` / empty means "the CLI default".
pub fn check_permission_mode(mode: Option<&str>) -> anyhow::Result<()> {
    match mode {
        None | Some("") => Ok(()),
        Some(m) if PERMISSION_MODES.contains(&m) => Ok(()),
        Some(m) => anyhow::bail!("permission mode \"{m}\" is not allowed (use default, acceptEdits or plan)"),
    }
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
pub fn check_base_url(url: &str) -> anyhow::Result<()> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) { anyhow::bail!("the provider address may not contain spaces"); }
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") { (true, r) }
        else if let Some(r) = url.strip_prefix("http://") { (false, r) }
        else { anyhow::bail!("the provider address must start with https://") };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') { anyhow::bail!("the provider address may not contain a username or password"); }
    let host = authority.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map_or(authority, |(h, _)| h);
    if host.is_empty() { anyhow::bail!("the provider address has no host"); }
    if !https && host != "localhost" && host != "127.0.0.1" {
        anyhow::bail!("the provider address must use https:// (http:// is only allowed for localhost)");
    }
    Ok(())
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
    /// Anthropic-compatible provider (sets ANTHROPIC_BASE_URL). Token lives in the keychain.
    pub base_url: Option<String>,
    pub has_api_token: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct SyncSettings {
    /// "git-auto" | "git-manual" | "folder" | "none"
    pub mode: String,
    /// "keychain" | "file"
    pub credential_store: String,
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
            modules: BTreeMap::new(),
            daemon: DaemonSettings { run_at_login: false, keep_running: true },
            notifications: true,
            idle_minutes: 15,
            wrap_nag_threshold: 5,
            projects: vec![],
            last_wrap: None,
            search: SearchSettings::default(),
            private_paths: vec![],
            usage_signals: true,
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
        Self { mode: "git-manual".into(), credential_store: "keychain".into() }
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

impl Settings {
    /// Read-only load (the app shell uses it too): a missing or unreadable file gives defaults and
    /// nothing on disk is touched. The daemon itself starts with `load_or_recover`.
    pub fn load() -> Self {
        std::fs::read_to_string(crate::paths::settings_file()).ok()
            .and_then(|s| parse(&s).ok())
            .unwrap_or_else(|| Settings::default().with_defaults())
    }

    /// Load `file`. A file that exists but cannot be understood is never silently replaced: it is
    /// moved aside to `settings.json.corrupt-<timestamp>` and `true` is returned alongside defaults.
    pub fn load_or_recover(file: &std::path::Path) -> (Self, bool) {
        let text = match std::fs::read(file) {
            Ok(b) => String::from_utf8_lossy(&b).to_string(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default().with_defaults(), false),
            Err(e) => {
                // Unreadable (permissions, disk): keep the file where it is and say so.
                crate::log::error(format!("settings could not be read ({e}); starting with defaults"));
                return (Settings::default().with_defaults(), true);
            }
        };
        match parse(&text) {
            Ok(s) => (s, false),
            Err(e) => {
                let aside = file.with_file_name(format!("{}.corrupt-{}",
                    file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "settings.json".into()),
                    chrono::Local::now().format("%Y%m%d-%H%M%S")));
                match std::fs::rename(file, &aside) {
                    Ok(()) => crate::log::error(format!("settings file was damaged ({e}); kept as {} and started with defaults", aside.display())),
                    Err(e2) => crate::log::error(format!("settings file was damaged ({e}) and could not be moved aside ({e2})")),
                }
                (Settings::default().with_defaults(), true)
            }
        }
    }

    fn with_defaults(mut self) -> Self {
        if self.search.file_threshold == 0 {
            self.search.file_threshold = 1500;
        }
        if self.search.extension_repo.is_empty() {
            self.search.extension_repo = DEFAULT_EXTENSION_REPO.into();
        }
        if self.schema_version == 0 {
            self.schema_version = SCHEMA_VERSION;
        }
        if self.wrap_nag_threshold == 0 {
            self.wrap_nag_threshold = 5;
        }
        self
    }

    pub fn save(&self) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(&crate::paths::settings_file(), text.as_bytes(), true)
    }

    /// Validate settings sent by a client against the current ones, and put back every
    /// server-owned field. Returns `true` when the provider address changed — the caller must
    /// then forget the stored API token so it can never be sent to the new address.
    pub fn sanitize_update(&mut self, old: &Settings) -> anyhow::Result<bool> {
        // Server-owned.
        self.schema_version = SCHEMA_VERSION;
        self.last_wrap = old.last_wrap;
        self.projects = old.projects.clone(); // projects change only through the project endpoints
        self.ai.claude.has_api_token = old.ai.claude.has_api_token;

        let c = &mut self.ai.claude;
        for v in [&mut c.binary, &mut c.base_url, &mut c.model, &mut c.effort, &mut c.permission_mode, &mut c.mini_wrap_model] { none_if_blank(v); }
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
        let old_url = old.ai.claude.base_url.as_deref().filter(|u| !u.trim().is_empty());
        let url_changed = c.base_url.as_deref() != old_url;
        if let Some(u) = &c.base_url { check_base_url(u)?; }
        if url_changed { c.has_api_token = false; }

        if self.search.extension_repo.trim().is_empty() { self.search.extension_repo = DEFAULT_EXTENSION_REPO.into(); }
        let repo = &self.search.extension_repo;
        if !repo.starts_with("https://") || repo.len() < 12 || repo.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
            anyhow::bail!("the search extension address must start with https://");
        }

        let mut private = vec![];
        for p in &self.private_paths {
            if p.trim().is_empty() { continue; }
            let clean = crate::paths::clean_rel(p)?;
            if !private.contains(&clean) { private.push(clean); }
        }
        self.private_paths = private;
        Ok(url_changed)
    }

    pub fn library(&self) -> Option<std::path::PathBuf> {
        self.library_path.as_ref().map(std::path::PathBuf::from)
    }

    pub fn git_enabled(&self) -> bool {
        self.sync.mode.starts_with("git")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_modes_are_allow_listed() {
        for ok in [None, Some(""), Some("default"), Some("acceptEdits"), Some("plan")] { assert!(check_permission_mode(ok).is_ok(), "{ok:?}"); }
        for bad in ["bypassPermissions", "auto", "dontAsk", "Plan", "plan --dangerously-skip-permissions"] {
            assert!(check_permission_mode(Some(bad)).is_err(), "{bad}");
        }
        assert!(check_cli_value("model", Some("claude-opus-4[1m]")).is_ok());
        assert!(check_cli_value("model", Some("--dangerously-skip-permissions")).is_err());
        assert!(check_cli_value("model", Some("a b")).is_err());
    }

    #[test]
    fn base_url_needs_https_unless_local() {
        for ok in ["https://api.example.com", "https://api.example.com:8443/v1", "http://localhost:11434", "http://127.0.0.1/v1"] {
            assert!(check_base_url(ok).is_ok(), "{ok}");
        }
        for bad in ["http://api.example.com", "http://localhost.evil.com", "http://localhost@evil.com", "ftp://x", "https://", "https://a b", "api.example.com"] {
            assert!(check_base_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn update_keeps_server_owned_fields_and_validates() {
        let mut old = Settings::default();
        old.projects.push(Project { id: "1".into(), name: "p".into(), path: "/tmp/p".into() });
        old.ai.claude.has_api_token = true;
        old.ai.claude.base_url = Some("https://a.example.com".into());

        // Unchanged address keeps the token; projects cannot be replaced.
        let mut new = old.clone();
        new.projects.clear();
        new.schema_version = 99;
        new.private_paths = vec!["me/finances/".into(), "./me/finances".into(), " ".into()];
        assert!(!new.sanitize_update(&old).unwrap());
        assert!(new.ai.claude.has_api_token);
        assert_eq!(new.projects.len(), 1);
        assert_eq!(new.schema_version, SCHEMA_VERSION);
        assert_eq!(new.private_paths, ["me/finances"]);

        // A changed address drops the token flag.
        let mut new = old.clone();
        new.ai.claude.base_url = Some("https://b.example.com".into());
        assert!(new.sanitize_update(&old).unwrap());
        assert!(!new.ai.claude.has_api_token);

        let bad = |f: &dyn Fn(&mut Settings)| { let mut n = old.clone(); f(&mut n); n.sanitize_update(&old).is_err() };
        assert!(bad(&|n| n.ai.claude.base_url = Some("http://evil.example.com".into())));
        assert!(bad(&|n| n.ai.claude.permission_mode = Some("bypassPermissions".into())));
        assert!(bad(&|n| n.search.extension_repo = "ssh://evil/repo".into()));
        assert!(bad(&|n| n.search.extension_repo = "--upload-pack=evil".into()));
        assert!(bad(&|n| n.private_paths = vec!["../outside".into()]));
        assert!(bad(&|n| n.ai.claude.binary = Some("claude".into())));
        assert!(bad(&|n| n.ai.claude.binary = Some("/bin/sh".into())));
        assert!(bad(&|n| n.ai.claude.binary = Some("/nonexistent/dir/claude".into())));
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
    }
}
