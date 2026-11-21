// The window's side of the screen check: no shell (or a second window) means nobody can look here
// (`null`); in the main window the shell command gets the address and plain screens, and its error
// codes come through as they are.
import { describe, it, expect, vi, afterEach } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: any[]) => invoke(...a) }));

const { screenCheck } = await import('./screencheck');

function inShell(label: string) {
  (window as any).__TAURI_INTERNALS__ = { metadata: { currentWindow: { label } } };
}
afterEach(() => { delete (window as any).__TAURI_INTERNALS__; invoke.mockReset(); });

describe('screenCheck', () => {
  it('is null in a browser, and in a second window', async () => {
    expect(await screenCheck('http://localhost:5173', [{ id: 'a', path: '/' }])).toBeNull();
    inShell('view-1');
    expect(await screenCheck('http://localhost:5173', [{ id: 'a', path: '/' }])).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it('asks the shell from the main window with plain screens', async () => {
    inShell('main');
    const result = [{ id: 'edit', path: '/customers/1/edit', ok: false, errors: ['x'], text: 'null', fields: [{ label: 'Name', value: '(empty)' }] }];
    invoke.mockResolvedValueOnce(result);
    const odd: any = { id: 'edit', path: '/customers/1/edit', extra: '<b>' };
    expect(await screenCheck('http://localhost:5173', [odd])).toEqual(result);
    expect(invoke).toHaveBeenCalledWith('mado_screen_check', { url: 'http://localhost:5173', screens: [{ id: 'edit', path: '/customers/1/edit' }] });
  });

  it('passes the shell\'s error codes on, and never answers with anything but a list', async () => {
    inShell('main');
    invoke.mockRejectedValueOnce('screen-check-busy');
    await expect(screenCheck('http://localhost:5173', [])).rejects.toThrow('screen-check-busy');
    invoke.mockResolvedValueOnce({ not: 'a list' });
    expect(await screenCheck('http://localhost:5173', [])).toEqual([]);
  });
});
