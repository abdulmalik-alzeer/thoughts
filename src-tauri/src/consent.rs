// The shell's half of `thoughtsd::consent`: the user's yes to bypassing permissions or to trusting a
// folder, proven to the daemon with a key the window never sees.
//
// The key is made here, once per launch, kept in memory and handed only to a daemon this shell
// starts (on its standard input, see `spawn_daemon`). A daemon this shell did not start (one started
// at login, or left running by an earlier launch) has no key: `ensure_daemon` replaces it with one
// of its own as soon as it is idle. The window asks for an approval with `approve_action`; the shell
// then asks the user itself, in a native dialog the page cannot draw, answer or word (the chat's
// name and the folder come from the service, not from the window). Only a yes there makes an
// approval, good for one action, once, for two minutes. One question at a time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use thoughtsd::consent::{self, Action, Key};
use tauri_plugin_dialog::MessageDialogKind;
use thoughtsd::paths::DaemonInfo;

/// This launch's key.
pub fn key() -> &'static Key {
    static K: OnceLock<Key> = OnceLock::new();
    K.get_or_init(consent::new_key)
}

/// The daemon (pid, token) last proven to hold this launch's key.
static PROVEN: Mutex<Option<(u32, String)>> = Mutex::new(None);

/// Does the daemon `i` hold this launch's key? Asked with a fresh challenge; the key never crosses.
pub fn holds_key(i: &DaemonInfo) -> bool {
    let mut proven = PROVEN.lock().unwrap_or_else(|e| e.into_inner());
    if proven.as_ref().is_some_and(|(pid, token)| *pid == i.pid && *token == i.token) {
        return true;
    }
    let nonce = thoughtsd::hello::new_nonce();
    let ok = ureq::get(&format!("http://127.0.0.1:{}/api/consent/proof?nonce={nonce}", i.port))
        .set("Authorization", &format!("Bearer {}", i.token))
        .timeout(Duration::from_secs(2))
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .is_some_and(|body| consent::proof_ok(&body, key(), &nonce));
    if ok {
        *proven = Some((i.pid, i.token.clone()));
    }
    ok
}

fn arabic() -> bool {
    thoughtsd::settings::Settings::load().language.starts_with("ar")
}

/// What the confirmation is about, as the service (not the window) describes it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Subject {
    /// One chat: its title (when the service could say) and whether Codex answers in it.
    Chat { title: Option<String>, codex: bool },
    /// The default for new chats.
    Default,
    /// A folder to trust (when the service could say which).
    Folder(Option<String>),
}

/// The words of the native confirmation.
#[derive(Debug, PartialEq)]
pub(crate) struct Ask {
    pub title: &'static str,
    pub body: String,
    pub yes: &'static str,
    pub no: &'static str,
}

/// A name inside a sentence: one line, not too long, kept apart from the text around it so it
/// reads the right way in either language.
fn named(text: &str, max: usize) -> String {
    let bidi = |c: char| matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let plain: String = text.chars().filter(|c| !bidi(*c)).collect();
    format!("\u{2068}{}\u{2069}", crate::recovery::one_line(&plain, max))
}

