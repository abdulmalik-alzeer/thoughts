<script lang="ts">
  // A banner at the top of every page while a meeting is being recorded or turned into text
  // (FEAT-55), wherever the user is in the app, so a recording is never forgotten; it covers
  // nothing. Not shown on the item's own Meetings tab, which has the full controls.
  import { onMount } from 'svelte';
  import { Square } from '@lucide/svelte';
  import { t } from '../i18n';
  import { openItem } from '../item';
  import { voice, voiceShell, initVoice, stopRecording, cancelTranscription, clock } from '../voice.svelte';

  onMount(() => { if (voiceShell()) initVoice(); });

  const rec = $derived(voice.rec && !voice.rec.ended && !voice.panels.includes(voice.rec.item) ? voice.rec : null);
  const jobItem = $derived(voice.job ? String(voice.job.meta?.item || voice.job.path.split('/meetings/')[0]) : '');
  const job = $derived(voice.job && !rec && !voice.panels.includes(jobItem) ? voice.job : null);
  const pct = $derived(job && job.total > 0 ? Math.min(100, Math.round((job.done / job.total) * 100)) : null);
</script>

{#if rec}
  <div class="vbar no-print" role="region" aria-label={t('voice.rec.formLabel')}>
    <span class="dot" class:paused={rec.paused} aria-hidden="true"></span>
    <span class="txt" dir="auto">{t(rec.paused ? 'voice.bar.paused' : 'voice.bar.recording', { title: rec.title })}</span>
    <span class="time" role="timer" aria-label={t('voice.rec.elapsed', { time: clock(rec.seconds) })}>{clock(rec.seconds)}</span>
    <button class="btn sm" onclick={() => openItem(rec.item, 'meetings')}>{t('voice.bar.open')}</button>
    <button class="btn sm primary" onclick={stopRecording}><Square size={12} strokeWidth={2.2} /> {t('voice.bar.stop')}</button>
  </div>
{:else if job}
  <div class="vbar no-print" role="region" aria-label={t('voice.job.title')}>
    <span class="txt" dir="auto">{t('voice.bar.transcribing', { title: String(job.meta?.title || t('voice.rec.meetingDefault')) })}</span>
    <span class="bar" role="progressbar" aria-label={t('voice.job.title')} aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct ?? undefined}><i style="width: {pct ?? 30}%"></i></span>
    {#if pct !== null}<span class="time">{pct}%</span>{/if}
    {#if jobItem}<button class="btn sm" onclick={() => openItem(jobItem, 'meetings')}>{t('voice.bar.open')}</button>{/if}
    <button class="btn sm" onclick={cancelTranscription}>{t('cancel')}</button>
  </div>
{/if}

<style>
  .vbar { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 10px; margin: 4px 28px 0; padding: 7px 10px 7px 14px; border-radius: var(--radius-lg); background: var(--panel);
    border: var(--border-w) solid var(--border); font-size: 13px; }
  .txt { flex: 1 1 180px; min-width: 0; overflow-wrap: anywhere; font-weight: 500; }
  .time { font-variant-numeric: tabular-nums; direction: ltr; color: var(--muted); }
  .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--danger); flex: none; animation: blink 1.4s ease-in-out infinite; }
  .dot.paused { animation: none; background: var(--muted); }
  @keyframes blink { 50% { opacity: 0.3; } }
  @media (prefers-reduced-motion: reduce) { .dot { animation: none; } }
  .bar { width: 90px; height: 6px; border-radius: 99px; background: var(--fill-2, var(--fill)); overflow: hidden; flex: none; }
  .bar i { display: block; height: 100%; background: var(--accent); transition: width 300ms ease; }
  .btn.sm { padding: 3px 10px; flex: none; }
</style>
