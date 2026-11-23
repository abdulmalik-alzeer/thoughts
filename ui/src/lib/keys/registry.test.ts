// The keyboard shortcut registry: every key the app answers to is in it, the user's keys replace the
// defaults, keys show the way each system writes them, and no view answers a key behind its back.
import { describe, it, expect, beforeEach } from 'vitest';
import RESERVED from './reserved.json';

// jsdom has no matchMedia; the store (which the registry reads add-on commands from) listens to it at load.
window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;
const { KEYS, matchKey, label, conflicts, effective, setOverrides, refusal, parse, norm, fromEvent, recording, keys, allKeys, searchText, flatQuery } = await import('./registry');
const { keyState } = await import('./state.svelte');

const ev = (code: string, o: Partial<KeyboardEventInit> = {}, target?: EventTarget) => {
  const e = new KeyboardEvent('keydown', { code, key: code.replace(/^(Key|Digit)/, '').toLowerCase(), bubbles: true, ...o });
  if (target) Object.defineProperty(e, 'target', { value: target });
  return e;
};

beforeEach(() => { setOverrides(null); recording.on = false; });

describe('the table', () => {
  it('has every id once, and every binding parses', () => {
    const ids = KEYS.map((k) => k.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const k of KEYS) for (const b of [...k.defaults, ...(k.mac ?? []), ...(k.windows ?? []), ...(k.linux ?? [])]) expect(parse(b), `${k.id}: ${b}`).not.toBeNull();
  });

  it('lists every key the app answered to before 0.5.0, and the ones the inventory found', () => {
    const want = [
      'palette', 'chat.new', 'find', 'search.focus', 'nav.1', 'nav.2', 'nav.3', 'nav.4', 'nav.5', 'nav.6', 'wrap.start', 'sidebar.toggle', 'back', 'forward',
      'zoom.in', 'zoom.out', 'zoom.reset', 'chat.composer.expand', 'print', 'tour', 'help', 'history', 'rollback', 'sync',
      'settings', 'window.close', 'app.quit', 'window.fullscreen', 'capture.global',
      'docs.save', 'docs.editor.bold', 'docs.editor.italic', 'docs.editor.link', 'docs.editor.heading0', 'docs.editor.heading1', 'docs.editor.heading2', 'docs.editor.heading3',
      'docs.editor.list.number', 'docs.editor.list.bullet', 'docs.editor.list.task', 'find.next', 'find.prev',
      'graph.zoomIn', 'graph.zoomOut', 'graph.fit', 'graph.home',
      'mado-graph.undo', 'mado-graph.redo', 'mado-graph.rename', 'mado-graph.delete', 'mado-graph.comment', 'mado-graph.zoomIn', 'mado-graph.zoomOut', 'mado-graph.fit',
      'mado-fields.apply', 'mado-design.undo', 'mado-design.redo', 'image.zoomIn', 'image.zoomOut', 'image.fit', 'comments.send', 'slides.present',
      'escape', 'fixed.tab', 'fixed.arrows', 'fixed.message', 'fixed.edit', 'fixed.recovery', 'fixed.capture', 'fixed.slides',
      'theme.saudi', 'theme.contrast', 'theme.tech', 'mode.light', 'mode.dark', 'mode.system',
    ];
    const have = new Set(KEYS.map((k) => k.id));
    expect(want.filter((id) => !have.has(id))).toEqual([]);
  });

  it('gives every fixed key a reason, and no editable key a reserved default the service would refuse', () => {
    for (const k of KEYS.filter((k) => !k.editable)) expect(k.fixedWhy, k.id).toBeTruthy();
    for (const os of ['macos', 'windows', 'linux'] as const)
      for (const k of KEYS.filter((k) => k.editable && k.native !== 'global'))
        for (const b of effective(k.id, os)) {
          const why = refusal(k.id, b, os);
          // A default that needs no modifier is fine for a `plain` key; anything else must be allowed.
          expect(why === '' || (why === 'kb.no.mod' && k.plain), `${os} ${k.id} ${b}: ${why}`).toBe(true);
        }
  });
});

