// The app's own history descriptions are shown in the reader's language, and each one still
// matches what the service writes (a renamed subject would quietly show in English again).
import { describe, it, expect, vi } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const app = { settings: { language: 'en' } as { language: string } };
vi.mock('./store.svelte', () => ({ app }));
const { appSubject, FIXED } = await import('./historySubject');

describe('history subjects', () => {
  it('reads the app’s own subjects in both languages and leaves the rest alone', () => {
    app.settings.language = 'en';
    expect(appSubject('edit: your changes')).toBe('Your changes');
    app.settings.language = 'ar';
    expect(appSubject('edit: your changes')).toBe('تغييراتك');
    expect(appSubject('outside: changed outside Thoughts')).toBe('تغيّر خارج Thoughts');
    expect(appSubject('wrap: 2026-11-17 — 2 chat(s)')).toBeNull();
    expect(appSubject('edit: moved a.md to b.md')).toBeNull();
    for (const s of Object.keys(FIXED)) expect(appSubject(s)).not.toContain('final-walk-a.');
    app.settings.language = 'en';
  });

  it('each one is a subject the service really writes', () => {
    const dir = join(__dirname, '../../../crates/thoughtsd/src');
    const src = readdirSync(dir).filter((f) => f.endsWith('.rs')).map((f) => readFileSync(join(dir, f), 'utf8')).join('\n');
    for (const s of Object.keys(FIXED)) {
      const [kind, ...rest] = s.split(': ');
      const why = rest.join(': ');
      const written = kind === 'edit' ? src.includes(`subject::edit("${why}")`) : src.includes(`"${s}"`);
      expect(written, s).toBe(true);
    }
  });
});
