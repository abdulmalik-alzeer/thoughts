<script lang="ts">
  // JSON as a tree whose branches open and close (native disclosure widgets, so keyboard and screen
  // readers work), or as pretty-printed, highlighted text. A file that isn't valid JSON is shown as
  // its text with a note. Parsing JSON runs nothing.
  import { untrack } from 'svelte';
  import Icon from '../../Icon.svelte';
  import { t } from '../../i18n';
  import { decodeText } from '../text';
  import Code from './Code.svelte';
  import JsonNode from './JsonNode.svelte';
  import type { RendererProps } from '../renderers';

  let props: RendererProps = $props();

  /** Tree view up to this size; above it the text view only (a tree of a huge file is slow). */
  const TREE_MAX = 2 * 1024 * 1024;

  let phase = $state<'loading' | 'tree' | 'text'>('loading');
  let value = $state<unknown>(undefined);
  let pretty = $state<string | undefined>(undefined);
  let invalid = $state(false);
  let canTree = $state(false);
  let seq = 0;

  async function parse(b: Blob) {
    const n = ++seq;
    phase = 'loading'; invalid = false; pretty = undefined; canTree = false;
    const s = decodeText(new Uint8Array(await b.arrayBuffer()));
    if (n !== seq) return;
    if (s === null) { props.unsupported(t('libf-preview-core.notText')); return; }
    try {
      value = JSON.parse(s);
      canTree = b.size <= TREE_MAX && value !== null && typeof value === 'object';
      pretty = JSON.stringify(value, null, 2);
      phase = canTree ? 'tree' : 'text';
    } catch {
      invalid = true;
      phase = 'text';
    }
  }
  $effect(() => { const b = props.data; untrack(() => parse(b)); });
</script>

{#snippet viewToggle()}
  {#if canTree}
    <button type="button" class="tb" aria-pressed={phase === 'tree'} onclick={() => (phase = 'tree')}>{t('libf-preview-core.json.tree')}</button>
    <button type="button" class="tb" aria-pressed={phase === 'text'} onclick={() => (phase = 'text')}>{t('libf-preview-core.json.text')}</button>
  {/if}
{/snippet}

{#if phase === 'loading'}
  <div class="pad" role="status"><span class="sr-only">{t('libf-preview-core.loading', { name: props.name })}</span><div class="skel"></div><div class="skel w70"></div></div>
{:else if phase === 'tree'}
  <div class="json" class:fill={props.fill}>
    <div class="bar" role="toolbar" aria-label={t('libf-preview-core.textTools')}><span class="grow"></span>{@render viewToggle()}</div>
    <div class="tree" dir="ltr" role="group" aria-label={t('libf-preview-core.textOf', { name: props.name })}><JsonNode k={null} v={value} depth={0} /></div>
  </div>
{:else}
  {#if invalid}<p class="muted sm note" role="note"><Icon name="info" size={13} /> {t('libf-preview-core.json.invalid')}</p>{/if}
  <Code {...props} text={invalid ? undefined : pretty} tools={viewToggle} />
{/if}

<style>
  .json { border: var(--border-w) solid var(--border); border-radius: var(--radius); background: var(--code-bg); overflow: hidden; display: flex; flex-direction: column; }
  .json.fill { height: 100%; border: 0; border-radius: 0; }
  .bar { display: flex; align-items: center; gap: 6px; padding: 6px 8px; border-bottom: var(--border-w) solid var(--border); background: var(--panel); }
  .grow { flex: 1; }
  .tb { font: inherit; font-size: var(--fs-xs); color: var(--fg); background: var(--fill); border: 0; border-radius: 7px; padding: 4px 9px; cursor: pointer; }
  .tb:hover { background: var(--fill-2); }
  .tb[aria-pressed="true"] { background: var(--tint); color: var(--on-tint); }
  .tb:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .tree { padding: 10px 14px; font-family: var(--mono); font-size: 12.5px; line-height: 1.6; overflow: auto; max-height: 75vh; text-align: left; }
  .fill .tree { max-height: none; flex: 1; min-height: 0; }
  .note { display: flex; align-items: center; gap: 6px; margin: 0 0 8px; }
  .sm { font-size: var(--fs-xs); }
  .pad { display: grid; gap: 10px; }
  .skel { height: 13px; margin: 5px 6px; border-radius: 8px; background: var(--fill); }
  .w70 { width: 70%; }
</style>
