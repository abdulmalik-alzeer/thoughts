// The picture viewer's zoom and move arithmetic (imageView.ts).
import { describe, it, expect } from 'vitest';
import {
  fitScale, fitView, clampPan, clampScale, zoomAt, zoomStep, panBy, arrowPan, toggleZoom, isFit,
  percent, wheelFactor, wheelPixels, keyStep, nameFromSrc, scaleRange, MAX_SCALE, FIT_PAD,
} from './imageView';

const stage = { w: 1000, h: 800 };
const big = { w: 4000, h: 2000 };
const small = { w: 200, h: 100 };

describe('fitScale', () => {
  it('shrinks a large picture to fit inside the padding', () => {
    expect(fitScale(big, stage)).toBeCloseTo((1000 - 2 * FIT_PAD) / 4000);
  });
  it('uses the tighter axis', () => {
    expect(fitScale({ w: 500, h: 3000 }, stage)).toBeCloseTo((800 - 2 * FIT_PAD) / 3000);
  });
  it('never enlarges a small picture', () => {
    expect(fitScale(small, stage)).toBe(1);
  });
  it('is 1 for sizes not known yet', () => {
    expect(fitScale({ w: 0, h: 0 }, stage)).toBe(1);
    expect(fitScale(big, { w: NaN, h: 10 })).toBe(1);
  });
});

describe('fitView and clampPan', () => {
  it('centres the fitted picture', () => {
    const v = fitView(big, stage);
    expect(v.x).toBeCloseTo((1000 - 4000 * v.scale) / 2);
    expect(v.y).toBeCloseTo((800 - 2000 * v.scale) / 2);
  });
  it('keeps a larger-than-stage picture from leaving empty space', () => {
    const v = clampPan({ scale: 1, x: 500, y: -5000 }, big, stage);
    expect(v.x).toBe(0);
    expect(v.y).toBe(800 - 2000);
  });
  it('centres an axis that fits', () => {
    const v = clampPan({ scale: 1, x: 999, y: 999 }, small, stage);
    expect(v).toEqual({ scale: 1, x: 400, y: 350 });
  });
});

describe('zooming', () => {
  it('keeps the point under the cursor still', () => {
    const v0 = { scale: 1, x: -1000, y: -500 };
    const px = 300, py = 200;
    const ix = (px - v0.x) / v0.scale, iy = (py - v0.y) / v0.scale;
    const v = zoomAt(v0, 2, px, py, big, stage);
    expect(v.scale).toBe(2);
    expect(v.x + ix * v.scale).toBeCloseTo(px);
    expect(v.y + iy * v.scale).toBeCloseTo(py);
  });
  it('is held between the fitted size and the maximum', () => {
    const { min, max } = scaleRange(big, stage);
    expect(clampScale(0.0001, big, stage)).toBe(min);
    expect(clampScale(100, big, stage)).toBe(max);
    expect(max).toBe(MAX_SCALE);
    expect(clampScale(NaN, big, stage)).toBe(min);
  });
  it('steps by a quarter and stops at 100% on the way', () => {
    const v1 = zoomStep({ scale: 1, x: 0, y: 0 }, 1, big, stage);
    expect(v1.scale).toBeCloseTo(1.25);
    const v2 = zoomStep({ scale: 0.9, x: 0, y: 0 }, 1, big, stage);
    expect(v2.scale).toBe(1);
    const v3 = zoomStep({ scale: 1.1, x: 0, y: 0 }, -1, big, stage);
    expect(v3.scale).toBe(1);
  });
  it('cannot step out past the fitted size', () => {
    const fit = fitView(big, stage);
    expect(zoomStep(fit, -1, big, stage).scale).toBeCloseTo(fit.scale);
  });
});

describe('wheel', () => {
  it('turns a scroll into a factor: up is in, down is out, nothing is nothing', () => {
    expect(wheelFactor(-100)).toBeGreaterThan(1);
    expect(wheelFactor(100)).toBeLessThan(1);
    expect(wheelFactor(0)).toBe(1);
    expect(wheelFactor(NaN)).toBe(1);
    expect(wheelFactor(-300)).toBeCloseTo(2);
  });
  it('limits one huge jump', () => {
    expect(wheelFactor(-1e6)).toBeCloseTo(2);
  });
  it('converts lines and pages to pixels', () => {
    expect(wheelPixels(3, 0)).toBe(3);
    expect(wheelPixels(3, 1)).toBe(48);
    expect(wheelPixels(1, 2, 700)).toBe(700);
  });
});

describe('moving', () => {
  it('drags and stays inside the edges', () => {
    const v = panBy({ scale: 1, x: -100, y: -100 }, 50, 20, big, stage);
    expect(v).toEqual({ scale: 1, x: -50, y: -80 });
    expect(panBy(v, 10_000, 10_000, big, stage)).toEqual({ scale: 1, x: 0, y: 0 });
  });
  it('arrow keys work like scrolling', () => {
    const v0 = { scale: 1, x: -1000, y: -600 };
    const st = keyStep(stage);
    expect(arrowPan(v0, 'ArrowRight', big, stage)!.x).toBe(-1000 - st.x);
    expect(arrowPan(v0, 'ArrowLeft', big, stage)!.x).toBe(-1000 + st.x);
    expect(arrowPan(v0, 'ArrowDown', big, stage)!.y).toBe(-600 - st.y);
    expect(arrowPan(v0, 'ArrowUp', big, stage)!.y).toBe(-600 + st.y);
    expect(arrowPan(v0, 'ArrowUp', big, stage, true)!.y).toBe(-600 + 4 * st.y);
    expect(arrowPan(v0, 'a', big, stage)).toBeNull();
  });
  it('a key step is at least 40 pixels', () => {
    expect(keyStep({ w: 100, h: 100 })).toEqual({ x: 40, y: 40 });
  });
});

describe('double-click', () => {
  it('goes from whole to 100% around the point, and back', () => {
    const fit = fitView(big, stage);
    const z = toggleZoom(fit, 500, 400, big, stage);
    expect(z.scale).toBe(1);
    expect(isFit(z, big, stage)).toBe(false);
    const back = toggleZoom(z, 10, 10, big, stage);
    expect(back).toEqual(fit);
  });
  it('a small picture already at 100% goes to 200%', () => {
    const fit = fitView(small, stage);
    expect(fit.scale).toBe(1);
    expect(toggleZoom(fit, 500, 400, small, stage).scale).toBe(2);
  });
});

describe('labels', () => {
  it('shows a whole percentage', () => {
    expect(percent({ scale: 0.2345, x: 0, y: 0 })).toBe(23);
  });
  it('names a picture from its address', () => {
    expect(nameFromSrc('https://x.test/a/b/My%20photo.png?x=1#y')).toBe('My photo.png');
    expect(nameFromSrc('pics/one.jpg')).toBe('one.jpg');
    expect(nameFromSrc('data:image/png;base64,AAAA')).toBe('');
    expect(nameFromSrc('blob:http://x/123')).toBe('');
    expect(nameFromSrc('')).toBe('');
    expect(nameFromSrc('bad%E0.png')).toBe('bad%E0.png');
  });
});
