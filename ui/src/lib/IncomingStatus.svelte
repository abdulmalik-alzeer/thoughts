<script lang="ts">
  // Under "Check for changes from your other computers" in Settings: when the location was last
  // looked at and what was found, or, quietly, that it could not be looked at.
  import { onMount } from 'svelte';
  import { ago, t, tn } from './i18n';
  import { incoming, watchIncoming, refreshIncoming } from './incoming.svelte';

  // "5 minutes ago" moves on while the page stays open.
  let now = $state(Date.now());
  onMount(() => {
    watchIncoming();
    refreshIncoming();
    const tick = setInterval(() => { now = Date.now(); }, 60_000);
    return () => clearInterval(tick);
  });
  const line = $derived.by(() => {
    void now;
    if (incoming.state === 'failed') return t(incoming.why === 'sign_in' ? 'libf-sync.set.failedSignIn' : incoming.why === 'offline' ? 'libf-sync.set.failedOffline' : 'libf-sync.set.failed');
    if (incoming.state !== 'ok' || !incoming.checkedAt) return t('libf-sync.set.notYet');
    const when = ago(incoming.checkedAt);
    return incoming.waiting > 0 ? tn('libf-sync.set.checkedWaiting', incoming.waiting, { when }) : t('libf-sync.set.checked', { when });
  });
</script>

{#if incoming.on}<p class="muted sm" role="status">{line}</p>{/if}
