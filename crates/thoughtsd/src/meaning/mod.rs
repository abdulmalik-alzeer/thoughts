//! Search by meaning (round 0.5.0): finds notes about what the user means, in Arabic and English,
//! even when the words differ. Optional, and off until the user switches it on.
//!
//! - Off (the default): no model, no index, no background work and no network. The only things it
//!   does are suggest itself (a large library, a word search that found nothing, an assistant that
//!   could not find what the user meant) and tell ordinary chats how to ask for it; "Don't ask
//!   again" ends both, and then a run's text is exactly what it was before this feature.
//! - On: the preset model is fetched once (`model.rs`; pause, cancel and resume), every library
//!   note outside private folders is cut into pieces and turned into vectors on this computer
//!   (`index.rs`), and the index follows changes to the library. Search gets "Related by meaning";
//!   chats are told the paths of up to 8 notes that may relate to the user's message.
//! - Remove deletes the model and every index and switches it off.
//!
//! The model and the index live in `<home>/meaning/`, outside the library, a folder every run is
//! closed to (`claude.rs` closes every unknown folder of the app's data). The index holds no note
//! text, only byte offsets and vectors; snippets are read from the file when a result is shown.

pub mod index;
pub mod model;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::chats::Msg;
use crate::state::AppState;
use index::Index;
use model::Embedder;

/// The line an assistant ends an answer with to suggest search by meaning (ordinary chats only).
pub const MARKER: &str = "[[thoughts:meaning-search]]";
/// Lowest similarity a note is shown or named with (cosine of unit vectors; E5 scores sit high).
/// Measured with the real model (`real_model_…` test): related notes 0.79–0.88 across Arabic and
/// English, questions about nothing in the library at most 0.75. Used with `index::MARGIN`.
pub const THRESHOLD: f32 = 0.78;
/// "Not now" lasts this long.
const SNOOZE_DAYS: i64 = 30;
/// Chats remembered as already asked.
const KEEP_CHATS: usize = 50;
/// After a change in the library, the index waits this long for quiet before it follows.
const QUIET: Duration = Duration::from_secs(10);
/// A chat's note waits at most this long for the notes that may relate.
pub(crate) const NOTE_BUDGET: Duration = Duration::from_millis(400);
const NOTE_PATHS: usize = 8;
const QUERY_CHARS: usize = 300;
const SNIPPET_CHARS: usize = 180;
/// The library's size is counted again at most this often (for the "large library" suggestion).
const SIZE_FRESH: Duration = Duration::from_secs(60);

pub(crate) const MARKER_NOTE: &str = "If you could not find notes the user refers to because their wording may differ from the notes', end your answer with a line holding only `[[thoughts:meaning-search]]`. The app then offers the user its search by meaning; do not mention the line or the offer yourself.";

// ---------- the work in the background ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase { Idle, Downloading, Indexing, Ready, Error }

struct Work {
    phase: Phase,
    running: bool,
    cancel: Arc<AtomicBool>,
    got: u64,
    total: u64,
    done: usize,
    todo: usize,
    error: Option<String>,
}

fn work() -> &'static Mutex<Work> {
    static W: OnceLock<Mutex<Work>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(Work { phase: Phase::Idle, running: false, cancel: Arc::new(AtomicBool::new(false)), got: 0, total: 0, done: 0, todo: 0, error: None }))
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> { m.lock().unwrap_or_else(|e| e.into_inner()) }

/// The model, once loaded.
fn engine_slot() -> &'static Mutex<Option<Arc<dyn Embedder>>> {
    static E: OnceLock<Mutex<Option<Arc<dyn Embedder>>>> = OnceLock::new();
    E.get_or_init(Default::default)
}
/// The open library's index, once read.
fn index_slot() -> &'static Mutex<Option<(PathBuf, Arc<RwLock<Index>>)>> {
    static I: OnceLock<Mutex<Option<(PathBuf, Arc<RwLock<Index>>)>>> = OnceLock::new();
    I.get_or_init(Default::default)
}
/// Chats with an open suggestion: chat id → the answer it is shown under.
fn offers() -> &'static Mutex<HashMap<String, String>> {
    static O: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    O.get_or_init(Default::default)
}
fn size_cache() -> &'static Mutex<Option<(PathBuf, Instant, usize)>> {
    static S: OnceLock<Mutex<Option<(PathBuf, Instant, usize)>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Where the files come from (tests serve them from this computer).
