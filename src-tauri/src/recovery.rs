//! Recovery that does not depend on the page.
//!
//! Everything in the Recovery menu is done here, in native code: the shell talks to the background
//! service itself, asks and reports with native dialogs, and reloads or replaces the window
//! natively. A page that is blank, frozen, or restyled by a customisation until its own dialogs
//! cannot be seen can therefore not stand in the way. Nothing here is part of the app layer, and
//! nothing in the app layer can change or remove it.
//!
//! Every action that changes something asks first, naming what will change, and every action that
//! can be taken back offers "Put it back" straight after.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{CheckMenuItem, IsMenuItem, MenuItem, PredefinedMenuItem, Submenu};
use tauri::Emitter;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult};

/// Whether the app speaks Arabic right now (read from the settings each time: it can change).
pub(crate) fn arabic() -> bool {
    thoughtsd::settings::Settings::load().language.starts_with("ar")
}

// ---------- words ----------

/// The words of the native dialogs: (English, Arabic). `{name}` parts are filled in by `fill`.
fn words(key: &str) -> (&'static str, &'static str) {
    match key {
        "ok" => ("OK", "حسناً"),
        "cancel" => ("Cancel", "إلغاء"),
        "wait" => ("Wait", "انتظر"),

        "svc.title" => ("The background service is not answering", "خدمة الخلفية لا تستجيب"),
        "svc.body" => (
            "Thoughts could not reach its background service, so nothing was changed. Choose Recovery ▸ Restart background service, then try again.",
            "تعذّر الوصول إلى خدمة الخلفية، فلم يتغيّر شيء. اختر «الاستعادة» ◂ «إعادة تشغيل خدمة الخلفية» ثم أعد المحاولة.",
        ),
        "fail.title" => ("That did not work", "لم ينجح ذلك"),
        "fail.body" => ("Thoughts could not finish this, and nothing was changed.", "تعذّر إتمام هذا الطلب، ولم يتغيّر شيء."),

        "undo.none.title" => ("Nothing to undo", "لا شيء للتراجع عنه"),
        "undo.none.body" => ("There is no app change to undo.", "لا يوجد تغيير في التطبيق للتراجع عنه."),
        "undo.ask.title" => ("Undo this change?", "التراجع عن هذا التغيير؟"),
        "undo.ask.body" => (
            "“{what}”\n\nOnly this change is taken out; everything else you kept stays. Your library (ideas, notes, todos) is not touched.",
            "«{what}»\n\nيُزال هذا التغيير وحده، ويبقى كل ما اعتمدته غيره. مكتبتك (الأفكار والملاحظات والمهام) لا تُمَسّ.",
        ),
        "undo.go" => ("Undo", "تراجع"),
        "undo.done.title" => ("Change undone", "تم التراجع عن التغيير"),
        "undo.done.body" => (
            "“{what}” is no longer part of the app.\n\nChanged your mind? Put it back now, or later from History.",
            "«{what}» لم يعد جزءاً من التطبيق.\n\nغيّرت رأيك؟ أعِده الآن، أو لاحقاً من «السجل».",
        ),
        "back.put" => ("Put it back", "أعِده كما كان"),
        "back.done.title" => ("Put back", "أُعيد كما كان"),
        "back.done.body" => ("The app is as it was before.", "عاد التطبيق كما كان قبل ذلك."),
        "back.fail.body" => (
            "Thoughts could not put it back. You can still do it from History.",
            "تعذّرت إعادته كما كان. يمكنك فعل ذلك من «السجل».",
        ),

        "rb.ask.title" => ("Go back to this version?", "العودة إلى هذه النسخة؟"),
        "rb.ask.body" => (
            "{when}\n“{what}”\n\nThe app goes back to how it was then: its look, add-ons and extra pages. Your library (ideas, notes, todos) is not touched. The version you have now is kept, so you can return to it.",
            "{when}\n«{what}»\n\nيعود التطبيق كما كان حينها: مظهره وإضافاته وصفحاته الإضافية. مكتبتك (الأفكار والملاحظات والمهام) لا تُمَسّ. نسختك الحالية تبقى محفوظة، فيمكنك الرجوع إليها.",
        ),
        "rb.go" => ("Go back", "العودة"),
        "rb.done.title" => ("The app went back to an earlier version", "عاد التطبيق إلى نسخة سابقة"),
        "rb.done.body" => (
            "It is now as it was on {when}.\n\nNot what you wanted? Put it back as it was a moment ago.",
            "هو الآن كما كان في {when}.\n\nليس ما أردت؟ أعِده كما كان قبل لحظة.",
        ),
        "rb.same.title" => ("Already there", "التطبيق على هذه النسخة بالفعل"),
        "rb.same.body" => ("The app is already the same as that version, so nothing was changed.", "التطبيق مطابق لتلك النسخة بالفعل، فلم يتغيّر شيء."),
        "rb.gone.title" => ("That version is no longer listed", "هذه النسخة لم تعد في القائمة"),
        "rb.gone.body" => (
            "The list of versions has changed. Open the Recovery menu again and choose from the new list.",
            "تغيّرت قائمة النسخ. افتح قائمة «الاستعادة» مجدداً واختر من القائمة الجديدة.",
        ),
        "rb.none.title" => ("Nothing to go back to", "لا شيء للعودة إليه"),
        "rb.none.body" => ("The app has not been changed yet, so there is no earlier version.", "لم يُغيَّر التطبيق بعد، فلا توجد نسخة سابقة."),
        "rb.found.title" => ("Earlier versions found", "وُجدت نسخ سابقة"),
        "rb.found.body" => (
            "They are now listed under Recovery ▸ Go back to an earlier version.",
            "تجدها الآن في «الاستعادة» ◂ «العودة إلى نسخة سابقة».",
        ),
        "rb.more.title" => ("The window is not responding", "النافذة لا تستجيب"),
        "rb.more.body" => (
            "The full list opens in the window, which is not answering right now. Choose a version from the Recovery menu instead, or use Recovery ▸ Open a fresh window first.",
            "القائمة الكاملة تُفتح في النافذة، وهي لا تستجيب الآن. اختر نسخة من قائمة «الاستعادة» بدلاً من ذلك، أو استخدم أولاً «الاستعادة» ◂ «فتح نافذة جديدة».",
        ),
        "note.safe" => (
            "Customisations are switched off at the moment, so you will not see a difference until you turn them back on (Recovery ▸ Customisations off).",
            "التخصيصات متوقفة حالياً، فلن ترى فرقاً حتى تعيد تفعيلها («الاستعادة» ◂ «إيقاف التخصيصات»).",
        ),

        "pend.none.title" => ("Nothing is waiting", "لا شيء في الانتظار"),
        "pend.none.body" => ("There is no change waiting for your review.", "لا يوجد تغيير ينتظر مراجعتك."),
        "pend.ask.title" => ("Discard the change waiting for review?", "تجاهل التغيير المنتظر للمراجعة؟"),
        "pend.ask.body" => (
            "“{what}”\n\nIt has not been applied to the app. Once discarded it cannot be brought back.",
            "«{what}»\n\nلم يُطبَّق على التطبيق بعد. إذا تجاهلته فلن يمكن استرجاعه.",
        ),
        "pend.go" => ("Discard", "تجاهل"),
        "pend.done.title" => ("Change discarded", "تم تجاهل التغيير"),
        "pend.done.body" => ("The app itself was not changed.", "التطبيق نفسه لم يتغيّر."),

        "reload.title" => ("Reload the window?", "إعادة تحميل النافذة؟"),
        "reload.go" => ("Reload", "إعادة التحميل"),
        "reload.note" => ("The window is loaded again for this.", "تُحمَّل النافذة من جديد لأجل ذلك."),
        "unsaved.lost" => ("Unsaved changes in {what} will be lost.", "ستُفقد التغييرات غير المحفوظة في {what}."),
        "unsaved.kept" => (
            "Your unsaved edits to “{what}” are kept as a draft and offered again next time.",
            "تعديلاتك غير المحفوظة على «{what}» تُحفظ كمسودة وتُعرض عليك في المرة القادمة.",
        ),

        "busy.title" => ("Something is still running", "ما زال هناك عمل جارٍ"),
        "busy.saving" => ("• Wrap is saving chats into your library", "• «تلخيص وحفظ» يحفظ المحادثات في مكتبتك"),
        "busy.chat" => ("• The assistant is working in “{title}”", "• المساعد يعمل في «{title}»"),
        "busy.chat.untitled" => ("• The assistant is working in a chat", "• المساعد يعمل في محادثة"),
        "busy.elsewhere" => ("• The assistant is working in your library “{library}”", "• المساعد يعمل في مكتبتك «{library}»"),
        "busy.elsewhere.untitled" => ("• The assistant is working in another of your libraries", "• المساعد يعمل في مكتبة أخرى من مكتباتك"),
        "restart.busy.body" => (
            "Restarting the background service now would stop it:\n{list}\n\nIf you wait, nothing is interrupted.",
            "إعادة تشغيل خدمة الخلفية الآن ستوقفه:\n{list}\n\nإن انتظرت فلن ينقطع شيء.",
        ),
        "restart.anyway" => ("Restart anyway", "أعد التشغيل على أي حال"),
        "restart.done.title" => ("Background service restarted", "أُعيد تشغيل خدمة الخلفية"),
        "restart.done.body" => ("Everything is connected again.", "عاد كل شيء إلى الاتصال."),
        "restart.fail.title" => ("The background service could not be restarted", "تعذّرت إعادة تشغيل خدمة الخلفية"),
        "restart.fail.body" => (
            "Quit Thoughts completely and open it again. If that does not help, restart your computer.",
            "أنهِ Thoughts تماماً ثم افتحه من جديد. وإن لم ينفع ذلك فأعد تشغيل حاسوبك.",
        ),

        "window.fail.title" => ("A new window could not be opened", "تعذّر فتح نافذة جديدة"),
        "window.fail.body" => ("Quit Thoughts and open it again.", "أنهِ Thoughts ثم افتحه من جديد."),

        "quit.title" => ("Quit Thoughts?", "إنهاء Thoughts؟"),
        "quit.work" => (
            "Something is still running:\n{list}\n\nIf you quit now it stops. A chat that was stopped shows as interrupted next time.",
            "ما زال هناك عمل جارٍ:\n{list}\n\nإن أنهيت التطبيق الآن فسيتوقف. المحادثة التي توقفت تظهر في المرة القادمة على أنها انقطعت.",
        ),
        "quit.later" => ("Quit when finished", "أنهِ عند الانتهاء"),
        "quit.now" => ("Quit now", "أنهِ الآن"),
        "quit.anyway" => ("Quit anyway", "أنهِ على أي حال"),
        "quit.go" => ("Quit", "إنهاء"),
        "quit.waiting" => ("Thoughts: finishing the work in progress, then quitting", "Thoughts: يُكمل العمل الجاري ثم يُغلق"),
        _ => ("", ""),
    }
}

