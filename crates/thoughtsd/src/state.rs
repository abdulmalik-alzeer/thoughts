use crate::{chats::ChatStore, settings::Settings};
use rand::Rng;
use serde_json::Value;
use std::collections::HashMap;
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
}

impl AppState {
    pub fn load() -> anyhow::Result<Self> {
        let token: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(40)
            .map(char::from)
            .collect();
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            token,
            settings: RwLock::new(Settings::load()),
            chats: ChatStore::load()?,
            events: tx,
            runs: Mutex::new(HashMap::new()),
            shutdown: Arc::new(Notify::new()),
            git_lock: Mutex::new(()),
        })
    }

    pub fn emit(&self, v: Value) {
        let _ = self.events.send(v);
    }

    pub fn notify(&self, title: &str, body: &str) {
        self.emit(serde_json::json!({"type": "notify", "title": title, "body": body}));
    }
}
