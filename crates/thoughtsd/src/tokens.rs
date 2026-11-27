//! Access keys (FEAT-37): keys the user makes in Settings for a script, a shortcut or another
//! program on this computer, each good for one purpose only.
//!
//! The app's own key (`AppState::token`, in daemon.json) can do everything the app can: start the
//! assistant with permissions bypassed, change settings, trust a folder. A program that only adds
//! todos should never hold that. An access key carries one or both of two scopes:
//! - `capture`: add a todo or a quick note (`POST /api/capture`) and nothing else;
//! - `read`: read the library (the file list, one file, search, ideas, todos), with private
//!   folders closed exactly as they are for an add-on.
//!
//! What a key can reach is the fixed list `ROUTES` (plus `GET /api/ping`, to see that it works).
//! Everything else — settings, chats, trust, add-ons, git, the events stream, these keys
//! themselves — answers "not allowed", and no key is accepted from a web page (a request that
//! carries an `Origin`). A key is shown once, when it is made: only its SHA-256 is kept
//! (`~/.thoughts/tokens.json`), so that file never holds a key anyone could use. Removing a key
//! stops it at once.
//!
//! How a program finds the app without sending its key to a stranger: the port is in
//! `~/.thoughts/port` (no key in it), and `GET /api/hello?nonce=…&key=<id>` answers with
//! `keyProof`, the HMAC of the challenge under the key's SHA-256 — which only the app that made the
//! key can give (`hello.rs`). `scripts/thoughts.sh` does all of this.

use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::routing::{delete, get};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Every access key starts with this, so it is recognised (and kept out of logs, `log::redact`).
pub const PREFIX: &str = "thoughts_";
const ID_LEN: usize = 8;
const SECRET_LEN: usize = 40;
/// More than anyone needs; keeps the file and the list small.
pub const MAX_KEYS: usize = 50;
const NAME_MAX: usize = 60;
/// "Last used" is written down at most this often per key, not on every request.
const LAST_USED_EVERY: i64 = 60;

/// What a key may do.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Add a todo or a quick note.
    Capture,
    /// Read the library, private folders left out.
    Read,
    /// Written by another version of the app: allows nothing here.
    #[serde(other)]
    Unknown,
}

/// Each request an access key can make: method, path, the scope it needs, and the parameters the
/// route reads. Anything else in the address is dropped before the route sees it.
const ROUTES: &[(&str, &str, Scope, &[&str])] = &[
    ("POST", "/api/capture", Scope::Capture, &[]),
    ("GET", "/api/library/tree", Scope::Read, &[]),
    ("GET", "/api/library/file", Scope::Read, &["path"]),
    ("GET", "/api/library/search", Scope::Read, &["q"]),
    ("GET", "/api/library/ideas", Scope::Read, &[]),
    ("GET", "/api/todos", Scope::Read, &[]),
];

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct Key {
    id: String,
    name: String,
    scopes: Vec<Scope>,
    /// SHA-256 of the whole key, as hex. The key itself is never kept.
    hash: String,
    created: DateTime<Utc>,
    #[serde(default)]
    last_used: Option<DateTime<Utc>>,
}

impl Key {
    /// What the app shows: everything but the hash.
    fn shown(&self) -> Value {
        json!({"id": self.id, "name": self.name, "scopes": self.scopes, "created": self.created, "lastUsed": self.last_used})
    }
}

/// Who a request with a valid access key comes from (put on the request for the routes).
#[derive(Clone, Debug)]
pub struct Caller {
    pub id: String,
    pub scopes: Vec<Scope>,
}

/// The access keys, as kept in `tokens.json`.
pub struct Store {
    file: PathBuf,
    keys: Mutex<Vec<Key>>,
}

fn hash_of(key: &str) -> String { format!("{:x}", Sha256::digest(key.as_bytes())) }