fn base_url() -> String {
    #[cfg(test)]
    if let Some(b) = lock(tests::base()).clone() { return b; }
    model::base_url()
}

fn loaded_engine(home: &Path) -> anyhow::Result<Arc<dyn Embedder>> {
    if let Some(e) = lock(engine_slot()).clone() { return Ok(e); }
    let e: Arc<dyn Embedder> = Arc::new(model::E5::load(home)?);
    *lock(engine_slot()) = Some(e.clone());
    Ok(e)
}

fn index_for(home: &Path, root: &Path) -> Arc<RwLock<Index>> {
    let mut slot = lock(index_slot());
    if let Some((r, ix)) = slot.as_ref() { if r == root { return ix.clone(); } }
    let ix = Arc::new(RwLock::new(Index::open(&index::dir_for(home, root), &model::tag())));
    *slot = Some((root.to_path_buf(), ix.clone()));
    ix
}

/// The open library's index, if it has been read and gone through once.
fn ready_index(root: &Path) -> Option<Arc<RwLock<Index>>> {
    let slot = lock(index_slot());
    let (r, ix) = slot.as_ref()?;
    (r == root && ix.read().unwrap_or_else(|e| e.into_inner()).updated.is_some()).then(|| ix.clone())
}

fn set_phase(st: &AppState, p: Phase) {
    lock(work()).phase = p;
    st.emit(json!({"type": "meaning.state"}));
}

/// At the daemon's start: go on with what was switched on (and not paused). Nothing at all when off.
pub fn start(st: Arc<AppState>) {
    tokio::spawn(async move {
        let m = st.settings.read().await.search.meaning.clone();
        if m.on && !m.paused { ensure(st); }
    });
}

/// Start the background work unless it is running already.
fn ensure(st: Arc<AppState>) {
    let cancel = {
        let mut w = lock(work());
        if w.running { return; }
        w.running = true;
        w.error = None;
        w.cancel = Arc::new(AtomicBool::new(false));
        w.cancel.clone()
    };
    tokio::spawn(async move {
        let r = run(&st, &crate::paths::home(), &cancel).await;
        {
            let mut w = lock(work());
            w.running = false;
            match r {
                Ok(()) => {}
                Err(e) if e.downcast_ref::<model::Stopped>().is_some() => w.phase = Phase::Idle,
                Err(e) => {
                    crate::log::warn(format!("search by meaning: {e:#}"));
                    w.phase = Phase::Error;
                    w.error = Some(e.to_string());
                }
            }
        }
        st.emit(json!({"type": "meaning.state"}));
    });
}

/// Fetch the model if needed, then keep the open library's index up to date until cancelled.
async fn run(st: &Arc<AppState>, home: &Path, cancel: &Arc<AtomicBool>) -> anyhow::Result<()> {
    if !model::installed(home) {
        { let mut w = lock(work()); w.got = model::got_so_far(home); w.total = model::total_bytes(); }
        set_phase(st, Phase::Downloading);
        let (h, c, tx, base) = (home.to_path_buf(), cancel.clone(), st.events.clone(), base_url());
        tokio::task::spawn_blocking(move || model::download(&h, &base, &c, &mut |got, total| {
            { let mut w = lock(work()); w.got = got; w.total = total; }
            let _ = tx.send(json!({"type": "meaning.progress", "phase": "download", "got": got, "total": total}));
        })).await??;
    }
    { let mut w = lock(work()); w.done = 0; w.todo = 0; }
    set_phase(st, Phase::Indexing);
    let h = home.to_path_buf();
    let emb = tokio::task::spawn_blocking(move || loaded_engine(&h)).await??;
    // Changes are listened for from before the first pass, so none made during it is missed.
    let mut rx = st.events.subscribe();
    loop {
        if cancel.load(Ordering::Relaxed) { return Err(model::Stopped::Cancelled.into()); }
        let (root, private) = { let s = st.settings.read().await; (s.library().filter(|l| l.is_dir()), s.private_paths.clone()) };
        if let Some(root) = root {
            let ix = index_for(home, &root);
            set_phase(st, Phase::Indexing);
            let (c, e, tx) = (cancel.clone(), emb.clone(), st.events.clone());
            let pass = tokio::task::spawn_blocking(move || {
                let mut last = Instant::now() - Duration::from_secs(1);
                index::run_pass(&ix, &root, &private, e.as_ref(), &c, &mut |done, total| {
                    { let mut w = lock(work()); w.done = done; w.todo = total; }
                    if done == total || last.elapsed() >= Duration::from_millis(250) {
                        last = Instant::now();
                        let _ = tx.send(json!({"type": "meaning.progress", "phase": "index", "done": done, "total": total}));
                    }
                })
            }).await??;
            if pass.cancelled { return Err(model::Stopped::Cancelled.into()); }
        }
        set_phase(st, Phase::Ready);
        wait_for_change(&mut rx, cancel).await?;
    }
}

