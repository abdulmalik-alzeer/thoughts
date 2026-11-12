<script lang="ts">
  // The message that waits for the current answer to finish: shown in full, and it can be taken
  // back to edit, dropped, or sent now (stopping the answer when one is still being written).
  import Icon from '../../lib/Icon.svelte';
  import { t, tn } from '../../lib/i18n';
  import type { Queued } from '../../lib/chatQueue.svelte';

  let { q, running, onedit, oncancel, onsend }: { q: Queued; running: boolean; onedit: () => void; oncancel: () => void; onsend: () => void } = $props();
</script>

<div class="queued" role="group" aria-label={t('chatc.queue.label')}>
  <div class="qhead">
    <Icon name={running ? 'clock' : 'info'} size={13} />
    <span class="grow" role="status">{t(running ? 'chatc.queue.waits' : 'chatc.queue.held')}</span>
  </div>
  <div class="qtext" dir="auto">{q.text}</div>
  {#if q.files.length || q.mentions.length}
    <div class="muted qmeta">
      {#if q.files.length}<span><Icon name="attach" size={11} /> {tn('chatc.queue.files', q.files.length)}</span>{/if}
      {#each q.mentions as m (m.path)}<bdi title={m.path}>@{m.label}</bdi>{/each}
    </div>
  {/if}
  <div class="qacts">
    <button type="button" class="act" onclick={onedit}><Icon name="edit" size={13} /><span>{t('edit')}</span></button>
    <button type="button" class="act" onclick={onsend} title={running ? t('chatc.queue.nowTip') : undefined}><Icon name="send" size={13} /><span>{t(running ? 'chatc.queue.now' : 'chatc.queue.sendNow')}</span></button>
    <button type="button" class="act" onclick={oncancel}><Icon name="close" size={13} /><span>{t('chatc.queue.cancel')}</span></button>
  </div>
</div>

<style>
  .queued { margin: 0 0 6px; padding: 8px 10px; border-radius: 12px; background: var(--fill); border: var(--border-w) dashed var(--border); display: grid; gap: 4px; }
  .qhead { display: flex; align-items: center; gap: 6px; font-size: var(--fs-xs); font-weight: 600; color: var(--muted); }
  .grow { flex: 1; }
  .qtext { white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--fs-sm); max-height: 7.5em; overflow-y: auto; }
  .qmeta { display: flex; flex-wrap: wrap; gap: 8px; font-size: var(--fs-xs); }
  .qmeta span { display: inline-flex; align-items: center; gap: 3px; }
  .qacts { display: flex; flex-wrap: wrap; gap: 2px; }
  .act { font: inherit; font-size: var(--fs-xs); color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 3px 7px; cursor: pointer; display: inline-flex; align-items: center; gap: 5px; }
  .act:hover { color: var(--fg); background: var(--fill-2); }
</style>
