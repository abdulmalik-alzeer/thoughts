// The design's settings as CSS: the variables a mock-up and the previews read. Nothing from the
// settings is ever written into CSS as it came: a colour must be `#rrggbb`, a font is looked up in
// the app's list and its stack written, numbers are clamped, the rest are fixed words. So a file a
// run tampered with can at worst pick odd colours.
import type { DesignTokens } from '../types';
import { contrast, fixContrast, fromHsl, hsl, isDark, isHex, readableOn } from './contrast';
import { FONTS, fontStack, isFont } from './fonts';

export const RADIUS = { min: 0, max: 32 } as const;
export const SPACING = { min: 2, max: 16 } as const;
export const DENSITIES = ['compact', 'regular', 'roomy'] as const;
export const SHADOWS = ['none', 'soft', 'strong'] as const;
export const COLOR_ROLES = ['bg', 'surface', 'text', 'muted', 'primary', 'onPrimary', 'accent', 'border', 'success', 'warning', 'danger'] as const;
export type ColorRole = (typeof COLOR_ROLES)[number];
export const FONT_ROLES = ['heading', 'body', 'mono'] as const;
export type FontRole = (typeof FONT_ROLES)[number];

const DENSITY_SCALE: Record<string, string> = { compact: '0.82', regular: '1', roomy: '1.2' };
const SHADOW_CSS: Record<string, string> = {
  none: 'none',
  soft: '0 1px 2px rgba(0,0,0,.06), 0 4px 16px rgba(0,0,0,.08)',
  strong: '0 2px 4px rgba(0,0,0,.14), 0 14px 36px rgba(0,0,0,.24)',
};

/** What is drawn when a setting is missing or not valid. */
export const FALLBACK: DesignTokens = {
  color: { bg: '#f7f7f5', surface: '#ffffff', text: '#1f2328', muted: '#5c6370', primary: '#3b5bdb', onPrimary: '#ffffff', accent: '#0b7285', border: '#dedfe3', success: '#2b8a3e', warning: '#a85a00', danger: '#c92a2a' },
  font: { heading: 'System', body: 'System', mono: 'Mono' },
  radius: 8, spacing: 8, density: 'regular', shadow: 'soft',
};

const clamp = (v: unknown, lo: number, hi: number, d: number) => (typeof v === 'number' && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : d);
const kebab = (s: string) => s.replace(/[A-Z]/g, (m) => '-' + m.toLowerCase());

/** The settings with every value made valid (the fallback's where it was not). Never throws. */
export function sane(t: unknown): DesignTokens {
  const x = (t ?? {}) as any;
  const color = {} as DesignTokens['color'];
  for (const r of COLOR_ROLES) color[r] = isHex(x.color?.[r]) ? String(x.color[r]).toLowerCase() : FALLBACK.color[r];
  const font = {} as DesignTokens['font'];
  for (const r of FONT_ROLES) {
    const name = x.font?.[r];
    const ok = typeof name === 'string' && isFont(name) && (r !== 'mono' || FONTS.find((f) => f.name === name)!.kind === 'mono');
    font[r] = ok ? name : FALLBACK.font[r];
  }
  return {
    color, font,
    radius: clamp(x.radius, RADIUS.min, RADIUS.max, FALLBACK.radius),
    spacing: clamp(x.spacing, SPACING.min, SPACING.max, FALLBACK.spacing),
    density: (DENSITIES as readonly string[]).includes(x.density) ? x.density : FALLBACK.density,
    shadow: (SHADOWS as readonly string[]).includes(x.shadow) ? x.shadow : FALLBACK.shadow,
  };
}

export const same = (a: DesignTokens | null, b: DesignTokens | null) => (a && b ? JSON.stringify(sane(a)) === JSON.stringify(sane(b)) : a === b);

/** The variables of the contract (`--mado-color-bg` …), in a fixed order. `onPrimary` is written
 *  both as `--mado-color-on-primary` and `--mado-color-onPrimary`; `--mado-density` (a scale,
 *  1 = regular) is an addition a mock-up may use. */
export function tokenVars(t: unknown): [string, string][] {
  const s = sane(t);
  const out: [string, string][] = [];
  for (const r of COLOR_ROLES) out.push([`--mado-color-${kebab(r)}`, s.color[r]]);
  out.push(['--mado-color-onPrimary', s.color.onPrimary]);
  for (const r of FONT_ROLES) out.push([`--mado-font-${r}`, fontStack(s.font[r], r)]);
  out.push(['--mado-radius', `${s.radius}px`], ['--mado-spacing', `${s.spacing}px`], ['--mado-shadow', SHADOW_CSS[s.shadow]], ['--mado-density', DENSITY_SCALE[s.density]]);
  return out;
}
/** For a `style` attribute of a preview: the variables apply inside that element only. */
export const inlineVars = (t: unknown) => tokenVars(t).map(([k, v]) => `${k}:${v}`).join(';');
/** For the head of a mock-up. */
export const rootVars = (t: unknown) => `:root{${tokenVars(t).map(([k, v]) => `${k}:${v}`).join(';')}}`;

