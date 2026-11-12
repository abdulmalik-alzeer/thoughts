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
    e("busy.sync", "busy", 423, "BUSY::",
        "A sync is still putting changes into your library. Try again in a moment.",
        "ما زالت المزامنة تُدخل تغييرات إلى مكتبتك. حاول مرة أخرى بعد لحظات."),
    e("busy.shutdown", "busy", 423, "BUSY::",
        "Your library is being updated right now. Wait for it to finish, or stop anyway.",
        "مكتبتك قيد التحديث الآن. انتظر حتى ينتهي ذلك، أو أوقف التطبيق على أي حال."),
    e("wrap.interrupted", "wrap_interrupted", 409, "WRAP_INTERRUPTED::", "{id}", "{id}"),
    e("chat.running", "busy", 409, "",
        "This chat is still working. Wait for it to finish or stop it first.",
        "هذه المحادثة ما زالت تعمل. انتظر حتى تنتهي أو أوقفها أولاً."),
    e("chat.retention_range", "invalid", 400, "",
        "Choose Off, or a number of days of at least {min}.",
        "اختر «إيقاف» أو عدداً من الأيام لا يقلّ عن {min}."),
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
    e("library.exists", "library_exists", 422, "",
        "There is already a library in {path}. To open it, go back and choose \"I already have one\". To start a new one, choose another folder.",
        "توجد مكتبة في {path} من قبل. لفتحها ارجع واختر «لديّ مكتبة سابقة»، ولبدء مكتبة جديدة اختر مجلداً آخر."),
    e("library.not_empty", "validation", 422, "",
        "{path} already has other files in it. Choose an empty folder, or type a new folder name.",
        "المجلد {path} فيه ملفات أخرى. اختر مجلداً فارغاً، أو اكتب اسم مجلد جديد."),
    e("library.not_folder", "validation", 422, "",
        "{path} is a file, not a folder. Choose a folder.",
        "{path} ملف وليس مجلداً. اختر مجلداً."),
    e("setup.done", "validation", 409, "",
        "This library is already set up. Start a chat to carry on.",
        "هذه المكتبة مجهّزة من قبل. ابدأ محادثة للمتابعة."),
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
    e("plugin.instructions", "forbidden", 403, "",
        "Add-ons cannot create or change the files that give the assistant its instructions.",
        "لا يمكن للإضافات إنشاء الملفات التي تحمل تعليمات المساعد ولا تغييرها."),
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
    e("not_found.draft", "not_found", 404, "",
        "That change is no longer waiting for review. It may have been kept or discarded already.",
        "لم يعد هذا التغيير بانتظار المراجعة. ربما اعتُمد أو تُجوهل من قبل."),
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
    e("not_found.item", "not_found", 404, "",
        "This page could not be found in your library. It may have been moved, renamed or deleted.",
        "تعذّر العثور على هذه الصفحة في مكتبتك. ربما نُقلت أو غُيّر اسمها أو حُذفت."),
    e("meeting.date", "validation", 422, "",
        "That is not a valid date for the meeting.",
        "هذا ليس تاريخاً صالحاً للاجتماع."),
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
    e("picture.type", "validation", 422, "",
        "Only pictures in the usual formats (PNG, JPEG, GIF or WebP) can be added to a note.",
        "لا تُضاف إلى الملاحظة إلا الصور بصيغها المعتادة (PNG أو JPEG أو GIF أو WebP)."),
    e("picture.too_large", "too_large", 413, "",
        "This picture is larger than 15 MB, too large to add to a note.",
        "حجم هذه الصورة يتجاوز 15 ميغابايت، وهو أكبر من أن تُضاف إلى ملاحظة."),
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
    e("todo.restore_invalid", "validation", 422, "",
        "This task could not be put back.",
        "تعذّرت إعادة هذه المهمة."),
    e("capture.kind", "validation", 422, "",
        "Only a todo or a note can be saved this way.",
        "يمكن حفظ مهمة أو ملاحظة فقط بهذه الطريقة."),
    // --- access keys for other programs (sec-b, FEAT-37) ---
    e("key.not_allowed", "forbidden", 403, "",
        "This key is not allowed to do that. Each key can only do what was chosen when it was made.",
        "هذا المفتاح غير مسموح له بذلك. كل مفتاح لا يستطيع إلا ما اختير له عند إنشائه."),
    e("key.name", "validation", 422, "",
        "Give the key a name of up to 60 characters, so you know later where you used it.",
        "أعطِ المفتاح اسماً لا يتجاوز 60 حرفاً، لتعرف لاحقاً أين استخدمته."),
    e("key.name_taken", "validation", 422, "",
        "You already have a key with this name. Choose another name.",
        "لديك مفتاح بهذا الاسم من قبل. اختر اسماً آخر."),
    e("key.scopes", "validation", 422, "",
        "Choose what the key may do.",
        "اختر ما يُسمح للمفتاح بفعله."),
    e("key.too_many", "validation", 422, "",
        "You already have {n} keys, the most there can be. Remove one you no longer use first.",
        "لديك {n} مفتاحاً، وهو الحد الأقصى. احذف أولاً مفتاحاً لم تعد تستخدمه."),
    e("key.gone", "not_found", 404, "",
        "This key no longer exists.",
        "لم يعد هذا المفتاح موجوداً."),
    // --- history and sync ---
    e("conflict", "conflict", 409, "CONFLICT::", "{files}", "{files}"),
    e("no_history", "no_history", 409, "NO_HISTORY::",
        "The history of changes is switched off for this library. Turn on \"Keep a history of changes on this computer\" in Settings to use this.",
        "سجلّ التغييرات متوقف لهذه المكتبة. فعّل «الاحتفاظ بسجلّ للتغييرات على هذا الجهاز» من الإعدادات لاستخدام هذه الخاصية."),
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
        "Sync sends your library to an online location or to a drive, and this library has neither as its backup. Choose \"Online location\" or \"Drive or folder\" as the backup in Settings to use Sync.",
        "ترسل المزامنة مكتبتك إلى موقع على الإنترنت أو إلى قرص، ولم يُختر أيٌّ منهما للنسخ الاحتياطي لهذه المكتبة. اختر «موقع على الإنترنت» أو «قرص أو مجلد» للنسخ الاحتياطي من الإعدادات لاستخدام المزامنة."),
    e("git.missing", "git_missing", 409, "",
        "The free tool Git is not installed on this computer, so the history of changes and backing up to an online location or a drive are not available yet.",
        "الأداة المجانية Git غير مثبّتة على هذا الجهاز، لذلك لا يتوفر بعدُ سجلّ التغييرات ولا النسخ الاحتياطي إلى موقع على الإنترنت أو إلى قرص."),
    e("git.timeout", "timeout", 504, "",
        "This took too long and was stopped. Check your internet connection and try again.",
        "استغرق هذا وقتاً طويلاً فأُوقف. تحقّق من اتصالك بالإنترنت وحاول مرة أخرى."),
    e("git.not_branch", "history_state", 409, "",
        "Your library's history is in an unusual state (it is not on a branch), so this cannot be done right now. Use \"Repair history\" to put it right; nothing is lost.",
        "سجلّ مكتبتك في حالة غير معتادة (ليس على فرع)، فلا يمكن تنفيذ هذا الآن. استخدم «إصلاح السجلّ» لإعادته إلى وضعه الصحيح، ولن يضيع شيء."),
    // A history the app cannot work in (see `repair`).
    e("history.unfinished", "history_state", 409, "",
        "Your library's history stopped halfway through combining changes, so nothing new can be recorded until that is settled. Use \"Repair history\" to put it right; nothing is lost.",
        "توقف سجلّ مكتبتك في منتصف دمج تغييرات، فلا يمكن تسجيل شيء جديد حتى تُحسم هذه الخطوة. استخدم «إصلاح السجلّ» لإعادته إلى وضعه الصحيح، ولن يضيع شيء."),
    e("history.locked", "history_state", 409, "",
        "Your library's history is locked by another program, so changes cannot be recorded right now. If this does not clear up within a few minutes, use \"Repair history\".",
        "سجلّ مكتبتك مقفل من برنامج آخر، فلا يمكن تسجيل التغييرات الآن. إن لم يزُل ذلك خلال دقائق فاستخدم «إصلاح السجلّ»."),
    e("history.in_use", "history_state", 409, "",
        "Another program is using your library's history right now. Wait a few minutes, then try again. Nothing was changed.",
        "يستخدم برنامج آخر سجلّ مكتبتك الآن. انتظر بضع دقائق ثم حاول مرة أخرى. لم يتغيّر شيء."),
    e("history.damaged", "history_state", 409, "",
        "Part of your library's history cannot be read: it is damaged. Your notes themselves are fine. Use \"Repair history\" to put it right.",
        "تعذّرت قراءة جزء من سجلّ مكتبتك لأنه تالف. ملاحظاتك نفسها سليمة. استخدم «إصلاح السجلّ» لإعادته إلى وضعه الصحيح."),
    e("repair.offline", "offline", 503, "",
        "To start the history again from your backup location, the location must be reachable. Check your internet connection, or plug the drive in, and try again. Nothing was changed.",
        "لبدء السجلّ من جديد انطلاقاً من موقع نسخك الاحتياطي، يجب أن يكون الوصول إليه ممكناً. تحقّق من اتصالك بالإنترنت أو صِل القرص، ثم حاول مرة أخرى. لم يتغيّر شيء."),
    e("repair.cannot", "history_state", 409, "",
        "This library's history is kept in a way the app cannot repair. Nothing was changed.",
        "سجلّ هذه المكتبة محفوظ بطريقة لا يستطيع التطبيق إصلاحها. لم يتغيّر شيء."),
    // The name part of the second copy kept when both versions of a clashing document are kept.
    e("sync.other_copy", "validation", 422, "",
        "other device",
        "الجهاز الآخر"),
    e("git.internal_branch", "history_state", 409, "",
        "Your library is on \"{branch}\", which the app keeps for its own use and never syncs.",
        "مكتبتك على الفرع «{branch}» الذي يحتفظ به التطبيق لاستخدامه الخاص ولا يزامنه."),
    e("git.no_remote", "validation", 422, "",
        "No backup location has been added for this library yet.",
        "لم يُضف بعدُ أي موقع للنسخ الاحتياطي لهذه المكتبة."),
    e("sync.no_location", "no_location", 409, "",
        "Nothing was backed up: no backup location has been added for this library yet. Add an online location or a drive in Settings.",
        "لم يُنسخ شيء احتياطياً: لم يُضف بعدُ أي موقع للنسخ الاحتياطي لهذه المكتبة. أضف موقعاً على الإنترنت أو قرصاً من الإعدادات."),
    // The pieces of a sync that did not reach every location (see `git::sync_outcome`).
    e("sync.sent_to", "sync_failed", 409, "",
        "Your library was sent to {names}.",
        "أُرسلت مكتبتك إلى {names}."),
    e("sync.not_sent_to", "sync_failed", 409, "",
        "It was not sent to \"{name}\": {why}",
        "لم تُرسَل إلى «{name}»: {why}"),
    e("sync.why.sign_in", "sync_failed", 409, "",
        "sign-in is needed. Its saved password or access token may have expired: choose \"Update sign-in\" beside it under Settings › Library & backup.",
        "يلزم تسجيل الدخول. ربما انتهت صلاحية كلمة المرور أو رمز الدخول المحفوظ له: اختر «تحديث الدخول» بجانبه في الإعدادات › المكتبة والنسخ الاحتياطي."),
    e("sync.why.offline", "sync_failed", 409, "",
        "it could not be reached. Check your internet connection and try again.",
        "تعذّر الوصول إليه. تحقّق من اتصالك بالإنترنت وحاول مرة أخرى."),
    e("sync.why.timeout", "sync_failed", 409, "",
        "it took too long and was stopped. Check your internet connection and try again.",
        "استغرق الإرسال وقتاً طويلاً فأُوقف. تحقّق من اتصالك بالإنترنت وحاول مرة أخرى."),
    e("sync.why.rejected", "sync_failed", 409, "",
        "it holds changes that this computer does not have, so it did not accept yours.",
        "فيه تغييرات ليست على هذا الجهاز، فلم يقبل تغييراتك."),
    e("sync.why.too_large", "sync_failed", 409, "",
        "a file in your library is larger than it accepts.",
        "في مكتبتك ملف أكبر مما يقبله."),
    e("sync.why.refused", "sync_failed", 409, "",
        "it did not accept the changes.",
        "لم يقبل التغييرات."),
    e("sync.why.unplugged", "sync_failed", 409, "",
        "the drive or folder is not connected. Plug in the drive (or connect to the network folder) and press Sync again. If the backup was moved, remove this location and add it again where it is now.",
        "القرص أو المجلد غير متصل. صِل القرص (أو اتصل بالمجلد على الشبكة) ثم اضغط «مزامنة» مجدداً. وإن نُقلت النسخة الاحتياطية، فأزل هذا الموقع وأضفه من جديد في مكانها الحالي."),
    e("sync.why.full", "sync_failed", 409, "",
        "there is no room left on the drive. Free some space on it, or choose another drive.",
        "لم تبقَ مساحة على القرص. أفرغ بعض المساحة عليه، أو اختر قرصاً آخر."),
    e("sync.why.no_write", "sync_failed", 409, "",
        "Thoughts is not allowed to write there. The drive may be locked or read-only; choose a folder you can save files in.",
        "لا يُسمح لـ«أفكار» بالكتابة هناك. ربما كان القرص مقفلاً أو للقراءة فقط؛ اختر مجلداً يمكنك حفظ الملفات فيه."),
    e("sync.why.host_key", "sync_failed", 409, "",
        "this computer has not connected to that server before, so it does not know whether to trust it. Open Settings › Library & backup and check the server there.",
        "لم يتصل هذا الجهاز بذلك الخادم من قبل، فلا يعرف هل يثق به. افتح الإعدادات › المكتبة والنسخ الاحتياطي وتحقّق من الخادم هناك."),
    e("sync.why.host_changed", "sync_failed", 409, "",
        "the server's identity has changed since this computer last connected. That can mean someone is intercepting the connection. Nothing was sent. Ask whoever runs the server before going on.",
        "تغيّرت هوية الخادم منذ آخر اتصال لهذا الجهاز به، وقد يعني ذلك أن أحداً يعترض الاتصال. لم يُرسَل شيء. اسأل من يدير الخادم قبل المتابعة."),
    e("sync.why.ssh_key", "sync_failed", 409, "",
        "the server did not accept this computer's key. Make sure the key is added to your account there. Thoughts cannot type a key's passphrase: if your key has one, add it to your computer's key agent first, then press Sync again.",
        "لم يقبل الخادم مفتاح هذا الجهاز. تأكّد أن المفتاح مضاف إلى حسابك هناك. لا يستطيع «أفكار» كتابة عبارة المرور الخاصة بالمفتاح: فإن كانت له عبارة مرور، فأضفه أولاً إلى وكيل المفاتيح في جهازك، ثم اضغط «مزامنة» مجدداً."),
    e("sync.not_received", "sync_failed", 409, "",
        "Nothing was synced, because changes could not be received from \"{name}\": {why}",
        "لم تتم المزامنة، لأنه تعذّر استلام التغييرات من «{name}»: {why}"),
    // --- backup locations: a drive or folder, sign-ins, SSH servers ---
    e("backup.folder_missing", "validation", 422, "",
        "The folder {path} cannot be found. If it is on a drive, plug the drive in first.",
        "تعذّر العثور على المجلد {path}. إن كان على قرص، فصِل القرص أولاً."),
    e("backup.folder_in_library", "validation", 422, "",
        "A backup cannot be kept inside the library itself. Choose a folder outside it, best of all on another drive.",
        "لا يمكن حفظ النسخة الاحتياطية داخل المكتبة نفسها. اختر مجلداً خارجها، ويُفضَّل أن يكون على قرص آخر."),
    e("backup.folder_in_cloud", "validation", 422, "",
        "That folder is kept in sync by {service}, which can damage a backup kept this way. Choose a folder on a drive, or choose \"Cloud folder\" as the backup instead.",
        "هذا المجلد تُزامنه {service}، وقد يتلف ذلك نسخةً احتياطية محفوظة بهذه الطريقة. اختر مجلداً على قرص، أو اختر «مجلد سحابي» للنسخ الاحتياطي بدلاً من ذلك."),
    e("backup.some_drive", "validation", 422, "", "a cloud drive", "خدمة سحابية"),
    e("backup.folder_taken", "validation", 422, "",
        "That folder already holds something named \"{name}\" that is not a backup. Choose another folder.",
        "في هذا المجلد شيء باسم «{name}» وليس نسخة احتياطية. اختر مجلداً آخر."),
    e("backup.folder_other_library", "validation", 422, "",
        "That folder already holds the backup of a different library. Choose another folder, so the two are never mixed.",
        "في هذا المجلد نسخة احتياطية لمكتبة أخرى. اختر مجلداً آخر حتى لا تختلط المكتبتان."),
    e("backup.folder_unreadable", "validation", 422, "",
        "The backup in that folder could not be read. It may be damaged, or the drive may be failing.",
        "تعذّرت قراءة النسخة الاحتياطية في هذا المجلد. ربما تكون تالفة، أو يكون القرص معطوباً."),
    e("backup.folder_unwritable", "validation", 422, "",
        "Thoughts could not create the backup there. The drive may be full, locked or read-only; choose a folder you can save files in.",
        "تعذّر على «أفكار» إنشاء النسخة الاحتياطية هناك. ربما كان القرص ممتلئاً أو مقفلاً أو للقراءة فقط؛ اختر مجلداً يمكنك حفظ الملفات فيه."),
    e("signin.empty", "validation", 422, "",
        "Enter the password or access token.",
        "أدخل كلمة المرور أو رمز الدخول."),
    e("signin.no_location", "not_found", 404, "",
        "That backup location is no longer in this library. Reopen Settings and try again.",
        "لم يعد موقع النسخ الاحتياطي هذا ضمن المكتبة. أعد فتح الإعدادات وحاول مرة أخرى."),
    e("signin.not_online", "validation", 422, "",
        "Only an online location whose address starts with https:// signs in with a password or access token.",
        "لا يسجّل الدخول بكلمة مرور أو رمز دخول إلا موقع على الإنترنت يبدأ عنوانه بـ https://."),
    e("hostkey.not_ssh", "validation", 422, "",
        "That location does not connect to a server with a key.",
        "هذا الموقع لا يتصل بخادم عن طريق مفتاح."),
    e("hostkey.no_tool", "validation", 422, "",
        "This computer does not have the tool needed to check a server. Connect to the server once from a terminal instead.",
        "لا توجد على هذا الجهاز الأداة اللازمة للتحقق من الخادم. اتصل بالخادم مرة واحدة من الطرفية بدلاً من ذلك."),
    e("hostkey.unreachable", "offline", 503, "",
        "The server could not be reached to check it. Check your internet connection and the address, then try again.",
        "تعذّر الوصول إلى الخادم للتحقق منه. تحقّق من اتصالك بالإنترنت ومن العنوان، ثم حاول مرة أخرى."),
    e("hostkey.changed_meanwhile", "conflict", 409, "",
        "The server showed a different key this time, so nothing was trusted. Check the server again.",
        "أظهر الخادم مفتاحاً مختلفاً هذه المرة، فلم يُوثَق بشيء. تحقّق من الخادم مرة أخرى."),
    e("git.strategy", "validation", 422, "",
        "Choose which version to keep: yours, the other one, or both.",
        "اختر النسخة التي تريد الإبقاء عليها: نسختك أم الأخرى أم كلتيهما."),
    e("sign_in", "sign_in", 401, "",
        "Sign-in is needed. Update the sign-in of your online location under Settings › Library & backup.",
        "يلزم تسجيل الدخول. حدّث بيانات الدخول لموقعك على الإنترنت من الإعدادات › المكتبة والنسخ الاحتياطي."),
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
    e("provider.url_blank", "validation", 422, "",
        "The service address cannot contain spaces.",
        "لا يمكن أن يحتوي عنوان الخدمة على مسافات."),
    e("provider.url_scheme", "validation", 422, "",
        "The service address must start with https://",
        "يجب أن يبدأ عنوان الخدمة بـ https://"),
    e("provider.url_login", "validation", 422, "",
        "Leave the user name and password out of the service address. The key is entered in its own box.",
        "لا تضع اسم المستخدم أو كلمة المرور في عنوان الخدمة. للمفتاح خانته الخاصة."),
    e("provider.url_host", "validation", 422, "",
        "The service address is incomplete: it has no site name.",
        "عنوان الخدمة ناقص: لا يحتوي على اسم الموقع."),
    e("provider.url_http", "validation", 422, "",
        "The service address must start with https://. A plain http:// address is only accepted for a service on this computer.",
        "يجب أن يبدأ عنوان الخدمة بـ https://. العنوان العادي http:// مقبول فقط لخدمة تعمل على هذا الجهاز."),
    e("provider.test_unreachable", "provider_test", 409, "",
        "The service could not be reached. Check the address, your internet connection, and that the service is running.",
        "تعذّر الوصول إلى الخدمة. تحقّق من العنوان ومن اتصالك بالإنترنت ومن أن الخدمة تعمل."),
    e("provider.test_address", "provider_test", 409, "",
        "The service answered, but not at this address (it said \"not found\"). Check the address against the service's instructions.",
        "ردّت الخدمة لكن ليس عند هذا العنوان (قالت إنه غير موجود). قارن العنوان بتعليمات الخدمة."),
    e("provider.test_model", "provider_test", 409, "",
        "The key works, but the service does not know the model that was asked for. Choose a model it offers under Settings › Assistant › Model.",
        "المفتاح يعمل، لكن الخدمة لا تعرف النموذج المطلوب. اختر نموذجاً تقدّمه الخدمة من «الإعدادات › المساعد › النموذج»."),
    e("provider.test_credit", "provider_test", 409, "",
        "The key was accepted, but the account has no credit left. Add credit with the service, then test again.",
        "قُبل المفتاح، لكن لا رصيد في الحساب. أضف رصيداً لدى الخدمة ثم جرّب مرة أخرى."),
    e("provider.test_limit", "provider_test", 409, "",
        "The service is asking you to slow down (too many requests, or a limit on this key). Wait a little and test again.",
        "تطلب الخدمة منك التمهّل (طلبات كثيرة أو حدّ على هذا المفتاح). انتظر قليلاً ثم جرّب مرة أخرى."),
    e("provider.test_timeout", "provider_test", 409, "",
        "The service did not answer in time. It may be busy or unreachable; try again in a moment.",
        "لم تردّ الخدمة في الوقت المناسب. قد تكون مشغولة أو لا يمكن الوصول إليها؛ جرّب بعد قليل."),
    e("provider.test_other", "provider_test", 409, "",
        "The test did not work. The service said: {detail}",
        "لم ينجح الاختبار. قالت الخدمة: {detail}"),
    e("provider.test_nobinary", "provider_test", 409, "",
        "The assistant program (Claude Code) is needed to test the connection, and it was not found.",
        "برنامج المساعد (Claude Code) لازم لاختبار الاتصال، ولم يُعثر عليه."),
    e("plan.limit", "limit", 409, "",
        "Your Claude plan's usage limit has been reached. It resets at {when}; after that you can carry on. Nothing was lost.",
        "بلغتَ حدّ الاستخدام في اشتراكك في Claude. يُعاد ضبطه في {when}، وبعدها يمكنك المتابعة. لم يضع شيء."),
    e("plan.limit_unknown", "limit", 409, "",
        "Your Claude plan's usage limit has been reached. Try again in a while. Nothing was lost.",
        "بلغتَ حدّ الاستخدام في اشتراكك في Claude. جرّب مرة أخرى بعد قليل. لم يضع شيء."),
    e("spend.limit_invalid", "validation", 422, "",
        "The monthly limit must be an amount between $1 and $100,000, and the warning point between 50% and 95%.",
        "يجب أن يكون الحد الشهري مبلغاً بين 1 و100000 دولار، ونقطة التنبيه بين 50% و95%."),
    e("spend.warn", "limit", 409, "",
        "You have used {percent}% of this month's spending limit ({spent} of {limit}).",
        "استخدمتَ {percent}% من حدّ الإنفاق لهذا الشهر ({spent} من {limit})."),
    e("spend.reached", "limit", 409, "",
        "This month's spending limit ({limit}) is reached. Automatic tasks and automatic summaries are paused until next month, or until you raise the limit. Your own chats still work.",
        "بلغتَ حدّ الإنفاق لهذا الشهر ({limit}). أُوقفت المهام التلقائية والملخصات التلقائية حتى الشهر القادم أو حتى ترفع الحد. محادثاتك أنت تعمل كالمعتاد."),
    e("run.stalled", "run_ended", 500, "",
        "The assistant gave no sign of life for {minutes} minutes, so this run was ended. You can try again.",
        "لم يصدر عن المساعد أي نشاط طوال {minutes} دقيقة، فأُنهيت هذه العملية. يمكنك المحاولة مرة أخرى."),
    e("run.too_long", "run_ended", 500, "",
        "This run went on for more than {minutes} minutes, so it was ended. You can try again.",
        "استمرت هذه العملية أكثر من {minutes} دقيقة، فأُنهيت. يمكنك المحاولة مرة أخرى."),
    e("run.lost", "run_ended", 500, "",
        "The connection to the assistant was lost, so this run was ended. You can try again.",
        "انقطع الاتصال بالمساعد، فأُنهيت هذه العملية. يمكنك المحاولة مرة أخرى."),
    e("run.ended_title", "run_ended", 500, "",
        "The assistant was stopped",
        "أُوقف المساعد"),
    e("run.denied", "denied", 500, "",
        "The assistant was not allowed to do this:",
        "لم يُسمح للمساعد بما يلي:"),
    e("run.restored", "denied", 500, "",
        "The assistant changed files this run may not change. They were put back as they were:",
        "عدّل المساعد ملفات لا يحق لهذه العملية تعديلها، فأُعيدت كما كانت:"),
    e("approval.asked", "approval", 500, "",
        "The assistant asks for your permission:",
        "يطلب المساعد إذنك:"),
    e("approval.gone", "approval_gone", 409, "",
        "This request is no longer waiting for an answer: the assistant has stopped, or the time to answer ran out.",
        "لم يعد هذا الطلب ينتظر جواباً: إما أن المساعد توقف وإما أن وقت الجواب انتهى."),
    e("approval.bad_answer", "invalid", 422, "",
        "Choose Allow once, Always for this chat or Deny.",
        "اختر «اسمح مرة واحدة» أو «اسمح دائماً في هذه المحادثة» أو «ارفض»."),
    e("bypass.unconfirmed", "confirm_needed", 409, "",
        "Bypassing permissions must be confirmed first. Choose it again and confirm the warning.",
        "تجاوز الأذونات يحتاج إلى تأكيدك أولاً. اختره مرة أخرى ووافق على التحذير."),
    e("bypass.chat_only", "forbidden", 403, "",
        "Bypassing permissions is only available in ordinary chats.",
        "تجاوز الأذونات متاح في المحادثات العادية فقط."),
    e("untrusted", "untrusted", 403, "",
        "This folder contains things that can run programs on your computer, and you have not trusted it yet: {path}",
        "هذا المجلد يحوي ما يمكنه تشغيل برامج على جهازك، ولم تمنحه ثقتك بعد: {path}"),
    e("trust.expired", "trust_expired", 409, "",
        "That question is no longer current. Try what you were doing again.",
        "هذا السؤال لم يعد قائماً. أعد ما كنت تفعله."),
    e("trust.too_large", "validation", 422, "",
        "A settings folder inside this folder holds too many files to check, so it cannot be trusted yet. Remove what is not needed from it and try again.",
        "في هذا المجلد مجلد إعدادات يحوي ملفات أكثر مما يمكن فحصه، فلا يمكن الوثوق به بعد. احذف منه ما لا حاجة إليه ثم حاول مرة أخرى."),
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
    e("wrap.no_history", "conflict", 409, "",
        "This library keeps no history, so the wrap cannot be undone automatically.",
        "هذه المكتبة لا تحتفظ بسجلّ، فلا يمكن التراجع عن التلخيص تلقائياً."),
    e("wrap.preparing", "busy", 409, "",
        "This plan is still being prepared. Wait for it to finish.",
        "ما زالت هذه الخطة قيد الإعداد. انتظر حتى تكتمل."),
    e("wrap.no_plan", "conflict", 409, "",
        "This plan was not finished, so there is nothing to approve. Start a new Wrap.",
        "لم تكتمل هذه الخطة، فلا يوجد ما يُعتمد. ابدأ تلخيصاً جديداً."),
    e("wrap.closed", "conflict", 409, "",
        "This Wrap is closed. Start a new one to save your chats.",
        "هذا التلخيص مغلق. ابدأ تلخيصاً جديداً لحفظ محادثاتك."),
    e("wrap.saved", "conflict", 409, "",
        "This wrap has already been saved.",
        "سبق حفظ هذا التلخيص."),
    e("wrap.already_saved", "conflict", 409, "",
        "A newer Wrap has already saved these chats, so this plan was not saved again.",
        "حفظ تلخيصٌ أحدث هذه المحادثات من قبل، فلم تُحفظ هذه الخطة مرة ثانية."),
    e("wrap.other_library", "conflict", 409, "",
        "This Wrap belongs to another library. Open that library to continue it.",
        "هذا التلخيص يخص مكتبة أخرى. افتح تلك المكتبة لمتابعته."),
    // A Wrap whose changes wait in a copy of the library for the person to look at them.
    e("wrap.reviewing", "wrap_review", 409, "WRAP_REVIEW::", "{id}", "{id}"),
    e("wrap.no_chats", "validation", 422, "",
        "Choose at least one chat for this Wrap.",
        "اختر محادثة واحدة على الأقل لهذا التلخيص."),
    e("wrap.not_offered", "conflict", 409, "",
        "One of the chosen chats is no longer waiting to be saved. Start a new Wrap to see the chats that are.",
        "إحدى المحادثات المختارة لم تعد بانتظار الحفظ. ابدأ تلخيصاً جديداً لترى المحادثات التي تنتظر."),
    e("wrap.writing", "busy", 409, "",
        "The changes are still being written. Wait for them to finish.",
        "ما زالت التغييرات قيد الكتابة. انتظر حتى تكتمل."),
    e("wrap.conflict", "conflict", 409, "WRAP_CONFLICT::", "{files}", "{files}"),
    e("reorganize.unfinished", "conflict", 409, "",
        "This reorganize was stopped before it finished, so its new arrangement is incomplete. Discard it and start again.",
        "توقفت إعادة التنظيم هذه قبل أن تكتمل، فالترتيب الجديد ناقص. تجاهلها وابدأ من جديد."),
    e("chat.other_library", "conflict", 409, "",
        "This chat belongs to another library ({path}). Open that library to continue it.",
        "هذه المحادثة تخص مكتبة أخرى ({path}). افتح تلك المكتبة لمتابعتها."),
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
    // --- maintenance: self-check, moving, removing (shellui-b) ---
    e("care.unknown_fix", "validation", 422, "",
        "This problem cannot be fixed from here.",
        "لا يمكن إصلاح هذه المشكلة من هنا."),
    e("care.running", "busy", 409, "",
        "A chat or a Wrap is still running. Wait for it to finish or stop it, then try again.",
        "ما زالت محادثة أو عملية «تلخيص وحفظ» قيد التشغيل. انتظر حتى تنتهي أو أوقفها، ثم حاول مرة أخرى."),
    e("move.bad", "validation", 422, "MOVE_BAD::",
        "This is not a Thoughts move file, or it was changed after it was made. Nothing was imported.",
        "هذا ليس ملف نقل من «أفكار»، أو أنه تغيّر بعد إنشائه. لم يُستورد شيء."),
    e("move.newer", "validation", 422, "MOVE_NEWER::",
        "This file was made by a newer version of Thoughts. Update this copy first, then import it.",
        "أُنشئ هذا الملف بإصدار أحدث من «أفكار». حدّث هذه النسخة أولاً، ثم استورده."),
    e("remove.unconfirmed", "confirm_needed", 409, "",
        "Removing Thoughts must be confirmed first.",
        "إزالة «أفكار» تحتاج إلى تأكيدك أولاً."),
    e("remove.blocked", "forbidden", 403, "REMOVE_BLOCKED::",
        "Thoughts' data folder cannot be removed safely from here: your library or a linked folder is inside it, or it is a link to another place. Nothing was removed.",
        "لا يمكن إزالة مجلد بيانات «أفكار» بأمان من هنا: مكتبتك أو مجلد مرتبط موجود داخله، أو أنه رابط إلى مكان آخر. لم يُحذف شيء."),
    // --- bringing in notes the user already has (setup-b) ---
    e("import.no_folder", "validation", 422, "",
        "There is no folder there. Choose the folder that holds your notes.",
        "لا يوجد مجلد هناك. اختر المجلد الذي يحوي ملاحظاتك."),
    e("import.library", "validation", 422, "",
        "That folder is your library, or holds it, or is inside it. Choose a folder of notes outside your library.",
        "هذا المجلد هو مكتبتك، أو يحويها، أو يقع داخلها. اختر مجلد ملاحظات خارج مكتبتك."),
    e("import.app_data", "validation", 422, "",
        "That folder holds Thoughts' own data, not notes. Choose the folder that holds your notes.",
        "هذا المجلد يحوي بيانات «أفكار» نفسها لا ملاحظاتك. اختر المجلد الذي يحوي ملاحظاتك."),
    e("import.bad_name", "validation", 422, "",
        "That is not a name a folder can have. Leave out characters such as : ? * and do not start or end with a dot or a space.",
        "هذا اسم لا يصلح لمجلد. تجنّب رموزاً مثل : ? * ولا تبدأ الاسم أو تنهِه بنقطة أو مسافة."),
    e("import.reserved", "validation", 422, "",
        "\"{name}\" is a folder the library uses for its own things. Choose another name.",
        "«{name}» مجلد تستخدمه المكتبة لأغراضها. اختر اسماً آخر."),
    e("import.exists", "import_exists", 409, "",
        "There is already something called \"{name}\" in your library. Choose another name, so nothing is mixed in or replaced.",
        "في مكتبتك شيء باسم «{name}» من قبل. اختر اسماً آخر حتى لا يختلط شيء أو يُستبدل."),
    e("import.too_big", "too_large", 413, "",
        "That folder is too big to bring in at once (more than 20,000 files or 2 GB). Bring in its folders one at a time.",
        "هذا المجلد أكبر من أن يُنقل دفعة واحدة (أكثر من 20,000 ملف أو 2 غيغابايت). انقل مجلداته واحداً تلو الآخر."),
    e("import.nothing", "validation", 422, "",
        "There is nothing in that folder to bring in.",
        "لا يوجد في هذا المجلد ما يمكن نقله."),
    e("import.adopt_empty", "validation", 422, "",
        "This folder is empty. Choose \"Start fresh\" to set up a new library in it.",
        "هذا المجلد فارغ. اختر «بداية جديدة» لتجهيز مكتبة جديدة فيه."),
    e("import.adopt_library", "validation", 422, "",
        "This folder is already a library, so there is nothing to add to it. Open it as it is.",
        "هذا المجلد مكتبة من قبل، فلا حاجة إلى إضافة شيء إليه. افتحه كما هو."),
    e("import.adopt_too_big", "validation", 422, "",
        "This folder holds too much to be used as a library (more than 20,000 files or 2 GB). Choose the folder that holds just your notes.",
        "يحوي هذا المجلد أكثر مما تتّسع له مكتبة (أكثر من 20,000 ملف أو 2 غيغابايت). اختر المجلد الذي يحوي ملاحظاتك فقط."),
    // --- customisations: history, review, packs, add-ons (layerd-c) ---
    e("layer.undo_builds_on", "conflict", 409, "",
        "This change cannot be undone on its own because later changes build on it. To take out an add-on, switch it off or remove it under Customise, Add-ons. To undo everything since, go back to an earlier point.",
        "لا يمكن التراجع عن هذا التغيير وحده لأن تغييرات لاحقة مبنية عليه. لإخراج إضافة أوقفها أو أزلها من «التخصيص» ثم «الإضافات». وللتراجع عن كل ما جاء بعده عُد إلى نقطة سابقة."),
    e("layer.already_undone", "conflict", 409, "",
        "This change is already undone.",
        "سبق التراجع عن هذا التغيير."),
    e("layer.bad_entry", "not_found", 404, "",
        "This entry could not be found in the history. Refresh the list and try again.",
        "تعذّر العثور على هذا الإدخال في السجل. حدّث القائمة ثم حاول مرة أخرى."),
    e("layer.preparing", "busy", 409, "",
        "Another customisation is still being prepared. Wait for it to finish, or stop it first.",
        "ما زال تخصيص آخر قيد الإعداد. انتظر حتى ينتهي، أو أوقفه أولاً."),
    e("layer.assistant_working", "busy", 409, "",
        "The assistant is still working on this change. Wait for it to finish, or stop it first.",
        "ما زال المساعد يعمل على هذا التغيير. انتظر حتى ينتهي، أو أوقفه أولاً."),
    e("layer.no_pending", "not_found", 404, "",
        "There is no change waiting for review.",
        "لا يوجد تغيير ينتظر المراجعة."),
    e("pack.not_pack", "validation", 422, "",
        "This file is not a Thoughts customisation pack.",
        "هذا الملف ليس حزمة تخصيص من «أفكار»."),
    e("pack.newer", "validation", 422, "",
        "This pack was made for a newer version of Thoughts. Update the app, then try again.",
        "صُنعت هذه الحزمة لإصدار أحدث من «أفكار». حدّث التطبيق ثم حاول مرة أخرى."),
    e("pack.empty", "validation", 422, "",
        "This pack has nothing in it.",
        "هذه الحزمة فارغة."),
    e("pack.too_many", "validation", 422, "",
        "This pack has too many files (at most {count}). Nothing was changed.",
        "في هذه الحزمة ملفات كثيرة جداً (الحد الأقصى {count}). لم يتغيّر شيء."),
    e("pack.bad_file", "validation", 422, "",
        "This pack contains a file that Thoughts does not accept as a customisation ({path}). Nothing was changed.",
        "تحتوي هذه الحزمة على ملف لا يقبله «أفكار» ضمن التخصيصات ({path}). لم يتغيّر شيء."),
    e("pack.not_text", "validation", 422, "",
        "{path} in this pack is not text. Nothing was changed.",
        "الملف {path} في هذه الحزمة ليس نصاً. لم يتغيّر شيء."),
    e("pack.file_too_big", "validation", 422, "",
        "{path} in this pack is too large (at most 512 KB per file). Nothing was changed.",
        "الملف {path} في هذه الحزمة كبير جداً (الحد الأقصى ٥١٢ كيلوبايت لكل ملف). لم يتغيّر شيء."),
    e("pack.too_big", "validation", 422, "",
        "This pack is too large (at most 4 MB in all). Nothing was changed.",
        "هذه الحزمة كبيرة جداً (الحد الأقصى ٤ ميغابايت في المجموع). لم يتغيّر شيء."),
    e("pack.grows_too_big", "validation", 422, "",
        "{path} would become too large once the pack is added (at most 512 KB). Nothing was changed.",
        "سيصبح الملف {path} كبيراً جداً بعد إضافة الحزمة (الحد الأقصى ٥١٢ كيلوبايت). لم يتغيّر شيء."),
    e("pack.bad_json", "validation", 422, "",
        "{path} in this pack is damaged. Nothing was changed.",
        "الملف {path} في هذه الحزمة تالف. لم يتغيّر شيء."),
    e("pack.yours_broken", "conflict", 409, "",
        "Your {path} is damaged, so the pack cannot be combined with it. Undo the change that damaged it, or ask the assistant to repair it, then try again. Nothing was changed.",
        "ملف {path} لديك تالف، فلا يمكن دمج الحزمة معه. تراجع عن التغيير الذي أتلفه، أو اطلب من المساعد إصلاحه، ثم حاول مرة أخرى. لم يتغيّر شيء."),
    e("pack.shape", "conflict", 409, "",
        "{path} in this pack is laid out differently from yours, so the two cannot be combined. Nothing was changed.",
        "الملف {path} في هذه الحزمة مبنيّ بشكل مختلف عن ملفك، فلا يمكن دمجهما. لم يتغيّر شيء."),
    e("pack.nothing", "validation", 422, "",
        "There is nothing to share: none of the chosen items exist any more.",
        "لا يوجد ما يُشارَك: لم يعد أيّ من العناصر المختارة موجوداً."),
    e("addon.gone", "not_found", 404, "",
        "This add-on is no longer in your app. The list has been refreshed.",
        "لم تعد هذه الإضافة موجودة في تطبيقك. حُدّثت القائمة."),
    e("addon.no_permission", "conflict", 409, "",
        "This add-on does not have that permission (any more).",
        "لا تملك هذه الإضافة هذه الصلاحية، أو لم تعد تملكها."),
    e("addon.version_gone", "not_found", 404, "",
        "That earlier copy of the saved data is no longer kept.",
        "لم تعد تلك النسخة السابقة من البيانات المحفوظة موجودة."),
    e("addon.data_in_use", "conflict", 409, "",
        "This add-on is still in your app, so its saved data cannot be deleted for good. Clear it instead, or remove the add-on first.",
        "هذه الإضافة ما زالت في تطبيقك، فلا يمكن حذف بياناتها نهائياً. امسحها بدلاً من ذلك، أو أزل الإضافة أولاً."),
    e("gallery.gone", "not_found", 404, "",
        "This ready-made pack is not part of this version of Thoughts.",
        "هذه الحزمة الجاهزة ليست ضمن هذا الإصدار من «أفكار»."),
    // --- several libraries (libs) ---
    e("libs.not_found", "not_found", 404, "",
        "That library is no longer in your list.",
        "لم تعد هذه المكتبة في قائمتك."),
    e("libs.name_long", "validation", 422, "",
        "A library's name can be at most {max} characters long.",
        "لا يزيد اسم المكتبة على {max} حرفاً."),
    e("libs.remove_open", "conflict", 409, "",
        "This is the library you have open. Open another one first, then take this one off the list.",
        "هذه هي المكتبة المفتوحة الآن. افتح مكتبة أخرى أولاً، ثم أزِل هذه من القائمة."),
    e("libs.missing", "library_missing", 404, "",
        "The folder of \"{name}\" cannot be found ({path}). If it was moved, add it again from its new place.",
        "تعذّر العثور على مجلد «{name}» ({path}). إن كان قد نُقل، فأضِفه من جديد من مكانه الجديد."),
    e("libs.switched", "library_switched", 409, "LIBRARY_SWITCHED::",
        "Another library was opened in the meantime, so this was not saved. The window now shows the library that is open.",
        "فُتحت مكتبة أخرى في هذه الأثناء، فلم يُحفظ هذا. تعرض النافذة الآن المكتبة المفتوحة."),
    e("libs.not_set_up", "conflict", 409, "",
        "Finish setting up Thoughts first. Then you can add more libraries.",
        "أكمل تجهيز «أفكار» أولاً، ثم يمكنك إضافة مكتبات أخرى."),
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
    ("REORG_CONFLICT::", 409, "conflict"), ("STALE::", 409, "stale"),
    ("NO_HISTORY::", 409, "no_history"), ("NEWER_SETTINGS::", 409, "newer_settings"), ("CHANGED::", 409, "changed"),
    ("SIGNED_OUT::", 409, "signed_out"),
];

