<script lang="ts">
  // The dictation button of a text box (the chat's message box, the note editor). Press it and
  // speak: the words appear in a small panel as they are recognised, on this Mac; Insert (or the
  // button again, or Enter) puts them where the cursor was. Shown only where the desktop app can
  // turn speech into text without sending it anywhere (the Mac app).
  import { onDestroy, onMount, untrack } from 'svelte';
  import { Mic } from '@lucide/svelte';
  import { t } from '../i18n';
  import { toast } from '../store.svelte';
  import {
    voice, voiceShell, initVoice, loadVoice, ensurePermission, ensureLang, setVoiceLang, langName,
    startDictation, stopDictation, cancelDictation, insertText, voiceError, voiceCode, type VoiceLang,
  } from '../voice.svelte';
  import { boxElement, type TextBox } from '../composer/textBox';

  let { target, tool = false, tabindex = undefined, place = 'above' }: {
    /** The text box the words go into. */
    target: () => HTMLTextAreaElement | TextBox | null | undefined;
    /** Part of a toolbar with arrow-key movement (`data-tool`). */
    tool?: boolean;
    tabindex?: number;
    /** Where the listening panel opens. */
    place?: 'above' | 'below';
  } = $props();

  const id = 'dict-' + Math.random().toString(36).slice(2, 8);
  let shown = $state(false);
  let busy = $state(false);
  const mine = $derived(voice.owner === id);
  /** Where the cursor was in the text box when listening started. */
  let sel: [number, number] | null = null;

  onMount(() => {
    if (!voiceShell()) return;
    initVoice();
    loadVoice().then((s) => (shown = !!s?.supported));
  });
  onDestroy(() => { if (voice.owner === id) cancelDictation(); });

  async function start(l: VoiceLang = voice.lang) {
    const el = target();
    if (!el || busy) return;
    sel = [el.selectionStart, el.selectionEnd];
    busy = true;
    try {
      if (!(await ensurePermission(true))) return;
      const use = await ensureLang(l);
      if (!use || use === 'later') return;
      await startDictation(use, id);
      // Back to the text box (a question may have taken the focus), where Enter inserts.
      const box = target();
      if (box && document.activeElement !== boxElement(box)) { box.focus(); if (sel) box.setSelectionRange(sel[0], sel[1]); }
    } catch (e) { voiceError(e); }
    finally { busy = false; }
  }

  async function finish() {
    if (busy) return;
    busy = true;
    try {
      const text = await stopDictation();
      const el = target();
      if (text.trim() && el) insertText(el, text, sel); else toast(t('voice.dict.nothing'), { key: 'voice-dict' });
    } catch (e) { voiceError(e); }
    finally { busy = false; }
  }

  function cancel() {
    cancelDictation();
    target()?.focus();
  }

  async function changeLang(l: VoiceLang) {
    setVoiceLang(l);
    if (!mine) return;
    // Start again in the other language; nothing heard so far is lost.
    const heard = voice.partial;
    cancelDictation();
    const el = target();
    if (heard.trim() && el) { insertText(el, heard, sel); sel = [el.selectionStart, el.selectionEnd]; }
    await start(l);
  }

  // Dictation that stopped by itself: keep what was heard and say so.
  $effect(() => {
    const e = voice.ended;
    if (!e || e.owner !== id) return;
    untrack(() => {
      voice.ended = null;
      const el = target();
      if (e.text.trim() && el) insertText(el, e.text, sel);
      if (e.error && !e.text.trim()) voiceError(e.error);
      else toast(t(e.text.trim() ? 'voice.dict.ended' : 'voice.dict.endedEmpty'), { key: 'voice-dict', kind: e.text.trim() ? 'info' : 'error',
        detail: e.error ? voiceCode(e.error).detail || undefined : undefined });
    });
  });

  /** Keep the panel inside the window: moved toward the middle when it would stick out of either side. */
  function fit(node: HTMLElement) {
    const place = () => {
      node.style.translate = '';
      const r = node.getBoundingClientRect();
      const over = r.right - (window.innerWidth - 12);
      const under = 12 - r.left;
      if (over > 0) node.style.translate = `${-over}px 0`;
      else if (under > 0) node.style.translate = `${under}px 0`;
    };
    place();
    window.addEventListener('resize', place);
    return { destroy: () => window.removeEventListener('resize', place) };
  }

  // While listening: Esc cancels (unless a dialog is open, which Esc closes); Enter in the text box
  // inserts the words instead of sending the message.
  function onKey(e: KeyboardEvent) {
    if (!mine || e.isComposing) return;
    if (e.key === 'Escape' && !document.querySelector('.modal-bg')) { e.preventDefault(); e.stopPropagation(); cancel(); return; }
    const inBox = (!!document.activeElement && document.activeElement === boxElement(target())) || !!(document.activeElement as HTMLElement | null)?.closest?.(`[data-dict="${id}"]`);
    if (!inBox) return;
    if (e.key === 'Enter' && !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey && (e.target as HTMLElement)?.tagName !== 'SELECT') {
      e.preventDefault(); e.stopPropagation(); finish();
    }
  }
