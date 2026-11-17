<script lang="ts">
  // What one answer changed in the library (FEAT-06), shown under it: "Changed 3 notes" opens the
  // changes document by document; Undo takes back this answer's files only (merged with whatever
  // changed them since, refused when a later change touched the same lines); after that, "Put them
  // back". What the user or another chat changed while the answer was written is named, never counted.
  import Icon from '../../lib/Icon.svelte';
  import DocChanges from '../../lib/DocChanges.svelte';
  import { api, post, ApiError } from '../../lib/api';
  import { errCode, errDetail, failed } from '../../lib/errors';
  import { t, tn } from '../../lib/i18n';
  import { toast } from '../../lib/store.svelte';

  let { chatId, m, busy = false, onchange }: { chatId: string; m: any; busy?: boolean; onchange?: (m: any) => void } = $props();

  const c = $derived(m.tool ?? {});
  const total = $derived(Number(c.total) || (c.files?.length ?? 0));
  const files = $derived<any[]>(c.files ?? []);
  // Notes are Markdown documents; anything else makes it "files".
  const allNotes = $derived(files.length === total && files.every((f) => /\.md$/i.test(String(f.path))));
  const undone = $derived(!!c.undone);
  const others = $derived<string[]>(c.others ?? []);
  const othersTotal = $derived(Number(c.othersTotal) || others.length);
  const kinds = $derived(['added', 'edited', 'moved', 'deleted'].filter((k) => Number(c.counts?.[k]) > 0));

  let open = $state(false);
  let docs = $state<any[] | null>(null);
  let loadErr = $state('');
  let loading = $state(false);
  let working = $state(false);
  // Why the last Undo or "Put them back" was refused, in the service's plain words (kept on screen).
  let refusal = $state('');
  const panelId = `cs-${m.id}`;

  async function load() {
    loading = true; loadErr = '';
    try { docs = (await api(`/chats/${encodeURIComponent(chatId)}/changes/${encodeURIComponent(m.id)}`)).files ?? []; }
    catch (e: any) { loadErr = String(e?.message ?? e).replace(/(^|\s)[A-Z][A-Z_]{2,}::/, '$1'); }
    finally { loading = false; }
  }
  function toggle() {
    open = !open;
    if (open && !docs && !loading) load();
  }
  async function act(op: 'undo' | 'redo') {
    if (working) return;
    working = true;
    refusal = '';
    try {
      const r = await post(`/chats/${encodeURIComponent(chatId)}/changes/${encodeURIComponent(m.id)}/${op}`);
      if (r?.message) onchange?.(r.message);
      // Undone by mistake: it can be put back straight from the note that says so.
      if (op === 'undo') toast(t('cs.undoneToast'), { kind: 'success', action: { label: t('cs.back'), run: () => act('redo') } });
      else toast(t('cs.backToast'), { kind: 'success' });
    } catch (e) {
      // Refused (a later change touched the same lines, the assistant is at work, the history no longer
      // has it): the reason stays next to the buttons. Anything else is a failure like any other.
      const refused = e instanceof ApiError && [404, 409, 423].includes(e.status) && !['NO_HISTORY', 'WRAP_INTERRUPTED', 'WRAP_REVIEW'].includes(errCode(e));
      if (refused) refusal = errDetail(e) || t(op === 'undo' ? 'cs.undoFailed' : 'cs.backFailed');
      else failed(t(op === 'undo' ? 'cs.undoFailed' : 'cs.backFailed'), e);
    } finally { working = false; }
  }
</script>

<div class="msg changes" role="group" aria-label={t('cs.label')}>
  <div class="row line">
    <button class="act show sum" onclick={toggle} aria-expanded={open} aria-controls={panelId}>
      <Icon name="docs" size={14} />
      <span>{tn(allNotes ? 'cs.notes' : 'cs.files', total)}</span>
      <Icon name={open ? 'down' : 'right'} size={13} class={open ? '' : 'flip-rtl'} />
    </button>
    {#if kinds.length > 1 || (kinds.length === 1 && kinds[0] !== 'edited')}
      <span class="muted kinds">{kinds.map((k) => tn('cs.kind.' + k, Number(c.counts[k]))).join(t('cs.sep'))}</span>
    {/if}
    {#if undone}
      <span class="pill">{t('cs.undoneTag')}</span>
      <button class="act show" onclick={() => act('redo')} disabled={working || busy} title={busy ? t('cs.busy') : t('cs.backTip')}>
        <Icon name="retry" size={14} /><span>{t('cs.back')}</span>
      </button>
    {:else}
      <button class="act show" onclick={() => act('undo')} disabled={working || busy} title={busy ? t('cs.busy') : t('cs.undoTip')}>
        <Icon name="undo" size={14} class="flip-rtl" /><span>{t('cs.undo')}</span>
      </button>
    {/if}
  </div>
  {#if refusal}
    <p class="note err" role="alert" dir="auto"><Icon name="warning" size={13} /> <span>{refusal}</span></p>
  {/if}
  {#if othersTotal > 0}
    <p class="note muted" dir="auto">
      <Icon name="info" size={13} />
      <span>{tn(c.othersUnsure ? 'cs.othersUnsure' : 'cs.others', othersTotal)}
        {#each others.slice(0, 5) as p, i (p)}{i ? t('cs.sep') : ' '}<bdi dir="ltr">{p}</bdi>{/each}{#if othersTotal > 5}{t('cs.sep')}{tn('cs.more', othersTotal - 5)}{/if}</span>
    </p>
  {/if}
  {#if open}
    <div class="panel" id={panelId}>
      {#if loading}<p class="muted note" role="status">{t('cs.loading')}</p>
      {:else if loadErr}
        <p class="note err" role="alert"><Icon name="warning" size={13} /> <span dir="auto">{loadErr}</span></p>
        <button class="act show" onclick={load}><Icon name="retry" size={14} /><span>{t('cs.retry')}</span></button>
      {:else if docs}
        {#if undone}<p class="muted note">{t('cs.undoneNote')}</p>{/if}
        <DocChanges files={docs} canOpen />
      {/if}
    </div>
  {/if}
</div>

<style>
  .msg.changes { font-size: var(--fs-sm); padding: 4px 6px; border-radius: var(--radius); border: var(--border-w, 1px) solid var(--border); display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .line { gap: 6px; flex-wrap: wrap; align-items: center; }
  .act { font: inherit; font-size: var(--fs-xs); color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 3px 7px; cursor: pointer; display: inline-flex; align-items: center; gap: 5px; }
  .act:hover:not(:disabled) { color: var(--fg); background: var(--fill); }
  .act:disabled { opacity: .55; cursor: default; }
  .sum { color: var(--fg); font-weight: 500; font-size: var(--fs-sm); }
  .kinds { font-size: var(--fs-xs); }
  .pill { font-size: var(--fs-xs); }
  .note { margin: 0; font-size: var(--fs-xs); display: flex; gap: 6px; align-items: flex-start; overflow-wrap: anywhere; padding-inline: 7px; }
  .note :global(.icon) { flex: none; margin-top: 2px; }
  .err { color: var(--danger); }
  .panel { display: grid; gap: 6px; padding: 2px; min-width: 0; }
</style>
