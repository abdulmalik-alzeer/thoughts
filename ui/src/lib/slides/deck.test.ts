// Slide decks written as markdown notes: what counts as a deck, where slides split, speaker notes,
// the direction, and the limits.
import { describe, it, expect } from 'vitest';
import { isDeck, parseDeck, firstDir, slideTitle, MAX_SLIDES, MAX_BYTES } from './deck';

const deck = (body: string, fm = 'slides: true') => `---\n${fm}\n---\n${body}`;

describe('decks', () => {
  it('is a deck only with slides: true in the frontmatter', () => {
    expect(isDeck(deck('# A'))).toBe(true);
    expect(isDeck(deck('# A', 'slides: "true"  # yes'))).toBe(true);
    expect(isDeck(deck('# A', 'slides: false'))).toBe(false);
    expect(isDeck('# A\n\n---\n\n# B')).toBe(false);
    expect(isDeck('slides: true\n# not frontmatter')).toBe(false);
  });

  it('splits on a line of exactly --- between blank lines, never inside code', () => {
    const d = parseDeck(deck('# One\n\ntext\n\n---\n\n## Two\n\n```md\na\n\n---\n\nb\n```\n\n---\n\n## Three\ntext\n---\nstill three\n\n ---\n\nstill three'));
    expect(d.slides.map((s) => slideTitle(s.body))).toEqual(['One', 'Two', 'Three']);
    expect(d.slides[1].body).toContain('---');
    expect(d.slides[2].body).toContain('still three');
    expect(d.title).toBe('One');
  });

  it('takes speaker notes after Note: or ملاحظة:', () => {
    const d = parseDeck(deck('# A\n\nshown\n\nNote: say this\nand this\n\n---\n\n## ب\n\nملاحظة: قل هذا\n\n---\n\n## C\n\n```\nNote: code, not notes\n```'));
    expect(d.slides[0]).toEqual({ body: '# A\n\nshown', notes: 'say this\nand this' });
    expect(d.slides[1].notes).toBe('قل هذا');
    expect(d.slides[2].notes).toBe('');
  });

  it('finds the direction from the first strong letter unless dir says', () => {
    expect(parseDeck(deck('# مراجعة الربع\n\nQ3')).dir).toBe('rtl');
    expect(parseDeck(deck('# Review `مرحبا`')).dir).toBe('ltr');
    expect(parseDeck(deck('# Review', 'slides: true\ndir: rtl')).dir).toBe('rtl');
    expect(firstDir('123 — ![](/x.png) **عربي** x')).toBe('rtl');
  });

  it('keeps only known options', () => {
    const d = parseDeck(deck('# A', 'slides: true\ntransition: zoom\ntheme: dark\ntitle: Talk'));
    expect(d).toMatchObject({ transition: 'slide', theme: 'dark', title: 'Talk' });
    expect(parseDeck(deck('# A', 'slides: true\ntransition: fade')).transition).toBe('fade');
  });

  it('reads a chat block without frontmatter', () => {
    const d = parseDeck('# A\n\n---\n\n# B');
    expect(d.slides).toHaveLength(2);
  });

  it('stops at 300 slides and 200 KB', () => {
    const many = Array.from({ length: MAX_SLIDES + 20 }, (_, i) => `# ${i}`).join('\n\n---\n\n');
    const d = parseDeck(deck(many));
    expect(d.slides).toHaveLength(MAX_SLIDES);
    expect(d.cut).toBe(true);
    const big = parseDeck(deck('# A\n\n' + 'ب'.repeat(MAX_BYTES)));
    expect(new TextEncoder().encode(big.slides.map((s) => s.body).join('')).length).toBeLessThanOrEqual(MAX_BYTES);
    expect(big.cut).toBe(true);
  });
});
