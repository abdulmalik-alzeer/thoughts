use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Msg {
    pub id: String,
    /// "user" | "assistant" | "tool" | "tool_result" | "system" | "error"
    pub role: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    pub ts: DateTime<Utc>,
}

impl Msg {
    pub fn new(role: &str, text: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: role.into(),
            text: text.into(),
            tool: None,
            attachments: vec![],
            ts: Utc::now(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct RunOptions {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub title: String,
    /// None = the library itself.
    pub project_id: Option<String>,
    /// "chat" | "wrap" | "customize" | "setup" | "module" | "reorganize"
    pub kind: String,
    pub provider: String,
    /// Provider-side session id for resume.
    pub session_id: Option<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub messages: Vec<Msg>,
    pub options: RunOptions,
    pub mini_wrap: Option<String>,
    pub mini_wrap_at: Option<DateTime<Utc>>,
    pub wrapped_at: Option<DateTime<Utc>>,
    /// Has activity since the last mini-wrap.
    pub dirty: bool,
    pub running: bool,
    /// The last run was cut off by a daemon restart and the user has not seen that yet.
    pub interrupted: bool,
    pub cost_usd: f64,
    /// Kind-specific data (wrap: chat ids; reorganize: branches).
    pub meta: Value,
}

impl Default for Chat {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title: "New chat".into(),
            project_id: None,
            kind: "chat".into(),
            provider: "claude".into(),
            session_id: None,
            created: now,
            updated: now,
            messages: vec![],
            options: RunOptions::default(),
            mini_wrap: None,
            mini_wrap_at: None,
            wrapped_at: None,
            dirty: false,
            running: false,
            interrupted: false,
            cost_usd: 0.0,
            meta: Value::Null,
        }
    }
}

impl Chat {
    /// Chats that count toward the next Wrap.
    pub fn is_unwrapped(&self) -> bool {
        self.kind == "chat"
            && self.messages.iter().any(|m| m.role == "user")
            && self.wrapped_at.map_or(true, |w| self.updated > w)
    }

    pub fn summary(&self) -> Value {
        serde_json::json!({
            "id": self.id, "title": self.title, "projectId": self.project_id,
            "kind": self.kind, "provider": self.provider, "created": self.created,
            "updated": self.updated, "running": self.running, "interrupted": self.interrupted,
            "unwrapped": self.is_unwrapped(), "hasMiniWrap": self.mini_wrap.is_some(),
            "messageCount": self.messages.len(),
        })
    }

    /// Plain-text transcript, newest content kept when truncating.
    pub fn transcript(&self, max_chars: usize) -> String {
        let mut out = String::new();
        for m in &self.messages {
            match m.role.as_str() {
                "user" => out.push_str(&format!("USER: {}\n\n", m.text)),
                "assistant" => out.push_str(&format!("ASSISTANT: {}\n\n", m.text)),
                "tool" => out.push_str(&format!("[tool] {}\n", m.text)),
                _ => {}
            }
        }
        if out.len() > max_chars {
            let mut start = out.len() - max_chars;
            while !out.is_char_boundary(start) {
                start += 1;
            }
            out = format!("…(earlier conversation truncated)…\n{}", &out[start..]);
        }
        out
    }
}

/// While a chat is streaming its file is rewritten at most this often; it is always written when
/// a run starts and when it finishes.
pub const PERSIST_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(2);

struct DiskState { last: std::time::Instant, dirty: bool }

pub struct ChatStore {
    pub chats: RwLock<HashMap<String, Chat>>,
    dir: std::path::PathBuf,
    /// Chat files that could not be read at startup. They are left on disk and skipped.
    pub unreadable: usize,
    disk: std::sync::Mutex<HashMap<String, DiskState>>,
    /// Where save failures are announced (set once by the app state).
    pub events: std::sync::OnceLock<tokio::sync::broadcast::Sender<Value>>,
    last_error_event: std::sync::Mutex<Option<std::time::Instant>>,
}

impl ChatStore {
    pub fn load() -> anyhow::Result<Self> { Ok(Self::load_from(&crate::paths::chats_dir())) }

