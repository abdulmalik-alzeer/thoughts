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
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
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

fn spawn_daemon() -> anyhow::Result<()> {
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

fn ensure_daemon() -> Result<thoughtsd::paths::DaemonInfo, String> {
    let _guard = DAEMON_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((i, version)) = daemon_alive() {
        // After an app update the old daemon is still running the old code: replace it.
        let stale = !VERSION_CHECKED.swap(true, Ordering::SeqCst) && version.as_deref().unwrap_or(&i.version) != VERSION;
        if !stale {
            return Ok(i);
        }
        stop_daemon(true);
    }
    VERSION_CHECKED.store(true, Ordering::SeqCst);
    spawn_daemon().map_err(|e| e.to_string())?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(150));
        if let Some((i, _)) = daemon_alive() {
            return Ok(i);
        }
    }
    Err("The Thoughts daemon did not start".into())
}

/// Save an exported customisation pack to the place the user picked in the save dialog.
/// JSON packs only, with a size cap, and never over an existing folder.
#[tauri::command]
fn save_text_file(path: String, contents: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(&path);
    if !p.is_absolute() || p.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()) != Some("json".into()) {
        return Err("packs are saved as .json files".into());
    }
    if contents.len() > 8 << 20 { return Err("pack is too large".into()); }
    if p.is_dir() { return Err("that is a folder".into()); }
    std::fs::write(&p, contents).map_err(|e| e.to_string())
}

/// Open a document from the library in the user's default app. Only regular files inside the
/// library, and only document/media types that cannot run code when opened.
#[tauri::command]
fn open_library_file(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    const SAFE: &[&str] = &["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "csv", "tsv", "txt", "md", "rtf", "odt",
        "ods", "odp", "png", "jpg", "jpeg", "gif", "webp", "heic", "tiff", "bmp", "mp3", "m4a", "wav", "mp4", "mov", "epub"];
    let lib = thoughtsd::settings::Settings::load().library().ok_or("no library")?;
    let root = std::fs::canonicalize(&lib).map_err(|e| e.to_string())?;
    let rel = std::path::Path::new(&path);
    if rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err("invalid path".into());
    }
    let full = std::fs::canonicalize(root.join(rel)).map_err(|e| e.to_string())?;
    if !full.starts_with(&root) || !full.is_file() { return Err("not a file in the library".into()); }
    let ext = full.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !SAFE.contains(&ext.as_str()) { return Err("this file type can't be opened from Thoughts".into()); }
    app.opener().open_path(full.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
async fn daemon_info() -> Result<Value, String> {
    let i = tauri::async_runtime::spawn_blocking(ensure_daemon).await.map_err(|e| e.to_string())??;
    Ok(json!({"port": i.port, "token": i.token, "version": i.version, "platform": PLATFORM}))
}

#[tauri::command]
async fn restart_daemon() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _guard = DAEMON_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        stop_daemon(true);
    })
    .await
    .map_err(|e| e.to_string())?;
    daemon_info().await
}