</script>

<svelte:window onkeydowncapture={onKey} />

{#if shown}
  <span class="dict" data-dict={id}>
    <!-- The cursor stays in the text box: the words go where it is. -->
    <button type="button" class="mic" class:tb={tool} class:btn={!tool} class:ghost={!tool} class:on={mine} data-tool={tool ? '' : undefined} {tabindex}
      onmousedown={(e) => e.preventDefault()} onclick={() => (mine ? finish() : start())} aria-pressed={mine} aria-busy={busy}
      aria-label={mine ? t('voice.dict.stop') : t('voice.dict.start')} title={mine ? t('voice.dict.stop') : t('voice.dict.startTip')}>
      <Mic size={tool ? 17 : 16} strokeWidth={1.9} />
      {#if mine}<span class="dot" aria-hidden="true"></span>{/if}
    </button>
    {#if mine}
      <div class="pop card" class:below={place === 'below'} use:fit role="group" aria-label={t('voice.dict.listening')}>
        <div class="top">
          <span class="meter" aria-hidden="true">{#each [0.6, 1, 0.75] as k, i (i)}<i style="transform: scaleY({0.18 + Math.min(1, voice.level * k * 1.3) * 0.82})"></i>{/each}</span>
          <b>{t('voice.dict.listening')}</b>
          <span class="sr-only" role="status">{t('voice.dict.listeningIn', { in: t('voice.lang.in.' + voice.lang) })}</span>
          <span class="grow"></span>
          <select class="lang" aria-label={t('voice.lang.label')} title={t('voice.lang.label')} value={voice.lang} onchange={(e) => changeLang(e.currentTarget.value as VoiceLang)}>
            <option value="en">{langName('en')}</option>
            <option value="ar">{langName('ar')}</option>
          </select>
        </div>
        <p class="said" class:muted={!voice.partial} dir="auto">{voice.partial || t('voice.dict.speakNow')}</p>
        <p class="muted sm">{t('voice.dict.local')}</p>
        <div class="btns">
          <span class="muted sm grow">{t('voice.dict.keys')}</span>
          <button type="button" class="btn sm" onclick={cancel}>{t('cancel')}</button>
          <button type="button" class="btn primary sm" onclick={finish} disabled={busy}>{t('voice.dict.insert')}</button>
        </div>
      </div>
    {/if}
  </span>
{/if}

<style>
  .dict { position: relative; display: inline-flex; }
  .mic { position: relative; }
  /* The note editor's toolbar look (its own buttons are styled inside the editor). */
  .tb { font: inherit; color: var(--fg); background: none; border: 0; min-width: 32px; height: 32px; padding: 0 6px; border-radius: 7px; display: inline-grid; place-items: center; cursor: pointer; flex: none; }
  .tb:hover:not(:disabled) { background: var(--fill); }
  .mic.on, .tb.on { color: var(--danger); }
  .tb.on { background: color-mix(in srgb, var(--danger) 12%, transparent); }
  .dot { position: absolute; top: 3px; inset-inline-end: 3px; width: 7px; height: 7px; border-radius: 50%; background: var(--danger); animation: blink 1.2s ease-in-out infinite; }
  @keyframes blink { 50% { opacity: 0.25; } }
  @media (prefers-reduced-motion: reduce) { .dot { animation: none; } .meter i { transition: none; } }
  .pop { position: absolute; bottom: calc(100% + 8px); inset-inline-start: 0; z-index: 30; width: min(380px, calc(100vw - 24px)); padding: 10px 12px; display: grid; gap: 6px; box-shadow: var(--shadow-lg, 0 8px 30px rgba(0, 0, 0, 0.18)); }
  .pop.below { bottom: auto; top: calc(100% + 8px); }
  .top { display: flex; align-items: center; gap: 8px; }
  .grow { flex: 1; }
  .lang { width: auto; font-size: 12.5px; padding: 2px 6px; }
  .meter { display: inline-flex; align-items: center; gap: 2px; height: 16px; }
  .meter i { display: block; width: 3px; height: 16px; border-radius: 2px; background: var(--danger); transform-origin: center; transition: transform 90ms linear; }
  .said { margin: 0; max-height: 9em; overflow-y: auto; font-size: 14px; line-height: 1.5; overflow-wrap: anywhere; }
  .sm { font-size: var(--fs-xs); margin: 0; }
  .btns { display: flex; align-items: center; gap: 6px; }
  .btn.sm { padding: 3px 10px; }
</style>
