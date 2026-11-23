<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { app, command, go, APP_VERSION } from '../lib/store.svelte';
  import { t, fmtMoney } from '../lib/i18n';
  import { keys } from '../lib/shortcuts';
  import { KEYS, effective, label, labelOf } from '../lib/keys/registry';
  import HelpGuide from './HelpGuide.svelte';
  import HelpReport from './HelpReport.svelte';
  import { guideNames, guideTitle, guideSource, changelogSource, hasGuide } from '../lib/guides';

  const terms = ['library', 'chat', 'wrap', 'sync', 'rollback'];
  // "Working with notes and chats": i18n stems in final-docs.ts (`final-docs.<k>T` and `final-docs.<k>`).
  const work = ['pic', 'preview', 'pdf', 'pane', 'edit', 'window', 'graph', 'need', 'sync'];
  // The window-wide keys as they are now (the user may have changed them in Settings › Keyboard).
  const rows = $derived(KEYS.filter((s) => !s.hidden && s.editable && (s.scope === 'app' || s.scope === 'native') && keys(s.id)).map((s) => ({
    keys: s.id === 'nav.1' && keys('nav.6') ? `${keys('nav.1')} … ${keys('nav.6')}` : effective(s.id).map((b) => label(b)).join('  ·  '),
    label: s.id === 'nav.1' ? t('shell.sc.nav') : labelOf(s),
  })));
  // The six tour cards, kept here for reference (icon + i18n stem).
  const around = [['home', 'home'], ['chats', 'chat'], ['wrap', 'wrap'], ['docs', 'files'], ['projects', 'folders'], ['palette', 'yours']];
  // Isolate an amount so it reads correctly inside Arabic text.
  const iso = (x: string) => String.fromCodePoint(0x2068) + x + String.fromCodePoint(0x2069);
  const usesFile = $derived(app.settings?.sync?.credentialStore === 'file');
  const monthCost = $derived(app.status?.usage?.monthCostUsd);

  // Help has sub-pages, kept in the view itself so Back works: id 'report', id 'whatsnew', or path 'GUIDE#section'.
  const sub = $derived(app.view.id === 'report' ? 'report' : app.view.id === 'whatsnew' ? 'new' : app.view.path ? 'guide' : '');
  const guidePath = $derived((app.view.path ?? '').split('#'));
  const guideName = $derived(guidePath[0]);
  const guideAnchor = $derived(guidePath.slice(1).join('#'));
  const guideSrc = $derived(sub === 'guide' ? guideSource(guideName) : undefined);
  const backToHelp = () => { go({ kind: 'help' }); };
  const openGuide = (name: string, anchor = '') => { go({ kind: 'help', path: anchor ? `${name}#${anchor}` : name }); };
  const notes = $derived(changelogSource());
</script>

