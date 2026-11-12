<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { api, apiBlob, on, post } from '../lib/api';
  import { app, go, track } from '../lib/store.svelte';
  import { t, tn, fmtNum, lang } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { saveFile } from '../lib/download';
  import TodoCalendar from './TodoCalendar.svelte';
  import Icon from '../lib/Icon.svelte';
  import ItemActions from '../lib/ItemActions.svelte';
  import { copyText, dueLabel as dueLabelOn, dueState as dueStateOn, isoDay, normTodo, notify, prettyName, withDue } from '../lib/content/util';
  import { clock } from '../lib/clock.svelte';

  type Filter = 'all' | 'soon' | 'overdue';
  type GroupBy = 'list' | 'due' | 'tag' | 'none';
  type Prefs = { groupBy: GroupBy; sort: 'list' | 'due'; filter: Filter; showDone: boolean };

  let { mode = 'list' }: { mode?: 'list' | 'calendar' } = $props();

  // How the list is shown: the user's own choice (remembered on this device) wins; otherwise the
  // defaults from the customisation layer's `todos.json`; otherwise the built-in ones.
  const PREF_KEY = 'thoughts.todos.view';
  const GROUPS: GroupBy[] = ['list', 'due', 'tag', 'none'];
  function initial(): Prefs {
    const base: Prefs = { groupBy: 'list', sort: 'list', filter: 'all', showDone: false };
    const pick = (o: any) => {
      if (!o || typeof o !== 'object') return;
      if (GROUPS.includes(o.groupBy)) base.groupBy = o.groupBy;
      if (o.sort === 'list' || o.sort === 'due') base.sort = o.sort;
      if (['all', 'soon', 'overdue'].includes(o.filter)) base.filter = o.filter;
      if (typeof o.showDone === 'boolean') base.showDone = o.showDone;
    };
    pick(app.todosCfg);
    try { pick(JSON.parse(localStorage.getItem(PREF_KEY) || 'null')); } catch { /* ignore */ }
    return base;
  }
  const start = initial();
  let touched = false;
  function remember() {
    touched = true;
    try { localStorage.setItem(PREF_KEY, JSON.stringify({ groupBy, sort, filter, showDone })); } catch { /* private mode */ }
  }
  function resetView() {
    try { localStorage.removeItem(PREF_KEY); } catch { /* ignore */ }
    touched = false;
    const d = initial();
    groupBy = d.groupBy; sort = d.sort; filter = d.filter; showDone = d.showDone;
  }

  let lists = $state<any[]>([]);
  let loading = $state(true);
  let loadError = $state('');
  let showDone = $state(start.showDone);
  let filter = $state<Filter>(start.filter);
  let sort = $state<'list' | 'due'>(start.sort);
  let groupBy = $state<GroupBy>(start.groupBy);
  // A customisation applied while this view is open becomes the default straight away — unless the user chose their own.
  $effect(() => { app.todosCfg; if (!touched && !hasOwn()) resetView(); });
  function hasOwn() { try { return !!localStorage.getItem(PREF_KEY); } catch { return false; } }
  let drafts = $state<Record<string, string>>({});
  let first = $state('');
  let editing = $state<string | null>(null);
  let editText = $state('');
  let dueFor = $state<string | null>(null);

  async function load() {
    try {
      const raw: any[] = await api('/todos');
      // `due` / `title` come from the daemon when it supports them; otherwise they are read from the text.
      lists = raw.map((l) => ({ ...l, items: l.items.map(normTodo) }));
      loadError = '';
    } catch (e: any) { loadError = e?.message ?? String(e); if (lists.length) failed(t('todo.loadFailed'), e); }
    finally { loading = false; }
  }
  onMount(() => { load(); return on((e) => { if (['todos.changed', 'library.changed', 'resync'].includes(e.type)) load(); }); });

  // Due wording follows the shared clock, so "today" and "overdue" move on at midnight by themselves.
  const dueState = (d: string) => { clock.day; return dueStateOn(d); };
  const dueLabel = (d: string) => { clock.day; return dueLabelOn(d); };

  // The general list: the service picks the one that exists (the starter keeps it in tasks/TODO.md).
  const GENERAL = '';

  const OP_FAIL: Record<string, string> = { add: 'ui.todo.addFailed', toggle: 'ui.todo.toggleFailed', edit: 'ui.todo.editFailed', delete: 'ui.todo.deleteFailed' };
  async function op(name: string, body: any): Promise<boolean> {
    let ok = true;
    try { await post(`/todos/${name}`, body); } catch (e) { failed(t(OP_FAIL[name] ?? 'ui.todo.editFailed'), e); ok = false; }
    await load();
    return ok;
  }

  /** Ticking a todo off can be undone from the toast (un-ticking needs no undo). When the change
   *  could not be saved the box goes back to what is really in the list. */
  async function toggle(l: any, it: any, box?: HTMLInputElement) {
    const r = ref(l, it);
    const wasDone = it.done;
    if (!(await op('toggle', r))) { if (box) box.checked = wasDone; return; }
    if (wasDone) return;
    track('todo.done');
    notify(t('ui.todo.completed'), { key: 'todo-done', action: { label: t('undo'), run: () => { op('toggle', r); } } });
  }

  /** Add a todo (optionally due on a day) to a list file — used by the list boxes and by the calendar. */
  async function addTo(file: string, text: string, due: string | null = null): Promise<boolean> {
    const ok = await op('add', { file, text: withDue(text, due) });
    if (ok) track('todo.add');
    return ok;
  }

  async function downloadCalendar() {
    try {
      const blob = await apiBlob('/todos/calendar.ics');
      const text = await blob.text();
      const r = await saveFile('thoughts-todos.ics', text, 'text/calendar');
      if (r === 'saved') notify(t('ui.cal.saved'), { kind: 'success' });
      else if (r === 'unsupported') notify((await copyText(text)) ? t('ui.cal.copied') : t('ui.cal.saveFailed'), { kind: 'info' });
    } catch (e) { failed(t('ui.cal.saveFailed'), e); }
  }

  async function add(file: string) {
    const text = drafts[file]?.trim();
    if (!text) return;
    drafts[file] = '';
    if (!(await addTo(file, text))) drafts[file] = text;
  }

  async function createFirst() {
    const text = first.trim();
    if (!text) return;
    first = '';
    if (!(await addTo(GENERAL, text))) first = text;
  }

  const key = (l: any, it: any) => l.file + ':' + it.line;
  const ref = (l: any, it: any) => ({ file: l.file, line: it.line, text: it.text });

  async function startEdit(l: any, it: any) {
    editing = key(l, it); editText = it.title; dueFor = null;
    await tick();
    document.getElementById('todo-edit')?.focus();
  }
  async function saveEdit(l: any, it: any) {
    const title = editText.trim();
    editing = null;
    if (!title || title === it.title) return;
    await op('edit', { ...ref(l, it), new_text: withDue(title, it.due) });
  }
  async function setDue(l: any, it: any, due: string | null) {
    dueFor = null;
    if ((due || null) === it.due) return;
    await op('edit', { ...ref(l, it), new_text: withDue(it.text, due || null) });
  }
  async function remove(l: any, it: any) {
    if (!(await op('delete', ref(l, it)))) return;
    notify(t('todo.deleted'), { action: { label: t('undo'), run: () => { op('add', { file: l.file, text: it.text }); } } });
  }

  const matches = (it: any) => {
    if (!showDone && it.done) return false;
    if (filter === 'all') return true;
    if (!it.due || it.done) return false;
    const s = dueState(it.due);
    return filter === 'overdue' ? s === 'overdue' : s === 'today' || s === 'soon';
  };
  const byDue = (a: any, b: any) => (a.due ?? '9999').localeCompare(b.due ?? '9999') || a.line - b.line;
  function visible(l: any): any[] {
    const v = l.items.filter(matches);
    return sort === 'due' ? [...v].sort(byDue) : v;
  }

  // One section per list (the default), or sections that cut across lists: by due date, by #tag, or one flat list.
  type Row = { l: any; it: any };
  type Section = { id: string; title: string; list: any | null; rows: Row[]; overdue?: boolean };
  const firstTag = (title: string) => /(?:^|\s)#([\p{L}\p{N}_-]+)/u.exec(title)?.[1] ?? '';
  const sections = $derived.by((): Section[] => {
    if (groupBy === 'list') return lists.map((l) => ({ id: l.file, title: listName(l), list: l, rows: visible(l).map((it) => ({ l, it })) }));
    const rows: Row[] = lists.flatMap((l) => visible(l).map((it) => ({ l, it })));
    if (sort === 'due' || groupBy === 'due') rows.sort((a, b) => byDue(a.it, b.it));
    if (groupBy === 'none') return [{ id: 'all', title: t('ui.todo.group.all'), list: null, rows }];
    const buckets = new Map<string, Section>();
    const put = (id: string, title: string, r: Row, overdue = false) => {
      if (!buckets.has(id)) buckets.set(id, { id, title, list: null, rows: [], overdue });
      buckets.get(id)!.rows.push(r);
    };
    if (groupBy === 'due') {
      const order = ['overdue', 'today', 'soon', 'later', 'none', 'done'];
      for (const r of rows) {
        const k = r.it.done ? 'done' : r.it.due ? dueState(r.it.due) : 'none';
        put(k, t('ui.todo.group.' + k), r, k === 'overdue');
      }
      return order.filter((k) => buckets.has(k)).map((k) => buckets.get(k)!);
    }
    for (const r of rows) { const tag = firstTag(r.it.title); put(tag || '~', tag ? '#' + tag : t('ui.todo.group.noTag'), r); }
    return [...buckets.values()].sort((a, b) => a.id.localeCompare(b.id));
  });

  const all = $derived(lists.flatMap((l) => l.items.filter((i: any) => !i.done)));
  const open = $derived(all.length);
  const nOverdue = $derived(all.filter((i: any) => i.due && dueState(i.due) === 'overdue').length);
  const nSoon = $derived(all.filter((i: any) => i.due && ['today', 'soon'].includes(dueState(i.due))).length);
  const anyVisible = $derived(sections.some((x) => x.rows.length));
  const setMode = (m: 'list' | 'calendar') => { if (m !== mode) go(m === 'calendar' ? { kind: 'todos', id: 'calendar' } : { kind: 'todos' }); };
  const listName = (l: any) => (l.idea ? prettyName(String(l.idea)) : t('todo.general'));
  const FILTERS: Filter[] = ['all', 'soon', 'overdue'];
