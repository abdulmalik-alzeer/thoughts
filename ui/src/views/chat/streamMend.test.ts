import { describe, it, expect } from 'vitest';
import { marked } from 'marked';
import { mendTail } from './streamMend';
import { StreamLexer } from './streamBlocks';

const p = (s: string) => mendTail(s, 'paragraph');

describe('mendTail: what is still open is shown finished', () => {
  it.each<[string, string]>([
    ['Some **bold', 'Some **bold**'],
    ['Some **bold ', 'Some **bold** '],
    ['an *idea', 'an *idea*'],
    ['**strong and *both', '**strong and *both***'],
    ['~~gone', '~~gone~~'],
    ['run `npm ci', 'run `npm ci`'],
    ['run ``a ` b', 'run ``a ` b``'],
    ['closed **bold** and *it*', 'closed **bold** and *it*'],
    ['snake_case_name here', 'snake_case_name here'],
    ['2 * 3 = 6', '2 * 3 = 6'],
    ['\\*not emphasis', '\\*not emphasis'],
    ['`**` in code', '`**` in code'],
    ['تجربة **عريض', 'تجربة **عريض**'],
  ])('%j', (src, want) => expect(p(src)).toBe(want));

  it('an opener with nothing after it yet is left out, not shown as a literal', () => {
    expect(p('Some **')).toBe('Some ');
    expect(p('Some `')).toBe('Some ');
    expect(p('a **b** and *')).toBe('a **b** and ');
  });

  it('half a link shows its text; half an image nothing yet', () => {
    expect(p('see [the docs](https://exa')).toBe('see the docs');
    expect(p('see [the do')).toBe('see the do');
    expect(p('a ![pic](http://x')).toBe('a ');
    expect(p('see [done](x.md) ok')).toBe('see [done](x.md) ok');
    expect(p('code `a[0')).toBe('code `a[0`');
  });

  it('a last line that may still turn into a heading, a rule or a list item waits', () => {
    expect(p('Text\n-')).toBe('Text\n');
    expect(p('Text\n==')).toBe('Text\n');
    expect(mendTail('- one\n- two\n-', 'list')).toBe('- one\n- two\n');
    expect(mendTail('1. one\n2', 'list')).toBe('1. one\n');
    expect(p('#')).toBe('');
  });

  it('a table waits for the row under its header', () => {
    expect(p('| a | b |')).toBe('');
    expect(p('Intro\n| a | b |\n|--')).toBe('Intro\n');
    const done = '| a | b |\n|---|---|\n| 1 | 2';
    expect(marked.lexer(done)[0].type).toBe('table');
    expect(mendTail(done, 'table')).toBe(done);
  });

  it('code and raw HTML are shown as they are', () => {
    expect(mendTail('```js\nconst a = **b', 'code')).toBe('```js\nconst a = **b');
    expect(mendTail('<div>**', 'html')).toBe('<div>**');
  });
});

describe('StreamLexer: only the end is read again', () => {
  const full = '# Title\n\nFirst paragraph with [a link][1].\n\n- one\n- two\n\n```ts\nconst x = 1;\n```\n\nLast para\n\n[1]: https://example.com\n';

  it('gives the same blocks as reading the whole text, piece by piece', () => {
    const lx = new StreamLexer();
    for (let n = 1; n <= full.length; n += 3) {
      const text = full.slice(0, n);
      const got = lx.read(text).tokens.map((x) => [x.type, x.raw]);
      expect(got).toEqual((marked.lexer(text) as any[]).map((x) => [x.type, x.raw]));
    }
    const { tokens, links } = lx.read(full);
    expect(tokens.map((x) => x.type)).toEqual((marked.lexer(full) as any[]).map((x) => x.type));
    expect(links['1']?.href).toBe('https://example.com');
  });

  it('agrees with a full read on trickier answers, one character at a time', () => {
    const samples = [
      'Intro\n\n- a\n\n- b\n\n  more of b\n\n- c\n\nTitle\n=====\n\nPara\n---\n\n> quote\ncontinued\n\n1. x\n2. y\n\n    indented code\n\n| h | i |\n|---|---|\n| 1 | 2 |\n\nEnd',
      'نص عربي **عريض**\n\n- بند\n- بند ثان\n\n```\nfence without lang\n```\n\n<details>\n<summary>s</summary>\n\nbody\n\n</details>\n\n[r]: http://x\n\nafter [y][r]',
    ];
    for (const src of samples) {
      const lx = new StreamLexer();
      for (let n = 1; n <= src.length; n++) {
        const text = src.slice(0, n);
        expect(lx.read(text).tokens.map((x) => [x.type, x.raw])).toEqual((marked.lexer(text) as any[]).map((x) => [x.type, x.raw]));
      }
    }
  });

  it('keeps finished blocks and starts again when the text is not a continuation', () => {
    const lx = new StreamLexer();
    lx.read(full);
    expect((lx as any).stableText.length).toBeGreaterThan(0);
    const { tokens } = lx.read('Other answer');
    expect(tokens.map((x) => x.raw)).toEqual(['Other answer']);
  });

  it('a definition in the kept part still counts', () => {
    const lx = new StreamLexer();
    const src = '[1]: https://a.example\n\nOne\n\nTwo\n\nThree\n\nFour [x][1]';
    for (let n = 1; n <= src.length; n++) lx.read(src.slice(0, n));
    expect(lx.read(src).links['1']?.href).toBe('https://a.example');
  });
});
