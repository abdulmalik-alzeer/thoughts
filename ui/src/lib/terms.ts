// The words the app uses for the "Ideas" area follow the user's line of work (settings.profession),
// and `nav.json` labels from the customisation layer override any built-in label.
//
// Each profession has a word for its work items, matching the starter presets: accountants have
// clients, lawyers matters, consultants engagements, teachers classes, researchers and founders
// projects. People in software and technical work have projects too (the kit has no preset for
// them yet). With a work word the page is "Work", one tab per area of the library; without one it
// stays "Ideas".
import { app } from './store.svelte';
import { lang, t, tn } from './i18n';
import { prettyName } from './content/util';
import prof from './i18n/layer';

export type AreaWord = 'ideas' | 'clients' | 'matters' | 'engagements' | 'classes' | 'projects';
const BY_ID: Record<string, AreaWord> = {
  finance: 'clients', accounting: 'clients', legal: 'matters', consulting: 'engagements',
  teaching: 'classes', research: 'projects', founder: 'projects', technical: 'projects',
};
// What the setup screen offers is stored either as its id or, by older versions, as its label (in
// either language); a profession typed by hand is matched on its words.
const BY_WORDS: [RegExp, AreaWord][] = [
  [/account|bookkeep|financ|\btax|audit|محاسب|مالي|ضريب/i, 'clients'],
  [/\blaw|legal|lawyer|attorney|counsel|paralegal|محام|قانون|قضا/i, 'matters'],
  [/consult|advis|استشار|مستشار/i, 'engagements'],
  [/teach|lectur|tutor|trainer|instruct|تدريس|معلّ?م|مدرّ?س|محاضر|مدرّ?ب/i, 'classes'],
  [/research|study|student|academ|thesis|founder|product|startup|بحث|باحث|دراس|طالب|أكاديم|تأسيس|منتج|ريادة/i, 'projects'],
  [/develop|engineer|software|programm|technical|devops|برمج|مهندس|تقني|مطوّ?ر/i, 'projects'],
];

function professionId(raw: string): string {
  const v = raw.trim().toLowerCase();
  if (BY_ID[v] || !v) return v;
  const dicts = [prof.en, prof.ar] as Record<string, string>[];
  for (const id of Object.keys(BY_ID)) {
    if (dicts.some((d) => (d[`layer.prof.${id}`] ?? '').toLowerCase() === v)) return id;
  }
  return v;
}

/** Which word family the Ideas area uses for this user. */
export function areaWord(): AreaWord {
  const id = professionId(String(app.settings?.profession ?? app.status?.profession ?? ''));
  if (!id || id === 'personal' || id === 'other') return 'ideas';
  return BY_ID[id] ?? BY_WORDS.find(([re]) => re.test(id))?.[1] ?? 'ideas';
}

/** A string about the Ideas area in the user's words: `term('title')` → "Clients", `term('active')` → "Active clients"…
 *  Forms: title · nav (page name) · active (stat label, use `termN`) · emptyTitle · emptyBody · capture · newPlaceholder ·
 *  continueSub · homeEmpty. Keys live in i18n/ui.ts as `term.<word>.<form>`. */
export function term(form: string, vars: Record<string, string | number> = {}): string {
  const key = `term.${areaWord()}.${form}`;
  const s = t(key, vars);
  return s === key ? t(`term.ideas.${form}`, vars) : s;
}

/** Plural-aware form, e.g. `termN('active', 3)` → "Active clients" (the label only; show the number beside it). */
export function termN(form: string, n: number): string {
  const key = `term.${areaWord()}.${form}`;
  const s = tn(key, n);
  return s === key ? tn(`term.ideas.${form}`, n) : s;
}

// The capture words the assistant's routines recognise ("new client: …"), and how they read in Arabic.
const TRIGGER: Record<AreaWord, { en: string; ar: string }> = {
  ideas: { en: 'new idea: ', ar: 'فكرة جديدة: ' },
  clients: { en: 'new client: ', ar: 'عميل جديد: ' },
  matters: { en: 'new matter: ', ar: 'قضية جديدة: ' },
  engagements: { en: 'new engagement: ', ar: 'مشروع استشاري جديد: ' },
  classes: { en: 'new class: ', ar: 'فصل جديد: ' },
  projects: { en: 'new project: ', ar: 'مشروع جديد: ' },
};

/** What starts a message that adds a new item of the user's kind: in English (`'en'`, what is sent),
 *  or in the app language (what is put in a box for the user to finish). */
export function newItemTrigger(language: 'en' | 'ar' = lang()): string {
  return TRIGGER[areaWord()][language];
}

/** Is this item one of the user's work items (a client, a matter…) rather than an idea or a reference page?
 *  Judged on its `kind:` when the item has one, otherwise on its area. Without a work word, every item counts. */
export function isWorkItem(i: { kind?: string; area?: string }): boolean {
  const w = areaWord();
  if (w === 'ideas') return true;
  const kind = String(i.kind ?? '').trim().toLowerCase();
  if (kind) return kind !== 'idea';
  return areaKey(String(i.area ?? '')) === w;
}

// Friendly names for the area folders the presets create (English folder names; Arabic ones are shown as written).
const AREAS: Record<string, { en: string; ar: string }> = {
  ideas: { en: 'Ideas', ar: 'الأفكار' }, clients: { en: 'Clients', ar: 'العملاء' }, matters: { en: 'Matters', ar: 'القضايا' },
  engagements: { en: 'Engagements', ar: 'المشاريع الاستشارية' }, classes: { en: 'Classes', ar: 'الفصول' },
  courses: { en: 'Courses', ar: 'المقررات' }, projects: { en: 'Projects', ar: 'المشاريع' }, proposals: { en: 'Proposals', ar: 'العروض' },
  playbook: { en: 'Playbook', ar: 'المنهجيات' }, reference: { en: 'Reference', ar: 'المراجع' }, periods: { en: 'Periods', ar: 'الفترات' },
  knowledge: { en: 'Knowledge', ar: 'المعرفة' }, units: { en: 'Units', ar: 'الوحدات' }, resources: { en: 'Resources', ar: 'المصادر' },
  admin: { en: 'Admin', ar: 'الإدارة' }, reading: { en: 'Reading', ar: 'القراءات' }, methods: { en: 'Methods', ar: 'الطرائق' },
  customers: { en: 'Customers', ar: 'العملاء' }, business: { en: 'Business', ar: 'الأعمال' }, decisions: { en: 'Decisions', ar: 'القرارات' },
};
// Arabic folder names map back to the same areas, so "is this the clients area?" works in both.
const AR_AREAS: Record<string, string> = Object.fromEntries(Object.entries(AREAS).filter(([k]) => k !== 'customers').map(([k, v]) => [v.ar, k]));

/** The English key of an area folder (`العملاء` → `clients`), or the folder name itself. */
export function areaKey(area: string): string {
  const a = area.trim();
  return AREAS[a.toLowerCase()] ? a.toLowerCase() : AR_AREAS[a] ?? a;
}

/** A heading for an area folder in the app language. */
export function areaLabel(area: string): string {
  const known = AREAS[area.trim().toLowerCase()];
  if (known) return lang() === 'ar' ? known.ar : known.en;
  return prettyName(area);
}

/** Label for a sidebar item / view id: nav.json label → profession word (ideas) → built-in name. */
export function navLabel(id: string, fallback?: string): string {
  const l = app.nav?.labels?.[id];
  const custom = l && (lang() === 'ar' ? l.ar || l.en : l.en || l.ar);
  if (custom) return String(custom).slice(0, 40);
  if (id === 'ideas') return term('nav');
  return fallback ?? t(id);
}
