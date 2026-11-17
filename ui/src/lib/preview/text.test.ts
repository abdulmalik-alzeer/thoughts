import { describe, expect, it } from 'vitest';
import { cutText, decodeText } from './text';

const enc = (s: string) => new TextEncoder().encode(s);

describe('decodeText', () => {
  it('reads UTF-8 with or without a byte-order mark, and Arabic', () => {
    expect(decodeText(enc('سلام hello'))).toBe('سلام hello');
    expect(decodeText(new Uint8Array([0xef, 0xbb, 0xbf, ...enc('x')]))).toBe('x');
  });
  it('reads UTF-16 with its mark', () => {
    expect(decodeText(new Uint8Array([0xff, 0xfe, 0x68, 0, 0x69, 0]))).toBe('hi');
    expect(decodeText(new Uint8Array([0xfe, 0xff, 0, 0x68, 0, 0x69]))).toBe('hi');
  });
  it('says bytes with NULs are not text', () => {
    expect(decodeText(new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0, 0, 0]))).toBeNull();
  });
  it('an empty file is empty text', () => { expect(decodeText(new Uint8Array())).toBe(''); });
});

describe('cutText', () => {
  it('keeps short text whole', () => {
    expect(cutText('a\nb\n', 10, 100)).toEqual({ text: 'a\nb\n', lines: 2, shownLines: 2, cut: false });
    expect(cutText('', 10, 100)).toEqual({ text: '', lines: 1, shownLines: 1, cut: false });
  });
  it('cuts by lines', () => {
    const r = cutText('1\n2\n3\n4\n5', 3, 100);
    expect(r).toEqual({ text: '1\n2\n3', lines: 5, shownLines: 3, cut: true });
  });
  it('cuts by characters at a line end when it can', () => {
    const r = cutText('aaaa\nbbbb\ncccc', 100, 7);
    expect(r.text).toBe('aaaa');
    expect(r.cut).toBe(true);
    expect(cutText('x'.repeat(50), 100, 10).text).toBe('x'.repeat(10));
  });
});