pub(crate) fn tx(ar: bool, key: &str) -> &'static str {
    let (en, arabic) = words(key);
    if ar { arabic } else { en }
}

/// `tx` with its `{name}` parts filled in. Values are plain text: nothing in them is interpreted.
pub(crate) fn fill(ar: bool, key: &str, vars: &[(&str, &str)]) -> String {
    let mut s = tx(ar, key).to_string();
    for (name, value) in vars {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

/// One line of text, at most `max` characters, for a label or a dialog: no line breaks or control
/// characters, and an ellipsis when it was cut.
pub(crate) fn one_line(text: &str, max: usize) -> String {
    let clean: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= max {
        return clean;
    }
    let cut: String = clean.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

// ---------- native dialogs ----------

/// Ask with native buttons (one to three; the first is the default). Returns the index of the
/// button pressed; closing the dialog any other way counts as the last one, which callers make
/// the "do nothing" choice. Blocks: never call it on the main thread.
pub(crate) fn choose(app: &tauri::AppHandle, kind: MessageDialogKind, title: &str, body: &str, buttons: &[&str]) -> usize {
    let last = buttons.len().saturating_sub(1);
    let set = match buttons {
        [a] => MessageDialogButtons::OkCustom(a.to_string()),
        [a, b] => MessageDialogButtons::OkCancelCustom(a.to_string(), b.to_string()),
        [a, b, c] => MessageDialogButtons::YesNoCancelCustom(a.to_string(), b.to_string(), c.to_string()),
        _ => MessageDialogButtons::Ok,
    };
    let mut dialog = app.dialog().message(body).title(title).kind(kind).buttons(set);
    // Attached to the window when there is one to see; otherwise it stands on its own.
    if let Some(w) = crate::main_window(app).filter(|w| w.is_visible().unwrap_or(false)) {
        dialog = dialog.parent(&w);
    }
    match dialog.blocking_show_with_result() {
        MessageDialogResult::Custom(pressed) => buttons.iter().position(|b| *b == pressed).unwrap_or(last),
        MessageDialogResult::Ok | MessageDialogResult::Yes => 0,
        MessageDialogResult::No => 1.min(last),
        MessageDialogResult::Cancel => last,
    }
}

/// Like `choose`, for a question where dismissing must never count as any answer (a consent
/// dialog): `Some(index)` only for a button the person really pressed — named by its label, or the
/// default (first) button — and `None` for Cancel, Esc, closing the dialog, or anything unknown.
/// Three buttons put the last one in the cancel role, so a dangerous choice can sit in the middle,
/// neither the default nor what Esc presses.
pub(crate) fn choose_exact(app: &tauri::AppHandle, kind: MessageDialogKind, title: &str, body: &str, buttons: &[&str]) -> Option<usize> {
    let set = match buttons {
        [a] => MessageDialogButtons::OkCustom(a.to_string()),
        [a, b] => MessageDialogButtons::OkCancelCustom(a.to_string(), b.to_string()),
        [a, b, c] => MessageDialogButtons::YesNoCancelCustom(a.to_string(), b.to_string(), c.to_string()),
        _ => return None,
    };
    let mut dialog = app.dialog().message(body).title(title).kind(kind).buttons(set);
    if let Some(w) = crate::main_window(app).filter(|w| w.is_visible().unwrap_or(false)) {
        dialog = dialog.parent(&w);
    }
    exact_choice(dialog.blocking_show_with_result(), buttons)
}

/// What `choose_exact` makes of a dialog's result (see there).
pub(crate) fn exact_choice(result: MessageDialogResult, buttons: &[&str]) -> Option<usize> {
    match result {
        MessageDialogResult::Custom(pressed) => buttons.iter().position(|b| *b == pressed),
        MessageDialogResult::Ok | MessageDialogResult::Yes => (!buttons.is_empty()).then_some(0),
        _ => None,
    }
}

/// Say something, with one OK button.
pub(crate) fn tell(app: &tauri::AppHandle, kind: MessageDialogKind, title: &str, body: &str) {
    choose(app, kind, title, body, &[tx(arabic(), "ok")]);
}

// ---------- the background service ----------

/// Why a request to the background service did not succeed.
#[derive(Debug, PartialEq, Eq)]
enum Fail {
    /// It could not be reached (or could not be started).
    Unreachable,
    /// It answered with a refusal; these are its own words (may be empty).
    Refused(String),
}

/// The background service, as Recovery talks to it: directly, never through the page.
struct Service {
    port: u16,
    token: String,
}

impl Service {
    /// The running service, started if it is not running.
    fn connect() -> Result<Self, Fail> {
        crate::ensure_daemon().map(|(i, _)| Self { port: i.port, token: i.token }).map_err(|_| Fail::Unreachable)
    }

    /// The service only if it is already running (used for things that are not worth starting it for).
    fn running() -> Option<Self> {
        crate::daemon_alive().map(|(i, _)| Self { port: i.port, token: i.token })
    }

    fn get(&self, path: &str) -> Result<Value, Fail> {
        self.call("GET", path, None)
    }

    fn post(&self, path: &str, body: Value) -> Result<Value, Fail> {
        self.call("POST", path, Some(body))
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, Fail> {
        let request = ureq::request(method, &format!("http://127.0.0.1:{}/api{}", self.port, path))
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json")
            .timeout(Duration::from_secs(60));
        let answer = match body {
            Some(b) => request.send_string(&b.to_string()),
            None => request.call(),
        };
        match answer {
            Ok(r) => Ok(r.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)),
            Err(ureq::Error::Status(_, r)) => Err(Fail::Refused(error_text(&r.into_string().unwrap_or_default()))),
            Err(_) => Err(Fail::Unreachable),
        }
    }
}

/// The sentence in an error answer of the service (`{"error": "CODE::words"}` or plain text),
/// without the machine-readable prefix.
fn error_text(body: &str) -> String {
    let message = serde_json::from_str::<Value>(body).ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_else(|| body.to_string());
    let message = message.trim();
    let plain = match message.split_once("::") {
        Some((code, rest)) if code.len() >= 3 && code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') => rest.trim(),
        _ => message,
    };
    plain.chars().take(600).collect()
}

/// Tell the user why something did not happen.
fn report(app: &tauri::AppHandle, fail: &Fail) {
    let ar = arabic();
    match fail {
        Fail::Unreachable => tell(app, MessageDialogKind::Error, tx(ar, "svc.title"), tx(ar, "svc.body")),
        Fail::Refused(why) => tell(app, MessageDialogKind::Error, tx(ar, "fail.title"), if why.is_empty() { tx(ar, "fail.body") } else { why }),
    }
}

// ---------- safe mode: a choice the shell remembers ----------

/// "Customisations off" is kept as a file of the shell's own, not in the page and not in the
/// settings, so it holds across restarts and still works when either of those is damaged.
fn safe_file() -> std::path::PathBuf {
    thoughtsd::paths::home().join("safe-mode")
}

pub(crate) fn safe_mode() -> bool {
    safe_file().exists()
}

pub(crate) fn set_safe_mode(on: bool) {
    if on {
        let _ = std::fs::create_dir_all(thoughtsd::paths::home());
        let _ = std::fs::write(safe_file(), "Customisations are switched off (Recovery menu). Delete this file to turn them back on.\n");
    } else {
        let _ = std::fs::remove_file(safe_file());
    }
}

/// The query the page is loaded with: `safe=1` keeps every customisation out of it, and
/// `recovered=1` tells it that a failed start no longer describes the customisations in place.
pub(crate) fn page_query(safe: bool, recovered: bool) -> Option<String> {
    let parts: Vec<&str> = [safe.then_some("safe=1"), recovered.then_some("recovered=1")].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join("&"))
}

// ---------- the window, natively ----------

/// Whether the page still runs script. Asked natively, with a time limit: a frozen page simply
/// never answers. Blocks for at most two seconds.
pub(crate) fn page_responds(app: &tauri::AppHandle) -> bool {
    let Some(w) = crate::main_window(app) else { return false };
    let (answered, answer) = std::sync::mpsc::channel();
    if w.eval_with_callback("1", move |_| { let _ = answered.send(()); }).is_err() {
        return false;
    }
    answer.recv_timeout(Duration::from_secs(2)).is_ok()
}

/// Load the page again without asking the page to do it. When it still answers, the window is
/// kept and navigated natively; when it does not (or is showing something that is not the app),
/// the window is replaced by a new one. Returns false only if no window could be opened.
fn reload_window(app: &tauri::AppHandle, recovered: bool) -> bool {
    let alive = page_responds(app);
    load_page(app, recovered, alive)
}

/// `reload_window`, for a caller that has already asked whether the page answers.
fn load_page(app: &tauri::AppHandle, recovered: bool, alive: bool) -> bool {
    if alive {
        if let Some(w) = crate::main_window(app) {
            if let Ok(mut url) = w.url() {
                if matches!(url.scheme(), "tauri" | "http" | "https") {
                    url.set_fragment(None);
                    url.set_query(page_query(safe_mode(), recovered).as_deref());
                    if w.navigate(url).is_ok() {
                        return true;
                    }
                }
            }
        }
    }
    replace_window(app, recovered)
}

/// Open a brand-new window and close the old one, whatever state the old one is in.
fn replace_window(app: &tauri::AppHandle, recovered: bool) -> bool {
    match crate::open_main_window(app, recovered) {
        Ok(w) => {
            let _ = w.set_focus();
            true
        }
        Err(e) => {
            let ar = arabic();
            tell(app, MessageDialogKind::Error, tx(ar, "window.fail.title"), &format!("{}\n\n{e}", tx(ar, "window.fail.body")));
            false
        }
    }
}

/// Reloading the window drops what the page holds unsaved. If it reported any, ask first.
fn may_reload(app: &tauri::AppHandle) -> bool {
    let ar = arabic();
    let lines = crate::quit::unsaved_lines(ar);
    if lines.is_empty() {
        return true;
    }
    choose(app, MessageDialogKind::Warning, tx(ar, "reload.title"), &lines.join("\n"), &[tx(ar, "reload.go"), tx(ar, "cancel")]) == 0
}

/// What a confirmation adds when going ahead reloads the window while the page holds unsaved work.
fn reload_warning(ar: bool) -> String {
    let lines = crate::quit::unsaved_lines(ar);
    if lines.is_empty() { String::new() } else { format!("\n\n{} {}", tx(ar, "reload.note"), lines.join(" ")) }
}

// ---------- versions: what "Go back to an earlier version" lists ----------

/// One entry of the app's own history (the customisation layer), newest first in a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Version {
    hash: String,
    date: String,
    summary: String,
    /// The app as it first was. Told by the entry's kind, since its summary comes in the app's
    /// language ("Starting point", «نقطة البداية»).
    start: bool,
}

/// How many earlier versions the menu lists before "More versions…".
const LISTED: usize = 12;

/// The history as last read. `None` until it has been read once.
static VERSIONS: Mutex<Option<Vec<Version>>> = Mutex::new(None);
static LAST_REFRESH: Mutex<Option<Instant>> = Mutex::new(None);

fn versions_of(history: &Value) -> Vec<Version> {
    history.as_array().into_iter().flatten().filter_map(|h| {
        let hash = h["hash"].as_str().filter(|x| thoughtsd::applayer::valid_hash(x))?;
        Some(Version {
            hash: hash.to_string(),
            date: h["date"].as_str().unwrap_or_default().to_string(),
            summary: h["summary"].as_str().or(h["subject"].as_str()).unwrap_or_default().to_string(),
            start: h["entry"] == "start",
        })
    }).collect()
}

fn history(service: &Service) -> Result<Vec<Version>, Fail> {
    service.get("/layer/history").map(|h| versions_of(&h))
}

/// A history date (`2026-11-12T14:05:09+03:00`, the time of day where it was made) for people:
/// `12 Nov 2026, 14:05`. Anything unexpected is shown as it came.
pub(crate) fn when(date: &str, ar: bool) -> String {
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const AR: [&str; 12] = ["يناير", "فبراير", "مارس", "أبريل", "مايو", "يونيو", "يوليو", "أغسطس", "سبتمبر", "أكتوبر", "نوفمبر", "ديسمبر"];
    let number = |range: std::ops::Range<usize>| date.get(range).and_then(|s| s.parse::<usize>().ok());
    let shaped = date.is_ascii() && date.len() >= 16 && &date[4..5] == "-" && &date[7..8] == "-" && &date[10..11] == "T" && &date[13..14] == ":";
    match (shaped, number(0..4), number(5..7), number(8..10), number(11..13), number(14..16)) {
        (true, Some(y), Some(m @ 1..=12), Some(d @ 1..=31), Some(h @ 0..=23), Some(min @ 0..=59)) => {
            if ar { format!("{d} {} {y}، {h:02}:{min:02}", AR[m - 1]) } else { format!("{d} {} {y}, {h:02}:{min:02}", EN[m - 1]) }
        }
        _ => date.to_string(),
    }
}

/// What a history entry is called. The app writes a few entries itself, in English: in Arabic
/// those are worded here; everything else is the summary as it was kept.
pub(crate) fn summary_of(v: &Version, ar: bool) -> String {
    let s = v.summary.trim();
    if !ar {
        return one_line(s, 200);
    }
    if s == "Starting point" {
        return "نقطة البداية".into();
    }
    if s == "Snapshot of manual changes" {
        return "تعديلات يدوية".into();
    }
    if s.starts_with("Went back to an earlier version") {
        return "عودة إلى نسخة سابقة".into();
    }
    if s == "Update customisation guide" || s.starts_with("Update built-in defaults") {
        return "تحديث من التطبيق".into();
    }
    match s.strip_prefix("Undo: ") {
        Some(rest) => one_line(&format!("تراجع عن: {rest}"), 200),
        None => one_line(s, 200),
    }
}

fn is_starting_point(v: &Version) -> bool {
    v.start || v.summary.trim() == "Starting point"
}

/// The menu line for a version: when it was made and what it was.
fn version_label(v: &Version, ar: bool) -> String {
    if is_starting_point(v) {
        return crate::label(ar, "rollback-first").to_string();
    }
    format!("{} · {}", when(&v.date, ar), one_line(&summary_of(v, ar), 48))
}

/// Read the history again and rebuild the menus if it changed. Never starts the service for this.
fn refresh_versions(app: &tauri::AppHandle) {
    *LAST_REFRESH.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    let Some(fresh) = Service::running().and_then(|s| history(&s).ok()) else { return };
    let changed = {
        let mut known = VERSIONS.lock().unwrap_or_else(|e| e.into_inner());
        let changed = known.as_ref() != Some(&fresh);
        *known = Some(fresh);
        changed
    };
    if changed {
        let _ = crate::install_menus(app);
    }
}

/// Refresh the list in the background, at most once every few seconds (called when the window
/// comes to the front, and when the page asks for the menus to be rebuilt).
pub(crate) fn refresh_versions_soon(app: &tauri::AppHandle) {
    let recent = LAST_REFRESH.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < Duration::from_secs(5));
    if recent {
        return;
    }
    *LAST_REFRESH.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    let app = app.clone();
    std::thread::spawn(move || refresh_versions(&app));
}

