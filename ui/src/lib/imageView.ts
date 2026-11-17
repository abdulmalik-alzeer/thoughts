// The picture viewer's arithmetic (lib/ImageViewer.svelte): how big a picture is shown, where it
// sits, and how zooming and moving change that. Pure functions, so they can be tested without a
// screen. Coordinates are screen pixels inside the viewer's stage, origin at its top-left corner,
// whatever the reading direction; a picture point p is drawn at `x + scale * p`.

export type Size = { w: number; h: number };
export type View = { scale: number; x: number; y: number };

/** Room kept around a fitted picture. */
export const FIT_PAD = 24;
/** The most a picture is enlarged (in its own pixels). */
export const MAX_SCALE = 8;
/** One step of the +/− buttons and keys. */
export const STEP = 1.25;

const ok = (n: number) => Number.isFinite(n) && n > 0;

/** The scale that shows the whole picture in the stage, never enlarged past its own size. */
export function fitScale(img: Size, stage: Size, pad = FIT_PAD): number {
  if (!ok(img.w) || !ok(img.h) || !ok(stage.w) || !ok(stage.h)) return 1;
  const w = Math.max(1, stage.w - 2 * pad), h = Math.max(1, stage.h - 2 * pad);
  return Math.min(1, w / img.w, h / img.h);
}

/** The smallest and largest scale allowed for this picture in this stage. */
export function scaleRange(img: Size, stage: Size): { min: number; max: number } {
  const fit = fitScale(img, stage);
  return { min: fit, max: Math.max(MAX_SCALE, fit) };
}

export function clampScale(s: number, img: Size, stage: Size): number {
  const { min, max } = scaleRange(img, stage);
  if (!Number.isFinite(s)) return min;
  return Math.min(max, Math.max(min, s));
}

/** Keep the picture in view: a picture smaller than the stage (on one axis) is centred on it, a
 *  larger one cannot be moved so far that empty space shows beside it. */
export function clampPan(v: View, img: Size, stage: Size): View {
  const axis = (pos: number, size: number, room: number) => {
    const drawn = size * v.scale;
    if (drawn <= room) return (room - drawn) / 2;
    return Math.min(0, Math.max(room - drawn, pos));
  };
  return { scale: v.scale, x: axis(v.x, img.w, stage.w), y: axis(v.y, img.h, stage.h) };
}

/** The whole picture, centred. */
export function fitView(img: Size, stage: Size): View {
  return clampPan({ scale: fitScale(img, stage), x: 0, y: 0 }, img, stage);
}

/** Zoom to `scale`, keeping the picture point under (px, py) where it is. */
export function zoomAt(v: View, scale: number, px: number, py: number, img: Size, stage: Size): View {
  const s = clampScale(scale, img, stage);
  const ix = (px - v.x) / v.scale, iy = (py - v.y) / v.scale;
  return clampPan({ scale: s, x: px - ix * s, y: py - iy * s }, img, stage);
}

/** One step in (dir > 0) or out (dir < 0), around a point (the stage's centre by default). */
export function zoomStep(v: View, dir: number, img: Size, stage: Size, px = stage.w / 2, py = stage.h / 2): View {
  const f = dir > 0 ? STEP : 1 / STEP;
  let s = v.scale * f;
  // Passing 100% on the way stops there, so the picture's own size is always one step away.
  if ((v.scale < 1 && s > 1) || (v.scale > 1 && s < 1)) s = 1;
  return zoomAt(v, s, px, py, img, stage);
}

/** A wheel or pinch movement as a zoom factor: `delta` in pixels (negative = in). */
export function wheelFactor(delta: number, pixelsPerDouble = 300): number {
  if (!Number.isFinite(delta)) return 1;
  const d = Math.max(-pixelsPerDouble, Math.min(pixelsPerDouble, delta));
  return Math.pow(2, -d / pixelsPerDouble);
}

/** A wheel event's movement in pixels (lines and pages are converted). */
export function wheelPixels(deltaY: number, deltaMode: number, pageHeight = 800): number {
  if (deltaMode === 1) return deltaY * 16;
  if (deltaMode === 2) return deltaY * pageHeight;
  return deltaY;
}

/** Move by (dx, dy) screen pixels. */
export function panBy(v: View, dx: number, dy: number, img: Size, stage: Size): View {
  return clampPan({ scale: v.scale, x: v.x + dx, y: v.y + dy }, img, stage);
}

/** How far one arrow-key press moves (a tenth of the stage, at least 40 pixels). */
export function keyStep(stage: Size): { x: number; y: number } {
  return { x: Math.max(40, Math.round(stage.w / 10)), y: Math.max(40, Math.round(stage.h / 10)) };
}

/** An arrow key, read like scrolling: Right shows more of the picture's right side (it moves left).
 *  Physical directions, also in right-to-left layouts: the picture has no reading direction. */
export function arrowPan(v: View, key: string, img: Size, stage: Size, big = false): View | null {
  const st = keyStep(stage);
  const m = big ? 4 : 1;
  switch (key) {
    case 'ArrowLeft': return panBy(v, st.x * m, 0, img, stage);
    case 'ArrowRight': return panBy(v, -st.x * m, 0, img, stage);
    case 'ArrowUp': return panBy(v, 0, st.y * m, img, stage);
    case 'ArrowDown': return panBy(v, 0, -st.y * m, img, stage);
    default: return null;
  }
}

/** Is the picture shown whole (at the fitted scale)? */
export function isFit(v: View, img: Size, stage: Size): boolean {
  return Math.abs(v.scale - fitScale(img, stage)) < 1e-3;
}

/** Double-click: whole picture ↔ its own size (100%), around the point clicked. A picture that
 *  already fits at 100% goes to 200% instead, so the gesture always does something. */
export function toggleZoom(v: View, px: number, py: number, img: Size, stage: Size): View {
  if (!isFit(v, img, stage)) return fitView(img, stage);
  const fit = fitScale(img, stage);
  return zoomAt(v, fit < 1 - 1e-3 ? 1 : 2, px, py, img, stage);
}

/** The zoom level as a whole percentage. */
export function percent(v: View): number {
  return Math.round(v.scale * 100);
}

/** A readable name for a picture from its address, when nothing better is known ("" for data and blob addresses). */
export function nameFromSrc(src: string): string {
  if (!src || /^(data|blob):/i.test(src)) return '';
  const path = src.split(/[?#]/)[0];
  const last = path.split('/').filter(Boolean).pop() ?? '';
  try { return decodeURIComponent(last); } catch { return last; }
}
