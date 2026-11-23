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
    /// One chat: its title and whether Codex answers in it.
    Chat { title: String, codex: bool },
    /// The default for new chats.
    Default,
    /// A folder to trust: its full path.
    Folder(String),
    /// Full access (`thoughtsd::full_access`): every limit the app adds, gone. Asked in two steps.
    FullAccess,
    /// A MADO plan's Final check: the project's name and the commands approving it lets the build run.
    MadoApprove { project: String, commands: Vec<String> },
    /// Switching off the confinement of a MADO build's commands.
    ConfineOff,
    /// Auto mode in one chat (its title): Claude Code's own classifier approves what it does.
    ChatAuto { title: String },
    /// Auto mode as the default for new chats.
    DefaultAuto,
}

/// How many commands the Final check's dialog lists before it says how many more there are.
pub(crate) const MADO_SHOWN: usize = 12;

/// The words of the native confirmation.
#[derive(Debug, PartialEq)]
pub(crate) struct Ask {
    pub title: &'static str,
    pub body: String,
    pub yes: &'static str,
    pub no: &'static str,
}

/// `text` with every character that could hide or reorder what is shown (control characters,
/// direction marks and overrides, invisible joiners) written out visibly as `<U+XXXX>` instead.
/// Nothing is dropped or merged, so what is shown is what the text really is.
fn visible(text: &str) -> String {
    let hidden = |c: char| {
        c.is_control()
            || matches!(c, '\u{00ad}' | '\u{061c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')
    };
    text.chars().map(|c| if hidden(c) { format!("<U+{:04X}>", c as u32) } else { c.to_string() }).collect()
}

