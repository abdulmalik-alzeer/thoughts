<script lang="ts">
  // Customise › Plugins › Export…: the name, version, maker and description the file will carry.
  // A loose item gets its name here and becomes a named plugin (one History entry), so later exports
  // keep its id and version. Then the file is saved, and its fingerprint shown.
  import Icon from '../lib/Icon.svelte';
  import { app } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { errDetail } from '../lib/errors';
  import { trapFocus } from '../lib/focus';
  import { describePlugin, exportPlugin, type PluginItem, type Saved } from '../lib/plugins';

  let { plugin, close, done }: { plugin: PluginItem; close: () => void; done: (saved: Saved, name: string) => void } = $props();

  // (The form starts from the plugin as it was when Export… was pressed.)
  const start = (() => plugin)();
  let who = $state({
    name: start.name, version: start.version || '1.0.0',
    author: start.author || String(app.settings?.userName ?? '').trim(), description: start.description,
  });
  let busy = $state(false);
  let error = $state('');

  async function save() {
    if (busy) return;
    const name = who.name.trim();
    if (!name) { error = t('plg.exp.needName'); return; }
    busy = true;
    error = '';
    try {
      const words = { name, version: who.version.trim(), author: who.author.trim(), description: who.description.trim() };
      const same = start.named && words.name === start.name && words.version === start.version && words.author === start.author && words.description === start.description;
      // Named (or renamed) first, so the file carries exactly what the list shows afterwards.
      const id = same ? start.id : (await describePlugin(start.id, words)).id;
      done(await exportPlugin(id), name);
    } catch (e) { error = `${t('plg.exp.failed')} ${errDetail(e) ?? ''}`.trim(); }
    finally { busy = false; }
  }

  /** The dialog belongs to the whole window: a page's entry animation would otherwise hold it inside the page. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
  }
</script>

<div class="modal-bg no-print" role="presentation" use:portal onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="modal share" role="dialog" aria-modal="true" aria-labelledby="pe-title" tabindex="-1" use:trapFocus={{ onEscape: close }}>
    <h2 id="pe-title" dir="auto">{t('plg.exp.title', { name: start.name })}</h2>
    <p class="muted sm">{t('plg.exp.intro')}</p>
    {#if !start.named}<p class="muted sm">{t('plg.exp.looseNote')}</p>{/if}
    <form class="fields" onsubmit={(e) => { e.preventDefault(); save(); }}>
      <label>{t('plg.exp.name')}<input dir="auto" maxlength="80" required bind:value={who.name} aria-invalid={error === t('plg.exp.needName')} /></label>
      <div class="two">
        <label>{t('plg.exp.author')}<input dir="auto" maxlength="80" bind:value={who.author} /></label>
        <label>{t('plg.exp.version')}<input dir="ltr" maxlength="20" pattern="[0-9A-Za-z.+\-]*" bind:value={who.version} /></label>
      </div>
      <label>{t('plg.exp.about')}<textarea dir="auto" rows="2" maxlength="400" bind:value={who.description}></textarea></label>
      {#if error}<p class="err sm" role="alert">{error}</p>{/if}
      <div class="row end">
        <button type="button" class="btn" onclick={close}>{t('cancel')}</button>
        <button type="submit" class="btn primary" disabled={busy} aria-busy={busy}><Icon name="download" size={15} /> {busy ? t('plg.exp.saving') : t('plg.exp.save')}</button>
      </div>
    </form>
  </div>
</div>

<style>
  .share { max-width: 520px; display: grid; gap: 10px; }
  .share h2 { margin: 0; font-size: 17px; }
  .share p { margin: 0; }
  .sm { font-size: var(--fs-xs); }
  .fields { display: grid; gap: 10px; }
  .fields label { display: grid; gap: 4px; font-size: 13px; }
  .two { display: grid; grid-template-columns: 2fr 1fr; gap: 10px; }
  .err { color: var(--danger); margin: 0; }
  .end { justify-content: flex-end; }
</style>
