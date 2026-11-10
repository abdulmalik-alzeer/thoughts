<script lang="ts">
  import { onMount } from 'svelte';
  import { api, post } from '../lib/api';
  import { go, toast } from '../lib/store.svelte';
  let { id }: { id?: string } = $props();
  let d = $state<any>(null);
  let busy = $state(false);
  onMount(async () => { try { d = await api(`/reorganize/${id}`); } catch (e: any) { toast(e.message); } });

  async function finish(op: 'merge' | 'discard') {
    if (op === 'merge' && !confirm('Keep the reorganized library? It is merged into your main branch (still undoable with git).')) return;
    busy = true;
    try { await post(`/reorganize/${id}/${op}`); toast(op === 'merge' ? 'Reorganization merged' : 'Reorganization discarded'); go({ kind: 'docs' }); }
    catch (e: any) { toast(e.message); busy = false; }
  }
  const lines = $derived((d?.diff || '').split('\n'));
</script>

<div class="page">
  <h1>Review reorganization</h1>
  {#if d}
    <div class="row"><span class="pill">{d.phase}</span><span class="grow"></span>
      <button class="btn" disabled={busy} onclick={() => finish('discard')}>Discard</button>
      <button class="btn primary" disabled={busy} onclick={() => finish('merge')}>Keep it</button></div>
    <pre>{d.stat}</pre>
    <pre class="diff">{#each lines as l}<span class:add={l.startsWith('+')} class:rem={l.startsWith('-')} class:hd={l.startsWith('@@') || l.startsWith('diff')}>{l}
</span>{/each}</pre>
  {/if}
</div>

<style>
  .diff { font-size: 12px; max-height: 70vh; }
  .add { color: var(--ok); } .rem { color: var(--danger); } .hd { color: var(--accent-2); }
</style>
