// Thoughts desktop shell.
//
// One executable, three modes:
//   * GUI (default)        — Tauri window; connects to (or spawns) the daemon
//   * `--daemon`           — runs thoughtsd, detached, so sessions outlive the UI
//   * THOUGHTS_ASKPASS=<code> — git's password prompt, answered by the daemon for a git it runs
// (`updates …` in a terminal only says that updates need no step there any more:
// `retired_updates_command`.)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod attention;
mod capture;
mod consent;
mod fullscreen;
mod links;
mod openfile;
mod pdf;
mod quit;
mod recovery;
mod screen_check;
mod voice;
mod windows;

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

/// Work a daemon would lose if it were stopped now: `{"saving": bool, "chats": [title, …],
/// "elsewhere": [library name, …]}` — `saving` while a wrap/reorganize holds the library, `chats`
/// the titles of chats with a run in progress in the open library, `elsewhere` the other libraries
/// in which a chat is still running (switching libraries does not stop them, and the chat list only
/// shows the open one's). `None` when it is idle (or does not answer).
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
    let elsewhere = get("/api/libraries").map(|l| running_elsewhere(&l)).unwrap_or_default();
    (saving || !chats.is_empty() || !elsewhere.is_empty()).then(|| json!({"saving": saving, "chats": chats, "elsewhere": elsewhere}))
}

/// The names of the libraries, other than the open one, with a chat still running in them
/// (`GET /api/libraries`: `{libraries: [{name, displayName, open, running}, …]}`).
fn running_elsewhere(libraries: &Value) -> Vec<String> {
    libraries["libraries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["open"] != true && l["running"].as_u64().unwrap_or(0) > 0)
        .map(|l| l["displayName"].as_str().or(l["name"].as_str()).unwrap_or("").to_string())
        .collect()
}

/// The error a command rejects with instead of cutting work off:
/// `BUSY::{"saving":…,"chats":[…],"elsewhere":[…]}` (see `daemon_work`).
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
    // The confirmation key goes over standard input, never in an argument or the environment
    // (`consent.rs`).
    c.arg("--daemon")
        .arg(thoughtsd::consent::KEY_FLAG)
        .stdin(std::process::Stdio::piped())
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
    let mut child = c.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = writeln!(stdin, "{}", thoughtsd::consent::key_hex(consent::key()));
    }
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
        // A daemon this launch did not start (at login, or left by an earlier launch) cannot be told
        // the user's yes to bypass or folder trust (`consent.rs`): it is replaced the same way.
        let keyed = consent::holds_key(&i);
        if !stale && keyed {
            return Ok((i, false));
        }
        // …but never in the middle of a save or a running chat: keep it for now and look again on
        // the next `daemon_info` call (the UI is told through `stale: true`).
        if daemon_work(&i).is_some() {
            if stale { VERSION_CHECKED.store(false, Ordering::SeqCst); }
            return Ok((i, stale));
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

/// Save an exported plugin or pack file (0.7.0: a zip of the customisation's real files, made by
/// the daemon). The save dialog is opened here, in native code, so the destination is always one
/// the user picked; the window never supplies a path. The bytes come as the raw request body, with
/// the name (URI-encoded) in `x-file-name` and the kind (`thoughts-plugin` or `thoughts-pack`) in
/// `x-file-kind`; see `pack_file_name` for what is accepted.
#[tauri::command]
async fn save_pack(app: tauri::AppHandle, window: tauri::Window, request: tauri::ipc::Request<'_>) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    main_only(&window)?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("nothing to save".into()) };
    let header = |k: &str| request.headers().get(k).and_then(|h| h.to_str().ok()).and_then(links::percent_decode).unwrap_or_default();
    let (file, ext) = pack_file_name(&header("x-file-name"), &header("x-file-kind"), bytes)?;
    let bytes = bytes.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let label = if ext == "thoughts-plugin" { "Thoughts plugin" } else { "Thoughts pack" };
        app.dialog().file().set_file_name(&file).add_filter(label, &[ext]).blocking_save_file()
    }).await.map_err(|e| e.to_string())?;
    let Some(mut path) = picked.and_then(|p| p.into_path().ok()) else { return Ok(false) };
    // Keep the kind's extension even if the user typed another one in the dialog.
    if path.extension().and_then(|e| e.to_str()) != Some(ext) { path.set_extension(ext); }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(true)
}

/// The name a plugin or pack file is offered under, and its extension, or why it is not saved:
/// only the two kinds, only a zip archive (what the daemon makes), within a sensible size. The name
/// keeps letters and digits of any language, dashes, underscores and spaces.
fn pack_file_name(name: &str, kind: &str, bytes: &[u8]) -> Result<(String, &'static str), String> {
    let ext = match kind { "thoughts-plugin" => "thoughts-plugin", "thoughts-pack" => "thoughts-pack", _ => return Err("this kind of file cannot be saved from Thoughts".into()) };
    if bytes.len() > 8 << 20 { return Err("pack is too large".into()); }
    if !bytes.starts_with(b"PK\x03\x04") { return Err("this is not a plugin or pack file".into()); }
    let stem = name.strip_suffix(&format!(".{ext}")).unwrap_or(name);
    let safe: String = stem.chars().filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' ')).take(60).collect();
    Ok((format!("{}.{ext}", if safe.trim().is_empty() { "thoughts-pack" } else { safe.trim() }), ext))
}

/// The name a generated file is offered under and its kind (extension), or why it cannot be saved:
/// only document formats that cannot run when opened, within a sensible size, and a Word document
/// only when it really is one (a ZIP package, as every .docx is).
fn export_file(name: &str, bytes: &[u8]) -> Result<(String, String), String> {
    const SAFE: &[&str] = &["html", "ics", "json", "md", "txt", "csv", "docx", "svg", "drawio"];
    let p = std::path::Path::new(name);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !SAFE.contains(&ext.as_str()) { return Err("this kind of file cannot be saved from Thoughts".into()); }
    let limit = if ext == "docx" { 32 << 20 } else { 16 << 20 };
    if bytes.len() > limit { return Err("file is too large".into()); }
    if ext == "docx" && !bytes.starts_with(b"PK\x03\x04") { return Err("this is not a Word document".into()); }
    // A drawing the app made (a diagram or a chart): a picture and nothing else. An SVG that could
    // run something when opened in a browser is not one of the app's.
    if ext == "svg" && !plain_svg(bytes) { return Err("this kind of file cannot be saved from Thoughts".into()); }
    if ext == "drawio" && !drawio_file(bytes) { return Err("this kind of file cannot be saved from Thoughts".into()); }
    let stem: String = p.file_stem().and_then(|e| e.to_str()).unwrap_or("export").chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' ' | '.')).take(80).collect();
    Ok((format!("{}.{ext}", if stem.trim().is_empty() { "export" } else { stem.trim() }), ext))
}

