// UI strings. English + Arabic (RTL). Missing Arabic keys fall back to English.
import { app } from './store.svelte';

const en = {
  home: 'Home', heroSub: 'Your notes, ideas and todos in one place. Ask anything, think out loud, and save what matters with Wrap.', continueSub: 'The ideas you touched most recently.', linkFolder: 'Link a folder', goodMorning: 'Good morning', goodAfternoon: 'Good afternoon', goodEvening: 'Good evening',
  askPlaceholder: 'Ask anything, or think out loud…', captureIdea: 'Capture idea', addTodoChip: 'Add todo', checkIn: 'Check-in',
  activeIdeas: 'Active ideas', openTodos: 'Open todos', toWrapAction: 'Chats to wrap — tap to save', allSaved: 'All chats saved',
  notesInLibrary: 'Notes in library', continueWhere: 'Pick up where you left off', seeAll: 'See all', recentChats: 'Recent chats',
  noIdeasYet: 'No ideas yet — capture one above.', nothingOpen: 'Nothing open. Nice.', manage: 'Manage', chatsLower: 'chats',
  noProjects: 'Folders you work in, with your AI alongside.', justNow: 'just now', minutesAgo: '{n}m ago', hoursAgo: '{n}h ago', daysAgo: '{n}d ago',
  chats: 'Chats', ideas: 'Ideas', todos: 'Todos', docs: 'Docs', projects: 'Projects', me: 'Me',
  settings: 'Settings', search: 'Search', newChat: 'New chat', library: 'Library', wrap: 'Wrap',
  sync: 'Sync', rollback: 'Rollback', send: 'Send', stop: 'Stop', attach: 'Attach', model: 'Model',
  effort: 'Thinking', permission: 'Permissions', default: 'Default', customize: 'Customize app',
  reorganize: 'Reorganize library', updates: 'Updates', addTodo: 'Add a todo…', save: 'Save', cancel: 'Cancel',
  edit: 'Edit', delete: 'Delete', close: 'Close', theme: 'Theme', mode: 'Appearance', language: 'Language',
  light: 'Day', dark: 'Night', system: 'System', saudi: 'Saudi', contrast: 'High contrast', tech: 'High tech',
  unwrapped: 'unwrapped', typeMessage: 'Message… (@ to mention a file, ⌘↩ to send)',
  wrapNagTitle: 'Time to wrap', next: 'Next', back: 'Back', finish: 'Finish', modules: 'Modules',
  addLater: 'Add later', enable: 'Enable', enabled: 'Enabled', general: 'General', ai: 'AI',
  git: 'Sync & git', remotes: 'Remotes', daemon: 'Background service', appLayer: 'App layer',
  noLibrary: 'No library yet', noChats: 'No chats yet — start one.', views: 'Views',
  wrapNagBody: 'You have {n} chats that aren’t saved into your library yet. Wrapping distills them into your ideas, notes and todos, then commits — so nothing you thought through evaporates.',
};

const ar: Partial<Record<keyof typeof en, string>> = {
  home: 'الرئيسية', heroSub: 'ملاحظاتك وأفكارك ومهامك في مكان واحد. اسأل أي شيء، فكّر بصوت عالٍ، واحفظ ما يهم عبر الحفظ.', continueSub: 'الأفكار التي عملت عليها مؤخراً.', linkFolder: 'اربط مجلداً', goodMorning: 'صباح الخير', goodAfternoon: 'مساء الخير', goodEvening: 'مساء الخير',
  askPlaceholder: 'اسأل أي شيء، أو فكّر بصوت عالٍ…', captureIdea: 'التقط فكرة', addTodoChip: 'أضف مهمة', checkIn: 'مراجعة',
  activeIdeas: 'أفكار نشطة', openTodos: 'مهام مفتوحة', toWrapAction: 'محادثات بانتظار الحفظ', allSaved: 'كل المحادثات محفوظة',
  notesInLibrary: 'ملاحظات في المكتبة', continueWhere: 'تابع من حيث توقفت', seeAll: 'عرض الكل', recentChats: 'أحدث المحادثات',
  noIdeasYet: 'لا أفكار بعد — التقط واحدة بالأعلى.', nothingOpen: 'لا شيء مفتوح.', manage: 'إدارة', chatsLower: 'محادثات',
  noProjects: 'اربط مجلداً لتعمل عليه مع الذكاء الاصطناعي.', justNow: 'الآن', minutesAgo: 'قبل {n} د', hoursAgo: 'قبل {n} س', daysAgo: 'قبل {n} يوم',
  chats: 'المحادثات', ideas: 'الأفكار', todos: 'المهام', docs: 'المستندات', projects: 'المشاريع', me: 'أنا',
  settings: 'الإعدادات', search: 'بحث', newChat: 'محادثة جديدة', library: 'المكتبة', wrap: 'تلخيص وحفظ',
  sync: 'مزامنة', rollback: 'استرجاع', send: 'إرسال', stop: 'إيقاف', attach: 'إرفاق', model: 'النموذج',
  effort: 'التفكير', permission: 'الصلاحيات', default: 'افتراضي', customize: 'تخصيص التطبيق',
  reorganize: 'إعادة تنظيم المكتبة', updates: 'التحديثات', addTodo: 'أضف مهمة…', save: 'حفظ', cancel: 'إلغاء',
  edit: 'تعديل', delete: 'حذف', close: 'إغلاق', theme: 'السمة', mode: 'المظهر', language: 'اللغة',
  light: 'نهاري', dark: 'ليلي', system: 'النظام', saudi: 'سعودي', contrast: 'تباين عالٍ', tech: 'تقني',
  unwrapped: 'غير محفوظة', typeMessage: 'اكتب رسالة… (@ للإشارة إلى ملف، ⌘↩ للإرسال)',
  wrapNagTitle: 'حان وقت الحفظ', next: 'التالي', back: 'رجوع', finish: 'إنهاء', modules: 'الوحدات',
  addLater: 'لاحقاً', enable: 'تفعيل', enabled: 'مفعّل', general: 'عام', ai: 'الذكاء الاصطناعي',
  git: 'المزامنة وgit', remotes: 'المستودعات البعيدة', daemon: 'الخدمة الخلفية', appLayer: 'طبقة التطبيق',
  noLibrary: 'لا توجد مكتبة بعد', noChats: 'لا توجد محادثات — ابدأ واحدة.', views: 'العروض',
  wrapNagBody: 'لديك {n} محادثات لم تُحفظ في مكتبتك بعد. الحفظ يلخصها في أفكارك وملاحظاتك ومهامك ثم يثبتها — حتى لا يضيع شيء فكرت فيه.',
};

export type Key = keyof typeof en;

export function t(k: Key, vars: Record<string, string | number> = {}): string {
  let s = (app.settings?.language === 'ar' ? ar[k] : undefined) ?? en[k] ?? k;
  for (const [n, v] of Object.entries(vars)) s = s.replace(`{${n}}`, String(v));
  return s;
}

export function ago(when: string | number): string {
  const m = Math.max(0, Math.round((Date.now() - new Date(when).getTime()) / 60000));
  if (m < 1) return t('justNow');
  if (m < 60) return t('minutesAgo', { n: m });
  if (m < 60 * 24) return t('hoursAgo', { n: Math.round(m / 60) });
  return t('daysAgo', { n: Math.round(m / 1440) });
}
