<script lang="ts">
  // In a chat whose library was taken off the list (FEAT-32): it cannot continue, and no Wrap saves
  // it, until that library is added back. Shown only once the list is known.
  import Icon from './Icon.svelte';
  import { t } from './i18n';
  import { libs, addBack } from './libraries.svelte';

  let { library }: { library?: string | null } = $props();
  const norm = (p: string) => p.replace(/[\\/]+$/, '');
  const gone = $derived(!!library && libs.loaded && libs.list.length > 0 && !libs.list.some((l) => norm(l.path) === norm(String(library))));
  let busy = $state(false);
  async function again() {
    if (busy || !library) return;
    busy = true;
    try { await addBack(String(library)); } finally { busy = false; }
  }
</script>

{#if gone}
  <div class="gone row" role="status">
    <Icon name="warning" size={15} />
    <span class="grow">{t('libs.unlisted')} <bdi dir="ltr" class="path">{library}</bdi></span>
    <button type="button" class="btn" disabled={busy} aria-busy={busy} onclick={again}>{t('libs.addBack')}</button>
  </div>
{/if}

<style>
  .gone { margin-top: 8px; gap: 8px; align-items: center; font-size: var(--fs-sm); padding: 6px 10px; border-radius: var(--radius);
    background: var(--panel-2); border: var(--border-w, 1px) solid var(--danger); }
  .gone > :global(.icon) { color: var(--danger); flex: none; }
  .grow { flex: 1; min-width: 0; }
  .path { color: var(--muted); overflow-wrap: anywhere; }
  @media print { .gone .btn { display: none; } }
</style>
