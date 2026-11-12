// The user guides (docs/*.md and docs/ar/*.md), bundled into the app at build time so Help can show
// them without any file on the user's computer. Developer documents (the glossary among them) are
// left out; so is the change log, which has its own view ("What's new").
import { lang } from './i18n';

const NOT_GUIDES = /^(DEVELOPMENT|RELEASING|REVIEW[-\w.]*|CHANGELOG|GLOSSARY)$/i;
const ORDER = ['PRIVACY', 'COST', 'HISTORY', 'CUSTOMISING'];

const en = import.meta.glob('../../../docs/*.md', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const ar = import.meta.glob('../../../docs/ar/*.md', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

const byName = (files: Record<string, string>) => {
  const out: Record<string, string> = {};
  for (const [p, src] of Object.entries(files)) out[p.split('/').pop()!.replace(/\.md$/i, '').toUpperCase()] = src;
  return out;
};
const EN = byName(en), AR = byName(ar);

/** Lines that only point at the other language's copy ("العربية: …", "English: …"): the app already speaks the chosen language. */
const clean = (s: string) => s.replace(/^(?:العربية|English)\s*:.*$/gm, '').replace(/\n{3,}/g, '\n\n');
const titleOf = (s: string) => /^#\s+(.+)$/m.exec(s)?.[1].trim().replace(/[*_`]/g, '') ?? '';

/** Guide names (file name without extension, upper-cased), the usual order first. */
export const guideNames = Object.keys(EN).filter((n) => !NOT_GUIDES.test(n)).sort((a, b) => {
  const ia = ORDER.indexOf(a), ib = ORDER.indexOf(b);
  return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib) || a.localeCompare(b);
});

/** The guide in the app's language, English when there is no translation. */
export function guideSource(name: string): string | undefined {
  const n = name.toUpperCase();
  const src = (lang() === 'ar' ? AR[n] : undefined) ?? EN[n];
  return src === undefined ? undefined : clean(src);
}
export const guideTitle = (name: string) => titleOf(guideSource(name) ?? '') || name;
export const hasGuide = (name: string) => guideNames.includes(name.toUpperCase());

/** Where a link inside a guide goes: another guide (`PRIVACY.md#section`, `../COST.md`), or nowhere in the app. */
export function guideLink(href: string): { name: string; anchor: string } | null {
  if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return null;
  const [file, anchor = ''] = href.split('#');
  const m = /(?:^|\/)([^/]+)\.md$/i.exec(file);
  if (!m || !hasGuide(m[1])) return null;
  let a = anchor;
  try { a = decodeURIComponent(anchor); } catch { /* keep as written */ }
  return { name: m[1].toUpperCase(), anchor: a };
}

/** GitHub-style heading slug, so `#المجلدات-الخاصة` and `#private-folders` find their heading. */
export const slug = (s: string) => s.trim().toLowerCase().replace(/[^\p{L}\p{N}\p{M}\s-]/gu, '').replace(/\s/g, '-');

/** The change log in the app's language (English when missing). */
export function changelogSource(): string {
  const f = (o: Record<string, string>) => o['CHANGELOG'];
  return (lang() === 'ar' ? f(AR) : undefined) ?? f(EN) ?? '';
}
