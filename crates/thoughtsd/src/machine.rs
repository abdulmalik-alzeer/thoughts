//! The machine bridge: what pages and add-ons may ask of the computer while full access is on
//! (`full_access.rs`). Off — the default — every route here refuses flatly (`full_access.off`),
//! whatever the page says: the check is this daemon's, made on the saved settings for each request.
//!
//! The add-on frames reach it only through the app window's host (`ui/src/lib/layer.ts`, methods
//! `machine.*`), which forwards their requests with the app token; access keys for other programs
//! never reach it (`tokens::admit` opens a fixed list of routes). Documented for add-on authors in
//! docs/CUSTOMISING.md ("Full access: the machine bridge").
//!
//! - `GET  /api/machine` → `{on, platform}`.
//! - `POST /api/machine/run` `{program, args?, cwd?, stdin?, env?, timeoutSecs?, stream?}`: runs a
//!   program directly (no shell, unless the program named is one: `sh -c …`). Without `stream`: waits
//!   and answers `{code, stdout, stderr, timedOut}` (each output kept to `OUTPUT_MAX`). With `stream`:
//!   answers `{id}` at once, then the events `machine.output` `{id, stream, data}` and `machine.exit`
//!   `{id, code}` follow on the event socket.
//! - `POST /api/machine/kill` `{id}`: stops a streamed command.
//! - `POST /api/machine/read` `{path, base64?}` → `{content}`; `POST /api/machine/write`
//!   `{path, content, base64?, append?}`; `POST /api/machine/list` `{path}` → `{entries:[{name,dir,size}]}`.
//! - `POST /api/machine/open` `{target, app?}`: opens a web address, a file or a folder in the
//!   program the system chooses, or in the app named (macOS `open -a`).

use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type S = State<Arc<AppState>>;
type R = Result<Json<Value>, crate::api::ApiErr>;

/// Most of one output kept for a command that is waited for.
pub const OUTPUT_MAX: usize = 16 * 1024 * 1024;
/// The longest a command that is waited for may take unless it says otherwise.
const DEFAULT_TIMEOUT: u64 = 120;
/// The longest any command may be given.
const MAX_TIMEOUT: u64 = 24 * 60 * 60;
/// The largest file read or written in one request.
const FILE_MAX: usize = 64 * 1024 * 1024;

/// Streamed commands still running, by id: what stops them.
fn running() -> &'static Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>> {
    static M: OnceLock<Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

/// Stop every command a page started (full access was switched off).
pub async fn stop_all() {
    let all: Vec<_> = running().lock().unwrap_or_else(|e| e.into_inner()).drain().collect();
    for (_, k) in all { let _ = k.send(()); }
}

/// The one check every route makes first.
async fn require_on(st: &AppState) -> anyhow::Result<()> {
    if st.settings.read().await.full_access() { Ok(()) } else { Err(crate::tr::err("full_access.off", &[])) }
}

fn bad(why: &str) -> anyhow::Error { crate::tr::err("machine.bad_request", &[("why", why)]) }

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/machine", get(info))
        .route("/api/machine/run", post(run))
        .route("/api/machine/kill", post(kill))
        .route("/api/machine/read", post(read))
        .route("/api/machine/write", post(write))
        .route("/api/machine/list", post(list))
        .route("/api/machine/open", post(open))
}

