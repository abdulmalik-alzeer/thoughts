// Diagrams and charts inside rendered markdown (an answer, a note, a preview).
//
// `md()` leaves a code block written in the language `archify` or `chart` as a code block, wrapped
// and stamped as a figure (`.md-fig`, with a mark markup in a note cannot forge). This module
// watches the page for those wrappers, wherever they appear and however often their container is
// rendered again, and puts the picture above the source: a chart is drawn here (`chart.ts`), a
// diagram by Archify in the background service (`POST /api/figures/archify`) and then made into
// one self-contained SVG (`standalone.ts`). Either is shown as an `<img>`, where an SVG can run
// nothing, and can be saved as an SVG file; a diagram also as a draw.io file (`drawio.ts`).
import { post } from '../api';
import { t } from '../i18n';
import { saveFile } from '../download';
import { chartSvg, parseChart, seriesColours, type Look } from './chart';
import { archifyDrawing } from './standalone';
import { svgToDrawio, colourOf } from './drawio';

type Drawn = { ok: true; svg: string; title: string; kind: string } | { ok: false; code: string; detail: string };

const STATE = 'data-fig-state';
let mark = '';

const isDark = () => document.documentElement.getAttribute('data-mode') === 'dark';

/** The app's current colours as plain values (a custom property may be a `color-mix`: a probe resolves it). */
function look(): Look {
  const probe = document.createElement('span');
  probe.style.cssText = 'position:absolute;visibility:hidden;pointer-events:none';
  document.body.appendChild(probe);
  const read = (token: string, fallback: string) => {
    probe.style.color = '';
    probe.style.color = `var(${token})`;
    const c = colourOf(getComputedStyle(probe).color);
    return c.hex === 'none' ? fallback : c.hex;
  };
  const dark = isDark();
  const out: Look = {
    fg: read('--fg', dark ? '#eeeeee' : '#1a1a1a'), muted: read('--muted', dark ? '#a0a0a0' : '#5c5c5c'),
    grid: read('--border', dark ? '#333333' : '#dcdcdc'), bg: read('--panel', dark ? '#151515' : '#ffffff'),
    series: seriesColours(dark), font: 'system-ui, -apple-system, "Segoe UI", sans-serif',
  };
  probe.remove();
  return out;
}

// Diagrams already drawn, by look and source: a chat shown again, or an answer rendered again when
// it finishes, asks nothing twice. A failure is kept only briefly (Node may just have been installed).
const drawn = new Map<string, { at: number; result: Promise<Drawn> }>();
const KEEP = 40;
const RETRY_AFTER = 20_000;

function diagram(source: string): Promise<Drawn> {
  const dark = isDark();
  const key = (dark ? 'd' : 'l') + source;
  const hit = drawn.get(key);
  if (hit) { drawn.delete(key); drawn.set(key, hit); return hit.result; }
  const result: Promise<Drawn> = (async () => {
    let kind = '';
    try { kind = String(JSON.parse(source)?.diagram_type ?? ''); } catch { /* the service says so */ }
    try {
      const r = await post<{ ok: boolean; html?: string; code?: string; detail?: string }>('/api/figures/archify', { source });
      if (!r.ok || !r.html) return { ok: false as const, code: r.code || 'failed', detail: r.detail || '' };
      const d = await archifyDrawing(r.html, dark);
      return { ok: true as const, svg: d.svg, title: d.title, kind };
    } catch (e) {
      return { ok: false as const, code: 'failed', detail: String((e as any)?.message ?? e ?? '') };
    }
  })();
  const entry = { at: Date.now(), result };
  drawn.set(key, entry);
  void result.then((r) => { if (!r.ok) setTimeout(() => { if (drawn.get(key) === entry) drawn.delete(key); }, RETRY_AFTER); });
  while (drawn.size > KEEP) drawn.delete(drawn.keys().next().value!);
  return result;
}

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text = '') => {
  const e = document.createElement(tag);
  e.className = cls;
  if (text) e.textContent = text;
  return e;
};

