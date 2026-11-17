<script lang="ts">
  // A list of an item's files on its page: meeting notes, notes, deliverables or other files. A file
  // opens in Docs (which previews notes, PDFs and pictures, and offers the usual app for the rest);
  // a note can go straight to the editor; deliverables can also be opened in their usual app or
  // shown in their folder, the way Docs does it.
  import Icon from '../../lib/Icon.svelte';
  import { inTauri } from '../../lib/api';
  import { go } from '../../lib/store.svelte';
  import { t, fmtDate, fmtHijri, lang } from '../../lib/i18n';
  import { fmtSize, notify, prettyName } from '../../lib/content/util';
  import { showHijri } from '../../lib/weekPrefs.svelte';
  import { canOpenInApp, openLibraryFile, revealLabelKey, revealLibraryFile } from '../../lib/openInApp';

  import type { Snippet } from 'svelte';
  let { files, mode, extra }: { files: any[]; mode: 'meetings' | 'notes' | 'outputs' | 'other'; /** More buttons for a file (a meeting note's recording, FEAT-55). */ extra?: Snippet<[any]> } = $props();

  const desktop = inTauri();
  const EDIT_EXT = /\.(md|txt|csv|json)$/i;
  const nameOf = (f: any) => String(f.title || prettyName(String(f.path)));
  const day = (d: any) => (/^\d{4}-\d{2}-\d{2}/.test(String(d ?? '')) ? String(d).slice(0, 10) : '');
  const when = (d: string) => fmtDate(d + 'T12:00:00', { day: 'numeric', month: 'short', year: 'numeric' });
  const names = (a: string[]) => { try { return new Intl.ListFormat(lang(), { type: 'conjunction' }).format(a); } catch { return a.join(', '); } };
  const ext = (p: string) => (/\.([a-z0-9]+)$/i.exec(p)?.[1] ?? '').toUpperCase();

  async function openExternal(p: string) {
    try { await openLibraryFile(p); }
    catch (e: any) { notify(t('docs.openFailed'), { kind: 'error', detail: String(e?.message ?? e) }); }
  }
  async function reveal(p: string) {
    try { await revealLibraryFile(p); }
    catch (e: any) { notify(t('docs.revealFailed'), { kind: 'error', detail: String(e?.message ?? e) }); }
  }
</script>

<ul class="files" role="list">
  {#each files as f (f.path)}
    {@const d = day(mode === 'meetings' ? f.date : mode === 'notes' ? f.updated || f.modified : f.modified)}
    <li class="file">
      <button class="open" onclick={() => go({ kind: 'docs', path: f.path })} title={f.path}>
        <span class="ic" aria-hidden="true"><Icon name={mode === 'meetings' ? 'chats' : mode === 'outputs' ? 'download' : 'docs'} size={16} /></span>
        <span class="grow body">
          <span class="row top">
            <b class="grow name" dir="auto">{nameOf(f)}</b>
            {#if d && mode === 'meetings'}<span class="muted sm when">{when(d)}{#if showHijri()}{' '}<span dir="auto">· {fmtHijri(d + 'T12:00:00', { day: 'numeric', month: 'short' })}</span>{/if}</span>{/if}
          </span>
          {#if mode === 'meetings' && f.attendees?.length}<span class="muted sm blk" dir="auto">{t('item.meeting.with', { names: names(f.attendees) })}</span>{/if}
          {#if f.summary}<span class="muted sm blk sum" dir="auto">{f.summary}</span>{/if}
          {#if mode !== 'meetings'}
            <span class="muted sm blk">
              {#if mode === 'outputs' || mode === 'other'}<bdi>{ext(f.path) || prettyName(f.path)}</bdi>{' · '}{fmtSize(f.size ?? 0)}{#if d}{' · '}{/if}{/if}
              {#if d}{t('item.updated', { date: when(d) })}{/if}
            </span>
          {/if}
        </span>
      </button>
      <span class="acts">
        {#if extra}{@render extra(f)}{/if}
        {#if (mode === 'meetings' || mode === 'notes') && EDIT_EXT.test(f.path)}
          <button class="ib" onclick={() => go({ kind: 'docs', path: f.path, id: 'edit' })} aria-label={t('item.editNote', { name: nameOf(f) })} title={t('edit')}><Icon name="edit" size={14} /></button>
        {/if}
        {#if mode === 'outputs' && desktop && canOpenInApp(f.path)}
          <button class="ib" onclick={() => openExternal(f.path)} aria-label={t('item.outputs.openTip', { name: f.name })} title={t('docs.openDefault')}><Icon name="external" size={14} /></button>
        {/if}
        {#if (mode === 'outputs' || mode === 'other') && desktop}
          <button class="ib" onclick={() => reveal(f.path)} aria-label={t('item.outputs.revealTip', { name: f.name })} title={t(revealLabelKey())}><Icon name="folder" size={14} /></button>
        {/if}
      </span>
    </li>
  {/each}
</ul>

<style>
  .files { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .file { display: flex; align-items: flex-start; gap: 4px; border-radius: var(--radius); background: var(--fill); }
  .open { font: inherit; color: inherit; text-align: start; background: none; border: 0; border-radius: var(--radius); padding: 10px 12px; cursor: pointer; display: flex; gap: 10px; align-items: flex-start; flex: 1; min-width: 0; }
  .open:hover { background: var(--fill-2); }
  .ic { color: var(--muted); padding-top: 2px; display: grid; }
  .body { display: grid; gap: 2px; }
  .top { gap: 10px; align-items: baseline; flex-wrap: wrap; }
  .name { overflow-wrap: anywhere; font-weight: 600; }
  .when { white-space: nowrap; }
  .blk { display: block; overflow-wrap: anywhere; }
  .sum { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .sm { font-size: var(--fs-xs); }
  .acts { display: inline-flex; gap: 2px; padding-block-start: 8px; padding-inline-end: 6px; }
  .ib { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 5px; min-width: 26px; min-height: 26px; cursor: pointer; display: grid; place-items: center; }
  .ib:hover, .ib:focus-visible { color: var(--fg); background: var(--fill-2); }
</style>
