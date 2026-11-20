// The reference (layer/builtin-pages.json) is what customisation runs are told and what the service
// checks. These tests hold it to the code, so it cannot drift: every page draws exactly the sections
// the reference names, every icon exists, every API call is handled by the host and offered by the
// frame runtime, and every permission has words in both languages.
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import REF from './layer/builtin-pages.json';

window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;

const here = (p: string) => readFileSync(fileURLToPath(new URL(p, import.meta.url)), 'utf8');
const ref = REF as any;

/** The keys of `parts={{ a: x, b: y }}` given to <Sections page="…"> in a page's source. */
function sectionsIn(src: string, page: string): string[] {
  const m = src.match(new RegExp(`<Sections page="${page}" parts=\\{\\{([^}]*)\\}\\}`));
  return m ? m[1].split(',').map((x) => x.split(':')[0].trim()).filter(Boolean) : [];
}

describe('the built-in pages reference', () => {
  const files: Record<string, string> = { todos: '../views/Todos.svelte', ideas: '../views/Ideas.svelte', chats: '../views/ChatList.svelte', projects: '../views/Projects.svelte', inbox: '../views/Inbox.svelte', graph: '../views/Graph.svelte', usage: '../views/usage/Usage.svelte' };
  for (const [page, file] of Object.entries(files)) {
    it(`${page} draws exactly the sections the reference lists`, () => {
      expect(sectionsIn(here(file), page).sort()).toEqual([...ref.pages[page].sections].sort());
      expect(here(file)).toContain(`<PageExtras page="${page}"`);
    });
  }
  it('Home draws exactly its sections', () => {
    const src = here('../views/Home.svelte');
    const m = src.match(/const partFor = \(k: string\) => \(\{([^}]*)\}/);
    expect(m).toBeTruthy();
    expect(m![1].split(',').map((x) => x.split(':')[0].trim()).filter(Boolean).sort()).toEqual([...ref.pages.home.sections].sort());
  });
  it('Settings categories are its sections', () => {
    const m = here('../views/Settings.svelte').match(/const cats = \[([^\]]*)\]/);
    expect(m![1].split(',').map((x) => x.trim().replace(/'/g, ''))).toEqual(ref.pages.settings.sections);
  });
  it('every part the reference lists is used by its page', () => {
    const files2: Record<string, string> = { ...files, home: '../views/Home.svelte', docs: '../views/Docs.svelte' };
    for (const [page, spec] of Object.entries(ref.pages as Record<string, any>)) {
      if (!files2[page]) continue;
      const src = here(files2[page]);
      for (const part of spec.parts) expect(src, `${page}.${part}`).toContain(`shows('${page}', '${part}')`);
      for (const label of spec.labels) expect(src, `${page}.${label}`).toContain(`pageText('${page}', '${label}'`);
    }
  });
  it('every sidebar icon exists', () => {
    const icon = here('./Icon.svelte');
    const body = icon.slice(icon.indexOf('export const icons'), icon.indexOf('};', icon.indexOf('export const icons')));
    const have = new Set([...body.matchAll(/([a-zA-Z0-9_]+)\s*:\s*[A-Z]/g)].map((m) => m[1]));
    for (const i of ref.icons) expect(have.has(i), i).toBe(true);
  });
  it('every API call is handled by the host and offered by the frame runtime', () => {
    const host = here('./layer.ts');
    const frame = here('../../public/plugin-frame.js');
    for (const m of Object.keys(ref.api)) {
      expect(host, m).toContain(`case '${m}'`);
      expect(frame, m).toContain(`'${m}'`);
    }
  });
  it('every permission has words in English and Arabic', async () => {
    const { t } = await import('./i18n');
    const { app } = await import('./store.svelte');
    const { permText } = await import('./layer');
    for (const lang of ['en', 'ar']) {
      app.settings = { ...(app.settings ?? {}), language: lang } as any;
      for (const p of ref.permissions) expect(permText(p), `${lang} ${p}`).not.toMatch(/^layer\.perm\./);
    }
    void t;
  });
});
