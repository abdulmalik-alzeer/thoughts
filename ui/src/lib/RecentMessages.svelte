<script lang="ts">
  // "Recent messages": the last 20 toasts, kept after they leave the screen, so an error that came
  // and went (or was pushed out by newer messages) can still be read and copied.
  import { app } from './store.svelte';
  import { t, ago, fmtDate } from './i18n';
  import { trapFocus } from './focus';
  import Icon from './Icon.svelte';

  let { close }: { close: () => void } = $props();
  let copied = $state<number | null>(null);
  let open = $state<number | null>(null);

  async function copy(id: number, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = id;
      setTimeout(() => { if (copied === id) copied = null; }, 2000);
    } catch { copied = null; }
  }
</script>

<div class="modal-bg recent-bg no-print" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
  <div class="modal recent" role="dialog" aria-modal="true" aria-labelledby="recent-title" tabindex="-1" use:trapFocus={{ onEscape: close }}>
    <div class="row head">
      <h2 id="recent-title" class="grow">{t('shellui.recent.title')}</h2>
      {#if app.toastLog.length}<button type="button" class="btn ghost sm" onclick={() => (app.toastLog = [])}>{t('shellui.recent.clear')}</button>{/if}
      <button type="button" class="x" aria-label={t('close')} title={t('close')} onclick={close} data-autofocus><Icon name="close" size={15} /></button>
    </div>
    {#if !app.toastLog.length}
      <p class="muted empty">{t('shellui.recent.empty')}</p>
    {:else}
      <ul class="list scroll">
        {#each app.toastLog as x (x.id)}
          <li class={x.kind}>
            <div class="row line">
              {#if x.kind === 'error'}<Icon name="warning" size={15} class="k" />{:else if x.kind === 'success'}<Icon name="check" size={15} class="k" />{:else}<Icon name="info" size={15} class="k" />{/if}
              <span class="grow msg" dir="auto">{#if x.kind === 'error'}<span class="sr-only">{t('shellui.recent.error')}: </span>{/if}{x.text}</span>
              <time class="muted sm" datetime={new Date(x.at).toISOString()} title={fmtDate(x.at, { dateStyle: 'medium', timeStyle: 'medium' })}>{ago(x.at)}</time>
            </div>
            <div class="row acts">
              {#if x.detail}
                <button type="button" class="btn ghost sm" aria-expanded={open === x.id} onclick={() => (open = open === x.id ? null : x.id)}>{t('shell.details')}</button>
              {/if}
              <button type="button" class="btn ghost sm" onclick={() => copy(x.id, x.detail ? `${x.text}\n\n${x.detail}` : x.text)}><Icon name={copied === x.id ? 'check' : 'copy'} size={13} /> {copied === x.id ? t('shell.copied') : t('shell.copy')}</button>
            </div>
            {#if x.detail && open === x.id}
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <pre class="detail" dir="auto" tabindex="0" aria-label={t('shell.details')}>{x.detail}</pre>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .recent { width: min(520px, 92vw); display: grid; gap: 10px; }
  .head { gap: 6px; }
  h2 { margin: 0; font-size: 17px; }
  .empty { margin: 0; }
  .list { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; max-height: 60vh; }
  li { padding: 8px 10px; border-radius: var(--radius); background: var(--fill); border: var(--border-w) solid transparent; display: grid; gap: 4px; }
  li.error { border-color: color-mix(in srgb, var(--danger) 45%, transparent); }
  li.error :global(.k) { color: var(--danger); }
  li.success :global(.k) { color: var(--ok); }
  .line { align-items: flex-start; }
  .line :global(.k) { flex: none; margin-top: 3px; color: var(--muted); }
  .msg { overflow-wrap: anywhere; white-space: pre-wrap; font-size: 13px; }
  time { flex: none; white-space: nowrap; }
  .acts { justify-content: flex-end; gap: 4px; }
  .sm { font-size: var(--fs-xs); padding: 2px 8px; }
  .detail { margin: 0; max-height: 180px; overflow: auto; font-size: var(--fs-xs); padding: 8px; white-space: pre-wrap; overflow-wrap: anywhere; text-align: start; }
  .x { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 6px; width: 28px; height: 28px; display: grid; place-items: center; cursor: pointer; }
  .x:hover { background: var(--fill-2); color: var(--fg); }
</style>
