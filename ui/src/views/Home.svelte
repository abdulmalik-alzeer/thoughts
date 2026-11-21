<script lang="ts">
  import { onMount } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { app, go, refreshChats, toast, track } from '../lib/store.svelte';
  import { startCustomise } from '../lib/layer';
  import { t, tn, ago, fmtDate, fmtHijri, fmtNum, lang } from '../lib/i18n';
  import { showHijri } from '../lib/weekPrefs.svelte';
  import { inSentence } from '../lib/todoline';
  import { dueLabel as dueLabelOn, dueState as dueStateOn, normTodo } from '../lib/content/util';
  import { clock } from '../lib/clock.svelte';
  import { failed } from '../lib/errors';
  import { isWorkItem, newItemTrigger, term, termN } from '../lib/terms';
  import { openItem } from '../lib/item';
  import { capturePrefix, captureKind, normalizeCapture } from '../lib/capture';
  import Icon from '../lib/Icon.svelte';
  import Emblem from '../lib/Emblem.svelte';
  import { imeKey } from '../lib/ime';
  import { inbox } from '../lib/inbox.svelte';
  import { homeBoxes, ownTitle, pageIntro, pageText, pageTitle, pageWarnings, placedElsewhere, shows, type Size } from '../lib/pages.svelte';
  import Sections from './Sections.svelte';
  import UsageTile from './usage/UsageTile.svelte';
  import { surfaceIn } from '../lib/surfaceIn';
  import { listedChats } from '../lib/mado/chats';

  let { projects, newChat, startWrap }: { projects: any[]; newChat: (pid?: string | null) => void; startWrap: () => void } = $props();

  let ideas = $state<any[]>([]);
  let lists = $state<any[]>([]);
  let files = $state(0);
  // What is in the box survives looking something up and coming back (and a restart), like a chat draft.
  const DRAFT_KEY = 'thoughts.homedraft';
  const DRAFT_MAX = 20000;
  function readDraft(): { text: string; chip: boolean } {
    try {
      const d = JSON.parse(localStorage.getItem(DRAFT_KEY) || 'null');
      if (d && typeof d.text === 'string') return { text: d.text.slice(0, DRAFT_MAX), chip: !!d.chip };
    } catch { /* no draft */ }
    return { text: '', chip: false };
  }
  const draft = readDraft();
  let ask = $state(draft.text);
  // True from the moment a chip put its own text in the box until the box is empty again: such a
  // message is not one the user wrote from scratch, so it never counts as `byUser`.
  let chipFilled = $state(draft.chip);
  $effect(() => { if (!ask.trim()) chipFilled = false; });
  $effect(() => {
    const text = ask.slice(0, DRAFT_MAX), chip = chipFilled;
    try {
      if (text.trim()) localStorage.setItem(DRAFT_KEY, JSON.stringify({ text, chip }));
      else localStorage.removeItem(DRAFT_KEY);
    } catch { /* storage full or unavailable: the draft just isn't kept */ }
  });
  // From pressing send until the new chat opens: a second Enter, click or chip does nothing.
  let sending = $state(false);
  let askEl: HTMLTextAreaElement;

  // Nothing is shown as "0" until it has really been counted: placeholders while loading, a notice when it failed.
  let loading = $state(true);
  let loadError = $state('');
  let partFailed = $state<Record<string, boolean>>({});

  async function load() {
    const [i, td, st] = await Promise.allSettled([api('/library/ideas'), api('/todos'), api('/library/stats')]);
    if (i.status === 'fulfilled') ideas = Array.isArray(i.value) ? i.value : [];
    if (td.status === 'fulfilled') lists = Array.isArray(td.value) ? td.value : [];
    // `notes` leaves out the library's own system files; older services only send `markdownFiles`.
    if (st.status === 'fulfilled') files = Number(st.value?.notes ?? st.value?.markdownFiles) || 0;
    partFailed = { ideas: i.status === 'rejected', todos: td.status === 'rejected', files: st.status === 'rejected' };
    const bad = [i, td, st].find((r) => r.status === 'rejected') as PromiseRejectedResult | undefined;
    loadError = bad ? String(bad.reason?.message ?? bad.reason ?? '') || t('ui.home.loadFailed') : '';
    loading = false;
  }
  const retryLoad = () => { loading = true; load(); };
  // ----- suggestions ("learns how you work"): local heuristics from the background service, at most two -----
  let suggestions = $state<any[]>([]);
  let sugBusy = $state('');
  const signalsOn = $derived(app.settings?.usageSignals !== false);
  async function loadSuggestions() {
    if (!signalsOn) { suggestions = []; return; }
    try { const r = await api('/suggestions'); suggestions = (Array.isArray(r) ? r : []).slice(0, 2); } catch { suggestions = []; }
  }
  const dismissSuggestion = (id: string) => post(`/suggestions/${encodeURIComponent(id)}/dismiss`);
  async function notNow(sg: any) {
    suggestions = suggestions.filter((x) => x.id !== sg.id);
    try { await dismissSuggestion(sg.id); } catch (e) { failed(t('ui.home.dismissFailed'), e); }
  }
  async function setUp(sg: any) {
    if (sugBusy) return;
    sugBusy = sg.id;
    try {
      await startCustomise(sg.prompt, sg.title);
      suggestions = suggestions.filter((x) => x.id !== sg.id);
      dismissSuggestion(sg.id).catch(() => {});
    } catch (e) { failed(t('layer.askFailed'), e); }
    finally { sugBusy = ''; }
  }

  // ----- layout from home.json: hidden sections and their order -----
  // `order` may also place `suggestions`, `automate` (the quiet Suggest automations link) and single plugin
  // widgets (`widget:<id>`); anything not mentioned keeps its usual place (suggestions first, widgets, then automate last).
  const SECTIONS = ['stats', 'usage', 'ideas', 'upcoming', 'todos', 'chats', 'projects'];
  const widgetKey = (w: { id: string }) => 'widget:' + w.id;
  // Home's usual order (home.json `order` / `hidden` still apply), then `pages.json` `home.sections`
  // on top (Sections): order, hidden, sizes, a section replaced by an add-on box, boxes between.
  const homeOrder = $derived.by(() => {
    const hidden = new Set(app.home.hidden);
    // A box placed on another page shows on Home only when Home places it here too.
    const elsewhere = placedElsewhere();
    const own = homeBoxes();
    const widgets = app.homeWidgets.map(widgetKey).filter((k) => !elsewhere.has(k) || app.home.order.includes(k) || own.includes(k));
    const known = (k: string) => SECTIONS.includes(k) || k === 'suggestions' || k === 'automate' || widgets.includes(k);
    const asked = app.home.order.filter(known);
    return ['hero', ...[...new Set([...(asked.includes('suggestions') ? [] : ['suggestions']), ...asked, ...SECTIONS, ...widgets, 'automate'])].filter((k) => !hidden.has(k))];
  });
  // Todos and chats share a row when they are neighbours (as Home always had them).
  const homeSizes = $derived.by(() => {
    const o = homeOrder, out: Record<string, Size> = {};
    for (let i = 0; i < o.length - 1; i++) if ((o[i] === 'todos' && o[i + 1] === 'chats') || (o[i] === 'chats' && o[i + 1] === 'todos')) { out.todos = 'half'; out.chats = 'half'; }
    return out;
  });
  const homeParts = $derived(Object.fromEntries(homeOrder.filter((k) => !k.startsWith('widget:')).map((k) => [k, partFor(k)])) as Record<string, any>);
  const partFor = (k: string) => ({ hero: heroS, stats: statsS, usage: usageS, ideas: ideasS, upcoming: upcomingS, todos: todosS, chats: chatsS, projects: projectsS, suggestions: suggestionsS, automate: automateS } as Record<string, any>)[k];

  /** Mounts a plugin's widget frame into the slot. */
  function plug(el: HTMLElement, w: { render: (el: HTMLElement) => any }) {
    let cleanup: any;
    try { cleanup = w.render(el); } catch (e) { console.error(e); }
    return { destroy() { try { if (typeof cleanup === 'function') cleanup(); } catch { /* frame already gone */ } } };
  }

  // A quick note starts with "note:" or «ملاحظة:» (only here: it never reaches the assistant).
  const NOTE = /^\s*(?:note|ملاحظ[ةه])\s*[:：]\s*/i;
  const notePrefix = () => (lang() === 'ar' ? 'ملاحظة: ' : 'note: ');
  /** What kind of quick capture the text is: a todo, a note, or neither (an ordinary message). */
  const quickKind = (text: string): 'todo' | 'note' | null => (NOTE.test(text) ? 'note' : captureKind(text) === 'todo' ? 'todo' : null);
  const quickBody = (text: string) => (NOTE.test(text) ? text.replace(NOTE, '') : normalizeCapture(text).replace(/^todo:\s*/i, '')).trim();

  /** Capture chips: with nothing typed they put the prefix in the box for the user to finish;
   *  otherwise they save what is typed straight into the library — no chat, nothing to wrap. */
  function capture(kind: 'todo' | 'note') {
    if (sending) return;
    track(kind === 'todo' ? 'home.chip:todo' : 'home.chip:note');
    const typed = ask.trim();
    const body = quickKind(typed) ? quickBody(typed) : typed;
    if (!body) {
      ask = kind === 'todo' ? capturePrefix('todo', lang()) : notePrefix();
      askEl?.focus();
      requestAnimationFrame(() => askEl?.setSelectionRange(ask.length, ask.length));
      return;
    }
    saveQuick(kind, body);
  }

  /** Write a todo or a note straight to the library. The toast offers Undo (todo) or taking the
   *  note to the assistant (one click: a chat that files it as a new item). */
  async function saveQuick(kind: 'todo' | 'note', body: string) {
    if (sending) return;
    sending = true;
    try {
      const r = await post('/capture', { kind, text: body });
      ask = '';
      if (kind === 'todo') {
        track('todo.add');
        const msg = r?.due ? t('todob.home.todoDue', { date: inSentence(dueLabelOn(r.due)) }) : t('todob.home.todoSaved');
        const undo = { file: r.file, line: r.line, text: r.text };
        toast(msg, { kind: 'success', action: { label: t('undo'), run: () => { post('/todos/delete', undo).then(load).catch((e) => failed(t('ui.todo.undoFailed'), e)); } } });
      } else {
        track('home.note');
        toast(t('todob.home.noteSaved'), { kind: 'success', action: { label: t('todob.home.think'), run: () => { think(body, r?.path); } } });
      }
      load();
    } catch (e) { failed(t(kind === 'todo' ? 'ui.todo.addFailed' : 'todob.home.noteFailed'), e); }
    finally { sending = false; }
  }

  /** Think a saved note through with the assistant: it is filed as a new item (in the user's own
   *  word, "new client: …"), so the copy waiting in the inbox is no longer needed. */
  async function think(body: string, path?: string) {
    try {
      const c = await post('/chats', { byUser: false });
      await post(`/chats/${c.id}/send`, { text: newItemTrigger() + body });
      if (path) post('/library/delete', { path }).catch(() => {});
      await refreshChats().catch(() => {});
      go({ kind: 'chat', id: c.id });
    } catch (e) { failed(t('ui.home.sendFailed'), e); }
  }

  /** Chips from customisations and add-ons only fill the box — the user sees the whole request and
   *  sends it themselves, so no click can start a run the user has not read. Text already typed is kept below it. */
  function useChip(c: { label: string; prompt: string }) {
    if (sending) return;
    track('home.chip:' + c.label);
    const typed = ask.trim();
    ask = !typed ? c.prompt : /\s$/.test(c.prompt) ? c.prompt + typed : c.prompt + '\n\n' + typed;
    chipFilled = true;
    askEl?.focus();
    requestAnimationFrame(() => askEl?.setSelectionRange(ask.length, ask.length));
  }

  onMount(() => { load(); loadSuggestions(); return on((e) => { if (['todos.changed', 'library.changed', 'chat.done'].includes(e.type)) load(); }); });

  // The greeting and the date follow the shared clock, so a window left open overnight catches up.
  const hour = $derived(clock.hour);
  // Arabic says «مساء الخير» from mid-afternoon; «طاب نهارك» covers noon to three. English keeps "afternoon" until six.
  const greeting = $derived(t(hour < 12 ? 'goodMorning' : hour < 15 || (hour < 18 && lang() !== 'ar') ? 'goodAfternoon' : 'goodEvening'));
  const today = $derived(fmtDate(clock.day + 'T12:00:00', { weekday: 'long', day: 'numeric', month: 'long' }) + (showHijri() ? ' · ' + fmtHijri(clock.day + 'T12:00:00', { day: 'numeric', month: 'long' }) : ''));
  const dueState = (d: string) => { clock.day; return dueStateOn(d); };
  const dueLabel = (d: string) => { clock.day; return dueLabelOn(d); };

  // Only what is really in progress: new items ("seed") are not active yet. With a work word
  // ("Active clients"), only work items count — not ideas or reference pages.
  const active = $derived(ideas.filter((i) => i.status === 'active' && isWorkItem(i)));
  const recentIdeas = $derived([...ideas].sort((a, b) => String(b.updated).localeCompare(String(a.updated))).slice(0, 4));
  const openTodos = $derived(lists.flatMap((l) => l.items.filter((i: any) => !i.done).map((i: any) => ({ ...normTodo(i), file: l.file, idea: l.idea }))));
  // Next few dated todos, earliest first — so anything overdue leads the list.
  const upcoming = $derived(openTodos.filter((i) => i.due).sort((a, b) => a.due!.localeCompare(b.due!)).slice(0, 5));
  const statusLabel = (s: string) => { const k = `idea.status.${s}`; const x = t(k); return x === k ? s : x; };
  const recentChats = $derived(app.chats.filter((c) => c.kind === 'chat').slice(0, 5));

  async function submit(prefix = '', typed = true) {
    if (sending) return;
    const raw = (prefix + ask).trim();
    // "todo: …" and "note: …" are saved straight away; everything else is a message to the assistant.
    const quick = prefix ? null : quickKind(raw);
    if (quick && quickBody(raw)) return saveQuick(quick, quickBody(raw));
    const text = normalizeCapture(raw);
    if (!text || quick) return askEl?.focus();
    sending = true;
    // Only that something was typed — never the words themselves (they stay in the chat, nowhere else).
    if (typed && !prefix) track('home.prompt:');
    try {
      // Only what the user typed themselves counts as a chat they started (see `inherited_mode` in the service).
      const c = await post('/chats', { byUser: typed && !prefix && !chipFilled });
      await post(`/chats/${c.id}/send`, { text });
      ask = '';
      await refreshChats().catch(() => {});
      go({ kind: 'chat', id: c.id });
    } catch (e) { failed(t('ui.home.sendFailed'), e); }
    finally { sending = false; }
  }

  /** When the change could not be saved the box is unticked again, so it shows what is really in the list. */
  async function toggle(it: any, box?: HTMLInputElement) {
    const ref = { file: it.file, line: it.line, text: it.text };
    try {
      const res: any = await post('/todos/toggle', ref);
      track('todo.done');
      if (res?.next) {
        // A repeating todo stays open on its next date: its box shows that again, and Undo puts the old date back.
        if (box) box.checked = false;
        toast(t('todob.repeatNext', { date: inSentence(dueLabel(res.next)) }), { key: 'todo-done', action: { label: t('undo'), run: () => { post('/todos/edit', { file: it.file, line: res.line, text: res.text, new_text: res.previous }).then(load).catch((e) => failed(t('ui.todo.undoFailed'), e)); } } });
      } else toast(t('ui.todo.completed'), { key: 'todo-done', action: { label: t('undo'), run: () => { post('/todos/toggle', ref).then(load).catch((e) => failed(t('ui.todo.undoFailed'), e)); } } });
    } catch (e) { failed(t('ui.todo.toggleFailed'), e); if (box) box.checked = false; }
    load();
  }

  /** A project card opens the latest chat in that folder; a new one only when there is none (or on "New chat"). */
  function openProject(p: any) {
    const latest = app.chats.filter((c) => c.projectId === p.id && c.kind === 'chat').sort((a, b) => String(b.updated).localeCompare(String(a.updated)))[0];
    if (latest) go({ kind: 'chat', id: latest.id }); else newChat(p.id);
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !imeKey(e)) { e.preventDefault(); if (!sending) submit(); }
  }
