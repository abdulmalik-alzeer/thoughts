<script lang="ts">
  // The seven steps in order, each with its state in plain words. One tab stop: the arrow keys move
  // between the steps (the other way round in right-to-left), Home and End jump to the ends.
  import { Check, Lock, LoaderCircle, RotateCw } from '@lucide/svelte';
  import { t, lang } from '../../lib/i18n';
  import type { MadoOverview, StepId } from '../../lib/mado/types';
  import { neighbour, stepperItems } from './steps';

  let { overview, current, onpick }: { overview: MadoOverview | null; current: StepId; onpick: (s: StepId) => void } = $props();
  const items = $derived(stepperItems(overview, current));
  let list = $state<HTMLOListElement>();

  function onkey(e: KeyboardEvent) {
    const rtl = lang() === 'ar';
    const to = e.key === 'ArrowRight' ? neighbour(current, rtl ? -1 : 1)
      : e.key === 'ArrowLeft' ? neighbour(current, rtl ? 1 : -1)
      : e.key === 'ArrowDown' ? neighbour(current, 1)
      : e.key === 'ArrowUp' ? neighbour(current, -1)
      : e.key === 'Home' ? neighbour(current, 'first')
      : e.key === 'End' ? neighbour(current, 'last') : null;
    if (!to) return;
    e.preventDefault();
    onpick(to);
    queueMicrotask(() => list?.querySelector<HTMLElement>(`[data-step="${to}"]`)?.focus());
  }
</script>

<nav aria-label={t('mado.stepper.label')}>
  <ol class="steps" bind:this={list}>
    {#each items as s (s.id)}
      <li class="step tone-{s.tone}" class:current={s.current}>
        <button type="button" class="stepb" data-step={s.id} tabindex={s.current ? 0 : -1} aria-current={s.current ? 'step' : undefined}
          onclick={() => onpick(s.id)} onkeydown={onkey}>
          <span class="dot" aria-hidden="true">
            {#if s.tone === 'done'}<Check size={14} strokeWidth={2.6} />
            {:else if s.tone === 'off'}<Lock size={12} strokeWidth={2.2} />
            {:else if s.tone === 'busy'}<LoaderCircle size={14} strokeWidth={2.4} class="spin" />
            {:else if s.tone === 'warn'}<RotateCw size={13} strokeWidth={2.4} />
            {:else}{s.n}{/if}
          </span>
          <span class="txt">
            <span class="name">{t(s.nameKey)}</span>
            <span class="state">{t(s.statusKey)}</span>
          </span>
        </button>
      </li>
    {/each}
  </ol>
</nav>

<style>
  .steps { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(7, minmax(0, 1fr)); gap: 0; }
  .step { position: relative; min-width: 0; }
  /* The line that joins a step to the next one; filled once the step is approved. */
  .step:not(:last-child)::after { content: ''; position: absolute; top: 23px; inset-inline-start: calc(50% + 20px); inset-inline-end: calc(-50% + 20px); height: 2px; border-radius: 2px; background: var(--hairline); transition: background .3s; }
  .step.tone-done:not(:last-child)::after { background: color-mix(in srgb, var(--accent) 55%, transparent); }
  .stepb { font: inherit; color: inherit; background: none; border: 0; width: 100%; padding: 8px 4px 10px; border-radius: var(--radius-lg); cursor: pointer; display: grid; justify-items: center; gap: 6px; text-align: center; transition: background .15s; }
  .stepb:hover { background: var(--fill); }
  .dot { width: 30px; height: 30px; border-radius: 99px; display: grid; place-items: center; font-size: 13px; font-weight: 600; background: var(--panel); color: var(--muted); border: 2px solid var(--control-border); position: relative; z-index: 1; transition: background .2s, border-color .2s, color .2s, box-shadow .2s; }
  .txt { display: grid; gap: 1px; min-width: 0; max-width: 100%; }
  .name { font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .state { font-size: var(--fs-xs); color: var(--muted); line-height: 1.3; }
  .tone-off .dot { border-style: dashed; border-color: var(--hairline); }
  .tone-off .name { color: var(--muted); font-weight: 500; }
  .tone-next .dot { border-color: var(--accent); color: var(--accent); }
  .tone-you .dot { background: var(--tint); border-color: var(--accent); color: var(--on-tint); }
  .tone-you .state { color: var(--on-tint); font-weight: 600; }
  .tone-busy .dot { border-color: var(--accent-2); color: var(--accent-2); }
  .tone-done .dot { background: var(--accent); border-color: var(--accent); color: var(--accent-fg); }
  .tone-warn .dot { border-color: var(--danger); color: var(--danger); }
  .tone-warn .state { color: var(--danger); }
  .step.current .stepb { background: var(--fill); }
  .step.current .dot { box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent) 22%, transparent); }
  :global([data-theme="contrast"]) .tone-you .dot { color: var(--accent-fg); }
  :global([data-theme="contrast"]) .tone-you .state { color: var(--fg); }
  .dot :global(.spin) { animation: mado-spin 1.1s linear infinite; }
  @keyframes mado-spin { to { transform: rotate(360deg); } }
  @container main (max-width: 760px) {
    .steps { grid-template-columns: 1fr; gap: 2px; }
    .step::after { display: none; }
    .stepb { grid-template-columns: auto 1fr; justify-items: start; text-align: start; gap: 10px; padding: 6px 8px; align-items: center; }
    .txt { justify-items: start; }
  }
</style>
