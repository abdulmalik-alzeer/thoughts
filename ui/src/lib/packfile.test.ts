// Plugin and pack files (lib/packfile.ts): the bytes pass through unchanged, the file's name and kind.
import { describe, expect, it, vi } from 'vitest';
vi.mock('./api', () => ({ inTauri: () => false }));
vi.mock('./i18n', () => ({ t: (k: string) => k }));
const { asPackFile, fromBase64, packFileName, PICK_ACCEPT, toBase64 } = await import('./packfile');

describe('plugin and pack files', () => {
  it('carries the bytes unchanged, also large ones', () => {
    const bytes = new Uint8Array(200_000).map((_, i) => (i * 31) % 256);
    expect(fromBase64(toBase64(bytes))).toEqual(bytes);
    expect(toBase64(new TextEncoder().encode('PK'))).toBe('UEs=');
  });

  it('is named after the plugin or pack, with the extension of its kind', () => {
    expect(packFileName('Kanban board', 'thoughts-plugin')).toBe('Kanban-board.thoughts-plugin');
    expect(packFileName('ملحقاتي', 'thoughts-pack')).toBe('ملحقاتي.thoughts-pack');
    expect(packFileName('../../etc/passwd', 'thoughts-pack')).toBe('etc-passwd.thoughts-pack');
    expect(packFileName('', 'thoughts-pack', 'thoughts-pack')).toBe('thoughts-pack.thoughts-pack');
  });

  it('takes only what the service made, and the picker accepts both kinds and legacy files', () => {
    expect(asPackFile({ extension: 'thoughts-plugin', name: 'Kanban', kind: 'feature', fingerprint: 'ab', file: 'UEs=' }).kind).toBe('feature');
    expect(() => asPackFile({ format: 'thoughts-pack', files: {} })).toThrow();
    expect(() => asPackFile({ extension: 'exe', file: 'UEs=' })).toThrow();
    for (const ext of ['.thoughts-plugin', '.thoughts-pack', '.json']) expect(PICK_ACCEPT.split(',')).toContain(ext);
  });
});
