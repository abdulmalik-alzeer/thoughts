// What a customisation does, in words: the service reads each change from its files
// (`staging/effects.rs`: add-ons and pages with the permissions they gain or lose, Home shortcuts
// and sections, the sidebar, todo settings, themes, the look, automatic tasks) and this module words
// each entry for the review and for the question after an app update. Nothing here is taken from
// what the assistant or a pack says about the change.
import { t, lang } from './i18n';
import { permText } from './layer';
import { pagesFx } from './pagesFx';

export type Effect = { area: string; op: string; [k: string]: any };
/** One line of the description: a tag (new / changed / removed), the sentence, details under it,
 *  text quoted as it will be used (a shortcut's prompt), and for an add-on or page what it may do. */
export type FxLine = { tag: '' | 'added' | 'changed' | 'removed'; text: string; detail: string[]; quote?: string; mayTitle?: string; may?: string[]; key: string };

/** Values from the files are set apart so their direction never reorders the sentence around them. */
const q = (v: unknown) => `\u2068${String(v ?? '')}\u2069`;
const list = (xs: unknown[]) => xs.map(q).join(t('ldb.listSep'));
const strs = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);

const BUILT_IN_PAGES = ['home', 'chats', 'ideas', 'todos', 'docs', 'projects', 'me'];
function page(id: string): string { return q(BUILT_IN_PAGES.includes(id) ? t(id as any) : id); }

function section(id: string): string {
  const known: Record<string, string> = { stats: 'ldb.sec.stats', ideas: 'continueWhere', upcoming: 'home.upcoming', todos: 'todos', chats: 'recentChats', projects: 'projects', suggestions: 'ldb.sec.suggestions' };
  if (known[id]) return q(t(known[id] as any));
  if (id.startsWith('widget:')) return t('ldb.sec.widget', { name: q(id.slice(7)) });
  return q(id);
}

function todoValue(key: string, value: unknown): string {
  if (typeof value === 'boolean') return t(`ldb.todos.${key}.${value ? 'on' : 'off'}`);
  return t(`ldb.todos.${key}`, { value: t(`ldb.todos.${key}.${String(value)}`) });
}

/** What an add-on or page may do, one line each (an add-on that asks for nothing still adds things).
 *  `read`: the only folders it reads in (layerui-b, FEAT-44), said on each line that reads. */
export function mayDo(kind: 'addon' | 'page', permissions: string[], write: string[], read: string[] = []): string[] {
  if (!permissions.length) return [t(kind === 'page' ? 'ldb.pageAsksNothing' : 'layer.pluginAsksNothing')];
  const lines = permissions.map((p) => p === 'library:write'
    ? (write.length ? `${permText(p)}: ${list(write)}${read.length ? ` — ${t('cln.addon.changeOnly', { folders: list(read) })}` : ''}` : `${permText(p)} — ${t('layer3.writeNone')}`)
    // An item's status only inside the declared folders (layerui-a, BUG-20).
    : p === 'ideas:write'
      ? (write.length ? `${permText(p)} — ${t('lua.statusIn')} ${list(write)}` : `${permText(p)} — ${t('lua.statusNone')}`)
      // Reading only in the `read` folders (layerui-b, FEAT-44).
      : read.length && READS.includes(p) ? `${permText(p)} — ${t('lub.onlyIn')} ${list(read)}` : permText(p));
  return lines;
}
/** The permissions a `read` list narrows. */
const READS = ['library:read', 'todos:read', 'ideas:read'];

