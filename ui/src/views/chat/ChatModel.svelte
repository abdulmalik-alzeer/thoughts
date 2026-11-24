<script lang="ts">
  // The composer's own chip for the model (round 0.7.2): it names the model in force and opens the model list directly, so the model is
  // no longer found only at the bottom of the options menu. The effort sits beside it as a small
  // menu of the levels the program itself lists for this model; nothing here is a list of the app's.
  import ModelPicker from '../../lib/models/ModelPicker.svelte';
  import type { ModelEntry } from '../../lib/models/picker';
  import { t } from '../../lib/i18n';

  let { model, effort, models, efforts, effortName, onmodel, oneffort, autoNote = '', scope = '', note = '', busy = false }: {
    model: string | null | undefined; effort: string | null | undefined;
    models: ModelEntry[]; efforts: string[];
    /** The agent's own name for an effort level, else the app's label for it. */
    effortName: (v: string) => string;
    /** A model was picked (`null` = automatic). Answers `false` when it could not be saved. */
    onmodel: (v: string | null) => unknown;
    oneffort: (v: string | null) => void;
    autoNote?: string; scope?: string;
    /** Said in the model list when the program could not list its models. */
    note?: string;
    busy?: boolean;
  } = $props();
</script>

<span class="cm">
  <ModelPicker chip placeholder={t('models72.modelAuto')} value={model} {models} label={t('model')} onpick={onmodel} {autoNote} {scope} {note} disabled={busy} />
  {#if efforts.length || effort}
    <select class="eff" class:set={!!effort} value={effort ?? ''} aria-label={t('effort')} title={t('effort')} disabled={busy}
      onchange={(e) => oneffort(e.currentTarget.value || null)}>
      <option value="">{t('models72.effortAuto')}</option>
      {#each efforts as m (m)}<option value={m}>{effortName(m)}</option>{/each}
      {#if effort && !efforts.includes(effort)}<option value={effort}>{effortName(effort)}</option>{/if}
    </select>
  {/if}
</span>

<style>
  /* One item of the composer's row that may shrink: the model's name is cut short before the row grows. */
  /* A small basis keeps it on the row's line when the window is narrow (the row decides its line breaks
     from the basis); it then takes the free room first (before the row's spacer), up to exactly what its text needs. */
  .cm { display: inline-flex; align-items: center; gap: 4px; min-width: 0; flex: 999 1 15em; max-width: fit-content; }
  .cm :global(.mp-btn) { flex: 0 1 auto; min-width: 6.5em; }
  .eff { flex: none; width: auto; max-width: 12ch; font-size: var(--fs-xs); color: var(--muted); background-color: var(--fill); border-color: transparent; border-radius: 99px;
    padding: 4px 8px; text-overflow: ellipsis; cursor: pointer; }
  .eff.set { color: var(--fg); }
  .eff:hover:not(:disabled) { background-color: var(--fill-2); }
  :global([data-theme="contrast"]) .eff { border-color: var(--border); }
</style>