/// After start-up: wait for the background service (the page starts it), then read the list once.
pub(crate) fn watch_versions(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        for _ in 0..120 {
            std::thread::sleep(Duration::from_secs(1));
            if Service::running().is_some() {
                refresh_versions(&app);
                return;
            }
        }
    });
}

// ---------- the menu ----------

/// The Recovery menu. It lives in the native shell, outside the app layer, so no customisation
/// can remove it, and none of its items needs the page.
pub(crate) fn menu(h: &tauri::AppHandle, ar: bool) -> tauri::Result<Submenu<tauri::Wry>> {
    let versions = VERSIONS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let plain = |id: String, text: String, enabled: bool| MenuItem::with_id(h, id, text, enabled, None::<&str>);
    let mut back: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = Vec::new();
    match &versions {
        // Not read yet (the service was not running): offer to look.
        None => back.push(Box::new(crate::item(h, ar, "rollback-find", None)?)),
        Some(all) if all.len() < 2 => back.push(Box::new(plain("rollback-none".into(), crate::label(ar, "rollback-none").into(), false)?)),
        Some(all) => {
            // The first entry is the app as it is now; everything after it is an earlier version.
            let earlier = &all[1..];
            for v in earlier.iter().take(LISTED) {
                back.push(Box::new(plain(format!("rollback:{}", v.hash), version_label(v, ar), true)?));
            }
            if earlier.len() > LISTED {
                back.push(Box::new(PredefinedMenuItem::separator(h)?));
                if let Some(first) = earlier.last().filter(|v| is_starting_point(v)) {
                    back.push(Box::new(plain(format!("rollback:{}", first.hash), version_label(first, ar), true)?));
                }
                back.push(Box::new(crate::item(h, ar, "rollback-more", None)?));
            }
        }
    }
    let back_items: Vec<&dyn IsMenuItem<tauri::Wry>> = back.iter().map(|b| &**b).collect();
    Submenu::with_items(
        h,
        crate::label(ar, "recovery"),
        true,
        &[
            &crate::item(h, ar, "undo-layer", None)?,
            &Submenu::with_items(h, crate::label(ar, "rollback"), true, &back_items)?,
            &crate::item(h, ar, "discard-pending", None)?,
            &PredefinedMenuItem::separator(h)?,
            &CheckMenuItem::with_id(h, "safe-mode", crate::label(ar, "safe-mode"), true, safe_mode(), None::<&str>)?,
            &crate::item(h, ar, "reload", Some("CmdOrCtrl+R"))?,
            &crate::item(h, ar, "fresh-window", None)?,
            &crate::item(h, ar, "restart-daemon", None)?,
        ],
    )
}

