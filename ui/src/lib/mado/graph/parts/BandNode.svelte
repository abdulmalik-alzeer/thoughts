<script lang="ts">
  // The column behind the tasks of one round of the build.
  import type { NodeProps } from '@xyflow/svelte';
  import { t, fmtNum } from '../../../i18n';
  import type { BandData } from '../flow';
  let { data }: NodeProps & { data: BandData } = $props();
</script>

<div class="band" title={t('mado-graph.round.tip')}>
  <span class="head" dir="auto">{t('mado-graph.round', { n: fmtNum(data.band.index + 1) })}</span>
</div>

<style>
  .band {
    width: 100%; height: 100%; border-radius: 16px; background: color-mix(in srgb, var(--fg) 4%, transparent);
    border: var(--border-w) dashed color-mix(in srgb, var(--fg) 16%, transparent); box-sizing: border-box;
  }
  .head { display: block; padding: 10px 16px; font-size: 12px; font-weight: 600; color: var(--muted); text-align: start; }
</style>
