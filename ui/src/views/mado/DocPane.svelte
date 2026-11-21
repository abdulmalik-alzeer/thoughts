<script lang="ts">
  // One document of a step: rendered by the app's own markdown renderer (lib/md.ts, sanitised),
  // with commented passages marked and a "Comment" button for whatever the reader selects. The Edit
  // toggle swaps it for the app's text editor; what is being typed is kept in the store, so
  // switching tabs does not lose it.
  import { tick, untrack } from 'svelte';
  import { MessageSquarePlus, Pencil, FileText } from '@lucide/svelte';
  import { md, mdClick } from '../../lib/md';
  import { t } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import { toast } from '../../lib/store.svelte';
  import { confirmDialog } from '../../lib/dialog.svelte';
  import Editor from '../docs/Editor.svelte';
  import type { DocName } from '../../lib/mado/api';
  import type { MadoStore } from '../../lib/mado/store.svelte';
  import type { Comment, CommentTarget } from '../../lib/mado/types';
  import { createAnchor, findAnchor, onPage, project } from '../../lib/mado/anchor';
  import { DOC_FILES, commentsOnDoc } from '../../lib/mado/comments/batch';
  import { commentAt, docClick, paint, pageText, pickedIn, unpaint, type Highlight, type PagePick } from '../../lib/mado/comments/dom';

  let { store, name: nameProp, canEdit, canComment, focusId = null, oncomment, onpick }: {
    store: MadoStore;
    name: DocName;
    /** The step is with the user: the document may be edited by hand. */
    canEdit: boolean;
    canComment: boolean;
    /** The comment to bring into view (picked in the comments panel). */
    focusId?: string | null;
    oncomment: (target: CommentTarget, label: string) => void;
    /** A marked passage was clicked. */
    onpick: (id: string) => void;
  } = $props();

  // One pane shows one document for as long as it lives (the workspace makes a new one per tab).
  const name = untrack(() => nameProp);
  const file = $derived(DOC_FILES[name]);
  const source = $derived(store.docs[name]);
  const html = $derived(typeof source === 'string' ? md(source) : '');
  const proj = $derived(typeof source === 'string' ? project(source) : null);
  const comments = $derived(commentsOnDoc(store.comments, name).filter((c) => c.status === 'open' || c.status === 'sent'));
  const editing = $derived(store.edits[name] !== undefined);
  const dirty = $derived(editing && store.edits[name] !== source);
  let saving = $state(false);
  let el = $state<HTMLElement>();
  let pick = $state<PagePick | null>(null);

  // Mark the commented passages whenever the text or the comments change.
  $effect(() => {
    const root = el, list = comments, p = proj, src = source;
    void html;
    if (!root || !p || typeof src !== 'string') return;
    unpaint(root);
    if (!list.length) return;
    const page = pageText(root);
    const marks: Highlight[] = [];
    for (const c of list as Comment[]) {
      if (c.target.kind !== 'text') continue;
      const at = findAnchor(src, c.target);
      const span = at && onPage(p, page.text, at);
      if (span) marks.push({ id: c.id, span, cls: c.status === 'sent' ? 'sent' : '' });
    }
    paint(root, marks, (id) => list.find((c) => c.id === id)?.body ?? '');
  });

  // Bring a comment's passage into view when it is picked in the panel.
  $effect(() => {
    const id = focusId, root = el;
    void html; void comments;
    if (!id || !root) return;
    tick().then(() => {
      const mark = Array.from(root.querySelectorAll<HTMLElement>('mark.mado-hl')).find((m) => m.dataset.madoComment === id);
      if (!mark) return;
      mark.scrollIntoView({ block: 'center', behavior: 'smooth' });
      for (const m of root.querySelectorAll<HTMLElement>('mark.mado-hl')) m.classList.toggle('flash', m.dataset.madoComment === id);
    });
  });

  function readSelection() {
    if (!el || !canComment || editing) { pick = null; return; }
    pick = pickedIn(el);
  }
  $effect(() => {
    // The selection can change without the mouse (keyboard, a double click elsewhere).
    let timer: ReturnType<typeof setTimeout>;
    const on = () => { clearTimeout(timer); timer = setTimeout(readSelection, 120); };
    document.addEventListener('selectionchange', on);
    return () => { clearTimeout(timer); document.removeEventListener('selectionchange', on); };
  });

  function comment() {
    if (!pick || typeof source !== 'string') return;
    const a = createAnchor(source, pick);
    if (!a) { toast(t('mado.comment.cantPlace'), { kind: 'error' }); return; }
    const { partial, ...anchor } = a;
    oncomment({ kind: 'text', file, ...anchor }, pick.label || anchor.quote);
    if (partial) toast(t('mado.comment.partial'));
    window.getSelection()?.removeAllRanges();
    pick = null;
  }

  // Links are left alone here: the app's own guard (App.svelte, `guardLinks`) takes every link
  // click in the window, and asks before opening one that goes somewhere other than it says.
  function click(e: MouseEvent) {
    if (mdClick(e)) return;
    const id = docClick(e);
    if (id) onpick(id);
  }
  function key(e: KeyboardEvent) {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    const id = commentAt(e);
    if (id) { e.preventDefault(); onpick(id); }
  }

  function startEdit() { if (typeof source === 'string') { store.edits[name] = source; pick = null; } }
  async function cancelEdit() {
    if (dirty && !(await confirmDialog({ title: t('mado.doc.discardTitle'), body: t('mado.doc.discardBody'), confirmLabel: t('mado.doc.discard'), danger: true }))) return;
    delete store.edits[name];
  }
  async function save() {
    const text = store.edits[name];
    if (text === undefined || saving) return;
    saving = true;
    try { await store.saveDoc(name, text); delete store.edits[name]; toast(t('mado.doc.saved'), { kind: 'success' }); }
    catch (e) { failed(t('mado.doc.saveFailed'), e); }
    finally { saving = false; }
  }

  // The floating button sits just under the selection (above it at the bottom of the window),
  // placed inside the document's own box so it moves with the page.
  let wrap = $state<HTMLElement>();
  const spot = $derived.by(() => {
    const r = pick?.rect, box = wrap?.getBoundingClientRect();
    if (!r || !box || (!r.width && !r.height)) return null;
    const x = Math.min(Math.max(r.left + r.width / 2 - box.left, 60), box.width - 60);
    const below = r.bottom + 48 < window.innerHeight;
    return { x, y: (below ? r.bottom + 8 : r.top - 40) - box.top };
  });
