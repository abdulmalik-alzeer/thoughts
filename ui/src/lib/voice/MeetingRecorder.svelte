<script lang="ts">
  // "Record a meeting" on an item's Meetings tab (FEAT-55). The Mac app records into the item's
  // meetings folder; Stop turns the recording into text on this Mac (with progress and Cancel; it
  // goes on in the background), files the transcript as a dated meeting note beside the recording,
  // and offers "Write meeting notes": a chat in which the assistant follows the meeting-notes
  // routine. Recordings without a note yet are listed with "Turn into text".
  import { untrack } from 'svelte';
  import { Mic, Pause, Play, Square } from '@lucide/svelte';
  import Icon from '../Icon.svelte';
  import { t, fmtDate } from '../i18n';
  import { fmtSize } from '../content/util';
  import { go } from '../store.svelte';
  import { openItem } from '../item';
  import {
    voice, voiceShell, initVoice, loadVoice, setVoiceLang, langName, startRecording, pauseRecording, stopRecording,
    discardRecording, transcribe, cancelTranscription, writeMeetingNotes, playRecording, unpaired, recordingMeta, clock, type VoiceLang,
  } from '../voice.svelte';

  let { item, privateMsg, reload }: {
    item: { dir: string; title: string; private: boolean; meetings: any[] };
    /** Why the assistant may not work on this item (it is private), or null. */
    privateMsg: string | null;
    reload: () => void;
  } = $props();

  let shown = $state(false);
  $effect(() => {
    if (!voiceShell()) return;
    initVoice();
    loadVoice().then((s) => (shown = !!s?.supported));
  });
  // While this is on screen the app-wide recording bar is not needed for this item.
  $effect(() => {
    const d = item.dir;
    if (!shown) return;
    untrack(() => voice.panels.push(d));
    return () => { const i = voice.panels.indexOf(d); if (i >= 0) voice.panels.splice(i, 1); };
  });

  const recHere = $derived(voice.rec && voice.rec.item === item.dir ? voice.rec : null);
  const recElsewhere = $derived(voice.rec && voice.rec.item !== item.dir ? voice.rec : null);
  const inItem = (p: string) => String(p ?? '').startsWith(item.dir + '/');
  const jobHere = $derived(voice.job && (voice.job.meta?.item === item.dir || inItem(voice.job.path)) ? voice.job : null);
  const ready = $derived(voice.ready.filter((r) => r.item === item.dir));
  const pending = $derived(unpaired(item.meetings ?? []).filter((f) => f.path !== voice.rec?.path && f.path !== voice.job?.path));

  // The page lists what is in the folder: look again when a recording starts or ends, or a transcript is filed.
  let seen: string | null = null;
  $effect(() => {
    const k = `${voice.rec?.path ?? ''}|${voice.job?.path ?? ''}|${voice.ready.map((r) => r.path).join(',')}`;
    if (seen !== null && k !== seen) untrack(reload);
    seen = k;
  });

  let open = $state(false);
  let topic = $state('');
  let starting = $state(false);
  let topicEl = $state<HTMLInputElement>();
  async function openForm() {
    open = true;
    queueMicrotask(() => topicEl?.focus());
  }
  async function begin() {
    if (starting) return;
    starting = true;
    try { if (await startRecording(item.dir, topic, voice.lang)) { open = false; topic = ''; } }
    finally { starting = false; }
  }
  const pct = (j: { done: number; total: number }) => (j.total > 0 ? Math.min(100, Math.round((j.done / j.total) * 100)) : null);
  const when = (d: string) => (/^\d{4}-\d{2}-\d{2}$/.test(d) ? fmtDate(d + 'T12:00:00', { day: 'numeric', month: 'short', year: 'numeric' }) : '');
  const name = (f: any) => recordingMeta(f.path, item.dir).title || t('voice.rec.meetingDefault');
</script>

