import { describe, expect, it } from 'vitest';
import { buildModel, fit, hit, search, step, type GraphData } from './model';

const data: GraphData = {
  nodes: [
    { path: 'apps/one/README.md', title: 'One', summary: 'The first app', area: 'apps', status: 'active', kind: 'item' },
    { path: 'apps/one/notes/plan.md', title: '', summary: '', area: 'apps', status: '', kind: 'note' },
    { path: 'business/shop/README.md', title: 'متجر', summary: 'بيع أونلاين', area: 'business', status: 'seed', kind: 'item' },
    { path: 'loose.md', title: 'Loose', summary: '', area: '', status: '', kind: 'note' },
  ],
  edges: [
    { from: 'apps/one/README.md', to: 'apps/one/notes/plan.md', count: 2 },
    { from: 'apps/one/notes/plan.md', to: 'apps/one/README.md', count: 1 },
    { from: 'business/shop/README.md', to: 'apps/one/README.md', count: 1 },
    { from: 'gone.md', to: 'apps/one/README.md', count: 1 },
  ],
};
const label = (n: { title: string; path: string }) => n.title || n.path.split('/').pop()!.replace(/\.md$/, '');
const fold = (s: string) => s.toLowerCase().replace(/[أإآ]/g, 'ا');

describe('graph model', () => {
  const m = buildModel(data, label);
  it('joins nodes both ways and drops links to unknown notes', () => {
    expect(m.edges.length).toBe(3);
    expect(m.out[0]).toEqual([1]);
    expect(m.inn[0].sort()).toEqual([1, 2]);
    expect(m.nodes[0].deg).toBe(2);
    expect(m.nodes[1].label).toBe('plan');
  });
  it('orders areas by size, the top level last', () => {
    expect(m.groups.map((g) => g.area)).toEqual(['apps', 'business', '']);
    expect(m.nodes[3].group).toBe(2);
  });
  it('searches titles, summaries and paths in either language', () => {
    expect(search(m, 'first', fold)).toEqual([0]);
    expect(search(m, 'اونلاين', fold)).toEqual([2]);
    expect(search(m, 'plan', fold)).toEqual([1]);
    expect(search(m, '  ', fold)).toEqual([]);
    expect(search(m, 'one', fold, (i) => i !== 0)).toEqual([1]);
  });
  it('fits, finds the note under the pointer and steps between notes', () => {
    const xs = [0, 100, -100, 0], ys = [0, 0, 0, 100];
    const cam = fit(xs, ys, [0, 1, 2, 3], 400, 300, 50);
    expect(cam.k).toBeGreaterThan(0);
    const all = () => true;
    expect(hit(m, xs, ys, cam, cam.x, cam.y, all)).toBe(0);
    expect(hit(m, xs, ys, cam, 5, 5, all)).toBe(-1);
    expect(step(m, xs, ys, 0, 'right', all)).toBe(1);
    expect(step(m, xs, ys, 0, 'left', all)).toBe(2);
    expect(step(m, xs, ys, 0, 'down', all)).toBe(3);
    expect(step(m, xs, ys, 0, 'up', all)).toBe(-1);
    expect(step(m, xs, ys, 0, 'right', (i) => i !== 1)).toBe(-1);
  });
});
