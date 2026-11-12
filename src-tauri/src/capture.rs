//! Quick capture from anywhere (FEAT-12).
//!
//! A small box that opens over whatever app the user is in — from a keyboard shortcut that works
//! system-wide, from the tray menu, from the File menu, or from a `thoughts://capture` link — and
//! writes a todo to the general list or a note to the inbox straight away (`POST /api/capture`, no
//! assistant run, nothing to wrap). The main window does not have to be open: the box is a window of
//! its own (`capture.html`), kept hidden between uses so it opens at once and keeps what was typed.
//!
//! The shortcut is the user's choice (Settings › General), on by default. The default combination
//! is one that neither the system nor common apps use, and that never types a character on any
//! keyboard layout: ⌃⌥⌘Space on a Mac (⌃Space and ⌃⌥Space switch the input language there, which
//! Arabic users rely on), Ctrl+Alt+Space on Windows and Linux. A combination the user picks must
//! hold two of Ctrl, Alt/Option and Cmd/Win (or be F13–F24), so it can never take a key that other
//! apps use for typing or for their own everyday shortcuts, and a short list the system itself uses
//! is refused.

use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// The capture window's label (its capability is `capabilities/capture.json`).
pub(crate) const LABEL: &str = "capture";
/// Size of the box, in points at 100% zoom.
const WIDTH: f64 = 580.0;
const HEIGHT: f64 = 290.0;

/// Text a link wants in the box, and which kind it is (`"todo"` / `"note"`). Taken by the page.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Prefill {
    pub text: String,
    pub kind: Option<&'static str>,
}

static PREFILL: Mutex<Option<Prefill>> = Mutex::new(None);
/// The page in the capture window has started and asked for its first text.
static READY: AtomicBool = AtomicBool::new(false);
/// A capture window is being built right now (a second press must not build another).
static BUILDING: AtomicBool = AtomicBool::new(false);
/// When the box was last brought up: a blur reported while it is still coming up is not the user
/// clicking away.
static SHOWN_AT: Mutex<Option<Instant>> = Mutex::new(None);
/// macOS: the app that was in front when the box opened, so closing the box hands it back.
static BEFORE: Mutex<Option<i32>> = Mutex::new(None);
/// The last time the box was asked for: the shortcut and a menu item showing the same keys can
/// both fire for one press.
static LAST_ASK: Mutex<Option<Instant>> = Mutex::new(None);

// ---------- the box ----------

/// Open the box (built the first time), on the screen the pointer is on, with `prefill` in it.
pub(crate) fn open(app: &AppHandle, prefill: Option<Prefill>) {
    if let Some(p) = prefill {
        *PREFILL.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    }
    match app.get_webview_window(LABEL) {
        Some(w) => {
            if !w.is_visible().unwrap_or(false) {
                remember_front_app();
                place(app, &w);
            }
            bring_up(&w);
            // The page takes any new text and puts the cursor in the box.
            let _ = app.emit_to(LABEL, "capture-open", ());
        }
        None => {
            if BUILDING.swap(true, Ordering::SeqCst) {
                return;
            }
            remember_front_app();
            let app = app.clone();
            // Built off the event loop: building waits for it.
            std::thread::spawn(move || {
                READY.store(false, Ordering::SeqCst);
                let built = build(&app);
                BUILDING.store(false, Ordering::SeqCst);
                let Ok(w) = built else { return };
                place(&app, &w);
                // The page shows the box once it is drawn (`capture_ready`); a page that never says
                // so must not leave the shortcut looking dead.
                std::thread::sleep(Duration::from_secs(2));
                if !READY.load(Ordering::SeqCst) && !w.is_visible().unwrap_or(true) {
                    bring_up(&w);
                }
            });
        }
    }
}

/// The shortcut: open the box, or put it away when it is already in front.
pub(crate) fn toggle(app: &AppHandle) {
    if !first_ask() {
        return;
    }
    if let Some(w) = app.get_webview_window(LABEL) {
        if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
            hide(app, true);
            return;
        }
    }
    open(app, None);
}

/// "Quick capture…" in the tray and File menus.
pub(crate) fn from_menu(app: &AppHandle) {
    if first_ask() {
        open(app, None);
    }
}

/// False for a second request within a moment of the first (one key press seen twice).
fn first_ask() -> bool {
    let mut last = LAST_ASK.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_some_and(|t| t.elapsed() < Duration::from_millis(350)) {
        return false;
    }
    *last = Some(Instant::now());
    true
}

