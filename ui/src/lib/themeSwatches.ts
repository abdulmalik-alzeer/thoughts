// The three colours a theme card in the picker shows: accent, background, second accent, read from
// the theme's own tokens for the given mode (Day / Night). Built-in themes and themes added through
// Customise are read the same way, so a theme added while the app is running shows its colours as
// soon as its styling is loaded.
//
// How: a hidden probe element carries `data-theme` / `data-mode`, so it matches exactly the rules
// that colour the app when that theme is on. Its parent resets the tokens, so nothing leaks in from
// the theme that is on now. Each token is read through a real colour property, which resolves
// `var()` / `color-mix()` in the theme's own context. A token the theme leaves out falls back to the
// next one that theme does define, and finally to the system's plain page and text colours for that
// mode, which is what such a theme shows. Never blank.

const TOKENS = ['--accent', '--accent-2', '--bg', '--panel', '--sidebar', '--fg'] as const;
type Token = (typeof TOKENS)[number];

/** For each swatch, the tokens to try in order; `base` / `ink` are the mode's plain page / text colour. */
const CHAINS: (Token | 'base' | 'ink')[][] = [
  ['--accent', '--accent-2', '--fg', 'ink'],
  ['--bg', '--panel', '--sidebar', 'base'],
  ['--accent-2', '--accent', '--fg', 'ink'],
];

const empty = (c: string) => !c || c === 'transparent' || /^rgba\(.*,\s*0\)$/.test(c) || /\/\s*0\)$/.test(c);

/** The swatch colours of each theme id for `mode` (`light` or `dark`), as computed colour strings. */
export function themeSwatches(ids: string[], mode: 'light' | 'dark'): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  if (typeof document === 'undefined' || !document.body || !ids.length) return out;
  const host = document.createElement('div');
  host.setAttribute('aria-hidden', 'true');
  host.style.cssText = 'position:fixed;inset-inline-start:-9999px;top:0;width:0;height:0;overflow:hidden;visibility:hidden;pointer-events:none;contain:strict;'
    + TOKENS.map((n) => `${n}:initial;`).join('');
  const probes = ids.map((id) => {
    const p = document.createElement('div');
    p.dataset.theme = id;
    p.dataset.mode = mode;
    p.style.colorScheme = mode;
    const spans = new Map<string, HTMLElement>();
    for (const n of TOKENS) {
      const s = document.createElement('span');
      s.style.backgroundColor = `var(${n})`;
      p.appendChild(s); spans.set(n, s);
    }
    const base = document.createElement('span'); base.style.backgroundColor = 'Canvas'; p.appendChild(base); spans.set('base', base);
    const ink = document.createElement('span'); ink.style.backgroundColor = 'CanvasText'; p.appendChild(ink); spans.set('ink', ink);
    host.appendChild(p);
    return { id, spans };
  });
  document.body.appendChild(host);
  try {
    for (const { id, spans } of probes) {
      const read = new Map<string, string>();
      for (const [n, s] of spans) read.set(n, getComputedStyle(s).backgroundColor);
      out[id] = CHAINS.map((chain) => {
        for (const n of chain) { const c = read.get(n) ?? ''; if (!empty(c)) return c; }
        return mode === 'dark' ? '#000000' : '#ffffff';
      });
    }
  } finally {
    host.remove();
  }
  return out;
}
