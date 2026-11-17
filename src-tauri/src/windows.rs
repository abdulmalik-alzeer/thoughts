//! More app windows: a note or a chat opened in a window of its own ("Open in new window"), so two
//! notes, or a chat and a note, can sit side by side.
//!
//! A second window loads the same app page as the main window, at the place it was opened for
//! (`index.html?open=docs&path=…` / `?open=chat&id=…`), and talks to the background service on its
//! own connection, so it hears every change the main window hears. Its labels are `view-<n>`: never
//! `main-…`, which Recovery uses for a window that replaces the main one.
//!
//! What stays with the main window alone: the Dock icon and its count, notifications, thoughts://
//! links, the menus, updates, safe mode, restarting or quitting, voice, the quick capture shortcut
//! and the yes to switching off permission checks or trusting a folder. A second window may call
//! only the commands granted in `capabilities/window.json` (see `the_second_window_gets_only_what_it_needs`);
//! the page sends the person to the main window for the rest (`focus_main`).
//!
//! Closing a second window never closes the main one or quits the app. When the window reported
//! unsaved work (`set_unsaved`), closing it asks first, in a native dialog of its own.

use serde::Deserialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult};

/// The labels of second windows start with this, followed by a number.
pub(crate) const PREFIX: &str = "view-";
/// No more second windows than this at once: each one is a whole web page.
const MOST: usize = 12;
/// How many second windows have been opened since the app started (their numbers).
static OPENED: AtomicU32 = AtomicU32::new(0);
/// Second windows whose close question is on screen.
static ASKING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Is `label` a second app window (`view-<n>`)?
pub(crate) fn is_view_label(label: &str) -> bool {
    label.strip_prefix(PREFIX).is_some_and(|n| !n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Is `label` a window that shows the app itself: the main window (or the one Recovery opened in
/// its place) or a second window?
pub(crate) fn is_app_label(label: &str) -> bool {
    crate::is_main_label(label) || is_view_label(label)
}

/// Refuse a command called from anywhere but an app window (the main one or a second one). For
/// the commands every app window needs: opening and showing library files and chat attachments,
/// saving an exported document, reporting unsaved work.
pub(crate) fn app_window_only(window: &tauri::Window) -> Result<(), String> {
    if is_app_label(window.label()) { Ok(()) } else { Err("not allowed".into()) }
}

/// A place in the app a window can be opened at or sent to.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Route {
    kind: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

/// What a route may name: an id of plain letters, digits and a few marks, a library path of
/// ordinary text. The page asks the service for what they name, and the service keeps to the
/// library; this only keeps the address of the window clean.
fn plain_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn plain_path(path: &str) -> bool {
    !path.is_empty()
        && path.chars().count() <= 1024
        && !path.starts_with(['/', '\\', '?'])
        && !path.chars().any(|c| c.is_control())
        && !path.split(['/', '\\']).any(|part| part == "..")
}

impl Route {
    /// A route a second window may be opened at: a note in Docs, or a chat.
    pub(crate) fn for_window(self) -> Result<Route, String> {
        match (self.kind.as_str(), self.id.as_deref(), self.path.as_deref()) {
            ("chat", Some(id), None) if thoughtsd::chats::valid_id(id) => Ok(self),
            ("docs", None, Some(path)) if plain_path(path) => Ok(self),
            _ => Err("this cannot be opened in a window of its own".into()),
        }
    }

    /// A route a second window may send the main window to: what it has and the places only the
    /// main window shows (Settings and Customise, with one of their sections).
    pub(crate) fn for_main(self) -> Result<Route, String> {
        match (self.kind.as_str(), self.id.as_deref(), self.path.as_deref()) {
            ("settings" | "customise", _, None) if self.id.as_deref().map_or(true, plain_id) => Ok(self),
            _ => self.for_window(),
        }
    }

    /// The page address of a window opened at this route (`safe`: with customisations off).
    pub(crate) fn page(&self, safe: bool) -> String {
        let mut q = format!("open={}", encode(&self.kind));
        if let Some(id) = &self.id { q.push_str(&format!("&id={}", encode(id))); }
        if let Some(path) = &self.path { q.push_str(&format!("&path={}", encode(path))); }
        if safe { q.push_str("&safe=1"); }
        format!("index.html?{q}")
    }
}

/// Percent-encode everything but the unreserved characters.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') { out.push(b as char); } else { out.push_str(&format!("%{b:02X}")); }
    }
    out
}

/// A window's place and size, in logical points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// How far each new window steps down and across from the window it was opened from.
const STEP: f64 = 28.0;
/// The main window's smallest size (tauri.conf.json): a second window is never smaller.
const MIN_W: f64 = 920.0;
const MIN_H: f64 = 600.0;

/// Where a new window goes: a step down and across from `from` (the window it was opened from),
/// one more step for each second window already open (starting again after eight), the same size,
/// and kept on `screen` (the screen `from` is on) when that is known.
pub(crate) fn place(from: Rect, screen: Option<Rect>, open: usize) -> Rect {
    let n = (open % 8) as f64 + 1.0;
    let mut r = Rect { x: from.x + STEP * n, y: from.y + STEP * n, w: from.w.max(MIN_W), h: from.h.max(MIN_H) };
    if let Some(s) = screen {
        r.w = r.w.min(s.w.max(MIN_W));
        r.h = r.h.min(s.h.max(MIN_H));
        if r.x + r.w > s.x + s.w { r.x = s.x + s.w - r.w; }
        if r.y + r.h > s.y + s.h { r.y = s.y + s.h - r.h; }
        r.x = r.x.max(s.x);
        r.y = r.y.max(s.y);
    }
    r
}

fn second_windows(app: &tauri::AppHandle) -> usize {
    app.webview_windows().keys().filter(|l| is_view_label(l)).count()
}

/// Where the window `from` is and its size, and the screen it is on, in logical points. A window
/// that fills its screen (maximised, full screen) or is hidden gives no place to step from.
fn whereabouts(from: &tauri::WebviewWindow) -> (Option<Rect>, Option<Rect>) {
    let scale = from.scale_factor().unwrap_or(1.0);
    let screen = from.current_monitor().ok().flatten().map(|m| {
        let s = m.scale_factor();
        Rect { x: m.position().x as f64 / s, y: m.position().y as f64 / s, w: m.size().width as f64 / s, h: m.size().height as f64 / s }
    });
    let filling = from.is_maximized().unwrap_or(false) || from.is_fullscreen().unwrap_or(false) || from.is_minimized().unwrap_or(false) || !from.is_visible().unwrap_or(false);
    let at = match (filling, from.outer_position(), from.inner_size()) {
        (false, Ok(p), Ok(sz)) => Some(Rect { x: p.x as f64 / scale, y: p.y as f64 / scale, w: sz.width as f64 / scale, h: sz.height as f64 / scale }),
        _ => None,
    };
    (at, screen)
}

/// Open a second app window at a note in Docs (`{kind: "docs", path}`) or a chat (`{kind: "chat",
/// id}`), stepped down and across from the window it was asked from. From an app window only.
#[tauri::command]
pub(crate) async fn open_window(app: tauri::AppHandle, window: tauri::Window, route: Route) -> Result<(), String> {
    app_window_only(&window)?;
    let route = route.for_window()?;
    if second_windows(&app) >= MOST {
        return Err(words(crate::recovery::arabic(), "too-many").into());
    }
    let mut config = app.config().app.windows.iter().find(|w| w.label == "main").cloned().ok_or("no window settings")?;
    config.label = format!("{PREFIX}{}", OPENED.fetch_add(1, Ordering::SeqCst) + 1);
    config.url = tauri::WebviewUrl::App(route.page(crate::recovery::safe_mode()).into());
    let (at, screen) = app.get_webview_window(window.label()).map(|w| whereabouts(&w)).unwrap_or((None, None));
    let from = at.unwrap_or_else(|| {
        let s = screen.unwrap_or(Rect { x: 0.0, y: 0.0, w: config.width, h: config.height });
        Rect { x: s.x + 40.0, y: s.y + 40.0, w: config.width, h: config.height }
    });
    let r = place(from, screen, second_windows(&app));
    let built = tauri::WebviewWindowBuilder::from_config(&app, &config)
        .map_err(|e| e.to_string())?
        .position(r.x, r.y)
        .inner_size(r.w, r.h)
        .focused(true)
        .build()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    crate::disable_webrtc(&built);
    let _ = built.set_focus();
    Ok(())
}

/// Bring the main window forward, at `route` when one is given: a second window sends the person
/// there for what only the main window does (Settings, Customise, a confirmation). The main window
/// alone hears where to go (`open-view`).
#[tauri::command]
pub(crate) fn focus_main(app: tauri::AppHandle, window: tauri::Window, route: Option<Route>) -> Result<(), String> {
    app_window_only(&window)?;
    let route = route.map(Route::for_main).transpose()?;
    crate::show_main(&app);
    if let (Some(route), Some(main)) = (route, crate::main_window(&app)) {
        let _ = app.emit_to(main.label(), "open-view", serde_json::json!({"kind": route.kind, "id": route.id, "path": route.path}));
    }
    Ok(())
}

fn asking(label: &str, on: bool) -> bool {
    let mut set = ASKING.lock().unwrap_or_else(|e| e.into_inner());
    let set = set.get_or_insert_with(HashSet::new);
    if on { set.insert(label.to_string()) } else { set.remove(label) }
}

/// A second window is being closed. With nothing unsaved in it, it simply closes; otherwise the
/// question is asked first (attached to that window), and only "Close" closes it.
pub(crate) fn on_window_event(window: &tauri::Window, ev: &tauri::WindowEvent) {
    match ev {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            let label = window.label().to_string();
            let items = crate::quit::unsaved_in(&label);
            if items.is_empty() {
                return;
            }
            api.prevent_close();
            if !asking(&label, true) {
                return;
            }
            let window = window.clone();
            std::thread::spawn(move || {
                let close = ask_close(&window, &items);
                asking(window.label(), false);
                if close {
                    crate::quit::forget(window.label());
                    let _ = window.destroy();
                }
            });
        }
        tauri::WindowEvent::Destroyed => crate::quit::forget(window.label()),
        _ => {}
    }
}

