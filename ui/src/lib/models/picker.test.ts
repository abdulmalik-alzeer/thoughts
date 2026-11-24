// The model picker's logic (round 0.7.2): what is listed, under which name, in which group, what a
// search finds, which row is current or the default, and what the keys do. The model names below
// are made up for the tests: the app itself has no list of models.
import { describe, it, expect, beforeEach } from 'vitest';
import { LONG_LIST, RECENT_MAX, entriesFrom, filterModels, moveBy, nameOf, placeList, recentModels, rememberModel, rowsOf, saysDefault, sectionsFor, splitId, startIndex, validName } from './picker';

const short = entriesFrom({ models: ['alpha', 'beta', 'gamma-2-0'] });
const makers = ['acme', 'bolt', 'crest', 'dune'];
const longIds = makers.flatMap((m) => Array.from({ length: 30 }, (_, i) => `${m}/model-${i + 1}`));
const long = entriesFrom({ models: longIds });

describe('what is listed', () => {
  it('takes the ids in their order, each once, and nothing that is not a name', () => {
    expect(entriesFrom({ models: ['b', 'a', 'b', '', 3, null, ' c '] }).map((e) => e.id)).toEqual(['b', 'a', 'c']);
    expect(entriesFrom(null)).toEqual([]);
    expect(entriesFrom({ models: 'nope' })).toEqual([]);
  });

  it('shows the name the program reported, and the bare id where it reported none', () => {
    const e = entriesFrom({ models: ['one', 'two', 'three'], modelNames: { one: 'Model One' }, modelInfo: [{ id: 'two', label: 'Model Two', description: 'Quick.', isDefault: true }] });
    expect(e.map((x) => x.name)).toEqual(['Model One', 'Model Two', 'three']);
    expect(e[1].note).toBe('Quick.');
    expect(e.map((x) => x.isDefault)).toEqual([false, true, false]);
  });

  it('marks the model the agent starts with as the default', () => {
    expect(entriesFrom({ models: ['one', 'two'], defaultModel: 'two' }).map((x) => x.isDefault)).toEqual([false, true]);
    expect(entriesFrom({ models: ['one', 'two'], defaultModel: 'gone' }).some((x) => x.isDefault)).toBe(false);
  });

  it('groups by maker only when the ids name at least two makers, and then drops the maker from the name', () => {
    expect(short.every((e) => e.group === '' && e.name === e.id)).toBe(true);
    const one = entriesFrom({ models: ['acme/a', 'acme/b', 'plain'] });
    expect(one.map((e) => [e.group, e.name])).toEqual([['', 'acme/a'], ['', 'acme/b'], ['', 'plain']]);
    const two = entriesFrom({ models: ['acme/a', 'bolt/a', 'plain'] });
    expect(two.map((e) => [e.group, e.name])).toEqual([['acme', 'a'], ['bolt', 'a'], ['', 'plain']]);
  });

  it('splits an id at its first slash only', () => {
    expect(splitId('acme/model/v2')).toEqual(['acme', 'model/v2']);
    expect(splitId('plain')).toEqual(['', 'plain']);
    expect(splitId('/odd')).toEqual(['', '/odd']);
    expect(splitId('odd/')).toEqual(['', 'odd/']);
  });

  it('names a choice for a button: the reported name, the full id inside a group, the raw value when unknown', () => {
    const e = entriesFrom({ models: ['acme/a', 'bolt/a', 'x'], modelNames: { x: 'Model X' } });
    expect(nameOf(e, 'x')).toBe('Model X');
    expect(nameOf(e, 'bolt/a')).toBe('bolt/a');
    expect(nameOf(e, 'typed-by-hand')).toBe('typed-by-hand');
    expect(nameOf(e, null)).toBe('');
  });

  it('knows when a reported name already says it is the default', () => {
    expect(saysDefault('Default (recommended)')).toBe(true);
    expect(saysDefault('Model One')).toBe(false);
  });
});

