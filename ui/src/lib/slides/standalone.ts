// A deck saved as one HTML file that opens in any browser (Docs › Save slides as HTML →
// `<idea>/outputs/<note>.slides.html`). It carries reveal.js and its stylesheet from the app's own
// bundle, the app's look with its colours written out, and the slides as sanitised markup; its CSP
// forbids loading anything, and its only scripts are reveal.js and the fixed start-up call (no text of
// the note ever goes into a script). Pictures are embedded as `data:` addresses; anything else is dropped.
import DOMPurify from 'dompurify';
import revealCss from 'reveal.js/reveal.css?raw';
import revealJs from '../../../../node_modules/reveal.js/dist/reveal.js?raw';
import themeCss from './theme.css?raw';
import { md } from '../md';
import type { Deck } from './deck';

export const CSP = "default-src 'none'; img-src data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'";

/** The app's tokens a saved deck needs, as the window has them now. */
const TOKENS = ['--bg', '--fg', '--panel', '--border', '--muted', '--accent', '--accent-2', '--accent-fg', '--font', '--font-ar', '--mono', '--radius', '--hairline'];

/** The current values of the app's tokens on `el` (the presentation, or the page). */
export function readLook(el: Element): Record<string, string> {
  const cs = getComputedStyle(el);
  const out: Record<string, string> = {};
  for (const k of TOKENS) {
    const v = cs.getPropertyValue(k).trim();
    if (v && !/[;{}<>]/.test(v) && v.length < 400) out[k] = v;
  }
  return out;
}

const TAGS = ['a', 'abbr', 'b', 'bdi', 'bdo', 'blockquote', 'br', 'caption', 'cite', 'code', 'dd', 'del', 'details', 'dfn', 'div', 'dl', 'dt',
  'em', 'figcaption', 'figure', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'i', 'img', 'ins', 'kbd', 'li', 'mark', 'ol', 'p', 'pre',
  'q', 'rp', 'rt', 'ruby', 's', 'samp', 'small', 'span', 'strong', 'sub', 'summary', 'sup', 'table', 'tbody', 'td', 'tfoot', 'th',
  'thead', 'time', 'tr', 'u', 'ul', 'var', 'wbr'];
const ATTRS = ['href', 'title', 'alt', 'src', 'width', 'height', 'align', 'start', 'reversed', 'colspan', 'rowspan', 'dir', 'lang', 'open', 'datetime', 'cite'];

/** One slide's content as clean nodes: the page's extras (copy buttons, figure bars, a drawn figure's
 *  source) taken out, then the markdown allowlist again, pictures only as `data:images`, links only
 *  to the web or mail. */
