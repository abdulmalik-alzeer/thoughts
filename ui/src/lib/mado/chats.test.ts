import { describe, it, expect } from 'vitest';
import { isPlanChat, listedChats } from './chats';

describe('MADO chats stay out of the chat lists', () => {
  it('step and build chats are left out; every other kind stays', () => {
    const chats = [{ id: '1', kind: 'chat' }, { id: '2', kind: 'mado' }, { id: '3', kind: 'mado-build' }, { id: '4', kind: 'wrap' }, { id: '5', kind: 'customize' }];
    expect(listedChats(chats).map((c) => c.id)).toEqual(['1', '4', '5']);
    expect(isPlanChat({ kind: 'mado' }) && isPlanChat({ kind: 'mado-build' })).toBe(true);
    expect(isPlanChat({ kind: 'chat' }) || isPlanChat(null) || isPlanChat({})).toBe(false);
  });
});
