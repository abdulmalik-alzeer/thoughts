<script lang="ts">
  import { onMount } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { app, go, refreshChats, toast, track } from '../lib/store.svelte';
  import { startCustomise } from '../lib/layer';
  import { t, tn, ago, fmtDate, fmtNum, lang } from '../lib/i18n';
  import { dueLabel, dueState, normTodo } from '../lib/content/util';
  import { failed } from '../lib/errors';
  import { term, termN } from '../lib/terms';
  import { capturePrefix, captureKind, normalizeCapture } from '../lib/capture';
  import Icon from '../lib/Icon.svelte';
  import Emblem from '../lib/Emblem.svelte';

  let { projects, newChat, startWrap }: { projects: any[]; newChat: (pid?: string | null) => void; startWrap: () => void } = $props();

  let ideas = $state<any[]>([]);
  let lists = $state<any[]>([]);
  let files = $state(0);
  let ask = $state('');
  // True from the moment a chip put its own text in the box until the box is empty again: such a
  // message is not one the user wrote from scratch, so it never counts as `byUser`.
  let chipFilled = false;
  $effect(() => { if (!ask.trim()) chipFilled = false; });
  let askEl: HTMLTextAreaElement;

  // Nothing is shown as "0" until it has really been counted: placeholders while loading, a notice when it failed.
  let loading = $state(true);
  let loadError = $state('');
  let partFailed = $state<Record<string, boolean>>({});

  async function load() {
    const [i, td, st] = await Promise.allSettled([api('/library/ideas'), api('/todos'), api('/library/stats')]);
    if (i.status === 'fulfilled') ideas = Array.isArray(i.value) ? i.value : [];
    if (td.status === 'fulfilled') lists = Array.isArray(td.value) ? td.value : [];
    if (st.status === 'fulfilled') files = Number(st.value?.markdownFiles) || 0;
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
      await startCustomise(sg.prompt);
      suggestions = suggestions.filter((x) => x.id !== sg.id);
      dismissSuggestion(sg.id).catch(() => {});
    } catch (e) { failed(t('layer.askFailed'), e); }
    finally { sugBusy = ''; }
  }

  // ----- layout from home.json: hidden sections and their order -----
  // `order` may also place `suggestions` and single plugin widgets (`widget:<id>`); anything not mentioned keeps
  // its usual place (suggestions first, widgets last).
  const SECTIONS = ['stats', 'ideas', 'upcoming', 'todos', 'chats', 'projects'];
  const widgetKey = (w: { id: string }) => 'widget:' + w.id;
  const layout = $derived.by(() => {
    const hidden = new Set(app.home.hidden);
    const widgets = app.homeWidgets.map(widgetKey);
    const known = (k: string) => SECTIONS.includes(k) || k === 'suggestions' || widgets.includes(k);
    const asked = app.home.order.filter(known);
    const order = [...new Set([...(asked.includes('suggestions') ? [] : ['suggestions']), ...asked, ...SECTIONS, ...widgets])].filter((k) => !hidden.has(k));
    // Todos and chats share a row when they are neighbours.
    const rows: string[][] = [];
    for (let i = 0; i < order.length; i++) {
      const a = order[i], b = order[i + 1];
      if ((a === 'todos' && b === 'chats') || (a === 'chats' && b === 'todos')) { rows.push([a, b]); i++; } else rows.push([a]);
    }
    return rows;
  });

  /** Mounts a plugin's widget frame into the slot. */
  function plug(el: HTMLElement, w: { render: (el: HTMLElement) => any }) {
    let cleanup: any;
    try { cleanup = w.render(el); } catch (e) { console.error(e); }
    return { destroy() { try { if (typeof cleanup === 'function') cleanup(); } catch { /* frame already gone */ } } };
  }

  /** Capture chips: with nothing typed they put the prefix in the box for the user to finish; otherwise they send. */
  function capture(kind: 'todo' | 'idea') {
    track(kind === 'todo' ? 'home.chip:todo' : 'home.chip:new idea');
    const typed = ask.trim();
    const prefix = capturePrefix(kind, lang());
    if (!typed || (captureKind(typed) === kind && !normalizeCapture(typed).replace(/^[^:]+:\s*/, '').trim())) {
      ask = prefix;
      askEl?.focus();
      requestAnimationFrame(() => askEl?.setSelectionRange(ask.length, ask.length));
      return;
    }
    submit(captureKind(typed) ? '' : prefix, false);
  }

  function useChip(c: { label: string; prompt: string }) {
    track('home.chip:' + c.label);
    // A prompt ending in a space is a sentence for the user to finish.
    if (/\s$/.test(c.prompt) && !ask.trim()) { ask = c.prompt; chipFilled = true; askEl?.focus(); return; }
    submit(/\s$/.test(c.prompt) || !ask.trim() ? c.prompt : c.prompt + '\n\n', false);
  }

  onMount(() => { load(); loadSuggestions(); return on((e) => { if (['todos.changed', 'library.changed', 'chat.done'].includes(e.type)) load(); }); });

  const hour = new Date().getHours();
  // Arabic says «مساء الخير» from mid-afternoon; «طاب نهارك» covers noon to three. English keeps "afternoon" until six.
  const greeting = $derived(t(hour < 12 ? 'goodMorning' : hour < 15 || (hour < 18 && lang() !== 'ar') ? 'goodAfternoon' : 'goodEvening'));
  const today = $derived(fmtDate(new Date(), { weekday: 'long', day: 'numeric', month: 'long' }));

  const active = $derived(ideas.filter((i) => i.status === 'active' || i.status === 'seed'));
  const recentIdeas = $derived([...ideas].sort((a, b) => String(b.updated).localeCompare(String(a.updated))).slice(0, 4));
  const openTodos = $derived(lists.flatMap((l) => l.items.filter((i: any) => !i.done).map((i: any) => ({ ...normTodo(i), file: l.file, idea: l.idea }))));
  // Next few dated todos, earliest first — so anything overdue leads the list.
  const upcoming = $derived(openTodos.filter((i) => i.due).sort((a, b) => a.due!.localeCompare(b.due!)).slice(0, 5));
  const statusLabel = (s: string) => { const k = `idea.status.${s}`; const x = t(k); return x === k ? s : x; };
  const recentChats = $derived(app.chats.filter((c) => c.kind === 'chat').slice(0, 5));

  async function submit(prefix = '', typed = true) {
    const text = normalizeCapture((prefix + ask).trim());
    if (!text) return askEl?.focus();
    if (captureKind(text) === 'todo') track('todo.add');
    if (typed && !prefix) track('home.prompt:' + text.split(/\s+/).slice(0, 3).join(' '));
    try {
      // Only what the user typed themselves counts as a chat they started (see `inherited_mode` in the service).
      const c = await post('/chats', { byUser: typed && !prefix && !chipFilled });
      await post(`/chats/${c.id}/send`, { text });
      ask = '';
      await refreshChats();
      go({ kind: 'chat', id: c.id });
    } catch (e) { failed(t('ui.home.sendFailed'), e); }
  }

  async function toggle(it: any) {
    const ref = { file: it.file, line: it.line, text: it.text };
    try {
      await post('/todos/toggle', ref);
      track('todo.done');
      toast(t('ui.todo.completed'), { key: 'todo-done', action: { label: t('undo'), run: () => { post('/todos/toggle', ref).then(load).catch((e) => failed(t('ui.todo.undoFailed'), e)); } } });
    } catch (e) { failed(t('ui.todo.toggleFailed'), e); }
    load();
  }

  /** A project card opens the latest chat in that folder; a new one only when there is none (or on "New chat"). */
  function openProject(p: any) {
    const latest = app.chats.filter((c) => c.projectId === p.id && c.kind === 'chat').sort((a, b) => String(b.updated).localeCompare(String(a.updated)))[0];
    if (latest) go({ kind: 'chat', id: latest.id }); else newChat(p.id);
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); submit(); }
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
        <b>{fmtNum(app.wrap.count)}</b><span>{app.wrap.count ? tn('ui.home.toWrap', app.wrap.count) : t('allSaved')}</span></button>
      <button class="stat" onclick={() => go({ kind: 'docs' })}>{@render num(files, partFailed.files)}<span>{tn('ui.home.notes', files)}</span></button>
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
      <div><h2>{t('continueWhere')}</h2><p class="muted">{term('continueSub')}</p></div>
      <button class="more" onclick={() => go({ kind: 'ideas' })}>{t('seeAll')} <Icon name="right" size={14} class="flip-rtl" /></button>
    </div>
    <div class="ideas">
      {#each recentIdeas as i (i.path)}
        <button class="idea surface lit" onclick={() => go({ kind: 'docs', path: i.path })}>
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
        <div class="ph"><div class="phl"><div class="eyebrow">{t('home.upcomingEyebrow')}</div><h2>{t('home.upcoming')}</h2></div>
          <button class="more" onclick={() => go({ kind: 'todos' })}>{t('seeAll')} <Icon name="right" size={14} class="flip-rtl" /></button></div>
        {#each upcoming as it (it.file + it.line)}
          {@const s = dueState(it.due!)}
          <label class="todo">
            <input type="checkbox" onchange={() => toggle(it)} />
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
      <div class="ph"><h2>{t('todos')}</h2><button class="more" onclick={() => go({ kind: 'todos' })}>{t('seeAll')} <Icon name="right" size={14} class="flip-rtl" /></button></div>
      {#each openTodos.slice(0, 7) as it (it.file + it.line)}
        <label class="todo">
          <input type="checkbox" onchange={() => toggle(it)} />
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
      <div class="ph"><h2>{t('recentChats')}</h2><button class="more" onclick={() => newChat()}><Icon name="plus" size={14} /> {t('newChat')}</button></div>
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
  <section class="block widget">
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
        <div><h2>{t('projects')}</h2><p class="muted">{t('noProjects')}</p></div>
        <button class="more" onclick={() => go({ kind: 'projects' })}>{t('manage')} <Icon name="right" size={14} class="flip-rtl" /></button>
      </div>
      <div class="projects">
        {#each projects.slice(0, 4) as p (p.id)}
          <div class="pcell">
            <button class="proj surface lit" title={t('ui.home.openProject', { name: p.name })} onclick={() => openProject(p)}>
              <Icon name="folder" size={18} />
              <b class="ell" dir="auto">{p.name}</b>
              <span class="muted when">{tn('home.chatCount', app.chats.filter((c) => c.projectId === p.id).length)}</span>
            </button>
            <button class="pnew" aria-label={t('shell.newChatIn', { name: p.name })} title={t('shell.newChatIn', { name: p.name })} onclick={() => newChat(p.id)}><Icon name="plus" size={15} /></button>
          </div>
        {/each}
        <button class="proj add" onclick={() => go({ kind: 'projects' })}><Icon name="plus" size={18} /><span>{t('linkFolder')}</span></button>
      </div>
    </section>
  {/if}
{/snippet}

<div class="home">
  <section class="hero">
    <div class="lead">
      <div class="eyebrow">{today}</div>
      <h1 class="display">{greeting}{app.settings?.userName ? `, ${app.settings.userName}` : ''}.</h1>
      <p class="sub">{t('heroSub')}</p>
      <div class="ask surface">
        <textarea bind:this={askEl} bind:value={ask} onkeydown={keydown} rows="2" placeholder={t('askPlaceholder')} aria-label={t('askPlaceholder')} dir="auto"></textarea>
        <div class="row chips">
          <button class="chip" onclick={() => capture('idea')}><Icon name="ideas" size={14} />{term('capture')}</button>
          <button class="chip" onclick={() => capture('todo')}><Icon name="todos" size={14} />{t('addTodoChip')}</button>
          {#if app.settings?.modules?.checkin}<button class="chip" onclick={() => { track('home.chip:check-in'); submit('check-in'); }}><Icon name="calendar" size={14} />{t('checkIn')}</button>{/if}
          {#each [...app.home.chips, ...app.homeChips] as c, i (i + ':' + c.label)}
            <button class="chip" dir="auto" title={c.prompt} onclick={() => useChip(c)}>{c.label}</button>
          {/each}
          <span class="grow"></span>
          <button class="btn primary round" onclick={() => submit()} disabled={!ask.trim()} aria-label={t('send')}><Icon name="send" size={16} stroke={2.25} /></button>
        </div>
      </div>
    </div>
    <div class="art"><Emblem size={250} /></div>
  </section>

  {#each layout as row (row.join('+'))}
    {#if row.length === 2}
      <section class="cols">{#each row as k (k)}{@render panel(k)}{/each}</section>
    {:else if row[0] === 'todos' || row[0] === 'chats'}
      <section class="cols one">{@render panel(row[0])}</section>
    {:else if row[0] === 'stats'}{@render statsS()}
    {:else if row[0] === 'ideas'}{@render ideasS()}
    {:else if row[0] === 'upcoming'}{@render upcomingS()}
    {:else if row[0] === 'projects'}{@render projectsS()}
    {:else if row[0] === 'suggestions'}{@render suggestionsS()}
    {:else if row[0].startsWith('widget:')}
      {#each app.homeWidgets.filter((w) => widgetKey(w) === row[0]) as w (w.plugin + ':' + w.id)}{@render widgetS(w)}{/each}
    {/if}
  {/each}

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
  .chip:hover { color: var(--fg); border-color: color-mix(in srgb, var(--fg) 25%, transparent); }
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
