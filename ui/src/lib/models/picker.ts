// What the model picker (ModelPicker.svelte) shows, worked out apart from the screen so it can be
// tested: the rows, their names, their grouping, the search and the keys. Nothing here names a
// model: every id and every name is what the assistant program or the connected service reported
// (`GET /ai/<program>/options`), and a name the app makes itself follows one general rule (below).

/** One model as the picker shows it. */
export interface ModelEntry {
  id: string;
  /** What is shown first: the name the program gave it, else the id (without its group prefix). */
  name: string;
  /** One quiet line from the program about the model (Codex's catalog has one). */
  note: string;
  /** Who makes it, when the ids say so (`anthropic/…`): '' when they do not. */
  group: string;
  /** The program's own choice when none is made. */
  isDefault: boolean;
}

/** The part of `GET /ai/<program>/options` the picker reads. */
export interface ModelSource {
  models?: unknown;
  /** The names the agent gives its models (over ACP). */
  modelNames?: Record<string, unknown> | null;
  /** Codex's catalog: its label, description and which one it starts with. */
  modelInfo?: { id?: unknown; label?: unknown; description?: unknown; isDefault?: unknown }[] | null;
  /** The model the agent starts a new session with (over ACP). */
  defaultModel?: unknown;
}

/** From how many models on the search box is always there and "Recently used" is shown. */
export const LONG_LIST = 9;
/** How many recently used models are kept and shown. */
export const RECENT_MAX = 4;

const str = (v: unknown) => (typeof v === 'string' ? v.trim() : '');

/** `maker/model` → ['maker', 'model'] (the shape services that pass on many makers' models use);
 *  ['', id] for anything else. Only the first `/` counts. */
export function splitId(id: string): [string, string] {
  const i = id.indexOf('/');
  return i > 0 && i < id.length - 1 ? [id.slice(0, i), id.slice(i + 1)] : ['', id];
}

/** The rows for a program's or a service's list, in its order, each once. Grouping by maker is only
 *  switched on when the ids name at least two makers; then the maker is taken off the shown name
 *  (the full id stays underneath). A name is the one reported, never one written into the app. */
export function entriesFrom(src: ModelSource | null | undefined): ModelEntry[] {
  const ids: string[] = [];
  for (const m of Array.isArray(src?.models) ? src!.models as unknown[] : []) { const id = str(m); if (id && !ids.includes(id)) ids.push(id); }
  const info = new Map((Array.isArray(src?.modelInfo) ? src!.modelInfo! : []).map((m) => [str(m?.id), m] as const));
  const makers = new Set(ids.map((id) => splitId(id)[0]).filter(Boolean));
  const grouped = makers.size >= 2;
  const dflt = str(src?.defaultModel);
  return ids.map((id) => {
    const [maker, rest] = splitId(id);
    const said = str(src?.modelNames?.[id]) || str(info.get(id)?.label);
    return {
      id,
      name: said || (grouped && maker ? rest : id),
      note: str(info.get(id)?.description),
      group: grouped ? maker : '',
      isDefault: info.get(id)?.isDefault === true || (!!dflt && dflt === id),
    };
  });
}

/** Is the raw id worth a second line under the name? Only where it says something the name does
 *  not: "Opus" over `opus`, or "Model One" over `model-one`, would only repeat itself. */
export function showsId(e: ModelEntry): boolean {
  const bare = (x: string) => x.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '');
  return bare(e.name) !== bare(e.id);
}

/** A model's name for a button or a sentence: the reported one, else the id as it is. */
export function nameOf(entries: ModelEntry[], id: string | null | undefined): string {
  if (!id) return '';
  const e = entries.find((x) => x.id === id);
  // Inside a group the short name alone ("gpt-4o") can belong to two makers: the full id is clearer.
  return e ? (e.group && e.name === splitId(id)[1] ? id : e.name) : id;
}

/** Does the program's own name for the model already say it is the default ("Default (recommended)")?
 *  Then the list does not say so a second time. */
export function saysDefault(name: string): boolean { return /\bdefault\b/i.test(name); }

/** May this be sent as a model's name? The same shape the service accepts for a typed name. */
export function validName(s: string): boolean { return /^[A-Za-z0-9][A-Za-z0-9._:/@\[\]-]{0,119}$/.test(s); }

const fold = (s: string) => s.toLowerCase();

