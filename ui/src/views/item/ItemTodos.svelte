<script lang="ts">
  // One item's own todo list (its todo.md), on the item's page: tick, add, edit, date, repeat, move
  // between its groups and up or down, delete with Undo. Same service calls and wording as the
  // Todos page, so the file is changed line by line in place exactly as there.
  import { tick, untrack } from 'svelte';
  import { post } from '../../lib/api';
  import { go, track } from '../../lib/store.svelte';
  import { t, tn, fmtHijri, lang } from '../../lib/i18n';
  import Icon from '../../lib/Icon.svelte';
  import ItemActions from '../../lib/ItemActions.svelte';
  import { dueLabel as dueLabelOn, dueState as dueStateOn, isoDay, normTodo, notify, withDue } from '../../lib/content/util';
  import { clock } from '../../lib/clock.svelte';
  import { nextWeekDay, showHijri } from '../../lib/weekPrefs.svelte';
  import { compose, doneGroupOf, inSentence, REPEATS } from '../../lib/todoline';
  import { itemTerm } from '../../lib/item';
  import type { AreaWord } from '../../lib/terms';
  import { isAt, reanchor, refOf, type TodoRef } from '../../lib/todoref';
  import { editLost, todoFailed } from '../../lib/todofail';
  import type { TodoItem } from '../../lib/types';

  let { list, dir, word, reload }: {
    /** `{file, exists, groups, items}` as the item page's service call returns it. */
    list: { file: string; exists: boolean; groups: string[]; items: TodoItem[] };
    dir: string;
    word: AreaWord;
    reload: () => Promise<void>;
  } = $props();

  const items = $derived((list.items ?? []).map(normTodo));
  const groups = $derived<string[]>(list.groups ?? []);
  let showDone = $state(false);
  let draft = $state('');
  let addGroup = $state('');
  // The open edit box, date picker and move bar belong to one todo by line AND text (todoref.ts), so
  // a reload that puts another todo on that line cannot point them — or their save — at it.
  let editing = $state<TodoRef | null>(null);
  let editText = $state('');
  let editFrom = '';
  let dueFor = $state<TodoRef | null>(null);
  let moveFor = $state<TodoRef | null>(null);
  let busy = $state(false);

  // After every reload the open boxes follow their todo to its new line, or close when it changed,
  // went away or has a twin with the same text. An edit that closes this way keeps the typed text in a toast.
  $effect(() => {
    const now = [{ file: list.file, items }];
    untrack(() => {
      if (editing) {
        const e = reanchor(editing, now);
        if (!e && editText.trim() && editText.trim() !== editFrom) editLost(editText.trim());
        if (e !== editing) editing = e;
      }
      if (dueFor) { const d = reanchor(dueFor, now); if (d !== dueFor) dueFor = d; }
      if (moveFor) { const m = reanchor(moveFor, now); if (m !== moveFor) moveFor = m; }
    });
  });

  const dueState = (d: string) => { clock.day; return dueStateOn(d); };
  const dueLabel = (d: string) => { clock.day; return dueLabelOn(d); };
  const open = $derived(items.filter((i: any) => !i.done));
  const finished = $derived(items.filter((i: any) => i.done));
  const shown = $derived(items.filter((i: any) => showDone || !i.done));
  const doneGroup = $derived(doneGroupOf({ groups }));
  const outsideDone = $derived(finished.filter((i: any) => !doneGroup || i.group !== doneGroup).length);

  const OP_FAIL: Record<string, string> = { add: 'ui.todo.addFailed', toggle: 'ui.todo.toggleFailed', edit: 'ui.todo.editFailed', delete: 'ui.todo.deleteFailed',
    restore: 'ui.todo.undoFailed', move: 'todob.moveFailed', reorder: 'todob.moveFailed', archive: 'todob.moveFailed' };
  /** One change to the list → the service's answer, or null when it failed (already reported). */
  async function op(name: string, body: any, kept?: string): Promise<any | null> {
    let res: any = null;
    try { res = (await post(`/todos/${name}`, { file: list.file, ...body })) ?? { ok: true }; }
    catch (e) { todoFailed(t(OP_FAIL[name] ?? 'ui.todo.editFailed'), e, kept); }
    await reload();
    return res;
  }
  const ref = (it: any) => ({ line: it.line, text: it.text });
  const here = (it: any): TodoRef => refOf(list.file, it);
  const at = (open: TodoRef | null, it: any) => isAt(open, list.file, it);
  /** The line and text captured when a box opened, when it is this row's; otherwise the row's own. */
  const held = (open: TodoRef | null, it: any) => (open && at(open, it) ? { line: open.line, text: open.text } : ref(it));

  async function toggle(it: any, box: HTMLInputElement) {
    const was = it.done;
    const res = await op('toggle', ref(it));
    if (!res) { box.checked = was; return; }
    if (was) return;
    track('todo.done');
    if (res.next) {
      // A repeat stays open (on its next date): the box the user ticked must show that again.
      box.checked = false;
      notify(t('todob.repeatNext', { date: inSentence(dueLabel(res.next)) }), { key: 'todo-done', action: { label: t('undo'), run: () => { op('edit', { line: res.line, text: res.text, new_text: res.previous }); } } });
      return;
    }
    const line = typeof res.line === 'number' ? res.line : it.line;
    notify(t('ui.todo.completed'), { key: 'todo-done', action: { label: t('undo'), run: () => { op('toggle', { line, text: it.text }); } } });
  }

  async function add() {
    const text = draft.trim();
    if (!text || busy) return;
    busy = true;
    draft = '';
    const res = await op('add', { text: withDue(text, null), dates: true, group: addGroup || null });
    busy = false;
    if (!res) { draft = text; return; }
    track('todo.add');
    const read = typeof res.text === 'string' ? /@due\((\d{4}-\d{2}-\d{2})\)\s*$/.exec(res.text)?.[1] : null;
    if (read && !/@due\(/.test(text)) notify(t('todob.dateRead', { date: inSentence(dueLabel(read)) }), { kind: 'success' });
    await tick();
    document.getElementById('itd-add')?.focus();
  }

  async function startEdit(it: any) {
    editing = here(it); editText = it.title; editFrom = it.title; dueFor = null; moveFor = null;
    await tick();
    document.getElementById('itd-edit')?.focus();
  }
  async function saveEdit(it: any) {
    const open = editing;
    const title = editText.trim();
    editing = null;
    if (!open || !title || title === it.title) return;
    await op('edit', { line: open.line, text: open.text, new_text: compose(title, it.every, it.due) }, title);
  }
  async function setDue(it: any, due: string | null) {
    const r = held(dueFor, it);
    dueFor = null;
    if ((due || null) === it.due) return;
    await op('edit', { ...r, new_text: compose(it.title, it.every, due || null) });
  }
  async function setRepeat(it: any, every: string | null) {
    if ((every || null) === (it.every ?? null)) return;
    const r = held(dueFor, it);
    dueFor = null;
    await op('edit', { ...r, new_text: compose(it.title, every || null, it.due) });
  }
  async function remove(it: any) {
    const res = await op('delete', ref(it));
    if (!res) return;
    const removed = res.removed;
    notify(t('todo.deleted'), { action: { label: t('undo'), run: () => {
      if (Array.isArray(removed?.lines) && removed.lines.length) op('restore', { line: removed.line, lines: removed.lines });
      else op('add', { text: it.text });
    } } });
  }
  async function moveGroup(it: any, group: string) {
    const r = held(moveFor, it);
    moveFor = null;
    const res = await op('move', { ...r, to_file: list.file, group });
    if (!res) return;
    notify(t('todob.moved', { place: group || t('item.todos.noGroup') }), { action: { label: t('undo'), run: () => { op('move', { line: res.line, text: it.text, to_file: list.file, group: it.group || '' }); } } });
  }
  const siblings = (it: any) => items.filter((x: any) => (x.group ?? '') === (it.group ?? '') && (showDone || !x.done || x.line === it.line));
  async function shift(it: any, dir: 'up' | 'down') {
    const res = await op('reorder', { ...held(moveFor, it), dir, skip_done: !showDone });
    if (!res || typeof res.line !== 'number') return;
    moveFor = { file: list.file, line: res.line, text: it.text };
    await tick();
    document.getElementById(`itd-${dir}`)?.focus();
  }
  async function archive() {
    const res = await op('archive', { group: doneGroup ?? t('todob.doneGroup') });
    if (res && res.moved) notify(tn('todob.archived', res.moved), { kind: 'success' });
  }
</script>

<div class="todos">
  {#if !items.length}
    <div class="state">
      <Icon name="todos" size={26} />
      <p class="lead" dir="auto">{itemTerm(word, 'itemTodosEmpty')}</p>
      <p class="muted sm">{t('item.todos.hint')}</p>
    </div>
  {:else if !open.length && !showDone}
    <div class="state small"><Icon name="check" size={22} /><p>{t('item.todos.allDone')}</p></div>
  {/if}

  {#if shown.length}
    <div class="card list">
      {#each shown as it, idx (it.line)}
        {#if groups.length && it.group && (idx === 0 || it.group !== shown[idx - 1]?.group)}<div class="group" dir="auto">{it.group}</div>{/if}
        <div class="item" class:done={it.done}>
          <div class="row line">
            <input type="checkbox" checked={it.done} aria-label={t(it.done ? 'todo.markOpen' : 'todo.markDone', { title: it.title })}
              onchange={(e) => toggle(it, e.currentTarget)} />
            {#if at(editing, it)}
              <form class="grow row" onsubmit={(e) => { e.preventDefault(); saveEdit(it); }}>
                <input id="itd-edit" class="grow" bind:value={editText} dir="auto" aria-label={t('todo.editLabel')}
                  onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); editing = null; } }} />
                <button class="btn primary sm">{t('save')}</button>
                <button class="btn sm" type="button" onclick={() => (editing = null)}>{t('cancel')}</button>
              </form>
            {:else}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <span class="txt grow" dir="auto" ondblclick={() => startEdit(it)}>{it.title}</span>
              {#if it.every}<span class="rep" title={t('todob.rep.' + it.every)}><Icon name="retry" size={11} /><span class="sr-only">{t('todob.repeats')}</span> {t('todob.rep.' + it.every)}</span>{/if}
              {#if it.due}
                {@const s = it.done ? 'later' : dueState(it.due)}
                <button class="due {s}" onclick={() => { moveFor = null; dueFor = at(dueFor, it) ? null : here(it); }} aria-expanded={at(dueFor, it)} title={t('todo.changeDue')}>
                  <Icon name={s === 'overdue' ? 'warning' : 'calendar'} size={12} />
                  {#if s === 'overdue'}<b>{t('todo.overdue')}</b> ·{/if}
                  {dueLabel(it.due)}
                  {#if showHijri()}<span class="hj" dir="auto">· {fmtHijri(it.due + 'T12:00:00', { day: 'numeric', month: 'short' })}</span>{/if}
                </button>
              {/if}
              <span class="acts">
                {#if !it.due}<button class="ib" onclick={() => { moveFor = null; dueFor = at(dueFor, it) ? null : here(it); }} aria-expanded={at(dueFor, it)} aria-label={t('todo.setDue')} title={t('todo.setDue')}><Icon name="calendar" size={14} /></button>{/if}
                <button class="ib" onclick={() => { dueFor = null; moveFor = at(moveFor, it) ? null : here(it); }} aria-expanded={at(moveFor, it)} aria-label={t('todob.moveItem', { title: it.title })} title={t('todob.move')}><Icon name="arrow" size={14} class="flip-rtl" /></button>
                <ItemActions target="todo" name={it.title} item={{ file: list.file, idea: dir, line: it.line, text: it.text, title: it.title, due: it.due, done: it.done, group: it.group ?? null }} />
                <button class="ib" onclick={() => startEdit(it)} aria-label={t('todo.editItem', { title: it.title })} title={t('edit')}><Icon name="edit" size={14} /></button>
                <button class="ib" onclick={() => remove(it)} aria-label={t('todo.deleteItem', { title: it.title })} title={t('delete')}><Icon name="trash" size={14} /></button>
              </span>
            {/if}
          </div>
          {#if at(dueFor, it)}
            <div class="row bar">
              <label class="inl" for="itd-due-{it.line}">{t('todo.dueDate')}</label>
              <input id="itd-due-{it.line}" type="date" lang={lang()} value={it.due ?? ''} onchange={(e) => setDue(it, (e.target as HTMLInputElement).value)} />
              <button class="btn sm" onclick={() => setDue(it, isoDay())}>{t('todo.today')}</button>
              <button class="btn sm" onclick={() => setDue(it, isoDay(1))}>{t('todo.tomorrow')}</button>
              <button class="btn sm" onclick={() => setDue(it, nextWeekDay())}>{t('todo.nextWeek')}</button>
              {#if it.due}<button class="btn ghost sm" onclick={() => setDue(it, null)}>{t('todo.clearDue')}</button>{/if}
              <label class="inl" for="itd-rep-{it.line}">{t('todob.repeat')}</label>
              <select id="itd-rep-{it.line}" class="sel" value={it.every ?? ''} onchange={(e) => setRepeat(it, e.currentTarget.value || null)}>
                <option value="">{t('todob.rep.none')}</option>
                {#each REPEATS as r}<option value={r}>{t('todob.rep.' + r)}</option>{/each}
              </select>
              <button class="ib" onclick={() => (dueFor = null)} aria-label={t('close')}><Icon name="close" size={14} /></button>
            </div>
          {/if}
          {#if at(moveFor, it)}
            {@const sib = siblings(it)}
            {@const pos = sib.findIndex((x: any) => x.line === it.line)}
            <div class="row bar">
              {#if groups.length}
                <label class="inl" for="itd-mv-{it.line}">{t('item.todos.moveGroup')}</label>
                <select id="itd-mv-{it.line}" class="sel" value={it.group ?? ''} onchange={(e) => moveGroup(it, e.currentTarget.value)}>
                  {#if !it.group}<option value="">{t('item.todos.noGroup')}</option>{/if}
                  {#each groups as g (g)}<option value={g}>{g}</option>{/each}
                </select>
              {/if}
              <button id="itd-up" class="btn sm" disabled={pos <= 0} onclick={() => shift(it, 'up')}><Icon name="send" size={12} /> {t('todob.up')}</button>
              <button id="itd-down" class="btn sm" disabled={pos < 0 || pos >= sib.length - 1} onclick={() => shift(it, 'down')}><Icon name="arrowDown" size={12} /> {t('todob.down')}</button>
              <button class="ib" onclick={() => (moveFor = null)} aria-label={t('close')}><Icon name="close" size={14} /></button>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <form class="row add" onsubmit={(e) => { e.preventDefault(); add(); }}>
    <input id="itd-add" class="grow" placeholder={t('todob.addPlaceholder')} aria-label={t('addTodo')} bind:value={draft} dir="auto" />
    {#if groups.length}
      <label class="inl" for="itd-group">{t('todob.addIn')}</label>
      <select id="itd-group" class="sel" bind:value={addGroup}>
        <option value="">{groups[0]}</option>
        {#each groups.slice(1) as g (g)}<option value={g}>{g}</option>{/each}
      </select>
    {/if}
    <button class="btn primary sm" disabled={!draft.trim() || busy}><Icon name="plus" size={13} /> {t('todo.add')}</button>
  </form>

  <div class="row foot">
    {#if finished.length}
      <button class="btn ghost sm" aria-pressed={showDone} onclick={() => (showDone = !showDone)}>{showDone ? t('item.todos.hideDone') : tn('item.todos.showDone', finished.length)}</button>
    {/if}
    {#if groups.length && outsideDone}<button class="btn ghost sm" onclick={archive} title={t('todob.archiveHint')}>{tn('todob.archive', outsideDone)}</button>{/if}
    <span class="grow"></span>
    <button class="btn ghost sm" onclick={() => go({ kind: 'todos' })} title={t('item.todos.openAllTip')}><Icon name="todos" size={13} /> {t('item.todos.openAll')}</button>
  </div>
</div>

<style>
  .state { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 6px; padding: 26px 0 14px; color: var(--muted); }
  .state.small { flex-direction: row; justify-content: center; padding: 14px 0; }
  .state p { margin: 0; max-width: 460px; }
  .lead { color: var(--fg); font-weight: 500; }
  .list { margin-top: 4px; }
  .group { font-size: var(--fs-xs); font-weight: 600; letter-spacing: .02em; color: var(--muted); margin: 10px 0 2px; }
  .group:first-child { margin-top: 0; }
  .item { padding: 3px 0; }
  .line { min-height: 30px; flex-wrap: wrap; }
  .item.done .txt { text-decoration: line-through; color: var(--muted); }
  .txt { cursor: text; overflow-wrap: anywhere; }
  .acts { display: inline-flex; align-items: center; gap: 2px; }
  .item:hover .ib, .acts:focus-within .ib { color: var(--fg); }
  .ib { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 5px; min-width: 24px; min-height: 24px; cursor: pointer; display: grid; place-items: center; }
  .ib:hover { color: var(--fg); background: var(--fill); }
  .rep { font-size: var(--fs-xs); color: var(--muted); display: inline-flex; align-items: center; gap: 3px; white-space: nowrap; }
  .hj { font-weight: 400; opacity: .85; }
  .due { font: inherit; font-size: var(--fs-xs); font-weight: 500; color: var(--muted); background: var(--fill); border: 1px solid transparent; border-radius: 99px; padding: 2px 9px; cursor: pointer; display: inline-flex; align-items: center; gap: 5px; white-space: nowrap; }
  .due.today, .due.soon { color: var(--on-tint); background: var(--tint); }
  .due.overdue { color: var(--danger); border-color: currentColor; background: color-mix(in srgb, var(--danger) 10%, transparent); }
  .bar { flex-wrap: wrap; margin: 4px 0 6px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); margin-inline-start: 24px; }
  .bar input[type="date"] { width: auto; padding: 4px 8px; font-size: 13px; background: var(--bg); }
  .inl { margin: 0; font-size: 13px; color: var(--muted); white-space: nowrap; }
  .sel { width: auto; max-width: 240px; font-size: 13px; padding: 4px 8px; }
  .add { margin-top: 10px; flex-wrap: wrap; }
  .add input { border-style: dashed; }
  .foot { margin-top: 8px; flex-wrap: wrap; }
  .sm { font-size: var(--fs-xs); }
  .btn.sm { padding: 3px 9px; }
</style>
