<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { api } from '../lib/api';
  import { app, command, toast } from '../lib/store.svelte';
  import { t, fmtNum } from '../lib/i18n';
  import { shortcuts, keys } from '../lib/shortcuts';

  const terms = ['library', 'chat', 'wrap', 'sync', 'rollback'];
  const rows = $derived(shortcuts.filter((s) => !s.hidden).map((s) => ({
    keys: s.id === 'nav.1' ? `${keys(s)} … ${keys(shortcuts.find((x) => x.id === 'nav.6')!)}` : keys(s),
    label: t(s.label),
  })));
  const monthCost = $derived(app.status?.usage?.monthCostUsd);

  let copying = $state(false);
  async function copyDiagnostics() {
    copying = true;
    const basics = [
      `Thoughts ${app.status?.version ?? '?'} (${app.status?.platform ?? '?'})`,
      `Language: ${app.settings?.language ?? '?'} · Theme: ${app.settings?.theme ?? '?'}/${app.settings?.mode ?? '?'} · Safe mode: ${app.safeMode ? 'on' : 'off'}`,
      `Assistant installed: ${app.status?.claude?.installed ?? '?'} · signed in: ${app.status?.claude?.loggedIn ?? '?'} · git: ${app.status?.git?.installed ?? '?'} ${app.status?.git?.version ?? ''}`,
      navigator.userAgent,
    ].join('\n');
    let text = basics, full = true;
    try {
      const d = await api('/diagnostics');
      text = `Thoughts ${d.version} (${d.platform})\n${basics.split('\n').slice(1).join('\n')}\n\n--- log ---\n${d.log ?? ''}`;
    } catch { full = false; /* older background service without diagnostics: copy the basics */ }
    try {
      await navigator.clipboard.writeText(text);
      toast(full ? t('shell.help.diagCopied') : t('shell.help.diagBasic'), { kind: 'success' });
    } catch {
      // Clipboard blocked: show it so it can be selected and copied by hand.
      toast(t('shell.help.diagManual'), { kind: 'error', detail: text, duration: 0 });
    } finally { copying = false; }
  }
</script>

<div class="page help">
  <h1 class="row"><Icon name="help" size={24} /> {t('shell.help')}</h1>
  <p class="muted lead">{t('shell.help.lead')}</p>

  <section aria-labelledby="h-how">
    <h2 id="h-how">{t('shell.help.how')}</h2>
    <p>{t('shell.help.howBody')}</p>
    <dl>
      {#each terms as k}
        <dt>{t(`shell.help.term.${k}`)}</dt><dd>{t(`shell.help.def.${k}`)}</dd>
      {/each}
    </dl>
    <button class="btn" onclick={() => command('tour')}><Icon name="book" size={15} /> {t('shell.pal.tour')}</button>
  </section>

  <section aria-labelledby="h-cust">
    <h2 id="h-cust" class="row"><Icon name="palette" size={18} /> {t('layer.customise')}</h2>
    <p>{t('layer.help.body')}</p>
    <dl>
      <dt>{t('layer.help.reviewT')}</dt><dd>{t('layer.help.review')}</dd>
      <dt>{t('layer.history')}</dt><dd>{t('layer.help.history')}</dd>
      <dt>{t('layer.share')}</dt><dd>{t('layer.help.share')}</dd>
    </dl>
    <div class="row wrap">
      <button class="btn primary" onclick={() => command('customise')}><Icon name="palette" size={15} /> {t('layer.customise')}</button>
      <button class="btn" onclick={() => command('history')}><Icon name="rollback" size={15} /> {t('layer.history')}</button>
    </div>
  </section>

  <section aria-labelledby="h-keys">
    <h2 id="h-keys" class="row"><Icon name="keyboard" size={18} /> {t('shell.help.keys')}</h2>
    <table>
      <thead><tr><th scope="col">{t('shell.help.keysCol')}</th><th scope="col">{t('shell.help.doesCol')}</th></tr></thead>
      <tbody>
        {#each rows as r}<tr><td><kbd dir="ltr">{r.keys}</kbd></td><td>{r.label}</td></tr>{/each}
      </tbody>
    </table>
  </section>

  <section aria-labelledby="h-priv">
    <h2 id="h-priv" class="row"><Icon name="lock" size={18} /> {t('shell.help.privacy')}</h2>
    <dl>
      <dt>{t('shell.help.sentT')}</dt><dd>{t('shell.help.sent')}</dd>
      <dt>{t('shell.help.localT')}</dt><dd>{t('shell.help.local')}</dd>
      <dt>{t('shell.help.privateT')}</dt><dd>{t('shell.help.private')}</dd>
      <dt>{t('shell.help.noTrackT')}</dt><dd>{t('shell.help.noTrack')}</dd>
    </dl>
    <p class="muted">{t('shell.help.privacyMore')} <bdi><code>docs/PRIVACY.md</code></bdi></p>
    <button class="btn" onclick={() => command('settings')}><Icon name="settings" size={15} /> {t('shell.help.openSettings')}</button>
  </section>

  <section aria-labelledby="h-cost">
    <h2 id="h-cost">{t('shell.help.cost')}</h2>
    <dl>
      <dt>{t('shell.help.subT')}</dt><dd>{t('shell.help.sub')}</dd>
      <dt>{t('shell.help.apiT')}</dt><dd>{t('shell.help.api')}</dd>
    </dl>
    {#if typeof monthCost === 'number'}
      <p><bdi>{t('shell.help.costMonth', { n: fmtNum(Math.round(monthCost * 100) / 100) })}</bdi></p>
    {/if}
  </section>

  <section aria-labelledby="h-fix">
    <h2 id="h-fix">{t('shell.help.trouble')}</h2>
    <dl>
      {#each ['noAnswer', 'broken', 'sync', 'offline'] as k}
        <dt>{t(`shell.help.fix.${k}T`)}</dt><dd>{t(`shell.help.fix.${k}`)}</dd>
      {/each}
    </dl>
    <p class="muted">{t('shell.help.diagBody')}</p>
    <div class="row wrap">
      <button class="btn primary" disabled={copying} onclick={copyDiagnostics}><Icon name="copy" size={15} /> {t('shell.help.diag')}</button>
      <button class="btn" onclick={() => command('rollback')}><Icon name="rollback" size={15} /> {t('shell.rb.title')}</button>
    </div>
    <p class="muted sm">{t('shell.help.version', { v: app.status?.version ?? '—' })}</p>
  </section>
</div>

<style>
  .help { max-width: 760px; }
  .lead { font-size: 15px; margin: 0 0 6px; }
  section { margin-bottom: 26px; }
  dl { margin: 0 0 12px; display: grid; gap: 10px; }
  dt { font-weight: 600; }
  dd { margin: 0; color: var(--muted); }
  table { width: 100%; }
  th { text-align: start; font-size: 12px; font-weight: 600; color: var(--muted); }
  td, th { border: 0; border-bottom: var(--border-w) solid var(--hairline); padding: 7px 8px; }
  td:first-child { white-space: nowrap; width: 1%; }
  .wrap { flex-wrap: wrap; }
  .sm { font-size: 12px; }
  h2.row { gap: 8px; }
</style>
