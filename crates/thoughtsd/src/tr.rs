//! Plain-language error messages in the UI language (English and Arabic).
//!
//! Every error a non-developer can plausibly meet has an entry in `TABLE`: a stable machine `code`,
//! an HTTP status, an optional machine-readable prefix the UI matches on (`BUSY::`, `STALE::`…) and
//! the sentence in both languages. Errors raised with `err` carry all of that (`UserErr`); anything
//! else that reaches the API is passed through `classify`, which recognises the well-known raw
//! messages (a missing folder, git asking for a password, no network…) and otherwise keeps the
//! English text behind a localised "something unexpected went wrong".

use std::sync::atomic::{AtomicBool, Ordering};

static ARABIC: AtomicBool = AtomicBool::new(false);

/// Remember the UI language (called whenever settings are loaded or saved).
pub fn set_language(lang: &str) {
    // Unit tests share one process: they always run in English and ask for Arabic explicitly.
    if cfg!(test) { return; }
    ARABIC.store(lang == "ar", Ordering::Relaxed);
}

pub fn arabic() -> bool { ARABIC.load(Ordering::Relaxed) }

/// An error meant for the person using the app: already worded for them, in their language.
#[derive(Debug, Clone)]
pub struct UserErr {
    /// Stable machine string (`busy`, `stale`, `not_found`…).
    pub code: &'static str,
    /// HTTP status the API answers with.
    pub status: u16,
    /// The full message, machine prefix included.
    pub message: String,
}

impl std::fmt::Display for UserErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.message) }
}
impl std::error::Error for UserErr {}

