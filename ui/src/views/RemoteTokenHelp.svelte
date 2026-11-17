<script lang="ts">
  // "How do I get one?" under the access-token field of an online backup location (first-run setup
  // and Settings › Library & backup): a private GitHub repository and a fine-grained token that can
  // only read and write that one repository. The link opens in the browser the app's usual way
  // (App.svelte sends external links there).
  import Icon from '../lib/Icon.svelte';
  import { t } from '../lib/i18n';

  const TOKEN_PAGE = 'https://github.com/settings/personal-access-tokens/new';
</script>

<details class="tokhelp">
  <summary>{t('ux-onboarding.tok.how')}</summary>
  <p class="sm">{t('ux-onboarding.tok.intro')}</p>
  <ol class="sm">
    <li>{t('ux-onboarding.tok.s1')}</li>
    <li>{t('ux-onboarding.tok.s2')} <a href={TOKEN_PAGE} target="_blank" rel="noreferrer">{t('ux-onboarding.tok.open')} <Icon name="external" size={12} /></a></li>
    <li>{t('ux-onboarding.tok.s3')}</li>
    <li>{t('ux-onboarding.tok.s4')}</li>
    <li>{t('ux-onboarding.tok.s5')}</li>
  </ol>
  <p class="sm muted">{t('ux-onboarding.tok.expiry')}</p>
  <p class="sm muted">{t('ux-onboarding.tok.other')}</p>
</details>

<style>
  .tokhelp { margin: 4px 0 8px; font-size: var(--fs-xs); }
  .tokhelp > summary { cursor: pointer; color: var(--accent); font-weight: 500; padding: 2px 0; width: fit-content; }
  .tokhelp > summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }
  .tokhelp p { margin: 6px 0 0; }
  .tokhelp ol { margin: 6px 0 0; padding-inline-start: 20px; display: grid; gap: 4px; }
  .tokhelp a { text-decoration: none; }
  .sm { font-size: var(--fs-xs); }
</style>
