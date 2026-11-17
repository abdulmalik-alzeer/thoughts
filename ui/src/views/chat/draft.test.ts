// A message that could not be sent, back into a chat's saved draft (draft.ts).
import { describe, it, expect } from 'vitest';
import { joinDraft, type SavedDraft } from './draft';

const back: SavedDraft = { text: 'sent words', mentions: [{ path: 'a.md', label: 'A' }], files: [{ name: 'p.png', size: 3, image: true }] };

describe('joinDraft', () => {
  it.each([
    ['nothing saved', null],
    ['unreadable', '{oops'],
    ['not a draft', '"text"'],
  ])('%s: the message is the draft', (_n, stored) => {
    expect(joinDraft(back, stored, 100)).toEqual(back);
  });
  it('written since: the message comes first, nothing twice', () => {
    const stored = JSON.stringify({ text: 'new words', mentions: [{ path: 'a.md', label: 'A' }, { path: 'b.md', label: 'B' }], files: [{ name: 'q.txt', size: 1, image: false }] });
    expect(joinDraft(back, stored, 100)).toEqual({
      text: 'sent words\n\nnew words',
      mentions: [{ path: 'a.md', label: 'A' }, { path: 'b.md', label: 'B' }],
      files: [{ name: 'p.png', size: 3, image: true }, { name: 'q.txt', size: 1, image: false }],
    });
  });
  it('the draft stays within its size', () => {
    expect(joinDraft(back, JSON.stringify({ text: 'x'.repeat(50) }), 20).text.length).toBe(20);
  });
});
