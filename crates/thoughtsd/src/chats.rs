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
    /// Spending ceiling for one run (automatic tasks). Set by the app only: never stored, never read from a client.
    #[serde(skip)]
    pub max_budget_usd: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Chat {
    /// Version of this file's shape, for future migrations.
    pub schema_version: u32,
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

/// Current shape of a chat file.
pub const CHAT_SCHEMA_VERSION: u32 = 1;

/// Is `id` the plain kind of id a chat has: letters, digits and dashes (the app only ever makes
/// UUIDs)? A chat's id becomes part of file and folder names — one of them deleted recursively —
/// so anything else (`..`, a slash, an empty id) must never be joined onto a path.
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Everything about a chat except its messages: what lists, the wrap logic and background loops
/// need. Cheap to copy, however long the conversation is.
#[derive(Clone, Debug)]
pub struct ChatMeta {
    pub id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub kind: String,
    pub provider: String,
    pub session_id: Option<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub options: RunOptions,
    pub has_mini_wrap: bool,
    pub wrapped_at: Option<DateTime<Utc>>,
    pub dirty: bool,
    pub running: bool,
    pub interrupted: bool,
    pub cost_usd: f64,
    pub meta: Value,
    pub message_count: usize,
    /// Has the user said anything in it?
    pub has_user_message: bool,
}

impl ChatMeta {
    /// Chats that count toward the next Wrap.
    pub fn is_unwrapped(&self) -> bool {
        self.kind == "chat" && self.has_user_message && self.wrapped_at.map_or(true, |w| self.updated > w)
    }

    pub fn summary(&self) -> Value {
        serde_json::json!({
            "id": self.id, "title": self.title, "projectId": self.project_id,
            "kind": self.kind, "provider": self.provider, "created": self.created,
            "updated": self.updated, "running": self.running, "interrupted": self.interrupted,
            "unwrapped": self.is_unwrapped(), "hasMiniWrap": self.has_mini_wrap,
            "messageCount": self.message_count,
        })
    }
}

impl Default for Chat {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            schema_version: CHAT_SCHEMA_VERSION,
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

    /// The chat without its messages.
    pub fn meta_only(&self) -> ChatMeta {
        ChatMeta {
            id: self.id.clone(), title: self.title.clone(), project_id: self.project_id.clone(), kind: self.kind.clone(),
            provider: self.provider.clone(), session_id: self.session_id.clone(), created: self.created, updated: self.updated,
            options: self.options.clone(), has_mini_wrap: self.mini_wrap.is_some(), wrapped_at: self.wrapped_at,
            dirty: self.dirty, running: self.running, interrupted: self.interrupted, cost_usd: self.cost_usd,
            meta: self.meta.clone(), message_count: self.messages.len(),
            has_user_message: self.messages.iter().any(|m| m.role == "user"),
        }
    }

    pub fn summary(&self) -> Value { self.meta_only().summary() }

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
                // The id inside the file is what later saves are named after: it must be a plain one.
                let parsed = parsed.and_then(|c| if valid_id(&c.id) { Ok(c) } else { Err(anyhow::anyhow!("its id is not a valid chat id")) });
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
        if !valid_id(&c.id) { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a valid chat id")); }
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
    /// Returns the chat's summary data (never a copy of its messages: this runs for every streamed message).
    pub async fn update<F: FnOnce(&mut Chat)>(&self, id: &str, f: F) -> Option<ChatMeta> {
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
        Some(c.meta_only())
    }

    /// Forget a chat and delete its file and its attachments folder; returns the chat that was
    /// removed. Nothing is touched unless `id` is a plain id (`valid_id`) *and* a chat in this
    /// store: the id becomes part of two paths, and one of them is deleted with everything in it.
    pub async fn remove(&self, id: &str) -> Option<Chat> {
        if !valid_id(id) { return None; }
        let chat = self.chats.write().await.remove(id)?;
        self.disk.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        let _ = std::fs::remove_file(self.dir.join(format!("{id}.json")));
        // Only a real folder: a link placed there is never followed to whatever it points at.
        let files = crate::paths::attachments_dir().join(id);
        if std::fs::symlink_metadata(&files).map_or(false, |m| m.is_dir()) { let _ = std::fs::remove_dir_all(&files); }
        Some(chat)
    }

    /// Every chat, newest first, without messages — safe to call from periodic loops.
    pub async fn list(&self) -> Vec<ChatMeta> {
        let mut v: Vec<ChatMeta> = self.chats.read().await.values().map(Chat::meta_only).collect();
        v.sort_by(|a, b| b.updated.cmp(&a.updated));
        v
    }

    /// One chat without its messages.
    pub async fn meta(&self, id: &str) -> Option<ChatMeta> {
        self.chats.read().await.get(id).map(Chat::meta_only)
    }

    /// Read something out of a chat without copying it.
    pub async fn with<T>(&self, id: &str, f: impl FnOnce(&Chat) -> T) -> Option<T> {
        self.chats.read().await.get(id).map(f)
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

    /// Ids a request could carry to reach outside a chat's own files.
    fn hostile_ids(neighbour: &str) -> Vec<String> {
        let mut ids: Vec<String> = ["", ".", "..", "../", "../..", "..\\..", "/", "\\", "a/b", "a\\b", "./a", "~", "%2e%2e", "..%2F", " ", "a b",
            "a.json", ".hidden", "x\0y", "a\n", "‥", "آ", "C:", "nul.txt"].iter().map(|s| s.to_string()).collect();
        ids.push(format!("../{neighbour}"));
        ids.push(format!("../attachments/../{neighbour}"));
        ids.push(crate::paths::home().join(neighbour).to_string_lossy().to_string());
        ids.push("a".repeat(65));
        ids
    }

    #[test]
    fn only_plain_ids_are_chat_ids() {
        for ok in ["a", "old", "c1", &uuid::Uuid::new_v4().to_string(), &uuid::Uuid::new_v4().simple().to_string(), &"a".repeat(64)] { assert!(valid_id(ok), "{ok}"); }
        for bad in hostile_ids("neighbour") { assert!(!valid_id(&bad), "{bad:?}"); }
    }

    #[tokio::test]
    async fn removing_a_chat_never_reaches_outside_its_own_files() {
        let home = crate::paths::home();
        let d = dir();
        let s = ChatStore::load_from(&d);
        // What a hostile id would point at: a folder next to the app's own, another chat's files, a file among the chats.
        let neighbour = format!("neighbour-{}", uuid::Uuid::new_v4());
        std::fs::create_dir_all(home.join(&neighbour).join("sub")).unwrap();
        std::fs::write(home.join(&neighbour).join("sub/keep.txt"), "keep").unwrap();
        let other = Chat::default();
        let other_files = crate::paths::attachments_dir().join(&other.id);
        std::fs::create_dir_all(&other_files).unwrap();
        std::fs::write(other_files.join("photo.png"), "keep").unwrap();
        let other_id = other.id.clone();
        s.insert(other).await;
        std::fs::write(d.join("notes.json.bak"), "keep").unwrap();
        let intact = || {
            assert!(home.join(&neighbour).join("sub/keep.txt").exists(), "a folder outside the chat was deleted");
            assert!(other_files.join("photo.png").exists() && d.join(format!("{other_id}.json")).exists(), "another chat's files were deleted");
            assert!(d.join("notes.json.bak").exists());
            for sub in ["chats", "attachments", "logs"] { assert!(home.join(sub).is_dir(), "{sub} is gone"); }
        };
        for id in hostile_ids(&neighbour) {
            assert!(s.remove(&id).await.is_none(), "{id:?} was accepted");
            intact();
            // Even if such an id were somehow in the store, it is never turned into a path.
            s.chats.write().await.insert(id.clone(), Chat { id: id.clone(), ..Default::default() });
            assert!(s.remove(&id).await.is_none(), "{id:?} was accepted");
            intact();
            s.chats.write().await.remove(&id);
            // And a chat with such an id is never written anywhere.
            s.insert(Chat { id: id.clone(), ..Default::default() }).await;
            s.chats.write().await.remove(&id);
            intact();
        }
        assert!(!home.join(format!("{neighbour}.json")).exists() && !d.parent().unwrap().join(format!("{neighbour}.json")).exists());
        // A plain id that is not one of the store's chats removes nothing either (`notes.json.bak` is not `notes.json`, but still).
        std::fs::write(d.join("stray.json"), "not a chat of this store").unwrap();
        assert!(s.remove("stray").await.is_none() && d.join("stray.json").exists());
        // A link where a chat's attachments folder would be is not followed.
        #[cfg(unix)]
        {
            let linked = Chat::default();
            let lid = linked.id.clone();
            s.insert(linked).await;
            std::os::unix::fs::symlink(home.join(&neighbour), crate::paths::attachments_dir().join(&lid)).unwrap();
            assert!(s.remove(&lid).await.is_some());
            intact();
            std::fs::remove_file(crate::paths::attachments_dir().join(&lid)).unwrap();
        }
        // The real thing still works: the chat, its file and its attachments go — and only those.
        assert_eq!(s.remove(&other_id).await.map(|c| c.id), Some(other_id.clone()));
        assert!(!other_files.exists() && !d.join(format!("{other_id}.json")).exists() && s.get(&other_id).await.is_none());
        assert!(home.join(&neighbour).join("sub/keep.txt").exists());
        std::fs::remove_dir_all(home.join(&neighbour)).unwrap();
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_chat_file_with_a_hostile_id_is_skipped_at_startup() {
        let d = dir();
        std::fs::write(d.join("evil.json"), r#"{"id":"../../outside","title":"x","running":true}"#).unwrap();
        std::fs::write(d.join("empty.json"), r#"{"id":"","title":"x"}"#).unwrap();
        std::fs::write(d.join("fine.json"), r#"{"id":"fine","title":"ok"}"#).unwrap();
        let s = ChatStore::load_from(&d);
        assert_eq!(s.unreadable, 2);
        assert_eq!(s.chats.try_read().unwrap().keys().collect::<Vec<_>>(), ["fine"]);
        // Nothing was written back under the hostile name, and the files are left for recovery.
        assert!(!d.parent().unwrap().parent().unwrap().join("outside.json").exists());
        assert!(d.join("evil.json").exists() && d.join("empty.json").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[tokio::test]
    async fn lists_carry_no_messages_and_files_carry_a_version() {
        let d = dir();
        // A file written before `schemaVersion` existed still loads, and gets the field on its next save.
        std::fs::write(d.join("old.json"), r#"{"id":"old","title":"Old","messages":[{"id":"m","role":"user","text":"hi","ts":"2026-01-01T00:00:00Z"}]}"#).unwrap();
        let s = ChatStore::load_from(&d);
        let mut c = Chat::default();
        c.messages.push(Msg::new("user", "x".repeat(10_000)));
        c.mini_wrap = Some("summary".into());
        let id = c.id.clone();
        s.insert(c).await;
        let list = s.list().await;
        assert_eq!(list.len(), 2);
        let m = list.iter().find(|m| m.id == id).unwrap();
        assert!(m.has_user_message && m.has_mini_wrap && m.message_count == 1 && m.is_unwrapped());
        assert_eq!(m.summary(), s.get(&id).await.unwrap().summary());
        assert_eq!(s.meta("old").await.unwrap().message_count, 1);
        assert_eq!(s.with(&id, |c| c.messages[0].text.len()).await, Some(10_000));
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(d.join(format!("{id}.json"))).unwrap()).unwrap();
        assert_eq!(raw["schemaVersion"], CHAT_SCHEMA_VERSION);
        s.update("old", |c| c.title = "Renamed".into()).await.unwrap();
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(d.join("old.json")).unwrap()).unwrap();
        assert_eq!((raw["schemaVersion"].as_u64(), raw["title"].as_str()), (Some(1), Some("Renamed")));
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