describe('matchKey', () => {
  it('reads Mod as ⌘ on a Mac and Ctrl elsewhere', () => {
    expect(matchKey(ev('KeyK', { metaKey: true }), 'app', 'macos')).toBe('palette');
    expect(matchKey(ev('KeyK', { ctrlKey: true }), 'app', 'macos')).toBeNull();
    expect(matchKey(ev('KeyK', { ctrlKey: true }), 'app', 'windows')).toBe('palette');
    expect(matchKey(ev('KeyK', { metaKey: true }), 'app', 'windows')).toBeNull();
    expect(matchKey(ev('KeyK', { metaKey: true, altKey: true }), 'app', 'macos')).toBeNull();
    expect(matchKey(ev('KeyW', { metaKey: true, shiftKey: true }), 'app', 'macos')).toBe('wrap.start');
  });

  it('runs a key with no modifier only outside text fields', () => {
    const box = document.createElement('input');
    document.body.append(box);
    expect(matchKey(ev('Slash'), 'app', 'macos')).toBe('search.focus');
    expect(matchKey(ev('Slash', {}, box), 'app', 'macos')).toBeNull();
    expect(matchKey(ev('KeyF', { metaKey: true, shiftKey: true }, box), 'app', 'macos')).toBe('search.focus');
    box.remove();
  });

  it('ignores a key while an input method composes, and while Settings records one', () => {
    expect(matchKey(ev('KeyK', { metaKey: true, isComposing: true }), 'app', 'macos')).toBeNull();
    recording.on = true;
    expect(matchKey(ev('KeyK', { metaKey: true }), 'app', 'macos')).toBeNull();
  });

  it('follows the physical key, so an Arabic layout works', () => {
    const e = new KeyboardEvent('keydown', { code: 'KeyN', key: 'ى', metaKey: true });
    expect(matchKey(e, 'app', 'macos')).toBe('chat.new');
  });

  it('lets the user replace defaults, and [] takes a key away', () => {
    setOverrides({ format: 1, bindings: { 'chat.new': ['Mod+KeyJ'], palette: [] } });
    expect(matchKey(ev('KeyJ', { metaKey: true }), 'app', 'macos')).toBe('chat.new');
    expect(matchKey(ev('KeyN', { metaKey: true }), 'app', 'macos')).toBeNull();
    expect(matchKey(ev('KeyK', { metaKey: true }), 'app', 'macos')).toBeNull();
    expect(effective('palette')).toEqual([]);
  });

  it('ignores keys for ids that are fixed or unknown, and bindings that do not parse', () => {
    setOverrides({ bindings: { escape: ['Mod+KeyE'], 'no.such': ['Mod+KeyU'], 'chat.new': ['Mod+Nope', 'Mod+KeyJ'] } });
    expect(effective('escape')).toEqual(['Escape']);
    expect(matchKey(ev('KeyU', { metaKey: true }), 'app', 'macos')).toBeNull();
    expect(effective('chat.new')).toEqual(['Mod+KeyJ']);
  });

  it('gives a palette command a key', () => {
    setOverrides({ bindings: { 'theme.contrast': ['Mod+Alt+KeyT'] } });
    expect(matchKey(ev('KeyT', { metaKey: true, altKey: true }), 'app', 'macos')).toBe('theme.contrast');
  });

  it('keeps local keys to their own part of the app', () => {
    expect(matchKey(ev('KeyS', { metaKey: true }), 'docs', 'macos')).toBe('docs.save');
    expect(matchKey(ev('KeyS', { metaKey: true }), 'app', 'macos')).toBeNull();
    expect(matchKey(ev('Equal'), 'graph', 'macos')).toBe('graph.zoomIn');
    expect(matchKey(ev('Equal', { shiftKey: true }), 'image', 'macos')).toBe('image.zoomIn');
    expect(matchKey(ev('KeyY', { ctrlKey: true }), 'mado-graph', 'macos')).toBe('mado-graph.redo');
    expect(matchKey(ev('KeyY', { ctrlKey: true }), 'mado-graph', 'windows')).toBe('mado-graph.redo');
    expect(matchKey(ev('Enter', { metaKey: true }), 'comments', 'macos')).toBe('comments.send');
    expect(matchKey(ev('F3', { shiftKey: true }), 'find', 'windows')).toBe('find.prev');
  });

  it('answers Esc as the fixed escape, and Settings ⌘, from the page as well as the menu', () => {
    expect(matchKey(ev('Escape'), 'app', 'macos')).toBe('escape');
    expect(matchKey(ev('Escape', { metaKey: true }), 'app', 'macos')).toBeNull();
    expect(matchKey(ev('Comma', { metaKey: true }), 'app', 'macos')).toBe('settings');
    expect(matchKey(ev('KeyW', { metaKey: true }), 'app', 'macos')).toBeNull();
  });
});

