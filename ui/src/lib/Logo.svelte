<script lang="ts">
  // The Facet mark, coloured live from the active theme's CSS variables.
  import { gemPaths } from './logo';
  let { size = 24, tile = true }: { size?: number; tile?: boolean } = $props();
  const gem = gemPaths({ a: 'var(--accent)', b: 'var(--accent-2)', ink: 'var(--fg)' });
</script>

<svg class="logo-mark" class:tile width={size} height={size} viewBox="0 0 100 100" aria-hidden="true">
  {#if tile}<rect x="2" y="2" width="96" height="96" rx="24" class="bgr" />{/if}
  <g transform={tile ? 'translate(50 50) scale(.9) translate(-50 -50)' : ''}>{@html gem}</g>
</svg>

<style>
  .logo-mark { flex: none; display: block; }
  .bgr { fill: var(--panel-2); stroke: color-mix(in srgb, var(--fg) 12%, transparent); stroke-width: 2; }
  polygon { transition: fill .4s; }
</style>
