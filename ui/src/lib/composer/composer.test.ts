// The message box: what each key does in its two sizes, and the formatting it gives typed markdown.
import { describe, it, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { parser, GFM } from '@lezer/markdown';
import { composerKey, sendKeys } from './keys';
import { decorate } from './mdLive';

const key = (k: string, o: Partial<KeyboardEventInit> & { keyCode?: number } = {}) => {
  const e = new KeyboardEvent('keydown', { key: k, ...o });
  if (o.keyCode) Object.defineProperty(e, 'keyCode', { value: o.keyCode });
  return e;
};

describe('composerKey', () => {
  it('compact: Enter sends, Shift+Enter is a new line, ⌘/Ctrl+Enter also sends', () => {
    expect(composerKey(key('Enter'), false, true)).toBe('send');
    expect(composerKey(key('Enter', { shiftKey: true }), false, true)).toBe('newline');
    expect(composerKey(key('Enter', { metaKey: true }), false, true)).toBe('send');
    expect(composerKey(key('Enter', { ctrlKey: true }), false, false)).toBe('send');
    expect(composerKey(key('Escape'), false, true)).toBeNull();
  });

  it('expanded: Enter is a new line; only ⌘ (Mac) or Ctrl (elsewhere) with Enter sends; Esc collapses', () => {
    expect(composerKey(key('Enter'), true, true)).toBe('newline');
    expect(composerKey(key('Enter', { shiftKey: true }), true, true)).toBe('newline');
    expect(composerKey(key('Enter', { metaKey: true }), true, true)).toBe('send');
    expect(composerKey(key('Enter', { ctrlKey: true }), true, true)).toBe('newline');
    expect(composerKey(key('Enter', { ctrlKey: true }), true, false)).toBe('send');
    expect(composerKey(key('Enter', { metaKey: true }), true, false)).toBe('newline');
    expect(composerKey(key('Escape'), true, true)).toBe('collapse');
    expect(composerKey(key('Escape', { shiftKey: true }), true, true)).toBeNull();
  });

  it('never while an input method is composing', () => {
    expect(composerKey(key('Enter', { isComposing: true }), false, true)).toBeNull();
    expect(composerKey(key('Enter', { keyCode: 229 }), false, true)).toBeNull();
    expect(composerKey(key('Enter', { metaKey: true, keyCode: 229 }), true, true)).toBeNull();
    expect(composerKey(key('Escape', { isComposing: true }), true, true)).toBeNull();
  });

  it('names the send keys per platform', () => {
    expect(sendKeys(true)).toBe('⌘↩');
    expect(sendKeys(false)).toBe('Ctrl+Enter');
  });

  it('has its own listed shortcut, taken by no other', async () => {
    window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;
    const { matchShortcut } = await import('../shortcuts');
    const { KEYS, conflicts, effective } = await import('../keys/registry');
    const s = KEYS.find((x) => x.id === 'chat.composer.expand')!;
    expect(s && !s.hidden).toBe(true);
    expect(conflicts(s.id, effective(s.id)[0])).toHaveLength(0);
    const e = new KeyboardEvent('keydown', { code: 'KeyE', key: 'E', shiftKey: true, metaKey: /Mac/.test(navigator.platform), ctrlKey: !/Mac/.test(navigator.platform) });
    expect(matchShortcut(e)).toBe('chat.composer.expand');
  });
});

describe('markdown as it is typed', () => {
  const md = parser.configure(GFM);
  /** Every stretch of text with the classes on it, and each line's classes. */
  function look(src: string) {
    const doc = EditorState.create({ doc: src }).doc;
    const set = decorate(doc, md.parse(src));
    const marks: [string, string][] = [];
    const lines: Record<number, { cls: string; dir: string }> = {};
    set.between(0, doc.length, (from, to, d) => {
      const spec = d.spec as { class?: string; attributes?: Record<string, string> };
      if (from === to || (d as any).point === false && spec.attributes) {
        lines[doc.lineAt(from).number] = { cls: spec.class ?? '', dir: spec.attributes?.dir ?? '' };
      } else marks.push([src.slice(from, to), spec.class ?? '']);
    });
    return { marks, lines };
  }
  const classesOf = (marks: [string, string][], text: string) => marks.filter(([t]) => t === text).map(([, c]) => c).join(' ');

  it('bold, italic, code, strikethrough and links take their look; marks stay, dimmed', () => {
    const { marks } = look('a **bold** *it* `code` ~~gone~~ [link](https://x.y)');
    expect(classesOf(marks, 'bold')).toContain('cmd-strong');
    expect(classesOf(marks, '**')).toMatch(/cmd-mark/);
    expect(classesOf(marks, 'it')).toContain('cmd-em');
    expect(classesOf(marks, 'code')).toContain('cmd-code');
    expect(classesOf(marks, 'gone')).toContain('cmd-strike');
    expect(classesOf(marks, 'link')).toContain('cmd-link');
  });

  it('headings, quotes and fenced code mark their lines; code lines run left to right', () => {
    const { lines } = look('# Title\n> quoted\n\n```js\nlet a\n```\nنص');
    expect(lines[1].cls).toContain('cmd-h1');
    expect(lines[2].cls).toContain('cmd-quote');
    expect(lines[4].cls).toContain('cmd-block-first');
    expect(lines[5].cls).toContain('cmd-block');
    expect(lines[5].dir).toBe('ltr');
    expect(lines[6].cls).toContain('cmd-block-last');
    expect(lines[7].dir).toBe('auto');
  });

  it('list marks are dimmed and the text is never changed', () => {
    const src = '- one\n1. two';
    const { marks } = look(src);
    expect(classesOf(marks, '-')).toContain('cmd-mark');
    expect(classesOf(marks, '1.')).toContain('cmd-mark');
  });
});
