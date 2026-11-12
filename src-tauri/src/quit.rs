//! Quitting without losing work.
//!
//! Quitting stops the background service (unless "keep running" hides the window instead), and
//! with it every chat that is still working; it also throws away whatever the page holds unsaved.
//! So before the app really exits, the shell looks at both and asks, with a native dialog.
//!
//! The page is never asked at that moment. It tells the shell about unsaved work as it happens
//! (`set_unsaved`), and the shell keeps the last thing it was told. A page that is frozen or gone
//! can therefore never hold a quit back: at worst the question mentions work that page had
//! reported, and "Quit anyway" goes through.

use crate::recovery::{arabic, choose, fill, one_line, tx, work_lines};
use serde::Deserialize;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri_plugin_dialog::MessageDialogKind;

/// Something in the window that is not saved, as the page described it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Unsaved {
    /// What it is, in the user's words and language: a note's name, "Settings"…
    what: String,
    /// True when the page keeps it as a draft that it offers again next time, so quitting loses
    /// nothing. False when it is simply gone once the window closes.
    #[serde(default)]
    kept: bool,
}

/// What the page last reported. Emptied whenever a page starts loading: a new page starts clean.
static UNSAVED: Mutex<Vec<Unsaved>> = Mutex::new(Vec::new());
/// A quit question is on screen.
static ASKING: AtomicBool = AtomicBool::new(false);
/// "Quit when finished" was chosen and the work is not finished yet.
static WAITING: AtomicBool = AtomicBool::new(false);
/// "Quit now" was chosen while a save was running: the user was told it stops, so the exit does not
/// sit for a minute waiting for it first.
static CUT_SHORT: AtomicBool = AtomicBool::new(false);

/// Whether the exit may stop the background service at once, even in the middle of a save.
pub(crate) fn cut_short() -> bool {
    CUT_SHORT.load(Ordering::SeqCst)
}

fn unsaved() -> Vec<Unsaved> {
    UNSAVED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// What the page may report: a few short, plain descriptions.
fn tidy(items: Vec<Unsaved>) -> Vec<Unsaved> {
    items.into_iter().map(|u| Unsaved { what: one_line(&u.what, 80), kept: u.kept }).filter(|u| !u.what.is_empty()).take(8).collect()
}

/// The page's one way to say "there is unsaved work here": the full list each time, empty when
/// there is none. `set_unsaved {items: [{what, kept?}]}`.
#[tauri::command]
pub(crate) fn set_unsaved(items: Vec<Unsaved>) {
    *UNSAVED.lock().unwrap_or_else(|e| e.into_inner()) = tidy(items);
}

/// A page is loading: whatever the last one held is gone already.
pub(crate) fn page_started() {
    UNSAVED.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

fn lines_for(ar: bool, items: &[Unsaved]) -> Vec<String> {
    items.iter().map(|u| fill(ar, if u.kept { "unsaved.kept" } else { "unsaved.lost" }, &[("what", &u.what)])).collect()
}

/// One sentence per unsaved thing the page reported (none: an empty list).
pub(crate) fn unsaved_lines(ar: bool) -> Vec<String> {
    lines_for(ar, &unsaved())
}

/// How a quit has to be asked about.
#[derive(Debug, PartialEq, Eq)]
enum Question {
    /// Nothing would be lost: just quit.
    None,
    /// Only drafts the page keeps: a gentle "these are kept" with Quit / Cancel.
    Kept,
    /// Unsaved work that will be gone: Quit anyway / Cancel.
    Lost,
    /// Chats or a save are still running: Quit when finished / Quit now / Cancel.
    Running,
}

fn question(work: Option<&Value>, unsaved: &[Unsaved]) -> Question {
    let listed = |w: &Value, k: &str| w[k].as_array().is_some_and(|c| !c.is_empty());
    let running = work.is_some_and(|w| w["saving"] == true || listed(w, "chats") || listed(w, "elsewhere"));
    if running {
        Question::Running
    } else if unsaved.iter().any(|u| !u.kept) {
        Question::Lost
    } else if !unsaved.is_empty() {
        Question::Kept
    } else {
        Question::None
    }
}

/// The text of the quit question: what is running, then what is unsaved.
fn body(ar: bool, work: Option<&Value>, unsaved: &[Unsaved]) -> String {
    let mut parts = Vec::new();
    if let Some(work) = work.filter(|w| !work_lines(ar, w).is_empty()) {
        parts.push(fill(ar, "quit.work", &[("list", &work_lines(ar, work).join("\n"))]));
    }
    let lines = lines_for(ar, unsaved);
    if !lines.is_empty() {
        parts.push(lines.join("\n"));
    }
    parts.join("\n\n")
}

enum Choice {
    Quit,
    WhenFinished,
    Stay,
}

fn ask(app: &tauri::AppHandle, work: Option<&Value>, unsaved: &[Unsaved]) -> Choice {
    let ar = arabic();
    let text = body(ar, work, unsaved);
    let title = tx(ar, "quit.title");
    match question(work, unsaved) {
        Question::None => Choice::Quit,
        Question::Kept => match choose(app, MessageDialogKind::Info, title, &text, &[tx(ar, "quit.go"), tx(ar, "cancel")]) {
            0 => Choice::Quit,
            _ => Choice::Stay,
        },
        Question::Lost => match choose(app, MessageDialogKind::Warning, title, &text, &[tx(ar, "quit.anyway"), tx(ar, "cancel")]) {
            0 => Choice::Quit,
            _ => Choice::Stay,
        },
        Question::Running => match choose(app, MessageDialogKind::Warning, title, &text, &[tx(ar, "quit.later"), tx(ar, "quit.now"), tx(ar, "cancel")]) {
            0 => Choice::WhenFinished,
            1 => Choice::Quit,
            _ => Choice::Stay,
        },
    }
}

/// The user asked to quit for real (not "hide to the tray"). `all`: this is "Quit Thoughts
/// completely", which stops the background service even when "keep running" is on.
/// Never blocks, and never waits for the page.
pub(crate) fn request(app: &tauri::AppHandle, all: bool) {
    if ASKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let work = crate::daemon_alive().and_then(|(i, _)| crate::daemon_work(&i));
        let choice = ask(&app, work.as_ref(), &unsaved());
        ASKING.store(false, Ordering::SeqCst);
        match choice {
            Choice::Quit => {
                if question(work.as_ref(), &[]) == Question::Running {
                    CUT_SHORT.store(true, Ordering::SeqCst);
                }
                exit(&app, all)
            }
            Choice::WhenFinished => wait_then_exit(&app, all),
            Choice::Stay => {}
        }
    });
}