/// `text` (already `visible`) cut from the middle when it is longer than `max` characters, so its
/// beginning and its end, where a path names the folder itself, always show.
fn middle_cut(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let tail = max / 2;
    let head = max - tail - 1;
    format!("{}…{}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>())
}

/// A name inside a sentence, kept apart from the text around it so it reads the right way in either
/// language.
fn named(text: &str, max: usize) -> String {
    format!("\u{2068}{}\u{2069}", middle_cut(&visible(text), max))
}

/// The folder's own name: the last part of `path`.
fn last_part(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().filter(|p| !p.is_empty()).unwrap_or(path)
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
    const AUTO_RISK: (&str, &str) = (
        "In auto mode Claude Code’s own safety check, not you, decides which commands and changes go ahead without asking. It can be wrong, and an instruction hidden in a file or web page Claude reads could lead it to run commands or change files you did not want.",
        "في الوضع التلقائي يقرّر فحص الأمان الخاص بـ Claude Code، لا أنت، أيّ الأوامر والتعديلات تمضي دون أن يستأذن. وقد يخطئ، وقد تدفعه تعليمات مخفية في ملف أو صفحة ويب يقرؤها Claude إلى تشغيل أوامر أو تعديل ملفات لم تُردها.",
    );
    let pick = |pair: (&'static str, &'static str)| if ar { pair.1 } else { pair.0 };
    let cancel = pick(("Cancel", "إلغاء"));
    match subject {
        Subject::Chat { title, codex } => {
            let (risk, note) = if *codex { (CODEX_RISK, CODEX_NOTE) } else { (CLAUDE_RISK, CLAUDE_NOTE) };
            let which = if ar { format!("المحادثة: «{}»", named(title, 160)) } else { format!("Chat: “{}”", named(title, 160)) };
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
            // The folder's own name on a line of its own, then the whole path (only a path longer
            // than any real one is cut, from the middle, so its end still shows).
            let (name, full) = (named(last_part(path), 300), named(path, 1000));
            let which = if ar { format!("المجلد: «{name}»\nمكانه: «{full}»") } else { format!("Folder: “{name}”\nLocation: “{full}”") };
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
        Subject::FullAccess => Ask {
            title: pick(("Give the assistant and every page full access to this computer?", "منح المساعد وكل صفحة وصولاً كاملاً إلى هذا الحاسوب؟")),
            body: pick((
                "With full access on, the assistant and any page or add-on in Thoughts can:\n\
                 • read, change or delete anything on this computer and in the app — every folder your account can reach, private folders and the app’s own data included;\n\
                 • run any program and use the network, the microphone, the camera, the screen and connected devices (Bluetooth, Wi-Fi and others);\n\
                 • change or remove any part of Thoughts, its Recovery menu and Rollback included, so there may be no way back.\n\n\
                 A mistake, or a malicious instruction hidden in a file, a note or a web page it reads, can cause real damage. Thoughts will no longer stop any of it; only what the assistant program itself refuses stays out of reach.\n\n\
                 You can turn it off at any time in Settings with one click.",
                "مع تفعيل الوصول الكامل يستطيع المساعد وأي صفحة أو إضافة في Thoughts أن:\n\
                 • يقرأ أي شيء على هذا الحاسوب وفي التطبيق أو يغيّره أو يحذفه — كل مجلد يصل إليه حسابك، بما فيها المجلدات الخاصة وبيانات التطبيق نفسه؛\n\
                 • يشغّل أي برنامج ويستخدم الشبكة والميكروفون والكاميرا والشاشة والأجهزة المتصلة (البلوتوث والواي فاي وغيرها)؛\n\
                 • يغيّر أي جزء من Thoughts أو يزيله، بما فيه قائمة الاسترداد والتراجع، فقد لا يبقى طريق للعودة.\n\n\
                 قد يُحدث خطأٌ، أو تعليمات خبيثة مخفية في ملف أو ملاحظة أو صفحة ويب يقرؤها، ضرراً حقيقياً. لن يوقف Thoughts أياً من ذلك بعد الآن؛ ولا يبقى بعيداً إلا ما يرفضه برنامج المساعد نفسه.\n\n\
                 يمكنك إيقافه في أي وقت من الإعدادات بنقرة واحدة.",
            ))
            .to_string(),
            yes: pick(("Continue…", "متابعة…")),
            no: cancel,
        },
        Subject::MadoApprove { project, commands } => {
            let which = if ar { format!("المشروع: «{}»", named(project, 160)) } else { format!("Project: “{}”", named(project, 160)) };
            let n = commands.len();
            let what = if ar {
                format!("باعتماد الفحص النهائي سيشغّل Thoughts هذه الأوامر ({n}) على هذا الحاسوب، في نسخة منفصلة من المشروع، لتهيئته والتحقق من كل مهمة. تشغّل هذه الأوامر شيفرة كتبها المساعد أثناء البناء (أمر الاختبار يشغّل اختبارات المشروع أياً كان ما فيها)، ويمكنها استخدام الشبكة.")
            } else {
                format!("Approving the Final check lets Thoughts run these {n} command(s) on this computer, in a separate copy of the project, to set it up and check each task. They run code the assistant wrote during the build (a test command runs the project's tests, whatever they contain), and they can use the network.")
            };
            let mut list: Vec<String> = commands.iter().take(MADO_SHOWN).map(|c| format!("• {}", named(c, 200))).collect();
            if n > MADO_SHOWN {
                let more = n - MADO_SHOWN;
                list.push(if ar { format!("و{more} غيرها — انظر القائمة في النافذة") } else { format!("and {more} more — see the list in the window") });
            }
            Ask {
                title: pick(("Approve the Final check and let the build run these commands?", "اعتماد الفحص النهائي والسماح للبناء بتشغيل هذه الأوامر؟")),
                body: format!("{which}\n\n{what}\n\n{}", list.join("\n")),
                yes: pick(("Approve and allow", "اعتمد واسمح")),
                no: cancel,
            }
        }
        Subject::ChatAuto { title } => {
            let which = if ar { format!("المحادثة: «{}»", named(title, 160)) } else { format!("Chat: “{}”", named(title, 160)) };
            Ask {
                title: pick(("Use auto mode in this chat?", "استخدام الوضع التلقائي في هذه المحادثة؟")),
                body: format!("{which}\n\n{}\n\n{}", pick(AUTO_RISK), pick((
                    "It applies to this chat until you change it back. Your private folders and the app’s own data stay blocked for Claude’s file tools.",
                    "يسري هذا على هذه المحادثة إلى أن تغيّره. وتبقى مجلداتك الخاصة وبيانات التطبيق نفسه محجوبة عن أدوات الملفات لدى Claude.",
                ))),
                yes: pick(("Use auto mode", "استخدم الوضع التلقائي")),
                no: cancel,
            }
        }
        Subject::DefaultAuto => Ask {
            title: pick(("Use auto mode in new chats?", "استخدام الوضع التلقائي في المحادثات الجديدة؟")),
            body: format!("{}\n\n{}", pick(AUTO_RISK), pick((
                "This becomes the default for new chats you start yourself. Chats started for you by an add-on or a shortcut chip use Accept edits.",
                "سيصبح هذا هو الوضع الافتراضي للمحادثات التي تبدؤها بنفسك. أما المحادثات التي تبدؤها إضافة أو زر اختصار فتستخدم «قبول التعديلات».",
            ))),
            yes: pick(("Use auto mode", "استخدم الوضع التلقائي")),
            no: cancel,
        },
        Subject::ConfineOff => Ask {
            title: pick(("Let the build's commands change files outside its copy of the project?", "السماح لأوامر البناء بتغيير ملفات خارج نسختها من المشروع؟")),
            body: pick((
                "With this off, the commands a build runs (installing packages, running tests) run as you, with nothing to stop them: they can read, change or delete files anywhere your account can reach, read your saved sign-ins, and reach other programs on this computer. They run code the assistant wrote during the build. Switch it off only for a project that needs it (for example to install from a private package registry with your saved credentials), and back on afterwards.",
                "مع إيقاف هذا تعمل الأوامر التي يشغّلها البناء (تثبيت الحزم وتشغيل الاختبارات) باسمك دون ما يوقفها: يمكنها قراءة الملفات أو تغييرها أو حذفها في أي مكان يصل إليه حسابك، وقراءة بيانات الدخول المحفوظة، والوصول إلى برامج أخرى على هذا الحاسوب. وهي تشغّل شيفرة كتبها المساعد أثناء البناء. أوقفه فقط لمشروع يحتاج ذلك (مثلاً للتثبيت من مستودع حزم خاص ببيانات دخولك المحفوظة)، ثم أعده بعدها.",
            ))
            .to_string(),
            yes: pick(("Switch it off", "أوقفه")),
            no: cancel,
        },
    }
}

/// The second, last step before full access is switched on. Its first (default) button keeps it
/// off, so pressing Enter twice never switches it on: the person has to choose the second button.
/// The button of `full_access_last_step` that switches it on: neither the first (default) nor the
/// last (cancel role).
pub(crate) const FULL_ACCESS_YES: usize = 1;

pub(crate) fn full_access_last_step(ar: bool) -> (&'static str, &'static str, [&'static str; 3]) {
    let pick = |pair: (&'static str, &'static str)| if ar { pair.1 } else { pair.0 };
    (
        pick(("Last step: switch full access on?", "الخطوة الأخيرة: تفعيل الوصول الكامل؟")),
        pick((
            "Thoughts will stop protecting this computer and itself from the assistant and from pages until you turn full access off again. Choose “Turn on full access” only if you have read the warning and accept the risk.",
            "سيتوقف Thoughts عن حماية هذا الحاسوب وحماية نفسه من المساعد ومن الصفحات إلى أن توقف الوصول الكامل من جديد. اختر «فعّل الوصول الكامل» فقط إن قرأت التحذير وقبلت المخاطرة.",
        )),
        [pick(("Keep it off", "أبقِه متوقفاً")), pick(("Turn on full access", "فعّل الوصول الكامل")), pick(("Cancel", "إلغاء"))],
    )
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
        .ok()
        // A chat with a long history can be large: read it as it comes, with no size cap.
        .and_then(|r| serde_json::from_reader(r.into_reader()).ok())
}

/// What the dialog names for `subject` of `what`, asked of the service, and what the approval is
/// signed over (`thoughtsd::consent::chat_subject` / `trust_subject`): the chat's title and the
/// program that answers in it, the folder's path. Nothing here comes from the window, which only
/// sent the id. `None` when the service cannot say: then nothing is asked and nothing approved.
fn learn(i: &DaemonInfo, what: Action, subject: &str) -> Option<(Subject, String)> {
    match what {
        Action::ChatBypass => {
            let chat = get(i, &format!("/chats/{subject}"))?;
            if chat["id"] != subject {
                return None;
            }
            let provider = chat["provider"].as_str()?;
            let title = chat["title"].as_str().unwrap_or("").to_string();
            Some((Subject::Chat { title, codex: provider == "codex" }, consent::chat_subject(subject, provider)))
        }
        Action::ChatAuto => {
            let chat = get(i, &format!("/chats/{subject}"))?;
            if chat["id"] != subject {
                return None;
            }
            let provider = chat["provider"].as_str()?;
            Some((Subject::ChatAuto { title: chat["title"].as_str().unwrap_or("").to_string() }, consent::chat_subject(subject, provider)))
        }
        Action::SettingsBypass => Some((Subject::Default, String::new())),
        Action::SettingsAuto => Some((Subject::DefaultAuto, String::new())),
        Action::FullAccess => Some((Subject::FullAccess, String::new())),
        Action::ConfineOff => Some((Subject::ConfineOff, String::new())),
        Action::MadoApprove => {
            // The project and the plan the window named; the name, the commands and the plan's hash come from the service.
            let (pid, hash) = consent::mado_parts(subject)?;
            let name = get(i, &format!("/mado/{pid}"))?["project"]["name"].as_str().unwrap_or("").to_string();
            let review = get(i, &format!("/mado/{pid}/review"))?;
            if review["planHash"].as_str()? != hash {
                return None;
            }
            let commands: Vec<String> = review["commands"].as_array()?.iter().filter_map(|c| c.as_str().map(String::from)).collect();
            Some((Subject::MadoApprove { project: name, commands }, consent::mado_subject(pid, hash)))
        }
        Action::Trust => {
            let path = get(i, &format!("/library/trust?id={subject}"))?["path"].as_str()?.to_string();
            Some((Subject::Folder(path.clone()), consent::trust_subject(subject, &path)))
        }
    }
}

/// Why no approval was made when the service could not say what it is for.
fn unknown(what: Action, ar: bool) -> String {
    match (what, ar) {
        (Action::Trust, false) => "Thoughts could not find the folder this question is about, so nothing was trusted. Try again.",
        (Action::Trust, true) => "تعذّر على Thoughts العثور على المجلد الذي يخصّه هذا السؤال، فلم يُمنح أي ثقة. حاول مرة أخرى.",
        (Action::MadoApprove, false) => "The plan changed, or Thoughts could not read it, so nothing was approved. Look at the Final check again and try again.",
        (Action::MadoApprove, true) => "تغيّرت الخطة أو تعذّر على Thoughts قراءتها، فلم يُعتمد شيء. راجع الفحص النهائي من جديد ثم حاول مرة أخرى.",
        (_, false) => "Thoughts could not find this chat to confirm the change, so nothing was changed. Try again.",
        (_, true) => "تعذّر على Thoughts العثور على هذه المحادثة لتأكيد التغيير، فلم يتغيّر شيء. حاول مرة أخرى.",
    }
    .to_string()
}

/// `approve_action(action, subject)` → an approval for exactly that action, which the window sends
/// with the request it confirms. `action`: "chat-bypass" or "chat-auto" (subject: the chat id),
/// "settings-bypass" or "settings-auto" (no subject), "trust" (subject: the trust question id), …. The app window only. The shell asks the
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
        let Some((shown, signed)) = learn(&i, what, &subject) else { return Err(unknown(what, ar)) };
        let ask = ask_text(&shown, ar);
        // Only a press of the yes button is a yes: Cancel, Esc and closing the dialog never are.
        if crate::recovery::choose_exact(&app, MessageDialogKind::Warning, ask.title, &ask.body, &[ask.yes, ask.no]) != Some(0) {
            return Err(DECLINED.to_string());
        }
        // Full access takes a second, deliberate choice: the default (Enter) button keeps it off,
        // the cancel-role (Esc) button cancels, and only the middle one, pressed, switches it on.
        if what == Action::FullAccess {
            let (title, body, buttons) = full_access_last_step(ar);
            if crate::recovery::choose_exact(&app, MessageDialogKind::Warning, title, body, &buttons) != Some(FULL_ACCESS_YES) {
                return Err(DECLINED.to_string());
            }
        }
        Ok(consent::mint(key(), what, &signed, consent::now_ms()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [fn() -> Subject; 9] = [
        || Subject::ChatAuto { title: "Budget".into() },
        || Subject::DefaultAuto,
        || Subject::MadoApprove { project: "Shop".into(), commands: vec!["npm ci".into(), "npm test".into()] },
        || Subject::ConfineOff,
        || Subject::FullAccess,
        || Subject::Chat { title: "Budget".into(), codex: false },
        || Subject::Chat { title: "Budget".into(), codex: true },
        || Subject::Default,
        || Subject::Folder("/Users/me/Thoughts".into()),
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
        for what in Action::ALL {
            assert_ne!(unknown(what, false), unknown(what, true));
        }
    }

    #[test]
    fn each_action_says_plainly_what_it_does_and_names_its_subject() {
        let chat = ask_text(&Subject::Chat { title: "Budget".into(), codex: false }, false);
        assert_eq!((chat.title, chat.yes), ("Bypass all permissions in this chat?", "Bypass permissions"));
        assert!(chat.body.contains("Budget") && chat.body.contains("Claude") && chat.body.contains("this chat"), "{}", chat.body);
        let codex = ask_text(&Subject::Chat { title: "Budget".into(), codex: true }, false);
        assert!(codex.title.contains("Codex") && codex.body.contains("no sandbox") && !codex.body.contains("Claude"), "{codex:?}");
        let default = ask_text(&Subject::Default, false);
        assert!(default.title.contains("new chats") && default.body.contains("default for new chats"), "{default:?}");
        let auto = ask_text(&Subject::ChatAuto { title: "Budget".into() }, false);
        assert!(auto.title.contains("auto mode") && auto.body.contains("Budget") && auto.body.contains("this chat"), "{auto:?}");
        assert!(ask_text(&Subject::DefaultAuto, false).body.contains("default for new chats"));
        let folder = ask_text(&Subject::Folder("/Users/me/Thoughts".into()), true);
        assert_eq!((folder.yes, folder.no), ("أثق به", "لا أثق به"));
        assert!(folder.body.contains("/Users/me/Thoughts"), "{}", folder.body);
    }

    #[test]
    fn a_long_path_keeps_its_end_and_the_folder_has_its_own_line() {
        let deep = format!("/Users/me/{}Downloads/evil-repo", "a/".repeat(600));
        let body = ask_text(&Subject::Folder(deep.clone()), false).body;
        let mut lines = body.lines();
        assert_eq!(lines.next().unwrap(), "Folder: “\u{2068}evil-repo\u{2069}”");
        let location = lines.next().unwrap();
        assert!(location.starts_with("Location: “\u{2068}/Users/me/a/") && location.ends_with("a/Downloads/evil-repo\u{2069}”"), "{location}");
        assert!(location.contains('…'));
        // An ordinary path is shown whole.
        let body = ask_text(&Subject::Folder("/Users/me/Projects/Thoughts/".into()), false).body;
        assert!(body.starts_with("Folder: “\u{2068}Thoughts\u{2069}”\nLocation: “\u{2068}/Users/me/Projects/Thoughts/\u{2069}”"), "{body}");
    }

    #[test]
    fn hidden_characters_are_shown_not_removed() {
        // A path that would read as ".../gpj.exe" reversed, a line break, a zero-width space.
        let path = "/Users/me/inv\u{202e}gpj.exe\nfake line\u{200b}x";
        let body = ask_text(&Subject::Folder(path.into()), false).body;
        assert!(body.contains("inv<U+202E>gpj.exe<U+000A>fake line<U+200B>x"), "{body}");
        assert!(!body.contains('\u{202e}') && !body.contains('\u{200b}'), "{body}");
        let title = "Line one\nline  two\u{0007}";
        let body = ask_text(&Subject::Chat { title: title.into(), codex: false }, false).body;
        assert!(body.lines().next().unwrap().contains("Line one<U+000A>line  two<U+0007>"), "{body}");
        // A very long title is cut from the middle: both ends stay.
        let long = format!("start {} the end", "x".repeat(400));
        let first = ask_text(&Subject::Chat { title: long, codex: false }, false).body.lines().next().unwrap().to_string();
        assert!(first.contains("start xx") && first.ends_with("x the end\u{2069}”") && first.contains('…'), "{first}");
    }

    #[test]
    fn full_access_says_what_it_allows_and_takes_a_second_deliberate_choice() {
        for ar in [false, true] {
            let a = ask_text(&Subject::FullAccess, ar);
            let (_, body, buttons) = full_access_last_step(ar);
            assert!(!body.is_empty() && buttons[0] != buttons[1] && buttons[1] != buttons[2]);
            if !ar {
                for word in ["read, change or delete", "network", "microphone", "camera", "screen", "devices", "Recovery", "Rollback", "real damage", "no longer stop"] {
                    assert!(a.body.contains(word), "{word}: {}", a.body);
                }
                // The default (first) button keeps it off, the last one cancels, the middle one switches it on.
                assert_eq!(buttons, ["Keep it off", "Turn on full access", "Cancel"]);
            }
            // Only that button, pressed, is a yes: Enter (the default), Esc, closing or anything unknown never is.
            use tauri_plugin_dialog::MessageDialogResult as M;
            let yes = |r: M| crate::recovery::exact_choice(r, &buttons) == Some(FULL_ACCESS_YES);
            assert!(yes(M::Custom(buttons[FULL_ACCESS_YES].to_string())));
            for no in [M::Ok, M::Yes, M::No, M::Cancel, M::Custom(buttons[0].into()), M::Custom(buttons[2].into()), M::Custom("something else".into())] {
                assert!(!yes(no), "{ar}");
            }
        }
    }

    /// Dismissing any consent dialog is never a yes (a two-button dialog's second button has the
    /// cancel role, which `choose` maps to it).
    #[test]
    fn dismissing_a_consent_dialog_is_never_an_answer() {
        use tauri_plugin_dialog::MessageDialogResult as M;
        let two = ["Bypass permissions", "Cancel"];
        assert_eq!(crate::recovery::exact_choice(M::Custom("Bypass permissions".into()), &two), Some(0));
        assert_eq!(crate::recovery::exact_choice(M::Ok, &two), Some(0));
        for gone in [M::Cancel, M::No, M::Custom("".into())] { assert_eq!(crate::recovery::exact_choice(gone, &two), None); }
        assert_eq!(crate::recovery::exact_choice(M::Custom("Cancel".into()), &two), Some(1), "a press of Cancel is Cancel, never a yes");
    }

    #[test]
    fn the_final_check_names_the_project_and_lists_its_commands_plainly() {
        let commands: Vec<String> = (1..=15).map(|i| format!("npm run check-{i}")).chain(["npm test \u{202e}hsab".to_string()]).collect();
        for ar in [false, true] {
            let a = ask_text(&Subject::MadoApprove { project: "Shop\u{202e}".into(), commands: commands.clone() }, ar);
            assert!(a.body.contains("Shop<U+202E>"), "{}", a.body);
            assert!(a.body.contains("npm run check-1\u{2069}") && a.body.contains("npm run check-12\u{2069}") && !a.body.contains("check-13"), "{}", a.body);
            if !ar {
                assert!(a.body.contains("and 4 more — see the list in the window"), "{}", a.body);
                for w in ["16 command", "separate copy", "code the assistant wrote", "network"] { assert!(a.body.contains(w), "{w}: {}", a.body); }
            } else {
                assert!(a.body.contains("و4 غيرها"), "{}", a.body);
            }
        }
        let few = ask_text(&Subject::MadoApprove { project: "Shop".into(), commands: vec!["npm test".into()] }, false);
        assert!(!few.body.contains("more —"), "{}", few.body);
        let off = ask_text(&Subject::ConfineOff, false);
        assert!(off.body.contains("saved sign-ins") && off.body.contains("private package registry"), "{}", off.body);
    }

    #[test]
    fn only_one_confirmation_at_a_time() {
        let first = Asking::start().expect("nothing is on screen");
        assert!(Asking::start().is_none(), "a second question while one is open is refused");
        drop(first);
        assert!(Asking::start().is_some(), "the next one may be asked once the first is answered");
    }
}