function item(e: Effect): FxLine {
  const kind = e.area === 'page' ? 'page' : 'addon';
  const name = q(e.name);
  const perms = strs(e.permissions), write = strs(e.write), read = strs(e.read);
  const detail: string[] = [];
  const key = `${kind}:${e.name}`;
  if (e.description) detail.push(t('ldb.fx.saysItIs', { text: q(e.description) }));
  if (e.op === 'added') {
    if (e.unsupported) return { tag: 'added', text: t(`ldb.fx.${kind}.added`, { name }), detail: [...detail, t('ldb.fx.unsupported')], key };
    // (One that asks for nothing gets one sentence saying what it can still do, not a list.)
    if (!perms.length) return { tag: 'added', text: t(`ldb.fx.${kind}.added`, { name }), detail: [...detail, ...mayDo(kind, perms, write, read)], key };
    return { tag: 'added', text: t(`ldb.fx.${kind}.added`, { name }), detail, mayTitle: t('ldb.fx.mayTitle'), may: mayDo(kind, perms, write, read), key };
  }
  if (e.op === 'removed') return { tag: 'removed', text: t(`ldb.fx.${kind}.removed`, { name }), detail: [], key };
  const gained = strs(e.gained), lost = strs(e.lost), wGained = strs(e.writeGained), wLost = strs(e.writeLost);
  // Reading the other way round (layerui-b): a list that appears narrows it, one that goes widens it.
  const readBefore = strs(e.readBefore), readChanged = e.readChanged === true;
  const asks = gained.length || lost.length || wGained.length || wLost.length || readChanged;
  for (const p of gained) detail.push(t('ldb.fx.nowAlso', { what: p === 'library:write' && write.length ? `${permText(p)}: ${list(write)}` : permText(p) }));
  if (wGained.length && !gained.includes('library:write')) detail.push(t('ldb.fx.nowAlsoFolders', { folders: list(wGained) }));
  for (const p of lost) detail.push(t('ldb.fx.noLonger', { what: permText(p) }));
  if (wLost.length && !lost.includes('library:write')) detail.push(t('ldb.fx.noLongerFolders', { folders: list(wLost) }));
  if (readChanged) detail.push(!read.length ? t('lub.readWide', { folders: list(readBefore) }) : readBefore.length ? t('lub.readMoved', { folders: list(read), before: list(readBefore) }) : t('lub.readNow', { folders: list(read) }));
  if (e.code) detail.push(t(asks ? 'ldb.fx.codeToo' : 'ldb.fx.codeOnly'));
  if (e.descriptionChanged && !e.description) detail.push(t('ldb.fx.descriptionGone'));
  if (e.unsupported) detail.push(t('ldb.fx.unsupported'));
  const line: FxLine = { tag: 'changed', text: t(`ldb.fx.${kind}.changed`, { name }), detail, key };
  if (asks && !e.unsupported) {
    if (perms.length) { line.mayTitle = t('ldb.fx.fromNowOn'); line.may = mayDo(kind, perms, write, read); } else detail.push(...mayDo(kind, perms, write, read));
  }
  return line;
}

