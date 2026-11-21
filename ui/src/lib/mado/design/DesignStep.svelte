<script lang="ts">
  // The design step of a plan: pick a look, fine-tune every part of it live, and look at the
  // screens the assistant drew in it. Talks to /api/mado/:projectId/design/* itself (client.ts).
  // The props are a fixed interface: the Plan view mounts this component.
  import { untrack } from 'svelte';
  import { t } from '../../i18n';
  import Icon from '../../Icon.svelte';
  import type { Comment, CommentTarget, DesignTokens, ScreenInfo, StyleCandidate } from '../types';
  import { client } from './client';
  import { History } from './history';
  import MockFrame from './MockFrame.svelte';
  import Screens from './Screens.svelte';
  import StyleGallery from './StyleGallery.svelte';
  import StylePreview from './StylePreview.svelte';
  import Tuner from './Tuner.svelte';
  import { same, sane } from './tokens';

  let {
    projectId,
    comments = [],
    locked = false,
    version = 0,
    oncomment,
  }: {
    /** The project whose design this is; the component talks to /api/mado/:projectId/design/* itself. */
    projectId: string;
    /** Comments whose target is a token or a screen: shown as marks and pins. */
    comments?: Comment[];
    /** true: look only (the step is approved or a run is writing). */
    locked?: boolean;
    /** Changes whenever the daemon said the design changed: fetch again. */
    version?: number;
    /** The user asked to comment on a token or dropped a pin on a screen. */
    oncomment?: (target: Extract<CommentTarget, { kind: 'token' | 'screen' }>) => void;
  } = $props();

  type Tab = 'style' | 'tune' | 'screens';
  const TABS: Tab[] = ['style', 'tune', 'screens'];
  let tab = $state<Tab>('style');
  let loaded = $state(false);
  let loadError = $state('');

  let styles = $state<StyleCandidate[]>([]);
  let picked = $state<string | null>(null);
  let dropped = $state(0);
  let screens = $state<ScreenInfo[]>([]);
  let rev = $state(0);

  // The settings on screen, and what the daemon has. They differ only while a change waits to be saved.
  let tokens = $state<DesignTokens | null>(null);
  let savedTokens: DesignTokens | null = null;
  let history: History<DesignTokens> | null = null;
  let canUndo = $state(false);
  let canRedo = $state(false);
  let status = $state<'saved' | 'unsaved' | 'saving' | 'failed'>('saved');
  let saveError = $state('');
  let picking = $state<string | null>(null);
  let pickError = $state('');
  let previewOn = $state('');

  const pickedStyle = $derived(styles.find((s) => s.id === picked) ?? null);
  const changed = $derived(!!tokens && !!pickedStyle && !same(tokens, pickedStyle.tokens));
  const drawn = $derived(screens.filter((s) => s.hasMock));

  const marks = () => { canUndo = !!history?.canUndo; canRedo = !!history?.canRedo; };
  function adopt(next: DesignTokens | null) {
    savedTokens = next ? sane(next) : null;
    tokens = savedTokens;
    history = savedTokens ? new History(savedTokens) : null;
    status = 'saved'; saveError = '';
    marks();
  }

  // ---------- loading ----------
  let loads = 0;
  async function load(first: boolean) {
    const mine = ++loads;
    const pid = projectId;
    try {
      const [st, tk, sc] = await Promise.all([client().styles(pid), client().tokens(pid).catch((e) => (first ? null : Promise.reject(e))), client().screens(pid)]);
      if (mine !== loads || pid !== projectId) return;
      styles = Array.isArray(st?.styles) ? st.styles : [];
      picked = typeof st?.picked === 'string' ? st.picked : null;
      dropped = Number(st?.dropped) || 0;
      screens = Array.isArray(sc) ? sc : [];
      rev++;
      // Settings that changed elsewhere (a run, another window) are taken, unless a change of
      // the user's own is on its way: theirs is the newer one and is about to be saved.
      const idle = status === 'saved' && saveTimer === null;
      if (first || (idle && !same(tk, savedTokens))) adopt(tk);
      if (first) tab = !tk ? 'style' : screens.some((s) => s.hasMock) ? 'screens' : 'tune';
      loadError = ''; loaded = true;
    } catch (e: any) {
      if (mine !== loads) return;
      if (!loaded) loadError = String(e?.message ?? e);
    }
  }
  // A new project starts from nothing; a new `version` fetches again in place.
  let lastProject = '';
  $effect(() => {
    void version;
    const pid = projectId;
    untrack(() => {
      const first = pid !== lastProject;
      if (first) { lastProject = pid; loaded = false; flush(); adopt(null); styles = []; screens = []; picked = null; }
      void load(first);
    });
  });

  // ---------- saving: a moment after the last change, one request at a time ----------
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let saving = false;
  function schedule() {
    status = 'unsaved';
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => { saveTimer = null; void save(); }, 450);
  }
  /** Leaving: what waits is sent at once. */
  function flush() { if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; void save(); } }
  async function save() {
    if (saving || !tokens || locked) return;
    const pid = projectId, sending = tokens;
    saving = true; status = 'saving';
    try {
      const back = await client().saveTokens(pid, sending);
      if (pid !== projectId) return;
      savedTokens = sane(back ?? sending);
      if (same(tokens, sending)) { status = 'saved'; saveError = ''; }
    } catch (e: any) {
      if (pid === projectId) { status = 'failed'; saveError = String(e?.message ?? e); }
    } finally {
      saving = false;
      // A change made while this was on its way goes next.
      if (pid === projectId && status !== 'failed' && tokens && !same(tokens, savedTokens) && saveTimer === null) schedule();
    }
  }
  $effect(() => () => flush());

  function change(next: DesignTokens, key: string | null) {
    if (locked || !history) return;
    const clean = sane(next);
    if (same(clean, tokens)) return;
    tokens = history.push(clean, key);
    marks(); schedule();
  }
  const undo = () => { if (history?.canUndo && !locked) { tokens = history.undo(); marks(); schedule(); } };
  const redo = () => { if (history?.canRedo && !locked) { tokens = history.redo(); marks(); schedule(); } };
  const reset = () => { if (pickedStyle) change(pickedStyle.tokens, null); };

  async function pick(id: string) {
    if (locked || picking) return;
    picking = id; pickError = '';
    if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }
    try {
      const r = await client().pick(projectId, id);
      picked = r?.picked ?? id;
      adopt(r?.tokens ?? styles.find((s) => s.id === id)?.tokens ?? null);
      tab = 'tune';
    } catch (e: any) {
      pickError = `${t('madoDesign.gallery.pickFailed')} ${String(e?.message ?? e)}`;
    } finally { picking = null; }
  }

  const tokenComments = $derived(comments.filter((c) => c.target.kind === 'token'));
  const screenComments = $derived(comments.filter((c) => c.target.kind === 'screen'));
  const previewScreen = $derived(drawn.find((s) => s.id === previewOn) ?? null);
  let previewHtml = $state<string | null>(null);
  $effect(() => {
    const s = previewScreen, pid = projectId;
    void rev;
    previewHtml = null;
    if (!s) return;
    client().screen(pid, s.id).then((r) => { if (previewScreen?.id === s.id) previewHtml = String(r?.html ?? ''); }).catch(() => { if (previewScreen?.id === s.id) previewOn = ''; });
  });

  function tabKeys(e: KeyboardEvent) {
    const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    const rtl = getComputedStyle(e.currentTarget as HTMLElement).direction === 'rtl';
    tab = TABS[(TABS.indexOf(tab) + (rtl ? -d : d) + TABS.length) % TABS.length];
    queueMicrotask(() => (document.getElementById(`mdd-tab-${tab}`) as HTMLElement | null)?.focus());
  }
