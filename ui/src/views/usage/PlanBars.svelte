<script lang="ts">
  // One assistant's plan limits: a bar per window (5-hour, weekly…) with the share used and when it
  // resets, "as of" the run that reported it — or why there is nothing to show.
  import Icon from '../../lib/Icon.svelte';
  import { go } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import { asOf, level, providerName, providerState, resetText, shareText, winLabel, type Plan, type Provider } from '../../lib/meter.svelte';

  let { provider, plan, compact = false, title = true }: { provider: Provider; plan: Plan; compact?: boolean; title?: boolean } = $props();
  const st = $derived(providerState(provider));
  const wins = $derived(plan?.windows ?? []);
</script>

<div class="plan" class:compact>
  {#if title}
    <div class="ph">
      <b>{providerName(provider)}</b>
      {#if st !== 'key' && plan?.seenAt}<span class="muted xs">{asOf(plan.seenAt)}</span>{/if}
    </div>
  {/if}
  {#if (st === 'missing' || st === 'signedOut') && !wins.length}
    <p class="muted xs none">{t(st === 'missing' ? 'r10-usage.plan.missing' : 'r10-usage.plan.signedOut', { name: providerName(provider) })}</p>
    {#if !compact}<button type="button" class="btn sm" onclick={() => go({ kind: 'settings', id: 'ai' })}><Icon name="settings" size={13} /> {t('r10-usage.plan.setUp')}</button>{/if}
  {:else if st === 'key'}
    <p class="muted xs none">{t('r10-usage.plan.key')}</p>
  {:else if !wins.length}
    <p class="muted xs none">{t('r10-usage.plan.none', { name: providerName(provider) })}</p>
  {:else}
    <ul class="wins">
      {#each wins as w (w.id)}
        {@const lv = level(w)}
        <li class="win">
          <div class="wl"><span class="ell">{winLabel(w)}</span><span class="v" class:full={lv === 'full' && !w.hasReset}>{#if lv === 'full' && !w.hasReset}<Icon name="warning" size={12} />{/if}{shareText(w)}</span></div>
          <div class="bar {w.hasReset ? 'ok' : lv}" role="meter" aria-label={winLabel(w)} aria-valuemin={0} aria-valuemax={100}
            aria-valuenow={w.hasReset ? undefined : (w.percent ?? undefined)} aria-valuetext={shareText(w)}>
            <span style="inline-size: {w.hasReset ? 0 : Math.max(w.percent ?? (lv === 'full' ? 100 : 0), 0)}%"></span>
          </div>
          {#if resetText(w)}<div class="muted xs">{resetText(w)}</div>{/if}
        </li>
      {/each}
    </ul>
    {#if wins.every((w) => w.percent == null && !w.hasReset)}<p class="muted xs">{t('r10-usage.plan.noShare')}</p>{/if}
  {/if}
</div>

<style>
  .plan { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .ph { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
  .wins { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 10px; }
  .compact .wins { gap: 7px; }
  .win { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .wl { display: flex; justify-content: space-between; gap: 8px; font-size: var(--fs-sm, 13px); }
  .v { font-variant-numeric: tabular-nums; white-space: nowrap; display: inline-flex; align-items: center; gap: 4px; }
  .v.full { color: var(--danger); font-weight: 600; }
  .bar { block-size: 6px; border-radius: 99px; background: var(--fill-2); overflow: hidden; }
  .compact .bar { block-size: 5px; }
  .bar span { display: block; block-size: 100%; border-radius: 99px; background: var(--accent); transition: inline-size .4s var(--ease, ease); }
  .bar.warn span { background: var(--accent-2); }
  .bar.full span { background: var(--danger); }
  .xs { font-size: var(--fs-xs); }
  .none { margin: 0; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .btn.sm { align-self: flex-start; font-size: 12px; padding: 4px 10px; }
  @media (prefers-reduced-motion: reduce) { .bar span { transition: none; } }
</style>