/// The confirmation for `subject`, in Arabic when `ar`.
pub(crate) fn ask_text(subject: &Subject, ar: bool) -> Ask {
    const CLAUDE_RISK: (&str, &str) = (
        "Claude will run commands and change files without asking first — anywhere on this computer your account can reach, not only in your library. A mistake, or an instruction hidden in a file or web page it reads, could delete or leak your data.",
        "سيشغّل Claude الأوامر ويعدّل الملفات دون أن يستأذن أولاً — في أي مكان على هذا الحاسوب يصل إليه حسابك، لا في مكتبتك فقط. خطأ واحد، أو تعليمات مخفية في ملف أو صفحة ويب يقرؤها، قد يحذف بياناتك أو يسرّبها.",
    );
    const CLAUDE_NOTE: (&str, &str) = (
        "Your private folders and the app’s own data stay blocked for Claude’s file tools, but a shell command it runs could still reach them. You can switch back at any time.",
        "تبقى مجلداتك الخاصة وبيانات التطبيق نفسه محجوبة عن أدوات الملفات لدى Claude، لكن أمراً يشغّله في الطرفية قد يصل إليها. ويمكنك التراجع في أي وقت.",
    );
    const CODEX_RISK: (&str, &str) = (
        "Codex will run commands and change files with no sandbox — anywhere on this computer your account can reach, and with internet access. A mistake, or an instruction hidden in a file or web page it reads, could delete or leak your data.",
        "سيشغّل Codex الأوامر ويعدّل الملفات دون بيئة معزولة — في أي مكان على هذا الحاسوب يصل إليه حسابك، ومع وصول إلى الإنترنت. خطأ واحد، أو تعليمات مخفية في ملف أو صفحة ويب يقرؤها، قد يحذف بياناتك أو يسرّبها.",
    );
    const CODEX_NOTE: (&str, &str) = (
        "Even in its other modes, Codex cannot keep the assistant out of your private folders or the app’s own data. You can switch back at any time.",
        "حتى في أوضاعه الأخرى لا يستطيع Codex أن يُبعد المساعد عن مجلداتك الخاصة ولا عن بيانات التطبيق نفسه. ويمكنك التراجع في أي وقت.",
    );
    let pick = |pair: (&'static str, &'static str)| if ar { pair.1 } else { pair.0 };
    let cancel = pick(("Cancel", "إلغاء"));
    match subject {
        Subject::Chat { title, codex } => {
            let (risk, note) = if *codex { (CODEX_RISK, CODEX_NOTE) } else { (CLAUDE_RISK, CLAUDE_NOTE) };
            let which = match title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                Some(t) if ar => format!("المحادثة: «{}»", named(t, 80)),
                Some(t) => format!("Chat: “{}”", named(t, 80)),
                None => pick(("This chat.", "هذه المحادثة.")).to_string(),
            };
            let applies = pick(("It applies to this chat until you change it back.", "يسري هذا على هذه المحادثة إلى أن تغيّره."));
            Ask {
                title: if *codex { pick(("Give Codex full access in this chat?", "منح Codex وصولاً كاملاً في هذه المحادثة؟")) } else { pick(("Bypass all permissions in this chat?", "تجاوز كل الأذونات في هذه المحادثة؟")) },
                body: format!("{which}\n\n{}\n\n{applies} {}", pick(risk), pick(note)),
                yes: if *codex { pick(("Give full access", "امنح وصولاً كاملاً")) } else { pick(("Bypass permissions", "تجاوز الأذونات")) },
                no: cancel,
            }
        }
        Subject::Default => Ask {
            title: pick(("Bypass all permissions in new chats?", "تجاوز كل الأذونات في المحادثات الجديدة؟")),
            body: format!(
                "{}\n\n{} {}",
                pick(CLAUDE_RISK),
                pick((
                    "This becomes the default for new chats you start yourself. Chats started for you by an add-on or a shortcut chip use Accept edits.",
                    "سيصبح هذا هو الوضع الافتراضي للمحادثات التي تبدؤها بنفسك. أما المحادثات التي تبدؤها إضافة أو زر اختصار فتستخدم «قبول التعديلات».",
                )),
                pick(CLAUDE_NOTE)
            ),
            yes: pick(("Bypass permissions", "تجاوز الأذونات")),
            no: cancel,
        },
        Subject::Folder(path) => {
            let which = match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
                Some(p) if ar => format!("المجلد: «{}»", named(p, 200)),
                Some(p) => format!("Folder: “{}”", named(p, 200)),
                None => pick(("The folder shown in the app window.", "المجلد المعروض في نافذة التطبيق.")).to_string(),
            };
            Ask {
                title: pick(("Trust this folder?", "هل تثق بهذا المجلد؟")),
                body: format!(
                    "{which}\n\n{}",
                    pick((
                        "The assistant settings and programs Thoughts found in it, listed in the app window, will be allowed to run when you work there. Trust it only if you know where its contents came from.",
                        "سيُسمح لإعدادات المساعد والبرامج التي وجدها Thoughts فيه، والمعروضة في نافذة التطبيق، بأن تعمل حين تعمل فيه. لا تثق به إلا إذا كنت تعرف مصدر محتواه.",
                    ))
                ),
                yes: pick(("Trust", "أثق به")),
                no: pick(("Don’t trust", "لا أثق به")),
            }
        }
    }
}

