// Colour maths for the design step: contrast as WCAG 2 defines it, a nudge that makes a colour
// readable with the smallest change, and a few one-click harmonies. All on `#rrggbb`.

export const HEX = /^#[0-9a-f]{6}$/i;
export const isHex = (c: unknown): c is string => typeof c === 'string' && HEX.test(c);

export function rgb(hex: string): [number, number, number] {
  if (!isHex(hex)) return [0, 0, 0];
  return [parseInt(hex.slice(1, 3), 16), parseInt(hex.slice(3, 5), 16), parseInt(hex.slice(5, 7), 16)];
}
const two = (n: number) => Math.round(Math.min(255, Math.max(0, n))).toString(16).padStart(2, '0');
export const hex = (r: number, g: number, b: number) => `#${two(r)}${two(g)}${two(b)}`;

/** Relative luminance, 0 (black) to 1 (white). */
export function luminance(c: string): number {
  const ch = (v: number) => { const s = v / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
  const [r, g, b] = rgb(c);
  return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b);
}

/** The contrast ratio of two colours, 1 to 21. */
export function contrast(a: string, b: string): number {
  const [la, lb] = [luminance(a), luminance(b)];
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** Hue 0–360, saturation and lightness 0–100. */
export function hsl(c: string): [number, number, number] {
  const [r, g, b] = rgb(c).map((v) => v / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  const l = (max + min) / 2;
  if (d === 0) return [0, 0, l * 100];
  const s = d / (1 - Math.abs(2 * l - 1));
  let h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  h = (h * 60 + 360) % 360;
  return [h, s * 100, l * 100];
}

export function fromHsl(h: number, s: number, l: number): string {
  h = ((h % 360) + 360) % 360; s = Math.min(100, Math.max(0, s)) / 100; l = Math.min(100, Math.max(0, l)) / 100;
  const c = (1 - Math.abs(2 * l - 1)) * s, x = c * (1 - Math.abs(((h / 60) % 2) - 1)), m = l - c / 2;
  const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return hex((r + m) * 255, (g + m) * 255, (b + m) * 255);
}

/** `fg` changed as little as possible (lightness only, hue kept) until it reaches `target` against
 *  every one of `grounds`. Goes the way that needs the smaller step; when neither way gets there
 *  (two grounds far apart), the best it found. A colour that already passes comes back unchanged. */
export function fixContrast(fg: string, grounds: string[], target: number): string {
  const worst = (c: string) => Math.min(...grounds.map((g) => contrast(c, g)));
  if (!isHex(fg) || !grounds.length || worst(fg) >= target) return fg;
  const [h, s, l] = hsl(fg);
  let best = fg, bestScore = worst(fg);
  const tryWay = (dir: 1 | -1): { c: string; step: number } | null => {
    for (let step = 1; step <= 100; step++) {
      const nl = l + dir * step;
      if (nl < 0 || nl > 100) break;
      const c = fromHsl(h, s, nl);
      const score = worst(c);
      if (score > bestScore) { best = c; bestScore = score; }
      if (score >= target) return { c, step };
    }
    return null;
  };
  const darker = tryWay(-1), lighter = tryWay(1);
  if (darker && lighter) return darker.step <= lighter.step ? darker.c : lighter.c;
  return (darker ?? lighter)?.c ?? best;
}

/** Black or white, whichever reads better on `bg`. */
export const readableOn = (bg: string): string => (contrast('#ffffff', bg) >= contrast('#111111', bg) ? '#ffffff' : '#111111');

export const isDark = (bg: string) => luminance(bg) < 0.18;