fn build(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let settings = thoughtsd::settings::Settings::load();
    let ar = settings.language.starts_with("ar");
    // Larger text means a larger box, never one that cuts the text off.
    let zoom = (settings.accessibility.zoom as f64 / 100.0).clamp(1.0, 2.0);
    let builder = tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("capture.html".into()))
        .title(if ar { "تدوين سريع — Thoughts" } else { "Quick capture — Thoughts" })
        .inner_size(WIDTH * zoom, HEIGHT * zoom)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .visible(false)
        .center()
        .focused(true)
        // Text dragged in from another app lands in the box instead of being taken as a file drop.
        .disable_drag_drop_handler();
    #[cfg(target_os = "macos")]
    let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    builder.build()
}

/// Show and focus the box.
fn bring_up(w: &tauri::WebviewWindow) {
    *SHOWN_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

/// Centre the box near the top of the screen the pointer is on (that is where the user is looking).
fn place(app: &AppHandle, w: &tauri::WebviewWindow) {
    let screen = app.cursor_position().ok().and_then(|c| app.monitor_from_point(c.x, c.y).ok().flatten()).or_else(|| app.primary_monitor().ok().flatten());
    let (Some(m), Ok(size)) = (screen, w.outer_size()) else {
        let _ = w.center();
        return;
    };
    let area = m.work_area();
    let x = area.position.x + (area.size.width.saturating_sub(size.width) / 2) as i32;
    let y = area.position.y + (area.size.height.saturating_sub(size.height) / 4) as i32;
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
}

/// Put the box away. `give_back`: the user is done (Esc, saved, the shortcut again), so the app
/// they came from gets the keyboard back. Not after a click elsewhere: they already moved on.
pub(crate) fn hide(app: &AppHandle, give_back: bool) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
    let before = BEFORE.lock().unwrap_or_else(|e| e.into_inner()).take();
    #[cfg(target_os = "macos")]
    if let Some(pid) = before.filter(|_| give_back) {
        let _ = app.run_on_main_thread(move || {
            use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
            if let Some(other) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
                #[allow(deprecated)]
                other.activateWithOptions(NSApplicationActivationOptions::empty());
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (before, give_back);
}

/// macOS: note which app is in front, unless it is this one.
fn remember_front_app() {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWorkspace;
        let front = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier());
        *BEFORE.lock().unwrap_or_else(|e| e.into_inner()) = front.filter(|pid| *pid != std::process::id() as i32);
    }
}

/// The capture window's own events: closing it only puts it away, and clicking into another app
/// puts it away too (what was typed stays for next time).
pub(crate) fn on_window_event(window: &tauri::Window, ev: &tauri::WindowEvent) {
    match ev {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide(window.app_handle(), true);
        }
        tauri::WindowEvent::Focused(false) => {
            let coming_up = SHOWN_AT.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < Duration::from_millis(500));
            if !coming_up && window.is_visible().unwrap_or(false) {
                hide(window.app_handle(), false);
            }
        }
        _ => {}
    }
}

fn from_capture(window: &tauri::Window) -> bool {
    window.label() == LABEL
}

