use crate::{chats::ChatStore, settings::Settings};
use rand::Rng;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, Notify, RwLock};

pub struct AppState {
    pub token: String,
    pub settings: RwLock<Settings>,
    pub chats: ChatStore,
    pub events: broadcast::Sender<Value>,
    /// chat id -> kill switch for the running AI process
    pub runs: Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
    pub shutdown: Arc<Notify>,
    /// Serialises git operations on the library.
    pub git_lock: Mutex<()>,
    /// Chats whose run was cut off by a daemon restart: (chat id, title). Cleared when the chat runs again.
    pub interrupted: Mutex<Vec<(String, String)>>,
    /// AI spend per calendar month ("YYYY-MM" -> USD), kept in `usage.json`.
    pub usage: Mutex<BTreeMap<String, f64>>,
    /// Held while a wrap or reorganize is changing the library (apply through commit), so nothing
    /// else syncs or shuts the daemon down halfway.
    pub library_lock: Arc<Mutex<()>>,
    /// Taken for writing by a sync while git changes the library's files (recording them, taking in
    /// what arrived), and for reading by a short edit while it writes, so the two never overlap.
    /// While a sync waits on the network it holds neither, and edits go through (see `api::library_edit`).
    pub sync_files: Arc<RwLock<()>>,
    /// A sync started from the window holds `library_lock` (see `api::sync_guard`).
    pub syncing: AtomicBool,
    /// settings.json was damaged at startup and has been moved aside.
    pub settings_recovered: AtomicBool,
    /// Automatic mini-wraps are paused after a sign-in or network failure, until any run succeeds.
    pub mini_wraps_blocked: AtomicBool,
    /// chat id -> retry state for failed automatic mini-wraps.
    pub mini_wrap_backoff: Mutex<HashMap<String, crate::wraps::Backoff>>,
    /// settings.json was damaged or missing at startup and the backup copy was put back.
    pub settings_restored: AtomicBool,
    /// settings.json was written by a newer version of the app: settings are read-only.
    pub settings_newer: AtomicBool,
    /// chat id -> the folder its assistant run is working in, while it runs.
    pub run_dirs: std::sync::Mutex<HashMap<String, std::path::PathBuf>>,
    /// Stands in for the assistant program (see `claude::RunHook`). Tests set it; the app never does.
    pub run_hook: std::sync::Mutex<Option<crate::claude::RunHook>>,
    /// Stands in for the assistant's one-shot answers (see `claude::CompleteHook`). Tests set it; the app never does.
    pub complete_hook: std::sync::Mutex<Option<crate::claude::CompleteHook>>,
    /// The assistant's last run was refused for lack of a valid sign-in, whatever the CLI's own
    /// status says (a sign-in that expired still reads as "logged in" there). Cleared by any run
    /// that works, a fresh sign-in, or "check again".
    pub auth_failed: AtomicBool,
    /// When the CLI last said "signed in", so a chat does not ask it again before every message.
    pub auth_ok_at: std::sync::Mutex<Option<std::time::Instant>>,
    /// The daemon is stopping: runs that end from here on were cut off by that, not finished.
    pub stopping: AtomicBool,
    /// Ends the sign-in attempt in progress (`claude auth login`), so there is only ever one.
    pub login: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Background summaries in flight (chat ids), plus `"sweep"` while a "new chat" pass is queued
    /// or running, so the same chat is never summarised twice at once.
    pub summarising: std::sync::Mutex<std::collections::HashSet<String>>,
    /// The spending level already announced this month: (month, level).
    pub spend_told: std::sync::Mutex<(String, crate::spend::Level)>,
    /// A limit on the plan the assistant reported, and when it was seen (seconds since 1970).
    pub plan_limit: std::sync::Mutex<Option<(crate::spend::PlanLimit, i64)>>,
}

fn month_key() -> String { chrono::Local::now().format("%Y-%m").to_string() }