/// Compare without leaking, through timing, how much matched.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The id inside a key, when the text has the shape of one: `thoughts_<8 hex>_<40 letters or digits>`.
fn id_of(key: &str) -> Option<&str> {
    let (id, secret) = key.strip_prefix(PREFIX)?.split_once('_')?;
    let ok = id.len() == ID_LEN && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        && secret.len() == SECRET_LEN && secret.bytes().all(|b| b.is_ascii_alphanumeric());
    ok.then_some(id)
}

impl Store {
    /// The keys on file. A file that cannot be read gives no keys (none of them works) and is
    /// replaced the next time a key is made or removed.
    pub fn load(file: PathBuf) -> Store {
        let keys = match std::fs::read(&file) {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| { crate::log::warn(format!("the access keys could not be read ({e}); none of them will work")); vec![] }),
            Err(_) => vec![],
        };
        Store { file, keys: Mutex::new(keys) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Key>> { self.keys.lock().unwrap_or_else(|e| e.into_inner()) }

    /// Written while the list is held, so two changes never undo each other.
    fn save(&self, keys: &[Key]) -> std::io::Result<()> {
        let text = serde_json::to_vec_pretty(keys).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(&self.file, &text, true)
    }

    /// Every key, newest first, without its hash.
    pub fn list(&self) -> Vec<Value> {
        let keys = self.lock();
        let mut out: Vec<&Key> = keys.iter().collect();
        out.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.name.cmp(&b.name)));
        out.into_iter().map(Key::shown).collect()
    }

    /// Make a key named `name` for `scopes`. Returns what is shown in the list, and the key itself
    /// — the only time it exists anywhere outside the program it is given to.
    pub fn create(&self, name: &str, scopes: &[Scope]) -> anyhow::Result<(Value, String)> {
        use rand::Rng;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > NAME_MAX || name.chars().any(char::is_control) { return Err(crate::tr::err("key.name", &[])); }
        let mut scopes: Vec<Scope> = scopes.iter().copied().filter(|s| *s != Scope::Unknown).collect();
        scopes.sort();
        scopes.dedup();
        if scopes.is_empty() { return Err(crate::tr::err("key.scopes", &[])); }
        let mut keys = self.lock();
        if keys.len() >= MAX_KEYS { return Err(crate::tr::err("key.too_many", &[("n", &MAX_KEYS.to_string())])); }
        if keys.iter().any(|k| k.name.to_lowercase() == name.to_lowercase()) { return Err(crate::tr::err("key.name_taken", &[])); }
        let mut rng = rand::thread_rng();
        let id = loop {
            let id: String = (0..ID_LEN).map(|_| char::from_digit(rng.gen_range(0..16), 16).unwrap()).collect();
            if !keys.iter().any(|k| k.id == id) { break id; }
        };
        let secret: String = (&mut rng).sample_iter(&rand::distributions::Alphanumeric).take(SECRET_LEN).map(char::from).collect();
        let full = format!("{PREFIX}{id}_{secret}");
        let key = Key { id, name: name.to_string(), scopes, hash: hash_of(&full), created: Utc::now(), last_used: None };
        keys.push(key.clone());
        if let Err(e) = self.save(&keys) { keys.pop(); return Err(e.into()); }
        Ok((key.shown(), full))
    }

    /// Remove key `id`: anything that uses it stops working at once.
    pub fn remove(&self, id: &str) -> anyhow::Result<()> {
        let mut keys = self.lock();
        let Some(i) = keys.iter().position(|k| k.id == id) else { return Err(crate::tr::err("key.gone", &[])) };
        let gone = keys.remove(i);
        if let Err(e) = self.save(&keys) { keys.insert(i, gone); return Err(e.into()); }
        Ok(())
    }

    /// The key a request presents, if it is one of these. Notes when it was last used.
    pub fn check(&self, presented: &str) -> Option<Caller> {
        let id = id_of(presented)?;
        let want = hash_of(presented);
        let mut keys = self.lock();
        let k = keys.iter_mut().find(|k| k.id == id)?;
        if !same(&k.hash, &want) { return None; }
        let caller = Caller { id: k.id.clone(), scopes: k.scopes.clone() };
        let now = Utc::now();
        if k.last_used.is_none_or(|t| (now - t).num_seconds() >= LAST_USED_EVERY) {
            k.last_used = Some(now);
            crate::log::ok_or_warn("could not note when an access key was used", self.save(&keys));
        }
        Some(caller)
    }

    /// What `/api/hello` proves knowledge of for key `id`: its SHA-256 (see the module notes).
    pub fn proof_key(&self, id: &str) -> Option<String> {
        self.lock().iter().find(|k| k.id == id).map(|k| k.hash.clone())
    }
}

