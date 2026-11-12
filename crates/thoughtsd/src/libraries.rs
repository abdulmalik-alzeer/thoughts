//! Several libraries (FEAT-32): a work library and a personal one, one per client or employer.
//!
//! The list lives in `Settings::libraries`; the open library's own settings are the top-level
//! fields of `Settings` (everything reads them there), each other library's wait in its entry
//! (`settings::LibrarySettings`). Chats carry the library they were started in (`Chat::library`,
//! see `wraps::Scope`), trust and history are kept per folder already.
//!
//! Switching is allowed while a chat or a Wrap is still at work: that work goes on in the library
//! it belongs to — everything it writes is addressed by that library's folder, never by "the open
//! one" (see `claude::start_run`, `wraps`, `history::record_saved`, `workflows`). A window that
//! still shows the library that was open before sends its id along (`x-thoughts-library`), and a
//! change it asks for is refused (`gate`) rather than written into the library opened since.

use crate::api::ApiErr;
use crate::settings::{Library, Settings};
use crate::state::AppState;
use crate::wraps::Scope;
use axum::extract::{Path, Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

type S = State<Arc<AppState>>;
type R = Result<Json<Value>, ApiErr>;

/// The header a window sends with every request: the id of the library it is showing.
pub const HEADER: &str = "x-thoughts-library";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/libraries", get(list))
        .route("/api/libraries/switch", post(switch))
        .route("/api/libraries/add", post(add_back))
        .route("/api/libraries/:id", patch(rename).delete(remove))
}

/// What a library has going on: assistant runs at work in it, and chats waiting for its next Wrap.
async fn activity(st: &AppState, path: &str) -> (usize, usize) {
    let scope = Scope::of(Some(PathBuf::from(path)));
    let mine: Vec<_> = st.chats.list().await.into_iter().filter(|c| c.library.is_some() && scope.holds(c.library.as_deref())).collect();
    (mine.iter().filter(|c| c.running).count(), mine.iter().filter(|c| c.is_unwrapped()).count())
}

async fn entry_json(st: &AppState, s: &Settings, l: &Library) -> Value {
    let (running, waiting) = activity(st, &l.path).await;
    json!({"id": l.id, "name": l.name, "displayName": l.display_name(), "path": l.path, "open": s.is_open_library(std::path::Path::new(&l.path)),
           "missing": !std::path::Path::new(&l.path).is_dir(), "running": running, "waiting": waiting})
}

/// `GET /api/libraries` → `{libraries: [{id, name, displayName, path, open, missing, running, waiting}], openId}`.
async fn list(State(st): S) -> R {
    let s = st.settings.read().await.clone();
    let mut out = vec![];
    for l in &s.libraries { out.push(entry_json(&st, &s, l).await); }
    Ok(Json(json!({"libraries": out, "openId": s.library_id()})))
}

#[derive(Deserialize)]
struct SwitchReq { id: String }

/// `POST /api/libraries/switch {id}` → `{id, left: {name, running}}`: open another listed library.
/// What is still at work in the one left behind goes on there (`left.running` says how much).
async fn switch(State(st): S, Json(b): Json<SwitchReq>) -> R {
    if crate::settings::read_only() || st.settings_newer.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(crate::tr::err("newer_settings", &[]).into());
    }
    let target = st.settings.read().await.libraries.iter().find(|l| l.id == b.id).cloned().ok_or_else(|| crate::tr::err("libs.not_found", &[]))?;
    let folder = PathBuf::from(&target.path);
    if !folder.is_dir() { return Err(crate::tr::err("libs.missing", &[("name", &target.display_name()), ("path", &target.path)]).into()); }
    // The folder is looked at like any other the app is pointed at: it may have changed since.
    crate::trust::require(&folder)?;
    let (before, after) = {
        let mut cur = st.settings.write().await;
        let before = cur.clone();
        let mut next = cur.clone();
        next.switch_to(&target.id)?;
        next.save()?;
        *cur = next.clone();
        (before, next)
    };
    let left = before.open_library().cloned();
    let running = match &left { Some(l) => activity(&st, &l.path).await.0, None => 0 };
    if before.library_id() != after.library_id() {
        crate::log::info(format!("libraries: opened another library ({} listed); {running} run(s) go on in the one left", after.libraries.len()));
        st.emit(json!({"type": "settings.changed"}));
        st.emit(json!({"type": "library.switched", "id": after.library_id()}));
    }
    Ok(Json(json!({"id": after.library_id(), "left": {"name": left.map(|l| l.display_name()), "running": running}})))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AddReq { path: String }

