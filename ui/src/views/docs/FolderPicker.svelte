<script lang="ts">
  // Choose the folder to move a file or folder into: the library's folders as a list, plus
  // "New folder…" for one that does not exist yet. Nothing is typed as a path.
  import Icon from '../../lib/Icon.svelte';
  import { trapFocus } from '../../lib/focus';
  import { t } from '../../lib/i18n';

  let { item, folder, folders, clean, pick, close }: {
    /** Library path of what is being moved. */
    item: string;
    /** Is `item` a folder? (It cannot go inside itself.) */
    folder: boolean;
    /** Every folder in the library, as library paths. */
    folders: string[];
    /** Tidies a typed name; null when it cannot be used. */
    clean: (raw: string) => string | null;
    /** Called with the chosen folder ('' = the top of the library). */
    pick: (dir: string) => void;
    close: () => void;
  } = $props();

  const parentOf = (p: string) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '');
  const here = parentOf(item);
  const name = item.split('/').pop() || item;
  const inside = (p: string, what: string) => p === what || p.startsWith(what + '/');

  let q = $state('');
  let sel = $state<string | null>(null);
  let making = $state(false);
  let newName = $state('');
  let newErr = $state('');
  let nameEl = $state<HTMLInputElement>();

  const choices = $derived.by(() => {
    const ql = q.trim().toLowerCase();
    return [...new Set(folders)]
      .filter((d) => d && !(folder && inside(d, item)))
      .filter((d) => !ql || d.toLowerCase().includes(ql))
      .sort((a, b) => a.localeCompare(b));
  });
  const depth = (d: string) => (q.trim() ? 0 : d.split('/').length - 1);
  const leaf = (d: string) => (q.trim() ? d : d.split('/').pop() || d);

  async function startNew() {
    making = true; newErr = ''; newName = '';
    await Promise.resolve();
    nameEl?.focus();
  }
  function confirm() {
    if (making) {
      const n = clean(newName);
      if (!n) { newErr = t('docs3.badName'); nameEl?.focus(); return; }
      const base = sel ?? '';
      const dir = base ? `${base}/${n}` : n;
      if (folder && inside(dir, item)) { newErr = t('docs3.badName'); return; }
      pick(dir);
      return;
    }
    if (sel === null || sel === here) return;
    pick(sel);
  }
  const canMove = $derived(making ? !!newName.trim() : sel !== null && sel !== here);
</script>

<div class="modal-bg no-print" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
  <div class="modal fp" role="dialog" aria-modal="true" aria-labelledby="fp-title" aria-describedby="fp-body" tabindex="-1" use:trapFocus={{ onEscape: close }}>
    <h2 id="fp-title"><span dir="auto">{t('docsa.moveTitle', { name })}</span></h2>
    <p id="fp-body" class="muted">{t('docsa.moveBody')}</p>
    <input placeholder={t('docsa.filterFolders')} aria-label={t('docsa.filterFolders')} bind:value={q} dir="auto" data-autofocus />
    <div class="list scroll" role="radiogroup" aria-label={t('docsa.folderList')}>
      {#if !q.trim()}
        <label class="opt" class:on={sel === ''} class:here={here === ''}>
          <input type="radio" name="fp-dir" value="" checked={sel === ''} disabled={here === '' && !making} onchange={() => (sel = '')} />
          <Icon name="home" size={14} /> <span class="grow">{t('docsa.top')}</span>
          {#if here === ''}<span class="muted sm">{t('docsa.here')}</span>{/if}
        </label>
      {/if}
      {#each choices as d (d)}
        <label class="opt" class:on={sel === d} class:here={here === d} style="padding-inline-start: {10 + depth(d) * 16}px">
          <input type="radio" name="fp-dir" value={d} checked={sel === d} disabled={here === d && !making} onchange={() => (sel = d)} />
          <Icon name="folder" size={14} /> <span class="grow nm"><bdi>{leaf(d)}</bdi></span>
          {#if here === d}<span class="muted sm">{t('docsa.here')}</span>{/if}
        </label>
      {:else}
        {#if q.trim()}<p class="muted sm none">{t('docsa.noFolders')}</p>{/if}
      {/each}
    </div>
    {#if making}
      <label class="newf">
        <span class="sm">{sel ? t('docsa.newFolderIn', { folder: sel }) : t('docsa.newFolderTop')}</span>
        <input bind:this={nameEl} bind:value={newName} placeholder={t('docs3.folderPlaceholder')} aria-label={t('docsa.newFolderName')} dir="auto"
          aria-invalid={newErr ? 'true' : undefined} onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); confirm(); } }} />
        {#if newErr}<span class="err sm" role="alert">{newErr}</span>{/if}
      </label>
    {/if}
    <div class="row foot">
      {#if !making}<button class="btn ghost" onclick={startNew}><Icon name="plus" size={14} /> {t('docsa.newFolder')}</button>{/if}
      <span class="grow"></span>
      <button class="btn" onclick={close}>{t('cancel')}</button>
      <button class="btn primary" disabled={!canMove} onclick={confirm}>{t('docsa.moveHere')}</button>
    </div>
  </div>
</div>

<style>
  .fp { width: min(520px, calc(100% - 32px)); display: flex; flex-direction: column; gap: 10px; }
  .fp h2 { margin: 0; font-size: 17px; overflow-wrap: anywhere; }
  .fp p { margin: 0; }
  .list { max-height: 46vh; min-height: 120px; border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 4px; display: flex; flex-direction: column; gap: 1px; }
  .opt { display: flex; align-items: center; gap: 8px; padding: 6px 10px; border-radius: 6px; cursor: pointer; font-size: 13px; }
  .opt:hover { background: var(--fill); }
  .opt.on { background: var(--tint); color: var(--on-tint); font-weight: 600; }
  .opt.here { cursor: default; }
  .opt input { margin: 0; }
  .opt :global(.icon) { color: var(--muted); flex: none; }
  .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .none { margin: 8px 10px; }
  .newf { display: grid; gap: 4px; }
  .err { color: var(--danger); }
  .sm { font-size: var(--fs-xs); }
  .foot { gap: 8px; }
</style>