fn exit(app: &tauri::AppHandle, all: bool) {
    if all {
        crate::QUIT_ALL.store(true, Ordering::SeqCst);
    }
    app.exit(0);
}

/// "Quit when finished": the window goes away now, the tray icon says what is happening, and the
/// app exits as soon as nothing is running any more. Opening the window again calls it off.
fn wait_then_exit(app: &tauri::AppHandle, all: bool) {
    if WAITING.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::hide_to_tray(app);
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tx(arabic(), "quit.waiting")));
    }
    loop {
        std::thread::sleep(Duration::from_secs(2));
        if !WAITING.load(Ordering::SeqCst) {
            return; // called off: `still_here` has put the tray back
        }
        if crate::daemon_alive().and_then(|(i, _)| crate::daemon_work(&i)).is_none() {
            break;
        }
    }
    WAITING.store(false, Ordering::SeqCst);
    exit(app, all);
}

/// The window was opened again: a quit that was waiting for work to finish is called off.
pub(crate) fn still_here(app: &tauri::AppHandle) {
    if WAITING.swap(false, Ordering::SeqCst) {
        if let Some(tray) = app.tray_by_id("main") {
            let _ = tray.set_tooltip(Some("Thoughts"));
            let _ = tray.set_visible(crate::keep_running());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(what: &str, kept: bool) -> Unsaved {
        Unsaved { what: what.into(), kept }
    }

    #[test]
    fn a_quit_is_asked_about_only_when_something_would_be_lost() {
        let chats = json!({"saving": false, "chats": ["Plan the week"]});
        let saving = json!({"saving": true, "chats": []});
        let idle = json!({"saving": false, "chats": []});
        assert_eq!(question(None, &[]), Question::None);
        assert_eq!(question(Some(&idle), &[]), Question::None);
        assert_eq!(question(Some(&chats), &[]), Question::Running);
        assert_eq!(question(Some(&saving), &[]), Question::Running);
        // A chat still running in another library is stopped by a quit just the same.
        assert_eq!(question(Some(&json!({"saving": false, "chats": [], "elsewhere": ["Work"]})), &[]), Question::Running);
        assert_eq!(question(Some(&json!({"saving": false, "chats": [], "elsewhere": []})), &[]), Question::None);
        // Running work comes first; unsaved work is mentioned in the same question.
        assert_eq!(question(Some(&chats), &[item("Settings", false)]), Question::Running);
        assert_eq!(question(None, &[item("Settings", false)]), Question::Lost);
        assert_eq!(question(None, &[item("plan.md", true), item("Settings", false)]), Question::Lost);
        // Drafts the page keeps only need a gentle word.
        assert_eq!(question(None, &[item("plan.md", true)]), Question::Kept);
    }

    #[test]
    fn the_question_names_what_is_running_and_what_is_unsaved() {
        let work = json!({"saving": true, "chats": ["Plan the week"]});
        let text = body(false, Some(&work), &[item("plan.md", true), item("Settings", false)]);
        assert!(text.starts_with("Something is still running:\n• Wrap is saving chats into your library\n• The assistant is working in “Plan the week”"), "{text}");
        assert!(text.contains("shows as interrupted next time"));
        assert!(text.ends_with("Your unsaved edits to “plan.md” are kept as a draft and offered again next time.\nUnsaved changes in Settings will be lost."), "{text}");
        assert_eq!(body(false, None, &[item("Settings", false)]), "Unsaved changes in Settings will be lost.");
        assert_eq!(body(true, None, &[item("الإعدادات", false)]), "ستُفقد التغييرات غير المحفوظة في الإعدادات.");
        assert_eq!(body(false, None, &[]), "");
        assert_eq!(body(false, Some(&json!({"saving": false, "chats": []})), &[]), "");
    }

    #[test]
    fn what_the_page_reports_is_kept_short_and_plain() {
        let reported: Vec<Unsaved> = serde_json::from_value(json!([
            {"what": "plan.md", "kept": true}, {"what": "Settings"}, {"what": "  "}, {"what": "line one\nline two\u{7}"}, {"what": "x".repeat(500)},
        ])).unwrap();
        let tidy = tidy(reported);
        assert_eq!(tidy[0], item("plan.md", true));
        assert_eq!(tidy[1], item("Settings", false), "not kept unless the page says so");
        assert_eq!(tidy[2].what, "line one line two");
        assert!(tidy[3].what.chars().count() <= 80 && tidy.len() == 4, "the blank one is dropped, the long one cut");
        let many: Vec<Unsaved> = (0..50).map(|n| item(&format!("note {n}"), true)).collect();
        assert_eq!(super::tidy(many).len(), 8);
        // Something that is not a list of descriptions is refused by the command itself.
        assert!(serde_json::from_value::<Vec<Unsaved>>(json!([{"kept": true}])).is_err());
    }
}
