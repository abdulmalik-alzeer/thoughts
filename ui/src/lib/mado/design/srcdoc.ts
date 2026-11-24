// The page a mock-up is shown as: the text of the `srcdoc` of a sealed frame
// (`<iframe sandbox="" referrerpolicy="no-referrer">`, see MockFrame.svelte).
//
// Three walls, each enough on its own:
// 1. the daemon cleaned the mock-up from an allowlist (no script, no handler, no address);
// 2. the first thing in the page is a content policy that allows nothing but inline styles and
//    embedded pictures, so nothing can load and nothing can run even if 1 missed something;
// 3. the frame is sandboxed with no permissions: no scripts, no forms, no same origin, no popups.
//
// The string built here goes only into the frame's `srcdoc` attribute, as an attribute value set
// through the DOM (never by building HTML text for the app's own page), so whatever the mock-up
// contains, `</iframe>` included, it stays text of that attribute.
import { rootVars } from './tokens';
import { NO_BOUNCE } from '../../noBounce';

/** The contract's policy for a mock-up. */
export const MOCK_CSP = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:";

const HEAD_OPEN = `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="${MOCK_CSP}"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">`;
// A calm page to start from; the mock-up's own styles come after and win.
const BASE = 'html{color-scheme:light}body{margin:0;background:var(--mado-color-bg);color:var(--mado-color-text);font-family:var(--mado-font-body);font-size:16px;line-height:1.5}';

/** The sealed page: our policy and the design's variables first, then the mock-up. The mock-up is
 *  placed after `</head>`, so nothing in it can come before the policy; a second policy inside it
 *  could only forbid more (policies add up, the strictest wins). */
export function sealedPage(mock: string, tokens: unknown): string {
  return `${HEAD_OPEN}<style>${NO_BOUNCE}${rootVars(tokens)}${BASE}</style></head>${String(mock ?? '')}`;
}

/** Does this look like what the daemon's cleaner writes (style blocks, then one body, nothing
 *  that runs or loads)? A second look in the window: a mock-up that fails it is not shown at all.
 *  The frame would hold it anyway; this catches a daemon that stopped cleaning. */
export function looksClean(html: unknown): html is string {
  if (typeof html !== 'string') return false;
  let i = 0;
  while (html.startsWith('<style>', i)) {
    const end = html.indexOf('</style>', i);
    if (end < 0) return false;
    i = end + 8;
  }
  if (!(html.startsWith('<body>', i) || html.startsWith('<body ', i)) || !html.endsWith('</body>')) return false;
  if (/<\s*\/?\s*(script|iframe|frame|object|embed|link|base|meta|template|noscript|applet|math|foreignobject)\b/i.test(html)) return false;
  // Attribute values are always in double quotes with `"` escaped: blank them, then look at the names.
  const bare = html.replace(/"[^"]*"/g, '""');
  if (/<[a-z][^>]*\s(on[a-z]*|srcdoc|srcset|action|formaction|xlink:href|http-equiv)\s*=/i.test(bare)) return false;
  for (const m of html.matchAll(/\s(?:src|href)="([^"]*)"/gi)) {
    if (!/^(#[\w-]*|data:image\/(png|jpeg|gif|webp|svg\+xml)[;,])/i.test(m[1].trim())) return false;
  }
  return true;
}

// ---------- the sizes a mock-up is shown at ----------

export const DEVICES = { phone: { w: 390, h: 844 }, tablet: { w: 820, h: 1180 }, desktop: { w: 1280, h: 800 } } as const;
export type Device = keyof typeof DEVICES;
export const LENGTHS = [1, 2, 3] as const;
export const ZOOM = { min: 0.25, max: 2, step: 0.1 } as const;

/** The scale at which a frame of `device` width fits into `room` pixels (never above 1), times the
 *  user's zoom (`null` = fit). */
export function scaleFor(device: Device, room: number, zoom: number | null): number {
  const fit = Math.min(1, Math.max(0.1, (room || DEVICES[device].w) / DEVICES[device].w));
  if (zoom === null) return fit;
  return Math.min(ZOOM.max, Math.max(ZOOM.min, zoom));
}

/** A click on the layer over the frame, as the pin's place: 0..1 of the mock-up, 3 decimals. */
export function pinAt(clientX: number, clientY: number, box: { left: number; top: number; width: number; height: number }): { x: number; y: number } | null {
  if (!(box.width > 0 && box.height > 0)) return null;
  const f = (v: number) => Math.round(Math.min(1, Math.max(0, v)) * 1000) / 1000;
  return { x: f((clientX - box.left) / box.width), y: f((clientY - box.top) / box.height) };
}
