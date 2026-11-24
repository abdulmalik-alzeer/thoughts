<script lang="ts">
  // The Plan view's header: which model and how much effort this plan's runs use (the same picker
  // and the same lists as a chat's: they come from the installed assistant program, never from a
  // list in the app), and the usage ring the chats have. No choice = the app's default.
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { t } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import ChatOptions from '../chat/ChatOptions.svelte';
  import { entriesFrom, type ModelEntry } from '../../lib/models/picker';
  import { autoNote } from '../../lib/models/auto';
  import { app } from '../../lib/store.svelte';
  import UsageRing from '../usage/UsageRing.svelte';
  import type { MadoApi } from '../../lib/mado/api';
  import type { MadoOptions } from '../../lib/mado/types';

  let { madoApi: client, running = false }: { madoApi: MadoApi; /** A run of this plan is at work (the ring loads again when it ends). */ running?: boolean } = $props();

  let lists = $state<{ models: ModelEntry[]; efforts: string[] }>({ models: [], efforts: [] });
  /** null until the service answered: an older service has no such setting, and then nothing is shown. */
  let options = $state<MadoOptions | null>(null);
  let saving = $state(false);
  const strings = (v: unknown) => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string' && !!x) : []);
  const clean = (o: any): MadoOptions => ({ model: typeof o?.model === 'string' && o.model ? o.model : null, effort: typeof o?.effort === 'string' && o.effort ? o.effort : null });

  onMount(() => {
    api('/ai/claude/options').then((o) => { lists = { models: entriesFrom(o), efforts: strings(o?.efforts) }; }).catch(() => {});
    client.options().then((o) => { options = clean(o); }).catch(() => {});
  });

  /** Translated when the app has a name for it, the program's own word otherwise. */
  const label = (prefix: string, v: string) => { const k = `${prefix}.${v}`; const s = t(k); return s === k ? v : s; };

  async function save(change: Partial<MadoOptions>): Promise<boolean> {
    if (!options || saving) return false;
    const before = options;
    options = { ...options, ...change };
    saving = true;
    try { options = clean(await client.saveOptions(options) ?? options); return true; }
    catch (e) { options = before; failed(t('mado.opts.saveFailed'), e); return false; }
    finally { saving = false; }
  }
</script>

<div class="row po">
  {#if options}
    <ChatOptions plain below busy={saving} {options} models={lists.models} efforts={lists.efforts} modes={[]} {label} scope="claude" autoNote={autoNote(lists.models, app.settings?.ai?.claude?.model)} onmodel={(v) => save({ model: v })} oneffort={(v) => { save({ effort: v }); }} />
  {/if}
  <UsageRing chatId="" provider="claude" {running} below plansOnly />
</div>

<style>
  .po { gap: 6px; flex: none; min-width: 0; }
</style>