/// The page is drawn: show the box. Returns the text a link wants in it, if any.
#[tauri::command]
pub(crate) fn capture_ready(window: tauri::Window) -> Option<Prefill> {
    if !from_capture(&window) {
        return None;
    }
    READY.store(true, Ordering::SeqCst);
    if let Some(w) = window.app_handle().get_webview_window(LABEL) {
        bring_up(&w);
    }
    PREFILL.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Text waiting for the box (after `capture-open`), once.
#[tauri::command]
pub(crate) fn capture_take(window: tauri::Window) -> Option<Prefill> {
    if !from_capture(&window) {
        return None;
    }
    PREFILL.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Bring the box back, e.g. when a save the user did not wait for went wrong.
#[tauri::command]
pub(crate) fn capture_show(window: tauri::Window) {
    if from_capture(&window) {
        if let Some(w) = window.app_handle().get_webview_window(LABEL) {
            bring_up(&w);
        }
    }
}

/// "Try it now" in Settings: open the box from the window.
#[tauri::command]
pub(crate) fn open_capture(window: tauri::Window) {
    if crate::is_main_label(window.label()) {
        from_menu(window.app_handle());
    }
}

/// Put the box away (Esc, or after saving).
#[tauri::command]
pub(crate) fn capture_hide(window: tauri::Window, give_back: Option<bool>) {
    if from_capture(&window) {
        hide(window.app_handle(), give_back.unwrap_or(true));
    }
}

/// "Open in Thoughts" after a save: the todo list, or the note just written to the inbox.
#[tauri::command]
pub(crate) fn capture_open_main(window: tauri::Window, view: String, path: Option<String>) -> Result<(), String> {
    if !from_capture(&window) {
        return Err("not allowed".into());
    }
    let app = window.app_handle();
    match (view.as_str(), path) {
        ("todos", _) => crate::links::route(app, json!({"kind": "view", "view": "todos"})),
        ("note", Some(p)) if p.starts_with("inbox/") && !p.contains("..") => {
            let settings = thoughtsd::settings::Settings::load();
            let root = settings.library().ok_or("no library")?;
            let (path, _) = crate::links::locate(&root, &p, &[]).map_err(|_| "not found".to_string())?;
            crate::links::route(app, json!({"kind": "note", "path": path}));
        }
        _ => crate::show_main(app),
    }
    hide(app, false);
    Ok(())
}

// ---------- the shortcut ----------

/// The shortcut as it stands: what the user chose, and whether the system accepted it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Keys {
    on: bool,
    keys: String,
    /// Registered with the system right now.
    active: bool,
    /// Why it is not, although it is on: `TAKEN` (another app has it) or `UNSUPPORTED` (this
    /// computer gives no app system-wide shortcuts, e.g. Linux on Wayland).
    problem: Option<&'static str>,
}

static KEYS: Mutex<Option<Keys>> = Mutex::new(None);
/// The global-shortcut service could be started on this computer.
static AVAILABLE: AtomicBool = AtomicBool::new(false);
/// Settings is listening for a new combination: the current one is let go meanwhile, so pressing it
/// reaches the page instead of opening the box.
static PAUSED: AtomicBool = AtomicBool::new(false);
static PAUSE_GEN: AtomicU64 = AtomicU64::new(0);

/// The app's own default combination on this computer.
pub(crate) fn default_keys() -> &'static str {
    if cfg!(target_os = "macos") { "Control+Alt+Super+Space" } else { "Control+Alt+Space" }
}

/// Why a combination cannot be the shortcut (the page has words for each code).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Refused {
    /// Not keys the app knows (or more than one main key).
    Format,
    /// Too few modifier keys: it would take a key other apps need.
    Modifiers,
    /// The system itself uses it.
    System,
    /// The system would not give it to Thoughts (another app has it).
    Taken,
    /// This computer has no system-wide shortcuts for apps.
    Unsupported,
}

impl Refused {
    fn code(self) -> &'static str {
        match self {
            Refused::Format => "FORMAT",
            Refused::Modifiers => "MODIFIERS",
            Refused::System => "SYSTEM",
            Refused::Taken => "TAKEN",
            Refused::Unsupported => "UNSUPPORTED",
        }
    }
}

/// Modifier names in the order they are written, with the spellings accepted for each.
const MODS: [(&str, &[&str]); 4] = [
    ("Control", &["control", "ctrl"]),
    ("Alt", &["alt", "option", "opt"]),
    ("Shift", &["shift"]),
    ("Super", &["super", "cmd", "command", "meta", "win", "windows"]),
];

/// Keys that can end a combination, as the page names them (`KeyboardEvent.code`).
const NAMED: &[&str] = &[
    "Space", "Enter", "Backquote", "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash", "Semicolon", "Quote", "Comma",
    "Period", "Slash", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown", "Insert", "Delete",
];

