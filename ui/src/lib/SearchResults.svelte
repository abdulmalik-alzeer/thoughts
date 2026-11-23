<script lang="ts">
  // The search page: notes and chats that contain every word, with the matches highlighted.
  // It is a place of its own in the Back / Forward history, so a result can be opened and returned from.
  import { onDestroy, onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import Marked from './Marked.svelte';
  import { t, tn, fmtNum, fmtDate } from './i18n';
  import { go, track } from './store.svelte';
  import { errDetail } from './errors';
  import { prettyName } from './content/util';
  import { itemOf } from './item';
  import { meaning, watchMeaning, isReady, mayOffer, searchByMeaning } from './meaning.svelte';
  import type { MeaningResult } from './types';
  import { cached, findOnOpen, findWords, markTerms, registerFind, searchAll, type ChatHit, type NoteHit, type Results } from './search';

  let { query }: { query: string } = $props();

  let q = $state(query);
  let res = $state<Results | null>(cached(query) ?? null);
  let loading = $state(false);
  let error = $state('');
  let show = $state<'all' | 'notes' | 'chats'>('all');
  let box = $state<HTMLInputElement>();
  let seq = 0;
  // Search by meaning, when it is ready: asked beside the word search, shown under its results.
  let related = $state<MeaningResult[]>([]);

  async function run(text: string) {
    const mine = ++seq;
    error = '';
    if (!text.trim()) { res = null; related = []; loading = false; return; }
    loading = true;
    const byMeaning = isReady() ? searchByMeaning(text).catch(() => [] as MeaningResult[]) : Promise.resolve([] as MeaningResult[]);
    try {
      const r = await searchAll(text);
      const m = await byMeaning;
      if (mine === seq) { res = r; related = m; track('search'); }
    } catch (e) {
      if (mine === seq) error = errDetail(e) || t('srch.failed');
    } finally { if (mine === seq) loading = false; }
  }

  let timer: ReturnType<typeof setTimeout> | undefined;
  function onInput() { clearTimeout(timer); timer = setTimeout(() => run(q), 250); }
  function submit(e: Event) {
    e.preventDefault();
    clearTimeout(timer);
    const v = q.trim();
    if (v && v !== query.trim()) go({ kind: 'docs', path: '?' + v }); else run(q);
  }

  /** Keep the words typed here in the history before leaving, so Back comes back to them. */
  async function open(v: { kind: string; id?: string; path?: string }, where: string, msg?: number) {
    const v0 = q.trim();
    if (v0 && v0 !== query.trim() && !(await go({ kind: 'docs', path: '?' + v0 }))) return;
    findOnOpen(where, findWords(v0 || query), msg);
    go(v);
  }
  // An item's front page opens as the item's page, its overview showing the words found.
  const openNote = (n: NoteHit) => open(itemOf(n.path) ? { kind: 'item', path: itemOf(n.path)!, id: 'overview' } : { kind: 'docs', path: n.path }, 'doc:' + n.path);
  const openChat = (c: ChatHit, msg?: number) => open({ kind: 'chat', id: c.id }, 'chat:' + c.id, msg);

  const notes = $derived(res?.notes ?? []);
  const chats = $derived(res?.chats ?? []);
  const terms = $derived(res?.terms ?? []);
  const total = $derived(notes.length + chats.length);
  // Notes already found by their words are not repeated.
  const relatedNotes = $derived(related.filter((r) => !notes.some((n) => n.path === r.path)));
  const words = $derived(findWords(q).split(' ').filter(Boolean).length);
  const setUp = () => go({ kind: 'settings', id: 'modules' });
  const noteTitle = (n: NoteHit) => n.title || prettyName(n.path);

  onMount(() => {
    watchMeaning();
    run(query); // with a cached list (Back), that list shows at once and is refreshed quietly
    box?.focus();
    const unregister = registerFind({ open: () => { box?.focus(); box?.select(); return true; } });
    return unregister;
  });
  onDestroy(() => clearTimeout(timer));
</script>

<div class="sr">
  <form class="qrow" role="search" onsubmit={submit}>
    <Icon name="search" size={16} />
    <input bind:this={box} bind:value={q} oninput={onInput} type="search" dir="auto" autocomplete="off" spellcheck="false"
      placeholder={t('srch.pagePh')} aria-label={t('srch.pagePh')} />
    {#if loading}<span class="spin" role="status" aria-label={t('srch.searching')}></span>{/if}
  </form>
  <p class="muted sm tip">{t('srch.tip')}</p>
  {#if mayOffer() && meaning.info?.suggest?.why === 'size'}
    <p class="muted sm tip offer">{t('meaning.large')} <button type="button" class="link" onclick={setUp}>{t('meaning.setUp')}</button></p>
  {/if}

  {#if error}
    <div class="state" role="alert"><Icon name="warning" size={24} /><p>{t('srch.failed')}</p><p class="muted sm" dir="auto">{error}</p>
      <button class="btn primary" onclick={() => run(q)}><Icon name="retry" size={14} /> {t('srch.retry')}</button></div>
  {:else if !q.trim()}
    <div class="state"><Icon name="search" size={24} /><p class="muted">{t('srch.empty')}</p></div>
  {:else if res && !total && !relatedNotes.length && !loading}
    <div class="state"><Icon name="search" size={24} /><p>{t('srch.none')}</p><p class="muted sm">{t('srch.noneTip')}</p>
      {#if words >= 2 && mayOffer()}<p class="muted sm">{t('meaning.tryIt')} <button type="button" class="link" onclick={setUp}>{t('meaning.setUp')}</button></p>{/if}</div>
  {:else if res}
    <div class="tabs" role="group" aria-label={t('srch.filter')}>
      {#each [['all', total], ['notes', notes.length], ['chats', chats.length]] as [k, n]}
        <button class="tab" aria-pressed={show === k} onclick={() => (show = k as any)} disabled={k !== 'all' && !n}>
          {t('srch.tab.' + k)} <span class="n">{fmtNum(n as number)}</span></button>
      {/each}
    </div>

    {#if show !== 'notes' && chats.length}
      <h2 class="sec">{t('srch.chats')}</h2>
      {#each chats as c (c.id)}
        <div class="hit">
          <button class="head" onclick={() => openChat(c, c.matches[0]?.msg)}>
            <Icon name="chat" size={15} />
            <span class="ttl" dir="auto"><Marked text={c.title} marks={markTerms(c.title, terms)} /></span>
            {#if c.updated}<span class="muted sm when">{fmtDate(c.updated)}</span>{/if}
          </button>
          {#each c.matches as m}
            <button class="snip" onclick={() => openChat(c, m.msg)}>
              <span class="who muted">{t(m.role === 'user' ? 'srch.you' : 'srch.assistant')}</span>
              <span dir="auto"><Marked text={m.text} marks={m.marks} /></span>
            </button>
          {/each}
        </div>
      {/each}
    {/if}

    {#if show !== 'chats' && notes.length}
      <h2 class="sec">{t('srch.notes')}</h2>
      {#each notes as n (n.path)}
        <div class="hit">
          <button class="head" onclick={() => openNote(n)}>
            <Icon name="docs" size={15} />
            <span class="ttl" dir="auto"><Marked text={noteTitle(n)} marks={markTerms(noteTitle(n), terms)} /></span>
            <bdi class="muted sm path"><Marked text={n.path} marks={markTerms(n.path, terms)} /></bdi>
          </button>
          {#each n.matches as m}
            <button class="snip" onclick={() => openNote(n)}>
              <span class="who muted" title={t('srch.line', { n: fmtNum(m.line ?? 0) })}>{fmtNum(m.line ?? 0)}</span>
              <span dir="auto"><Marked text={m.text} marks={m.marks} /></span>
            </button>
          {/each}
        </div>
      {/each}
    {/if}
    {#if show !== 'chats' && relatedNotes.length}
      <h2 class="sec" title={t('meaning.relatedTip')}>{t('meaning.related')}</h2>
      {#each relatedNotes as r (r.path)}
        <div class="hit">
          <button class="head" onclick={() => openNote({ path: r.path, title: r.title, matches: [] })}>
            <Icon name="docs" size={15} />
            <span class="ttl" dir="auto">{r.title || prettyName(r.path)}</span>
            <bdi class="muted sm path">{r.path}</bdi>
          </button>
          {#if r.snippet}
            <button class="snip" onclick={() => openNote({ path: r.path, title: r.title, matches: [] })}>
              <span class="who muted" title={t('srch.line', { n: fmtNum(r.line ?? 0) })}>{fmtNum(r.line ?? 0)}</span>
              <span dir="auto">{r.snippet}</span>
            </button>
          {/if}
        </div>
      {/each}
    {/if}
    {#if total >= 40}<p class="muted sm more">{t('srch.more')}</p>{/if}
    <p class="sr-only" role="status">{tn('srch.count', total + (show !== 'chats' ? relatedNotes.length : 0))}</p>
  {:else}
    <div class="pad" role="status" aria-busy="true"><span class="sr-only">{t('srch.searching')}</span><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div></div>
  {/if}
</div>

<style>
  .sr { max-width: 820px; padding-top: 18px; }
  .qrow { display: flex; align-items: center; gap: 10px; padding: 8px 12px; border: var(--border-w, 1px) solid var(--border); border-radius: var(--radius-lg, 12px); background: var(--panel); color: var(--muted); }
  .qrow input { flex: 1; min-width: 0; border: 0; background: transparent; box-shadow: none; font-size: 17px; padding: 2px 0; color: var(--fg); }
  .tip { margin: 6px 2px 14px; }
  .tabs { display: flex; gap: 6px; margin: 4px 0 10px; flex-wrap: wrap; }
  .tab { font: inherit; font-size: var(--fs-sm); background: var(--fill); color: var(--fg); border: 0; border-radius: 99px; padding: 4px 12px; cursor: pointer; }
  .tab[aria-pressed="true"] { background: var(--tint); color: var(--on-tint); font-weight: 600; }
  :global([data-theme="contrast"]) .tab[aria-pressed="true"] { background: var(--accent); color: var(--accent-fg); }
  .tab:disabled { opacity: .5; cursor: default; }
  .tab .n { opacity: .75; margin-inline-start: 4px; font-variant-numeric: tabular-nums; }
  .sec { font-size: var(--fs-sm); text-transform: none; color: var(--muted); margin: 18px 0 6px; font-weight: 600; }
  .hit { border-bottom: 1px solid var(--hairline); padding: 8px 0; }
  .head, .snip { font: inherit; color: inherit; background: none; border: 0; width: 100%; text-align: start; cursor: pointer; display: flex; gap: 8px; align-items: baseline; border-radius: 8px; padding: 4px 8px; }
  .head:hover, .snip:hover { background: var(--fill); }
  .head :global(.icon) { flex: none; align-self: center; color: var(--muted); }
  .ttl { font-weight: 600; min-width: 0; overflow-wrap: anywhere; }
  .path { margin-inline-start: auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 45%; }
  .when { margin-inline-start: auto; white-space: nowrap; }
  .snip { font-size: var(--fs-sm); line-height: 1.5; }
  .snip > span:last-child { min-width: 0; overflow-wrap: anywhere; }
  .who { flex: none; min-width: 3.2em; font-size: var(--fs-xs); font-variant-numeric: tabular-nums; }
  .state { display: grid; justify-items: center; gap: 6px; padding: 40px 0; text-align: center; color: var(--muted); }
  .state p { margin: 0; }
  .more { margin-top: 14px; }
  .offer { margin-top: -8px; }
  .link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent); cursor: pointer; text-decoration: underline; }
  .spin { width: 14px; height: 14px; border-radius: 50%; border: 2px solid var(--hairline); border-top-color: var(--accent); animation: srspin .8s linear infinite; flex: none; }
  @keyframes srspin { to { transform: rotate(360deg); } }
  .skel { height: 13px; margin: 8px 0; border-radius: 8px; background: var(--fill); }
  .skel.h { height: 22px; width: 40%; }
  .skel.w70 { width: 70%; }
  @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
</style>
