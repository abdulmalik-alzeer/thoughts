// Thoughts desktop shell.
//
// One executable, three modes:
//   * GUI (default)        — Tauri window; connects to (or spawns) the daemon
//   * `--daemon`           — runs thoughtsd, detached, so sessions outlive the UI
//   * THOUGHTS_ASKPASS=1   — git credential helper backed by the OS keychain
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod links;
mod quit;
mod recovery;

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

/// Version of this binary. The daemon is the same executable, so a running
/// daemon reporting anything else was left behind by an older install.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// "macos" | "windows" | "linux"
const PLATFORM: &str = std::env::consts::OS;

/// Set by "Quit Thoughts completely": stop the daemon on exit whatever the setting says.
static QUIT_ALL: AtomicBool = AtomicBool::new(false);
/// The stale-daemon check runs once per app launch.
static VERSION_CHECKED: AtomicBool = AtomicBool::new(false);
/// Serialises daemon start/stop so concurrent callers cannot spawn two.
static DAEMON_LOCK: Mutex<()> = Mutex::new(());
/// The label of the window that is "the" app window right now: `main` at first, then `main-2`,
/// `main-3`… for each window Recovery opened in its place (see `open_main_window`).
static MAIN_LABEL: Mutex<String> = Mutex::new(String::new());
/// How many app windows have been opened since the app started.
static WINDOWS_OPENED: AtomicU32 = AtomicU32::new(0);

/// Whether an updater public key and endpoint are configured in this build.
struct UpdaterConfigured(bool);

/// The live daemon and the version it reports, if one answers.
///
/// "Something answers on the port in daemon.json" is not enough: that file outlives a daemon that
/// was killed, and any program may have taken the port since. The responder has to prove that it
/// knows the token (`thoughtsd::hello`: an HMAC over a fresh random challenge). Until it has, it is
/// not the daemon, and the token is not sent to it: not by this function, and not by any caller,
/// because every caller gets the port and token from here.
fn daemon_alive() -> Option<(thoughtsd::paths::DaemonInfo, Option<String>)> {
    let info = thoughtsd::paths::read_daemon_info()?;
    let version = thoughtsd::hello::proven(info.port, &info.token)?;
    Some((info, Some(version)))
}

/// What to do about the daemon recorded in daemon.json when it has not proved itself.
#[derive(Debug, PartialEq, Eq)]
enum Unproven {
    /// Nothing: there is no old daemon to replace, or it cannot be told for certain.
    Leave,
    /// End this process: it is a daemon from before 0.0.9, which cannot answer the challenge.
    Stop(u32),
}

/// A daemon left running by an app older than 0.0.9 cannot prove itself, so it is never trusted and
/// never sent the token; but while it lives it holds the daemon lock, and no new daemon can start.
/// It is therefore replaced as a *process*, and only when all of this holds:
///   * the port answered the way an old daemon does (`Probe::Legacy`), not with silence;
///   * the daemon lock is held by another process. A free lock means no daemon is alive and the
///     recorded pid is stale; a lock that cannot be read means nothing is known. Either way: leave;
///   * the recorded pid is a running copy of this app started as the background service.
/// Nothing the responder on the port says is believed beyond "this looks like an old daemon".
fn unproven_daemon(
    probe: &thoughtsd::hello::Probe,
    lock: &std::io::Result<Option<std::fs::File>>,
    pid: u32,
    command: Option<&str>,
    exe: &str,
) -> Unproven {
    let held_by_another = matches!(lock, Ok(None));
    let ours = command.is_some_and(|c| is_daemon_command(c, exe));
    if *probe == thoughtsd::hello::Probe::Legacy && held_by_another && ours && pid > 1 && pid != std::process::id() {
        Unproven::Stop(pid)
    } else {
        Unproven::Leave
    }
}

/// Whether `command` (a process's command line) is this app running as the background service:
/// `<this program> --daemon`, or the stand-alone `thoughtsd`. `exe` is this program's file name.
/// On Windows only the program's file name is known (see `process_command`).
fn is_daemon_command(command: &str, exe: &str) -> bool {
    let name = |path: &str| path.rsplit(['/', '\\']).next().unwrap_or("").trim_end_matches(".exe").to_lowercase();
    let exe = name(&exe.to_lowercase());
    let command = command.trim();
    if exe.is_empty() || command.is_empty() {
        return false;
    }
    if cfg!(windows) {
        return name(&command.to_lowercase()) == exe;
    }
    match command.strip_suffix(" --daemon") {
        Some(program) => name(&program.to_lowercase()) == exe,
        None => name(&command.to_lowercase()) == "thoughtsd",
    }
}

