// One shared clock for everything that depends on today's date or the hour: the calendar's
// "today", due-date badges, Home's greeting and date, and the due-today reminders. It is checked
// every minute and whenever the window comes back (after sleep, a timer can be far behind), so
// leaving the app open overnight never shows yesterday.
import { isoDay } from './content/util';

export const clock = $state({ day: isoDay(), hour: new Date().getHours() });

const newDay = new Set<() => void>();

function check() {
  const day = isoDay(), hour = new Date().getHours();
  if (hour !== clock.hour) clock.hour = hour;
  if (day !== clock.day) {
    clock.day = day;
    for (const f of newDay) { try { f(); } catch (e) { console.error(e); } }
  }
}

/** Run `f` whenever the date changes while the app is open. Returns a function that stops it. */
export function onNewDay(f: () => void): () => void {
  newDay.add(f);
  return () => { newDay.delete(f); };
}

if (typeof window !== 'undefined') {
  setInterval(check, 60 * 1000);
  window.addEventListener('focus', check);
  document.addEventListener('visibilitychange', () => { if (!document.hidden) check(); });
}
