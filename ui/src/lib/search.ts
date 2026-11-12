// Search across notes and chats (daemon: GET /api/search), plus the hand-off that lets an opened
// result show the find bar with the words highlighted.
import { api } from './api';
import { fold } from './CommandPalette.svelte';

export type Snip = { text: string; marks?: [number, number][]; line?: number; msg?: number; role?: string };
export type NoteHit = { path: string; title?: string | null; matches: Snip[]; nameHit?: boolean; modified?: string };
export type ChatHit = { id: string; title: string; projectId?: string | null; updated?: string; titleHit?: boolean; matches: Snip[] };
export type Results = { terms: string[]; notes: NoteHit[]; chats: ChatHit[] };

// The last few searches, so going Back to a results page shows it at once.
const recent = new Map<string, Results>();

export function cached(q: string): Results | undefined { return recent.get(q.trim()); }

export async function searchAll(q: string, limit = 40): Promise<Results> {
  const key = q.trim();
  const r = await api(`/search?q=${encodeURIComponent(key)}&limit=${limit}`);
  const out: Results = { terms: r?.terms ?? [], notes: Array.isArray(r?.notes) ? r.notes : [], chats: Array.isArray(r?.chats) ? r.chats : [] };
  if (limit >= 40) {
    recent.delete(key);
    recent.set(key, out);
    while (recent.size > 8) recent.delete(recent.keys().next().value!);
  }
  return out;
}

/** Text split into plain and marked parts (`marks` are [start, end) offsets into `text`). */
export function segments(text: string, marks?: [number, number][]): { t: string; m: boolean }[] {
  const out: { t: string; m: boolean }[] = [];
  let at = 0;
  for (const [a, b] of [...(marks ?? [])].sort((x, y) => x[0] - y[0])) {
    if (a < at || b > text.length || b <= a) continue;
    if (a > at) out.push({ t: text.slice(at, a), m: false });
    out.push({ t: text.slice(a, b), m: true });
    at = b;
  }
  if (at < text.length) out.push({ t: text.slice(at), m: false });
  return out;
}

/** `text` folded like `fold()`, with the index in `text` each folded character came from. */
export function foldMap(text: string): { s: string; from: number[] } {
  // Most text folds one character to one character: no need to go letter by letter.
  const whole = fold(text);
  if (whole.length === text.length && !/[\u0300-\u036f\u064b-\u065f\u0670\u0610-\u061a\u0640]/.test(text)) {
    return { s: whole, from: Array.from({ length: text.length }, (_, i) => i) };
  }
  let s = '';
  const from: number[] = [];
  let i = 0;
  for (const ch of text) {
    const f = fold(ch);
    for (let k = 0; k < f.length; k++) from.push(i);
    s += f;
    i += ch.length;
  }
  return { s, from };
}

/** Where the (already folded) `terms` occur in `text`, for highlighting a title or a path. */
export function markTerms(text: string, terms: string[]): [number, number][] {
  if (!text || !terms.length) return [];
  const { s, from } = foldMap(text);
  const found: [number, number][] = [];
  for (const t of terms) {
    if (!t) continue;
    for (let i = s.indexOf(t); i >= 0; i = s.indexOf(t, i + t.length)) {
      const last = from[i + t.length - 1];
      const code = text.charCodeAt(last);
      found.push([from[i], last + (code >= 0xd800 && code <= 0xdbff ? 2 : 1)]);
    }
  }
  found.sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const r of found) { const l = merged[merged.length - 1]; if (l && r[0] <= l[1]) l[1] = Math.max(l[1], r[1]); else merged.push([...r]); }
  return merged;
}

/** The words to look for once a result is open (quotes dropped: the find bar tries the phrase first). */
export const findWords = (q: string) => q.replace(/["“”«»]/g, ' ').replace(/\s+/g, ' ').trim();

// ---------- hand-off to the find bar ----------

/** What a view should look for right after it opens: `where` is `chat:<id>` or `doc:<path>`;
 *  `msg` (chats) is the position of the matching message among the shown messages. */
type Pending = { where: string; query: string; msg?: number; at: number };
let pending: Pending | null = null;

export function findOnOpen(where: string, query: string, msg?: number) {
  pending = query.trim() ? { where, query: query.trim(), msg, at: Date.now() } : null;
}

/** The hand-off for `where`, if there is a fresh one (used once). */
export function takeFind(where: string): Pending | null {
  const p = pending;
  if (!p || p.where !== where || Date.now() - p.at > 15000) return null;
  pending = null;
  return p;
}

// ---------- the find command ----------

/** `open` answers false when it has nothing to search right now (the next one down is asked). */
type FindTarget = { open: (query?: string) => boolean };
const targets: FindTarget[] = [];

/** A find bar (or a search page) registers itself while it is on screen; the newest one answers ⌘F. */
export function registerFind(t: FindTarget): () => void {
  targets.push(t);
  return () => { const i = targets.indexOf(t); if (i >= 0) targets.splice(i, 1); };
}

/** Open the find bar of the open note or chat. False when there is none. */
export function openFind(query?: string): boolean {
  for (let i = targets.length - 1; i >= 0; i--) if (targets[i].open(query)) return true;
  return false;
}