/// One Recovery action at a time: a second choice while a dialog of the first is open is ignored.
static RUNNING: AtomicBool = AtomicBool::new(false);

struct Running;
impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Run the Recovery item `id`. Returns false when `id` is not one of them. Never blocks: the work
/// and its dialogs happen on a thread of their own.
pub(crate) fn handle(app: &tauri::AppHandle, id: &str) -> bool {
    let action: Box<dyn FnOnce(&tauri::AppHandle) + Send> = match id {
        "undo-layer" => Box::new(undo_last),
        "discard-pending" => Box::new(discard_pending),
        "safe-mode" => Box::new(toggle_safe_mode),
        "reload" => Box::new(reload),
        "fresh-window" => Box::new(fresh_window),
        "restart-daemon" => Box::new(restart_service),
        "rollback-find" => Box::new(find_versions),
        "rollback-more" => Box::new(more_versions),
        _ => match id.strip_prefix("rollback:") {
            Some(hash) => {
                let hash = hash.to_string();
                Box::new(move |app: &tauri::AppHandle| rollback(app, &hash))
            }
            None => return false,
        },
    };
    // Whatever the window was doing (hidden in the tray, minimised), Recovery happens in front.
    crate::show_main(app);
    // The tick of "Customisations off" flips by itself on every click; afterwards it is put back
    // to what is true (never from inside the menu's own click, hence always on the thread).
    let ticked = id == "safe-mode";
    let busy = RUNNING.swap(true, Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        if !busy {
            let running = Running;
            action(&app);
            drop(running);
            // The list of earlier versions may have changed (this rebuilds the menus if it did).
            refresh_versions(&app);
        }
        if ticked {
            let _ = crate::install_menus(&app);
        }
    });
    true
}

