<script lang="ts">
  // The choice asked before a large folder is brought into a library that is sent to a backup
  // location: everything brought in is sent at every Sync and stays in what is sent for good. The
  // safe answer, "Don't bring it in", is chosen until the user picks another; nothing is copied
  // without their choice (the daemon refuses too).
  import Icon from '../lib/Icon.svelte';
  import { t, fmtNum } from '../lib/i18n';
  import { fmtSize } from '../lib/content/util';

  type Keep = 'none' | 'notes' | 'all';
  let { preview, keep = $bindable('none') }: { preview: any; keep?: Keep } = $props();

  const notes = $derived(Number(preview?.notes) || 0);
  const notesOk = $derived(!!preview?.notesFit && notes > 0);
  const allOk = $derived(!!preview?.allowAll);
  const options = $derived([
    { id: 'none' as Keep, label: t('data-import.none'), hint: t('data-import.noneHint'), ok: true },
    { id: 'notes' as Keep, label: t('data-import.notes'), ok: notesOk,
      hint: notes === 0 ? t('data-import.notesNone') : notesOk ? t('data-import.notesHint', { n: fmtNum(notes), size: fmtSize(Number(preview?.notesBytes) || 0) }) : t('data-import.notesTooBig') },
    { id: 'all' as Keep, label: t('data-import.all'), ok: allOk,
      hint: allOk ? t('data-import.allHint', { size: fmtSize(Number(preview?.bytes) || 0) }) : t('data-import.allTooBig') },
  ]);
</script>

<fieldset class="large" aria-describedby="dimp-why">
  <legend class="sr-only">{t('data-import.legend')}</legend>
  <p class="head"><Icon name="warning" size={15} /> <b>{t('data-import.title')}</b></p>
  <p id="dimp-why" class="muted sm">{t('data-import.why', { size: fmtSize(Number(preview?.bytes) || 0) })}</p>
  {#each options as o (o.id)}
    <label class="opt" class:off={!o.ok}>
      <input type="radio" name="dimp-keep" value={o.id} bind:group={keep} disabled={!o.ok} aria-describedby={'dimp-' + o.id} />
      <span><span class="lbl">{o.label}</span><span class="muted sm hint" id={'dimp-' + o.id}>{o.hint}</span></span>
    </label>
  {/each}
</fieldset>

<style>
  .large { border: 1px solid var(--border); border-radius: var(--radius); padding: 10px 12px; margin: 10px 0 4px; background: var(--panel-2); }
  .head { display: flex; gap: 8px; align-items: center; margin: 0 0 4px; }
  .head :global(.icon) { flex: none; color: var(--danger); }
  .opt { display: flex; gap: 8px; align-items: flex-start; margin: 8px 0 0; cursor: pointer; color: var(--fg); font-size: 13px; }
  .opt input { margin-top: 3px; flex: none; }
  .opt.off { cursor: default; opacity: 0.6; }
  .lbl, .hint { display: block; }
  .sm { font-size: var(--fs-xs); }
</style>
