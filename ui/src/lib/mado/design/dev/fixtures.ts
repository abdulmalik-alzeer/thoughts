// Dev only: made-up styles and two mock-ups, one friendly and one hostile, and a stand-in for the daemon.
import type { DesignTokens, ScreenInfo, StyleCandidate } from '../../types';
import type { DesignClient } from '../client';
import { FALLBACK } from '../tokens';

const t = (color: Partial<DesignTokens['color']>, rest: Partial<DesignTokens> = {}): DesignTokens =>
  ({ ...FALLBACK, ...rest, color: { ...FALLBACK.color, ...color }, font: { ...FALLBACK.font, ...(rest.font ?? {}) } });

export const STYLES: StyleCandidate[] = [
  { id: 'app-calm', name: 'Calm neutral', description: 'Quiet greys, one steady blue and plenty of air.', source: 'app', tokens: t({}) },
  { id: 'app-editorial', name: 'Bold editorial', description: 'Black on warm paper, sharp corners, tall serif headlines.', source: 'app',
    tokens: t({ bg: '#fffdf8', text: '#111111', muted: '#555555', primary: '#111111', accent: '#d12b2b', border: '#111111' }, { font: { heading: 'Didone', body: 'Transitional', mono: 'Typewriter' }, radius: 0, spacing: 10, density: 'roomy', shadow: 'none' }) },
  { id: 'app-terminal', name: 'Dark technical', description: 'Near-black panels, signal green and clear blue.', source: 'app',
    tokens: t({ bg: '#0d1117', surface: '#161b22', text: '#e6edf3', muted: '#9da7b3', primary: '#3fb950', onPrimary: '#04210c', accent: '#58a6ff', border: '#30363d', success: '#3fb950', warning: '#d29922', danger: '#f85149' }, { font: { heading: 'Grotesque', body: 'System', mono: 'Mono' }, radius: 6, shadow: 'none' }) },
  { id: 'app-warm', name: 'Warm friendly', description: 'Cream and terracotta, round corners and soft letters.', source: 'app',
    tokens: t({ bg: '#fff8f0', text: '#3b2a20', muted: '#75594a', primary: '#c2410c', accent: '#0f766e', border: '#f0dccb' }, { font: { heading: 'Rounded', body: 'Humanist', mono: 'Mono' }, radius: 16, spacing: 9, density: 'roomy' }) },
  { id: 'harbour', name: 'Harbour', description: 'Suggested for a booking app by the sea: deep teal and sand.', source: 'assistant',
    tokens: t({ bg: '#f4efe6', surface: '#fffaf2', text: '#12303a', muted: '#4a6670', primary: '#0b5c6b', accent: '#b4532a', border: '#e0d6c3' }, { font: { heading: 'Slab', body: 'Humanist', mono: 'Mono' }, radius: 10 }) },
  { id: 'night-market', name: 'Night market', description: 'Suggested: lantern orange on indigo.', source: 'assistant',
    tokens: t({ bg: '#141228', surface: '#1e1b3a', text: '#f2efff', muted: '#aaa5cf', primary: '#ff9f43', onPrimary: '#2a1500', accent: '#7ee0c3', border: '#35315c', success: '#7ee0a0', warning: '#ffd166', danger: '#ff7b7b' }, { font: { heading: 'Geometric', body: 'Geometric', mono: 'Mono' }, radius: 14, shadow: 'strong' }) },
];

/** As the daemon's cleaner would hand it out: style blocks, then one body. */
export const FRIENDLY = `<style>
.app{display:grid;grid-template-columns:220px 1fr;min-height:100vh}
.side{background:var(--mado-color-surface);border-right:1px solid var(--mado-color-border);padding:calc(var(--mado-spacing)*2)}
.side b{font-family:var(--mado-font-heading);font-size:20px}
.side a{display:block;padding:var(--mado-spacing);border-radius:var(--mado-radius);color:var(--mado-color-muted);text-decoration:none}
.side a.on{background:var(--mado-color-primary);color:var(--mado-color-on-primary)}
.main{padding:calc(var(--mado-spacing)*3)}
h1{font-family:var(--mado-font-heading);margin:0 0 var(--mado-spacing)}
.cards{display:grid;grid-template-columns:repeat(3,1fr);gap:calc(var(--mado-spacing)*2);margin:calc(var(--mado-spacing)*2) 0}
.card{background:var(--mado-color-surface);border:1px solid var(--mado-color-border);border-radius:var(--mado-radius);box-shadow:var(--mado-shadow);padding:calc(var(--mado-spacing)*2)}
.card small{color:var(--mado-color-muted)}.card strong{display:block;font-size:26px;font-family:var(--mado-font-heading)}
.ok{color:var(--mado-color-success)}.warn{color:var(--mado-color-warning)}.bad{color:var(--mado-color-danger)}
table{width:100%;border-collapse:collapse;background:var(--mado-color-surface);border-radius:var(--mado-radius)}
td,th{padding:var(--mado-spacing);border-bottom:1px solid var(--mado-color-border);text-align:start}
th{color:var(--mado-color-muted);font-weight:600}code{font-family:var(--mado-font-mono)}
button{font:inherit;background:var(--mado-color-primary);color:var(--mado-color-on-primary);border:0;border-radius:var(--mado-radius);padding:var(--mado-spacing) calc(var(--mado-spacing)*2)}
input{font:inherit;padding:var(--mado-spacing);border:1px solid var(--mado-color-border);border-radius:var(--mado-radius);background:var(--mado-color-bg);color:var(--mado-color-text)}
@media (max-width:700px){.app{grid-template-columns:1fr}.side{display:none}.cards{grid-template-columns:1fr}}
</style><body><div class="app"><nav class="side"><b>Harbour</b><a class="on" href="#home">Bookings</a><a href="#boats">Boats</a><a href="#people">People</a></nav>
<main class="main"><h1>Bookings</h1><p>Three boats go out today. <a href="#all">See the week</a></p>
<div class="cards"><div class="card"><small>Today</small><strong>12</strong><span class="ok">All paid</span></div><div class="card"><small>Waiting</small><strong>3</strong><span class="warn">Need an answer</span></div><div class="card"><small>Cancelled</small><strong>1</strong><span class="bad">Refund due</span></div></div>
<p><input type="search" placeholder="Find a booking"> <button type="button">New booking</button></p>
<table><thead><tr><th>Booking</th><th>Guest</th><th>Boat</th></tr></thead><tbody><tr><td><code>#2041</code></td><td>Sara A.</td><td>Mistral</td></tr><tr><td><code>#2042</code></td><td>Omar K.</td><td>Levante</td></tr><tr><td><code>#2043</code></td><td>Lina H.</td><td>Sirocco</td></tr></tbody></table>
</main></div></body>`;

