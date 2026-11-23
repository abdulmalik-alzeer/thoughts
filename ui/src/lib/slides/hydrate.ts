// A slide deck inside an answer: md() leaves a code block in the language `slides` as a code block,
// wrapped and stamped (`.md-slides`, with a mark markup in an answer cannot forge). This module watches
// the page for those wrappers and puts a card in front of the source: "Slides · 8", Present, and
// Save as note (a new note in the library, with `slides: true`, through the usual new-file route).
import { post } from '../api';
import { t } from '../i18n';
import { go, toast } from '../store.svelte';
import { failed } from '../errors';
import { promptDialog } from '../dialog.svelte';
import { frontmatter, parseDeck } from './deck';
import { present } from './present';

const STATE = 'data-slides-state';
let mark = '';
let started = false;
let queued = false;

/** The source as a note: `slides: true` (and a title) added to its frontmatter if need be. */
export function asNote(src: string): string {
  const text = src.replace(/\r\n?/g, '\n').trim() + '\n';
  const fm = frontmatter(text);
  if (/^(true|yes)$/i.test(fm.slides ?? '')) return text;
  const title = parseDeck(text).title.replace(/[\n"]/g, ' ').trim();
  if (/^---\n/.test(text) && Object.keys(fm).length) return text.replace(/^---\n/, '---\nslides: true\n');
  return `---\nslides: true\n${title ? `title: "${title}"\n` : ''}---\n\n${text}`;
}

/** A file name from a title: letters and digits of any script, joined by dashes. */
export function slug(title: string): string {
  const s = title.normalize('NFC').replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-+|-+$/g, '').slice(0, 50);
  return s || 'slides';
}

async function saveNote(src: string) {
  const deck = parseDeck(src);
  const path = await promptDialog({ title: t('slides.card.saveTitle'), body: t('slides.card.saveBody'), value: `slides/${slug(deck.title)}.md`, confirmLabel: t('slides.card.save') });
  if (!path?.trim()) return;
  const p = path.trim().replace(/^\/+/, '');
  const file = /\.md$/i.test(p) ? p : p + '.md';
  try {
    const r = await post<{ path?: string }>('/library/new', { path: file, content: asNote(src) });
    const at = r?.path || file;
    toast(t('slides.card.saved'), { kind: 'success', action: { label: t('slides.card.open'), run: () => go({ kind: 'docs', path: at }) } });
  } catch (e) { failed(t('slides.card.saveFailed'), e); }
}

function card(wrap: HTMLElement) {
  const src = wrap.querySelector('pre code')?.textContent ?? '';
  const deck = parseDeck(src);
  const bar = document.createElement('div');
  bar.className = 'slides-card';
  const name = document.createElement('span');
  name.className = 'slides-name';
  name.dir = 'auto';
  name.textContent = t('slides.card', { n: String(deck.slides.length) }) + (deck.title ? ` — ${deck.title}` : '');
  const show = document.createElement('button');
  show.type = 'button';
  show.className = 'slides-btn primary';
  show.textContent = t('slides.present');
  show.title = t('slides.presentTip');
  show.addEventListener('click', () => { void present(deck); });
  const save = document.createElement('button');
  save.type = 'button';
  save.className = 'slides-btn';
  save.textContent = t('slides.card.save');
  save.addEventListener('click', () => { void saveNote(src); });
  bar.append(name, show, save);
  wrap.prepend(bar);
  wrap.setAttribute(STATE, 'card');
}

function scan() {
  queued = false;
  for (const w of Array.from(document.querySelectorAll<HTMLElement>('.md-slides'))) {
    if (w.getAttribute('data-md-slides') !== mark || w.hasAttribute(STATE)) continue;
    card(w);
  }
}
function later() { if (!queued) { queued = true; requestAnimationFrame(scan); } }

export function startSlides(slidesMark: string) {
  mark = slidesMark;
  if (started || typeof document === 'undefined' || typeof MutationObserver === 'undefined') { later(); return; }
  started = true;
  addStyles();
  new MutationObserver((muts) => {
    for (const m of muts) for (const n of Array.from(m.addedNodes)) {
      if (n instanceof HTMLElement && (n.classList.contains('md-slides') || n.querySelector('.md-slides'))) { later(); return; }
    }
  }).observe(document.body, { childList: true, subtree: true });
  // A change of language: the cards are made again in it.
  new MutationObserver(() => {
    for (const w of Array.from(document.querySelectorAll<HTMLElement>(`.md-slides[${STATE}]`))) { w.querySelector(':scope > .slides-card')?.remove(); w.removeAttribute(STATE); }
    later();
  }).observe(document.documentElement, { attributes: true, attributeFilter: ['lang'] });
  later();
}

// Styles live here because the card is built in injected HTML (no component scope).
function addStyles() {
  if (document.getElementById('md-slides')) return;
  const s = document.createElement('style');
  s.id = 'md-slides';
  s.textContent = `
.md-slides[data-slides-state="card"] > .md-codehead, .md-slides[data-slides-state="card"] > pre { display: none; }
.slides-card { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding: 8px 10px; padding-inline-start: 12px; font-size: 13px; background: var(--panel); }
.slides-name { margin-inline-end: auto; font-weight: 600; }
.slides-btn { font: inherit; font-size: 12px; font-weight: 500; color: var(--fg); background: var(--fill); border: 1px solid var(--hairline); border-radius: 7px; padding: 3px 10px; cursor: pointer; }
.slides-btn.primary { background: var(--accent); color: var(--accent-fg); border-color: transparent; }
.slides-btn:hover, .slides-btn:focus-visible { filter: brightness(1.05); }
`;
  document.head.append(s);
}
