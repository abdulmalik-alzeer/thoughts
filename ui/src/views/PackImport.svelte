<script lang="ts">
  // "Add a file someone sent": the one import dialog, shared by Customise › Share (packs) and
  // Customise › Plugins (plugin files, which are packs). The service checks the whole file and works
  // out its fingerprint before anything is staged; what the file says about itself is shown as its
  // maker's words; then it goes through the usual review. Nothing is ever applied here.
  import Icon from '../lib/Icon.svelte';
  import { toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { t, tn } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { readPack, stagePack } from '../lib/layer';
  import { inspectPack } from '../lib/packs';
  import { importLines, type PluginLook } from '../lib/plugins';

  let { label = 'layer.import', checking = 'ldc.imp.checking', primary = false }: { label?: string; checking?: string; primary?: boolean } = $props();

  let busy = $state(false);
  let picker = $state<HTMLInputElement>();

  /** A file's own words as one plain line: nothing hidden, nothing that reorders it, no quotation marks. */
  function packWords(v: unknown, max: number): string {
    return String(v ?? '')
      .replace(/[\u0000-\u001f\u007f-\u009f\u00ad\u180e\u200b\u2028-\u202e\u2060-\u206f\ufeff]|[\u{e0000}-\u{e0fff}]/gu, ' ')
      .replace(/["“”«»]/g, '').replace(/\s+/g, ' ').trim().slice(0, max);
  }

  function pick(e: Event) {
    const el = e.target as HTMLInputElement;
    const f = el.files?.[0];
    el.value = '';
    if (!f || busy) return;
    busy = true;
    (async () => {
      const r = await readPack(f);
      if (r.unsupported) { toast(t('layer3.packNewer'), { kind: 'error' }); return; }
      const look = await inspectPack(r.pack);
      // As a plugin: new, an update of one you have, older than yours, or one this version cannot run.
      const plugin = importLines((look as { plugin?: PluginLook | null }).plugin, packWords);
      if (plugin.refused) { toast(plugin.refused, { kind: 'error' }); return; }
      const name = packWords(look.name, 80);
      const lines: string[] = [];
      const said = [name ? t('ldc.imp.who', { name }) : '', look.author ? t('ldc.imp.by', { author: packWords(look.author, 80) }) : '', look.version ? t('ldc.imp.version', { version: packWords(look.version, 20) }) : ''].filter(Boolean);
      if (said.length) lines.push(said.join(', ') + '.', t('ldc.imp.claims'));
      lines.push(...plugin.lines);
      lines.push(t(look.kind === 'feature' ? 'layer3.importFeature' : 'layer3.importFull') + ' ' + tn('layer3.importFiles', look.files.length));
      if (look.matches === false) lines.push(t('ldc.imp.fpBad', { claimed: look.claimed ?? '' }));
      lines.push(t('ldc.imp.fp', { fp: look.code }));
      if (look.earlier && !look.earlier.same && !plugin.lines.length) lines.push(look.earlier.version ? t('ldc.imp.earlier', { version: packWords(look.earlier.version, 20), fp: look.earlier.code }) : t('ldc.imp.earlierNoVersion', { fp: look.earlier.code }));
      if (!look.changes) lines.push(t('ldc.imp.same'));
      const says = packWords(look.description, 400);
      if (says) lines.push(`${t('lda.packSaysIntro')}\n“${says}”`);
      const ok = await confirmDialog({
        title: name ? `${t('layer3.importTitle')} “${name}”` : t('layer3.importTitle'),
        body: lines.join('\n\n'),
        list: look.files.slice(0, 30),
        note: t('layer3.importNote'),
        confirmLabel: t('layer3.importGo'),
        danger: look.matches === false,
      });
      if (ok) await stagePack(r.pack);
    })().catch((err) => failed(t('ldc.imp.failed'), err)).finally(() => { busy = false; });
  }
</script>

<button type="button" class="btn" class:primary disabled={busy} aria-busy={busy} onclick={() => picker?.click()}><Icon name="upload" size={15} /> {busy ? t(checking) : t(label)}</button>
<input type="file" accept=".json,application/json" hidden bind:this={picker} onchange={pick} />