/// The command line of process `pid` if it is running. Windows: its program file name only.
fn process_command(pid: u32) -> Option<String> {
    let out;
    #[cfg(unix)]
    {
        out = std::process::Command::new("ps").args(["-p", &pid.to_string(), "-o", "command="]).stderr(std::process::Stdio::null()).output().ok()?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // "thoughts.exe","1234","Console","1","12,345 K". UNTESTED on Windows.
        out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .creation_flags(0x08000000)
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    if cfg!(windows) {
        return line.strip_prefix('"').and_then(|l| l.split('"').next()).filter(|n| !n.is_empty()).map(String::from);
    }
    Some(line.to_string())
}

/// End process `pid`: politely, or with `force` at once.
fn end_process(pid: u32, force: bool) {
    let mut kill;
    #[cfg(unix)]
    {
        kill = std::process::Command::new("kill");
        if force {
            kill.arg("-KILL");
        }
        kill.arg(pid.to_string());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = force;
        kill = std::process::Command::new("taskkill");
        kill.args(["/PID", &pid.to_string(), "/F"]).creation_flags(0x08000000); // UNTESTED on Windows
    }
    let _ = kill.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
}

/// Stop a daemon from before 0.0.9 that is still running (see `unproven_daemon`), so that this
/// version's daemon can start. It is not asked whether it is busy: asking needs the token, and the
/// token is not sent to a daemon that cannot prove itself. A chat it was running shows as
/// interrupted afterwards, exactly as after a crash. Returns whether a process was stopped.
fn retire_unproven_daemon() -> bool {
    let Some(info) = thoughtsd::paths::read_daemon_info() else { return false };
    let probe = thoughtsd::hello::probe(info.port, &info.token);
    if probe != thoughtsd::hello::Probe::Legacy {
        return false;
    }
    let lock_file = thoughtsd::paths::lock_file();
    let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_default();
    let lock = thoughtsd::acquire_lock(&lock_file);
    let decision = unproven_daemon(&probe, &lock, info.pid, process_command(info.pid).as_deref(), &exe);
    drop(lock);
    let Unproven::Stop(pid) = decision else { return false };
    let gone = |wait: Duration| {
        let start = Instant::now();
        while start.elapsed() < wait {
            // The lock is free again once its holder has exited.
            if matches!(thoughtsd::acquire_lock(&lock_file), Ok(Some(_))) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    };
    end_process(pid, false);
    if !gone(Duration::from_secs(3)) {
        end_process(pid, true);
        gone(Duration::from_secs(2));
    }
    true
}

/// The AppImage this app was started from (`$APPIMAGE`), if any.
#[cfg(target_os = "linux")]
fn appimage_path() -> Option<std::path::PathBuf> {
    std::env::var_os("APPIMAGE").map(std::path::PathBuf::from).filter(|p| p.is_absolute() && p.is_file())
}

/// Work a daemon would lose if it were stopped now: `{"saving": bool, "chats": [title, …]}` —
/// `saving` while a wrap/reorganize holds the library, `chats` the titles of chats with a run in
/// progress. `None` when it is idle (or does not answer).
fn daemon_work(i: &thoughtsd::paths::DaemonInfo) -> Option<Value> {
    let get = |path: &str| -> Option<Value> {
        let body = ureq::get(&format!("http://127.0.0.1:{}{}", i.port, path))
            .set("Authorization", &format!("Bearer {}", i.token))
            .timeout(Duration::from_secs(2))
            .call()
            .ok()?
            .into_string()
            .ok()?;
        serde_json::from_str(&body).ok()
    };
    let saving = get("/api/status").is_some_and(|s| s["busy"].as_bool().unwrap_or(false));
    let chats: Vec<String> = get("/api/chats")
        .and_then(|c| c.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|c| c["running"].as_bool().unwrap_or(false))
        .map(|c| c["title"].as_str().unwrap_or("").to_string())
        .collect();
    (saving || !chats.is_empty()).then(|| json!({"saving": saving, "chats": chats}))
}

/// The error a command rejects with instead of cutting work off: `BUSY::{"saving":…,"chats":[…]}`.
fn busy_error(work: &Value) -> String {
    format!("BUSY::{work}")
}

/// Stop the daemon so it can be replaced or restarted. Unless `force`, refuses with a `BUSY::`
/// error while it is saving or a chat is still running.
fn stop_daemon_checked(force: bool) -> Result<(), String> {
    let _guard = DAEMON_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !force {
        if let Some(work) = daemon_alive().and_then(|(i, _)| daemon_work(&i)) {
            return Err(busy_error(&work));
        }
    }
    stop_daemon(true);
    Ok(())
}

fn spawn_daemon() -> anyhow::Result<()> {
    // Inside an AppImage `current_exe()` is a path under the temporary mount (/tmp/.mount_…), which
    // disappears when the window's process exits. Start the daemon through the AppImage file itself
    // so it gets a mount of its own. UNTESTED on Linux.
    #[cfg(target_os = "linux")]
    let exe = appimage_path().map_or_else(std::env::current_exe, Ok)?;
    #[cfg(not(target_os = "linux"))]
    let exe = std::env::current_exe()?;
    let mut c = std::process::Command::new(exe);
    c.arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0); // detach from the UI's process group
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP. Not DETACHED_PROCESS: Windows ignores
        // CREATE_NO_WINDOW when both are set, and a console-less daemon makes every console
        // child (git, claude) pop up its own window. UNTESTED on Windows.
        c.creation_flags(0x08000000 | 0x00000200);
    }
    c.spawn()?;
    Ok(())
}

/// Ask a running daemon to stop and wait until it is gone, so a new one can
/// take over (a second daemon refuses to start while the first still answers).
/// Never starts a daemon. With `wait: false` it only sends the request (used on exit).
fn stop_daemon(wait: bool) {
    let Some((i, _)) = daemon_alive() else { return };
    // The daemon refuses with BUSY:: while a save (wrap/reorganize) is mid-flight. Give it up to a
    // minute to finish rather than cutting a save in half; only then insist.
    let shutdown = |force: bool| {
        ureq::post(&format!("http://127.0.0.1:{}/api/shutdown", i.port))
            .set("Authorization", &format!("Bearer {}", i.token))
            .set("Content-Type", "application/json")
            .timeout(Duration::from_secs(2))
            .send_string(if force { r#"{"force":true}"# } else { "{}" })
    };
    let busy = |r: Result<ureq::Response, ureq::Error>| match r {
        Err(ureq::Error::Status(_, resp)) => resp.into_string().map(|b| b.contains("BUSY::")).unwrap_or(false),
        _ => false,
    };
    let waited = Instant::now();
    // Unless the user chose "Quit now" knowing that a save in progress stops (`quit`).
    let mut refused = busy(shutdown(quit::cut_short()));
    while refused && waited.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_secs(2));
        refused = busy(shutdown(false));
    }
    if refused { let _ = shutdown(true); }
    if !wait {
        return;
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(150));
        if daemon_alive().is_none() {
            return;
        }
    }
    // Still answering (graceful shutdown waits on open connections): end the process.
    end_process(i.pid, false);
    std::thread::sleep(Duration::from_millis(300));
    if daemon_alive().is_none() {
        thoughtsd::paths::clear_daemon_info();
    }
}

/// The running daemon, started if needed. The flag is `true` when it is a daemon left by another
/// app version that was NOT replaced because it is in the middle of something (see `daemon_info`).
fn ensure_daemon() -> Result<(thoughtsd::paths::DaemonInfo, bool), String> {
    let _guard = DAEMON_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((i, version)) = daemon_alive() {
        // After an app update the old daemon is still running the old code: replace it.
        let stale = !VERSION_CHECKED.swap(true, Ordering::SeqCst) && version.as_deref().unwrap_or(&i.version) != VERSION;
        if !stale {
            return Ok((i, false));
        }
        // …but never in the middle of a save or a running chat: keep it for now and look again on
        // the next `daemon_info` call (the UI is told through `stale: true`).
        if daemon_work(&i).is_some() {
            VERSION_CHECKED.store(false, Ordering::SeqCst);
            return Ok((i, true));
        }
        stop_daemon(true);
    }
    VERSION_CHECKED.store(true, Ordering::SeqCst);
    // Nothing proved itself. If that is because a daemon from before 0.0.9 is still running, it is
    // stopped first (it holds the lock). A second round covers one that was only just starting.
    for _round in 0..2 {
        retire_unproven_daemon();
        spawn_daemon().map_err(|e| e.to_string())?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(150));
            if let Some((i, _)) = daemon_alive() {
                return Ok((i, false));
            }
        }
    }
    Err("The Thoughts daemon did not start".into())
}

/// Save an exported customisation pack. The save dialog is opened here, in native code, so the
/// destination is always one the user picked; the window never supplies a path.
#[tauri::command]
async fn save_pack(app: tauri::AppHandle, name: String, contents: String) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    if contents.len() > 8 << 20 { return Err("pack is too large".into()); }
    let safe: String = name.chars().filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' ')).take(60).collect();
    let file = format!("{}.json", if safe.trim().is_empty() { "thoughts-pack" } else { safe.trim() });
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_file_name(&file).add_filter("Thoughts pack", &["json"]).blocking_save_file()
    }).await.map_err(|e| e.to_string())?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else { return Ok(false) };
    std::fs::write(&path, contents).map_err(|e| e.to_string())?;
    Ok(true)
}