fn daemon_post(path: &str, body: Value) -> Result<(), String> {
    let i = ensure_daemon()?;
    ureq::post(&format!("http://127.0.0.1:{}{}", i.port, path))
        .set("Authorization", &format!("Bearer {}", i.token))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Is there an update? `{configured, available, version?, notes?}`.
/// `configured: false` means this build has no updater key/endpoint — not an error.
#[tauri::command]
async fn update_check(app: tauri::AppHandle) -> Result<Value, String> {
    if !app.state::<UpdaterConfigured>().0 {
        return Ok(json!({"configured": false, "available": false}));
    }
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    Ok(match update {
        Some(u) => json!({"configured": true, "available": true, "version": u.version, "notes": u.body}),
        None => json!({"configured": true, "available": false}),
    })
}

/// Download, verify (signature) and install the available update, then restart.
/// Emits `update-progress` `{downloaded, total}` while downloading.
#[tauri::command]
async fn update_install(app: tauri::AppHandle) -> Result<(), String> {
    if !app.state::<UpdaterConfigured>().0 {
        return Err("Updates are not configured in this build".into());
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
    let _ = tauri::async_runtime::spawn_blocking(|| {
        let _guard = DAEMON_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        stop_daemon(true);
    })
    .await;
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
        "recovery" => ("Recovery", "الاسترداد"),
        "rollback" => ("Rollback app…", "استعادة نسخة سابقة…"),
        "undo-layer" => ("Undo last app change", "التراجع عن آخر تغيير"),
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
        _ => ("", ""),
    };
    if ar { arabic } else { en }
}

fn item(h: &tauri::AppHandle, ar: bool, id: &str, accel: Option<&str>) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(h, id, label(ar, id), true, accel)
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

/// macOS: the standard menu bar, with Settings… (⌘,) and Check for Updates… in the app menu.
#[cfg(target_os = "macos")]
fn build_menu(h: &tauri::AppHandle, ar: bool) -> tauri::Result<Menu<tauri::Wry>> {
    use tauri::menu::{MenuItemKind, HELP_SUBMENU_ID};
    let menu = Menu::default(h)?;
    // On macOS the first submenu is always the app menu: About, ―, Services, …
    if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
        app_menu.insert(&item(h, ar, "check-updates", None)?, 1)?;
        app_menu.insert(&PredefinedMenuItem::separator(h)?, 2)?;
        app_menu.insert(&item(h, ar, "settings", Some("CmdOrCtrl+Comma"))?, 3)?;
    }
    // Recovery goes before Help, which stays last.
    let help = menu.get(HELP_SUBMENU_ID).and_then(|k| k.as_submenu().cloned());
    if let Some(help) = &help {
        menu.remove(help)?;
    }
    menu.append(&recovery_menu(h, ar)?)?;
    if let Some(help) = &help {
        help.append(&item(h, ar, "help", None)?)?;
        menu.append(help)?;
    }
    Ok(menu)
}

/// Windows / Linux: File (Settings…, Close, Quit) · Edit · Window · Recovery · Help.
/// UNTESTED: written on macOS; needs a pass on real Windows and Linux.
#[cfg(not(target_os = "macos"))]
fn build_menu(h: &tauri::AppHandle, ar: bool) -> tauri::Result<Menu<tauri::Wry>> {
    use tauri::menu::{AboutMetadata, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID};
    let about = AboutMetadata {
        name: Some(h.package_info().name.clone()),
        version: Some(h.package_info().version.to_string()),
        ..Default::default()
    };
    let file = Submenu::with_items(
        h,
        label(ar, "file"),
        true,
        &[
            &item(h, ar, "settings", Some("CmdOrCtrl+Comma"))?,
            &PredefinedMenuItem::separator(h)?,
            &PredefinedMenuItem::close_window(h, None)?,
            &PredefinedMenuItem::quit(h, None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        h,
        label(ar, "edit"),
        true,
        &[
            &PredefinedMenuItem::undo(h, None)?,
            &PredefinedMenuItem::redo(h, None)?,
            &PredefinedMenuItem::separator(h)?,
            &PredefinedMenuItem::cut(h, None)?,
            &PredefinedMenuItem::copy(h, None)?,
            &PredefinedMenuItem::paste(h, None)?,
            &PredefinedMenuItem::select_all(h, None)?,
        ],
    )?;
    let window = Submenu::with_id_and_items(
        h,
        WINDOW_SUBMENU_ID,
        label(ar, "window"),
        true,
        &[&PredefinedMenuItem::minimize(h, None)?, &PredefinedMenuItem::maximize(h, None)?],
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
            &PredefinedMenuItem::about(h, None, Some(about))?,
        ],
    )?;
    Menu::with_items(h, &[&file, &edit, &window, &recovery_menu(h, ar)?, &help])
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
            save_text_file,
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
                    if let Some(tray) = window.app_handle().tray_by_id("main") {
                        let _ = tray.set_visible(true);
                    }
                    let _ = window.hide();
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
                "undo-layer" => {
                    let _ = daemon_post("/api/layer/rollback", json!({"hash": "HEAD~1"}));
                    if let Some(w) = win { let _ = w.eval("location.reload()"); }
                }
                "safe-mode" => if let Some(w) = win {
                    let _ = w.eval("location.replace(location.pathname + '?safe=1')");
                },
                "reload" => if let Some(w) = win { let _ = w.eval("location.reload()"); },
                "restart-daemon" => {
                    tauri::async_runtime::spawn(async { let _ = restart_daemon().await; });
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
