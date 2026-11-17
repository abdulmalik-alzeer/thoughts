# Glossary

One name for each thing, in both languages. The app (`ui/src/lib/i18n*.ts`, `ui/src/lib/i18n/*.ts`),
the background service's messages (`crates/thoughtsd/src/tr.rs`), the native menus (`src-tauri`),
the starter kit (`starter/`) and the guides (`docs/`, `docs/ar/`, `README.md`, `README.ar.md`)
all use these words. When you add a string, look here first; when you need a new term, add it here
in the same change.

The Arabic is Modern Standard Arabic for a Saudi professional audience: plain, natural sentences
rather than word-for-word translations of the English.

## Writing rules

| Rule | English | Arabic |
|---|---|---|
| The app's name | **Thoughts** | **Thoughts**, in Latin letters, treated as masculine (تطبيق): «يحفظ Thoughts مكتبتك». Never «أفكار»: that word is the Ideas page. |
| Spelling | British: customise, colour, summarise. *Reorganize* keeps its z: it is the name of a routine. | Tanween on the alif: «تلقائياً», not «تلقائيًا». |
| Counts | `tn()` with `_one` and `_other` | `tn()` with all six forms: `_zero` («لا أفكار»), `_one`, `_two`, `_few`, `_many`, `_other`. |
| "As" a format or role | as a PDF, as private | «بصيغة PDF»، «على هيئة حزمة»، «بوصفها مجلدات خاصة». Not «كـ». |
| Where to find a control | Settings › Library & backup | الإعدادات › المكتبة والنسخ الاحتياطي |
| The working week | Monday to Friday; the weekly review is on Friday | Sunday to Thursday; the weekly review is on Thursday («كل خميس»). |
| What the user does | click, press, choose. Never "tap": this is a desktop app. | اضغط، اختر |
| Things not to show | file names (`TODO.md`), markers (`@due(…)`), commands, flags, *commit*, *push*, *repo*, *CLI* | the same |

## The app

| English | Arabic | What it is |
|---|---|---|
| Thoughts | Thoughts | The app. |
| Home | الرئيسية | The first page. |
| Library | المكتبة | The folder on the computer that holds the user's notes as plain files. |
| Assistant | المساعد | The AI program Thoughts works with (Claude Code). |
| Background service | خدمة الخلفية | The part of Thoughts that keeps running and does the work. |
| Settings | الإعدادات | Categories: General «عام» · Accessibility «تسهيلات الاستخدام» · Assistant «المساعد» · Usage «الاستخدام» · Library & backup «المكتبة والنسخ الاحتياطي» · Privacy & cost «الخصوصية والتكلفة» · Features «المزايا» · Background service «خدمة الخلفية» · Customise «التخصيص» · Maintenance «الصيانة». |
| Help | المساعدة | The Help page. |
| Command palette | لوحة الأوامر | Opened with ⌘K / Ctrl+K. |
| Quick capture | التدوين السريع | Write a todo or a note from any app. |
| Inbox | صندوق الوارد | Quick notes and phone captures waiting to be filed. |
| Recovery (menu) | الاستعادة | The native menu that works even when the window does not. |
| Safe mode | الوضع الآمن | Customisations switched off. |
| Feature | ميزة | Optional parts switched on and off in Settings › Features «المزايا». |

## Your library

| English | Arabic | What it is |
|---|---|---|
| Idea / Ideas | فكرة / الأفكار | The default work item and its page. |
| Work item: client, matter, engagement, class, project | عميل، قضية، مشروع استشاري، فصل، مشروع | The word for an item follows the user's line of work; the page is then called Work «العمل». |
| Area | قسم | A top-level group of items ("Areas of your library" «أقسام مكتبتك»). |
| Note | ملاحظة | Any text file in the library. |
| Docs | المستندات | The page that lists every file. A file is a document «مستند». |
| Todo / Todos | مهمة / المهام | One line in a todo list. English never says *task* for a todo. |
| Todo list; the general list | قائمة المهام؛ القائمة العامة | |
| Due date | تاريخ الاستحقاق (الموعد) | Given with the calendar button on a todo. |
| Reminder | تذكير | |
| Meeting notes | محضر الاجتماع | |
| Deliverables | المخرجات | Files made for an item. |
| Log (of an item) | سجل العمل | Not the same as History «السجل». |
| Folders | المجلدات | Folders outside the library linked to Thoughts to chat in. |
| Private folder | مجلد خاص | Closed to the assistant. *Strict private folders* «المجلدات الخاصة بصرامة». |
| Me | ملفي | The user's profile. |
| Status: New · Active · On hold · Done · Dropped | جديد · قيد العمل · معلّق · منتهٍ · متروك | |

