<script lang="ts">
  // The one place a model is chosen (round 0.7.2): the chat's options, a plan's, the defaults in
  // Settings and a connected service's model all use this list. A button names the choice; it opens
  // a list with each model under the name its program gave it (the raw id underneath where the two
  // differ), the choice in force ticked, the program's default marked, models under their maker
  // when a service lists several makers', and — for a long list — a search box and the models used
  // lately on top. Typing always filters, and a name that is not listed can be typed and used.
  // Nothing here knows a model: rows come from `entriesFrom` (lib/models/picker.ts).
  import { tick } from 'svelte';
  import Icon from '../Icon.svelte';
  import { t, tn } from '../i18n';
  import { imeKey } from '../ime';
  import { LONG_LIST, saysDefault, showsId, moveBy, nameOf, placeList, recentModels, rememberModel, rowsOf, sectionsFor, startIndex, validName, type Box, type ModelEntry, type Row } from './picker';

  let { value, models, onpick, label, id = '', auto = t('models72.auto'), autoNote = '', placeholder = '', typed = true, scope = '', disabled = false, small = false, chip = false, note = '', describedby = undefined }: {
    /** The choice in force: a model's id, or nothing for "Automatic". */
    value: string | null | undefined;
    models: ModelEntry[];
    /** A choice was made (`null` = Automatic). Answer `false` when it could not be kept. */
    onpick: (v: string | null) => unknown;
    /** What is being chosen ("Model"), for screen readers. */
    label: string;
    id?: string;
    /** The name of "no choice"; '' leaves that row out. */
    auto?: string;
    /** What "no choice" means here, said under it. */
    autoNote?: string;
    /** What the button says while nothing is chosen, where that is not the name of "no choice". */
    placeholder?: string;
    /** A name outside the list may be typed. */
    typed?: boolean;
    /** Whose models these are, for "Recently used" (an assistant program's name). */
    scope?: string;
    disabled?: boolean;
    /** The compact size, inside a small menu. */
    small?: boolean;
    /** Drawn as one of the composer's round chips: as wide as its text, cut short when the row is tight. */
    chip?: boolean;
    /** One plain line shown in the list (the program could not list its models, say). */
    note?: string;
    describedby?: string;
  } = $props();

  const uid = Math.random().toString(36).slice(2, 8);
  let open = $state(false);
  let query = $state('');
  let active = $state(-1);
  let recents = $state<string[]>([]);
  let box = $state<Box | null>(null);
  let btn = $state<HTMLButtonElement>();
  let pop = $state<HTMLElement>();
  let input = $state<HTMLInputElement>();
  let list = $state<HTMLElement>();

  const long = $derived(models.length >= LONG_LIST);
  const sections = $derived(sectionsFor(models, { value, query, recents, auto, typed }));
  const rows = $derived(rowsOf(sections));
  const shown = $derived(value ? nameOf(models, value) : placeholder || auto || t('models72.auto'));
  const q = $derived(query.trim());
  /** Something is typed that can be neither found nor used as a name. */
  const bad = $derived(!!q && !rows.length && typed && !validName(q));

  /** The choice in force is shown on top (a long list): its row further down keeps the tick, not the colour. */
  const echoed = $derived(rows.some((r) => r.key.startsWith('c:')));
  const rowId = (i: number) => `mp-${uid}-o${i}`;
  const indexOf = (r: Row) => rows.indexOf(r);

  function place() {
    if (!btn) return;
    const r = btn.getBoundingClientRect();
    box = placeList(r, { width: window.innerWidth, height: window.innerHeight }, { rtl: getComputedStyle(btn).direction === 'rtl', minWidth: 320, wanted: long ? 440 : 360 });
  }

  async function show() {
    if (disabled || open) return;
    query = ''; recents = scope ? recentModels(scope) : [];
    open = true; place();
    active = startIndex(rows, '');
    await tick();
    input?.focus({ preventScroll: true });
    reveal();
  }
  function close(refocus = true) {
    if (!open) return;
    open = false; query = '';
    if (refocus) btn?.focus({ preventScroll: true });
  }
  /** Bring the active row into view inside the list (never scrolling the page behind it). */
  function reveal() {
    const el = active >= 0 ? document.getElementById(rowId(active)) : null;
    if (!el || !list) return;
    const a = el.getBoundingClientRect(), b = list.getBoundingClientRect();
    if (a.top < b.top + 28) list.scrollTop -= b.top + 28 - a.top; // 28: room for a sticky heading
    else if (a.bottom > b.bottom) list.scrollTop += a.bottom - b.bottom;
    if (active === 0) list.scrollTop = 0;
  }

  async function pick(r: Row | undefined) {
    if (!r) return;
    const before = value || '';
    close();
    if (r.id === before) return;
    const kept = await onpick(r.id || null);
    if (kept !== false && r.id && scope) rememberModel(scope, r.id);
  }

  function typedIn() { active = startIndex(rows, query); if (list) list.scrollTop = 0; }

  function onkey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === 'Escape') {
      e.preventDefault(); e.stopPropagation();
      if (query) { query = ''; active = startIndex(rows, ''); tick().then(reveal); } else close();
      return;
    }
    if (e.key === 'Tab') { close(); return; }
    if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); pick(rows[active]); return; }
    // Home and End belong to the text while something is typed.
    if ((e.key === 'Home' || e.key === 'End') && query) return;
    const to = moveBy(e.key, active, rows.length);
    if (to === null) { e.stopPropagation(); return; } // typing never reaches the page's own shortcuts
    e.preventDefault(); e.stopPropagation();
    active = to; tick().then(reveal);
  }
  function btnKey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); show(); }
  }

  /** The list lives at the end of the page, so no scrolling box, card or menu it was opened from
   *  can cut it off; it is placed from the button and kept inside the window (`placeList`). */
  function atPageEnd(node: HTMLElement) {
    document.body.appendChild(node);
    // Heard on the list itself (not passed up to the page first), so a key used here stops here.
    node.addEventListener('keydown', onkey);
    return { destroy() { node.removeEventListener('keydown', onkey); node.remove(); } };
  }
  function outside(e: Event) {
    if (!open) return;
    const n = e.target as Node;
    if (pop?.contains(n) || btn?.contains(n)) return;
    close(false);
  }
  /** The page behind moved (a scroll that is not the list's own, a resize): follow the button. */
  function moved(e: Event) { if (open && !(e.target instanceof Node && pop?.contains(e.target))) place(); }