/// Is this an SVG picture and nothing else? Decided on the parsed document, never on its text:
/// only the elements a drawing of the app's is made of, no handler, no link, no inline style, no
/// document type or processing instruction, nothing referred to outside the file (a paint or a
/// marker by `#id`, a font carried inside a `<style>` as `data:font/…`).
fn plain_svg(bytes: &[u8]) -> bool {
    const SVG: &str = "http://www.w3.org/2000/svg";
    const ELEMENTS: &[&str] = &["svg", "g", "defs", "title", "desc", "style", "rect", "circle", "ellipse", "line", "polyline", "polygon", "path",
        "text", "tspan", "marker", "pattern"];
    let Ok(text) = std::str::from_utf8(bytes) else { return false };
    // (A document type, and so every entity of its own making, is refused by the parser.)
    let Ok(doc) = roxmltree::Document::parse(text) else { return false };
    if doc.root_element().tag_name().name() != "svg" { return false; }
    for n in doc.descendants() {
        if n.is_pi() { return false; }
        if !n.is_element() { continue; }
        let name = n.tag_name();
        if name.namespace() != Some(SVG) || !ELEMENTS.contains(&name.name()) { return false; }
        for a in n.attributes() {
            let key = a.name().to_ascii_lowercase();
            if a.namespace().is_some() || key.starts_with("on") || key == "style" || key.contains("href") || key == "src" { return false; }
            if !local_refs(a.value()) { return false; }
        }
        if name.name() == "style" {
            let css: String = n.children().filter_map(|c| c.text()).collect();
            if n.children().any(|c| c.is_element()) || !fonts_only(&css) { return false; }
        }
    }
    true
}

/// A value as it is compared: lower case, without blanks, quotes or the characters CSS lets a name hide behind.
fn squeezed(v: &str) -> Option<String> {
    if v.contains('\\') || v.contains("/*") { return None; }
    Some(v.chars().filter(|c| !c.is_whitespace() && !c.is_control() && *c != '"' && *c != '\'').flat_map(char::to_lowercase).collect())
}

/// Does an attribute's value refer to nothing but parts of the same file (`url(#id)`)?
fn local_refs(v: &str) -> bool {
    let Some(t) = squeezed(v) else { return false };
    if t.contains("javascript:") || t.contains("expression(") || t.contains("@import") { return false; }
    t.match_indices("url(").all(|(i, _)| t[i + 4..].starts_with('#'))
}

/// Is a style sheet only fonts carried inside it: `@font-face` rules whose every `url()` is `data:font/…`?
fn fonts_only(css: &str) -> bool {
    let Some(t) = squeezed(css) else { return false };
    if t.contains("@import") || t.contains("expression(") || t.contains("javascript:") { return false; }
    if !t.match_indices("url(").all(|(i, _)| t[i + 4..].starts_with("data:font/")) { return false; }
    let mut rest = t.as_str();
    while !rest.is_empty() {
        let Some(body) = rest.strip_prefix("@font-face{") else { return false };
        let Some(end) = body.find('}') else { return false };
        if body[..end].contains('{') { return false; }
        rest = &body[end + 1..];
    }
    true
}

/// Is this a draw.io file: an `mxfile` holding a graph model and nothing else?
fn drawio_file(bytes: &[u8]) -> bool {
    const ELEMENTS: &[&str] = &["mxfile", "diagram", "mxGraphModel", "root", "mxCell", "mxGeometry", "mxPoint", "Array"];
    let Ok(text) = std::str::from_utf8(bytes) else { return false };
    let Ok(doc) = roxmltree::Document::parse(text) else { return false };
    doc.root_element().tag_name().name() == "mxfile"
        && doc.descendants().all(|n| !n.is_pi() && (!n.is_element() || (n.tag_name().namespace().is_none() && ELEMENTS.contains(&n.tag_name().name()))))
}

/// Save a file the app generated (an exported document, a calendar file, a Word document). Like
/// `save_pack`, the save dialog is opened in native code so the destination is always the user's
/// choice (see `export_file` for what may be saved). Text comes as `{name, contents}`; anything else
/// (a Word document) as the raw request body, with the file name, URI-encoded, in `x-file-name`.
#[tauri::command]
async fn save_file(app: tauri::AppHandle, window: tauri::Window, request: tauri::ipc::Request<'_>) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    windows::app_window_only(&window)?;
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

/// `{port, token, version, platform, stale}`. `stale: true` means the daemon belongs to another app
/// version and was left running because it is busy (a save or a running chat); it is replaced on
/// a later `daemon_info` call once idle, or at once by `restart_daemon {force: true}`.
/// Only the app window and the quick capture box (which saves through the service) are answered,
/// and a second app window (`windows.rs`), which shows the app as the main window does. A second
/// window is told about the service already running and nothing more: starting, replacing or
/// restarting the service stays with the main window, which a second window cannot be without.
#[tauri::command]
async fn daemon_info(window: tauri::Window) -> Result<Value, String> {
    if windows::is_view_label(window.label()) {
        return running_connection().await;
    }
    if !is_main_label(window.label()) && window.label() != capture::LABEL {
        return Err("not allowed".into());
    }
    connection().await
}

/// `daemon_info` for a second window: the service that has proved itself, never one started here.
async fn running_connection() -> Result<Value, String> {
    let alive = tauri::async_runtime::spawn_blocking(daemon_alive).await.map_err(|e| e.to_string())?;
    let (i, version) = alive.ok_or("The Thoughts daemon is not running")?;
    let stale = version.as_deref().unwrap_or(&i.version) != VERSION;
    Ok(json!({"port": i.port, "token": i.token, "version": i.version, "platform": PLATFORM, "stale": stale}))
}

/// What `daemon_info` answers.
async fn connection() -> Result<Value, String> {
    let (i, stale) = tauri::async_runtime::spawn_blocking(ensure_daemon).await.map_err(|e| e.to_string())??;
    Ok(json!({"port": i.port, "token": i.token, "version": i.version, "platform": PLATFORM, "stale": stale}))
}

/// Restart the background service: `restart_daemon {force?: boolean}` → same shape as `daemon_info`.
/// Without `force` it rejects with `BUSY::{"saving":bool,"chats":[title,…]}` while the daemon is
/// saving or a chat is running, and nothing is stopped.
#[tauri::command]
async fn restart_daemon(window: tauri::Window, force: Option<bool>) -> Result<Value, String> {
    main_only(&window)?;
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || stop_daemon_checked(force)).await.map_err(|e| e.to_string())??;
    connection().await
}