</script>

{#snippet statsS()}
  <section class="stats" aria-busy={loading}>
    {#if loading}
      <span class="sr-only" role="status">{t('shell.loading')}</span>
      {#each [0, 1, 2, 3] as i (i)}<div class="stat" aria-hidden="true"><div class="skel num"></div><div class="skel lbl"></div></div>{/each}
    {:else}
      {#snippet num(n: number, bad: boolean | undefined)}<b>{#if bad}<span aria-hidden="true">–</span><span class="sr-only">{t('ui.home.unknown')}</span>{:else}{fmtNum(n)}{/if}</b>{/snippet}
      <button class="stat" onclick={() => go({ kind: 'ideas' })}>{@render num(active.length, partFailed.ideas)}<span>{termN('active', active.length)}</span></button>
      <button class="stat" onclick={() => go({ kind: 'todos' })}>{@render num(openTodos.length, partFailed.todos)}<span>{tn('ui.home.openTodos', openTodos.length)}</span></button>
      <button class="stat" class:attn={app.wrap.count > 0} onclick={() => (app.wrap.count ? startWrap() : go({ kind: 'chats' }))}>
        <b>{#if app.wrap.count}{fmtNum(app.wrap.count)}{:else}<Icon name="check" size={26} />{/if}</b><span>{app.wrap.count ? tn('ui.home.toWrap', app.wrap.count) : t('allSaved')}</span></button>
      <button class="stat" onclick={() => go({ kind: 'docs' })}>{@render num(files, partFailed.files)}<span>{tn('ui.home.notes', files)}</span></button>
      {#if inbox.count > 0}<button class="stat attn" onclick={() => go({ kind: 'inbox' })}><b>{fmtNum(inbox.count)}</b><span>{tn('inbox.home', inbox.count)}</span></button>{/if}
    {/if}
  </section>
  {#if loadError && !loading}
    <div class="loaderr surface" role="alert">
      <Icon name="warning" size={16} />
      <span class="grow">{t('ui.home.loadFailed')} <span class="muted" dir="auto">{loadError}</span></span>
      <button class="btn" onclick={retryLoad}><Icon name="retry" size={14} /> {t('ui.retry')}</button>
    </div>
  {/if}
{/snippet}

{#snippet ideasS()}
  <section class="block">
    <div class="bh">
      <div><h2>{pageText('home', 'ideasTitle', t('continueWhere'))}</h2><p class="muted">{pageText('home', 'ideasIntro', term('continueSub'))}</p></div>
      <button class="more" onclick={() => go({ kind: 'ideas' })} aria-label={t('ux-small.seeAll', { what: pageText('home', 'ideasTitle', t('continueWhere')) })}>{t('seeAll')} <Icon name="right" size={14} class="flip-rtl" /></button>
    </div>
    <div class="ideas">
      {#each recentIdeas as i (i.path)}
        <button class="idea surface lit" onclick={() => openItem(i.path)}>
          <span class="badge"><Icon name="ideas" size={12} />{statusLabel(i.status)}</span>
          <span class="glyph" aria-hidden="true">{(i.title || '?').trim()[0]}</span>
          <b class="ell" dir="auto">{i.title}</b>
          <span class="muted clamp" dir="auto">{i.summary}</span>
        </button>
      {:else}
        {#if loading}
          {#each [0, 1, 2, 3] as i (i)}<div class="skel card-skel" aria-hidden="true"></div>{/each}
        {:else}
          <div class="empty surface"><Icon name="ideas" size={22} /><span>{term('homeEmpty')}</span><span class="hint">{t('ui.home.ideasHint')}</span></div>
        {/if}
      {/each}
    </div>
  </section>
{/snippet}

{#snippet upcomingS()}
  {#if upcoming.length}
    <section class="block up">
      <div class="surface panel">
        <div class="ph"><div class="phl"><div class="eyebrow">{t('home.upcomingEyebrow')}</div><h2>{pageText('home', 'upcomingTitle', t('home.upcoming'))}</h2></div>
          <button class="more" onclick={() => go({ kind: 'todos' })} aria-label={t('ux-small.seeAll', { what: pageText('home', 'upcomingTitle', t('home.upcoming')) })}>{t('seeAll')} <Icon name="right" size={14} class="flip-rtl" /></button></div>
        {#each upcoming as it (it.file + it.line)}
          {@const s = dueState(it.due!)}
          <label class="todo">
            <input type="checkbox" onchange={(e) => toggle(it, e.currentTarget)} />
            <span class="grow" dir="auto">{it.title}</span>
            {#if it.idea}<span class="muted when ell"><bdi>{it.idea.split('/').pop()}</bdi></span>{/if}
            <span class="due {s}"><Icon name={s === 'overdue' ? 'warning' : 'calendar'} size={12} />{#if s === 'overdue'}<b>{t('todo.overdue')}</b> ·{/if} {dueLabel(it.due!)}</span>
          </label>
        {/each}
      </div>
    </section>
  {/if}
{/snippet}

{#snippet panel(k: string)}
  {#if k === 'todos'}
    <div class="surface panel">
      <div class="ph"><h2>{pageText('home', 'todosTitle', t('todos'))}</h2><button class="more" onclick={() => go({ kind: 'todos' })} aria-label={t('ux-small.seeAll', { what: pageText('home', 'todosTitle', t('todos')) })}>{t('seeAll')} <Icon name="right" size={14} class="flip-rtl" /></button></div>
      {#each openTodos.slice(0, 7) as it (it.file + it.line)}
        <label class="todo">
          <input type="checkbox" onchange={(e) => toggle(it, e.currentTarget)} />
          <span class="grow" dir="auto">{it.title}</span>
          {#if it.idea}<span class="muted when ell"><bdi>{it.idea.split('/').pop()}</bdi></span>{/if}
        </label>
      {:else}
        {#if loading}<div class="skel line"></div><div class="skel line w70"></div><div class="skel line w50"></div>
        {:else}<div class="empty"><Icon name="check" size={20} /><span>{t('nothingOpen')}</span><span class="hint">{t('ui.home.todosHint')}</span></div>{/if}
      {/each}
    </div>
  {:else}
    <div class="surface panel">
      <div class="ph"><h2>{pageText('home', 'chatsTitle', t('recentChats'))}</h2><button class="more" onclick={() => newChat()}><Icon name="plus" size={14} /> {t('newChat')}</button></div>
      {#each recentChats as c (c.id)}
        <button class="rowbtn" onclick={() => go({ kind: 'chat', id: c.id })}>
          <Icon name="chat" size={15} />
          <span class="grow ell" dir="auto">{c.title}</span>
          {#if c.unwrapped}<span class="dot" role="img" aria-label={t('home.notSaved')} title={t('home.notSaved')}></span>{/if}
          <span class="muted when">{ago(c.updated)}</span>
        </button>
      {:else}
        <div class="empty"><Icon name="chats" size={20} /><span>{t('noChats')}</span><span class="hint">{t('ui.home.chatsHint')}</span></div>
      {/each}
    </div>
  {/if}
{/snippet}

{#snippet suggestionsS()}
  {#if suggestions.length}
    <section class="sugs" aria-label={t('layer.sug.label')}>
      {#each suggestions as sg (sg.id)}
        <div class="sug surface">
          <span class="sgi"><Icon name="wrap" size={16} /></span>
          <div class="grow">
            <b dir="auto">{sg.title}</b>
            <p class="muted" dir="auto">{sg.body}</p>
          </div>
          <div class="row sga">
            <button class="btn ghost" disabled={sugBusy === sg.id} onclick={() => notNow(sg)}>{t('layer.sug.notNow')}</button>
            <button class="btn primary" disabled={!!sugBusy} aria-busy={sugBusy === sg.id} onclick={() => setUp(sg)}>{t('layer.sug.setUp')}</button>
          </div>
        </div>
      {/each}
    </section>
  {/if}
{/snippet}

{#snippet widgetS(w: any)}
  <section class="block widget" use:surfaceIn>
    <div class="surface panel">
      <div class="ph"><h2 dir="auto">{w.title}</h2></div>
      <div class="wslot" use:plug={w}></div>
    </div>
  </section>
{/snippet}

{#snippet projectsS()}
  {#if app.settings?.modules?.builds !== false}
    <section class="block">
      <div class="bh">
        <div><h2>{pageText('home', 'projectsTitle', t('projects'))}</h2><p class="muted">{t('noProjects')}</p></div>
        <button class="more" onclick={() => go({ kind: 'projects' })} aria-label={t('ux-small.manage', { what: pageText('home', 'projectsTitle', t('projects')) })}>{t('manage')} <Icon name="right" size={14} class="flip-rtl" /></button>
      </div>
      <div class="projects">
        {#each projects.slice(0, 4) as p (p.id)}
          <div class="pcell">
            <button class="proj surface lit" title={t('ui.home.openProject', { name: p.name })} onclick={() => openProject(p)}>
              <Icon name="folder" size={18} />
              <b class="ell" dir="auto">{p.name}</b>
              <span class="muted when">{tn('home.chatCount', listedChats(app.chats).filter((c) => c.projectId === p.id).length)}</span>
            </button>
            <button class="pnew" aria-label={t('shell.newChatIn', { name: p.name })} title={t('shell.newChatIn', { name: p.name })} onclick={() => newChat(p.id)}><Icon name="plus" size={15} /></button>
          </div>
        {/each}
        <button class="proj add" onclick={() => go({ kind: 'projects' })}><Icon name="plus" size={18} /><span>{t('linkFolder')}</span></button>
      </div>
    </section>
  {/if}
{/snippet}

{#snippet heroS()}
  <section class="hero">
    <div class="lead">
      {#if shows('home', 'date')}<div class="eyebrow">{today}</div>{/if}
      {#if shows('home', 'greeting')}<h1 class="display" dir="auto">{ownTitle('home') ? pageTitle('home').replaceAll('{greeting}', greeting).replaceAll('{name}', app.settings?.userName ?? '') : app.settings?.userName ? t('coord.greetName', { greeting, name: app.settings.userName }) : t('coord.greet', { greeting })}</h1>{/if}
      {#if shows('home', 'intro')}<p class="sub" dir="auto">{pageIntro('home') || t('heroSub')}</p>{/if}
      <div class="ask surface">
        <textarea bind:this={askEl} bind:value={ask} onkeydown={keydown} rows="2" placeholder={pageText('home', 'askPlaceholder', t('askPlaceholder'))} aria-label={pageText('home', 'askPlaceholder', t('askPlaceholder'))} dir="auto"></textarea>
        <div class="row chips">
          {#if shows('home', 'todoChip')}<button class="chip" disabled={sending} onclick={() => capture('todo')} title={t('todob.home.todoHint')}><Icon name="todos" size={14} />{pageText('home', 'todoChip', t('addTodoChip'))}</button>{/if}
          {#if shows('home', 'noteChip')}<button class="chip" disabled={sending} onclick={() => capture('note')} title={t('todob.home.noteHint')}><Icon name="inbox" size={14} />{pageText('home', 'noteChip', t('todob.home.note'))}</button>{/if}
          {#if app.settings?.modules?.checkin && shows('home', 'checkInChip')}<button class="chip" disabled={sending} onclick={() => { if (sending) return; track('home.chip:check-in'); submit('check-in'); }}><Icon name="calendar" size={14} />{t('checkIn')}</button>{/if}
          {#each [...app.home.chips, ...app.homeChips] as c, i (i + ':' + c.label)}
            <button class="chip" dir="auto" disabled={sending} title={c.prompt.slice(0, 300)} onclick={() => useChip(c)}>{c.label}</button>
          {/each}
          <span class="grow"></span>
          <button class="btn primary round" onclick={() => submit()} disabled={!ask.trim() || sending} aria-busy={sending} aria-label={t('send')}><Icon name="send" size={16} stroke={2.25} /></button>
        </div>
      </div>
    </div>
    {#if shows('home', 'emblem')}<div class="art"><Emblem size={250} /></div>{/if}
  </section>
{/snippet}

{#snippet todosS()}<section class="cols one">{@render panel('todos')}</section>{/snippet}
{#snippet chatsS()}<section class="cols one">{@render panel('chats')}</section>{/snippet}
{#snippet usageS()}<UsageTile />{/snippet}
{#snippet automateS()}<section class="autoq"><button type="button" class="more" onclick={() => go({ kind: 'customise', id: 'automate' })} title={t('auto-ui.homeTip')}><Icon name="ai" size={14} /> {t('auto-ui.suggest')}</button></section>{/snippet}

<div class="home">
  {#if pageWarnings('home').length}<p class="muted sm hw" role="status">{t('pgs.unknown', { list: pageWarnings('home').map((w) => `\u2068${w}\u2069`).join(t('ldb.listSep')) })}</p>{/if}
  <Sections page="home" parts={homeParts} order={homeOrder} sizes={homeSizes} gap="12px" box={widgetS} />


</div>

<style>
  .home { max-width: 1080px; margin: 0 auto; padding: 8px 36px 48px; }
  .hero { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 32px; min-height: 330px; padding-top: 18px; }
  .lead { min-width: 0; }
  .eyebrow { color: var(--accent-2); margin-bottom: 10px; }
  .sub { color: var(--muted); font-size: 15px; max-width: 520px; margin: 12px 0 22px; }
  .art { padding: 10px; }

  .ask { border-radius: 18px; padding: 12px 12px 10px 16px; transition: box-shadow .2s; }
  .ask:focus-within { box-shadow: var(--shadow-lg), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent); }
  .ask textarea { position: relative; background: transparent; border: 0; box-shadow: none; padding: 4px 0; resize: none; font-size: 15px; }
  .ask textarea:focus { background: transparent; box-shadow: none; }
  .ask .row { position: relative; }
  .chip { font: inherit; background: none; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; font-size: var(--fs-sm); font-weight: 500; color: var(--muted); padding: 5px 11px; border-radius: 99px; border: 1px solid var(--hairline); }
  .chip:hover:not(:disabled) { color: var(--fg); border-color: color-mix(in srgb, var(--fg) 25%, transparent); }
  .chip:disabled { cursor: default; opacity: .6; }
  .round { width: 32px; height: 32px; padding: 0; border-radius: 50%; }

  .stats { display: flex; gap: 56px; padding: 26px 2px 8px; flex-wrap: wrap; }
  .stat { font: inherit; color: inherit; background: none; border: 0; padding: 0; text-align: start; cursor: pointer; display: flex; flex-direction: column; gap: 2px; }
  .stat b { font-size: 30px; font-weight: 500; letter-spacing: -.04em; line-height: 1.1; }
  .stat span { font-size: var(--fs-sm); color: var(--muted); }
  .stat:hover b { color: var(--accent); }
  .stat.attn b, .stat.attn span { color: var(--accent-2); }

  .block { margin-top: 44px; }
  .bh { display: flex; align-items: flex-end; gap: 16px; margin-bottom: 16px; }
  .bh > div { flex: 1; }
  .bh h2 { font-size: 24px; margin: 0 0 4px; letter-spacing: -.03em; }
  .bh p { margin: 0; font-size: 13.5px; }

  .ideas { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; }
  .idea { font: inherit; color: inherit; text-align: start; box-sizing: border-box; cursor: pointer; position: relative; height: 196px; padding: 14px; display: flex; flex-direction: column; gap: 3px; transition: transform .2s; }
  .idea:hover { transform: translateY(-2px); }
  .badge { position: relative; align-self: flex-start; display: inline-flex; align-items: center; gap: 5px; font-size: var(--fs-xs); font-weight: 500; color: var(--muted); }
  .glyph { position: relative; flex: 1; display: grid; place-items: center; font-size: 46px; font-weight: 300; letter-spacing: -.05em; color: color-mix(in srgb, var(--fg) 85%, transparent); }
  .idea b { position: relative; font-size: 13.5px; font-weight: 600; }
  .clamp { position: relative; font-size: var(--fs-xs); line-height: 1.4; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }

  .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-top: 44px; }
  .cols.one { grid-template-columns: 1fr; }
  .chips { flex-wrap: wrap; }
  .sugs { display: grid; gap: 10px; margin-top: 20px; }
  .sug { display: flex; align-items: center; gap: 12px; padding: 12px 14px; flex-wrap: wrap; }
  .sug > * { position: relative; }
  .sug .grow { min-width: 220px; }
  .sug p { margin: 2px 0 0; font-size: 13px; }
  .sgi { width: 30px; height: 30px; border-radius: 8px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); flex: none; }
  .sga { gap: 6px; }
  .widget { margin-top: 28px; }
  .autoq { margin-top: 24px; display: flex; justify-content: center; }
  /* An add-on box rises like the sections around it, once its add-on has drawn it (lib/surfaceIn.ts). */
  .widget:global([data-drawn='no']) { opacity: 0; animation: none; }
  .widget:global([data-drawn='yes']) { animation: rise .5s var(--ease) backwards; }
  @media (prefers-reduced-motion: reduce) { .widget:global([data-drawn='no']) { opacity: 1; } }
  :global(html[data-motion='reduce']) .widget:global([data-drawn='no']) { opacity: 1; }
  @media print { .widget:global([data-drawn='no']) { opacity: 1; } }
  .wslot { position: relative; padding: 4px 10px 10px; }
  .panel { padding: 6px; min-width: 0; overflow: hidden; }
  .ph { position: relative; display: flex; align-items: center; padding: 10px 10px 8px; }
  .ph h2 { margin: 0; font-size: 15px; flex: 1; }
  .rowbtn { font: inherit; background: none; border: 0; text-align: start; position: relative; box-sizing: border-box; width: 100%; cursor: pointer; display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: 9px; min-width: 0; color: var(--fg); }
  .rowbtn :global(.icon) { color: var(--muted); flex: none; }
  .rowbtn:hover { background: var(--fill); }
  .todo { position: relative; display: flex; align-items: center; gap: 10px; padding: 8px 10px; margin: 0; border-radius: 9px; font-size: 13.5px; color: var(--fg); cursor: pointer; }
  .todo:hover { background: var(--fill); }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .when { font-size: var(--fs-xs); white-space: nowrap; max-width: 140px; }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--accent-2); }
  .up { margin-top: 28px; }
  .phl { flex: 1; }
  .phl .eyebrow { color: var(--accent-2); margin-bottom: 2px; }
  .due { font-size: var(--fs-xs); font-weight: 500; color: var(--muted); background: var(--fill); border: 1px solid transparent; border-radius: 99px; padding: 2px 9px; display: inline-flex; align-items: center; gap: 5px; white-space: nowrap; }
  .due.today, .due.soon { color: var(--on-tint); background: var(--tint); }
  .due.overdue { color: var(--danger); border-color: currentColor; background: color-mix(in srgb, var(--danger) 10%, transparent); }
  .empty { position: relative; display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 24px 10px; color: var(--muted); font-size: 13px; }
  .ideas .empty { grid-column: 1 / -1; }

  .projects { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; }
  .proj { font: inherit; color: inherit; text-align: start; box-sizing: border-box; cursor: pointer; padding: 14px; display: flex; flex-direction: column; gap: 6px; min-height: 104px; justify-content: flex-end; }
  .proj :global(.icon) { position: relative; color: var(--accent-2); margin-bottom: auto; }
  .proj b, .proj span { position: relative; }
  .pcell { position: relative; display: grid; min-width: 0; }
  .pnew { position: absolute; inset-block-start: 8px; inset-inline-end: 8px; font: inherit; color: var(--muted); background: var(--fill); border: 0; border-radius: 8px; width: 28px; height: 28px; display: grid; place-items: center; cursor: pointer; }
  .pnew:hover { color: var(--fg); background: var(--fill-2); }
  .stat .skel.num { width: 44px; height: 30px; margin-bottom: 6px; }
  .stat .skel.lbl { width: 92px; height: 12px; }
  .card-skel { height: 196px; border-radius: var(--radius-lg); }
  .skel.line { height: 14px; margin: 12px 10px; }
  .w70 { width: 70%; } .w50 { width: 50%; }
  .loaderr { display: flex; align-items: center; gap: 10px; padding: 10px 14px; margin-top: 10px; font-size: 13px; }
  .loaderr > * { position: relative; }
  .loaderr > :global(.icon) { color: var(--danger); flex: none; }
  .hint { font-size: var(--fs-xs); max-width: 360px; text-align: center; }
  .proj.add { background: transparent; box-shadow: none; border: 1px dashed var(--hairline); border-radius: var(--radius-lg); color: var(--muted); align-items: center; justify-content: center; font-size: 13px; }
  .proj.add:hover { color: var(--fg); border-color: color-mix(in srgb, var(--fg) 30%, transparent); }
  .proj.add :global(.icon) { margin: 0; color: inherit; }

  @container main (max-width: 900px) { .art { display: none; } .hero { grid-template-columns: 1fr; min-height: 0; padding-top: 30px; } .ideas, .projects { grid-template-columns: repeat(2, 1fr); } }
  @container main (max-width: 620px) { .cols { grid-template-columns: 1fr; } .stats { gap: 28px; } .home { padding-inline: 20px; } }
</style>
