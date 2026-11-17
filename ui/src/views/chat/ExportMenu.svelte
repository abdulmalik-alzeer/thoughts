<script lang="ts">
  // Taking a whole conversation out of the app: as a Markdown file, copied as Markdown, or printed
  // (which is also how it becomes a PDF). Everything happens on this computer; nothing is uploaded.
  import { tick } from 'svelte';
  import Icon from '../../lib/Icon.svelte';
  import { t, fmtDate } from '../../lib/i18n';
  import { saveFile } from '../../lib/download';
  import { copyText, notify } from '../../lib/content/util';
  import { printView } from '../../lib/printPdf';
  import { chatMarkdown, exportName } from './exportChat';

  let { chat }: { chat: any } = $props();
  let open = $state(false);
  let root = $state<HTMLElement>();
  let button = $state<HTMLButtonElement>();

  const text = () => chatMarkdown(chat, { t, date: (d) => fmtDate(d, { dateStyle: 'long', timeStyle: 'short' }) });

  async function save() {
    close(false);
    const r = await saveFile(exportName(chat.title) + '.md', text(), 'text/markdown');
    if (r === 'saved') notify(t('chatc.export.saved'), { kind: 'success' });
    else if (r === 'unsupported') copy();
  }
  async function copy() {
    close(false);
    const ok = await copyText(text());
    notify(t(ok ? 'chatc.export.copied' : 'chat.copyFailed'), { kind: ok ? 'info' : 'error' });
  }
  function print() { close(false); printView(chat.title, 'chat-pdf-hint'); }

  function close(refocus = true) { open = false; if (refocus) button?.focus(); }
  async function toggle() {
    open = !open;
    if (open) { await tick(); root?.querySelector<HTMLElement>('[role="menuitem"]')?.focus(); }
  }
  function onkey(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === 'Escape') { e.stopPropagation(); e.preventDefault(); close(); return; }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    e.preventDefault();
    const items = [...(root?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    items[(at + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus();
  }
</script>

<svelte:window onclick={(e) => { if (open && root && !root.contains(e.target as Node)) open = false; }} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span class="em" bind:this={root} onkeydown={onkey}>
  <button bind:this={button} type="button" class="btn ghost" aria-haspopup="menu" aria-expanded={open} onclick={toggle}
    title={t('chatc.export.tip')} aria-label={t('chatc.export.tip')} disabled={!chat.messages?.length}><Icon name="download" /></button>
  {#if open}
    <span class="menu" role="menu" aria-label={t('chatc.export.tip')}>
      <button type="button" role="menuitem" class="mi" onclick={save}><Icon name="download" size={14} /> {t('chatc.export.md')}</button>
      <button type="button" role="menuitem" class="mi" onclick={copy}><Icon name="copy" size={14} /> {t('chatc.export.copy')}</button>
      <button type="button" role="menuitem" class="mi" onclick={print}><Icon name="print" size={14} /> {t('chatc.export.print')}</button>
    </span>
  {/if}
</span>

<style>
  .em { position: relative; display: inline-flex; }
  .menu { position: absolute; inset-block-start: calc(100% + 4px); inset-inline-end: 0; z-index: 20; min-width: 220px; padding: 4px; display: grid; gap: 1px;
    background: var(--panel); border: var(--border-w) solid var(--hairline); border-radius: var(--radius); box-shadow: var(--shadow-lg); }
  .mi { font: inherit; font-size: 13px; color: var(--fg); background: none; border: 0; text-align: start; padding: 7px 10px; border-radius: 6px; cursor: pointer;
    display: flex; align-items: center; gap: 8px; white-space: nowrap; }
  .mi:hover, .mi:focus-visible { background: var(--fill); }
  @media print { .em { display: none; } }
</style>
