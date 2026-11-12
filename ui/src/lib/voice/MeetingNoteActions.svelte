<script lang="ts">
  // Beside a meeting note made from a recording (FEAT-55): play the recording, and, while the note
  // is still only a transcript (no summary yet), "Write meeting notes" from it.
  import { Play } from '@lucide/svelte';
  import Icon from '../Icon.svelte';
  import { inTauri } from '../api';
  import { t } from '../i18n';
  import { recordingOf, playRecording, writeMeetingNotes } from '../voice.svelte';

  let { note, files, privateMsg }: { note: { path: string; summary?: string | null; title?: string | null }; files: { path: string }[]; privateMsg: string | null } = $props();

  const rec = $derived(/\.md$/i.test(note.path) ? recordingOf(note.path, files) : null);
  const name = $derived(String(note.title || note.path.split('/').pop()));
</script>

{#if rec}
  {#if !String(note.summary ?? '').trim()}
    <button class="ib" onclick={() => writeMeetingNotes(note.path, privateMsg)} aria-label={`${t('voice.notes.write')}: ${name}`} title={t('voice.notes.writeTip')}><Icon name="wrap" size={14} /></button>
  {/if}
  {#if inTauri()}
    <button class="ib" onclick={() => playRecording(rec)} aria-label={`${t('voice.notes.play')}: ${name}`} title={t('voice.notes.play')}><Play size={14} /></button>
  {/if}
{/if}

<style>
  .ib { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 5px; min-width: 26px; min-height: 26px; cursor: pointer; display: grid; place-items: center; }
  .ib:hover, .ib:focus-visible { color: var(--fg); background: var(--fill-2); }
</style>
