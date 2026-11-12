<script lang="ts">
  // Settings › General › Quick capture: the keyboard shortcut that opens the capture box from any
  // app. Applied at once by the desktop app (which registers it with the system and says when it
  // cannot), then saved in the settings for the next start — so it is never part of the page's
  // unsaved draft. Shown only in the desktop app.
  import { onDestroy, onMount } from 'svelte';
  import { saveSettings } from './store.svelte';
  import { t } from './i18n';
  import { failed } from './errors';
  import { inTauri } from './api';
  import { isolated, keysFromEvent, keysLabel, modsText } from './quickKeys';

  type State = { on: boolean; keys: string; defaultKeys: string; active: boolean; problem: string | null; supported: boolean };
  let st = $state<State | null>(null);
  let recording = $state(false);
  let busy = $state(false);
  let note = $state<{ tone: 'ok' | 'error'; text: string } | null>(null);

  async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<T>(cmd, args);
  }

  onMount(async () => {
    if (!inTauri()) return;
    try { st = await invoke<State>('capture_shortcut'); } catch { st = null; /* an older desktop app */ }
  });

  const shown = (keys: string) => isolated(keysLabel(keys));
  /** The words for a refusal code from the desktop app. */
  function why(code: string, keys: string): string {
    const k = `nativeb.set.err.${code}`;
    const s = t(k, { keys: shown(keys), mods: modsText() });
    return s === k ? t('nativeb.set.failed') : s;
  }
  const problem = $derived.by(() => {
    if (!st?.on || st.active || !st.problem) return '';
    return st.problem === 'TAKEN' ? t('nativeb.set.problem.TAKEN', { keys: shown(st.keys) }) : why(st.problem, st.keys);
  });

  async function apply(on: boolean, keys: string, box?: HTMLInputElement) {
    if (!st || busy) return;
    busy = true;
    note = null;
    try {
      st = await invoke<State>('set_capture_shortcut', { on, keys });
      // The usual combination is saved as "the usual one", so it follows the app if that ever changes.
      try { await saveSettings({ capture: { shortcutOn: st.on, shortcut: st.keys === st.defaultKeys ? '' : st.keys } }); }
      catch (e) { failed(t('nativeb.set.saveFailed'), e); }
      if (on) note = { tone: 'ok', text: t('nativeb.set.saved', { keys: shown(st.keys) }) };
    } catch (e: any) {
      note = { tone: 'error', text: why(String(e?.message ?? e), keys) };
      try { st = await invoke<State>('capture_shortcut'); } catch { /* keep what we had */ }
    } finally {
      busy = false;
      // A switch the click moved stays where the desktop app really is.
      if (box && st) box.checked = st.on;
    }
  }

  // ----- listening for a new combination -----
  async function record() {
    if (!st || busy || recording) return;
    note = null;
    recording = true;
    // The current one is let go meanwhile, so pressing it again reaches this page.
    try { await invoke('pause_capture_shortcut', { paused: true }); } catch { /* still listen */ }
    window.addEventListener('keydown', onKey, true);
  }
  function stopListening() {
    window.removeEventListener('keydown', onKey, true);
    recording = false;
  }
  function cancel() {
    stopListening();
    invoke('pause_capture_shortcut', { paused: false }).catch(() => {});
  }
  function onKey(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.isComposing) return;
    if (e.code === 'Escape' && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) { cancel(); return; }
    const keys = keysFromEvent(e);
    if (!keys) return; // only modifier keys so far
    stopListening();
    apply(true, keys);
  }
  onDestroy(() => { if (recording) cancel(); });

  async function tryIt() {
    try { await invoke('open_capture'); } catch (e) { failed(t('nativeb.set.failed'), e); }
  }
</script>

{#if st}
  <fieldset class="qcap" aria-busy={busy}>
    <legend>{t('nativeb.set.title')}</legend>
    <p class="sm muted">{t('nativeb.set.intro')}</p>
    <label class="check">
      <input type="checkbox" checked={st.on} disabled={busy || recording} onchange={(e) => apply(e.currentTarget.checked, st!.keys, e.currentTarget)} />
      {t('nativeb.set.switch')}
    </label>
    {#if st.on}
      <div class="row">
        <span>{t('nativeb.set.shortcut')}</span>
        <kbd dir="ltr" class:dim={!st.active}>{keysLabel(st.keys)}</kbd>
        {#if recording}
          <button type="button" class="btn" onclick={cancel}>{t('nativeb.set.cancel')}</button>
        {:else}
          <button type="button" class="btn" disabled={busy} onclick={record}>{t('nativeb.set.change')}</button>
          {#if st.keys !== st.defaultKeys}
            <button type="button" class="btn ghost" disabled={busy} onclick={() => apply(true, st!.defaultKeys)}>{t('nativeb.set.reset', { keys: shown(st.defaultKeys) })}</button>
          {/if}
        {/if}
        <button type="button" class="btn ghost" onclick={tryIt}>{t('nativeb.set.try')}</button>
      </div>
      {#if recording}
        <p class="sm listen" role="status">{t('nativeb.set.recording')} {t('nativeb.set.recordHint', { mods: modsText() })}</p>
      {/if}
      {#if problem && !note}<p class="sm err" role="alert">{problem}</p>{/if}
    {:else}
      <p class="sm muted">{t('nativeb.set.offNote')}</p>
    {/if}
    {#if note}<p class="sm" class:err={note.tone === 'error'} role={note.tone === 'error' ? 'alert' : 'status'}>{note.text}</p>{/if}
  </fieldset>
{/if}

<style>
  .qcap { border: 0; padding: 0; margin: 14px 0 0; min-width: 0; }
  .qcap legend { font-weight: 600; padding: 0; margin-bottom: 4px; }
  .check { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; margin-top: 6px; }
  .row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-top: 6px; }
  kbd { font-size: 13px; padding: 2px 8px; }
  kbd.dim { opacity: .6; text-decoration: line-through; }
  .sm { font-size: var(--fs-xs); margin: 4px 0 0; }
  .listen { color: var(--fg); font-weight: 500; }
  .err { color: var(--danger); }
</style>