## Saving, history and backup

| English | Arabic | What it is |
|---|---|---|
| Wrap | تلخيص وحفظ | Turns finished chats into lasting notes, ideas and todos. |
| History | السجل | Every saved change. Tabs: Your notes «ملاحظاتك» · App changes «تغييرات التطبيق». The switch is "Keep a history of changes on this computer". |
| Versions | النسخ السابقة | The earlier versions of one document. |
| Undo this change | التراجع عن هذا التغيير | |
| Restore (a version) | استعادة | |
| Repair history | إصلاح السجلّ | |
| Backup | النسخ الاحتياطي | One of: No backup «بلا نسخ احتياطي» · Cloud folder «مجلد سحابي» · Online location «موقع على الإنترنت» · Drive or folder «قرص أو مجلد». History is not a backup. |
| Backup location; main location | موقع النسخ الاحتياطي؛ الموقع الرئيسي | |
| Sync | مزامنة | Sends the library to its backup and brings in changes from other computers. |
| Sign-in (of a location); Update sign-in | بيانات الدخول؛ تحديث الدخول | |
| Password store | خزنة كلمات المرور | The system's own (Keychain on a Mac). |
| Move to another computer | النقل إلى جهاز آخر | In Settings › Maintenance. |

## Chats and the assistant

| English | Arabic | What it is |
|---|---|---|
| Chat | محادثة | |
| Model · Effort · Permission mode | النموذج · الجهد · وضع الأذونات | Values keep Claude Code's own names. |
| Approval | موافقة | The assistant asks before it does something. |
| Automatic summaries | الملخصات التلقائية | Short summaries made in the background for Wrap. |
| Spending limit | حد الإنفاق | |
| Usage · usage circle | الاستخدام · دائرة الاستخدام | The Usage page and Settings › Usage; the circle beside Send in a chat that shows the plan's limit and the chat's use. |
| Plan (Claude subscription) | الاشتراك / خطة Claude | |
| Key for other programs | مفتاح للبرامج الأخرى | Lets a shortcut or a script on the computer add to the library or read it. |

## Routines: what you can say in a chat

| English | Arabic | What it does |
|---|---|---|
| Routine | روتين (الروتينات) | Steps the assistant follows when asked. |
| new idea: … | فكرة جديدة: … | Adds an item (or "new client: …" «عميل جديد: …» and so on). |
| todo: … | مهمة: … | Adds a todo. |
| continue … | تابع … | Picks up an item where it was left. |
| Check-in | وقفة دورية (يومية أو أسبوعية) | A short daily or weekly debrief. |
| Tune-up | ضبط المكتبة | Proposes small changes to the library's rules so they fit how it is used. |
| Review the library | فحص المكتبة | A full health check of the library, with prioritised feedback. Longer and costlier than a tune-up. |
| Reorganize | إعادة تنظيم المكتبة | Proposes a new arrangement of the library, reviewed before it is kept. |
| Process the inbox | معالجة صندوق الوارد | Files every inbox capture where it belongs. |
| Interview me | حاورني | Questions that fill in the profile (Me). |

«مراجعة» is not the name of any routine. In the app it means looking at a change before keeping it
(see below), and it names the weekly review of your week («المراجعة الأسبوعية», a ready-made
automatic task); nothing else. A user's own work (a contract review, a trial balance) uses the word
freely.

## Customising the app

| English | Arabic | What it is |
|---|---|---|
| Customise | التخصيص | The page (in the sidebar) and the verb. What the user kept are customisations «تخصيصات». |
| Change | تغيير | A prepared customisation waiting to be kept. |
| Review | المراجعة | Looking at a change before it is kept. The only meaning of «مراجعة» in the app. |
| Try it · Keep · Discard | تجربة · اعتماد · تجاهل | The three buttons of a review. |
| Keep these changes (a Wrap) | أبقِ هذه التغييرات | A Wrap's changes, looked at before they enter the library. |
| Add-on · Page · Look · Layout | إضافة · صفحة · المظهر · الترتيب | Kinds of customisation. |
| Automatic task | مهمة تلقائية | Something the assistant does on a schedule. Never *job*. |
| Weekly review | المراجعة الأسبوعية | The ready-made automatic task that writes up the week, on Thursday in Arabic. |
| Monthly tune-up (of the app) | الضبط الشهري للتطبيق | Home's monthly suggestion for one customisation. |
| Pack · Ready-made packs | حزمة · حزم جاهزة | A file of customisations to share or import. |
| App updates | تحديثات التطبيق | Where a new release and the user's changes meet. |
| Go back to an earlier version | العودة إلى نسخة سابقة | Puts the whole app back to an earlier point (History › App changes, or the Recovery menu). |
