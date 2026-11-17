import { describe, expect, it } from 'vitest';
import { LIMITS, SERVED_MAX, codeLang, extOf, mimeFor, previewKind, tooLarge } from './kinds';

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
