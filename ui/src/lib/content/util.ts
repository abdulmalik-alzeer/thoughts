// Small helpers shared by the content views (Chat, Docs, Todos, Ideas, Home).
import { libraryBlob } from '../api';
import { toast } from '../store.svelte';
import { t, fmtNum, fmtDate } from '../i18n';
import { failed } from '../errors';

export const MAX_FILE_BYTES = 25 * 1024 * 1024;

export type ToastOpts = { action?: { label: string; run: () => void }; kind?: 'error' | 'info' | 'success'; detail?: string; key?: string; duration?: number };
/** `toast(msg, opts?)` — the options are ignored by older one-argument versions of the store. */
export const notify = (msg: string, opts?: ToastOpts) => (toast as (m: string, o?: ToastOpts) => void)(msg, opts);
/** Error toast with a plain headline; the background service's own message is the detail, never the headline.
 *  Prefer `failed(t('…'), e)` from lib/errors.ts, which names the action that failed. */
export const fail = (e: unknown, headline?: string) => failed(headline ?? t('ui.failedGeneric'), e);

export async function copyText(text: string): Promise<boolean> {
  try { await navigator.clipboard.writeText(text); return true; } catch { /* fall through */ }
  try {
    const ta = document.createElement('textarea');
    ta.value = text; ta.style.position = 'fixed'; ta.style.opacity = '0';
    document.body.appendChild(ta); ta.select();
    const ok = document.execCommand('copy');
    ta.remove();
    return ok;
  } catch { return false; }
}

export function fmtSize(bytes: number): string {
  if (bytes < 1024) return t('content.bytes', { n: fmtNum(bytes) });
  if (bytes < 1024 * 1024) return t('content.kb', { n: fmtNum(Math.round(bytes / 1024)) });
  return t('content.mb', { n: fmtNum(Math.round((bytes / 1024 / 1024) * 10) / 10) });
}

export function readAsDataURL(f: Blob): Promise<string> {
  return new Promise((res, rej) => {
    const r = new FileReader();
    r.onload = () => res(r.result as string);
    r.onerror = () => rej(r.error ?? new Error('read failed'));
    r.readAsDataURL(f);
  });
}

export const isImageName = (name: string) => /\.(png|jpe?g|gif|webp|svg|bmp|avif)$/i.test(name);

// ---- todos: due dates ( ` @due(YYYY-MM-DD)` suffix on the task line) ----
const DUE = /\s*@due\((\d{4}-\d{2}-\d{2})\)\s*$/;

export function parseDue(text: string): { title: string; due: string | null } {
  const m = text.match(DUE);
  return m ? { title: text.replace(DUE, ''), due: m[1] } : { title: text, due: null };
}
export function withDue(text: string, due: string | null): string {
  const base = text.replace(DUE, '').trimEnd();
  return due ? `${base} @due(${due})` : base;
}
/** Local calendar date as YYYY-MM-DD, `offset` days from today. */
export function isoDay(offset = 0): string {
  const d = new Date();
  d.setDate(d.getDate() + offset);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}
export type DueState = 'overdue' | 'today' | 'soon' | 'later';
export function dueState(due: string): DueState {
  const today = isoDay();
  if (due < today) return 'overdue';
  if (due === today) return 'today';
  return due <= isoDay(7) ? 'soon' : 'later';
}
/** "Today", "Tomorrow" or a short date. Overdue wording is added by the caller (never colour alone). */
export function dueLabel(due: string): string {
  if (due === isoDay()) return t('todo.dueToday');
  if (due === isoDay(1)) return t('todo.dueTomorrow');
  const sameYear = due.slice(0, 4) === isoDay().slice(0, 4);
  return fmtDate(due + 'T12:00:00', sameYear ? { day: 'numeric', month: 'short' } : { day: 'numeric', month: 'short', year: 'numeric' });
}
/** Normalises an API todo item so views work whether or not the daemon sends `due` / `title`. */
export function normTodo<T extends { text: string; due?: string | null; title?: string }>(it: T): T & { due: string | null; title: string } {
  const p = parseDue(it.text);
  return { ...it, due: it.due ?? p.due, title: it.title ?? p.title };
}

// ---- library files ----
/** Fetches a library file as a blob URL (needs the daemon's `GET /api/library/raw?path=`). Null when unavailable. */
export async function rawBlobUrl(path: string): Promise<string | null> {
  const b = await libraryBlob(path);
  return b ? URL.createObjectURL(b) : null;
}

/** "notes/my-first_note.md" → "My first note" */
export function prettyName(path: string): string {
  const base = (path.split('/').pop() || path).replace(/\.[a-z0-9]+$/i, '');
  if (/^readme$/i.test(base)) {
    const parent = path.split('/').slice(-2, -1)[0];
    if (parent) return prettyName(parent);
  }
  const s = base.replace(/[-_]+/g, ' ').trim();
  return s ? s[0].toUpperCase() + s.slice(1) : base;
}