impl AppState {
    pub fn load() -> anyhow::Result<Self> {
        let token: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(40)
            .map(char::from)
            .collect();
        let (tx, _) = broadcast::channel(1024);
        let chats = ChatStore::load()?;
        let _ = chats.events.set(tx.clone());
        // Runs cut off by a restart stay listed (the flag is saved in the chat) until acknowledged.
        let mut interrupted: Vec<(String, String)> = chats.chats.try_read().map(|m| m.values()
            .filter(|c| c.interrupted).map(|c| (c.id.clone(), c.title.clone())).collect()).unwrap_or_default();
        interrupted.sort();
        let loaded = Settings::load_full(&crate::paths::settings_file());
        let (settings, settings_recovered) = (loaded.settings, loaded.reset);
        // Never under test: unit tests share one process and one settings file.
        if !cfg!(test) { crate::settings::set_read_only(loaded.newer); }
        crate::tr::set_language(&settings.language);
        let usage: BTreeMap<String, f64> = match std::fs::read_to_string(crate::paths::usage_file()) {
            Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
            // First run with the ledger: start this month from the chats created in it.
            Err(_) => {
                let now = chrono::Local::now();
                let spent: f64 = chats.chats.try_read().map(|m| m.values()
                    .filter(|c| c.created.with_timezone(&chrono::Local).format("%Y-%m").to_string() == now.format("%Y-%m").to_string())
                    .map(|c| c.cost_usd).sum()).unwrap_or(0.0);
                // An empty sum is -0.0, which shows as "-$0.00"; no amount is ever below zero.
                BTreeMap::from([(month_key(), if spent > 0.0 { spent } else { 0.0 })])
            }
        };
        Ok(Self {
            token,
            settings: RwLock::new(settings),
            chats,
            events: tx,
            runs: Mutex::new(HashMap::new()),
            shutdown: Arc::new(Notify::new()),
            git_lock: Mutex::new(()),
            interrupted: Mutex::new(interrupted),
            usage: Mutex::new(usage),
            library_lock: Arc::new(Mutex::new(())),
            sync_files: Default::default(),
            syncing: AtomicBool::new(false),
            settings_recovered: AtomicBool::new(settings_recovered),
            mini_wraps_blocked: AtomicBool::new(false),
            mini_wrap_backoff: Mutex::new(HashMap::new()),
            settings_restored: AtomicBool::new(loaded.restored),
            settings_newer: AtomicBool::new(loaded.newer),
            run_dirs: Default::default(),
            run_hook: Default::default(),
            complete_hook: Default::default(),
            auth_failed: AtomicBool::new(false),
            auth_ok_at: Default::default(),
            stopping: AtomicBool::new(false),
            login: Default::default(),
            summarising: Default::default(),
            spend_told: std::sync::Mutex::new((month_key(), crate::spend::Level::Off)),
            plan_limit: Default::default(),
        })
    }

