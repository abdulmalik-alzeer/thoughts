import { describe, expect, it, vi } from 'vitest';
import { effortsFor, modelsNoteKey, versionNumber } from './codexModels';

const app = { settings: { language: 'en' } as { language: string } };
vi.mock('./store.svelte', () => ({ app }));
const { t } = await import('./i18n');

describe('Codex models on the screen', () => {
  it('names the installed version by its number', () => {
    expect(versionNumber('codex-cli 0.27.0')).toBe('0.27.0');
    expect(versionNumber(' 0.171.0\n')).toBe('0.171.0');
    expect(versionNumber(undefined)).toBe('');
  });

  it('says plainly when an old Codex cannot list its models', () => {
    // Codex 0.27.0 as installed: no catalog at all.
    const old = { modelsSource: 'none', modelsDetected: false, modelsTooOld: true, codexVersion: 'codex-cli 0.27.0' };
    expect(modelsNoteKey(old)).toBe('codex.models.tooOld');
    const text = t('codex.models.tooOld', { version: versionNumber(old.codexVersion) });
    expect(text).toContain('0.27.0');
    expect(text).toMatch(/Updating Codex/);
    // A newer one that did not answer just now; one that listed them; one still being asked.
    expect(modelsNoteKey({ modelsSource: 'seen', modelsDetected: false, modelsTooOld: false })).toBe('codex.models.notListed');
    expect(modelsNoteKey({ modelsSource: 'debugModels', modelsDetected: true })).toBe('');
    expect(modelsNoteKey({ modelsSource: 'none', modelsRefreshing: true })).toBe('');
    expect(modelsNoteKey({ modelsSource: 'none', modelsTooOld: true }, true)).toBe('');
    expect(modelsNoteKey({})).toBe('');
  });

  it('offers the efforts of the chosen model where the catalog lists them', () => {
    const o = { efforts: ['low', 'medium', 'high', 'xhigh', 'max'], modelInfo: [{ id: 'gpt-5.5', efforts: ['low', 'medium'] }, { id: 'bare', efforts: [] }] };
    expect(effortsFor(o, 'gpt-5.5')).toEqual(['low', 'medium']);
    expect(effortsFor(o, 'bare')).toEqual(o.efforts);
    expect(effortsFor(o, 'typed-by-hand')).toEqual(o.efforts);
    expect(effortsFor(o, null)).toEqual(o.efforts);
    expect(effortsFor({}, 'x')).toEqual([]);
  });

  it('has every source and note in English and Arabic', () => {
    const keys = ['refresh', 'refreshing', 'refreshFailed', 'tooOld', 'notListed',
      ...['debugModels', 'bundled', 'appServer', 'cache', 'config', 'seen', 'none'].map((s) => 'source.' + s)].map((k) => 'codex.models.' + k);
    for (const lang of ['en', 'ar']) {
      app.settings.language = lang;
      for (const k of keys) expect(t(k as any), k).not.toBe(k);
    }
    expect(t('codex.models.tooOld', { version: '0.27.0' })).toContain('0.27.0');
    app.settings.language = 'en';
  });
});
