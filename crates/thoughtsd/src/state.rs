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
    /// settings.json was damaged at startup and has been moved aside.
    pub settings_recovered: AtomicBool,
    /// Automatic mini-wraps are paused after a sign-in or network failure, until any run succeeds.
    pub mini_wraps_blocked: AtomicBool,
    /// chat id -> retry state for failed automatic mini-wraps.
    pub mini_wrap_backoff: Mutex<HashMap<String, crate::wraps::Backoff>>,
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
        let (settings, settings_recovered) = Settings::load_or_recover(&crate::paths::settings_file());
        let usage: BTreeMap<String, f64> = match std::fs::read_to_string(crate::paths::usage_file()) {
            Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
            // First run with the ledger: start this month from the chats created in it.
            Err(_) => {
                let now = chrono::Local::now();
                let spent: f64 = chats.chats.try_read().map(|m| m.values()
                    .filter(|c| c.created.with_timezone(&chrono::Local).format("%Y-%m").to_string() == now.format("%Y-%m").to_string())
                    .map(|c| c.cost_usd).sum()).unwrap_or(0.0);
                BTreeMap::from([(month_key(), spent)])
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
            settings_recovered: AtomicBool::new(settings_recovered),
            mini_wraps_blocked: AtomicBool::new(false),
            mini_wrap_backoff: Mutex::new(HashMap::new()),
        })
    }

    pub fn emit(&self, v: Value) {
        let _ = self.events.send(v);
    }

    /// Record what a finished run cost against the current month.
    pub async fn add_usage(&self, cost_usd: f64) {
        if !(cost_usd.is_finite() && cost_usd > 0.0) { return; }
        let mut u = self.usage.lock().await;
        *u.entry(month_key()).or_insert(0.0) += cost_usd;
        if let Ok(text) = serde_json::to_string_pretty(&*u) {
            let _ = crate::paths::write_atomic(&crate::paths::usage_file(), text.as_bytes(), true);
        }
    }

    /// This calendar month's AI spend in USD.
    pub async fn month_cost(&self) -> f64 {
        self.usage.lock().await.get(&month_key()).copied().unwrap_or(0.0)
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
    pub fn note_run_ok(&self) { self.mini_wraps_blocked.store(false, Ordering::Relaxed); }

    pub fn notify(&self, title: &str, body: &str) {
        self.emit(serde_json::json!({"type": "notify", "title": title, "body": body}));
    }
}

