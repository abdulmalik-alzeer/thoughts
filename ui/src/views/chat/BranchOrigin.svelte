<script lang="ts">
  // At the top of a conversation: where a branch came from (a link to the original chat and to the
  // message it starts from, or that the original is gone), and the branches a chat has. Logic: branch.ts.
  import Icon from '../../lib/Icon.svelte';
  import { app, go } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import { branchInfo, branchesOf, jumpLater, origin } from './branch';

  let { chat }: { chat: any } = $props();

  const SHOWN = 5;
  const info = $derived(branchInfo(chat?.meta));
  const from = $derived(info ? origin(info, app.chats ?? []) : null);
  const branches = $derived(branchesOf(chat?.meta, app.chats ?? []));

  function toMessage() {
    if (!info) return;
    jumpLater(info.from, info.at);
    go({ kind: 'chat', id: info.from });
  }
</script>

{#if info && from}
  <div class="origin" role="note">
    <Icon name="git" size={13} />
    <div class="grow">
      {#if from.exists}
        <span>{t('chatfBranch.origin.from')}
          <button type="button" class="link" onclick={() => go({ kind: 'chat', id: info.from })} aria-label={t('chatfBranch.origin.openAria', { title: from.title })}><bdi>{from.title}</bdi></button>
          ·
          <button type="button" class="link" onclick={toMessage} aria-label={t('chatfBranch.origin.atAria')} title={info.atText}>{t(info.before ? 'chatfBranch.origin.atBefore' : 'chatfBranch.origin.at')}</button></span>
      {:else}
        <span>{from.title ? t('chatfBranch.origin.gone', { title: from.title }) : t('chatfBranch.origin.goneUntitled')}</span>
      {/if}
      <span class="muted sub">{t(info.how === 'fork' ? 'chatfBranch.origin.fork' : 'chatfBranch.origin.text')} {t('chatfBranch.origin.saved')}</span>
    </div>
  </div>
{/if}
{#if branches.length}
  <nav class="origin" aria-label={t('chatfBranch.branches')}>
    <Icon name="git" size={13} />
    <span class="grow">{t('chatfBranch.branches')}
      {#each branches.slice(0, SHOWN) as b, i (b.id)}{#if i > 0}<span aria-hidden="true">, </span>{/if}
        <button type="button" class="link" onclick={() => go({ kind: 'chat', id: b.id })} aria-label={t('chatfBranch.openBranch', { title: b.title })}><bdi>{b.title}</bdi></button>{/each}
      {#if branches.length > SHOWN} <span class="muted">{t('chatfBranch.branchesMore', { n: branches.length - SHOWN })}</span>{/if}
    </span>
  </nav>
{/if}

<style>
  .origin { display: flex; gap: 8px; align-items: flex-start; font-size: var(--fs-sm); color: var(--muted); padding: 6px 10px; border: var(--border-w, 1px) dashed var(--border); border-radius: var(--radius); }
  .origin :global(svg) { flex: none; margin-top: 3px; }
  .grow { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; overflow-wrap: anywhere; }
  nav .grow { display: block; }
  .sub { font-size: var(--fs-xs); }
  .link { font: inherit; color: var(--accent); background: none; border: 0; padding: 0; cursor: pointer; text-decoration: underline; text-underline-offset: 2px; }
  .link:hover { color: var(--fg); }
  .link:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 3px; }
</style>