{#if shown}
  {#if recHere}
    <section class="card vc rec" aria-label={t('voice.rec.formLabel')}>
      <div class="row head">
        <span class="dot" class:paused={recHere.paused} aria-hidden="true"></span>
        <b>{recHere.paused ? t('voice.rec.paused') : t('voice.rec.recording')}</b>
        <span class="time" role="timer" aria-label={t('voice.rec.elapsed', { time: clock(recHere.seconds) })}>{clock(recHere.seconds)}</span>
        <span class="level" role="meter" aria-label={t('voice.rec.level')} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(recHere.level * 100)}><i style="width: {Math.round(recHere.level * 100)}%"></i></span>
      </div>
      <p class="title" dir="auto">{recHere.title}</p>
      <div class="row wrap btns">
        <button class="btn primary" onclick={stopRecording}><Square size={13} strokeWidth={2.2} /> {t('voice.rec.stop')}</button>
        <button class="btn" onclick={() => pauseRecording(!recHere.paused)}>
          {#if recHere.paused}<Play size={14} /> {t('voice.rec.resume')}{:else}<Pause size={14} /> {t('voice.rec.pause')}{/if}
        </button>
        <button class="btn ghost danger" onclick={discardRecording}><Icon name="trash" size={14} /> {t('voice.rec.discard')}</button>
      </div>
      <p class="muted sm">{t('voice.rec.keepGoing')}</p>
    </section>
  {:else if jobHere}
    {@const p = pct(jobHere)}
    <section class="card vc" aria-label={t('voice.job.title')}>
      <div class="row head"><b>{t('voice.job.title')}</b><span class="grow"></span>
        {#if jobHere.total > 0}<span class="muted sm">{t('voice.job.progress', { done: clock(jobHere.done), total: clock(jobHere.total) })}</span>{/if}
      </div>
      <div class="bar" role="progressbar" aria-label={t('voice.job.title')} aria-valuemin={0} aria-valuemax={100} aria-valuenow={p ?? undefined} class:indet={p === null}>
        <i style={p === null ? '' : `width: ${p}%`}></i>
      </div>
      <div class="row wrap btns">
        <span class="muted sm grow">{t('voice.job.note')}</span>
        <button class="btn sm" onclick={cancelTranscription}>{t('cancel')}</button>
      </div>
    </section>
  {:else if open}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <form class="card vc" aria-label={t('voice.rec.formLabel')} onsubmit={(e) => { e.preventDefault(); begin(); }}
      onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); open = false; } }}>
      <div class="row wrap fields">
        <div class="grow f1">
          <label for="vrec-topic">{t('voice.rec.topic')}</label>
          <input id="vrec-topic" bind:this={topicEl} bind:value={topic} placeholder={t('item.meeting.titlePh')} dir="auto" maxlength="120" />
        </div>
        <div>
          <label for="vrec-lang">{t('voice.rec.lang')}</label>
          <select id="vrec-lang" value={voice.lang} onchange={(e) => setVoiceLang(e.currentTarget.value as VoiceLang)}>
            <option value="en">{langName('en')}</option>
            <option value="ar">{langName('ar')}</option>
          </select>
        </div>
      </div>
      <p class="muted sm" dir="auto"><Icon name="lock" size={12} /> {t('voice.rec.where', { where: item.title })}</p>
      <p class="sm consent">{t('voice.rec.consent')}</p>
      <div class="row wrap btns">
        <button class="btn primary" disabled={starting} aria-busy={starting}><Mic size={14} /> {t('voice.rec.go')}</button>
        <button class="btn" type="button" onclick={() => (open = false)}>{t('cancel')}</button>
      </div>
    </form>
  {:else if recElsewhere}
    <p class="row wrap muted sm elsewhere">
      <span class="dot" aria-hidden="true"></span><span dir="auto">{t('voice.rec.elsewhere', { title: recElsewhere.title })}</span>
      <button class="btn sm" onclick={() => openItem(recElsewhere.item, 'meetings')}>{t('voice.rec.goThere')}</button>
    </p>
  {:else}
    <div class="row wrap start">
      <button class="btn" onclick={openForm} title={t('voice.rec.startTip')}><Mic size={15} /> {t('voice.rec.start')}</button>
    </div>
  {/if}

  {#each ready as r (r.path)}
    <section class="card vc ready" aria-label={t('voice.done.title')}>
      <div class="row head"><Icon name="check" size={15} /><b>{t('voice.done.title')}</b><span class="grow"></span>
        <button class="ib" onclick={() => (voice.ready = voice.ready.filter((x) => x.path !== r.path))} aria-label={t('voice.done.dismiss')} title={t('voice.done.dismiss')}><Icon name="close" size={13} /></button>
      </div>
      <p class="sm" dir="auto">{t('voice.done.body', { title: r.title })}</p>
      {#if !r.complete}<p class="muted sm">{t('voice.done.partial')}</p>{/if}
      {#if item.private && privateMsg}<p class="muted sm" dir="auto"><Icon name="lock" size={12} /> {privateMsg}</p>{/if}
      <div class="row wrap btns">
        <button class="btn" class:primary={!item.private} onclick={() => writeMeetingNotes(r.path, item.private ? privateMsg : null)} title={t('voice.notes.writeTip')}><Icon name="wrap" size={14} /> {t('voice.notes.write')}</button>
        <button class="btn" onclick={() => go({ kind: 'docs', path: r.path })}><Icon name="docs" size={14} /> {t('voice.done.openNote')}</button>
      </div>
    </section>
  {/each}

  {#if pending.length}
    <h3 class="sub">{t('voice.pending.title')}</h3>
    <ul class="pend" role="list">
      {#each pending as f (f.path)}
        {@const m = recordingMeta(f.path, item.dir)}
        <li>
          <span class="grow body"><b dir="auto">{name(f)}</b><span class="muted sm">{when(m.date)}{#if when(m.date)}{' · '}{/if}<bdi>{fmtSize(f.size ?? 0)}</bdi></span></span>
          <button class="btn sm" disabled={!!voice.job} onclick={() => transcribe(f.path, voice.lang, m)}>{t('voice.pending.transcribe')}</button>
          <button class="ib" onclick={() => playRecording(f.path)} aria-label={t('voice.notes.play')} title={t('voice.notes.play')}><Play size={14} /></button>
        </li>
      {/each}
    </ul>
  {/if}
{/if}

<style>
  .vc { margin-bottom: 12px; display: grid; gap: 8px; }
  .head { gap: 8px; align-items: center; }
  .grow { flex: 1; }
  .wrap { flex-wrap: wrap; }
  .btns { gap: 8px; align-items: center; }
  .sm { font-size: var(--fs-xs); margin: 0; }
  .btn.sm { padding: 3px 10px; }
  .start { margin-bottom: 12px; }
  .fields { align-items: flex-end; gap: 10px; }
  .fields label { margin-top: 0; }
  .fields select { width: auto; }
  .f1 { min-width: min(260px, 100%); }
  .consent { margin: 0; color: var(--fg); }
  .title { margin: 0; font-weight: 500; overflow-wrap: anywhere; }
  .dot { width: 10px; height: 10px; border-radius: 50%; background: var(--danger); flex: none; animation: blink 1.4s ease-in-out infinite; }
  .dot.paused { animation: none; background: var(--muted); }
  @keyframes blink { 50% { opacity: 0.3; } }
  @media (prefers-reduced-motion: reduce) { .dot { animation: none; } }
  .time { font-variant-numeric: tabular-nums; font-size: 18px; font-weight: 600; direction: ltr; }
  .level { flex: 1; max-width: 220px; height: 6px; border-radius: 99px; background: var(--fill-2, var(--fill)); overflow: hidden; }
  .level i { display: block; height: 100%; background: var(--ok); transition: width 200ms linear; }
  .bar { height: 8px; border-radius: 99px; background: var(--fill-2, var(--fill)); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--accent); transition: width 300ms ease; }
  .bar.indet i { width: 30%; animation: slide 1.4s ease-in-out infinite; }
  @keyframes slide { from { margin-inline-start: -30%; } to { margin-inline-start: 100%; } }
  @media (prefers-reduced-motion: reduce) { .bar.indet i { animation: none; width: 100%; opacity: 0.4; } }
  .elsewhere { gap: 8px; align-items: center; margin: 0 0 12px; }
  .sub { font-size: 15px; margin: 4px 0 8px; }
  .pend { list-style: none; margin: 0 0 14px; padding: 0; display: grid; gap: 6px; }
  .pend li { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-radius: var(--radius); background: var(--fill); }
  .body { display: grid; gap: 1px; min-width: 0; }
  .body b { overflow-wrap: anywhere; font-weight: 600; }
  .ib { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 5px; min-width: 26px; min-height: 26px; cursor: pointer; display: grid; place-items: center; }
  .ib:hover, .ib:focus-visible { color: var(--fg); background: var(--fill-2); }
</style>
