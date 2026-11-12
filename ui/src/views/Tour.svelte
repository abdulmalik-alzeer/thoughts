<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { t } from '../lib/i18n';
  import { trapFocus } from '../lib/focus';
  import { keys } from '../lib/shortcuts';
  // `note`: one quiet line under the cards (the first-run tour says setup carries on meanwhile).
  let { close, note = '' }: { close: () => void; note?: string } = $props();
  // icon + i18n key stem (shell.tour.<stem>Title / <stem>Body)
  // The short version: the rest (files, folders) is explained in Help and in each view's empty state.
  const cards = [['home', 'home'], ['chats', 'chat'], ['wrap', 'wrap'], ['palette', 'yours']];
  let i = $state(0);
  let box = $state<HTMLElement | null>(null);
  // Next/Get started swap places, so put focus back on the main button after each step.
  $effect(() => { i; queueMicrotask(() => box?.querySelector<HTMLElement>('.btn.primary')?.focus()); });
  const last = $derived(i === cards.length - 1);

  function onKey(e: KeyboardEvent) {
    const fwd = document.documentElement.dir === 'rtl' ? 'ArrowLeft' : 'ArrowRight';
    const back = document.documentElement.dir === 'rtl' ? 'ArrowRight' : 'ArrowLeft';
    if (e.key === fwd && !last) i++;
    else if (e.key === back && i > 0) i--;
  }
</script>

<div class="modal-bg no-print" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="modal tour" role="dialog" aria-modal="true" aria-labelledby="tour-title" aria-describedby="tour-body" tabindex="-1" bind:this={box}
    use:trapFocus={{ onEscape: close }} onkeydown={onKey}>
    <div class="ic"><Icon name={cards[i][0]} size={26} /></div>
    <div aria-live="polite">
      <h1 id="tour-title">{t(`shell.tour.${cards[i][1]}Title`)}</h1>
      <p id="tour-body">{t(`shell.tour.${cards[i][1]}Body`, { palette: keys('palette') })}</p>
    </div>
    {#if note}<p class="note">{note}</p>{/if}
    <div class="dots" role="img" aria-label={t('shell.tour.step', { n: i + 1, total: cards.length })}>{#each cards as _, j}<i class:on={j === i}></i>{/each}</div>
    <div class="row"><button class="btn ghost" onclick={close}>{t('shell.tour.skip')}</button><span class="grow"></span>
      {#if i > 0}<button class="btn" onclick={() => i--}>{t('back')}</button>{/if}
      {#if !last}<button class="btn primary" onclick={() => i++}>{t('next')}</button>{:else}<button class="btn primary" onclick={close}>{t('shell.tour.start')}</button>{/if}
    </div>
  </div>
</div>

<style>
  .tour { max-width: 460px; text-align: center; padding: 28px; }
  .ic { width: 56px; height: 56px; border-radius: 16px; margin: 0 auto 14px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); }
  p { color: var(--muted); margin: 0 0 18px; }
  .note { font-size: var(--fs-xs); margin: -6px 0 14px; }
  .dots { display: flex; justify-content: center; gap: 6px; margin-bottom: 18px; }
  .dots i { width: 6px; height: 6px; border-radius: 50%; background: var(--fill-2); border: 1px solid var(--muted); }
  .dots i.on { background: var(--accent); border-color: var(--accent); }
</style>