const fileName = (title: string, fallback: string) => (title.replace(/[\\/:*?"<>|\u0000-\u001f]+/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 60) || fallback);

function button(label: string, hint: string, run: () => void): HTMLButtonElement {
  const b = el('button', 'fig-btn', label);
  b.type = 'button';
  b.title = hint;
  b.setAttribute('aria-label', hint);
  b.addEventListener('click', (e) => { e.preventDefault(); e.stopPropagation(); run(); });
  return b;
}

/** The picture with its bar: what it is, the files it can be saved as, and its source. */
function show(wrap: HTMLElement, r: Drawn & { ok: true }, what: 'diagram' | 'chart') {
  const fig = el('div', 'fig');
  const bar = el('div', 'fig-bar');
  bar.setAttribute('data-md-copy', ''); // an export or a printout drops it with the copy buttons
  const typeKey = `figures.type.${r.kind}`;
  const typeName = t(typeKey);
  bar.append(el('span', 'fig-name', typeName && typeName !== typeKey ? t('figures.named', { what: t(`figures.${what}`), type: typeName }) : t(`figures.${what}`)));
  const name = fileName(r.title, what);
  bar.append(button(t('figures.svg'), t('figures.saveSvg'), () => void saveFile(`${name}.svg`, r.svg, 'image/svg+xml')));
  if (what === 'diagram') {
    bar.append(button(t('figures.drawio'), t('figures.saveDrawio'), () => {
      try { void saveFile(`${name}.drawio`, svgToDrawio(r.svg, r.title || name), 'application/xml'); } catch { /* the SVG is still there to save */ }
    }));
  }
  const src = button(t('figures.source'), t('figures.showSource'), () => {
    const open = wrap.classList.toggle('fig-open');
    src.textContent = t(open ? 'figures.hideSource' : 'figures.source');
    src.setAttribute('aria-expanded', String(open));
  });
  src.setAttribute('aria-expanded', 'false');
  bar.append(src);
  const img = el('img', 'fig-img');
  img.alt = r.title || t(`figures.${what}`);
  img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(r.svg);
  fig.append(bar, img);
  wrap.querySelector(':scope > .fig')?.remove();
  wrap.prepend(fig);
  wrap.setAttribute(STATE, 'drawn');
}

/** Why there is no picture, above the source (which stays in view). */
function fail(wrap: HTMLElement, message: string, detail = '') {
  const fig = el('div', 'fig fig-failed');
  fig.setAttribute('role', 'note');
  fig.append(el('p', 'fig-why', message));
  if (detail) {
    const pre = el('pre', 'fig-detail', detail);
    pre.dir = 'ltr';
    const copy = button(t('figures.copyMessage'), t('figures.copyMessageHint'), () => {
      void navigator.clipboard.writeText(detail).then(() => { copy.textContent = t('md.copied'); setTimeout(() => { copy.textContent = t('figures.copyMessage'); }, 1600); }, () => {});
    });
    fig.append(pre, copy);
  }
  wrap.querySelector(':scope > .fig')?.remove();
  wrap.prepend(fig);
  wrap.setAttribute(STATE, 'failed');
}

function hydrate(wrap: HTMLElement) {
  if (wrap.getAttribute('data-md-fig') !== mark || wrap.hasAttribute(STATE)) return;
  const source = wrap.querySelector(':scope > pre')?.textContent ?? '';
  const kind = wrap.getAttribute('data-fig-kind');
  // A block still being written (or not JSON at all) stays a code block until it is whole.
  let whole = true;
  try { const v = JSON.parse(source); whole = !!v && typeof v === 'object'; } catch { whole = false; }
  if (!whole) { wrap.setAttribute(STATE, 'source'); return; }
  if (kind === 'chart') {
    const parsed = parseChart(source);
    if ('problem' in parsed) { fail(wrap, t(`figures.chart.${parsed.problem.code}`, { at: parsed.problem.at ?? '' })); return; }
    show(wrap, { ok: true, svg: chartSvg(parsed.spec, look()), title: parsed.spec.title, kind: parsed.spec.type }, 'chart');
    return;
  }
  wrap.setAttribute(STATE, 'drawing');
  const wait = el('div', 'fig fig-wait', t('figures.drawing'));
  wait.setAttribute('role', 'status');
  wrap.prepend(wait);
  void diagram(source).then((r) => {
    if (!wrap.isConnected) return;
    if (r.ok) { show(wrap, r, 'diagram'); return; }
    const key = `figures.err.${r.code}`;
    const known = t(key) !== key;
    const inMessage = r.code === 'old-node' || r.code === 'unknown-type';
    fail(wrap, known ? t(key, { detail: r.detail }) : t('figures.err.failed'), inMessage ? '' : r.detail);
  });
}

let queued = false;
function scan() {
  queued = false;
  for (const w of Array.from(document.querySelectorAll<HTMLElement>(`.md-fig:not([${STATE}])`))) hydrate(w);
}
const later = () => { if (!queued) { queued = true; requestAnimationFrame(scan); } };

let started = false;
/** Start watching the page for figures (once). `figMark` is the stamp `md()` puts on its wrappers. */
export function startFigures(figMark: string) {
  mark = figMark;
  if (started || typeof document === 'undefined' || typeof MutationObserver === 'undefined') { later(); return; }
  started = true;
  addStyles();
  new MutationObserver((muts) => {
    for (const m of muts) {
      for (const n of Array.from(m.addedNodes)) {
        if (n instanceof HTMLElement && (n.classList.contains('md-fig') || n.querySelector('.md-fig'))) { later(); return; }
      }
    }
  }).observe(document.body, { childList: true, subtree: true });
  // Another theme, day or night, or language: the pictures are drawn again in it.
  new MutationObserver(() => {
    for (const w of Array.from(document.querySelectorAll<HTMLElement>(`.md-fig[${STATE}]`))) {
      w.querySelector(':scope > .fig')?.remove();
      w.removeAttribute(STATE);
      w.classList.remove('fig-open');
    }
    later();
  }).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-mode', 'lang'] });
  later();
}

// Styles live here because the figures are built in injected HTML (no component scope).
function addStyles() {
  if (document.getElementById('md-figures')) return;
  const s = document.createElement('style');
  s.id = 'md-figures';
  s.textContent = `
.md-fig[data-fig-state="drawn"]:not(.fig-open) > .md-codehead, .md-fig[data-fig-state="drawn"]:not(.fig-open) > pre,
.md-fig[data-fig-state="drawing"] > .md-codehead, .md-fig[data-fig-state="drawing"] > pre { display: none; }
.md-fig[data-fig-state="drawn"] { background: var(--panel); }
.md-fig.fig-open > .md-codehead { border-top: var(--border-w, 1px) solid var(--hairline); background: var(--code-bg); }
.md-fig.fig-open > pre { background: var(--code-bg); max-height: 320px; overflow: auto; }
.fig-bar { display: flex; align-items: center; gap: 4px; min-height: 30px; padding: 4px 6px; padding-inline-start: 12px; font-size: 12px; color: var(--muted);
  border-bottom: var(--border-w, 1px) solid var(--hairline); }
.fig-name { margin-inline-end: auto; font-weight: 500; }
.fig-btn { font: inherit; font-size: 12px; font-weight: 500; line-height: 1.4; color: var(--muted); background: transparent; border: 1px solid transparent;
  border-radius: 7px; padding: 2px 8px; cursor: pointer; transition: color .15s, background .15s; }
.fig-btn:hover, .fig-btn:focus-visible { color: var(--fg); background: var(--fill); border-color: var(--hairline); }
.fig-img { display: block; width: 100%; height: auto; max-height: 78vh; object-fit: contain; margin: 0; border-radius: 0; cursor: zoom-in; }
.fig-wait { padding: 14px 12px; font-size: 12.5px; color: var(--muted); }
.fig-failed { padding: 10px 12px; border-bottom: var(--border-w, 1px) solid var(--hairline); }
.fig-why { margin: 0 0 6px; font-size: 12.5px; color: var(--fg); }
.fig-detail { margin: 0 0 6px; padding: 8px 10px; max-height: 160px; overflow: auto; white-space: pre-wrap; font-family: var(--mono); font-size: 11.5px;
  text-align: left; color: var(--muted); background: var(--fill); border: 0; border-radius: var(--radius); }
@media print { .fig-bar, .fig-wait { display: none !important; } .fig-img { max-height: none; } }
`;
  document.head.appendChild(s);
}
