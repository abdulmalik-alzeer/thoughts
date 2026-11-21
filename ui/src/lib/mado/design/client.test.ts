// The design step's addresses. `api()` puts `/api` in front itself: a path that already starts with
// it goes nowhere (0.3.0–0.3.2 said "The design could not be loaded" for every project).
import { describe, it, expect, vi } from 'vitest';

const calls: string[] = [];
vi.mock('../../api', () => ({
  api: async (p: string) => { calls.push(p); return null; },
  post: async (p: string) => { calls.push(p); return null; },
  put: async (p: string) => { calls.push(p); return null; },
}));

import { client } from './client';

describe('the design step talks to the service', () => {
  it('at /mado/<project>/design/…, without a second /api', async () => {
    const c = client();
    await c.styles('p 1'); await c.pick('p 1', 'x'); await c.tokens('p 1'); await c.saveTokens('p 1', {} as any); await c.screens('p 1'); await c.screen('p 1', 'a/b');
    expect(calls).toEqual(['/mado/p%201/design/styles', '/mado/p%201/design/style', '/mado/p%201/design/tokens', '/mado/p%201/design/tokens', '/mado/p%201/design/screens', '/mado/p%201/design/screens/a%2Fb']);
  });
});
