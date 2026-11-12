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
    /// The title the app itself last gave this chat (from its first message, then from its summary's
    /// topic). While the title is still this, the app may improve it; once the person renames the
    /// chat it no longer is, and the title is theirs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_title: Option<String>,
    /// Kept at the top of the lists, and never removed by the automatic clean-up.
    pub pinned: bool,
    /// Put away: out of the sidebar and the default Chats list, still kept (and still saved by Wrap).
    pub archived: bool,
    /// None = the library itself.
    pub project_id: Option<String>,
    /// "chat" | "wrap" | "customize" | "setup" | "module" | "reorganize"
    pub kind: String,
    pub provider: String,
    /// Provider-side session id for resume.
    pub session_id: Option<String>,
    /// Every provider-side session this chat has had (kept by the store, see `note_session`), so
    /// deleting the chat can also delete what the assistant program itself kept about it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub session_ids: Vec<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub messages: Vec<Msg>,
    pub options: RunOptions,
    pub mini_wrap: Option<String>,
    pub mini_wrap_at: Option<DateTime<Utc>>,
    pub wrapped_at: Option<DateTime<Utc>>,
    /// What earlier wraps already saved from this chat (a summary), so the next one only adds what is new.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrapped_summary: Option<String>,
    /// The library folder this chat belongs to. None = not tied to a library (customising the app).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
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

/// The longest title the app makes up itself.
const TITLE_MAX: usize = 60;

/// A one-line title out of some text: its first line, spaces tidied, cut at a word (with "…") when
/// longer than a title should be. Empty when the text has no words.
pub fn short_title(text: &str) -> String {
    let line = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).find(|l| !l.is_empty()).unwrap_or_default();
    if line.chars().count() <= TITLE_MAX { return line; }
    let cut: String = line.chars().take(TITLE_MAX).collect();
    // Back to the last whole word, unless that would leave almost nothing.
    let at_word = line.chars().nth(TITLE_MAX).map_or(true, char::is_whitespace);
    let head = if at_word { cut.as_str() } else {
        match cut.rfind(' ') { Some(i) if cut[..i].chars().count() >= TITLE_MAX / 3 => &cut[..i], _ => cut.as_str() }
    };
    format!("{}…", head.trim_end_matches(|c: char| c.is_whitespace() || ",.;:-–—".contains(c)))
}

/// The topic line of a summary (`- **Topic:** …`) as a title. `None` when it has none.
pub fn topic_of(summary: &str) -> Option<String> {
    let line = summary.lines().find(|l| {
        let l = l.trim_start_matches(|c: char| c == '-' || c == '*' || c == '•' || c.is_whitespace()).to_lowercase();
        l.starts_with("topic:") || l.starts_with("topic :") || l.starts_with("الموضوع")
    })?;
    let after = line.split_once(':').map(|(_, r)| r)?;
    let clean = after.replace("**", "").replace('`', "");
    let clean = clean.trim().trim_matches(|c: char| c == '"' || c == '“' || c == '”' || c == '.' || c.is_whitespace());
    let title = short_title(clean);
    (title.chars().filter(|c| c.is_alphanumeric()).count() >= 2).then_some(title)
}

/// Everything about a chat except its messages: what lists, the wrap logic and background loops
/// need. Cheap to copy, however long the conversation is.
#[derive(Clone, Debug)]
pub struct ChatMeta {
    pub id: String,
    pub title: String,
    pub pinned: bool,
    pub archived: bool,
    pub project_id: Option<String>,
    pub kind: String,
    pub provider: String,
    pub session_id: Option<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub options: RunOptions,
    pub has_mini_wrap: bool,
    pub wrapped_at: Option<DateTime<Utc>>,
    pub library: Option<String>,
    pub dirty: bool,
    pub running: bool,
    pub interrupted: bool,
    pub cost_usd: f64,
    pub meta: Value,
    pub message_count: usize,
    /// What the person would call messages: their own and the assistant's replies (no tool calls or results).
    pub turn_count: usize,
    /// Has the user said anything in it?
    pub has_user_message: bool,
}

