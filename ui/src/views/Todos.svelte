<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { go } from '../lib/store.svelte';
  import { t, fmtNum } from '../lib/i18n';
  import Icon from '../lib/Icon.svelte';
  import ItemActions from '../lib/ItemActions.svelte';
  import { dueLabel, dueState, fail, isoDay, normTodo, notify, prettyName, withDue } from '../lib/content/util';

  type Filter = 'all' | 'soon' | 'overdue';

  let lists = $state<any[]>([]);
  let loading = $state(true);
  let loadError = $state('');
  let showDone = $state(false);
  let filter = $state<Filter>('all');
  let sort = $state<'list' | 'due'>('list');
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
    } catch (e: any) { loadError = e?.message ?? String(e); if (lists.length) fail(e); }
    finally { loading = false; }
  }
  onMount(() => { load(); return on((e) => { if (['todos.changed', 'library.changed', 'resync'].includes(e.type)) load(); }); });

  async function op(name: string, body: any): Promise<boolean> {
    let ok = true;
    try { await post(`/todos/${name}`, body); } catch (e) { fail(e); ok = false; }
    await load();
    return ok;
  }

  async function add(file: string) {
    const text = drafts[file]?.trim();
    if (!text) return;
    drafts[file] = '';
    if (!(await op('add', { file, text }))) drafts[file] = text;
  }

  async function createFirst() {
    const text = first.trim();
    if (!text) return;
    first = '';
    if (!(await op('add', { file: 'TODO.md', text }))) first = text;
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
  function visible(l: any): any[] {
    const v = l.items.filter(matches);
    if (sort === 'due') return [...v].sort((a, b) => (a.due ?? '9999').localeCompare(b.due ?? '9999') || a.line - b.line);
    return v;
  }

  const all = $derived(lists.flatMap((l) => l.items.filter((i: any) => !i.done)));
  const open = $derived(all.length);
  const nOverdue = $derived(all.filter((i: any) => i.due && dueState(i.due) === 'overdue').length);
  const nSoon = $derived(all.filter((i: any) => i.due && ['today', 'soon'].includes(dueState(i.due))).length);
  const anyVisible = $derived(lists.some((l) => visible(l).length));
  const listName = (l: any) => (l.idea ? prettyName(String(l.idea)) : t('todo.general'));
  const FILTERS: Filter[] = ['all', 'soon', 'overdue'];
</script>

<div class="page">
  <div class="row top">
    <h1 class="grow">{t('todos')} <span class="pill count" aria-label={t('todo.openCount', { n: fmtNum(open) })}>{fmtNum(open)}</span></h1>
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
      <form class="row firstform" onsubmit={(e) => { e.preventDefault(); createFirst(); }}>
        <input class="grow" bind:value={first} placeholder={t('todo.firstPlaceholder')} aria-label={t('todo.firstPlaceholder')} dir="auto" />
        <button class="btn primary" disabled={!first.trim()}><Icon name="plus" size={14} /> {t('todo.add')}</button>
      </form>
    </div>
  {:else}
    <div class="row tools">
      <div class="seg" role="group" aria-label={t('todo.filterLabel')}>
        {#each FILTERS as f}
          <button class="segb" class:on={filter === f} aria-pressed={filter === f} onclick={() => (filter = f)}>
            {#if f === 'overdue' && nOverdue}<Icon name="warning" size={12} />{/if}
            {t(`todo.filter.${f}`)}
            {#if f === 'soon' && nSoon}<span class="n">{fmtNum(nSoon)}</span>{/if}
            {#if f === 'overdue' && nOverdue}<span class="n">{fmtNum(nOverdue)}</span>{/if}
          </button>
        {/each}
      </div>
      <span class="grow"></span>
      <label class="row inl" for="todo-sort">{t('todo.sortLabel')}</label>
      <select id="todo-sort" bind:value={sort}>
        <option value="list">{t('todo.sort.list')}</option>
        <option value="due">{t('todo.sort.due')}</option>
      </select>
      <label class="row inl"><input type="checkbox" bind:checked={showDone} /> {t('todo.showDone')}</label>
    </div>

    {#if !anyVisible && (filter !== 'all' || open === 0)}
      <div class="state">
        <Icon name={filter === 'all' ? 'check' : 'filter'} size={24} />
        <p>{t(filter === 'overdue' ? 'todo.noneOverdue' : filter === 'soon' ? 'todo.noneSoon' : 'todo.allDone')}</p>
        {#if filter !== 'all'}<button class="btn" onclick={() => (filter = 'all')}>{t('todo.showAll')}</button>{/if}
      </div>
    {/if}

    {#each lists as l (l.file)}
      {@const items = visible(l)}
      {#if items.length || filter === 'all'}
        <section class="card list" aria-label={listName(l)}>
          <div class="row head">
            <b class="grow" dir="auto">{listName(l)}</b>
            <button class="btn ghost sm" onclick={() => go({ kind: 'docs', path: l.idea ? `${l.idea}/README.md` : l.file })} title={t('todo.openSource')}><bdi>{l.file}</bdi></button>
          </div>
          {#each items as it, idx (key(l, it))}
            {@const k = key(l, it)}
            {#if sort === 'list' && it.group && (idx === 0 || it.group !== items[idx - 1]?.group)}<div class="group" dir="auto">{it.group}</div>{/if}
            <div class="item" class:done={it.done}>
              <div class="row line">
                <input type="checkbox" checked={it.done} aria-label={t(it.done ? 'todo.markOpen' : 'todo.markDone', { title: it.title })}
                  onchange={() => op('toggle', ref(l, it))} />
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
                  <input id="due-{k}" type="date" value={it.due ?? ''} onchange={(e) => setDue(l, it, (e.target as HTMLInputElement).value)} />
                  <button class="btn sm" onclick={() => setDue(l, it, isoDay())}>{t('todo.today')}</button>
                  <button class="btn sm" onclick={() => setDue(l, it, isoDay(1))}>{t('todo.tomorrow')}</button>
                  <button class="btn sm" onclick={() => setDue(l, it, isoDay(7))}>{t('todo.nextWeek')}</button>
                  {#if it.due}<button class="btn ghost sm" onclick={() => setDue(l, it, null)}>{t('todo.clearDue')}</button>{/if}
                  <button class="ib" onclick={() => (dueFor = null)} aria-label={t('close')}><Icon name="close" size={14} /></button>
                </div>
              {/if}
            </div>
          {:else}
            <p class="muted none">{t('todo.listEmpty')}</p>
          {/each}
          <form class="row add" onsubmit={(e) => { e.preventDefault(); add(l.file); }}>
            <input class="grow" placeholder={t('addTodo')} aria-label={t('todo.addTo', { list: listName(l) })} bind:value={drafts[l.file]} dir="auto" />
            {#if drafts[l.file]?.trim()}<button class="btn primary sm">{t('todo.add')}</button>{/if}
          </form>
        </section>
      {/if}
    {/each}
  {/if}
</div>

<style>
  .top h1 { display: flex; align-items: center; gap: 10px; }
  .count { font-size: 12px; }
  .tools { flex-wrap: wrap; gap: 10px; margin-bottom: 4px; }
  .tools select { width: auto; font-size: 13px; padding: 4px 8px; }
  .inl { margin: 0; font-size: 13px; color: var(--muted); white-space: nowrap; }
  .seg { display: inline-flex; padding: 3px; gap: 2px; border-radius: 99px; background: var(--fill); }
  .segb { font: inherit; font-size: 13px; font-weight: 500; color: var(--muted); background: none; border: 0; border-radius: 99px; padding: 4px 12px; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; }
  .segb.on { background: var(--bg); color: var(--fg); box-shadow: var(--shadow); }
  .segb .n { font-size: 12px; opacity: .8; }

  .list { margin-top: 12px; }
  .head { margin-bottom: 6px; }
  .group { font-size: 12px; font-weight: 600; letter-spacing: .02em; color: var(--muted); margin: 10px 0 2px; }
  .item { padding: 3px 0; }
  .line { min-height: 30px; }
  .item.done .txt { text-decoration: line-through; color: var(--muted); }
  .txt { cursor: text; overflow-wrap: anywhere; }
  .acts { display: inline-flex; align-items: center; gap: 2px; opacity: .35; transition: opacity .15s; }
  .item:hover .acts, .acts:focus-within { opacity: 1; }
  @media (hover: none) { .acts { opacity: 1; } }
  .ib { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 5px; cursor: pointer; display: grid; place-items: center; }
  .ib:hover { color: var(--fg); background: var(--fill); }
  .due { font: inherit; font-size: 12px; font-weight: 500; color: var(--muted); background: var(--fill); border: 1px solid transparent; border-radius: 99px; padding: 2px 9px; cursor: pointer; display: inline-flex; align-items: center; gap: 5px; white-space: nowrap; }
  .due.today, .due.soon { color: var(--accent); background: var(--tint); }
  .due.overdue { color: var(--danger); border-color: currentColor; background: color-mix(in srgb, var(--danger) 10%, transparent); }
  .duebar { flex-wrap: wrap; margin: 4px 0 6px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); margin-inline-start: 24px; }
  .duebar input[type="date"] { width: auto; padding: 4px 8px; font-size: 13px; background: var(--bg); }
  .add input { border-style: dashed; margin-top: 6px; }
  .add .btn { margin-top: 6px; }
  .none { margin: 4px 0; font-size: 13px; }
  .sm { font-size: 12px; padding: 3px 9px; }

  .state { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 8px; padding: 40px 0; color: var(--muted); }
  .state h2 { margin: 4px 0 0; color: var(--fg); }
  .state p { margin: 0; max-width: 420px; }
  .firstform { width: min(460px, 100%); margin-top: 10px; }
  .skel { height: 14px; margin: 8px 0; border-radius: 8px; background: var(--fill); }
  .w70 { width: 70%; } .w50 { width: 50%; }
</style>
