// What a change to `pages.json` does, in words for the review (entries from `staging/pages.rs`:
// `{area: "pages", op, page, …}`). Values from the files are set apart so their direction never
// reorders the sentence around them.
import { t } from './i18n';
import type { FxLine } from './effects';
import { app } from './store.svelte';

const q = (v: unknown) => `⁨${String(v ?? '')}⁩`;
const PAGE_NAMES = ['home', 'chats', 'ideas', 'todos', 'docs', 'projects', 'settings'];
const OWN: Record<string, string> = { graph: 'libf-graph.nav', inbox: 'inbox.nav', usage: 'r10-usage.nav' };
const page = (id: string) => q(PAGE_NAMES.includes(id) ? t(id) : OWN[id] ? t(OWN[id]) : id);
/** A section of a page in words (its name from the dictionary, else its id). */
function section(pg: string, id: string): string {
  if (id.startsWith('widget:')) return t('r10-cust-pages.fx.box', { name: q(id.slice(7)) });
  const k = `pgs.sec.${pg}.${id}`, s = t(k);
  return s !== k ? q(s) : q(id);
}
function part(pg: string, name: string): string {
  const k = `r10-cust-pages.part.${pg}.${name}`, s = t(k);
  return s !== k ? s : q(name);
}
/** `{name}` / `{other}` from a text given in one or both languages, and whether both are there. */
function names(e: any): { both: boolean; name: string; other: string } {
  const en = typeof e.en === 'string' ? e.en : '', ar = typeof e.ar === 'string' ? e.ar : '';
  return en && ar ? { both: true, name: q(en), other: q(ar) } : { both: false, name: q(en || ar), other: '' };
}

export function pagesFx(e: any, key: string): FxLine | null {
  const pg = String(e.page ?? ''), P = page(pg);
  const line = (tag: FxLine['tag'], text: string, detail: string[] = []): FxLine => ({ tag, text, detail, key });
  const k = (s: string) => `r10-cust-pages.fx.${s}`;
  switch (e.op) {
    case 'title': case 'intro': {
      // Home's title is the greeting: shown as it will read this morning, with the person's name.
      if (pg === 'home' && e.op === 'title') {
        const fill = (s: unknown) => (typeof s === 'string' ? s.replaceAll('{greeting}', t('goodMorning')).replaceAll('{name}', app.settings?.userName || t('r10-cust-pages.fx.yourName')) : s);
        const n = names({ en: fill(e.en), ar: fill(e.ar) });
        return line('changed', t(k(n.both ? 'greetingBoth' : 'greeting'), { name: n.name, other: n.other }));
      }
      const n = names(e);
      return line('changed', t(k(e.op + (n.both ? 'Both' : '')), { page: P, name: n.name, other: n.other }));
    }
    case 'titleRemoved': case 'introRemoved': return line('changed', t(k(e.op), { page: P }));
    case 'hidden': case 'shown': return line('changed', t(k(e.op), { page: P, part: part(pg, String(e.what)) }));
    case 'renamed': {
      const n = names(e);
      return line('changed', t(k(n.both ? 'renamedBoth' : 'renamed'), { page: P, part: part(pg, String(e.what)), name: n.name, other: n.other }));
    }
    case 'renameRemoved': return line('changed', t(k('renameRemoved'), { page: P, part: part(pg, String(e.what)) }));
    case 'boxes': {
      const place = e.place === 'bottom' ? 'bottom' : 'top';
      const boxes = (Array.isArray(e.boxes) ? e.boxes : []).filter((x: unknown) => typeof x === 'string').map((x: string) => t(k('box'), { name: q(x.replace(/^widget:/, '')) }));
      return line('changed', boxes.length ? t(k('boxes.' + place), { page: P, list: boxes.join(t('ldb.listSep')) }) : t(k('boxesNone.' + place), { page: P }));
    }
    case 'sectionOrder': return line('changed', t(k('sectionOrder'), { page: P, list: (Array.isArray(e.order) ? e.order : []).map((x: string) => section(pg, String(x))).join(t('ldb.listSep')) }));
    case 'sectionOrderReset': return line('changed', t(k('sectionOrderReset'), { page: P }));
    case 'sectionHidden': case 'sectionShown': case 'sectionRestored': return line('changed', t(k(e.op), { page: P, what: section(pg, String(e.what)) }));
    case 'sectionSize': return line('changed', t(k('sectionSize'), { page: P, what: section(pg, String(e.what)), size: t('pgs.size.' + String(e.size)) }));
    case 'sectionReplaced': return line('changed', t(k('sectionReplaced'), { page: P, what: section(pg, String(e.what)), by: section(pg, String(e.by)) }), [t(k('sectionReplacedNote'))]);
    case 'sectionInsert': return line('changed', t(k(e.place === 'before' ? 'sectionInsertBefore' : 'sectionInsertAfter'), { page: P, box: section(pg, String(e.box)), what: section(pg, String(e.what)) }));
    case 'sectionInsertRemoved': return line('changed', t(k('sectionInsertRemoved'), { page: P, box: section(pg, String(e.box)) }));
    case 'replaced': return line('changed', t(k('replaced'), { page: P, by: q(e.by) }), [t(k('replacedNote'))]);
    case 'replaceRemoved': return line('changed', t(k('replaceRemoved'), { page: P }));
  }
  return null;
}