/// The key's own name (`KeyN`, `Digit5`, `F13`, `Space`…), whatever the case; `N` and `5` work too.
fn key_name(k: &str) -> Option<String> {
    let low = k.to_ascii_lowercase();
    let single = |s: &str| -> Option<char> { let mut c = s.chars(); let first = c.next()?; c.next().is_none().then_some(first) };
    let letter = |s: &str| single(s).filter(char::is_ascii_lowercase).map(|c| format!("Key{}", c.to_ascii_uppercase()));
    let digit = |s: &str, name: &str| single(s).filter(char::is_ascii_digit).map(|c| format!("{name}{c}"));
    if let Some(k) = letter(&low).or_else(|| digit(&low, "Digit")) {
        return Some(k);
    }
    if let Some(rest) = low.strip_prefix("key") {
        return letter(rest);
    }
    if let Some(rest) = low.strip_prefix("digit") {
        return digit(rest, "Digit");
    }
    if let Some(rest) = low.strip_prefix("numpad") {
        return digit(rest, "Numpad");
    }
    if let Some(n) = low.strip_prefix('f').filter(|n| !n.starts_with('0')).and_then(|n| n.parse::<u8>().ok()) {
        return (1..=24).contains(&n).then(|| format!("F{n}"));
    }
    NAMED.iter().find(|n| n.to_ascii_lowercase() == low).map(|n| n.to_string())
}

/// Combinations the system itself uses (each written the way `check_keys` writes it).
fn reserved() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &[
            "Control+Alt+Space",          // next input language
            "Alt+Super+Space",            // Finder search
            "Control+Super+Space",        // emoji and symbols
            "Control+Super+KeyQ",         // lock screen
            "Control+Super+KeyF",         // full screen
            "Control+Super+KeyD",         // look up
            "Alt+Super+KeyD",             // hide the Dock
            "Alt+Super+KeyH",             // hide others
            "Alt+Super+KeyM",             // minimise all
            "Alt+Super+KeyW",             // close all
            "Control+Alt+Super+Digit8",   // invert colours
            "Control+Alt+Super+Comma",    // contrast
            "Control+Alt+Super+Period",   // contrast
        ]
    } else if cfg!(windows) {
        &[
            "Control+Alt+Delete",
            "Alt+Super+Space",            // Command Palette
            "Control+Super+KeyD",         // new desktop
            "Control+Super+F4",           // close desktop
            "Control+Super+ArrowLeft",    // previous desktop
            "Control+Super+ArrowRight",   // next desktop
            "Control+Super+Enter",        // Narrator
            "Control+Shift+Super+KeyB",   // graphics restart
            "Alt+Super+KeyB", "Alt+Super+KeyG", "Alt+Super+KeyM", "Alt+Super+KeyR", "Alt+Super+KeyT", // Game Bar
        ]
    } else {
        &[
            "Control+Alt+Delete",
            "Control+Alt+KeyT",           // terminal
            "Control+Alt+KeyL",           // lock screen
            "Control+Alt+KeyD",           // show desktop
            "Control+Alt+ArrowLeft", "Control+Alt+ArrowRight", "Control+Alt+ArrowUp", "Control+Alt+ArrowDown", // workspaces
            "Control+Alt+F1", "Control+Alt+F2", "Control+Alt+F3", "Control+Alt+F4", "Control+Alt+F5", "Control+Alt+F6",
            "Control+Alt+F7", "Control+Alt+F8", "Control+Alt+F9", "Control+Alt+F10", "Control+Alt+F11", "Control+Alt+F12", // switch screens
        ]
    }
}

/// The combination written the one way the app stores it (`Control+Alt+Shift+Super+<key>`), or why
/// it cannot be the shortcut.
pub(crate) fn check_keys(raw: &str) -> Result<String, Refused> {
    let parts: Vec<&str> = raw.split('+').map(str::trim).collect();
    if raw.len() > 80 || parts.len() > 5 || parts.iter().any(|p| p.is_empty()) {
        return Err(Refused::Format);
    }
    let (key, mods) = parts.split_last().ok_or(Refused::Format)?;
    let mut have = [false; 4];
    for m in mods {
        let low = m.to_ascii_lowercase();
        let i = MODS.iter().position(|(_, names)| names.contains(&low.as_str())).ok_or(Refused::Format)?;
        if have[i] {
            return Err(Refused::Format);
        }
        have[i] = true;
    }
    let key = key_name(key).ok_or(Refused::Format)?;
    let canonical = MODS.iter().zip(have).filter(|(_, on)| *on).map(|((name, _), _)| *name).chain([key.as_str()]).collect::<Vec<_>>().join("+");
    // Two of Control, Alt and Super (Shift does not count: Shift+Cmd+Z is Redo everywhere), unless
    // it is one of the extra function keys F13–F24, which no app types or uses for itself.
    let strong = [have[0], have[1], have[3]].iter().filter(|h| **h).count();
    let extra_f = key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| n >= 13);
    if strong < 2 && !extra_f {
        return Err(Refused::Modifiers);
    }
    if reserved().contains(&canonical.as_str()) {
        return Err(Refused::System);
    }
    canonical.parse::<Shortcut>().map_err(|_| Refused::Format)?;
    Ok(canonical)
}

