// Usage: the plans' limits (5-hour, weekly…) and the tokens the assistants used, with what they
// would cost at API prices. The figures come from the service (`meter.rs`); this file loads them,
// follows `meter.changed`, and words them.
import { api, on } from './api';
import { app } from './store.svelte';
import { fmtDate, fmtMoney, lang, t } from './i18n';
import type { WatchOne } from './planView';

export const PROVIDERS = ['claude', 'codex'] as const;
export type Provider = (typeof PROVIDERS)[number];

export type Win = { id: string; percent: number | null; resetsAt: number | null; minutes: number | null; status: string | null; seenAt: number; hasReset: boolean; /** What a window for one model or product is for, in the service's words. */ label?: string | null };
export type Plan = { windows: Win[]; seenAt: number } | null;
export type Tok = { input: number; output: number; cacheRead: number; cacheWrite: number };
export type Use = { tokens: Tok; totalTokens: number; costUsd: number; unpricedTokens: number; estimated: boolean; count: number };

export const meter = $state({
  /** Bumped whenever the service says the figures changed; views reload on it. */
  tick: 0,
  /** The plans' limits as the service last knew them, for every view at once (`null`: not asked yet). */
  plans: null as Record<Provider, Plan> | null,
  /** Whether the service could ask each assistant for its plan's usage without a run (`plan_watch.rs`). */
  watch: null as Record<Provider, WatchOne> | null,
  /** The time, moved along once a minute so "as of" appears by itself when figures grow old. */
  now: Date.now(),
});

function took(r: any) { if (r?.plans) meter.plans = r.plans; if (r?.watch) meter.watch = r.watch; meter.now = Date.now(); }
/** The plans' limits, read from the service (which also tells it a window is there). */
export const loadPlans = () => api<any>('/meter/plans').then(took).catch(() => {});
/** The window opened or the set-up changed: ask the service for fresh figures (it decides how often is too often). */
export const refreshPlans = () => api<any>('/meter/refresh', { method: 'POST' }).then(took).catch(() => {});

let listening = false;
let quiet: ReturnType<typeof setTimeout> | undefined;
/** Follow `meter.changed` (once for the whole window), and keep the plans' limits on screen without
 *  anyone asking: fresh when the window opens or comes back, read again while it is in view. */
export function followMeter() {
  if (listening) return;
  listening = true;
  on((e) => {
    // A sign-in, or a change in Settings (another service, the switch for this): what can be asked has changed.
    if (e?.type === 'auth.changed' || e?.type === 'settings.changed') { refreshPlans(); return; }
    if (e?.type !== 'meter.changed') return;
    meter.tick++;
    // Runs end in bursts: one read once they have.
    clearTimeout(quiet);
    quiet = setTimeout(loadPlans, 300);
  });
  refreshPlans();
  const seen = () => typeof document === 'undefined' || document.visibilityState === 'visible';
  setInterval(() => { meter.now = Date.now(); }, 60_000);
  // While the window is in view the service keeps the figures current; a hidden one asks for nothing.
  setInterval(() => { if (seen()) loadPlans(); }, 5 * 60_000);
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', () => { if (seen()) refreshPlans(); });
}

/** The usage page's figures for the last `days` days. */
export const loadMeter = (days: number) => api<any>(`/meter?days=${days}`);
/** One chat's figures and the plans' limits. */
export const loadChatMeter = (id: string) => api<any>(`/meter/chat?id=${encodeURIComponent(id)}`);

/** How each assistant stands, for the empty states: not there, not signed in, paying with a key
 *  (no plan limits then), or on a plan. */
export function providerState(p: Provider): 'missing' | 'signedOut' | 'key' | 'plan' | 'unknown' {
  const s: any = p === 'claude' ? app.status?.claude : app.status?.codex;
  if (!s) return 'unknown';
  if (s.installed === false) return 'missing';
  if (p === 'claude') {
    if (s.provider === 'anthropicKey' || s.provider === 'service') return 'key';
    return s.loggedIn ? 'plan' : 'signedOut';
  }
  if (!s.loggedIn) return 'signedOut';
  return s.method && s.method !== 'chatgpt' ? 'key' : 'plan';
}

export const providerName = (p: string) => t(p === 'codex' ? 'r10-usage.codex' : 'r10-usage.claude');

const locale = () => (lang() === 'ar' ? 'ar-SA-u-nu-latn' : 'en');

/** 1234567 → "1.2M" (in the app language's style). */
export const fmtTokens = (n: number) => new Intl.NumberFormat(locale(), { notation: 'compact', maximumFractionDigits: n < 1000 ? 0 : 1 }).format(n || 0);
/** A share 0–100 as "42%". */
export const fmtPct = (p: number) => new Intl.NumberFormat(locale(), { style: 'percent', maximumFractionDigits: 0 }).format(Math.max(0, Math.min(100, p)) / 100);