/// A check that could not tell whether a newer version exists. `code` is what the window has a
/// sentence for (`unpublished`, `offline`, `failed`); `detail` what happened.
struct NoAnswer { code: &'static str, detail: String }

/// What a failed check means, in a code the window has a sentence for. The address answered, but
/// with no release (`ReleaseNotFound`: nothing is published there yet): `unpublished`, which is
/// news rather than a failure. No connection: `offline`. Anything else: `failed`.
fn no_answer(e: tauri_plugin_updater::Error) -> NoAnswer {
    match e {
        tauri_plugin_updater::Error::ReleaseNotFound => NoAnswer { code: "unpublished", detail: String::new() },
        tauri_plugin_updater::Error::Reqwest(e) => NoAnswer { code: "offline", detail: e.to_string() },
        e => NoAnswer { code: "failed", detail: e.to_string() },
    }
}

/// Look for a newer version at the address this build was given (the public release's
/// `latest.json`), with no sign-in. `Ok(None)`: this copy is up to date.
async fn find_update(app: &tauri::AppHandle) -> Result<Option<tauri_plugin_updater::Update>, NoAnswer> {
    let updater = app.updater_builder().timeout(Duration::from_secs(10)).build().map_err(no_answer)?;
    updater.check().await.map_err(no_answer)
}

/// Is this copy somewhere it can never be replaced? An app opened straight from its disk image,
/// or from the hidden read-only place macOS runs a just-downloaded app from, cannot update
/// itself: it has to be moved to the Applications folder first. (A folder that merely needs an
/// administrator's password is not this: the updater asks for it.)
#[cfg(target_os = "macos")]
fn cannot_be_replaced() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let Some(bundle) = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) else { return false };
    if bundle.components().any(|c| c.as_os_str() == "AppTranslocation") {
        return true;
    }
    let Some(folder) = bundle.parent() else { return false };
    let probe = folder.join(format!(".thoughts-update-check-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            false
        }
        Err(e) => e.kind() == std::io::ErrorKind::ReadOnlyFilesystem,
    }
}

#[cfg(not(target_os = "macos"))]
fn cannot_be_replaced() -> bool {
    false
}

/// How a command reports a check that could not tell: `PROBLEM::<code>::<detail>`.
fn problem_error(code: &str, detail: &str) -> String {
    format!("PROBLEM::{code}::{detail}")
}

/// What `update_check` says about a newer version: its number and its
/// release page on GitHub (`page`, only for a plain version number). Never the manifest's notes:
/// that file is not signed, so its text must not reach the update dialog as the app's own words
/// before anything was checked. What a version changes is read in that version itself once it
/// runs (Help › What's new, from its own signed copy of the change log), or on its release page.
fn offered(version: &str) -> Value {
    let tag = format!("v{}", version.trim_start_matches('v'));
    let page = (!version.trim_start_matches('v').is_empty() && thoughtsd::release::valid_tag(&tag)).then(|| thoughtsd::release::page_url(&tag));
    json!({"configured": true, "available": true, "version": version, "page": page})
}