</script>

<svelte:window onpointerdown={outside} onresize={moved} />
<svelte:document onscrollcapture={moved} />

<button bind:this={btn} {id} type="button" class="mp-btn" class:small class:chip class:open aria-haspopup="listbox" aria-expanded={open} aria-controls={open ? `mp-${uid}-list` : undefined}
  aria-label={t('models72.button', { label, model: value ? shown : auto || shown })} title={chip ? shown : undefined} aria-describedby={describedby} {disabled} onclick={() => (open ? close() : show())} onkeydown={btnKey}>
  {#if chip}<Icon name="ai" size={13} />{/if}
  <span class="mp-val" class:unset={!value && !!placeholder} dir="auto">{shown}</span>
  <Icon name="down" size={small || chip ? 12 : 14} />
</button>

{#if open && box}
  <div bind:this={pop} use:atPageEnd class="mp-pop" class:above={box.above} class:small data-model-picker
    style:left="{box.left}px" style:top={box.top === null ? 'auto' : `${box.top}px`} style:bottom={box.bottom === null ? 'auto' : `${box.bottom}px`} style:width="{box.width}px" style:max-height="{box.maxHeight}px">
    <!-- Always there, so typing filters at once; it is only drawn for a long list or once something is typed. -->
    <div class="mp-search" class:quiet={!long && !query}>
      <Icon name="search" size={14} />
      <input bind:this={input} bind:value={query} oninput={typedIn} type="text" role="combobox" dir="auto" autocomplete="off" autocapitalize="off" spellcheck="false"
        aria-label={t('models72.search')} placeholder={t('models72.search')} aria-expanded="true" aria-autocomplete="list" aria-controls="mp-{uid}-list"
        aria-activedescendant={active >= 0 && rows[active] ? rowId(active) : undefined} />
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div bind:this={list} class="mp-list" id="mp-{uid}-list" role="listbox" aria-label={label} tabindex="-1" onmousedown={(e) => e.preventDefault()}>
      {#each sections as s (s.key)}
        <div class="mp-sec" role={s.label ? 'group' : 'presentation'} aria-labelledby={s.label ? `mp-${uid}-h-${s.key}` : undefined}>
          {#if s.label}
            <div class="mp-head" id="mp-{uid}-h-{s.key}">
              {#if s.kind === 'recent'}{t('models72.recent')}{:else if s.kind === 'all'}{t('models72.all')}{:else if s.kind === 'other'}{t('models72.other')}{:else}<bdi>{s.label}</bdi>{/if}
            </div>
          {/if}
          {#each s.rows as r (r.key)}
            {@const i = indexOf(r)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div class="mp-row" class:on={i === active} class:current={r.current} class:echo={r.current && echoed && r.key.startsWith('m:')} id={rowId(i)} role="option" tabindex="-1" aria-selected={r.current}
              onclick={() => pick(r)} onpointermove={() => { if (active !== i) active = i; }}>
              <span class="mp-text">
                {#if r.kind === 'auto'}
                  <span class="mp-name">{auto}</span>
                  {#if autoNote}<span class="mp-sub">{autoNote}</span>{/if}
                {:else if r.kind === 'typed'}
                  {#if r.current && !q}
                    <span class="mp-name"><bdi>{r.id}</bdi></span>
                    <span class="mp-sub">{t('models72.unlisted')}</span>
                  {:else}
                    <span class="mp-name">{t('models72.use', { name: r.id })}</span>
                    <span class="mp-sub">{t('models72.useNote')}</span>
                  {/if}
                {:else if r.entry}
                  <span class="mp-name"><bdi>{r.entry.name}</bdi>{#if r.entry.isDefault && !saysDefault(r.entry.name)}<span class="mp-pill">{t('models72.default')}</span>{/if}</span>
                  {#if showsId(r.entry)}<span class="mp-sub mp-id"><bdi>{r.entry.id}</bdi></span>{/if}
                  {#if r.entry.note}<span class="mp-sub">{r.entry.note}</span>{/if}
                {/if}
              </span>
              {#if r.current}<span class="mp-tick"><Icon name="check" size={15} /><span class="sr-only">{t('models72.current')}</span></span>{/if}
            </div>
          {/each}
        </div>
      {/each}
      {#if !rows.length || (!models.length && !q)}
        <p class="mp-none" role="status">{bad ? t('models72.badName') : q ? t('models72.none') : t(typed ? 'models72.empty' : 'models72.emptyFixed')}</p>
      {/if}
    </div>
    {#if note}<p class="mp-note" role="status">{note}</p>{/if}
    <!-- A long list says how many there are (or were found); every list says that typing works. -->
    <div class="mp-foot">
      <span class="mp-count" aria-live="polite">{#if q}{tn('models72.found', rows.filter((r) => r.kind === 'model').length)}{:else if long}{tn('models72.count', models.length)}{/if}</span>
      {#if !q}<span class="mp-hint">{t(typed ? 'models72.hintTyped' : 'models72.hint')}</span>{/if}
    </div>
  </div>
{/if}

<style>
  /* The button: a field like the app's own select boxes. */
  .mp-btn { font: inherit; color: var(--fg); background: var(--fill); border: var(--border-w) solid var(--control-border); border-radius: var(--radius);
    padding: 7px 10px; width: 100%; min-width: 0; display: flex; align-items: center; gap: 8px; cursor: pointer; text-align: start; transition: box-shadow .15s, background .15s; }
  .mp-btn:hover:not(:disabled) { background: var(--fill-2); }
  .mp-btn.open, .mp-btn:focus-visible { background: var(--bg); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 35%, transparent); outline-offset: 0; }
  .mp-btn:disabled { opacity: .55; cursor: default; }
  .mp-btn.small { font-size: var(--fs-xs); padding: 4px 8px; gap: 6px; }
  :global([data-theme="contrast"]) .mp-btn { border-color: var(--border); background: var(--bg); }
  .mp-val { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* The composer's chip: the look of its neighbour (the options button), never wider than its share of the row. */
  .mp-btn.chip { width: auto; max-width: 100%; font-size: var(--fs-xs); border-radius: 99px; border-color: transparent; padding: 4px 10px; gap: 5px; }
  .mp-btn.chip .mp-val { flex: 0 1 auto; max-width: 22ch; }
  .mp-btn.chip:hover:not(:disabled), .mp-btn.chip.open { background: var(--fill-2); box-shadow: none; }
  .mp-btn.chip:focus-visible { background: var(--fill-2); }
  :global([data-theme="contrast"]) .mp-btn.chip { border-color: var(--border); }
  .mp-note { flex: none; margin: 0; padding: 8px 12px; border-top: var(--border-w) solid var(--hairline); font-size: var(--fs-xs); background: var(--fill); }
  /* The name takes its own direction (a Latin model name is cut at its end, not its start, in Arabic too)
     but stays on the side the page reads from. */
  .mp-val { text-align: left; }
  :global([dir="rtl"]) .mp-val { text-align: right; }
  .mp-val.unset { color: var(--muted); }
  .mp-btn :global(svg) { flex: none; color: var(--muted); }

  /* The list: the app's menu surface (panel, hairline, large shadow), placed by the script. */
  .mp-pop { position: fixed; z-index: 1200; display: flex; flex-direction: column; min-height: 0; overflow: hidden; color: var(--fg); background: var(--panel);
    border: var(--border-w) solid var(--border); border-radius: var(--radius-lg); box-shadow: var(--shadow-lg); font-size: 13px; }
  @media (prefers-reduced-motion: no-preference) { .mp-pop { animation: mp-in .12s ease-out; } .mp-pop.above { animation-name: mp-in-up; } }
  :global(html[data-motion='reduce']) .mp-pop { animation: none; }
  @keyframes mp-in { from { opacity: 0; transform: translateY(-3px); } to { opacity: 1; transform: none; } }
  @keyframes mp-in-up { from { opacity: 0; transform: translateY(3px); } to { opacity: 1; transform: none; } }

  .mp-search { flex: none; display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: var(--border-w) solid var(--hairline); color: var(--muted); }
  .mp-search input { flex: 1; min-width: 0; font: inherit; color: var(--fg); background: transparent; border: 0; border-radius: 0; padding: 2px 0; box-shadow: none; outline: none; }
  /* A short list has no search box to look at, but the field is still what the keys go to. */
  .mp-search.quiet { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip-path: inset(50%); border: 0; }

  .mp-list { flex: 1 1 auto; min-height: 0; overflow-y: auto; padding: 6px; outline: none; }
  .mp-sec + .mp-sec { margin-top: 4px; padding-top: 4px; border-top: var(--border-w) solid var(--hairline); }
  .mp-head { position: sticky; top: -6px; z-index: 1; margin: 0 -6px; padding: 7px 16px 4px; font-size: var(--fs-xxs); font-weight: 600; letter-spacing: .02em; color: var(--muted); background: var(--panel); }
  .mp-row { display: flex; align-items: center; gap: 10px; padding: 6px 10px; border-radius: calc(var(--radius) - 1px); cursor: pointer; scroll-margin: 6px; }
  .mp-row.on { background: var(--fill-2); }
  .mp-row.current { background: var(--tint); color: var(--on-tint); }
  .mp-row.current.echo { background: transparent; color: inherit; }
  .mp-row.current.echo.on { background: var(--fill-2); }
  .mp-row.current.echo .mp-sub { color: var(--muted); }
  .mp-row.current.echo .mp-tick { color: var(--accent); }
  .mp-row.current.on { background: color-mix(in srgb, var(--accent) 24%, transparent); }
  :global([data-theme="contrast"]) .mp-row.on { outline: 2px solid var(--fg); outline-offset: -2px; }
  :global([data-theme="contrast"]) .mp-row.current, :global([data-theme="contrast"]) .mp-row.current .mp-sub { color: var(--accent-fg); }
  .mp-text { flex: 1; min-width: 0; display: grid; gap: 1px; }
  .mp-name { display: flex; align-items: center; gap: 8px; min-width: 0; font-weight: 500; line-height: 1.35; overflow-wrap: anywhere; }
  .mp-row.current .mp-name { font-weight: 600; }
  .mp-sub { font-size: var(--fs-xs); line-height: 1.35; color: var(--muted); overflow-wrap: anywhere; }
  .mp-row.current .mp-sub { color: color-mix(in srgb, var(--on-tint) 80%, var(--muted)); }
  /* Ids are Latin in every language, so the larger small sizes Arabic script needs do not apply. */
  .mp-id { font-family: var(--mono); font-size: 11px; }
  .mp-pill { flex: none; font-size: var(--fs-xxs); font-weight: 600; line-height: 1.5; padding: 0 7px; border-radius: 99px; color: var(--muted); border: var(--border-w) solid var(--control-border); white-space: nowrap; }
  .mp-row.current .mp-pill { color: inherit; border-color: currentColor; }
  .mp-tick { flex: none; display: inline-flex; }
  .mp-none { margin: 0; padding: 14px 10px; color: var(--muted); text-align: center; }

  .mp-foot { flex: none; display: flex; align-items: baseline; justify-content: space-between; gap: 12px; padding: 6px 12px; border-top: var(--border-w) solid var(--hairline);
    font-size: var(--fs-xxs); color: var(--muted); }
  .mp-count { white-space: nowrap; }
  .mp-count:empty { display: none; }
  .mp-hint { flex: 1; text-align: end; }
</style>