/** The models matching what was typed: every word typed must be somewhere in the name, the id or
 *  the note. Best first: a name or id that starts with it, then one with a part that starts with
 *  it (`gpt-4o` for "4o"), then the rest; the list's own order inside each. */
export function filterModels(entries: ModelEntry[], query: string): ModelEntry[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (!words.length) return entries;
  const q = words.join(' ');
  const rank = (e: ModelEntry): number => {
    const name = fold(e.name), id = fold(e.id), hay = `${name} ${id} ${fold(e.note)}`;
    if (!words.every((w) => hay.includes(w))) return -1;
    if (name.startsWith(q) || id.startsWith(q) || splitId(id)[1].startsWith(q)) return 0;
    const parts = `${name} ${id}`.split(/[\s/\-_.:@()[\]]+/);
    return words.every((w) => parts.some((p) => p.startsWith(w))) ? 1 : 2;
  };
  return entries.map((e, i) => ({ e, r: rank(e), i })).filter((x) => x.r >= 0).sort((a, b) => a.r - b.r || a.i - b.i).map((x) => x.e);
}

/** One line of the list. `auto` = no choice (the default); `typed` = a name that is not in the list. */
export interface Row {
  kind: 'auto' | 'model' | 'typed';
  /** What picking it sends: '' for `auto`. */
  id: string;
  /** Unique in the list (a recent model, or the current one of a long list, is listed twice: on top and in its place). */
  key: string;
  entry?: ModelEntry;
  /** It is the choice in force now. */
  current: boolean;
}
export interface Section { key: string; /** '' = no heading; `recent`, `all` and `other` are named by the screen; else a maker as the ids give it. */ label: string; kind: 'top' | 'recent' | 'all' | 'other' | 'group' | 'found'; rows: Row[] }

export interface Shown {
  value: string | null | undefined;
  query?: string;
  /** Ids picked lately, newest first (only those still in the list are shown). */
  recents?: string[];
  /** "Automatic" (no choice) is offered, under this name (searchable). */
  auto?: string;
  /** A name outside the list may be typed. */
  typed?: boolean;
}

/** The list as drawn, top to bottom. Nothing typed: Automatic, the current choice if the list does
 *  not have it (in a long list: the current choice always), the recently used (long lists only), then every model, under its maker where the
 *  ids name several. Something typed: the matches in one run, then the typed name itself. */
export function sectionsFor(entries: ModelEntry[], o: Shown): Section[] {
  const value = o.value || '', q = (o.query ?? '').trim();
  const model = (e: ModelEntry, pre = 'm'): Row => ({ kind: 'model', id: e.id, key: `${pre}:${e.id}`, entry: e, current: e.id === value });
  const out: Section[] = [];
  if (q) {
    const rows: Row[] = [];
    if (o.auto && fold(o.auto).includes(fold(q))) rows.push({ kind: 'auto', id: '', key: 'auto', current: !value });
    rows.push(...filterModels(entries, q).map((e) => model(e)));
    if (o.typed && validName(q) && !entries.some((e) => e.id === q)) rows.push({ kind: 'typed', id: q, key: `t:${q}`, current: q === value });
    if (rows.length) out.push({ key: 'found', label: '', kind: 'found', rows });
    return out;
  }
  const top: Row[] = [];
  if (o.auto) top.push({ kind: 'auto', id: '', key: 'auto', current: !value });
  const long = entries.length >= LONG_LIST, mine = entries.find((e) => e.id === value);
  if (value && !mine) top.push({ kind: 'typed', id: value, key: `t:${value}`, current: true });
  // In a long list the choice in force is also shown on top, so it is seen without scrolling.
  if (mine && long) top.push(model(mine, 'c'));
  if (top.length) out.push({ key: 'top', label: '', kind: 'top', rows: top });
  const recent = long ? (o.recents ?? []).filter((id) => id !== value).map((id) => entries.find((e) => e.id === id)).filter((e): e is ModelEntry => !!e).slice(0, RECENT_MAX) : [];
  if (recent.length) out.push({ key: 'recent', label: 'recent', kind: 'recent', rows: recent.map((e) => model(e, 'r')) });
  const groups = [...new Set(entries.map((e) => e.group))];
  if (groups.length > 1 || groups[0]) {
    // Makers in the order the service lists them; names without one (the program's own short
    // names beside a service's list) stay where the list has them, under "Other".
    for (const g of groups) {
      out.push({ key: `g:${g}`, label: g || 'other', kind: g ? 'group' : 'other', rows: entries.filter((e) => e.group === g).map((e) => model(e)) });
    }
  } else if (entries.length) {
    out.push({ key: 'all', label: recent.length ? 'all' : '', kind: 'all', rows: entries.map((e) => model(e)) });
  }
  return out;
}

