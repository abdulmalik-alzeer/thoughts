<script lang="ts">
  // "Edit sidebar": the person arranges the sidebar by hand — order (drag, or the up and down
  // buttons), show or hide, name, icon, group and the start page. Saving writes the layer's
  // `nav.json` through the service (`PUT /api/layer/nav`), which checks it as it checks a reviewed
  // change and keeps it as one history entry with an Undo, so it can be undone like any other.
  import Icon from '../lib/Icon.svelte';
  import { app, toast } from '../lib/store.svelte';
  import { t, lang } from '../lib/i18n';
  import { put } from '../lib/api';
  import { failed } from '../lib/errors';
  import { revertChange } from '../lib/layer';
  import { moved } from '../lib/navLayout';
  import REF from '../lib/layer/builtin-pages.json';
  import type { NavConfig } from '../lib/store.svelte';

  type Item = { id: string; alias?: string; label: string; icon: string; custom?: boolean };
  let { items, close }: { items: Item[]; close: () => void } = $props();

  const ICONS: string[] = (REF as any).icons;
  const NEVER_HIDDEN: string[] = (REF as any).nav.neverHidden;
  const FIXED: string[] = (REF as any).nav.fixedLook;
  const MAX_GROUPS: number = (REF as any).nav.maxGroups;
  const L = lang();
  const other = L === 'ar' ? 'en' : 'ar';

  type Row = { id: string; label: string; icon: string; hidden: boolean; name: string; otherName: string; iconPick: string; group: string };
  const nav: NavConfig = $state.snapshot(app.nav) ?? { hidden: [], order: [], labels: {} };
  const hiddenSet = new Set(nav.hidden ?? []);
  const groupOf = (id: string, alias?: string) => (nav.groups ?? []).find((g) => g.items.includes(id) || (!!alias && g.items.includes(alias)))?.id ?? '';
  // The current order: what nav.json lists first, then the rest as the sidebar has them.
  const ids = items.map((i) => i.id);
  const listed = (nav.order ?? []).map((o) => items.find((i) => i.id === o || i.alias === o)?.id).filter((x): x is string => !!x);
  const start = [...new Set([...listed, ...ids])];
  let rows = $state<Row[]>(start.map((id) => {
    const it = items.find((i) => i.id === id)!;
    const l = nav.labels?.[id] ?? (it.alias ? nav.labels?.[it.alias] : undefined) ?? {};
    return { id, label: it.label, icon: it.icon, hidden: hiddenSet.has(id) || (!!it.alias && hiddenSet.has(it.alias)), name: (l as any)[L] ?? '', otherName: (l as any)[other] ?? '', iconPick: nav.icons?.[id] ?? (it.alias ? nav.icons?.[it.alias] : undefined) ?? '', group: groupOf(id, it.alias) };
  }));
  let groups = $state<{ id: string; name: string; otherName: string }[]>((nav.groups ?? []).map((g) => ({ id: g.id, name: (g.label as any)[L] ?? '', otherName: (g.label as any)[other] ?? '' })));
  let startPage = $state(nav.start ?? '');
  let busy = $state(false);
  let problem = $state('');
  let drag = $state(-1);
  let announce = $state('');

  const rowLabel = (r: Row) => r.name.trim() || r.label;
  function move(i: number, to: number, focus = true) {
    if (to < 0 || to >= rows.length) return;
    rows = moved(rows, i, to);
    announce = t('sbe.moved', { name: rowLabel(rows[to]), n: to + 1, of: rows.length });
    if (focus) setTimeout(() => document.getElementById(`sbe-${to > i ? 'down' : 'up'}-${to}`)?.focus(), 0);
  }
  function addGroup() {
    if (groups.length >= MAX_GROUPS) return;
    let n = groups.length + 1;
    while (groups.some((g) => g.id === 'group-' + n)) n++;
    groups = [...groups, { id: 'group-' + n, name: '', otherName: '' }];
  }
  function removeGroup(id: string) { groups = groups.filter((g) => g.id !== id); rows = rows.map((r) => (r.group === id ? { ...r, group: '' } : r)); }

  /** The nav.json these choices make (`{}` is the usual sidebar). */
  function build(): Record<string, any> {
    const out: Record<string, any> = {};
    const order = rows.map((r) => r.id);
    if (order.join() !== ids.join()) out.order = order;
    const hidden = rows.filter((r) => r.hidden && !NEVER_HIDDEN.includes(r.id)).map((r) => r.id);
    if (hidden.length) out.hidden = hidden;
    const labels: Record<string, any> = {};
    for (const r of rows) {
      if (FIXED.includes(r.id)) continue;
      const l: Record<string, string> = {};
      if (r.name.trim()) l[L] = r.name.trim().slice(0, 40);
      if (r.otherName.trim()) l[other] = r.otherName.trim().slice(0, 40);
      if (Object.keys(l).length) labels[r.id] = l;
    }
    if (Object.keys(labels).length) out.labels = labels;
    const icons: Record<string, string> = {};
    for (const r of rows) if (r.iconPick && !FIXED.includes(r.id)) icons[r.id] = r.iconPick;
    if (Object.keys(icons).length) out.icons = icons;
    const gs = groups.map((g) => {
      const label: Record<string, string> = {};
      if (g.name.trim()) label[L] = g.name.trim().slice(0, 40);
      if (g.otherName.trim()) label[other] = g.otherName.trim().slice(0, 40);
      if (!Object.keys(label).length) label[L] = t('sbe.groupUnnamed');
      return { id: g.id, label, items: rows.filter((r) => r.group === g.id).map((r) => r.id) };
    }).filter((g) => g.items.length);
    if (gs.length) out.groups = gs;
    if (startPage && !hidden.includes(startPage)) out.start = startPage;
    return out;
  }

  async function save(nav: Record<string, any>) {
    busy = true; problem = '';
    try {
      const r = await put('/layer/nav', nav);
      close();
      if (r?.hash) toast(t('sbe.saved'), { kind: 'success', key: 'sidebar', action: { label: t('undo'), run: () => { revertChange(r.hash).then(() => toast(t('layer.undone'), { kind: 'success', key: 'sidebar' }), (e) => failed(t('layer.undoFailed'), e)); } } });
    } catch (e: any) { problem = e?.message ?? String(e); }
    finally { busy = false; }
  }