/// Is there an update? `{configured, available, reason?, version?, page?, problem?, detail?}`.
/// `configured: false` is not an error; `reason` says why: `"build"` (this build has no updater
/// key/endpoint) or `"deb"` (Linux, not running from an AppImage: a package-manager install
/// cannot replace itself — update through the package instead). A check that could not tell is
/// not an error either: `problem` is a code the window has a sentence for (see `NoAnswer`),
/// `detail` what happened.
#[tauri::command]
async fn update_check(app: tauri::AppHandle, window: tauri::Window) -> Result<Value, String> {
    main_only(&window)?;
    if !app.state::<UpdaterConfigured>().0 {
        return Ok(json!({"configured": false, "available": false, "reason": "build"}));
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_none() {
        return Ok(json!({"configured": false, "available": false, "reason": "deb"}));
    }
    Ok(match find_update(&app).await {
        // A newer version exists, but this copy sits where nothing can replace it.
        Ok(Some(update)) if cannot_be_replaced() => json!({"configured": true, "available": false, "problem": "move-first", "version": update.version}),
        Ok(Some(update)) => offered(&update.version),
        Ok(None) => json!({"configured": true, "available": false}),
        Err(n) => json!({"configured": true, "available": false, "problem": n.code, "detail": n.detail}),
    })
}

/// `update_install {force?: boolean}`: download, verify (signature) and install the available
/// update, then restart. Emits `update-progress` `{downloaded, total}` while downloading.
/// Installing stops the background service, so without `force` it rejects with
/// `BUSY::{"saving":bool,"chats":[title,…]}` while a save or a chat is running (checked before the
/// download and again just before installing); nothing is stopped or installed in that case.
#[tauri::command]
async fn update_install(app: tauri::AppHandle, window: tauri::Window, force: Option<bool>) -> Result<(), String> {
    main_only(&window)?;
    install_update(app, force.unwrap_or(false)).await
}

/// What `update_install` does. When it works it does not return: the app restarts as the new
/// version.
async fn install_update(app: tauri::AppHandle, force: bool) -> Result<(), String> {
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
    if cannot_be_replaced() {
        return Err(problem_error("move-first", ""));
    }
    let update = match find_update(&app).await {
        Ok(Some(update)) => update,
        Ok(None) => return Err("Thoughts is already up to date".into()),
        Err(n) => return Err(problem_error(n.code, &n.detail)),
    };
    let progress = app.clone();
    let mut downloaded = 0usize;
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

/// A rehearsal build updates itself with nobody clicking and writes down what happened, one JSON
/// line per step, for `scripts/release/rehearse-update.mjs` to read. No release contains this:
/// the `rehearsal` feature is switched on by that script alone, for a throwaway copy with its
/// own name, its own folder and a throwaway key.
#[cfg(feature = "rehearsal")]
mod rehearsal {
    use super::*;
    use std::io::Write;

    /// The keychain entry a rehearsal saves and reads back, apart from anything the real app keeps.
    const SERVICE: &str = "thoughts-rehearsal";

    fn note(file: &std::path::Path, v: Value) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
            let _ = writeln!(f, "{v}");
        }
    }

    /// Read the saved entry the way the app reads its own secrets, but stop waiting after a few
    /// seconds: a read that does not come back means macOS is asking the person about it.
    fn read_saved() -> Value {
        let (tx, rx) = std::sync::mpsc::channel();
        let started = Instant::now();
        std::thread::spawn(move || {
            let _ = tx.send(thoughtsd::secrets::get(SERVICE, "check"));
        });
        match rx.recv_timeout(Duration::from_secs(8)) {
            Ok(value) => json!({"value": value, "ms": started.elapsed().as_millis() as u64}),
            Err(_) => json!({"asked": true}),
        }
    }

    /// Started from `setup` when THOUGHTS_REHEARSAL names the file to write to.
    pub fn start(app: &tauri::AppHandle) {
        let Some(file) = std::env::var_os("THOUGHTS_REHEARSAL").map(std::path::PathBuf::from) else { return };
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            note(&file, json!({"step": "started", "version": VERSION, "exe": std::env::current_exe().ok()}));
            // The keychain, only in the scenario that asks for it (THOUGHTS_REHEARSAL_SAVE names the
            // version that saves): the old version saves an entry, the new one only reads it. The
            // new one must not save as well: macOS would hold that call until the person answered.
            if let Ok(saver) = std::env::var("THOUGHTS_REHEARSAL_SAVE") {
                note(&file, json!({"step": "keychain-read", "version": VERSION, "result": read_saved()}));
                if saver == VERSION {
                    let ok = thoughtsd::secrets::set(SERVICE, "check", &format!("saved by {VERSION}")).is_ok();
                    note(&file, json!({"step": "keychain-save", "version": VERSION, "ok": ok, "inAFile": thoughtsd::paths::secrets_file().exists()}));
                }
            }
            tauri::async_runtime::block_on(async {
                match find_update(&app).await {
                    Ok(Some(update)) => {
                        note(&file, json!({"step": "check", "version": VERSION, "offered": update.version, "notes": update.body, "notesAr": update.raw_json["notes_ar"]}));
                        drop(update);
                        // When this works it does not come back: the app restarts as the new version.
                        let failed = install_update(app.clone(), false).await.err();
                        note(&file, json!({"step": "install", "version": VERSION, "error": failed}));
                    }
                    Ok(None) => note(&file, json!({"step": "check", "version": VERSION, "offered": null})),
                    Err(n) => note(&file, json!({"step": "check", "version": VERSION, "problem": n.code, "detail": n.detail})),
                }
            });
            note(&file, json!({"step": "done", "version": VERSION}));
            QUIT_ALL.store(true, Ordering::SeqCst);
            app.exit(0);
        });
    }
}

/// Quit completely (after "Remove Thoughts from this computer": the settings are gone, so
/// "keep running in the background" must not keep a hidden window alive).
#[tauri::command]
fn quit_app(app: tauri::AppHandle, window: tauri::Window) {
    if !is_main_label(window.label()) {
        return;
    }
    QUIT_ALL.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// Rebuild the native menus after the UI language or the keyboard shortcuts changed. `bare`: while
/// Settings › Keyboard records a key the menu carries no keys of its own, so the combination reaches
/// the page; they come back with `bare: false`, or by themselves after a minute.
#[tauri::command]
fn refresh_menu(app: tauri::AppHandle, window: tauri::Window, bare: Option<bool>) -> Result<(), String> {
    main_only(&window)?;
    if let Some(on) = bare {
        let gen = BARE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        BARE_MENU.store(on, Ordering::SeqCst);
        if on {
            let a = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(60));
                if BARE_MENU.load(Ordering::SeqCst) && BARE_GEN.load(Ordering::SeqCst) == gen {
                    BARE_MENU.store(false, Ordering::SeqCst);
                    let b = a.clone();
                    let _ = a.run_on_main_thread(move || { let _ = install_menus(&b); });
                }
            });
        }
    } else {
        recovery::refresh_versions_soon(&app);
    }
    install_menus(&app).map_err(|e| e.to_string())
}

/// The menu shows no keys of its own right now (`refresh_menu { bare: true }`).
static BARE_MENU: AtomicBool = AtomicBool::new(false);
static BARE_GEN: AtomicU32 = AtomicU32::new(0);

/// The keys of the menu's own items: the user's (`keys.json` in the layer, `thoughtsd::keys::native`),
/// the app's own with customisations off, none at all while a key is being recorded.
pub(crate) fn menu_keys() -> thoughtsd::keys::NativeKeys {
    if BARE_MENU.load(Ordering::SeqCst) {
        return thoughtsd::keys::NativeKeys { settings: None, close_window: None, quit: None, fullscreen: None, close_changed: true };
    }
    let full = thoughtsd::settings::Settings::load().full_access();
    thoughtsd::keys::native_on(&thoughtsd::applayer::Dirs::real().layer, recovery::safe_mode(), thoughtsd::keys::Os::here(), full)
}

