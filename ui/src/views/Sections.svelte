<script lang="ts">
  // The sections of one of the app's own pages, drawn in the order, visibility and size the
  // customisation layer gives them (`pages.json` `<page>.sections`, see lib/pages.svelte.ts), with
  // add-on boxes between them or in place of one. Each block carries `data-slot="<page>.<id>"`, a
  // stable hook for `custom.css`. The DOM follows the order on screen, so the keyboard and screen
  // readers meet the sections in the order they are seen.
  import type { Snippet } from 'svelte';
  import type { HomeWidget } from '../lib/store.svelte';
  import PageBox from './PageBox.svelte';
  import { sectionPlan, type Size } from '../lib/pages.svelte';
  // `parts`: one snippet per section id (from the reference); `order`: the sections the page can
  // draw right now, in their usual order (default: the order of `parts`); `sizes`: its own defaults.
  // `box`: how this page draws an add-on box (default: PageBox).
  let { page, parts, order, sizes = {}, gap = '14px', box }: { page: string; parts: Record<string, Snippet>; order?: string[]; sizes?: Record<string, Size>; gap?: string; box?: Snippet<[HomeWidget]> } = $props();
  const plan = $derived(sectionPlan(page, (order ?? Object.keys(parts)).filter((k) => parts[k] || k.startsWith('widget:')), sizes));
  // Only when some block is not full width does the page lay its sections out in rows; otherwise it
  // flows exactly as it always has.
  const rows = $derived(plan.some((e) => e.size !== 'full'));
</script>

<div class="slots" class:rows style:--slot-gap={gap} data-slots={page}>
  {#each plan as e (e.key)}
    <div class="slot" data-slot={e.box && !e.instead ? `${page}.${e.key}` : `${page}.${e.id}`} data-size={e.size}>
      {#if e.box}{#if box}{@render box(e.box)}{:else}<PageBox w={e.box} />{/if}{:else}{@render parts[e.id]()}{/if}
    </div>
  {/each}
</div>

<style>
  .slot { min-width: 0; }
  .slot:empty { display: none; }
  .rows { display: flex; flex-wrap: wrap; align-items: flex-start; column-gap: var(--slot-gap); }
  .rows > .slot { flex: 0 0 100%; }
  .rows > .slot[data-size='wide'] { flex-basis: calc(66.666% - var(--slot-gap) / 3); }
  .rows > .slot[data-size='half'] { flex-basis: calc(50% - var(--slot-gap) / 2); }
  .rows > .slot[data-size='third'] { flex-basis: calc(33.333% - var(--slot-gap) * 2 / 3); }
  /* A narrow window: every section takes the whole width again. */
  @container (max-width: 640px) { .rows > .slot { flex-basis: 100% !important; } }
  @media (max-width: 640px) { .rows > .slot { flex-basis: 100% !important; } }
</style>
