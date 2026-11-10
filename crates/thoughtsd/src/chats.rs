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
            "updated": self.updated, "running": self.running,
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

pub struct ChatStore {
    pub chats: RwLock<HashMap<String, Chat>>,
}

impl ChatStore {
    pub fn load() -> anyhow::Result<Self> {
        let mut map = HashMap::new();
        if let Ok(rd) = std::fs::read_dir(crate::paths::chats_dir()) {
            for e in rd.flatten() {
                if e.path().extension().map_or(false, |x| x == "json") {
                    if let Ok(s) = std::fs::read_to_string(e.path()) {
                        if let Ok(mut c) = serde_json::from_str::<Chat>(&s) {
                            c.running = false; // a previous daemon died mid-run
                            map.insert(c.id.clone(), c);
                        }
                    }
                }
            }
        }
        Ok(Self { chats: RwLock::new(map) })
    }

    pub fn persist(c: &Chat) {
        let p = crate::paths::chats_dir().join(format!("{}.json", c.id));
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_string(c).unwrap()).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }

    pub async fn get(&self, id: &str) -> Option<Chat> {
        self.chats.read().await.get(id).cloned()
    }

    pub async fn insert(&self, c: Chat) {
        Self::persist(&c);
        self.chats.write().await.insert(c.id.clone(), c);
    }

    /// Mutate a chat in place and persist it.
    pub async fn update<F: FnOnce(&mut Chat)>(&self, id: &str, f: F) -> Option<Chat> {
        let mut w = self.chats.write().await;
        let c = w.get_mut(id)?;
        f(c);
        Self::persist(c);
        Some(c.clone())
    }

    pub async fn remove(&self, id: &str) {
        self.chats.write().await.remove(id);
        let _ = std::fs::remove_file(crate::paths::chats_dir().join(format!("{id}.json")));
        let _ = std::fs::remove_dir_all(crate::paths::attachments_dir().join(id));
    }

    pub async fn list(&self) -> Vec<Chat> {
        let mut v: Vec<Chat> = self.chats.read().await.values().cloned().collect();
        v.sort_by(|a, b| b.updated.cmp(&a.updated));
        v
    }
}
