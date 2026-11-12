<script lang="ts">
  // Bring a folder of notes the user already has into the library (FEAT-09): an Obsidian vault, a
  // Notion or Bear export, any folder of documents. A preview says what comes in, what is left out
  // and why, and where it goes; nothing happens before the user says so. The user's own folder is
  // only read. The copy is one entry in History, and the toast afterwards can undo it.
  import Icon from '../lib/Icon.svelte';
  import { post } from '../lib/api';
  import { toast } from '../lib/store.svelte';
  import { t, fmtNum } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { trapFocus } from '../lib/focus';
  import { fmtSize } from '../lib/content/util';

  let { close, done }: { close: () => void; done?: (into: string) => void } = $props();

  let path = $state('');
  let into = $state('');
  let intoTouched = $state(false);
  let preview = $state<any>(null);
  let looking = $state(false);
  let lookErr = $state('');
  let busy = $state(false);

  const fullPath = (p: string) => /^(~\/|\/|[A-Za-z]:[\\/])/.test(p);
  let seq = 0;
  async function look() {
    const p = path.trim();
    const n = ++seq;
    lookErr = '';
    if (!p || !fullPath(p)) { preview = null; return; }
    looking = true;
    try {
      // The name the user typed is checked when they bring the notes in (a clash is said then).
      const v = await post('/library/import-folder/preview', { path: p });
      if (n !== seq) return;
      preview = v;
      if (!intoTouched) into = String(v?.into ?? '');
    } catch (e: any) {
      if (n === seq) { preview = null; lookErr = e?.message || t('sb.imp.previewFailed'); }
    } finally { if (n === seq) looking = false; }
  }
  // Typing a path is looked up after a short pause, not letter by letter.
  let timer: ReturnType<typeof setTimeout> | undefined;
  function typed() { clearTimeout(timer); timer = setTimeout(look, 400); }

  async function browse() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const p = await open({ directory: true });
      if (typeof p === 'string') { path = p; look(); }
    } catch { toast(t('sb.bring.needPick'), { kind: 'error' }); }
  }

  const leftLines = $derived(Object.entries(preview?.leftBy ?? {}).filter(([, n]) => Number(n) > 0).map(([w, n]) => t('sb.imp.left.' + w, { n: fmtNum(Number(n)) })));
  const canGo = $derived(!!preview && !preview.tooBig && Number(preview.files) > 0 && !!into.trim() && !looking && !busy);

  async function go() {
    if (!canGo) return;
    busy = true;
    try {
      const r = await post('/library/import-folder', { path: path.trim(), into: into.trim() });
      const where = String(r?.into ?? into);
      const hash = r?.hash ? String(r.hash) : '';
      toast(t('sb.imp.done', { into: where }), {
        kind: 'success', duration: 15000,
        action: hash ? { label: t('sb.imp.undoNow'), run: () => undo(hash, where) } : undefined,
      });
      done?.(where);
      close();
    } catch (e) {
      failed(t('sb.imp.failed'), e);
    } finally { busy = false; }
  }

  async function undo(hash: string, where: string) {
    try { await post('/library/undo', { hash }); toast(t('sb.imp.undone'), { kind: 'success' }); done?.(where); }
    catch (e) { failed(t('sb.imp.undoFailed'), e); }
  }
</script>

<div class="modal-bg no-print" role="presentation" onclick={(e) => e.target === e.currentTarget && !busy && close()}>
  <div class="modal imp" role="dialog" aria-modal="true" aria-labelledby="imp-title" aria-describedby="imp-body" tabindex="-1"
    use:trapFocus={{ onEscape: () => { if (!busy) close(); } }}>
    <h1 id="imp-title"><Icon name="upload" size={20} /> {t('sb.imp.title')}</h1>
    <p id="imp-body" class="muted">{t('sb.imp.body')}</p>

    <label for="imp-path">{t('sb.bring.pathLabel')}</label>
    <div class="row">
      <input id="imp-path" class="grow" dir="ltr" bind:value={path} oninput={typed} autocomplete="off" data-autofocus aria-describedby="imp-says" />
      <button type="button" class="btn" onclick={browse}><Icon name="folder" size={14} /> {t('sb.imp.browse')}</button>
    </div>

    <div id="imp-says" role="status" aria-live="polite">
      {#if looking}<p class="muted sm">{t('sb.imp.looking')}</p>
      {:else if lookErr}<p class="warn"><Icon name="warning" size={15} /> <span>{lookErr}</span></p>
      {:else if preview?.tooBig}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sb.imp.tooBig')}</span></p>
      {:else if preview && !preview.files}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sb.imp.empty')}</span></p>
      {:else if preview}
        <p><b>{t('sb.imp.counts', { notes: fmtNum(preview.notes ?? 0), other: fmtNum(preview.other ?? 0), size: fmtSize(Number(preview.bytes) || 0) })}</b></p>
        {#if preview.sample?.length}
          <p class="muted sm">{t('sb.imp.sample')} {#each preview.sample.slice(0, 4) as f, i}{#if i}, {/if}<bdi>{f}</bdi>{/each}</p>
        {/if}
        {#if leftLines.length}
          <div class="left">
            <p class="muted sm">{t('sb.imp.leftTitle')}</p>
            <ul class="muted sm">{#each leftLines as l}<li>{l}</li>{/each}</ul>
          </div>
        {/if}
      {/if}
    </div>

    {#if preview && !preview.tooBig && preview.files}
      <label for="imp-into">{t('sb.imp.into')}</label>
      <input id="imp-into" dir="auto" bind:value={into} oninput={() => (intoTouched = true)} autocomplete="off" aria-describedby="imp-into-hint" />
      <p class="muted sm" id="imp-into-hint">{t('sb.imp.intoHint')} {t(preview.undo ? 'sb.imp.undo' : 'sb.imp.noUndo')}</p>
    {/if}

    <div class="row end">
      <button type="button" class="btn" disabled={busy} onclick={close}>{t('cancel')}</button>
      <button type="button" class="btn primary" disabled={!canGo} aria-busy={busy} onclick={go}>{busy ? t('sb.imp.going') : t('sb.imp.go')}</button>
    </div>
  </div>
</div>

<style>
  .imp { max-width: 560px; width: 100%; padding: 22px 24px; }
  .imp h1 { display: flex; gap: 10px; align-items: center; font-size: 20px; margin: 0 0 6px; }
  .imp p { margin: 6px 0; }
  .imp input { width: 100%; }
  .row .grow { min-width: 0; }
  .warn { display: flex; gap: 8px; align-items: flex-start; font-size: 13px; }
  .warn :global(.icon) { flex: none; margin-top: 3px; color: var(--danger); }
  .left ul { margin: 2px 0 0; padding-inline-start: 20px; }
  .end { justify-content: flex-end; gap: 8px; margin-top: 16px; }
  .sm { font-size: var(--fs-xs); }
</style>