fn keys_now() -> Keys {
    KEYS.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or(Keys { on: false, keys: default_keys().into(), active: false, problem: None })
}

fn set_keys(k: Keys) {
    *KEYS.lock().unwrap_or_else(|e| e.into_inner()) = Some(k);
}

fn register(app: &AppHandle, keys: &str) -> Result<(), Refused> {
    if !AVAILABLE.load(Ordering::SeqCst) {
        return Err(Refused::Unsupported);
    }
    let s = keys.parse::<Shortcut>().map_err(|_| Refused::Format)?;
    app.global_shortcut().register(s).map_err(|_| Refused::Taken)
}

fn unregister(app: &AppHandle, keys: &str) {
    if let Ok(s) = keys.parse::<Shortcut>() {
        let _ = app.global_shortcut().unregister(s);
    }
}

/// The keys for the File and tray menus, when the shortcut is working.
pub(crate) fn accelerator() -> Option<String> {
    let k = keys_now();
    k.active.then_some(k.keys)
}

/// Start the shortcut service and put the user's shortcut in place (at start-up, on the main thread).
pub(crate) fn init(app: &AppHandle) {
    let service = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            let ours = keys_now().keys.parse::<Shortcut>().is_ok_and(|k| k.id() == shortcut.id());
            if event.state == ShortcutState::Pressed && ours {
                toggle(app);
            }
        })
        .build();
    // Some systems have no system-wide shortcuts for apps (Linux on Wayland): the app still starts.
    AVAILABLE.store(app.plugin(service).is_ok(), Ordering::SeqCst);
    let saved = thoughtsd::settings::Settings::load().capture;
    let keys = check_keys(if saved.shortcut.is_empty() { default_keys() } else { &saved.shortcut }).unwrap_or_else(|_| default_keys().into());
    let mut k = Keys { on: saved.shortcut_on, keys, active: false, problem: None };
    if k.on {
        match register(app, &k.keys) {
            Ok(()) => k.active = true,
            Err(why) => k.problem = Some(why.code()),
        }
    }
    set_keys(k);
}

fn state() -> Value {
    let k = keys_now();
    json!({
        "on": k.on, "keys": k.keys, "defaultKeys": default_keys(), "active": k.active, "problem": k.problem,
        "supported": AVAILABLE.load(Ordering::SeqCst), "platform": crate::PLATFORM,
    })
}

/// `capture_shortcut` → `{on, keys, defaultKeys, active, problem, supported, platform}`.
#[tauri::command]
pub(crate) fn capture_shortcut() -> Value {
    state()
}

/// `set_capture_shortcut {on, keys}`: switch the shortcut on or off, or change it. The new one is
/// only kept when the system accepted it; otherwise the old one stays in force and the answer is
/// one of `FORMAT`, `MODIFIERS`, `SYSTEM`, `TAKEN`, `UNSUPPORTED`. The page then saves the choice
/// in the settings, which is where the next start reads it.
#[tauri::command]
pub(crate) fn set_capture_shortcut(app: AppHandle, on: bool, keys: String) -> Result<Value, String> {
    // Whatever comes of this, Settings has stopped listening for keys: the one in force is back.
    resume(&app);
    let keys = check_keys(&keys).map_err(|r| r.code().to_string())?;
    let was = keys_now();
    if was.on == on && was.keys == keys && (was.active || !on) {
        return Ok(state());
    }
    if was.active {
        unregister(&app, &was.keys);
    }
    let mut next = Keys { on, keys, active: false, problem: None };
    if on {
        if let Err(why) = register(&app, &next.keys) {
            // Put the old one back exactly as it was.
            if was.active && register(&app, &was.keys).is_err() {
                set_keys(Keys { active: false, problem: Some(Refused::Taken.code()), ..was });
            }
            crate::refresh_menus(&app);
            return Err(why.code().to_string());
        }
        next.active = true;
    }
    set_keys(next);
    crate::refresh_menus(&app);
    Ok(state())
}

