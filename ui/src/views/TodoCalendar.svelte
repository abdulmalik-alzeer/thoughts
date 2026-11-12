<script lang="ts">
  // Month view of todos with due dates. The grid is one tab stop: arrow keys move by day and week,
  // Page Up / Page Down by month, Home / End to the ends of the week, Enter adds a todo on that day.
  // A click selects a day and lists its todos below (where each can be moved to another day);
  // a double-click or the day's + button adds one.
  import { tick } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { promptDialog } from '../lib/dialog.svelte';
  import { t, tn, fmtDate, fmtNum, lang } from '../lib/i18n';
  import { prettyName } from '../lib/content/util';
  import { clock } from '../lib/clock.svelte';

  let { lists, add, toggle, setDue }: {
    lists: any[];
    add: (text: string, due: string) => Promise<boolean>;
    toggle: (l: any, it: any, box?: HTMLInputElement) => void;
    setDue: (l: any, it: any, due: string | null) => void;
  } = $props();

  const today = $derived(clock.day);
  const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const parse = (s: string) => new Date(+s.slice(0, 4), +s.slice(5, 7) - 1, +s.slice(8, 10), 12);
  const shift = (s: string, days: number) => { const d = parse(s); d.setDate(d.getDate() + days); return iso(d); };
  const shiftMonth = (s: string, n: number) => {
    const d = parse(s); const day = d.getDate();
    d.setDate(1); d.setMonth(d.getMonth() + n);
    d.setDate(Math.min(day, new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate()));
    return iso(d);
  };

  let cursor = $state(clock.day); // the focused day; its month is the one shown
  let gridEl = $state<HTMLElement | null>(null);

  // Weeks start on Sunday in Arabic (the Saudi working week), Monday in English.
  const weekStart = $derived(lang() === 'ar' ? 0 : 1);
  const weeks = $derived.by(() => {
    const first = parse(cursor.slice(0, 8) + '01');
    const lead = (first.getDay() - weekStart + 7) % 7;
    const start = shift(iso(first), -lead);
    const daysInMonth = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
    const rows = Math.ceil((lead + daysInMonth) / 7);
    return Array.from({ length: rows }, (_, w) => Array.from({ length: 7 }, (_, d) => shift(start, w * 7 + d)));
  });
  const weekdays = $derived(weeks[0].map((d) => ({ short: fmtDate(parse(d), { weekday: 'short' }), long: fmtDate(parse(d), { weekday: 'long' }) })));
  const monthTitle = $derived(fmtDate(parse(cursor), { month: 'long', year: 'numeric' }));

  type Row = { l: any; it: any };
  const byDay = $derived.by(() => {
    const m = new Map<string, Row[]>();
    for (const l of lists) for (const it of l.items) {
      if (!it.due) continue;
      if (!m.has(it.due)) m.set(it.due, []);
      m.get(it.due)!.push({ l, it });
    }
    return m;
  });
  const openOn = (d: string) => (byDay.get(d) ?? []).filter((r) => !r.it.done);
  const isOverdue = (d: string) => d < today && openOn(d).length > 0;
  const undated = $derived(lists.reduce((n, l) => n + l.items.filter((i: any) => !i.done && !i.due).length, 0));
  const selected = $derived(byDay.get(cursor) ?? []);

  function dayLabel(d: string): string {
    const n = openOn(d).length;
    const parts = [fmtDate(parse(d), { weekday: 'long', day: 'numeric', month: 'long' })];
    if (d === today) parts.push(t('todo.dueToday'));
    parts.push(n ? tn('ui.cal.dayCount', n) : t('ui.cal.dayNone'));
    if (isOverdue(d)) parts.push(t('todo.overdue'));
    return parts.join(' — ');
  }

  async function move(to: string) {
    cursor = to;
    await tick();
    gridEl?.querySelector<HTMLElement>(`[data-day="${to}"]`)?.focus();
  }

  function onKey(e: KeyboardEvent) {
    const rtl = document.documentElement.dir === 'rtl';
    const col = weeks.flat().indexOf(cursor) % 7;
    const map: Record<string, () => string> = {
      ArrowLeft: () => shift(cursor, rtl ? 1 : -1), ArrowRight: () => shift(cursor, rtl ? -1 : 1),
      ArrowUp: () => shift(cursor, -7), ArrowDown: () => shift(cursor, 7),
      Home: () => shift(cursor, -col), End: () => shift(cursor, 6 - col),
      PageUp: () => shiftMonth(cursor, -1), PageDown: () => shiftMonth(cursor, 1),
    };
    if (map[e.key]) { e.preventDefault(); move(map[e.key]()); }
    else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); addOn(cursor); }
  }

  async function addOn(d: string) {
    cursor = d;
    const text = await promptDialog({ title: t('ui.cal.addTitle', { date: fmtDate(parse(d), { weekday: 'long', day: 'numeric', month: 'long' }) }), placeholder: t('addTodo'), confirmLabel: t('todo.add') });
    if (text?.trim()) await add(text.trim(), d);
    await tick();
    gridEl?.querySelector<HTMLElement>(`[data-day="${d}"]`)?.focus();
  }

  const listName = (l: any) => (l.idea ? prettyName(String(l.idea)) : t('todo.general'));
