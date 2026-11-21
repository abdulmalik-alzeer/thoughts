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

import { looksLikeText } from './text';

describe('looksLikeText (a file whose name does not say what it is)', () => {
  it('accepts plain text in any script, with or without a byte-order mark', () => {
    expect(looksLikeText(enc('just some notes\nsecond line\n'))).toBe(true);
    expect(looksLikeText(enc('مرحبا بالعالم\tx'))).toBe(true);
    expect(looksLikeText(new Uint8Array([0xef, 0xbb, 0xbf, ...enc('with a mark')]))).toBe(true);
    expect(looksLikeText(new Uint8Array([0xff, 0xfe, 0x68, 0, 0x69, 0]))).toBe(true);
    expect(looksLikeText(new Uint8Array())).toBe(true);
    // Coloured terminal output is still text.
    expect(looksLikeText(enc('\u001b[32mok\u001b[0m all tests passed, nothing else to say here at all, really nothing\n'.repeat(3)))).toBe(true);
  });
  it('a sample that ends in the middle of a character is not held against the file', () => {
    const big = enc('é'.repeat(5000));
    expect(big.length).toBeGreaterThan(8192);
    expect(looksLikeText(new Uint8Array([0x61, ...big]))).toBe(true);
  });
  it('refuses programs, packed files and pictures', () => {
    expect(looksLikeText(new Uint8Array([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1, 0, 0, 0]))).toBe(false);
    expect(looksLikeText(new Uint8Array([0x50, 0x4b, 3, 4, 20, 0, 8, 8]))).toBe(false);
    expect(looksLikeText(new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))).toBe(false);
    // No zero byte, but not valid UTF-8.
    expect(looksLikeText(new Uint8Array([0x61, 0xff, 0xfe, 0x62, 0xc3, 0x28]))).toBe(false);
    // Valid bytes, mostly control characters.
    expect(looksLikeText(new Uint8Array(200).fill(1))).toBe(false);
  });
});