/// Wait until the library changed and then stayed quiet for `QUIET`.
async fn wait_for_change(rx: &mut tokio::sync::broadcast::Receiver<Value>, cancel: &AtomicBool) -> anyhow::Result<()> {
    let mut since: Option<Instant> = None;
    loop {
        if cancel.load(Ordering::Relaxed) { return Err(model::Stopped::Cancelled.into()); }
        if since.is_some_and(|t| t.elapsed() >= QUIET) { return Ok(()); }
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Ok(v)) => {
                if matches!(v["type"].as_str(), Some("library.changed" | "library.switched" | "settings.changed")) { since = Some(Instant::now()); }
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => since = Some(Instant::now()),
            Ok(Err(_)) => return Err(model::Stopped::Cancelled.into()),
            Err(_) => {}
        }
    }
}

/// Stop the background work and wait (up to half a minute) until it has stopped.
async fn stop() {
    lock(work()).cancel.store(true, Ordering::Relaxed);
    for _ in 0..600 {
        if !lock(work()).running { return; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn disk_bytes(home: &Path) -> u64 {
    let d = home.join("meaning");
    if std::fs::symlink_metadata(&d).is_err() { return 0; }
    walkdir::WalkDir::new(d).into_iter().flatten().filter(|e| e.file_type().is_file()).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()
}

// ---------- suggestions ----------

fn snoozed(m: &crate::settings::MeaningSettings) -> bool {
    m.snooze_until.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).is_some_and(|t| t > chrono::Utc::now())
}
/// Is it not to be suggested now ("Don't ask again", or "Not now" not yet over)?
fn quiet(m: &crate::settings::MeaningSettings) -> bool { m.never || snoozed(m) }

/// Text files in the library, counted at most once a minute.
fn library_size(root: &Path, private: &[String]) -> usize {
    if let Some((r, at, n)) = lock(size_cache()).as_ref() { if r == root && at.elapsed() < SIZE_FRESH { return *n; } }
    let n = index::walk(root, private).len();
    *lock(size_cache()) = Some((root.to_path_buf(), Instant::now(), n));
    n
}

/// What `claude::push` does with an assistant's answer: in an ordinary chat, a line holding only
/// the marker is taken out and, at most once per chat and only while search by meaning is off and
/// not declined, becomes a suggestion (an event the caller sends after the message). Every other
/// message, and the marker in every other kind of run, passes as it is.
pub async fn screen(st: &AppState, chat_id: &str, mut m: Msg) -> (Msg, Option<Value>) {
    if m.role != "assistant" || !m.text.contains(MARKER) { return (m, None); }
    if st.chats.meta(chat_id).await.is_none_or(|c| c.kind != "chat") { return (m, None); }
    m.text = strip_marker(&m.text);
    let mut s = st.settings.write().await;
    let ms = &mut s.search.meaning;
    if ms.on || quiet(ms) || ms.suggested_chats.iter().any(|c| c == chat_id) { return (m, None); }
    ms.suggested_chats.push(chat_id.to_string());
    let n = ms.suggested_chats.len();
    if n > KEEP_CHATS { ms.suggested_chats.drain(..n - KEEP_CHATS); }
    crate::log::ok_or_warn("search by meaning: could not save the suggestion", s.save());
    drop(s);
    lock(offers()).insert(chat_id.to_string(), m.id.clone());
    let ev = json!({"type": "meaning.suggest", "chatId": chat_id, "messageId": m.id});
    (m, Some(ev))
}

/// The text without the lines that hold only the marker.
pub fn strip_marker(text: &str) -> String {
    let kept: Vec<&str> = text.split('\n').filter(|l| l.trim() != MARKER).collect();
    kept.join("\n").trim_end().to_string()
}

// ---------- what a run is told ----------

/// What a run is told about searching by meaning (`run_notes::all`), in ordinary chats in a
/// library only. Off and not declined: how to ask for it. Ready: the notes that may relate.
pub async fn run_note(state: &AppState, kind: &str, prompt: &str, library: Option<&Path>) -> Option<String> {
    if kind != "chat" { return None; }
    let root = library?.to_path_buf();
    let (m, private) = { let s = state.settings.read().await; (s.search.meaning.clone(), s.private_paths.clone()) };
    if !m.on { return (!quiet(&m)).then(|| MARKER_NOTE.to_string()); }
    let emb = lock(engine_slot()).clone()?;
    let ix = ready_index(&root)?;
    let q: String = prompt.chars().take(QUERY_CHARS * 4).collect();
    let found = tokio::time::timeout(NOTE_BUDGET, tokio::task::spawn_blocking(move || -> Option<Vec<String>> {
        let qv = emb.embed(&[model::query_text(&q)]).ok()?.pop()?;
        let ix = ix.read().unwrap_or_else(|e| e.into_inner());
        Some(ix.top(&qv, NOTE_PATHS * 2, THRESHOLD).into_iter().map(|(i, _)| ix.chunks[i].path.clone())
            .filter(|p| !crate::paths::is_private(&root, p, &private)).take(NOTE_PATHS).collect())
    })).await.ok()?.ok()??;
    if found.is_empty() { return None; }
    let list = found.iter().map(|p| format!("`{}`", p.replace('`', "'"))).collect::<Vec<_>>().join(", ");
    Some(format!("This library has search by meaning. Notes that may relate to the user's message (paths only; read them with your file tools if they help): {list}."))
}

/// The private folders changed (`PUT /api/settings`): what is now under one leaves the index at once.
pub async fn private_changed(st: &AppState) {
    let (on, root, private) = { let s = st.settings.read().await; (s.search.meaning.on, s.library(), s.private_paths.clone()) };
    let Some(root) = root.filter(|_| on) else { return };
    let home = crate::paths::home();
    if !index::dir_for(&home, &root).exists() && lock(index_slot()).is_none() { return; }
    let _ = tokio::task::spawn_blocking(move || {
        let ix = index_for(&home, &root);
        let mut w = ix.write().unwrap_or_else(|e| e.into_inner());
        if w.drop_where(|p| crate::paths::is_under(p, &private) || crate::paths::is_private(&root, p, &private)) > 0 {
            crate::log::ok_or_warn("search by meaning: could not save the index", w.save());
        }
    }).await;
}

// ---------- routes ----------

type R = Result<Json<Value>, crate::api::ApiErr>;
type Q = Query<HashMap<String, String>>;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/meaning", get(info))
        .route("/api/meaning/install", post(install))
        .route("/api/meaning/pause", post(pause))
        .route("/api/meaning/rebuild", post(rebuild))
        .route("/api/meaning/remove", post(remove))
        .route("/api/meaning/search", get(search))
        .route("/api/meaning/offer", post(offer))
}

/// Never for add-ons (`scope=plugin`), like `/api/search`. Access keys reach none of these routes
/// (`tokens::ROUTES` names none of them).
fn no_addons(q: &HashMap<String, String>, path: &str) -> anyhow::Result<()> {
    if q.get("scope").map(String::as_str) == Some("plugin") { return Err(crate::tr::err("plugin.not_allowed", &[("path", path)])); }
    Ok(())
}

/// `GET /api/meaning`: where it stands.
async fn info(State(st): State<Arc<AppState>>, Query(q): Q) -> R {
    no_addons(&q, "/api/meaning")?;
    let (m, root, private, threshold) = {
        let s = st.settings.read().await;
        (s.search.meaning.clone(), s.library().filter(|l| l.is_dir()), s.private_paths.clone(), s.search.file_threshold.max(1))
    };
    let home = crate::paths::home();
    let (phase, running, got, total, done, todo, error) = {
        let w = lock(work());
        (w.phase, w.running, w.got, w.total, w.done, w.todo, w.error.clone())
    };
    let state = if !m.on { "off" } else {
        match phase {
            Phase::Downloading if running => "downloading",
            Phase::Indexing if running => "indexing",
            Phase::Ready if running => "ready",
            Phase::Error => "error",
            _ if m.paused => "paused",
            // Switched on, but nothing is running (the start has not happened yet): go on now.
            _ => { ensure(st.clone()); if model::installed(&home) { "indexing" } else { "downloading" } }
        }
    };
    let mut v = json!({
        "state": state,
        "model": {"id": model::ID, "name": model::NAME, "bytes": model::total_bytes(), "license": model::LICENSE, "languages": model::LANGUAGES},
        "diskBytes": disk_bytes(&home),
        "suggest": Value::Null,
        "quiet": quiet(&m),
        "offers": lock(offers()).iter().map(|(c, id)| json!({"chatId": c, "messageId": id})).collect::<Vec<_>>(),
    });
    if m.on && !model::installed(&home) {
        let got = if running && phase == Phase::Downloading { got } else { model::got_so_far(&home) };
        v["download"] = json!({"got": got, "total": if total > 0 { total } else { model::total_bytes() }});
    }
    if m.on {
        if let Some(root) = &root {
            let slot = lock(index_slot());
            if let Some((_, ix)) = slot.as_ref().filter(|(r, _)| r == root) {
                let ix = ix.read().unwrap_or_else(|e| e.into_inner());
                v["index"] = json!({"files": ix.files.len(), "chunks": ix.chunks.len(), "done": done, "total": todo, "updated": ix.updated});
            }
        }
    }
    if let Some(e) = error.filter(|_| state == "error") { v["error"] = json!(e); }
    if !m.on && !quiet(&m) {
        let files = match root.clone() {
            Some(r) => tokio::task::spawn_blocking(move || library_size(&r, &private)).await.unwrap_or(0),
            None => 0,
        };
        if files >= threshold { v["suggest"] = json!({"why": "size", "files": files}); }
        else if !lock(offers()).is_empty() { v["suggest"] = json!({"why": "chat"}); }
    }
    Ok(Json(v))
}

/// `POST /api/meaning/install`: switch it on, then fetch (or go on fetching) the model, then index.
async fn install(State(st): State<Arc<AppState>>, Query(q): Q) -> R {
    no_addons(&q, "/api/meaning/install")?;
    if crate::settings::read_only() || st.settings_newer.load(Ordering::Relaxed) { return Err(crate::tr::err("newer_settings", &[]).into()); }
    {
        let mut s = st.settings.write().await;
        s.search.meaning.on = true;
        s.search.meaning.paused = false;
        s.save()?;
    }
    lock(offers()).clear();
    {
        // What failed is tried again, from where it was.
        let mut w = lock(work());
        if w.phase == Phase::Error { w.phase = Phase::Idle; w.error = None; }
    }
    ensure(st.clone());
    st.emit(json!({"type": "meaning.state"}));
    Ok(Json(json!({"ok": true})))
}

/// `POST /api/meaning/pause`: stop for now, keeping what was fetched and indexed.
async fn pause(State(st): State<Arc<AppState>>, Query(q): Q) -> R {
    no_addons(&q, "/api/meaning/pause")?;
    {
        let mut s = st.settings.write().await;
        if !s.search.meaning.on { return Ok(Json(json!({"ok": true}))); }
        s.search.meaning.paused = true;
        s.save()?;
    }
    stop().await;
    st.emit(json!({"type": "meaning.state"}));
    Ok(Json(json!({"ok": true})))
}

/// `POST /api/meaning/rebuild`: forget the open library's index and make it again.
async fn rebuild(State(st): State<Arc<AppState>>, Query(q): Q) -> R {
    no_addons(&q, "/api/meaning/rebuild")?;
    let (on, root) = { let s = st.settings.read().await; (s.search.meaning.on, s.library()) };
    if !on { return Err(crate::tr::err("meaning.not_ready", &[]).into()); }
    stop().await;
    *lock(index_slot()) = None;
    if let Some(root) = root {
        let d = index::dir_for(&crate::paths::home(), &root);
        if d.exists() { std::fs::remove_dir_all(&d)?; }
    }
    { let mut s = st.settings.write().await; s.search.meaning.paused = false; s.save()?; }
    ensure(st.clone());
    st.emit(json!({"type": "meaning.state"}));
    Ok(Json(json!({"ok": true})))
}

/// `POST /api/meaning/remove`: stop, delete the model and every index, switch it off. → `{ok, freed}`.
/// Also what "Cancel" during the download does.
async fn remove(State(st): State<Arc<AppState>>, Query(q): Q) -> R {
    no_addons(&q, "/api/meaning/remove")?;
    stop().await;
    let home = crate::paths::home();
    let freed = disk_bytes(&home);
    *lock(engine_slot()) = None;
    *lock(index_slot()) = None;
    let d = home.join("meaning");
    if std::fs::symlink_metadata(&d).is_ok() {
        tokio::task::spawn_blocking(move || std::fs::remove_dir_all(d)).await??;
    }
    {
        let mut w = lock(work());
        w.phase = Phase::Idle;
        w.error = None;
        (w.got, w.total, w.done, w.todo) = (0, 0, 0, 0);
    }
    {
        let mut s = st.settings.write().await;
        s.search.meaning.on = false;
        s.search.meaning.paused = false;
        s.save()?;
    }
    st.emit(json!({"type": "meaning.state"}));
    Ok(Json(json!({"ok": true, "freed": freed})))
}

/// `GET /api/meaning/search?q=&limit=` → `{results: [{path, title, score, snippet, line}]}`; 409
/// `meaning.not_ready` until the model and the open library's index are there. Private folders
/// are never in the index; each path is checked against them again here.
async fn search(State(st): State<Arc<AppState>>, Query(q): Q) -> R {
    no_addons(&q, "/api/meaning/search")?;
    let (on, root, private) = { let s = st.settings.read().await; (s.search.meaning.on, s.library(), s.private_paths.clone()) };
    let not_ready = || crate::api::ApiErr::from(crate::tr::err("meaning.not_ready", &[]));
    let root = root.filter(|_| on).ok_or_else(not_ready)?;
    let emb = lock(engine_slot()).clone().ok_or_else(not_ready)?;
    let ix = ready_index(&root).ok_or_else(not_ready)?;
    let text: String = q.get("q").map(String::as_str).unwrap_or("").trim().chars().take(QUERY_CHARS).collect();
    if text.is_empty() { return Ok(Json(json!({"results": []}))); }
    let limit = q.get("limit").and_then(|l| l.parse::<usize>().ok()).unwrap_or(10).clamp(1, 30);
    let results = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Value>> {
        let qv = emb.embed(&[model::query_text(&text)])?.pop().unwrap_or_default();
        let ix = ix.read().unwrap_or_else(|e| e.into_inner());
        let mut out = vec![];
        for (i, score) in ix.top(&qv, limit * 2, THRESHOLD) {
            let c = &ix.chunks[i];
            if crate::paths::is_private(&root, &c.path, &private) { continue; }
            let (snippet, line, title) = match index::snippet(&root, c, SNIPPET_CHARS) {
                Some((s, l, t)) => (Some(s), Some(l), t),
                None => (None, None, index::title(&c.path, "")),
            };
            out.push(json!({"path": c.path, "title": title, "score": (score * 1000.0).round() / 1000.0, "snippet": snippet, "line": line}));
            if out.len() >= limit { break; }
        }
        Ok(out)
    }).await??;
    Ok(Json(json!({"results": results})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfferReq { op: String }

/// `POST /api/meaning/offer {op: 'later'|'never', chatId?}`: "Not now" (30 days) or "Don't ask
/// again". Either hides every open suggestion.
async fn offer(State(st): State<Arc<AppState>>, Query(q): Q, Json(b): Json<OfferReq>) -> R {
    no_addons(&q, "/api/meaning/offer")?;
    {
        let mut s = st.settings.write().await;
        match b.op.as_str() {
            "later" => s.search.meaning.snooze_until = Some((chrono::Utc::now() + chrono::Duration::days(SNOOZE_DAYS)).to_rfc3339()),
            "never" => s.search.meaning.never = true,
            _ => return Err(anyhow::anyhow!("unknown op").into()),
        }
        s.save()?;
    }
    lock(offers()).clear();
    st.emit(json!({"type": "meaning.state"}));
    Ok(Json(json!({"ok": true})))
}
