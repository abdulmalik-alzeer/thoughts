//! Full screen, the standard way on each system, and never left by Esc.
//!
//! * macOS: View › Enter Full Screen (⌃⌘F), the green window button, and the system's own ⌃⌘F.
//!   The menu item is an ordinary item rather than AppKit's predefined one: AppKit retitles that one
//!   itself ("Enter Full Screen" / "Exit Full Screen") in the system's language, which would put
//!   English into the Arabic menus. This item says the same thing in the app's language and flips
//!   between the two whenever a window goes in or out of full screen (`sync`).
//! * Windows and Linux: Window › Full Screen (F11). UNTESTED there: written and built on macOS.
//!
//! Why Esc used to leave full screen on macOS: a key the page does not handle (it did not call
//! `preventDefault`) goes back to AppKit, which turns Esc into the `cancelOperation:` command and
//! sends it up the responder chain (WKWebView → its parent view → the window). The window's own
//! `-[NSWindow doCommandBySelector:]` answers `cancelOperation:` in full screen by leaving it, and
//! so does `-[NSWindow cancelOperation:]` when called directly. Neither the HTML Fullscreen API nor
//! the app's own Esc handling is involved (the app never asks for full screen from the page).
//! `install_esc_guard` makes the window ignore that one command while it is in full screen; the
//! page still sees every Esc first, so closing dialogs, palettes and menus works as before.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use tauri::menu::MenuItem;
use tauri::{AppHandle, Manager, Wry};

/// The menu item's id (handled in `on_menu_event`).
pub(crate) const ID: &str = "fullscreen";

/// The item's usual keys: the system's own ⌃⌘F on a Mac, F11 elsewhere (`thoughtsd::keys::native_defaults`).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn accelerator() -> &'static str {
    if cfg!(target_os = "macos") { "Ctrl+Super+F" } else { "F11" }
}

/// The words on the item: what choosing it would do now.
pub(crate) fn label(ar: bool, full: bool) -> &'static str {
    match (full, ar) {
        (false, false) => "Enter Full Screen",
        (false, true) => "الدخول إلى ملء الشاشة",
        (true, false) => "Exit Full Screen",
        (true, true) => "الخروج من ملء الشاشة",
    }
}

/// The item in the menu bar now, and its language (the menus are rebuilt when the language changes).
static ITEM: Mutex<Option<(MenuItem<Wry>, bool)>> = Mutex::new(None);
/// What the item says now: 0 enter, 1 exit, 2 not known yet.
static SHOWN: AtomicU8 = AtomicU8::new(2);

/// The app window in front (a second one, or the main one). Never the quick capture box.
fn front(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.webview_windows()
        .into_values()
        .find(|w| crate::windows::is_app_label(w.label()) && w.is_focused().unwrap_or(false))
        .or_else(|| crate::main_window(app))
}

fn front_is_full(app: &AppHandle) -> bool {
    front(app).is_some_and(|w| w.is_fullscreen().unwrap_or(false))
}

/// Build the item for a new menu bar.
pub(crate) fn item(h: &AppHandle, ar: bool) -> tauri::Result<MenuItem<Wry>> {
    let full = front_is_full(h);
    // The user's key (Settings › Keyboard, `window.fullscreen`), or the system's own by default.
    let it = crate::keyed_item(h, ID, label(ar, full), crate::menu_keys().fullscreen.as_deref())?;
    *ITEM.lock().unwrap_or_else(|e| e.into_inner()) = Some((it.clone(), ar));
    SHOWN.store(full as u8, Ordering::SeqCst);
    Ok(it)
}

/// The item was chosen: the window in front goes in or out of full screen.
pub(crate) fn toggle(app: &AppHandle) {
    if let Some(w) = front(app) {
        let full = w.is_fullscreen().unwrap_or(false);
        let _ = w.set_fullscreen(!full);
    }
}

/// Whether the window was already full screen when a deck started (so leaving the deck keeps it so).
static BEFORE_DECK: AtomicU8 = AtomicU8::new(2);