    /// A state of its own for a unit test: default settings and an empty chat store in a fresh
    /// temp folder, so tests running side by side never see each other's chats.
    #[cfg(test)]
    pub fn for_test() -> Self {
        let (tx, _) = broadcast::channel(1024);
        let dir = std::env::temp_dir().join(format!("thoughts-test-chats-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let chats = ChatStore::load_from(&dir);
        let _ = chats.events.set(tx.clone());
        Self {
            token: "test-token".into(),
            settings: RwLock::new(Settings::default().with_defaults()),
            chats,
            events: tx,
            runs: Mutex::new(HashMap::new()),
            shutdown: Arc::new(Notify::new()),
            git_lock: Mutex::new(()),
            interrupted: Mutex::new(vec![]),
            usage: Mutex::new(BTreeMap::new()),
            library_lock: Arc::new(Mutex::new(())),
            sync_files: Default::default(),
            syncing: AtomicBool::new(false),
            settings_recovered: AtomicBool::new(false),
            mini_wraps_blocked: AtomicBool::new(false),
            mini_wrap_backoff: Mutex::new(HashMap::new()),
            settings_restored: AtomicBool::new(false),
            settings_newer: AtomicBool::new(false),
            run_dirs: Default::default(),
            run_hook: Default::default(),
            complete_hook: Default::default(),
            auth_failed: AtomicBool::new(false),
            auth_ok_at: Default::default(),
            stopping: AtomicBool::new(false),
            login: Default::default(),
            summarising: Default::default(),
            spend_told: std::sync::Mutex::new((month_key(), crate::spend::Level::Off)),
            plan_limit: Default::default(),
        }
    }

    /// Is an assistant run (a chat, a workflow…) working inside `dir` right now?
    pub fn run_active_in(&self, dir: &std::path::Path) -> bool {
        self.run_dirs.lock().unwrap_or_else(|e| e.into_inner()).values().any(|d| d == dir)
    }

    pub fn emit(&self, v: Value) {
        let _ = self.events.send(v);
    }

    /// Record what a finished run cost against the current month.
    pub async fn add_usage(&self, cost_usd: f64) {
        if !(cost_usd.is_finite() && cost_usd > 0.0) { return; }
        {
            let mut u = self.usage.lock().await;
            *u.entry(month_key()).or_insert(0.0) += cost_usd;
            if let Ok(text) = serde_json::to_string_pretty(&*u) {
                let _ = crate::paths::write_atomic(&crate::paths::usage_file(), text.as_bytes(), true);
            }
        }
        // The window shows the new figure, and the spending guard may have something to say.
        crate::spend::after_cost(self).await;
    }

    /// This calendar month's AI spend in USD.
    pub async fn month_cost(&self) -> f64 {
        // `+ 0.0` turns a stored -0.0 (written by 0.0.8 on a fresh install) into 0.
        self.usage.lock().await.get(&month_key()).copied().unwrap_or(0.0).max(0.0) + 0.0
    }

    /// Is a wrap or reorganize changing the library right now?
    pub fn library_busy(&self) -> bool { self.library_lock.try_lock().is_err() }

    /// Forget that a chat's run was cut off (the user has seen it, or the chat is running again).
    pub async fn ack_interrupted(&self, chat_id: &str) {
        self.interrupted.lock().await.retain(|(c, _)| c != chat_id);
        if self.chats.get(chat_id).await.map_or(false, |c| c.interrupted) {
            if let Some(ch) = self.chats.update(chat_id, |c| c.interrupted = false).await {
                self.emit(serde_json::json!({"type": "chat.updated", "chat": ch.summary()}));
            }
        }
    }

    /// A run finished successfully: sign-in and network evidently work again.
    pub fn note_run_ok(&self) {
        self.mini_wraps_blocked.store(false, Ordering::Relaxed);
        // Something went through, so a refusal for the plan's limit is over (a warning stays until its reset).
        if self.plan_limit.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(false, |(l, _)| l.reached) { crate::spend::set_plan_limit(self, None); }
        if self.auth_failed.swap(false, Ordering::Relaxed) { self.emit(serde_json::json!({"type": "auth.changed", "loggedIn": true})); }
    }

    /// A run was refused because the assistant is not (or no longer) signed in.
    pub fn note_auth_failed(&self) {
        *self.auth_ok_at.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if !self.auth_failed.swap(true, Ordering::Relaxed) { self.emit(serde_json::json!({"type": "auth.changed", "loggedIn": false})); }
    }

    /// Forget what is remembered about the sign-in, so the next look asks the CLI afresh.
    pub fn forget_auth(&self) {
        self.auth_failed.store(false, Ordering::Relaxed);
        *self.auth_ok_at.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn notify(&self, title: &str, body: &str) {
        self.emit(serde_json::json!({"type": "notify", "title": title, "body": body}));
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_month_with_nothing_spent_is_zero_never_minus_zero() {
        let st = AppState::for_test();
        // 0.0.8 stored the sum of no chats, which Rust makes -0.0 and the window then showed as "-$0.00".
        st.usage.lock().await.insert(month_key(), -0.0);
        let shown = serde_json::to_string(&st.month_cost().await).unwrap();
        assert_eq!(shown, "0.0");
        assert_eq!(crate::spend::usage_json(&st).await["monthCostUsd"].to_string(), "0.0");
    }
}
