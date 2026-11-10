<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onDestroy, onMount, tick } from 'svelte';
  import { api, inTauri, on, post, put } from '../lib/api';
  import { md, mdClick, resolveLink } from '../lib/md';
  import { app, go, refreshChats } from '../lib/store.svelte';
  import { t, tn, fmtDate } from '../lib/i18n';
  import { trapFocus } from '../lib/focus';
  import { MAX_FILE_BYTES, copyText, fail, fmtSize, isImageName, notify, prettyName, rawBlobUrl, readAsDataURL } from '../lib/content/util';

  let { path }: { path?: string } = $props();

  let files = $state<any[]>([]);
  let loading = $state(true);
  let treeError = $state('');
  let current = $state<string>('');
  let doc = $state<any>(null);
  let docLoading = $state(false);
  let docError = $state('');
  let editing = $state(false);
  let draft = $state('');
  let saving = $state(false);
  let preview = $state(readPref());
  let results = $state<any[] | null>(null);
  let filter = $state('');
  let open = $state<Record<string, boolean>>({});
  let imgUrl = $state<string | null>(null);
  let importing = $state(false);
  let dragging = $state(false);
  let dragDepth = 0;
  let unsaved = $state<{ resolve: (v: 'save' | 'discard' | 'keep') => void } | null>(null);
  let article = $state<HTMLElement>();
  let picker = $state<HTMLInputElement>();

  const TEXT_EXT = /\.(txt|csv|tsv|json|ya?ml|toml|log|xml|html?|css|js|ts|py|sh|ini|conf)$/i;
  const desktop = inTauri();
  const isMac = /mac/i.test(navigator.platform || navigator.userAgent);
  const dirty = $derived(editing && !!doc && draft !== doc.content);
  const info = $derived(files.find((f) => f.path === current));
  const absPath = $derived(app.status?.library ? `${String(app.status.library).replace(/[\\/]+$/, '')}/${current}` : '');

  function readPref() { try { return localStorage.getItem('thoughts.docs.preview') !== '0'; } catch { return true; } }
  function togglePreview() { preview = !preview; try { localStorage.setItem('thoughts.docs.preview', preview ? '1' : '0'); } catch { /* private mode */ } }

  // Unsaved text is mirrored to sessionStorage so that leaving the view (or a crash) never loses it.
  const stashKey = (p: string) => `thoughts.draft:${p}`;
  const stashGet = (p: string) => { try { return sessionStorage.getItem(stashKey(p)); } catch { return null; } };
  const stashDrop = (p: string) => { try { sessionStorage.removeItem(stashKey(p)); } catch { /* ignore */ } };
  $effect(() => {
    if (!current || !editing) return;
    const d = draft, isDirty = dirty;
    try { if (isDirty) sessionStorage.setItem(stashKey(current), d); else sessionStorage.removeItem(stashKey(current)); } catch { /* ignore */ }
  });

  async function loadTree() {
    try { files = await api('/library/tree'); treeError = ''; }
    catch (e: any) { treeError = e?.message ?? String(e); }
    finally { loading = false; }
  }

  function beforeUnload(e: BeforeUnloadEvent) { if (dirty) { e.preventDefault(); e.returnValue = ''; } }
  function onKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's' && editing) { e.preventDefault(); save(); }
  }

  let off: (() => void) | undefined;
  onMount(async () => {
    window.addEventListener('beforeunload', beforeUnload);
    window.addEventListener('keydown', onKey);
    off = on((e) => { if (e.type === 'library.changed' || e.type === 'resync') loadTree(); });
    await loadTree();
    if (treeError) return;
    if (path?.startsWith('?')) {
      try { results = await api(`/library/search?q=${encodeURIComponent(path.slice(1))}`); } catch (e) { fail(e); results = []; }
    } else openFile(path || (files.some((f) => f.path === 'INDEX.md') ? 'INDEX.md' : files.find((f) => f.path.endsWith('.md'))?.path ?? files[0]?.path));
  });
  onDestroy(() => {
    window.removeEventListener('beforeunload', beforeUnload);
    window.removeEventListener('keydown', onKey);
    off?.();
    if (imgUrl) URL.revokeObjectURL(imgUrl);
    if (dirty) notify(t('docs.draftKept', { name: prettyName(current) }), { kind: 'info', action: { label: t('docs.reopen'), run: () => go({ kind: 'docs', path: current }) } });
  });

  /** Resolves true when it is safe to replace what is on screen. */
  async function leaveOk(): Promise<boolean> {
    if (!dirty) return true;
    const choice = await new Promise<'save' | 'discard' | 'keep'>((resolve) => { unsaved = { resolve }; });
    unsaved = null;
    if (choice === 'save') return save();
    if (choice === 'discard') { stashDrop(current); editing = false; return true; }
    return false;
  }

  async function openFile(p?: string) {
    if (!p) return;
    if (!(await leaveOk())) return;
    results = null; editing = false; current = p; docError = '';
    if (imgUrl) { URL.revokeObjectURL(imgUrl); imgUrl = null; }
    if (p.includes('/')) open[p.split('/')[0]] = true;
    if (!p.endsWith('.md')) {
      doc = { content: '', binary: true };
      if (isImageName(p)) rawBlobUrl(p).then((u) => { if (current === p) imgUrl = u; else if (u) URL.revokeObjectURL(u); });
      else if (TEXT_EXT.test(p)) api(`/library/file?path=${encodeURIComponent(p)}`).then((d) => { if (current === p) doc = { ...d, binary: true, plain: true }; }).catch(() => {});
      return;
    }
    docLoading = true;
    try {
      const d = await api(`/library/file?path=${encodeURIComponent(p)}`);
      if (current !== p) return;
      doc = d;
      const kept = stashGet(p);
      if (kept !== null && kept !== d.content) { draft = kept; editing = true; notify(t('docs.draftRestored'), { kind: 'info' }); }
    } catch (e: any) {
      if (current === p) { doc = null; docError = e?.message ?? String(e); }
    } finally { if (current === p) docLoading = false; }
  }

  function startEdit() { draft = doc.content; editing = true; }

  async function cancelEdit() {
    if (dirty && !(await leaveOk())) return;
    stashDrop(current);
    editing = false;
  }

  async function save(): Promise<boolean> {
    if (saving) return false;
    saving = true;
    try {
      await put('/library/file', { path: current, content: draft });
      doc = { ...doc, content: draft };
      stashDrop(current);
      editing = false;
      notify(t('docs.saved'));
      // Re-read so frontmatter pills and the rendered body reflect what is on disk.
      api(`/library/file?path=${encodeURIComponent(current)}`).then((d) => { if (d.path === current && !editing) doc = d; }).catch(() => {});
      return true;
    } catch (e: any) {
      notify(t('docs.saveFailed', { error: e?.message ?? String(e) }), { kind: 'error' });
      return false;
    } finally { saving = false; }
  }

  function clickLink(e: MouseEvent) {
    if (mdClick(e)) return;
    const a = (e.target as HTMLElement).closest('a');
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (/^[a-z]+:/i.test(href) || href.startsWith('#')) return;
    e.preventDefault();
    openFile(resolveLink(current, href));
  }

  async function discuss() {
    try {
      const c = await post('/chats', {});
      const slug = current.split('/').slice(0, 2).join('/');
      const text = current.endsWith('.md') ? `continue ${slug.split('/')[1] || current}` : t('docs.discussPrompt', { path: current });
      await post(`/chats/${c.id}/send`, { text, mentions: [current] });
      await refreshChats().catch(() => {});
      go({ kind: 'chat', id: c.id });
    } catch (e) { fail(e); }
  }

  async function openExternal() {
    try { const { openPath } = await import('@tauri-apps/plugin-opener'); await openPath(absPath); }
    catch { notify(t('docs.openFailed'), { kind: 'error' }); }
  }
  async function reveal() {
    try { const { revealItemInDir } = await import('@tauri-apps/plugin-opener'); await revealItemInDir(absPath); }
    catch { notify(t('docs.revealFailed'), { kind: 'error' }); }
  }
  async function copyDoc() {
    const text = article?.innerText?.trim() || doc?.body || doc?.content || '';
    notify(t((await copyText(text)) ? 'docs.copied' : 'docs.copyFailed'));
  }

  /** Prints only the document: a copy of the rendered article is placed at the top of <body> for the print pass. */
  async function printDoc() {
    await tick();
    if (!article) return;
    document.getElementById('print-root')?.remove();
    const root = document.createElement('div');
    root.id = 'print-root';
    root.className = 'md';
    root.dir = 'auto';
    for (const n of article.childNodes) root.appendChild(n.cloneNode(true));
    document.body.appendChild(root);
    const done = () => { root.remove(); window.removeEventListener('afterprint', done); };
    window.addEventListener('afterprint', done);
    try { window.print(); } catch { done(); notify(t('docs.printFailed'), { kind: 'error' }); }
  }

  async function importFiles(list: Iterable<File>) {
    const all = [...list];
    if (!all.length || importing) return;
    const ok = all.filter((f) => f.size <= MAX_FILE_BYTES);
    for (const f of all) if (f.size > MAX_FILE_BYTES) notify(t('docs.tooLarge', { name: f.name, size: fmtSize(f.size) }), { kind: 'error' });
    if (!ok.length) return;
    importing = true;
    try {
      const payload = [];
      for (const f of ok) payload.push({ name: f.name, dataBase64: (await readAsDataURL(f)).split(',').pop() || '' });
      const r = await post('/library/import', { files: payload });
      const paths: string[] = r?.paths ?? [];
      await loadTree();
      notify(tn('docs.imported', paths.length || ok.length));
      if (paths[0] && !dirty) openFile(paths[0]);
    } catch (e: any) {
      const msg = e?.message ?? String(e);
      notify(/not found|404|method not allowed/i.test(msg) ? t('docs.importUnavailable') : t('docs.importFailed', { error: msg }), { kind: 'error' });
    } finally { importing = false; }
  }
  function pick(e: Event) { const el = e.target as HTMLInputElement; importFiles(el.files || []); el.value = ''; }

  const hasFiles = (e: DragEvent) => [...(e.dataTransfer?.types || [])].includes('Files');
  function dragenter(e: DragEvent) { if (!hasFiles(e)) return; e.preventDefault(); dragDepth++; dragging = true; }
  function dragover(e: DragEvent) { if (hasFiles(e)) e.preventDefault(); }
  function dragleave(e: DragEvent) { if (!hasFiles(e)) return; dragDepth = Math.max(0, dragDepth - 1); if (!dragDepth) dragging = false; }
  function drop(e: DragEvent) { if (!hasFiles(e)) return; e.preventDefault(); dragDepth = 0; dragging = false; importFiles(e.dataTransfer?.files || []); }

  const titleOf = (f: any) => f.title || prettyName(f.path);
  const groups = $derived.by(() => {
    const g = new Map<string, any[]>();
    const q = filter.trim().toLowerCase();
    for (const f of files) {
      if (q && !`${f.path} ${titleOf(f)}`.toLowerCase().includes(q)) continue;
      const parts = f.path.split('/');
      const k = parts.length === 1 ? '/' : parts[0];
      g.set(k, [...(g.get(k) || []), f]);
    }
    return [...g.entries()].sort(([a], [b]) => (a === '/' ? -1 : b === '/' ? 1 : a.localeCompare(b)));
  });

  const FM = ['status', 'area', 'updated', 'level'];
  function fmValue(k: string, v: any): string {
    const s = String(v);
    if (k === 'status') { const key = `idea.status.${s}`; const x = t(key); return x === key ? s : x; }
    if (k === 'updated' && /^\d{4}-\d{2}-\d{2}/.test(s)) return fmtDate(s.slice(0, 10) + 'T12:00:00');
    return s;
  }
  const ext = (p: string) => (p.includes('.') ? p.split('.').pop()!.toUpperCase() : '');
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="docs" ondragenter={dragenter} ondragover={dragover} ondragleave={dragleave} ondrop={drop}>
  <aside class="tree scroll" aria-label={t('docs.filesLabel')}>
    <div class="treehead">
      <input placeholder={t('docs.filter')} aria-label={t('docs.filter')} bind:value={filter} dir="auto" />
      <button class="btn import" onclick={() => picker?.click()} disabled={importing} title={t('docs.importTip')}><Icon name="upload" size={14} /> {t(importing ? 'docs.importing' : 'docs.import')}</button>
      <input type="file" multiple hidden bind:this={picker} onchange={pick} />
    </div>
    {#if loading}
      <div class="skel"></div><div class="skel w70"></div><div class="skel w50"></div><div class="skel w70"></div>
    {:else if !files.length}
      <p class="muted sm note">{t('docs.treeEmpty')}</p>
    {:else if !groups.length}
      <p class="muted sm note">{t('docs.noFilterMatches')}</p>
      <button class="btn ghost sm" onclick={() => (filter = '')}>{t('docs.clearFilter')}</button>
    {/if}
    {#each groups as [k, list]}
      {#if k === '/'}
        {#each list as f (f.path)}
          <button class="f" class:active={current === f.path} aria-current={current === f.path ? 'page' : undefined} onclick={() => openFile(f.path)} title={f.path}>
            <span class="ft" dir="auto">{titleOf(f)}</span><bdi class="fp">{f.path}</bdi></button>
        {/each}
      {:else}
        <button class="dir" onclick={() => (open[k] = !open[k])} aria-expanded={!!(open[k] || filter)}>
          <Icon name={open[k] || filter ? 'down' : 'right'} size={13} class={open[k] || filter ? '' : 'flip-rtl'} /> <bdi>{k}</bdi> <span class="count muted">{list.length}</span></button>
        {#if open[k] || filter}
          {#each list as f (f.path)}
            <button class="f in" class:active={current === f.path} aria-current={current === f.path ? 'page' : undefined} onclick={() => openFile(f.path)} title={f.path}>
              <span class="ft" dir="auto">{titleOf(f)}</span><bdi class="fp">{f.path.slice(k.length + 1)}</bdi></button>
          {/each}
        {/if}
      {/if}
    {/each}
  </aside>

  <section class="view scroll">
    {#if loading}
      <div class="pad"><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div><div class="skel w50"></div></div>
    {:else if treeError}
      <div class="state" role="alert"><Icon name="warning" size={26} /><h2>{t('docs.loadFailed')}</h2><p class="muted" dir="auto">{treeError}</p>
        <button class="btn primary" onclick={() => { loading = true; loadTree().then(() => { if (!treeError && !current) openFile(files[0]?.path); }); }}><Icon name="retry" size={14} /> {t('docs.tryAgain')}</button></div>
    {:else if results}
      <h1>{t('search')}: <span dir="auto">{path?.slice(1)}</span></h1>
      {#if !results.length}<div class="state"><Icon name="search" size={24} /><p class="muted">{t('docs.noMatches')}</p></div>{/if}
      {#each results as r}
        <button class="hit" onclick={() => openFile(r.path)}>
          <b><bdi>{r.path}</bdi></b>
          {#each r.matches as m}<div class="muted sm" dir="auto"><bdi>{m.line}</bdi>: {m.text}</div>{/each}
        </button>
      {/each}
    {:else if !files.length}
      <div class="state">
        <Icon name="docs" size={30} />
        <h2>{t('docs.emptyTitle')}</h2>
        <p class="muted">{t('docs.emptyBody')}</p>
        <div class="row">
          <button class="btn primary" onclick={() => picker?.click()} disabled={importing}><Icon name="upload" size={14} /> {t('docs.import')}</button>
          <button class="btn" onclick={() => go({ kind: 'home' })}>{t('docs.emptyAsk')}</button>
        </div>
      </div>
    {:else if docError}
      <div class="state" role="alert"><Icon name="warning" size={26} /><h2>{t('docs.fileFailed')}</h2><p class="muted"><bdi>{current}</bdi></p><p class="muted sm" dir="auto">{docError}</p>
        <button class="btn primary" onclick={() => openFile(current)}><Icon name="retry" size={14} /> {t('docs.tryAgain')}</button></div>
    {:else if doc}
      <div class="row bar">
        <code class="grow path"><bdi>{current}</bdi></code>
        {#if editing}
          <span class="dirty" class:on={dirty} role="status"><span class="dot" aria-hidden="true"></span>{t(dirty ? 'docs.unsaved' : 'docs.noChanges')}</span>
          <button class="btn" onclick={togglePreview} aria-pressed={preview}><Icon name="eye" size={15} /> {t('docs.preview')}</button>
          <button class="btn" onclick={cancelEdit}>{t('cancel')}</button>
          <button class="btn primary" onclick={save} disabled={!dirty || saving} title={t('docs.saveTip')}>{t(saving ? 'docs.saving' : 'save')}</button>
        {:else}
          {#if !doc.binary}
            <button class="btn ghost ic" onclick={printDoc} title={t('docs.print')} aria-label={t('docs.print')}><Icon name="print" size={16} /></button>
            <button class="btn ghost ic" onclick={copyDoc} title={t('docs.copyText')} aria-label={t('docs.copyText')}><Icon name="copy" size={16} /></button>
          {/if}
          {#if desktop && absPath}
            <button class="btn ghost ic" onclick={reveal} title={t(isMac ? 'docs.revealMac' : 'docs.reveal')} aria-label={t(isMac ? 'docs.revealMac' : 'docs.reveal')}><Icon name="folder" size={16} /></button>
          {/if}
          <button class="btn" onclick={discuss} title={t('docs.discussTip')}><Icon name="chat" size={15} /> {t('docs.discuss')}</button>
          {#if !doc.binary}<button class="btn" onclick={startEdit}><Icon name="edit" size={14} /> {t('edit')}</button>{/if}
        {/if}
      </div>

      {#if doc.binary}
        <div class="filecard surface">
          <div class="row">
            <span class="fbadge"><Icon name="docs" size={20} /></span>
            <div class="grow">
              <b class="fname" dir="auto">{current.split('/').pop()}</b>
              <div class="muted sm">{[ext(current) && t('docs.fileType', { type: ext(current) }), info ? fmtSize(info.size) : '', info?.modified ? fmtDate(info.modified) : ''].filter(Boolean).join(' · ')}</div>
            </div>
          </div>
          {#if !isImageName(current) && !doc.plain}<p class="muted">{t('docs.notPreviewable')}</p>{/if}
          <div class="row wrapr">
            {#if desktop && absPath}<button class="btn primary" onclick={openExternal}><Icon name="external" size={14} /> {t('docs.openDefault')}</button>{/if}
            <button class="btn" onclick={discuss}><Icon name="chat" size={14} /> {t('docs.discussAssistant')}</button>
          </div>
        </div>
        {#if imgUrl}<img class="inline-img" src={imgUrl} alt={current.split('/').pop()} />{/if}
        {#if doc.plain}<pre class="plain" dir="auto">{doc.content}</pre>{/if}
      {:else if editing}
        <div class="split" class:two={preview}>
          <textarea class="editor" bind:value={draft} dir="auto" spellcheck="true" aria-label={t('docs.editorLabel', { name: current })}></textarea>
          {#if preview}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
            <article class="md pv scroll" dir="auto" aria-label={t('docs.preview')} onclick={(e) => { if (!mdClick(e) && (e.target as HTMLElement).closest('a')) e.preventDefault(); }}>{@html md(draft)}</article>
          {/if}
        </div>
      {:else}
        {#if doc.frontmatter && typeof doc.frontmatter === 'object'}
          <div class="row fm">
            {#each Object.entries(doc.frontmatter).filter(([k, v]) => FM.includes(k) && v != null && v !== '') as [k, v]}<span class="pill">{t(`docs.fm.${k}`)}: <bdi>{fmValue(k, v)}</bdi></span>{/each}
          </div>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <article class="md" dir="auto" bind:this={article} onclick={clickLink}>{@html md(doc.content)}</article>
      {/if}
    {:else if docLoading}
      <div class="pad"><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div><div class="skel w50"></div></div>
    {/if}
  </section>

  {#if dragging}<div class="dropzone" aria-hidden="true"><Icon name="upload" size={28} /><span>{t('docs.dropHere')}</span></div>{/if}
</div>

{#if unsaved}
  <div class="modal-bg" role="presentation">
    <div class="modal unsaved" role="dialog" aria-modal="true" aria-labelledby="unsaved-title" tabindex="-1" use:trapFocus={{ onEscape: () => unsaved?.resolve('keep') }}>
      <h2 id="unsaved-title">{t('docs.unsavedTitle')}</h2>
      <p class="muted">{t('docs.unsavedBody')} <bdi>{current}</bdi></p>
      <div class="row end">
        <button class="btn" onclick={() => unsaved?.resolve('keep')}>{t('docs.keepEditing')}</button>
        <button class="btn danger" onclick={() => unsaved?.resolve('discard')}>{t('docs.discard')}</button>
        <!-- svelte-ignore a11y_autofocus -->
        <button class="btn primary" autofocus onclick={() => unsaved?.resolve('save')}>{t('save')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .docs { position: relative; display: grid; grid-template-columns: 280px minmax(0, 1fr); height: calc(100vh - 26px); }
  .tree { border-inline-end: var(--border-w) solid var(--border); padding: 0 8px 12px; display: flex; flex-direction: column; gap: 1px; }
  .treehead { position: sticky; top: 0; z-index: 1; background: var(--bg); display: grid; gap: 6px; padding-bottom: 6px; }
  .treehead input { padding: 5px 8px; font-size: 13px; }
  .import { font-size: 12.5px; }
  .f, .dir { font: inherit; color: inherit; background: none; border: 0; text-align: start; cursor: pointer; font-size: 13px; padding: 4px 6px; border-radius: 6px; min-width: 0; flex: none; }
  .f { display: flex; flex-direction: column; line-height: 1.3; }
  .ft, .fp { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  .fp { font-size: 12px; color: var(--muted); font-family: var(--mono); }
  .dir { display: flex; align-items: center; gap: 5px; font-weight: 600; margin-top: 4px; }
  .dir .count { margin-inline-start: auto; font-weight: 400; font-size: 12px; }
  .f.in { padding-inline-start: 22px; }
  .f:hover, .dir:hover { background: var(--panel-2); }
  .f.active { background: var(--accent); color: var(--accent-fg); }
  .f.active .fp { color: inherit; opacity: .8; }
  .note { margin: 8px 6px; }

  .view { padding: 0 32px 40px; min-width: 0; }
  .bar { position: sticky; top: 0; background: var(--bg); padding: 6px 0; z-index: 2; flex-wrap: wrap; }
  .path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12.5px; }
  .ic { padding: 6px 8px; }
  .dirty { display: inline-flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--muted); }
  .dirty .dot { width: 8px; height: 8px; border-radius: 50%; border: 1.5px solid var(--muted); }
  .dirty.on { color: var(--fg); font-weight: 500; }
  .dirty.on .dot { background: var(--accent-2); border-color: var(--accent-2); }
  .fm { flex-wrap: wrap; margin: 6px 0 10px; }
  .fm .pill { font-size: 12px; }

  .split { display: grid; grid-template-columns: minmax(0, 1fr); gap: 16px; height: calc(100vh - 110px); }
  .split.two { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
  .editor { height: 100%; font-family: var(--mono); font-size: 13px; line-height: 1.55; resize: none; }
  .pv { height: 100%; padding: 8px 14px; border-radius: var(--radius); border: var(--border-w) solid var(--border); overflow-wrap: anywhere; }
  @media (max-width: 1000px) { .split.two { grid-template-columns: minmax(0, 1fr); grid-template-rows: 1fr 1fr; } }

  .hit { font: inherit; color: inherit; text-align: start; width: 100%; box-sizing: border-box; display: block; cursor: pointer; background: var(--panel); border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 10px; margin-bottom: 8px; }
  .hit:hover { border-color: var(--accent); }

  .state { display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; gap: 6px; min-height: 60%; padding: 40px 0; color: var(--muted); }
  .state h2 { margin: 6px 0 0; color: var(--fg); }
  .state p { margin: 0; max-width: 440px; }
  .state .row, .state > .btn { margin-top: 12px; }

  .filecard { display: grid; gap: 12px; max-width: 560px; margin-top: 10px; }
  .filecard > * { position: relative; }
  .filecard p { margin: 0; }
  .fbadge { width: 40px; height: 40px; border-radius: 10px; display: grid; place-items: center; background: var(--fill); color: var(--accent-2); flex: none; }
  .fname { display: block; overflow-wrap: anywhere; }
  .wrapr { flex-wrap: wrap; }
  .inline-img { display: block; max-width: 100%; margin-top: 16px; border-radius: var(--radius); box-shadow: var(--shadow); }
  .plain { margin-top: 16px; font-size: 12.5px; white-space: pre-wrap; overflow-wrap: anywhere; }

  .pad { padding-top: 16px; display: grid; gap: 12px; }
  .skel { height: 13px; margin: 5px 6px; border-radius: 8px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .skel.h { height: 26px; width: 45%; }
  .w70 { width: 70%; } .w50 { width: 50%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel { animation: none; } }

  .dropzone { position: absolute; inset: 8px; z-index: 10; pointer-events: none; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; font-weight: 500; color: var(--accent); border: 2px dashed var(--accent); border-radius: var(--radius-lg); background: color-mix(in srgb, var(--bg) 88%, transparent); }

  .unsaved { max-width: 460px; display: grid; gap: 12px; }
  .unsaved h2 { margin: 0; font-size: 17px; }
  .unsaved p { margin: 0; overflow-wrap: anywhere; }
  .end { justify-content: flex-end; flex-wrap: wrap; }
  .sm { font-size: 12px; }

  /* Printing: only the copy of the document made by printDoc() is laid out. */
  :global(#print-root) { display: none; }
  @media print {
    :global(html), :global(body) { height: auto !important; overflow: visible !important; background: #fff !important; }
    :global(body > *:not(#print-root)) { display: none !important; }
    :global(#print-root) { display: block; color: #000; background: #fff; font-size: 12pt; line-height: 1.5; padding: 0; }
    :global(#print-root a) { color: inherit; text-decoration: underline; }
    :global(#print-root pre), :global(#print-root table), :global(#print-root img), :global(#print-root blockquote) { break-inside: avoid; }
    :global(#print-root pre) { white-space: pre-wrap; border: 1px solid #ccc; padding: 8px; }
    :global(#print-root h1), :global(#print-root h2), :global(#print-root h3) { break-after: avoid; }
    :global(#print-root td), :global(#print-root th) { border: 1px solid #999; }
  }
</style>