// ---------- reading a token by its path ("color.primary", "radius") ----------

export function getToken(t: DesignTokens, path: string): unknown {
  return path.split('.').reduce<any>((o, k) => (o == null ? undefined : o[k]), t);
}
/** A copy with one value changed. The path must be one of the settings' own. */
export function setToken(t: DesignTokens, path: string, value: unknown): DesignTokens {
  const next: any = { ...t, color: { ...t.color }, font: { ...t.font } };
  const [a, b] = path.split('.');
  if (b === undefined) { if (['radius', 'spacing', 'density', 'shadow'].includes(a)) next[a] = value; }
  else if (a === 'color' && (COLOR_ROLES as readonly string[]).includes(b)) next.color[b] = value;
  else if (a === 'font' && (FONT_ROLES as readonly string[]).includes(b)) next.font[b] = value;
  return next;
}

// ---------- contrast of the roles ----------

/** What each colour must be readable against, and how strictly: 4.5 for running text, 3 for
 *  colours that mark a control or carry a meaning. Grounds and the border have no demand. */
export const DEMANDS: Partial<Record<ColorRole, { on: ColorRole[]; target: number }>> = {
  text: { on: ['bg', 'surface'], target: 4.5 },
  muted: { on: ['bg', 'surface'], target: 4.5 },
  onPrimary: { on: ['primary'], target: 4.5 },
  primary: { on: ['bg', 'surface'], target: 3 },
  accent: { on: ['bg', 'surface'], target: 3 },
  success: { on: ['bg', 'surface'], target: 3 },
  warning: { on: ['bg', 'surface'], target: 3 },
  danger: { on: ['bg', 'surface'], target: 3 },
};

export interface Reading { ratio: number; target: number; ok: boolean; level: 'AAA' | 'AA' | 'low' }
/** The contrast of a role against the worst of its grounds; `null` for a role with no demand. */
export function reading(t: DesignTokens, role: ColorRole): Reading | null {
  const d = DEMANDS[role];
  if (!d) return null;
  const ratio = Math.min(...d.on.map((g) => contrast(t.color[role], t.color[g])));
  // Rounded down to what is shown, so "4.5" on screen is never a fail.
  const shown = Math.floor(ratio * 10) / 10;
  return { ratio: shown, target: d.target, ok: ratio >= d.target, level: ratio >= 7 ? 'AAA' : ratio >= d.target ? 'AA' : 'low' };
}
/** The role's colour nudged until it is readable (see `fixContrast`). */
export function fixed(t: DesignTokens, role: ColorRole): DesignTokens {
  const d = DEMANDS[role];
  if (!d) return t;
  return setToken(t, `color.${role}`, fixContrast(t.color[role], d.on.map((g) => t.color[g]), d.target));
}
/** Every role that is hard to read, fixed, text first (other fixes lean on the grounds, which stay). */
export function fixAll(t: DesignTokens, keep: ColorRole[] = []): DesignTokens {
  let out = t;
  for (const r of ['text', 'muted', 'primary', 'onPrimary', 'accent', 'success', 'warning', 'danger'] as ColorRole[]) if (!keep.includes(r)) out = fixed(out, r);
  return out;
}
export const lowRoles = (t: DesignTokens) => COLOR_ROLES.filter((r) => reading(t, r)?.ok === false);

// ---------- harmonies: one click derives colours from the primary ----------

export const HARMONIES = ['complement', 'analogous', 'triad', 'tinted', 'mono'] as const;
export type Harmony = (typeof HARMONIES)[number];

/** Colours derived from the primary. The accent ones turn the primary's hue; `tinted` gives the
 *  grounds, the border and the texts a breath of it; `mono` does both from one hue. What comes
 *  out is made readable, except the main colour itself, which is never touched. */
export function harmony(t: DesignTokens, kind: Harmony): DesignTokens {
  const [h, s, l] = hsl(t.color.primary);
  const dark = isDark(t.color.bg);
  let c = { ...t.color };
  const accent = (turn: number) => fromHsl(h + turn, Math.max(35, s), dark ? Math.max(l, 62) : Math.min(l, 42));
  const tint = () => {
    c.bg = fromHsl(h, 22, dark ? 8 : 97);
    c.surface = fromHsl(h, 22, dark ? 12 : 100);
    c.border = fromHsl(h, 18, dark ? 24 : 88);
    c.text = fromHsl(h, 28, dark ? 93 : 13);
    c.muted = fromHsl(h, 14, dark ? 68 : 40);
  };
  if (kind === 'complement') c.accent = accent(180);
  else if (kind === 'analogous') c.accent = accent(32);
  else if (kind === 'triad') c.accent = accent(120);
  else if (kind === 'tinted') tint();
  else { tint(); c.accent = fromHsl(h, Math.max(20, s * 0.55), dark ? 74 : 30); }
  c.onPrimary = contrast(c.onPrimary, c.primary) >= 4.5 ? c.onPrimary : readableOn(c.primary);
  // The main colour is the user's own choice: everything is derived from it, it is never changed here.
  return fixAll({ ...t, color: c }, ['primary']);
}
