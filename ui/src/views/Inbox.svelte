<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { api, on, post } from '../lib/api';
  import { go, refreshChats, toast, track } from '../lib/store.svelte';
  import { t, tn, fmtDate, fmtNum } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { areaLabel } from '../lib/terms';
  import { fold } from '../lib/CommandPalette.svelte';
  import { inSentence } from '../lib/todoline';
  import { dueLabel as dueLabelOn } from '../lib/content/util';
  import { inbox, refreshInbox } from '../lib/inbox.svelte';

  type Capture = { path: string; name: string; title: string; text: string; raw: string; captured: string; big: boolean };

  let items = $state<Capture[]>([]);
  let loading = $state(true);
  let loadError = $state('');
  // Captures being worked on, so a second click does nothing.
  let busy = $state<Record<string, boolean>>({});
  let processing = $state(false);
  let expanded = $state<Record<string, boolean>>({});
  // "File it under…": which capture is being filed, and the items to choose from.
  let filing = $state<Capture | null>(null);
  let targets = $state<any[] | null>(null);
  let q = $state('');

  async function load() {
    try {
      const r = await api('/inbox');
      items = Array.isArray(r?.items) ? r.items : [];
      inbox.count = items.length;
      loadError = '';
    } catch (e: any) { loadError = e?.message ?? String(e); }
    finally { loading = false; }
  }
  onMount(() => { load(); return on((e) => { if (e.type === 'library.changed' || e.type === 'resync') load(); }); });

  /** Taken out of the list at once; put back if the change did not go through. */
  const hide = (c: Capture) => { items = items.filter((x) => x.path !== c.path); inbox.count = items.length; };
  async function settle() { await load(); refreshInbox(); }

  /** Put a deleted capture back exactly as it was. */
  async function restore(c: Capture) {
    try { await post('/library/new', { path: c.path, content: c.raw }); }
    catch (e) { failed(t('inbox.undoFailed'), e); }
    settle();
  }

  async function toTodo(c: Capture) {
    if (busy[c.path] || c.big) return;
    busy[c.path] = true;
    const text = c.text.trim() || c.title;
    try {
      // "Tomorrow" in a capture means the day after it was written, not the day after today.
      const r = await post('/capture', { kind: 'todo', text, on: (c.captured || '').slice(0, 10) });
      try { await post('/library/delete', { path: c.path }); }
      catch (e) {
        // The todo is saved but the capture is still there: take the todo back so nothing is doubled.
        await post('/todos/delete', { file: r.file, line: r.line, text: r.text }).catch(() => {});
        throw e;
      }
      hide(c);
      track('inbox.todo');
      const undo = {
        label: t('undo'),
        run: async () => {
          try {
            await post('/todos/delete', { file: r.file, line: r.line, text: r.text });
            await post('/library/new', { path: c.path, content: c.raw });
          } catch (e) { failed(t('inbox.undoFailed'), e); }
          settle();
        },
      };
      toast(r?.due ? t('inbox.todoDue', { date: inSentence(dueLabelOn(r.due)) }) : t('inbox.todoDone'), { kind: 'success', action: undo });
    } catch (e) { failed(t('inbox.todoFailed'), e); }
    finally { busy[c.path] = false; settle(); }
  }

  async function remove(c: Capture) {
    if (busy[c.path]) return;
    busy[c.path] = true;
    try {
      await post('/library/delete', { path: c.path });
      hide(c);
      track('inbox.delete');
      toast(t('inbox.deleted'), { kind: 'success', action: { label: t('undo'), run: () => restore(c) } });
    } catch (e) { failed(t('inbox.deleteFailed'), e); }
    finally { busy[c.path] = false; settle(); }
  }

  async function openFiling(c: Capture) {
    filing = c; q = '';
    if (targets) return;
    try { targets = await api('/library/ideas'); }
    catch (e) { filing = null; failed(t('inbox.fileFailed'), e); }
  }
  const dirOf = (p: string) => p.replace(/\/README\.md$/i, '');
  const matches = $derived((targets ?? []).filter((i) => !q.trim() || fold(`${i.title} ${i.summary ?? ''} ${[i.tags ?? []].flat().join(' ')} ${areaLabel(i.area)}`).includes(fold(q.trim()))).slice(0, 60));

  /** Moved into the item's notes folder (a plain text file becomes a note, .md, on the way) and
   *  listed in the item's files, where the assistant looks for its notes. */
  async function fileUnder(c: Capture, item: any) {
    if (busy[c.path]) return;
    busy[c.path] = true;
    try {
      const r = await post('/inbox/file', { path: c.path, item: dirOf(item.path) });
      filing = null;
      hide(c);
      track('inbox.file');
      toast(t('inbox.filed', { name: item.title }), { kind: 'success', action: { label: t('undo'), run: async () => {
        try { await post('/inbox/unfile', { path: r.path, to: c.path, line: r.line }); } catch (e) { failed(t('inbox.undoFailed'), e); }
        settle();
      } } });
    } catch (e) { failed(t('inbox.fileFailed'), e); }
    finally { busy[c.path] = false; settle(); }
  }

  async function process() {
    if (processing || !items.length) return;
    processing = true;
    try {
      // A library made before the kit had the routine gets it first.
      await post('/inbox/prepare');
      const c = await post('/chats', { byUser: false });
      await post(`/chats/${c.id}/send`, { text: t('inbox.processPrompt') });
      track('inbox.process');
      await refreshChats().catch(() => {});
      go({ kind: 'chat', id: c.id });
    } catch (e) { failed(t('inbox.processFailed'), e); }
    finally { processing = false; }
  }

  const when = (iso: string) => fmtDate(iso, { dateStyle: 'medium', timeStyle: 'short' });
  const long = (c: Capture) => c.text.length > 400 || c.text.split('\n').length > 7;
  const guide = () => go({ kind: 'help', path: 'PHONE-CAPTURE' });