</script>

<div class="cal">
  <div class="row bar">
    <h2 class="grow" aria-live="polite">{monthTitle}</h2>
    <button class="btn" onclick={() => move(today)} disabled={cursor === today}>{t('todo.today')}</button>
    <button class="ib" aria-label={t('ui.cal.prev')} title={t('ui.cal.prev')} onclick={() => move(shiftMonth(cursor, -1))}><Icon name="left" class="flip-rtl" /></button>
    <button class="ib" aria-label={t('ui.cal.next')} title={t('ui.cal.next')} onclick={() => move(shiftMonth(cursor, 1))}><Icon name="right" class="flip-rtl" /></button>
  </div>
  <p class="sr-only" id="cal-help">{t('ui.cal.keys')}</p>

  <!-- The grid itself is not a tab stop: focus roves between the day buttons inside it. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_interactive_supports_focus -->
  <div class="grid card" role="grid" aria-label={monthTitle} aria-describedby="cal-help" bind:this={gridEl} onkeydown={onKey}>
    <div class="wk head" role="row">
      {#each weekdays as w (w.long)}<div class="wd" role="columnheader" aria-label={w.long}><span aria-hidden="true">{w.short}</span></div>{/each}
    </div>
    {#each weeks as week (week[0])}
      <div class="wk" role="row">
        {#each week as d (d)}
          {@const items = byDay.get(d) ?? []}
          {@const out = d.slice(0, 7) !== cursor.slice(0, 7)}
          <div class="cell" class:out class:today={d === today} class:sel={d === cursor} role="gridcell" aria-selected={d === cursor}>
            <button class="daybtn" data-day={d} tabindex={d === cursor ? 0 : -1} aria-label={dayLabel(d)} title={t('todoa.cal.dayHint')}
              onclick={() => (cursor = d)} ondblclick={() => addOn(d)} onfocus={() => (cursor = d)}>
              <span class="num">{fmtNum(+d.slice(8))}</span>
              {#if d === today}<span class="tag">{t('todo.dueToday')}</span>{/if}
              {#if isOverdue(d)}<span class="tag late"><Icon name="warning" size={11} /> {t('todo.overdue')}</span>{/if}
            </button>
            <button class="cadd" tabindex="-1" aria-label={t('ui.cal.addHere')} title={t('ui.cal.addHere')} onclick={() => addOn(d)}><Icon name="plus" size={12} /></button>
            <div class="items" aria-hidden="true">
              {#each items.slice(0, 3) as r (r.l.file + r.it.line)}
                <div class="ev" class:done={r.it.done} class:late={!r.it.done && d < today} dir="auto">{r.it.title}</div>
              {/each}
              {#if items.length > 3}<div class="ev morec">{tn('ui.cal.more', items.length - 3)}</div>{/if}
            </div>
          </div>
        {/each}
      </div>
    {/each}
  </div>

  <section class="card daylist" aria-label={fmtDate(parse(cursor), { dateStyle: 'full' })}>
    <div class="row">
      <b class="grow">{fmtDate(parse(cursor), { weekday: 'long', day: 'numeric', month: 'long' })}{#if cursor === today} · {t('todo.dueToday')}{/if}</b>
      <button class="btn" onclick={() => addOn(cursor)}><Icon name="plus" size={14} /> {t('ui.cal.addBtn')}</button>
    </div>
    {#each selected as r (r.l.file + r.it.line)}
      <label class="todo" class:done={r.it.done}>
        <input type="checkbox" checked={r.it.done} onchange={(e) => toggle(r.l, r.it, e.currentTarget)} />
        <span class="grow" dir="auto">{r.it.title}</span>
        {#if !r.it.done && cursor < today}<span class="tag late"><Icon name="warning" size={11} /> {t('todo.overdue')}</span>{/if}
        <span class="muted sm" dir="auto">{listName(r.l)}</span>
        <input type="date" class="mv" lang={lang()} value={r.it.due} aria-label={t('todoa.cal.move', { title: r.it.title })} title={t('todoa.cal.moveHint')}
          onchange={(e) => { const v = e.currentTarget.value; if (v && v !== r.it.due) setDue(r.l, r.it, v); }} />
      </label>
    {:else}
      <p class="muted sm none">{t('ui.cal.dayEmpty')}</p>
    {/each}
  </section>
  {#if undated}<p class="muted sm">{tn('ui.cal.undated', undated)}</p>{/if}
</div>

<style>
  .cal { margin-top: 8px; }
  .bar { margin-bottom: 8px; }
  .bar h2 { margin: 0; font-size: 18px; }
  .ib { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 7px; width: 30px; height: 30px; cursor: pointer; display: grid; place-items: center; }
  .ib:hover { color: var(--fg); background: var(--fill); }
  .grid { padding: 6px; display: grid; gap: 2px; }
  .wk { display: grid; grid-template-columns: repeat(7, minmax(0, 1fr)); gap: 2px; }
  .wd { font-size: var(--fs-xs); font-weight: 600; color: var(--muted); padding: 4px 6px; text-align: start; }
  .cell { position: relative; min-height: 88px; border-radius: 8px; border: var(--border-w) solid var(--hairline); display: flex; flex-direction: column; min-width: 0; }
  .cell.out { opacity: 1; background: var(--fill); }
  .cell.out .num { color: var(--muted); }
  .cell.sel { border-color: var(--control-border); }
  .cell.today { border-color: var(--accent); border-width: 2px; }
  .daybtn { font: inherit; color: inherit; background: none; border: 0; text-align: start; cursor: pointer; padding: 4px 6px 0; display: flex; flex-wrap: wrap; align-items: center; gap: 4px; border-radius: 7px; min-height: 26px; }
  /* The whole cell is the click target. */
  .daybtn::after { content: ''; position: absolute; inset: 0; border-radius: 7px; }
  .daybtn:hover::after { background: var(--fill); }
  .daybtn:focus-visible { outline: none; }
  .daybtn:focus-visible::after { outline: 2px solid var(--accent); outline-offset: -2px; }
  .num { font-size: 13px; font-weight: 600; }
  /* The + appears on the selected day and on hover; it is a mouse shortcut (Enter does the same from the keyboard). */
  .cadd { position: absolute; inset-block-start: 3px; inset-inline-end: 3px; z-index: 1; font: inherit; color: var(--muted); background: var(--bg); border: var(--border-w) solid var(--hairline); border-radius: 6px; width: 22px; height: 22px; padding: 0; display: grid; place-items: center; cursor: pointer; opacity: 0; }
  .cell:hover .cadd, .cell.sel .cadd { opacity: 1; }
  .cadd:hover { color: var(--fg); }
  .mv { width: auto; padding: 2px 6px; font-size: var(--fs-xs); }
  .tag { font-size: var(--fs-xxs); font-weight: 600; color: var(--on-tint); background: var(--tint); border-radius: 99px; padding: 0 6px; display: inline-flex; align-items: center; gap: 3px; white-space: nowrap; }
  .tag.late { color: var(--danger); background: none; border: 1px solid currentColor; }
  .items { padding: 2px 4px 4px; display: grid; gap: 2px; position: relative; pointer-events: none; }
  .ev { font-size: var(--fs-xs); line-height: 1.35; padding: 1px 5px; border-radius: 5px; background: var(--fill-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ev.late { border-inline-start: 3px solid var(--danger); }
  .ev.done { text-decoration: line-through; color: var(--muted); }
  .ev.morec { background: none; color: var(--muted); }
  .daylist { margin-top: 12px; }
  .todo { display: flex; align-items: center; gap: 10px; padding: 6px 0; margin: 0; font-size: 13.5px; color: var(--fg); cursor: pointer; }
  .todo.done .grow { text-decoration: line-through; color: var(--muted); }
  .none { margin: 8px 0 0; }
  .sm { font-size: var(--fs-xs); }
  @container main (max-width: 700px) { .cell { min-height: 64px; } .ev { display: none; } .ev:first-child { display: block; } }
</style>