/// The name a generated file is offered under and its kind (extension), or why it cannot be saved:
/// only document formats that cannot run when opened, within a sensible size, and a Word document
/// only when it really is one (a ZIP package, as every .docx is).
fn export_file(name: &str, bytes: &[u8]) -> Result<(String, String), String> {
    const SAFE: &[&str] = &["html", "ics", "json", "md", "txt", "csv", "docx"];
    let p = std::path::Path::new(name);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !SAFE.contains(&ext.as_str()) { return Err("this kind of file cannot be saved from Thoughts".into()); }
    let limit = if ext == "docx" { 32 << 20 } else { 16 << 20 };
    if bytes.len() > limit { return Err("file is too large".into()); }
    if ext == "docx" && !bytes.starts_with(b"PK\x03\x04") { return Err("this is not a Word document".into()); }
    let stem: String = p.file_stem().and_then(|e| e.to_str()).unwrap_or("export").chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' ' | '.')).take(80).collect();
    Ok((format!("{}.{ext}", if stem.trim().is_empty() { "export" } else { stem.trim() }), ext))
}

/// Save a file the app generated (an exported document, a calendar file, a Word document). Like
/// `save_pack`, the save dialog is opened in native code so the destination is always the user's
/// choice (see `export_file` for what may be saved). Text comes as `{name, contents}`; anything else
/// (a Word document) as the raw request body, with the file name, URI-encoded, in `x-file-name`.
#[tauri::command]
async fn save_file(app: tauri::AppHandle, request: tauri::ipc::Request<'_>) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    let (name, bytes) = match request.body() {
        tauri::ipc::InvokeBody::Json(v) => {
            let contents = v["contents"].as_str().ok_or("nothing to save")?;
            (v["name"].as_str().unwrap_or_default().to_string(), contents.as_bytes().to_vec())
        }
        tauri::ipc::InvokeBody::Raw(b) => {
            let name = request.headers().get("x-file-name").and_then(|h| h.to_str().ok()).and_then(links::percent_decode).unwrap_or_default();
            (name, b.clone())
        }
    };
    let (file, ext) = export_file(&name, &bytes)?;
    let ext2 = ext.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_file_name(&file).add_filter(&ext2.to_uppercase(), &[ext2.as_str()]).blocking_save_file()
    }).await.map_err(|e| e.to_string())?;
    let Some(mut path) = picked.and_then(|p| p.into_path().ok()) else { return Ok(false) };
    // Keep the safe extension even if the user typed another one in the dialog.
    if path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()) != Some(ext.clone()) { path.set_extension(&ext); }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Document and media types the system may open in their own app: none of them can run code when
/// opened (no scripts, programs, web pages, shortcuts or macro-enabled documents).
const SAFE_TO_OPEN: &[&str] = &["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "csv", "tsv", "txt", "md", "rtf", "odt",
    "ods", "odp", "png", "jpg", "jpeg", "gif", "webp", "heic", "tiff", "bmp", "mp3", "m4a", "wav", "mp4", "mov", "epub"];

/// Open a document from the library in the user's default app. Only regular files inside the
/// library, and only document/media types that cannot run code when opened.
#[tauri::command]
fn open_library_file(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    const SAFE: &[&str] = SAFE_TO_OPEN;
    let lib = thoughtsd::settings::Settings::load().library().ok_or("no library")?;
    // dunce: plain `C:\…` paths on Windows — the shell cannot open `\\?\` verbatim paths.
    let root = dunce::canonicalize(&lib).map_err(|e| e.to_string())?;
    let rel = std::path::Path::new(&path);
    if rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err("invalid path".into());
    }
    let full = dunce::canonicalize(root.join(rel)).map_err(|e| e.to_string())?;
    if !full.starts_with(&root) || !full.is_file() { return Err("not a file in the library".into()); }
    let ext = full.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !SAFE.contains(&ext.as_str()) { return Err("this file type can't be opened from Thoughts".into()); }
    app.opener().open_path(full.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

/// The real path of file `name` sent with chat `chat_id`, when it may be opened: a plain file name,
/// a regular file (not a link) directly in that chat's own folder under `attachments`, whose real
/// location is inside it, and a kind that cannot run code when opened (`SAFE_TO_OPEN`).
fn chat_attachment(attachments: &std::path::Path, chat_id: &str, name: &str) -> Result<std::path::PathBuf, String> {
    let refuse = || "this file can't be opened from Thoughts".to_string();
    if !thoughtsd::chats::valid_id(chat_id) {
        return Err(refuse());
    }
    let plain = !name.is_empty() && name.len() <= 400 && !name.starts_with('.') && name != ".."
        && !name.chars().any(|c| matches!(c, '/' | '\\' | ':') || c.is_control());
    if !plain {
        return Err(refuse());
    }
    let ext = std::path::Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !SAFE_TO_OPEN.contains(&ext.as_str()) {
        return Err(refuse());
    }
    let gone = || "the file is no longer there".to_string();
    let base = dunce::canonicalize(attachments).map_err(|_| gone())?;
    let dir = attachments.join(chat_id);
    if !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
        return Err(gone());
    }
    let file = dir.join(name);
    if !std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_file()) {
        return Err(gone());
    }
    let real = dunce::canonicalize(&file).map_err(|_| gone())?;
    if real.parent() != Some(base.join(chat_id).as_path()) {
        return Err(refuse());
    }
    Ok(real)
}

