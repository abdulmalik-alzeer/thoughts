<script lang="ts">
  // Settings › Maintenance: the check-up with its fixes, moving to another computer, and removing
  // Thoughts from this computer. (The weekly version check is a setting; Settings shows it.)
  import Icon from '../lib/Icon.svelte';
  import { onMount, tick } from 'svelte';
  import { api, halt, inTauri, post } from '../lib/api';
  import { toast } from '../lib/store.svelte';
  import { confirmDialog, promptDialog } from '../lib/dialog.svelte';
  import { fmtDate, fmtNum, t, tn } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { restartService } from '../lib/shell';
  import { healthIssues, type Issue } from '../lib/maintenance';

  let { show, open }: {
    /** Which parts the Settings search shows (by label key). */
    show: (...keys: string[]) => boolean;
    /** Open another Settings section: 'git' for the library, 'ai' for the provider key. */
    open: (section: 'git' | 'ai') => void;
  } = $props();

  // ---------- check-up ----------
  let issues = $state<Issue[] | null>(null);
  let checkErr = $state(false);
  let checking = $state(false);
  let fixing = $state<Record<string, boolean>>({});

  async function refresh() {
    checking = true; checkErr = false;
    try { issues = await healthIssues(); } catch { checkErr = true; }
    finally { checking = false; }
  }
  onMount(refresh);

  const issueTitle = (i: Issue) => (i.n != null ? tn(`care.issue.${i.id}.title`, i.n) : t(`care.issue.${i.id}.title`));

  async function fix(i: Issue) {
    if (!i.fix || fixing[i.id]) return;
    if (i.fix === 'choose-library') { open('git'); return; }
    if (i.fix === 'enter-key') { open('ai'); return; }
    if (i.fix === 'finish-history' && !(await confirmDialog({ title: t('care.fix.finishTitle'), body: t('care.fix.finishBody'), confirmLabel: t('care.fix.finish-history') }))) return;
    fixing[i.id] = true;
    try {
      if (i.fix === 'install-git') {
        const r = await post('/git/install');
        if (r?.url) { try { const { openUrl } = await import('@tauri-apps/plugin-opener'); await openUrl(r.url); } catch { window.open(r.url, '_blank', 'noopener'); } }
        toast(t('ob.gitInstallToast'), r?.hint ? { detail: String(r.hint), duration: 0 } : {});
        return;
      }
      if (i.fix === 'restart-service') { if (await restartService()) await refresh(); return; }
      const r = await post('/health/fix', { id: i.fix });
      issues = Array.isArray(r?.issues) ? r.issues : issues;
      const still = (issues ?? []).some((x) => x.id === i.id);
      toast(t(still ? 'care.health.fixFailed' : 'care.health.fixed'), { kind: still ? 'error' : 'success', key: 'health' });
    } catch (e) { failed(t('care.health.fixFailed'), e); }
    finally { fixing[i.id] = false; }
  }

  // ---------- move to another computer ----------
  let exporting = $state(false);
  let exported = $state<any>(null);
  async function exportMove() {
    if (exporting) return;
    exporting = true;
    try { exported = await post('/move/export'); }
    catch (e) { failed(t('care.move.exportFailed'), e); }
    finally { exporting = false; }
  }
  async function reveal() {
    try { const { revealItemInDir } = await import('@tauri-apps/plugin-opener'); await revealItemInDir(exported.path); } catch { /* not in the app window */ }
  }

  let fileInput = $state<HTMLInputElement | null>(null);
  /** The file being imported. Kept out of reactive state: it can be large. */
  let bundle: any = null;
  let reading = $state(false);
  let inspected = $state<any>(null);
  let pick = $state({ chats: true, settings: true, customisations: true, replace: false, addonData: true, addonDataReplace: false });
  let folders = $state<Record<string, string>>({});
  let importing = $state(false);
  let imported = $state<any>(null);

  function resetImport() { bundle = null; inspected = null; imported = null; folders = {}; if (fileInput) fileInput.value = ''; }

  async function chooseFile(e: Event) {
    const f = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!f) return;
    resetImport();
    reading = true;
    try {
      let parsed: any;
      try { parsed = JSON.parse(await f.text()); } catch { toast(t('care.move.notFile'), { kind: 'error' }); return; }
      const info = await post('/move/inspect', { bundle: parsed });
      bundle = parsed;
      folders = Object.fromEntries((info.projects ?? []).map((p: any) => [p.id, p.here ? p.path : '']));
      pick = { chats: info.chats > 0, settings: !!info.settings, customisations: info.customisations > 0, replace: false, addonData: (info.addonData ?? 0) > 0, addonDataReplace: false };
      inspected = info;
    } catch (err) { failed(t('care.move.failed'), err); }
    finally { reading = false; if (fileInput) fileInput.value = ''; }
  }

  async function chooseFolder(id: string) {
    let p: string | null = null;
    if (inTauri()) {
      try { const { open: pickDir } = await import('@tauri-apps/plugin-dialog'); const r = await pickDir({ directory: true }); p = typeof r === 'string' ? r : null; }
      catch { p = null; }
      if (p === null) return;
    } else {
      p = await promptDialog({ title: t('care.move.typeTitle'), body: t('care.move.typeBody'), value: folders[id] || '' });
    }
    if (p !== null && p.trim()) folders[id] = p.trim();
  }

  const nothingPicked = $derived(!!inspected && !((pick.chats && inspected.chats > 0) || (pick.settings && inspected.settings) || (pick.customisations && inspected.customisations > 0) || (pick.addonData && inspected.addonData > 0)));

  async function importMove() {
    if (!bundle || importing || nothingPicked) return;
    importing = true;
    try {
      const choices = { chats: pick.chats, settings: pick.settings, customisations: pick.customisations, replace: pick.replace, projects: $state.snapshot(folders), addonData: pick.addonData, addonDataReplace: pick.addonDataReplace };
      const r = await post('/move/import', { bundle, choices });
      imported = r;
      bundle = null; inspected = null;
      toast(t('care.move.done', { added: fmtNum(r.chatsAdded ?? 0), replaced: fmtNum(r.chatsReplaced ?? 0), skipped: fmtNum(r.chatsSkipped ?? 0) }), { kind: 'success', key: 'move' });
      if (r.customisations === 'failed') toast(t('care.move.customFailed'), { kind: 'error', detail: r.customisationsError || undefined });
      const data = (r.addonData?.added ?? 0) + (r.addonData?.replaced ?? 0);
      if (data) toast(t('ldc.move.dataDone', { n: fmtNum(data) }), { kind: 'success', key: 'move-data' });
    } catch (e) { failed(t('care.move.failed'), e); }
    finally { importing = false; }
  }

  // ---------- remove Thoughts from this computer ----------
  let rmPlan = $state<any>(null);
  let rmOpen = $state(false);
  let rmErr = $state('');
  let deleteData = $state(true);
  let typed = $state('');
  let removing = $state(false);
  let removed = $state<any>(null);
  let doneHeading = $state<HTMLElement | null>(null);
  const word = $derived(t('care.rm.word'));
  /** A path inside a sentence keeps its own direction (Arabic text around a left-to-right path). */
  const iso = (p: unknown) => String.fromCharCode(0x2068) + String(p ?? '') + String.fromCharCode(0x2069);
  const typedOk = $derived([word, 'remove'].includes(typed.trim().toLowerCase()));

  async function startRemove() {
    rmErr = ''; typed = '';
    try {
      rmPlan = await api('/remove/plan');
      deleteData = !rmPlan.blocked;
      rmOpen = true;
    } catch (e) { failed(t('care.rm.failed'), e); }
  }

  async function doRemove() {
    if (!typedOk || removing || !rmPlan) return;
    removing = true; rmErr = '';
    try {
      const r = await post('/remove', { deleteData: deleteData && !rmPlan.blocked, confirm: rmPlan.confirm });
      // From here on nothing may reach the background service: it would start a new one.
      halt();
      if (!r.dataKept) {
        try { for (const k of Object.keys(localStorage)) if (k.startsWith('thoughts.')) localStorage.removeItem(k); } catch { /* ignore */ }
      }
      removed = r;
      await tick();
      doneHeading?.focus();
    } catch (e: any) {
      if (e?.code === 'busy' || e?.status === 409) rmErr = t('care.rm.busy');
      else failed(t('care.rm.failed'), e);
    } finally { removing = false; }
  }

  let quitFailed = $state(false);
  async function quit() {
    try { const { invoke } = await import('@tauri-apps/api/core'); await invoke('quit_app'); }
    catch { quitFailed = true; }
  }

  /** The "removed" screen covers the whole window (the sidebar included), not only this page. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
  }

</script>

{#if show('care.health.title')}
  <div class="card">
    <h3>{t('care.health.title')}</h3>
    <p class="muted sm">{t('care.health.intro')}</p>
    {#if issues === null && checking}
      <p class="muted sm" role="status">{t('care.health.checking')}</p>
    {:else if checkErr}
      <p class="err" role="alert">{t('care.health.failed')}</p>
    {:else if issues && issues.length === 0}
      <p class="ok" role="status"><Icon name="check" size={15} /> {t('care.health.ok')}</p>
    {:else if issues}
      <ul class="issues">
        {#each issues as i (i.id)}
          <li class="issue">
            <Icon name="warning" size={16} />
            <div class="grow">
              <b>{issueTitle(i)}</b>
              <p class="muted sm">{t(`care.issue.${i.id}.body`)}</p>
            </div>
            {#if i.fix}<button type="button" class="btn" disabled={fixing[i.id]} aria-busy={fixing[i.id]} onclick={() => fix(i)}>{t('care.fix.' + i.fix)}</button>{/if}
          </li>
        {/each}
      </ul>
    {/if}
    <div class="row gap-top"><button type="button" class="btn ghost" disabled={checking} aria-busy={checking} onclick={refresh}><Icon name="sync" size={14} /> {checking ? t('care.health.checking') : t('care.health.again')}</button></div>
  </div>
{/if}

{#if show('care.move.title')}
  <div class="card">
    <h3>{t('care.move.title')}</h3>
    <p class="muted sm">{t('care.move.intro')}</p>
    <div class="row wrap gap-top">
      <button type="button" class="btn" disabled={exporting} aria-busy={exporting} onclick={exportMove}><Icon name="download" size={14} /> {exporting ? t('care.move.exporting') : t('care.move.export')}</button>
      <button type="button" class="btn" disabled={reading || importing} aria-busy={reading} onclick={() => fileInput?.click()}><Icon name="upload" size={14} /> {reading ? t('care.move.reading') : t('care.move.import')}</button>
      <input bind:this={fileInput} class="hidden" type="file" accept=".json,application/json" tabindex="-1" aria-hidden="true" onchange={chooseFile} />
    </div>
    {#if exported}
      <div class="result" role="status">
        <p>{t('care.move.saved', { name: exported.name ?? '', folder: exported.folder ?? '', chats: fmtNum(exported.info?.chats ?? 0), files: fmtNum(exported.info?.attachments ?? 0) })}</p>
        {#if exported.info?.attachmentsLeft}<p class="muted sm">{tn('care.move.left', exported.info.attachmentsLeft)}</p>{/if}
        {#if inTauri()}<button type="button" class="btn ghost" onclick={reveal}><Icon name="folder" size={14} /> {t('care.move.show')}</button>{/if}
      </div>
    {/if}
    <p class="note"><Icon name="lock" size={15} /> <span>{t('care.move.safe')}</span></p>

    {#if inspected}
      <div class="panel" role="group" aria-label={t('care.move.import')}>
        {#if inspected.appVersion || inspected.createdAt}
          <p class="muted sm">{t('care.move.from', { version: inspected.appVersion || '?', date: inspected.createdAt ? fmtDate(inspected.createdAt) : '?' })}</p>
        {/if}
        {#if inspected.chats > 0}
          <label class="check"><input type="checkbox" bind:checked={pick.chats} /> {t('care.move.chats', { n: fmtNum(inspected.chats) })}</label>
          {#if pick.chats && inspected.existing > 0}
            <div class="sub">
              <p class="muted sm">{tn('care.move.existing', inspected.existing)}</p>
              <label class="check"><input type="checkbox" bind:checked={pick.replace} /> {t('care.move.replace')}</label>
              <p class="muted sm">{pick.replace ? t('care.move.running') : t('care.move.keepHint')}</p>
            </div>
          {/if}
        {/if}
        {#if inspected.settings}
          <label class="check"><input type="checkbox" bind:checked={pick.settings} aria-describedby="care-set-hint" /> {t('care.move.settings')}</label>
          <p class="muted sm sub" id="care-set-hint">{t('care.move.settingsHint')}</p>
        {/if}
        {#if inspected.customisations > 0}
          <label class="check"><input type="checkbox" bind:checked={pick.customisations} aria-describedby="care-cust-hint" /> {t('care.move.custom', { n: fmtNum(inspected.customisations) })}</label>
          <p class="muted sm sub" id="care-cust-hint">{t('care.move.customHint')}</p>
        {/if}
        {#if inspected.addonData > 0}
          <label class="check"><input type="checkbox" bind:checked={pick.addonData} aria-describedby="care-data-hint" /> {t('ldc.move.data', { n: fmtNum(inspected.addonData) })}</label>
          <p class="muted sm sub" id="care-data-hint">{t('ldc.move.dataHint')}</p>
          {#if pick.addonData && inspected.addonDataExisting > 0}
            <div class="sub">
              <label class="check"><input type="checkbox" bind:checked={pick.addonDataReplace} aria-describedby="care-data-rep-hint" /> {t('ldc.move.dataReplace', { n: fmtNum(inspected.addonDataExisting) })}</label>
              <p class="muted sm" id="care-data-rep-hint">{t('ldc.move.dataReplaceHint')}</p>
            </div>
          {/if}
        {/if}
        {#if pick.chats && inspected.projects?.length}
          <h4>{t('care.move.folders')}</h4>
          <p class="muted sm">{t('care.move.foldersHint')}</p>
          {#each inspected.projects as p (p.id)}
            <div class="row wrap folder">
              <div class="grow">
                <b dir="auto">{p.name}</b>
                <div class="muted sm path"><bdi>{t('care.move.oldPath', { path: iso(p.path) })}</bdi></div>
                {#if folders[p.id]}<code class="sm path"><bdi>{folders[p.id]}</bdi></code>{/if}
                {#if p.linked && folders[p.id] === p.path}<div class="muted sm">{t('care.move.linkedHere', { name: p.linked })}</div>{/if}
              </div>
              <button type="button" class="btn" onclick={() => chooseFolder(p.id)}><Icon name="folder" size={14} /> {t('care.move.choose')}</button>
              {#if folders[p.id]}<button type="button" class="btn ghost" onclick={() => (folders[p.id] = '')}>{t('care.move.skip')}</button>{/if}
            </div>
          {/each}
        {/if}
        {#if nothingPicked}<p class="err" role="alert">{t('care.move.nothing')}</p>{/if}
        <div class="row gap-top end">
          <button type="button" class="btn ghost" disabled={importing} onclick={resetImport}>{t('cancel')}</button>
          <button type="button" class="btn primary" disabled={importing || nothingPicked} aria-busy={importing} onclick={importMove}>{t('care.move.go')}</button>
        </div>
      </div>
    {/if}
    {#if imported}
      <div class="result" role="status">
        <p>{t('care.move.done', { added: fmtNum(imported.chatsAdded ?? 0), replaced: fmtNum(imported.chatsReplaced ?? 0), skipped: fmtNum(imported.chatsSkipped ?? 0) })}</p>
        {#if imported.settings}<p class="muted sm">{t('care.move.doneSettings')}</p>{/if}
        {#if imported.customisations === 'staged'}<p class="muted sm">{t('care.move.doneCustom')}</p>{/if}
      </div>
    {/if}
  </div>
{/if}

{#if show('care.rm.title')}
  <div class="card dangerzone">
    <h3>{t('care.rm.title')}</h3>
    <p class="muted sm">{t('care.rm.intro')}</p>
    {#if !rmOpen}
      <button type="button" class="btn danger" onclick={startRemove}><Icon name="trash" size={14} /> {t('care.rm.start')}</button>
    {:else if rmPlan}
      <div class="cols">
        <div>
          <h4>{t('care.rm.goes')}</h4>
          <ul class="list">
            {#if deleteData && !rmPlan.blocked}
              <li>{t('care.rm.g.chats')}</li>
              <li>{t('care.rm.g.settings')}</li>
              {#if rmPlan.keptHistory}<li>{t('care.rm.g.history')}</li>{/if}
            {/if}
            {#if rmPlan.shared}
              <li>{t('care.rm.g.passwords')}</li>
              <li>{t('care.rm.g.login')}</li>
            {/if}
            <li>{t('care.rm.g.service')}</li>
          </ul>
        </div>
        <div>
          <h4>{t('care.rm.stays')}</h4>
          <ul class="list">
            {#if rmPlan.library}<li>{t('care.rm.s.library')}<br /><code class="sm path"><bdi>{rmPlan.library}</bdi></code></li>{/if}
            {#if rmPlan.folders?.length}<li>{tn('care.rm.s.folders', rmPlan.folders.length)}</li>{/if}
            {#if !deleteData || rmPlan.blocked}<li>{t('care.rm.s.data')}</li>{/if}
            <li>{t('care.rm.s.synced')}</li>
            <li>{t('care.rm.s.provider')}</li>
            <li>{t('care.rm.s.app')}</li>
          </ul>
        </div>
      </div>
      {#if !rmPlan.shared}<p class="note"><Icon name="info" size={15} /> <span>{t('care.rm.sandbox')}</span></p>{/if}
      {#if rmPlan.blocked}
        <p class="note" role="status"><Icon name="warning" size={15} /> <span>{t('care.rm.blocked.' + rmPlan.blocked)}</span></p>
      {:else}
        <label class="check"><input type="checkbox" bind:checked={deleteData} aria-describedby="care-keep-hint" /> {t('care.rm.deleteData')}</label>
        <p class="muted sm sub" id="care-keep-hint">{t('care.rm.keepHint', { path: iso(rmPlan.dataFolder) })}</p>
      {/if}
      {#if rmPlan.busy}<p class="err" role="alert">{t('care.rm.busy')}</p>{/if}
      <label for="care-rm-type">{t('care.rm.type', { word })}</label>
      <input id="care-rm-type" dir="auto" bind:value={typed} autocomplete="off" spellcheck="false" onkeydown={(e) => { if (e.key === 'Enter') doRemove(); }} />
      {#if rmErr}<p class="err" role="alert">{rmErr}</p>{/if}
      <div class="row gap-top end">
        <button type="button" class="btn ghost" disabled={removing} onclick={() => { rmOpen = false; typed = ''; }}>{t('cancel')}</button>
        <button type="button" class="btn danger strong" disabled={!typedOk || removing} aria-busy={removing} onclick={doRemove}>{removing ? t('care.rm.working') : t('care.rm.go')}</button>
      </div>
    {/if}
  </div>
{/if}

{#if removed}
  <div class="gone" role="dialog" aria-modal="true" aria-labelledby="care-gone-title" use:portal>
    <div class="card">
      <h2 id="care-gone-title" tabindex="-1" bind:this={doneHeading}>{t('care.rm.doneTitle')}</h2>
      <p>{t('care.rm.doneBody')}</p>
      {#if removed.dataKept}<p class="muted sm">{t('care.rm.doneKept', { path: iso(rmPlan?.dataFolder) })}</p>{/if}
      {#if removed.passwords === 'failed'}<p class="note"><Icon name="warning" size={15} /> <span>{t('care.rm.r.passwords')}</span></p>{/if}
      {#if removed.loginItem === 'failed'}<p class="note"><Icon name="warning" size={15} /> <span>{t('care.rm.r.login')}</span></p>{/if}
      {#if removed.failed?.length}<p class="note"><Icon name="warning" size={15} /> <span>{t('care.rm.r.notDeleted', { path: iso(rmPlan?.dataFolder) })}</span></p>{/if}
      {#if removed.left?.length}<p class="muted sm">{tn('care.rm.r.unknown', removed.left.length, { path: iso(rmPlan?.dataFolder) })}</p>{/if}
      {#if quitFailed || !inTauri()}<p class="muted sm">{t('care.rm.quitByHand')}</p>
      {:else}<div class="row end gap-top"><button type="button" class="btn primary" onclick={quit}>{t('care.rm.quit')}</button></div>{/if}
    </div>
  </div>
{/if}

<style>
  h3 { font-size: 14px; font-weight: 600; margin: 0 0 6px; }
  h4 { font-size: 13px; font-weight: 600; margin: 14px 0 4px; }
  .card { margin-bottom: 12px; }
  .wrap { flex-wrap: wrap; }
  .gap-top { margin-top: 10px; }
  .end { justify-content: flex-end; }
  .sm { font-size: var(--fs-xs); }
  .grow { flex: 1; min-width: 0; }
  .hidden { display: none; }
  .path { overflow-wrap: anywhere; }
  .ok { display: flex; gap: 6px; align-items: center; color: var(--ok); font-size: 13px; margin: 8px 0 0; }
  .err { color: var(--danger); font-size: var(--fs-sm); margin: 6px 0 0; }
  .issues { list-style: none; margin: 8px 0 0; padding: 0; }
  .issue { display: flex; gap: 10px; align-items: flex-start; padding: 10px 0; border-bottom: 1px solid var(--hairline); }
  .issue:last-child { border-bottom: 0; }
  .issue :global(.icon) { flex: none; margin-top: 2px; color: var(--danger); }
  .issue p { margin: 2px 0 0; }
  .issue .btn { flex: none; }
  .note { display: flex; gap: 8px; align-items: flex-start; font-size: var(--fs-sm); margin: 10px 0; }
  .note :global(.icon) { flex: none; margin-top: 3px; color: var(--accent-2); }
  .result { margin-top: 10px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); font-size: 13px; }
  .result p { margin: 0 0 4px; }
  .panel { margin-top: 12px; padding: 12px; border: 1px solid var(--hairline); border-radius: var(--radius); }
  .check { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; margin-top: 8px; }
  .sub { padding-inline-start: 24px; margin: 2px 0 0; }
  .sub p { margin: 4px 0 0; }
  .folder { padding: 8px 0; border-bottom: 1px solid var(--hairline); align-items: center; }
  .dangerzone { border-color: color-mix(in srgb, var(--danger) 45%, var(--hairline)); }
  .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 0 16px; }
  .list { margin: 0; padding-inline-start: 18px; font-size: 13px; }
  .list li { margin: 3px 0; }
  .btn.danger.strong { background: var(--danger); color: #fff; }
  .gone { position: fixed; inset: 0; z-index: 1000; display: grid; place-items: center; padding: 24px; background: var(--bg); }
  .gone .card { max-width: 520px; }
  .gone h2 { font-size: 18px; margin: 0 0 8px; outline: none; }
  @container main (max-width: 700px) { .cols { grid-template-columns: 1fr; } }
</style>