fn has_arabic(s: &str) -> bool { s.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)) }

/// Raw messages (from git, the OS, older code paths) that have one plain meaning.
fn known_raw(msg: &str) -> Option<&'static str> {
    let m = msg.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    if has(&["the library is not on a branch"]) { return Some("git.not_branch"); }
    // A history the app cannot work in: each has its way out (Repair history). Only git's own
    // words (a file elsewhere can be "corrupt" too).
    let from_git = m.starts_with("git ") || m.contains(": git ") || m.contains("fatal: ");
    if from_git {
        if has(&["index.lock': file exists", "another git process seems to be running"]) { return Some("history.locked"); }
        if has(&["you have not concluded your merge", "there is already a rebase-merge directory", "there is already a rebase-apply directory",
                 "it looks like you are in the middle of", "you are currently rebasing", "because you have unmerged files", "you need to resolve your current index first",
                 "a cherry-pick or revert is already in progress", "cherry-pick is already in progress", "revert is already in progress"]) { return Some("history.unfinished"); }
        if has(&["index file corrupt", "index file smaller than expected", "bad index file", "bad signature 0x", "is corrupt", "loose object", "bad object",
                 "unable to read tree", "appears to be broken", "inflate:", "failed to traverse parents", "missing blob", "missing tree", "missing commit", "bad tree object"]) { return Some("history.damaged"); }
    }
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
    // A folder that needs the user's trust is known by its type alone. No wording can stand in for
    // it: text that merely contains a trust marker is an ordinary message.
    if let Some(u) = err.chain().find_map(|c| c.downcast_ref::<crate::trust::Untrusted>()) {
        return UserErr { code: "untrusted", status: 403, message: u.message_in(ar) };
    }
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
        // …but never "untrusted": text that looks like the old trust marker is an ordinary message,
        // wherever it sits, so nothing echoed into an error can raise the trust question.
        for text in ["UNTRUSTED::/x::git hooks", "This add-on is not allowed to change a UNTRUSTED::/Users/Shared/lib::Looks fine.md"] {
            let u = c(anyhow::anyhow!("{text}"), false);
            assert_eq!((u.status, u.code, u.message.as_str()), (400, "error", text));
        }
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
        // Each state of the history the app cannot work in says so, and points at its repair.
        for raw in ["git add -A: fatal: Unable to create '/Users/x/Notes/.git/index.lock': File exists.\n\nAnother git process seems to be running in this repository",
                    "git commit -q -m x: error: Committing is not possible because you have unmerged files.",
                    "git pull: fatal: It seems that there is already a rebase-merge directory, and\nI wonder if you are in the middle of another rebase.",
                    "git log: fatal: loose object 3b18e512dba79e4c8300dd08aeb37f8e728b8dad (stored in .git/objects/3b/18e5) is corrupt",
                    "git status: error: bad signature 0x00000000\nfatal: index file corrupt",
                    "git log: fatal: bad object HEAD"] {
            let u = c(anyhow::anyhow!("{raw}"), false);
            assert_eq!(u.code, "history_state", "{raw}");
            assert!(u.message.contains("Repair history") && !u.message.contains(".git"), "{}", u.message);
        }
        assert_eq!(c(anyhow::anyhow!("no such chat"), false).status, 404);
        // An untranslated message stays readable: as is in English, behind a generic sentence in Arabic.
        assert_eq!(c(anyhow::anyhow!("give the project a name"), false).message, "give the project a name");
        assert_eq!(c(anyhow::anyhow!("give the project a name"), true).message, "حدث خطأ غير متوقع. (give the project a name)");
        assert_eq!(c(anyhow::anyhow!("هذه رسالة عربية"), true).message, "هذه رسالة عربية");
    }
}