export const rowsOf = (sections: Section[]): Row[] => sections.flatMap((s) => s.rows);

/** Where the list starts when it opens or the search changes: with something typed the best match
 *  (the first row), otherwise the choice in force (its first row: on top in a long list). */
export function startIndex(rows: Row[], query: string): number {
  if (!rows.length) return -1;
  return query.trim() ? 0 : Math.max(0, rows.findIndex((r) => r.current));
}

/** How many rows Page Up / Page Down move. */
export const PAGE = 8;

/** The row a key moves to (-1 while there are none). Up and down go round; a page stops at the
 *  ends. `null` = not a key that moves. Left and right are never used, so the keys mean the same
 *  in a right-to-left language. */
export function moveBy(key: string, active: number, count: number): number | null {
  if (!['ArrowDown', 'ArrowUp', 'PageDown', 'PageUp', 'Home', 'End'].includes(key)) return null;
  if (count <= 0) return -1;
  const at = Math.min(Math.max(active, -1), count - 1);
  switch (key) {
    case 'ArrowDown': return (at + 1) % count;
    case 'ArrowUp': return at <= 0 ? count - 1 : at - 1;
    case 'PageDown': return Math.min(count - 1, Math.max(at, 0) + PAGE);
    case 'PageUp': return Math.max(0, at - PAGE);
    case 'Home': return 0;
    default: return count - 1;
  }
}

// ---------- recently used ----------

const RECENT_KEY = 'thoughts.models.recent';
const KEPT = 8;

function readAll(): Record<string, string[]> {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '{}');
    return v && typeof v === 'object' && !Array.isArray(v) ? v : {};
  } catch { return {}; }
}

/** The models picked lately for `scope` (an assistant program), newest first. Kept in this window's
 *  own storage: it is a convenience of the list, not a setting. */
export function recentModels(scope: string): string[] {
  const l = readAll()[scope];
  return Array.isArray(l) ? l.filter((x): x is string => typeof x === 'string' && validName(x)).slice(0, KEPT) : [];
}

/** Remember that `id` was picked (never "Automatic"). */
export function rememberModel(scope: string, id: string | null | undefined): void {
  if (!scope || !id || !validName(id)) return;
  try {
    const all = readAll();
    all[scope] = [id, ...recentModels(scope).filter((x) => x !== id)].slice(0, KEPT);
    localStorage.setItem(RECENT_KEY, JSON.stringify(all));
  } catch { /* no storage: nothing is remembered */ }
}

// ---------- where the list opens ----------

/** `top` when it opens under the button, `bottom` (from the window's bottom edge) when over it: a
 *  list shorter than the room then still touches the button. */
export interface Box { left: number; top: number | null; bottom: number | null; width: number; maxHeight: number; above: boolean }

/** Where the list goes: under the button, or over it when there is more room there; as wide as the
 *  button but never narrower than `minWidth`; and always `margin` inside the window on every side,
 *  with its height cut to the room there is (it scrolls inside). `rtl` lines it up with the
 *  button's right edge instead of its left. */
export function placeList(btn: { left: number; right: number; top: number; bottom: number }, win: { width: number; height: number },
  o: { rtl?: boolean; minWidth?: number; wanted?: number; margin?: number; gap?: number } = {}): Box {
  const margin = o.margin ?? 8, gap = o.gap ?? 4, wanted = o.wanted ?? 420;
  const width = Math.max(120, Math.min(Math.max(btn.right - btn.left, o.minWidth ?? 300), win.width - 2 * margin));
  const below = win.height - btn.bottom - gap - margin, over = btn.top - gap - margin;
  // Under the button unless the whole list does not fit there and there is clearly more room over it.
  const above = below < wanted && over > below * 1.25;
  const maxHeight = Math.max(120, Math.min(wanted, above ? over : below));
  const start = o.rtl ? btn.right - width : btn.left;
  const left = Math.min(Math.max(margin, start), Math.max(margin, win.width - margin - width));
  return { left, top: above ? null : btn.bottom + gap, bottom: above ? win.height - btn.top + gap : null, width, maxHeight, above };
}
