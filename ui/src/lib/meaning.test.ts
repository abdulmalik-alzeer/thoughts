// Search by meaning in the window (lib/meaning.svelte.ts): the marker never shows while an answer
// streams, a suggestion is only ever declined from the card (never installed), and the state is
// read from the service.
import { describe, it, expect, vi, beforeEach } from 'vitest';

const post = vi.fn();
const api = vi.fn();
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), post: (...a: unknown[]) => post(...a), api: (...a: unknown[]) => api(...a) }));
window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;

const m = await import('./meaning.svelte');

const off = { state: 'off', model: { id: 'e5', name: 'E5', bytes: 488e6, license: 'MIT', languages: '' }, diskBytes: 0, suggest: null, quiet: false, offers: [{ chatId: 'c1', messageId: 'm9' }] };

describe('search by meaning', () => {
  beforeEach(() => { post.mockReset(); api.mockReset(); api.mockResolvedValue(off); });

  it('hides the marker line, and the start of one at the end of a streaming answer', () => {
    expect(m.hideMarker('Nothing found.\n[[thoughts:meaning-search]]')).toBe('Nothing found.');
    expect(m.hideMarker('Nothing found.\n[[thoughts:mean')).toBe('Nothing found.');
    expect(m.hideMarker('a [[link]] stays\n[[x]]')).toBe('a [[link]] stays\n[[x]]');
    expect(m.hideMarker('plain text')).toBe('plain text');
  });

  it('reads where it stands and the open suggestions', async () => {
    await m.refresh();
    expect(api).toHaveBeenCalledWith('/meaning');
    expect(m.meaning.offers).toEqual({ c1: 'm9' });
    expect(m.mayOffer()).toBe(true);
    expect(m.isReady()).toBe(false);
  });

  it('declining sends only the decline and hides every suggestion at once', async () => {
    await m.refresh();
    post.mockResolvedValue({ ok: true });
    await m.decline('later', 'c1');
    expect(post).toHaveBeenCalledTimes(1);
    expect(post).toHaveBeenCalledWith('/meaning/offer', { op: 'later', chatId: 'c1' });
    expect(post.mock.calls.some((c) => c[0] === '/meaning/install')).toBe(false);
  });

  it('a search asks the service with the words encoded', async () => {
    api.mockResolvedValue({ results: [{ path: 'a.md', title: 'A', score: 0.9, snippet: null, line: null }] });
    const r = await m.searchByMeaning('رسوم & fees');
    expect(api.mock.calls[0][0]).toBe('/meaning/search?q=' + encodeURIComponent('رسوم & fees') + '&limit=10');
    expect(r[0].path).toBe('a.md');
  });
});
