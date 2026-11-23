// A slide deck is a markdown note whose frontmatter says `slides: true` (round 0.5.0). Slides are
// separated by a line holding exactly `---` with a blank line before and after, outside fenced code;
// a line starting with `Note:` or `ملاحظة:` begins that slide's speaker notes. The deck is shown by
// Present.svelte (reveal.js over md()'s sanitised output) and saved by standalone.ts.

export type Dir = 'rtl' | 'ltr';
export type Transition = 'none' | 'fade' | 'slide';
export type DeckTheme = 'app' | 'light' | 'dark';
export interface Deck {
  title: string;
  dir: Dir;
  transition: Transition;
  theme: DeckTheme;
  slides: { body: string; notes: string }[];
  /** The note was longer than the limits and was cut. */
  cut?: boolean;
}

export const MAX_SLIDES = 300;
export const MAX_BYTES = 200 * 1024;

const FRONT = /^---\r?\n([\s\S]*?)\r?\n---[ \t]*(?:\r?\n|$)/;

/** The frontmatter's plain `key: value` lines (quotes and trailing comments removed). */
export function frontmatter(src: string): Record<string, string> {
  const m = FRONT.exec(src);
  const out: Record<string, string> = {};
  if (!m) return out;
  for (const line of m[1].split(/\r?\n/)) {
    const kv = /^([A-Za-z][\w-]*)\s*:\s*(.*)$/.exec(line);
    if (!kv) continue;
    let v = kv[2].replace(/\s+#.*$/, '').trim();
    if (/^(['"]).*\1$/.test(v)) v = v.slice(1, -1);
    out[kv[1].toLowerCase()] = v;
  }
  return out;
}

/** Is this note a deck (`slides: true` in its frontmatter)? */
export function isDeck(src: string): boolean {
  return /^(true|yes)$/i.test(frontmatter(src).slides ?? '');
}

/** Letters written right to left (Hebrew, Arabic, Syriac, Thaana and their presentation forms). */
const RTL = /[֐-ࣿיִ-﷿ﹰ-﻿]/;
const LETTER = /\p{L}/u;

/** The direction of the first strong letter in `text` (markdown syntax and code skipped), else ltr. */
export function firstDir(text: string): Dir {
  const plain = text.replace(/```[\s\S]*?```|~~~[\s\S]*?~~~/g, ' ').replace(/`[^`]*`/g, ' ').replace(/\]\([^)]*\)/g, ']').replace(/<[^>]*>/g, ' ');
  for (const ch of plain) {
    if (RTL.test(ch)) return 'rtl';
    if (LETTER.test(ch)) return 'ltr';
  }
  return 'ltr';
}

const NOTE = /^\s*(?:Note|Notes|ملاحظة|ملاحظات)\s*:\s?/i;
const FENCE = /^\s{0,3}(`{3,}|~{3,})/;

/** Split one slide's text into what is shown and its speaker notes. */
function slideOf(lines: string[]): { body: string; notes: string } {
  let fence = '';
  for (let i = 0; i < lines.length; i++) {
    const f = FENCE.exec(lines[i]);
    if (f) { if (!fence) fence = f[1]; else if (f[1][0] === fence[0] && f[1].length >= fence.length) fence = ''; continue; }
    if (!fence && NOTE.test(lines[i])) {
      const notes = [lines[i].replace(NOTE, ''), ...lines.slice(i + 1)].join('\n').trim();
      return { body: lines.slice(0, i).join('\n').trim(), notes };
    }
  }
  return { body: lines.join('\n').trim(), notes: '' };
}

/** A deck from a note (or from a chat's `slides` block, whose frontmatter is optional). */
export function parseDeck(src: string): Deck {
  let text = String(src ?? '').replace(/\r\n?/g, '\n');
  let cut = false;
  if (new TextEncoder().encode(text).length > MAX_BYTES) {
    // Cut at a character boundary below the limit.
    let n = MAX_BYTES;
    while (new TextEncoder().encode(text.slice(0, n)).length > MAX_BYTES) n -= 1024;
    text = text.slice(0, n);
    cut = true;
  }
  const fm = frontmatter(text);
  const body = text.replace(FRONT, '');
  const lines = body.split('\n');
  const groups: string[][] = [[]];
  let fence = '';
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const f = FENCE.exec(line);
    if (f) {
      if (!fence) fence = f[1];
      else if (f[1][0] === fence[0] && f[1].length >= fence.length && /^\s{0,3}[`~]+\s*$/.test(line)) fence = '';
    } else if (!fence && line === '---'
      && (i === 0 || lines[i - 1].trim() === '') && (i === lines.length - 1 || lines[i + 1].trim() === '')) {
      groups.push([]);
      continue;
    }
    groups[groups.length - 1].push(line);
  }
  let slides = groups.map(slideOf).filter((s) => s.body || s.notes);
  if (slides.length > MAX_SLIDES) { slides = slides.slice(0, MAX_SLIDES); cut = true; }
  if (!slides.length) slides = [{ body: '', notes: '' }];
  const heading = /^#{1,6}\s+(.+)$/m.exec(slides[0].body)?.[1]?.trim() ?? '';
  const pick = <T extends string>(v: string | undefined, ok: readonly T[], dflt: T): T => (ok.includes(v as T) ? (v as T) : dflt);
  return {
    title: (fm.title || heading).slice(0, 200),
    dir: pick(fm.dir?.toLowerCase(), ['rtl', 'ltr'] as const, firstDir(slides.map((s) => s.body).join('\n'))),
    transition: pick(fm.transition?.toLowerCase(), ['none', 'fade', 'slide'] as const, 'slide'),
    theme: pick(fm.theme?.toLowerCase(), ['app', 'light', 'dark'] as const, 'app'),
    slides,
    ...(cut ? { cut } : {}),
  };
}

/** The first heading of a slide (the presenter panel's "next"), or its first words. */
export function slideTitle(body: string): string {
  const h = /^#{1,6}\s+(.+)$/m.exec(body)?.[1];
  const t = (h ?? body.split('\n').find((l) => l.trim()) ?? '').replace(/[*_`#>[\]]/g, '').trim();
  return t.length > 80 ? t.slice(0, 79) + '…' : t;
}