impl ChatMeta {
    /// Still named "New chat" (in either language): nobody has named it, and it has said nothing to name it by.
    pub fn title_is_default(&self) -> bool { matches!(self.title.as_str(), "New chat" | "محادثة جديدة") }

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
            // Their messages and the replies only: tool calls and results are not "messages" to the person.
            "messageCount": self.turn_count,
            "pinned": self.pinned, "archived": self.archived,
            // Where a wrap or reorganize stands (preview, applying, review…), so an open view follows it.
            "phase": self.meta["phase"],
            // Every permission check is off in this chat: lists mark it, not only the chat's own picker.
            "bypass": self.kind == "chat" && self.options.permission_mode.as_deref() == Some(crate::settings::BYPASS),
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
            auto_title: None,
            pinned: false,
            archived: false,
            project_id: None,
            kind: "chat".into(),
            provider: "claude".into(),
            session_id: None,
            session_ids: vec![],
            created: now,
            updated: now,
            messages: vec![],
            options: RunOptions::default(),
            mini_wrap: None,
            mini_wrap_at: None,
            wrapped_at: None,
            wrapped_summary: None,
            library: None,
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
            id: self.id.clone(), title: self.title.clone(), pinned: self.pinned, archived: self.archived, project_id: self.project_id.clone(), kind: self.kind.clone(),
            provider: self.provider.clone(), session_id: self.session_id.clone(), created: self.created, updated: self.updated,
            options: self.options.clone(), has_mini_wrap: self.mini_wrap.is_some(), wrapped_at: self.wrapped_at,
            library: self.library.clone(), dirty: self.dirty, running: self.running, interrupted: self.interrupted, cost_usd: self.cost_usd,
            meta: self.meta.clone(), message_count: self.messages.len(),
            turn_count: self.messages.iter().filter(|m| m.role == "user" || m.role == "assistant").count(),
            has_user_message: self.messages.iter().any(|m| m.role == "user"),
        }
    }

    pub fn summary(&self) -> Value { self.meta_only().summary() }

    /// Is the title still the one the app gave (so the app may replace it), rather than the person's own?
    /// Chats from before this was tracked count as the app's when they still read as the cut-off first line.
    pub fn title_is_auto(&self) -> bool {
        if self.kind != "chat" { return false; }
        if self.auto_title.as_deref() == Some(self.title.as_str()) { return true; }
        if matches!(self.title.as_str(), "New chat" | "محادثة جديدة") { return true; }
        match self.messages.iter().find(|m| m.role == "user") {
            Some(m) => {
                let first = m.text.lines().next().unwrap_or("");
                self.title == first.chars().take(60).collect::<String>() || self.title == short_title(&m.text)
            }
            None => false,
        }
    }

    /// Give the chat a title of the app's own making.
    pub fn set_auto_title(&mut self, title: String) {
        self.title = title.clone();
        self.auto_title = Some(title);
    }

    /// Remember the current session among the ones this chat has had.
    fn note_session(&mut self) {
        if let Some(s) = &self.session_id {
            if !self.session_ids.iter().any(|x| x == s) { self.session_ids.push(s.clone()); }
        }
    }

    /// Every session this chat is known to have had, the current one included.
    pub fn all_sessions(&self) -> Vec<String> {
        let mut all = self.session_ids.clone();
        if let Some(s) = &self.session_id { if !all.contains(s) { all.push(s.clone()); } }
        all
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
    /// Chats for which a message is being turned into a run right now (see `claim_send`).
    sending: std::sync::Mutex<std::collections::HashSet<String>>,
    /// chat id -> the answer in progress (see `Partial`).
    partial: std::sync::Mutex<HashMap<String, Partial>>,
}

/// The part of an answer the assistant has written so far but not yet completed as a message. It
/// lives in memory only, for a view that opens (or reconnects) in the middle of an answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Partial {
    /// The text of the message being written.
    pub text: String,
    /// How many pieces of text this run has produced so far (`text` holds the ones since the last
    /// completed message). Each `chat.delta` event carries its number, so a view can tell which
    /// pieces a copy fetched from here already includes.
    pub seq: u64,
    /// The latest of the assistant's reasoning, as shown in the "thinking" line.
    pub thinking: String,
}

/// Proof that this request is the only one starting a run for a chat; given back when dropped.
pub struct SendClaim<'a> { store: &'a ChatStore, id: String }