/** One entry in words, or null for one this version of the app cannot word (never shown blank). */
export function fxLine(e: Effect, i = 0): FxLine | null {
  if (!e || typeof e.area !== 'string' || typeof e.op !== 'string') return null;
  const key = `${e.area}:${e.op}:${i}`;
  const line = (tag: FxLine['tag'], text: string, more: Partial<FxLine> = {}): FxLine => ({ tag, text, detail: [], key, ...more });
  switch (e.area) {
    case 'addon': case 'page': return item(e);
    case 'home':
      switch (e.op) {
        case 'chipAdded': return line('added', t('ldb.fx.chipAdded', { label: q(e.label) }), { quote: String(e.prompt ?? '') });
        case 'chipChanged': return line('changed', t('ldb.fx.chipChanged', { label: q(e.label) }), { quote: String(e.prompt ?? '') });
        case 'chipRemoved': return line('removed', t('ldb.fx.chipRemoved', { label: q(e.label) }));
        case 'hidden': return line('changed', t('ldb.fx.homeHides', { what: section(String(e.what)) }));
        case 'shown': return line('changed', t('ldb.fx.homeShows', { what: section(String(e.what)) }));
        case 'order': return line('changed', t('ldb.fx.homeOrder', { order: strs(e.order).map(section).join(t('ldb.listSep')) }));
        case 'orderReset': return line('changed', t('ldb.fx.homeOrderReset'));
      }
      break;
    case 'nav':
      switch (e.op) {
        case 'hidden': return line('changed', t('ldb.fx.navHides', { what: page(String(e.what)) }));
        case 'shown': return line('changed', t('ldb.fx.navShows', { what: page(String(e.what)) }));
        case 'renamed': {
          const names = [e.en ? q(e.en) : '', e.ar ? q(e.ar) : ''].filter(Boolean);
          return line('changed', t(names.length > 1 ? 'ldb.fx.navRenamedBoth' : 'ldb.fx.navRenamed', { what: page(String(e.what)), name: names[0] ?? '', other: names[1] ?? '' }));
        }
        case 'renameRemoved': return line('changed', t('ldb.fx.navRenameRemoved', { what: page(String(e.what)) }));
        case 'order': return line('changed', t('ldb.fx.navOrder', { order: strs(e.order).map(page).join(t('ldb.listSep')) }));
        case 'orderReset': return line('changed', t('ldb.fx.navOrderReset'));
        case 'icon': return line('changed', t('pgs.fx.navIcon', { what: page(String(e.what)), icon: q(e.icon) }));
        case 'iconRemoved': return line('changed', t('pgs.fx.navIconRemoved', { what: page(String(e.what)) }));
        case 'groups': return line('changed', t('pgs.fx.navGroups', { list: (Array.isArray(e.groups) ? e.groups : []).map((g: any) => t('pgs.fx.navGroup', { name: q((lang() === 'ar' ? g.ar || g.en : g.en || g.ar) || g.id), items: strs(g.items).map(page).join(t('ldb.listSep')) })).join('; ') }));
        case 'groupsRemoved': return line('changed', t('pgs.fx.navGroupsRemoved'));
        case 'start': return line('changed', t('pgs.fx.navStart', { what: page(String(e.what)) }));
        case 'startRemoved': return line('changed', t('pgs.fx.navStartRemoved'));
      }
      break;
    case 'todos':
      if (e.op === 'set') return line('changed', todoValue(String(e.key), e.value));
      if (e.op === 'unset') return line('changed', t('ldb.todos.unset', { what: t(`ldb.todos.what.${String(e.key)}`) }));
      break;
    case 'themes':
      if (['added', 'changed', 'removed'].includes(e.op)) return line(e.op as FxLine['tag'], t(`ldb.fx.theme.${e.op}`, { name: q(e.name || e.id) }));
      break;
    case 'style': {
      if (e.op === 'removed') return line('removed', t('ldb.fx.styleRemoved'));
      const kind = String(e.report?.kind ?? '');
      const flags = strs(e.report?.flags).filter((f) => ['text', 'overlay', 'dialogs', 'hides', 'remote'].includes(f));
      // (Worded so it reads true both in a review and for what a new built-in version changes.)
      const what = kind === 'unreadable' ? t('lda.css.unreadableTitle') : kind === 'removes' ? t('ldb.css.removes') : ['colours', 'layout'].includes(kind) ? t('lda.css.' + kind) : '';
      return line(e.op === 'added' ? 'added' : 'changed', t('ldb.fx.style', { what }), { detail: flags.map((f) => t('lda.css.flag.' + f)) });
    }
    case 'task':
      if (['added', 'changed', 'removed'].includes(e.op)) return line(e.op as FxLine['tag'], t(`ldb.fx.task.${e.op}`, { name: q(e.name) }));
      break;
    case 'pages': { const l = pagesFx(e, key); if (l) return l; break; }
    case 'guide': return line('changed', t('ldb.fx.guide'));
    case 'file':
      if (['added', 'changed', 'removed'].includes(e.op)) return line(e.op as FxLine['tag'], t(`ldb.fx.file.${e.op}`, { path: q(e.path) }));
      break;
  }
  return line('changed', t('ldb.fx.unknown'));
}

/** The order the review groups them in: what can act on its own first, how things look last. */
const ORDER = ['addon', 'page', 'task', 'home', 'nav', 'todos', 'pages', 'themes', 'style', 'guide', 'file'];

/** Every entry in words, grouped by area: `[{area, lines}]`. */
export function fxGroups(effects: unknown): { area: string; lines: FxLine[] }[] {
  const all = (Array.isArray(effects) ? effects : []).filter((e): e is Effect => !!e && typeof e === 'object');
  const areas = [...ORDER, ...new Set(all.map((e) => e.area).filter((a) => !ORDER.includes(a)))];
  return areas.map((area) => ({ area, lines: all.map((e, i) => (e.area === area ? fxLine(e, i) : null)).filter((x): x is FxLine => !!x) })).filter((g) => g.lines.length);
}
