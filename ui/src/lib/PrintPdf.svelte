<script lang="ts">
  // "Print…" and "Save as PDF…" for a note shown in a view (an item's front page, its log). `source`
  // hands over the rendered note when a button is pressed; null when there is nothing to print.
  import Icon from './Icon.svelte';
  import { t } from './i18n';
  import { toast } from './store.svelte';
  import { printNote, saveNotePdf, type PrintSource } from './printPdf';

  let { source }: { source: () => PrintSource | null } = $props();
  let saving = $state(false);

  function print() {
    const s = source();
    if (s) printNote(s);
  }
  async function save() {
    const s = source();
    if (!s || saving) return;
    saving = true;
    try {
      if ((await saveNotePdf(s)) === 'busy') toast(t('libf-pdf.busy'), { kind: 'info', key: 'pdf-save' });
    } finally { saving = false; }
  }
</script>

<span class="pp no-print" role="group" aria-label={t('libf-pdf.group')}>
  <button type="button" class="btn ghost sm" onclick={print} title={t('libf-pdf.printTip')}><Icon name="print" size={13} /> {t('libf-pdf.print')}</button>
  <button type="button" class="btn ghost sm" onclick={save} disabled={saving} aria-busy={saving} title={t('libf-pdf.saveTip')}>
    <Icon name="download" size={13} /> {t(saving ? 'libf-pdf.saving' : 'libf-pdf.save')}
  </button>
</span>

<style>
  .pp { display: inline-flex; flex-wrap: wrap; gap: 4px; }
</style>