/// What the window is told when no approval was made because the user said no, or because another
/// confirmation is already on screen. The window treats it as a plain "no" and says nothing.
pub(crate) const DECLINED: &str = "DECLINED::";

/// A confirmation is on screen (only one at a time).
static ASKING: AtomicBool = AtomicBool::new(false);

/// Holds `ASKING` while a confirmation is open.
struct Asking;
impl Asking {
    fn start() -> Option<Asking> {
        ASKING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| Asking)
    }
}
impl Drop for Asking {
    fn drop(&mut self) {
        ASKING.store(false, Ordering::SeqCst);
    }
}

/// A GET to the service, as JSON.
fn get(i: &DaemonInfo, path: &str) -> Option<serde_json::Value> {
    ureq::get(&format!("http://127.0.0.1:{}/api{path}", i.port))
        .set("Authorization", &format!("Bearer {}", i.token))
        .timeout(Duration::from_secs(3))
        .call()
        .ok()?
        .into_string()
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
}

/// What `subject` of `what` is, asked of the service: the chat's title and assistant, the folder's
/// path. Nothing here comes from the window, which only sent the id the approval is bound to.
fn learn(i: &DaemonInfo, what: Action, subject: &str) -> Subject {
    match what {
        Action::ChatBypass => {
            let chat = get(i, "/chats").and_then(|list| list.as_array()?.iter().find(|c| c["id"] == subject).cloned());
            Subject::Chat {
                title: chat.as_ref().and_then(|c| c["title"].as_str()).map(str::to_string),
                codex: chat.as_ref().is_some_and(|c| c["provider"] == "codex"),
            }
        }
        Action::SettingsBypass => Subject::Default,
        Action::Trust => Subject::Folder(get(i, &format!("/library/trust?id={subject}")).and_then(|v| v["path"].as_str().map(str::to_string))),
    }
}

