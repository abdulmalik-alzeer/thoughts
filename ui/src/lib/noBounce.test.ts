// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { NO_BOUNCE } from './noBounce';

// No page or pane may stretch past its ends (theme.css, "No elastic scrolling anywhere"). The rule is
// global, so what can break it is a component that says otherwise for itself; these checks read every
// stylesheet the window loads. (theme.css is read from disk: the test runner hands stylesheets over empty.)
const svelte = import.meta.glob('../**/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const scripts = import.meta.glob(['../**/*.ts', '!../**/*.test.ts'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const fs: { readFileSync(p: URL, enc: string): string } = await import(/* @vite-ignore */ 'node:fs' as string);
const theme = fs.readFileSync(new URL('../theme.css', import.meta.url), 'utf8');
const sources: Record<string, string> = { '../theme.css': theme };
for (const [file, src] of Object.entries(svelte)) {
  const css = [...src.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map((m) => m[1]).join('\n');
  if (css) sources[file] = css;
}
// Styles written as text in a module (rendered notes, figures, the shield).
for (const [file, src] of Object.entries(scripts)) if (/overflow|overscroll/.test(src)) sources[file] = src;
const plain = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, '');
/** Every `{ … }` of declarations, without nested blocks. */
const blocks = (css: string) => plain(css).match(/\{[^{}]*\}/g) ?? [];

describe('no elastic scrolling', () => {
  it('the document cannot scroll and every scroll container stops at its ends', () => {
    const css = plain(theme);
    expect(css).toMatch(/html, body \{ overflow: hidden; overscroll-behavior: none; \}/);
    expect(css).toMatch(/#app \{ height: 100%; overflow: auto; \}/);
    expect(css).toMatch(/\*, \*::before, \*::after \{ overscroll-behavior: none; \}/);
    // Printing needs the whole page again, not one window of it.
    expect(css.slice(css.indexOf('@media print'))).toMatch(/#app \{ height: auto; overflow: visible; \}/);
  });

  it('no component switches the bounce back on', () => {
    const bad: string[] = [];
    for (const [file, css] of Object.entries(sources)) {
      // The one allowed exception is the up-and-down axis of a strip that only scrolls sideways (below).
      for (const m of plain(css).matchAll(/overscroll-behavior(-x|-y|-inline|-block)?\s*:\s*([a-z ]+)/g)) {
        if (m[2].trim() !== 'none' && !(m[1] === '-y' && m[2].trim() === 'auto')) bad.push(`${file}: ${m[0]}`);
      }
    }
    expect(bad).toEqual([]);
  });

  it('a strip that only scrolls sideways still lets the page scroll up and down over it', () => {
    const bad: string[] = [];
    for (const [file, css] of Object.entries(sources)) {
      for (const b of blocks(css)) {
        const sideways = /overflow-x\s*:\s*(auto|scroll)/.test(b) && !/overflow-y\s*:\s*(auto|scroll)/.test(b);
        if (sideways !== /overscroll-behavior-y\s*:\s*auto/.test(b)) bad.push(`${file}: ${b.slice(0, 90)}`);
      }
    }
    expect(bad).toEqual([]);
  });

  it('the rule for frames is a style rule and nothing else', () => {
    expect(NO_BOUNCE).toBe('html,body,*,*::before,*::after{overscroll-behavior:none}');
  });
});
