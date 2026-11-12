<script lang="ts">
  // "Report a problem": prepares a report the user reads in full, then copies or saves themselves.
  // Nothing is sent anywhere from here.
  import Icon from '../lib/Icon.svelte';
  import { api } from '../lib/api';
  import { app, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { saveFile } from '../lib/download';

  let { back }: { back: () => void } = $props();

  let what = $state('');
  let diag: any = $state(null);
  let phase: 'loading' | 'full' | 'basic' = $state('loading');

  $effect(() => {
    let live = true;
    api('/diagnostics').then((d) => { if (live) { diag = d; phase = 'full'; } }, () => { if (live) phase = 'basic'; });
    return () => { live = false; };
  });

  // The report is always English: whoever reads it may not read the user's language.
  const report = $derived.by(() => {
    const d = diag;
    const lines = [
      'Problem report — Thoughts', '',
      'What happened:', what.trim() || '(nothing written)', '',
      `Thoughts ${d?.version ?? app.status?.version ?? '?'} (${d?.platform ?? app.status?.platform ?? '?'})`,
      `Window: ${typeof navigator !== 'undefined' ? navigator.userAgent : '?'}`,
      `Language: ${app.settings?.language ?? '?'} · Theme: ${app.settings?.theme ?? '?'}/${app.settings?.mode ?? '?'} · Safe mode: ${app.safeMode ? 'on' : 'off'}`,
    ];
    if (d) {
      const { version: _v, platform: _p, log, ...rest } = d;
      lines.push('', '--- details ---', JSON.stringify(rest, null, 2), '', '--- log ---', String(log ?? '').trim());
    } else if (phase === 'basic') {
      lines.push(`Assistant installed: ${app.status?.claude?.installed ?? '?'} · signed in: ${app.status?.claude?.loggedIn ?? '?'}`, `Git installed: ${app.status?.git?.installed ?? '?'} ${app.status?.git?.version ?? ''}`);
    }
    return lines.join('\n');
  });

  let copied = $state(false);
  async function copy() {
    try { await navigator.clipboard.writeText(report); copied = true; setTimeout(() => (copied = false), 2500); toast(t('help.rep.copied'), { kind: 'success' }); }
    catch { toast(t('help.rep.copyFailed'), { kind: 'error' }); }
  }
  async function save() {
    const r = await saveFile('thoughts-problem-report.txt', report);
    if (r === 'saved') toast(t('help.rep.saved'), { kind: 'success' });
    else if (r === 'unsupported') toast(t('help.rep.saveFailed'), { kind: 'error' });
  }
</script>

<p><button class="btn" onclick={back}><Icon name="left" size={15} /> {t('help.back')}</button></p>
<h1 class="row"><Icon name="warning" size={24} /> {t('help.rep.title')}</h1>
<p class="muted lead">{t('help.rep.lead')}</p>

<section>
  <label for="rep-what"><b>{t('help.rep.what')}</b></label>
  <textarea id="rep-what" rows="5" dir="auto" bind:value={what} placeholder={t('help.rep.whatHint')}></textarea>
</section>

<section aria-labelledby="rep-prev">
  <h2 id="rep-prev">{t('help.rep.preview')}</h2>
  <p class="muted">{t('help.rep.previewBody')}</p>
  {#if phase === 'loading'}<p class="muted" role="status">{t('help.rep.loading')}</p>
  {:else if phase === 'basic'}<p class="muted" role="status">{t('help.rep.basicOnly')}</p>{/if}
  <textarea class="preview" rows="16" readonly dir="ltr" aria-labelledby="rep-prev" value={report}></textarea>
  <div class="row wrap">
    <button class="btn primary" disabled={phase === 'loading'} onclick={copy}><Icon name={copied ? 'check' : 'copy'} size={15} /> {t(copied ? 'help.rep.copiedBtn' : 'help.rep.copy')}</button>
    <button class="btn" disabled={phase === 'loading'} onclick={save}><Icon name="download" size={15} /> {t('help.rep.save')}</button>
  </div>
  <p class="muted sm">{t('help.rep.nothing')}</p>
</section>

<style>
  .lead { font-size: 15px; margin: 0 0 6px; }
  section { margin-bottom: 22px; }
  textarea { width: 100%; box-sizing: border-box; margin: 6px 0 10px; font: inherit; }
  .preview { font-family: var(--mono); font-size: var(--fs-xs); white-space: pre; }
  .wrap { flex-wrap: wrap; }
  .sm { font-size: var(--fs-xs); }
</style>