    /// Load every chat file in `dir`. A chat still marked `running` belongs to a run that died with
    /// the previous daemon: it is marked `interrupted` and written back. Unreadable files are
    /// skipped, logged and counted, never deleted.
    pub fn load_from(dir: &std::path::Path) -> Self {
        let mut map = HashMap::new();
        let mut unreadable = 0;
        let mut cut_off = vec![];
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map_or(true, |x| x != "json") { continue; }
                let parsed = std::fs::read_to_string(&p).map_err(anyhow::Error::from)
                    .and_then(|s| serde_json::from_str::<Chat>(&s).map_err(anyhow::Error::from));
                match parsed {
                    Ok(mut c) => {
                        if c.running {
                            c.running = false;
                            c.interrupted = true;
                            cut_off.push(c.id.clone());
                        }
                        map.insert(c.id.clone(), c);
                    }
                    Err(err) => {
                        unreadable += 1;
                        crate::log::error(format!("chat file {} could not be read and was skipped: {err}", p.display()));
                    }
                }
            }
        }
        let store = Self { chats: RwLock::new(map), dir: dir.to_path_buf(), unreadable, disk: Default::default(),
            events: Default::default(), last_error_event: Default::default() };
        if let Ok(m) = store.chats.try_read() {
            for id in &cut_off {
                if let Some(c) = m.get(id) {
                    crate::log::warn(format!("chat {id} was cut off by a restart"));
                    store.persist_now(c);
                }
            }
        }
        store
    }

    fn write(dir: &std::path::Path, c: &Chat) -> std::io::Result<()> {
        let text = serde_json::to_vec(c).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(&dir.join(format!("{}.json", c.id)), &text, true)
    }

    /// Write the chat now. A failure is logged, announced (`chat.persistError`) and retried by `flush`.
    fn persist_now(&self, c: &Chat) {
        let res = Self::write(&self.dir, c);
        let mut disk = self.disk.lock().unwrap_or_else(|e| e.into_inner());
        disk.insert(c.id.clone(), DiskState { last: std::time::Instant::now(), dirty: res.is_err() });
        drop(disk);
        if let Err(e) = res {
            crate::log::error(format!("chat {} could not be saved: {e}", c.id));
            let mut last = self.last_error_event.lock().unwrap_or_else(|e| e.into_inner());
            if last.map_or(true, |t| t.elapsed() > std::time::Duration::from_secs(30)) {
                *last = Some(std::time::Instant::now());
                if let Some(tx) = self.events.get() {
                    let _ = tx.send(serde_json::json!({"type": "chat.persistError", "chatId": c.id, "error": e.to_string()}));
                }
            }
        }
    }

    /// Write out chats whose latest changes are only in memory. `force` ignores the debounce.
    pub async fn flush(&self, force: bool) {
        let ids: Vec<String> = self.disk.lock().unwrap_or_else(|e| e.into_inner()).iter()
            .filter(|(_, d)| d.dirty && (force || d.last.elapsed() >= PERSIST_DEBOUNCE)).map(|(id, _)| id.clone()).collect();
        if ids.is_empty() { return; }
        let chats = self.chats.read().await;
        for id in ids {
            match chats.get(&id) {
                Some(c) => self.persist_now(c),
                None => { self.disk.lock().unwrap_or_else(|e| e.into_inner()).remove(&id); }
            }
        }
    }

    pub async fn get(&self, id: &str) -> Option<Chat> {
        self.chats.read().await.get(id).cloned()
    }

    pub async fn insert(&self, c: Chat) {
        let mut w = self.chats.write().await;
        self.persist_now(&c);
        w.insert(c.id.clone(), c);
    }

    /// Mutate a chat in place. It is saved right away, except while a run is streaming into it,
    /// when saves are spaced `PERSIST_DEBOUNCE` apart (`flush` picks up the rest).
    pub async fn update<F: FnOnce(&mut Chat)>(&self, id: &str, f: F) -> Option<Chat> {
        let mut w = self.chats.write().await;
        let c = w.get_mut(id)?;
        let was_running = c.running;
        f(c);
        let streaming = was_running && c.running;
        let due = self.disk.lock().unwrap_or_else(|e| e.into_inner()).get(id).map_or(true, |d| d.last.elapsed() >= PERSIST_DEBOUNCE);
        if !streaming || due {
            self.persist_now(c);
        } else if let Some(d) = self.disk.lock().unwrap_or_else(|e| e.into_inner()).get_mut(id) {
            d.dirty = true;
        }
        Some(c.clone())
    }

    pub async fn remove(&self, id: &str) {
        self.chats.write().await.remove(id);
        self.disk.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        let _ = std::fs::remove_file(self.dir.join(format!("{id}.json")));
        let _ = std::fs::remove_dir_all(crate::paths::attachments_dir().join(id));
    }

    pub async fn list(&self) -> Vec<Chat> {
        let mut v: Vec<Chat> = self.chats.read().await.values().cloned().collect();
        v.sort_by(|a, b| b.updated.cmp(&a.updated));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-chats-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn on_disk(d: &std::path::Path, id: &str) -> Chat {
        serde_json::from_str(&std::fs::read_to_string(d.join(format!("{id}.json"))).unwrap()).unwrap()
    }

    #[test]
    fn startup_marks_cut_off_runs_and_counts_unreadable_files() {
        let d = dir();
        std::fs::write(d.join("a.json"), r#"{"id":"a","title":"Plan trip","running":true}"#).unwrap();
        std::fs::write(d.join("b.json"), r#"{"id":"b","title":"Done","running":false}"#).unwrap();
        std::fs::write(d.join("c.json.tmp"), r#"{"id":"c","running":true}"#).unwrap();
        std::fs::write(d.join("broken.json"), r#"{"id":"x","title":"#).unwrap();
        let s = ChatStore::load_from(&d);
        assert_eq!(s.unreadable, 1);
        let m = s.chats.try_read().unwrap();
        assert_eq!(m.len(), 2);
        assert!(m["a"].interrupted && !m["a"].running && !m["b"].interrupted);
        // Written back, so the flag survives another restart until it is acknowledged.
        assert!(on_disk(&d, "a").interrupted && !on_disk(&d, "a").running);
        // The damaged file is still there for recovery.
        assert!(d.join("broken.json").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[tokio::test]
    async fn streaming_updates_are_debounced_but_start_and_finish_are_saved() {
        let d = dir();
        let s = ChatStore::load_from(&d);
        let c = Chat::default();
        let id = c.id.clone();
        s.insert(c).await;
        s.update(&id, |c| c.running = true).await;
        assert!(on_disk(&d, &id).running, "the running flag must reach disk at once");
        for i in 0..5 { s.update(&id, |c| c.messages.push(Msg::new("assistant", format!("part {i}")))).await; }
        assert_eq!(on_disk(&d, &id).messages.len(), 0, "streamed messages wait for the debounce");
        s.flush(false).await;
        assert_eq!(on_disk(&d, &id).messages.len(), 0);
        s.flush(true).await;
        assert_eq!(on_disk(&d, &id).messages.len(), 5);
        s.update(&id, |c| c.messages.push(Msg::new("assistant", "last"))).await;
        s.update(&id, |c| c.running = false).await;
        let saved = on_disk(&d, &id);
        assert!(!saved.running && saved.messages.len() == 6);
        // Outside a run every change is saved immediately.
        s.update(&id, |c| c.title = "Renamed".into()).await;
        assert_eq!(on_disk(&d, &id).title, "Renamed");
        s.remove(&id).await;
        assert!(!d.join(format!("{id}.json")).exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[tokio::test]
    async fn a_failed_save_is_announced() {
        let d = dir();
        let s = ChatStore::load_from(&d.join("missing-folder"));
        let (tx, mut rx) = tokio::sync::broadcast::channel(4);
        let _ = s.events.set(tx);
        let c = Chat::default();
        let id = c.id.clone();
        s.insert(c).await;
        let ev = rx.try_recv().unwrap();
        assert_eq!(ev["type"], "chat.persistError");
        assert_eq!(ev["chatId"], id);
        // Once the folder exists the pending chat is written by the next flush.
        std::fs::create_dir_all(d.join("missing-folder")).unwrap();
        s.flush(true).await;
        assert!(d.join("missing-folder").join(format!("{id}.json")).exists());
        std::fs::remove_dir_all(d).unwrap();
    }
}