/// `POST /api/libraries/add {path}` → the entry: put a library folder (back) on the list without
/// opening it — "Add it back" for a chat whose library was taken off. Only a folder that holds a
/// Thoughts library (its rules page or its index), looked at like any folder the app is pointed at.
async fn add_back(State(st): S, Json(b): Json<AddReq>) -> R {
    let typed = b.path.trim();
    let folder = crate::paths::expand_tilde(typed)?;
    if typed.is_empty() || !folder.is_absolute() || !folder.is_dir() { return Err(crate::tr::err("library.no_folder", &[("path", typed)]).into()); }
    if !folder.join("CLAUDE.md").is_file() && !folder.join("INDEX.md").is_file() { return Err(crate::tr::err("libs.not_library", &[("path", typed)]).into()); }
    crate::trust::require(&folder)?;
    let (s, id) = {
        let mut cur = st.settings.write().await;
        let mut next = cur.clone();
        let id = next.list_folder(&folder.to_string_lossy())?;
        next.save()?;
        *cur = next.clone();
        (next, id)
    };
    st.emit(json!({"type": "libraries.changed"}));
    let l = s.libraries.iter().find(|l| l.id == id).cloned().ok_or_else(|| crate::tr::err("libs.not_found", &[]))?;
    Ok(Json(entry_json(&st, &s, &l).await))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RenameReq { name: String }

/// `PATCH /api/libraries/:id {name}`: the name the switcher shows (empty: the folder's own).
async fn rename(State(st): S, Path(id): Path<String>, Json(b): Json<RenameReq>) -> R {
    let entry = {
        let mut cur = st.settings.write().await;
        let mut next = cur.clone();
        next.rename_library(&id, &b.name)?;
        next.save()?;
        *cur = next.clone();
        next.libraries.iter().find(|l| l.id == id).cloned()
    };
    st.emit(json!({"type": "libraries.changed"}));
    let s = st.settings.read().await.clone();
    Ok(Json(match entry { Some(l) => entry_json(&st, &s, &l).await, None => json!(null) }))
}

/// `DELETE /api/libraries/:id`: take a library off the list. Nothing in its folder is touched, and
/// its chats are kept: they are listed again if the same folder is added back.
async fn remove(State(st): S, Path(id): Path<String>) -> R {
    let gone = {
        let mut cur = st.settings.write().await;
        let mut next = cur.clone();
        // Work still going on there (a chat, a Wrap, an automatic task) keeps it on the list until it ends.
        if let Some(l) = next.libraries.iter().find(|l| l.id == id) {
            if activity(&st, &l.path).await.0 > 0 { return Err(crate::tr::err("libs.remove_busy", &[("name", &l.display_name())]).into()); }
        }
        let gone = next.remove_library(&id)?;
        next.save()?;
        *cur = next;
        gone
    };
    crate::log::info("libraries: a library was taken off the list (its folder and chats are kept)");
    st.emit(json!({"type": "libraries.changed"}));
    Ok(Json(json!({"ok": true, "removed": gone.id})))
}

/// The settings after "Add a library": the app's own settings as they are (`old`), the library
/// open until now parked in its entry, and the new one opened at `path` with the choices made on
/// the setup screens (`form`: backup, history, private folders, line of work, features).
pub fn added(old: &Settings, form: Settings, path: &str, name: &str) -> anyhow::Result<Settings> {
    let mut own = form.own();
    // Linked folders and the last wrap are the other library's (a form carries them along).
    own.projects.clear();
    own.last_wrap = None;
    let mut s = old.clone();
    let id = s.open_folder(path, Some(own))?;
    if !name.trim().is_empty() { s.rename_library(&id, name)?; }
    Ok(s)
}

/// The name of the library at `lib` when it is not the one open now — for a message about work
/// that finished there while another library was open.
pub async fn elsewhere(st: &AppState, lib: &std::path::Path) -> Option<String> {
    let s = st.settings.read().await;
    if s.is_open_library(lib) { return None; }
    s.library_at(lib).map(Library::display_name)
}

/// The listed library (the open one or another) whose folder is `p` or holds it — the innermost
/// one, should one library sit inside another.
fn library_holding(s: &Settings, p: &std::path::Path) -> Option<PathBuf> {
    let real = crate::paths::canonical_lenient(p);
    s.library().into_iter().chain(s.libraries.iter().map(|l| PathBuf::from(&l.path)))
        .filter(|l| real.starts_with(crate::paths::canonical_lenient(l)))
        .max_by_key(|l| crate::paths::canonical_lenient(l).components().count())
}

/// Every listed library (open or not) besides `own` that holds `cwd` or one of `add_dirs`, or sits
/// inside one of them. Libraries are never added one inside another, but a pair from before that
/// was refused may be listed: a run in either is then denied the private folders of both.
pub fn around(s: &Settings, cwd: &std::path::Path, add_dirs: &[PathBuf], own: Option<&std::path::Path>) -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = std::iter::once(cwd).chain(add_dirs.iter().map(PathBuf::as_path)).map(crate::paths::canonical_lenient).collect();
    let own = own.map(crate::paths::canonical_lenient);
    let mut out: Vec<PathBuf> = vec![];
    for l in s.library().into_iter().chain(s.libraries.iter().map(|l| PathBuf::from(&l.path))) {
        let real = crate::paths::canonical_lenient(&l);
        if own.as_ref() == Some(&real) || out.iter().any(|o| crate::paths::canonical_lenient(o) == real) { continue; }
        if dirs.iter().any(|d| d.starts_with(&real) || real.starts_with(d)) { out.push(l); }
    }
    out
}

/// Is `lib` the open library or one in the list?
fn listed(s: &Settings, lib: &std::path::Path) -> bool { s.is_open_library(lib) || s.library_at(lib).is_some() }

/// The library a run works for — whose private folders it is denied and whose trust it needs —
/// decided from one place. Where the run works (`cwd`) decides when that is inside a library;
/// otherwise (a linked folder, a Wrap's working copy, the app's own folders) the library its chat
/// belongs to. It fails closed: a run inside one library for a chat of another, a folder given
/// beside it that is a different library, or a chat whose library is no longer in the list, does
/// not start. Only a chat from before 0.0.9, which names no library at all, works for the open one.
pub fn of_run(s: &Settings, cwd: &std::path::Path, add_dirs: &[PathBuf], chat_library: Option<&str>) -> anyhow::Result<Option<PathBuf>> {
    let chat = chat_library.map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from);
    let other = |c: &std::path::Path| crate::tr::err("chat.other_library", &[("path", &c.to_string_lossy())]);
    if let Some(c) = &chat { if !listed(s, c) { return Err(crate::tr::err("libs.chat_unlisted", &[("path", &c.to_string_lossy())])); } }
    let lib = match (library_holding(s, cwd), &chat) {
        (Some(l), Some(c)) if !crate::settings::same_folder(&l, c) => return Err(other(c)),
        (Some(l), _) => Some(l),
        (None, Some(c)) => Some(c.clone()),
        (None, None) => s.library(),
    };
    for d in add_dirs {
        if let Some(l) = library_holding(s, d) {
            if !lib.as_deref().map_or(false, |x| crate::settings::same_folder(x, &l)) { return Err(other(&l)); }
        }
    }
    Ok(lib)
}

/// A chat whose library is not in the list any more: it cannot run, and no Wrap files it.
pub fn chat_unlisted(s: &Settings, chat_library: Option<&str>) -> Option<String> {
    chat_library.map(str::trim).filter(|l| !l.is_empty() && !listed(s, std::path::Path::new(l))).map(String::from)
}