/// Open a file sent in a chat (a Word or Excel file, say) in the user's default app. Only the
/// chat's own files, only from the app window, and only kinds that cannot run code when opened.
#[tauri::command]
fn open_chat_attachment(app: tauri::AppHandle, window: tauri::Window, chat_id: String, name: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !is_main_label(window.label()) {
        return Err("not allowed".into());
    }
    let file = chat_attachment(&thoughtsd::paths::attachments_dir(), &chat_id, &name)?;
    app.opener().open_path(file.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

/// `{port, token, version, platform, stale}`. `stale: true` means the daemon belongs to another app
/// version and was left running because it is busy (a save or a running chat); it is replaced on
/// a later `daemon_info` call once idle, or at once by `restart_daemon {force: true}`.
#[tauri::command]
async fn daemon_info() -> Result<Value, String> {
    let (i, stale) = tauri::async_runtime::spawn_blocking(ensure_daemon).await.map_err(|e| e.to_string())??;
    Ok(json!({"port": i.port, "token": i.token, "version": i.version, "platform": PLATFORM, "stale": stale}))
}

/// Restart the background service: `restart_daemon {force?: boolean}` → same shape as `daemon_info`.
/// Without `force` it rejects with `BUSY::{"saving":bool,"chats":[title,…]}` while the daemon is
/// saving or a chat is running, and nothing is stopped.
#[tauri::command]
async fn restart_daemon(force: Option<bool>) -> Result<Value, String> {
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || stop_daemon_checked(force)).await.map_err(|e| e.to_string())??;
    daemon_info().await
}

/// Is there an update? `{configured, available, reason?, version?, notes?}`.
/// `configured: false` is not an error; `reason` says why: `"build"` (this build has no updater
/// key/endpoint) or `"deb"` (Linux, not running from an AppImage: a package-manager install
/// cannot replace itself — update through the package instead).
#[tauri::command]
async fn update_check(app: tauri::AppHandle) -> Result<Value, String> {
    if !app.state::<UpdaterConfigured>().0 {
        return Ok(json!({"configured": false, "available": false, "reason": "build"}));
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_none() {
        return Ok(json!({"configured": false, "available": false, "reason": "deb"}));
    }
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    Ok(match update {
        Some(u) => json!({"configured": true, "available": true, "version": u.version, "notes": u.body}),
        None => json!({"configured": true, "available": false}),
    })
}

/// `update_install {force?: boolean}`: download, verify (signature) and install the available
/// update, then restart. Emits `update-progress` `{downloaded, total}` while downloading.
/// Installing stops the background service, so without `force` it rejects with
/// `BUSY::{"saving":bool,"chats":[title,…]}` while a save or a chat is running (checked before the
/// download and again just before installing); nothing is stopped or installed in that case.
#[tauri::command]
async fn update_install(app: tauri::AppHandle, force: Option<bool>) -> Result<(), String> {
    let force = force.unwrap_or(false);
    if !app.state::<UpdaterConfigured>().0 {
        return Err("Updates are not configured in this build".into());
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_none() {
        return Err("This install is updated through its package, not from inside the app".into());
    }
    if !force {
        let work = tauri::async_runtime::spawn_blocking(|| daemon_alive().and_then(|(i, _)| daemon_work(&i)))
            .await
            .map_err(|e| e.to_string())?;
        if let Some(work) = work {
            return Err(busy_error(&work));
        }
    }
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let Some(update) = update else { return Err("Thoughts is already up to date".into()) };
    let mut downloaded = 0usize;
    let progress = app.clone();
    let bytes = update
        .download(
            move |chunk, total| {
                downloaded += chunk;
                let _ = progress.emit("update-progress", json!({"downloaded": downloaded, "total": total}));
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    // The daemon is this same executable: it must not be running while it is replaced
    // (Windows cannot overwrite a running .exe). The new version starts a fresh one.
    tauri::async_runtime::spawn_blocking(move || stop_daemon_checked(force)).await.map_err(|e| e.to_string())??;
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart()
}

/// Quit completely (after "Remove Thoughts from this computer": the settings are gone, so
/// "keep running in the background" must not keep a hidden window alive).
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    QUIT_ALL.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// Rebuild the native menus after the UI language changed.
#[tauri::command]
fn refresh_menu(app: tauri::AppHandle) -> Result<(), String> {
    recovery::refresh_versions_soon(&app);
    install_menus(&app).map_err(|e| e.to_string())
}

/// The page turned customisations off or back on (its own "Reload without customisations" and
/// "Turn customisations back on" buttons). The shell remembers it, so the choice survives a
/// restart and the Recovery menu shows the same thing as the page.
#[tauri::command]
fn set_safe_mode(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    recovery::set_safe_mode(on);
    install_menus(&app).map_err(|e| e.to_string())
}

/// Rebuild the menus (the quick capture shortcut they show changed).
fn refresh_menus(app: &tauri::AppHandle) {
    let _ = install_menus(app);
}

fn is_main_label(label: &str) -> bool {
    label == "main" || label.starts_with("main-")
}

/// The app window, if it exists (it may be hidden in the tray).
fn main_window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    let label = MAIN_LABEL.lock().unwrap_or_else(|e| e.into_inner()).clone();
    app.get_webview_window(if label.is_empty() { "main" } else { &label })
}

/// Open the app window: at start-up, and again whenever Recovery replaces a window whose page no
/// longer answers. The new window is built first and takes the old one's place and size; only
/// then is the old one destroyed, with its page process, so the app is never without a window.
/// The page is loaded with customisations off when that was chosen (`recovery::safe_mode`), and
/// with `recovered` after Recovery changed the customisations.
fn open_main_window(app: &tauri::AppHandle, recovered: bool) -> tauri::Result<tauri::WebviewWindow> {
    let old = main_window(app);
    let mut config = app.config().app.windows.iter().find(|w| w.label == "main").cloned().ok_or(tauri::Error::WindowNotFound)?;
    let n = WINDOWS_OPENED.fetch_add(1, Ordering::SeqCst) + 1;
    if n > 1 {
        config.label = format!("main-{n}");
    }
    let page = match recovery::page_query(recovery::safe_mode(), recovered) {
        Some(query) => format!("index.html?{query}"),
        None => "index.html".to_string(),
    };
    config.url = tauri::WebviewUrl::App(page.into());
    let mut builder = tauri::WebviewWindowBuilder::from_config(app, &config)?;
    let in_place = |w: &&tauri::WebviewWindow| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false) && !w.is_fullscreen().unwrap_or(false);
    if let Some(o) = old.as_ref().filter(in_place) {
        if o.is_maximized().unwrap_or(false) {
            builder = builder.maximized(true);
        } else if let (Ok(at), Ok(size), Ok(scale)) = (o.outer_position(), o.inner_size(), o.scale_factor()) {
            builder = builder.position(at.x as f64 / scale, at.y as f64 / scale).inner_size(size.width as f64 / scale, size.height as f64 / scale);
        }
    }
    let window = builder.build()?;
    *MAIN_LABEL.lock().unwrap_or_else(|e| e.into_inner()) = config.label;
    if let Some(o) = old {
        let _ = o.destroy();
    }
    Ok(window)
}

fn show_main(app: &tauri::AppHandle) {
    // Whoever opens the window again is still using the app: a quit that was only waiting for
    // running work to finish is called off.
    quit::still_here(app);
    if let Some(w) = main_window(app) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn keep_running() -> bool {
    thoughtsd::settings::Settings::load().daemon.keep_running
}

/// Menu labels: Arabic when the UI language is Arabic, English otherwise.
fn label(ar: bool, key: &str) -> &'static str {
    let (en, arabic) = match key {
        "settings" => ("Settings…", "الإعدادات…"),
        "recovery" => ("Recovery", "الاستعادة"),
        "undo-layer" => ("Undo last app change…", "التراجع عن آخر تغيير في التطبيق…"),
        "rollback" => ("Go back to an earlier version", "العودة إلى نسخة سابقة"),
        "rollback-none" => ("No earlier versions yet", "لا توجد نسخ سابقة بعد"),
        "rollback-find" => ("Look for earlier versions…", "البحث عن نسخ سابقة…"),
        "rollback-first" => ("The app as it first was (no customisations)", "التطبيق كما كان أول مرة (بلا تخصيصات)"),
        "rollback-more" => ("More versions…", "نسخ أخرى…"),
        "discard-pending" => ("Discard the change waiting for review…", "تجاهل التغيير المنتظر للمراجعة…"),
        "safe-mode" => ("Customisations off (safe mode)", "إيقاف التخصيصات (الوضع الآمن)"),
        "reload" => ("Reload", "إعادة التحميل"),
        "fresh-window" => ("Open a fresh window", "فتح نافذة جديدة"),
        "restart-daemon" => ("Restart background service", "إعادة تشغيل خدمة الخلفية"),
        "help" => ("Thoughts Help", "مساعدة Thoughts"),
        "check-updates" => ("Check for Updates…", "التحقق من وجود تحديثات…"),
        "capture" | "tray-capture" => ("Quick capture…", "تدوين سريع…"),
        "tray-open" => ("Open Thoughts", "فتح Thoughts"),
        "tray-quit" => ("Quit Thoughts completely", "إنهاء Thoughts تمامًا"),
        "file" => ("File", "ملف"),
        "edit" => ("Edit", "تحرير"),
        "window" => ("Window", "نافذة"),
        "help-menu" => ("Help", "مساعدة"),
        "view" => ("View", "عرض"),
        "quit" => ("Quit Thoughts", "إنهاء Thoughts"),
        "std-undo" => ("Undo", "تراجع"),
        "std-redo" => ("Redo", "إعادة"),
        "std-minimize" => ("Minimize", "تصغير"),
        "std-maximize" => ("Maximize", "تكبير"),
        "std-close" => ("Close Window", "إغلاق النافذة"),
        _ => ("", ""),
    };
    if ar { arabic } else { en }
}

fn item(h: &tauri::AppHandle, ar: bool, id: &str, accel: Option<&str>) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(h, id, label(ar, id), true, accel)
}

/// Arabic text for a standard (OS-provided) menu item; `None` keeps the system's own wording.
fn std_text(ar: bool, kind: &str) -> Option<&'static str> {
    if !ar {
        return None;
    }
    Some(match kind {
        "undo" => "تراجع",
        "redo" => "إعادة",
        "cut" => "قص",
        "copy" => "نسخ",
        "paste" => "لصق",
        "select_all" => "تحديد الكل",
        "minimize" => "تصغير",
        "maximize" => "تكبير",
        "close_window" => "إغلاق النافذة",
        "hide" => "إخفاء Thoughts",
        "hide_others" => "إخفاء الآخرين",
        "services" => "الخدمات",
        "fullscreen" => "ملء الشاشة",
        "about" => "حول Thoughts",
        _ => return None,
    })
}

/// A standard menu item (Undo, Copy, Minimize…), in Arabic when the UI is.
///
/// On Linux the menu library (muda, GTK 3) does not implement undo, redo, minimize, maximize and
/// close-window as predefined items, so there they are ordinary items (`std-<kind>`) handled in
/// `on_menu_event`. Quit is never predefined: see `quit_requested`. Linux is UNTESTED.
fn std_item(h: &tauri::AppHandle, ar: bool, kind: &str) -> tauri::Result<Box<dyn IsMenuItem<tauri::Wry>>> {
    if cfg!(target_os = "linux") && matches!(kind, "undo" | "redo" | "minimize" | "maximize" | "close_window") {
        let id = format!("std-{}", if kind == "close_window" { "close" } else { kind });
        // No accelerator for undo/redo: the web view already handles Ctrl+Z itself.
        let accel = (kind == "close_window").then_some("CmdOrCtrl+W");
        return Ok(Box::new(MenuItem::with_id(h, &id, label(ar, &id), true, accel)?));
    }
    let text = std_text(ar, kind);
    Ok(Box::new(match kind {
        "undo" => PredefinedMenuItem::undo(h, text)?,
        "redo" => PredefinedMenuItem::redo(h, text)?,
        "cut" => PredefinedMenuItem::cut(h, text)?,
        "copy" => PredefinedMenuItem::copy(h, text)?,
        "paste" => PredefinedMenuItem::paste(h, text)?,
        "select_all" => PredefinedMenuItem::select_all(h, text)?,
        "minimize" => PredefinedMenuItem::minimize(h, text)?,
        "maximize" => PredefinedMenuItem::maximize(h, text)?,
        "close_window" => PredefinedMenuItem::close_window(h, text)?,
        "hide" => PredefinedMenuItem::hide(h, text)?,
        "hide_others" => PredefinedMenuItem::hide_others(h, text)?,
        "services" => PredefinedMenuItem::services(h, text)?,
        "fullscreen" => PredefinedMenuItem::fullscreen(h, text)?,
        _ => PredefinedMenuItem::separator(h)?,
    }))
}

/// "Quit Thoughts": an ordinary item rather than the predefined one, so quitting goes through
/// `quit_requested` on every platform (and works on Linux, where predefined Quit does not exist).
fn quit_item(h: &tauri::AppHandle, ar: bool) -> tauri::Result<MenuItem<tauri::Wry>> {
    // Windows closes with Alt+F4; Ctrl+Q is not a convention there.
    let accel = if cfg!(windows) { None } else { Some("CmdOrCtrl+Q") };
    item(h, ar, "quit", accel)
}

/// "Quick capture…", showing the shortcut while it works (a menu that cannot show those keys gets
/// the item without them).
fn capture_item(h: &tauri::AppHandle, ar: bool, id: &str) -> tauri::Result<MenuItem<tauri::Wry>> {
    match capture::accelerator() {
        Some(keys) => item(h, ar, id, Some(&keys)).or_else(|_| item(h, ar, id, None)),
        None => item(h, ar, id, None),
    }
}

fn edit_menu(h: &tauri::AppHandle, ar: bool) -> tauri::Result<Submenu<tauri::Wry>> {
    Submenu::with_items(
        h,
        label(ar, "edit"),
        true,
        &[
            &*std_item(h, ar, "undo")?,
            &*std_item(h, ar, "redo")?,
            &PredefinedMenuItem::separator(h)?,
            &*std_item(h, ar, "cut")?,
            &*std_item(h, ar, "copy")?,
            &*std_item(h, ar, "paste")?,
            &*std_item(h, ar, "select_all")?,
        ],
    )
}

fn about_metadata(h: &tauri::AppHandle) -> tauri::menu::AboutMetadata<'static> {
    tauri::menu::AboutMetadata {
        name: Some(h.package_info().name.clone()),
        version: Some(h.package_info().version.to_string()),
        ..Default::default()
    }
}

