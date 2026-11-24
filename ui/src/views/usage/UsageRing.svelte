<script lang="ts">
  // The small ring beside a chat's Send button: it fills with how much of the plan's limit is used
  // (the fullest window). Hover or click shows the figures: the plan's windows and when they reset,
  // and what this chat has used, with its cost at API prices.
  import { onDestroy } from 'svelte';
  import Icon from '../../lib/Icon.svelte';
  import { go } from '../../lib/store.svelte';
  import { fmtMoney, t } from '../../lib/i18n';
  import { costText, followMeter, fmtPct, fmtTokens, fullest, level, loadChatMeter, meter, providerName, type Plan, type Provider } from '../../lib/meter.svelte';
  import PlanBars from './PlanBars.svelte';

  let { chatId, provider, running = false, below = false, plansOnly = false }: {
    chatId: string; provider: string; running?: boolean;
    /** Open under the ring (in a page header) instead of over it. */
    below?: boolean;
    /** Only the plan's limits: there is no one chat to give figures for. */
    plansOnly?: boolean;
  } = $props();
  const prov = $derived<Provider>(provider === 'codex' ? 'codex' : 'claude');

  let data = $state<any>(null);
  let open = $state(false);
  let pinned = $state(false);
  let root = $state<HTMLElement | null>(null);
  const uid = `ur-${Math.random().toString(36).slice(2, 8)}`;
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;
  let loadTimer: ReturnType<typeof setTimeout> | undefined;

  async function load() { try { data = await loadChatMeter(chatId); } catch { /* the ring stays as it was */ } }
  followMeter();
  $effect(() => { chatId; load(); });
  let seen = meter.tick;
  $effect(() => { const n = meter.tick; if (n !== seen) { seen = n; clearTimeout(loadTimer); loadTimer = setTimeout(load, 500); } });
  // A run that ends brings new figures.
  let wasRunning: boolean | null = null;
  $effect(() => { const r = running; if (wasRunning && !r) { clearTimeout(loadTimer); loadTimer = setTimeout(load, 1200); } wasRunning = r; });
  onDestroy(() => { clearTimeout(hoverTimer); clearTimeout(loadTimer); });

  // What the window keeps for every view (there before this chat's own figures, and kept current), else what came with them.
  const plan = $derived<Plan>(meter.plans?.[prov] ?? data?.plans?.[prov] ?? null);
  const top = $derived(fullest(plan));
  const pct = $derived(top?.percent ?? 0);
  const lv = $derived(top ? level(top) : 'ok');
  const R = 7, C = 2 * Math.PI * R;
  const use = $derived(data?.usage);
  const label = $derived(top ? t('r10-usage.ring.label', { pct: fmtPct(pct) }) : t('r10-usage.ring.labelNone'));

  function enter() { clearTimeout(hoverTimer); hoverTimer = setTimeout(() => { open = true; }, 180); }
  function leave() { clearTimeout(hoverTimer); if (!pinned) hoverTimer = setTimeout(() => { open = false; }, 250); }
  function toggle() { clearTimeout(hoverTimer); if (open && pinned) { open = false; pinned = false; } else { open = true; pinned = true; load(); } }
  function close() { open = false; pinned = false; }
  function outside(e: PointerEvent) { if (open && pinned && root && !root.contains(e.target as Node)) close(); }
  function key(e: KeyboardEvent) { if (e.key === 'Escape' && open) { e.stopPropagation(); close(); root?.querySelector<HTMLButtonElement>('button.ring')?.focus(); } }
</script>