/// Is a change asked for by a window that shows another library than the open one? Reading is
/// always fine (the window reloads on `library.switched`); the switcher's own requests, a window's
/// error report and stopping the app are not about any library.
fn stale(method: &Method, path: &str, sent: Option<&str>, open: Option<&str>) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) { return false; }
    if path.starts_with("/api/libraries") || path == "/api/ui-error" || path == "/api/shutdown" { return false; }
    match (sent.map(str::trim).filter(|s| !s.is_empty()), open) { (Some(s), Some(o)) => s != o, _ => false }
}

/// Middleware: refuse a change from a window that still shows the library open before (see `stale`).
pub async fn gate(State(st): S, req: Request, next: Next) -> Response {
    let sent = req.headers().get(HEADER).and_then(|v| v.to_str().ok()).map(str::to_string);
    if sent.is_some() {
        let open = st.settings.read().await.library_id();
        if stale(req.method(), req.uri().path(), sent.as_deref(), open.as_deref()) {
            return ApiErr::from(crate::tr::err("libs.switched", &[])).into_response();
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chats::{Chat, Msg};
    use crate::wraps::{self, testutil as t};
    use std::path::Path;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn serve(st: &Arc<AppState>) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = crate::api::router(st.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        port
    }

    /// One request with the test token (and the window's library, when `lib` is given) → (status, body).
    async fn http(port: u16, method: &str, path: &str, body: &str, lib: Option<&str>) -> (u16, Value) {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let extra = lib.map(|l| format!("{HEADER}: {l}\r\n")).unwrap_or_default();
        let head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer test-token\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        s.write_all(head.as_bytes()).await.unwrap();
        s.write_all(body.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out).await;
        let out = String::from_utf8_lossy(&out).to_string();
        let status = out.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        let body = out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
        (status, serde_json::from_str(&body).unwrap_or(Value::String(body)))
    }

    /// A second library beside `a`: a folder with a note of its own, recorded in a history of its own.
    fn second(a: &Path, name: &str) -> PathBuf {
        let b = a.parent().unwrap().join(name);
        t::write(&b, "README.md", "the other library\n");
        t::sh(&b, &["init", "-q", "-b", "main"]);
        for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com")] { t::sh(&b, &["config", k, v]); }
        t::sh(&b, &["add", "-A"]);
        t::sh(&b, &["commit", "-q", "-m", "first"]);
        dunce::canonicalize(b).unwrap()
    }

    /// Two libraries in the list, `a` open: (id of a, id of b).
    async fn two(st: &AppState, a: &Path, b: &Path) -> (String, String) {
        let mut s = st.settings.write().await;
        s.onboarded = true;
        s.private_paths = vec!["clients".into()];
        let a_id = s.open_folder(&a.to_string_lossy(), None).unwrap();
        let b_id = s.open_folder(&b.to_string_lossy(), None).unwrap();
        s.private_paths = vec![];
        s.switch_to(&a_id).unwrap();
        (a_id, b_id)
    }

    #[tokio::test]
    async fn the_switcher_opens_each_library_with_its_own_chats_and_settings() {
        let (st, a) = t::library().await;
        let b = second(&a, "personal");
        let (a_id, b_id) = two(&st, &a, &b).await;
        let in_a = t::chat(&st, "CLIENTFEE nine thousand for client A").await;
        st.chats.update(&in_a, |c| c.library = Some(a.to_string_lossy().to_string())).await;
        let in_b = t::chat(&st, "a personal note").await;
        st.chats.update(&in_b, |c| c.library = Some(b.to_string_lossy().to_string())).await;
        let port = serve(&st).await;
        let mut rx = st.events.subscribe();

        let (code, list) = http(port, "GET", "/api/libraries", "", None).await;
        assert_eq!(code, 200, "{list}");
        assert_eq!(list["openId"], a_id.as_str());
        assert_eq!(list["libraries"].as_array().unwrap().iter().map(|l| (l["displayName"].as_str().unwrap().to_string(), l["open"].as_bool().unwrap(), l["waiting"].as_u64().unwrap())).collect::<Vec<_>>(),
            [("library".to_string(), true, 1), ("personal".to_string(), false, 1)]);
        let (_, status) = http(port, "GET", "/api/status", "", None).await;
        assert_eq!((status["libraryId"].as_str(), status["libraryName"].as_str()), (Some(a_id.as_str()), Some("library")));

        // Open the second library: its own settings, its own chats, its own search.
        let (code, r) = http(port, "POST", "/api/libraries/switch", &json!({"id": b_id}).to_string(), Some(&a_id)).await;
        assert_eq!(code, 200, "{r}");
        assert_eq!(r["left"]["name"], "library");
        let ev = loop { let e = rx.recv().await.unwrap(); if e["type"] == "library.switched" { break e; } };
        assert_eq!(ev["id"], b_id.as_str());
        let s = st.settings.read().await.clone();
        assert_eq!((s.library(), s.private_paths.len()), (Some(b.clone()), 0));
        assert_eq!(s.settings_of(&a).unwrap().private_paths, ["clients"]);
        let (_, chats) = http(port, "GET", "/api/chats", "", None).await;
        assert_eq!(chats.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect::<Vec<_>>(), [in_b.as_str()]);
        let (_, found) = http(port, "GET", "/api/search?q=CLIENTFEE", "", None).await;
        assert!(found["chats"].as_array().unwrap().is_empty(), "another library's chats are never found: {found}");

        // A window that still shows the first library cannot change anything in the second.
        let before = st.settings.read().await.clone();
        let mut sent = serde_json::to_value(&before).unwrap();
        sent["privatePaths"] = json!(["clients"]);
        let (code, r) = http(port, "PUT", "/api/settings", &sent.to_string(), Some(&a_id)).await;
        assert_eq!(code, 409, "{r}");
        assert!(r["error"].as_str().unwrap().starts_with("LIBRARY_SWITCHED::"), "{r}");
        assert!(st.settings.read().await.private_paths.is_empty());
        let (code, _) = http(port, "POST", "/api/capture", &json!({"kind": "note", "text": "for A"}).to_string(), Some(&a_id)).await;
        assert_eq!(code, 409);
        assert!(!a.join("inbox").exists() && !b.join("inbox").exists());
        // Reading is fine, and a window that shows the open library goes through.
        assert_eq!(http(port, "GET", "/api/libraries", "", Some(&a_id)).await.0, 200);
        let (code, _) = http(port, "POST", "/api/capture", &json!({"kind": "note", "text": "a quick note"}).to_string(), Some(&b_id)).await;
        assert_eq!(code, 200);

        // Named, and taken off the list: never the open one, and never its folder or its chats.
        let (code, r) = http(port, "PATCH", &format!("/api/libraries/{a_id}"), &json!({"name": " Client A "}).to_string(), Some(&b_id)).await;
        assert_eq!((code, r["displayName"].as_str()), (200, Some("Client A")));
        assert_eq!(http(port, "DELETE", &format!("/api/libraries/{b_id}"), "", Some(&b_id)).await.0, 409);
        assert_eq!(http(port, "DELETE", &format!("/api/libraries/{a_id}"), "", Some(&b_id)).await.0, 200);
        assert!(a.join("README.md").is_file() && st.chats.meta(&in_a).await.is_some());
        assert_eq!(st.settings.read().await.libraries.len(), 1);
        assert_eq!(http(port, "POST", "/api/libraries/switch", &json!({"id": a_id}).to_string(), None).await.0, 404);
        std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn quick_capture_writes_into_the_library_open_now() {
        let (st, a) = t::library().await;
        let b = second(&a, "personal");
        let (_, b_id) = two(&st, &a, &b).await;
        let port = serve(&st).await;
        assert_eq!(http(port, "POST", "/api/capture", &json!({"kind": "note", "text": "in the first"}).to_string(), None).await.0, 200);
        assert_eq!(http(port, "POST", "/api/libraries/switch", &json!({"id": b_id}).to_string(), None).await.0, 200);
        let (code, r) = http(port, "POST", "/api/capture", &json!({"kind": "todo", "text": "call the bank"}).to_string(), None).await;
        assert_eq!(code, 200, "{r}");
        let files = |d: &Path| std::fs::read_dir(d.join("inbox")).map_or(0, |r| r.count());
        assert_eq!((files(&a), files(&b)), (1, 0));
        assert!(std::fs::read_to_string(b.join(r["file"].as_str().unwrap())).unwrap().contains("call the bank"));
        assert_eq!(t::read(&a, "TODO.md"), "# Todo\n\n- [ ] call the bank\n", "the first library's list is as it was");
        std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn each_library_is_trusted_on_its_own() {
        let (st, a) = t::library().await;
        let b = second(&a, "shared-with-me");
        // The second library came with assistant settings that run a command.
        t::write(&b, ".claude/settings.json", r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}"#);
        let (a_id, b_id) = two(&st, &a, &b).await;
        let port = serve(&st).await;
        let (code, r) = http(port, "POST", "/api/libraries/switch", &json!({"id": b_id}).to_string(), None).await;
        assert_eq!((code, r["code"].as_str()), (403, Some("untrusted")), "{r}");
        assert_eq!(st.settings.read().await.library(), Some(a.clone()), "nothing switched before the yes");
        crate::trust::accept(r["trust"]["id"].as_str().unwrap()).unwrap();
        assert_eq!(http(port, "POST", "/api/libraries/switch", &json!({"id": b_id}).to_string(), None).await.0, 200);
        // Trusting the second library says nothing about the first: it is looked at on its own, and is fine.
        assert_eq!(http(port, "POST", "/api/libraries/switch", &json!({"id": a_id}).to_string(), None).await.0, 200);
        // A change in the second library asks again when it is opened next.
        t::write(&b, ".claude/settings.json", r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"say something else"}]}]}}"#);
        assert_eq!(http(port, "POST", "/api/libraries/switch", &json!({"id": b_id}).to_string(), None).await.0, 403);
        std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn a_wrap_started_in_one_library_finishes_there_after_another_is_opened() {
        let (st, a) = t::library().await;
        let b = second(&a, "personal");
        let (_, b_id) = two(&st, &a, &b).await;
        let key_a = a.to_string_lossy().to_string();
        let chat = t::chat(&st, "let us plan the shop").await;
        let wrap = t::wrap_ready(&st, &[chat.clone()]).await;
        let k = key_a.clone();
        st.chats.update(&chat, |c| c.library = Some(k.clone())).await;
        st.chats.update(&wrap, |c| { c.library = Some(k); c.meta["direct"] = json!(true); }).await;
        let go = Arc::new(tokio::sync::Notify::new());
        let (g, a2) = (go.clone(), a.clone());
        t::assistant(&st, move |info| { let (g, a) = (g.clone(), a2.clone()); async move {
            assert_eq!(info.cwd, a, "the wrap works in its own library");
            // The other library is opened while the wrap writes.
            g.notified().await;
            t::write(&info.cwd, "ideas/shop/log.md", "## today\n- Decided: open in May\n");
            true
        } });
        let mut rx = st.events.subscribe();
        wraps::apply(st.clone(), &wrap, "").await.unwrap();
        assert!(st.library_busy());
        st.settings.write().await.switch_to(&b_id).unwrap();
        go.notify_one();
        t::event(&mut rx, &st, "wrap.done").await;
        let told = loop { let e = rx.recv().await.unwrap(); if e["type"] == "notify" { break e; } };
        assert_eq!(told["body"], "1 chat saved into the library \"library\"");
        // Everything went into the first library, and its history; the second is exactly as it was.
        assert!(a.join("ideas/shop/log.md").is_file());
        assert_eq!(t::sh(&a, &["log", "-1", "--format=%s"]), crate::git::subject::wrap(1));
        assert!(!b.join("ideas").exists());
        assert_eq!(t::sh(&b, &["log", "--format=%s"]), "first");
        assert_eq!(t::sh(&b, &["status", "--porcelain"]), "");
        // The first library was wrapped, the open one was not.
        let s = st.settings.read().await.clone();
        assert_eq!(s.library(), Some(b.clone()));
        assert!(s.last_wrap.is_none());
        assert!(s.settings_of(&a).unwrap().last_wrap.is_some());
        assert!(!st.chats.meta(&chat).await.unwrap().is_unwrapped());
        std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn an_automatic_task_runs_in_the_library_it_belongs_to() {
        let (st, a) = t::library().await;
        let b = second(&a, "personal");
        let (_, b_id) = two(&st, &a, &b).await;
        let home = crate::paths::home();
        let (file, other, gone) = (format!("libs-{}.json", uuid::Uuid::new_v4()), format!("libs-{}.json", uuid::Uuid::new_v4()), format!("libs-{}.json", uuid::Uuid::new_v4()));
        // Allowed while the first library was open; another one was never tied to any.
        crate::workflows::bind_to_open(&st, &[file.clone()]).await;
        st.settings.write().await.switch_to(&b_id).unwrap();
        // Allowing it again from the second library does not move it.
        crate::workflows::bind_to_open(&st, &[file.clone()]).await;
        assert_eq!(crate::workflows::load_bound(&home).get(&file).map(String::as_str), Some(a.to_string_lossy().as_ref()));
        // Settings lists it as the first library's, the untied one as this one's.
        let mut list = vec![json!({"file": file}), json!({"file": other})];
        crate::workflows::add_libraries(&st, &mut list).await;
        assert_eq!((list[0]["here"].as_bool(), list[0]["libraryName"].as_str()), (Some(false), Some("library")));
        assert_eq!((list[1]["here"].as_bool(), list[1]["orphaned"].as_bool()), (Some(true), Some(false)));
        // Its run works in the first library although the second is open.
        let seen = Arc::new(std::sync::Mutex::new(vec![]));
        let s2 = seen.clone();
        t::assistant(&st, move |info| { let s = s2.clone(); async move { s.lock().unwrap().push((info.cwd.clone(), info.deny.clone())); true } });
        let w = crate::workflows::Workflow { name: "Morning".into(), trigger: crate::workflows::Trigger::Event { on: "app.start".into() }, prompt: "tidy the todo list".into() };
        let id = crate::workflows::start(&st, &file, &w, 1.0).await.unwrap();
        for _ in 0..400 { if !seen.lock().unwrap().is_empty() { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
        let (cwd, deny) = seen.lock().unwrap()[0].clone();
        assert_eq!(cwd, a);
        assert!(deny.iter().any(|d| d.contains("clients")), "the first library's private folders apply: {deny:?}");
        assert_eq!(st.chats.meta(&id).await.unwrap().library.as_deref(), Some(a.to_string_lossy().as_ref()));
        // The one never tied runs in the open library.
        crate::workflows::start(&st, &other, &w, 1.0).await.unwrap();
        for _ in 0..400 { if seen.lock().unwrap().len() > 1 { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
        assert_eq!(seen.lock().unwrap()[1].0, b);
        // A task whose library left the list runs nowhere, and says why.
        crate::workflows::bind(&home, &[gone.clone()], "/no/longer/listed", |_| true).unwrap();
        let e = crate::workflows::start(&st, &gone, &w, 1.0).await.unwrap_err();
        assert_eq!(crate::tr::classify_in(false, &e).message, crate::tr::text_in(false, "workflow.no_library", &[]));
        let mut list = vec![json!({"file": gone})];
        crate::workflows::add_libraries(&st, &mut list).await;
        assert_eq!(list[0]["orphaned"], true);
        std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn another_folder_chosen_in_settings_is_opened_with_its_own_settings_and_a_moved_one_keeps_them() {
        let (st, a) = t::library().await;
        let b = second(&a, "personal");
        let (a_id, _) = two(&st, &a, &b).await;
        let port = serve(&st).await;
        let put = |s: &crate::settings::Settings, lib: &Path, q: &str| {
            let mut v = serde_json::to_value(s).unwrap();
            v["libraryPath"] = json!(lib.to_string_lossy());
            let q = q.to_string();
            async move { http(port, "PUT", &format!("/api/settings{q}"), &v.to_string(), None).await }
        };
        // Pointing the library folder at a listed library opens it, with its own settings.
        let cur = st.settings.read().await.clone();
        let (code, r) = put(&cur, &b, "").await;
        assert_eq!(code, 200, "{r}");
        let s = st.settings.read().await.clone();
        assert_eq!((s.library(), s.private_paths.len(), s.libraries.len()), (Some(b.clone()), 0, 2));
        assert_eq!(s.settings_of(&a).unwrap().private_paths, ["clients"]);
        // A folder that is no library yet becomes one more in the list, starting afresh.
        let c = second(&a, "third");
        let (code, _) = put(&s, &c, "").await;
        assert_eq!(code, 200);
        assert_eq!(st.settings.read().await.libraries.len(), 3);
        // The open library moved: same entry, same settings, and its chats go with it.
        let a2 = { let mut s = st.settings.write().await; s.switch_to(&a_id).unwrap(); a.parent().unwrap().join("library-moved") };
        let chat = t::chat(&st, "about the first").await;
        st.chats.update(&chat, |c| c.library = Some(a.to_string_lossy().to_string())).await;
        std::fs::rename(&a, &a2).unwrap();
        let cur = st.settings.read().await.clone();
        let (code, r) = put(&cur, &a2, "?library=moved").await;
        assert_eq!(code, 200, "{r}");
        let s = st.settings.read().await.clone();
        assert_eq!((s.library_id().as_deref(), s.libraries.len(), s.private_paths.clone()), (Some(a_id.as_str()), 3, vec!["clients".to_string()]));
        assert_eq!(st.chats.meta(&chat).await.unwrap().library.as_deref(), Some(a2.to_string_lossy().as_ref()));
        std::fs::remove_dir_all(a2.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn adding_a_library_reuses_setup_and_leaves_the_apps_own_settings_alone() {
        let (st, a) = t::library().await;
        let root = a.parent().unwrap().to_path_buf();
        {
            let mut s = st.settings.write().await;
            s.onboarded = true;
            s.language = "ar".into();
            s.user_name = "Sara".into();
            s.ai.claude.model = Some("opus".into());
            s.private_paths = vec!["clients".into()];
            s.profession = Some("legal".into());
            *s = std::mem::take(&mut *s).with_defaults();
        }
        let a_id = st.settings.read().await.library_id().unwrap();
        let port = serve(&st).await;
        // What the setup screens send in "add a library" mode: a whole settings object, of which
        // only the library's part counts.
        let mut form = serde_json::to_value(&*st.settings.read().await).unwrap();
        let c = root.join("Family");
        form["libraryPath"] = json!(c.to_string_lossy());
        form["privatePaths"] = json!([]);
        form["profession"] = json!("personal");
        form["sync"]["mode"] = json!("none");
        form["language"] = json!("en");
        form["userName"] = json!("Someone else");
        form["ai"]["claude"]["model"] = json!("haiku");
        let body = json!({"settings": form, "libraryMode": "new", "assistantLater": true, "addLibrary": true, "libraryName": "Family"});
        let (code, r) = http(port, "POST", "/api/onboarding", &body.to_string(), Some(&a_id)).await;
        assert_eq!(code, 200, "{r}");
        assert!(c.join("CLAUDE.md").is_file(), "a new library made from the starter kit");
        let s = st.settings.read().await.clone();
        assert_eq!(s.library(), Some(c.clone()));
        assert_eq!(s.open_library().unwrap().display_name(), "Family");
        assert_eq!((s.profession.as_deref(), s.private_paths.len(), s.sync.mode.as_str()), (Some("personal"), 0, "none"));
        // The app's own settings did not move.
        assert_eq!((s.language.as_str(), s.user_name.as_str(), s.ai.claude.model.as_deref()), ("ar", "Sara", Some("opus")));
        // The first library is parked, whole.
        let first = s.settings_of(&a).unwrap();
        assert_eq!((first.private_paths, first.profession.as_deref()), (vec!["clients".to_string()], Some("legal")));
        assert_eq!(s.libraries.len(), 2);
        // Its setup waits for the assistant, tied to the new library.
        let setup = st.chats.list().await.into_iter().find(|c| c.kind == "setup").unwrap();
        assert_eq!(setup.library.as_deref(), Some(c.to_string_lossy().as_ref()));
        // Back in the first library, that setup cannot be run there.
        st.settings.write().await.switch_to(&a_id).unwrap();
        let (code, r) = http(port, "POST", &format!("/api/onboarding/setup/{}", setup.id), "", None).await;
        assert_eq!(code, 409);
        assert!(r["error"].as_str().unwrap().contains("belongs to another library"), "{r}");
        // Before Thoughts is set up at all, there is nothing to add to.
        st.settings.write().await.onboarded = false;
        assert_eq!(http(port, "POST", "/api/onboarding", &body.to_string(), None).await.0, 409);
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn spec(chat: &str, cwd: &Path, add_dirs: Vec<PathBuf>) -> crate::claude::RunSpec {
        crate::claude::RunSpec { chat_id: chat.into(), prompt: "go".into(), cwd: cwd.to_path_buf(), add_dirs,
            options: crate::chats::RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() }, resume: None, append_system: None, extra_deny: vec![] }
    }

    #[tokio::test]
    async fn a_chat_whose_library_left_the_list_cannot_run_and_is_never_wrapped() {
        let (st, a) = t::library().await;
        let b = second(&a, "old-client");
        t::write(&b, "CLAUDE.md", "# Rules\n");
        let (_, b_id) = two(&st, &a, &b).await;
        let gone = t::chat(&st, "about the old client").await;
        st.chats.update(&gone, |c| c.library = Some(b.to_string_lossy().to_string())).await;
        t::assistant(&st, |_| async { panic!("nothing may run for a library that is no longer listed") });
        let port = serve(&st).await;
        assert_eq!(http(port, "DELETE", &format!("/api/libraries/{b_id}"), "", None).await.0, 200);
        // It cannot run: not by sending, and not by any run started for it, wherever that would work.
        let (code, r) = http(port, "POST", &format!("/api/chats/{gone}/send"), &json!({"text": "go on"}).to_string(), None).await;
        assert_eq!((code, r["code"].as_str()), (409, Some("library_unlisted")), "{r}");
        for cwd in [&a, &b] {
            let e = crate::claude::start_run(st.clone(), spec(&gone, cwd, vec![]), None).await.unwrap_err();
            assert_eq!(crate::tr::classify_in(false, &e).code, "library_unlisted", "{}", cwd.display());
        }
        // No Wrap files it, in the open library or anywhere else.
        assert_eq!(wraps::status(&st).await["count"], 0);
        let e = wraps::start(st.clone(), false).await.unwrap_err();
        assert_eq!(crate::tr::classify_in(false, &e).message, crate::tr::text_in(false, "wrap.nothing", &[]));
        // "Add it back": listed again (not opened), and it is that library's chat once more.
        let (code, r) = http(port, "POST", "/api/libraries/add", &json!({"path": b.to_string_lossy()}).to_string(), None).await;
        assert_eq!((code, r["open"].as_bool()), (200, Some(false)), "{r}");
        assert!(crate::libraries::chat_unlisted(&*st.settings.read().await, Some(&b.to_string_lossy())).is_none());
        // Only a folder that holds a library can be added.
        let plain = a.parent().unwrap().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(http(port, "POST", "/api/libraries/add", &json!({"path": plain.to_string_lossy()}).to_string(), None).await.0, 422);
        std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn a_run_works_for_one_library_only_and_is_denied_that_librarys_private_folders() {
        let (st, a) = t::library().await;
        let b = second(&a, "personal");
        let (_, b_id) = two(&st, &a, &b).await;
        { let mut s = st.settings.write().await; s.switch_to(&b_id).unwrap(); s.private_paths = vec!["diary".into()]; }
        let in_a = t::chat(&st, "about the first library").await;
        st.chats.update(&in_a, |c| c.library = Some(a.to_string_lossy().to_string())).await;
        let seen = Arc::new(std::sync::Mutex::new(vec![]));
        let s2 = seen.clone();
        t::assistant(&st, move |info| { let s = s2.clone(); async move { s.lock().unwrap().push((info.cwd.clone(), info.deny.clone())); true } });
        // Working in the second library for a chat of the first: refused, whichever way it is put.
        let linked = a.parent().unwrap().join("linked-project");
        std::fs::create_dir_all(&linked).unwrap();
        for s in [spec(&in_a, &b, vec![]), spec(&in_a, &b.join("notes"), vec![]), spec(&in_a, &linked, vec![b.clone()])] {
            let e = crate::claude::start_run(st.clone(), s, None).await.unwrap_err();
            let m = crate::tr::classify_in(false, &e).message;
            assert!(m.starts_with("This chat belongs to another library"), "{m}");
        }
        assert!(seen.lock().unwrap().is_empty());
        // In its own library — not the open one — with that library's private folders denied, not the open one's.
        crate::claude::start_run(st.clone(), spec(&in_a, &a, vec![]), None).await.unwrap();
        for _ in 0..400 { if !seen.lock().unwrap().is_empty() { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
        let (cwd, deny) = seen.lock().unwrap()[0].clone();
        assert_eq!(cwd, a);
        assert!(deny.iter().any(|d| d.contains("clients")), "{deny:?}");
        assert!(!deny.iter().any(|d| d.contains("diary")), "{deny:?}");
        // A chat from before 0.0.9 (no library named) works for the open library, with its private folders.
        let old = t::chat(&st, "from an older version").await;
        crate::claude::start_run(st.clone(), spec(&old, &b, vec![]), None).await.unwrap();
        for _ in 0..400 { if seen.lock().unwrap().len() > 1 { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
        let (cwd, deny) = seen.lock().unwrap()[1].clone();
        assert_eq!(cwd, b);
        assert!(deny.iter().any(|d| d.contains("diary")) && !deny.iter().any(|d| d.contains("clients")), "{deny:?}");
        std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn a_library_with_work_going_on_stays_on_the_list() {
        let (st, a) = t::library().await;
        let b = second(&a, "personal");
        let (_, b_id) = two(&st, &a, &b).await;
        let busy = t::chat(&st, "still answering").await;
        st.chats.update(&busy, |c| { c.library = Some(b.to_string_lossy().to_string()); c.running = true; }).await;
        let port = serve(&st).await;
        let (code, r) = http(port, "DELETE", &format!("/api/libraries/{b_id}"), "", None).await;
        assert_eq!(code, 409, "{r}");
        assert!(r["error"].as_str().unwrap().contains("\"personal\""), "{r}");
        assert_eq!(st.settings.read().await.libraries.len(), 2);
        st.chats.update(&busy, |c| c.running = false).await;
        assert_eq!(http(port, "DELETE", &format!("/api/libraries/{b_id}"), "", None).await.0, 200);
        std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn a_library_is_never_added_inside_another_or_around_one() {
        let (st, a) = t::library().await;
        let root = a.parent().unwrap().to_path_buf();
        { let mut s = st.settings.write().await; s.onboarded = true; *s = std::mem::take(&mut *s).with_defaults(); }
        let inner = a.join("clients/acme");
        t::write(&inner, "CLAUDE.md", "# Rules\n");
        t::write(&root, "INDEX.md", "# Everything\n");
        let port = serve(&st).await;
        for (folder, what) in [(&inner, "inside"), (&root, "around")] {
            let (code, r) = http(port, "POST", "/api/libraries/add", &json!({"path": folder.to_string_lossy()}).to_string(), None).await;
            assert_eq!(code, 422, "{what}: {r}");
            assert!(r["error"].as_str().unwrap().contains("\"library\""), "names the other library: {r}");
        }
        // Nor through setup's "add a library", before anything is made.
        let mut form = serde_json::to_value(&*st.settings.read().await).unwrap();
        let fresh = a.join("notes/new-library");
        form["libraryPath"] = json!(fresh.to_string_lossy());
        let body = json!({"settings": form.clone(), "libraryMode": "new", "assistantLater": true, "addLibrary": true});
        assert_eq!(http(port, "POST", "/api/onboarding", &body.to_string(), None).await.0, 422);
        assert!(!fresh.exists());
        form["libraryPath"] = json!(root.to_string_lossy());
        let body = json!({"settings": form.clone(), "libraryMode": "existing", "assistantLater": true, "addLibrary": true});
        let (code, r) = http(port, "POST", "/api/onboarding", &body.to_string(), None).await;
        assert_eq!(code, 422, "around: {r}");
        assert!(r["error"].as_str().unwrap().contains("inside the library"), "{r}");
        // Setup itself runs once: afterwards it never puts another library in the open one's place.
        let body = json!({"settings": form.clone(), "libraryMode": "existing", "assistantLater": true});
        let (code, r) = http(port, "POST", "/api/onboarding", &body.to_string(), None).await;
        assert_eq!((code, r["error"].as_str().map(|e| e.starts_with("Thoughts is already set up"))), (409, Some(true)), "{r}");
        // Nor by pointing the library folder of Settings there.
        let mut v = serde_json::to_value(&*st.settings.read().await).unwrap();
        v["libraryPath"] = json!(inner.to_string_lossy());
        assert_eq!(http(port, "PUT", "/api/settings", &v.to_string(), None).await.0, 422);
        assert_eq!(st.settings.read().await.libraries.len(), 1);
        // First-time setup with a list left by an earlier install: not inside one of those either.
        { let mut s = st.settings.write().await; s.onboarded = false; s.library_path = None; }
        form["libraryPath"] = json!(fresh.to_string_lossy());
        let body = json!({"settings": form, "libraryMode": "new", "assistantLater": true});
        let (code, r) = http(port, "POST", "/api/onboarding", &body.to_string(), None).await;
        assert_eq!(code, 422, "{r}");
        assert!(r["error"].as_str().unwrap().contains("inside the library"), "{r}");
        assert!(!fresh.exists() && !st.settings.read().await.onboarded);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn a_nested_pair_listed_earlier_closes_both_libraries_private_folders() {
        let (st, outer) = t::library().await;
        let inner = outer.join("team");
        t::write(&inner, "CLAUDE.md", "# Team rules\n");
        // A pair from an older settings file: the inner library open, the outer one parked.
        {
            let mut s = st.settings.write().await;
            s.onboarded = true;
            s.private_paths = vec!["clients".into()];
            *s = std::mem::take(&mut *s).with_defaults();
            let parked = s.own();
            s.libraries.push(crate::settings::Library { id: "lib-inner".into(), name: String::new(), path: inner.to_string_lossy().to_string(), parked: None });
            s.libraries[0].parked = Some(parked);
            s.library_path = Some(inner.to_string_lossy().to_string());
            s.private_paths = vec!["diary".into()];
            *s = std::mem::take(&mut *s).with_defaults();
        }
        let in_inner = t::chat(&st, "about the team").await;
        st.chats.update(&in_inner, |c| c.library = Some(inner.to_string_lossy().to_string())).await;
        let seen = Arc::new(std::sync::Mutex::new(vec![]));
        let s2 = seen.clone();
        t::assistant(&st, move |info| { let s = s2.clone(); async move { s.lock().unwrap().push(info.deny.clone()); true } });
        let wait = |n: usize| { let seen = seen.clone(); async move { for _ in 0..400 { if seen.lock().unwrap().len() >= n { break; } tokio::time::sleep(std::time::Duration::from_millis(5)).await; } } };
        // The pair was there before: settings can still be saved.
        let port = serve(&st).await;
        let mut v = serde_json::to_value(&*st.settings.read().await).unwrap();
        v["userName"] = json!("Sara");
        let (code, r) = http(port, "PUT", "/api/settings", &v.to_string(), None).await;
        assert_eq!(code, 200, "{r}");
        crate::claude::start_run(st.clone(), spec(&in_inner, &inner, vec![]), None).await.unwrap();
        wait(1).await;
        let deny = seen.lock().unwrap()[0].clone();
        let outer_clients = outer.join("clients").to_string_lossy().to_string();
        assert!(deny.iter().any(|d| d.contains("diary")), "its own: {deny:?}");
        assert!(deny.iter().any(|d| d.contains(&outer_clients)), "the library around it: {deny:?}");
        // And the other way round: a run in the outer library is kept out of the inner one's.
        let outer_id = st.settings.read().await.libraries[0].id.clone();
        st.settings.write().await.switch_to(&outer_id).unwrap();
        let in_outer = t::chat(&st, "about everything").await;
        st.chats.update(&in_outer, |c| c.library = Some(outer.to_string_lossy().to_string())).await;
        crate::claude::start_run(st.clone(), spec(&in_outer, &outer, vec![]), None).await.unwrap();
        wait(2).await;
        let deny = seen.lock().unwrap()[1].clone();
        assert!(deny.iter().any(|d| d.contains("clients")), "{deny:?}");
        assert!(deny.iter().any(|d| d.contains(&inner.join("diary").to_string_lossy().to_string())), "the library inside it: {deny:?}");
        std::fs::remove_dir_all(outer.parent().unwrap()).unwrap();
    }

    #[test]
    fn only_changes_from_a_window_showing_another_library_are_refused() {
        let (a, b) = (Some("lib-a"), Some("lib-b"));
        assert!(stale(&Method::PUT, "/api/library/file", a, b));
        assert!(stale(&Method::POST, "/api/chats", a, b));
        assert!(!stale(&Method::GET, "/api/library/file", a, b), "reading is fine");
        assert!(!stale(&Method::POST, "/api/libraries/switch", a, b), "the switcher itself");
        assert!(!stale(&Method::POST, "/api/ui-error", a, b));
        assert!(!stale(&Method::PUT, "/api/library/file", b, b));
        assert!(!stale(&Method::PUT, "/api/library/file", None, b), "a window that does not say (the capture box, an older window)");
        assert!(!stale(&Method::PUT, "/api/library/file", Some(" "), b));
        assert!(!stale(&Method::PUT, "/api/library/file", a, None), "before there is a library");
    }

    #[tokio::test]
    async fn chats_from_before_libraries_belong_to_the_one_open_at_the_upgrade_and_stay_there() {
        let (st, a) = t::library().await;
        let b = a.parent().unwrap().join("second");
        std::fs::create_dir_all(&b).unwrap();
        // Chats written by 0.0.8: no library in the file at all.
        let old: Chat = serde_json::from_str(r#"{"id":"0a8-chat","title":"Old chat","kind":"chat","messages":[{"id":"m1","role":"user","text":"hello","ts":"2026-05-01T10:00:00Z"}]}"#).unwrap();
        assert!(old.library.is_none());
        st.chats.insert(old).await;
        let custom = Chat { kind: "customize".into(), ..Default::default() };
        let custom_id = custom.id.clone();
        st.chats.insert(custom).await;
        // The first start of this version: the chats are the open library's.
        wraps::recover_chats(&st).await;
        let key_a = a.to_string_lossy().to_string();
        assert_eq!(st.chats.meta("0a8-chat").await.unwrap().library.as_deref(), Some(key_a.as_str()));
        assert!(st.chats.meta(&custom_id).await.unwrap().library.is_none(), "customising the app belongs to no library");
        // Another library is opened and the app starts again: they stay where they were tied once.
        st.settings.write().await.open_folder(&b.to_string_lossy(), None).unwrap();
        wraps::recover_chats(&st).await;
        assert_eq!(st.chats.meta("0a8-chat").await.unwrap().library.as_deref(), Some(key_a.as_str()));
        assert!(!wraps::Scope::now(&st).await.holds(Some(&key_a)), "not listed while the second library is open");
        // A chat started now belongs to the second library, and an older one never moves into it.
        let mut new = Chat::default();
        new.messages.push(Msg::new("user", "about the second library"));
        new.library = Some(b.to_string_lossy().to_string());
        st.chats.insert(new).await;
        let scope = wraps::Scope::now(&st).await;
        let here: Vec<String> = st.chats.list().await.into_iter().filter(|c| c.kind == "chat" && scope.holds(c.library.as_deref())).map(|c| c.title).collect();
        assert_eq!(here, ["New chat"]);
        std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    }
}
