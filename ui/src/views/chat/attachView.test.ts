import { describe, expect, it } from 'vitest';
import { chipAction, dataUrlBytes, fileName } from './attachView';
import { previewKind, previewPlan } from '../../lib/preview/kinds';
import { renderers } from '../../lib/preview/renderers';

describe('files in a chat', () => {
  it('every file opens with one click: pictures in the picture viewer, the rest in the preview', () => {
    for (const n of ['shot.PNG', 'a.jpeg', 'a.jpg', 'a.gif', 'a.webp', 'a.svg', 'a.bmp', 'a.avif']) expect(chipAction(n), n).toBe('picture');
    expect(chipAction('photo.heic')).toBe('preview');
    for (const n of ['budget.xlsx', 'letter.docx', 'deck.pptx', 'paper.pdf', 'notes.txt', 'main.rs', 'old.xls', 'Budget.numbers', 'noext', 'clip.mp4', 'a.zip'])
      expect(chipAction(n), n).toBe('preview');
  });
  it('office files attached to a chat have a preview of their own', () => {
    for (const n of ['budget.xlsx', 'macro.xlsm', 'letter.docx', 'deck.pptx', 'show.ppsx', 'sheet.ods', 'essay.odt', 'talk.odp', 'memo.rtf', 'rows.csv'])
      expect(renderers[previewKind(n)], n).toBeTruthy();
    // What only its own app can show says so; a name that says nothing is tried as text.
    expect(previewPlan('Budget.numbers')).toEqual({ kind: 'none', trial: false, appOnly: true });
    expect(previewPlan('attached-file')).toEqual({ kind: 'text', trial: true, appOnly: false });
  });
  it('takes the name from a path of either kind', () => {
    expect(fileName('/Users/x/.thoughts/attachments/c-1/Q3 report.xlsx')).toBe('Q3 report.xlsx');
    expect(fileName('C:\\Users\\x\\a\\تقرير.docx')).toBe('تقرير.docx');
    expect(fileName('plain.txt')).toBe('plain.txt');
  });
  it('reads back the bytes of a file waiting in the message box', () => {
    const bytes = new Uint8Array([0x50, 0x4b, 3, 4, 0, 255, 128, 10]);
    const url = 'data:application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;base64,' + btoa(String.fromCharCode(...bytes));
    expect([...dataUrlBytes(url)]).toEqual([...bytes]);
    expect([...dataUrlBytes('data:;base64,')]).toEqual([]);
    expect(new TextDecoder().decode(dataUrlBytes('data:text/plain;charset=utf-8;base64,' + btoa(unescape(encodeURIComponent('مرحبا')))))).toBe('مرحبا');
    expect(new TextDecoder().decode(dataUrlBytes('data:text/plain,hello%20there'))).toBe('hello there');
    expect(() => dataUrlBytes('https://example.com/x.xlsx')).toThrow();
    expect(() => dataUrlBytes('data:text/plain;base64,@@@')).toThrow();
  });
});