<div class="page help">
{#if sub === 'report'}
  <HelpReport back={backToHelp} />
{:else if sub === 'new'}
  <HelpGuide src={notes || `# ${t('help.new.title')}\n\n${t('help.new.empty')}`} back={backToHelp} open={openGuide} />
{:else if sub === 'guide' && guideSrc !== undefined}
  <HelpGuide src={guideSrc} anchor={guideAnchor} back={backToHelp} open={openGuide} />
{:else}
  <h1 class="row"><Icon name="help" size={24} /> {t('shell.help')}</h1>
  <p class="muted lead">{t('shell.help.lead')}</p>

  <section aria-labelledby="h-how">
    <h2 id="h-how">{t('shell.help.how')}</h2>
    <p>{t('shell.help.howBody')}</p>
    <dl>
      {#each terms as k}
        <dt>{t(`shell.help.term.${k}`)}</dt><dd>{t(k === 'rollback' ? 'help.def.history' : `shell.help.def.${k}`)}</dd>
      {/each}
    </dl>
  </section>

  <section aria-labelledby="h-around">
    <h2 id="h-around" class="row"><Icon name="book" size={18} /> {t('ob3.help.around')}</h2>
    <dl>
      {#each around as [icon, k]}
        <dt class="row"><Icon name={icon} size={15} /> {t(`shell.tour.${k}Title`)}</dt><dd>{t(`shell.tour.${k}Body`, { palette: keys('palette') })}</dd>
      {/each}
    </dl>
    <button class="btn" onclick={() => command('tour')}><Icon name="book" size={15} /> {t('ob3.help.takeTour')}</button>
  </section>

  <section aria-labelledby="h-work">
    <h2 id="h-work" class="row"><Icon name="docs" size={18} /> {t('final-docs.title')}</h2>
    <p class="muted">{t('final-docs.lead')}</p>
    <dl>
      {#each work as k}
        <dt>{t(`final-docs.${k}T`)}</dt><dd>{t(`final-docs.${k}`)}</dd>
      {/each}
    </dl>
    {#if hasGuide('USING')}<button class="btn" onclick={() => openGuide('USING')}><Icon name="book" size={15} /> {t('final-docs.open')}</button>{/if}
  </section>

  <section aria-labelledby="h-hist">
    <h2 id="h-hist" class="row"><Icon name="rollback" size={18} /> {t('ob3.help.history')}</h2>
    <dl>
      <dt>{t('ob3.help.notesHistT')}</dt><dd>{t('ob3.help.notesHist')}</dd>
      <dt>{t('ob3.help.appHistT')}</dt><dd>{t('ob3.help.appHist')}</dd>
    </dl>
    <div class="row wrap">
      <button class="btn" onclick={() => command('history')}><Icon name="rollback" size={15} /> {t('ob3.help.openNotesHist')}</button>
      <button class="btn" onclick={() => command('rollback')}><Icon name="palette" size={15} /> {t('ob3.help.openAppHist')}</button>
    </div>
  </section>

  <section aria-labelledby="h-cust">
    <h2 id="h-cust" class="row"><Icon name="palette" size={18} /> {t('layer.customise')}</h2>
    <p>{t('layer.help.body')}</p>
    <dl>
      <dt>{t('layer.help.reviewT')}</dt><dd>{t('layer.help.review')}</dd>
      <dt>{t('layer.history')}</dt><dd>{t('layer.help.history')}</dd>
      <dt>{t('layer.share')}</dt><dd>{t('layer.help.share')}</dd>
      <dt>{t('final-docs.autoT')}</dt><dd>{t('final-docs.auto')}</dd>
    </dl>
    <div class="row wrap">
      <button class="btn primary" onclick={() => command('customise.new')} title={t('r10-cust-start.newTip')}><Icon name="palette" size={15} /> {t('r10-cust-start.new')}</button>
      <button class="btn" onclick={() => command('customise')}>{t('ldc.set.open')}</button>
      {#if hasGuide('CUSTOMISING')}<button class="btn" onclick={() => openGuide('CUSTOMISING', t('auto-ui.suggest'))}><Icon name="book" size={15} /> {t('final-docs.autoOpen')}</button>{/if}
      <button class="btn" onclick={() => command('rollback')}><Icon name="rollback" size={15} /> {t('ob3.help.openAppHist')}</button>
    </div>
  </section>

  <section aria-labelledby="h-guides">
    <h2 id="h-guides" class="row"><Icon name="book" size={18} /> {t('help.guides')}</h2>
    <p class="muted">{t('help.guidesBody')}</p>
    <div class="row wrap">
      {#each guideNames as g}<button class="btn" onclick={() => openGuide(g)}><Icon name="book" size={15} /> <span dir="auto">{guideTitle(g)}</span></button>{/each}
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
    <button class="btn" onclick={() => go({ kind: 'settings', id: 'keyboard' })}><Icon name="keyboard" size={15} /> {t('kb.help.change')}</button>
  </section>

  <section aria-labelledby="h-priv">
    <h2 id="h-priv" class="row"><Icon name="lock" size={18} /> {t('shell.help.privacy')}</h2>
    <dl>
      <dt>{t('shell.help.sentT')}</dt><dd>{t('shell.help.sent')}</dd>
      <dt>{t('shell.help.localT')}</dt><dd>{t('help.local')} {t(usesFile ? 'ob3.help.secretsFile' : 'ob3.help.secretsBoth')}</dd>
      <dt>{t('shell.help.privateT')}</dt><dd>{t('shell.help.private')}</dd>
      <dt>{t('shell.help.noTrackT')}</dt><dd>{t('shell.help.noTrack')}</dd>
    </dl>
    <p class="muted">{t('help.privacyMore')}</p>
    <button class="btn" onclick={() => command('settings')}><Icon name="settings" size={15} /> {t('shell.help.openSettings')}</button>
  </section>

  <section aria-labelledby="h-cost">
    <h2 id="h-cost">{t('shell.help.cost')}</h2>
    <dl>
      <dt>{t('shell.help.subT')}</dt><dd>{t('ob3.help.sub')}</dd>
      <dt>{t('shell.help.apiT')}</dt><dd>{t('ob3.help.api')}</dd>
    </dl>
    {#if typeof monthCost === 'number'}
      <p>{t('ob3.help.costMonth', { amount: iso(fmtMoney(monthCost)) })}</p>
    {/if}
  </section>

  <section aria-labelledby="h-fix">
    <h2 id="h-fix">{t('shell.help.trouble')}</h2>
    <dl>
      {#each ['noAnswer', 'limit', 'noNet', 'broken', 'sync', 'offline'] as k}
        {@const ns = k === 'limit' || k === 'noNet' ? 'help' : 'shell.help'}
        <dt>{t(`${ns}.fix.${k}T`)}</dt><dd>{t(`${ns}.fix.${k}`)}</dd>
      {/each}
    </dl>
    <p class="muted">{t('help.diagBody')}</p>
    <div class="row wrap">
      <button class="btn primary" onclick={() => go({ kind: 'help', id: 'report' })}><Icon name="warning" size={15} /> {t('help.rep.btn')}</button>
      <button class="btn" onclick={() => command('rollback')}><Icon name="rollback" size={15} /> {t('shell.rb.title')}</button>
    </div>
    <div class="row wrap"><button class="btn" onclick={() => go({ kind: 'help', id: 'whatsnew' })}><Icon name="info" size={15} /> {t('help.new.btn')}</button></div>
    <p class="muted sm">{t('shell.help.version', { v: APP_VERSION })}{#if app.status?.version && app.status.version !== APP_VERSION} — {t('ver.mismatch', { app: APP_VERSION, service: app.status.version })}{/if}</p>
    <p class="muted sm">{t('shell.help.creditFlow')}</p>
  </section>
{/if}
</div>

<style>
  .help { max-width: 760px; }
  .lead { font-size: 15px; margin: 0 0 6px; }
  section { margin-bottom: 26px; }
  dl { margin: 0 0 12px; display: grid; gap: 10px; }
  dt { font-weight: 600; }
  dd { margin: 0; color: var(--muted); }
  table { width: 100%; }
  th { text-align: start; font-size: var(--fs-xs); font-weight: 600; color: var(--muted); }
  td, th { border: 0; border-bottom: var(--border-w) solid var(--hairline); padding: 7px 8px; }
  td:first-child { white-space: nowrap; width: 1%; }
  .wrap { flex-wrap: wrap; }
  .sm { font-size: var(--fs-xs); }
  h2.row { gap: 8px; }
</style>
