<script lang="ts">
  // One suggestion on the Automate tab: what kind it is, what it would do, why it came up and what
  // building it involves, with Build it and Not for me. Everything the assistant wrote is plain text.
  import Icon from '../../lib/Icon.svelte';
  import { t } from '../../lib/i18n';
  import { kindIcon, kindKey, type Suggestion } from '../../lib/automate';

  let { s, busy = false, locked = false, onbuild, ondismiss }: {
    s: Suggestion; busy?: boolean; locked?: boolean; onbuild: () => void; ondismiss: () => void;
  } = $props();
  const head = $derived('au-sg-' + s.id);
</script>

<article class="card sg" aria-labelledby={head}>
  <div class="top">
    <span class="pill accent kind"><Icon name={kindIcon(s.kind)} size={13} /> {t(kindKey(s.kind))}</span>
  </div>
  <h3 id={head} dir="auto">{s.title}</h3>
  <dl>
    <dt>{t('auto-ui.card.what')}</dt>
    <dd dir="auto">{s.what}</dd>
    {#if s.why}
      <dt>{t('auto-ui.card.why')}</dt>
      <dd dir="auto">{s.why}</dd>
    {/if}
    {#if s.build}
      <dt>{t('auto-ui.card.build')}</dt>
      <dd dir="auto">{s.build}</dd>
    {/if}
  </dl>
  <div class="row acts">
    <button type="button" class="btn primary" disabled={busy || locked} aria-busy={busy} aria-describedby={head} onclick={onbuild}>
      <Icon name="check" size={15} /> {busy ? t('auto-ui.card.building') : t('auto-ui.card.buildIt')}
    </button>
    <button type="button" class="btn ghost" disabled={busy || locked} aria-describedby={head} onclick={ondismiss}>{t('auto-ui.card.notForMe')}</button>
  </div>
</article>

<style>
  .sg { display: grid; gap: 6px; margin-bottom: 12px; }
  .top { display: flex; }
  .kind { display: inline-flex; align-items: center; gap: 5px; }
  h3 { font-size: 15px; font-weight: 600; margin: 2px 0 0; }
  dl { margin: 0; display: grid; gap: 2px; }
  dt { font-size: var(--fs-xs); font-weight: 600; color: var(--muted); margin-top: 6px; }
  dd { margin: 0; font-size: 13.5px; line-height: 1.5; white-space: pre-line; overflow-wrap: anywhere; }
  .acts { gap: 8px; flex-wrap: wrap; margin-top: 8px; }
</style>
