<script lang="ts">
  // The fields of one table, edited in a small box: add, rename, retype, mark as key, remove. Nothing changes until "Done".
  import { onMount, tick } from 'svelte';
  import type { ErdField } from '../../types';
  import { t } from '../../../i18n';
  import { ICON } from '../icons';
  import { matchKey } from '../../../keys/registry';
  import { FIELDS_MAX, FIELD_TEXT_MAX, cleanFields } from '../ops';

  let { name, fields, dir, onapply, oncancel }: { name: string; fields: ErdField[]; dir: 'ltr' | 'rtl'; onapply: (fields: ErdField[]) => void; oncancel: () => void } = $props();
  // svelte-ignore state_referenced_locally
  let rows = $state(fields.map((f) => ({ ...f, key: f.key ?? '' })) as { name: string; type: string; key: string; note?: string }[]);
  let box: HTMLDivElement;
  const Add = ICON.add, Remove = ICON.remove;
  const focusRow = (i: number) => tick().then(() => (box.querySelectorAll<HTMLInputElement>('input.fname')[i] ?? box.querySelector<HTMLElement>('button'))?.focus());
  onMount(() => { focusRow(0); });
  function add() { if (rows.length >= FIELDS_MAX) return; rows.push({ name: '', type: '', key: '' }); focusRow(rows.length - 1); }
  function apply() { onapply(cleanFields(rows.map((r) => ({ ...r, key: r.key || undefined })))); }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="fields nodrag nopan nowheel" {dir} bind:this={box} role="dialog" tabindex="-1" aria-label={t('mado-graph.fields.title', { name })}
  onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Escape') { e.preventDefault(); oncancel(); } else if (matchKey(e, 'mado-fields') === 'mado-fields.apply') { e.preventDefault(); apply(); } }}>
  <p class="title" dir="auto">{t('mado-graph.fields.title', { name })}</p>
  {#if rows.length}
    <ul>
      {#each rows as r, i (i)}
        <li>
          <input class="fname" dir="auto" bind:value={r.name} maxlength={FIELD_TEXT_MAX} placeholder={t('mado-graph.fields.name')} aria-label={t('mado-graph.fields.nameOf', { n: i + 1 })} spellcheck="false" autocomplete="off" />
          <input class="ftype" dir="ltr" bind:value={r.type} maxlength={FIELD_TEXT_MAX} placeholder={t('mado-graph.fields.type')} aria-label={t('mado-graph.fields.typeOf', { n: i + 1 })} spellcheck="false" autocomplete="off" />
          <select bind:value={r.key} aria-label={t('mado-graph.fields.keyOf', { n: i + 1 })}>
            <option value="">{t('mado-graph.key.none')}</option>
            <option value="pk">{t('mado-graph.key.pk.long')}</option>
            <option value="fk">{t('mado-graph.key.fk.long')}</option>
          </select>
          <button type="button" class="x" onclick={() => { rows.splice(i, 1); focusRow(Math.min(i, rows.length - 1)); }} aria-label={t('mado-graph.fields.remove', { name: r.name || String(i + 1) })} title={t('mado-graph.fields.removeTip')}>
            <Remove size={14} aria-hidden="true" />
          </button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="muted">{t('mado-graph.fields.none')}</p>
  {/if}
  <div class="foot">
    <button type="button" class="btn ghost" onclick={add} disabled={rows.length >= FIELDS_MAX}><Add size={14} aria-hidden="true" /> {t('mado-graph.fields.add')}</button>
    <span class="grow"></span>
    <button type="button" class="btn ghost" onclick={oncancel}>{t('cancel')}</button>
    <button type="button" class="btn primary" onclick={apply}>{t('mado-graph.fields.done')}</button>
  </div>
  <p class="muted hint">{t('mado-graph.fields.hint')}</p>
</div>

<style>
  .fields {
    width: min(430px, 86vw); max-height: min(60vh, 440px); overflow: auto; padding: 12px; background: var(--panel); color: var(--fg); border: var(--border-w) solid var(--hairline);
    border-radius: var(--radius-lg); box-shadow: var(--shadow-lg); font-size: 13px; text-align: start; cursor: default;
  }
  .title { margin: 0 0 8px; font-weight: 600; }
  ul { list-style: none; margin: 0 0 8px; padding: 0; display: grid; gap: 6px; }
  li { display: grid; grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr) auto auto; gap: 6px; align-items: center; }
  input, select { font: inherit; font-size: 13px; color: var(--fg); background: var(--bg); border: var(--border-w) solid var(--control-border); border-radius: 7px; padding: 4px 7px; min-width: 0; width: 100%; box-sizing: border-box; }
  .ftype { font-family: var(--mono); font-size: 12px; }
  select { width: auto; max-width: 130px; }
  .x { display: grid; place-items: center; width: 28px; height: 28px; border: 0; border-radius: 7px; background: transparent; color: var(--muted); cursor: pointer; }
  .x:hover { background: var(--fill); color: var(--danger); }
  .foot { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  .grow { flex: 1; }
  .muted { color: var(--muted); margin: 4px 0 8px; }
  .hint { font-size: 12px; margin: 8px 0 0; }
</style>