/// `approve_action(action, subject)` → an approval for exactly that action, which the window sends
/// with the request it confirms. `action`: "chat-bypass" (subject: the chat id), "settings-bypass"
/// (no subject) or "trust" (subject: the trust question id). The app window only. The shell asks the
/// user first in a native dialog; a "no", or a second call while one is open, fails with `DECLINED::`.
#[tauri::command]
pub async fn approve_action(app: tauri::AppHandle, window: tauri::Window, action: String, subject: String) -> Result<String, String> {
    crate::main_only(&window)?;
    let ar = arabic();
    let Some(what) = Action::from_name(&action).filter(|a| a.subject_ok(&subject)) else {
        return Err(if ar { "تعذّر تأكيد هذا." } else { "This could not be confirmed." }.into());
    };
    let Some(asking) = Asking::start() else { return Err(DECLINED.into()) };
    tauri::async_runtime::spawn_blocking(move || {
        let _asking = asking;
        let Some(i) = crate::daemon_alive().map(|(i, _)| i).filter(holds_key) else {
            // A background service this launch did not start cannot be told the user's yes. When it is
            // idle it is replaced now (the window reconnects on its own); otherwise it has to finish first.
            let replaced = crate::ensure_daemon().is_ok_and(|(i, _)| holds_key(&i));
            return Err(match (replaced, ar) {
                (true, false) => "Thoughts restarted its background service so that it can confirm this. Try again.",
                (true, true) => "أعاد Thoughts تشغيل خدمة الخلفية ليتمكّن من تأكيد هذا. حاول مرة أخرى.",
                (false, false) => "Thoughts cannot confirm this while its background service finishes work it started before the app opened. Try again once that is done.",
                (false, true) => "لا يستطيع Thoughts تأكيد هذا ما دامت خدمة الخلفية تُنهي عملاً بدأته قبل فتح التطبيق. حاول مرة أخرى بعد انتهائه.",
            }
            .to_string());
        };
        let ask = ask_text(&learn(&i, what, &subject), ar);
        if crate::recovery::choose(&app, MessageDialogKind::Warning, ask.title, &ask.body, &[ask.yes, ask.no]) != 0 {
            return Err(DECLINED.to_string());
        }
        Ok(consent::mint(key(), what, &subject, consent::now_ms()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [fn() -> Subject; 6] = [
        || Subject::Chat { title: Some("Budget".into()), codex: false },
        || Subject::Chat { title: Some("Budget".into()), codex: true },
        || Subject::Chat { title: None, codex: false },
        || Subject::Default,
        || Subject::Folder(Some("/Users/me/Thoughts".into())),
        || Subject::Folder(None),
    ];

    #[test]
    fn every_confirmation_is_worded_in_both_languages() {
        for make in ALL {
            let (en, ar) = (ask_text(&make(), false), ask_text(&make(), true));
            for a in [&en, &ar] {
                assert!(!a.title.is_empty() && !a.body.is_empty() && !a.yes.is_empty() && !a.no.is_empty(), "{a:?}");
                assert_ne!(a.yes, a.no, "{a:?}");
            }
            let arabic = |s: &str| s.chars().any(|c| ('\u{0600}'..='\u{06ff}').contains(&c));
            assert!(arabic(ar.title) && arabic(&ar.body) && arabic(ar.yes) && arabic(ar.no), "{ar:?}");
            assert!(!arabic(en.title) && !arabic(&en.body) && !arabic(en.yes) && !arabic(en.no), "{en:?}");
        }
    }

    #[test]
    fn each_action_says_plainly_what_it_does_and_names_its_subject() {
        let chat = ask_text(&Subject::Chat { title: Some("Budget".into()), codex: false }, false);
        assert_eq!((chat.title, chat.yes), ("Bypass all permissions in this chat?", "Bypass permissions"));
        assert!(chat.body.contains("Budget") && chat.body.contains("Claude") && chat.body.contains("this chat"), "{}", chat.body);
        let codex = ask_text(&Subject::Chat { title: Some("Budget".into()), codex: true }, false);
        assert!(codex.title.contains("Codex") && codex.body.contains("no sandbox") && !codex.body.contains("Claude"), "{codex:?}");
        let default = ask_text(&Subject::Default, false);
        assert!(default.title.contains("new chats") && default.body.contains("default for new chats"), "{default:?}");
        let folder = ask_text(&Subject::Folder(Some("/Users/me/Thoughts".into())), true);
        assert_eq!((folder.yes, folder.no), ("أثق به", "لا أثق به"));
        assert!(folder.body.contains("/Users/me/Thoughts"), "{}", folder.body);
        // Without a name from the service, nothing is made up.
        assert!(ask_text(&Subject::Folder(None), false).body.starts_with("The folder shown in the app window."));
        assert!(ask_text(&Subject::Chat { title: Some("  ".into()), codex: false }, false).body.starts_with("This chat."));
    }

    #[test]
    fn a_name_cannot_break_the_sentence_around_it() {
        let title = "Line one\nline two\u{202e}\u{0007} and much more text than any dialog line should ever carry, repeated, repeated, repeated";
        let body = ask_text(&Subject::Chat { title: Some(title.into()), codex: false }, false).body;
        let first = body.lines().next().unwrap();
        assert!(first.starts_with("Chat: “\u{2068}Line one line two") && first.ends_with("\u{2069}”"), "{first}");
        assert!(first.contains('…') && !first.contains('\u{0007}') && !first.contains('\u{202e}'), "{first}");
    }

    #[test]
    fn only_one_confirmation_at_a_time() {
        let first = Asking::start().expect("nothing is on screen");
        assert!(Asking::start().is_none(), "a second question while one is open is refused");
        drop(first);
        assert!(Asking::start().is_some(), "the next one may be asked once the first is answered");
    }
}
