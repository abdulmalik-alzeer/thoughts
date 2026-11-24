<script lang="ts">
  // One control in the composer for how this chat's assistant works: what it may do (Claude Code's
  // permission modes), and — less often needed — which model and how much effort. The button always
  // names the permission mode, so what the assistant may do is never hidden behind it. Every list
  // comes from the installed assistant program; nothing here is a fixed list of models or modes.
  import Icon from '../../lib/Icon.svelte';
  import { t } from '../../lib/i18n';
  import HelpTip from '../../lib/HelpTip.svelte';
  import ModelPicker from '../../lib/models/ModelPicker.svelte';
  import { nameOf, type ModelEntry } from '../../lib/models/picker';

  let { options, models = [], efforts = [], modes, label, onmodel = () => {}, autoNote = '', scope = '', oneffort = () => {}, onperm, permPrefix = 'chat3.perm', hintPrefix = 'set.perm', effortPrefix = 'chat.effort', programs = [], program = '', onprogram, modelNote = '', plain = false, below = false, busy = false }: {
    options: { model?: string | null; effort?: string | null; permissionMode?: string | null };
    /** The models as their program reported them (`entriesFrom`: its names, its default). */
    models?: ModelEntry[]; efforts?: string[]; modes: string[];
    /** Where the names of the modes and efforts are (Codex has names of its own for them). */
    permPrefix?: string; hintPrefix?: string; effortPrefix?: string;
    /** The assistant programs this chat can be moved between, when there are two. */
    programs?: { id: string; label: string; ok: boolean }[]; program?: string; onprogram?: (id: string) => void;
    /** Translated label when there is one, the raw value otherwise. */
    label: (prefix: string, v: string) => string;
    /** A model was picked (`null` = automatic). Answers `false` when it could not be saved. */
    onmodel?: (v: string | null) => unknown;
    /** What "Automatic" means here, said under it in the model list. */
    autoNote?: string;
    /** Whose models these are (the assistant program), for the list's "Recently used". */
    scope?: string;
    oneffort?: (v: string | null) => void; onperm?: (v: string) => void;
    /** Only the model and the effort (a plan's runs have their own fixed rules): the button names them, or the default. */
    plain?: boolean;
    /** Open under the button (in a page header) instead of over it (in the composer). */
    below?: boolean;
    /** A change is being saved: the menus wait. */
    busy?: boolean;
    /** Said under the model menu when the program could not list its models. */
    modelNote?: string;
  } = $props();

  const BYPASS = 'bypassPermissions';
  let open = $state(false);
  let root = $state<HTMLElement>();
  let button = $state<HTMLButtonElement>();
  const uid = Math.random().toString(36).slice(2, 8);

  const mode = $derived(options.permissionMode ?? 'acceptEdits');
  // What the button says: the mode always, and the assistant when there are two; for a plan, the model
  // and effort when they are not the default (a chat names those on its own chip).
  const extra = $derived([programs.length > 1 ? programs.find((p) => p.id === program)?.label : null, ...(plain ? [nameOf(models, options.model), options.effort ? label(effortPrefix, options.effort) : null] : [])].filter(Boolean).join(' · '));
  const hint = (m: string) => { const k = `${hintPrefix}.${m}.hint`; const s = t(k); return s === k ? '' : s; };

  function close(refocus = true) { open = false; if (refocus) button?.focus(); }
  function onkey(e: KeyboardEvent) { if (e.key === 'Escape' && open) { e.stopPropagation(); e.preventDefault(); close(); } }
</script>

