// Drawing the Graph page's map on a canvas: links as hairlines, notes as dots in their area's colour,
// names where there is room. One picture per call; the page calls it again only when something
// changed (the view moved, the pointer found another note, a filter changed), never in a loop.
import type { Camera, Model } from './model';
import type { Ink } from './palette';

export type Scene = {
  m: Model;
  xs: ArrayLike<number>;
  ys: ArrayLike<number>;
  cam: Camera;
  w: number;
  h: number;
  dpr: number;
  palette: string[];
  ink: Ink;
  shown: (i: number) => boolean;
  /** The note under the pointer or chosen with the keyboard (-1: none). */
  active: number;
  /** Search hits (null: no search). */
  hits: Set<number> | null;
  /** The note a "links around" map is about (-1: none). */
  centre: number;
  font: string;
  /** High contrast: stronger lines. */
  strong: boolean;
};

const MAX_LABELS = 260;

export const colourOf = (palette: string[], group: number) => palette.length ? palette[group % palette.length] : '#888888';

export function draw(ctx: CanvasRenderingContext2D, s: Scene) {
  const { m, xs, ys, cam, w, h, dpr, ink, shown, active, hits } = s;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  const sx = (i: number) => xs[i] * cam.k + cam.x, sy = (i: number) => ys[i] * cam.k + cam.y;
  const inView = (i: number, pad: number) => { const x = sx(i), y = sy(i); return x > -pad && x < w + pad && y > -pad && y < h + pad; };
  const near = new Set<number>();
  if (active >= 0) { for (const j of m.out[active]) near.add(j); for (const j of m.inn[active]) near.add(j); }
  const lit = (i: number) => !hits || hits.has(i) || i === active || near.has(i);

  // Links: the ordinary ones in one stroke, then those of the active note on top.
  ctx.lineCap = 'round';
  const base = (alpha: number) => { ctx.globalAlpha = alpha; ctx.strokeStyle = ink.muted; };
  const pass = (dim: boolean) => {
    ctx.beginPath();
    let any = false;
    for (const e of m.edges) {
      if (!shown(e.s) || !shown(e.t) || e.s === active || e.t === active) continue;
      if ((!lit(e.s) || !lit(e.t)) !== dim) continue;
      if (!inView(e.s, 0) && !inView(e.t, 0) && !crosses(sx(e.s), sy(e.s), sx(e.t), sy(e.t), w, h)) continue;
      ctx.moveTo(sx(e.s), sy(e.s)); ctx.lineTo(sx(e.t), sy(e.t)); any = true;
    }
    if (any) ctx.stroke();
  };
  ctx.lineWidth = s.strong ? 1.25 : 1;
  base(hits ? 0.08 : s.strong ? 0.55 : 0.32); pass(true);
  base(s.strong ? 0.7 : 0.38); pass(false);
  if (active >= 0) {
    ctx.globalAlpha = 1; ctx.strokeStyle = ink.accent; ctx.fillStyle = ink.accent; ctx.lineWidth = 2;
    for (const e of m.edges) {
      if ((e.s !== active && e.t !== active) || !shown(e.s) || !shown(e.t)) continue;
      arrow(ctx, sx(e.s), sy(e.s), sx(e.t), sy(e.t), m.nodes[e.t].r * cam.k);
    }
  }

  // Notes.
  const order: number[] = [];
  for (const n of m.nodes) if (shown(n.i) && inView(n.i, 30)) order.push(n.i);
  for (const i of order) {
    const n = m.nodes[i];
    const r = Math.max(1.6, n.r * cam.k);
    ctx.globalAlpha = lit(i) ? 1 : 0.2;
    ctx.beginPath();
    ctx.arc(sx(i), sy(i), r, 0, Math.PI * 2);
    ctx.fillStyle = colourOf(s.palette, n.group);
    ctx.fill();
    if (n.kind === 'item' && r > 3) { ctx.lineWidth = Math.min(2, r / 3); ctx.strokeStyle = ink.bg; ctx.stroke(); }
  }
  ctx.globalAlpha = 1;
  const ring = (i: number, colour: string, width: number, gap: number) => {
    if (i < 0 || !shown(i)) return;
    const r = Math.max(1.6, m.nodes[i].r * cam.k);
    ctx.beginPath(); ctx.arc(sx(i), sy(i), r + gap, 0, Math.PI * 2);
    ctx.lineWidth = width; ctx.strokeStyle = colour; ctx.stroke();
  };
  if (hits) for (const i of hits) if (i !== active) ring(i, ink.fg, 1.5, 2.5);
  if (s.centre >= 0) ring(s.centre, ink.fg, 2, 3.5);
  if (active >= 0) { ring(active, ink.bg, 5, 3.5); ring(active, ink.accent, 2.5, 3.5); }

  // Names: the active note and its neighbours, search hits, the centre, then whatever has room.
  ctx.font = s.font;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'top';
  ctx.lineJoin = 'round';
  const want: number[] = [];
  const seen = new Set<number>();
  const push = (i: number) => { if (i >= 0 && !seen.has(i) && shown(i) && inView(i, 0)) { seen.add(i); want.push(i); } };
  push(active); push(s.centre);
  for (const i of near) push(i);
  if (hits) for (const i of hits) push(i);
  if (!hits) {
    const roomy = order.filter((i) => m.nodes[i].r * cam.k >= 5 || cam.k >= 1.6).sort((a, b) => m.nodes[b].deg - m.nodes[a].deg);
    for (const i of roomy) { if (want.length >= MAX_LABELS) break; push(i); }
  }
  const boxes: [number, number, number, number][] = [];
  for (const i of want.slice(0, MAX_LABELS)) {
    const n = m.nodes[i];
    const text = n.label.length > 40 ? n.label.slice(0, 39) + '…' : n.label;
    const x = sx(i), y = sy(i) + Math.max(1.6, n.r * cam.k) + 4;
    const tw = ctx.measureText(text).width;
    const box: [number, number, number, number] = [x - tw / 2 - 2, y - 1, x + tw / 2 + 2, y + 15];
    const vip = i === active || i === s.centre || near.has(i) || (hits?.has(i) ?? false);
    if (!vip && boxes.some((b) => b[0] < box[2] && box[0] < b[2] && b[1] < box[3] && box[1] < b[3])) continue;
    boxes.push(box);
    ctx.globalAlpha = 1;
    ctx.lineWidth = 3.5; ctx.strokeStyle = ink.bg; ctx.strokeText(text, x, y);
    ctx.fillStyle = i === active ? ink.fg : vip ? ink.fg : ink.muted;
    ctx.fillText(text, x, y);
  }
  ctx.globalAlpha = 1;
}

/** Does the segment pass through the w×h box although both ends are outside it? (Rough: by its box.) */
function crosses(x1: number, y1: number, x2: number, y2: number, w: number, h: number) {
  return Math.max(x1, x2) > 0 && Math.min(x1, x2) < w && Math.max(y1, y2) > 0 && Math.min(y1, y2) < h;
}

/** A line from (x1, y1) to the edge of the dot at (x2, y2), with an arrowhead showing which way the link goes. */
function arrow(ctx: CanvasRenderingContext2D, x1: number, y1: number, x2: number, y2: number, r: number) {
  const dx = x2 - x1, dy = y2 - y1, l = Math.hypot(dx, dy);
  if (l < 1) return;
  const ux = dx / l, uy = dy / l;
  const ex = x2 - ux * (r + 3), ey = y2 - uy * (r + 3);
  ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(ex, ey); ctx.stroke();
  if (l < r + 12) return;
  const a = 7, b = 4;
  ctx.beginPath();
  ctx.moveTo(ex, ey);
  ctx.lineTo(ex - ux * a - uy * b, ey - uy * a + ux * b);
  ctx.lineTo(ex - ux * a + uy * b, ey - uy * a - ux * b);
  ctx.closePath(); ctx.fill();
}
