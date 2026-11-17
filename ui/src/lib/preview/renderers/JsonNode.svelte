<script lang="ts">
  // One branch or value of the JSON tree (Json.svelte). A closed branch's children are not built
  // until it is opened, so a big file stays quick.
  import { t, tn, fmtNum } from '../../i18n';
  import { untrack } from 'svelte';
  import Self from './JsonNode.svelte';

  let { k, v, depth }: { k: string | null; v: unknown; depth: number } = $props();

  /** Children listed per branch before "… n more". */
  const PER_BRANCH = 300;
  // The first two levels start open.
  let open = $state(untrack(() => depth < 2));

  const isObj = (x: unknown): x is Record<string, unknown> | unknown[] => x !== null && typeof x === 'object';
  const entries = (x: Record<string, unknown> | unknown[]) => (Array.isArray(x) ? x.map((y, i) => [String(i), y] as const) : Object.entries(x));
  const count = (x: Record<string, unknown> | unknown[]) => (Array.isArray(x) ? x.length : Object.keys(x).length);
  const summary = (x: Record<string, unknown> | unknown[]) => Array.isArray(x)
    ? tn('libf-preview-core.json.items', x.length)
    : tn('libf-preview-core.json.keys', count(x));
  const leafClass = (x: unknown) => (typeof x === 'string' ? 't-str' : typeof x === 'number' ? 't-num' : 't-lit');
  const leaf = (x: unknown) => (typeof x === 'string' ? JSON.stringify(x) : String(x));
</script>

{#if isObj(v)}
  <details bind:open>
    <summary>{#if k !== null}<span class="t-key">{k}</span><span class="p">: </span>{/if}<span class="p">{Array.isArray(v) ? '[ ]' : '{ }'}</span> <span class="muted">{summary(v)}</span></summary>
    {#if open}
      <div class="kids">
        {#each entries(v).slice(0, PER_BRANCH) as [ck, cv] (ck)}
          <Self k={ck} v={cv} depth={depth + 1} />
        {/each}
        {#if count(v) > PER_BRANCH}<div class="muted more">{t('libf-preview-core.json.more', { n: fmtNum(count(v) - PER_BRANCH) })}</div>{/if}
      </div>
    {/if}
  </details>
{:else}
  <div class="leaf">{#if k !== null}<span class="t-key">{k}</span><span class="p">: </span>{/if}<span class={leafClass(v)}>{leaf(v)}</span></div>
{/if}

<style>
  summary { cursor: pointer; border-radius: 4px; }
  summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .kids { padding-left: 2ch; border-left: 1px solid var(--border); margin-left: .5ch; }
  .leaf { white-space: pre-wrap; overflow-wrap: anywhere; padding-left: 1.4ch; }
  .more { padding-left: 1.4ch; font-style: italic; }
  .p { color: var(--muted); }
  .t-key { color: color-mix(in srgb, var(--accent) 75%, var(--fg)); }
  .t-str { color: var(--ok); }
  .t-num, .t-lit { color: var(--accent-2); }
</style>
