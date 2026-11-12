<script lang="ts">
  // What a chat allows without asking ("Always for this chat" on a card), each with "Ask again".
  // Nothing is shown while the chat allows nothing.
  import Icon from '../lib/Icon.svelte';
  import { api, del, on } from '../lib/api';
  import { t, tn } from '../lib/i18n';
  import { failed } from '../lib/errors';

  let { chatId }: { chatId: string } = $props();

  let list = $state<any[]>([]);
  let open = $state(false);

  async function load(id: string) {
    try { const r = await api(`/chats/${id}/approvals`); if (id === chatId) list = Array.isArray(r?.always) ? r.always : []; }
    catch { /* an older service, or the chat is gone: nothing to show */ if (id === chatId) list = []; }
  }

  $effect(() => {
    const id = chatId;
    list = []; open = false;
    load(id);
    return on((e) => { if (e.type === 'approval.changed' && e.chatId === id) load(id); else if (e.type === 'resync') load(id); });
  });

  async function remove(a: any) {
    try { const r = await del(`/chats/${chatId}/always/${a.id}`); list = Array.isArray(r?.always) ? r.always : list.filter((x) => x.id !== a.id); }
    catch (e) { failed(t('appr.allowed.failed'), e); }
  }

  const KINDS = ['command', 'change', 'open', 'look', 'connected', 'tool'];
  const ISOLATE = [String.fromCharCode(0x2068), String.fromCharCode(0x2069)];
  const words = (a: any) => t(`appr.allowed.${KINDS.includes(a.what) ? a.what : 'tool'}`, { target: ISOLATE[0] + String(a.target || a.tool || '') + ISOLATE[1] });
</script>

{#if list.length}
  <div class="allowed">
    <button class="sum" onclick={() => (open = !open)} aria-expanded={open}>
      <Icon name="shield" size={13} /> <span>{tn('appr.allowed.summary', list.length)}</span>
      <Icon name={open ? 'down' : 'right'} size={12} class={open ? '' : 'flip-rtl'} />
    </button>
    {#if open}
      <ul>
        {#each list as a (a.id)}
          <li><span class="grow" dir="auto">{words(a)}</span>
            <button class="btn ghost" onclick={() => remove(a)} title={t('appr.allowed.removeTip')}>{t('appr.allowed.remove')}</button></li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  .allowed { margin-top: 8px; font-size: var(--fs-sm); }
  .sum { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 6px; padding: 2px 4px; margin-inline-start: -4px; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; }
  .sum:hover { color: var(--fg); background: var(--fill); }
  ul { list-style: none; margin: 4px 0 0; padding: 6px 10px; border-radius: var(--radius); background: var(--panel-2); display: grid; gap: 4px; }
  li { display: flex; align-items: center; gap: 8px; overflow-wrap: anywhere; min-width: 0; }
  li .btn { font-size: var(--fs-xs); padding: 2px 8px; flex: none; }
  @media print { .allowed { display: none; } }
</style>
