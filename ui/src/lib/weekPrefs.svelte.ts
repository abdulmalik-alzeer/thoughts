// The user's week and calendar: the first day of the week, the weekend, and whether Hijri (Umm
// al-Qura) dates are shown beside Gregorian ones. Days are counted from Sunday = 0, like Date.getDay().
//
// Each comes from, in order: Settings › General › Calendar; the library's own `Week:` line (the
// working week chosen at setup, read by the background service); the computer's region; the app
// language (Arabic: a Sunday–Thursday week with a Friday–Saturday weekend).
import { api } from './api';
import { app } from './store.svelte';
import { lang } from './i18n';
import { isoDay } from './content/util';

type LibWeek = { start: number | null; working: number[] | null };
const lib = $state<{ week: LibWeek | null }>({ week: null });
let asked = false;

/** Ask the service for the library's `Week:` line once (again after `refreshWeek`). */
export function loadLibraryWeek() {
  if (asked) return;
  asked = true;
  api<LibWeek>('/todos/week').then((w) => { lib.week = w && typeof w === 'object' ? w : null; }).catch(() => { asked = false; });
}
export function refreshWeek() { asked = false; loadLibraryWeek(); }

type WeekInfo = { firstDay?: number; weekend?: number[] };
/** The region's week, where the browser engine knows it (Intl.Locale weekInfo, 1 = Monday … 7 = Sunday). */
function regionWeek(): WeekInfo | null {
  try {
    const loc: any = new Intl.Locale(typeof navigator !== 'undefined' ? navigator.language : 'en');
    const w: WeekInfo | undefined = typeof loc.getWeekInfo === 'function' ? loc.getWeekInfo() : loc.weekInfo;
    return w ?? null;
  } catch { return null; }
}
const fromIntl = (d: number) => d % 7; // Intl's 7 (Sunday) → 0

const cal = () => app.settings?.calendar ?? {};
const valid = (d: any) => Number.isInteger(d) && d >= 0 && d < 7;

/** The first day of the week. */
export const weekStart = (): number => (valid(cal().weekStart) ? cal().weekStart : autoWeekStart());
/** The first day of the week without a setting (what "Automatic" means). */
export function autoWeekStart(): number {
  if (valid(lib.week?.start)) return lib.week!.start!;
  const r = regionWeek();
  if (r?.firstDay && lang() !== 'ar') return fromIntl(r.firstDay);
  return lang() === 'ar' ? 0 : 1;
}

/** The weekend days. */
export function weekend(): number[] {
  const set = cal().weekend;
  return Array.isArray(set) && set.every(valid) ? set : autoWeekend();
}
/** The weekend without a setting (what "Automatic" means). */
export function autoWeekend(): number[] {
  const working = lib.week?.working;
  if (Array.isArray(working) && working.length && working.every(valid)) return [0, 1, 2, 3, 4, 5, 6].filter((d) => !working.includes(d));
  const r = regionWeek();
  if (r?.weekend?.length && lang() !== 'ar') return r.weekend.map(fromIntl);
  return lang() === 'ar' ? [5, 6] : [6, 0];
}

/** Show Hijri dates beside Gregorian ones. */
export const showHijri = () => cal().dates === 'both';

/** The first day of next week, as YYYY-MM-DD (for "Next week"). */
export function nextWeekDay(): string {
  const now = new Date();
  const k = (7 + weekStart() - now.getDay()) % 7 || 7;
  return isoDay(k);
}