describe('showing keys', () => {
  it('writes them the way each system does, ⌃⌥⇧⌘ on a Mac', () => {
    expect(label('Mod+Control+Alt+Shift+KeyK', 'macos')).toBe('⌃⌥⇧⌘K');
    expect(label('Mod+Alt+Shift+KeyK', 'windows')).toBe('Ctrl+Alt+Shift+K');
    expect(label('Mod+Super+KeyK', 'windows')).toBe('Ctrl+Win+K');
    expect(label('Mod+NumpadAdd', 'macos')).toBe('⌘Num +');
    expect(label('Slash', 'macos')).toBe('/');
    expect(label('F11', 'linux')).toBe('F11');
  });

  it('isolates keys inside text', () => {
    expect(keys('chat.new')).not.toBe('');
    setOverrides({ bindings: { 'chat.new': [] } });
    expect(keys('chat.new')).toBe('');
  });

  it('finds keys by the words people search with', () => {
    for (const q of ['⌘K', 'cmd k', 'ctrl+k', 'Ctrl+K', 'command k', 'Mod+KeyK']) expect(searchText('Mod+KeyK'), q).toContain(flatQuery(q));
    expect(searchText('Mod+KeyK')).not.toContain(flatQuery('cmd n'));
  });
});

describe('checking a key', () => {
  it('refuses what the system or Recovery uses, and a bare key for a command that needs a modifier', () => {
    expect(refusal('chat.new', 'Mod+KeyC', 'macos')).toBe('kb.no.system');
    expect(refusal('chat.new', 'Mod+KeyH', 'macos')).toBe('kb.no.system');
    expect(refusal('chat.new', 'Mod+KeyH', 'windows')).toBe('');
    expect(refusal('chat.new', 'Super+KeyJ', 'windows')).toBe('kb.no.system');
    expect(refusal('chat.new', 'Mod+KeyR', 'linux')).toBe('kb.no.recovery');
    expect(refusal('chat.new', 'KeyJ', 'macos')).toBe('kb.no.mod');
    expect(refusal('chat.new', 'F5', 'macos')).toBe('');
    expect(refusal('graph.zoomIn', 'KeyJ', 'macos')).toBe('');
    expect(refusal('mado-graph.undo', 'Mod+KeyZ', 'macos')).toBe('');
    expect(refusal('chat.new', 'Mod+KeyZ', 'macos')).toBe('kb.no.system');
  });

  it('writes a key press the canonical way', () => {
    expect(fromEvent(ev('KeyK', { metaKey: true, ctrlKey: true, shiftKey: true }), 'macos')).toBe('Mod+Control+Shift+KeyK');
    expect(fromEvent(ev('KeyK', { metaKey: true, ctrlKey: true }), 'windows')).toBe('Mod+Super+KeyK');
    expect(fromEvent(new KeyboardEvent('keydown', { code: 'ShiftLeft', key: 'Shift', shiftKey: true }), 'macos')).toBeNull();
    expect(norm('Super+KeyK', 'macos')).toBe('Mod+KeyK');
    expect(norm('Control+KeyY', 'windows')).toBe('Mod+KeyY');
    expect(parse('Shift+Mod+KeyK')).toBeNull();
    expect(parse('Mod+Mod+KeyK')).toBeNull();
    expect(parse('Mod+KeyLL')).toBeNull();
  });

  it('warns only between parts that can be active together', () => {
    expect(conflicts('chat.new', 'Mod+KeyF', 'macos').map((c) => c.id)).toContain('find');
    // The docs editor and the graph are never open together; the app-wide keys touch everything.
    expect(conflicts('graph.zoomIn', 'Mod+KeyB', 'macos').map((c) => c.id)).toEqual([]);
    expect(conflicts('docs.editor.bold', 'Mod+KeyK', 'macos').map((c) => c.id)).toEqual(['palette', 'docs.editor.link']);
    expect(conflicts('image.zoomIn', 'Equal', 'macos')).toEqual([]);
  });

  it('uses the same reserved list as the service', () => {
    const rs = (import.meta.glob('../../../../crates/thoughtsd/src/keys.rs', { query: '?raw', import: 'default', eager: true }) as Record<string, string>)['../../../../crates/thoughtsd/src/keys.rs'];
    expect(rs).toContain('ui/src/lib/keys/reserved.json');
    expect(RESERVED.all).toContain('Escape');
  });

  it('knows add-on commands and the user’s themes', () => {
    expect(allKeys().length).toBeGreaterThanOrEqual(KEYS.length);
    expect(keyState.overrides).toEqual({});
  });
});

