//! A chat waiting for the user's OK, seen from outside the window, and notifications that lead back
//! to their chat.
//!
//! - `set_attention`: the page says how many approval questions wait (ui/src/lib/attention.svelte.ts).
//!   While some wait and the window is not in front, the Dock icon carries the count (macOS; the
//!   launcher badge on Linux where the desktop supports it; Windows has no badge count) and a new
//!   question asks for attention once (one Dock bounce, a flashing taskbar button on Windows).
//! - A click on a notification that names a chat (`extra.chatId`, App.svelte `notify`) opens that
//!   chat the way a `thoughts://chat/…` link does; a click on any other brings the window forward.

use serde_json::json;
use tauri::UserAttentionType;
use tauri_plugin_notification::{ActionPerformed, NotificationExt};

#[tauri::command]
pub(crate) fn set_attention(window: tauri::Window, waiting: u32, ask: bool) -> Result<(), String> {
    if !crate::is_main_label(window.label()) {
        return Err("not allowed".into());
    }
    let away = !window.is_focused().unwrap_or(false);
    #[cfg(not(windows))]
    let _ = window.set_badge_count(badge(waiting, away));
    if waiting > 0 && away && ask {
        let _ = window.request_user_attention(Some(UserAttentionType::Informational));
    } else if waiting == 0 || !away {
        // macOS ends the bounce by itself and ignores this; Windows and Linux keep flashing otherwise.
        #[cfg(not(target_os = "macos"))]
        let _ = window.request_user_attention(None);
    }
    Ok(())
}

/// The Dock count: only while something waits and the window is in the background.
#[cfg_attr(windows, allow(dead_code))]
fn badge(waiting: u32, away: bool) -> Option<i64> {
    (waiting > 0 && away).then_some(i64::from(waiting))
}

/// The chat a clicked notification is about: a click on the notification itself (not one of its
/// buttons) that carries a well-formed chat id.
fn chat_of(p: &ActionPerformed) -> Option<String> {
    if p.action_id() != "tap" {
        return None;
    }
    let id = p.notification()?.extra().get("chatId")?.as_str()?;
    thoughtsd::chats::valid_id(id).then(|| id.to_string())
}

/// Once at start-up: clicks on the app's notifications open what they are about.
pub(crate) fn watch_clicks(app: &tauri::AppHandle) {
    let handle = app.clone();
    let _ = app.notification().on_action(move |p| {
        if p.action_id() != "tap" {
            return;
        }
        let chat = chat_of(p);
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || match chat {
            Some(id) => crate::links::route(&app, json!({"kind": "chat", "id": id})),
            None => crate::show_main(&app),
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the notification plugin hands its click handlers: the notification as it was sent.
    fn clicked(action: &str, sent: serde_json::Value) -> ActionPerformed {
        let data: tauri_plugin_notification::NotificationData = serde_json::from_value(sent).unwrap();
        serde_json::from_value(json!({"actionId": action, "inputValue": null, "notification": serde_json::to_value(&data).unwrap()})).unwrap()
    }

    #[test]
    fn a_click_opens_the_chat_the_notification_names_and_nothing_else() {
        let id = "0b0f6c2e-1d2a-4f8e-9c1e-5c9f2d7e8a10";
        assert_eq!(chat_of(&clicked("tap", json!({"title": "Thoughts", "body": "done", "extra": {"chatId": id}}))), Some(id.to_string()));
        // A notification about no chat (a reminder), a button rather than the notification, a bad id.
        assert_eq!(chat_of(&clicked("tap", json!({"title": "Reminder"}))), None);
        assert_eq!(chat_of(&clicked("snooze", json!({"title": "x", "extra": {"chatId": id}}))), None);
        for bad in ["", "a/b", "../settings/x", "a b"] {
            assert_eq!(chat_of(&clicked("tap", json!({"title": "x", "extra": {"chatId": bad}}))), None, "{bad}");
        }
        assert_eq!(chat_of(&clicked("tap", json!({"title": "x", "extra": {"chatId": 7}}))), None);
    }

    #[test]
    fn the_dock_count_shows_only_while_something_waits_in_the_background() {
        assert_eq!(badge(0, true), None);
        assert_eq!(badge(2, false), None);
        assert_eq!(badge(2, true), Some(2));
    }
}
