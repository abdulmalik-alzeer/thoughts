// UI strings. English + Arabic (RTL). Missing Arabic keys fall back to English.
import { app } from './store.svelte';

const en = {
  home: 'Home', heroSub: 'Your notes, ideas and todos in one place. Ask anything, think out loud, and save what matters with Wrap.', continueSub: 'The ideas you touched most recently.', linkFolder: 'Link a folder', goodMorning: 'Good morning', goodAfternoon: 'Good afternoon', goodEvening: 'Good evening',
  askPlaceholder: 'Ask anything, or think out loud…', captureIdea: 'Capture idea', addTodoChip: 'Add todo', checkIn: 'Check-in',
  activeIdeas: 'Active ideas', openTodos: 'Open todos', toWrapAction: 'Chats to wrap — click to save', allSaved: 'All chats saved',
  notesInLibrary: 'Notes in library', continueWhere: 'Pick up where you left off', seeAll: 'See all', recentChats: 'Recent chats',
  noIdeasYet: 'No ideas yet — capture one above.', nothingOpen: 'Nothing open. Nice.', manage: 'Manage', chatsLower: 'chats',
  noProjects: 'Folders you work in, with your AI alongside.', justNow: 'just now', minutesAgo: '{n} min ago', hoursAgo: '{n} hr ago', daysAgo: '{n} days ago',
  chats: 'Chats', ideas: 'Ideas', todos: 'Todos', docs: 'Docs', projects: 'Folders', me: 'Me',
  settings: 'Settings', search: 'Search', newChat: 'New chat', library: 'Library', wrap: 'Wrap',
  sync: 'Sync', rollback: 'Go back to an earlier version', send: 'Send', stop: 'Stop', attach: 'Attach', model: 'Model',
  effort: 'Effort', permission: 'Permission mode', default: 'Default', customize: 'Customise',
  reorganize: 'Reorganize library', updates: 'Updates', addTodo: 'Add a todo…', save: 'Save', cancel: 'Cancel',
  edit: 'Edit', delete: 'Delete', close: 'Close', theme: 'Theme', mode: 'Appearance', language: 'Language',
  light: 'Day', dark: 'Night', system: 'System', saudi: 'Saudi', contrast: 'High contrast', tech: 'High tech',
  unwrapped: 'unwrapped', typeMessage: 'Message… (@ to mention a file, ⌘↩ to send)',
  wrapNagTitle: 'Time to wrap', next: 'Next', back: 'Back', finish: 'Finish', modules: 'Features',
  addLater: 'Add later', enable: 'Enable', enabled: 'Enabled', general: 'General', ai: 'Assistant',
  git: 'Library & backup', remotes: 'Backup locations', daemon: 'Background service', appLayer: 'Customise',
  noLibrary: 'No library yet', noChats: 'No chats yet — start one.', views: 'Views',
  wrapNagBody: 'You have {n} chats that aren’t saved into your library yet. Wrap summarises them into your ideas, notes and todos, then saves them to the history — so nothing you thought through evaporates.',
  wrapNagBody_one: 'You have {n} chat that isn’t saved into your library yet. Wrap summarises it into your ideas, notes and todos, then saves them to the history — so nothing you thought through evaporates.',
  wrapNagBody_other: 'You have {n} chats that aren’t saved into your library yet. Wrap summarises them into your ideas, notes and todos, then saves them to the history — so nothing you thought through evaporates.',
};

// Shared ending of the Arabic wrap reminder; the opening changes with the count (see wrapNagBody_*).
const wrapNagTailAr = ' «تلخيص وحفظ» يستخلص منها أفكارك وملاحظاتك ومهامك ثم يحفظها في السجل — حتى لا يضيع شيء مما فكّرت فيه.';