</script>

<div class="mdd" data-mado-design>
  {#if !loaded}
    {#if loadError}
      <div class="state" role="alert">
        <Icon name="warning" size={22} />
        <p>{t('madoDesign.loadFailed')}</p>
        <p class="muted sm">{loadError}</p>
        <button type="button" class="btn" onclick={() => { loadError = ''; void load(true); }}>{t('madoDesign.retry')}</button>
      </div>
    {:else}
      <div class="state" role="status" aria-label={t('madoDesign.loading')}>
        <div class="skelrow"><span class="skel"></span><span class="skel"></span><span class="skel"></span></div>
      </div>
    {/if}
  {:else}
    <div class="top">
      <div class="seg" role="tablist" aria-label={t('madoDesign.tabs')}>
        {#each TABS as k (k)}
          <button type="button" class="segb" role="tab" id="mdd-tab-{k}" aria-selected={tab === k} aria-controls="mdd-panel" tabindex={tab === k ? 0 : -1} onclick={() => (tab = k)} onkeydown={tabKeys}>
            <Icon name={k === 'style' ? 'palette' : k === 'tune' ? 'tool' : 'window'} size={14} />
            {t(`madoDesign.tab.${k}`)}
            {#if k === 'screens' && screens.length}<span class="count">{drawn.length}/{screens.length}</span>{/if}
          </button>
        {/each}
      </div>
      {#if locked}<p class="lock" role="status"><Icon name="lock" size={14} /> {t('madoDesign.locked')}</p>{/if}
    </div>

    <div id="mdd-panel" role="tabpanel" aria-labelledby="mdd-tab-{tab}" class="panel">
      {#if tab === 'style'}
        <StyleGallery {styles} {picked} {dropped} {changed} {locked} busy={picking} error={pickError} onpick={pick} ontune={() => (tab = 'tune')} />
      {:else if tab === 'tune'}
        {#if !tokens}
          <div class="state">
            <Icon name="palette" size={24} />
            <p>{t('madoDesign.tune.none')}</p>
            <button type="button" class="btn primary" onclick={() => (tab = 'style')}>{t('madoDesign.tune.goPick')}</button>
          </div>
        {:else}
          <div class="split">
            <div class="controls">
              <Tuner {tokens} comments={tokenComments} {locked} {canUndo} {canRedo} canReset={changed} {status} {saveError}
                onchange={change} onundo={undo} onredo={redo} onreset={reset} onretry={() => void save()}
                oncomment={oncomment ? (tokenPath) => oncomment?.({ kind: 'token', tokenPath }) : undefined} />
            </div>
            <aside class="preview" aria-label={t('madoDesign.tune.preview')}>
              <div class="phead">
                <span class="eyebrow grow">{t('madoDesign.tune.preview')}</span>
                {#if drawn.length}
                  <select class="pon" bind:value={previewOn} aria-label={t('madoDesign.tune.previewOn')}>
                    <option value="">{t('madoDesign.tune.previewSample')}</option>
                    {#each drawn as s (s.id)}<option value={s.id}>{s.label}</option>{/each}
                  </select>
                {/if}
              </div>
              <div class="pbody">
                {#if previewScreen && previewHtml !== null}
                  <MockFrame html={previewHtml} {tokens} name={previewScreen.label} device="desktop" />
                {:else}
                  <StylePreview {tokens} size="full" />
                {/if}
              </div>
            </aside>
          </div>
        {/if}
      {:else}
        <Screens {projectId} {screens} tokens={tokens ?? pickedStyle?.tokens ?? null} comments={screenComments} {locked} {rev}
          onpin={oncomment ? (screenId, x, y) => oncomment?.({ kind: 'screen', screenId, x, y }) : undefined} />
      {/if}
    </div>
  {/if}
</div>

<style>
  .mdd { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  .top { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  .count { font-size: var(--fs-xxs); font-variant-numeric: tabular-nums; opacity: .8; }
  .lock { margin: 0; font-size: var(--fs-sm); color: var(--muted); display: flex; align-items: center; gap: 6px; }
  .panel { min-width: 0; animation: mdd-in .22s ease backwards; }
  @keyframes mdd-in { from { opacity: 0; transform: translateY(6px); } }
  .split { display: grid; grid-template-columns: minmax(330px, 440px) minmax(0, 1fr); gap: 20px; align-items: start; }
  @container (max-width: 820px) { .split { grid-template-columns: minmax(0, 1fr); } }
  @media (max-width: 900px) { .split { grid-template-columns: minmax(0, 1fr); } }
  .controls { min-width: 0; }
  .preview { position: sticky; top: 8px; min-width: 0; border: var(--border-w) solid var(--border); border-radius: var(--radius-lg); background: var(--panel); box-shadow: var(--shadow); overflow: hidden; }
  .phead { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: var(--border-w) solid var(--border); }
  .pon { width: auto; max-width: 60%; padding: 4px 8px; font-size: var(--fs-sm); }
  .pbody { max-height: 74vh; overflow: auto; }
  .grow { flex: 1; min-width: 0; }
  .sm { font-size: var(--fs-sm); }
  .state { min-height: 240px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; text-align: center; border: 1px dashed var(--control-border); border-radius: var(--radius-lg); padding: 24px; }
  .state p { margin: 0; }
  .skelrow { display: grid; grid-template-columns: repeat(3, minmax(120px, 220px)); gap: 16px; }
  .skelrow .skel { height: 180px; border-radius: var(--radius-lg); }
</style>
