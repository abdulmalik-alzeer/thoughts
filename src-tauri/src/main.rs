// Thoughts desktop shell.
//
// One executable, three modes:
//   * GUI (default)        — Tauri window; connects to (or spawns) the daemon
//   * `--daemon`           — runs thoughtsd, detached, so sessions outlive the UI
//   * THOUGHTS_ASKPASS=1   — git credential helper backed by the OS keychain
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Whether an updater public key and endpoint are configured in this build.
struct UpdaterConfigured(bool);

/// The live daemon and the version it reports, if one answers.
fn daemon_alive() -> Option<(thoughtsd::paths::DaemonInfo, Option<String>)> {
    let info = thoughtsd::paths::read_daemon_info()?;
    let body = ureq::get(&format!("http://127.0.0.1:{}/api/ping", info.port))
        .set("Authorization", &format!("Bearer {}", info.token))
        .timeout(Duration::from_millis(800))
        .call()
        .ok()?
        .into_string()
        .unwrap_or_default();
    let version = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v["version"].as_str().map(String::from));
    Some((info, version))
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
    let mut refused = busy(shutdown(false));
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
    let mut kill;
    #[cfg(unix)]
    {
        kill = std::process::Command::new("kill");
        kill.arg(i.pid.to_string());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        kill = std::process::Command::new("taskkill");
        kill.args(["/PID", &i.pid.to_string(), "/F"]).creation_flags(0x08000000); // UNTESTED on Windows
    }
    let _ = kill.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
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
    spawn_daemon().map_err(|e| e.to_string())?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(150));
        if let Some((i, _)) = daemon_alive() {
            return Ok((i, false));
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

/// Open a document from the library in the user's default app. Only regular files inside the
/// library, and only document/media types that cannot run code when opened.
#[tauri::command]
fn open_library_file(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    const SAFE: &[&str] = &["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "csv", "tsv", "txt", "md", "rtf", "odt",
        "ods", "odp", "png", "jpg", "jpeg", "gif", "webp", "heic", "tiff", "bmp", "mp3", "m4a", "wav", "mp4", "mov", "epub"];
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

/// Rebuild the native menus after the UI language changed.
#[tauri::command]
fn refresh_menu(app: tauri::AppHandle) -> Result<(), String> {
    install_menus(&app).map_err(|e| e.to_string())
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
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
        "rollback" => ("Rollback app…", "استعادة نسخة سابقة…"),
        "undo-layer" => ("Undo last app change", "التراجع عن آخر تغيير في التطبيق"),
        "safe-mode" => ("Reload in safe mode (no customisations)", "إعادة التحميل في الوضع الآمن"),
        "reload" => ("Reload", "إعادة التحميل"),
        "restart-daemon" => ("Restart background service", "إعادة تشغيل خدمة الخلفية"),
        "help" => ("Thoughts Help", "مساعدة Thoughts"),
        "check-updates" => ("Check for Updates…", "التحقق من وجود تحديثات…"),
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

fn recovery_menu(h: &tauri::AppHandle, ar: bool) -> tauri::Result<Submenu<tauri::Wry>> {
    // The Recovery menu lives in the native shell, outside the app layer,
    // so no customisation can remove it.
    Submenu::with_items(
        h,
        label(ar, "recovery"),
        true,
        &[
            &item(h, ar, "rollback", None)?,
            &item(h, ar, "undo-layer", None)?,
            &PredefinedMenuItem::separator(h)?,
            &item(h, ar, "safe-mode", None)?,
            &item(h, ar, "reload", Some("CmdOrCtrl+R"))?,
            &item(h, ar, "restart-daemon", None)?,
        ],
    )
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
    let file = Submenu::with_items(h, label(ar, "file"), true, &[&*std_item(h, ar, "close_window")?])?;
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
    Menu::with_items(h, &[&app_menu, &file, &edit_menu(h, ar)?, &view, &window, &recovery_menu(h, ar)?, &help])
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
    Menu::with_items(h, &[&file, &edit_menu(h, ar)?, &window, &recovery_menu(h, ar)?, &help])
}

/// Hide the main window and leave the tray icon as the way back.
fn hide_to_tray(app: &tauri::AppHandle) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_visible(true);
    }
    if let Some(w) = app.get_webview_window("main") {
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
        app.exit(0);
    }
}

fn tray_menu(h: &tauri::AppHandle, ar: bool) -> tauri::Result<Menu<tauri::Wry>> {
    Menu::with_items(
        h,
        &[&item(h, ar, "tray-open", None)?, &PredefinedMenuItem::separator(h)?, &item(h, ar, "tray-quit", None)?],
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
        // Must be the first plugin: a second launch focuses the existing window and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        // The app window only ever shows the app itself; any attempt to navigate elsewhere is refused.
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("nav-guard")
                .on_navigation(|_, url| {
                    matches!(url.scheme(), "tauri" | "asset" | "about" | "blob" | "data")
                        || matches!(url.host_str(), Some("tauri.localhost") | Some("localhost") | Some("127.0.0.1"))
                            && (url.host_str() != Some("127.0.0.1") || cfg!(debug_assertions))
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
            save_pack,
            daemon_info,
            restart_daemon,
            set_dock_icon,
            update_check,
            update_install,
            refresh_menu
        ])
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
            install_menus(h)?;
            Ok(())
        })
        .on_window_event(|window, ev| {
            // With "keep running in the background" on, closing the window hides it and
            // leaves the tray icon; otherwise closing quits (and stops the daemon) as before.
            if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
                if window.label() == "main" && keep_running() && !QUIT_ALL.load(Ordering::SeqCst) {
                    api.prevent_close();
                    hide_to_tray(window.app_handle());
                }
            }
        })
        .on_menu_event(|app, ev| {
            let win = app.get_webview_window("main");
            match ev.id().as_ref() {
                "settings" => { show_main(app); let _ = app.emit("open-settings", ()); }
                "help" => { show_main(app); let _ = app.emit("open-help", ()); }
                "check-updates" => { show_main(app); let _ = app.emit("check-updates", ()); }
                "tray-open" => show_main(app),
                "tray-quit" => {
                    QUIT_ALL.store(true, Ordering::SeqCst);
                    app.exit(0);
                }
                "rollback" => { let _ = app.emit("recovery", "rollback"); }
                // The UI does the undo (POST /api/layer/undo-last) so it can confirm and report.
                "undo-layer" => { show_main(app); let _ = app.emit("undo-layer", ()); }
                "quit" => quit_requested(app),
                // Linux stand-ins for the standard items GTK does not provide (see `std_item`).
                "std-close" => if let Some(w) = win { let _ = w.close(); },
                "std-minimize" => if let Some(w) = win { let _ = w.minimize(); },
                "std-maximize" => if let Some(w) = win {
                    let _ = if w.is_maximized().unwrap_or(false) { w.unmaximize() } else { w.maximize() };
                },
                "std-undo" => if let Some(w) = win { let _ = w.eval("document.execCommand('undo')"); },
                "std-redo" => if let Some(w) = win { let _ = w.eval("document.execCommand('redo')"); },
                "safe-mode" => if let Some(w) = win {
                    let _ = w.eval("location.replace(location.pathname + '?safe=1')");
                },
                "reload" => if let Some(w) = win { let _ = w.eval("location.reload()"); },
                // Refused while a save or a chat is running: the UI is told with `daemon-busy`
                // `{action: "restart", saving, chats}` and may call `restart_daemon {force: true}`.
                "restart-daemon" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = restart_daemon(None).await {
                            let mut work = e.strip_prefix("BUSY::").and_then(|j| serde_json::from_str::<Value>(j).ok())
                                .unwrap_or_else(|| json!({"error": e}));
                            work["action"] = json!("restart");
                            show_main(&app);
                            let _ = app.emit("daemon-busy", work);
                        }
                    });
                }
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