<svelte:window onpointerdown={outside} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="wrapr" bind:this={root} onmouseenter={enter} onmouseleave={leave} onkeydown={key}>
  <button type="button" class="ring btn ghost {lv}" aria-label={label} title={open ? undefined : label} aria-expanded={open} aria-controls={uid} onclick={toggle}>
    <svg viewBox="0 0 18 18" width="18" height="18" aria-hidden="true">
      <circle class="track" cx="9" cy="9" r={R} />
      {#if top}<circle class="fill" cx="9" cy="9" r={R} stroke-dasharray="{(C * Math.min(pct, 100)) / 100} {C}" />{/if}
    </svg>
  </button>
  {#if open}
    <div class="pop surface" class:below id={uid} role="dialog" aria-label={t('r10-usage.ring.title')}>
      <PlanBars provider={prov} {plan} compact />
      {#if !plansOnly}
      <div class="chat">
        <b class="h">{t('r10-usage.ring.thisChat')}</b>
        {#if use?.count}
          <dl>
            <dt>{t('r10-usage.tok.input')}</dt><dd>{fmtTokens(use.tokens.input)}</dd>
            <dt>{t('r10-usage.tok.output')}</dt><dd>{fmtTokens(use.tokens.output)}</dd>
            <dt>{t('r10-usage.tok.cache')}</dt><dd>{fmtTokens(use.tokens.cacheRead + use.tokens.cacheWrite)}</dd>
            <dt>{t('r10-usage.ring.apiCost')}</dt><dd><bdi>{costText(use)}</bdi></dd>
          </dl>
          {#if use.estimated}<p class="muted xs">{t('r10-usage.estimateNote')}</p>{/if}
          <!-- A chat begun before tokens were counted: its whole cost too (the chat header no longer shows it). -->
          {#if data?.chatCostUsd > use.costUsd + 0.005}<p class="xs">{t('r10-polish.ring.total', { cost: fmtMoney(data.chatCostUsd) })}</p>{/if}
        {:else if data?.chatCostUsd > 0}
          <p class="xs">{t('r10-usage.ring.earlier', { cost: fmtMoney(data.chatCostUsd) })}</p>
        {:else}
          <p class="muted xs">{t('r10-usage.ring.none', { name: providerName(prov) })}</p>
        {/if}
      </div>
      {/if}
      <button type="button" class="link" onclick={() => { close(); go({ kind: 'usage' }); }}>{t('r10-usage.ring.all')} <Icon name="right" size={12} class="flip-rtl" /></button>
    </div>
  {/if}
</div>

<style>
  .wrapr { position: relative; display: inline-flex; }
  .ring { padding: 6px; display: inline-grid; place-items: center; }
  svg { transform: rotate(-90deg); }
  :global([dir="rtl"]) .ring svg { transform: scaleX(-1) rotate(-90deg); }
  circle { fill: none; stroke-width: 2.5; }
  .track { stroke: var(--fill-2); }
  .fill { stroke: var(--accent); stroke-linecap: round; transition: stroke-dasharray .4s var(--ease, ease); }
  .warn .fill { stroke: var(--accent-2); }
  .full .fill { stroke: var(--danger); }
  .pop { position: absolute; inset-block-end: calc(100% + 8px); inset-inline-end: 0; inline-size: 290px; max-inline-size: calc(100vw - 32px); z-index: 30;
    padding: 14px; display: flex; flex-direction: column; gap: 12px; box-shadow: var(--shadow-lg, var(--shadow)); text-align: start; }
  .pop.below { inset-block-end: auto; inset-block-start: calc(100% + 8px); }
  .chat { border-top: var(--border-w, 1px) solid var(--hairline); padding-top: 10px; display: flex; flex-direction: column; gap: 6px; }
  .h { font-size: var(--fs-sm, 13px); }
  dl { margin: 0; display: grid; grid-template-columns: 1fr auto; gap: 3px 12px; font-size: var(--fs-sm, 13px); }
  dt { color: var(--muted); }
  dd { margin: 0; text-align: end; font-variant-numeric: tabular-nums; }
  .xs { font-size: var(--fs-xs); margin: 0; }
  .link { font: inherit; font-size: 13px; background: none; border: 0; padding: 0; color: var(--accent); cursor: pointer; align-self: flex-start; display: inline-flex; align-items: center; gap: 4px; }
  .link:hover { text-decoration: underline; }
  @media (prefers-reduced-motion: reduce) { .fill { transition: none; } }
</style>
