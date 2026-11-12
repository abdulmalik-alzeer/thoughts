<script lang="ts">
  // "Add a library" (FEAT-32): the setup screens for the library part only, over the whole window.
  // The app's own settings, the assistant and its sign-in are not asked again.
  import { libs, addedLibrary } from './libraries.svelte';
  import { t } from './i18n';
  import Onboarding from '../views/Onboarding.svelte';

  // While the screens are up, the window behind them is out of reach (keyboard and screen readers).
  $effect(() => {
    if (!libs.adding) return;
    const shell = document.querySelector<HTMLElement>('.shell');
    shell?.setAttribute('inert', '');
    return () => shell?.removeAttribute('inert');
  });
</script>

{#if libs.adding}
  <div class="adding" role="dialog" aria-modal="true" aria-label={t('libs.addTitle')}>
    <Onboarding adding cancel={() => (libs.adding = false)} done={(id, name) => addedLibrary(id, name ?? '')} />
  </div>
{/if}

<style>
  .adding { position: fixed; inset: 0; z-index: 900; background: var(--bg); overflow-y: auto; }
</style>
