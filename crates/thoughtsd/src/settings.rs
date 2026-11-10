use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
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

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(crate::paths::settings_file())
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .unwrap_or_default()
            .with_defaults()
    }

    fn with_defaults(mut self) -> Self {
        if self.search.file_threshold == 0 {
            self.search.file_threshold = 1500;
        }
        if self.search.extension_repo.is_empty() {
            self.search.extension_repo = "https://github.com/thoughts-app/thoughts-search.git".into();
        }
        if self.wrap_nag_threshold == 0 {
            self.wrap_nag_threshold = 5;
        }
        self
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(crate::paths::settings_file(), serde_json::to_string_pretty(self).unwrap())
    }

    pub fn library(&self) -> Option<std::path::PathBuf> {
        self.library_path.as_ref().map(std::path::PathBuf::from)
    }

    pub fn git_enabled(&self) -> bool {
        self.sync.mode.starts_with("git")
    }
}