describe('the rows', () => {
  it('a short list: Automatic, then the models, with no headings and nothing twice', () => {
    const s = sectionsFor(short, { value: 'beta', auto: 'Automatic', typed: true });
    expect(s.map((x) => [x.kind, x.label])).toEqual([['top', ''], ['all', '']]);
    const rows = rowsOf(s);
    expect(rows.map((r) => r.id)).toEqual(['', 'alpha', 'beta', 'gamma-2-0']);
    expect(rows.filter((r) => r.current).map((r) => r.id)).toEqual(['beta']);
    expect(new Set(rows.map((r) => r.key)).size).toBe(rows.length);
  });

  it('Automatic is the current row while nothing is chosen, and is left out where it is not offered', () => {
    expect(rowsOf(sectionsFor(short, { value: null, auto: 'Automatic' })).filter((r) => r.current).map((r) => r.kind)).toEqual(['auto']);
    expect(rowsOf(sectionsFor(short, { value: '', auto: '' })).map((r) => r.kind)).toEqual(['model', 'model', 'model']);
  });

  it('a choice the list does not have is still shown, marked as the current one', () => {
    const rows = rowsOf(sectionsFor(short, { value: 'my-own-model', auto: 'Automatic' }));
    expect(rows[1]).toMatchObject({ kind: 'typed', id: 'my-own-model', current: true });
  });

  it('a long list: the current choice on top, the recently used, then every maker in the list’s order', () => {
    expect(long.length).toBeGreaterThanOrEqual(LONG_LIST);
    const s = sectionsFor(long, { value: 'crest/model-7', auto: 'Automatic', recents: ['dune/model-2', 'crest/model-7', 'gone/model', 'acme/model-1'] });
    expect(s.map((x) => x.kind)).toEqual(['top', 'recent', 'group', 'group', 'group', 'group']);
    expect(s.slice(2).map((x) => x.label)).toEqual(makers);
    expect(s[0].rows.map((r) => r.key)).toEqual(['auto', 'c:crest/model-7']);
    // The current one is not repeated under "Recently used"; one that is gone from the list is dropped.
    expect(s[1].rows.map((r) => r.id)).toEqual(['dune/model-2', 'acme/model-1']);
    const rows = rowsOf(s);
    expect(new Set(rows.map((r) => r.key)).size).toBe(rows.length);
    expect(rows.filter((r) => r.current).map((r) => r.key)).toEqual(['c:crest/model-7', 'm:crest/model-7']);
  });

  it('shows at most a few recent models, and none for a short list', () => {
    const many = longIds.slice(0, 8);
    expect(sectionsFor(long, { value: null, recents: many }).find((x) => x.kind === 'recent')!.rows.length).toBe(RECENT_MAX);
    expect(sectionsFor(short, { value: null, recents: ['alpha'] }).some((x) => x.kind === 'recent')).toBe(false);
  });

  it('names without a maker keep their place in the list, under their own heading', () => {
    const e = entriesFrom({ models: ['acme/a', 'bolt/b', 'plain-1', 'plain-2'] });
    expect(sectionsFor(e, { value: null }).map((x) => [x.kind, x.rows.length])).toEqual([['group', 1], ['group', 1], ['other', 2]]);
  });

  it('a long list without makers gets an "all" heading only when something is listed above it', () => {
    const flat = entriesFrom({ models: Array.from({ length: 12 }, (_, i) => `m-${i}`) });
    expect(sectionsFor(flat, { value: null, auto: 'Automatic' }).at(-1)).toMatchObject({ kind: 'all', label: '' });
    expect(sectionsFor(flat, { value: null, auto: 'Automatic', recents: ['m-3'] }).at(-1)).toMatchObject({ kind: 'all', label: 'all' });
  });
});

