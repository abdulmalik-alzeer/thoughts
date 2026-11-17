// Reference links in an answer being written (StreamText renders it block by block).
import { describe, it, expect, vi } from 'vitest';
import { marked } from 'marked';
import { linkDefs, type LinkMap } from './linkDefs';

vi.mock('../../lib/i18n', () => ({ t: (k: string) => k }));
const { md } = await import('../../lib/md');

/** Each block as StreamText renders it, joined. */
function streamed(src: string): HTMLDivElement {
  const tokens = marked.lexer(src) as unknown as { raw: string; type: string }[];
  const defs = linkDefs((tokens as { links?: LinkMap }).links);
  const d = document.createElement('div');
  d.innerHTML = tokens.filter((t) => t.raw.trim()).map((t) => md(['paragraph', 'list'].includes(t.type) && defs ? t.raw + '\n\n' + defs : t.raw)).join('');
  return d;
}

describe('linkDefs', () => {
  it('none, nothing', () => {
    expect(linkDefs(undefined)).toBe('');
    expect(linkDefs({})).toBe('');
  });
  it('a definition further down makes the link work in an earlier block', () => {
    const d = streamed('See [the plan][1] and [notes].\n\n- also [the plan][1]\n\n[1]: https://a.example/p "Plan"\n[notes]: <https://b.example/a b>\n');
    const links = Array.from(d.querySelectorAll('a'));
    expect(links.map((a) => [a.textContent, a.getAttribute('href')])).toEqual([
      ['the plan', 'https://a.example/p'], ['notes', 'https://b.example/a%20b'], ['the plan', 'https://a.example/p'],
    ]);
    expect(links[0].getAttribute('title')).toBe('Plan');
    expect(d.textContent).not.toContain('[1]');
  });
  it('labels, addresses and titles with brackets and quotes survive', () => {
    const defs = linkDefs({ 'a\\]b': { href: 'https://x.example/<y> z', title: 'say "hi"' } });
    const d = document.createElement('div');
    d.innerHTML = md('[t][a\\]b]\n\n' + defs);
    const a = d.querySelector('a');
    expect([a?.getAttribute('href'), a?.getAttribute('title')]).toEqual(['https://x.example/%3Cy%3E%20z', 'say "hi"']);
  });
});
