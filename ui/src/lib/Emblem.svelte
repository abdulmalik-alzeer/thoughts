<script lang="ts">
  // A slowly turning low-poly sphere, lit from the top-right and coloured from the active theme
  // (--accent-2 for highlights, --accent for mid tones). Pure SVG, no assets.
  import { onMount } from 'svelte';

  let { size = 260 }: { size?: number } = $props();

  const LAT = 7, LON = 12;
  // Deterministic jitter so facets look hand-cut rather than perfectly regular.
  const rnd = (i: number) => { const x = Math.sin(i * 12.9898) * 43758.5453; return x - Math.floor(x); };
  const verts: [number, number, number][][] = [];
  for (let i = 0; i <= LAT; i++) {
    const row: [number, number, number][] = [];
    for (let j = 0; j < LON; j++) {
      const th = (i / LAT) * Math.PI + (i % LAT ? (rnd(i * 31 + j) - .5) * .18 : 0);
      const ph = (j / LON) * Math.PI * 2 + (i % 2) * (Math.PI / LON) + (rnd(j * 17 + i) - .5) * .2;
      const r = 1 + (i % LAT ? (rnd(i * 7 + j * 3) - .5) * .12 : 0);
      row.push([r * Math.sin(th) * Math.cos(ph), r * Math.cos(th), r * Math.sin(th) * Math.sin(ph)]);
    }
    verts.push(row);
  }
  const tris: [number, number][][] = [];
  for (let i = 0; i < LAT; i++) for (let j = 0; j < LON; j++) {
    const a: [number, number] = [i, j], b: [number, number] = [i, (j + 1) % LON], c: [number, number] = [i + 1, j], d: [number, number] = [i + 1, (j + 1) % LON];
    tris.push([a, c, b], [b, c, d]);
  }

  let angle = $state(0.6);
  const tilt = -0.35;
  const light = (() => { const v = [0.55, 0.75, 0.6]; const n = Math.hypot(...v); return v.map((x) => x / n); })();

  function rot([x, y, z]: [number, number, number]): [number, number, number] {
    const c = Math.cos(angle), s = Math.sin(angle);
    let X = x * c + z * s, Z = -x * s + z * c;
    const ct = Math.cos(tilt), st = Math.sin(tilt);
    const Y = y * ct - Z * st; Z = y * st + Z * ct;
    return [X, Y, Z];
  }

  const faces = $derived.by(() => {
    const P = verts.map((row) => row.map(rot));
    const out: { d: string; z: number; tone: number; spec: number }[] = [];
    for (const t of tris) {
      const [p, q, r] = t.map(([i, j]) => P[i][j]);
      const u = [q[0] - p[0], q[1] - p[1], q[2] - p[2]], v = [r[0] - p[0], r[1] - p[1], r[2] - p[2]];
      let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
      const len = Math.hypot(...n) || 1; n = n.map((x) => x / len);
      if (n[2] < 0) n = n.map((x) => -x);
      const cz = (p[2] + q[2] + r[2]) / 3;
      if (cz < 0.02) continue;
      const diff = Math.max(0, n[0] * light[0] + n[1] * light[1] + n[2] * light[2]);
      const spec = Math.pow(diff, 8);
      const pt = (a: number[]) => `${(50 + a[0] * 46).toFixed(2)},${(50 - a[1] * 46).toFixed(2)}`;
      out.push({ d: `M${pt(p)}L${pt(q)}L${pt(r)}Z`, z: cz, tone: diff, spec });
    }
    return out.sort((a, b) => a.z - b.z);
  });

  onMount(() => {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    let raf = 0, last = performance.now();
    const tick = (now: number) => { angle += (now - last) * 0.00006; last = now; raf = requestAnimationFrame(tick); };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  });
</script>

<div class="emblem" style="width:{size}px;height:{size}px">
  <div class="halo"></div>
  <svg viewBox="0 0 100 100" aria-hidden="true">
    {#each faces as f}
      <path d={f.d}
        style="fill: color-mix(in oklab, color-mix(in oklab, var(--accent-2) {Math.round(f.spec * 100)}%, var(--accent)) {Math.round(4 + f.tone * f.tone * 92)}%, var(--em-base));"
        stroke="var(--em-edge)" stroke-width=".18" stroke-linejoin="round" />
    {/each}
  </svg>
</div>

<style>
  .emblem { position: relative; flex: none; --em-base: color-mix(in oklab, var(--bg) 70%, #000); --em-edge: color-mix(in srgb, var(--fg) 14%, transparent); }
  :global([data-mode="light"]) .emblem { --em-base: color-mix(in oklab, var(--fg) 45%, var(--bg)); --em-edge: color-mix(in srgb, var(--bg) 35%, transparent); }
  .halo { position: absolute; inset: 8%; border-radius: 50%; filter: blur(40px); opacity: .45;
    background: radial-gradient(circle at 65% 30%, var(--accent-2), transparent 60%), radial-gradient(circle at 30% 80%, var(--accent), transparent 60%); }
  svg { position: relative; width: 100%; height: 100%; filter: drop-shadow(0 20px 30px rgba(0,0,0,.35)); }
</style>
