use crate::{chats::ChatStore, settings::Settings};
use rand::Rng;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
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
}

fn month_key() -> String { chrono::Local::now().format("%Y-%m").to_string() }

/// Chat files still marked `running` belong to runs that died with the previous daemon.
fn find_interrupted(dir: &std::path::Path) -> Vec<(String, String)> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().extension().map_or(true, |x| x != "json") { continue; }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        if v["running"] == true {
            if let Some(id) = v["id"].as_str() {
                out.push((id.to_string(), v["title"].as_str().unwrap_or_default().to_string()));
            }
        }
    }
    out
}

impl AppState {
    pub fn load() -> anyhow::Result<Self> {
        let token: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(40)
            .map(char::from)
            .collect();
        let (tx, _) = broadcast::channel(1024);
        let interrupted = find_interrupted(&crate::paths::chats_dir());
        let chats = ChatStore::load()?;
        // Write the cleared `running` flag back so the next restart doesn't report these again.
        for (id, _) in &interrupted {
            if let Some(c) = chats.chats.try_read().ok().and_then(|m| m.get(id).cloned()) { ChatStore::persist(&c); }
        }
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
            settings: RwLock::new(Settings::load()),
            chats,
            events: tx,
            runs: Mutex::new(HashMap::new()),
            shutdown: Arc::new(Notify::new()),
            git_lock: Mutex::new(()),
            interrupted: Mutex::new(interrupted),
            usage: Mutex::new(usage),
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

    pub fn notify(&self, title: &str, body: &str) {
        self.emit(serde_json::json!({"type": "notify", "title": title, "body": body}));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_chats_cut_off_mid_run() {
        let d = std::env::temp_dir().join(format!("thoughts-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.json"), r#"{"id":"a","title":"Plan trip","running":true}"#).unwrap();
        std::fs::write(d.join("b.json"), r#"{"id":"b","title":"Done","running":false}"#).unwrap();
        std::fs::write(d.join("c.json.tmp"), r#"{"id":"c","running":true}"#).unwrap();
        assert_eq!(super::find_interrupted(&d), [("a".to_string(), "Plan trip".to_string())]);
        std::fs::remove_dir_all(d).unwrap();
    }
}
