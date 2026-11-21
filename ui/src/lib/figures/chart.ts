// Charts of numbers, drawn by the app itself (Archify draws diagrams, not data charts). A fenced
// block with the language `chart` holds one JSON document (the format is in vendor/archify/GUIDE.md,
// which is also what the assistant reads); `chartSvg` turns it into a self-contained SVG: every
// colour, size and font is written on the element, there is no script, no style sheet and nothing
// loaded from anywhere, so the same text is the picture shown and the file saved.
// Pure: no DOM, no I/O.

export type ChartType = 'bar' | 'line' | 'area' | 'pie' | 'donut' | 'scatter';
export interface Series { name: string; values: (number | null)[]; points: [number, number][] }
export interface ChartSpec {
  type: ChartType; title: string; subtitle: string; labels: string[]; series: Series[];
  xLabel: string; yLabel: string; yMin?: number; yMax?: number; stacked: boolean; horizontal: boolean; unit: string;
}
/** Why a block is not a chart the app can draw: a key of the `figures.chart.*` strings, with what it names. */
export interface ChartProblem { code: 'json' | 'type' | 'series' | 'labels' | 'length' | 'number' | 'pie' | 'tooMany' | 'empty'; at?: string }

export const CHART_TYPES: ChartType[] = ['bar', 'line', 'area', 'pie', 'donut', 'scatter'];
export const MAX_SERIES = 8;
export const MAX_LABELS = 60;
export const MAX_POINTS = 2000;

const text = (v: unknown, max = 200) => (typeof v === 'string' || typeof v === 'number' ? String(v).replace(/\s+/g, ' ').trim().slice(0, max) : '');
const num = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);

/** Read a chart block. Nothing is guessed: a document that is not in the format says what is wrong with it. */
export function parseChart(src: string): { spec: ChartSpec } | { problem: ChartProblem } {
  let doc: any;
  try { doc = JSON.parse(src); } catch { return { problem: { code: 'json' } }; }
  if (!doc || typeof doc !== 'object' || Array.isArray(doc)) return { problem: { code: 'json' } };
  const type = String(doc.type ?? '').toLowerCase() as ChartType;
  if (!CHART_TYPES.includes(type)) return { problem: { code: 'type', at: text(doc.type, 40) } };
  if (!Array.isArray(doc.series) || doc.series.length === 0) return { problem: { code: 'series' } };
  if (doc.series.length > MAX_SERIES) return { problem: { code: 'tooMany', at: String(MAX_SERIES) } };
  const scatter = type === 'scatter';
  const labels: string[] = scatter ? [] : Array.isArray(doc.labels) ? doc.labels.map((l: unknown) => text(l, 80)) : [];
  if (!scatter && labels.length === 0) return { problem: { code: 'labels' } };
  if (labels.length > MAX_LABELS) return { problem: { code: 'tooMany', at: String(MAX_LABELS) } };
  const series: Series[] = [];
  let count = 0;
  for (let i = 0; i < doc.series.length; i++) {
    const s = doc.series[i];
    const name = text(s?.name, 80) || String(i + 1);
    if (scatter) {
      if (!Array.isArray(s?.points)) return { problem: { code: 'series', at: name } };
      const points: [number, number][] = [];
      for (const p of s.points) {
        if (!Array.isArray(p) || !num(p[0]) || !num(p[1])) return { problem: { code: 'number', at: name } };
        points.push([p[0], p[1]]);
      }
      count += points.length;
      series.push({ name, values: [], points });
    } else {
      if (!Array.isArray(s?.values)) return { problem: { code: 'series', at: name } };
      if (s.values.length !== labels.length) return { problem: { code: 'length', at: name } };
      const values: (number | null)[] = [];
      for (const v of s.values) {
        if (v === null) values.push(null);
        else if (num(v)) values.push(v);
        else return { problem: { code: 'number', at: name } };
      }
      count += values.length;
      series.push({ name, values, points: [] });
    }
  }
  if (count > MAX_POINTS) return { problem: { code: 'tooMany', at: String(MAX_POINTS) } };
  if (count === 0 || series.every((s) => s.values.every((v) => v === null) && s.points.length === 0)) return { problem: { code: 'empty' } };
  if (type === 'pie' || type === 'donut') {
    if (series.length !== 1 || series[0].values.some((v) => v !== null && v < 0)) return { problem: { code: 'pie' } };
    if (!series[0].values.some((v) => (v ?? 0) > 0)) return { problem: { code: 'empty' } };
  }
  const axis = (a: any) => (a && typeof a === 'object' ? a : {});
  const [x, y] = [axis(doc.x), axis(doc.y)];
  return {
    spec: {
      type, title: text(doc.title), subtitle: text(doc.subtitle), labels, series,
      xLabel: text(x.label, 80), yLabel: text(y.label, 80),
      yMin: num(y.min) ? y.min : undefined, yMax: num(y.max) ? y.max : undefined,
      stacked: doc.stacked === true && (type === 'bar' || type === 'area'),
      horizontal: doc.horizontal === true && type === 'bar', unit: text(doc.unit, 12),
    },
  };
}

