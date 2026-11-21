// From the page Archify writes to a drawing that stands on its own.
//
// Archify's output is a whole web page: the drawing, a style sheet of ~200 KB that gives it its
// looks through classes and custom properties, a font, and a viewer written in JavaScript. None of
// that page ever runs here. The drawing and the style sheets are lifted out of it as text and laid
// out once in a frame that cannot run scripts (`sandbox` without `allow-scripts`); there every
// element is asked what it finally looks like, and that is written onto the element itself. What
// comes back is one SVG with no classes, no style sheet to depend on and no script: it is shown as
// a picture (`<img>`, where an SVG can do nothing but be looked at), saved as the SVG file, and
// read by `drawio.ts`, which needs the looks on the elements.
import { colourOf } from './drawio';

/** What is copied from the computed style onto each drawn element. */
const PAINT = ['fill', 'stroke', 'stroke-width', 'stroke-dasharray', 'stroke-linecap', 'stroke-linejoin', 'opacity', 'fill-opacity', 'stroke-opacity'];
const TEXT = ['font-family', 'font-size', 'font-weight', 'font-style', 'text-anchor', 'dominant-baseline', 'letter-spacing'];
const SHAPES = new Set(['rect', 'circle', 'ellipse', 'line', 'polyline', 'polygon', 'path', 'text', 'tspan', 'use']);
/** Values that say nothing (the property's own default), left off to keep the file small. */
const DEFAULTS: Record<string, string[]> = {
  'stroke-dasharray': ['none'], 'stroke-linecap': ['butt'], 'stroke-linejoin': ['miter'], opacity: ['1'], 'fill-opacity': ['1'], 'stroke-opacity': ['1'],
  'font-style': ['normal'], 'text-anchor': ['start'], 'dominant-baseline': ['auto'], 'letter-spacing': ['normal', '0px'],
};

/** The drawing in a parsed Archify page: its largest SVG that is not an icon inside a control. */
export function pickDrawing(page: Document): SVGSVGElement | null {
  let best: SVGSVGElement | null = null, size = 0;
  for (const svg of Array.from(page.querySelectorAll('svg'))) {
    if (svg.closest('button, a, nav, menu, [role="menu"], [role="toolbar"], svg svg')) continue;
    if (svg.parentElement?.closest('svg')) continue;
    const n = svg.querySelectorAll('*').length;
    if (n > size) { best = svg as SVGSVGElement; size = n; }
  }
  return best;
}

