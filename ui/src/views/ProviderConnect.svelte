<script lang="ts">
  // Settings › Assistant: "Use another service" (round 0.7.2). One short page for both assistants:
  // what is connected now, a handful of ready-made services where only a key is pasted, and one form
  // (address, key, model) for any other. Each entry says which assistant it works with, because that
  // is a fact about the service: Codex reaches a service only at an OpenAI-compatible address, Claude
  // Code only at an Anthropic-compatible one. Nothing here is a guard: an address and its key are
  // saved together through the program's own route, and the background service decides every save
  // (`providers.rs`, `claude::set_provider_with`). What may be shown or sent is `lib/providers.ts`.
  import { onMount } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import HelpTip from '../lib/HelpTip.svelte';
  import { api, del, post, put } from '../lib/api';
  import { app, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import ModelPicker from '../lib/models/ModelPicker.svelte';
  import { entriesFrom } from '../lib/models/picker';
  import { confirmDialog } from '../lib/dialog.svelte';
  import {
    CUSTOM, PROGRAM_NAME, formState, keylessAllowed, onHomeNetwork, programsOf, savedAs, savedKind, startAddress, startProgram,
    destination, ltr, type Form, type Program, type Saved, type Service,
  } from '../lib/providers';

  let { ai = $bindable(), onsaved, savemodel, onmodels, readOnly = false }: {
    /** The Settings draft (`s.ai`): a program's model is an ordinary setting, saved like the others. */
    ai: any;
    /** After a save or a removal: Settings reads the server-owned fields again. */
    onsaved: () => Promise<void>;
    /** Save the draft now (Settings' own save, which queues behind one that is running). */
    savemodel: () => Promise<void>;
    /** A service's own list of models was loaded for this program. */
    onmodels?: (program: Program) => void;
    readOnly?: boolean;
  } = $props();

  const PROGRAMS: Program[] = ['claude', 'codex'];
  let services = $state<Service[]>([]);
  let ready = $state(false);
  onMount(() => { api<Service[]>('/ai/services').then((l) => (services = l)).catch(() => {}).finally(() => (ready = true)); });

  const saved = $derived<Record<Program, Saved>>({ claude: app.settings?.ai?.claude as Saved, codex: app.settings?.ai?.codex as Saved });
  const ids = $derived<Record<Program, string>>({ claude: savedAs('claude', saved.claude, services), codex: savedAs('codex', saved.codex, services) });
  const kinds = $derived({ claude: savedKind('claude', saved.claude), codex: savedKind('codex', saved.codex) });
  const installed = $derived<Record<Program, boolean>>({ claude: !!app.status?.claude?.installed, codex: !!app.status?.codex?.installed });
  const fullOn = $derived(!!app.settings?.fullAccess?.on);

  const name = (id: string) => t('prov72.svc.' + (id || CUSTOM));
  /** The second line of a card, for the two entries whose name alone does not say what they are. */
  const sub = (id: string) => (id === 'ollama' || id === CUSTOM ? t('prov72.sub.' + id) : '');
  const can = (id: string): Program[] => { const s = services.find((x) => x.id === id); return s ? programsOf(s) : ['codex', 'claude']; };
  // The generic entry is offered as what it is for nearly everyone: an OpenAI-compatible service (Codex).
  const works = (id: string) => { const c = can(id); return t('prov72.works.' + (id === CUSTOM ? 'codex' : c.length > 1 ? 'both' : c[0])); };
  const entries = $derived([...services.map((s) => s.id), CUSTOM]);

  // ----- the open form: one entry, for one program -----
  let open = $state<{ id: string; program: Program } | null>(null);
  let form = $state<Form>({ url: '', token: '', noKey: false });
  let model = $state('');
  let models = $state<string[]>([]);
  /** What the program said about its models besides their ids (its names, its default); none for a service's own list. */
  let said72 = $state<any>({});
  const modelEntries = $derived(entriesFrom({ ...said72, models }));
  /** `models` is the service's own list (not the program's usual one), so a name outside it is worth a word. */
  let fromService = $state(false);
  let listErr = $state('');
  let test = $state<{ ok: boolean; message?: string } | null>(null);
  let busy = $state<Record<string, boolean>>({});

  const st = $derived.by(() => {
    if (!open) return null;
    const s = formState(open.program, form, saved[open.program], fullOn);
    // The generic entry is always an address: without one it would read as "an Anthropic key".
    return open.id === CUSTOM && !destination(open.program, form.url) ? { ...s, canSave: false, canTest: false } : s;
  });
  const svc = $derived(open ? services.find((s) => s.id === open!.id) : undefined);
  /** This entry is what its program uses now. */
  const connected = $derived(!!open && ids[open.program] === open.id && kinds[open.program] !== 'own');
  /** Does a model belong to this entry? Not to Anthropic itself: Claude Code's own list applies, as always. */
  const hasModel = $derived(open?.id === CUSTOM || (!!open && !!destination(open.program, form.url)));
  /** A ready-made service shows the model once it is connected (its list is fetched then), so
   *  connecting is one field; the generic entry has address, key and model together. */
  const showModel = $derived(hasModel && (connected || open?.id === CUSTOM));
  const other = (p: Program): Program => (p === 'claude' ? 'codex' : 'claude');
  /** The names a sentence about what `p` uses now is made of. A service that is not ready-made is
   *  called by its address (kept left-to-right inside an Arabic sentence). */
  const said = (p: Program) => ({ program: PROGRAM_NAME[p], service: ids[p] === CUSTOM ? ltr(destination(p, saved[p]?.baseUrl)) : name(ids[p]) });

  function reset() {
    if (!open) return;
    const { id, program } = open;
    const mine = ids[program] === id;
    form = { url: startAddress(id, program, services, saved[program]), token: '', noKey: mine ? !!saved[program]?.keyless : !!services.find((s) => s.id === id)?.noKey };
    model = mine ? String(ai?.[program]?.model ?? '') : '';
    models = []; fromService = false; listErr = ''; test = null;
    if (mine && kinds[program] === 'service') knownModels(program);
  }
  function pick(id: string, program?: Program) {
    if (open?.id === id && !program) { open = null; return; }
    open = { id, program: program ?? startProgram(id, services, saved, ai?.primary, installed) };
    reset();
  }
  function useWith(p: Program) { if (open && open.program !== p) { open = { id: open.id, program: p }; reset(); } }

  /** The models already known for the saved service: asked of the background service only, nothing goes out. */
  function knownModels(program: Program) {
    api(`/ai/${program}/options`).then((o: any) => {
      if (open?.program !== program) return;
      models = Array.isArray(o?.models) ? o.models : [];
      said72 = { modelNames: o?.modelNames, modelInfo: o?.modelInfo, defaultModel: o?.defaultModel };
      fromService = program === 'claude' ? !!o?.modelsLoaded : !!o?.throughProvider && models.length > 0;
    }).catch(() => {});
  }

  /** The model typed in the form becomes the program's model. A blank one never wipes a model while a
   *  service is first being connected: only a form opened on the connected service may clear it. */
  async function applyModel(clearing: boolean) {
    if (!open || !hasModel || !ai?.[open.program]) return;
    const next = model.trim(), cur = String(ai[open.program].model ?? '');
    if (next === cur || (!next && !clearing)) return;
    ai[open.program].model = next || null;
    await savemodel();
  }

  async function act(key: string, headline: string, fn: () => Promise<void>) {
    if (busy[key]) return;
    busy[key] = true;
    try { await fn(); } catch (e) { failed(headline, e); }
    finally { busy[key] = false; }
  }

  const save = () => act('save', t('prov72.saveFailed'), async () => {
    if (!open || !st?.canSave) return;
    const { id, program } = open, was = connected;
    const r = await put<{ provider: string; keyRemoved?: boolean }>(`/ai/${program}/provider`, st.body);
    form.token = ''; test = null;
    await onsaved();
    // The form now shows what is saved, so it no longer counts as edited.
    form.url = String((app.settings?.ai as any)?.[program]?.baseUrl ?? ''); form.noKey = !!(app.settings?.ai as any)?.[program]?.keyless;
    await applyModel(was);
    toast(t('prov72.saved', { program: PROGRAM_NAME[program], service: name(id) }) + (r.keyRemoved ? ' ' + t('runa.prov.oldKeyRemoved') : ''), { kind: 'success' });
    // Straight after connecting, the service is asked for its models with the key just saved: it is
    // what the user needs next, and a key the service refuses shows at once.
    if (r.provider === 'service') await loadModels();
  });

  async function loadModels() {
    if (!open || busy.models) return;
    const program = open.program;
    busy.models = true; listErr = '';
    try {
      const r = await post<{ models: string[] }>(`/ai/${program}/provider/models`);
      if (open?.program === program) { models = r.models; said72 = {}; fromService = true; }
      onmodels?.(program);
    } catch (e) { if (open?.program === program) listErr = e instanceof Error ? e.message : String(e); }
    finally { busy.models = false; }
  }

  const tryIt = () => act('test', t('prov72.test'), async () => {
    if (!open || !st?.canTest) return;
    test = null;
    await applyModel(true); // the test uses the saved model
    test = await post(`/ai/${open.program}/provider/test`, {});
  });

  async function remove(program: Program) {
    if (kinds[program] === 'own' || readOnly) return;
    const w = said(program);
    if (!(await confirmDialog({ title: t('prov72.removeTitle', w), body: t('prov72.removeBody', w), confirmLabel: t('prov72.remove'), danger: true }))) return;
    await act('save', t('prov72.saveFailed'), async () => {
      await del(`/ai/${program}/provider`);
      await onsaved();
      if (open?.program === program) open = null;
      toast(t('prov72.removed', w));
    });
  }

  const modelNote = $derived.by(() => {
    if (!open || !showModel) return '';
    if (busy.models) return t('prov72.modelLoading');
    if (listErr) return `${listErr} ${t('prov72.modelNone')}`;
    const m = model.trim();
    if (fromService && models.length) return m && !models.includes(m) ? t('prov72.modelUnknown', { model: m }) : m ? '' : t('prov72.modelCount', { n: models.length });
    return connected && !m ? t('prov72.modelChoose') : '';
  });

  // ----- what Settings asks before the page is left (a pasted key would be lost without a word) -----
  /** The fields holding something typed and not saved, by their labels. */
  export function typed(): string[] {
    if (!open || readOnly) return [];
    const addr = open.id === CUSTOM && form.url.trim() !== startAddress(open.id, open.program, services, saved[open.program]);
    return [...(form.token.trim() ? [t('prov72.key')] : []), ...(addr ? [t('prov72.address')] : [])];
  }
  export const canSaveTyped = () => !!st?.canSave;
  /** Save what is typed; `true` when nothing is left unsaved. */
  export async function saveTyped(): Promise<boolean> { await save(); return typed().length === 0; }
  export function dropTyped() { open = null; form = { url: '', token: '', noKey: false }; }
</script>

<h3>{t('prov72.title')} <HelpTip text={t('prov72.help')} label={t('prov72.title')} /></h3>

<!-- What is connected now, one line for each assistant that uses a service. -->
{#each PROGRAMS as p (p)}
  {#if kinds[p] !== 'own' && ready}
    {@const w = said(p)}
    <div class="now">
      <span class="what" dir="auto">{t('prov72.now', w)}</span>
      <span class="pill" class:accent={kinds[p] === 'service'}>{t('prov72.state.' + (kinds[p] === 'incomplete' ? 'missing' : saved[p]?.hasApiToken ? 'key' : 'noKey'))}</span>
      <button type="button" class="btn ghost tiny" disabled={readOnly} onclick={() => pick(ids[p], p)}>{t('prov72.change')}</button>
      <button type="button" class="btn ghost tiny danger" disabled={readOnly || busy.save} onclick={() => remove(p)}>{t('prov72.remove')}</button>
    </div>
    {#if kinds[p] === 'incomplete'}<p class="err" role="status">{t('prov72.missing', w)}</p>{/if}
  {/if}
{/each}
{#if ready && kinds.claude === 'own' && kinds.codex === 'own'}<p class="muted sm">{t('prov72.none')}</p>{/if}

{#if ready}
  <div class="pick" role="group" aria-labelledby="pv-pick">
    <span class="muted sm" id="pv-pick">{t('prov72.pick')} <HelpTip text={t('prov72.worksHelp')} label={t('prov72.pick')} /></span>
    <div class="cards">
      {#each entries as id (id)}
        <button type="button" class="svc" class:on={open?.id === id} aria-pressed={open?.id === id} disabled={readOnly} onclick={() => pick(id)}>
          <b><bdi>{name(id)}</bdi></b>
          {#if sub(id)}<span class="sub">{sub(id)}</span>{/if}
          <span class="works">{works(id)}</span>
        </button>
      {/each}
    </div>
  </div>
{/if}

{#if open && st}
  {@const program = open.program}
  {@const pn = PROGRAM_NAME[program]}
  <div class="form" role="group" aria-label={name(open.id)}>
    <p class="about" dir="auto">{t('prov72.about.' + open.id)}
      {#if svc?.keysUrl}<a href={svc.keysUrl} target="_blank" rel="noreferrer">{t('prov72.getKey')} <Icon name="external" size={13} /></a>{/if}</p>

    {#if can(open.id).length > 1}
      <fieldset class="with">
        <legend>{t('prov72.useWith')} <HelpTip text={t('prov72.worksHelp')} label={t('prov72.useWith')} /></legend>
        {#each can(open.id) as p (p)}
          <label class="radio"><input type="radio" name="pv-with" value={p} checked={program === p} disabled={readOnly} onchange={() => useWith(p)} />
            <span><bdi>{PROGRAM_NAME[p]}</bdi>{#if open.id === CUSTOM}<span class="muted fmt">({t('prov72.format.' + p)})</span>{/if}</span></label>
        {/each}
      </fieldset>
    {:else}
      <p class="sm"><b>{t('prov72.works.' + program)}</b></p>
    {/if}
    {#if !installed[program]}<p class="muted sm" role="note">{t('prov72.notInstalled', { program: pn })}</p>
    {:else if ai?.primary && ai.primary !== program}<p class="muted sm" role="note">{t('prov72.notPrimary', { program: pn, other: PROGRAM_NAME[other(program)] })}</p>{/if}
    {#if kinds[program] !== 'own' && ids[program] !== open.id}<p class="muted sm" role="note">{t('prov72.replaces', said(program))}</p>{/if}

    {#if open.id === CUSTOM}
      <label for="pv-url">{t('prov72.address')} <HelpTip text={t('prov72.addressHelp.' + program)} /></label>
      <input id="pv-url" dir="ltr" bind:value={form.url} placeholder={program === 'codex' ? 'https://api.example.com/v1' : 'https://api.example.com/anthropic'} aria-invalid={st.badUrl} aria-describedby={st.badUrl ? 'pv-url-err' : undefined} autocomplete="off" disabled={readOnly} />
      {#if st.badUrl}<p class="err" id="pv-url-err" role="alert">{t('set.errUrl')}</p>{/if}
      {#if onHomeNetwork(form.url, fullOn)}<p class="muted sm" role="note">{t('prov5.lan')}</p>{/if}
      {#if keylessAllowed(form.url, fullOn)}
        <label class="check"><input type="checkbox" bind:checked={form.noKey} disabled={readOnly} /> {t('prov5.noKey')} <HelpTip text={t('prov72.noKeyHelp')} /></label>
      {/if}
    {/if}

    {#if !svc?.noKey}
      <label for="pv-key">{t('prov72.key')} <HelpTip text={t('prov72.keyHelp')} />
        {#if connected && saved[program]?.hasApiToken}<span class="pill accent">{t('prov72.state.key')}</span>{/if}</label>
      <input id="pv-key" dir="ltr" type="password" bind:value={form.token} placeholder={connected && saved[program]?.hasApiToken ? t('prov72.keyReplace') : t('prov72.keyPaste')} autocomplete="off" disabled={readOnly || st.noKey} />
      {#if st.keyMoved}<p class="muted sm" role="status">{t('prov72.keyMoved')}</p>{/if}
    {/if}

    {#if showModel}
      <label for="pv-model">{t('prov72.model')} <HelpTip text={t('prov72.modelHelp')} /></label>
      <div class="row wrap">
        <span class="model"><ModelPicker id="pv-model" value={model.trim() || null} models={modelEntries} label={t('prov72.model')} placeholder={t('prov72.modelAuto')} auto={t('models72.unset')} autoNote={t('models72.unsetNote')} scope={program}
          disabled={readOnly} describedby={modelNote ? 'pv-model-note' : undefined} onpick={(v) => { model = v ?? ''; if (connected && !st?.dirty) return applyModel(true); }} /></span>
        {#if connected}
          <button type="button" class="btn" disabled={st.dirty || busy.models} aria-busy={busy.models} onclick={loadModels}><Icon name="sync" size={14} /> {t('prov72.modelLoad')}</button>
        {/if}
      </div>
      {#if modelNote}<p class="muted sm" id="pv-model-note" role="status" dir="auto">{modelNote}</p>{/if}
    {/if}

    <div class="row wrap gap-top">
      <button type="button" class="btn primary" disabled={!st.canSave || busy.save || readOnly} aria-busy={busy.save} onclick={save}>{t('prov72.save')}</button>
      <button type="button" class="btn" disabled={!st.canTest || busy.test} aria-busy={busy.test} aria-describedby={st.dirty ? 'pv-test-note' : undefined} onclick={tryIt}>{busy.test ? t('runa.prov.testing') : t('prov72.test')}</button>
      <HelpTip text={t('prov72.testHelp')} label={t('prov72.test')} />
      <span class="grow"></span>
      <button type="button" class="btn ghost" onclick={() => (open = null)}>{t('prov72.close')}</button>
    </div>
    {#if st.dirty && st.canSave}<p class="muted sm" id="pv-test-note">{t('prov72.testAfterSave')}</p>{/if}
    {#if test}<p class="sm" class:err={!test.ok} role="status" dir="auto">{test.ok ? t('runa.prov.testOk') : test.message}</p>{/if}
  </div>
{/if}

<style>
  /* The same look as the rest of Settings › Assistant (Settings.svelte); logical properties only, so
     the page mirrors in Arabic. */
  h3 { font-size: 14px; font-weight: 600; margin: 18px 0 6px; }
  .sm { font-size: var(--fs-xs); }
  p { margin: 4px 0 0; }
  .row { display: flex; gap: 8px; align-items: center; }
  .wrap { flex-wrap: wrap; }
  .gap-top { margin-top: 12px; }
  .grow { flex: 1; }
  .tiny { font-size: var(--fs-xs); padding: 2px 8px; }
  .err { color: var(--danger); font-size: var(--fs-sm); }
  /* One line: the sentence takes the room and wraps inside itself (a long address), the buttons stay put. */
  .now { display: flex; gap: 8px; align-items: center; padding: 6px 0; border-bottom: 1px solid var(--hairline); font-size: 13px; }
  .now .what { font-weight: 500; flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .now .pill, .now .btn { flex: none; }
  .fmt { margin-inline-start: 4px; }
  .pick { display: grid; gap: 6px; margin-top: 12px; }
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
  .svc {
    font: inherit; color: var(--fg); text-align: start; cursor: pointer; display: grid; gap: 2px; align-content: start;
    padding: 8px 10px; border-radius: var(--radius); background: var(--fill); border: var(--border-w) solid transparent;
  }
  .svc:hover { background: var(--fill-2); }
  .svc.on { background: var(--tint); color: var(--on-tint); border-color: var(--accent); }
  .svc:disabled { opacity: .5; cursor: default; }
  .svc b { font-size: 13px; font-weight: 600; }
  .svc .sub, .svc .works { font-size: var(--fs-xs); }
  .svc .works { color: var(--muted); }
  .svc.on .works { color: inherit; }
  :global([data-theme="contrast"]) .svc { border-color: var(--border); }
  .form { margin-top: 10px; padding: 10px 12px 12px; border-radius: var(--radius); background: var(--fill); }
  .about { margin: 0 0 6px; font-size: 13px; }
  .with { border: 0; padding: 0; margin: 6px 0 0; min-width: 0; display: flex; flex-wrap: wrap; gap: 4px 16px; align-items: center; }
  .with legend { font-size: var(--fs-xs); font-weight: 500; color: var(--muted); padding: 0; }
  .radio { display: flex; gap: 6px; align-items: center; margin: 4px 0; cursor: pointer; font-size: 13px; color: var(--fg); }
  .check { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; }
  .model { flex: 1; min-width: 180px; }
</style>
