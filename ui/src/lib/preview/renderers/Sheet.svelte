<script lang="ts">
  // A spreadsheet (Excel, OpenDocument) or a CSV/TSV file as a table, one sheet at a time with tabs.
  // The file is read in a Web Worker (office/sheetClient.ts); formulas are never worked out, each
  // cell shows the value saved in the file. Big sheets are cut (office/sheet.ts MAX_ROWS × MAX_COLS)
  // and say so.
  import { onDestroy, untrack } from 'svelte';
  import Icon from '../../Icon.svelte';
  import { t, fmtNum } from '../../i18n';
  import { openSheets, type SheetClient } from '../office/sheetClient';
  import { colName, type SheetData, type SheetInfo } from '../office/sheet';
  import { problemOf } from '../office/safezip';
  import { problemText } from '../office/problems';
  import type { RendererProps } from '../renderers';

  let { data, name, kind, fill, unsupported }: RendererProps = $props();
  const uid = $props.id();

  let sheets = $state<SheetInfo[] | null>(null);
  let active = $state(0);
  let sheet = $state<SheetData | null>(null);
  let sheetFailed = $state(false);
  let client: SheetClient | null = null;
  const cache = new Map<number, SheetData>();
  let seq = 0;
  let tabEls: HTMLButtonElement[] = $state([]);

  async function open(b: Blob) {
    const n = ++seq;
    client?.close(); client = null; cache.clear();
    sheets = null; sheet = null; active = 0; sheetFailed = false;
    try {
      const k = kind === 'ods' ? 'ods' : kind === 'csv' ? 'csv' : 'xlsx';
      const r = await openSheets(k, b, name);
      if (n !== seq) { r.client.close(); return; }
      client = r.client;
      if (!r.sheets.length) { unsupported(t('libf-preview-office.sheet.none')); return; }
      sheets = r.sheets;
      await show(0);
    } catch (e) {
      if (n === seq) unsupported(problemText(problemOf(e)));
    }
  }

  async function show(i: number) {
    active = i;
    sheetFailed = false;
    const known = cache.get(i);
    if (known) { sheet = known; return; }
    sheet = null;
    const n = seq;
    try {
      const s = await client!.sheet(i);
      if (n !== seq || active !== i) return;
      cache.set(i, s);
      sheet = s;
    } catch (e) {
      if (n !== seq) return;
      if (problemOf(e) === 'bomb') { unsupported(problemText('bomb')); return; }
      sheetFailed = true;
    }
  }

  $effect(() => { const b = data; untrack(() => open(b)); });
  onDestroy(() => { seq++; client?.close(); client = null; });

  /** Arrow keys move between sheet tabs (mirrored in a right-to-left layout), Home/End to the ends. */
  function tabKey(e: KeyboardEvent, i: number) {
    if (!sheets) return;
    const rtl = getComputedStyle(e.currentTarget as Element).direction === 'rtl';
    const step = e.key === 'ArrowRight' ? (rtl ? -1 : 1) : e.key === 'ArrowLeft' ? (rtl ? 1 : -1) : 0;
    let to = -1;
    if (step) to = (i + step + sheets.length) % sheets.length;
    else if (e.key === 'Home') to = 0;
    else if (e.key === 'End') to = sheets.length - 1;
    if (to < 0) return;
    e.preventDefault();
    show(to);
    tabEls[to]?.focus();
  }

  const RTL = /[֐-ࣿיִ-﷿ﹰ-ﻼ]/g;
  const LTR = /[A-Za-zÀ-ɏͰ-ϿЀ-ӿ]/g;
  /** The sheet runs right to left when Excel says so, or when most of its letters are Arabic/Hebrew. */
  function dirOf(s: SheetData): 'rtl' | 'ltr' {
    if (s.rtl) return 'rtl';
    // Counted by cells, not letters: a column of long Arabic names doesn't outweigh English headings.
    let r = 0, l = 0, seen = 0;
    for (const row of s.rows) {
      for (const c of row) if (c && !c.num) {
        const a = c.v.match(RTL)?.length ?? 0, b = c.v.match(LTR)?.length ?? 0;
        if (a > b) r++; else if (b > a) l++;
        seen++;
      }
      if (seen > 2000) break;
    }
    return r > l ? 'rtl' : 'ltr';
  }

  type View = { head: (SheetData['rows'][number]) | null; body: { n: number; cells: { c: number; cell: SheetData['rows'][number][number]; rs: number; cs: number }[] }[] };
  /** Rows to draw: merged cells get their spans, the cells they cover are left out. */
  function view(s: SheetData): View {
    const covered = new Set<number>();
    const origin = new Map<number, { rs: number; cs: number }>();
    const key = (r: number, c: number) => r * 100000 + c;
    for (const m of s.merges) {
      origin.set(key(m.r, m.c), { rs: m.rs, cs: m.cs });
      for (let r = m.r; r < m.r + m.rs; r++) for (let c = m.c; c < m.c + m.cs; c++) if (r !== m.r || c !== m.c) covered.add(key(r, c));
    }
    const first = s.header ? 1 : 0;
    const body = s.rows.slice(first).map((row, i) => {
      const r = i + first;
      const cells = [];
      for (let c = 0; c < s.cols; c++) {
        if (covered.has(key(r, c))) continue;
        const o = origin.get(key(r, c));
        cells.push({ c, cell: row[c] ?? null, rs: o?.rs ?? 1, cs: o?.cs ?? 1 });
      }
      return { n: r + 1, cells };
    });
    return { head: s.header ? s.rows[0] : null, body };
  }

  const v = $derived(sheet ? view(sheet) : null);
  const dir = $derived(sheet ? dirOf(sheet) : 'ltr');