struct Entry { key: &'static str, code: &'static str, status: u16, prefix: &'static str, en: &'static str, ar: &'static str }

const fn e(key: &'static str, code: &'static str, status: u16, prefix: &'static str, en: &'static str, ar: &'static str) -> Entry {
    Entry { key, code, status, prefix, en, ar }
}

const TABLE: &[Entry] = &[
    // --- the library is in use ---
    e("busy", "busy", 423, "BUSY::",
        "Your library is being updated right now. Try again when that has finished.",
        "مكتبتك قيد التحديث الآن. حاول مرة أخرى بعد انتهاء ذلك."),
    e("busy.run", "busy", 423, "BUSY::",
        "The assistant is working in your library right now. Try again when it has finished.",
        "المساعد يعمل في مكتبتك الآن. حاول مرة أخرى بعد أن ينتهي."),
    e("busy.shutdown", "busy", 423, "BUSY::",
        "Your library is being updated right now. Wait for it to finish, or stop anyway.",
        "مكتبتك قيد التحديث الآن. انتظر حتى ينتهي ذلك، أو أوقف التطبيق على أي حال."),
    e("wrap.interrupted", "wrap_interrupted", 409, "WRAP_INTERRUPTED::", "{id}", "{id}"),
    e("chat.running", "busy", 409, "",
        "This chat is still working. Wait for it to finish or stop it first.",
        "هذه المحادثة ما زالت تعمل. انتظر حتى تنتهي أو أوقفها أولاً."),
    e("assistant.session_lost", "session_lost", 409, "",
        "The assistant could no longer open its earlier session for this conversation.",
        "لم يعد المساعد قادراً على فتح جلسته السابقة لهذه المحادثة."),
    e("chat.session_lost", "session_lost", 409, "",
        "The assistant's earlier session for this chat was no longer available, so it is continuing from the conversation saved here.",
        "جلسة المساعد السابقة لهذه المحادثة لم تعد متاحة، لذا يتابع من نصّ المحادثة المحفوظ هنا."),
    // --- the library itself ---
    e("library.none", "no_library", 409, "",
        "No library has been set up yet.",
        "لم تُجهَّز مكتبة بعد."),
    e("library.missing", "library_missing", 404, "",
        "Your library folder could not be found at {path}. If you moved or renamed it, choose it again in Settings.",
        "تعذّر العثور على مجلد مكتبتك في {path}. إن كنت نقلته أو غيّرت اسمه فاختره من جديد في الإعدادات."),
    e("library.no_folder", "validation", 422, "",
        "There is no folder at {path}. Choose a folder that exists.",
        "لا يوجد مجلد في {path}. اختر مجلداً موجوداً."),
    e("library.choose", "validation", 422, "",
        "Choose a library folder (a full path).",
        "اختر مجلداً لمكتبتك (بمساره الكامل)."),
    e("library.not_thoughts", "validation", 422, "",
        "{path} does not look like a Thoughts library. Choose the folder that holds your notes, or start a new library.",
        "المجلد {path} لا يبدو مكتبة «أفكار». اختر المجلد الذي يحوي ملاحظاتك، أو أنشئ مكتبة جديدة."),
    e("library.cloud", "validation", 422, "",
        "This library is inside a folder that a cloud drive already syncs (iCloud, Dropbox, OneDrive…). Backing it up to an online location as well would make the two fight over the same files. Choose \"Cloud folder\" as the backup, or move the library out of the cloud drive.",
        "هذه المكتبة داخل مجلد تُزامنه خدمة سحابية أصلاً (iCloud أو Dropbox أو OneDrive…). نسخها احتياطياً إلى موقع على الإنترنت أيضاً يجعل الطريقتين تتنازعان الملفات نفسها. اختر «مجلد سحابي» للنسخ الاحتياطي، أو انقل المكتبة خارج المجلد السحابي."),
    // --- files ---
    e("protected", "protected", 403, "",
        "This file is protected and cannot be opened or changed from the app.",
        "هذا الملف محميّ، ولا يمكن فتحه أو تغييره من التطبيق."),
    e("private", "private", 403, "",
        "{path} is in a private folder.",
        "«{path}» داخل مجلد خاص."),
    e("private.mention", "private", 403, "",
        "{path} is in a private folder and cannot be shared with the assistant.",
        "«{path}» داخل مجلد خاص، ولا يمكن مشاركته مع المساعد."),
    e("plugin.not_allowed", "forbidden", 403, "",
        "This add-on is not allowed to change {path}.",
        "هذه الإضافة غير مسموح لها بتغيير «{path}»."),
    e("search.no_addon", "validation", 422, "",
        "No smart-search add-on has been chosen, so there is nothing to install. Search already looks inside your notes and chats.",
        "لم تُحدَّد إضافة للبحث الذكي، فلا يوجد ما يُثبَّت. البحث يشمل أصلاً نصوص ملاحظاتك ومحادثاتك."),
    e("plugin.file_type", "validation", 422, "",
        "Add-ons can only save text files (.md, .json, .csv, .txt).",
        "الإضافات تحفظ الملفات النصية فقط (md و json و csv و txt)."),
    e("not_found.file", "not_found", 404, "",
        "This file could not be found. It may have been moved or deleted.",
        "تعذّر العثور على هذا الملف. ربما نُقل أو حُذف."),
    e("not_found.version", "not_found", 404, "",
        "That earlier version could not be found in the history.",
        "تعذّر العثور على تلك النسخة السابقة في السجلّ."),
    e("not_found.chat", "not_found", 404, "",
        "This chat no longer exists.",
        "هذه المحادثة لم تعد موجودة."),
    e("not_found.wrap", "not_found", 404, "",
        "This wrap no longer exists.",
        "عملية التلخيص والحفظ هذه لم تعد موجودة."),
    e("not_found.module", "not_found", 404, "",
        "There is no such feature.",
        "لا توجد ميزة بهذا الاسم."),
    e("not_found.project", "not_found", 404, "",
        "This project is no longer in your list.",
        "هذا المشروع لم يعد في قائمتك."),
    e("not_found.folder", "not_found", 404, "",
        "There is no folder at {path}.",
        "لا يوجد مجلد في {path}."),
    e("exists", "exists", 409, "",
        "There is already something named {path}. Choose another name.",
        "يوجد عنصر بالاسم «{path}». اختر اسماً آخر."),
    e("folder.not_empty", "validation", 422, "",
        "This folder still has files in it. Move or delete them first.",
        "ما زال في هذا المجلد ملفات. انقلها أو احذفها أولاً."),
    e("folder.protected", "protected", 403, "",
        "This folder holds files the app never changes (such as its own setup), so it cannot be deleted here.",
        "يحتوي هذا المجلد على ملفات لا يغيّرها التطبيق أبداً (مثل ملفات إعداده)، لذا لا يمكن حذفه من هنا."),
    e("invalid.path", "validation", 422, "",
        "That is not a valid name or location for a file.",
        "هذا ليس اسماً أو موقعاً صالحاً لملف."),
    e("too_large", "too_large", 413, "",
        "{name} is larger than 25 MB.",
        "حجم «{name}» يتجاوز 25 ميغابايت."),
    e("too_large.show", "too_large", 413, "",
        "This file is larger than 25 MB and cannot be shown in the app.",
        "حجم هذا الملف يتجاوز 25 ميغابايت، ولا يمكن عرضه في التطبيق."),
    e("chat.files_too_large", "too_large", 413, "",
        "The attached files are more than 50 MB together. Send some of them in a separate message.",
        "حجم الملفات المرفقة معاً يتجاوز 50 ميغابايت. أرسل بعضها في رسالة أخرى."),
    e("unreadable", "validation", 422, "",
        "{name} could not be read.",
        "تعذّرت قراءة «{name}»."),
    e("import.empty", "validation", 422, "",
        "No files were chosen.",
        "لم تُختر أي ملفات."),
    // --- saving over something that changed ---
    e("stale.changed", "stale", 409, "STALE::",
        "This file was changed somewhere else after you opened it. Reload it to see the latest version before saving.",
        "تغيّر هذا الملف في مكان آخر بعد أن فتحته. أعد تحميله لترى أحدث نسخة قبل الحفظ."),
    e("stale.gone", "stale", 409, "STALE::",
        "This file was moved or deleted after you opened it.",
        "نُقل هذا الملف أو حُذف بعد أن فتحته."),
    e("stale.todo", "stale", 409, "",
        "This task changed in the meantime. The list has been refreshed — try again.",
        "تغيّرت هذه المهمة في هذه الأثناء. حُدّثت القائمة، فحاول مرة أخرى."),
    e("todo.empty", "validation", 422, "",
        "A task cannot be empty.",
        "لا يمكن أن تكون المهمة فارغة."),
    e("todo.not_file", "validation", 422, "",
        "That is not a task list.",
        "هذه ليست قائمة مهام."),
    // --- history and sync ---
    e("conflict", "conflict", 409, "CONFLICT::", "{files}", "{files}"),
    e("no_history", "no_history", 409, "NO_HISTORY::",
        "The history of changes is switched off for this library. Turn on \"Keep a history of changes\" in Settings to use this.",
        "سجلّ التغييرات متوقف لهذه المكتبة. فعّل «الاحتفاظ بسجلّ للتغييرات» من الإعدادات لاستخدام هذه الخاصية."),
    e("history.bad_entry", "validation", 422, "",
        "That is not an entry in your history.",
        "هذا ليس عنصراً في سجلّك."),
    e("history.first", "conflict", 409, "",
        "This is where the history of your library begins, so there is nothing earlier to go back to.",
        "من هنا يبدأ سجلّ مكتبتك، فلا يوجد ما هو أقدم منه للرجوع إليه."),
    e("history.merge", "conflict", 409, "",
        "This entry combined changes from two devices, so it cannot be undone on its own. Restore the files you need from an earlier version instead.",
        "هذا العنصر جمع تغييرات من جهازين، فلا يمكن التراجع عنه وحده. استعِد الملفات التي تحتاجها من نسخة سابقة بدلاً من ذلك."),
    e("history.nothing", "conflict", 409, "",
        "There is nothing to undo here — it is already undone.",
        "لا شيء يُتراجع عنه هنا، فقد سبق التراجع عنه."),
    e("git.off", "sync_off", 409, "",
        "This library is not backed up to an online location. Choose \"Online location\" as the backup in Settings to use Sync.",
        "هذه المكتبة لا تُنسخ احتياطياً إلى موقع على الإنترنت. اختر «موقع على الإنترنت» للنسخ الاحتياطي من الإعدادات لاستخدام المزامنة."),
    e("git.missing", "git_missing", 409, "",
        "The free tool Git is not installed on this computer, so the history of changes and the online backup are not available yet.",
        "الأداة المجانية Git غير مثبّتة على هذا الجهاز، لذلك لا يتوفر سجلّ التغييرات ولا النسخ الاحتياطي على الإنترنت بعد."),
    e("git.timeout", "timeout", 504, "",
        "This took too long and was stopped. Check your internet connection and try again.",
        "استغرق هذا وقتاً طويلاً فأُوقف. تحقّق من اتصالك بالإنترنت وحاول مرة أخرى."),
    e("git.not_branch", "history_state", 409, "",
        "Your library's history is in an unusual state (it is not on a branch), so this cannot be done right now.",
        "سجلّ مكتبتك في حالة غير معتادة (ليس على فرع)، فلا يمكن تنفيذ هذا الآن."),
    e("git.internal_branch", "history_state", 409, "",
        "Your library is on \"{branch}\", which the app keeps for its own use and never syncs.",
        "مكتبتك على الفرع «{branch}» الذي يحتفظ به التطبيق لاستخدامه الخاص ولا يزامنه."),
    e("git.no_remote", "validation", 422, "",
        "No online location has been added for this library yet.",
        "لم يُضف بعد موقع على الإنترنت لهذه المكتبة."),
    e("git.strategy", "validation", 422, "",
        "Choose which version to keep: yours or the other one.",
        "اختر النسخة التي تريد الإبقاء عليها: نسختك أم الأخرى."),
    e("sign_in", "sign_in", 401, "",
        "Sign-in is needed. Check the username and password (or access token) of your online location in Settings.",
        "يلزم تسجيل الدخول. تحقّق من اسم المستخدم وكلمة المرور (أو رمز الدخول) لموقعك على الإنترنت في الإعدادات."),
    e("offline", "offline", 503, "",
        "Could not reach the internet. Check your connection and try again.",
        "تعذّر الاتصال بالإنترنت. تحقّق من اتصالك وحاول مرة أخرى."),
    e("invalid.remote_name", "validation", 422, "",
        "That name cannot be used. Use letters, digits, dots and dashes, up to 40 characters.",
        "لا يمكن استخدام هذا الاسم. استخدم حروفاً لاتينية وأرقاماً ونقاطاً وشرطات، حتى 40 خانة."),
    e("invalid.host", "validation", 422, "",
        "That is not a valid site name.",
        "هذا ليس اسم موقع صالحاً."),
    e("invalid.url", "validation", 422, "",
        "That is not a valid address for an online location. It must start with https://, ssh:// or git@.",
        "هذا ليس عنواناً صالحاً لموقع على الإنترنت. يجب أن يبدأ بـ https:// أو ssh:// أو git@."),
    e("invalid.credential", "validation", 422, "",
        "That username or password contains characters that cannot be used.",
        "اسم المستخدم أو كلمة المرور يحوي رموزاً لا يمكن استخدامها."),
    // --- settings ---
    e("newer_settings", "newer_settings", 409, "NEWER_SETTINGS::",
        "Your settings were saved by a newer version of Thoughts. Update the app to change them.",
        "حُفظت إعداداتك بإصدار أحدث من «أفكار». حدّث التطبيق لتتمكن من تغييرها."),
    e("login_item.translocated", "validation", 422, "",
        "Move Thoughts into your Applications folder and open it from there, then turn this on.",
        "انقل «أفكار» إلى مجلد التطبيقات وافتحه من هناك، ثم فعّل هذا الخيار."),
    e("login_item.other_home", "validation", 422, "",
        "This copy of Thoughts keeps its data in a separate folder, so it cannot change whether Thoughts starts when you log in. Change it in the app you normally use.",
        "تحفظ هذه النسخة من «أفكار» بياناتها في مجلد مستقل، لذا لا يمكنها تغيير تشغيل «أفكار» عند تسجيل الدخول. غيّر ذلك من التطبيق الذي تستخدمه عادةً."),
    e("module.always_on", "validation", 422, "",
        "This feature is the backbone of the app and stays on.",
        "هذه الميزة أساس التطبيق وتبقى مفعّلة."),
    // --- the assistant ---
    e("assistant.missing", "assistant_missing", 409, "",
        "The assistant (Claude Code) is not installed, or could not be found.",
        "المساعد (Claude Code) غير مثبّت، أو تعذّر العثور عليه."),
    e("assistant.signed_out", "signed_out", 409, "SIGNED_OUT::",
        "The assistant is not signed in to Claude, or the sign-in has expired. Sign in, then send your message again.",
        "المساعد غير مسجّل الدخول إلى Claude، أو انتهت صلاحية تسجيل الدخول. سجّل الدخول ثم أرسل رسالتك مرة أخرى."),
    e("provider.incomplete", "provider_incomplete", 409, "",
        "A service address is saved under Settings › Assistant without a key for it, so the assistant was not started. Add the key there, or remove the address to use your Claude account.",
        "هناك عنوان خدمة محفوظ في «الإعدادات › المساعد» دون مفتاح له، ولذلك لم يبدأ المساعد. أضف المفتاح هناك، أو أزل العنوان لتستخدم حسابك في Claude."),
    e("provider.key_needed", "validation", 422, "",
        "Enter the key for this service address. A key saved for one address is never sent to another.",
        "أدخل مفتاح هذا العنوان. المفتاح المحفوظ لعنوان لا يُرسل أبداً إلى عنوان آخر."),
    e("provider.key_unreadable", "provider_incomplete", 409, "",
        "The saved key could not be read from this computer's password store, so the assistant was not started. Save the key again under Settings › Assistant.",
        "تعذّرت قراءة المفتاح المحفوظ من خزنة كلمات المرور في جهازك، ولذلك لم يبدأ المساعد. احفظ المفتاح من جديد في «الإعدادات › المساعد»."),
    e("provider.key_refused", "provider_key", 409, "",
        "The service did not accept the saved key. Check the key and the service address under Settings › Assistant, then try again.",
        "لم تقبل الخدمة المفتاح المحفوظ. راجع المفتاح وعنوان الخدمة في «الإعدادات › المساعد» ثم حاول مرة أخرى."),
    e("bypass.unconfirmed", "confirm_needed", 409, "",
        "Bypassing permissions must be confirmed first. Choose it again and confirm the warning.",
        "تجاوز الأذونات يحتاج إلى تأكيدك أولاً. اختره مرة أخرى ووافق على التحذير."),
    e("bypass.chat_only", "forbidden", 403, "",
        "Bypassing permissions is only available in ordinary chats.",
        "تجاوز الأذونات متاح في المحادثات العادية فقط."),
    e("reorganize.untrusted", "forbidden", 403, "",
        "Your library's saved history contains assistant settings that differ from the ones in the folder ({files}). Reorganize was not started.",
        "السجلّ المحفوظ لمكتبتك يحوي إعدادات للمساعد تختلف عمّا في المجلد ({files}). لم تبدأ إعادة التنظيم."),
    e("wrap.nothing", "validation", 422, "",
        "There is nothing to wrap yet.",
        "لا يوجد ما يُلخَّص ويُحفظ بعد."),
    e("wrap.no_undo", "conflict", 409, "",
        "This wrap has nothing to undo.",
        "لا يوجد في هذا التلخيص ما يُتراجع عنه."),
    e("wrap.moved_on", "conflict", 409, "",
        "Your library has been saved or synced since this wrap was cut off, so it cannot be undone automatically.",
        "حُفظت مكتبتك أو زُومنت بعد انقطاع هذا التلخيص، فلا يمكن التراجع عنه تلقائياً."),
    // --- automatic tasks and routines ---
    e("workflow.gone", "not_found", 404, "",
        "This automatic task no longer exists. The list has been refreshed.",
        "لم تعد هذه المهمة التلقائية موجودة. حُدّثت القائمة."),
    e("workflow.changed", "stale", 409, "",
        "This automatic task was changed after you looked at it. Look at what it does now, then try again.",
        "تغيّرت هذه المهمة التلقائية بعد أن اطّلعت عليها. راجع ما تفعله الآن ثم حاول مرة أخرى."),
    e("workflow.busy", "busy", 409, "",
        "Another automatic task, a wrap or a reorganisation is running. Try again when it has finished.",
        "تعمل الآن مهمة تلقائية أخرى أو عملية تلخيص وحفظ أو إعادة تنظيم. حاول مرة أخرى بعد أن تنتهي."),
    e("workflow.limits", "validation", 422, "",
        "The time limit must be between 1 and 240 minutes, and the spending limit between $0.10 and $100.",
        "يجب أن يكون الحد الزمني بين دقيقة واحدة و240 دقيقة، وحد الإنفاق بين 0.10 و100 دولار."),
    e("workflow.time_limit", "limit", 409, "",
        "This automatic task was stopped because it ran longer than its time limit of {minutes} minutes.",
        "أُوقفت هذه المهمة التلقائية لأنها تجاوزت حدّها الزمني ({minutes} دقيقة)."),
    e("workflow.spend_limit", "limit", 409, "",
        "This automatic task stopped because it reached its spending limit of {amount}.",
        "توقفت هذه المهمة التلقائية لأنها بلغت حدّ الإنفاق المحدد لها ({amount})."),
    e("routine.changed", "stale", 409, "",
        "This routine was changed after you opened it. Open it again to see the latest version.",
        "تغيّر هذا الروتين بعد أن فتحته. افتحه مرة أخرى لترى أحدث نسخة."),
    e("routine.gone", "not_found", 404, "",
        "This routine no longer exists.",
        "لم يعد هذا الروتين موجوداً."),
    e("routine.empty", "validation", 422, "",
        "A routine needs a name, a line saying when to use it, and its steps.",
        "يحتاج الروتين إلى اسم، وسطر يوضح متى يُستخدم، وخطواته."),
    e("routine.too_long", "validation", 422, "",
        "This routine is too long. Keep the name under 80 characters, the \"when to use it\" line under 500 and the steps under 20,000.",
        "هذا الروتين طويل جداً. اجعل الاسم أقل من 80 حرفاً، وسطر «متى يُستخدم» أقل من 500 حرف، والخطوات أقل من 20,000 حرف."),
    e("routine.unsafe", "validation", 422, "",
        "Routines made in the app cannot run programs on this computer. Remove the lines that start with \"!\" and any text written as !`…`.",
        "لا يمكن للروتينات المُنشأة في التطبيق تشغيل برامج على هذا الجهاز. احذف الأسطر التي تبدأ بـ «!» وأي نص مكتوب على شكل !`…`."),
    e("routine.not_kept", "forbidden", 403, "",
        "The routine was not kept: saving it would have changed what the assistant is allowed to do. Nothing was changed.",
        "لم يُحفظ الروتين: كان حفظه سيغيّر ما يُسمح للمساعد بفعله. لم يتغيّر شيء."),
    e("routine.shipped", "forbidden", 403, "",
        "This routine comes with the app and cannot be removed. You can change its steps instead.",
        "هذا الروتين مرفق بالتطبيق ولا يمكن حذفه. يمكنك تعديل خطواته بدلاً من ذلك."),
    e("routine.draft_failed", "assistant", 502, "",
        "The assistant's draft could not be read. Try again, or write the routine yourself.",
        "تعذّرت قراءة المسودة التي كتبها المساعد. حاول مرة أخرى، أو اكتب الروتين بنفسك."),
    // --- anything else ---
    e("unexpected", "internal", 500, "",
        "Something unexpected went wrong.",
        "حدث خطأ غير متوقع."),
];

fn entry(key: &str) -> &'static Entry {
    TABLE.iter().find(|x| x.key == key).unwrap_or_else(|| TABLE.iter().find(|x| x.key == "unexpected").expect("the table has a fallback"))
}

fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in args { out = out.replace(&format!("{{{k}}}"), v); }
    out
}

/// The sentence for `key` in the given language, without its machine prefix.
pub fn text_in(ar: bool, key: &str, args: &[(&str, &str)]) -> String {
    let x = entry(key);
    fill(if ar { x.ar } else { x.en }, args)
}

/// The sentence for `key` in the UI language, without its machine prefix.
pub fn text(key: &str, args: &[(&str, &str)]) -> String { text_in(arabic(), key, args) }

pub fn user_err_in(ar: bool, key: &str, args: &[(&str, &str)]) -> UserErr {
    let x = entry(key);
    UserErr { code: x.code, status: x.status, message: format!("{}{}", x.prefix, fill(if ar { x.ar } else { x.en }, args)) }
}

/// A user-facing error in the UI language.
pub fn err(key: &str, args: &[(&str, &str)]) -> anyhow::Error { user_err_in(arabic(), key, args).into() }

/// Machine prefixes other parts of the daemon raise as plain strings, with their status and code.
const PREFIXES: &[(&str, u16, &str)] = &[
    ("BUSY::", 423, "busy"), ("WRAP_INTERRUPTED::", 409, "wrap_interrupted"), ("CONFLICT::", 409, "conflict"),
    ("REORG_CONFLICT::", 409, "conflict"), ("STALE::", 409, "stale"), ("UNTRUSTED::", 403, "untrusted"),
    ("NO_HISTORY::", 409, "no_history"), ("NEWER_SETTINGS::", 409, "newer_settings"), ("CHANGED::", 409, "changed"),
    ("SIGNED_OUT::", 409, "signed_out"),
];