</script>

<div class="page">
  <div class="row head">
    <div class="grow">
      <h1>{t('inbox.title')}</h1>
      <p class="muted sub">{t('inbox.sub')}</p>
    </div>
    {#if items.length}
      <button class="btn primary" disabled={processing} aria-busy={processing} onclick={process} title={t('inbox.processHint')}>
        <Icon name="chat" size={15} /> {t('inbox.process')}
      </button>
    {/if}
  </div>

  {#if loading}
    <div class="list sk" aria-busy="true"><div class="skel"></div><div class="skel"></div></div>
  {:else if loadError && !items.length}
    <div class="state" role="alert"><Icon name="warning" size={24} /><p>{t('inbox.loadFailed')}</p><p class="muted sm" dir="auto">{loadError}</p>
      <button class="btn primary" onclick={() => { loading = true; load(); }}><Icon name="retry" size={14} /> {t('inbox.retry')}</button></div>
  {:else if !items.length}
    <div class="state">
      <Icon name="inbox" size={28} />
      <h2>{t('inbox.emptyTitle')}</h2>
      <p>{t('inbox.emptyBody')}</p>
      <button class="btn" onclick={guide}><Icon name="book" size={15} /> {t('inbox.phoneGuide')}</button>
    </div>
  {:else}
    <p class="muted sm" role="status">{tn('inbox.count', items.length)}</p>
    <ul class="list">
      {#each items as c (c.path)}
        <li class="cap surface" aria-busy={busy[c.path] || undefined}>
          <div class="row meta sm muted">
            <Icon name="inbox" size={13} />
            <span class="grow">{t('inbox.when', { when: when(c.captured) })}</span>
          </div>
          {#if c.big}
            <b dir="auto" class="ttl">{c.title}</b>
            <p class="muted sm">{t('inbox.big')}</p>
          {:else}
            <div class="text" class:clamp={long(c) && !expanded[c.path]} dir="auto">{c.text || t('inbox.empty')}</div>
            {#if long(c)}
              <button class="link sm" aria-expanded={!!expanded[c.path]} onclick={() => (expanded[c.path] = !expanded[c.path])}>{expanded[c.path] ? t('inbox.less') : t('inbox.more')}</button>
            {/if}
          {/if}
          <div class="row actions">
            {#if c.big}
              <button class="btn" onclick={() => go({ kind: 'docs', path: c.path })}><Icon name="docs" size={14} /> {t('inbox.open')}</button>
            {:else}
              <button class="btn" disabled={busy[c.path]} title={t('inbox.todoHint')} onclick={() => toTodo(c)}><Icon name="todos" size={14} /> {t('inbox.todo')}</button>
              <button class="btn" disabled={busy[c.path]} aria-expanded={filing?.path === c.path} onclick={() => (filing?.path === c.path ? (filing = null) : openFiling(c))}><Icon name="folder" size={14} /> {t('inbox.file')}</button>
            {/if}
            <span class="grow"></span>
            <button class="btn danger" disabled={busy[c.path]} onclick={() => remove(c)}><Icon name="trash" size={14} /> {t('inbox.delete')}</button>
          </div>
          {#if filing?.path === c.path}
            <div class="filing" role="group" aria-label={t('inbox.fileTitle', { title: c.title })}>
              {#if targets && !targets.length}
                <p class="muted sm">{t('inbox.fileNoItems')}</p>
              {:else}
                <!-- svelte-ignore a11y_autofocus -->
                <input type="search" bind:value={q} placeholder={t('inbox.fileSearch')} aria-label={t('inbox.fileSearch')} dir="auto" autofocus />
                {#if targets === null}
                  <p class="muted sm" role="status">{t('shell.loading')}</p>
                {:else if !matches.length}
                  <p class="muted sm">{t('inbox.fileNone')}</p>
                {:else}
                  <ul class="targets">
                    {#each matches as i (i.path)}
                      <li><button class="target" disabled={busy[c.path]} onclick={() => fileUnder(c, i)}><b class="ell" dir="auto">{i.title}</b><span class="muted sm" dir="auto">{areaLabel(i.area)}</span></button></li>
                    {/each}
                  </ul>
                {/if}
              {/if}
              <div class="row"><button class="btn" onclick={() => (filing = null)}>{t('inbox.fileCancel')}</button></div>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    <p class="muted sm foot"><button class="link" onclick={guide}>{t('inbox.phoneGuide')}</button> · {t('inbox.phoneHint')}</p>
  {/if}
</div>

<style>
  .head { align-items: flex-start; gap: 16px; flex-wrap: wrap; }
  .sub { margin: 4px 0 14px; }
  .list { list-style: none; margin: 8px 0 0; padding: 0; display: flex; flex-direction: column; gap: 10px; }
  .cap { padding: 14px 16px; display: flex; flex-direction: column; gap: 8px; }
  .meta { align-items: center; gap: 6px; }
  .ttl { overflow-wrap: anywhere; }
  .text { white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--fs-base, 15px); line-height: 1.5; }
  .text.clamp { display: -webkit-box; -webkit-line-clamp: 6; line-clamp: 6; -webkit-box-orient: vertical; overflow: hidden; }
  .actions { flex-wrap: wrap; gap: 8px; margin-top: 2px; }
  .link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent); cursor: pointer; align-self: flex-start; }
  .link:hover { text-decoration: underline; }
  .filing { border-top: var(--border-w) solid var(--hairline); padding-top: 10px; display: flex; flex-direction: column; gap: 8px; }
  .targets { list-style: none; margin: 0; padding: 0; max-height: 240px; overflow: auto; display: flex; flex-direction: column; gap: 2px; }
  .target { font: inherit; color: inherit; width: 100%; text-align: start; background: none; border: 0; border-radius: var(--radius, 8px); padding: 7px 10px; cursor: pointer; display: flex; justify-content: space-between; gap: 12px; align-items: baseline; }
  .target:hover, .target:focus-visible { background: var(--fill); }
  .btn.danger { color: var(--danger); }
  .foot { margin-top: 14px; }
  .state { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 8px; padding: 40px 0; color: var(--muted); }
  .state h2 { margin: 4px 0 0; color: var(--fg); }
  .state p { margin: 0; max-width: 440px; }
  .skel { height: 96px; border-radius: var(--radius-lg); background: var(--fill); }
  .sm { font-size: var(--fs-xs); }
</style>