</script>

{#if source === undefined}
  <div class="state muted" role="status">{t('mado.loading')}</div>
{:else if source === null}
  <div class="empty">
    <FileText size={26} strokeWidth={1.5} aria-hidden="true" />
    <b>{t(`mado.emptyDoc.${name}.title`)}</b>
    <span class="muted">{t(`mado.emptyDoc.${name}.body`)}</span>
  </div>
{:else}
  <div class="bar">
    {#if editing}
      <span class="muted hint grow">{t('mado.doc.editHint')}</span>
      <button type="button" class="btn ghost" onclick={cancelEdit} disabled={saving}>{t('cancel')}</button>
      <button type="button" class="btn primary" onclick={save} disabled={saving || !dirty} aria-busy={saving}>{t(saving ? 'mado.doc.saving' : 'save')}</button>
    {:else}
      <span class="muted hint grow">{t(canComment ? 'mado.doc.selectHint' : 'mado.doc.lookOnly')}</span>
      {#if canComment}
        <button type="button" class="btn" disabled={!pick} onmousedown={(e) => e.preventDefault()} onclick={comment} title={t('mado.comment.onSelectionTip')}>
          <MessageSquarePlus size={15} aria-hidden="true" /> {t('mado.comment.onSelection')}</button>
      {/if}
      {#if canEdit}<button type="button" class="btn" onclick={startEdit}><Pencil size={14} aria-hidden="true" /> {t('edit')}</button>{/if}
    {/if}
  </div>
  {#if editing}
    <div class="editor">
      <Editor bind:value={() => store.edits[name] ?? '', (v) => { store.edits[name] = v; }} path={file} plain label={t('mado.doc.editorLabel', { name: t(`mado.tab.${name}`) })} />
    </div>
  {:else if !source.trim()}
    <div class="empty"><FileText size={26} strokeWidth={1.5} aria-hidden="true" /><b>{t('mado.doc.blankTitle')}</b><span class="muted">{t('mado.doc.blankBody')}</span></div>
  {:else}
    <div class="docwrap" bind:this={wrap}>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_click_events_have_key_events -->
      <article class="md doc" bind:this={el} onclick={click} onkeydown={key} onmouseup={readSelection} onkeyup={readSelection} aria-label={t(`mado.tab.${name}`)}>{@html html}</article>
      {#if pick && spot && canComment}
        <button type="button" class="float btn primary" style:left="{spot.x}px" style:top="{spot.y}px" onmousedown={(e) => e.preventDefault()} onclick={comment}>
          <MessageSquarePlus size={15} aria-hidden="true" /> {t('mado.comment.add')}</button>
      {/if}
    </div>
  {/if}
{/if}

<style>
  .bar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 14px; min-height: 32px; }
  .hint { font-size: var(--fs-sm); }
  .doc { font-size: 15px; line-height: 1.7; max-width: 74ch; padding-bottom: 40px; }
  .doc :global(h1) { font-size: 26px; margin: 0 0 16px; }
  .doc :global(h2) { font-size: 19px; margin: 28px 0 10px; }
  .doc :global(h3) { font-size: 16px; font-weight: 600; margin: 22px 0 8px; }
  .doc :global(::selection) { background: color-mix(in srgb, var(--accent) 30%, transparent); }
  .doc :global(mark.mado-hl) { background: color-mix(in srgb, var(--accent-2) 26%, transparent); color: inherit; border-radius: 3px; padding: 1px 0; cursor: pointer; box-shadow: inset 0 -2px 0 var(--accent-2); transition: background .2s; }
  .doc :global(mark.mado-hl:hover), .doc :global(mark.mado-hl:focus-visible) { background: color-mix(in srgb, var(--accent-2) 42%, transparent); }
  .doc :global(mark.mado-hl.sent) { background: var(--fill); box-shadow: inset 0 -2px 0 var(--control-border); }
  .doc :global(mark.mado-hl.flash) { animation: mado-flash 1.4s ease-out 1; }
  @keyframes mado-flash { 0%, 40% { background: color-mix(in srgb, var(--accent) 50%, transparent); } }
  :global([data-theme="contrast"]) .doc :global(mark.mado-hl) { background: none; box-shadow: inset 0 -3px 0 var(--accent); }
  .docwrap { position: relative; }
  .float { position: absolute; z-index: 40; transform: translateX(-50%); box-shadow: var(--shadow-lg); animation: mado-pop .16s var(--ease) backwards; }
  @keyframes mado-pop { from { opacity: 0; transform: translateX(-50%) translateY(4px) scale(.96); } }
  .editor { min-height: 420px; }
  .state { padding: 40px 0; text-align: center; }
  .empty { display: grid; justify-items: center; gap: 6px; text-align: center; padding: 56px 24px; border: 1px dashed var(--border); border-radius: var(--radius-lg); color: var(--muted); }
  .empty b { color: var(--fg); font-size: 15px; }
  .empty span { max-width: 46ch; }
</style>
