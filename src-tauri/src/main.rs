// Thoughts desktop shell.
//
// One executable, three modes:
//   * GUI (default)        — Tauri window; connects to (or spawns) the daemon
//   * `--daemon`           — runs thoughtsd, detached, so sessions outlive the UI
//   * THOUGHTS_ASKPASS=1   — git credential helper backed by the OS keychain
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager};

fn daemon_alive() -> Option<thoughtsd::paths::DaemonInfo> {
    let info = thoughtsd::paths::read_daemon_info()?;
    let ok = ureq::get(&format!("http://127.0.0.1:{}/api/ping", info.port))
        .set("Authorization", &format!("Bearer {}", info.token))
        .timeout(Duration::from_millis(800))
        .call()
        .is_ok();
    ok.then_some(info)
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
        c.creation_flags(0x00000008 | 0x00000200); // DETACHED_PROCESS | NEW_PROCESS_GROUP
    }
    c.spawn()?;
    Ok(())
}

fn ensure_daemon() -> Result<thoughtsd::paths::DaemonInfo, String> {
    if let Some(i) = daemon_alive() {
        return Ok(i);
    }
    spawn_daemon().map_err(|e| e.to_string())?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(150));
        if let Some(i) = daemon_alive() {
            return Ok(i);
        }
    }
    Err("The Thoughts daemon did not start".into())
}

#[tauri::command]
async fn daemon_info() -> Result<Value, String> {
    let i = tauri::async_runtime::spawn_blocking(ensure_daemon).await.map_err(|e| e.to_string())??;
    Ok(json!({"port": i.port, "token": i.token, "version": i.version}))
}

#[tauri::command]
async fn restart_daemon() -> Result<Value, String> {
    if let Some(i) = daemon_alive() {
        let _ = ureq::post(&format!("http://127.0.0.1:{}/api/shutdown", i.port))
            .set("Authorization", &format!("Bearer {}", i.token))
            .call();
        std::thread::sleep(Duration::from_millis(600));
    }
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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![daemon_info, restart_daemon, set_dock_icon])
        .setup(|app| {
            // The Recovery menu lives in the native shell, outside the app layer,
            // so no customisation can remove it.
            let h = app.handle();
            let menu = Menu::default(h)?;
            // ⌘, opens Settings, like every Mac app: insert into the app menu after About.
            if let Some(tauri::menu::MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
                app_menu.insert(&PredefinedMenuItem::separator(h)?, 1)?;
                app_menu.insert(&MenuItem::with_id(h, "settings", "Settings…", true, Some("CmdOrCtrl+Comma"))?, 2)?;
            }
            let recovery = Submenu::with_items(
                h,
                "Recovery",
                true,
                &[
                    &MenuItem::with_id(h, "rollback", "Rollback app…", true, None::<&str>)?,
                    &MenuItem::with_id(h, "undo-layer", "Undo last app change", true, None::<&str>)?,
                    &PredefinedMenuItem::separator(h)?,
                    &MenuItem::with_id(h, "safe-mode", "Reload in safe mode (no customisations)", true, None::<&str>)?,
                    &MenuItem::with_id(h, "reload", "Reload", true, Some("CmdOrCtrl+R"))?,
                    &MenuItem::with_id(h, "restart-daemon", "Restart background service", true, None::<&str>)?,
                ],
            )?;
            menu.append(&recovery)?;
            app.set_menu(menu)?;
            Ok(())
        })
        .on_menu_event(|app, ev| {
            let win = app.get_webview_window("main");
            match ev.id().as_ref() {
                "settings" => { let _ = app.emit("open-settings", ()); }
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
        .run(|_app, ev| {
            if let tauri::RunEvent::Exit = ev {
                let keep = thoughtsd::settings::Settings::load().daemon.keep_running;
                if !keep {
                    let _ = daemon_post("/api/shutdown", json!({}));
                }
            }
        });
}
