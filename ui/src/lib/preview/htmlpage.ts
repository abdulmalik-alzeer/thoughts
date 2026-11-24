// An HTML file made safe to show as a page in the preview (renderers/Html.svelte). Three layers:
// 1. the page is cleaned (DOMPurify): no scripts, frames, forms, plugins, <meta>/<base>/<link>,
//    no event handlers, and no addresses at all except pictures embedded in the file itself and
//    links within the page (#…), so it can neither navigate nor fetch;
// 2. a Content-Security-Policy is put first in its <head>: nothing may load but embedded pictures
//    and the page's own styles;
// 3. it is shown in an <iframe sandbox=""> (Html.svelte): no scripts, no same-origin, no popups,
//    no forms, no top navigation.
import createDOMPurify from 'dompurify';
import { NO_BOUNCE } from '../noBounce';

export const PAGE_CSP = "default-src 'none'; img-src data:; style-src 'unsafe-inline'";

const FORBID_TAGS = ['script', 'noscript', 'meta', 'base', 'link', 'iframe', 'frame', 'frameset', 'object', 'embed', 'applet', 'portal',
  'form', 'input', 'button', 'textarea', 'select', 'option', 'video', 'audio', 'source', 'track', 'template', 'math'];
const URL_ATTRS = ['src', 'poster', 'background', 'xlink:href', 'data', 'codebase', 'lowsrc', 'dynsrc', 'longdesc', 'cite', 'action', 'formaction'];

let purify: ReturnType<typeof createDOMPurify> | null = null;
function instance() {
  if (purify) return purify;
  // An instance of our own, so these rules never touch the one that cleans notes (md.ts).
  const p = createDOMPurify(window);
  p.addHook('afterSanitizeAttributes', (node) => {
    const el = node as Element;
    if (!el.getAttribute) return;
    const href = el.getAttribute('href');
    if (href !== null && !/^#[^\s]*$/.test(href.trim())) el.removeAttribute('href');
    for (const a of URL_ATTRS) {
      const v = el.getAttribute(a);
      if (v !== null && !(a === 'src' && /^data:image\/(png|jpe?g|gif|webp|bmp|avif);/i.test(v.trim()))) el.removeAttribute(a);
    }
    el.removeAttribute('srcset');
    el.removeAttribute('ping');
    el.removeAttribute('target');
  });
  purify = p;
  return p;
}

/** The `srcdoc` for showing `html` as a page. */
export function safePage(html: string): string {
  const clean = instance().sanitize(html, {
    WHOLE_DOCUMENT: true, FORBID_TAGS, FORBID_ATTR: ['srcset', 'ping', 'target', 'action', 'formaction', 'http-equiv'],
    ADD_TAGS: ['style'], ALLOW_DATA_ATTR: false,
  }) as unknown as string;
  // First thing in the document, so the policy is in force before anything of the page is read.
  return `<!doctype html><meta http-equiv="Content-Security-Policy" content="${PAGE_CSP}"><meta charset="utf-8"><style>${NO_BOUNCE}</style>${clean}`;
}
