// Both languages: every string an area of the app adds in English has its Arabic (i18n.ts falls
// back to English, so a missing one shows English in the Arabic app — for a count, the English
// "1 day" form is taken over the Arabic plural ones). Keys only in Arabic are its extra plural forms.
import { describe, it, expect, vi } from 'vitest';

const app = { settings: { language: 'en' } as { language: string } };
vi.mock('./store.svelte', () => ({ app }));
const { t, tn } = await import('./i18n');

const dicts = import.meta.glob('./i18n/*.ts', { eager: true }) as Record<string, { default: { en: Record<string, string>; ar?: Record<string, string> } }>;

describe('i18n: the two languages', () => {
  it('every English string of an area has its Arabic', () => {
    const missing: string[] = [];
    for (const [file, m] of Object.entries(dicts)) {
      const ar = m.default.ar ?? {};
      for (const k of Object.keys(m.default.en)) if (!(k in ar)) missing.push(`${file}: ${k}`);
    }
    expect(missing).toEqual([]);
  });

  it('an Arabic key with no English is one of its plural forms', () => {
    const odd: string[] = [];
    for (const [file, m] of Object.entries(dicts)) {
      for (const k of Object.keys(m.default.ar ?? {})) {
        if (k in m.default.en) continue;
        const base = k.replace(/_(zero|one|two|few|many|other)$/, '');
        if (base === k || !Object.keys(m.default.en).some((e) => e === base || e.startsWith(`${base}_`))) odd.push(`${file}: ${k}`);
      }
    }
    expect(odd).toEqual([]);
  });

  it('a count in Arabic never takes the English form', () => {
    app.settings.language = 'ar';
    try {
      expect(tn('r10-usage.days', 1)).not.toMatch(/day/);
      expect(t('r10-usage.days_one', { n: 1 })).not.toMatch(/day/);
    } finally { app.settings.language = 'en'; }
  });
});