/// Present a slide deck full screen, and back (`ui/src/lib/slides/Present.svelte`). The app window
/// only; a window that was full screen before the deck stays full screen after it.
#[tauri::command]
pub(crate) fn present_fullscreen(window: tauri::Window, on: bool) -> Result<(), String> {
    crate::main_only(&window)?;
    let app = window.app_handle();
    let Some(w) = app.get_webview_window(window.label()) else { return Err("no window".into()) };
    let full = w.is_fullscreen().unwrap_or(false);
    let target = if on {
        BEFORE_DECK.store(full as u8, Ordering::SeqCst);
        true
    } else {
        BEFORE_DECK.swap(2, Ordering::SeqCst) == 1
    };
    if full != target { w.set_fullscreen(target).map_err(|e| e.to_string())?; }
    sync(app);
    Ok(())
}

/// A window was resized or came to the front: make the item say what it would do now. (A window
/// reports a resize once it has finished going in or out of full screen, however that happened.)
pub(crate) fn sync(app: &AppHandle) {
    let full = front_is_full(app);
    if SHOWN.swap(full as u8, Ordering::SeqCst) == full as u8 {
        return;
    }
    if let Some((it, ar)) = ITEM.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = it.set_text(label(*ar, full));
    }
}

/// The window events that can change what the item should say.
pub(crate) fn on_window_event(window: &tauri::Window, ev: &tauri::WindowEvent) {
    if crate::windows::is_app_label(window.label()) && matches!(ev, tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Focused(true)) {
        sync(window.app_handle());
    }
}

// ---------- putting a full-screen window away ----------
//
// macOS gives a full-screen window a Space of its own. Hiding the window (`orderOut:`, which is what
// closing the main window does while "keep running in the background" is on) leaves that Space
// behind with nothing in it: a black screen the user has to swipe away from. So a full-screen
// window first leaves full screen, and is hidden only once AppKit says the way out has finished
// (`NSWindowDidExitFullScreenNotification`; the way out is animated, and hiding during it gives the
// same black screen). It then comes back from the Dock or the tray as an ordinary window.

/// What putting a window away has to do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PutAway {
    /// Hide it now.
    Hide,
    /// Leave full screen, and hide it when that has finished.
    LeaveThenHide,
    /// It is already on its way out of full screen to be hidden: nothing more to do.
    Wait,
}

/// `full`: the window is in full screen; `leaving`: it is already leaving it to be hidden.
/// Only macOS keeps a Space for a full-screen window; elsewhere it is simply hidden.
pub(crate) fn put_away_plan(mac: bool, full: bool, leaving: bool) -> PutAway {
    if !mac { PutAway::Hide } else if leaving { PutAway::Wait } else if full { PutAway::LeaveThenHide } else { PutAway::Hide }
}

/// The window that is leaving full screen to be hidden: its label, its NSWindow (as a number, to
/// tell its notification from another window's) and a count that tells one such wait from the next.
static LEAVING: Mutex<Option<(String, usize, u64)>> = Mutex::new(None);
static LEAVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// If AppKit never says the window left full screen, it is hidden after this long anyway, so
/// closing the window can never do nothing at all.
const LEAVE_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

fn leaving(label: &str) -> bool {
    LEAVING.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|(l, _, _)| l == label)
}

/// Hide `w` (the main window going to the tray), leaving full screen first when it has to.
pub(crate) fn put_away(app: &AppHandle, w: &tauri::WebviewWindow) {
    match put_away_plan(cfg!(target_os = "macos"), w.is_fullscreen().unwrap_or(false), leaving(w.label())) {
        PutAway::Hide => { let _ = w.hide(); }
        PutAway::Wait => {}
        PutAway::LeaveThenHide => {
            #[cfg(target_os = "macos")]
            {
                let ns = w.ns_window().map(|p| p as usize).unwrap_or(0);
                let n = LEAVES.fetch_add(1, Ordering::SeqCst) + 1;
                *LEAVING.lock().unwrap_or_else(|e| e.into_inner()) = Some((w.label().to_string(), ns, n));
                let a = app.clone();
                let _ = app.run_on_main_thread(move || mac::watch_exits(a));
                let _ = w.set_fullscreen(false);
                let a = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(LEAVE_LIMIT);
                    left_full_screen(&a, None, Some(n));
                });
            }
            #[cfg(not(target_os = "macos"))]
            { let _ = (app, w.hide()); }
        }
    }
}

