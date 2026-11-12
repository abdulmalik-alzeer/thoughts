<script lang="ts">
  // One control in the composer for how this chat's assistant works: what it may do (Claude Code's
  // permission modes), and — less often needed — which model and how much effort. The button always
  // names the permission mode, so what the assistant may do is never hidden behind it. Every list
  // comes from the installed assistant program; nothing here is a fixed list of models or modes.
  import Icon from '../../lib/Icon.svelte';
  import { t } from '../../lib/i18n';

  let { options, models, efforts, modes, label, onmodel, extraModel, oneffort, onperm }: {
    options: { model?: string | null; effort?: string | null; permissionMode?: string | null };
    models: string[]; efforts: string[]; modes: string[];
    /** Translated label when there is one, the raw value otherwise. */
    label: (prefix: string, v: string) => string;
    /** The model menu changed (its value: '' = automatic). The handler may put the menu back. */
    onmodel: (e: Event & { currentTarget: HTMLSelectElement }) => void;
    /** One more entry at the end of the model menu (typing a model's name). */
    extraModel?: { value: string; label: string };
    oneffort: (v: string | null) => void; onperm: (v: string) => void;
  } = $props();

  const BYPASS = 'bypassPermissions';
  let open = $state(false);
  let root = $state<HTMLElement>();
  let button = $state<HTMLButtonElement>();
  const uid = Math.random().toString(36).slice(2, 8);

  const mode = $derived(options.permissionMode ?? 'acceptEdits');
  // What the button says: the mode always; model and effort only when they are not the default.
  const extra = $derived([options.model, options.effort ? label('chat.effort', options.effort) : null].filter(Boolean).join(' · '));
  const hint = (m: string) => { const k = `set.perm.${m}.hint`; const s = t(k); return s === k ? '' : s; };

  function close(refocus = true) { open = false; if (refocus) button?.focus(); }
  function onkey(e: KeyboardEvent) { if (e.key === 'Escape' && open) { e.stopPropagation(); e.preventDefault(); close(); } }
</script>

<svelte:window onclick={(e) => { if (open && root && !root.contains(e.target as Node) && !(e.target as HTMLElement)?.closest?.('[aria-modal="true"]')) open = false; }} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span class="co" bind:this={root} onkeydown={onkey}>
  <button bind:this={button} type="button" class="cobtn" class:bypass={mode === BYPASS} class:plan={mode === 'plan'}
    aria-haspopup="dialog" aria-expanded={open} aria-controls="co-{uid}" onclick={() => (open = !open)}
    title={t('chatc.opt.tip')} aria-label={t('chatc.opt.aria', { mode: label('chat3.perm', mode) })}>
    <Icon name={mode === BYPASS ? 'warning' : mode === 'plan' ? 'eye' : 'shield'} size={13} />
    <span class="ell">{label('chat3.perm', mode)}</span>
    {#if extra}<span class="ell muted extra" dir="ltr">· {extra}</span>{/if}
    <Icon name="down" size={12} />
  </button>
  {#if open}
    <div class="pop" id="co-{uid}" role="dialog" aria-label={t('chatc.opt.title')}>
      {#if modes.length}
        <fieldset>
          <legend>{t('chatc.opt.may')}</legend>
          {#each modes as m (m)}
            <label class="radio" class:danger={m === BYPASS}>
              <input type="radio" name="co-perm-{uid}" value={m} checked={mode === m} onchange={() => onperm(m)} />
              <span><b>{label('chat3.perm', m)}</b>{#if hint(m)}<span class="muted sm blk">{hint(m)}</span>{/if}</span>
            </label>
          {/each}
        </fieldset>
      {/if}
      <div class="row2">
        <label class="field">
          <span class="lbl">{t('model')}</span>
          <select value={options.model ?? ''} onchange={onmodel}>
            <option value="">{t('chatc.opt.auto')}</option>
            {#each models as m (m)}<option value={m}>{m}</option>{/each}
            {#if options.model && !models.includes(options.model)}<option value={options.model}>{options.model}</option>{/if}
            {#if extraModel}<option value={extraModel.value}>{extraModel.label}</option>{/if}
          </select>
        </label>
        {#if efforts.length}
          <label class="field">
            <span class="lbl">{t('effort')}</span>
            <select value={options.effort ?? ''} onchange={(e) => oneffort((e.target as HTMLSelectElement).value || null)}>
              <option value="">{t('chatc.opt.auto')}</option>
              {#each efforts as m (m)}<option value={m}>{label('chat.effort', m)}</option>{/each}
            </select>
          </label>
        {/if}
      </div>
      <p class="muted sm note">{t('chatc.opt.modelHint')}</p>
    </div>
  {/if}
</span>

<style>
  .co { position: relative; display: inline-flex; min-width: 0; }
  .cobtn { font: inherit; font-size: var(--fs-xs); color: var(--fg); background: var(--fill); border: var(--border-w) solid transparent; border-radius: 99px;
    padding: 4px 10px; display: inline-flex; align-items: center; gap: 5px; cursor: pointer; max-width: 100%; min-width: 0; }
  .cobtn:hover, .cobtn[aria-expanded="true"] { background: var(--fill-2); }
  .cobtn.bypass { color: var(--danger); border-color: var(--danger); font-weight: 600; background: color-mix(in srgb, var(--danger) 8%, transparent); }
  .extra { max-width: 18ch; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pop { position: absolute; bottom: calc(100% + 6px); inset-inline-start: 0; z-index: 6; width: min(380px, 86vw); padding: 12px; display: grid; gap: 10px;
    background: var(--panel); border: var(--border-w) solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg); }
  fieldset { border: 0; margin: 0; padding: 0; display: grid; gap: 2px; min-width: 0; }
  legend { font-weight: 600; font-size: var(--fs-sm); margin-bottom: 4px; padding: 0; }
  .radio { display: flex; gap: 8px; align-items: flex-start; margin: 0; padding: 6px 8px; border-radius: 8px; cursor: pointer; font-size: var(--fs-sm); }
  .radio:hover { background: var(--fill); }
  .radio input { margin-top: 3px; flex: none; }
  .radio.danger b { color: var(--danger); }
  .blk { display: block; margin-top: 2px; line-height: 1.4; }
  .sm { font-size: var(--fs-xs); }
  .row2 { display: flex; gap: 10px; flex-wrap: wrap; }
  .field { display: grid; gap: 3px; flex: 1 1 140px; min-width: 0; margin: 0; }
  .lbl { font-size: var(--fs-xs); font-weight: 600; }
  .field, .lbl { margin: 0; }
  .field select { width: 100%; font-size: var(--fs-xs); padding: 4px 6px; }
  .note { margin: 0; }
</style>
