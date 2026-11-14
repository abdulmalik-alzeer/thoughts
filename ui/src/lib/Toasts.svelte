<script lang="ts">
  import { app, dismissToast, type ToastItem } from './store.svelte';
  import { t } from './i18n';
  import Icon from './Icon.svelte';

  // Time left per toast. A single ticker counts down; while the pointer or keyboard focus is
  // inside the stack nothing counts, and once both have left every toast gets its full time
  // again, so nothing vanishes while it is being read. A toast whose time is up fades out
  // (no fade with reduce-motion) and is then removed; × removes it at once.
  const left = new Map<number, number>();
  let wrap = $state<HTMLElement>();
  let held = $state(false);
  let leaving = $state<Set<number>>(new Set());
  let copied = $state<number | null>(null);
  const FADE = 320;
  const osReduce = typeof matchMedia === 'function' ? matchMedia('(prefers-reduced-motion: reduce)') : null;
  const reduced = () => !!osReduce?.matches || document.documentElement.dataset.motion === 'reduce';

  $effect(() => {
    const ids = new Set(app.toasts.map((x) => x.id));
    for (const x of app.toasts) if (!left.has(x.id)) left.set(x.id, x.duration);
    for (const id of [...left.keys()]) if (!ids.has(id)) left.delete(id);
    if ([...leaving].some((id) => !ids.has(id))) leaving = new Set([...leaving].filter((id) => ids.has(id)));
  });

  function fadeOut(id: number) {
    if (reduced()) { dismissToast(id); return; }
    leaving = new Set([...leaving, id]);
    setTimeout(() => { if (leaving.has(id)) dismissToast(id); }, FADE);
  }

  /** Pointer over or focus inside the stack: hold every timer, and bring back one that was fading. */
  function hold() {
    held = true;
    if (leaving.size) leaving = new Set();
  }
  /** Pointer and focus both gone: every toast starts its time again from the beginning. */
  function release() {
    queueMicrotask(() => {
      if (!wrap || wrap.matches(':hover') || wrap.contains(document.activeElement)) return;
      held = false;
      for (const x of app.toasts) if (left.has(x.id)) left.set(x.id, x.duration);
    });
  }

  $effect(() => {
    if (held) return;
    const step = 250;
    const h = setInterval(() => {
      for (const [id, ms] of left) {
        if (ms === 0 || leaving.has(id)) continue; // 0 = still in progress, stays until replaced or closed
        if (ms <= step) { left.set(id, -1); fadeOut(id); } else left.set(id, ms - step);
      }
    }, step);
    return () => clearInterval(h);
  });

  // A toast that held focus and was then closed takes the focus with it, and no focusout may
  // follow: let go of the hold once the stack truly has neither pointer nor focus.
  $effect(() => {
    void app.toasts.length;
    if (held) release();
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

<div class="toast-wrap no-print" role="region" aria-label={t('shell.notifications')} bind:this={wrap}
  onmouseover={hold} onmouseleave={release}
  onfocusin={hold} onfocusout={release}>
  {#each app.toasts as x (x.id)}
    <div class="toast {x.kind}" class:leaving={leaving.has(x.id)} role={x.kind === 'error' ? 'alert' : 'status'} aria-live={x.kind === 'error' ? 'assertive' : 'polite'} aria-atomic="true">
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
      {#if x.tech}
        <!-- What git, the system or the assistant said, for whoever helps: folded away. -->
        <details class="tech">
          <summary>{t('wording.showDetails')}</summary>
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <pre class="detail" dir="ltr" tabindex="0" aria-label={t('shell.details')}>{x.tech}</pre>
          <div class="line end">
            <button class="btn sm" onclick={async () => { try { await navigator.clipboard.writeText(x.tech ?? ''); copied = x.id; setTimeout(() => { if (copied === x.id) copied = null; }, 2000); } catch { copied = null; } }}><Icon name={copied === x.id ? 'check' : 'copy'} size={13} /> {copied === x.id ? t('shell.copied') : t('shell.copy')}</button>
          </div>
        </details>
      {/if}
    </div>
  {/each}
</div>

<style>
  .toast.leaving { animation: toast-out 320ms var(--ease) both; }
  @keyframes toast-out { to { opacity: 0; transform: translateY(-6px) scale(.98); } }
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
  .detail { margin: 0; max-height: 180px; overflow: auto; font-size: var(--fs-xs); padding: 8px; white-space: pre-wrap; overflow-wrap: anywhere; text-align: start; }
  .sm { font-size: var(--fs-xs); padding: 3px 8px; }
  .tech { display: grid; gap: 6px; }
  .tech summary { cursor: pointer; font-size: var(--fs-xs); color: var(--muted); width: max-content; }
  .tech[open] { gap: 6px; }
</style>
