// The sidebar as nav.json arranges it (lib/navLayout.ts).
import { describe, it, expect } from 'vitest';
import { layoutNav, moved } from './navLayout';

const items = ['home', 'chats', 'ideas', 'todos', 'graph', 'customise'].map((id) => ({ id, icon: id }));
const ids = (l: { id: string }[]) => l.map((x) => x.id);

describe('layoutNav', () => {
  it('leaves the sidebar alone without nav.json', () => {
    expect(ids(layoutNav(items, null).flat)).toEqual(ids(items));
  });
  it('hides, orders, re-icons and groups', () => {
    const r = layoutNav([...items, { id: 'hours.js:board', alias: 'board', icon: 'plugin' }], {
      hidden: ['graph', 'home', 'customise'], order: ['todos', 'board'], labels: {},
      icons: { todos: 'calendar', board: 'list', customise: 'eye' },
      groups: [{ id: 'work', label: { en: 'Work' }, items: ['ideas', 'board'] }],
    });
    // Home and Customise never hide; Customise keeps its icon.
    expect(ids(r.flat)).toEqual(['todos', 'home', 'chats', 'customise', 'hours.js:board', 'ideas']);
    expect(r.flat.find((x) => x.id === 'todos')?.icon).toBe('calendar');
    expect(r.flat.find((x) => x.id === 'hours.js:board')?.icon).toBe('list');
    expect(r.flat.find((x) => x.id === 'customise')?.icon).toBe('customise');
    expect(r.groups.map((g) => g.id)).toEqual([null, 'work']);
    expect(ids(r.groups[1].items)).toEqual(['hours.js:board', 'ideas']);
  });
  it('ignores an icon the reference does not have', () => {
    expect(layoutNav(items, { hidden: [], order: [], labels: {}, icons: { todos: 'shield' } }).flat.find((x) => x.id === 'todos')?.icon).toBe('todos');
  });
  it('moves an entry (Edit sidebar)', () => {
    expect(moved([1, 2, 3], 0, 2)).toEqual([2, 3, 1]);
    expect(moved([1, 2, 3], 0, 5)).toEqual([1, 2, 3]);
  });
});
