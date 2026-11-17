// The side pane beside a chat: which clicks open it, where links point, how wide it may be.
import { describe, it, expect } from 'vitest';
import {
  CHAT_MIN, KEY_STEP, KEY_STEP_BIG, PANE_DEFAULT, PANE_MAX, PANE_MIN,
  clampWidth, clickIntent, dragWidth, isNote, isOverlay, keyWidth, linkTarget, loadWidth, looksLikeFolder, maxWidth, saveWidth,
} from './sidePane';

describe('clickIntent', () => {
  it('a plain click reads the file beside the chat', () => {
    expect(clickIntent({ button: 0 })).toBe('pane');
    expect(clickIntent({})).toBe('pane');
  });
  it('a click with Cmd, Ctrl or Shift, or the middle button, opens it in Docs', () => {
    expect(clickIntent({ button: 0, metaKey: true })).toBe('docs');
    expect(clickIntent({ button: 0, ctrlKey: true })).toBe('docs');
    expect(clickIntent({ button: 0, shiftKey: true })).toBe('docs');
    expect(clickIntent({ button: 1 })).toBe('docs');
  });
  it('the right button and Alt-click are left alone', () => {
    expect(clickIntent({ button: 2 })).toBeNull();
    expect(clickIntent({ button: 0, altKey: true })).toBeNull();
  });
});

describe('linkTarget', () => {
  const root = '/Users/me/Thoughts';
  it('a relative link starts at the top of the library', () => {
    expect(linkTarget('apps/x/README.md', root)).toBe('apps/x/README.md');
    expect(linkTarget('./apps/x/notes/a.md', root)).toBe('apps/x/notes/a.md');
    expect(linkTarget('/apps/x/README.md', root)).toBe('apps/x/README.md');
  });
  it('decodes each part, and drops anchors and queries', () => {
    expect(linkTarget('ملاحظات/%D8%AE%D8%B7%D8%A9.md', root)).toBe('ملاحظات/خطة.md');
    expect(linkTarget('my%20note.md#part', root)).toBe('my note.md');
    expect(linkTarget('a.md?x=1', root)).toBe('a.md');
  });
  it('a full path inside the library becomes relative to it', () => {
    expect(linkTarget('/Users/me/Thoughts/apps/x/README.md', root)).toBe('apps/x/README.md');
    expect(linkTarget('file:///Users/me/Thoughts/a%20b.md', root)).toBe('a b.md');
    expect(linkTarget('file://localhost/Users/me/Thoughts/a.md', root + '/')).toBe('a.md');
  });
  it('a file elsewhere on the computer is not a library file', () => {
    expect(linkTarget('file:///etc/hosts', root)).toBeNull();
    expect(linkTarget('file:///Users/me/Thoughts/a.md', '')).toBeNull();
  });
  it('web addresses, mail, anchors and empty links are not files', () => {
    for (const h of ['https://example.com/a.md', 'mailto:x@y.z', 'javascript:alert(1)', '#top', '', '   ', '//cdn.example.com/a.md', null, undefined]) expect(linkTarget(h as any, root)).toBeNull();
  });
  it('never climbs above the library', () => {
    expect(linkTarget('../secret.md', root)).toBeNull();
    expect(linkTarget('a/../../b.md', root)).toBeNull();
    expect(linkTarget('a/../b.md', root)).toBe('b.md');
    expect(linkTarget('/', root)).toBeNull();
    expect(linkTarget(root, root)).toBeNull();
  });
});

describe('width', () => {
  it('keeps room for the chat when docked', () => {
    expect(isOverlay(1400)).toBe(false);
    expect(maxWidth(1200)).toBe(Math.min(PANE_MAX, 1200 - CHAT_MIN));
    expect(clampWidth(2000, 1000)).toBe(1000 - CHAT_MIN);
    expect(clampWidth(10, 1400)).toBe(PANE_MIN);
    expect(clampWidth(500, 3000)).toBe(500);
    expect(clampWidth(5000, 3000)).toBe(PANE_MAX);
    expect(clampWidth(NaN, 3000)).toBe(PANE_DEFAULT);
  });
  it('lies over the chat when there is no room beside it, never wider than the space', () => {
    expect(isOverlay(PANE_MIN + CHAT_MIN - 1)).toBe(true);
    expect(isOverlay(PANE_MIN + CHAT_MIN)).toBe(false);
    expect(clampWidth(500, 400)).toBe(400);
    expect(clampWidth(500, 250)).toBe(250);
    expect(clampWidth(100, 600)).toBe(PANE_MIN);
  });
  it('dragging the handle towards the start makes the pane wider, in both directions of writing', () => {
    expect(dragWidth(400, 800, 760, false)).toBe(440);
    expect(dragWidth(400, 200, 240, true)).toBe(440);
    expect(dragWidth(400, 800, 840, false)).toBe(360);
  });
  it('arrow keys move the handle the way they point; Home and End go to the ends', () => {
    expect(keyWidth('ArrowLeft', 400, 2000, false)).toBe(400 + KEY_STEP);
    expect(keyWidth('ArrowRight', 400, 2000, false)).toBe(400 - KEY_STEP);
    expect(keyWidth('ArrowLeft', 400, 2000, true)).toBe(400 - KEY_STEP);
    expect(keyWidth('ArrowRight', 400, 2000, true, true)).toBe(400 + KEY_STEP_BIG);
    expect(keyWidth('Home', 400, 2000, false)).toBe(PANE_MIN);
    expect(keyWidth('End', 400, 2000, false)).toBe(PANE_MAX);
    expect(keyWidth('ArrowRight', PANE_MIN, 2000, false)).toBe(PANE_MIN);
    expect(keyWidth('Enter', 400, 2000, false)).toBeNull();
  });
  it('remembers the width, and survives storage that is missing or broken', () => {
    const m = new Map<string, string>();
    const s = { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
    expect(loadWidth(s)).toBe(PANE_DEFAULT);
    saveWidth(512.4, s);
    expect(loadWidth(s)).toBe(512);
    m.set('thoughts.chatPaneWidth', 'nonsense');
    expect(loadWidth(s)).toBe(PANE_DEFAULT);
    const broken = { getItem: () => { throw new Error('denied'); }, setItem: () => { throw new Error('full'); } };
    expect(loadWidth(broken)).toBe(PANE_DEFAULT);
    expect(() => saveWidth(400, broken)).not.toThrow();
    expect(loadWidth(null)).toBe(PANE_DEFAULT);
  });
});

describe('kinds', () => {
  it('tells notes, other files and folders apart', () => {
    expect(isNote('a/b.md')).toBe(true);
    expect(isNote('a/b.MD')).toBe(true);
    expect(isNote('a/b.pdf')).toBe(false);
    expect(looksLikeFolder('apps/my-idea')).toBe(true);
    expect(looksLikeFolder('apps/my-idea/README.md')).toBe(false);
  });
});
