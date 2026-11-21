<script lang="ts">
  // A text of the project shown for reading only (its conventions, its README): drawn by the app's
  // own safe markdown renderer, without pictures. Links go through the window's link guard.
  import { FileText } from '@lucide/svelte';
  import { mdClick } from '../../lib/md';
  import { t } from '../../lib/i18n';
  import { projectDocHtml } from '../../lib/mado/readme';

  let { text, label, max = 8000, hint = '' }: { /** undefined: being fetched · null: not there. */ text: string | null | undefined; label: string; max?: number; hint?: string } = $props();
  const html = $derived(typeof text === 'string' ? projectDocHtml(text, max) : '');
</script>

{#if text === undefined}
  <div class="state muted" role="status">{t('mado.loading')}</div>
{:else if text === null || !text.trim()}
  <div class="empty"><FileText size={26} strokeWidth={1.5} aria-hidden="true" /><b>{t('mado.doc.blankTitle')}</b><span class="muted">{t('mado.pdoc.none')}</span></div>
{:else}
  {#if hint}<p class="muted hint">{hint}</p>{/if}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_click_events_have_key_events -->
  <article class="md doc" aria-label={label} onclick={mdClick}>{@html html}</article>
{/if}

<style>
  .hint { font-size: var(--fs-sm); margin: 0 0 14px; }
  .doc { font-size: 15px; line-height: 1.7; max-width: 74ch; overflow-wrap: anywhere; }
  .doc :global(h1) { font-size: 24px; margin: 0 0 14px; }
  .doc :global(h2) { font-size: 18px; margin: 24px 0 10px; }
  .doc :global(h3) { font-size: 16px; font-weight: 600; margin: 20px 0 8px; }
  .state { padding: 40px 0; text-align: center; }
  .empty { display: grid; justify-items: center; gap: 6px; text-align: center; padding: 56px 24px; border: 1px dashed var(--border); border-radius: var(--radius-lg); color: var(--muted); }
  .empty b { color: var(--fg); font-size: 15px; }
</style>
