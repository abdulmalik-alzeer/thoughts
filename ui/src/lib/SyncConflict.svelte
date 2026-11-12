<script lang="ts">
  // The same documents changed on this computer and on another device: for each one, keep mine,
  // take the other device's, or keep both (the other one saved beside it). Each can be compared
  // first. Mounted once in App; opened by `askClash` (lib/conflict.svelte.ts).
  import Icon from './Icon.svelte';
  import ChangeView, { type ChangeFile } from './ChangeView.svelte';
  import { api } from './api';
  import { errDetail } from './errors';
  import { trapFocus } from './focus';
  import { clash, closeClash, type Choice } from './conflict.svelte';
  import { t, tn } from './i18n';

  const CHOICES: Choice[] = ['mine', 'theirs', 'both'];
  const label = (c: Choice) => t('histc.clash.' + c);
  let shown = $state<Record<string, { loading: boolean; error: string; files: ChangeFile[] | null }>>({});
  // A fresh list (asked again with more documents): what was opened before is forgotten.
  $effect(() => { if (clash.answer) shown = {}; });

  const allAre = (c: Choice) => clash.files.length > 0 && clash.files.every((f) => clash.choices[f] === c);
  function setAll(c: Choice) { for (const f of clash.files) clash.choices[f] = c; }

  async function compare(f: string) {
    if (shown[f] && !shown[f].error) { delete shown[f]; return; }
    shown[f] = { loading: true, error: '', files: null };
    try {
      const r = await api(`/git/conflict?path=${encodeURIComponent(f)}`);
      shown[f] = { loading: false, error: '', files: Array.isArray(r?.files) ? r.files : [] };
    } catch (e) {
      shown[f] = { loading: false, error: errDetail(e) || t('histc.clash.compareFailed'), files: null };
    }
  }
  const name = (f: string) => f.split('/').pop() || f;
  const both = $derived(clash.files.filter((f) => clash.choices[f] === 'both'));
</script>

{#if clash.answer}
  <div class="modal-bg top" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) closeClash(null); }}>
    <div class="modal clash" role="dialog" aria-modal="true" aria-labelledby="clash-title" aria-describedby="clash-body" tabindex="-1" use:trapFocus={{ onEscape: () => closeClash(null) }}>
      <h2 id="clash-title">{t('shell.conflict.title')}</h2>
      <p id="clash-body" class="muted">{tn('histc.clash.body', clash.files.length)}</p>

      {#if clash.files.length > 1}
        <div class="row all" role="group" aria-label={t('histc.clash.forAll')}>
          <span class="muted sm">{t('histc.clash.forAll')}</span>
          {#each CHOICES as c (c)}
            <button type="button" class="btn sm" class:on={allAre(c)} aria-pressed={allAre(c)} onclick={() => setAll(c)}>{label(c)}</button>
          {/each}
        </div>
      {/if}

      <ul class="docs scroll">
        {#each clash.files as f (f)}
          <li class="doc">
            <div class="row line">
              <span class="grow fname" title={f}><Icon name="docs" size={14} /> <bdi>{f}</bdi></span>
              <div class="seg" role="radiogroup" aria-label={t('histc.clash.choiceFor', { name: name(f) })}>
                {#each CHOICES as c (c)}
                  <button type="button" class="segb" role="radio" aria-checked={clash.choices[f] === c} onclick={() => (clash.choices[f] = c)}>{label(c)}</button>
                {/each}
              </div>
              <button type="button" class="btn ghost sm" aria-expanded={!!shown[f] && !shown[f].error} onclick={() => compare(f)}>
                <Icon name="eye" size={14} /> {t(shown[f] && !shown[f].error ? 'histc.clash.hide' : 'histc.clash.compare')}
              </button>
            </div>
            {#if shown[f]}
              <div class="cmp" aria-live="polite">
                {#if shown[f].loading}
                  <div class="skel" aria-busy="true"></div>
                {:else if shown[f].error}
                  <p class="muted sm" role="alert" dir="auto"><Icon name="warning" size={14} /> {shown[f].error}</p>
                {:else if shown[f].files}
                  <ChangeView files={shown[f].files ?? []} single labels={{ added: t('histc.clash.onlyTheirs'), removed: t('histc.clash.onlyMine') }} />
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>

      <p class="note">
        {t('shell.conflict.backup')}
        {#if both.length}<br />{tn('histc.clash.bothNote', both.length)}{/if}
      </p>
      <div class="row end">
        <button type="button" class="btn" onclick={() => closeClash(null)}>{t('cancel')}</button>
        <button type="button" class="btn primary" data-autofocus onclick={() => closeClash({ ...clash.choices })}><Icon name="sync" size={14} /> {t('histc.clash.finish')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .clash { width: min(760px, calc(100vw - 32px)); box-sizing: border-box; display: grid; gap: 12px; }
  h2 { margin: 0; font-size: 17px; }
  p { margin: 0; }
  .all { gap: 6px; flex-wrap: wrap; }
  .all .btn.on { background: var(--tint); color: var(--on-tint); }
  .docs { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; max-height: 50vh; }
  .doc { border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 8px 10px; display: grid; gap: 8px; min-width: 0; }
  .line { gap: 8px; flex-wrap: wrap; align-items: center; }
  .fname { min-width: 160px; overflow-wrap: anywhere; display: inline-flex; gap: 6px; align-items: center; }
  .seg .segb[aria-checked='true'] { background: var(--accent); color: var(--accent-fg); }
  .cmp { max-height: 40vh; overflow: auto; }
  .skel { height: 40px; }
  .note { font-size: 13px; color: var(--fg); background: var(--fill); border-radius: var(--radius); padding: 8px 10px; }
  .end { justify-content: flex-end; gap: 8px; flex-wrap: wrap; }
  .sm { font-size: var(--fs-xs); }
</style>