describe('searching', () => {
  it('finds by any part of the name or the id, in any letter case, every word typed', () => {
    expect(filterModels(long, 'BOLT model-12').map((e) => e.id)).toEqual(['bolt/model-12']);
    expect(filterModels(long, 'model-30').map((e) => e.id)).toEqual(makers.map((m) => `${m}/model-30`));
    expect(filterModels(long, 'nothing-like-it')).toEqual([]);
    expect(filterModels(short, '   ')).toBe(short);
  });

  it('finds by the reported name and by the note too', () => {
    const e = entriesFrom({ models: ['x-1', 'x-2'], modelInfo: [{ id: 'x-1', label: 'Swift', description: 'For long tasks' }] });
    expect(filterModels(e, 'swift').map((x) => x.id)).toEqual(['x-1']);
    expect(filterModels(e, 'long tasks').map((x) => x.id)).toEqual(['x-1']);
  });

  it('puts a name that starts with what was typed first, then a part that starts with it, then the rest', () => {
    const e = entriesFrom({ models: ['big-turbo', 'turbo-1', 'superturbo'] });
    expect(filterModels(e, 'turbo').map((x) => x.id)).toEqual(['turbo-1', 'big-turbo', 'superturbo']);
    // Inside a group the short name counts as the start.
    expect(filterModels(long, 'model-1')[0].id).toBe('acme/model-1');
  });

  it('lists the matches in one run and offers the typed name itself last, unless it is a model already', () => {
    const s = sectionsFor(long, { value: 'acme/model-1', query: 'model-29', auto: 'Automatic', typed: true, recents: ['acme/model-29'] });
    expect(s.length).toBe(1);
    expect(s[0].rows.map((r) => r.key)).toEqual([...makers.map((m) => `m:${m}/model-29`), 't:model-29']);
    expect(rowsOf(sectionsFor(long, { value: null, query: 'acme/model-2', typed: true })).some((r) => r.kind === 'typed')).toBe(false);
  });

  it('offers no typed name where typing one is not allowed or it could not be a name', () => {
    expect(rowsOf(sectionsFor(short, { value: null, query: 'zzz', typed: false }))).toEqual([]);
    expect(rowsOf(sectionsFor(short, { value: null, query: 'two words', typed: true }))).toEqual([]);
    expect(rowsOf(sectionsFor(short, { value: null, query: 'نموذج', typed: true }))).toEqual([]);
  });

  it('finds Automatic by its own (translated) name', () => {
    expect(rowsOf(sectionsFor(short, { value: null, query: 'auto', auto: 'Automatic' }))[0].kind).toBe('auto');
    expect(rowsOf(sectionsFor(short, { value: null, query: 'تلق', auto: 'تلقائي' }))[0].kind).toBe('auto');
  });

  it('accepts the names the service accepts, and nothing else', () => {
    for (const ok of ['opus', 'acme/model-1:free', 'a.b_c-d@e[1m]']) expect(validName(ok)).toBe(true);
    for (const bad of ['', ' x', 'two words', '-lead', 'x'.repeat(121), 'a;b', 'a"b']) expect(validName(bad)).toBe(false);
  });
});

describe('the keys', () => {
  it('starts on the current choice, or on the best match once something is typed', () => {
    expect(startIndex(rowsOf(sectionsFor(short, { value: 'gamma-2-0', auto: 'Automatic' })), '')).toBe(3);
    expect(startIndex(rowsOf(sectionsFor(short, { value: null, auto: 'Automatic' })), '')).toBe(0);
    expect(startIndex(rowsOf(sectionsFor(long, { value: 'dune/model-30', auto: 'Automatic' })), '')).toBe(1); // its copy on top
    expect(startIndex(rowsOf(sectionsFor(short, { value: 'gamma-2-0', query: 'a' })), 'a')).toBe(0);
    expect(startIndex([], '')).toBe(-1);
  });

  it('moves down and up and goes round at the ends', () => {
    expect(moveBy('ArrowDown', 0, 4)).toBe(1);
    expect(moveBy('ArrowDown', 3, 4)).toBe(0);
    expect(moveBy('ArrowUp', 0, 4)).toBe(3);
    expect(moveBy('ArrowUp', 2, 4)).toBe(1);
    expect(moveBy('ArrowDown', -1, 4)).toBe(0);
    expect(moveBy('ArrowUp', -1, 4)).toBe(3);
  });

  it('moves a page at a time without going round, and to either end', () => {
    expect(moveBy('PageDown', 0, 100)).toBe(8);
    expect(moveBy('PageDown', 97, 100)).toBe(99);
    expect(moveBy('PageUp', 3, 100)).toBe(0);
    expect(moveBy('Home', 50, 100)).toBe(0);
    expect(moveBy('End', 50, 100)).toBe(99);
  });

  it('uses no left or right key (the same in a right-to-left language) and answers nothing for other keys', () => {
    for (const k of ['ArrowLeft', 'ArrowRight', 'a', 'Enter', 'Escape', 'Tab']) expect(moveBy(k, 1, 4)).toBeNull();
    expect(moveBy('ArrowDown', 0, 0)).toBe(-1);
  });
});

