// Customise › Plugins: how the service's list is read and worded (lib/plugins.ts), in both
// languages, and the name of the file a plugin is saved as.
import { describe, it, expect, vi } from 'vitest';

const app = { settings: { language: 'en' } as { language: string }, preview: false, pending: null as unknown };
vi.mock('./store.svelte', () => ({ app }));
vi.mock('./api', () => ({ api: vi.fn(), post: vi.fn() }));
vi.mock('./layer', () => ({ checkPending: vi.fn(), loadLayer: vi.fn() }));
vi.mock('./packs', () => ({ fingerprintCode: (s: string) => s.slice(0, 4).toUpperCase() }));

const { asPlugin, exportFileName, importLines, kindIcon, kindLabel, originText, partText } = await import('./plugins');
const { icons } = await import('./Icon.svelte');
const clean = (v: unknown, max: number) => String(v ?? '').slice(0, max);
const ar = <T>(fn: () => T): T => { app.settings.language = 'ar'; try { return fn(); } finally { app.settings.language = 'en'; } };

const KINDS = ['bundle', 'addon', 'page', 'task', 'theme', 'home', 'sidebar', 'todos', 'pages', 'look', 'keys'];

describe('the list of plugins', () => {
  it('every kind has a label in both languages and an icon the app has', () => {
    for (const kind of [...KINDS, 'css']) {
      expect(kindLabel(kind), kind).not.toMatch(/^plg\./);
      expect(ar(() => kindLabel(kind)), kind).toMatch(/[؀-ۿ]/);
      expect(icons[kindIcon(kind)], kind).toBeTruthy();
    }
    expect(kindLabel('bundle')).toBe('Plugin');
    expect(ar(() => kindLabel('bundle'))).toBe('ملحق');
    expect(kindLabel('something new')).toBe('Plugin');
    expect(icons[kindIcon('something new')]).toBeTruthy();
  });

  it('a part reads as its kind and its name', () => {
    expect(partText({ kind: 'addon', label: 'hours' })).toBe('Add-on: hours');
    expect(partText({ kind: 'home', label: 'Board' })).toBe('Home: Board');
    expect(partText({ kind: 'css', label: '' })).toBe('Its look');
    expect(ar(() => partText({ kind: 'task', label: 'Weekly' }))).toBe('مهمة تلقائية: Weekly');
  });

  it('a row from the service is read as it is, and anything out of shape is left out', () => {
    const p = asPlugin({ id: 'p-abcdefghijkl', kind: 'bundle', named: true, name: 'Kanban', version: '1.2.0', author: 'Sara', origin: 'pack', on: true, state: 'ok', minApp: '0.5.0',
      contains: [{ kind: 'addon', label: 'board', path: 'plugins/board.js' }, null, { label: 'no kind' }], permissions: ['ideas:read', 7], permissionWords: ['read your ideas'],
      tasks: [{ file: 'weekly.json', name: 'Weekly', approved: true }, { name: 'no file' }], canSwitch: true, canRemove: true, canExport: true })!;
    expect(p.contains).toEqual([{ kind: 'addon', label: 'board', path: 'plugins/board.js' }]);
    expect(p.permissions).toEqual(['ideas:read']);
    expect(p.tasks).toEqual([{ file: 'weekly.json', name: 'Weekly', approved: true }]);
    expect(originText(p)).toBe('From a file');
    expect(ar(() => originText(p))).toBe('من ملف');
    // A loose one: no origin words, never "on" or switchable unless the service says so.
    const loose = asPlugin({ id: 'home', kind: 'home', name: 'Home arrangement', origin: 'system', state: 'odd' })!;
    expect([loose.origin, originText(loose), loose.on, loose.canSwitch, loose.canRemove, loose.state, loose.named]).toEqual(['loose', '', false, false, false, 'ok', false]);
    for (const bad of [null, {}, { id: 'x' }, { id: '', kind: 'home' }, { id: 'x', kind: 'virus' }, { id: 5, kind: 'home' }]) expect(asPlugin(bad)).toBeNull();
  });
});

describe('the file a plugin is saved as', () => {
  it('is named after the plugin, in whatever script, and ends in .thoughts-plugin', () => {
    expect(exportFileName('Kanban board')).toBe('Kanban-board.thoughts-plugin');
    expect(exportFileName('ساعات العمل')).toBe('ساعات-العمل.thoughts-plugin');
    expect(exportFileName('../../etc/passwd')).toBe('etc-passwd.thoughts-plugin');
    expect(exportFileName('a/b\\c:d*e?"<>|')).toBe('a-b-c-d-e.thoughts-plugin');
    expect(exportFileName('   ')).toBe('plugin.thoughts-plugin');
    expect(exportFileName('x'.repeat(200))).toBe('x'.repeat(60) + '.thoughts-plugin');
  });
});

describe('what the import dialog says about a plugin file', () => {
  const look = { id: 'p-abcdefghijkl', name: 'Kanban', version: '1.2', minApp: '0.5.0', layerFormat: 1, compatible: true, why: null, replaces: null, olderThanYours: false, migrates: false };

  it('new, an update, older than yours, made for an older format', () => {
    expect(importLines(look, clean)).toEqual({ lines: ['Plugin “Kanban”, version 1.2 (what the file says).', 'New: you do not have this plugin yet.'], refused: '' });
    expect(importLines({ ...look, replaces: { id: look.id, version: '1.0' } }, clean).lines[1]).toBe('Updates your plugin “Kanban” from 1.0 to 1.2. What the new version no longer has is taken out.');
    expect(importLines({ ...look, version: '0.9', replaces: { id: look.id, version: '1.0' }, olderThanYours: true }, clean).lines[1]).toMatch(/^Older than the one you have: you have 1\.0, this file is 0\.9\./);
    expect(importLines({ ...look, migrates: true }, clean).lines.at(-1)).toBe('Made for an older format: Thoughts will update it.');
    expect(ar(() => importLines({ ...look, replaces: { id: look.id, version: '1.0' } }, clean).lines.join(' '))).toMatch(/يحدّث ملحقك «Kanban» من 1\.0 إلى 1\.2/);
  });

  it('one this version cannot run is refused, with the version it needs', () => {
    const r = importLines({ ...look, compatible: false, why: 'newerApp', minApp: '0.6.0' }, clean);
    expect(r.lines).toEqual([]);
    expect(r.refused).toBe('This plugin needs Thoughts 0.6.0 or newer, so it was not added. Update Thoughts first.');
    expect(ar(() => importLines({ ...look, compatible: false, why: 'newerApp', minApp: '0.6.0' }, clean).refused)).toMatch(/0\.6\.0 أو أحدث/);
    expect(importLines({ ...look, compatible: false, why: 'newerFormat' }, clean).refused).not.toBe('');
  });

  it('a pack that is no plugin adds no line; a file of everything names its plugins', () => {
    expect(importLines(null, clean)).toEqual({ lines: [], refused: '' });
    expect(importLines({ all: [] }, clean).lines).toEqual([]);
    expect(importLines({ all: [{ id: 'p-a', name: 'Kanban', version: '1', on: true }, { id: 'p-b', name: 'Hours', version: '', on: false }] }, clean).lines[0]).toContain('Kanban · Hours');
  });
});
