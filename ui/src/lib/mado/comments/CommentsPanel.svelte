<script lang="ts" module>
  import type { CommentTarget as Target } from '../types';
  export type Draft = { target: Target; label: string };
  export type Described = { where: string; quote: string };
</script>

<script lang="ts">
  // The comments of one step. A comment is a note to the assistant: it waits here ("not sent yet")
  // until "Send changes" hands every waiting one over in a single request.
  import { tick } from 'svelte';
  import { MessageSquareText, Pencil, Trash2, Undo2, MapPin } from '@lucide/svelte';
  import { t, tn, ago } from '../../i18n';
  import { imeKey } from '../../ime';
  import type { Comment, CommentStatus, CommentTarget } from '../types';

  let { comments, draft = null, canComment, describe, focusId = null, onadd, oncancel, onedit, onremove, onstatus, onlocate }: {
    comments: Comment[];
    /** A comment being written: what it is about. */
    draft?: Draft | null;
    canComment: boolean;
    /** Plain words for what a comment is about. */
    describe: (target: CommentTarget) => Described;
    focusId?: string | null;
    onadd: (body: string) => Promise<void> | void;
    oncancel: () => void;
    onedit: (id: string, body: string) => Promise<void> | void;
    onremove: (id: string) => Promise<void> | void;
    onstatus: (id: string, status: CommentStatus) => Promise<void> | void;
    onlocate: (c: Comment) => void;
  } = $props();

  const ORDER: CommentStatus[] = ['open', 'sent', 'orphaned', 'resolved'];
  const groups = $derived(ORDER.map((status) => ({ status, items: comments.filter((c) => c.status === status) })).filter((g) => g.items.length));
  let body = $state('');
  let box = $state<HTMLTextAreaElement>();
  let busy = $state(false);
  let editing = $state<string | null>(null);
  let editBody = $state('');
  let listEl = $state<HTMLElement>();

  // A new draft: the cursor goes into its box.
  $effect(() => { if (draft) tick().then(() => box?.focus({ preventScroll: true })); });
  $effect(() => {
    const id = focusId;
    if (!id || !listEl) return;
    tick().then(() => listEl?.querySelector<HTMLElement>(`[data-comment="${CSS.escape(id)}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' }));
  });

  async function add() {
    if (!body.trim() || busy) return;
    busy = true;
    try { await onadd(body.trim()); body = ''; }
    finally { busy = false; }
  }
  function draftKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); add(); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); body = ''; oncancel(); }
  }
  function startEdit(c: Comment) { editing = c.id; editBody = c.body; }
  async function saveEdit(c: Comment) {
    const text = editBody.trim();
    if (!text) return;
    if (text !== c.body) await onedit(c.id, text);
    editing = null;
  }
  function editKey(e: KeyboardEvent, c: Comment) {
    if (imeKey(e)) return;
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); saveEdit(c); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); editing = null; }
  }
</script>

<div class="panel">
  {#if draft}
    {@const d = describe(draft.target)}
    <form class="draft" onsubmit={(e) => { e.preventDefault(); add(); }}>
      <div class="about"><span class="where">{d.where}</span>{#if draft.label || d.quote}<q dir="auto">{draft.label || d.quote}</q>{/if}</div>
      <label class="sr-only" for="mado-draft">{t('mado.comment.bodyLabel')}</label>
      <textarea id="mado-draft" bind:this={box} bind:value={body} rows="3" dir="auto" placeholder={t('mado.comment.placeholder')} onkeydown={draftKey}></textarea>
      <div class="row end">
        <button type="button" class="btn ghost" onclick={() => { body = ''; oncancel(); }}>{t('cancel')}</button>
        <button type="submit" class="btn primary" disabled={!body.trim() || busy} aria-busy={busy}>{t('mado.comment.keep')}</button>
      </div>
    </form>
  {/if}

  {#if !comments.length && !draft}
    <div class="empty">
      <MessageSquareText size={24} strokeWidth={1.5} aria-hidden="true" />
      <b>{t('mado.comments.emptyTitle')}</b>
      <span class="muted">{t(canComment ? 'mado.comments.emptyBody' : 'mado.comments.emptyLocked')}</span>
    </div>
  {/if}

  <div class="groups" bind:this={listEl}>
    {#each groups as g (g.status)}
      <section aria-label={t(`mado.comment.${g.status}`)}>
        <h3 class="gh"><span class="mark s-{g.status}" aria-hidden="true"></span>{t(`mado.comment.${g.status}`)} <span class="pill">{g.items.length}</span></h3>
        <p class="muted gsub">{t(`mado.comment.${g.status}.what`)}</p>
        <ul>
          {#each g.items as c (c.id)}
            {@const d = describe(c.target)}
            <li class="c s-{c.status}" class:focus={focusId === c.id} data-comment={c.id}>
              <button type="button" class="about link" onclick={() => onlocate(c)} title={t('mado.comment.show')}>
                <span class="where">{#if c.target.kind === 'screen'}<MapPin size={12} aria-hidden="true" /> {/if}{d.where}</span>
                {#if d.quote}<q dir="auto">{d.quote}</q>{/if}
              </button>
              {#if editing === c.id}
                <label class="sr-only" for="mado-edit-{c.id}">{t('mado.comment.bodyLabel')}</label>
                <textarea id="mado-edit-{c.id}" bind:value={editBody} rows="3" dir="auto" onkeydown={(e) => editKey(e, c)}></textarea>
                <div class="row end">
                  <button type="button" class="btn ghost" onclick={() => (editing = null)}>{t('cancel')}</button>
                  <button type="button" class="btn primary" disabled={!editBody.trim()} onclick={() => saveEdit(c)}>{t('save')}</button>
                </div>
              {:else}
                <p class="body" dir="auto">{c.body}</p>
                <div class="row foot">
                  <span class="muted when grow">{ago(c.createdAt)}</span>
                  {#if c.status === 'open'}
                    <button type="button" class="ib" onclick={() => startEdit(c)} aria-label={t('mado.comment.edit')} title={t('mado.comment.edit')}><Pencil size={14} /></button>
                  {:else if c.status === 'resolved' || c.status === 'orphaned'}
                    <button type="button" class="ib" onclick={() => onstatus(c.id, 'open')} aria-label={t('mado.comment.reopen')} title={t('mado.comment.reopen')}><Undo2 size={14} /></button>
                  {/if}
                  {#if c.status !== 'sent'}
                    <button type="button" class="ib" onclick={() => onremove(c.id)} aria-label={t('mado.comment.delete')} title={t('mado.comment.delete')}><Trash2 size={14} /></button>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  </div>
  <p class="sr-only" role="status">{tn('mado.comments.count', comments.filter((c) => c.status === 'open').length)}</p>
</div>

<style>
  .panel { display: grid; gap: 14px; align-content: start; }
  .draft { display: grid; gap: 8px; padding: 12px; border-radius: var(--radius-lg); background: var(--tint); animation: mado-in .22s var(--ease) backwards; }
  :global([data-theme="contrast"]) .draft { background: var(--panel); border: var(--border-w) solid var(--accent); }
  .draft .about { color: var(--on-tint); }
  :global([data-theme="contrast"]) .draft .about { color: var(--fg); }
  .draft textarea, .c textarea { resize: vertical; min-height: 64px; background: var(--bg); }
  @keyframes mado-in { from { opacity: 0; transform: translateY(-6px); } }
  .about { display: grid; gap: 3px; text-align: start; font-size: var(--fs-sm); min-width: 0; }
  .where { font-weight: 600; font-size: var(--fs-xs); display: inline-flex; gap: 4px; align-items: center; }
  q { quotes: none; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; border-inline-start: 2px solid currentColor; padding-inline-start: 8px; opacity: .92; overflow-wrap: anywhere; }
  .link { font: inherit; color: var(--muted); background: none; border: 0; padding: 0; cursor: pointer; width: 100%; border-radius: 6px; }
  .link:hover { color: var(--fg); }
  .end { justify-content: flex-end; }
  .gh { font-size: 13px; font-weight: 600; margin: 0; display: flex; gap: 8px; align-items: center; }
  .gsub { font-size: var(--fs-xs); margin: 2px 0 8px; }
  .mark { width: 8px; height: 8px; border-radius: 99px; background: var(--muted); flex: none; }
  .mark.s-open { background: var(--accent-2); }
  .mark.s-sent { background: none; border: 2px solid var(--accent); }
  .mark.s-resolved { background: var(--ok); }
  .mark.s-orphaned { background: none; border: 2px dashed var(--danger); border-radius: 2px; }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 8px; }
  .c { display: grid; gap: 6px; padding: 10px 12px; border-radius: var(--radius-lg); border: var(--border-w) solid var(--hairline); background: var(--panel); transition: box-shadow .2s, border-color .2s; }
  .c.focus { border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 20%, transparent); }
  .c.s-resolved .body { color: var(--muted); }
  .body { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 13.5px; }
  .when { font-size: var(--fs-xs); }
  .foot { gap: 2px; }
  .ib { font: inherit; background: none; border: 0; width: 28px; height: 26px; display: grid; place-items: center; border-radius: 7px; color: var(--muted); cursor: pointer; }
  .ib:hover { background: var(--fill); color: var(--fg); }
  .empty { display: grid; justify-items: center; gap: 6px; text-align: center; padding: 36px 16px; color: var(--muted); }
  .empty b { color: var(--fg); }
  .empty span { max-width: 34ch; font-size: 13px; }
</style>