/// A window finished leaving full screen (`ns`: which one), or the wait `n` ran out: the window
/// that was waiting for exactly that is hidden now. Anything else changes nothing.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn left_full_screen(app: &AppHandle, ns: Option<usize>, n: Option<u64>) {
    let label = {
        let mut waiting = LEAVING.lock().unwrap_or_else(|e| e.into_inner());
        match waiting.as_ref() {
            Some((_, w, k)) if is_the_wait(*w, *k, ns, n) => waiting.take().map(|(l, _, _)| l),
            _ => None,
        }
    };
    if let Some(w) = label.and_then(|l| app.get_webview_window(&l)) {
        let _ = w.hide();
    }
    sync(app);
}

/// Is this the news the waiting window (`window`, wait number `wait`) was waiting for? Its own
/// window left full screen, or its own wait ran out; never another window's or an earlier wait's.
fn is_the_wait(window: usize, wait: u64, ns: Option<usize>, n: Option<u64>) -> bool {
    ns == Some(window) || n == Some(wait)
}

/// The window was asked for again (Dock, tray, a menu item) before it was hidden: it stays.
pub(crate) fn keep(label: &str) {
    let mut waiting = LEAVING.lock().unwrap_or_else(|e| e.into_inner());
    if waiting.as_ref().is_some_and(|(l, _, _)| l == label) { *waiting = None; }
}

/// Should the window ignore this command? Only `cancelOperation:` (Esc, ⌘.) while in full screen.
pub(crate) fn swallow(is_cancel: bool, in_full_screen: bool) -> bool {
    is_cancel && in_full_screen
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{msg_send, sel};
    use std::sync::OnceLock;

    /// `NSWindowStyleMaskFullScreen`.
    const FULL_SCREEN: usize = 1 << 14;

    /// NSWindow's own implementations, called for everything the guard lets through.
    pub(super) static DO_COMMAND: OnceLock<Imp> = OnceLock::new();
    pub(super) static CANCEL: OnceLock<Imp> = OnceLock::new();

    unsafe fn in_full_screen(this: *mut AnyObject) -> bool {
        let mask: usize = unsafe { msg_send![this, styleMask] };
        mask & FULL_SCREEN != 0
    }

    /// `-[NSWindow doCommandBySelector:]`, where Esc arrives from the web view.
    pub(super) unsafe extern "C-unwind" fn do_command(this: *mut AnyObject, cmd: Sel, command: Sel) {
        if super::swallow(command == sel!(cancelOperation:), unsafe { in_full_screen(this) }) {
            return;
        }
        if let Some(orig) = DO_COMMAND.get() {
            let f: unsafe extern "C-unwind" fn(*mut AnyObject, Sel, Sel) = unsafe { std::mem::transmute(*orig) };
            unsafe { f(this, cmd, command) }
        }
    }

    /// `-[NSWindow cancelOperation:]`, for anything that sends it to the window directly.
    pub(super) unsafe extern "C-unwind" fn cancel(this: *mut AnyObject, cmd: Sel, sender: *mut AnyObject) {
        if super::swallow(true, unsafe { in_full_screen(this) }) {
            return;
        }
        if let Some(orig) = CANCEL.get() {
            let f: unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) = unsafe { std::mem::transmute(*orig) };
            unsafe { f(this, cmd, sender) }
        }
    }

    /// Put the guard in place, once (a second call changes nothing).
    pub(super) fn install() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let Some(window) = AnyClass::get(c"NSWindow") else { return };
            // SAFETY: both replacements take exactly the arguments of the methods they replace
            // and call the saved original for everything they do not handle themselves.
            unsafe {
                if let Some(m) = window.instance_method(sel!(doCommandBySelector:)) {
                    let mine: Imp = std::mem::transmute(do_command as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, Sel));
                    let _ = DO_COMMAND.set(m.set_implementation(mine));
                }
                if let Some(m) = window.instance_method(sel!(cancelOperation:)) {
                    let mine: Imp = std::mem::transmute(cancel as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject));
                    let _ = CANCEL.set(m.set_implementation(mine));
                }
            }
        });
    }

    /// Hear every window of the app finish leaving full screen, from now on (a second call changes
    /// nothing). On the main thread. The observer is never removed: it lives as long as the app.
    pub(super) fn watch_exits(app: tauri::AppHandle) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| observe_exits(move |window| super::left_full_screen(&app, Some(window), None)));
    }

    /// Call `heard` with the NSWindow (as a number) of every window that has left full screen.
    pub(super) fn observe_exits(heard: impl Fn(usize) + 'static) {
        use objc2::class;
        use objc2_foundation::NSString;
        let name = NSString::from_str("NSWindowDidExitFullScreenNotification");
        let block = block2::RcBlock::new(move |note: *mut AnyObject| {
            // SAFETY: AppKit hands the block a live NSNotification; its object is the window.
            let window: *mut AnyObject = unsafe { msg_send![note, object] };
            heard(window as usize);
        });
        // SAFETY: plain Foundation calls; the centre copies the block and keeps it. No queue:
        // the block runs where the notification is posted, which AppKit does on the main thread.
        unsafe {
            let centre: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
            let nothing: *mut AnyObject = std::ptr::null_mut();
            let _token: *mut AnyObject = msg_send![centre, addObserverForName: &*name, object: nothing, queue: nothing, usingBlock: &*block];
        }
    }

    /// AppKit adds an "Enter Full Screen" item of its own to a menu called View, in the system's
    /// language, next to ours. This asks it not to, for this run only (the registration domain is
    /// never saved). Before the menu bar is put in place.
    pub(super) fn no_system_item() {
        use objc2::class;
        use objc2_foundation::NSString;
        let key = NSString::from_str("NSFullScreenMenuItemEverywhere");
        // SAFETY: plain Foundation calls with objects that live through the call.
        unsafe {
            let no: *mut AnyObject = msg_send![class!(NSNumber), numberWithBool: false];
            let dict: *mut AnyObject = msg_send![class!(NSDictionary), dictionaryWithObject: no, forKey: &*key];
            let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
            let _: () = msg_send![defaults, registerDefaults: dict];
        }
    }
}

