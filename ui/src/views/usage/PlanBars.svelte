<script lang="ts">
  // One assistant's plan limits: a bar per window (5-hour, weekly…) with the share used and when it
  // resets. The service asks the assistant by itself (when the app opens, every few minutes, after
  // each chat), so the bars are there before the first message; "as of" appears only once the
  // figures are old. Otherwise: a placeholder of the bars' own size while it asks, or one plain
  // sentence saying why there is nothing to show (`planView`).
  import Icon from '../../lib/Icon.svelte';
  import { app, go } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import { asOf, level, meter, providerName, providerState, resetText, shareText, winLabel, type Plan, type Provider } from '../../lib/meter.svelte';
  import { planView } from '../../lib/planView';

  let { provider, plan, compact = false, title = true }: { provider: Provider; plan: Plan; compact?: boolean; title?: boolean } = $props();
  // The newest of what the page loaded and what the window keeps for every view.
  const shared = $derived(meter.plans?.[provider] ?? null);
  const cur = $derived<Plan>(shared && (!plan || (shared.seenAt ?? 0) >= (plan.seenAt ?? 0)) ? shared : plan);
  const watch = $derived(meter.watch?.[provider]);
  const view = $derived(planView(providerState(provider), cur, watch, meter.now / 1000));
  const wins = $derived(cur?.windows ?? []);
  const name = $derived(providerName(provider));
  const service = $derived((app.status as any)?.[provider]?.provider === 'service' || watch?.reason === 'service');
  const words = $derived(
    view.kind === 'missing' ? t('r10-usage.plan.missing', { name })
    : view.kind === 'signedOut' ? t('r10-usage.plan.signedOut', { name })
    : view.kind === 'key' ? t(service ? 'usage72.service' : 'usage72.key')
    : view.kind === 'old' ? t('usage72.old', { name })
    : view.kind === 'noReport' ? t('usage72.noReport', { name })
    : view.kind === 'failed' ? t('usage72.failed', { name })
    : view.kind === 'waiting' ? t('r10-usage.plan.none', { name })
    : '');
</script>

<div class="plan" class:compact aria-busy={view.kind === 'loading'}>
  {#if title}
    <div class="ph">
      <b>{name}</b>
      {#if view.kind === 'known' && view.asOf}<span class="muted xs">{asOf(view.asOf, meter.now)}</span>{/if}
    </div>
  {/if}
  {#if view.kind === 'loading'}
    <!-- The size of two windows, so nothing moves when the figures arrive. -->
    <ul class="wins" role="status" aria-label={t('usage72.loading', { name })}>
      {#each [0, 1] as i (i)}
        <li class="win skel" aria-hidden="true"><div class="wl"><span class="sk w1"></span><span class="sk w2"></span></div><div class="bar ok"></div><div class="xs"><span class="sk w3"></span></div></li>
      {/each}
    </ul>
  {:else if view.kind === 'known'}
    <ul class="wins">
      {#each wins as w (w.id)}
        {@const lv = level(w)}
        <li class="win">
          <div class="wl"><span class="ell">{winLabel(w)}</span><span class="v" class:full={lv === 'full' && !w.hasReset}>{#if lv === 'full' && !w.hasReset}<Icon name="warning" size={12} />{/if}{shareText(w)}</span></div>
          <div class="bar {w.hasReset ? 'ok' : lv}" role="meter" aria-label={winLabel(w)} aria-valuemin={0} aria-valuemax={100}
            aria-valuenow={w.hasReset ? undefined : (w.percent ?? undefined)} aria-valuetext={shareText(w)}>
            <span style="inline-size: {w.hasReset ? 0 : Math.max(w.percent ?? (lv === 'full' ? 100 : 0), 0)}%"></span>
          </div>
          {#if resetText(w, meter.now)}<div class="muted xs">{resetText(w, meter.now)}</div>{/if}
        </li>
      {/each}
    </ul>
    {#if wins.every((w) => w.percent == null && !w.hasReset)}<p class="muted xs">{t('r10-usage.plan.noShare')}</p>{/if}
  {:else}
    <p class="muted xs none">{words}</p>
    {#if !compact && (view.kind === 'missing' || view.kind === 'signedOut')}<button type="button" class="btn sm" onclick={() => go({ kind: 'settings', id: 'ai' })}><Icon name="settings" size={13} /> {t('r10-usage.plan.setUp')}</button>{/if}
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
  .sk { display: inline-block; block-size: 9px; border-radius: 4px; background: var(--fill); vertical-align: middle; }
  .sk.w1 { inline-size: 84px; } .sk.w2 { inline-size: 56px; } .sk.w3 { inline-size: 132px; max-inline-size: 70%; }
  .skel .wl, .skel .xs { line-height: 1.5; }
  @media (prefers-reduced-motion: reduce) { .bar span { transition: none; } }
</style>
