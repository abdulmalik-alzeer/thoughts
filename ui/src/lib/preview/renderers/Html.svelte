<script lang="ts">
  // An HTML file: its source by default; "Show as page" renders it, cleaned and locked down
  // (../htmlpage.ts), in a frame that may run nothing and load nothing.
  import { untrack } from 'svelte';
  import { t } from '../../i18n';
  import { decodeText } from '../text';
  import { safePage } from '../htmlpage';
  import Code from './Code.svelte';
  import type { RendererProps } from '../renderers';

  let props: RendererProps = $props();

  let asPage = $state(false);
  let page = $state('');
  let seq = 0;

  async function build(b: Blob) {
    const n = ++seq;
    page = '';
    const s = decodeText(new Uint8Array(await b.arrayBuffer()));
    if (n !== seq) return;
    if (s === null) { props.unsupported(t('libf-preview-core.notText')); return; }
    page = safePage(s);
  }
  // Built only when asked for, and again for a new file.
  $effect(() => { const b = props.data; if (asPage) untrack(() => build(b)); });
  $effect(() => { void props.data; untrack(() => { asPage = false; }); });
</script>

{#snippet toggle()}
  <button type="button" class="tb" aria-pressed={asPage} onclick={() => (asPage = !asPage)}>{t('libf-preview-core.html.page')}</button>
{/snippet}

{#if asPage}
  <div class="frame" class:fill={props.fill}>
    <div class="bar" role="toolbar" aria-label={t('libf-preview-core.textTools')}>
      <span class="muted sm grow">{t('libf-preview-core.html.safe')}</span>
      {@render toggle()}
    </div>
    {#if page}
      <iframe sandbox="" srcdoc={page} title={t('libf-preview-core.html.title', { name: props.name })} referrerpolicy="no-referrer"></iframe>
    {:else}
      <div class="wait" role="status"><span class="sr-only">{t('libf-preview-core.loading', { name: props.name })}</span></div>
    {/if}
  </div>
{:else}
  <Code {...props} tools={toggle} />
{/if}

<style>
  .frame { display: flex; flex-direction: column; border: var(--border-w) solid var(--border); border-radius: var(--radius); overflow: hidden; background: var(--panel); }
  .frame.fill { height: 100%; border: 0; border-radius: 0; }
  .bar { display: flex; align-items: center; gap: 6px; padding: 6px 8px 6px 12px; border-bottom: var(--border-w) solid var(--border); }
  .grow { flex: 1; min-width: 0; }
  .sm { font-size: var(--fs-xs); }
  .tb { font: inherit; font-size: var(--fs-xs); color: var(--fg); background: var(--fill); border: 0; border-radius: 7px; padding: 4px 9px; cursor: pointer; }
  .tb:hover { background: var(--fill-2); }
  .tb[aria-pressed="true"] { background: var(--tint); color: var(--on-tint); }
  .tb:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  /* A web page is made for a white page, whatever the app's theme. */
  iframe { display: block; width: 100%; height: 70vh; border: 0; background: #fff; }
  .fill iframe { height: auto; flex: 1; min-height: 0; }
  .wait { height: 70vh; }
</style>