</script>

{#if sheets === null}
  <div class="pad" role="status"><span class="sr-only">{t('libf-preview-core.loading', { name })}</span><div class="skel"></div><div class="skel"></div><div class="skel w70"></div></div>
{:else}
  <div class="sheet" class:fill>
    {#if sheets.length > 1}
      <div class="tabs" role="tablist" aria-label={t('libf-preview-office.sheet.tabs')}>
        {#each sheets as s, i (i)}
          <button type="button" role="tab" id="{uid}-tab-{i}" bind:this={tabEls[i]} aria-selected={i === active} aria-controls="{uid}-panel" tabindex={i === active ? 0 : -1}
            class:on={i === active} onclick={() => show(i)} onkeydown={(e) => tabKey(e, i)}><bdi dir="auto">{s.name}</bdi></button>
        {/each}
      </div>
    {/if}
    <div class="panel" id="{uid}-panel" role={sheets.length > 1 ? 'tabpanel' : undefined} aria-labelledby={sheets.length > 1 ? `${uid}-tab-${active}` : undefined}>
      {#if sheetFailed}
        <p class="msg" role="alert"><Icon name="warning" size={16} /> {t('libf-preview-office.sheet.failed')}</p>
      {:else if !sheet || !v}
        <div class="pad" role="status"><span class="sr-only">{t('libf-preview-core.loading', { name: sheets[active]?.name ?? name })}</span><div class="skel"></div><div class="skel w70"></div></div>
      {:else if sheet.cols === 0 || (!sheet.rows.length)}
        <p class="muted msg">{t('libf-preview-office.sheet.empty')}</p>
      {:else}
        {#if sheet.totalRows > sheet.rows.length || sheet.totalCols > sheet.cols}
          <p class="muted sm cut" role="note">
            <Icon name="info" size={14} />
            {#if sheet.totalRows > sheet.rows.length}{t('libf-preview-office.sheet.cutRows', { n: fmtNum(sheet.rows.length), total: fmtNum(sheet.totalRows) })}{/if}
            {#if sheet.totalCols > sheet.cols}{' '}{t('libf-preview-office.sheet.cutCols', { n: fmtNum(sheet.cols), total: fmtNum(sheet.totalCols) })}{/if}
          </p>
        {/if}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div class="scroll" tabindex="0" role="region" aria-label={t('libf-preview-office.sheet.caption', { sheet: sheet.name, name })}>
          <table {dir}>
            <caption class="sr-only">{t('libf-preview-office.sheet.caption', { sheet: sheet.name, name })}</caption>
            <thead>
              <tr>
                <th scope="col" class="corner"><span class="sr-only">{t('libf-preview-office.sheet.row')}</span></th>
                {#each { length: sheet.cols } as _, c (c)}
                  {#if v.head}<th scope="col" dir="auto">{v.head[c]?.v ?? ''}</th>{:else}<th scope="col" class="letter">{colName(c)}</th>{/if}
                {/each}
              </tr>
            </thead>
            <tbody>
              {#each v.body as row (row.n)}
                <tr>
                  <th scope="row" class="rn">{row.n}</th>
                  {#each row.cells as x (x.c)}
                    <td class:num={x.cell?.num} class:long={!!x.cell && (x.cell.v.length > 40 || x.cell.v.includes('\n'))} rowspan={x.rs > 1 ? x.rs : undefined} colspan={x.cs > 1 ? x.cs : undefined} dir={x.cell?.num ? 'ltr' : 'auto'}>{x.cell?.v ?? ''}</td>
                  {/each}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .sheet { border: var(--border-w) solid var(--border); border-radius: var(--radius); background: var(--panel); overflow: hidden; display: flex; flex-direction: column; }
  .sheet.fill { height: 100%; border: 0; border-radius: 0; }
  .tabs { display: flex; gap: 2px; padding: 6px 8px 0; border-bottom: var(--border-w) solid var(--border); overflow-x: auto; flex: none; }
  .tabs button { border: var(--border-w) solid transparent; border-bottom: 0; background: none; color: var(--muted); padding: 6px 12px; border-radius: var(--radius) var(--radius) 0 0; cursor: pointer; white-space: nowrap; font: inherit; font-size: var(--fs-sm, 13px); max-width: 24ch; overflow: hidden; text-overflow: ellipsis; }
  .tabs button:hover { color: var(--fg); background: var(--fill); }
  .tabs button.on { color: var(--fg); background: var(--bg); border-color: var(--border); font-weight: 600; margin-bottom: calc(-1 * var(--border-w)); }
  .tabs button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .panel { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .cut { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; margin: 0; padding: 6px 12px; border-bottom: var(--border-w) solid var(--border); }
  .sm { font-size: var(--fs-xs); }
  .scroll { overflow: auto; max-height: 70vh; }
  .fill .scroll { max-height: none; flex: 1; min-height: 0; }
  .scroll:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  table { border-collapse: separate; border-spacing: 0; font-size: var(--fs-sm, 13px); font-variant-numeric: tabular-nums; }
  th, td { border-inline-end: 1px solid var(--fill-2); border-bottom: 1px solid var(--fill-2); padding: 4px 8px; vertical-align: top; }
  /* Short values stay on one line (columns widen, the table scrolls); long ones wrap in a wide column. */
  td { white-space: pre; text-align: start; }
  td.long { white-space: pre-wrap; overflow-wrap: break-word; min-width: 18em; max-width: 32em; }
  td.num { text-align: right; white-space: nowrap; }
  table[dir='rtl'] td.num { text-align: left; }
  thead th { position: sticky; top: 0; z-index: 1; background: linear-gradient(var(--fill), var(--fill)), var(--panel); color: var(--muted); font-weight: 600; font-size: var(--fs-xs); text-align: start; white-space: nowrap; }
  thead th.letter { text-align: center; }
  th.rn { position: sticky; inset-inline-start: 0; background: linear-gradient(var(--fill), var(--fill)), var(--panel); color: var(--muted); font-weight: 400; font-size: var(--fs-xs); text-align: end; min-width: 2.5em; }
  th.corner { inset-inline-start: 0; z-index: 2; }
  .msg { display: flex; align-items: center; gap: 8px; margin: 0; padding: 16px; }
  .pad { display: grid; gap: 10px; padding: 8px 0; }
  .skel { height: 13px; margin: 5px 6px; border-radius: 8px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .w70 { width: 70%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel { animation: none; } }
  :global([data-motion="reduce"]) .skel { animation: none; }
</style>