/** The colours a chart is drawn with: the app's own for text and lines, and one per series. */
export interface Look { fg: string; muted: string; grid: string; bg: string; series: string[]; font: string }

// One colour per series, told apart without relying on red against green (after Okabe and Ito),
// in a set for light surfaces and one for dark. They are data marks, not part of the app's theme.
const SERIES_LIGHT = ['#0b6fb8', '#d55e00', '#009e73', '#b0439c', '#8a6d00', '#3aa3d9', '#a02c2c', '#555f6b'];
const SERIES_DARK = ['#56b4e9', '#ff9d4d', '#3ddbb0', '#e08bd0', '#f0d24a', '#9ecff2', '#ff8a80', '#b8c2cc'];
export const seriesColours = (dark: boolean) => (dark ? SERIES_DARK : SERIES_LIGHT);
export const DEFAULT_LOOK: Look = { fg: '#1a1a1a', muted: '#5c5c5c', grid: '#dcdcdc', bg: '#ffffff', series: SERIES_LIGHT, font: 'system-ui, -apple-system, "Segoe UI", sans-serif' };

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const r2 = (n: number) => String(Math.round(n * 100) / 100);

/** A number as it is written on a chart: at most two decimals, thousands grouped, big ones shortened. */
export function formatValue(n: number, unit = ''): string {
  const a = Math.abs(n);
  let s: string;
  if (a >= 1e9) s = trim(n / 1e9) + 'B';
  else if (a >= 1e6) s = trim(n / 1e6) + 'M';
  else if (a >= 1e5) s = trim(n / 1e3) + 'K';
  else s = group(trim(n));
  return unit ? (/^[%°]/.test(unit) ? s + unit : `${s} ${unit}`) : s;
}
const trim = (n: number) => String(Math.round(n * 100) / 100);
const group = (s: string) => { const [i, d] = s.split('.'); return i.replace(/\B(?=(\d{3})+(?!\d))/g, ',') + (d ? '.' + d : ''); };

/** Round steps along an axis from `lo` to `hi` (about five of them), and where the axis then ends. */
export function ticks(lo: number, hi: number, want = 5): { min: number; max: number; step: number; at: number[] } {
  if (!(hi > lo)) { hi = lo + (lo === 0 ? 1 : Math.abs(lo) * 0.5); }
  const raw = (hi - lo) / want;
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? 10 * mag;
  const min = Math.floor(lo / step + 1e-9) * step;
  const max = Math.ceil(hi / step - 1e-9) * step;
  const at: number[] = [];
  for (let v = min; v <= max + step * 1e-6; v += step) at.push(Math.round(v / step) * step);
  return { min, max, step, at };
}

