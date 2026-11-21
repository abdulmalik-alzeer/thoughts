// A diagram as a draw.io file (diagrams.net), made from its finished SVG: every box becomes a shape
// that can be moved and restyled, every connection an edge attached to the two boxes it joins, and
// whatever else is drawn (frames, lanes, captions, loose lines) a shape, a line or a text of its
// own. Archify has no draw.io output; it does mark its drawing with what each part is
// (`data-node-id`, `data-edge-from`, `data-edge-to`, `data-edge-label`), and that is what is read here.
//
// The SVG must carry its looks on the elements themselves (`fill`, `stroke`, `font-size` as
// attributes), which is how `standalone.ts` writes it. Curves are kept as the points they pass
// through (draw.io rounds the corners again); rotated parts are placed unrotated.
// Pure: it parses text and returns text.

type M = [number, number, number, number, number, number]; // a b c d e f
const ID: M = [1, 0, 0, 1, 0, 0];
const mul = (m: M, n: M): M => [m[0] * n[0] + m[2] * n[1], m[1] * n[0] + m[3] * n[1], m[0] * n[2] + m[2] * n[3], m[1] * n[2] + m[3] * n[3], m[0] * n[4] + m[2] * n[5] + m[4], m[1] * n[4] + m[3] * n[5] + m[5]];
const apply = (m: M, x: number, y: number): [number, number] => [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];

/** A `transform` attribute as a matrix (translate, scale and matrix; a rotation or skew is left out). */
export function transformOf(attr: string | null): M {
  let m = ID;
  for (const [, fn, args] of (attr || '').matchAll(/(\w+)\s*\(([^)]*)\)/g)) {
    const v = args.split(/[\s,]+/).filter(Boolean).map(Number);
    if (v.some(Number.isNaN)) continue;
    if (fn === 'translate') m = mul(m, [1, 0, 0, 1, v[0] ?? 0, v[1] ?? 0]);
    else if (fn === 'scale') m = mul(m, [v[0] ?? 1, 0, 0, v[1] ?? v[0] ?? 1, 0, 0]);
    else if (fn === 'matrix' && v.length === 6) m = mul(m, v as M);
  }
  return m;
}

/** The points a path passes through: where each of its segments ends. `curved`: it has curves. */
export function pathPoints(d: string): { points: [number, number][]; curved: boolean; closed: boolean } {
  const points: [number, number][] = [];
  let [x, y, sx, sy] = [0, 0, 0, 0];
  let curved = false, closed = false;
  const tokens = d.match(/[a-zA-Z]|-?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?/gi) ?? [];
  let i = 0, cmd = '';
  const isCmd = (t: string) => /^[a-zA-Z]$/.test(t);
  const ARGS: Record<string, number> = { M: 2, L: 2, T: 2, H: 1, V: 1, C: 6, S: 4, Q: 4, A: 7 };
  while (i < tokens.length) {
    if (isCmd(tokens[i])) {
      cmd = tokens[i++];
      if (cmd.toUpperCase() === 'Z') { closed = true; x = sx; y = sy; }
      continue;
    }
    const C = cmd.toUpperCase();
    const n = ARGS[C];
    if (!n) { i++; continue; }
    const v: number[] = [];
    for (let k = 0; k < n; k++) v.push(Number(tokens[i++]));
    if (v.some(Number.isNaN)) break;
    const rel = cmd !== C;
    if (C === 'H') x = rel ? x + v[0] : v[0];
    else if (C === 'V') y = rel ? y + v[0] : v[0];
    else { x = rel ? x + v[n - 2] : v[n - 2]; y = rel ? y + v[n - 1] : v[n - 1]; }
    if (C === 'M') { sx = x; sy = y; cmd = rel ? 'l' : 'L'; } // more numbers after a move are lines
    if ('CSQTA'.includes(C)) curved = true;
    points.push([x, y]);
  }
  return { points, curved, closed };
}