</script>

{#snippet row(l: any, it: any, showList: boolean)}
  {@const k = key(l, it)}
  <div class="item" class:done={it.done}>
    <div class="row line">
      <input type="checkbox" checked={it.done} aria-label={t(it.done ? 'todo.markOpen' : 'todo.markDone', { title: it.title })}
        onchange={(e) => toggle(l, it, e.currentTarget)} />
      {#if editing === k}
        <form class="grow row" onsubmit={(e) => { e.preventDefault(); saveEdit(l, it); }}>
          <input id="todo-edit" class="grow" bind:value={editText} dir="auto" aria-label={t('todo.editLabel')}
            onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); editing = null; } }} />
          <button class="btn primary sm">{t('save')}</button>
          <button class="btn sm" type="button" onclick={() => (editing = null)}>{t('cancel')}</button>
        </form>
      {:else}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <span class="txt grow" dir="auto" ondblclick={() => startEdit(l, it)}>{it.title}</span>
        {#if showList}<span class="muted from ell" dir="auto">{listName(l)}</span>{/if}
        {#if it.due}
          {@const s = it.done ? 'later' : dueState(it.due)}
          <button class="due {s}" onclick={() => (dueFor = dueFor === k ? null : k)} aria-expanded={dueFor === k} title={t('todo.changeDue')}>
            <Icon name={s === 'overdue' ? 'warning' : 'calendar'} size={12} />
            {#if s === 'overdue'}<b>{t('todo.overdue')}</b> ·{/if}
            {dueLabel(it.due)}
          </button>
        {/if}
        <span class="acts">
          {#if !it.due}<button class="ib" onclick={() => (dueFor = dueFor === k ? null : k)} aria-expanded={dueFor === k} aria-label={t('todo.setDue')} title={t('todo.setDue')}><Icon name="calendar" size={14} /></button>{/if}
          <ItemActions target="todo" name={it.title} item={{ file: l.file, idea: l.idea ?? null, line: it.line, text: it.text, title: it.title, due: it.due, done: it.done, group: it.group ?? null }} />
          <button class="ib" onclick={() => startEdit(l, it)} aria-label={t('todo.editItem', { title: it.title })} title={t('edit')}><Icon name="edit" size={14} /></button>
          <button class="ib" onclick={() => remove(l, it)} aria-label={t('todo.deleteItem', { title: it.title })} title={t('delete')}><Icon name="trash" size={14} /></button>
        </span>
      {/if}
    </div>
    {#if dueFor === k}
      <div class="row duebar">
        <label class="inl" for="due-{k}">{t('todo.dueDate')}</label>
        <input id="due-{k}" type="date" lang={lang()} value={it.due ?? ''} onchange={(e) => setDue(l, it, (e.target as HTMLInputElement).value)} />
        <button class="btn sm" onclick={() => setDue(l, it, isoDay())}>{t('todo.today')}</button>
        <button class="btn sm" onclick={() => setDue(l, it, isoDay(1))}>{t('todo.tomorrow')}</button>
        <button class="btn sm" onclick={() => setDue(l, it, isoDay(7))}>{t('todo.nextWeek')}</button>
        {#if it.due}<button class="btn ghost sm" onclick={() => setDue(l, it, null)}>{t('todo.clearDue')}</button>{/if}
        <button class="ib" onclick={() => (dueFor = null)} aria-label={t('close')}><Icon name="close" size={14} /></button>
      </div>
    {/if}
  </div>
{/snippet}

<div class="page">
  <div class="row top">
    <h1 class="grow">{t('todos')} <span class="pill count" aria-label={t('todo.openCount', { n: fmtNum(open) })}>{fmtNum(open)}</span></h1>
    {#if lists.length}
      <div class="seg" role="group" aria-label={t('ui.cal.viewLabel')}>
        <button class="segb" aria-pressed={mode === 'list'} onclick={() => setMode('list')}><Icon name="todos" size={14} /> {t('ui.cal.list')}</button>
        <button class="segb" aria-pressed={mode === 'calendar'} onclick={() => setMode('calendar')}><Icon name="calendar" size={14} /> {t('ui.cal.title')}</button>
      </div>
      <button class="btn" onclick={downloadCalendar} title={t('ui.cal.addHint')}><Icon name="download" size={14} /> {t('ui.cal.add')}</button>
    {/if}
  </div>

  {#if loading}
    <div class="card list" aria-busy="true"><div class="skel"></div><div class="skel w70"></div><div class="skel w50"></div></div>
  {:else if loadError && !lists.length}
    <div class="state" role="alert"><Icon name="warning" size={24} /><p>{t('todo.loadFailed')}</p><p class="muted sm" dir="auto">{loadError}</p>
      <button class="btn primary" onclick={() => { loading = true; load(); }}><Icon name="retry" size={14} /> {t('todo.tryAgain')}</button></div>
  {:else if !lists.length}
    <div class="state">
      <Icon name="todos" size={28} />
      <h2>{t('todo.emptyTitle')}</h2>
      <p class="muted">{t('todo.emptyBody')}</p>
      <p class="muted sm">{t('ui.todo.emptyTip')}</p>
      <form class="row firstform" onsubmit={(e) => { e.preventDefault(); createFirst(); }}>
        <input class="grow" bind:value={first} placeholder={t('todo.firstPlaceholder')} aria-label={t('todo.firstPlaceholder')} dir="auto" />
        <button class="btn primary" disabled={!first.trim()}><Icon name="plus" size={14} /> {t('todo.add')}</button>
      </form>
    </div>
  {:else if mode === 'calendar'}
    <TodoCalendar {lists} add={(text, due) => addTo(GENERAL, text, due)} toggle={(l, it, box) => toggle(l, it, box)} setDue={(l, it, due) => setDue(l, it, due)} />
  {:else}
    <div class="row tools">
      <div class="seg" role="group" aria-label={t('todo.filterLabel')}>
        {#each FILTERS as f}
          <button class="segb" aria-pressed={filter === f} onclick={() => { filter = f; remember(); }}>
            {#if f === 'overdue' && nOverdue}<Icon name="warning" size={12} />{/if}
            {t(`todo.filter.${f}`)}
            {#if f === 'soon' && nSoon}<span class="n">{fmtNum(nSoon)}</span>{/if}
            {#if f === 'overdue' && nOverdue}<span class="n">{fmtNum(nOverdue)}</span>{/if}
          </button>
        {/each}
      </div>
      <span class="grow"></span>
      <label class="row inl" for="todo-group">{t('ui.todo.groupLabel')}</label>
      <select id="todo-group" bind:value={groupBy} onchange={remember}>
        {#each GROUPS as g}<option value={g}>{t('ui.todo.groupBy.' + g)}</option>{/each}
      </select>
      <label class="row inl" for="todo-sort">{t('todo.sortLabel')}</label>
      <select id="todo-sort" bind:value={sort} onchange={remember}>
        <option value="list">{t('todo.sort.list')}</option>
        <option value="due">{t('todo.sort.due')}</option>
      </select>
      <label class="row inl"><input type="checkbox" bind:checked={showDone} onchange={remember} /> {t('todo.showDone')}</label>
      {#if app.todosCfg && hasOwn()}<button class="btn ghost sm" onclick={resetView} title={t('ui.todo.resetViewHint')}>{t('ui.todo.resetView')}</button>{/if}
    </div>

    {#if !anyVisible && (filter !== 'all' || open === 0)}
      <div class="state">
        <Icon name={filter === 'all' ? 'check' : 'filter'} size={24} />
        <p>{t(filter === 'overdue' ? 'todo.noneOverdue' : filter === 'soon' ? 'todo.noneSoon' : 'todo.allDone')}</p>
        {#if filter !== 'all'}<button class="btn" onclick={() => { filter = 'all'; remember(); }}>{t('todo.showAll')}</button>{/if}
      </div>
    {/if}

    {#if groupBy !== 'list'}
      <form class="row add quick" onsubmit={(e) => { e.preventDefault(); add(GENERAL); }}>
        <input class="grow" placeholder={t('addTodo')} aria-label={t('todo.addTo', { list: t('todo.general') })} bind:value={drafts[GENERAL]} dir="auto" />
        {#if drafts[GENERAL]?.trim()}<button class="btn primary sm">{t('todo.add')}</button>{/if}
      </form>
    {/if}

    {#each sections as sec (sec.id)}
      {@const l = sec.list}
      {#if sec.rows.length || (l && filter === 'all')}
        <section class="card list" aria-label={sec.title}>
          <div class="row head">
            <b class="grow" dir="auto">{#if sec.overdue}<Icon name="warning" size={13} /> {/if}{sec.title}{#if !l} <span class="muted n">{tn('ui.todo.count', sec.rows.length)}</span>{/if}</b>
            {#if l}<button class="btn ghost sm" onclick={() => go({ kind: 'docs', path: l.idea ? `${l.idea}/README.md` : l.file })} title={t('todo.openSource')}><bdi>{l.file}</bdi></button>{/if}
          </div>
          {#each sec.rows as r, idx (key(r.l, r.it))}
            {#if l && sort === 'list' && r.it.group && (idx === 0 || r.it.group !== sec.rows[idx - 1]?.it.group)}<div class="group" dir="auto">{r.it.group}</div>{/if}
            {@render row(r.l, r.it, !l)}
          {:else}
            <p class="muted none">{t('todo.listEmpty')}</p>
          {/each}
          {#if l}
            <form class="row add" onsubmit={(e) => { e.preventDefault(); add(l.file); }}>
              <input class="grow" placeholder={t('addTodo')} aria-label={t('todo.addTo', { list: listName(l) })} bind:value={drafts[l.file]} dir="auto" />
              {#if drafts[l.file]?.trim()}<button class="btn primary sm">{t('todo.add')}</button>{/if}
            </form>
          {/if}
        </section>
      {/if}
    {/each}
  {/if}
</div>

<style>
  .top h1 { display: flex; align-items: center; gap: 10px; }
  .count { font-size: var(--fs-xs); }
  .tools { flex-wrap: wrap; gap: 10px; margin-bottom: 4px; }
  .tools select { width: auto; font-size: 13px; padding: 4px 8px; }
  .inl { margin: 0; font-size: 13px; color: var(--muted); white-space: nowrap; }
  .segb .n { font-size: var(--fs-xs); }
  .top { flex-wrap: wrap; }
  .head .n { font-weight: 400; font-size: var(--fs-xs); margin-inline-start: 6px; }
  .from { font-size: var(--fs-xs); max-width: 160px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .quick { margin-top: 10px; }

  .list { margin-top: 12px; }
  .head { margin-bottom: 6px; }
  .group { font-size: var(--fs-xs); font-weight: 600; letter-spacing: .02em; color: var(--muted); margin: 10px 0 2px; }
  .item { padding: 3px 0; }
  .line { min-height: 30px; }
  .item.done .txt { text-decoration: line-through; color: var(--muted); }
  .txt { cursor: text; overflow-wrap: anywhere; }
  /* Row actions are always fully visible: --muted is ≥4.5:1 at rest, and hover/focus take the text colour. */
  .acts { display: inline-flex; align-items: center; gap: 2px; }
  .item:hover .ib, .acts:focus-within .ib { color: var(--fg); }
  .ib { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 5px; min-width: 24px; min-height: 24px; cursor: pointer; display: grid; place-items: center; }
  .ib:hover { color: var(--fg); background: var(--fill); }
  .due { font: inherit; font-size: var(--fs-xs); font-weight: 500; color: var(--muted); background: var(--fill); border: 1px solid transparent; border-radius: 99px; padding: 2px 9px; cursor: pointer; display: inline-flex; align-items: center; gap: 5px; white-space: nowrap; }
  .due.today, .due.soon { color: var(--on-tint); background: var(--tint); }
  .due.overdue { color: var(--danger); border-color: currentColor; background: color-mix(in srgb, var(--danger) 10%, transparent); }
  .duebar { flex-wrap: wrap; margin: 4px 0 6px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); margin-inline-start: 24px; }
  .duebar input[type="date"] { width: auto; padding: 4px 8px; font-size: 13px; background: var(--bg); }
  .add input { border-style: dashed; margin-top: 6px; }
  .add .btn { margin-top: 6px; }
  .none { margin: 4px 0; font-size: 13px; }
  .sm { font-size: var(--fs-xs); padding: 3px 9px; }

  .state { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 8px; padding: 40px 0; color: var(--muted); }
  .state h2 { margin: 4px 0 0; color: var(--fg); }
  .state p { margin: 0; max-width: 420px; }
  .firstform { width: min(460px, 100%); margin-top: 10px; }
  .skel { height: 14px; margin: 8px 0; border-radius: 8px; background: var(--fill); }
  .w70 { width: 70%; } .w50 { width: 50%; }
</style>
