<script lang="ts" module>
  // Which part of each item's page was looked at last, while the app is open (so Back from a note
  // lands on the list it was opened from).
  const lastTab = new Map<string, string>();
</script>

<script lang="ts">
  // One page per item — an idea, a client, a matter, a class, a project (FEAT-17): its title, status
  // and dates on top, then its overview (the front page), its own todos, meeting notes, notes,
  // deliverables and log. Everything comes from one call (`GET /api/library/item`); changes go
  // through the same service calls as the rest of the app, so the files keep their conventions.
  import { onMount, tick, untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import ItemActions from '../lib/ItemActions.svelte';
  import FindBar from '../lib/FindBar.svelte';
  import PrintPdf from '../lib/PrintPdf.svelte';
  import ItemTodos from './item/ItemTodos.svelte';
  import ItemFiles from './item/ItemFiles.svelte';
  import { ApiError, api, on, post } from '../lib/api';
  import { go, refreshChats } from '../lib/store.svelte';
  import { t, tn, fmtDate, fmtHijri, fmtNum } from '../lib/i18n';
  import { errDetail, failed } from '../lib/errors';
  import { hasHiddenText, md, mdClick, resolveLink } from '../lib/md';
  import { zoomImages } from '../lib/zoomImages';
  import { dueLabel, dueState, isoDay, notify, prettyName, rawBlobUrl } from '../lib/content/util';
  import { clock } from '../lib/clock.svelte';
  import { showHijri } from '../lib/weekPrefs.svelte';
  import { areaLabel, term } from '../lib/terms';
  import { itemOf, itemTerm, openItem, wordOf } from '../lib/item';
  import MeetingRecorder from '../lib/voice/MeetingRecorder.svelte';
  import MeetingNoteActions from '../lib/voice/MeetingNoteActions.svelte';
  import { isAudio, unpaired } from '../lib/voice.svelte';

  let { path, tab: first }: { path?: string; tab?: string } = $props();

  type Tab = 'overview' | 'todos' | 'meetings' | 'notes' | 'outputs' | 'log';
  const TABS: Tab[] = ['overview', 'todos', 'meetings', 'notes', 'outputs', 'log'];
  const isTab = (x: unknown): x is Tab => TABS.includes(x as Tab);
  const dir = $derived(itemOf(path ?? '', true) ?? String(path ?? ''));

  let data = $state<any>(null);
  let loading = $state(true);
  let loadError = $state('');
  let notFound = $state(false);
  let tab = $state<Tab>(untrack(() => (isTab(first) ? first : isTab(lastTab.get(dir)) ? (lastTab.get(dir) as Tab) : 'overview')));
  $effect(() => { const k = dir, v = tab; untrack(() => lastTab.set(k, v)); });

  let seq = 0;
  async function load() {
    const mine = ++seq;
    try {
      const v = await api(`/library/item?path=${encodeURIComponent(dir)}`);
      if (mine !== seq) return;
      data = v; loadError = ''; notFound = false;
    } catch (e) {
      if (mine !== seq) return;
      if (e instanceof ApiError && e.status === 404) { notFound = true; data = null; }
      else { loadError = errDetail(e) || String(e); if (data) failed(t('item.loadFailed'), e, { key: 'item-load' }); }
    } finally { if (mine === seq) loading = false; }
  }
  onMount(() => {
    load();
    // A change anywhere in the library (the assistant, a wrap, another view) refreshes the page; bursts settle into one.
    let timer: ReturnType<typeof setTimeout> | undefined;
    const off = on((e) => { if (['library.changed', 'todos.changed', 'resync'].includes(e.type)) { clearTimeout(timer); timer = setTimeout(load, 250); } });
    return () => { off(); clearTimeout(timer); };
  });

  const word = $derived(wordOf(data));
  const say = (form: string) => itemTerm(word, form);
  const title = $derived(String(data?.title || prettyName(dir + '/README.md')));
  // The front page usually starts with its title as a heading: the page already shows it above.
  const body = $derived.by(() => {
    const b = String(data?.readme?.body ?? '');
    const m = /^\s*#\s+(.+?)\s*#*\s*(\r?\n|$)/.exec(b);
    return m && m[1].trim().toLowerCase() === title.trim().toLowerCase() ? b.slice(m[0].length) : b;
  });

  // ----- status -----
  const ORDER = ['active', 'seed', 'parked', 'done', 'dropped'];
  const statusLabel = (s: string) => { const k = `idea.status.${s}`; const x = t(k); return x === k ? s : x; };
  let savingStatus = $state(false);
  async function setStatus(next: string, sel: HTMLSelectElement) {
    if (!data || !next || next === data.status || savingStatus) return;
    const prev = String(data.status ?? '');
    const p = data.path, name = title;
    savingStatus = true;
    data.status = next;
    try {
      await post('/library/ideas/status', { path: p, status: next });
      notify(t('todob.idea.statusSaved', { title: name, status: statusLabel(next) }), { kind: 'success', key: 'idea-status:' + p,
        action: prev ? { label: t('undo'), run: () => { post('/library/ideas/status', { path: p, status: prev }).then(load).catch((e) => failed(t('todob.idea.statusFailed'), e)); } } : undefined });
    } catch (e) { data.status = prev; sel.value = prev; failed(t('todob.idea.statusFailed'), e); }
    finally { savingStatus = false; }
  }

  // ----- dates -----
  const isDay = (d: any) => /^\d{4}-\d{2}-\d{2}/.test(String(d ?? ''));
  const when = (d: string) => fmtDate(String(d).slice(0, 10) + 'T12:00:00', { day: 'numeric', month: 'short', year: 'numeric' });
  const hijri = (d: string) => (showHijri() ? fmtHijri(String(d).slice(0, 10) + 'T12:00:00', { day: 'numeric', month: 'short', year: 'numeric' }) : '');
  const nextDue = $derived.by(() => { clock.day; return ((data?.todo?.items ?? []) as any[]).filter((i) => !i.done && i.due).sort((a, b) => String(a.due).localeCompare(String(b.due)))[0] ?? null; });
  const lastMeeting = $derived(((data?.meetings ?? []) as any[]).find((m) => isDay(m.date)) ?? null);
  const upcoming = $derived.by(() => { clock.day; const today = isoDay(); return ((data?.keyDates ?? []) as any[]).filter((k) => k.date >= today).slice(0, 3); });

  // ----- counts on the tabs -----
  // Meeting recordings (FEAT-55) are shown with the note made from them, or as waiting to be turned into text.
  const meetingNotes = $derived(((data?.meetings ?? []) as any[]).filter((m) => !isAudio(m.path)));
  const looseRecordings = $derived(unpaired(data?.meetings ?? []).length);
  const openTodos = $derived(((data?.todo?.items ?? []) as any[]).filter((i) => !i.done).length);
  const count = (k: Tab): number => !data ? 0 : k === 'todos' ? openTodos : k === 'meetings' ? meetingNotes.length + looseRecordings
    : k === 'notes' ? data.notes.length + data.other.length : k === 'outputs' ? data.outputs.length : k === 'log' ? data.log.entries.length : 0;

  async function pick(k: Tab, focus = false) {
    tab = k;
    if (focus) { await tick(); document.getElementById('itab-' + k)?.focus(); }
  }
  function onTabKey(e: KeyboardEvent) {
    const rtl = getComputedStyle(e.currentTarget as HTMLElement).direction === 'rtl';
    const i = TABS.indexOf(tab);
    let n: number;
    if (e.key === 'ArrowRight') n = rtl ? i - 1 : i + 1;
    else if (e.key === 'ArrowLeft') n = rtl ? i + 1 : i - 1;
    else if (e.key === 'Home') n = 0;
    else if (e.key === 'End') n = TABS.length - 1;
    else return;
    e.preventDefault();
    pick(TABS[(n + TABS.length) % TABS.length], true);
  }

  // ----- actions -----
  /** A new chat with this item's front page mentioned and a first line ready; nothing is sent until the user does. */
  async function continueChat() {
    if (!data) return;
    if (data.private) {
      notify(say('itemPrivate'), { kind: 'error', key: 'item-private', action: { label: t('item.privacySettings'), run: () => { go({ kind: 'settings', id: 'privacy' }); } } });
      return;
    }
    try {
      const c = await post('/chats', {});
      const p = String(data.path);
      // The chat view picks this up as the new chat's draft (the shape it keeps its own drafts in).
      try { localStorage.setItem(`thoughts.chatdraft:${c.id}`, JSON.stringify({ text: t('item.continueDraft', { mention: '@' + p }) + ' ', mentions: [p], files: [] })); } catch { /* storage unavailable: an empty chat */ }
      await refreshChats().catch(() => {});
      go({ kind: 'chat', id: c.id });
    } catch (e) { failed(t('item.continueFailed'), e); }
  }

  let meetingOpen = $state(false);
  let meetingTitle = $state('');
  let meetingDate = $state(isoDay());
  let meetingBusy = $state(false);
  async function startMeeting() {
    tab = 'meetings';
    meetingOpen = true;
    if (!meetingTitle) meetingDate = isoDay();
    await tick();
    document.getElementById('itm-title')?.focus();
  }
  /** The note is made in the item's meetings folder with a dated name, then opened in the editor. */
  async function createMeeting() {
    if (meetingBusy || !data) return;
    meetingBusy = true;
    try {
      const r = await post('/library/item/meeting', { path: data.dir, title: meetingTitle.trim(), date: meetingDate || '' });
      meetingOpen = false; meetingTitle = '';
      go({ kind: 'docs', path: r.path, id: 'edit' });
    } catch (e) { failed(t('item.meeting.failed'), e); }
    finally { meetingBusy = false; }
  }
  const editFront = () => data && go({ kind: 'docs', path: data.path, id: 'edit' });

  // ----- rendered notes: links and pictures -----
  /** A link inside the front page or the log opens there: another item's page, or the file in Docs.
   *  Web links are left to the shell, which asks before opening them. */
  function clickLink(e: MouseEvent, from: string) {
    if (mdClick(e)) return;
    const a = (e.target as HTMLElement).closest('a');
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('#')) return;
    e.preventDefault();
    const target = resolveLink(from, href);
    if (!target) return;
    const other = itemOf(target) ?? (/\.[a-z0-9]+$/i.test(target) ? null : itemOf(target, true));
    if (other) openItem(other, 'overview'); else go({ kind: 'docs', path: target });
  }
  let article = $state<HTMLElement>();
  let logEl = $state<HTMLElement>();
  const inline: string[] = [];
  const dropInline = () => { for (const u of inline.splice(0)) URL.revokeObjectURL(u); };
  $effect(() => {
    const el = article, p = data?.path;
    void body;
    dropInline();
    if (!el || !p) return;
    tick().then(() => {
      for (const img of Array.from(el.querySelectorAll('img'))) {
        const src = img.getAttribute('src') || '';
        if (!src || /^([a-z][a-z0-9+.-]*:|\/\/|#)/i.test(src)) continue;
        img.removeAttribute('src');
        rawBlobUrl(resolveLink(p, src)).then((u) => {
          if (!u) return;
          if (!img.isConnected) { URL.revokeObjectURL(u); return; }
          inline.push(u);
          img.src = u;
        });
      }
    });
  });
  onMount(() => dropInline);
</script>

<div class="page item">
  <nav class="crumbs" aria-label={t('item.backTo', { name: term('nav') })}>
    <button class="crumb" onclick={() => go({ kind: 'ideas' })}><Icon name="left" size={14} class="flip-rtl" /> {term('nav')}</button>
    {#if data?.area}<span class="sep" aria-hidden="true">/</span><span class="muted" dir="auto">{areaLabel(data.area)}</span>{/if}
  </nav>

  <div class="row head">
    <h1 class="grow" dir="auto">{title}</h1>
    {#if data?.private}<span class="pill priv" title={t('item.privateTip')}><Icon name="lock" size={12} /> {t('item.private')}<span class="sr-only"> — {t('item.privateTip')}</span></span>{/if}
    {#if data}<ItemActions target="idea" name={title} item={{ path: data.path, area: data.area, slug: data.slug, title: data.title, status: data.status, summary: data.summary, tags: data.tags, updated: data.updated }} />{/if}
  </div>

  {#if loading && !data}
    <div class="sk" aria-busy="true" role="status"><span class="sr-only">{t('item.loading')}</span><div class="skel w60"></div><div class="skel w40"></div><div class="skel box"></div></div>
  {:else if notFound}
    <div class="state" role="alert">
      <Icon name="search" size={26} />
      <h2>{t('item.notFound.title')}</h2>
      <p class="muted">{t('item.notFound.body')}</p>
      <div class="row wrap">
        <button class="btn primary" onclick={() => go({ kind: 'ideas' })}><Icon name="left" size={14} class="flip-rtl" /> {t('item.backTo', { name: term('nav') })}</button>
        <button class="btn" onclick={() => go({ kind: 'docs' })}><Icon name="docs" size={14} /> {t('docs')}</button>
      </div>
    </div>
  {:else if !data}
    <div class="state" role="alert">
      <Icon name="warning" size={26} />
      <p>{t('item.loadFailed')}</p>
      {#if loadError}<p class="muted sm" dir="auto">{loadError}</p>{/if}
      <button class="btn primary" onclick={() => { loading = true; load(); }}><Icon name="retry" size={14} /> {t('ui.retry')}</button>
    </div>
  {:else}
    {#if data.summary}<p class="summary" dir="auto">{data.summary}</p>{/if}

    <div class="row meta">
      <label class="stl" for="item-status">{t('item.status')}</label>
      <select id="item-status" class="stsel st-{data.status}" value={data.status ?? ''} disabled={savingStatus} title={t('todob.idea.statusHint')}
        aria-label={t('todob.idea.statusOf', { title })} onchange={(e) => setStatus(e.currentTarget.value, e.currentTarget)}>
        {#if !data.status}<option value="" disabled>{t('todob.idea.noStatus')}</option>{/if}
        {#each ORDER as st (st)}<option value={st}>{statusLabel(st)}</option>{/each}
        {#if data.status && !ORDER.includes(data.status)}<option value={data.status}>{data.status}</option>{/if}
      </select>
      {#each (data.tags ?? []).filter((x: any) => typeof x === 'string' && x.trim()) as tg (tg)}<span class="pill" dir="auto">#{tg}</span>{/each}
    </div>

    {#if isDay(data.created) || isDay(data.updated) || nextDue || lastMeeting || upcoming.length}
      <dl class="dates" aria-label={t('item.datesLabel')}>
        {#each upcoming as k, i (k.date + ':' + i)}
          <div class="d key"><dt><Icon name="calendar" size={12} /> {t('item.date.key')}</dt><dd><time datetime={k.date}>{when(k.date)}</time>{#if hijri(k.date)}{' '}<span class="muted hj" dir="auto">· {hijri(k.date)}</span>{/if}{#if k.text}{' · '}<span dir="auto">{k.text}</span>{/if}</dd></div>
        {/each}
        {#if nextDue}
          {@const s = dueState(nextDue.due)}
          <div class="d" class:over={s === 'overdue'}><dt><Icon name={s === 'overdue' ? 'warning' : 'todos'} size={12} /> {t('item.date.nextDue')}</dt>
            <dd><button class="lnk" onclick={() => pick('todos')}>{#if s === 'overdue'}<b>{t('todo.overdue')}</b>{' · '}{/if}{dueLabel(nextDue.due)}{' · '}<span dir="auto">{nextDue.title}</span></button></dd></div>
        {/if}
        {#if lastMeeting}<div class="d"><dt>{t('item.date.lastMeeting')}</dt><dd><button class="lnk" onclick={() => go({ kind: 'docs', path: lastMeeting.path })}><time datetime={lastMeeting.date}>{when(lastMeeting.date)}</time></button></dd></div>{/if}
        {#if isDay(data.created)}<div class="d"><dt>{t('item.date.created')}</dt><dd><time datetime={String(data.created).slice(0, 10)}>{when(data.created)}</time></dd></div>{/if}
        {#if isDay(data.updated)}<div class="d"><dt>{t('item.date.updated')}</dt><dd><time datetime={String(data.updated).slice(0, 10)}>{when(data.updated)}</time></dd></div>{/if}
      </dl>
    {/if}

    <div class="row actions">
      <button class="btn" class:primary={!data.private} onclick={continueChat} title={data.private ? say('itemPrivate') : t('item.continueTip')}><Icon name="chat" size={15} /> {t('item.continue')}</button>
      <button class="btn" onclick={startMeeting} aria-expanded={meetingOpen && tab === 'meetings'}><Icon name="plus" size={15} /> {t('item.newMeeting')}</button>
      <button class="btn ghost" onclick={editFront}><Icon name="edit" size={14} /> {t('edit')}</button>
    </div>
    {#if data.private}<p class="muted sm privnote" dir="auto"><Icon name="lock" size={12} /> {say('itemPrivate')}</p>{/if}

    <div class="tabs" role="tablist" aria-label={t('item.tabsLabel')} tabindex="-1" onkeydown={onTabKey}>
      {#each TABS as k (k)}
        {@const n = count(k)}
        <button id="itab-{k}" class="tab" role="tab" aria-selected={tab === k} aria-controls="ipanel-{k}" tabindex={tab === k ? 0 : -1} onclick={() => pick(k)}>
          {t('item.tab.' + k)}{#if n}<span class="n" aria-hidden="true">{fmtNum(n)}</span><span class="sr-only"> ({tn('item.tabCount', n)})</span>{/if}
        </button>
      {/each}
    </div>

    <div class="panel" id="ipanel-{tab}" role="tabpanel" aria-labelledby="itab-{tab}" tabindex="0">
      {#if tab === 'overview'}
        {#if data.detailsUnreadable}<p class="warn sm" role="note"><Icon name="warning" size={12} /> {t('todoa.work.unreadable')}</p>{/if}
        {#if !body.trim()}
          <div class="state">
            <Icon name="book" size={26} />
            <p class="lead" dir="auto">{say('itemOverviewEmpty')}</p>
            <p class="muted sm">{t('item.overview.emptyHint')}</p>
            <button class="btn primary" onclick={editFront}><Icon name="edit" size={14} /> {t('edit')}</button>
          </div>
        {:else}
          {#if hasHiddenText(body)}
            <div class="banner" role="note"><Icon name="eye" size={15} /><span class="grow">{t('docsa.hiddenBody')}</span>
              <button class="btn sm" onclick={() => go({ kind: 'docs', path: data.path })}>{t('item.openInDocs')}</button></div>
          {/if}
          <div class="row tools">
            <PrintPdf source={() => (article ? { content: article, title, heading: title } : null)} />
            <span class="grow"></span>
            <button class="btn ghost sm" onclick={() => go({ kind: 'docs', path: data.path })} title={t('item.openInDocsTip')}><Icon name="docs" size={13} /> {t('item.openInDocs')}</button>
            <button class="btn sm" onclick={editFront}><Icon name="edit" size={13} /> {t('edit')}</button>
          </div>
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
          <article class="md" dir="auto" bind:this={article} onclick={(e) => clickLink(e, data.path)} use:zoomImages>{@html md(body)}</article>
          <FindBar root={() => article} where={'doc:' + data.path} bottom />
        {/if}
      {:else if tab === 'todos'}
        <ItemTodos list={data.todo} dir={data.dir} {word} reload={load} />
      {:else if tab === 'meetings'}
        <MeetingRecorder item={{ dir: data.dir, title, private: !!data.private, meetings: data.meetings }} privateMsg={data.private ? say('itemPrivate') : null} reload={load} />
        {#if meetingOpen}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <form class="card newm" aria-label={t('item.meeting.formLabel')} onsubmit={(e) => { e.preventDefault(); createMeeting(); }}
            onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); meetingOpen = false; } }}>
            <div class="row wrap fields">
              <div class="grow f1">
                <label for="itm-title">{t('item.meeting.title')}</label>
                <input id="itm-title" bind:value={meetingTitle} placeholder={t('item.meeting.titlePh')} dir="auto" maxlength="120" />
              </div>
              <div class="f2">
                <label for="itm-date">{t('item.meeting.date')}</label>
                <input id="itm-date" type="date" bind:value={meetingDate} />
              </div>
            </div>
            <div class="row wrap btns">
              <button class="btn primary" disabled={meetingBusy} aria-busy={meetingBusy}><Icon name="edit" size={14} /> {t('item.meeting.create')}</button>
              <button class="btn" type="button" onclick={() => (meetingOpen = false)}>{t('cancel')}</button>
            </div>
          </form>
        {/if}
        {#snippet noteExtra(f: any)}<MeetingNoteActions note={f} files={data.meetings} privateMsg={data.private ? say('itemPrivate') : null} />{/snippet}
        {#if meetingNotes.length}
          <ItemFiles files={meetingNotes} mode="meetings" extra={noteExtra} />
        {:else if !meetingOpen && !looseRecordings}
          <div class="state">
            <Icon name="chats" size={26} />
            <p class="lead" dir="auto">{say('itemMeetingsEmpty')}</p>
            <p class="muted sm">{t('item.meetings.hint')}</p>
            <button class="btn primary" onclick={startMeeting}><Icon name="plus" size={14} /> {t('item.newMeeting')}</button>
          </div>
        {/if}
      {:else if tab === 'notes'}
        {#if data.notes.length}
          <ItemFiles files={data.notes} mode="notes" />
        {:else}
          <div class="state">
            <Icon name="docs" size={26} />
            <p class="lead" dir="auto">{say('itemNotesEmpty')}</p>
            <p class="muted sm">{t('item.notes.hint')}</p>
          </div>
        {/if}
        {#if data.other.length}
          <h2 class="sub">{t('item.other')}</h2>
          <ItemFiles files={data.other} mode="other" />
        {/if}
      {:else if tab === 'outputs'}
        {#if data.outputs.length}
          <ItemFiles files={data.outputs} mode="outputs" />
        {:else}
          <div class="state">
            <Icon name="download" size={26} />
            <p class="lead" dir="auto">{say('itemOutputsEmpty')}</p>
            <p class="muted sm">{t('item.outputs.hint')}</p>
          </div>
        {/if}
      {:else if tab === 'log'}
        {#if data.log.entries.length}
          <div class="log" bind:this={logEl}>
            {#each data.log.entries as en, i (i)}
              <article class="entry">
                <h3 dir="auto">
                  {#if en.date}<time datetime={en.date}>{when(en.date)}</time>{#if en.topic}<span class="muted" aria-hidden="true">{' — '}</span>{/if}{/if}{#if en.topic}<span dir="auto">{en.topic}</span>{:else if !en.date}<span>{t('item.log.undated')}</span>{/if}
                </h3>
                {#if String(en.body ?? '').trim()}
                  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                  <div class="md" dir="auto" onclick={(e) => clickLink(e, data.log.path)} use:zoomImages>{@html md(en.body)}</div>
                {/if}
              </article>
            {/each}
          </div>
          <div class="row tools">
            {#if data.log.archive}<button class="btn ghost sm" onclick={() => go({ kind: 'docs', path: data.log.archive })}><Icon name="rollback" size={13} /> {t('item.log.older')}</button>{/if}
            <PrintPdf source={() => (logEl ? { content: logEl, title: `${title} — ${t('item.tab.log')}`, heading: `${title} — ${t('item.tab.log')}` } : null)} />
            <span class="grow"></span>
            <button class="btn ghost sm" onclick={() => go({ kind: 'docs', path: data.log.path })}><Icon name="docs" size={13} /> {t('item.openInDocs')}</button>
          </div>
        {:else}
          <div class="state">
            <Icon name="rollback" size={26} />
            <p class="lead" dir="auto">{say('itemLogEmpty')}</p>
            <p class="muted sm" dir="auto">{say('itemLogHint')}</p>
            {#if data.log.archive}<button class="btn" onclick={() => go({ kind: 'docs', path: data.log.archive })}><Icon name="rollback" size={14} /> {t('item.log.older')}</button>{/if}
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .crumbs { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; font-size: 13px; margin-bottom: 8px; }
  .crumb { font: inherit; color: var(--accent-2, var(--accent)); background: none; border: 0; padding: 2px 4px; margin-inline-start: -4px; border-radius: 6px; cursor: pointer; display: inline-flex; align-items: center; gap: 2px; }
  .crumb:hover { text-decoration: underline; text-underline-offset: 3px; }
  .sep { color: var(--muted); }
  .head { align-items: flex-start; flex-wrap: wrap; }
  .head h1 { margin-bottom: 6px; overflow-wrap: anywhere; }
  .priv { display: inline-flex; align-items: center; gap: 4px; margin-top: 8px; }
  .summary { margin: 0 0 10px; color: var(--muted); font-size: 15px; max-width: 760px; overflow-wrap: anywhere; }
  .meta { flex-wrap: wrap; gap: 6px; margin-bottom: 10px; }
  .stl { margin: 0; font-size: 13px; }
  .stsel { width: auto; font-size: 13px; font-weight: 500; padding: 3px 10px; border-radius: 99px; cursor: pointer; }
  .stsel.st-active { background: var(--tint); color: var(--on-tint); }
  .dates { display: flex; flex-wrap: wrap; gap: 6px 18px; margin: 0 0 14px; padding: 10px 14px; border-radius: var(--radius); background: var(--fill); }
  .d { display: grid; gap: 1px; min-width: 0; }
  .d dt { font-size: var(--fs-xs); font-weight: 500; color: var(--muted); display: inline-flex; align-items: center; gap: 4px; }
  .d dd { margin: 0; font-size: 13.5px; overflow-wrap: anywhere; }
  .d.key dd { font-weight: 500; }
  .d.over dt, .d.over b { color: var(--danger); }
  .hj { font-size: var(--fs-xs); }
  .lnk { font: inherit; color: inherit; background: none; border: 0; padding: 0; cursor: pointer; text-align: start; text-decoration: underline; text-decoration-color: var(--hairline); text-underline-offset: 3px; overflow-wrap: anywhere; }
  .lnk:hover { text-decoration-color: currentColor; }
  .actions { flex-wrap: wrap; margin-bottom: 6px; }
  .privnote { display: flex; gap: 6px; align-items: center; margin: 4px 0 0; max-width: 640px; }
  .tabs { display: flex; flex-wrap: wrap; gap: 2px; margin-top: 16px; border-bottom: var(--border-w) solid var(--hairline); }
  .tab { font: inherit; font-size: 13.5px; font-weight: 500; color: var(--muted); background: none; border: 0; border-bottom: 2px solid transparent; margin-bottom: -1px; padding: 7px 10px; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; }
  .tab:hover { color: var(--fg); }
  .tab[aria-selected='true'] { color: var(--fg); border-bottom-color: var(--accent); }
  .tab .n { font-size: var(--fs-xs); font-weight: 500; padding: 0 7px; border-radius: 99px; background: var(--fill); color: var(--muted); }
  .tab[aria-selected='true'] .n { background: var(--tint); color: var(--on-tint); }
  .panel { padding-top: 14px; }
  .panel:focus-visible { outline-offset: 4px; }
  .tools { flex-wrap: wrap; margin-bottom: 4px; }
  .md { max-width: 820px; overflow-wrap: anywhere; }
  .banner { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; padding: 8px 12px; border-radius: var(--radius); background: var(--fill); margin-bottom: 10px; font-size: 13px; }
  .warn { display: inline-flex; align-items: center; gap: 4px; color: var(--muted); margin: 0 0 8px; }
  .state { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 8px; padding: 32px 0; color: var(--muted); }
  .state h2 { margin: 4px 0 0; color: var(--fg); }
  .state p { margin: 0; max-width: 480px; }
  .lead { color: var(--fg); font-weight: 500; }
  .wrap { flex-wrap: wrap; }
  .newm { margin-bottom: 12px; }
  .newm label { margin-top: 0; }
  .fields { align-items: flex-end; gap: 10px; }
  .f1 { min-width: min(260px, 100%); }
  .f2 input { width: auto; }
  .btns { margin-top: 10px; }
  .sub { font-size: 15px; margin: 20px 0 8px; }
  .log { display: grid; gap: 10px; max-width: 820px; }
  .entry { padding: 10px 14px; border-radius: var(--radius); background: var(--fill); }
  .entry h3 { margin: 0 0 6px; font-size: 14px; font-weight: 600; }
  .entry .md :global(p:last-child), .entry .md :global(ul:last-child) { margin-bottom: 0; }
  .sm { font-size: var(--fs-xs); }
  .btn.sm { padding: 3px 9px; }
  .sk { display: grid; gap: 10px; margin-top: 8px; }
  .skel { height: 14px; border-radius: 8px; }
  .skel.w60 { width: 60%; } .skel.w40 { width: 40%; }
  .skel.box { height: 160px; border-radius: var(--radius-lg); }
</style>