/** A colour as draw.io takes it: `#rrggbb` (or `none`), and how see-through it is (1 = solid). */
export function colourOf(value: string | null | undefined): { hex: string; alpha: number } {
  const v = (value || '').trim().toLowerCase();
  if (!v || v === 'none' || v === 'transparent' || v.startsWith('url(')) return { hex: 'none', alpha: 0 };
  const hex2 = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0');
  let m = /^#([0-9a-f]{3,8})$/.exec(v);
  if (m) {
    const h = m[1];
    if (h.length === 3 || h.length === 4) return { hex: '#' + [0, 1, 2].map((k) => h[k] + h[k]).join(''), alpha: h.length === 4 ? parseInt(h[3] + h[3], 16) / 255 : 1 };
    if (h.length === 6 || h.length === 8) return { hex: '#' + h.slice(0, 6), alpha: h.length === 8 ? parseInt(h.slice(6), 16) / 255 : 1 };
  }
  const parts = (s: string) => s.split(/[\s,/]+/).filter(Boolean);
  const alphaOf = (s: string | undefined) => (s === undefined ? 1 : s.endsWith('%') ? parseFloat(s) / 100 : parseFloat(s));
  m = /^rgba?\(([^)]+)\)$/.exec(v);
  if (m) {
    const p = parts(m[1]);
    const ch = (s: string) => (s.endsWith('%') ? (parseFloat(s) / 100) * 255 : parseFloat(s));
    if (p.length >= 3) return { hex: '#' + hex2(ch(p[0])) + hex2(ch(p[1])) + hex2(ch(p[2])), alpha: alphaOf(p[3]) };
  }
  m = /^color\(\s*srgb\s+([^)]+)\)$/.exec(v);
  if (m) {
    const p = parts(m[1]);
    if (p.length >= 3) return { hex: '#' + hex2(parseFloat(p[0]) * 255) + hex2(parseFloat(p[1]) * 255) + hex2(parseFloat(p[2]) * 255), alpha: alphaOf(p[3]) };
  }
  const NAMED: Record<string, string> = { black: '#000000', white: '#ffffff', red: '#ff0000', green: '#008000', blue: '#0000ff', gray: '#808080', grey: '#808080' };
  return NAMED[v] ? { hex: NAMED[v], alpha: 1 } : { hex: 'none', alpha: 0 };
}

const xml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/\n/g, '&#10;');
const n1 = (n: number) => String(Math.round(n * 10) / 10);
const numAttr = (el: Element, name: string, fallback = 0) => { const v = parseFloat(el.getAttribute(name) || ''); return Number.isFinite(v) ? v : fallback; };

interface Box { x: number; y: number; w: number; h: number }

/** How a shape or line is drawn, as draw.io style entries, from the element's own attributes. */
function paint(el: Element, kind: 'shape' | 'line'): string[] {
  const s: string[] = [];
  const opacity = numAttr(el, 'opacity', 1);
  const stroke = colourOf(el.getAttribute('stroke'));
  const sw = numAttr(el, 'stroke-width', 1);
  if (kind === 'shape') {
    const fill = colourOf(el.getAttribute('fill') ?? '#000000');
    s.push(`fillColor=${fill.hex}`);
    const fo = fill.alpha * numAttr(el, 'fill-opacity', 1) * opacity;
    if (fill.hex !== 'none' && fo < 0.995) s.push(`fillOpacity=${Math.round(fo * 100)}`);
  }
  s.push(`strokeColor=${stroke.hex}`);
  if (stroke.hex !== 'none') {
    s.push(`strokeWidth=${n1(sw)}`);
    const so = stroke.alpha * numAttr(el, 'stroke-opacity', 1) * opacity;
    if (so < 0.995) s.push(`strokeOpacity=${Math.round(so * 100)}`);
    const dash = (el.getAttribute('stroke-dasharray') || '').trim();
    if (dash && dash !== 'none' && !/^0[\s,]*$/.test(dash)) s.push('dashed=1', `dashPattern=${dash.split(/[\s,]+/).map((d) => n1(parseFloat(d) / Math.max(sw, 1))).join(' ')}`);
  }
  return s;
}

/** Is the element not drawn at all? */
function unseen(el: Element): boolean {
  return el.getAttribute('display') === 'none' || el.getAttribute('visibility') === 'hidden' || numAttr(el, 'opacity', 1) === 0;
}

const SKIP = new Set(['defs', 'style', 'title', 'desc', 'metadata', 'script', 'marker', 'pattern', 'clippath', 'mask', 'symbol', 'lineargradient', 'radialgradient', 'filter', 'foreignobject']);

