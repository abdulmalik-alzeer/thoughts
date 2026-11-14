// What a change to `pages.json` does, in words for the review (entries from `staging/pages.rs`:
// `{area: "pages", op, page, …}`). Values from the files are set apart so their direction never
// reorders the sentence around them.
import { t } from './i18n';
import type { FxLine } from './effects';

const q = (v: unknown) => `⁨${String(v ?? '')}⁩`;
const PAGE_NAMES = ['home', 'chats', 'ideas', 'todos', 'docs', 'projects'];
const page = (id: string) => q(PAGE_NAMES.includes(id) ? t(id) : id);
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
    case 'replaced': return line('changed', t(k('replaced'), { page: P, by: q(e.by) }), [t(k('replacedNote'))]);
    case 'replaceRemoved': return line('changed', t(k('replaceRemoved'), { page: P }));
  }
  return null;
}