impl Drop for SendClaim<'_> {
    fn drop(&mut self) { self.store.sending.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.id); }
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
            events: Default::default(), last_error_event: Default::default(), sending: Default::default(), partial: Default::default() };
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

    /// Reserve the chat for one "send": from accepting a message until its run has started (when
    /// `running` takes over). `None` means another request holds it — a chat's `running` flag is
    /// only set once the assistant has been started, and two sends in that gap would start two
    /// runs that stop each other.
    pub fn claim_send(&self, id: &str) -> Option<SendClaim<'_>> {
        self.sending.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string()).then(|| SendClaim { store: self, id: id.to_string() })
    }

    /// A piece of the answer being written has arrived (`thinking`: of the reasoning shown while
    /// the assistant works). Returns the piece's number within this run.
    pub fn add_partial(&self, id: &str, piece: &str, thinking: bool) -> u64 {
        /// The "thinking" line shows only its last words: no need to keep more than this.
        const THINKING_KEEP: usize = 2000;
        // Not for a chat that has just been deleted while its run was still winding down.
        if self.chats.try_read().map_or(false, |m| !m.contains_key(id)) { return 0; }
        let mut all = self.partial.lock().unwrap_or_else(|e| e.into_inner());
        let p = all.entry(id.to_string()).or_default();
        if thinking {
            p.thinking.push_str(piece);
            if p.thinking.len() > THINKING_KEEP * 2 {
                let mut cut = p.thinking.len() - THINKING_KEEP;
                while !p.thinking.is_char_boundary(cut) { cut += 1; }
                p.thinking.drain(..cut);
            }
        } else {
            p.text.push_str(piece);
            p.seq += 1;
        }
        p.seq
    }

    /// The answer in progress in this chat, if a run has written any of it yet.
    pub fn partial(&self, id: &str) -> Option<Partial> {
        self.partial.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
    }

    /// A message has been added to the chat: what was being written is now that message (the chat
    /// view drops its own copy at the same moments).
    fn settle_partial(&self, id: &str, role: &str) {
        if let Some(p) = self.partial.lock().unwrap_or_else(|e| e.into_inner()).get_mut(id) {
            if role == "assistant" { p.text.clear(); }
            if role != "user" { p.thinking.clear(); }
        }
    }

    pub async fn get(&self, id: &str) -> Option<Chat> {
        self.chats.read().await.get(id).cloned()
    }

    pub async fn insert(&self, mut c: Chat) {
        c.note_session();
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
        let (was_running, had) = (c.running, c.messages.len());
        f(c);
        c.note_session();
        // A run starting or ending leaves nothing half-written; a new message completes what was being written.
        if c.running != was_running { self.partial.lock().unwrap_or_else(|e| e.into_inner()).remove(id); }
        else if c.messages.len() > had { if let Some(m) = c.messages.last() { self.settle_partial(id, &m.role); } }
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
        self.partial.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
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
    async fn the_answer_in_progress_is_kept_until_it_becomes_a_message() {
        let d = dir();
        let s = ChatStore::load_from(&d);
        let c = Chat::default();
        let id = c.id.clone();
        s.insert(c).await;
        assert_eq!(s.partial(&id), None);
        s.update(&id, |c| c.running = true).await;
        // Pieces are numbered through the whole run.
        assert_eq!(s.add_partial(&id, "Thinking about ", true), 0);
        assert_eq!((s.add_partial(&id, "Hel", false), s.add_partial(&id, "lo ", false), s.add_partial(&id, "there", false)), (1, 2, 3));
        assert_eq!(s.partial(&id).unwrap(), Partial { text: "Hello there".into(), seq: 3, thinking: "Thinking about ".into() });
        // A tool call ends the thinking line but not the text; the user's own message ends neither.
        s.update(&id, |c| c.messages.push(Msg::new("user", "hi"))).await;
        assert_eq!(s.partial(&id).unwrap().thinking, "Thinking about ");
        s.update(&id, |c| c.messages.push(Msg::new("tool", "Read: a.md"))).await;
        assert_eq!(s.partial(&id).unwrap(), Partial { text: "Hello there".into(), seq: 3, thinking: String::new() });
        // The completed message replaces the text; the numbering goes on, so an old piece is never taken for a new one.
        s.update(&id, |c| c.messages.push(Msg::new("assistant", "Hello there"))).await;
        assert_eq!(s.partial(&id).unwrap(), Partial { text: String::new(), seq: 3, thinking: String::new() });
        assert_eq!(s.add_partial(&id, "Next", false), 4);
        // An update that adds no message (a title, the cost) leaves it alone.
        s.update(&id, |c| c.title = "Greeting".into()).await;
        assert_eq!(s.partial(&id).unwrap().text, "Next");
        // Nothing is left over when the run ends, and the next run counts from one again.
        s.update(&id, |c| c.running = false).await;
        assert_eq!(s.partial(&id), None);
        s.update(&id, |c| c.running = true).await;
        assert_eq!(s.add_partial(&id, "Again", false), 1);
        // The thinking line never grows without bound, and is cut between characters.
        for _ in 0..3000 { s.add_partial(&id, "فكرة ", true); }
        let t = s.partial(&id).unwrap().thinking;
        assert!(t.len() <= 4100 && t.ends_with("فكرة "), "{}", t.len());
        // It is memory only: the chat file never carries it.
        assert!(!std::fs::read_to_string(d.join(format!("{id}.json"))).unwrap().contains("Again"));
        s.remove(&id).await;
        assert_eq!((s.add_partial(&id, "late", false), s.partial(&id)), (0, None), "a deleted chat keeps nothing");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_chat_is_claimed_by_one_send_at_a_time() {
        let d = dir();
        let s = ChatStore::load_from(&d);
        let first = s.claim_send("a").expect("free");
        assert!(s.claim_send("a").is_none(), "a second send got in while the first was starting its run");
        let other = s.claim_send("b");
        assert!(other.is_some(), "another chat is not held up");
        drop(first);
        assert!(s.claim_send("a").is_some(), "the claim is given back when the send returns, whatever its outcome");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[tokio::test]
    async fn every_session_a_chat_had_is_remembered() {
        let d = dir();
        let s = ChatStore::load_from(&d);
        let c = Chat::default();
        let id = c.id.clone();
        s.insert(c).await;
        assert!(s.get(&id).await.unwrap().all_sessions().is_empty());
        for sid in ["s1", "s1", "s2"] { s.update(&id, |c| c.session_id = Some(sid.into())).await; }
        // A session that can no longer be resumed is dropped as the current one, but not forgotten.
        s.update(&id, |c| c.session_id = None).await;
        assert_eq!(s.get(&id).await.unwrap().all_sessions(), ["s1", "s2"]);
        assert_eq!(on_disk(&d, &id).session_ids, ["s1", "s2"]);
        s.update(&id, |c| c.session_id = Some("s3".into())).await;
        assert_eq!(s.remove(&id).await.unwrap().all_sessions(), ["s1", "s2", "s3"]);
        // A chat saved before the list existed still knows its current session.
        std::fs::write(d.join("old.json"), r#"{"id":"old","sessionId":"s9"}"#).unwrap();
        let s = ChatStore::load_from(&d);
        assert_eq!(s.get("old").await.unwrap().all_sessions(), ["s9"]);
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

    #[test]
    fn titles_are_cut_at_a_word_and_topics_come_from_the_summary() {
        assert_eq!(short_title("hi"), "hi");
        assert_eq!(short_title("\n  todo:   call the bank \nsecond line"), "todo: call the bank");
        assert_eq!(short_title(""), "");
        let long = "Please help me plan the migration of our whole accounting system to the new provider by March";
        let t = short_title(long);
        assert!(t.ends_with('…') && t.chars().count() <= 61 && long.starts_with(t.trim_end_matches('…')), "{t}");
        assert!(!t.trim_end_matches('…').ends_with(' ') && t.trim_end_matches('…').ends_with("accounting"), "cut at a whole word: {t}");
        // Exactly at the limit stays whole; one word with no spaces is cut where it must be.
        assert_eq!(short_title(&"a".repeat(60)), "a".repeat(60));
        assert_eq!(short_title(&"a".repeat(80)).chars().count(), 61);
        assert_eq!(short_title(&"ب".repeat(70)).chars().count(), 61);
        assert_eq!(topic_of("- **Topic:** Planning the shop launch\n- **Decided:** x").as_deref(), Some("Planning the shop launch"));
        assert_eq!(topic_of("**Topic:** \"Quoted.\"").as_deref(), Some("Quoted"));
        assert_eq!(topic_of("Topic: plain").as_deref(), Some("plain"));
        assert_eq!(topic_of("- **الموضوع:** خطة المتجر").as_deref(), Some("خطة المتجر"));
        for none in ["", "- **Decided:** a topic: here", "- **Topic:**", "- **Topic:** …", "no preamble"] { assert_eq!(topic_of(none), None, "{none:?}"); }
    }

    #[test]
    fn the_app_replaces_only_titles_it_made() {
        let mut c = Chat::default();
        assert!(c.title_is_auto(), "a fresh chat");
        c.messages.push(Msg::new("user", "hi there\nmore"));
        c.title = "hi there".into();
        assert!(c.title_is_auto(), "a title from before auto titles were tracked: the first line");
        c.set_auto_title("Shop launch".into());
        assert!(c.title_is_auto());
        // The person renamed it: not the app's any more, even if the app made another one meanwhile.
        c.title = "My shop".into();
        assert!(!c.title_is_auto());
        let mut old = Chat::default();
        old.messages.push(Msg::new("user", "x".repeat(100)));
        old.title = "x".repeat(60);
        assert!(old.title_is_auto());
        old.title = "Taxes 2026".into();
        assert!(!old.title_is_auto());
        // Only ordinary chats are named by the app.
        let w = Chat { kind: "wrap".into(), ..Default::default() };
        assert!(!w.title_is_auto());
    }

    #[test]
    fn messages_are_counted_as_the_person_sees_them() {
        let mut c = Chat::default();
        for (r, t) in [("user", "a"), ("tool", "Read"), ("tool_result", "x"), ("assistant", "b"), ("tool", "Edit"), ("system", "s"), ("assistant", "c")] { c.messages.push(Msg::new(r, t)); }
        let s = c.summary();
        assert_eq!((s["messageCount"].as_u64(), s["pinned"].as_bool(), s["archived"].as_bool()), (Some(3), Some(false), Some(false)));
        assert_eq!(c.meta_only().message_count, 7);
    }
}