fn ask_close(window: &tauri::Window, items: &[crate::quit::Unsaved]) -> bool {
    let ar = crate::recovery::arabic();
    let lost = items.iter().any(|u| !u.kept);
    let (close, cancel) = (words(ar, if lost { "close-anyway" } else { "close" }), crate::recovery::tx(ar, "cancel"));
    let body = crate::quit::lines_for(ar, items).join("\n");
    let mut dialog = window
        .app_handle()
        .dialog()
        .message(body)
        .title(words(ar, "close-title"))
        .kind(if lost { MessageDialogKind::Warning } else { MessageDialogKind::Info })
        .buttons(MessageDialogButtons::OkCancelCustom(close.to_string(), cancel.to_string()));
    if let Some(w) = window.app_handle().get_webview_window(window.label()) {
        dialog = dialog.parent(&w);
    }
    match dialog.blocking_show_with_result() {
        MessageDialogResult::Custom(pressed) => pressed == close,
        MessageDialogResult::Ok | MessageDialogResult::Yes => true,
        _ => false,
    }
}

/// What this file says, in English and Arabic.
fn words(ar: bool, key: &str) -> &'static str {
    let (en, arabic) = match key {
        "close-title" => ("Close this window?", "إغلاق هذه النافذة؟"),
        "close" => ("Close", "إغلاق"),
        "close-anyway" => ("Close anyway", "أغلق على أي حال"),
        "too-many" => (
            "Thoughts already has many windows open. Close one of them first.",
            "في Thoughts نوافذ كثيرة مفتوحة الآن. أغلق إحداها أولاً.",
        ),
        _ => ("", ""),
    };
    if ar { arabic } else { en }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn route(v: Value) -> Route { serde_json::from_value(v).unwrap() }

    #[test]
    fn second_windows_have_labels_of_their_own() {
        for yes in ["view-1", "view-12", "view-999999"] {
            assert!(is_view_label(yes), "{yes}");
            assert!(!crate::is_main_label(yes), "{yes} is not the main window");
            assert!(is_app_label(yes));
        }
        // Never the main window's, Recovery's replacement windows', or the quick capture box's.
        for no in ["main", "main-2", "view", "view-", "view-x", "view-1a", "view--1", "view-1234567", "capture", "views-1", "View-1", "main-view-1"] {
            assert!(!is_view_label(no), "{no}");
        }
        assert!(is_app_label("main") && is_app_label("main-3"));
        assert!(!is_app_label("capture") && !is_app_label(""));
    }

    #[test]
    fn a_window_opens_only_at_a_note_or_a_chat() {
        let chat = route(json!({"kind": "chat", "id": "0b6f3a52-6c0e-4c55-9d4c-2f1e0e5b7a10"}));
        assert_eq!(chat.clone().for_window(), Ok(chat.clone()));
        assert_eq!(chat.page(false), "index.html?open=chat&id=0b6f3a52-6c0e-4c55-9d4c-2f1e0e5b7a10");
        let note = route(json!({"kind": "docs", "path": "apps/خطة العمل/README.md"}));
        assert_eq!(note.clone().for_window(), Ok(note.clone()));
        assert_eq!(note.page(true), "index.html?open=docs&path=apps%2F%D8%AE%D8%B7%D8%A9%20%D8%A7%D9%84%D8%B9%D9%85%D9%84%2FREADME.md&safe=1");
        assert_eq!(route(json!({"kind": "docs", "path": "a&b=c#d?.md"})).page(false), "index.html?open=docs&path=a%26b%3Dc%23d%3F.md");
        for bad in [
            json!({"kind": "chat", "id": "../x"}), json!({"kind": "chat", "id": ""}), json!({"kind": "chat"}),
            json!({"kind": "chat", "id": "abc", "path": "x.md"}), json!({"kind": "docs", "path": ""}), json!({"kind": "docs"}),
            json!({"kind": "docs", "path": "/etc/passwd"}), json!({"kind": "docs", "path": "a/../../x.md"}), json!({"kind": "docs", "path": "..\\x"}),
            json!({"kind": "docs", "path": "?search words"}), json!({"kind": "docs", "path": "a\nb.md"}), json!({"kind": "docs", "path": "x".repeat(1025)}),
            json!({"kind": "settings"}), json!({"kind": "plugin", "id": "x"}), json!({"kind": "home"}), json!({"kind": "javascript:alert(1)"}),
        ] {
            assert!(route(bad.clone()).for_window().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_second_window_may_send_the_main_window_only_to_plain_places() {
        for ok in [json!({"kind": "settings"}), json!({"kind": "settings", "id": "accessibility"}), json!({"kind": "customise", "id": "automate"}),
            json!({"kind": "chat", "id": "abc-1"}), json!({"kind": "docs", "path": "me/README.md"})] {
            assert!(route(ok.clone()).for_main().is_ok(), "{ok}");
        }
        for bad in [json!({"kind": "settings", "id": "a b"}), json!({"kind": "settings", "path": "x"}), json!({"kind": "plugin", "id": "x"}),
            json!({"kind": "rollback"}), json!({"kind": "customise", "id": "x".repeat(65)})] {
            assert!(route(bad.clone()).for_main().is_err(), "{bad}");
        }
    }

    #[test]
    fn new_windows_step_from_the_one_they_came_from_and_stay_on_its_screen() {
        let screen = Rect { x: 0.0, y: 0.0, w: 1728.0, h: 1117.0 };
        let main = Rect { x: 100.0, y: 60.0, w: 1320.0, h: 860.0 };
        assert_eq!(place(main, Some(screen), 0), Rect { x: 128.0, y: 88.0, w: 1320.0, h: 860.0 });
        assert_eq!(place(main, Some(screen), 2), Rect { x: 184.0, y: 144.0, w: 1320.0, h: 860.0 });
        // After eight it starts again rather than walking off the screen.
        assert_eq!(place(main, Some(screen), 8), place(main, Some(screen), 0));
        // A window at the screen's far corner: the new one is pulled back onto the screen.
        let corner = Rect { x: 400.0, y: 250.0, w: 1320.0, h: 860.0 };
        let r = place(corner, Some(screen), 3);
        assert!(r.x + r.w <= screen.w && r.y + r.h <= screen.h, "{r:?}");
        // A second screen to the left (negative coordinates) and a small window: never below the smallest size.
        let left = Rect { x: -1440.0, y: 0.0, w: 1440.0, h: 900.0 };
        let r = place(Rect { x: -1400.0, y: 10.0, w: 500.0, h: 300.0 }, Some(left), 0);
        assert_eq!((r.w, r.h), (MIN_W, MIN_H));
        assert!(r.x >= left.x && r.x + r.w <= left.x + left.w && r.y >= 0.0, "{r:?}");
        // No screen known: just the step.
        assert_eq!(place(main, None, 1), Rect { x: 156.0, y: 116.0, w: 1320.0, h: 860.0 });
    }

    /// The grants of every window, from `capabilities/*.json`: {file: (windows, permissions)}.
    fn grants(file: &str) -> (Vec<String>, Vec<String>) {
        let v: Value = serde_json::from_str(file).unwrap();
        let list = |k: &str| v[k].as_array().unwrap().iter().map(|p| p.as_str().map(str::to_string).unwrap_or_else(|| p["identifier"].as_str().unwrap().to_string())).collect();
        (list("windows"), list("permissions"))
    }

    #[test]
    fn the_second_window_gets_only_what_it_needs() {
        let (windows, mut perms) = grants(include_str!("../capabilities/window.json"));
        assert_eq!(windows, ["view-*"]);
        perms.sort();
        let mut want = vec![
            // Talking to the background service, as every app window does.
            "allow-daemon-info",
            // Opening more windows, and going back to the main one for what only it does.
            "allow-open-window", "allow-focus-main",
            // Unsaved work, so closing this window and quitting the app ask first.
            "allow-set-unsaved",
            // Opening and showing library files and chat attachments; saving an exported document.
            "allow-open-library-file", "allow-reveal-library-file", "allow-open-chat-attachment", "allow-reveal-chat-attachment", "allow-save-file",
            // Saving the note shown as a PDF (to a file the person picks), and opening that PDF.
            "allow-save-pdf", "allow-open-saved-pdf",
            // The window itself: zoom, print, its title, dragging it by the top, the developer tools of a development build.
            "core:webview:allow-set-webview-zoom", "core:webview:allow-print", "core:webview:allow-internal-toggle-devtools",
            "core:window:allow-start-dragging", "core:window:allow-internal-toggle-maximize", "core:window:allow-set-title",
            // Choosing a folder or file (a project's folder, notes to import) and opening web links in the browser.
            "dialog:allow-open", "opener:default",
        ];
        want.sort();
        assert_eq!(perms, want);
        // The main window's file does not reach second windows, and no other file names them.
        let (main, _) = grants(include_str!("../capabilities/default.json"));
        assert!(main.iter().all(|w| w == "main" || w.starts_with("main-")), "{main:?}");
        let (capture, _) = grants(include_str!("../capabilities/capture.json"));
        assert_eq!(capture, ["capture"]);
    }
}
