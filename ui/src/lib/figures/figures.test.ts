// Charts, the draw.io file made from a drawing, and how markdown marks a figure block.
import { describe, expect, it, vi } from 'vitest';
import { chartSvg, formatValue, parseChart, ticks, MAX_LABELS } from './chart';
import { colourOf, pathPoints, svgToDrawio, transformOf } from './drawio';

// The strings come from the app's i18n, which pulls in the whole app store; the keys are enough here.
// (The part that draws on the page is not started: it needs a real browser.)
vi.mock('../i18n', () => ({ t: (k: string) => k }));
vi.mock('./hydrate', () => ({ startFigures: () => {} }));
const { md } = await import('../md');

const spec = (o: object) => { const r = parseChart(JSON.stringify(o)); if ('problem' in r) throw new Error(r.problem.code); return r.spec; };
const problem = (o: unknown) => { const r = parseChart(typeof o === 'string' ? o : JSON.stringify(o)); return 'problem' in r ? r.problem.code : 'ok'; };
const parse = (svg: string) => new DOMParser().parseFromString(svg, 'image/svg+xml');

describe('chart blocks', () => {
  const bar = { type: 'bar', title: 'Revenue', labels: ['Q1', 'Q2', 'Q3'], series: [{ name: '2025', values: [12, 15, null] }, { name: '2026', values: [14, 18, 21] }], y: { label: 'SAR' } };

  it('reads a document in the format and says what is wrong with one that is not', () => {
    expect(spec(bar)).toMatchObject({ type: 'bar', title: 'Revenue', labels: ['Q1', 'Q2', 'Q3'], yLabel: 'SAR', stacked: false, horizontal: false });
    expect(problem('{"type": "bar", "labels": [')).toBe('json');
    expect(problem([1])).toBe('json');
    expect(problem({ ...bar, type: 'radar' })).toBe('type');
    expect(problem({ type: 'bar', labels: ['a'] })).toBe('series');
    expect(problem({ type: 'bar', series: [{ values: [1] }] })).toBe('labels');
    expect(problem({ ...bar, series: [{ name: 'x', values: [1, 2] }] })).toBe('length');
    expect(problem({ ...bar, series: [{ name: 'x', values: [1, '2', 3] }] })).toBe('number');
    expect(problem({ ...bar, series: [{ name: 'x', values: [1, true, 3] }] })).toBe('number');
    expect(problem({ type: 'pie', labels: ['a', 'b'], series: [{ values: [1, -2] }] })).toBe('pie');
    expect(problem({ type: 'pie', labels: ['a'], series: [{ values: [1] }, { values: [2] }] })).toBe('pie');
    expect(problem({ type: 'bar', labels: ['a'], series: [{ values: [null] }] })).toBe('empty');
    expect(problem({ type: 'bar', labels: Array(MAX_LABELS + 1).fill('a'), series: [{ values: Array(MAX_LABELS + 1).fill(1) }] })).toBe('tooMany');
    expect(problem({ type: 'bar', labels: ['a'], series: Array(9).fill({ values: [1] }) })).toBe('tooMany');
    expect(problem({ type: 'scatter', series: [{ name: 'p', points: [[1, 2], [3, 'x']] }] })).toBe('number');
    expect(spec({ type: 'scatter', series: [{ name: 'p', points: [[1, 2], [3, 4]] }] }).series[0].points).toEqual([[1, 2], [3, 4]]);
  });

  it('draws every kind as an SVG that carries its own looks and can run nothing', () => {
    const all: object[] = [bar, { ...bar, stacked: true }, { ...bar, horizontal: true }, { ...bar, type: 'line' }, { ...bar, type: 'area', stacked: true },
      { type: 'pie', title: 'Share', labels: ['A', 'B', 'C'], series: [{ values: [5, 3, 2] }] }, { type: 'donut', labels: ['Only'], series: [{ values: [7] }], unit: '%' },
      { type: 'scatter', series: [{ name: 'p', points: [[1, 2], [3, 4], [5, 1]] }], x: { label: 'x' }, y: { label: 'y' } },
      { type: 'bar', labels: ['a', 'b'], series: [{ values: [-5, 5] }] }, { type: 'line', labels: ['a'], series: [{ values: [3] }] }];
    for (const o of all) {
      const svg = chartSvg(spec(o));
      const doc = parse(svg);
      expect(doc.querySelector('parsererror'), JSON.stringify(o)).toBeNull();
      expect(doc.documentElement.nodeName).toBe('svg');
      expect(svg).not.toMatch(/<script|<style|class=|\son\w+=|href=|NaN|Infinity|undefined/);
      for (const el of Array.from(doc.querySelectorAll('rect, circle, path, polygon'))) expect(el.getAttribute('fill')).toBeTruthy();
    }
  });

  it('starts bars at zero, breaks a line at a missing value and names every slice', () => {
    const doc = parse(chartSvg(spec(bar)));
    const bars = Array.from(doc.querySelectorAll('rect')).filter((r) => !r.hasAttribute('data-background') && Number(r.getAttribute('width')) > 12);
    expect(bars).toHaveLength(5); // six values, one of them missing
    const bottoms = new Set(bars.map((r) => Math.round(Number(r.getAttribute('y')) + Number(r.getAttribute('height')))));
    expect(bottoms.size).toBe(1);
    const line = parse(chartSvg(spec({ type: 'line', labels: ['a', 'b', 'c', 'd', 'e'], series: [{ values: [1, 2, null, 4, 5] }] })));
    expect(line.querySelectorAll('polyline')).toHaveLength(2);
    const pie = chartSvg(spec({ type: 'pie', labels: ['Riyadh', 'Jeddah'], series: [{ values: [3, 1] }] }));
    expect(pie).toContain('Riyadh'); expect(pie).toContain('75%'); expect(pie).toContain('25%');
    // Text from the document is written as text, never as markup.
    expect(chartSvg(spec({ ...bar, title: '<script>x</script> & "q"' }))).toContain('&lt;script&gt;x&lt;/script&gt; &amp; &quot;q&quot;');
  });

  it('writes numbers and picks round steps', () => {
    expect([formatValue(1234.5), formatValue(0.256), formatValue(250000), formatValue(3_400_000), formatValue(12, '%'), formatValue(5, 'SAR')]).toEqual(['1,234.5', '0.26', '250K', '3.4M', '12%', '5 SAR']);
    expect(ticks(0, 24).at).toEqual([0, 5, 10, 15, 20, 25]);
    expect(ticks(-5, 5).at).toContain(0);
    expect(ticks(3, 3).max).toBeGreaterThan(3);
  });
});