/// macOS: the standard menu bar (same layout as Tauri's default menu, built here so every item
/// can carry Arabic text), with Check for Updates… and Settings… (⌘,) in the app menu.
#[cfg(target_os = "macos")]
fn build_menu(h: &tauri::AppHandle, ar: bool) -> tauri::Result<Menu<tauri::Wry>> {
    use tauri::menu::{HELP_SUBMENU_ID, WINDOW_SUBMENU_ID};
    let app_menu = Submenu::with_items(
        h,
        h.package_info().name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(h, std_text(ar, "about"), Some(about_metadata(h)))?,
            &item(h, ar, "check-updates", None)?,
            &PredefinedMenuItem::separator(h)?,
            &item(h, ar, "settings", Some("CmdOrCtrl+Comma"))?,
            &PredefinedMenuItem::separator(h)?,
            &*std_item(h, ar, "services")?,
            &PredefinedMenuItem::separator(h)?,
            &*std_item(h, ar, "hide")?,
            &*std_item(h, ar, "hide_others")?,
            &PredefinedMenuItem::separator(h)?,
            &quit_item(h, ar)?,
        ],
    )?;
    let file = Submenu::with_items(h, label(ar, "file"), true, &[&capture_item(h, ar, "capture")?, &PredefinedMenuItem::separator(h)?, &*std_item(h, ar, "close_window")?])?;
    let view = Submenu::with_items(h, label(ar, "view"), true, &[&*std_item(h, ar, "fullscreen")?])?;
    let window = Submenu::with_id_and_items(
        h,
        WINDOW_SUBMENU_ID,
        label(ar, "window"),
        true,
        &[
            &*std_item(h, ar, "minimize")?,
            &*std_item(h, ar, "maximize")?,
            &PredefinedMenuItem::separator(h)?,
            &*std_item(h, ar, "close_window")?,
        ],
    )?;
    // Recovery goes before Help, which stays last.
    let help = Submenu::with_id_and_items(h, HELP_SUBMENU_ID, label(ar, "help-menu"), true, &[&item(h, ar, "help", None)?])?;
    Menu::with_items(h, &[&app_menu, &file, &edit_menu(h, ar)?, &view, &window, &recovery::menu(h, ar)?, &help])
}

