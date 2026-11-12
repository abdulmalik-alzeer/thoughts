<script lang="ts">
  // Customise › Share: save a pack with a name, its maker and a version (and see its fingerprint),
  // and add a pack someone sent, after seeing what it says it is and the fingerprint the app works out.
  import Icon from '../lib/Icon.svelte';
  import { app, toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { t, tn } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { trapFocus } from '../lib/focus';
  import { packChoices, readPack, stagePack } from '../lib/layer';
  import { inspectPack, savePack } from '../lib/packs';

  let busy = $state<Record<string, boolean>>({});
  let picker = $state<HTMLInputElement>();

  // ----- sharing -----
  let form = $state<null | { some: boolean }>(null);
  let who = $state({ name: '', author: '', version: '1', description: '' });
  let choices = $state<Awaited<ReturnType<typeof packChoices>>>([]);
  let chosen = $state<Record<string, boolean>>({});
  let formError = $state('');
  let result = $state<null | { copied: boolean; fp: string }>(null);
  const nChosen = $derived(Object.values(chosen).filter(Boolean).length);

  async function openForm(some: boolean) {
    if (busy.open) return;
    busy.open = true;
    try {
      if (some) {
        const list = await packChoices();
        if (!list.length) { toast(t('layer3.exportNothing')); return; }
        choices = list;
        chosen = {};
      }
      formError = '';
      who = { name: who.name || t('ldc.share.namePh'), author: who.author || String(app.settings?.userName ?? '').trim(), version: who.version || '1', description: who.description };
      form = { some };
    } catch (e) { failed(t('layer3.exportListFailed'), e); }
    finally { busy.open = false; }
  }

  async function save() {
    if (!form || busy.save) return;
    if (!who.name.trim()) { formError = t('ldc.share.needName'); return; }
    const paths = form.some ? Object.entries(chosen).filter(([, v]) => v).map(([k]) => k) : undefined;
    if (form.some && !paths?.length) { formError = t('ldc.share.needItems'); return; }
    busy.save = true;
    try {
      const r = await savePack({ name: who.name.trim(), author: who.author.trim(), version: who.version.trim(), description: who.description.trim() }, paths);
      form = null;
      if (r.result !== 'cancelled') result = { copied: r.result === 'copied', fp: r.fingerprint };
    } catch (e) { failed(t('layer.exportFailed'), e); }
    finally { busy.save = false; }
  }

  async function copyFp() {
    if (!result) return;
    try { await navigator.clipboard.writeText(result.fp); toast(t('ldc.share.fpCopied'), { kind: 'success' }); } catch { /* the code is on screen */ }
  }

  // ----- adding a pack someone sent -----
  /** A pack's own words as one plain line: nothing hidden, nothing that reorders it, no quotation marks. */
  function packWords(v: unknown, max: number): string {
    return String(v ?? '')
      .replace(/[\u0000-\u001f\u007f-\u009f\u00ad\u180e\u200b\u2028-\u202e\u2060-\u206f\ufeff]|[\u{e0000}-\u{e0fff}]/gu, ' ')
      .replace(/["“”«»]/g, '').replace(/\s+/g, ' ').trim().slice(0, max);
  }

  function pick(e: Event) {
    const el = e.target as HTMLInputElement;
    const f = el.files?.[0];
    el.value = '';
    if (!f || busy.import) return;
    busy.import = true;
    (async () => {
      const r = await readPack(f);
      if (r.unsupported) { toast(t('layer3.packNewer'), { kind: 'error' }); return; }
      // The service checks the whole pack and works out its fingerprint before anything is staged.
      const look = await inspectPack(r.pack);
      const name = packWords(look.name, 80);
      const lines: string[] = [];
      const said = [name ? t('ldc.imp.who', { name }) : '', look.author ? t('ldc.imp.by', { author: packWords(look.author, 80) }) : '', look.version ? t('ldc.imp.version', { version: packWords(look.version, 20) }) : ''].filter(Boolean);
      if (said.length) lines.push(said.join(', ') + '.', t('ldc.imp.claims'));
      lines.push(t(look.kind === 'feature' ? 'layer3.importFeature' : 'layer3.importFull') + ' ' + tn('layer3.importFiles', look.files.length));
      if (look.matches === false) lines.push(t('ldc.imp.fpBad', { claimed: look.claimed ?? '' }));
      lines.push(t('ldc.imp.fp', { fp: look.code }));
      if (look.earlier && !look.earlier.same) lines.push(look.earlier.version ? t('ldc.imp.earlier', { version: packWords(look.earlier.version, 20), fp: look.earlier.code }) : t('ldc.imp.earlierNoVersion', { fp: look.earlier.code }));
      if (!look.changes) lines.push(t('ldc.imp.same'));
      const says = packWords(look.description, 400);
      if (says) lines.push(`${t('lda.packSaysIntro')}\n“${says}”`);
      const ok = await confirmDialog({
        title: name ? `${t('layer3.importTitle')} “${name}”` : t('layer3.importTitle'),
        body: lines.join('\n\n'),
        list: look.files.slice(0, 30),
        note: t('layer3.importNote'),
        confirmLabel: t('layer3.importGo'),
        danger: look.matches === false,
      });
      if (ok) await stagePack(r.pack);
    })().catch((err) => failed(t('ldc.imp.failed'), err)).finally(() => { busy.import = false; });
  }
</script>

<div class="card">
  <h3>{t('ldc.share.title')}</h3>
  <p class="muted sm">{t('layer.shareHint')}</p>
  <div class="row wrap">
    <button type="button" class="btn" disabled={busy.open || busy.save} onclick={() => openForm(false)}><Icon name="download" size={15} /> {t('ldc.share.all')}</button>
    <button type="button" class="btn" disabled={busy.open || busy.save} onclick={() => openForm(true)}>{t('ldc.share.some')}</button>
  </div>
  <p class="muted sm gap"><Icon name="lock" size={13} /> {t('ldc.share.privacy')}</p>
  {#if result}
    <div class="done" role="status">
      <p>{t(result.copied ? 'ldc.share.copied' : 'ldc.share.saved')} <bdi class="code" dir="ltr">{result.fp}</bdi></p>
      <p class="muted sm">{t('ldc.share.fpHint')}</p>
      <div class="row wrap">
        <button type="button" class="btn sm" onclick={copyFp}><Icon name="copy" size={13} /> {t('ldc.share.copyFp')}</button>
        <button type="button" class="btn ghost sm" onclick={() => (result = null)}>{t('close')}</button>
      </div>
    </div>
  {/if}
</div>

<div class="card">
  <h3>{t('ldc.imp.title')}</h3>
  <p class="muted sm">{t('layer.importHint')}</p>
  <button type="button" class="btn" disabled={busy.import} aria-busy={busy.import} onclick={() => picker?.click()}><Icon name="upload" size={15} /> {busy.import ? t('ldc.imp.checking') : t('layer.import')}</button>
  <input type="file" accept=".json,application/json" hidden bind:this={picker} onchange={pick} />
</div>

{#if form}
  <div class="modal-bg no-print" role="presentation" onclick={(e) => e.target === e.currentTarget && (form = null)}>
    <div class="modal share" role="dialog" aria-modal="true" aria-labelledby="sp-title" tabindex="-1" use:trapFocus={{ onEscape: () => (form = null) }}>
      <h2 id="sp-title">{t(form.some ? 'ldc.share.dialogSome' : 'ldc.share.dialog')}</h2>
      <form class="fields" onsubmit={(e) => { e.preventDefault(); save(); }}>
        <label>{t('ldc.share.name')}<input dir="auto" maxlength="80" required bind:value={who.name} placeholder={t('ldc.share.namePh')} aria-invalid={formError === t('ldc.share.needName')} /></label>
        <div class="two">
          <label>{t('ldc.share.author')}<input dir="auto" maxlength="80" bind:value={who.author} /></label>
          <label>{t('ldc.share.version')}<input dir="auto" maxlength="20" bind:value={who.version} /></label>
        </div>
        <label>{t('ldc.share.about')}<textarea dir="auto" rows="2" maxlength="400" bind:value={who.description} placeholder={t('ldc.share.aboutPh')}></textarea></label>
        {#if form.some}
          <fieldset>
            <legend>{t('ldc.share.choose')}</legend>
            <p class="muted sm">{t('layer3.exportChooseHint')}</p>
            <ul class="inst">
              {#each choices as c (c.path)}
                <li><label class="ck"><input type="checkbox" bind:checked={chosen[c.path]} /> <span class="pill">{t('layer3.kind.' + c.kind)}</span> <span class="grow" dir="auto">{c.label}</span></label></li>
              {/each}
            </ul>
          </fieldset>
        {/if}
        {#if formError}<p class="err sm" role="alert">{formError}</p>{/if}
        <div class="row end">
          <button type="button" class="btn" onclick={() => (form = null)}>{t('cancel')}</button>
          <button type="submit" class="btn primary" disabled={busy.save || (form.some && !nChosen)} aria-busy={busy.save}><Icon name="download" size={15} /> {busy.save ? t('ldc.share.saving') : t('ldc.share.save')}</button>
        </div>
      </form>
    </div>
  </div>
{/if}

<style>
  h3 { font-size: 14px; font-weight: 600; margin: 0 0 6px; }
  .card { margin-bottom: 12px; }
  .wrap { flex-wrap: wrap; }
  .sm { font-size: var(--fs-xs); }
  .gap { margin: 10px 0 0; }
  .done { margin-top: 10px; padding: 10px 12px; border-radius: var(--radius); background: var(--fill); display: grid; gap: 6px; }
  .done p { margin: 0; }
  .code { font-family: var(--mono); font-weight: 600; overflow-wrap: anywhere; }
  .share { max-width: 540px; display: grid; gap: 10px; }
  .share h2 { margin: 0; font-size: 17px; }
  .fields { display: grid; gap: 10px; }
  .fields label { display: grid; gap: 4px; font-size: 13px; }
  .two { display: grid; grid-template-columns: 2fr 1fr; gap: 10px; }
  fieldset { border: 0; padding: 0; margin: 0; }
  legend { font-size: 13px; font-weight: 600; padding: 0; }
  .inst { list-style: none; margin: 0; padding: 0; max-height: 240px; overflow: auto; }
  .inst li { padding: 6px 0; border-bottom: 1px solid var(--hairline); }
  .ck { display: flex !important; align-items: center; gap: 8px; margin: 0; font-weight: 400; cursor: pointer; min-height: 24px; }
  .err { color: var(--danger); margin: 0; }
  .end { justify-content: flex-end; }
</style>