const ar: Partial<Record<keyof typeof en | `wrapNagBody_${'two' | 'few' | 'many'}`, string>> = {
  home: 'الرئيسية', heroSub: 'ملاحظاتك وأفكارك ومهامك في مكان واحد. اسأل عمّا تشاء، وفكّر بصوت مسموع، ثم احفظ ما يهمّك عبر «تلخيص وحفظ».', continueSub: 'آخر الأفكار التي عملت عليها.', linkFolder: 'ربط مجلد', goodMorning: 'صباح الخير', goodAfternoon: 'طاب نهارك', goodEvening: 'مساء الخير',
  askPlaceholder: 'اسأل عمّا تشاء، أو فكّر بصوت مسموع…', captureIdea: 'دوّن فكرة', addTodoChip: 'أضف مهمة', checkIn: 'وقفة دورية',
  activeIdeas: 'أفكار نشطة', openTodos: 'مهام مفتوحة', toWrapAction: 'محادثات بانتظار الحفظ — اضغط للحفظ', allSaved: 'كل المحادثات محفوظة',
  notesInLibrary: 'ملاحظات في المكتبة', continueWhere: 'تابع من حيث توقفت', seeAll: 'عرض الكل', recentChats: 'أحدث المحادثات',
  noIdeasYet: 'لا أفكار بعد — دوّن واحدة من الأعلى', nothingOpen: 'لا مهام مفتوحة. أحسنت!', manage: 'إدارة', chatsLower: 'محادثات',
  noProjects: 'مجلدات تعمل فيها برفقة مساعدك الذكي', justNow: 'الآن', minutesAgo: 'قبل {n} دقيقة', hoursAgo: 'قبل {n} ساعة', daysAgo: 'قبل {n} يوم',
  chats: 'المحادثات', ideas: 'الأفكار', todos: 'المهام', docs: 'المستندات', projects: 'المجلدات', me: 'ملفي',
  settings: 'الإعدادات', search: 'بحث', newChat: 'محادثة جديدة', library: 'المكتبة', wrap: 'تلخيص وحفظ',
  sync: 'مزامنة', rollback: 'العودة إلى نسخة سابقة', send: 'إرسال', stop: 'إيقاف', attach: 'إرفاق', model: 'النموذج',
  effort: 'الجهد', permission: 'وضع الأذونات', default: 'افتراضي', customize: 'التخصيص',
  reorganize: 'إعادة تنظيم المكتبة', updates: 'التحديثات', addTodo: 'أضف مهمة…', save: 'حفظ', cancel: 'إلغاء',
  edit: 'تعديل', delete: 'حذف', close: 'إغلاق', theme: 'السمة', mode: 'المظهر', language: 'اللغة',
  light: 'نهاري', dark: 'ليلي', system: 'حسب النظام', saudi: 'سعودية', contrast: 'تباين عالٍ', tech: 'تقنية عالية',
  unwrapped: 'لم تُلخَّص بعد', typeMessage: 'اكتب رسالتك… (@ للإشارة إلى ملف، ⌘↩ للإرسال)',
  wrapNagTitle: 'حان وقت التلخيص والحفظ', next: 'التالي', back: 'رجوع', finish: 'إنهاء', modules: 'المزايا',
  addLater: 'أضفها لاحقاً', enable: 'تفعيل', enabled: 'مفعّلة', general: 'عام', ai: 'المساعد',
  git: 'المكتبة والنسخ الاحتياطي', remotes: 'مواقع النسخ الاحتياطي', daemon: 'خدمة الخلفية', appLayer: 'التخصيص',
  noLibrary: 'لا توجد مكتبة بعد', noChats: 'لا محادثات بعد — ابدأ واحدة.', views: 'الواجهات',
  wrapNagBody: 'لديك محادثات لم تُحفظ في مكتبتك بعد ({n}).' + wrapNagTailAr,
  wrapNagBody_one: 'لديك محادثة واحدة لم تُحفظ في مكتبتك بعد.' + wrapNagTailAr,
  wrapNagBody_two: 'لديك محادثتان لم تُحفظا في مكتبتك بعد.' + wrapNagTailAr,
  wrapNagBody_few: 'لديك {n} محادثات لم تُحفظ في مكتبتك بعد.' + wrapNagTailAr,
  wrapNagBody_many: 'لديك {n} محادثة لم تُحفظ في مكتبتك بعد.' + wrapNagTailAr,
  wrapNagBody_other: 'لديك {n} محادثة لم تُحفظ في مكتبتك بعد.' + wrapNagTailAr,
};