/// Esc never leaves full screen (macOS; elsewhere the system does not do that in the first place),
/// and the View menu holds one full-screen item, ours.
pub(crate) fn install_esc_guard() {
    #[cfg(target_os = "macos")]
    {
        mac::install();
        mac::no_system_item();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_cancel_in_full_screen_is_ignored() {
        assert!(swallow(true, true));
        assert!(!swallow(true, false), "out of full screen Esc does what it always did");
        assert!(!swallow(false, true), "every other command reaches the window");
        assert!(!swallow(false, false));
    }

    #[test]
    fn a_full_screen_window_leaves_full_screen_before_it_is_hidden() {
        // macOS: hidden as it is, its Space would stay behind, black.
        assert_eq!(put_away_plan(true, true, false), PutAway::LeaveThenHide);
        assert_eq!(put_away_plan(true, false, false), PutAway::Hide);
        // Closed a second time on the way out (the window no longer counts as full screen then):
        // still not hidden before the way out has finished.
        assert_eq!(put_away_plan(true, false, true), PutAway::Wait);
        assert_eq!(put_away_plan(true, true, true), PutAway::Wait);
        // Elsewhere a full-screen window has no Space of its own.
        assert_eq!(put_away_plan(false, true, false), PutAway::Hide);
        assert_eq!(put_away_plan(false, true, true), PutAway::Hide);
    }

    #[test]
    fn only_its_own_window_or_its_own_wait_hides_the_waiting_window() {
        assert!(is_the_wait(0x10, 3, Some(0x10), None), "its window left full screen");
        assert!(!is_the_wait(0x10, 3, Some(0x20), None), "another window did");
        assert!(is_the_wait(0x10, 3, None, Some(3)), "its wait ran out");
        assert!(!is_the_wait(0x10, 3, None, Some(2)), "an earlier wait, already called off");
        // Called off (the window was asked for again): nothing is waiting, so nothing is hidden.
        *LEAVING.lock().unwrap() = Some(("main".into(), 0x10, 3));
        assert!(leaving("main") && !leaving("main-2"));
        keep("main-2");
        assert!(leaving("main"));
        keep("main");
        assert!(!leaving("main"));
    }

    /// The real notification centre: the observer is registered, and a notification posted the way
    /// AppKit posts it reaches it with the window it is about.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_end_of_leaving_full_screen_is_heard_with_its_window() {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_foundation::NSString;
        use std::sync::atomic::AtomicUsize;
        static HEARD: AtomicUsize = AtomicUsize::new(0);
        mac::observe_exits(|window| HEARD.store(window, Ordering::SeqCst));
        let name = NSString::from_str("NSWindowDidExitFullScreenNotification");
        // SAFETY: plain Foundation calls with objects that live through the call.
        unsafe {
            let stand_in: *mut AnyObject = msg_send![class!(NSObject), new];
            let centre: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![centre, postNotificationName: &*name, object: stand_in];
            assert_eq!(HEARD.load(Ordering::SeqCst), stand_in as usize);
            let _: () = msg_send![stand_in, release];
        }
    }

    #[test]
    fn the_item_says_what_it_will_do_in_both_languages() {
        assert_eq!(label(false, false), "Enter Full Screen");
        assert_eq!(label(false, true), "Exit Full Screen");
        for full in [false, true] {
            assert!(label(true, full).chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)), "Arabic text");
            assert_ne!(label(true, full), label(true, !full));
        }
        // The system's own keys: ⌃⌘F on a Mac (as AppKit's own item shows), F11 elsewhere.
        let keys = accelerator();
        if cfg!(target_os = "macos") { assert_eq!(keys, "Ctrl+Super+F") } else { assert_eq!(keys, "F11") }
    }

    #[test]
    fn the_menu_keys_from_keys_json_parse_as_menu_accelerators() {
        use std::str::FromStr;
        use thoughtsd::keys::{accelerator as accel, native_defaults, Os};
        // The defaults are what the menu had before 0.5.0.
        assert_eq!(native_defaults(Os::here()).fullscreen.as_deref(), Some(accelerator()));
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            let d = native_defaults(os);
            for k in [d.settings, d.close_window, d.quit, d.fullscreen].into_iter().flatten() {
                assert!(muda::accelerator::Accelerator::from_str(&k).is_ok(), "{k}");
            }
        }
        // Every kind of key the registry can store becomes a key the menu takes.
        for b in ["Mod+Comma", "Mod+Shift+KeyW", "Mod+Control+KeyF", "F11", "Mod+Alt+Digit3", "Control+Alt+Shift+Super+Slash", "Mod+NumpadAdd",
            "Mod+BracketLeft", "Mod+Backquote", "Mod+Semicolon", "Mod+Quote", "Mod+Period", "Mod+Minus", "Mod+Equal", "Mod+Enter", "Mod+Space",
            "Mod+ArrowUp", "Mod+PageDown", "Mod+Home", "Mod+Delete", "Mod+Backspace", "F24", "Mod+IntlBackslash"] {
            let a = accel(b).unwrap();
            assert!(muda::accelerator::Accelerator::from_str(&a).is_ok(), "{b} -> {a}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_guard_replaces_the_window_methods_once() {
        use objc2::runtime::{AnyClass, Imp};
        use objc2::sel;
        install_esc_guard();
        install_esc_guard();
        let window = AnyClass::get(c"NSWindow").unwrap();
        let now = |s| window.instance_method(s).unwrap().implementation() as usize;
        let mine_do = mac::do_command as *const () as usize;
        let mine_cancel = mac::cancel as *const () as usize;
        assert_eq!(now(sel!(doCommandBySelector:)), mine_do);
        assert_eq!(now(sel!(cancelOperation:)), mine_cancel);
        // The saved originals are AppKit's, never the guard itself (that would call itself forever).
        let orig = |o: &std::sync::OnceLock<Imp>| *o.get().unwrap() as usize;
        assert_ne!(orig(&mac::DO_COMMAND), mine_do);
        assert_ne!(orig(&mac::CANCEL), mine_cancel);
    }
}