/// Windows / Linux: File (Settings…, Close, Quit) · Edit · Window · Recovery · Help.
/// UNTESTED: written on macOS; needs a pass on real Windows and Linux.
#[cfg(not(target_os = "macos"))]
fn build_menu(h: &tauri::AppHandle, ar: bool) -> tauri::Result<Menu<tauri::Wry>> {
    use tauri::menu::{HELP_SUBMENU_ID, WINDOW_SUBMENU_ID};
    let file = Submenu::with_items(
        h,
        label(ar, "file"),
        true,
        &[
            &capture_item(h, ar, "capture")?,
            &item(h, ar, "settings", Some("CmdOrCtrl+Comma"))?,
            &PredefinedMenuItem::separator(h)?,
            &*std_item(h, ar, "close_window")?,
            &quit_item(h, ar)?,
        ],
    )?;
    let window = Submenu::with_id_and_items(
        h,
        WINDOW_SUBMENU_ID,
        label(ar, "window"),
        true,
        &[&*std_item(h, ar, "minimize")?, &*std_item(h, ar, "maximize")?],
    )?;
    let help = Submenu::with_id_and_items(
        h,
        HELP_SUBMENU_ID,
        label(ar, "help-menu"),
        true,
        &[
            &item(h, ar, "help", None)?,
            &item(h, ar, "check-updates", None)?,
            &PredefinedMenuItem::separator(h)?,
            &PredefinedMenuItem::about(h, std_text(ar, "about"), Some(about_metadata(h)))?,
        ],
    )?;
    Menu::with_items(h, &[&file, &edit_menu(h, ar)?, &window, &recovery::menu(h, ar)?, &help])
}

/// Hide the main window and leave the tray icon as the way back.
fn hide_to_tray(app: &tauri::AppHandle) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_visible(true);
    }
    if let Some(w) = main_window(app) {
        let _ = w.hide();
    }
}

/// Quit Thoughts (menu item, ⌘Q / Ctrl+Q).
///   * "keep running in the background" OFF: the app exits and the background service is stopped.
///   * ON: the background service must stay reachable, so the window is hidden and the app stays
///     in the tray (same as closing the window). The tray's "Quit Thoughts completely" is the one
///     action that stops everything.
/// (macOS Dock ▸ Quit and a logout/shutdown end the process without passing through here; with
/// keep-running on the service then carries on without a tray until the app is opened again.)
fn quit_requested(app: &tauri::AppHandle) {
    if keep_running() && !QUIT_ALL.load(Ordering::SeqCst) {
        hide_to_tray(app);
    } else {
        // Really quitting stops running chats and drops unsaved edits: `quit` asks first.
        quit::request(app, false);
    }
}

fn tray_menu(h: &tauri::AppHandle, ar: bool) -> tauri::Result<Menu<tauri::Wry>> {
    Menu::with_items(
        h,
        &[
            &item(h, ar, "tray-open", None)?,
            &capture_item(h, ar, "tray-capture")?,
            &PredefinedMenuItem::separator(h)?,
            &item(h, ar, "tray-quit", None)?,
        ],
    )
}

/// (Re)build the menu bar and the tray menu in the current UI language.
fn install_menus(h: &tauri::AppHandle) -> tauri::Result<()> {
    let ar = thoughtsd::settings::Settings::load().language.starts_with("ar");
    h.set_menu(build_menu(h, ar)?)?;
    if let Some(tray) = h.tray_by_id("main") {
        tray.set_menu(Some(tray_menu(h, ar)?))?;
    }
    Ok(())
}

/// Replace the running app's Dock icon with a theme-coloured PNG rendered by the UI.
#[tauri::command]
fn set_dock_icon(app: tauri::AppHandle, png: Vec<u8>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        app.run_on_main_thread(move || {
            use objc2::{AllocAnyThread, MainThreadMarker};
            use objc2_app_kit::{NSApplication, NSImage};
            use objc2_foundation::NSData;
            let Some(mtm) = MainThreadMarker::new() else { return };
            let data = NSData::with_bytes(&png);
            if let Some(img) = NSImage::initWithData(NSImage::alloc(), &data) {
                unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&img)) };
            }
        })
        .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, png);
    Ok(())
}

/// Whether the app window, or a frame inside it, may load `url`: only the app's own pages and the
/// documents the page builds itself. Never another site, and never another web server on this
/// computer, whatever its port. `dev` is the development server's address (development builds only).
fn nav_allowed(url: &tauri::Url, dev: Option<&tauri::Url>) -> bool {
    let same_server = |a: &tauri::Url| a.scheme() == url.scheme() && a.host_str() == url.host_str() && a.port_or_known_default() == url.port_or_known_default();
    match url.scheme() {
        // Empty frames, and documents made in the page (a PDF preview is a blob). Never `data:`: nothing
        // of the app uses it, and an add-on frame must not swap its page for one (SEC-41).
        "about" | "blob" => true,
        // The bundled app…
        "tauri" => url.host_str() == Some("localhost") && url.port().is_none(),
        // …which Windows and Android serve as http(s)://tauri.localhost. Elsewhere that name is just
        // another way to reach this computer, so it is refused like any other local server.
        "http" | "https" => {
            cfg!(any(windows, target_os = "android")) && url.host_str() == Some("tauri.localhost") && url.port().is_none()
                || dev.is_some_and(same_server)
        }
        _ => false,
    }
}

