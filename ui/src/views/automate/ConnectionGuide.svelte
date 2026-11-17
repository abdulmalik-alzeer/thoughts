<script lang="ts">
  // Building a Connection: plain steps the person follows themselves. The app never signs in, stores
  // a password or connects anything. "Walk me through it in a chat" opens a new ordinary chat with a
  // ready-written request in its message box, unsent. Done marks the suggestion as built.
  import Icon from '../../lib/Icon.svelte';
  import { t } from '../../lib/i18n';
  import { trapFocus } from '../../lib/focus';
  import { toBody, type Guide } from '../../lib/automate';

  let { guide, busy = false, onwalk, ondone, onclose }: {
    guide: Guide; busy?: boolean; onwalk: () => void; ondone: () => void; onclose: () => void;
  } = $props();
</script>

<div class="modal-bg no-print" role="presentation" use:toBody onclick={(e) => e.target === e.currentTarget && !busy && onclose()}>
  <div class="modal gd" role="dialog" aria-modal="true" aria-labelledby="au-gd-h" aria-describedby="au-gd-intro" tabindex="-1" use:trapFocus={{ onEscape: () => { if (!busy) onclose(); } }}>
    <span class="pill accent kind"><Icon name="external" size={13} /> {t('auto-ui.kind.integration')}</span>
    <h2 id="au-gd-h" dir="auto">{guide.title}</h2>
    {#if guide.intro}<p id="au-gd-intro" class="intro" dir="auto">{guide.intro}</p>{:else}<p id="au-gd-intro" class="muted sm">{t('auto-ui.gd.hint')}</p>{/if}
    {#if guide.steps.length}
      <h3>{t('auto-ui.gd.steps')}</h3>
      <ol class="steps">
        {#each guide.steps as s, i (i)}<li dir="auto">{s}</li>{/each}
      </ol>
    {/if}
    <p class="muted sm safe"><Icon name="shield" size={14} /> <span>{t('auto-ui.gd.safe')}</span></p>
    {#if guide.chatPrompt}
      <div class="walk">
        <button type="button" class="btn" disabled={busy} aria-busy={busy} onclick={onwalk} aria-describedby="au-gd-walk"><Icon name="chat" size={15} /> {t('auto-ui.gd.walk')}</button>
        <p id="au-gd-walk" class="muted sm">{t('auto-ui.gd.walkHint')}</p>
      </div>
    {/if}
    <div class="row end">
      <button type="button" class="btn" disabled={busy} onclick={onclose}>{t('auto-ui.gd.later')}</button>
      <button type="button" class="btn primary" disabled={busy} onclick={ondone}><Icon name="check" size={15} /> {t('auto-ui.gd.done')}</button>
    </div>
  </div>
</div>

<style>
  .gd { max-width: 600px; width: 100%; display: grid; gap: 6px; max-height: 90vh; overflow: auto; justify-items: start; }
  .gd > * { justify-self: stretch; }
  .gd > .kind { justify-self: start; display: inline-flex; align-items: center; gap: 5px; }
  h2 { margin: 2px 0 0; font-size: 17px; }
  h3 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); }
  p { margin: 0; }
  .intro { white-space: pre-line; line-height: 1.5; font-size: 13.5px; overflow-wrap: anywhere; }
  .steps { margin: 0; padding-inline-start: 22px; display: grid; gap: 6px; font-size: 13.5px; line-height: 1.5; overflow-wrap: anywhere; }
  .sm { font-size: var(--fs-xs); }
  .safe { display: flex; gap: 6px; align-items: flex-start; margin-top: 4px; }
  .safe :global(.icon) { flex: none; margin-top: 1px; }
  .walk { display: grid; gap: 4px; justify-items: start; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); margin-top: 4px; }
  .end { justify-content: flex-end; gap: 8px; margin-top: 10px; flex-wrap: wrap; }
</style>