/// `pause_capture_shortcut {paused}`: Settings is listening for a new combination (true) or
/// stopped (false). A pause ends by itself after a minute, so a page that went away never leaves
/// the shortcut off.
#[tauri::command]
pub(crate) fn pause_capture_shortcut(app: AppHandle, paused: bool) {
    if !paused {
        resume(&app);
        return;
    }
    let gen = PAUSE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    PAUSED.store(true, Ordering::SeqCst);
    let k = keys_now();
    if k.active {
        unregister(&app, &k.keys);
        set_keys(Keys { active: false, ..k });
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        if PAUSED.load(Ordering::SeqCst) && PAUSE_GEN.load(Ordering::SeqCst) == gen {
            let a = app.clone();
            let _ = app.run_on_main_thread(move || resume(&a));
        }
    });
}

fn resume(app: &AppHandle) {
    PAUSED.store(false, Ordering::SeqCst);
    let k = keys_now();
    if k.on && !k.active {
        let mut k = k;
        match register(app, &k.keys) {
            Ok(()) => { k.active = true; k.problem = None; }
            Err(why) => k.problem = Some(why.code()),
        }
        set_keys(k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combinations_are_written_one_way() {
        assert_eq!(check_keys("Control+Alt+Super+Space"), Ok("Control+Alt+Super+Space".into()));
        // Any order and spelling of the modifiers, the key by its code or its letter.
        assert_eq!(check_keys("cmd+ctrl+option+space"), Ok("Control+Alt+Super+Space".into()));
        assert_eq!(check_keys(" Shift + Control + Alt + KeyN "), Ok("Control+Alt+Shift+KeyN".into()));
        assert_eq!(check_keys("Control+Alt+n"), Ok("Control+Alt+KeyN".into()));
        assert_eq!(check_keys("Control+Alt+7"), Ok("Control+Alt+Digit7".into()));
        assert_eq!(check_keys("ctrl+win+Slash"), Ok("Control+Super+Slash".into()));
        assert_eq!(check_keys("F13"), Ok("F13".into()));
        assert_eq!(check_keys("Shift+F19"), Ok("Shift+F19".into()));
        // Not keys at all, two main keys, a modifier twice, keys the box cannot use.
        for bad in ["", "+", "Control+", "Control+Alt", "Control+Alt+KeyN+KeyM", "Control+Control+KeyN", "Hyper+Alt+KeyN", "Control+Alt+Escape",
            "Control+Alt+Tab", "Control+Alt+Backspace", "Control+Alt+F0", "Control+Alt+F25", "Control+Alt+F05", "Control+Alt+KeyNN", "Control+Alt+مسافة", "Control+Alt+Super+Shift+Space+X"] {
            assert_eq!(check_keys(bad), Err(Refused::Format), "{bad}");
        }
    }

    #[test]
    fn a_shortcut_never_takes_keys_other_apps_need() {
        // One modifier (or Shift with one) is every app's own shortcut: Cmd+S, Ctrl+C, Shift+Cmd+Z, Alt+F4.
        for weak in ["KeyN", "Space", "Shift+KeyN", "Control+KeyS", "Super+Space", "Shift+Super+KeyZ", "Alt+F4", "Control+F5", "Alt+Space", "Shift+Alt+KeyN", "F5"] {
            assert_eq!(check_keys(weak), Err(Refused::Modifiers), "{weak}");
        }
        // The defaults pass on the computer they are for.
        assert_eq!(check_keys(default_keys()).as_deref(), Ok(default_keys()));
        if cfg!(target_os = "macos") {
            // ⌃⌥Space switches the input language (Arabic ↔ English), ⌥⌘Space is Finder search.
            for system in ["Control+Alt+Space", "Alt+Super+Space", "Control+Super+Space", "Control+Super+KeyQ"] {
                assert_eq!(check_keys(system), Err(Refused::System), "{system}");
            }
        } else {
            assert_eq!(check_keys("Control+Alt+Delete"), Err(Refused::System));
        }
        for r in reserved() {
            assert_eq!(check_keys(r), Err(Refused::System), "{r} must be written as check_keys writes it");
        }
    }

    #[test]
    fn every_refusal_has_a_code_the_page_knows() {
        let codes: Vec<_> = [Refused::Format, Refused::Modifiers, Refused::System, Refused::Taken, Refused::Unsupported].iter().map(|r| r.code()).collect();
        assert_eq!(codes, ["FORMAT", "MODIFIERS", "SYSTEM", "TAKEN", "UNSUPPORTED"]);
    }
}
