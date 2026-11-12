<script lang="ts">
  // The inside of the trust dialog: which folder, why it is asking, and what would run — each
  // finding in the app's own sentence, with what came from the folder shown beside it as data.
  import Icon from './Icon.svelte';
  import { t, fmtNum } from './i18n';
  import { inTauri } from './api';
  import { trustIntro, trustSentence, type TrustInfo } from './trust';

  let { info }: { info: TrustInfo } = $props();
  let showFailed = $state('');

  /** Open the file's folder in the system file manager with the file selected (desktop app only). */
  async function show(path: string) {
    showFailed = '';
    try { const { revealItemInDir } = await import('@tauri-apps/plugin-opener'); await revealItemInDir(path); }
    catch { showFailed = path; }
  }
</script>

<div class="trust">
  <p class="folder"><Icon name="folder" size={15} /><span class="lbl">{t('seca.trust.folder')}</span><code dir="ltr"><bdi>{info.path}</bdi></code></p>
  <p id="dlg-body" class="muted" dir="auto">{trustIntro(info)}</p>
  {#if info.known}<p class="muted sm" dir="auto">{t('seca.trust.syncHint')}</p>{/if}

  {#if info.items.length}
    <h3>{t('seca.trust.itemsTitle')}</h3>
    <ul class="items scroll">
      {#each info.items as i (i.kind + '|' + i.file + '|' + i.name + '|' + i.detail)}
        <li>
          <p class="what" dir="auto">{trustSentence(i)}{#if i.new}<span class="pill accent new">{t('seca.trust.new')}</span>{/if}</p>
          {#if i.detail}<code class="detail" dir="ltr"><bdi>{i.detail}</bdi></code>{/if}
          {#if i.file}
            <p class="where">
              <span>{t('seca.trust.where')}</span> <code dir="ltr"><bdi>{i.file}</bdi></code>
              {#if i.open && inTauri()}<button type="button" class="more" onclick={() => show(i.open!)}>{t('seca.trust.show')}</button>{/if}
            </p>
            {#if showFailed && showFailed === i.open}<p class="where err" role="alert">{t('seca.trust.showFailed')}</p>{/if}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if info.files.length}
    <h3>{t('seca.trust.filesTitle')}</h3>
    <ul class="files scroll">
      {#each info.files as f (f.file)}
        <li><span class="pill">{t(`seca.trust.file.${f.what}`)}</span> <code dir="ltr"><bdi>{f.file}</bdi></code></li>
      {/each}
      {#if info.moreFiles > 0}<li class="muted">{t('seca.trust.moreFiles', { n: fmtNum(info.moreFiles) })}</li>{/if}
    </ul>
  {/if}

  <p class="note" dir="auto">{t('seca.trust.note')}</p>
</div>

<style>
  .trust { display: grid; gap: 10px; }
  p { margin: 0; }
  h3 { margin: 4px 0 0; font-size: 13px; font-weight: 600; }
  code { font-size: 12px; overflow-wrap: anywhere; }
  .folder { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; background: var(--fill); border-radius: var(--radius); padding: 8px 10px; }
  .folder .lbl { font-weight: 600; font-size: 13px; }
  .sm { font-size: 13px; }
  ul { margin: 0; padding: 0; list-style: none; display: grid; gap: 8px; }
  .items { max-height: 34vh; }
  .files { max-height: 18vh; font-size: 13px; gap: 4px; }
  .items li { display: grid; gap: 4px; border: 1px solid var(--hairline); border-radius: var(--radius); padding: 8px 10px; }
  .what { font-size: 13.5px; font-weight: 500; }
  .new { margin-inline-start: 8px; }
  .detail { display: block; background: var(--code-bg); border-radius: 6px; padding: 6px 8px; white-space: pre-wrap; max-height: 6.5em; overflow: auto; }
  .where { font-size: 12px; color: var(--muted); display: flex; gap: 6px; align-items: baseline; flex-wrap: wrap; }
  .where.err { color: var(--danger); }
  .note { font-size: 13px; color: var(--fg); background: var(--fill); border-radius: var(--radius); padding: 8px 10px; }
</style>