/// The page turned customisations off or back on (its own "Reload without customisations" and
/// "Turn customisations back on" buttons). The shell remembers it, so the choice survives a
/// restart and the Recovery menu shows the same thing as the page.
#[tauri::command]
fn set_safe_mode(app: tauri::AppHandle, window: tauri::Window, on: bool) -> Result<(), String> {
    main_only(&window)?;
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

/// Refuse a command called from anywhere but the app window (`main`, or the `main-<n>` Recovery
/// opens in its place). The second lock after the window grants in `capabilities/` (see build.rs).
fn main_only(window: &tauri::Window) -> Result<(), String> {
    if is_main_label(window.label()) { Ok(()) } else { Err("not allowed".into()) }
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
    #[cfg(target_os = "macos")]
    disable_webrtc(&window);
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
        // Asked for while it was leaving full screen to be hidden: it is not hidden after all.
        fullscreen::keep(w.label());
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
        "tray-quit" => ("Quit Thoughts completely", "إنهاء Thoughts تماماً"),
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
    let keys = menu_keys();
    // Close Window with a key the user chose (or none) is the app's own item: the system's has ⌘W fixed.
    let own_close = kind == "close_window" && keys.close_changed;
    if own_close || cfg!(target_os = "linux") && matches!(kind, "undo" | "redo" | "minimize" | "maximize" | "close_window") {
        let id = format!("std-{}", if kind == "close_window" { "close" } else { kind });
        // No accelerator for undo/redo: the web view already handles Ctrl+Z itself.
        let accel = if kind == "close_window" { keys.close_window.clone() } else { None };
        return Ok(Box::new(keyed_item(h, &id, label(ar, &id), accel.as_deref())?));
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
        _ => PredefinedMenuItem::separator(h)?,
    }))
}

/// "Quit Thoughts": an ordinary item rather than the predefined one, so quitting goes through
/// `quit_requested` on every platform (and works on Linux, where predefined Quit does not exist).
fn quit_item(h: &tauri::AppHandle, ar: bool) -> tauri::Result<MenuItem<tauri::Wry>> {
    // Windows closes with Alt+F4; Ctrl+Q is not a convention there.
    // Windows closes with Alt+F4; Ctrl+Q is not a convention there (`keys::native_defaults`).
    let accel = menu_keys().quit;
    keyed_item(h, "quit", label(ar, "quit"), accel.as_deref())
}

/// An item with the user's key, or without one if the system will not take that key.
pub(crate) fn keyed_item(h: &tauri::AppHandle, id: &str, text: &str, accel: Option<&str>) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(h, id, text, true, accel).or_else(|_| MenuItem::with_id(h, id, text, true, None::<&str>))
}

/// "Settings…" with the user's key.
fn settings_item(h: &tauri::AppHandle, ar: bool) -> tauri::Result<MenuItem<tauri::Wry>> {
    let accel = menu_keys().settings;
    keyed_item(h, "settings", label(ar, "settings"), accel.as_deref())
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
            &settings_item(h, ar)?,
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
    let view = Submenu::with_items(h, label(ar, "view"), true, &[&fullscreen::item(h, ar)?])?;
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

/// Windows / Linux: File (Settings…, Close, Quit) · Edit · Window (with Full Screen, F11) · Recovery · Help.
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
            &settings_item(h, ar)?,
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
        &[&*std_item(h, ar, "minimize")?, &*std_item(h, ar, "maximize")?, &PredefinedMenuItem::separator(h)?, &fullscreen::item(h, ar)?],
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
    // A full-screen window leaves full screen first: hidden as it is, macOS would keep its empty
    // Space on screen, black (`fullscreen::put_away`).
    if let Some(w) = main_window(app) {
        fullscreen::put_away(app, &w);
    }
}

/// The main window was closed (red button, ⌘W, Close Window).
///   * "keep running in the background" ON: the window is hidden and the app stays in the tray
///     and the Dock, with the background service.
///   * OFF: closing is quitting (`quit_requested`).
fn close_requested(app: &tauri::AppHandle) {
    if keep_running() && !QUIT_ALL.load(Ordering::SeqCst) {
        hide_to_tray(app);
    } else {
        quit_requested(app);
    }
}