// ---------- the actions ----------

/// After a change to the app's history: bring the window up to date, say what happened and
/// offer the way back. `before` is the newest entry from before the change.
fn finish(app: &tauri::AppHandle, service: &Service, before: Option<&Version>, reload: bool, done_title: &str, done_body: &str) {
    let ar = arabic();
    let after = history(service).unwrap_or_default();
    refresh_page(app, reload);
    // The newest entry is the one this action just added. Putting things back undoes exactly that
    // entry, as History's own Undo would: the app returns to how it was a moment ago (edits made by
    // hand included, which were saved as an entry of their own first), and the history stays
    // truthful, so a later "Undo last app change" names the change that is really in effect again.
    let way_back = after.first().filter(|v| Some(&v.hash) != before.map(|b| &b.hash));
    let mut body = done_body.to_string();
    if safe_mode() {
        body = format!("{body}\n\n{}", tx(ar, "note.safe"));
    }
    let Some(way_back) = way_back else {
        tell(app, MessageDialogKind::Info, done_title, &body);
        return;
    };
    if choose(app, MessageDialogKind::Info, done_title, &body, &[tx(ar, "ok"), tx(ar, "back.put")]) != 1 {
        return;
    }
    match service.post("/layer/revert", json!({"hash": way_back.hash})) {
        Ok(_) => {
            refresh_page(app, reload);
            tell(app, MessageDialogKind::Info, tx(ar, "back.done.title"), tx(ar, "back.done.body"));
        }
        Err(Fail::Unreachable) => report(app, &Fail::Unreachable),
        Err(Fail::Refused(why)) => {
            let body = if why.is_empty() { tx(ar, "back.fail.body").to_string() } else { format!("{}\n\n{why}", tx(ar, "back.fail.body")) };
            tell(app, MessageDialogKind::Error, tx(ar, "fail.title"), &body);
        }
    }
}

