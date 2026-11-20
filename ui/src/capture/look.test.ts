import { describe, expect, it } from 'vitest';
import { themeFor, themeTokens } from './look';

const SAND = `[data-theme="sand"][data-mode="light"] { --bg: #f7f1e5; --accent: #a4571b; }
[data-theme="sand"][data-mode="dark"] { --bg: #19140e; --accent: #e39a4f !important; }
:root { --font: "Sand Sans", sans-serif; }
.sidebar { display: none; }
textarea { color: red; --x: 1px; }
body, main { background: hotpink; }
@media (prefers-reduced-motion: reduce) { :root { --speed: 0s; } .nav { opacity: 0; } }
@media print { .nav { color: red; } }
@font-face { font-family: "Sand Sans"; src: url(data:font/woff2;base64,AAAA); }`;

describe('quick capture look', () => {
  it('takes only the theme tokens from the customisations', () => {
    const out = themeTokens(SAND);
    expect(out).toContain('--bg:#f7f1e5');
    expect(out).toContain('--accent:#a4571b');
    expect(out).toContain('--accent:#e39a4f !important');
    expect(out).toContain('--font:"Sand Sans", sans-serif');
    // A rule that restyles the app keeps its tokens and loses everything else.
    expect(out).toContain('--x:1px');
    expect(out).not.toMatch(/display|hotpink|color:\s*red|opacity/);
    // Tokens inside @media keep their condition; a block with no tokens is dropped.
    expect(out).toMatch(/@media[^{]*reduce[^{]*\{:root\{--speed:0s\}\}/);
    expect(out).not.toContain('print');
    // Fonts a theme brings come along.
    expect(out).toContain('@font-face');
    // Nothing was left in the page while reading.
    expect(document.head.querySelectorAll('style').length).toBe(0);
  });

  it('reads nothing from nothing', () => {
    expect(themeTokens('')).toBe('');
    expect(themeTokens('not css at all {{{')).toBe('');
    expect(themeTokens(undefined as any)).toBe('');
  });

  it('paints the chosen theme when it exists, the standard one otherwise', () => {
    expect(themeFor('tech')).toBe('tech');
    expect(themeFor('contrast', [])).toBe('contrast');
    expect(themeFor('sand', [{ id: 'sand' }])).toBe('sand');
    // Removed from Customise, or customisations off: never a theme with no colours.
    expect(themeFor('sand', [])).toBe('saudi');
    expect(themeFor('sand', [{ id: 'other' }, null as any])).toBe('saudi');
    expect(themeFor(undefined)).toBe('saudi');
    expect(themeFor('')).toBe('saudi');
  });
});
