<script lang="ts">
  // Settings › Customisations: the changes in progress that are set aside (besides the one open for
  // review), each with Review and Discard. Nothing in them is part of the app until it is kept.
  import { onMount } from 'svelte';
  import { app, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { drafts, draftTitle, discardDraft, openDraft, refreshDrafts } from '../lib/drafts.svelte';

  let busy = $state('');
  onMount(() => { refreshDrafts(); });

  async function act(id: string, fn: () => Promise<unknown>, failKey: string) {
    if (busy) return;
    busy = id;
    try { await fn(); } catch (e) { failed(t(failKey), e); }
    finally { busy = ''; }
  }
  const open = (id: string) => act(id, () => openDraft(id), 'ldb.openFailed');
  async function discard(id: string, title: string) {
    const ok = await confirmDialog({ title: t('ldb.discardTitle', { title }), body: t('ldb.discardBody'), confirmLabel: t('ldb.discard'), danger: true });
    if (ok) await act(id, async () => { await discardDraft(id); toast(t('ldb.discarded'), { key: 'layer' }); }, 'ldb.discardFailed');
  }
</script>

{#if drafts.list.length}
  <div class="waiting" role="group" aria-labelledby="ldb-waiting">
    <p class="sm" id="ldb-waiting"><b>{t(app.pending?.exists ? 'ldb.waitingTitle' : 'ldb.waitingOnly')}</b></p>
    <p class="muted sm">{t('ldb.waitingCardHint')}</p>
    <ul>
      {#each drafts.list as d (d.id)}
        <li>
          <span dir="auto">{draftTitle(d)}</span>
          <div class="row ctl">
            <span class="pill">{d.origin === 'manual' ? t('lda.origin.manual') : t('layer.origin.' + d.origin)}</span>
            <span class="grow"></span>
            <button type="button" class="btn" disabled={!!busy} aria-busy={busy === d.id} onclick={() => open(d.id)}>{t('ldb.open')}</button>
            <button type="button" class="btn ghost danger" disabled={!!busy} onclick={() => discard(d.id, draftTitle(d))}>{t('ldb.discard')}</button>
          </div>
        </li>
      {/each}
    </ul>
  </div>
{/if}

<style>
  .waiting { margin-top: 10px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); }
  .waiting p { margin: 0 0 4px; }
  .sm { font-size: var(--fs-xs); }
  ul { list-style: none; margin: 6px 0 0; padding: 0; display: grid; gap: 6px; }
  li { display: grid; gap: 4px; font-size: 13px; padding-bottom: 6px; border-bottom: 1px solid var(--hairline); }
  li:last-child { border-bottom: 0; padding-bottom: 0; }
  .ctl { gap: 8px; flex-wrap: wrap; }
</style>