/// Show the changed app. A page that still runs is told by the service itself and updates in
/// place, unless a full reload was asked for; a page that does not answer is replaced.
fn refresh_page(app: &tauri::AppHandle, reload: bool) {
    let alive = page_responds(app);
    if reload || !alive {
        load_page(app, true, alive);
    }
}

/// Undo last app change… — names the change, asks, undoes, offers to put it back.
fn undo_last(app: &tauri::AppHandle) {
    let ar = arabic();
    let service = match Service::connect() {
        Ok(s) => s,
        Err(fail) => return report(app, &fail),
    };
    let (before, next) = match (history(&service), service.get("/layer/undo-last")) {
        (Ok(h), Ok(n)) => (h, n),
        (Err(fail), _) | (_, Err(fail)) => return report(app, &fail),
    };
    let hash = next["hash"].as_str().filter(|h| thoughtsd::applayer::valid_hash(h)).unwrap_or_default().to_string();
    if next["exists"] != true || hash.is_empty() {
        return tell(app, MessageDialogKind::Info, tx(ar, "undo.none.title"), tx(ar, "undo.none.body"));
    }
    let what = summary_of(&Version { hash: hash.clone(), date: String::new(), summary: next["summary"].as_str().unwrap_or_default().to_string(), start: false }, ar);
    let body = fill(ar, "undo.ask.body", &[("what", &what)]);
    if choose(app, MessageDialogKind::Warning, tx(ar, "undo.ask.title"), &body, &[tx(ar, "undo.go"), tx(ar, "cancel")]) != 0 {
        return;
    }
    // Exactly the change that was named: by its own entry, not "whatever is newest by now".
    if let Err(fail) = service.post("/layer/revert", json!({"hash": hash})) {
        return report(app, &fail);
    }
    finish(app, &service, before.first(), false, tx(ar, "undo.done.title"), &fill(ar, "undo.done.body", &[("what", &what)]));
}

/// Go back to an earlier version ▸ (one entry) — asks, goes back, reloads, offers to put it back.
fn rollback(app: &tauri::AppHandle, hash: &str) {
    let ar = arabic();
    let service = match Service::connect() {
        Ok(s) => s,
        Err(fail) => return report(app, &fail),
    };
    let before = match history(&service) {
        Ok(h) => h,
        Err(fail) => return report(app, &fail),
    };
    // The menu may be older than the history: only a version that is still there can be chosen.
    let Some(target) = before.iter().find(|v| v.hash == hash).cloned() else {
        return tell(app, MessageDialogKind::Info, tx(ar, "rb.gone.title"), tx(ar, "rb.gone.body"));
    };
    let date = when(&target.date, ar);
    let what = if is_starting_point(&target) { crate::label(ar, "rollback-first").to_string() } else { summary_of(&target, ar) };
    let body = format!("{}{}", fill(ar, "rb.ask.body", &[("when", &date), ("what", &what)]), reload_warning(ar));
    if choose(app, MessageDialogKind::Warning, tx(ar, "rb.ask.title"), &body, &[tx(ar, "rb.go"), tx(ar, "cancel")]) != 0 {
        return;
    }
    if let Err(fail) = service.post("/layer/rollback", json!({"hash": target.hash})) {
        return report(app, &fail);
    }
    let unchanged = history(&service).ok().and_then(|h| h.first().map(|v| v.hash.clone())) == before.first().map(|v| v.hash.clone());
    if unchanged {
        return tell(app, MessageDialogKind::Info, tx(ar, "rb.same.title"), tx(ar, "rb.same.body"));
    }
    finish(app, &service, before.first(), true, tx(ar, "rb.done.title"), &fill(ar, "rb.done.body", &[("when", &date)]));
}

/// Look for earlier versions… — shown while the list has not been read yet.
fn find_versions(app: &tauri::AppHandle) {
    let ar = arabic();
    let found = match Service::connect().and_then(|s| history(&s)) {
        Ok(h) => h,
        Err(fail) => return report(app, &fail),
    };
    let any = found.len() > 1;
    *VERSIONS.lock().unwrap_or_else(|e| e.into_inner()) = Some(found);
    let _ = crate::install_menus(app);
    if any {
        tell(app, MessageDialogKind::Info, tx(ar, "rb.found.title"), tx(ar, "rb.found.body"));
    } else {
        tell(app, MessageDialogKind::Info, tx(ar, "rb.none.title"), tx(ar, "rb.none.body"));
    }
}

/// More versions… — the full list is the page's History; say so plainly when the page is gone.
fn more_versions(app: &tauri::AppHandle) {
    if page_responds(app) {
        let _ = app.emit("recovery", "rollback");
    } else {
        let ar = arabic();
        tell(app, MessageDialogKind::Warning, tx(ar, "rb.more.title"), tx(ar, "rb.more.body"));
    }
}

