import { describe, expect, it } from 'vitest';
import { highlight, lines, type Tok } from './highlight';

const join = (toks: Tok[]) => toks.map((x) => x.t).join('');
const cls = (toks: Tok[], text: string) => toks.find((x) => x.t.includes(text))?.c;

describe('highlight', () => {
  it('never loses or changes text', () => {
    const samples: [string, string][] = [
      ['const a = "x\\"y"; // done\n/* multi\nline */ let b = 0x1f;', 'js'],
      ['def f(x):\n    """doc\n    string"""\n    return None  # c', 'py'],
      ['<?xml version="1.0"?>\n<!DOCTYPE x>\n<a href="b" c=d>t &amp; <![CDATA[<x>]]><!-- c --></a>\n< not a tag', 'xml'],
      ['{"a": [1, 2.5e3, true, null], "b": "c"}', 'json'],
      ['key: value # c\n- item: 1\nanchor: &x\n---\n', 'yaml'],
      ['[section]\nname = "v" ; c\n', 'ini'],
      ['+added\n-removed\n@@ -1 +1 @@\n', 'diff'],
      ["fn x<'a>(s: &'a str) -> char { 'c' }", 'rust'],
      ['SELECT * FROM t -- c\nWHERE a = \'x\'', 'sql'],
      ['', 'js'], ['unterminated "string\nnext', 'js'], ['<a', 'html'], ['plain text', ''], ['x', 'nope'],
    ];
    for (const [src, lang] of samples) expect(join(highlight(src, lang)), lang).toBe(src);
  });
  it('classes the usual parts', () => {
    const js = highlight('const a = "s"; // c\nfoo(1)', 'js');
    expect(cls(js, 'const')).toBe('kw');
    expect(cls(js, '"s"')).toBe('str');
    expect(cls(js, '// c')).toBe('com');
    expect(cls(js, 'foo')).toBe('fn');
    expect(cls(js, '1')).toBe('num');
    const j = highlight('{"k": "v", "n": false}', 'json');
    expect(cls(j, '"k"')).toBe('key');
    expect(cls(j, '"v"')).toBe('str');
    expect(cls(j, 'false')).toBe('lit');
    const x = highlight('<a href="u">t</a>', 'html');
    expect(cls(x, '<a')).toBe('tag');
    expect(cls(x, 'href')).toBe('attr');
    expect(cls(x, '"u"')).toBe('str');
    expect(x.find((k) => k.t === 't')?.c).toBe(''); // the text between tags is plain
    const y = highlight('name: Sara\n  list: 1', 'yaml');
    expect(cls(y, 'name')).toBe('key');
    expect(cls(y, 'list')).toBe('key');
    const sql = highlight('select x from t', 'sql');
    expect(cls(sql, 'select')).toBe('kw');
    const ini = highlight('[s]\nk = 1', 'ini');
    expect(cls(ini, '[s]')).toBe('tag');
    expect(cls(ini, 'k')).toBe('key');
  });
  it('splits into lines, cutting tokens that span lines', () => {
    const l = lines(highlight('a /* b\nc */ d\n', 'js'));
    expect(l.length).toBe(3);
    expect(l[0].map((x) => x.t).join('')).toBe('a /* b');
    expect(l[1][0]).toEqual({ t: 'c */', c: 'com' });
    expect(l[2]).toEqual([]);
  });
});