/** A colour and its transparency as two plain attributes any SVG reader understands. */
function setColour(el: Element, prop: 'fill' | 'stroke', value: string) {
  if (value.startsWith('url(')) { el.setAttribute(prop, value.replace(/^url\("?([^")]+)"?\)$/, 'url($1)')); return; }
  const c = colourOf(value);
  el.setAttribute(prop, c.hex);
  if (c.hex !== 'none' && c.alpha < 0.999) {
    const own = parseFloat(el.getAttribute(`${prop}-opacity`) || '1');
    el.setAttribute(`${prop}-opacity`, String(Math.round((Number.isFinite(own) ? own : 1) * c.alpha * 1000) / 1000));
  }
}

/** Write each element's final looks onto it and take away what they came from. `view` is the frame's window. */
export function inlineLooks(svg: SVGSVGElement, view: Window): void {
  const all = [svg as Element, ...Array.from(svg.querySelectorAll('*'))];
  // Read everything first (one layout), then write.
  const looks = all.map((el) => {
    const tag = el.nodeName.toLowerCase();
    const cs = view.getComputedStyle(el);
    const got: [string, string][] = [];
    if (cs.display === 'none' || cs.visibility === 'hidden') return { el, hide: true, got };
    const take = (props: string[]) => { for (const p of props) { const v = cs.getPropertyValue(p).trim(); if (v) got.push([p, v]); } };
    if (SHAPES.has(tag)) take(PAINT);
    else if (tag === 'g' || tag === 'svg' || tag === 'a') take(['opacity']);
    if (tag === 'text' || tag === 'tspan') take(TEXT);
    for (const m of ['marker-start', 'marker-end']) { const v = cs.getPropertyValue(m).trim(); if (v && v !== 'none') got.push([m, v]); }
    return { el, hide: false, got };
  });
  for (const { el, hide, got } of looks) {
    const cls = el.getAttribute('class') || '';
    // What the classes said about the part's role is kept as plain marks for `drawio.ts`.
    if (/\bc-mask\b/.test(cls)) el.setAttribute('data-mask', '');
    if (/\bt-edge/.test(cls)) el.setAttribute('data-edge-text', '');
    if (hide) { el.setAttribute('display', 'none'); }
    const after: [string, string][] = [];
    for (const [p, v] of got) {
      if (p === 'fill' || p === 'stroke') { after.push([p, v]); continue; }
      if (DEFAULTS[p]?.includes(v)) { el.removeAttribute(p); continue; }
      if (p.startsWith('marker-')) { el.setAttribute(p, v.replace(/^url\("?([^")]+)"?\)$/, 'url($1)')); continue; }
      el.setAttribute(p, p === 'stroke-width' || p === 'font-size' ? v.replace(/px$/, '') : v);
    }
    // Colours last: their transparency multiplies the opacity just written.
    for (const [p, v] of after) setColour(el, p as 'fill' | 'stroke', v);
    el.removeAttribute('class');
    el.removeAttribute('style');
    el.removeAttribute('data-animate');
  }
  for (const s of Array.from(svg.querySelectorAll('style, script'))) s.remove();
  for (const el of all) for (const a of Array.from(el.attributes)) if (/^on/i.test(a.name)) el.removeAttribute(a.name);
}

/** The `@font-face` rules of the page whose family the drawing uses, to travel inside the SVG. */
function fontRules(page: Document, used: Set<string>): string {
  const css = Array.from(page.querySelectorAll('style')).map((s) => s.textContent || '').join('\n');
  const rules = css.match(/@font-face\s*\{[^}]*\}/g) ?? [];
  return rules.filter((r) => { const f = /font-family:\s*["']?([^;"']+)/.exec(r)?.[1]?.trim().toLowerCase(); return !!f && used.has(f) && /url\(\s*["']?data:/.test(r); }).join('\n');
}

/** A hidden frame that lays out `html` and cannot run scripts. Resolves once it has loaded. */
function frameOf(html: string): Promise<HTMLIFrameElement> {
  return new Promise((resolve, reject) => {
    const f = document.createElement('iframe');
    f.setAttribute('sandbox', 'allow-same-origin');
    f.setAttribute('aria-hidden', 'true');
    f.tabIndex = -1;
    // Laid out (so styles compute) but never seen: off the page, not `display:none`.
    f.style.cssText = 'position:fixed;left:-20000px;top:0;width:1400px;height:1000px;border:0;opacity:0;pointer-events:none;';
    const timer = setTimeout(() => { f.remove(); reject(new Error('frame timeout')); }, 8000);
    f.addEventListener('load', () => { clearTimeout(timer); resolve(f); }, { once: true });
    f.srcdoc = html;
    document.body.appendChild(f);
  });
}

export interface Drawing { svg: string; title: string; width: number; height: number }

/** The drawing of an Archify page as one self-contained SVG, in Archify's light or dark look. */
export async function archifyDrawing(pageHtml: string, dark: boolean): Promise<Drawing> {
  const page = new DOMParser().parseFromString(pageHtml, 'text/html');
  const found = pickDrawing(page);
  if (!found) throw new Error('no drawing in the page');
  const styles = Array.from(page.querySelectorAll('style')).filter((s) => !s.closest('svg')).map((s) => s.textContent || '');
  const safe = (css: string) => css.replace(/<\/style/gi, '<\\/style');
  const html = `<!doctype html><html lang="${found.getAttribute('lang') || 'en'}" data-theme="${dark ? 'dark' : 'light'}"><head><meta charset="utf-8">`
    + styles.map((css) => `<style>${safe(css)}</style>`).join('')
    + '<style>*,*::before,*::after{animation:none!important;transition:none!important}html,body{margin:0}</style></head><body></body></html>';
  const frame = await frameOf(html);
  try {
    const doc = frame.contentDocument, view = frame.contentWindow;
    if (!doc || !view) throw new Error('frame unavailable');
    // The drawing goes in as nodes, not as text: nothing in it is parsed as a page again.
    for (const s of Array.from(found.querySelectorAll('script, foreignObject'))) s.remove();
    const svg = doc.importNode(found, true) as SVGSVGElement;
    doc.body.appendChild(svg);
    try { await (doc as any).fonts?.ready; } catch { /* the fallback font is drawn */ }
    const background = colourOf(view.getComputedStyle(doc.body).backgroundColor);
    inlineLooks(svg, view);
    const vb = (svg.getAttribute('viewBox') || '').split(/[\s,]+/).map(Number);
    const [w, h] = vb.length === 4 && vb.every(Number.isFinite) ? [vb[2], vb[3]] : [1080, 600];
    svg.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
    svg.setAttribute('width', String(w));
    svg.setAttribute('height', String(h));
    svg.removeAttribute('data-theme');
    const ns = 'http://www.w3.org/2000/svg';
    const first = svg.querySelector(':scope > defs')?.nextSibling ?? svg.firstChild;
    if (background.hex !== 'none') {
      const bg = doc.createElementNS(ns, 'rect');
      bg.setAttribute('data-background', '');
      for (const [k, v] of [['x', String(vb[0] || 0)], ['y', String(vb[1] || 0)], ['width', String(w)], ['height', String(h)], ['fill', background.hex]]) bg.setAttribute(k, v);
      svg.insertBefore(bg, first);
    }
    const used = new Set(Array.from(svg.querySelectorAll('[font-family]')).flatMap((el) => (el.getAttribute('font-family') || '').split(',').map((f) => f.replace(/["']/g, '').trim().toLowerCase())));
    const fonts = fontRules(page, used);
    if (fonts) {
      const style = doc.createElementNS(ns, 'style');
      style.textContent = fonts;
      svg.insertBefore(style, svg.firstChild);
    }
    const title = (svg.querySelector(':scope > title')?.textContent || '').trim();
    return { svg: new XMLSerializer().serializeToString(svg), title, width: w, height: h };
  } finally {
    frame.remove();
  }
}
