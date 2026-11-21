<script lang="ts">
  // The box a name is typed in, on a node or an edge. Enter keeps it, Escape leaves it as it was.
  import { onMount } from 'svelte';
  let { value, max, label, oncommit, oncancel }: { value: string; max: number; label: string; oncommit: (text: string) => void; oncancel: () => void } = $props();
  let el: HTMLInputElement;
  let done = false;
  // svelte-ignore state_referenced_locally
  let text = $state(value);
  onMount(() => { el.focus(); el.select(); });
  function finish(keep: boolean) { if (done) return; done = true; if (keep) oncommit(text); else oncancel(); }
</script>

<input bind:this={el} bind:value={text} class="name nodrag nopan nowheel" dir="auto" maxlength={max} aria-label={label} spellcheck="false" autocomplete="off"
  onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); finish(true); } else if (e.key === 'Escape') { e.preventDefault(); finish(false); } }}
  onblur={() => finish(true)} onpointerdown={(e) => e.stopPropagation()} ondblclick={(e) => e.stopPropagation()} />

<style>
  .name {
    font: inherit; font-weight: inherit; color: var(--fg); background: var(--bg); border: var(--border-w) solid var(--accent); border-radius: 6px;
    padding: 2px 6px; width: 100%; min-width: 90px; box-sizing: border-box; text-align: inherit; outline: none;
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 30%, transparent);
  }
</style>
