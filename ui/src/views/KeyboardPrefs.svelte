<script lang="ts">
  // Settings › Keyboard: every shortcut of the app, the user's own keys for them (the layer's
  // `keys.json`, saved through `PUT /api/layer/keys` as one undoable History entry), and the keys
  // that stay fixed with the reason. The registry is lib/keys/registry.ts.
  import { onDestroy } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import QuickCapturePrefs from '../lib/QuickCapturePrefs.svelte';
  import { app, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { revertChange } from '../lib/layer';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { keyState } from '../lib/keys/state.svelte';
  import {
    allKeys, effective, defaultsOf, isChanged, label, labelOf, iso, refusal, conflicts, fromEvent, recording, searchText, flatQuery,
    type KeyDef, type Binding,
  } from '../lib/keys/registry';
  import { saveKeys, bareMenu } from '../lib/keys/native';

  // Both languages, so a search finds a shortcut by its English or its Arabic words.
  const dicts = import.meta.glob('../lib/i18n/*.ts', { eager: true }) as Record<string, { default: { en: Record<string, string>; ar?: Record<string, string> } }>;
  const EN: Record<string, string> = {}, AR: Record<string, string> = {};
  for (const m of Object.values(dicts)) { Object.assign(EN, m.default.en); Object.assign(AR, m.default.ar ?? {}); }

  const GROUPS = ['kb.g.general', 'kb.g.nav', 'kb.g.chats', 'kb.g.editor', 'kb.g.find', 'kb.g.graph', 'kb.g.mado', 'kb.g.pictures', 'kb.g.window', 'kb.g.anywhere', 'kb.g.addons', 'kb.g.fixed'];
  const MAX = 3;

  let query = $state('');
  let busy = $state(false);
  /** The row recording a key: `change` replaces its keys, `add` adds one. */
  let rec = $state<{ id: string; mode: 'change' | 'add' } | null>(null);
  let live = $state<Binding | null>(null);
  let problem = $state('');
  let clash = $state<{ id: string; b: Binding; others: { id: string; label: string }[] } | null>(null);
  let said = $state('');

  const words = (d: KeyDef) => [labelOf(d), d.title ? '' : EN[d.label] ?? '', d.title ? '' : AR[d.label] ?? ''].join(' ').toLowerCase();
  const matches = (d: KeyDef, q: string) => {
    if (!q) return true;
    const fq = flatQuery(q);
    if (effective(d.id).some((b) => searchText(b).includes(fq))) return true;
    const hay = words(d);
    return q.split(/\s+/).every((w) => hay.includes(w));
  };
  const rows = $derived.by(() => {
    keyState.overrides; app.commands; app.customThemes;
    const q = query.trim().toLowerCase();
    const list = allKeys().filter((d) => matches(d, q));
    return GROUPS.map((g) => ({ g, items: list.filter((d) => d.group === g) })).filter((x) => x.items.length);
  });

  /** keys.json's `bindings` with `id` set to `list` (an entry equal to the defaults is dropped). */
  function withList(map: Record<string, Binding[]>, id: string, list: Binding[] | null): Record<string, Binding[]> {
    const def = allKeys().find((d) => d.id === id);
    const next = { ...map };
    const dflt = def ? defaultsOf(def) : [];
    if (list === null || (list.length === dflt.length && list.every((b, i) => b === dflt[i]))) delete next[id];
    else next[id] = list;
    return next;
  }
  const current = (): Record<string, Binding[]> => Object.fromEntries(Object.entries(keyState.overrides).map(([k, v]) => [k, [...v]]));

  async function save(next: Record<string, Binding[]>, done: string) {
    busy = true;
    try {
      const r = await saveKeys(next);
      said = done;
      if (r?.hash) toast(done, { kind: 'success', key: 'keys', action: { label: t('undo'), run: () => { revertChange(r.hash!).then(() => toast(t('layer.undone'), { kind: 'success', key: 'keys' }), (e) => failed(t('layer.undoFailed'), e)); } } });
    } catch (e) { failed(t('kb.saveFailed'), e); }
    finally { busy = false; }
  }

  function start(id: string, mode: 'change' | 'add') {
    stop();
    rec = { id, mode }; live = null; problem = ''; clash = null; said = '';
    recording.on = true;
    bareMenu(true);
    window.addEventListener('keydown', onRec, true);
  }
  function stop() {
    if (!rec) return;
    window.removeEventListener('keydown', onRec, true);
    recording.on = false;
    bareMenu(false);
    rec = null; live = null; problem = '';
  }
  onDestroy(stop);

  function onRec(e: KeyboardEvent) {
    if (!rec) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.isComposing || e.repeat) return;
    const b = fromEvent(e);
    if (!b) return; // only modifier keys so far
    if (b === 'Escape') { stop(); return; }
    if (b === 'Backspace' && rec.mode === 'change') { const id = rec.id; stop(); void save(withList(current(), id, []), t('kb.saved.none', { name: nameOf(id) })); return; }
    if (b === 'Enter') { if (live && !problem) pick(live); return; }
    live = b;
    const why = refusal(rec.id, b);
    problem = why ? t(why) : '';
  }

  const nameOf = (id: string) => { const d = allKeys().find((x) => x.id === id); return d ? labelOf(d) : id; };

  function pick(b: Binding) {
    if (!rec) return;
    const { id, mode } = rec;
    const others = conflicts(id, b);
    stop();
    if (others.length) { clash = { id, b, others }; return; }
    void apply(id, mode, b, []);
  }

  async function apply(id: string, mode: 'change' | 'add', b: Binding, takeFrom: string[]) {
    clash = null;
    let next = current();
    for (const o of takeFrom) next = withList(next, o, effective(o).filter((x) => x !== b && label(x) !== label(b)));
    const mine = mode === 'add' ? [...effective(id).filter((x) => x !== b), b].slice(0, MAX) : [b];
    next = withList(next, id, mine);
    await save(next, t('kb.saved.one', { name: nameOf(id), keys: iso(label(b)) }));
  }

  async function remove(id: string, b: Binding) {
    await save(withList(current(), id, effective(id).filter((x) => x !== b)), t('kb.saved.removed', { name: nameOf(id), keys: iso(label(b)) }));
  }
  async function reset(id: string) { await save(withList(current(), id, null), t('kb.saved.reset', { name: nameOf(id) })); }
  async function resetAll() {
    if (!(await confirmDialog({ title: t('kb.resetAll.title'), body: t('kb.resetAll.body'), confirmLabel: t('kb.resetAll.confirm') }))) return;
    await save({}, t('kb.resetAll.done'));
  }
  const anyChanged = $derived(Object.keys(keyState.overrides).length > 0);