function clean(from: Element, doc: Document): DocumentFragment {
  const copy = from.cloneNode(true) as Element;
  for (const x of Array.from(copy.querySelectorAll('.md-codehead, [data-md-copy], .fig-bar, button'))) x.remove();
  for (const fig of Array.from(copy.querySelectorAll('.md-fig'))) if (fig.querySelector('img')) fig.querySelector(':scope > pre')?.remove();
  const frag = DOMPurify.sanitize(copy.innerHTML, { ALLOWED_TAGS: TAGS, ALLOWED_ATTR: ATTRS, ALLOW_DATA_ATTR: false, ALLOW_ARIA_ATTR: false, RETURN_DOM_FRAGMENT: true }) as DocumentFragment;
  for (const img of Array.from(frag.querySelectorAll('img'))) if (!/^data:image\/(png|jpe?g|gif|webp|svg\+xml)[;,]/i.test(img.getAttribute('src') || '')) img.remove();
  for (const a of Array.from(frag.querySelectorAll('a'))) {
    const h = a.getAttribute('href') || '';
    if (!/^(https?:|mailto:|#)/i.test(h)) a.removeAttribute('href');
    a.setAttribute('rel', 'noopener noreferrer');
  }
  return doc.importNode(frag, true) as DocumentFragment;
}

/** The deck as one self-contained HTML document. `sections`: the slides as shown (pictures already
 *  `data:` addresses); `look`: the app's token values (`readLook`). */
export function toHtml(deck: Deck, sections: Element[], look: Record<string, string> = {}, lang = deck.dir === 'rtl' ? 'ar' : 'en'): string {
  const doc = document.implementation.createHTMLDocument('');
  doc.documentElement.setAttribute('lang', lang);
  doc.documentElement.setAttribute('dir', deck.dir);
  const head = doc.head;
  head.replaceChildren();
  const meta = (attrs: Record<string, string>) => { const m = doc.createElement('meta'); for (const [k, v] of Object.entries(attrs)) m.setAttribute(k, v); head.append(m); };
  meta({ charset: 'utf-8' });
  meta({ 'http-equiv': 'Content-Security-Policy', content: CSP });
  meta({ name: 'viewport', content: 'width=device-width, initial-scale=1' });
  const title = doc.createElement('title');
  title.textContent = deck.title || 'Slides';
  head.append(title);
  const style = (text: string) => { const s = doc.createElement('style'); s.textContent = text; head.append(s); };
  style(revealCss.replace(/<\/style/gi, '<\\/style'));
  const vars = Object.entries(look).filter(([k, v]) => TOKENS.includes(k) && !/[;{}<>]/.test(v) && v.length < 400).map(([k, v]) => `${k}: ${v};`).join(' ');
  style(`:root { ${vars} } html, body { margin: 0; height: 100%; background: var(--bg); }\n` + themeCss.replace(/\.present \.reveal/g, '.reveal').replace(/<\/style/gi, '<\\/style'));

  const reveal = doc.createElement('div');
  reveal.className = 'reveal';
  reveal.setAttribute('dir', deck.dir);
  const slides = doc.createElement('div');
  slides.className = 'slides';
  sections.forEach((sec, i) => {
    const s = doc.createElement('section');
    s.append(clean(sec, doc));
    const notes = deck.slides[i]?.notes;
    if (notes) {
      const aside = doc.createElement('aside');
      aside.className = 'notes';
      const holder = document.createElement('div');
      holder.innerHTML = md(notes); // md(): the app's sanitiser
      aside.append(clean(holder, doc));
      s.append(aside);
    }
    slides.append(s);
  });
  reveal.append(slides);
  doc.body.append(reveal);

  const script = (text: string) => { const s = doc.createElement('script'); s.textContent = text; doc.body.append(s); };
  script(revealJs.replace(/<\/script/gi, '<\\/script'));
  const rtl = deck.dir === 'rtl';
  const transition = (['none', 'fade', 'slide'] as const).includes(deck.transition) ? deck.transition : 'slide';
  script(`Reveal.initialize({ hash: false, history: false, controls: true, progress: true, center: true, width: 1280, height: 720, rtl: ${rtl}, transition: '${transition}', backgroundTransition: 'none', plugins: [], help: false, jumpToSlide: false, navigationMode: 'linear', fragments: false, postMessage: false, postMessageEvents: false, previewLinks: false, fragmentInURL: false, scrollActivationWidth: 0 });`);
  return '<!doctype html>\n' + doc.documentElement.outerHTML + '\n';
}

/** Blob pictures (the library's, fetched for the page) made into `data:` addresses, in place. */
export async function embedPictures(root: Element): Promise<void> {
  for (const img of Array.from(root.querySelectorAll('img'))) {
    const src = img.getAttribute('src') || '';
    if (!src.startsWith('blob:')) continue;
    try {
      const b = await (await fetch(src)).blob();
      img.setAttribute('src', await new Promise<string>((ok, no) => { const r = new FileReader(); r.onload = () => ok(String(r.result)); r.onerror = () => no(r.error); r.readAsDataURL(b); }));
    } catch { img.removeAttribute('src'); }
  }
}