// Extra dictionaries live in ./i18n/<area>.ts, each `export default { en: {...}, ar: {...} }`.
// They are merged here so separate areas of the app can add strings without touching this file.
const extra = import.meta.glob('./i18n/*.ts', { eager: true }) as Record<string, { default: { en: Record<string, string>; ar?: Record<string, string> } }>;
const EN: Record<string, string> = { ...en };
const AR: Record<string, string> = { ...(ar as Record<string, string>) };
for (const m of Object.values(extra)) { Object.assign(EN, m.default.en); Object.assign(AR, m.default.ar ?? {}); }

export type Key = keyof typeof en | (string & {});
export const lang = () => (app.settings?.language === 'ar' ? 'ar' : 'en');

/** Every string of the current language (English for any key without an Arabic one), for a page
 *  an add-on draws in place of one of the app's own (`thoughts.i18n`). */
export function dictionary(): Record<string, string> { return lang() === 'ar' ? { ...EN, ...AR } : { ...EN }; }

export function t(k: Key, vars: Record<string, string | number> = {}): string {
  let s = (lang() === 'ar' ? AR[k] : undefined) ?? EN[k] ?? k;
  for (const [n, v] of Object.entries(vars)) s = s.replaceAll(`{${n}}`, String(v));
  return s;
}

/** Plural-aware string: looks up `${k}_${category}` (zero|one|two|few|many|other per Intl.PluralRules)
 *  and falls back to `${k}_other`, then `k`. `{n}` is replaced with the locale-formatted number. */
export function tn(k: string, n: number, vars: Record<string, string | number> = {}): string {
  const cat = new Intl.PluralRules(lang()).select(n);
  const dict = lang() === 'ar' ? AR : EN;
  const key = [`${k}_${cat}`, `${k}_other`, k].find((x) => dict[x] ?? EN[x]) ?? k;
  return t(key, { n: fmtNum(n), ...vars });
}

/** Numbers and dates always go through these so digits/calendars stay consistent per language. */
export const fmtNum = (n: number) => new Intl.NumberFormat(lang() === 'ar' ? 'ar-SA-u-nu-latn' : 'en').format(n);
export function fmtDate(d: string | number | Date, opts: Intl.DateTimeFormatOptions = { dateStyle: 'medium' }): string {
  return new Intl.DateTimeFormat(lang() === 'ar' ? 'ar-SA-u-ca-gregory-nu-latn' : 'en', opts).format(new Date(d));
}
/** The same day in the Hijri (Umm al-Qura) calendar, for showing beside the Gregorian date. */
export function fmtHijri(d: string | number | Date, opts: Intl.DateTimeFormatOptions = { day: 'numeric', month: 'long', year: 'numeric' }): string {
  try { return new Intl.DateTimeFormat(lang() === 'ar' ? 'ar-SA-u-ca-islamic-umalqura-nu-latn' : 'en-u-ca-islamic-umalqura', opts).format(new Date(d)); }
  catch { return ''; }
}

/** A US-dollar amount (the assistant's usage is billed in dollars) in the app language's number style. */
export const fmtMoney = (amount: number) => {
  const usd = amount || 0; // never "-$0.00": a stored -0 (or nothing) is simply no amount
  return new Intl.NumberFormat(lang() === 'ar' ? 'ar-SA-u-nu-latn' : 'en', { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: usd > 0 && usd < 1 ? 3 : 2 }).format(usd);
};

export function ago(when: string | number): string {
  const m = Math.max(0, Math.round((Date.now() - new Date(when).getTime()) / 60000));
  if (m < 1) return t('justNow');
  const rtf = new Intl.RelativeTimeFormat(lang() === 'ar' ? 'ar-SA-u-nu-latn' : 'en', { numeric: 'always', style: lang() === 'ar' ? 'long' : 'narrow' });
  if (m < 60) return rtf.format(-m, 'minute');
  if (m < 60 * 24) return rtf.format(-Math.round(m / 60), 'hour');
  return rtf.format(-Math.round(m / 1440), 'day');
}
