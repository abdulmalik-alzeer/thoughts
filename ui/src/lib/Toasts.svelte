<script lang="ts">
  import { app, dismissToast, type ToastItem } from './store.svelte';
  import { t } from './i18n';
  import Icon from './Icon.svelte';

  // Time left per toast. A single ticker counts down, and stops while the pointer or
  // keyboard focus is inside the stack so nothing vanishes while it is being read.
  const left = new Map<number, number>();
  let paused = $state(false);
  let copied = $state<number | null>(null);

  $effect(() => {
    const ids = new Set(app.toasts.map((x) => x.id));
    for (const x of app.toasts) if (!left.has(x.id)) left.set(x.id, x.duration);
    for (const id of [...left.keys()]) if (!ids.has(id)) left.delete(id);
  });

  $effect(() => {
    if (paused) return;
    const step = 250;
    const h = setInterval(() => {
      for (const [id, ms] of left) {
        if (ms === 0) continue; // 0 = stays until dismissed
        if (ms <= step) dismissToast(id); else left.set(id, ms - step);
      }
    }, step);
    return () => clearInterval(h);
  });

  function act(x: ToastItem) {
    dismissToast(x.id);
    try { x.action?.run(); } catch (e) { console.error(e); }
  }

  async function copy(x: ToastItem) {
    try {
      await navigator.clipboard.writeText(x.detail ?? x.text);
      copied = x.id;
      setTimeout(() => { if (copied === x.id) copied = null; }, 2000);
    } catch { copied = null; }
  }
</script>

<div class="toast-wrap no-print" role="region" aria-label={t('shell.notifications')}
  onmouseenter={() => (paused = true)} onmouseleave={() => (paused = false)}
  onfocusin={() => (paused = true)} onfocusout={() => (paused = false)}>
  {#each app.toasts as x (x.id)}
    <div class="toast {x.kind}" role={x.kind === 'error' ? 'alert' : 'status'} aria-live={x.kind === 'error' ? 'assertive' : 'polite'} aria-atomic="true">
      <div class="line">
        {#if x.kind === 'error'}<Icon name="warning" size={16} class="k" />{:else if x.kind === 'success'}<Icon name="check" size={16} class="k" />{/if}
        <span class="msg" dir="auto">{x.text}</span>
        {#if x.action}<button class="btn act" onclick={() => act(x)}>{x.action.label}</button>{/if}
        <button class="x" aria-label={t('shell.dismiss')} title={t('shell.dismiss')} onclick={() => dismissToast(x.id)}><Icon name="close" size={14} /></button>
      </div>
      {#if x.detail}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <pre class="detail" dir="auto" tabindex="0" aria-label={t('shell.details')}>{x.detail}</pre>
        <div class="line end">
          <button class="btn sm" onclick={() => copy(x)}><Icon name={copied === x.id ? 'check' : 'copy'} size={13} /> {copied === x.id ? t('shell.copied') : t('shell.copy')}</button>
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .toast { display: grid; gap: 8px; width: min(400px, calc(100vw - 36px)); padding-block: 10px; padding-inline: 14px 10px; }
  .toast.error { border-color: color-mix(in srgb, var(--danger) 55%, transparent); }
  .toast.error :global(.k) { color: var(--danger); }
  .toast.success :global(.k) { color: var(--ok); }
  .line { display: flex; align-items: flex-start; gap: 8px; }
  .line :global(.k) { flex: none; margin-top: 3px; }
  .line.end { justify-content: flex-end; }
  .msg { flex: 1; min-width: 0; overflow-wrap: anywhere; white-space: pre-wrap; }
  .act { flex: none; padding: 3px 10px; color: var(--accent); font-weight: 600; }
  .x { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 6px; width: 24px; height: 24px; display: grid; place-items: center; flex: none; cursor: pointer; }
  .x:hover { background: var(--fill); color: var(--fg); }
  .detail { margin: 0; max-height: 180px; overflow: auto; font-size: 12px; padding: 8px; white-space: pre-wrap; overflow-wrap: anywhere; text-align: start; }
  .sm { font-size: 12px; padding: 3px 8px; }
</style>
