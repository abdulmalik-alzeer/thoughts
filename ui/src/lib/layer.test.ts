// The customisation review's test load (dryLoadStaged in layer.ts): a staged change that could not
// even be read must never come back as "tested, nothing failed".
import { describe, it, expect, vi, beforeEach } from 'vitest';

const api = vi.fn();
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), api: (...a: unknown[]) => api(...a) }));

// jsdom has no matchMedia; the store listens to the system's dark mode at load.
window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;

const { dryLoadStaged, DryLoadFailed } = await import('./layer');
const { app } = await import('./store.svelte');

describe('dryLoadStaged', () => {
  beforeEach(() => { api.mockReset(); app.safeMode = false; });

  it('throws, not an empty list, when the staged change cannot be fetched', async () => {
    api.mockRejectedValue(new Error('service restarting'));
    await expect(dryLoadStaged()).rejects.toBeInstanceOf(DryLoadFailed);
  });

  it('still answers with the items when the fetch works (safe mode: listed, not run)', async () => {
    app.safeMode = true;
    api.mockResolvedValue({ manifest: {}, plugins: [{ name: 'clock', code: 'export default {}' }], views: [] });
    const items = await dryLoadStaged();
    expect(items.map((x) => x.name)).toEqual(['clock']);
    expect(items[0].error).toBeFalsy();
  });
});