/** What a sum of use costs, in words: an amount, "about" one when estimated, and "unknown" for
 *  tokens of a model with no published price. */
export function costText(u: Use | null | undefined): string {
  if (!u || !u.count) return fmtMoney(0);
  const known = u.costUsd > 0 || u.unpricedTokens === 0;
  if (!known) return t('r10-usage.unknown');
  const amount = u.estimated ? t('r10-usage.about', { amount: fmtMoney(u.costUsd) }) : fmtMoney(u.costUsd);
  return u.unpricedTokens > 0 ? t('r10-usage.plusUnknown', { amount }) : amount;
}

export function winLabel(w: Win): string {
  if (w.id === 'five_hour') return t('r10-usage.win.fiveHour');
  if (w.id === 'seven_day') return t('r10-usage.win.week');
  if (w.id === 'seven_day_opus') return t('r10-usage.win.weekModel', { model: 'Opus' });
  if (w.id === 'seven_day_sonnet') return t('r10-usage.win.weekModel', { model: 'Sonnet' });
  if (w.id === 'seven_day_overage_included') return t('r10-usage.win.weekExtra');
  if (w.label && w.id.startsWith('seven_day_')) return t('r10-usage.win.weekModel', { model: w.label });
  const m = w.minutes ?? (/^m(\d+)$/.exec(w.id)?.[1] ? Number(/^m(\d+)$/.exec(w.id)![1]) : null);
  if (m && m % 1440 === 0) return t('r10-usage.win.days', { n: m / 1440 });
  if (m && m % 60 === 0) return t('r10-usage.win.hours', { n: m / 60 });
  return t('r10-usage.win.other');
}

const time = (ms: number) => fmtDate(ms, { hour: 'numeric', minute: '2-digit' });
const dayTime = (ms: number) => fmtDate(ms, { weekday: 'short', day: 'numeric', month: 'short', hour: 'numeric', minute: '2-digit' });
const sameDay = (a: number, b: number) => new Date(a).toDateString() === new Date(b).toDateString();

/** "in 2 h 5 min" / "in 3 days" from now. */
function until(ms: number, now: number): string {
  const min = Math.max(1, Math.round((ms - now) / 60000));
  const rtf = new Intl.RelativeTimeFormat(locale(), { numeric: 'always', style: 'long' });
  if (min < 60) return rtf.format(min, 'minute');
  if (min < 48 * 60) return rtf.format(Math.round(min / 60), 'hour');
  return rtf.format(Math.round(min / 1440), 'day');
}

/** When a window resets, in words: "Resets 3:40 pm (in 2 hours)". */
export function resetText(w: Win, now = Date.now()): string {
  if (w.hasReset) return t('r10-usage.hasReset');
  if (!w.resetsAt) return '';
  const ms = w.resetsAt * 1000;
  const when = sameDay(ms, now) ? time(ms) : dayTime(ms);
  return t('r10-usage.resets', { when, until: until(ms, now) });
}

/** "As of 3:10 pm" — limits are known only from the last run. */
export function asOf(seenAt: number | null | undefined, now = Date.now()): string {
  if (!seenAt) return '';
  const ms = seenAt * 1000;
  return t('r10-usage.asOf', { when: sameDay(ms, now) ? time(ms) : dayTime(ms) });
}

/** How full the ring is: the fullest window that is still in force. */
export function fullest(plan: Plan): Win | null {
  let best: Win | null = null;
  for (const w of plan?.windows ?? []) if (w.percent != null && !w.hasReset && (!best || w.percent > (best.percent ?? 0))) best = w;
  return best;
}

/** "ok" / "warn" (75% or close to the limit) / "full", for the colour of a bar — always with words beside it. */
export function level(w: Win): 'ok' | 'warn' | 'full' {
  if (w.status === 'reached' || (w.percent ?? 0) >= 100) return 'full';
  if (w.status === 'warning' || (w.percent ?? 0) >= 75) return 'warn';
  return 'ok';
}

/** A window's share in words, for screen readers and when no share is known. */
export function shareText(w: Win): string {
  if (w.hasReset) return t('r10-usage.noFigure');
  if (w.percent != null) return t('r10-usage.used', { pct: fmtPct(w.percent) });
  if (w.status === 'reached') return t('r10-usage.status.reached');
  if (w.status === 'warning') return t('r10-usage.status.warning');
  return t('r10-usage.status.ok');
}