/** The end of a line as draw.io names it, from the marker Archify put there (`er-many-start`, `cl-triangle`, `arrowhead`…). */
function arrow(marker: string | null): { kind: string; filled: boolean } {
  const id = (marker || '').toLowerCase();
  if (!id || id === 'none') return { kind: 'none', filled: false };
  if (id.includes('many-optional')) return { kind: 'ERzeroToMany', filled: false };
  if (id.includes('one-optional')) return { kind: 'ERzeroToOne', filled: false };
  if (id.includes('er-many')) return { kind: 'ERmany', filled: false };
  if (id.includes('er-one')) return { kind: 'ERone', filled: false };
  if (id.includes('diamond')) return { kind: 'diamond', filled: !id.includes('open') && !id.includes('hollow') };
  if (id.includes('triangle')) return { kind: 'block', filled: false };
  if (id.includes('open')) return { kind: 'open', filled: false };
  return { kind: 'block', filled: true };
}

/** Where things are placed: the cell they belong to and that cell's corner (a child's place is counted from it). */
interface Place { parent: string; ox: number; oy: number }
const PAGE: Place = { parent: '1', ox: 0, oy: 0 };
/** How far from a box the end of a connection may be and still be attached to it. */
const TOUCH = 14;

/** The draw.io file for one diagram. `name` is the page's name. */
export function svgToDrawio(svgText: string, name = 'Diagram'): string {
  const doc = new DOMParser().parseFromString(svgText, 'image/svg+xml');
  const svg = doc.documentElement;
  if (!svg || svg.nodeName.toLowerCase() !== 'svg') throw new Error('not an SVG');
  const vb = (svg.getAttribute('viewBox') || '').split(/[\s,]+/).map(Number);
  const [vw, vh] = vb.length === 4 && vb.every(Number.isFinite) ? [vb[2], vb[3]] : [numAttr(svg, 'width', 800), numAttr(svg, 'height', 600)];
  const cells: string[] = [];
  let seq = 1;
  const id = () => `c${++seq}`;
  const nodeCell = new Map<string, { id: string; box: Box }>();
  const edges: { el: Element; m: M }[] = [];
  const edgeLabels = new Set<string>();

  const vertex = (value: string, style: string[], b: Box, at: Place = PAGE) => {
    const cid = id();
    cells.push(`<mxCell id="${cid}" value="${xml(value)}" style="${style.join(';')};" vertex="1" parent="${at.parent}"><mxGeometry x="${n1(b.x - at.ox)}" y="${n1(b.y - at.oy)}" width="${n1(b.w)}" height="${n1(b.h)}" as="geometry"/></mxCell>`);
    return cid;
  };
  const boxOf = (el: Element, m: M): Box | null => {
    const tag = el.nodeName.toLowerCase();
    let x: number, y: number, w: number, h: number;
    if (tag === 'rect') { [x, y, w, h] = [numAttr(el, 'x'), numAttr(el, 'y'), numAttr(el, 'width'), numAttr(el, 'height')]; }
    else if (tag === 'circle') { const r = numAttr(el, 'r'); [x, y, w, h] = [numAttr(el, 'cx') - r, numAttr(el, 'cy') - r, r * 2, r * 2]; }
    else if (tag === 'ellipse') { const [rx, ry] = [numAttr(el, 'rx'), numAttr(el, 'ry')]; [x, y, w, h] = [numAttr(el, 'cx') - rx, numAttr(el, 'cy') - ry, rx * 2, ry * 2]; }
    else return null;
    const [x0, y0] = apply(m, x, y);
    const [x1, y1] = apply(m, x + w, y + h);
    const b = { x: Math.min(x0, x1), y: Math.min(y0, y1), w: Math.abs(x1 - x0), h: Math.abs(y1 - y0) };
    return b.w > 0 && b.h > 0 ? b : null;
  };
  const textOf = (el: Element) => {
    const lines = Array.from(el.querySelectorAll('tspan')).filter((t) => t.hasAttribute('x') || t.hasAttribute('dy'));
    const raw = lines.length > 1 ? lines.map((t) => t.textContent || '').join('\n') : el.textContent || '';
    return raw.split('\n').map((l) => l.replace(/\s+/g, ' ').trim()).filter(Boolean).join('\n');
  };
  const fontStyle = (el: Element) => {
    const s = [`fontSize=${n1(numAttr(el, 'font-size', 12))}`, `fontColor=${colourOf(el.getAttribute('fill') ?? '#000000').hex.replace('none', '#000000')}`];
    const weight = el.getAttribute('font-weight') || '';
    const bold = weight === 'bold' || parseInt(weight, 10) >= 600;
    const italic = el.getAttribute('font-style') === 'italic';
    if (bold || italic) s.push(`fontStyle=${(bold ? 1 : 0) + (italic ? 2 : 0)}`);
    const family = (el.getAttribute('font-family') || '').split(',')[0].replace(/["']/g, '').trim();
    if (family) s.push(`fontFamily=${family.replace(/[;=]/g, '')}`);
    return s;
  };
  const shapeStyle = (el: Element) => {
    const tag = el.nodeName.toLowerCase();
    const s = tag === 'rect' ? ['rounded=' + (numAttr(el, 'rx') > 0 ? 1 : 0)] : ['ellipse'];
    if (tag === 'rect' && numAttr(el, 'rx') > 0) { const short = Math.min(numAttr(el, 'width', 1), numAttr(el, 'height', 1)); s.push(`arcSize=${Math.min(50, Math.round((numAttr(el, 'rx') / short) * 100))}`); }
    return s.concat('whiteSpace=wrap', 'html=1', paint(el, 'shape'));
  };

  /** One box of the diagram: the group's own frame, with its name in it — or, when it holds more
   *  than its name (a table's columns, a class's members), with everything in it as parts that
   *  move with it. Returns false when the group has no frame. */
  const node = (g: Element, m: M, at: Place): boolean => {
    const frames = Array.from(g.children).filter((c) => ['rect', 'circle', 'ellipse'].includes(c.nodeName.toLowerCase()) && !c.hasAttribute('data-mask') && !unseen(c));
    // The frame is the largest plain shape directly in the group (smaller ones are badges and marks).
    let best: { el: Element; b: Box } | null = null;
    for (const el of frames) {
      const b = boxOf(el, mul(m, transformOf(el.getAttribute('transform'))));
      if (b && (!best || b.w * b.h > best.b.w * best.b.h)) best = { el, b };
    }
    if (!best) return false;
    const label = (g.getAttribute('data-node-label') || '').trim();
    const sub = (g.getAttribute('data-node-sublabel') || '').trim();
    const texts = Array.from(g.querySelectorAll('text')).filter((t) => !unseen(t));
    const said = texts.map(textOf).filter(Boolean);
    const own = new Set([label, sub, (g.getAttribute('data-node-tag') || '').trim(), `${label} ${sub}`.trim()].filter(Boolean));
    const justItsName = label ? said.every((s) => own.has(s) || s.split('\n').every((l) => own.has(l))) : said.length <= 2;
    const nid = g.getAttribute('data-node-id') || '';
    if (justItsName) {
      const value = label ? label + (sub ? '\n' + sub : '') : said.join('\n');
      const main = texts.find((t) => textOf(t) === label) ?? texts[0];
      const cid = vertex(value, shapeStyle(best.el).concat(main ? fontStyle(main) : []), best.b, at);
      if (nid && !nodeCell.has(nid)) nodeCell.set(nid, { id: cid, box: best.b });
      return true;
    }
    const cid = vertex('', shapeStyle(best.el).concat('container=1', 'collapsible=0', 'recursiveResize=0'), best.b, at);
    if (nid && !nodeCell.has(nid)) nodeCell.set(nid, { id: cid, box: best.b });
    const inside: Place = { parent: cid, ox: best.b.x, oy: best.b.y };
    for (const c of Array.from(g.children)) if (c !== best.el) walk(c, m, inside);
    return true;
  };

  const line = (el: Element, m: M, pts: [number, number][], curved: boolean, at: Place, ends: { from?: string; to?: string; label?: string } = {}) => {
    const p = pts.map(([x, y]) => apply(m, x, y));
    if (p.length < 2) return;
    const [a, z] = [p[0], p[p.length - 1]];
    const [start, end] = [arrow(el.getAttribute('marker-start')), arrow(el.getAttribute('marker-end'))];
    const style = [`endArrow=${end.kind}`, `endFill=${end.filled ? 1 : 0}`, `startArrow=${start.kind}`, `startFill=${start.filled ? 1 : 0}`,
      'html=1', curved ? 'rounded=1' : 'rounded=0', 'edgeStyle=none'].concat(paint(el, 'line'));
    // An end is attached to a box only where the line really touches it (a message between two
    // lifelines does not touch the boxes at their heads), at the very spot it touches.
    const touch = (key: string | undefined, pt: [number, number], side: 'exit' | 'entry') => {
      const n = key ? nodeCell.get(key) : undefined;
      if (!n) return '';
      const b = n.box;
      if (pt[0] < b.x - TOUCH || pt[0] > b.x + b.w + TOUCH || pt[1] < b.y - TOUCH || pt[1] > b.y + b.h + TOUCH) return '';
      const rel = (v: number, from: number, size: number) => Math.round(Math.min(1, Math.max(0, (v - from) / size)) * 1000) / 1000;
      style.push(`${side}X=${rel(pt[0], b.x, b.w)}`, `${side}Y=${rel(pt[1], b.y, b.h)}`, `${side}Dx=0`, `${side}Dy=0`);
      return n.id;
    };
    const [src, dst] = [touch(ends.from, a, 'exit'), touch(ends.to, z, 'entry')];
    if (ends.label) style.push('labelBackgroundColor=default', 'fontSize=11');
    const pt = ([x, y]: [number, number], as = '') => `<mxPoint x="${n1(x - at.ox)}" y="${n1(y - at.oy)}"${as ? ` as="${as}"` : ''}/>`;
    const mid = p.slice(1, -1).map((q) => pt(q)).join('');
    const attach = (src ? ` source="${src}"` : '') + (dst ? ` target="${dst}"` : '');
    cells.push(`<mxCell id="${id()}" value="${xml(ends.label || '')}" style="${style.join(';')};" edge="1" parent="${at.parent}"${attach}><mxGeometry relative="1" as="geometry">${pt(a, 'sourcePoint')}${pt(z, 'targetPoint')}${mid ? `<Array as="points">${mid}</Array>` : ''}</mxGeometry></mxCell>`);
  };
  const linePoints = (el: Element): { points: [number, number][]; curved: boolean; closed: boolean } | null => {
    const tag = el.nodeName.toLowerCase();
    if (tag === 'line') return { points: [[numAttr(el, 'x1'), numAttr(el, 'y1')], [numAttr(el, 'x2'), numAttr(el, 'y2')]], curved: false, closed: false };
    if (tag === 'polyline' || tag === 'polygon') {
      const v = (el.getAttribute('points') || '').split(/[\s,]+/).filter(Boolean).map(Number);
      const points: [number, number][] = [];
      for (let k = 0; k + 1 < v.length; k += 2) if (Number.isFinite(v[k]) && Number.isFinite(v[k + 1])) points.push([v[k], v[k + 1]]);
      return { points, curved: false, closed: tag === 'polygon' };
    }
    if (tag === 'path') return pathPoints(el.getAttribute('d') || '');
    return null;
  };

  function walk(el: Element, m: M, at: Place): void {
    const tag = el.nodeName.toLowerCase();
    if (SKIP.has(tag) || unseen(el)) return;
    if (el.hasAttribute('data-background') || el.hasAttribute('data-mask') || (el.getAttribute('data-graph-role') || '').includes('underlay')) return;
    const here = mul(m, transformOf(el.getAttribute('transform')));
    if (el.hasAttribute('data-edge-from') && el.hasAttribute('data-edge-to')) { edges.push({ el, m: here }); return; }
    if (tag === 'g' || tag === 'svg' || tag === 'a') {
      if (el.hasAttribute('data-node-id') && at === PAGE && node(el, here, at)) return;
      for (const c of Array.from(el.children)) walk(c, here, at);
      return;
    }
    if (tag === 'rect' || tag === 'circle' || tag === 'ellipse') {
      const b = boxOf(el, here);
      const [fill, stroke] = [colourOf(el.getAttribute('fill') ?? '#000000'), colourOf(el.getAttribute('stroke'))];
      // Nothing to see, or the page itself behind the drawing.
      if (!b || (fill.hex === 'none' && stroke.hex === 'none') || (b.w >= vw * 0.995 && b.h >= vh * 0.995)) return;
      vertex('', shapeStyle(el), b, at);
      return;
    }
    if (tag === 'text') {
      const value = textOf(el);
      if (!value || (el.hasAttribute('data-edge-text') && edgeLabels.has(value))) return;
      const size = numAttr(el, 'font-size', 12);
      const lines = value.split('\n');
      const w = Math.max(...lines.map((l) => l.length)) * size * 0.62 + 8;
      const h = lines.length * size * 1.3 + 4;
      const first = el.querySelector('tspan[x]');
      const [x, y] = apply(here, numAttr(first ?? el, 'x', numAttr(el, 'x')), numAttr(el, 'y'));
      const anchor = el.getAttribute('text-anchor') || 'start';
      const align = anchor === 'middle' ? 'center' : anchor === 'end' ? 'right' : 'left';
      const left = anchor === 'middle' ? x - w / 2 : anchor === 'end' ? x - w : x;
      vertex(value, ['text', 'html=1', 'strokeColor=none', 'fillColor=none', `align=${align}`, 'verticalAlign=middle', 'whiteSpace=nowrap'].concat(fontStyle(el)), { x: left, y: y - size * 1.05, w, h }, at);
      return;
    }
    const lp = linePoints(el);
    if (!lp || lp.points.length < 2) return;
    const fill = colourOf(el.getAttribute('fill') ?? (tag === 'line' || tag === 'polyline' ? 'none' : '#000000'));
    const stroke = colourOf(el.getAttribute('stroke'));
    if (fill.hex === 'none' && stroke.hex === 'none') return;
    if (stroke.hex === 'none' || (lp.closed && fill.hex !== 'none')) {
      // A filled outline (an icon, an arrow head drawn by hand): its bounds as a plain shape.
      const p = lp.points.map(([x, y]) => apply(here, x, y));
      const [xs, ys] = [p.map((q) => q[0]), p.map((q) => q[1])];
      const b = { x: Math.min(...xs), y: Math.min(...ys), w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) };
      if (b.w < 1 || b.h < 1 || b.w * b.h < 36) return;
      vertex('', ['rounded=0', 'html=1'].concat(paint(el, 'shape')), b, at);
      return;
    }
    line(el, here, lp.closed ? lp.points.concat([lp.points[0]]) : lp.points, lp.curved, at);
  }

  // The connections' own captions are written on the edges, so the loose texts repeating them are left out.
  for (const e of Array.from(svg.querySelectorAll('[data-edge-from][data-edge-to]'))) {
    const l = (e.getAttribute('data-edge-label') || '').trim();
    if (l) edgeLabels.add(l);
  }
  walk(svg, ID, PAGE);
  for (const { el, m } of edges) {
    const shape = ['path', 'line', 'polyline'].includes(el.nodeName.toLowerCase()) ? el : el.querySelector('path, line, polyline');
    const lp = shape && linePoints(shape);
    if (!shape || !lp || lp.points.length < 2) continue;
    const mm = shape === el ? m : mul(m, transformOf(shape.getAttribute('transform')));
    line(shape, mm, lp.points, lp.curved, PAGE, { from: el.getAttribute('data-edge-from') || '', to: el.getAttribute('data-edge-to') || '', label: (el.getAttribute('data-edge-label') || '').trim() });
  }
  const page = xml(name.replace(/\s+/g, ' ').trim().slice(0, 80) || 'Diagram');
  return `<mxfile host="Thoughts" type="device"><diagram id="d1" name="${page}">`
    + `<mxGraphModel dx="${n1(vw)}" dy="${n1(vh)}" grid="1" gridSize="10" guides="1" tooltips="1" connect="1" arrows="1" fold="1" page="0" pageScale="1" pageWidth="${n1(vw)}" pageHeight="${n1(vh)}" math="0" shadow="0">`
    + `<root><mxCell id="0"/><mxCell id="1" parent="0"/>${cells.join('')}</root></mxGraphModel></diagram></mxfile>`;
}