fn main() {
    if std::env::var("THOUGHTS_ASKPASS").is_ok() {
        std::process::exit(thoughtsd::askpass::main());
    }
    if std::env::args().any(|a| a == "--daemon") {
        let rt = tokio::runtime::Runtime::new().expect("tokio");
        if let Err(e) = rt.block_on(thoughtsd::run()) {
            eprintln!("thoughtsd: {e}");
            std::process::exit(1);
        }
        return;
    }

    tauri::Builder::default()
        // Must be the first plugin: a second launch focuses the existing window and exits. A second
        // launch that carries a thoughts:// link (Windows, Linux) is handed to the link handler first
        // (the `deep-link` feature), and that decides what to show.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| if !links::in_args(&args) { show_main(app) }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // The app window only ever shows the app itself; any attempt to navigate elsewhere is refused.
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("nav-guard")
                .on_navigation(|w, url| {
                    let dev = if tauri::is_dev() { w.config().build.dev_url.clone() } else { None };
                    nav_allowed(url, dev.as_ref())
                })
                // Tell the UI which OS it is on before it renders: only macOS has a
                // transparent (vibrancy) window.
                .js_init_script(format!(
                    "window.__THOUGHTS_PLATFORM__='{PLATFORM}';(function(){{var a=function(){{document.documentElement.classList.add('os-{PLATFORM}')}};document.documentElement?a():document.addEventListener('DOMContentLoaded',a)}})();"
                ))
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            open_library_file,
            open_chat_attachment,
            save_pack,
            save_file,
            daemon_info,
            restart_daemon,
            set_dock_icon,
            update_check,
            update_install,
            refresh_menu,
            quit_app,
            set_safe_mode,
            quit::set_unsaved,
            links::take_deep_link,
            capture::open_capture,
            capture::capture_ready,
            capture::capture_take,
            capture::capture_show,
            capture::capture_hide,
            capture::capture_open_main,
            capture::capture_shortcut,
            capture::set_capture_shortcut,
            capture::pause_capture_shortcut
        ])
        // A page that starts loading holds nothing unsaved yet, whatever the last one reported.
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started && is_main_label(webview.label()) {
                quit::page_started();
            }
        })
        .setup(|app| {
            let h = app.handle();
            // The updater only exists when this build carries a public key and an endpoint
            // (plugins.updater in the Tauri config, see docs/RELEASING.md). Without them the
            // plugin is not loaded and update_check answers `configured: false`.
            let configured = app.config().plugins.0.get("updater").is_some_and(|u| {
                u["pubkey"].as_str().is_some_and(|k| !k.trim().is_empty())
                    && u["endpoints"].as_array().is_some_and(|e| !e.is_empty())
            });
            if configured {
                h.plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            app.manage(UpdaterConfigured(configured));

            // Tray icon: the only visible handle on the app once its window is closed
            // while the background service keeps running.
            let mut tray = tauri::tray::TrayIconBuilder::with_id("main").tooltip("Thoughts");
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            let tray = tray.build(app)?;
            let _ = tray.set_visible(keep_running());
            // The quick capture shortcut first: the menus show its keys.
            capture::init(h);
            install_menus(h)?;
            // The window is opened here rather than by the config ("create": false), so that it
            // can start with customisations off when that was chosen in the Recovery menu.
            open_main_window(h, false)?;
            recovery::watch_versions(h);
            // thoughts:// links: those that arrive while the app runs, and the one it was opened with.
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let links_app = h.clone();
                h.deep_link().on_open_url(move |e| links::handle(&links_app, e.urls()));
                // An AppImage is not installed by a package, so it says itself that it opens these
                // links (a .deb, the Windows installer and the macOS app say so when installed).
                #[cfg(target_os = "linux")]
                if appimage_path().is_some() {
                    let _ = h.deep_link().register_all();
                }
                if let Ok(Some(urls)) = h.deep_link().get_current() {
                    links::handle(h, urls);
                }
            }
            Ok(())
        })
        .on_window_event(|window, ev| {
            if window.label() == capture::LABEL {
                capture::on_window_event(window, ev);
                return;
            }
            if !is_main_label(window.label()) {
                return;
            }
            match ev {
                // With "keep running in the background" on, closing the window hides it and leaves
                // the tray icon. Otherwise closing quits and stops the daemon, and like every real
                // quit it first asks about chats still running and unsaved edits (`quit`).
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    quit_requested(window.app_handle());
                }
                // Back in front: the list of earlier versions in the Recovery menu may be out of date.
                tauri::WindowEvent::Focused(true) => recovery::refresh_versions_soon(window.app_handle()),
                _ => {}
            }
        })
        .on_menu_event(|app, ev| {
            let id = ev.id().as_ref();
            // Recovery is done natively, start to finish: it must work when the page does not.
            if recovery::handle(app, id) {
                return;
            }
            let win = main_window(app);
            match id {
                "settings" => { show_main(app); let _ = app.emit("open-settings", ()); }
                "help" => { show_main(app); let _ = app.emit("open-help", ()); }
                "check-updates" => { show_main(app); let _ = app.emit("check-updates", ()); }
                "tray-open" => show_main(app),
                "capture" | "tray-capture" => capture::from_menu(app),
                // The one action that stops everything, "keep running" or not: asks first.
                "tray-quit" => quit::request(app, true),
                "quit" => quit_requested(app),
                // Linux stand-ins for the standard items GTK does not provide (see `std_item`).
                "std-close" => if let Some(w) = win { let _ = w.close(); },
                "std-minimize" => if let Some(w) = win { let _ = w.minimize(); },
                "std-maximize" => if let Some(w) = win {
                    let _ = if w.is_maximized().unwrap_or(false) { w.unmaximize() } else { w.maximize() };
                },
                "std-undo" => if let Some(w) = win { let _ = w.eval("document.execCommand('undo')"); },
                "std-redo" => if let Some(w) = win { let _ = w.eval("document.execCommand('redo')"); },
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Thoughts")
        .run(|_app, ev| match ev {
            tauri::RunEvent::Exit => {
                if QUIT_ALL.load(Ordering::SeqCst) || !keep_running() {
                    stop_daemon(false);
                }
            }
            // Clicking the Dock icon brings back a window hidden to the tray.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { has_visible_windows: false, .. } => show_main(_app),
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> tauri::Url { tauri::Url::parse(s).unwrap() }

    #[test]
    fn only_a_chats_own_harmless_files_are_opened() {
        let base = std::env::temp_dir().join(format!("thoughts-att-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&base);
        let att = base.join("attachments");
        let chat = "0b0f6c2e-1d2a-4f8e-9c1e-5c9f2d7e8a10";
        std::fs::create_dir_all(att.join(chat)).unwrap();
        std::fs::create_dir_all(att.join("other-chat")).unwrap();
        for f in ["Budget 2026.xlsx", "عقد.docx", "run.command", "page.html", "macro.docm", "noext"] {
            std::fs::write(att.join(chat).join(f), "x").unwrap();
        }
        std::fs::write(att.join("other-chat").join("theirs.docx"), "x").unwrap();
        std::fs::write(base.join("outside.docx"), "x").unwrap();
        let real = |p: std::path::PathBuf| dunce::canonicalize(p).unwrap();
        assert_eq!(chat_attachment(&att, chat, "Budget 2026.xlsx"), Ok(real(att.join(chat).join("Budget 2026.xlsx"))));
        assert_eq!(chat_attachment(&att, chat, "عقد.docx"), Ok(real(att.join(chat).join("عقد.docx"))));
        // Kinds that can run something when opened, and files with no kind at all.
        for bad in ["run.command", "page.html", "macro.docm", "noext"] {
            assert!(chat_attachment(&att, chat, bad).is_err(), "{bad}");
        }
        // Another chat's file, a way out of the folder, a made-up chat, a missing file.
        assert!(chat_attachment(&att, "other-chat", "missing.docx").is_err());
        assert!(chat_attachment(&att, chat, "theirs.docx").is_err());
        for bad in ["../other-chat/theirs.docx", "..", "../../outside.docx", "a/b.docx", "a\\b.docx", "C:x.docx", ".hidden.docx", ""] {
            assert!(chat_attachment(&att, chat, bad).is_err(), "{bad}");
        }
        for bad in ["..", "../attachments", "a/b", ""] {
            assert!(chat_attachment(&att, bad, "theirs.docx").is_err(), "{bad}");
        }
        // A link in the chat's folder that leads elsewhere is not opened, whatever it is named.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("outside.docx"), att.join(chat).join("link.docx")).unwrap();
            assert!(chat_attachment(&att, chat, "link.docx").is_err());
            std::os::unix::fs::symlink(att.join("other-chat"), att.join("c0ffee")).unwrap();
            assert!(chat_attachment(&att, "c0ffee", "theirs.docx").is_err());
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn generated_files_are_saved_only_as_documents_that_cannot_run() {
        let docx = b"PK\x03\x04 rest of a Word package";
        assert_eq!(export_file("Plan.docx", docx), Ok(("Plan.docx".into(), "docx".into())));
        assert_eq!(export_file("خطة العمل.DOCX", docx), Ok(("خطة العمل.docx".into(), "docx".into())));
        assert_eq!(export_file("calendar.ics", b"BEGIN:VCALENDAR"), Ok(("calendar.ics".into(), "ics".into())));
        assert_eq!(export_file("../../etc/evil.md", b"x"), Ok(("evil.md".into(), "md".into())));
        assert_eq!(export_file("<>.html", b"x"), Ok(("export.html".into(), "html".into())));
        // Kinds that can run, and names without a kind.
        for bad in ["run.exe", "macro.docm", "page.htm", "script.sh", "app.command", "noext", "x.docx.exe"] {
            assert!(export_file(bad, docx).is_err(), "{bad}");
        }
        // A Word file has to be one, and nothing is saved past the size limits.
        assert!(export_file("fake.docx", b"<html>").is_err());
        assert!(export_file("big.docx", &[b"PK\x03\x04".as_slice(), &vec![0u8; 33 << 20]].concat()).is_err());
        assert!(export_file("big.txt", &vec![b'a'; (16 << 20) + 1]).is_err());
        assert!(export_file("ok.txt", &vec![b'a'; 16 << 20]).is_ok());
    }

    #[test]
    fn the_window_only_loads_the_app_itself() {
        let dev = url("http://localhost:1420");
        // The bundled app and what its pages build for themselves.
        for ok in ["tauri://localhost", "tauri://localhost/index.html?safe=1", "tauri://localhost/plugin-frame.html", "about:blank", "about:srcdoc", "blob:tauri://localhost/0b1c"] {
            assert!(nav_allowed(&url(ok), None), "{ok}");
        }
        // Another web server on this computer is not the app, whatever port it listens on.
        for bad in ["http://localhost:8080/login", "http://localhost/", "http://localhost:1420/", "http://127.0.0.1:3000/", "https://localhost:8443/",
            "https://example.com/", "file:///etc/passwd", "asset://localhost/etc/passwd", "tauri://evil.example/", "tauri://localhost:99/", "javascript:alert(1)",
            // A page an add-on frame could build from text and swap itself for.
            "data:text/html,<script>parent.postMessage(1,'*')</script>"] {
            assert!(!nav_allowed(&url(bad), None), "{bad}");
        }
        // A development build also loads its own dev server, and only that one.
        assert!(nav_allowed(&url("http://localhost:1420/?safe=1"), Some(&dev)));
        for bad in ["http://localhost:8080/", "http://localhost/", "https://localhost:1420/", "http://127.0.0.1:1420/", "http://localhost.evil.example:1420/"] {
            assert!(!nav_allowed(&url(bad), Some(&dev)), "{bad}");
        }
        // Only Windows and Android serve the bundle over http(s)://tauri.localhost.
        assert_eq!(nav_allowed(&url("http://tauri.localhost/index.html"), None), cfg!(any(windows, target_os = "android")));
        assert!(!nav_allowed(&url("http://tauri.localhost:8080/"), None));
    }

    #[test]
    fn a_daemon_that_cannot_prove_itself_is_never_trusted_and_only_stopped_when_certain() {
        use thoughtsd::hello::Probe;
        let exe = "Thoughts";
        let daemon = "/Applications/Thoughts.app/Contents/MacOS/Thoughts --daemon";
        let held = || -> std::io::Result<Option<std::fs::File>> { Ok(None) };
        // Without a proof there is nobody to send the token to, whatever the lock says: there is no
        // code path from an unproven responder to a (port, token) pair. `daemon_alive` is
        // `hello::proven` and nothing else, and that is tested in the daemon crate.
        // The only thing left to decide is whether an old daemon *process* may be stopped.
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), 4242, Some(daemon), exe), Unproven::Stop(4242));
        // The lock could not be read: nothing is known, so nothing is done (fail closed).
        let unreadable = || -> std::io::Result<Option<std::fs::File>> { Err(std::io::Error::other("locks are not supported here")) };
        assert_eq!(unproven_daemon(&Probe::Legacy, &unreadable(), 4242, Some(daemon), exe), Unproven::Leave);
        // The lock is free: no daemon is alive, the recorded pid is stale and may be anyone's by now.
        let scratch = std::env::temp_dir().join(format!("thoughts-shell-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let free = || -> std::io::Result<Option<std::fs::File>> { Ok(Some(std::fs::File::create(&scratch).unwrap())) };
        assert_eq!(unproven_daemon(&Probe::Legacy, &free(), 4242, Some(daemon), exe), Unproven::Leave);
        let _ = std::fs::remove_file(&scratch);
        // Silence on the port (a slow or starting daemon) is not evidence of an old one.
        assert_eq!(unproven_daemon(&Probe::Nobody, &held(), 4242, Some(daemon), exe), Unproven::Leave);
        assert_eq!(unproven_daemon(&Probe::Daemon("0.0.9".into()), &held(), 4242, Some(daemon), exe), Unproven::Leave);
        // The recorded pid is gone, or belongs to some other program: never stop that.
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), 4242, None, exe), Unproven::Leave);
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), 4242, Some("/usr/bin/vim notes.md"), exe), Unproven::Leave);
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), 4242, Some("/Applications/Thoughts.app/Contents/MacOS/Thoughts"), exe), Unproven::Leave);
        // Never this process, and never pid 0 or 1.
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), std::process::id(), Some(daemon), exe), Unproven::Leave);
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), 1, Some(daemon), exe), Unproven::Leave);
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), 0, Some(daemon), exe), Unproven::Leave);
    }

    #[cfg(not(windows))]
    #[test]
    fn recognises_this_app_running_as_the_background_service() {
        for yes in ["/Applications/Thoughts.app/Contents/MacOS/Thoughts --daemon", "/Users/a b/My Apps/Thoughts.app/Contents/MacOS/thoughts --daemon",
            "/tmp/.mount_ThoughXyZ/usr/bin/thoughts --daemon", "thoughts --daemon", "/home/me/code/target/debug/thoughtsd", "thoughtsd"] {
            assert!(is_daemon_command(yes, "Thoughts"), "{yes}");
        }
        // The window itself, another program, another program that happens to take --daemon, and
        // a program whose arguments merely mention this one.
        for no in ["/Applications/Thoughts.app/Contents/MacOS/Thoughts", "/usr/bin/vim notes.md", "/usr/sbin/sshd --daemon", "/usr/bin/vim Thoughts --daemon",
            "/bin/sh -c thoughts --daemon; true", "/Applications/Thoughts.app/Contents/MacOS/Thoughts --daemon --verbose", "", "   "] {
            assert!(!is_daemon_command(no, "Thoughts"), "{no}");
        }
        assert!(!is_daemon_command("/Applications/Thoughts.app/Contents/MacOS/Thoughts --daemon", ""));
        // This test process is running, and it is not the background service.
        let me = process_command(std::process::id()).expect("ps knows this process");
        assert!(!me.is_empty() && !is_daemon_command(&me, "Thoughts"), "{me}");
        assert_eq!(process_command(0x7fff_fff0), None, "no such process");
    }
}