fn has_arabic(s: &str) -> bool { s.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)) }

/// Raw messages (from git, the OS, older code paths) that have one plain meaning.
fn known_raw(msg: &str) -> Option<&'static str> {
    let m = msg.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    if has(&["the library is not on a branch"]) { return Some("git.not_branch"); }
    if has(&["no library configured", "no library"]) && m.len() < 40 { return Some("library.none"); }
    if has(&["no such chat"]) { return Some("not_found.chat"); }
    if has(&["claude code cli not found"]) { return Some("assistant.missing"); }
    if has(&["authentication failed", "could not read username", "could not read password", "terminal prompts disabled",
             "invalid username or password", "permission denied (publickey", "http 401", "error: 401", "error: 403",
             "requested url returned error: 403", "requested url returned error: 401"]) { return Some("sign_in"); }
    if has(&["could not resolve host", "couldn't resolve host", "network is unreachable", "failed to connect to",
             "connection timed out", "could not resolve hostname", "connection refused", "operation timed out",
             "no route to host", "temporary failure in name resolution"]) { return Some("offline"); }
    None
}

/// Turn any error that reaches the API into what the person should see: status, code, message.
pub fn classify_in(ar: bool, err: &anyhow::Error) -> UserErr {
    if let Some(u) = err.chain().find_map(|c| c.downcast_ref::<UserErr>()) { return u.clone(); }
    let msg = err.to_string();
    // Machine-readable errors keep their exact text (the UI parses it).
    for (prefix, status, code) in PREFIXES {
        if msg.starts_with(prefix) || msg.contains(&format!(" {prefix}")) {
            return UserErr { code, status: *status, message: msg };
        }
    }
    if let Some(key) = known_raw(&msg) { return user_err_in(ar, key, &[]); }
    // Bare operating-system and parser errors mean nothing to a person.
    let io = err.chain().find_map(|c| c.downcast_ref::<std::io::Error>());
    if let Some(io) = io {
        if let Some(u) = io.get_ref().and_then(|inner| inner.downcast_ref::<UserErr>()) { return u.clone(); }
        if io.kind() == std::io::ErrorKind::NotFound { return user_err_in(ar, "not_found.file", &[]); }
        if io.kind() == std::io::ErrorKind::TimedOut { return user_err_in(ar, "git.timeout", &[]); }
        return UserErr { code: "internal", status: 500, message: format!("{} ({msg})", text_in(ar, "unexpected", &[])) };
    }
    if msg.contains("(os error ") {
        if msg.contains("os error 2)") { return user_err_in(ar, "not_found.file", &[]); }
        return UserErr { code: "internal", status: 500, message: format!("{} ({msg})", text_in(ar, "unexpected", &[])) };
    }
    if err.chain().any(|c| c.downcast_ref::<serde_json::Error>().is_some()) {
        return UserErr { code: "internal", status: 500, message: format!("{} ({msg})", text_in(ar, "unexpected", &[])) };
    }
    // Everything else is a message some part of the app wrote for the person. Ones that are not
    // translated yet stay in English behind a sentence in the UI language.
    let message = if ar && !has_arabic(&msg) { format!("{} ({msg})", text_in(true, "unexpected", &[])) } else { msg };
    UserErr { code: "error", status: 400, message }
}

