// @vitest-environment node
import { describe, expect, it } from 'vitest';

// An entrance animation that slides or blurs (transform/filter) must not keep filling after it ends.
// While an animation of transform or filter is in effect, browsers treat the element as transformed,
// so it becomes the containing block of every `position: fixed` box inside it: a dialog opened in a
// page section (Customise › Routines, Automate, add-ons…) would be placed and clipped by that section,
// partly off screen, instead of covering the window. `backwards` still holds the first frame during
// the delay, then lets go.
const svelte = import.meta.glob('../**/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
// Only the <style> blocks: markup and scripts are not CSS. (theme.css is read from disk: the test
// runner hands stylesheets over empty.)
const fs: { readFileSync(p: URL, enc: string): string } = await import(/* @vite-ignore */ 'node:fs' as string);
const sources: Record<string, string> = { '../theme.css': fs.readFileSync(new URL('../theme.css', import.meta.url), 'utf8') };
for (const [file, src] of Object.entries(svelte)) {
  const css = [...src.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map((m) => m[1]).join('\n');
  if (css) sources[file] = css;
}

function moving(css: string): Set<string> {
  const names = new Set<string>();
  for (const m of css.matchAll(/@keyframes\s+([\w-]+)\s*\{((?:[^{}]*\{[^{}]*\})*[^{}]*)\}/g)) {
    if (/\b(transform|filter|translate|scale|rotate)\s*:/.test(m[2])) names.add(m[1]);
  }
  return names;
}

describe('entrance animations', () => {
  const all = Object.values(sources).join('\n');
  const names = moving(all);

  it('finds the moving entrance animations', () => {
    for (const n of ['rise', 'view-in', 'pop', 'pane-in']) expect(names.has(n)).toBe(true);
  });

  it('never fill forwards on page sections, views or panes, so dialogs inside them cover the window', () => {
    const bad: string[] = [];
    for (const [file, css] of Object.entries(sources)) {
      for (const m of css.matchAll(/([^{};]*)\{[^{}]*?animation\s*:\s*([^;}]+)/g)) {
        const [sel, decl] = [m[1].trim(), m[2]];
        const name = decl.split(/\s+/).find((w) => names.has(w));
        if (!name || !/\b(both|forwards)\b/.test(decl)) continue;
        // The dialog box and its backdrop are the fixed boxes themselves (and already blur what is behind them).
        if (/\.modal|\.toast/.test(sel)) continue;
        bad.push(`${file}: ${sel} { animation: ${decl} }`);
      }
    }
    expect(bad).toEqual([]);
  });
});