describe('a diagram as a draw.io file', () => {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 200" width="400" height="200">
    <defs><marker id="arrow"><polygon points="0 0, 10 3.5, 0 7" fill="#94a3b8"/></marker></defs>
    <rect data-background="" x="0" y="0" width="400" height="200" fill="#020617"/>
    <rect x="10" y="10" width="380" height="180" rx="8" fill="none" stroke="#334155" stroke-dasharray="4 4"/>
    <g data-node-id="api" data-node-label="API &amp; &lt;gateway&gt;" data-node-sublabel="edge">
      <rect data-mask="" x="40" y="60" width="120" height="60" fill="#0f172a"/>
      <rect x="40" y="60" width="120" height="60" rx="6" fill="rgba(52, 211, 153, 0.13)" stroke="color(srgb 0.2 0.8 0.6)" stroke-width="1.25"/>
      <circle cx="52" cy="72" r="4" fill="#34d399"/>
      <text x="100" y="88" font-size="14" font-weight="600" fill="#ffffff" text-anchor="middle">API &amp; &lt;gateway&gt;</text>
    </g>
    <g transform="translate(200 0)"><g data-node-id="db" data-node-label="Postgres"><rect x="40" y="60" width="120" height="60" fill="#1e293b" stroke="#38bdf8"/></g></g>
    <g data-graph-role="automatic-crossover"><path data-graph-role="automatic-crossover-underlay" d="M 160 90 L 240 90" stroke="#020617" stroke-width="6" fill="none"/>
      <path data-edge-id="api-to-db" data-edge-from="api" data-edge-to="db" data-edge-label="SQL" d="M 160 90 L 192 90 Q 200 90 200 98 L 200 100 L 240 100" fill="none" stroke="#94a3b8" stroke-width="1.5" stroke-dasharray="6 3" marker-end="url(#arrow)"/></g>
    <text data-edge-text="" x="200" y="84" font-size="11" fill="#94a3b8" text-anchor="middle">SQL</text>
    <text x="20" y="30" font-size="12" fill="#94a3b8">Region: riyadh</text>
    <line x1="20" y1="150" x2="120" y2="150" stroke="#475569"/>
    <rect x="300" y="150" width="20" height="20" fill="none" display="none" stroke="#fff"/>
  </svg>`;

  it('makes boxes, attaches each connection to its two boxes and keeps what else is drawn', () => {
    const out = svgToDrawio(svg, 'Web <app>');
    const doc = new DOMParser().parseFromString(out, 'application/xml');
    expect(doc.querySelector('parsererror')).toBeNull();
    expect(doc.documentElement.nodeName).toBe('mxfile');
    expect(doc.querySelector('diagram')!.getAttribute('name')).toBe('Web <app>');
    const cells = Array.from(doc.querySelectorAll('mxCell'));
    const byValue = (v: string) => cells.find((c) => c.getAttribute('value') === v)!;
    // The two boxes, by their names; the second one moved by its group.
    const api = byValue('API & <gateway>\nedge'), db = byValue('Postgres');
    expect(api.getAttribute('vertex')).toBe('1');
    expect(api.getAttribute('style')).toMatch(/rounded=1;.*fillColor=#34d399;fillOpacity=13;strokeColor=#33cc99;strokeWidth=1\.3;.*fontSize=14;fontColor=#ffffff;fontStyle=1/);
    expect(db.querySelector('mxGeometry')!.getAttribute('x')).toBe('240');
    // One box per node: the mask behind it, its badge and its own text are not shapes of their own.
    expect(cells.filter((c) => c.getAttribute('vertex') === '1')).toHaveLength(4); // api, db, the frame, the caption
    // The connection: joined to both, dashed, with its arrow, its caption and the points it passes.
    const edge = byValue('SQL');
    expect([edge.getAttribute('edge'), edge.getAttribute('source'), edge.getAttribute('target')]).toEqual(['1', api.getAttribute('id'), db.getAttribute('id')]);
    expect(edge.getAttribute('style')).toMatch(/endArrow=block;.*rounded=1;.*strokeColor=#94a3b8;.*dashed=1/);
    expect(edge.querySelectorAll('Array > mxPoint')).toHaveLength(3);
    expect(cells.filter((c) => c.getAttribute('value') === 'SQL')).toHaveLength(1); // its loose caption is not repeated
    // The frame, the loose line and the caption are kept; the page behind, the underlay and what is hidden are not.
    expect(byValue('Region: riyadh').getAttribute('style')).toContain('text;');
    expect(cells.filter((c) => c.getAttribute('edge') === '1')).toHaveLength(2);
    expect(out).not.toContain('#020617');
  });

  it('reads paths, transforms and colours', () => {
    expect(pathPoints('M 10 20 L 30 20 Q 40 20 40 30 V 60 h -10 Z')).toEqual({ points: [[10, 20], [30, 20], [40, 30], [40, 60], [30, 60]], curved: true, closed: true });
    expect(pathPoints('M0,0 10,0 10,10').points).toEqual([[0, 0], [10, 0], [10, 10]]);
    expect(pathPoints('m 5 5 l 5 0 c 1 1 2 2 3 3').points).toEqual([[5, 5], [10, 5], [13, 8]]);
    expect(pathPoints('M 1 2 L nonsense').points).toEqual([[1, 2]]);
    expect(transformOf('translate(10, 5) scale(2)')).toEqual([2, 0, 0, 2, 10, 5]);
    expect(transformOf('rotate(45)')).toEqual([1, 0, 0, 1, 0, 0]);
    expect(colourOf('rgb(255, 0, 128)')).toEqual({ hex: '#ff0080', alpha: 1 });
    expect(colourOf('rgba(0 0 0 / 50%)')).toEqual({ hex: '#000000', alpha: 0.5 });
    expect(colourOf('color(srgb 1 0.5 0 / 0.25)')).toEqual({ hex: '#ff8000', alpha: 0.25 });
    expect(colourOf('#abc')).toEqual({ hex: '#aabbcc', alpha: 1 });
    expect([colourOf('none').hex, colourOf('url(#grid)').hex, colourOf(null).hex, colourOf('expression(1)').hex]).toEqual(['none', 'none', 'none', 'none']);
    expect(() => svgToDrawio('<html/>')).toThrow();
  });
});

describe('figure blocks in markdown', () => {
  const figures = (html: string) => { const d = document.createElement('div'); d.innerHTML = html; return Array.from(d.querySelectorAll('.md-fig')); };

  it('stamps a diagram or chart block and leaves its source in it', () => {
    const out = figures(md('Here:\n\n```archify\n{"diagram_type":"tree"}\n```\n\n```chart\n{"type":"bar"}\n```\n\n```json\n{"type":"bar"}\n```\n'));
    expect(out.map((f) => f.getAttribute('data-fig-kind'))).toEqual(['archify', 'chart']);
    expect(out[0].querySelector('pre')!.textContent).toContain('"diagram_type":"tree"');
    expect(out[0].getAttribute('data-md-fig')).toMatch(/^[0-9a-f]{24}$/);
    expect(out[0].getAttribute('data-md-fig')).toBe(out[1].getAttribute('data-md-fig'));
  });

  it('cannot be forged by markup written in a note', () => {
    const mark = figures(md('```chart\n{}\n```'))[0].getAttribute('data-md-fig')!;
    const forged = md(`<div class="md-fig" data-md-fig="${mark}" data-fig-kind="chart" data-fig-state="drawn"><pre>{}</pre></div>\n\n<img class="fig-img" src="x">`);
    expect(figures(forged)).toHaveLength(0);
    expect(forged).not.toMatch(/data-md-fig|data-fig-kind|data-fig-state|class="fig/);
  });
});
