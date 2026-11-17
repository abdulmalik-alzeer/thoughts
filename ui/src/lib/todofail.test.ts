// A failed todo change (todofail.ts): only the "changed meanwhile" refusal counts as handled, so an
// edit that failed for any other reason opens its box again with the typed text.
import { describe, it, expect, vi } from 'vitest';

vi.mock('./i18n', () => ({ t: (k: string) => k }));
const toast = vi.fn();
const failed = vi.fn();
vi.mock('./store.svelte', () => ({ toast }));
vi.mock('./errors', () => ({ failed, errCode: (e: any) => e?.code ?? '' }));

const { todoFailed } = await import('./todofail');

describe('todoFailed', () => {
  it('a stale refusal keeps the typed text in the toast and says so', () => {
    expect(todoFailed('h', { code: 'STALE' }, 'my text')).toBe(true);
    expect(toast).toHaveBeenCalledWith('ui-todos.staleKept', expect.objectContaining({ detail: 'my text' }));
  });
  it('any other failure is reported and left to the caller to keep the text', () => {
    expect(todoFailed('h', { code: '' }, 'my text')).toBe(false);
    expect(failed).toHaveBeenCalledWith('h', { code: '' });
  });
});