// ---------- no view answers a key behind the registry's back ----------
// Every literal key a handler compares against must be one that stays fixed (Esc, Tab, Enter,
// Space, arrows, Home/End, PageUp/Down, the modifier keys themselves), or be listed here with the
// reason. A new handler that checks, say, `e.key === 'k'` or `e.code === 'KeyS'` fails this test
// until its key goes into the registry and the handler asks `matchKey`.
const FIXED_VALUES = new Set(['Escape', 'Esc', 'Tab', 'Enter', ' ', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown',
  'Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'AltGraph']);
const ALLOWED: Record<string, { keys: string[]; why: string }> = {
  'capture/Capture.svelte': { keys: ['z'], why: 'the quick capture box (a separate window) keeps the system undo ⌘Z: fixed.capture' },
  'lib/keys/registry.ts': { keys: [], why: 'the registry itself' },
};
const KEYISH = /^(Key[A-Z]|Digit\d|F\d{1,2}|Numpad\w+|[a-zA-Z0-9]|[+=\-_/\\,.;'`[\]]|Backspace|Delete|Insert|Slash|Comma|Period|Minus|Equal|Backquote|Semicolon|Quote|Bracket\w+)$/;
const LITERAL = /\b(?:e|ev|event|k|key)(?:\.(?:key|code))?(?:\.toLowerCase\(\)|\.toUpperCase\(\))?\s*[!=]==?\s*'([^']*)'/g;

// Every source file of the window, as text (tests excluded), keyed by its path under ui/src.
const SOURCES = Object.fromEntries(Object.entries(import.meta.glob(['../../**/*.svelte', '../../**/*.ts', '!../../**/*.test.ts'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>)
  .map(([p, text]) => [p.replace(/^\.\.\/\.\.\//, ''), text]));

describe('every key handler goes through the registry', () => {
  it('reads the whole window', () => { expect(Object.keys(SOURCES).length).toBeGreaterThan(100); expect(SOURCES['App.svelte']).toContain("matchKey(e, 'app')"); });

  it('has no hard-coded key outside the fixed ones', () => {
    const bad: string[] = [];
    for (const [rel, text] of Object.entries(SOURCES)) {
      if (!/KeyboardEvent|onkeydown|keydown|onkeyup/.test(text)) continue;
      for (const m of text.matchAll(LITERAL)) {
        const v = m[1];
        if (FIXED_VALUES.has(v) || !KEYISH.test(v)) continue;
        if (ALLOWED[rel]?.keys.includes(v)) continue;
        bad.push(`${rel}: '${v}'`);
      }
    }
    expect(bad, 'add the key to lib/keys/registry.ts and ask matchKey(e, scope)').toEqual([]);
  });

  it('catches a handler that bypasses the registry', () => {
    const sample = "function onKey(e: KeyboardEvent) { if (e.key === 'k' && e.metaKey) go(); if (e.code === 'KeyS') save(); }";
    const found = [...sample.matchAll(LITERAL)].map((m) => m[1]).filter((v) => !FIXED_VALUES.has(v) && KEYISH.test(v));
    expect(found).toEqual(['k', 'KeyS']);
  });

  it('asks only for scopes the registry has', () => {
    const scopes = new Set<string>(KEYS.map((k) => k.scope));
    for (const [rel, text] of Object.entries(SOURCES)) {
      for (const m of text.matchAll(/matchKey\([^,()]+,\s*'([^']+)'/g)) expect(scopes.has(m[1]) || m[1] === 'app', `${rel} ${m[1]}`).toBe(true);
    }
  });
});