</script>

<div class="card kb">
  <p class="muted sm">{t('kb.intro')}</p>
  {#if app.safeMode}<p class="sm warn" role="note">{t('kb.safe')}</p>{/if}
  <div class="find">
    <label for="kb-q" class="sr-only">{t('kb.search')}</label>
    <Icon name="search" size={14} />
    <input id="kb-q" type="search" dir="auto" bind:value={query} placeholder={t('kb.search')} autocomplete="off" spellcheck="false" />
  </div>
  <p class="sr-only" role="status" aria-live="polite">{said}</p>

  {#each rows as group (group.g)}
    <section class="grp" aria-labelledby={'kb-' + group.g}>
      <h3 id={'kb-' + group.g}>{t(group.g)}</h3>
      {#if group.g === 'kb.g.anywhere'}
        <QuickCapturePrefs />
      {:else}
        <ul>
          {#each group.items as d (d.id)}
            {@const list = effective(d.id)}
            <li class="kr" class:fixed={!d.editable}>
              <div class="what">
                <span class="name" dir="auto">{labelOf(d)}</span>
                {#if !d.editable && d.fixedWhy}<span class="muted sm why">{t(d.fixedWhy)}</span>{/if}
              </div>
              <div class="chips">
                {#if rec?.id === d.id}
                  <span class="chip live" role="status" aria-live="polite">{#if live}<kbd dir="ltr">{iso(label(live))}</kbd>{:else}{t('kb.press')}{/if}</span>
                {:else if !list.length}
                  <span class="muted sm">{t('kb.none')}</span>
                {:else}
                  {#each list as b (b)}
                    <span class="chip"><kbd dir="ltr">{iso(label(b))}</kbd>{#if d.editable}<button type="button" class="x" disabled={busy} aria-label={t('kb.removeKey', { keys: iso(label(b)), name: labelOf(d) })} title={t('kb.removeKey', { keys: iso(label(b)), name: labelOf(d) })} onclick={() => remove(d.id, b)}><Icon name="close" size={11} /></button>{/if}</span>
                  {/each}
                {/if}
              </div>
              {#if d.editable}
                <div class="acts">
                  {#if rec?.id === d.id}
                    <button type="button" class="btn ghost sm" onclick={stop}>{t('cancel')}</button>
                  {:else}
                    <button type="button" class="btn sm" disabled={busy} onclick={() => start(d.id, 'change')} aria-label={t('kb.changeFor', { name: labelOf(d) })}>{t('kb.change')}</button>
                    {#if list.length < MAX}<button type="button" class="btn ghost sm" disabled={busy} onclick={() => start(d.id, 'add')} aria-label={t('kb.addFor', { name: labelOf(d) })} title={t('kb.addFor', { name: labelOf(d) })}><Icon name="plus" size={13} /></button>{/if}
                    {#if isChanged(d.id)}<button type="button" class="btn ghost sm" disabled={busy} onclick={() => reset(d.id)} aria-label={t('kb.resetFor', { name: labelOf(d) })}>{t('kb.reset')}</button>{/if}
                  {/if}
                </div>
              {/if}
              {#if rec?.id === d.id}
                <p class="muted sm hint" class:bad={!!problem} role={problem ? 'alert' : undefined}>{problem || t('kb.recordHint')}</p>
              {/if}
              {#if clash?.id === d.id}
                <div class="clash" role="alert">
                  <p class="sm">{t('kb.clash', { others: clash.others.map((o) => o.label).join('، '), keys: iso(label(clash.b)) })}</p>
                  <div class="row">
                    <button type="button" class="btn sm" disabled={busy} onclick={() => clash && apply(clash.id, 'change', clash.b, clash.others.map((o) => o.id))}>{t('kb.clash.use')}</button>
                    <button type="button" class="btn ghost sm" onclick={() => (clash = null)}>{t('cancel')}</button>
                  </div>
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {:else}
    <p class="muted">{t('kb.noMatch')}</p>
  {/each}

  <div class="row foot">
    <span class="grow muted sm">{t('kb.where')}</span>
    <button type="button" class="btn ghost" disabled={busy || !anyChanged} onclick={resetAll}><Icon name="rollback" size={14} /> {t('kb.resetAll')}</button>
  </div>
</div>

<style>
  .kb .find { display: flex; align-items: center; gap: 6px; border: 1px solid var(--hairline); border-radius: var(--radius); padding-inline: 8px; margin-block: 8px 4px; }
  .kb .find input { border: 0; background: transparent; flex: 1; min-width: 0; padding-block: 6px; color: inherit; }
  .grp h3 { font-size: 13px; margin: 16px 0 4px; }
  .grp ul { list-style: none; margin: 0; padding: 0; }
  .kr { display: grid; grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr) auto; gap: 4px 10px; align-items: center; padding: 6px 0; border-top: 1px solid var(--hairline); }
  .kr .what { display: flex; flex-direction: column; min-width: 0; }
  .kr .why { font-size: 12px; }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; }
  .chip { display: inline-flex; align-items: center; gap: 2px; border: 1px solid var(--hairline); border-radius: 6px; padding: 1px 4px; background: var(--surface); }
  .chip.live { border-color: var(--accent); }
  .chip kbd { font-family: var(--mono); font-size: 12px; }
  .chip .x { border: 0; background: transparent; color: inherit; opacity: .65; cursor: pointer; padding: 1px; display: inline-flex; border-radius: 4px; }
  .chip .x:hover { opacity: 1; }
  .acts { display: flex; gap: 4px; justify-content: flex-end; }
  .hint, .clash { grid-column: 1 / -1; margin: 0; }
  .hint.bad { color: var(--danger); }
  .clash { border: 1px solid var(--hairline); border-radius: 8px; padding: 8px; }
  .warn { margin: 4px 0; }
  .foot { margin-top: 14px; gap: 8px; align-items: center; }
  @media (max-width: 640px) { .kr { grid-template-columns: 1fr; } .acts { justify-content: flex-start; } }
</style>
