<script lang="ts" module>
  import { Terminal, FileText, FilePen, Search, Globe, Bot, ListTodo, Wrench } from '@lucide/svelte';
  import type { StepKind } from './steps';
  const KIND_ICON: Record<StepKind, any> = { command: Terminal, read: FileText, edit: FilePen, search: Search, web: Globe, agent: Bot, plan: ListTodo, other: Wrench };
  let uid = 0;
</script>

<script lang="ts">
  // A run of the assistant's steps folded into one quiet row ("Ran 3 commands, edited 2 files ›").
  // Opened, it lists the steps; each step opens on what it was given and what came back.
  import Icon from '../../lib/Icon.svelte';
  import { t, tn } from '../../lib/i18n';
  import { failures, summarize, type Step } from './steps';

  let { steps, running = false }: { steps: Step[]; running?: boolean } = $props();

  const id = `r10steps-${++uid}`;
  let open = $state(false);
  let openStep = $state<Record<string, boolean>>({});

  const summary = $derived(summarize(steps));
  const failed = $derived(failures(steps));
  const kinds = $derived([...new Set(steps.map((s) => s.kind))]);
  const HeadIcon = $derived(KIND_ICON[kinds.length === 1 ? kinds[0] : 'other']);
  const keyOf = (s: Step) => String(s.call.tool?.id ?? s.call.id);

  /** What the step was given: a command as it was typed, anything else as its fields. */
  function input(s: Step): string {
    const i = s.call.tool?.input;
    if (i && typeof i.command === 'string') return i.command;
    if (i == null) return '';
    return JSON.stringify(i, null, 2);
  }
</script>

<div class="steps">
  <button type="button" class="head" aria-expanded={open} aria-controls={id} onclick={() => (open = !open)}
    title={t(open ? 'r10-chatview.hide' : 'r10-chatview.show')}>
    <HeadIcon size={13} strokeWidth={1.75} class="icon" aria-hidden="true" />
    <span class="ell" dir="auto">{summary}</span>
    {#if failed}<span class="pill bad">{tn('r10-chatview.failed', failed)}</span>{/if}
    <Icon name={open ? 'down' : 'right'} size={13} class={open ? '' : 'flip-rtl'} />
  </button>
  {#if open}
    <ul class="list" {id} aria-label={t('r10-chatview.list')}>
      {#each steps as s (keyOf(s))}
        {@const k = keyOf(s)}
        {@const KindIcon = KIND_ICON[s.kind]}
        <li>
          <button type="button" class="step" aria-expanded={!!openStep[k]} aria-controls={`${id}-${k}`} onclick={() => (openStep[k] = !openStep[k])}
            title={t(openStep[k] ? 'chat.hideDetails' : 'chat.showDetails')}>
            <KindIcon size={12} strokeWidth={1.75} class="icon" aria-hidden="true" />
            <span class="ell" dir="auto">{s.call.text}</span>
            {#if s.result?.tool?.isError}<span class="pill bad">{t('chat.toolFailed')}</span>{/if}
            <Icon name={openStep[k] ? 'down' : 'right'} size={12} class={openStep[k] ? '' : 'flip-rtl'} />
          </button>
          {#if openStep[k]}
            <div class="detail" id={`${id}-${k}`}>
              <div class="eyebrow">{t('chat.toolInput')}</div>
              <pre class="sm" dir="ltr">{input(s) || t('chat.toolEmpty')}</pre>
              <div class="eyebrow">{t('chat.toolResult')}</div>
              {#if s.result}<pre class="sm" dir="auto">{s.result.text || t('chat.toolEmpty')}</pre>
              {:else}<p class="muted sm">{t(running ? 'chat.toolPending' : 'chat.toolNoResult')}</p>{/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .steps { min-width: 0; }
  .head, .step { font: inherit; background: none; border: 0; border-radius: 6px; text-align: start; cursor: pointer; color: var(--muted); display: flex; align-items: center; gap: 6px; max-width: 100%; }
  .head { font-size: var(--fs-sm); padding: 2px 4px; margin-inline-start: -4px; }
  .step { font-family: var(--mono); font-size: var(--fs-xs); padding: 2px 4px; }
  .head:hover, .step:hover { color: var(--accent); }
  .head :global(.icon), .step :global(.icon) { flex: none; }
  .list { list-style: none; margin: 4px 0 2px; padding: 0; padding-inline-start: 10px; margin-inline-start: 2px; border-inline-start: var(--border-w, 1px) solid var(--hairline); display: grid; gap: 2px; min-width: 0; }
  .list li { min-width: 0; }
  .detail { margin: 4px 0 6px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); display: grid; gap: 4px; min-width: 0; }
  .detail pre { margin: 0 0 4px; max-height: 260px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
  .detail p { margin: 0; }
  .pill.bad { color: var(--danger); }
  @media print { .detail pre { max-height: none; overflow: visible; } }
</style>
