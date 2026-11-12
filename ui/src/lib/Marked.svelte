<script lang="ts">
  // Text with the parts a search matched highlighted.
  import { segments } from './search';
  let { text, marks }: { text: string; marks?: [number, number][] } = $props();
  const parts = $derived(segments(text ?? '', marks));
</script>

{#each parts as p}{#if p.m}<mark class="hl">{p.t}</mark>{:else}{p.t}{/if}{/each}

<style>
  .hl { background: var(--hl-bg, color-mix(in srgb, var(--accent) 22%, transparent)); color: inherit; border-radius: 3px; padding: 0 1px; font-weight: 600; }
  :global([data-theme="contrast"]) .hl { background: var(--accent); color: var(--accent-fg); }
</style>