/// The request a program with key `caller` may make of `method` `uri`: the same path with only the
/// parameters its route reads — plus `scope=plugin` for reading, so private folders stay closed
/// just as they do for an add-on (`api::scope_of`, `api::plugin_gate`). `None` when the key may not
/// make it. Parameters are matched as written: one spelled any other way is not a parameter the
/// route reads, and is dropped.
pub fn admit(caller: &Caller, method: &Method, uri: &Uri) -> Option<Uri> {
    let path = uri.path();
    if method == Method::GET && path == "/api/ping" { return Some(Uri::from_static("/api/ping")); }
    let (_, _, scope, keep) = ROUTES.iter().find(|(m, p, _, _)| *m == method.as_str() && *p == path)?;
    if !caller.scopes.contains(scope) { return None; }
    let mut query: Vec<&str> = uri.query().unwrap_or("").split('&').filter(|kv| {
        let name = kv.split_once('=').map_or(*kv, |(k, _)| k);
        keep.contains(&name)
    }).collect();
    if *scope == Scope::Read { query.push("scope=plugin"); }
    let to = if query.is_empty() { path.to_string() } else { format!("{path}?{}", query.join("&")) };
    to.parse().ok()
}

// ---------- the routes Settings uses (the app's own key only: no access key reaches them) ----------

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/access-keys", get(api_list).post(api_create))
        .route("/api/access-keys/:id", delete(api_remove))
}

type R = Result<Json<Value>, crate::api::ApiErr>;

async fn api_list(State(st): State<Arc<AppState>>) -> R { Ok(Json(json!(st.keys.list()))) }

#[derive(Deserialize)]
struct NewKey { name: String, scopes: Vec<Scope> }

/// `POST /api/access-keys {name, scopes}` → `{key: {id, name, scopes, created, lastUsed}, secret}`.
async fn api_create(State(st): State<Arc<AppState>>, Json(b): Json<NewKey>) -> R {
    let (key, secret) = st.keys.create(&b.name, &b.scopes)?;
    crate::log::info(format!("an access key was made ({}, {})", key["id"].as_str().unwrap_or_default(), key["scopes"]));
    Ok(Json(json!({"key": key, "secret": secret})))
}