/// Discard the change waiting for review… — a change the assistant prepared but that was never applied.
fn discard_pending(app: &tauri::AppHandle) {
    let ar = arabic();
    let service = match Service::connect() {
        Ok(s) => s,
        Err(fail) => return report(app, &fail),
    };
    let pending = match service.get("/layer/pending") {
        Ok(p) => p,
        Err(fail) => return report(app, &fail),
    };
    if pending["exists"] != true {
        return tell(app, MessageDialogKind::Info, tx(ar, "pend.none.title"), tx(ar, "pend.none.body"));
    }
    let what = one_line(pending["summary"].as_str().unwrap_or_default().lines().next().unwrap_or_default(), 200);
    if choose(app, MessageDialogKind::Warning, tx(ar, "pend.ask.title"), &fill(ar, "pend.ask.body", &[("what", &what)]), &[tx(ar, "pend.go"), tx(ar, "cancel")]) != 0 {
        return;
    }
    match service.post("/layer/pending/discard", json!({})) {
        Ok(_) => tell(app, MessageDialogKind::Info, tx(ar, "pend.done.title"), tx(ar, "pend.done.body")),
        Err(fail) => report(app, &fail),
    }
}

/// Customisations off (safe mode) — a tick that stays until it is taken away again.
fn toggle_safe_mode(app: &tauri::AppHandle) {
    if !may_reload(app) {
        return;
    }
    set_safe_mode(!safe_mode());
    reload_window(app, false);
}

/// Reload — natively, so it works on a page that cannot reload itself.
fn reload(app: &tauri::AppHandle) {
    if may_reload(app) {
        reload_window(app, false);
    }
}

/// Open a fresh window — a new window and a new page process, whatever state the old one is in.
fn fresh_window(app: &tauri::AppHandle) {
    if may_reload(app) {
        replace_window(app, false);
    }
}

/// The lines of a "something is still running" dialog for `work` (`{"saving":bool,"chats":[title,…]}`).
pub(crate) fn work_lines(ar: bool, work: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    if work["saving"] == true {
        lines.push(tx(ar, "busy.saving").to_string());
    }
    let chats: Vec<&Value> = work["chats"].as_array().into_iter().flatten().collect();
    for chat in chats.iter().take(6) {
        let title = one_line(chat.as_str().unwrap_or_default(), 60);
        lines.push(if title.is_empty() { tx(ar, "busy.chat.untitled").to_string() } else { fill(ar, "busy.chat", &[("title", &title)]) });
    }
    if chats.len() > 6 {
        lines.push("• …".into());
    }
    // Chats still running in libraries other than the open one (`daemon_work`).
    for library in work["elsewhere"].as_array().into_iter().flatten().take(6) {
        let name = one_line(library.as_str().unwrap_or_default(), 60);
        lines.push(if name.is_empty() { tx(ar, "busy.elsewhere.untitled").to_string() } else { fill(ar, "busy.elsewhere", &[("library", &name)]) });
    }
    lines
}