/** Roughly how wide `s` is at `size` px (no measuring is possible without a page; this errs wide). */
const widthOf = (s: string, size: number) => { let w = 0; for (const ch of s) w += /[؀-ۿ֐-׿]/.test(ch) ? 0.52 : /[A-Z0-9MW@%]/.test(ch) ? 0.64 : /[il.,:;|' ]/.test(ch) ? 0.3 : 0.54; return w * size; };
/** `s` cut to fit `max` px at `size`, with an ellipsis. */
function fit(s: string, max: number, size: number): string {
  if (widthOf(s, size) <= max) return s;
  let out = s;
  while (out.length > 1 && widthOf(out + '…', size) > max) out = out.slice(0, -1);
  return out.trimEnd() + '…';
}

const W = 720;
/** Does the text start with a letter written right to left? */
const RTL = /^[^\p{L}]*[\u0590-\u08ff\ufb1d-\ufdff\ufe70-\ufefc]/u;

/** The chart as a self-contained SVG. */
export function chartSvg(spec: ChartSpec, look: Look = DEFAULT_LOOK): string {
  const colour = (i: number) => look.series[i % look.series.length];
  const T = (x: number, y: number, s: string, o: { size?: number; fill?: string; anchor?: string; weight?: number; rotate?: number } = {}) =>
    `<text x="${r2(x)}" y="${r2(y)}" font-family="${esc(look.font)}" font-size="${o.size ?? 12}" font-weight="${o.weight ?? 400}" fill="${o.fill ?? look.muted}" text-anchor="${o.anchor ?? 'start'}"${o.rotate ? ` transform="rotate(${o.rotate} ${r2(x)} ${r2(y)})"` : ''}>${esc(s)}</text>`;
  const out: string[] = [];
  let top = 20;
  // A heading in Arabic or Hebrew starts from the right, as it is read.
  const heading = (s: string, y: number, o: { size: number; weight?: number; fill?: string }) =>
    RTL.test(s) ? T(W - 20, y, s, o).replace('<text ', '<text direction="rtl" ') : T(20, y, s, o);
  if (spec.title) { out.push(heading(fit(spec.title, W - 40, 16), top + 16, { size: 16, weight: 600, fill: look.fg })); top += 28; }
  if (spec.subtitle) { out.push(heading(fit(spec.subtitle, W - 40, 12.5), top + 12, { size: 12.5 })); top += 22; }
  const pie = spec.type === 'pie' || spec.type === 'donut';
  // The key: which colour is which series (one series needs none; a pie's key is its slices, below).
  if (!pie && spec.series.length > 1) {
    let x = 20, y = top + 14;
    for (let i = 0; i < spec.series.length; i++) {
      const name = fit(spec.series[i].name, 180, 12);
      const w = 18 + widthOf(name, 12) + 18;
      if (x + w > W - 20 && x > 20) { x = 20; y += 20; }
      out.push(`<rect x="${r2(x)}" y="${r2(y - 9)}" width="10" height="10" rx="2" fill="${colour(i)}"/>`, T(x + 16, y, name, { fill: look.fg }));
      x += w;
    }
    top = y + 14;
  }
  const H = pie ? top + 300 : top + (spec.horizontal ? Math.max(220, spec.labels.length * (spec.stacked || spec.series.length === 1 ? 30 : 14 + spec.series.length * 12) + 60) : 340);
  if (pie) out.push(pieMarks(spec, look, top, T));
  else if (spec.type === 'scatter') out.push(scatterMarks(spec, look, top, H, T, colour));
  else out.push(axisMarks(spec, look, top, H, T, colour));
  const label = esc(spec.title || spec.type);
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="${label}"><title>${label}</title>`
    + `<rect data-background="" x="0" y="0" width="${W}" height="${H}" fill="${look.bg}"/>${out.join('')}</svg>`;
}

type TextFn = (x: number, y: number, s: string, o?: { size?: number; fill?: string; anchor?: string; weight?: number; rotate?: number }) => string;

/** Bars, lines and areas: categories along one axis, values along the other. */
function axisMarks(spec: ChartSpec, look: Look, top: number, H: number, T: TextFn, colour: (i: number) => string): string {
  const out: string[] = [];
  const n = spec.labels.length;
  const horizontal = spec.horizontal;
  // The range of the values (stacked: of the sums, above and below zero apart).
  let lo = Infinity, hi = -Infinity;
  if (spec.stacked) {
    for (let j = 0; j < n; j++) {
      let pos = 0, neg = 0;
      for (const s of spec.series) { const v = s.values[j] ?? 0; if (v >= 0) pos += v; else neg += v; }
      hi = Math.max(hi, pos); lo = Math.min(lo, neg);
    }
  } else for (const s of spec.series) for (const v of s.values) if (v !== null) { lo = Math.min(lo, v); hi = Math.max(hi, v); }
  // Bars and areas are read by their length, so their axis includes zero; a line may start elsewhere.
  if (spec.type !== 'line') { lo = Math.min(lo, 0); hi = Math.max(hi, 0); }
  if (spec.yMin !== undefined && (spec.type === 'line' || spec.yMin <= 0)) lo = spec.yMin;
  if (spec.yMax !== undefined && spec.yMax > lo) hi = spec.yMax;
  const tk = ticks(lo, hi);
  const tickText = tk.at.map((v) => formatValue(v, spec.unit));
  const catW = Math.min(170, Math.max(...spec.labels.map((l) => widthOf(l, 12))) + 12);
  const left = 20 + (horizontal ? catW : Math.max(...tickText.map((s) => widthOf(s, 11.5))) + 10) + ((horizontal ? spec.xLabel : spec.yLabel) ? 18 : 0);
  const right = W - 24;
  const bottom = H - 30 - ((horizontal ? spec.yLabel : spec.xLabel) ? 20 : 0);
  const plotTop = top + 10;
  const [pw, ph] = [right - left, bottom - plotTop];
  const vPos = (v: number) => (horizontal ? left + ((v - tk.min) / (tk.max - tk.min)) * pw : bottom - ((v - tk.min) / (tk.max - tk.min)) * ph);
  const band = (horizontal ? ph : pw) / n;
  const cPos = (j: number) => (horizontal ? plotTop : left) + band * (j + 0.5);
  // Grid lines and the numbers beside them.
  tk.at.forEach((v, i) => {
    const p = vPos(v);
    const zero = Math.abs(v) < tk.step * 1e-6;
    if (horizontal) {
      out.push(`<line x1="${r2(p)}" y1="${r2(plotTop)}" x2="${r2(p)}" y2="${r2(bottom)}" stroke="${zero ? look.muted : look.grid}" stroke-width="1"/>`, T(p, bottom + 16, tickText[i], { size: 11.5, anchor: 'middle' }));
    } else {
      out.push(`<line x1="${r2(left)}" y1="${r2(p)}" x2="${r2(right)}" y2="${r2(p)}" stroke="${zero ? look.muted : look.grid}" stroke-width="1"/>`, T(left - 8, p + 4, tickText[i], { size: 11.5, anchor: 'end' }));
    }
  });
  // The names of the categories: every one when they fit, otherwise every second, third…
  if (horizontal) {
    spec.labels.forEach((l, j) => out.push(T(left - 8, cPos(j) + 4, fit(l, catW - 8, 12), { anchor: 'end', fill: look.fg })));
  } else {
    const longest = Math.max(...spec.labels.map((l) => widthOf(l, 11.5)));
    const room = Math.max(band - 6, 26);
    const every = longest <= room ? 1 : Math.ceil(Math.min(longest, 90) / room);
    spec.labels.forEach((l, j) => { if (j % every === 0) out.push(T(cPos(j), bottom + 16, fit(l, Math.max(room * every, 30), 11.5), { size: 11.5, anchor: 'middle', fill: look.fg })); });
  }
  // What the axes measure.
  const [valueLabel, catLabel] = [spec.yLabel, spec.xLabel];
  if (horizontal) {
    if (valueLabel) out.push(T(left + pw / 2, H - 10, fit(valueLabel, pw, 12), { anchor: 'middle' }));
    if (catLabel) out.push(T(14, plotTop + ph / 2, fit(catLabel, ph, 12), { anchor: 'middle', rotate: -90 }));
  } else {
    if (catLabel) out.push(T(left + pw / 2, H - 10, fit(catLabel, pw, 12), { anchor: 'middle' }));
    if (valueLabel) out.push(T(14, plotTop + ph / 2, fit(valueLabel, ph, 12), { anchor: 'middle', rotate: -90 }));
  }
  const zeroAt = vPos(Math.min(Math.max(0, tk.min), tk.max));
  const k = spec.series.length;
  if (spec.type === 'bar') {
    const inner = band * (n > 30 ? 0.86 : 0.72);
    const each = spec.stacked ? inner : inner / k;
    const direct = k === 1 && n <= 12;
    for (let j = 0; j < n; j++) {
      let pos = 0, neg = 0;
      for (let i = 0; i < k; i++) {
        const v = spec.series[i].values[j];
        if (v === null) continue;
        let from = zeroAt, to = vPos(v);
        if (spec.stacked) {
          if (v >= 0) { from = vPos(pos); pos += v; to = vPos(pos); } else { from = vPos(neg); neg += v; to = vPos(neg); }
        }
        const start = cPos(j) - inner / 2 + (spec.stacked ? 0 : each * i);
        const thick = Math.max(each - (spec.stacked || k === 1 ? 0 : 1.5), 1);
        const [a, len] = [Math.min(from, to), Math.max(Math.abs(to - from), v === 0 ? 0 : 1)];
        out.push(horizontal
          ? `<rect x="${r2(a)}" y="${r2(start)}" width="${r2(len)}" height="${r2(thick)}" rx="${thick > 6 ? 2 : 0}" fill="${colour(i)}"/>`
          : `<rect x="${r2(start)}" y="${r2(a)}" width="${r2(thick)}" height="${r2(len)}" rx="${thick > 6 ? 2 : 0}" fill="${colour(i)}"/>`);
        if (direct) {
          const s = formatValue(v, spec.unit);
          out.push(horizontal
            ? T(v >= 0 ? to + 5 : to - 5, cPos(j) + 4, s, { size: 11.5, anchor: v >= 0 ? 'start' : 'end', fill: look.fg })
            : T(cPos(j), v >= 0 ? to - 5 : to + 14, s, { size: 11.5, anchor: 'middle', fill: look.fg }));
        }
      }
    }
    return out.join('');
  }
  // Lines and areas. A missing value breaks the line; stacked areas sit on the ones before them.
  const base = new Array(n).fill(0);
  for (let i = 0; i < k; i++) {
    const vals = spec.series[i].values;
    const runs: { j: number; y: number; y0: number }[][] = [];
    let run: { j: number; y: number; y0: number }[] = [];
    for (let j = 0; j < n; j++) {
      const v = vals[j];
      if (v === null) { if (run.length) runs.push(run); run = []; continue; }
      const from = spec.stacked ? base[j] : 0;
      run.push({ j, y: vPos(from + v), y0: vPos(Math.min(Math.max(from, tk.min), tk.max)) });
      if (spec.stacked) base[j] += v;
    }
    if (run.length) runs.push(run);
    for (const pts of runs) {
      const line = pts.map((p) => `${r2(cPos(p.j))},${r2(p.y)}`).join(' ');
      if (spec.type === 'area') {
        const back = pts.slice().reverse().map((p) => `${r2(cPos(p.j))},${r2(p.y0)}`).join(' ');
        out.push(`<polygon points="${line} ${back}" fill="${colour(i)}" fill-opacity="${spec.stacked ? 0.55 : 0.22}" stroke="none"/>`);
      }
      if (pts.length > 1) out.push(`<polyline points="${line}" fill="none" stroke="${colour(i)}" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>`);
      if (pts.length === 1 || n <= 30) for (const p of pts) out.push(`<circle cx="${r2(cPos(p.j))}" cy="${r2(p.y)}" r="${pts.length === 1 ? 3.5 : 3}" fill="${colour(i)}" stroke="${look.bg}" stroke-width="1.5"/>`);
    }
  }
  return out.join('');
}

/** Points on two number axes. */
function scatterMarks(spec: ChartSpec, look: Look, top: number, H: number, T: TextFn, colour: (i: number) => string): string {
  const out: string[] = [];
  const all = spec.series.flatMap((s) => s.points);
  const span = (vals: number[]) => { const [a, b] = [Math.min(...vals), Math.max(...vals)]; const pad = (b - a || Math.abs(a) || 1) * 0.06; return ticks(a - pad, b + pad); };
  const [tx, ty] = [span(all.map((p) => p[0])), span(all.map((p) => p[1]))];
  const yText = ty.at.map((v) => formatValue(v, spec.unit));
  const left = 20 + Math.max(...yText.map((s) => widthOf(s, 11.5))) + 10 + (spec.yLabel ? 18 : 0);
  const right = W - 24, plotTop = top + 10, bottom = H - 30 - (spec.xLabel ? 20 : 0);
  const X = (v: number) => left + ((v - tx.min) / (tx.max - tx.min)) * (right - left);
  const Y = (v: number) => bottom - ((v - ty.min) / (ty.max - ty.min)) * (bottom - plotTop);
  ty.at.forEach((v, i) => out.push(`<line x1="${r2(left)}" y1="${r2(Y(v))}" x2="${r2(right)}" y2="${r2(Y(v))}" stroke="${look.grid}" stroke-width="1"/>`, T(left - 8, Y(v) + 4, yText[i], { size: 11.5, anchor: 'end' })));
  tx.at.forEach((v) => out.push(`<line x1="${r2(X(v))}" y1="${r2(plotTop)}" x2="${r2(X(v))}" y2="${r2(bottom)}" stroke="${look.grid}" stroke-width="1"/>`, T(X(v), bottom + 16, formatValue(v), { size: 11.5, anchor: 'middle' })));
  if (spec.xLabel) out.push(T((left + right) / 2, H - 10, fit(spec.xLabel, right - left, 12), { anchor: 'middle' }));
  if (spec.yLabel) out.push(T(14, (plotTop + bottom) / 2, fit(spec.yLabel, bottom - plotTop, 12), { anchor: 'middle', rotate: -90 }));
  spec.series.forEach((s, i) => { for (const [x, y] of s.points) out.push(`<circle cx="${r2(X(x))}" cy="${r2(Y(y))}" r="4" fill="${colour(i)}" fill-opacity="0.8" stroke="${look.bg}" stroke-width="1"/>`); });
  return out.join('');
}

/** A pie or a donut, with its slices named beside it (largest share first is the writer's choice: the order given is kept). */
function pieMarks(spec: ChartSpec, look: Look, top: number, T: TextFn): string {
  const out: string[] = [];
  const vals = spec.series[0].values.map((v) => v ?? 0);
  const total = vals.reduce((a, b) => a + b, 0);
  const [cx, cy, R] = [170, top + 150, 125];
  const inner = spec.type === 'donut' ? R * 0.58 : 0;
  const colour = (i: number) => look.series[i % look.series.length];
  let angle = -Math.PI / 2;
  const at = (a: number, r: number) => `${r2(cx + Math.cos(a) * r)} ${r2(cy + Math.sin(a) * r)}`;
  vals.forEach((v, i) => {
    if (v <= 0) return;
    const sweep = (v / total) * Math.PI * 2;
    if (sweep >= Math.PI * 2 - 1e-9) {
      // One slice is the whole: a ring cannot be drawn as a single arc.
      out.push(`<circle cx="${cx}" cy="${cy}" r="${r2((R + inner) / 2)}" fill="none" stroke="${colour(i)}" stroke-width="${r2(R - inner)}"/>`);
    } else {
      const [a0, a1, big] = [angle, angle + sweep, sweep > Math.PI ? 1 : 0];
      out.push(inner
        ? `<path d="M ${at(a0, R)} A ${R} ${R} 0 ${big} 1 ${at(a1, R)} L ${at(a1, inner)} A ${r2(inner)} ${r2(inner)} 0 ${big} 0 ${at(a0, inner)} Z" fill="${colour(i)}" stroke="${look.bg}" stroke-width="1.5"/>`
        : `<path d="M ${cx} ${cy} L ${at(a0, R)} A ${R} ${R} 0 ${big} 1 ${at(a1, R)} Z" fill="${colour(i)}" stroke="${look.bg}" stroke-width="1.5"/>`);
    }
    angle += sweep;
  });
  if (inner) out.push(T(cx, cy + 6, formatValue(total, spec.unit), { size: 18, weight: 600, anchor: 'middle', fill: look.fg }));
  // The slices by name, with their value and share.
  const rows = vals.length;
  const gap = Math.min(24, 280 / rows);
  const y0 = cy - (gap * (rows - 1)) / 2;
  const small = gap < 18;
  vals.forEach((v, i) => {
    const y = y0 + gap * i;
    const share = total ? Math.round((v / total) * 1000) / 10 : 0;
    // Values given in percent are their own share: said once.
    const figure = spec.unit === '%' ? formatValue(v, '%') : `${formatValue(v, spec.unit)} · ${share}%`;
    out.push(`<rect x="340" y="${r2(y - 9)}" width="10" height="10" rx="2" fill="${colour(i)}"/>`,
      T(358, y, fit(spec.labels[i] ?? '', 210, small ? 11 : 12.5), { size: small ? 11 : 12.5, fill: look.fg }),
      T(W - 20, y, figure, { size: small ? 11 : 12, anchor: 'end' }));
  });
  return out.join('');
}
