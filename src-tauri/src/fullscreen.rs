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

/// The keys shown on the item: the system's own ⌃⌘F on a Mac, F11 elsewhere.
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
    let it = MenuItem::with_id(h, ID, label(ar, full), true, Some(accelerator()))?;
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
