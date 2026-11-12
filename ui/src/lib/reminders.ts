// Due-date reminders. On launch, every hour, when the date changes, when the window comes back after
// a while (for example after the computer slept), and whenever todos change, open todos that are due today
// or overdue are counted (for the badge on the Todos item) and — if notifications are on — announced
// with a system notification, once per todo per day. What was already announced is remembered locally.
// Todos due today are announced from the reminder time chosen in Settings (09:00 unless changed; the
// same time the exported calendar uses), so a window left open overnight does not ring at midnight.
import { api, inTauri, on } from './api';
import { app } from './store.svelte';
import { t, tn } from './i18n';
import { isoDay, normTodo } from './content/util';
import { onNewDay } from './clock.svelte';

const KEY = 'thoughts.reminded';
type Seen = { day: string; ids: string[] };

function seen(): Seen {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) || 'null');
    if (s && s.day === isoDay() && Array.isArray(s.ids)) return s;
  } catch { /* fall through */ }
  return { day: isoDay(), ids: [] };
}
function remember(s: Seen) { try { localStorage.setItem(KEY, JSON.stringify({ day: s.day, ids: s.ids.slice(-500) })); } catch { /* private mode */ } }

/** Asks for permission at most once; afterwards only reports whether it was granted. */
async function send(title: string, body: string): Promise<boolean> {
  try {
    if (inTauri()) {
      const n = await import('@tauri-apps/plugin-notification');
      let ok = await n.isPermissionGranted();
      if (!ok) {
        let asked = false;
        try { asked = localStorage.getItem('thoughts.notifAsked') === '1'; localStorage.setItem('thoughts.notifAsked', '1'); } catch { /* ignore */ }
        if (asked) return false;
        ok = (await n.requestPermission()) === 'granted';
      }
      if (!ok) return false;
      n.sendNotification({ title, body });
      return true;
    }
    // Browser preview: only when permission was already given (never prompt from a timer).
    if (typeof Notification !== 'undefined' && Notification.permission === 'granted') { new Notification(title, { body }); return true; }
  } catch { /* notifications unavailable */ }
  return false;
}

let running = false;
let lastCheck = 0;
let atTimer: ReturnType<typeof setTimeout> | undefined;

/** Minutes after midnight of the reminder time, or null when there is none (announce at once). */
function reminderMinutes(): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(String(app.settings?.calendar?.alarm ?? ''));
  return m ? Math.min(23, +m[1]) * 60 + Math.min(59, +m[2]) : null;
}
export async function checkReminders(): Promise<void> {
  if (running || !app.settings?.onboarded) return;
  running = true;
  lastCheck = Date.now();
  try {
    const lists: any[] = await api('/todos');
    const today = isoDay();
    const due: { id: string; title: string; overdue: boolean }[] = [];
    for (const l of Array.isArray(lists) ? lists : []) {
      for (const raw of l.items ?? []) {
        const it = normTodo(raw);
        if (it.done || !it.due || it.due > today) continue;
        due.push({ id: `${l.file}|${it.title}|${it.due}`, title: it.title, overdue: it.due < today });
      }
    }
    app.dueToday = due.length;
    if (!app.settings?.notifications || !due.length) return;
    // Before the reminder time only what is already overdue is announced; look again at that time.
    const at = reminderMinutes();
    const now = new Date();
    const early = at !== null && now.getHours() * 60 + now.getMinutes() < at;
    if (early) {
      clearTimeout(atTimer);
      atTimer = setTimeout(checkReminders, ((at! - now.getHours() * 60 - now.getMinutes()) * 60 - now.getSeconds()) * 1000 + 1000);
    }
    const s = seen();
    const fresh = due.filter((d) => !s.ids.includes(d.id) && (!early || d.overdue));
    if (!fresh.length) return;
    let sent: boolean;
    if (fresh.length <= 3) {
      sent = true;
      for (const d of fresh) sent = (await send(t(d.overdue ? 'ui.rem.overdueTitle' : 'ui.rem.todayTitle'), d.title)) && sent;
    } else {
      const nOver = fresh.filter((d) => d.overdue).length;
      sent = await send(tn('ui.rem.manyTitle', fresh.length), [nOver ? tn('ui.rem.overdueCount', nOver) : '', fresh.slice(0, 3).map((d) => d.title).join(' · ')].filter(Boolean).join(' — '));
    }
    // Only remember what was really shown, so a refused permission can still be granted later today.
    if (sent) { s.ids.push(...fresh.map((d) => d.id)); remember(s); }
  } catch { /* offline or no library: try again at the next tick */ }
  finally { running = false; }
}

let started = false;
/** Start the hourly check (idempotent). Called once the app is set up. */
export function startReminders() {
  if (started) return;
  started = true;
  checkReminders();
  setInterval(checkReminders, 60 * 60 * 1000);
  onNewDay(checkReminders);
  // Back at the computer: an hourly timer may have been asleep with it, so look again.
  const back = () => { if (Date.now() - lastCheck > 5 * 60 * 1000) checkReminders(); };
  window.addEventListener('focus', back);
  document.addEventListener('visibilitychange', () => { if (!document.hidden) back(); });
  let timer: ReturnType<typeof setTimeout> | undefined;
  on((e) => {
    if (e.type !== 'todos.changed' && e.type !== 'library.changed' && e.type !== 'resync') return;
    clearTimeout(timer);
    timer = setTimeout(checkReminders, 1500);
  });
}