/// Quit Thoughts (menu item, ⌘Q / Ctrl+Q): the desktop app always exits, window hidden or not.
/// What happens to the background service is the rule it always had (`RunEvent::Exit`):
///   * "keep running in the background" OFF: it is stopped, so `quit` first asks about chats
///     still running;
///   * ON: it carries on by itself, chats and all, until the app is opened again. The tray's
///     "Quit Thoughts completely" is the one action that stops it too.
/// Unsaved edits in a window are asked about either way.
/// (Until 0.7.1 ⌘Q with "keep running" on only hid the window, like closing it: the app never
/// left the Dock, which read as "⌘Q does not quit".)
fn quit_requested(app: &tauri::AppHandle) {
    quit::request(app, false);
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

/// The most a Dock icon may weigh: the UI's 512-pixel PNG is far smaller.
const DOCK_ICON_MAX: usize = 1 << 20;

/// May `bytes` go to the system as a Dock icon? Only a PNG (by its signature), and not a huge one.
fn dock_icon_ok(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > DOCK_ICON_MAX {
        return Err("icon is too large".into());
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("icon is not a PNG".into());
    }
    Ok(())
}

/// Replace the running app's Dock icon with a theme-coloured PNG rendered by the UI. The PNG is the
/// raw request body (a `Uint8Array`), from the app window only.
#[tauri::command]
fn set_dock_icon(app: tauri::AppHandle, window: tauri::Window, request: tauri::ipc::Request<'_>) -> Result<(), String> {
    main_only(&window)?;
    let tauri::ipc::InvokeBody::Raw(png) = request.body() else { return Err("icon is not a PNG".into()) };
    dock_icon_ok(png)?;
    #[cfg(target_os = "macos")]
    {
        let png = png.clone();
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

/// Switch peer connections (WebRTC) off in the app's web view. The app never uses them, and for an
/// add-on they would be a way to the network that no content-security policy governs: the STUN or
/// TURN server is chosen by the page, and its name is looked up (SEC-24). The switch is a WebKit
/// preference without a public name; it is only used when this system's WebKit has it. Frames
/// created after this (every add-on frame is) have no `RTCPeerConnection` at all.
#[cfg(target_os = "macos")]
fn disable_webrtc(win: &tauri::WebviewWindow) {
    let _ = win.with_webview(|wv| unsafe {
        use objc2::{msg_send, runtime::AnyObject, sel};
        let view = wv.inner() as *mut AnyObject;
        if view.is_null() { return; }
        let conf: *mut AnyObject = msg_send![view, configuration];
        if conf.is_null() { return; }
        let prefs: *mut AnyObject = msg_send![conf, preferences];
        if prefs.is_null() { return; }
        let has: bool = msg_send![prefs, respondsToSelector: sel!(_setPeerConnectionEnabled:)];
        if has { let _: () = msg_send![prefs, _setPeerConnectionEnabled: false]; }
    });
}

/// `thoughts updates …` in a terminal. Copies 0.0.12 to 0.7.0 had `updates gh on|off|status`
/// there: a switch that let a tester fetch updates with their GitHub sign-in while the
/// repository was private. The repository is public now and the updater asks the public release
/// with no sign-in, so the command only says so. It reads and writes nothing: a switch file an
/// older copy left in the app's folder (`updates-gh.json`) stays where it is and nothing reads it.
fn retired_updates_command(out: &mut impl std::io::Write) -> i32 {
    let _ = writeln!(out, "Thoughts checks for updates by itself, with no sign-in: nothing needs to be switched on in a terminal any more.");
    let _ = writeln!(out, "Choose Check for Updates… in the Thoughts menu, or Settings › Maintenance.");
    0
}

fn main() {
    if std::env::var("THOUGHTS_ASKPASS").is_ok() {
        std::process::exit(thoughtsd::askpass::main());
    }
    if std::env::args().nth(1).as_deref() == Some(thoughtsd::shell_guard::FLAG) {
        std::process::exit(thoughtsd::shell_guard::main());
    }
    // `thoughts updates …`: answered in the terminal, no window opens.
    if std::env::args().nth(1).as_deref() == Some("updates") {
        std::process::exit(retired_updates_command(&mut std::io::stdout()));
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
        // Without its own click script: that script opens a Shift/Ctrl-clicked or `_blank` link
        // before the window's link guard (App.svelte `guardLinks`) sees it, which skipped the
        // "this link goes somewhere else" warning. The guard opens every safe link itself.
        .plugin(tauri_plugin_opener::Builder::new().open_js_links_on_click(false).build())
        .plugin(voice::plugin())
        // The app window only ever shows the app itself; any attempt to navigate elsewhere is refused.
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("nav-guard")
                .on_navigation(|w, url| {
                    // A screen window's own, stricter check already passed (screen_check.rs).
                    if screen_check::is_label(w.label()) { return true; }
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
            openfile::open_library_file,
            openfile::reveal_library_file,
            openfile::open_chat_attachment,
            openfile::reveal_chat_attachment,
            save_pack,
            save_file,
            pdf::save_pdf,
            pdf::open_saved_pdf,
            daemon_info,
            restart_daemon,
            consent::approve_action,
            set_dock_icon,
            attention::set_attention,
            update_check,
            update_install,
            refresh_menu,
            quit_app,
            set_safe_mode,
            quit::set_unsaved,
            windows::open_window,
            windows::focus_main,
            links::take_deep_link,
            capture::open_capture,
            capture::capture_ready,
            capture::capture_take,
            capture::capture_show,
            capture::capture_hide,
            capture::capture_fit,
            capture::capture_open_main,
            capture::capture_shortcut,
            capture::set_capture_shortcut,
            capture::pause_capture_shortcut,
            voice::voice_status,
            voice::voice_permit,
            voice::voice_dictate_start,
            voice::voice_dictate_stop,
            voice::voice_dictate_cancel,
            voice::voice_record_start,
            voice::voice_record_pause,
            voice::voice_record_stop,
            voice::voice_record_discard,
            voice::voice_transcribe,
            voice::voice_transcribe_cancel,
            voice::voice_result,
            voice::voice_result_clear,
            screen_check::mado_screen_check,
            fullscreen::present_fullscreen
        ])
        // A page that starts loading holds nothing unsaved yet, whatever the last one reported.
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started && windows::is_app_label(webview.label()) {
                quit::page_started(webview.label());
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
            attention::watch_clicks(h);
            #[cfg(feature = "rehearsal")]
            rehearsal::start(h);
            #[cfg(feature = "screen-check-probe")]
            screen_check::probe::start(h);

            // Tray icon: the only visible handle on the app once its window is closed
            // while the background service keeps running.
            let mut tray = tauri::tray::TrayIconBuilder::with_id("main").tooltip("Thoughts");
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            let tray = tray.build(app)?;
            let _ = tray.set_visible(keep_running());
            // Esc never takes a window out of full screen (see fullscreen.rs).
            fullscreen::install_esc_guard();
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
            fullscreen::on_window_event(window, ev);
            if window.label() == capture::LABEL {
                capture::on_window_event(window, ev);
                return;
            }
            if windows::is_view_label(window.label()) {
                windows::on_window_event(window, ev);
                return;
            }
            if !is_main_label(window.label()) {
                return;
            }
            screen_check::main_window_event(ev);
            match ev {
                // With "keep running in the background" on, closing the window hides it and leaves
                // the tray icon. Otherwise closing quits and stops the daemon, and like every real
                // quit it first asks about chats still running and unsaved edits (`quit`).
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    close_requested(window.app_handle());
                }
                // Back in front: the list of earlier versions in the Recovery menu may be out of date.
                tauri::WindowEvent::Focused(true) => recovery::refresh_versions_soon(window.app_handle()),
                // A window Recovery replaced: what it reported unsaved went with it.
                tauri::WindowEvent::Destroyed => quit::forget(window.label()),
                _ => {}
            }
        })
        .on_menu_event(|app, ev| {
            let id = ev.id().as_ref();
            // Recovery is done natively, start to finish: it must work when the page does not.
            if recovery::handle(app, id) {
                return;
            }
            // The standard window items act on the app window in front (a second one, or the main one).
            let win = app.webview_windows().into_values().find(|w| windows::is_app_label(w.label()) && w.is_focused().unwrap_or(false)).or_else(|| main_window(app));
            match id {
                "settings" => { show_main(app); let _ = app.emit("open-settings", ()); }
                "help" => { show_main(app); let _ = app.emit("open-help", ()); }
                "check-updates" => { show_main(app); let _ = app.emit("check-updates", ()); }
                "tray-open" => show_main(app),
                "capture" | "tray-capture" => capture::from_menu(app),
                fullscreen::ID => fullscreen::toggle(app),
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
                if quit::stops_service(QUIT_ALL.load(Ordering::SeqCst), keep_running()) {
                    stop_daemon(false);
                }
            }
            // Clicking the Dock icon brings back a window hidden to the tray, also while only the
            // quick-capture box is on screen (it counts as a visible window).
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { has_visible_windows, .. } => {
                if !has_visible_windows || main_window(_app).map_or(true, |w| !w.is_visible().unwrap_or(false)) { show_main(_app); }
            }
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn plugin_and_pack_files_are_saved_only_as_what_they_are() {
        let zip = b"PK\x03\x04rest";
        assert_eq!(super::pack_file_name("Kanban-board.thoughts-plugin", "thoughts-plugin", zip).unwrap(), ("Kanban-board.thoughts-plugin".to_string(), "thoughts-plugin"));
        assert_eq!(super::pack_file_name("ساعات العمل", "thoughts-pack", zip).unwrap().0, "ساعات العمل.thoughts-pack");
        assert_eq!(super::pack_file_name("../../etc/passwd", "thoughts-pack", zip).unwrap().0, "etcpasswd.thoughts-pack");
        assert_eq!(super::pack_file_name("", "thoughts-plugin", zip).unwrap().0, "thoughts-pack.thoughts-plugin");
        assert!(super::pack_file_name("x", "app", zip).is_err() && super::pack_file_name("x", "", zip).is_err());
        assert!(super::pack_file_name("x", "thoughts-pack", b"{\"format\":\"thoughts-pack\"}").is_err(), "only the archive the daemon made");
        assert!(super::pack_file_name("x", "thoughts-pack", &[b"PK\x03\x04".as_slice(), &vec![0u8; 8 << 20]].concat()).is_err());
    }

    use super::*;

    #[test]
    fn an_offered_update_never_carries_the_manifest_notes() {
        let v = offered("0.1.0");
        assert_eq!(v["version"], "0.1.0");
        assert_eq!(v["page"], format!("https://github.com/{}/releases/tag/v0.1.0", thoughtsd::release::REPO));
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert!(!keys.iter().any(|k| k.to_lowercase().contains("notes") || *k == "body"), "{keys:?}");
        // A leading "v" is not doubled; anything that is not a plain version number gets no page.
        assert_eq!(offered("v0.2.0-beta.1")["page"], format!("https://github.com/{}/releases/tag/v0.2.0-beta.1", thoughtsd::release::REPO));
        for odd in ["", "1.0/../../x", "1.0 run this", "1.0?x=1", "1.0#x"] {
            assert!(offered(odd)["page"].is_null(), "{odd}");
        }
    }

    #[test]
    fn a_check_with_no_release_says_unpublished_and_nothing_else_is_asked() {
        let n = no_answer(tauri_plugin_updater::Error::ReleaseNotFound);
        assert_eq!((n.code, n.detail.as_str()), ("unpublished", ""));
        assert_eq!(no_answer(tauri_plugin_updater::Error::UnsupportedOs).code, "failed");
    }

    #[test]
    fn the_old_terminal_switch_only_explains_and_a_leftover_switch_file_changes_nothing() {
        // A home an older copy left with the fallback switched on.
        let home = std::env::temp_dir().join(format!("thoughts-retired-switch-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let switch = home.join("updates-gh.json");
        std::fs::write(&switch, "{\"gh\": true}\n").unwrap();
        let mut said = Vec::new();
        assert_eq!(retired_updates_command(&mut said), 0);
        let said = String::from_utf8(said).unwrap();
        assert!(said.contains("no sign-in") && !said.contains("gh auth"), "{said}");
        assert_eq!(std::fs::read_to_string(&switch).unwrap(), "{\"gh\": true}\n", "the file is neither read nor changed");
        std::fs::remove_dir_all(&home).unwrap();
    }

    fn url(s: &str) -> tauri::Url { tauri::Url::parse(s).unwrap() }

    #[test]
    fn only_a_png_of_sensible_size_becomes_the_dock_icon() {
        let png = |len: usize| { let mut b = b"\x89PNG\r\n\x1a\n".to_vec(); b.resize(len, 0); b };
        assert!(dock_icon_ok(&png(40_000)).is_ok());
        assert!(dock_icon_ok(&png(DOCK_ICON_MAX)).is_ok());
        assert!(dock_icon_ok(&png(DOCK_ICON_MAX + 1)).is_err(), "too large");
        assert!(dock_icon_ok(b"").is_err());
        assert!(dock_icon_ok(b"\x89PNG").is_err(), "a cut-off signature");
        assert!(dock_icon_ok(b"GIF89a\0\0\0\0").is_err());
        assert!(dock_icon_ok(b"<svg xmlns='http://www.w3.org/2000/svg'/>").is_err());
        let mut tiff = vec![0u8; 100];
        tiff[..4].copy_from_slice(b"MM\0*");
        assert!(dock_icon_ok(&tiff).is_err());
    }

    #[test]
    fn generated_files_are_saved_only_as_documents_that_cannot_run() {
        let docx = b"PK\x03\x04 rest of a Word package";
        assert_eq!(export_file("Plan.docx", docx), Ok(("Plan.docx".into(), "docx".into())));
        assert_eq!(export_file("خطة العمل.DOCX", docx), Ok(("خطة العمل.docx".into(), "docx".into())));
        assert_eq!(export_file("calendar.ics", b"BEGIN:VCALENDAR"), Ok(("calendar.ics".into(), "ics".into())));
        assert_eq!(export_file("../../etc/evil.md", b"x"), Ok(("evil.md".into(), "md".into())));
        assert_eq!(export_file("<>.html", b"x"), Ok(("export.html".into(), "html".into())));
        // A drawing: an SVG that is only a picture, or a draw.io file.
        let svg = br##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" role="img" aria-label="x" data-node-ui=""><style>@font-face { font-family: 'M'; src: url("data:font/woff2;base64,AA==") format('woff2'); } @font-face{font-family:'N';src:url(data:font/woff2;base64,AA==)}</style><defs><marker id="a"><polygon points="0 0"/></marker><pattern id="g"><path d="M0 0"/></pattern></defs><title>t</title><desc>d</desc><g data-node-id="n" tabindex="0"><rect fill="url(#g)" width="10" height="10"/><circle r="1"/><ellipse rx="1"/><line x1="0"/><polyline points="0,0"/><path d="M0 0" marker-end="url(#a)"/><text font-family="M" direction="rtl">onload=alert(1) &lt;script&gt; javascript: <tspan x="1">on = off</tspan></text></g></svg>"##;
        assert_eq!(export_file("Sales by region.svg", svg), Ok(("Sales by region.svg".into(), "svg".into())));
        assert_eq!(export_file("flow.drawio", b"<mxfile host=\"Thoughts\"></mxfile>"), Ok(("flow.drawio".into(), "drawio".into())));
        assert!(export_file("flow.drawio", b"<html>").is_err());
        assert!(export_file("flow.drawio", b"<mxfile><diagram><script/></diagram></mxfile>").is_err());
        assert!(export_file("flow.drawio", b"<mxfile><diagram name=\"a\"><mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"2\" value=\"A &amp;lt;b&amp;gt;\" vertex=\"1\"><mxGeometry x=\"1\" as=\"geometry\"><Array as=\"points\"><mxPoint x=\"1\"/></Array></mxGeometry></mxCell></root></mxGraphModel></diagram></mxfile>").is_ok());
        // Not a picture and nothing else: every way an SVG can run or fetch something, however it is spelled.
        let ns = "xmlns=\"http://www.w3.org/2000/svg\"";
        for bad in [
            "<html><svg/></html>".to_string(), "<svg/>".to_string(), "not xml".to_string(),
            format!("<svg {ns}><script>alert(1)</script></svg>"), format!("<svg {ns}><SCRIPT/></svg>"),
            format!("<svg {ns} onload=\"x()\"/>"), format!("<svg {ns}><rect ONCLICK='x()'/></svg>"), format!("<svg {ns}><rect\nonmouseover\n=\n'x()'/></svg>"),
            format!("<svg {ns}><a href=\"javascript:x()\"/></svg>"), format!("<svg {ns}><foreignObject/></svg>"),
            format!("<svg {ns}><image href=\"https://x/y.png\"/></svg>"), format!("<svg {ns}><use href=\"#a\"/></svg>"),
            format!("<svg {ns} xmlns:xlink=\"http://www.w3.org/1999/xlink\"><rect xlink:href=\"#a\"/></svg>"),
            format!("<svg {ns}><animate attributeName=\"x\"/></svg>"), format!("<svg {ns}><set/></svg>"),
            format!("<svg {ns}><rect style=\"background:url(https://x/y)\"/></svg>"),
            format!("<svg {ns}><rect fill=\"url(https://x/#a)\"/></svg>"), format!("<svg {ns}><rect fill=\"u&#114;l( 'HTTPS://x/#a')\"/></svg>"),
            format!("<svg {ns}><rect fill=\"url(data:image/svg+xml,x)\"/></svg>"), format!("<svg {ns}><rect fill=\"u\\72l(//x)\"/></svg>"),
            format!("<svg {ns}><style>@import url(https://x/y.css);</style></svg>"), format!("<svg {ns}><style>@im/**/port 'x';</style></svg>"),
            format!("<svg {ns}><style>rect{{fill:red}}</style></svg>"), format!("<svg {ns}><style>@font-face{{src:url(https://x/f.woff2)}}</style></svg>"),
            format!("<svg {ns}><style>@font-face{{src:url(data:font/woff2;base64,AA==)}}*{{background:url(//x)}}</style></svg>"),
            format!("<svg {ns}><style><![CDATA[@font-face{{}}]]><script/></style></svg>"),
            format!("<?xml-stylesheet href=\"https://x/y.css\"?><svg {ns}/>"), format!("<svg {ns}><?php x ?></svg>"),
            format!("<!DOCTYPE svg [<!ENTITY x \"y\">]><svg {ns}/>"), format!("<x:svg xmlns:x=\"http://www.w3.org/1999/xhtml\"/>"),
            format!("<svg {ns}><h:script xmlns:h=\"http://www.w3.org/1999/xhtml\"/></svg>"),
        ] {
            assert!(export_file("x.svg", bad.as_bytes()).is_err(), "{bad}");
        }
        assert!(export_file("x.svg", b"\xff\xfe").is_err());
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
        // (Windows knows a process by its program's file name only: see `is_daemon_command`.)
        let daemon = if cfg!(windows) { "Thoughts.exe" } else { "/Applications/Thoughts.app/Contents/MacOS/Thoughts --daemon" };
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
        #[cfg(not(windows))]
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), 4242, Some("/Applications/Thoughts.app/Contents/MacOS/Thoughts"), exe), Unproven::Leave);
        // Never this process, and never pid 0 or 1.
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), std::process::id(), Some(daemon), exe), Unproven::Leave);
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), 1, Some(daemon), exe), Unproven::Leave);
        assert_eq!(unproven_daemon(&Probe::Legacy, &held(), 0, Some(daemon), exe), Unproven::Leave);
    }

    #[test]
    fn chats_running_in_another_library_count_as_work() {
        // `GET /api/libraries` as the service answers it: the open library's chats are already in
        // the chat list, so only the others are named, and only when something runs in them.
        let libraries = json!({"openId": "a", "libraries": [
            {"id": "a", "name": "Personal", "displayName": "Personal", "open": true, "running": 1},
            {"id": "b", "name": "work", "displayName": "Work", "open": false, "running": 2},
            {"id": "c", "name": "Client", "displayName": "Client", "open": false, "running": 0},
            {"id": "d", "name": "Old", "open": false, "running": 1},
        ]});
        assert_eq!(running_elsewhere(&libraries), ["Work", "Old"]);
        // A service from before several libraries existed answers with an error, or nothing.
        assert!(running_elsewhere(&json!({"error": "not found"})).is_empty());
        assert!(running_elsewhere(&Value::Null).is_empty());
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
