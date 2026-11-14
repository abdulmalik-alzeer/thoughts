<script lang="ts">
  // Usage at a glance: each assistant's plan limits and the last 30 days' tokens with what they would
  // cost at API prices. On Home (as a tile) and in Settings › Usage.
  import { onMount, untrack } from 'svelte';
  import { app } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import { costText, followMeter, fmtTokens, loadMeter, meter, providerState, PROVIDERS, type Provider } from '../../lib/meter.svelte';
  import PlanBars from './PlanBars.svelte';

  let data = $state<any>(null);
  let failed = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function load() {
    try { data = await loadMeter(30); failed = false; } catch { failed = true; }
  }
  onMount(() => { followMeter(); return () => clearTimeout(timer); });
  // Again when the user changes what the figures cover.
  $effect(() => { app.settings?.spending?.wholeComputer; untrack(load); });
  // A run just ended: its figures (debounced, runs end in bursts).
  let seen = meter.tick;
  $effect(() => { const n = meter.tick; if (n !== seen) { seen = n; clearTimeout(timer); timer = setTimeout(load, 800); } });

  /** Only the assistants that are there (both, when the status is not known yet). */
  const shown = $derived(PROVIDERS.filter((p) => providerState(p) !== 'missing' || (data?.totals?.[p]?.count ?? 0) > 0));
  const list = $derived<Provider[]>(shown.length ? shown : ['claude']);
</script>

{#if failed && !data}
  <p class="muted xs" role="status">{t('r10-usage.loadFailed')}</p>
{:else}
  <div class="glance" aria-busy={!data}>
    {#each list as p (p)}
      {@const u = data?.totals?.[p]}
      <div class="col">
        <PlanBars provider={p} plan={data?.plans?.[p] ?? null} compact />
        <p class="muted xs sum">
          {#if !data}<span class="skel"></span>
          {:else if u?.count}{t('r10-usage.glance.month', { tokens: fmtTokens(u.totalTokens), cost: costText(u) })}
          {:else}{t('r10-usage.glance.none')}{/if}
        </p>
      </div>
    {/each}
  </div>
{/if}

<style>
  .glance { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 18px 28px; }
  .col { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .sum { margin: 0; }
  .xs { font-size: var(--fs-xs); }
  .skel { display: inline-block; inline-size: 60%; block-size: 10px; border-radius: 4px; background: var(--fill); }
</style>
