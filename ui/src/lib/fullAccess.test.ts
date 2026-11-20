// Full access in the window (lib/fullAccess.ts, lib/layer/machine.ts): switching it on always goes
// through the desktop app's own approval, off is one request, and the machine bridge only forwards.
import { describe, it, expect, vi, beforeEach } from 'vitest';

const put = vi.fn();
const post = vi.fn();
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), put: (...a: unknown[]) => put(...a), post: (...a: unknown[]) => post(...a) }));
const approval = vi.fn();
vi.mock('./consent', () => ({ approval: (...a: unknown[]) => approval(...a) }));
window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;

const { setFullAccess, fullAccessOn } = await import('./fullAccess');
const { machineHandle } = await import('./layer/machine');
const { app } = await import('./store.svelte');

describe('full access switch', () => {
  beforeEach(() => { put.mockReset(); approval.mockReset(); app.settings = { fullAccess: { on: false, since: null, sourceDir: null } }; });

  it('asks the desktop app first and sends its approval, never the body alone', async () => {
    approval.mockResolvedValue('123.abc.def');
    put.mockImplementation(async (_p: string, body: any) => ({ ...body, fullAccess: { on: true, since: 'now', sourceDir: null } }));
    expect(await setFullAccess(true)).toBe('on');
    expect(approval).toHaveBeenCalledWith('full-access');
    expect(put.mock.calls[0][0]).toBe('/settings?confirm=full-access&approval=123.abc.def');
    expect(fullAccessOn()).toBe(true);
  });

  it('a no in the dialog, or no desktop app, sends nothing', async () => {
    approval.mockResolvedValue(null);
    expect(await setFullAccess(true)).toBe('declined');
    approval.mockResolvedValue(undefined);
    expect(await setFullAccess(true)).toBe('unavailable');
    expect(put).not.toHaveBeenCalled();
  });

  it('off is one request and needs no approval', async () => {
    app.settings = { fullAccess: { on: true } };
    put.mockResolvedValue({ fullAccess: { on: false } });
    expect(await setFullAccess(false)).toBe('off');
    expect(approval).not.toHaveBeenCalled();
    expect(put.mock.calls[0][0]).toBe('/settings?fullAccess=off');
  });
});

describe('machine bridge host', () => {
  beforeEach(() => post.mockReset());
  it('forwards each call to the service as it is', async () => {
    post.mockResolvedValue({ code: 0, stdout: 'hi\n', stderr: '' });
    await machineHandle('machine.run', ['echo', ['hi', 2], { cwd: '/tmp', timeoutSecs: 5 }], () => {});
    expect(post).toHaveBeenCalledWith('/machine/run', expect.objectContaining({ program: 'echo', args: ['hi', '2'], cwd: '/tmp', timeoutSecs: 5 }));
    post.mockResolvedValue({ content: 'x' });
    expect(await machineHandle('machine.read', ['/etc/hosts'], () => {})).toBe('x');
    await expect(machineHandle('machine.format', [], () => {})).rejects.toThrow();
  });
});