async fn api_remove(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> R {
    st.keys.remove(&id)?;
    crate::log::info(format!("an access key was removed ({})", id.chars().filter(char::is_ascii_hexdigit).take(ID_LEN).collect::<String>()));
    Ok(Json(json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Store, PathBuf) {
        let d = std::env::temp_dir().join(format!("thoughts-keys-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        (Store::load(d.join("tokens.json")), d)
    }

    #[test]
    fn a_key_is_shown_once_and_only_its_hash_is_kept() {
        let (s, d) = store();
        let (shown, key) = s.create("  Raycast  ", &[Scope::Capture, Scope::Capture, Scope::Unknown]).unwrap();
        assert!(id_of(&key).is_some(), "{key}");
        assert_eq!((shown["name"].as_str(), shown["scopes"].clone()), (Some("Raycast"), json!(["capture"])));
        assert!(shown.get("hash").is_none() && !shown.to_string().contains(&key[PREFIX.len() + ID_LEN + 1..]));
        let file = std::fs::read_to_string(d.join("tokens.json")).unwrap();
        assert!(!file.contains(&key[PREFIX.len() + ID_LEN + 1..]) && file.contains(&hash_of(&key)), "{file}");
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(d.join("tokens.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // It works, and keeps working after a restart (the file is read again).
        assert_eq!(s.check(&key).unwrap().scopes, [Scope::Capture]);
        let again = Store::load(d.join("tokens.json"));
        assert!(again.check(&key).is_some());
        assert!(again.list()[0]["lastUsed"].is_string(), "last use is noted");
        // A key with one letter changed, another key's id, the app's own kind of key, or nothing.
        let mut wrong = key.clone().into_bytes();
        let last = wrong.len() - 1;
        wrong[last] = if wrong[last] == b'a' { b'b' } else { b'a' };
        for bad in [String::from_utf8(wrong).unwrap(), format!("{PREFIX}00000000_{}", &key[PREFIX.len() + ID_LEN + 1..]), "test-token".into(), String::new(), key.to_uppercase()] {
            assert!(again.check(&bad).is_none(), "{bad}");
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn names_scopes_and_removal() {
        let (s, d) = store();
        assert_eq!(s.create("", &[Scope::Read]).unwrap_err().to_string(), crate::tr::err("key.name", &[]).to_string());
        assert!(s.create(&"x".repeat(61), &[Scope::Read]).is_err());
        assert!(s.create("a\nb", &[Scope::Read]).is_err());
        assert_eq!(s.create("Phone", &[]).unwrap_err().to_string(), crate::tr::err("key.scopes", &[]).to_string());
        assert!(s.create("Phone", &[Scope::Unknown]).is_err(), "an unknown scope allows nothing, so it is no scope");
        let (a, key_a) = s.create("Phone", &[Scope::Read, Scope::Capture]).unwrap();
        assert_eq!(a["scopes"], json!(["capture", "read"]));
        assert_eq!(s.create("phone", &[Scope::Read]).unwrap_err().to_string(), crate::tr::err("key.name_taken", &[]).to_string());
        let (_, key_b) = s.create("Laptop script", &[Scope::Read]).unwrap();
        assert_eq!(s.list().len(), 2);
        // Removing one stops it at once; the other keeps working; removing it twice says so.
        s.remove(a["id"].as_str().unwrap()).unwrap();
        assert!(s.check(&key_a).is_none() && s.check(&key_b).is_some());
        assert_eq!(s.remove(a["id"].as_str().unwrap()).unwrap_err().to_string(), crate::tr::err("key.gone", &[]).to_string());
        assert!(Store::load(d.join("tokens.json")).check(&key_a).is_none(), "the removal is on file");
        // There is a limit.
        for i in 0..MAX_KEYS - 1 { s.create(&format!("k{i}"), &[Scope::Read]).unwrap(); }
        assert!(s.create("one too many", &[Scope::Read]).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_damaged_file_gives_no_working_keys() {
        let (s, d) = store();
        let (_, key) = s.create("x", &[Scope::Read]).unwrap();
        std::fs::write(d.join("tokens.json"), "{ not json").unwrap();
        let broken = Store::load(d.join("tokens.json"));
        assert!(broken.check(&key).is_none() && broken.list().is_empty());
        // A key from a newer version with a scope this one does not know grants nothing more.
        let id = id_of(&key).unwrap();
        std::fs::write(d.join("tokens.json"), json!([{"id": id, "name": "x", "scopes": ["everything"], "hash": hash_of(&key), "created": Utc::now()}]).to_string()).unwrap();
        let newer = Store::load(d.join("tokens.json"));
        let c = newer.check(&key).unwrap();
        assert_eq!(c.scopes, [Scope::Unknown]);
        assert!(admit(&c, &Method::GET, &"/api/library/tree".parse().unwrap()).is_none());
        assert!(admit(&c, &Method::POST, &"/api/capture".parse().unwrap()).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_key_reaches_only_its_own_routes_and_reads_as_an_add_on_would() {
        let read = Caller { id: "r".into(), scopes: vec![Scope::Read] };
        let capture = Caller { id: "c".into(), scopes: vec![Scope::Capture] };
        let both = Caller { id: "b".into(), scopes: vec![Scope::Capture, Scope::Read] };
        let to = |c: &Caller, m: Method, u: &str| admit(c, &m, &u.parse().unwrap()).map(|u| u.to_string());
        assert_eq!(to(&read, Method::GET, "/api/library/file?path=a%20b.md&scope=x&allow=ideas&scop%65=y&exclude=private"), Some("/api/library/file?path=a%20b.md&scope=plugin".into()));
        assert_eq!(to(&read, Method::GET, "/api/library/search?q=bank&limit=9"), Some("/api/library/search?q=bank&scope=plugin".into()));
        assert_eq!(to(&read, Method::GET, "/api/library/tree"), Some("/api/library/tree?scope=plugin".into()));
        assert_eq!(to(&read, Method::GET, "/api/todos?scope=other"), Some("/api/todos?scope=plugin".into()));
        assert_eq!(to(&capture, Method::POST, "/api/capture?scope=plugin&allow=x"), Some("/api/capture".into()));
        assert_eq!(to(&capture, Method::GET, "/api/ping?x=1"), Some("/api/ping".into()));
        // Each scope only its own routes; the method counts; nothing else at all.
        assert_eq!(to(&read, Method::POST, "/api/capture"), None);
        assert_eq!(to(&capture, Method::GET, "/api/library/file?path=a.md"), None);
        assert_eq!(to(&read, Method::PUT, "/api/library/file?path=a.md"), None);
        assert_eq!(to(&read, Method::HEAD, "/api/library/file?path=a.md"), None);
        for path in ["/api/settings", "/api/chats", "/api/chats/x/send", "/api/library/trust", "/api/access-keys", "/api/access-keys/abc",
                     "/api/events", "/api/library/raw", "/api/library/history", "/api/library/restore", "/api/search", "/api/status",
                     "/api/layer/plugins", "/api/extensions", "/api/git/sync", "/api/shutdown", "/api/todos/add", "/api/library/new",
                     "/api/todos/", "/api/library/tree/", "/api/routines/save", "/api/diagnostics", "/api/hello"] {
            for m in [Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
                assert_eq!(to(&both, m.clone(), path), None, "{m} {path}");
            }
        }
    }

    #[test]
    fn keys_never_reach_the_log() {
        let (s, d) = store();
        let (_, key) = s.create("x", &[Scope::Read]).unwrap();
        let line = crate::log::redact(&format!("curl -H 'X: {key}' and {key}."));
        assert!(!line.contains(&key[PREFIX.len()..]), "{line}");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// One request to a daemon on a real port: (status, body).
    async fn call(port: u16, method: &str, path: &str, auth: &str, origin: Option<&str>, body: &str) -> (u16, String) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let origin = origin.map(|o| format!("Origin: {o}\r\n")).unwrap_or_default();
        let req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {auth}\r\n{origin}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        let status = out.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        (status, out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default())
    }

    /// FEAT-37 over a real connection: keys are made with the app's own key; a capture key adds
    /// todos and notes and reads nothing; a read key reads with private folders closed (under any
    /// name, and through a link) and adds nothing; neither reaches settings, the bypass switch,
    /// trust, add-ons, chats, the events stream or the keys; a web page cannot use one; the hello
    /// answer proves the app holds a key before the key is sent; a removed key stops at once.
    #[tokio::test]
    async fn over_http_a_key_reaches_only_what_it_was_made_for() {
        let (st, lib) = crate::wraps::testutil::library().await;
        crate::wraps::testutil::write(&lib, "clients/acme/todo.md", "# Todo\n\n- [ ] invoice acme\n");
        crate::wraps::testutil::write(&lib, "clients/acme/notes.md", "zebracode secret\n");
        st.settings.write().await.private_paths = vec!["clients".into()];
        #[cfg(unix)]
        std::os::unix::fs::symlink(lib.join("clients"), lib.join("work")).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = crate::api::router(st.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let make = |name: &str, scopes: Value| json!({"name": name, "scopes": scopes}).to_string();

        let (code, body) = call(port, "POST", "/api/access-keys", "test-token", None, &make("Raycast", json!(["capture"]))).await;
        assert_eq!(code, 200, "{body}");
        let made: Value = serde_json::from_str(&body).unwrap();
        let cap = made["secret"].as_str().unwrap().to_string();
        let (_, body) = call(port, "POST", "/api/access-keys", "test-token", None, &make("Reader", json!(["read"]))).await;
        let made: Value = serde_json::from_str(&body).unwrap();
        let (read, read_id) = (made["secret"].as_str().unwrap().to_string(), made["key"]["id"].as_str().unwrap().to_string());
        let (_, list) = call(port, "GET", "/api/access-keys", "test-token", None, "").await;
        assert!(list.contains("Raycast") && !list.contains(&cap) && !list.contains(&read) && !list.contains("hash"), "{list}");

        // Adding.
        let (code, body) = call(port, "POST", "/api/capture", &cap, None, r#"{"kind":"todo","text":"Buy milk"}"#).await;
        assert_eq!(code, 200, "{body}");
        assert!(crate::wraps::testutil::read(&lib, "TODO.md").contains("- [ ] Buy milk"));
        let (code, body) = call(port, "POST", "/api/capture", &cap, None, r#"{"kind":"note","text":"An idea"}"#).await;
        assert_eq!(code, 200, "{body}");
        let mut bad = vec![r#"{"kind":"todo","text":"x","file":"clients/acme/todo.md"}"#, r#"{"kind":"todo","text":"x","file":"Clients/acme/todo.md"}"#,
                           r#"{"kind":"todo","text":"x","file":"ideas/../clients/acme/todo.md"}"#];
        if cfg!(unix) { bad.push(r#"{"kind":"todo","text":"x","file":"work/acme/todo.md"}"#); }
        for body in bad {
            let (code, answer) = call(port, "POST", "/api/capture", &cap, None, body).await;
            assert!(code == 403 || code == 422, "{body}: {code} {answer}");
        }
        assert_eq!(crate::wraps::testutil::read(&lib, "clients/acme/todo.md"), "# Todo\n\n- [ ] invoice acme\n", "nothing written in the private folder");
        // …while the app itself still may (it is the user's own page).
        assert_eq!(call(port, "POST", "/api/capture", "test-token", None, r#"{"kind":"todo","text":"y","file":"clients/acme/todo.md"}"#).await.0, 200);
        // A private inbox is closed to a key as well.
        st.settings.write().await.private_paths = vec!["clients".into(), "inbox".into()];
        assert_eq!(call(port, "POST", "/api/capture", &cap, None, r#"{"kind":"note","text":"z"}"#).await.0, 403);
        st.settings.write().await.private_paths = vec!["clients".into()];
        // The capture key reads nothing.
        assert_eq!(call(port, "GET", "/api/library/file?path=README.md", &cap, None, "").await.0, 403);
        assert_eq!(call(port, "GET", "/api/todos", &cap, None, "").await.0, 403);

        // Reading, with private folders closed under any name.
        let (code, body) = call(port, "GET", "/api/library/file?path=README.md", &read, None, "").await;
        assert!(code == 200 && body.contains("my library"), "{code} {body}");
        let mut hidden = vec!["clients/acme/notes.md", "Clients/acme/notes.md", "clients/acme/notes.md&scope=x"];
        if cfg!(unix) { hidden.push("work/acme/notes.md"); }
        for p in hidden {
            let (code, body) = call(port, "GET", &format!("/api/library/file?path={p}"), &read, None, "").await;
            assert!(code == 403 && !body.contains("zebracode"), "{p}: {code} {body}");
        }
        for path in ["/api/library/tree", "/api/todos", "/api/library/search?q=zebracode", "/api/library/search?q=invoice", "/api/library/ideas"] {
            let (code, body) = call(port, "GET", path, &read, None, "").await;
            assert!(code == 200 && !body.contains("acme") && !body.contains("zebracode"), "{path}: {code} {body}");
        }
        assert!(call(port, "GET", "/api/todos", &read, None, "").await.1.contains("Buy milk"));
        assert_eq!(call(port, "POST", "/api/capture", &read, None, r#"{"kind":"todo","text":"x"}"#).await.0, 403);

        // Nothing else, for either key: settings and the bypass switch, trust, add-ons, keys, chats…
        for key in [&cap, &read] {
            for (m, p, b) in [("GET", "/api/settings", ""), ("PUT", "/api/settings?confirm=bypass", "{}"), ("POST", "/api/library/trust", r#"{"id":"x"}"#),
                              ("GET", "/api/access-keys", ""), ("POST", "/api/access-keys", &make("more", json!(["read", "capture"]))),
                              ("DELETE", &format!("/api/access-keys/{read_id}"), ""), ("POST", "/api/chats", "{}"), ("GET", "/api/layer", ""),
                              ("POST", "/api/layer/customize", "{}"), ("GET", "/api/layer/storage/x", ""), ("POST", "/api/workflows/run", "{}"),
                              ("POST", "/api/routines/save", "{}"), ("GET", "/api/status", ""), ("GET", "/api/search?q=milk", ""),
                              ("POST", "/api/library/new", "{}"), ("PUT", "/api/library/file?path=README.md", "{}"), ("POST", "/api/todos/add", "{}"),
                              ("GET", "/api/library/raw?path=README.md", ""), ("POST", "/api/shutdown", ""), ("GET", "/api/events", "")] {
                let (code, body) = call(port, m, p, key, None, b).await;
                assert!(code == 403 && body.contains("\"code\":\"forbidden\""), "{m} {p}: {code} {body}");
            }
            // A web page cannot use a key, not even from the app's own origin.
            assert_eq!(call(port, "GET", "/api/ping", key, Some("tauri://localhost"), "").await.0, 401);
            assert_eq!(call(port, "GET", "/api/ping", key, None, "").await.0, 200);
        }
        assert_eq!(st.keys.list().len(), 2, "no key made another");

        // Before it sends the key, a program can make the app prove it holds that key.
        let nonce = crate::hello::new_nonce();
        let r = get_without_key(port, &format!("/api/hello?nonce={nonce}&key={read_id}")).await;
        let v: Value = serde_json::from_str(&r).unwrap();
        assert_eq!(v["keyProof"].as_str(), Some(crate::hello::proof(&hash_of(&read), &nonce).as_str()), "{r}");
        let r = get_without_key(port, &format!("/api/hello?nonce={nonce}&key=00000000")).await;
        assert!(serde_json::from_str::<Value>(&r).unwrap().get("keyProof").is_none(), "{r}");

        // Removed: it stops at once. The app's own key still reaches everything.
        assert_eq!(call(port, "DELETE", &format!("/api/access-keys/{read_id}"), "test-token", None, "").await.0, 200);
        assert_eq!(call(port, "GET", "/api/library/file?path=README.md", &read, None, "").await.0, 401);
        assert_eq!(call(port, "GET", "/api/settings", "test-token", None, "").await.0, 200);
        server.abort();
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    /// A request with no key at all (the hello route needs none).
    async fn get_without_key(port: u16, path: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default()
    }
}
