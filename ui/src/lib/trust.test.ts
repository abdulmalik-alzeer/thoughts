// The trust question (trust.ts): a folder that can never be trusted as it is offers no yes.
import { describe, it, expect, vi } from 'vitest';

vi.mock('./i18n', () => ({ t: (k: string) => k }));
const confirmDialog = vi.fn(async () => true);
const choiceDialog = vi.fn(async () => null);
vi.mock('./dialog.svelte', () => ({ confirmDialog, choiceDialog }));

const { askTrust } = await import('./trust');
const info = (blocked: string | null) => ({ id: 'q1', path: '/x', known: false, changed: [], items: [], files: [], moreFiles: 0, blocked });

describe('askTrust', () => {
  it('a folder that cannot be trusted shows why, with no Trust button, and never answers yes', async () => {
    const answer = vi.fn(async () => null);
    expect(await askTrust(info('Cannot be trusted: reason.'), answer)).toBe('Cannot be trusted: reason.');
    expect(choiceDialog).toHaveBeenCalledWith(expect.objectContaining({ choices: [], cancelLabel: 'close' }));
    expect(confirmDialog).not.toHaveBeenCalled();
    expect(answer).not.toHaveBeenCalled();
  });
  it('an ordinary folder is asked, and the yes is sent', async () => {
    const answer = vi.fn(async () => null);
    expect(await askTrust(info(null), answer)).toBe(true);
    expect(confirmDialog).toHaveBeenCalledOnce();
    expect(answer).toHaveBeenCalledWith('q1');
  });
  it('a folder that became untrustable while the dialog was open stops there', async () => {
    const answer = vi.fn(async () => info('Now blocked.'));
    expect(await askTrust(info(null), answer)).toBe('Now blocked.');
  });
});