<svelte:window onclick={(e) => { if (open && root && !root.contains(e.target as Node) && !(e.target as HTMLElement)?.closest?.('[aria-modal="true"], [data-model-picker]')) open = false; }} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span class="co" bind:this={root} onkeydown={onkey}>
  <button bind:this={button} type="button" class="cobtn" class:bypass={mode === BYPASS} class:plan={mode === 'plan'}
    aria-haspopup="dialog" aria-expanded={open} aria-controls="co-{uid}" onclick={() => (open = !open)}
    title={plain ? t('mado.opts.tip') : t('models72.optTip')} aria-label={plain ? t('mado.opts.aria', { what: extra || t('default') }) : t('chatc.opt.aria', { mode: label(permPrefix, mode) })}>
    {#if plain}
      <Icon name="settings" size={13} />
      {#if extra}<span class="ell extra wide" dir="ltr">{extra}</span>{:else}<span class="ell">{t('mado.opts.default')}</span>{/if}
    {:else}
    <Icon name={mode === BYPASS ? 'warning' : mode === 'plan' ? 'eye' : 'shield'} size={13} />
    <span class="ell">{label(permPrefix, mode)}</span>
    {#if extra}<span class="ell muted extra" dir="ltr">· {extra}</span>{/if}
    {/if}
    <Icon name="down" size={12} />
  </button>
  {#if open}
    <div class="pop" class:below id="co-{uid}" role="dialog" aria-label={plain ? t('mado.opts.title') : t('chatc.opt.title')}>
      {#if programs.length > 1 && onprogram}
        <fieldset>
          <legend>{t('codex.move.label')}</legend>
          <div class="progs">
            {#each programs as p (p.id)}
              <label class="radio prog" class:on={p.id === program} title={p.ok ? undefined : t('codex.move.unavailable', { to: p.label })}>
                <input type="radio" name="co-prog-{uid}" value={p.id} checked={p.id === program} disabled={!p.ok && p.id !== program} onchange={() => { close(false); onprogram?.(p.id); }} />
                <span><b><bdi>{p.label}</bdi></b></span>
              </label>
            {/each}
          </div>
        </fieldset>
      {/if}
      {#if modes.length}
        <fieldset>
          <legend>{t('chatc.opt.may')}</legend>
          {#each modes as m (m)}
            <label class="radio" class:danger={m === BYPASS}>
              <input type="radio" name="co-perm-{uid}" value={m} checked={mode === m} onchange={() => onperm?.(m)} />
              <span><b>{label(permPrefix, m)}</b>{#if hint(m)}<span class="muted sm blk">{hint(m)}</span>{/if}</span>
            </label>
          {/each}
        </fieldset>
      {/if}
      <!-- A chat has the model and the effort on the composer itself (ChatModel.svelte); a plan chooses them here. -->
      {#if plain}
      <div class="row2">
        <div class="field">
          <span class="lbl" id="co-model-{uid}">{t('model')}</span>
          <ModelPicker small value={options.model} {models} label={t('model')} onpick={onmodel} {autoNote} {scope} disabled={busy} />
        </div>
        {#if efforts.length}
          <label class="field">
            <span class="lbl">{t('effort')}</span>
            <select value={options.effort ?? ''} onchange={(e) => oneffort((e.target as HTMLSelectElement).value || null)} disabled={busy}>
              <option value="">{t('chatc.opt.auto')}</option>
              {#each efforts as m (m)}<option value={m}>{label(effortPrefix, m)}</option>{/each}
            </select>
          </label>
        {/if}
      </div>
      {#if modelNote}<p class="sm note model-note" role="status">{modelNote}</p>{/if}
      <p class="muted sm note help"><HelpTip text={t('mado.opts.hint')} label={t('model')} /></p>
      {/if}
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
  .extra.wide { max-width: 30ch; }
  .pop.below { bottom: auto; top: calc(100% + 6px); inset-inline-start: auto; inset-inline-end: 0; }
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
  div.field { flex: 1.4 1 170px; }
  .note { margin: 0; }
  .help { display: flex; justify-content: flex-end; }
  .model-note { margin-bottom: 6px; padding: 6px 8px; border-radius: var(--radius); background: var(--fill); color: var(--fg); }
  .progs { display: flex; gap: 4px; flex-wrap: wrap; }
  .prog { flex: 1 1 120px; }
  .prog.on { background: var(--fill); }
</style>
