// Colours for the Graph page, all from the theme that is on: one colour per area, made from the
// theme's accent colours (the accent itself, then the same colour turned round the colour wheel, at
// the theme's own lightness), each made dark or light enough to stand out from the page (≥ 3:1).
// Works for every theme, built-in or added through Customise, by day and by night.

export type Rgb = [number, number, number];
export type Ink = { bg: string; fg: string; muted: string; accent: string; line: string; panel: string };

/** `rgb(1, 2, 3)`, `rgba(…)`, `#abc`, `#aabbcc` or `color(srgb 0.1 0.2 0.3)` → [r, g, b] (0–255). */
export function parseColor(s: string): Rgb | null {
  const v = String(s ?? '').trim().toLowerCase();
  let m = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/.exec(v);
  if (m) return [+m[1], +m[2], +m[3]];
  m = /^color\(srgb\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)/.exec(v);
  if (m) return [+m[1] * 255, +m[2] * 255, +m[3] * 255];
  m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/.exec(v);
  if (m) {
    const h = m[1].length === 3 ? m[1].split('').map((c) => c + c).join('') : m[1];
    return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
  }
  return null;
}

export const hex = (c: Rgb) => '#' + c.map((x) => Math.round(Math.max(0, Math.min(255, x))).toString(16).padStart(2, '0')).join('');

function luminance([r, g, b]: Rgb): number {
  const f = (x: number) => { const c = x / 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}
export function contrast(a: Rgb, b: Rgb): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

function toHsl([r, g, b]: Rgb): [number, number, number] {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min, s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h * 60, s, l];
}
function fromHsl(h: number, s: number, l: number): Rgb {
  h = ((h % 360) + 360) % 360 / 360;
  if (s === 0) return [l * 255, l * 255, l * 255];
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s, p = 2 * l - q;
  const f = (t: number) => { t = (t + 1) % 1; return t < 1 / 6 ? p + (q - p) * 6 * t : t < 1 / 2 ? q : t < 2 / 3 ? p + (q - p) * (2 / 3 - t) * 6 : p; };
  return [f(h + 1 / 3) * 255, f(h) * 255, f(h - 1 / 3) * 255];
}

/** `c` made darker (on a light page) or lighter (on a dark one) until it stands out ≥ `min`:1. */
export function legible(c: Rgb, bg: Rgb, min = 3, also?: Rgb): Rgb {
  const [h, s] = toHsl(c);
  let l = toHsl(c)[2];
  const dark = luminance(bg) < 0.4;
  let out = c;
  const low = (x: Rgb) => contrast(x, bg) < min || (!!also && contrast(x, also) < min);
  for (let i = 0; i < 40 && low(out); i++) {
    l = dark ? Math.min(1, l + 0.025) : Math.max(0, l - 0.025);
    out = fromHsl(h, s, l);
  }
  return out;
}

const far = (a: Rgb, b: Rgb) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]) > 48;

/** Up to `count` area colours from the theme's colours. */
export function derivePalette(t: { accent: string; accent2?: string; bg: string; panel?: string }, count = 10): string[] {
  const bg = parseColor(t.bg) ?? [255, 255, 255];
  const panel = t.panel ? parseColor(t.panel) ?? undefined : undefined;
  const accent = parseColor(t.accent) ?? (luminance(bg) < 0.4 ? [120, 170, 255] : [20, 70, 170]);
  const out: Rgb[] = [];
  const add = (c: Rgb) => { const l = legible(c, bg, 3, panel); if (out.every((o) => far(o, l))) out.push(l); };
  add(accent);
  const a2 = t.accent2 ? parseColor(t.accent2) : null;
  if (a2 && toHsl(a2)[1] > 0.25) add(a2);
  const [h, s0, l0] = toHsl(accent);
  // A grey accent still gives colours: enough colour to tell areas apart, at its lightness.
  const s = Math.max(s0, 0.55), l = Math.min(Math.max(l0, 0.3), 0.7);
  const steps = [150, 60, 210, 105, 270, 30, 180, 240, 315, 90, 330, 15];
  for (const d of steps) { if (out.length >= count) break; add(fromHsl(h + d, s, l)); }
  return out.slice(0, count).map(hex);
}

/** The theme's colours as they are on the page now (read through a hidden element, so `var()` and
 *  `color-mix()` resolve exactly as the app draws them). */
export function readTheme(el: Element = document.documentElement): { palette: string[]; ink: Ink } {
  const probe = document.createElement('span');
  probe.setAttribute('aria-hidden', 'true');
  probe.style.cssText = 'position:absolute;width:0;height:0;overflow:hidden;visibility:hidden;pointer-events:none;';
  el.appendChild(probe);
  const read = (v: string, fallback: string) => {
    probe.style.backgroundColor = '';
    probe.style.backgroundColor = `var(${v}, ${fallback})`;
    const c = getComputedStyle(probe).backgroundColor;
    const rgb = parseColor(c);
    return rgb && !/,\s*0\)$/.test(c) ? hex(rgb) : fallback;
  };
  try {
    const bg = read('--bg', '#ffffff');
    const dark = luminance(parseColor(bg) ?? [255, 255, 255]) < 0.4;
    const ink: Ink = {
      bg, panel: read('--panel', bg),
      fg: read('--fg', dark ? '#ffffff' : '#000000'),
      muted: read('--muted', dark ? '#bbbbbb' : '#555555'),
      accent: read('--accent', dark ? '#88aaff' : '#1144aa'),
      line: read('--border', dark ? '#444444' : '#cccccc'),
    };
    return { palette: derivePalette({ accent: ink.accent, accent2: read('--accent-2', ink.accent), bg, panel: ink.panel }), ink };
  } finally { probe.remove(); }
}
