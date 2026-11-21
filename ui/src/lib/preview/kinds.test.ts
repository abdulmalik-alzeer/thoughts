import { describe, expect, it } from 'vitest';
import { LIMITS, SERVED_MAX, TRIAL_MAX, appOnly, codeLang, extOf, mimeFor, previewKind, previewPlan, tooLarge } from './kinds';
import { renderers } from './renderers';

describe('previewKind', () => {
  it('knows the common kinds, in any case', () => {
    const cases: [string, string][] = [
      ['photo.PNG', 'image'], ['a.jpeg', 'image'], ['x.heic', 'image'], ['logo.svg', 'svg'], ['report.pdf', 'pdf'],
      ['notes.txt', 'text'], ['README.md', 'text'], ['main.rs', 'code'], ['app.tsx', 'code'], ['data.json', 'json'],
      ['ci.yml', 'yaml'], ['feed.xml', 'xml'], ['Info.plist', 'xml'], ['page.html', 'html'], ['song.mp3', 'audio'],
      ['clip.mov', 'video'], ['bundle.zip', 'zip'], ['letter.docx', 'docx'], ['budget.xlsx', 'xlsx'], ['rows.csv', 'csv'],
      ['rows.tsv', 'csv'], ['deck.pptx', 'pptx'], ['essay.odt', 'odt'], ['sheet.ods', 'ods'], ['talk.odp', 'odp'],
      ['old.doc', 'legacy'], ['old.xls', 'legacy'], ['old.ppt', 'legacy'], ['memo.rtf', 'legacy'],
    ];
    for (const [name, kind] of cases) expect(previewKind(name), name).toBe(kind);
  });
  it('macro-enabled Office files are previewed as data', () => {
    expect(previewKind('m.docm')).toBe('docx');
    expect(previewKind('m.xlsm')).toBe('xlsx');
    expect(previewKind('m.pptm')).toBe('pptx');
  });
  it('uses the last part of a path, and whole names without an extension', () => {
    expect(previewKind('ideas/x/outputs/Makefile')).toBe('code');
    expect(previewKind('a\\b\\LICENSE')).toBe('text');
    expect(previewKind('.gitignore')).toBe('code');
    expect(previewKind('archive.tar.gz')).toBe('none');
    expect(previewKind('noext')).toBe('none');
    expect(previewKind('program.exe')).toBe('none');
    expect(previewKind('')).toBe('none');
  });
  it('a file the service will not hand out is not previewable', () => {
    expect(previewKind('big.png', SERVED_MAX)).toBe('image');
    expect(previewKind('big.png', SERVED_MAX + 1)).toBe('none');
  });
});

describe('codeLang / extOf / limits', () => {
  it('gives a language hint for source and structured text', () => {
    expect(codeLang('x.py')).toBe('py');
    expect(codeLang('x.ts')).toBe('ts');
    expect(codeLang('Dockerfile')).toBe('sh');
    expect(codeLang('a.json')).toBe('json');
    expect(codeLang('a.yaml')).toBe('yaml');
    expect(codeLang('a.html')).toBe('html');
    expect(codeLang('a.plist')).toBe('xml');
    expect(codeLang('a.md')).toBe('md');
    expect(codeLang('a.txt')).toBe('');
    expect(codeLang('LICENSE')).toBe('');
  });
  it('extOf ignores leading dots and folders', () => {
    expect(extOf('.env')).toBe('');
    expect(extOf('a.b/c')).toBe('');
    expect(extOf('x/y.Tar.GZ')).toBe('gz');
  });
  it('tooLarge compares against the kind limit', () => {
    expect(tooLarge('text', undefined)).toBe(false);
    expect(tooLarge('text', LIMITS.text)).toBe(false);
    expect(tooLarge('text', LIMITS.text + 1)).toBe(true);
    expect(tooLarge('none', 1)).toBe(true);
    expect(Object.values(LIMITS).every((n) => n <= SERVED_MAX)).toBe(true);
  });
  it('an SVG is only ever typed as a picture, a PDF as a PDF', () => {
    expect(mimeFor('a.svg')).toBe('image/svg+xml');
    expect(mimeFor('a.pdf')).toBe('application/pdf');
    expect(mimeFor('a.jpg')).toBe('image/jpeg');
    expect(mimeFor('a.html')).toBe('text/plain');
    expect(mimeFor('a.mov')).toBe('video/quicktime');
    expect(mimeFor('a.docx')).toBe('application/octet-stream');
  });
});

describe('any kind of file', () => {
  it('knows the other spellings of the office kinds and more kinds of plain text', () => {
    const cases: [string, string][] = [
      ['t.dotm', 'docx'], ['t.xltm', 'xlsx'], ['t.ppsm', 'pptx'], ['t.potm', 'pptx'], ['t.ott', 'odt'], ['t.ots', 'ods'], ['t.otp', 'odp'],
      ['events.jsonl', 'text'], ['invite.ics', 'text'], ['card.vcf', 'text'], ['mail.eml', 'text'], ['guide.adoc', 'text'],
    ];
    for (const [name, kind] of cases) expect(previewKind(name), name).toBe(kind);
  });
  it('every kind but "none" has a renderer, so a known file never shows nothing', () => {
    for (const [kind, loader] of Object.entries(renderers)) expect(!!loader, kind).toBe(kind !== 'none');
  });
  it('a file only its own app can show says so instead of being tried', () => {
    for (const n of ['Budget.numbers', 'Letter.pages', 'Talk.key', 'big.xlsb', 'book.epub', 'art.psd', 'a.tar.gz', 'setup.exe', 'disk.dmg', 'data.sqlite', 'font.woff2', 'film.mkv']) {
      expect(appOnly(n), n).toBe(true);
      expect(previewPlan(n), n).toEqual({ kind: 'none', trial: false, appOnly: true });
    }
    for (const n of ['a.xlsx', 'a.txt', 'noext', 'a.unknownext', 'a.dat']) expect(appOnly(n), n).toBe(false);
  });
  it('a name that says nothing is tried as text, up to a size', () => {
    for (const n of ['noext', 'notes.unknownext', 'server.log.1', 'values.dat', 'folder/Thing', 'x.cfg2'])
      expect(previewPlan(n), n).toEqual({ kind: 'text', trial: true, appOnly: false });
    expect(previewPlan('noext', TRIAL_MAX)).toEqual({ kind: 'text', trial: true, appOnly: false });
    expect(previewPlan('noext', TRIAL_MAX + 1)).toEqual({ kind: 'none', trial: false, appOnly: false });
    expect(previewPlan('')).toEqual({ kind: 'none', trial: false, appOnly: false });
  });
  it('a known kind is never a trial, and one too big for the service stays "none"', () => {
    expect(previewPlan('budget.xlsx')).toEqual({ kind: 'xlsx', trial: false, appOnly: false });
    expect(previewPlan('old.xls')).toEqual({ kind: 'legacy', trial: false, appOnly: false });
    expect(previewPlan('big.png', SERVED_MAX + 1)).toEqual({ kind: 'none', trial: false, appOnly: false });
    expect(previewPlan('Budget.numbers', SERVED_MAX + 1)).toEqual({ kind: 'none', trial: false, appOnly: false });
  });
});
