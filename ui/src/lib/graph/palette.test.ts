import { describe, expect, it } from 'vitest';
import { contrast, derivePalette, parseColor } from './palette';

// The six built-in looks (theme.css): page, panel, accent, second accent.
const THEMES = {
  'saudi day': { bg: '#f6f3ec', panel: '#fffdf9', accent: '#006c35', accent2: '#7d5d12' },
  'saudi night': { bg: '#0e1612', panel: '#15211b', accent: '#2fae6e', accent2: '#d4af37' },
  'contrast day': { bg: '#ffffff', panel: '#ffffff', accent: '#0030a8', accent2: '#000000' },
  'contrast night': { bg: '#000000', panel: '#000000', accent: '#ffd400', accent2: '#ffffff' },
  'tech day': { bg: '#f3f1fa', panel: '#ffffff', accent: '#b5008a', accent2: '#006f80' },
  'tech night': { bg: '#0b0816', panel: '#141029', accent: '#ff2bd6', accent2: '#00f0ff' },
};

describe('graph palette', () => {
  for (const [name, th] of Object.entries(THEMES)) {
    it(`gives distinct, legible area colours in ${name}`, () => {
      const p = derivePalette(th);
      expect(p.length).toBeGreaterThanOrEqual(6);
      expect(new Set(p).size).toBe(p.length);
      for (const c of p) {
        expect(contrast(parseColor(c)!, parseColor(th.bg)!)).toBeGreaterThanOrEqual(3);
        expect(contrast(parseColor(c)!, parseColor(th.panel)!)).toBeGreaterThanOrEqual(3);
      }
      // The theme's own accent comes first.
      expect(contrast(parseColor(p[0])!, parseColor(th.accent)!)).toBeLessThan(1.6);
    });
  }

  it('reads the colour forms a browser hands back', () => {
    expect(parseColor('rgb(1, 2, 3)')).toEqual([1, 2, 3]);
    expect(parseColor('rgba(10, 20, 30, 0.5)')).toEqual([10, 20, 30]);
    expect(parseColor('#fff')).toEqual([255, 255, 255]);
    expect(parseColor('color(srgb 1 0 0.5)')).toEqual([255, 0, 127.5]);
    expect(parseColor('nonsense')).toBeNull();
  });
});
