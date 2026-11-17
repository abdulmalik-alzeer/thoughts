<script lang="ts">
  // The backup location holds a copy of the library this computer's copy was not made from: choose,
  // once, how to put the two together. The safe way (keep everything from both) is chosen to begin
  // with. Mounted once in App; opened by `settleJoin` (lib/join.svelte.ts).
  import Icon from './Icon.svelte';
  import { trapFocus } from './focus';
  import { join, cancelJoin, confirmJoin, type How } from './join.svelte';
  import { t } from './i18n';

  const WAYS: { id: How; label: string; hint: string }[] = [
    { id: 'combine', label: 'data-sync.join.both', hint: 'data-sync.join.bothHint' },
    { id: 'theirs', label: 'data-sync.join.theirs', hint: 'data-sync.join.theirsHint' },
  ];
</script>

{#if join.open}
  <div class="modal-bg top" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) cancelJoin(); }}>
    <div class="modal sjoin" role="dialog" aria-modal="true" aria-labelledby="sjoin-title" aria-describedby="sjoin-body" tabindex="-1" aria-busy={join.busy} use:trapFocus={{ onEscape: cancelJoin }}>
      <h2 id="sjoin-title">{t('data-sync.join.title')}</h2>
      <p id="sjoin-body" class="muted" dir="auto">{t('data-sync.join.body', { name: join.name })}</p>

      <fieldset class="ways" disabled={join.busy}>
        <legend class="sr-only">{t('data-sync.join.how')}</legend>
        {#each WAYS as w (w.id)}
          <label class="way" class:on={join.how === w.id}>
            <input type="radio" name="sjoin-how" value={w.id} bind:group={join.how} data-autofocus={w.id === 'combine' ? true : undefined} />
            <span class="txt">
              <span class="lbl">{t(w.label, { name: join.name })}</span>
              <span class="muted sm">{t(w.hint, { name: join.name })}</span>
            </span>
          </label>
        {/each}
      </fieldset>

      <p class="note"><Icon name="check" size={14} /> {t('data-sync.join.safe')}</p>
      <div class="row end">
        <button type="button" class="btn" disabled={join.busy} onclick={cancelJoin}>{t('data-sync.join.notNow')}</button>
        <button type="button" class="btn primary" disabled={join.busy} aria-busy={join.busy} onclick={confirmJoin}>
          <Icon name="sync" size={14} /> {join.busy ? t('data-sync.join.working') : t('data-sync.join.go')}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .sjoin { width: min(620px, calc(100vw - 32px)); box-sizing: border-box; display: grid; gap: 12px; }
  h2 { margin: 0; font-size: 17px; }
  p { margin: 0; }
  .ways { border: 0; margin: 0; padding: 0; display: grid; gap: 8px; min-width: 0; }
  .way { display: flex; gap: 10px; align-items: flex-start; cursor: pointer; border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 10px 12px; }
  .way.on { border-color: var(--accent); }
  .way input { margin-top: 3px; flex: none; }
  .txt { display: grid; gap: 2px; min-width: 0; }
  .lbl { font-weight: 600; }
  .note { font-size: 13px; color: var(--fg); background: var(--fill); border-radius: var(--radius); padding: 8px 10px; display: flex; gap: 6px; align-items: flex-start; }
  .end { justify-content: flex-end; gap: 8px; flex-wrap: wrap; }
  .sm { font-size: var(--fs-xs); }
</style>