describe('recently used', () => {
  beforeEach(() => localStorage.clear());

  it('keeps the newest first, each once, per assistant', () => {
    rememberModel('claude', 'a'); rememberModel('claude', 'b'); rememberModel('claude', 'a'); rememberModel('codex', 'z');
    expect(recentModels('claude')).toEqual(['a', 'b']);
    expect(recentModels('codex')).toEqual(['z']);
    expect(recentModels('other')).toEqual([]);
  });

  it('never keeps Automatic, a bad name or more than a handful', () => {
    rememberModel('claude', null); rememberModel('claude', ''); rememberModel('claude', 'two words'); rememberModel('', 'a');
    expect(recentModels('claude')).toEqual([]);
    for (let i = 0; i < 20; i++) rememberModel('claude', `m-${i}`);
    expect(recentModels('claude').length).toBe(8);
    expect(recentModels('claude')[0]).toBe('m-19');
  });

  it('survives storage that holds something else', () => {
    localStorage.setItem('thoughts.models.recent', 'not json');
    expect(recentModels('claude')).toEqual([]);
    localStorage.setItem('thoughts.models.recent', JSON.stringify({ claude: ['ok', 5, 'bad name'] }));
    expect(recentModels('claude')).toEqual(['ok']);
    localStorage.setItem('thoughts.models.recent', JSON.stringify([1, 2]));
    rememberModel('claude', 'x');
    expect(recentModels('claude')).toEqual(['x']);
  });
});

describe('where the list opens', () => {
  const win = { width: 1000, height: 700 };

  it('opens under the button, as wide as it, when there is room', () => {
    const b = placeList({ left: 100, right: 500, top: 100, bottom: 130 }, win);
    expect(b).toMatchObject({ above: false, left: 100, top: 134, bottom: null, width: 400, maxHeight: 420 });
  });

  it('opens over the button when the list does not fit under it and there is clearly more room above', () => {
    const b = placeList({ left: 100, right: 500, top: 600, bottom: 630 }, win);
    expect(b.above).toBe(true);
    expect(b.top).toBeNull();
    expect(b.bottom).toBe(700 - 600 + 4);
    expect(b.maxHeight).toBe(420);
  });

  it('never leaves the window: height cut to the room, sideways pushed back inside', () => {
    const tight = placeList({ left: 100, right: 500, top: 300, bottom: 330 }, { width: 1000, height: 560 });
    expect(tight.above).toBe(true);
    expect(tight.maxHeight).toBe(300 - 4 - 8);
    const right = placeList({ left: 900, right: 990, top: 100, bottom: 130 }, win, { minWidth: 320 });
    expect(right.left + right.width).toBeLessThanOrEqual(1000 - 8);
    const narrow = placeList({ left: 10, right: 100, top: 100, bottom: 130 }, { width: 260, height: 700 }, { minWidth: 320 });
    expect(narrow).toMatchObject({ left: 8, width: 244 });
  });

  it('lines up with the button’s right edge in a right-to-left language', () => {
    const b = placeList({ left: 600, right: 700, top: 100, bottom: 130 }, win, { rtl: true, minWidth: 320 });
    expect(b.left + b.width).toBe(700);
    const edge = placeList({ left: 10, right: 110, top: 100, bottom: 130 }, win, { rtl: true, minWidth: 320 });
    expect(edge.left).toBe(8);
  });
});