pub fn classify(err: &anyhow::Error) -> UserErr { classify_in(arabic(), err) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_exists_in_both_languages_with_the_same_placeholders() {
        let holes = |s: &str| { let mut v: Vec<String> = regex::Regex::new(r"\{[a-z]+\}").unwrap().find_iter(s).map(|m| m.as_str().to_string()).collect(); v.sort(); v };
        let mut seen = std::collections::HashSet::new();
        for x in TABLE {
            assert!(seen.insert(x.key), "duplicate key {}", x.key);
            assert!(!x.en.is_empty() && !x.ar.is_empty(), "{}", x.key);
            assert_eq!(holes(x.en), holes(x.ar), "{}", x.key);
            assert!((400..600).contains(&x.status), "{}", x.key);
            // Arabic copy uses the tanween-on-alif spelling throughout.
            assert!(!x.ar.contains("ًا"), "{}: write اً, not ًا", x.key);
        }
    }

    #[test]
    fn messages_are_localised_and_keep_their_machine_prefix() {
        let en = user_err_in(false, "busy", &[]);
        let ar = user_err_in(true, "busy", &[]);
        assert!(en.message.starts_with("BUSY::Your library") && ar.message.starts_with("BUSY::مكتبتك"));
        assert_eq!((en.status, en.code), (423, "busy"));
        assert_eq!(user_err_in(false, "wrap.interrupted", &[("id", "abc")]).message, "WRAP_INTERRUPTED::abc");
        let missing = user_err_in(true, "library.missing", &[("path", "/Users/x/Notes")]);
        assert!(missing.message.contains("/Users/x/Notes") && missing.status == 404);
        // An unknown key never panics.
        assert_eq!(user_err_in(false, "no.such.key", &[]).code, "internal");
    }

    #[test]
    fn raw_errors_become_plain_ones() {
        let c = |e: anyhow::Error, ar: bool| classify_in(ar, &e);
        // Typed errors pass through untouched, even with context added.
        let typed = c(anyhow::Error::from(user_err_in(false, "stale.gone", &[])).context("saving"), false);
        assert_eq!((typed.status, typed.code), (409, "stale"));
        // Machine prefixes keep their text and get a proper status.
        let u = c(anyhow::anyhow!("UNTRUSTED::/x::git hooks"), true);
        assert_eq!((u.status, u.code, u.message.as_str()), (403, "untrusted", "UNTRUSTED::/x::git hooks"));
        assert_eq!(c(anyhow::anyhow!("CONFLICT::a.md;b.md"), false).status, 409);
        assert_eq!(c(anyhow::anyhow!("CHANGED::the pending change is different now"), false).status, 409);
        // The operating system's own words never reach the person.
        let io = c(std::io::Error::from_raw_os_error(2).into(), false);
        assert_eq!((io.status, io.code), (404, "not_found"));
        assert!(!io.message.contains("os error"));
        let denied = c(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into(), true);
        assert_eq!(denied.status, 500);
        assert!(denied.message.starts_with("حدث خطأ غير متوقع."));
        // Git's sign-in and network failures.
        assert_eq!(c(anyhow::anyhow!("git push: fatal: Authentication failed for 'https://github.com/a/b.git/'"), false).code, "sign_in");
        assert_eq!(c(anyhow::anyhow!("pull failed: git pull: fatal: unable to access 'https://x/': Could not resolve host: x"), false).code, "offline");
        assert_eq!(c(anyhow::anyhow!("the library is not on a branch"), true).code, "history_state");
        assert_eq!(c(anyhow::anyhow!("no such chat"), false).status, 404);
        // An untranslated message stays readable: as is in English, behind a generic sentence in Arabic.
        assert_eq!(c(anyhow::anyhow!("give the project a name"), false).message, "give the project a name");
        assert_eq!(c(anyhow::anyhow!("give the project a name"), true).message, "حدث خطأ غير متوقع. (give the project a name)");
        assert_eq!(c(anyhow::anyhow!("هذه رسالة عربية"), true).message, "هذه رسالة عربية");
    }
}