</script>

<div class="sbe" role="region" aria-label={t('sbe.title')}>
  <div class="head"><h2 class="grow">{t('sbe.title')}</h2></div>
  <p class="muted hint">{t('sbe.hint')}</p>
  <label class="startl" for="sbe-start">{t('sbe.start')}</label>
  <select id="sbe-start" bind:value={startPage}>
    <option value="">{t('home')}</option>
    {#each rows.filter((r) => !r.hidden && r.id !== 'home') as r (r.id)}<option value={r.id}>{rowLabel(r)}</option>{/each}
  </select>
  <ol class="rows">
    {#each rows as r, i (r.id)}
      <li class="row" class:off={r.hidden} class:over={drag >= 0 && drag !== i} draggable="true"
        ondragstart={(e) => { drag = i; e.dataTransfer?.setData('text/plain', r.id); }}
        ondragover={(e) => e.preventDefault()} ondrop={(e) => { e.preventDefault(); if (drag >= 0) move(drag, i, false); drag = -1; }} ondragend={() => (drag = -1)}>
        <span class="grip" aria-hidden="true">⋮⋮</span>
        <Icon name={r.iconPick || r.icon} size={15} />
        <div class="grow fields">
          {#if FIXED.includes(r.id)}
            <span class="fixed" dir="auto">{r.label}</span>
          {:else}
            <input value={r.name} placeholder={r.label} dir="auto" aria-label={t('sbe.name', { name: r.label })} maxlength="40" oninput={(e) => (rows[i].name = e.currentTarget.value)} />
            <div class="sub">
              <select aria-label={t('sbe.icon', { name: rowLabel(r) })} value={r.iconPick} onchange={(e) => (rows[i].iconPick = e.currentTarget.value)}>
                <option value="">{t('sbe.iconUsual')}</option>
                {#each ICONS as ic (ic)}<option value={ic}>{t('sbe.ic.' + ic) !== 'sbe.ic.' + ic ? t('sbe.ic.' + ic) : ic}</option>{/each}
              </select>
              {#if groups.length}
                <select aria-label={t('sbe.groupOf', { name: rowLabel(r) })} value={r.group} onchange={(e) => (rows[i].group = e.currentTarget.value)}>
                  <option value="">{t('sbe.noGroup')}</option>
                  {#each groups as g (g.id)}<option value={g.id}>{g.name || t('sbe.groupUnnamed')}</option>{/each}
                </select>
              {/if}
            </div>
          {/if}
        </div>
        {#if !NEVER_HIDDEN.includes(r.id)}
          <label class="show" title={t('sbe.show')}><input type="checkbox" checked={!r.hidden} onchange={(e) => (rows[i].hidden = !e.currentTarget.checked)} aria-label={t('sbe.showNamed', { name: rowLabel(r) })} /></label>
        {/if}
        <button id={'sbe-up-' + i} class="ib" disabled={i === 0} aria-label={t('sbe.up', { name: rowLabel(r) })} title={t('sbe.upShort')} onclick={() => move(i, i - 1)}><Icon name="send" size={13} /></button>
        <button id={'sbe-down-' + i} class="ib" disabled={i === rows.length - 1} aria-label={t('sbe.down', { name: rowLabel(r) })} title={t('sbe.downShort')} onclick={() => move(i, i + 1)}><Icon name="arrowDown" size={13} /></button>
      </li>
    {/each}
  </ol>
  <div class="groups">
    {#each groups as g, gi (g.id)}
      <div class="grp">
        <input value={g.name} placeholder={t('sbe.groupName')} aria-label={t('sbe.groupName')} dir="auto" maxlength="40" oninput={(e) => (groups[gi].name = e.currentTarget.value)} />
        <button class="ib" aria-label={t('sbe.groupRemove', { name: g.name || t('sbe.groupUnnamed') })} onclick={() => removeGroup(g.id)}><Icon name="close" size={13} /></button>
      </div>
    {/each}
    {#if groups.length < MAX_GROUPS}<button class="btn ghost sm" onclick={addGroup}><Icon name="plus" size={13} /> {t('sbe.groupAdd')}</button>{/if}
  </div>
  {#if problem}<p class="err" role="alert" dir="auto">{problem}</p>{/if}
  <div class="actions">
    <button class="btn primary sm" disabled={busy} aria-busy={busy} onclick={() => save(build())}>{t('save')}</button>
    <button class="btn sm" disabled={busy} onclick={close}>{t('cancel')}</button>
    <span class="grow"></span>
    <button class="btn ghost sm" disabled={busy} onclick={() => save({})} title={t('sbe.resetHint')}>{t('sbe.reset')}</button>
  </div>
  <div class="sr-only" role="status" aria-live="polite">{announce}</div>
</div>

<style>
  .sbe { display: flex; flex-direction: column; gap: 6px; padding: 8px 4px; font-size: 13px; }
  .head { display: flex; align-items: center; }
  .head h2 { margin: 0; font-size: 13px; }
  .hint { margin: 0; font-size: var(--fs-xs); }
  .startl { font-size: var(--fs-xs); color: var(--muted); }
  select, input { font-size: 12.5px; padding: 3px 6px; }
  .rows { list-style: none; margin: 4px 0 0; padding: 0; display: flex; flex-direction: column; gap: 3px; }
  .row { display: flex; align-items: flex-start; gap: 5px; padding: 4px; border-radius: 6px; border: 1px solid var(--hairline); background: var(--fill); }
  .row.off { opacity: .6; }
  .row.over { border-style: dashed; }
  .row :global(.icon) { flex: none; margin-top: 5px; }
  .grip { cursor: grab; color: var(--muted); font-size: 11px; margin-top: 4px; letter-spacing: -2px; user-select: none; }
  .fields { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .fields input { width: 100%; }
  .sub { display: flex; gap: 3px; flex-wrap: wrap; }
  .sub select { flex: 1; min-width: 0; }
  .fixed { padding: 3px 0; }
  .show { margin-top: 4px; }
  .ib { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 6px; padding: 4px; cursor: pointer; display: grid; place-items: center; }
  .ib:hover:not(:disabled) { color: var(--fg); background: var(--fill-2); }
  .ib:disabled { opacity: .35; cursor: default; }
  .groups { display: flex; flex-direction: column; gap: 4px; margin-top: 4px; }
  .grp { display: flex; gap: 4px; align-items: center; }
  .grp input { flex: 1; min-width: 0; }
  .err { color: var(--danger); font-size: var(--fs-xs); margin: 0; white-space: pre-line; }
  .actions { display: flex; gap: 6px; align-items: center; margin-top: 4px; }
</style>