async fn info(State(st): S) -> R {
    Ok(Json(json!({"on": st.settings.read().await.full_access(), "platform": std::env::consts::OS})))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct RunBody {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub stdin: Option<String>,
    pub env: HashMap<String, String>,
    pub timeout_secs: Option<u64>,
    pub stream: bool,
}

/// A path the bridge was given: `~` expanded; a relative path is taken from the user's home folder.
fn path_of(raw: &str) -> anyhow::Result<PathBuf> {
    if raw.trim().is_empty() || raw.contains('\0') { return Err(bad("no path was given")); }
    let p = crate::paths::expand_tilde(raw.trim())?;
    Ok(if p.is_absolute() { p } else { dirs::home_dir().unwrap_or_default().join(p) })
}

fn command_for(b: &RunBody) -> anyhow::Result<tokio::process::Command> {
    if b.program.trim().is_empty() || b.program.contains('\0') || b.args.iter().any(|a| a.contains('\0')) { return Err(bad("no program was named")); }
    let mut c = crate::proc::command(b.program.trim());
    c.args(&b.args);
    c.current_dir(match &b.cwd { Some(d) if !d.trim().is_empty() => path_of(d)?, _ => dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")) });
    for (k, v) in &b.env { if !k.is_empty() && !k.contains('=') && !k.contains('\0') && !v.contains('\0') { c.env(k, v); } }
    c.stdin(if b.stdin.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() })
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    Ok(c)
}

/// Read all of `r`, keeping at most `max` bytes (the rest is read and dropped).
async fn read_capped(mut r: impl tokio::io::AsyncRead + Unpin, max: usize) -> String {
    let (mut out, mut buf) = (Vec::new(), [0u8; 8192]);
    while let Ok(n) = r.read(&mut buf).await {
        if n == 0 { break; }
        let room = max.saturating_sub(out.len());
        out.extend_from_slice(&buf[..n.min(room)]);
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn run(State(st): S, Json(b): Json<RunBody>) -> R {
    require_on(&st).await?;
    let mut c = command_for(&b)?;
    let mut child = c.spawn().map_err(|e| bad(&e.to_string()))?;
    if let (Some(text), Some(mut stdin)) = (b.stdin.clone(), child.stdin.take()) {
        tokio::spawn(async move { let _ = stdin.write_all(text.as_bytes()).await; let _ = stdin.shutdown().await; });
    }
    let (out, err) = (child.stdout.take(), child.stderr.take());
    crate::log::info(format!("machine bridge: started {} ({} argument(s)){}", b.program.trim(), b.args.len(), if b.stream { ", streamed" } else { "" }));
    if !b.stream {
        let limit = std::time::Duration::from_secs(b.timeout_secs.unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT));
        let reading = async {
            let (o, e) = tokio::join!(
                async { match out { Some(o) => read_capped(o, OUTPUT_MAX).await, None => String::new() } },
                async { match err { Some(e) => read_capped(e, OUTPUT_MAX).await, None => String::new() } });
            (o, e, child.wait().await)
        };
        return match tokio::time::timeout(limit, reading).await {
            Ok((stdout, stderr, status)) => Ok(Json(json!({"code": status.ok().and_then(|s| s.code()), "stdout": stdout, "stderr": stderr, "timedOut": false}))),
            // Dropping the child ends it (`kill_on_drop`).
            Err(_) => Ok(Json(json!({"code": null, "stdout": "", "stderr": "", "timedOut": true}))),
        };
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
    running().lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), tx);
    let limit = b.timeout_secs.map(|s| std::time::Duration::from_secs(s.clamp(1, MAX_TIMEOUT)));
    let (st2, id2) = (st.clone(), id.clone());
    tokio::spawn(async move {
        let pump = |r: Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>>, name: &'static str| {
            let (st, id) = (st2.clone(), id2.clone());
            async move {
                let Some(mut r) = r else { return };
                let mut buf = vec![0u8; 16 * 1024];
                while let Ok(n) = r.read(&mut buf).await {
                    if n == 0 { break; }
                    st.emit(json!({"type": "machine.output", "id": id, "stream": name, "data": String::from_utf8_lossy(&buf[..n])}));
                }
            }
        };
        let out: Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>> = out.map(|o| Box::new(o) as _);
        let err: Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>> = err.map(|e| Box::new(e) as _);
        let pumps = futures::future::join(pump(out, "stdout"), pump(err, "stderr"));
        let waited = async { pumps.await; child.wait().await.ok().and_then(|s| s.code()) };
        let code = tokio::select! {
            c = waited => c,
            _ = &mut rx => None,
            _ = async { match limit { Some(l) => tokio::time::sleep(l).await, None => futures::future::pending::<()>().await } } => None,
        };
        running().lock().unwrap_or_else(|e| e.into_inner()).remove(&id2);
        st2.emit(json!({"type": "machine.exit", "id": id2, "code": code}));
    });
    Ok(Json(json!({"id": id})))
}

#[derive(Deserialize)]
struct IdBody { id: String }

async fn kill(State(st): S, Json(b): Json<IdBody>) -> R {
    require_on(&st).await?;
    let k = running().lock().unwrap_or_else(|e| e.into_inner()).remove(&b.id);
    let found = k.map(|k| k.send(()).is_ok()).is_some();
    Ok(Json(json!({"stopped": found})))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ReadBody { path: String, base64: bool }

async fn read(State(st): S, Json(b): Json<ReadBody>) -> R {
    require_on(&st).await?;
    let p = path_of(&b.path)?;
    let len = tokio::fs::metadata(&p).await.map_err(|e| bad(&e.to_string()))?.len() as usize;
    if len > FILE_MAX { return Err(bad("the file is too large to read in one go").into()); }
    let bytes = tokio::fs::read(&p).await.map_err(|e| bad(&e.to_string()))?;
    let content = if b.base64 { base64::engine::general_purpose::STANDARD.encode(&bytes) } else { String::from_utf8_lossy(&bytes).into_owned() };
    Ok(Json(json!({"content": content, "path": p.to_string_lossy()})))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct WriteBody { path: String, content: String, base64: bool, append: bool }

async fn write(State(st): S, Json(b): Json<WriteBody>) -> R {
    require_on(&st).await?;
    let p = path_of(&b.path)?;
    let bytes = if b.base64 { base64::engine::general_purpose::STANDARD.decode(b.content.trim()).map_err(|e| bad(&e.to_string()))? } else { b.content.into_bytes() };
    if bytes.len() > FILE_MAX { return Err(bad("the content is too large to write in one go").into()); }
    if let Some(parent) = p.parent() { tokio::fs::create_dir_all(parent).await.map_err(|e| bad(&e.to_string()))?; }
    if b.append {
        let mut f = tokio::fs::OpenOptions::new().create(true).append(true).open(&p).await.map_err(|e| bad(&e.to_string()))?;
        f.write_all(&bytes).await.map_err(|e| bad(&e.to_string()))?;
    } else {
        tokio::fs::write(&p, &bytes).await.map_err(|e| bad(&e.to_string()))?;
    }
    crate::log::info(format!("machine bridge: wrote {} byte(s) to {}", bytes.len(), p.display()));
    Ok(Json(json!({"ok": true, "path": p.to_string_lossy()})))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListBody { path: String }

async fn list(State(st): S, Json(b): Json<ListBody>) -> R {
    require_on(&st).await?;
    let p = path_of(&b.path)?;
    let mut rd = tokio::fs::read_dir(&p).await.map_err(|e| bad(&e.to_string()))?;
    let mut entries = vec![];
    while let Ok(Some(e)) = rd.next_entry().await {
        let m = e.metadata().await.ok();
        entries.push(json!({"name": e.file_name().to_string_lossy(), "dir": m.as_ref().map_or(false, |m| m.is_dir()), "size": m.map_or(0, |m| m.len())}));
        if entries.len() >= 10_000 { break; }
    }
    Ok(Json(json!({"entries": entries})))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OpenBody { target: String, app: Option<String> }

/// The command that opens `target` (in `app`, where the system can name one) on this system.
pub fn open_command(target: &str, app: Option<&str>) -> (String, Vec<String>) {
    let app = app.map(str::trim).filter(|a| !a.is_empty());
    if cfg!(target_os = "macos") {
        let mut a = vec![];
        if let Some(app) = app { a.extend(["-a".to_string(), app.to_string()]); }
        if !target.is_empty() { a.push(target.to_string()); }
        ("open".into(), a)
    } else if cfg!(windows) {
        let mut a = vec!["/c".to_string(), "start".into(), String::new()];
        if let Some(app) = app { a.push(app.to_string()); }
        if !target.is_empty() { a.push(target.to_string()); }
        ("cmd".into(), a)
    } else {
        match app { Some(app) => (app.to_string(), if target.is_empty() { vec![] } else { vec![target.to_string()] }), None => ("xdg-open".into(), vec![target.to_string()]) }
    }
}

async fn open(State(st): S, Json(b): Json<OpenBody>) -> R {
    require_on(&st).await?;
    if b.target.trim().is_empty() && b.app.as_deref().map_or(true, |a| a.trim().is_empty()) { return Err(bad("nothing to open was named").into()); }
    if b.target.contains('\0') { return Err(bad("nothing to open was named").into()); }
    let (prog, args) = open_command(b.target.trim(), b.app.as_deref());
    let out = crate::proc::command(&prog).args(&args).stdin(std::process::Stdio::null()).output().await.map_err(|e| bad(&e.to_string()))?;
    if !out.status.success() { return Err(bad(String::from_utf8_lossy(&out.stderr).trim()).into()); }
    Ok(Json(json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_route_refuses_while_full_access_is_off() {
        let st = Arc::new(AppState::for_test());
        assert!(!st.settings.read().await.full_access());
        let code = |r: R| match r { Ok(_) => 200, Err(e) => crate::api::shown_status(&e) };
        let run_body = || Json(RunBody { program: "echo".into(), args: vec!["hi".into()], ..Default::default() });
        assert_eq!(code(run(State(st.clone()), run_body()).await), 403);
        assert_eq!(code(read(State(st.clone()), Json(ReadBody { path: "/etc/hosts".into(), base64: false })).await), 403);
        let target = std::env::temp_dir().join(format!("machine-off-{}", uuid::Uuid::new_v4()));
        assert_eq!(code(write(State(st.clone()), Json(WriteBody { path: target.to_string_lossy().into(), content: "x".into(), ..Default::default() })).await), 403);
        assert!(!target.exists(), "nothing was written");
        assert_eq!(code(list(State(st.clone()), Json(ListBody { path: "/".into() })).await), 403);
        assert_eq!(code(open(State(st.clone()), Json(OpenBody { target: "https://example.com".into(), app: None })).await), 403);
        assert_eq!(code(kill(State(st.clone()), Json(IdBody { id: "x".into() })).await), 403);
        assert_eq!(info(State(st.clone())).await.unwrap().0["on"], false);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn with_full_access_on_it_runs_reads_and_writes() {
        let st = Arc::new(AppState::for_test());
        st.settings.write().await.full_access.on = true;
        let r = run(State(st.clone()), Json(RunBody { program: "sh".into(), args: vec!["-c".into(), "echo out; echo err >&2; exit 3".into()], ..Default::default() })).await.unwrap().0;
        assert_eq!((r["code"].as_i64(), r["stdout"].as_str(), r["stderr"].as_str()), (Some(3), Some("out\n"), Some("err\n")));
        let fed = run(State(st.clone()), Json(RunBody { program: "cat".into(), stdin: Some("typed".into()), ..Default::default() })).await.unwrap().0;
        assert_eq!(fed["stdout"], "typed");
        let slow = run(State(st.clone()), Json(RunBody { program: "sleep".into(), args: vec!["5".into()], timeout_secs: Some(1), ..Default::default() })).await.unwrap().0;
        assert_eq!(slow["timedOut"], true);
        let dir = std::env::temp_dir().join(format!("machine-on-{}", uuid::Uuid::new_v4()));
        let f = dir.join("sub/a.txt");
        let _ = write(State(st.clone()), Json(WriteBody { path: f.to_string_lossy().into(), content: "one".into(), ..Default::default() })).await.unwrap();
        let _ = write(State(st.clone()), Json(WriteBody { path: f.to_string_lossy().into(), content: "dHdv".into(), base64: true, append: true })).await.unwrap();
        let back = read(State(st.clone()), Json(ReadBody { path: f.to_string_lossy().into(), base64: false })).await.unwrap().0;
        assert_eq!(back["content"], "onetwo");
        let ls = list(State(st.clone()), Json(ListBody { path: dir.join("sub").to_string_lossy().into() })).await.unwrap().0;
        assert_eq!(ls["entries"][0]["name"], "a.txt");
        // A streamed command answers with an id at once; its output and end come as events.
        let mut events = st.events.subscribe();
        let id = run(State(st.clone()), Json(RunBody { program: "sh".into(), args: vec!["-c".into(), "echo streamed".into()], stream: true, ..Default::default() })).await.unwrap().0["id"].as_str().unwrap().to_string();
        let (mut said, mut code) = (String::new(), None);
        while let Ok(Ok(ev)) = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv()).await {
            if ev["id"] != id.as_str() { continue; }
            if ev["type"] == "machine.output" { said.push_str(ev["data"].as_str().unwrap()); }
            if ev["type"] == "machine.exit" { code = ev["code"].as_i64(); break; }
        }
        assert_eq!((said.as_str(), code), ("streamed\n", Some(0)));
        // Switching full access off stops what is still running.
        let long = run(State(st.clone()), Json(RunBody { program: "sleep".into(), args: vec!["30".into()], stream: true, ..Default::default() })).await.unwrap().0["id"].as_str().unwrap().to_string();
        assert!(running().lock().unwrap().contains_key(&long));
        stop_all().await;
        assert!(!running().lock().unwrap().contains_key(&long));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn opening_uses_the_systems_own_opener() {
        let (p, a) = open_command("https://example.com", None);
        if cfg!(target_os = "macos") { assert_eq!((p.as_str(), a), ("open", vec!["https://example.com".to_string()])); }
        let (_, a) = open_command("", Some("Bluetooth File Exchange"));
        if cfg!(target_os = "macos") { assert_eq!(a, ["-a", "Bluetooth File Exchange"]); }
    }
}