/** NOT cleaned, on purpose: everything a mock-up could try. The demo shows it raw in the sealed
 *  frame to prove the frame and its policy hold it; the real step refuses it (it does not look cleaned). */
export const HOSTILE = `</iframe></style></head><meta http-equiv="Content-Security-Policy" content="default-src * 'unsafe-inline' 'unsafe-eval'">
<meta http-equiv="refresh" content="1;url=http://127.0.0.1:9/mado-hostile-refresh">
<base href="http://127.0.0.1:9/mado-hostile-base/"><link rel="stylesheet" href="http://127.0.0.1:9/mado-hostile.css">
<style>@import url(http://127.0.0.1:9/mado-hostile-import.css); body{background:url(http://127.0.0.1:9/mado-hostile-bg.png)} #marker{color:red}</style>
<body onload="parent.postMessage('mado-hostile:onload','*')">
<h1 id="marker">Hostile mock-up (shown raw)</h1>
<p>If the frame holds, this page is only words: the counter above stays at 0 and nothing asks the network for anything.</p>
<script>parent.postMessage('mado-hostile:script','*'); try { top.location = 'http://127.0.0.1:9/mado-hostile-top' } catch (e) {} document.getElementById('marker').textContent = 'SCRIPT RAN';</script>
<img src="http://127.0.0.1:9/mado-hostile-pixel.gif" onerror="parent.postMessage('mado-hostile:onerror','*')" alt="remote picture">
<svg onload="parent.postMessage('mado-hostile:svg','*')"><script>parent.postMessage('mado-hostile:svgscript','*')</script></svg>
<iframe src="http://127.0.0.1:9/mado-hostile-frame" srcdoc="<script>parent.parent.postMessage('mado-hostile:inner','*')</script>"></iframe>
<form action="http://127.0.0.1:9/mado-hostile-form" method="post"><input name="q" autofocus onfocus="parent.postMessage('mado-hostile:focus','*')"><button>Send</button></form>
<a href="http://127.0.0.1:9/mado-hostile-link" target="_top">A link out</a> <a href="javascript:parent.postMessage('mado-hostile:jslink','*')">A script link</a>
<object data="http://127.0.0.1:9/mado-hostile-object"></object><embed src="http://127.0.0.1:9/mado-hostile-embed">
<video src="http://127.0.0.1:9/mado-hostile-video" autoplay></video><audio src="http://127.0.0.1:9/mado-hostile-audio" autoplay></audio>
<div style="position:fixed;inset:0;background:url(http://127.0.0.1:9/mado-hostile-inline.png)"></div>
</body>`;

export const SCREENS: ScreenInfo[] = [
  { id: 'bookings', label: 'Bookings', hasMock: true },
  { id: 'hostile', label: 'Hostile (uncleaned)', hasMock: true },
  { id: 'boat', label: 'Boat details', hasMock: false },
];

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** A stand-in for the daemon that keeps everything in memory. */
export function fakeClient(log: (line: string) => void): DesignClient {
  let tokens: DesignTokens | null = null;
  let picked: string | undefined;
  return {
    async styles() { await wait(250); return { styles: STYLES, picked, dropped: 1 }; },
    async pick(_p, id) { await wait(300); const s = STYLES.find((x) => x.id === id)!; tokens = s.tokens; picked = id; log(`POST style ${id}`); return { picked: id, tokens: s.tokens }; },
    async tokens() { await wait(150); return tokens; },
    async saveTokens(_p, next) { await wait(200); tokens = next; log(`PUT tokens (primary ${next.color.primary}, radius ${next.radius})`); return next; },
    async screens() { await wait(150); return SCREENS; },
    async screen(_p, id) { await wait(200); if (id === 'hostile') return { html: HOSTILE }; if (id === 'bookings') return { html: FRIENDLY }; throw new Error('no mock-up'); },
  };
}