/// Restart background service — asks natively while work is running.
fn restart_service(app: &tauri::AppHandle) {
    let ar = arabic();
    let stopped = match crate::stop_daemon_checked(false) {
        Err(busy) => {
            let work = busy.strip_prefix("BUSY::").and_then(|j| serde_json::from_str::<Value>(j).ok()).unwrap_or(Value::Null);
            let body = fill(ar, "restart.busy.body", &[("list", &work_lines(ar, &work).join("\n"))]);
            if choose(app, MessageDialogKind::Warning, tx(ar, "busy.title"), &body, &[tx(ar, "wait"), tx(ar, "restart.anyway")]) != 1 {
                return;
            }
            crate::stop_daemon_checked(true)
        }
        ok => ok,
    };
    match stopped.and_then(|_| crate::ensure_daemon()) {
        Ok(_) => tell(app, MessageDialogKind::Info, tx(ar, "restart.done.title"), tx(ar, "restart.done.body")),
        Err(_) => tell(app, MessageDialogKind::Error, tx(ar, "restart.fail.title"), tx(ar, "restart.fail.body")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(summary: &str, date: &str) -> Version {
        Version { hash: "0123abcd".into(), date: date.into(), summary: summary.into(), start: false }
    }

    #[test]
    fn every_dialog_sentence_exists_in_both_languages() {
        let keys = [
            "ok", "cancel", "wait", "svc.title", "svc.body", "fail.title", "fail.body", "undo.none.title", "undo.none.body", "undo.ask.title", "undo.ask.body",
            "undo.go", "undo.done.title", "undo.done.body", "back.put", "back.done.title",
            "back.done.body", "back.fail.body", "rb.ask.title", "rb.ask.body", "rb.go", "rb.done.title", "rb.done.body", "rb.same.title", "rb.same.body",
            "rb.gone.title", "rb.gone.body", "rb.none.title", "rb.none.body", "rb.found.title", "rb.found.body", "rb.more.title", "rb.more.body", "note.safe",
            "pend.none.title", "pend.none.body", "pend.ask.title", "pend.ask.body", "pend.go", "pend.done.title", "pend.done.body", "reload.title", "reload.go",
            "reload.note", "unsaved.lost", "unsaved.kept", "busy.title", "busy.saving", "busy.chat", "busy.chat.untitled", "busy.elsewhere", "busy.elsewhere.untitled", "restart.busy.body", "restart.anyway",
            "restart.done.title", "restart.done.body", "restart.fail.title", "restart.fail.body", "window.fail.title", "window.fail.body", "quit.title",
            "quit.work", "quit.later", "quit.now", "quit.anyway", "quit.go", "quit.waiting",
        ];
        for key in keys {
            let (en, ar) = words(key);
            assert!(!en.is_empty() && !ar.is_empty(), "{key}");
            // The same blanks to fill in, in both.
            let blanks = |s: &str| { let mut b: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split_once('}').map(|(n, _)| n.to_string())).collect(); b.sort(); b };
            assert_eq!(blanks(en), blanks(ar), "{key}");
            assert!(ar.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)), "{key} is not Arabic");
        }
        assert_eq!(words("no such key"), ("", ""));
        assert_eq!(fill(false, "busy.chat", &[("title", "Tax {what} notes")]), "• The assistant is working in “Tax {what} notes”");
        assert_eq!(fill(true, "unsaved.lost", &[("what", "الإعدادات")]), "ستُفقد التغييرات غير المحفوظة في الإعدادات.");
    }

    #[test]
    fn history_dates_read_like_dates() {
        assert_eq!(when("2026-11-12T14:05:09+03:00", false), "12 Nov 2026, 14:05");
        assert_eq!(when("2026-01-03T00:07:00Z", false), "3 Jan 2026, 00:07");
        assert_eq!(when("2026-11-12T14:05:09+03:00", true), "12 نوفمبر 2026، 14:05");
        // Anything else is shown as it came rather than guessed at.
        for odd in ["", "yesterday", "2026-13-01T10:00:00Z", "2026-11-12 14:05", "٢٠٢٦-١١-١٢T14:05:09"] {
            assert_eq!(when(odd, false), odd);
        }
    }

    #[test]
    fn versions_are_listed_by_when_and_what() {
        assert_eq!(version_label(&version("Make the sidebar green", "2026-11-12T14:05:09+03:00"), false), "12 Nov 2026, 14:05 · Make the sidebar green");
        let long = version_label(&version(&"word ".repeat(40), "2026-11-12T14:05:09+03:00"), false);
        assert!(long.ends_with('…') && long.chars().count() < 75, "{long}");
        // Line breaks and control characters never reach a menu.
        assert_eq!(one_line("two\nlines\u{7}\there", 50), "two lines here");
        assert_eq!(one_line("  spaced   out  ", 50), "spaced out");
        assert_eq!(one_line("abcdefghij", 5), "abcd…");
        // The entries the app writes itself are worded for people, in Arabic too.
        assert_eq!(version_label(&version("Starting point", "2026-01-01T09:00:00Z"), false), "The app as it first was (no customisations)");
        assert_eq!(summary_of(&version("Undo: Make text red", ""), true), "تراجع عن: Make text red");
        assert_eq!(summary_of(&version("Went back to an earlier version (abc1234)", ""), true), "عودة إلى نسخة سابقة");
        assert_eq!(summary_of(&version("Snapshot of manual changes", ""), true), "تعديلات يدوية");
        assert_eq!(summary_of(&version("أضف صفحة للعملاء", ""), true), "أضف صفحة للعملاء");
        assert_eq!(summary_of(&version("Undo: Make text red", ""), false), "Undo: Make text red");
    }

    #[test]
    fn only_well_formed_history_entries_become_menu_items() {
        let history = json!([
            {"hash": "aaaa1111", "short": "aaaa111", "summary": "Newest", "date": "2026-11-12T14:05:09+03:00", "files": ["custom.css"]},
            {"hash": "--upload-pack=x", "summary": "not a version"},
            {"hash": "bbbb2222", "subject": "Older client field", "date": "2026-11-11T10:00:00+03:00"},
            {"summary": "no hash"},
        ]);
        let v = versions_of(&history);
        assert_eq!(v.iter().map(|x| x.hash.as_str()).collect::<Vec<_>>(), ["aaaa1111", "bbbb2222"]);
        assert_eq!(v[1].summary, "Older client field");
        assert!(versions_of(&json!({"error": "nope"})).is_empty() && versions_of(&Value::Null).is_empty());
        // The service words its entries in the app's language: the starting point is told by its kind.
        let arabic = versions_of(&json!([{"hash": "cccc3333", "summary": "نقطة البداية", "entry": "start", "date": "2026-01-01T09:00:00Z"}]));
        assert!(is_starting_point(&arabic[0]));
        assert_eq!(version_label(&arabic[0], true), "التطبيق كما كان أول مرة (بلا تخصيصات)");
        assert!(!is_starting_point(&v[0]) && is_starting_point(&version("Starting point", "")));
    }

    #[test]
    fn the_page_is_loaded_with_the_right_query() {
        assert_eq!(page_query(false, false), None);
        assert_eq!(page_query(true, false).as_deref(), Some("safe=1"));
        assert_eq!(page_query(false, true).as_deref(), Some("recovered=1"));
        assert_eq!(page_query(true, true).as_deref(), Some("safe=1&recovered=1"));
    }

    #[test]
    fn service_errors_are_shown_as_sentences() {
        assert_eq!(error_text(r#"{"error":"BUSY::Thoughts is saving right now.","code":"busy"}"#), "Thoughts is saving right now.");
        assert_eq!(error_text(r#"{"error":"This change is already undone."}"#), "This change is already undone.");
        assert_eq!(error_text("unauthorized"), "unauthorized");
        assert_eq!(error_text(r#"{"error":"NO_HISTORY::"}"#), "");
        // A sentence with "::" in it is not mistaken for a machine prefix.
        assert_eq!(error_text(r#"{"error":"Could not read std::fs data"}"#), "Could not read std::fs data");
        assert_eq!(error_text(""), "");
    }

    #[test]
    fn running_work_is_listed_line_by_line() {
        let work = json!({"saving": true, "chats": ["Plan the week", "", "A very long chat title that goes on and on and on and on and on and on and on"]});
        let lines = work_lines(false, &work);
        assert_eq!(lines[0], "• Wrap is saving chats into your library");
        assert_eq!(lines[1], "• The assistant is working in “Plan the week”");
        assert_eq!(lines[2], "• The assistant is working in a chat");
        assert!(lines[3].ends_with("…”") && lines.len() == 4);
        let many = json!({"chats": ["1", "2", "3", "4", "5", "6", "7", "8"]});
        assert_eq!(work_lines(true, &many).len(), 7, "six chats and a line that says there are more");
        assert!(work_lines(false, &Value::Null).is_empty());
        // Chats running in another library are named by the library they run in.
        let elsewhere = json!({"saving": false, "chats": ["Plan the week"], "elsewhere": ["Work", ""]});
        assert_eq!(work_lines(false, &elsewhere), [
            "• The assistant is working in “Plan the week”",
            "• The assistant is working in your library “Work”",
            "• The assistant is working in another of your libraries",
        ]);
        assert_eq!(work_lines(true, &json!({"elsewhere": ["العمل"]})), ["• المساعد يعمل في مكتبتك «العمل»"]);
    }
}
