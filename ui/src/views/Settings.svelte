<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import { askNotify, notifState, notifySettingsUrl, openNotifySettings, testNotify, type NotifState } from '../lib/notifyPerm';
  import { api, del, inTauri, post, put, reloadApp } from '../lib/api';
  import { reportUnsaved } from '../lib/unsaved';
  import { a11y, app, applyTheme, command, go, saveSettings, setA11y, setShowCost, toast, zoomStep, A11Y_DEFAULTS, ZOOM_STEPS, type A11y } from '../lib/store.svelte';
  import { recheckSignIn } from '../lib/auth';
  import { isolatedKeys, tip } from '../lib/shortcuts';
  import { choiceDialog, confirmDialog, promptDialog } from '../lib/dialog.svelte';
  import { ago, fmtMoney, fmtNum, lang, t, tn } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { backup, driveName, lastBackupText, refreshBackup } from '../lib/backup.svelte';
  import { cloudFor } from '../lib/cloud.svelte';
  import ProviderPresets from './ProviderPresets.svelte';
  import CodexSettings from './CodexSettings.svelte';
  import SpendingGuard from './SpendingGuard.svelte';
  import { restartService } from '../lib/shell';
  import { keepLibraryToMe, libraryOpenToOthers, offerIndexCleanup } from '../lib/privacy';
  import ThemePicker from './ThemePicker.svelte';
  import CalendarPrefs from '../lib/CalendarPrefs.svelte';
  import QuickCapturePrefs from '../lib/QuickCapturePrefs.svelte';
  import { fold } from '../lib/CommandPalette.svelte';
  import Maintenance from './Maintenance.svelte';
  import AssistantGuide from './AssistantGuide.svelte';
  import AccessKeys from './AccessKeys.svelte';
  import UsageSettings from './usage/UsageSettings.svelte';
  import { checkAppUpdate } from '../lib/updates';

  let { reload }: { reload: () => Promise<void> } = $props();

  const cats = ['general', 'accessibility', 'ai', 'usage', 'git', 'privacy', 'modules', 'daemon', 'appLayer', 'maintenance'] as const;
  type Cat = (typeof cats)[number];
  const catIcon: Record<Cat, string> = { general: 'settings', accessibility: 'accessibility', ai: 'ai', usage: 'usage', git: 'sync', privacy: 'shield', modules: 'modules', daemon: 'daemon', appLayer: 'palette', maintenance: 'tool' };
  // Row labels per category (i18n keys) — used by the search box. `adv` rows live behind the Advanced disclosure.
  const basic: Record<Cat, string[]> = {
    general: ['language', 'set.name', 'ui.set.profession', 'theme', 'mode', 'set.notify', 'todob.set.title', 'todob.set.weekStart', 'todob.set.dates', 'nativeb.set.title', 'nativeb.set.switch'],
    accessibility: ['a11y.zoom', 'a11y.motion', 'a11y.links', 'a11y.focus', 'a11y.spacing'],
    ai: ['codex.set.which', 'set.account', 'codex.set.title', 'codex.set.limitsTitle', 'set.perm', 'set.wrapNag', 'chatc.set.suggest', 'r10-chatview.set.title'],
    usage: ['r10-usage.set.glance', 'r10-usage.set.scopeTitle'],
    git: ['set.libraryPath', 'hista.historyTitle', 'hista.keep', 'set.backup', 'hista.backup.online', 'histb.backup.drive', 'hista.online.when', 'set.credStore', 'histb.loc.title', 'hista.online.locations', 'histb.signin.update', 'reorganize'],
    privacy: ['set.privTitle', 'appr.strict.title', 'seca.access.title', 'wrapa.set.auto', 'wording.reports.title', 'layer.signalsTitle', 'set.cost', 'runc.spend.title', 'ui.set.showCost', 'secb.keys.title'],
    modules: ['set.modulesIntro'],
    daemon: ['set.keepRunning', 'set.runAtLogin'],
    appLayer: ['layer.customise', 'layer.history', 'layer.share', 'layer.workflows', 'auto.routines', 'ldc.tab.addons', 'ldc.tab.ready', 'ldc.tab.updates'],
    maintenance: ['care.health.title', 'care.upd.title', 'care.move.title', 'care.rm.title'],
  };
  const adv: Record<Cat, string[]> = {
    general: [],
    accessibility: [],
    usage: [],
    ai: ['set.model', 'set.effort', 'set.binary', 'set.miniModel', 'set.idle', 'set.provider', 'set.baseUrl', 'set.apiKey', 'codex.set.model', 'codex.set.effort', 'codex.set.mini', 'codex.set.binary'],
    git: [],
    privacy: [],
    modules: [],
    daemon: ['set.restart'],
    appLayer: ['set.safeMode'],
    maintenance: [],
  };
  // Claude Code's own mode names. "Ask before changes" (`default`) was removed — the app has no way
  // to answer the questions — so a stored `default` means plan mode. Bypass is confirmed in a dialog.
  const BYPASS = 'bypassPermissions';
  const perms = ['acceptEdits', 'plan', BYPASS] as const;
  /** The mode a stored value stands for. Nothing stored is accept-edits, exactly as the service reads it. */
  const realPerm = (m: unknown) => (m == null || m === '' ? 'acceptEdits' : m === 'default' ? 'plan' : String(m));
  const professions = ['finance', 'legal', 'consulting', 'teaching', 'research', 'founder', 'personal', 'other'];

  const fresh = () => {
    const x = structuredClone($state.snapshot(app.settings));
    x.privatePaths ??= []; x.strictPrivate ??= false; x.usageSignals ??= true; x.profession ??= null; x.autoSummaries ??= true; x.keepHistory ??= true; x.releaseCheck ??= false; x.assistantReports ??= false; x.spending ??= { monthlyLimitUsd: null, warnPercent: 80 };
    if (x.ai?.claude) x.ai.claude.permissionMode = realPerm(x.ai.claude.permissionMode);
    return x;
  };
  const norm = (x: any) => JSON.stringify(x, (_k, v) => (v === '' || v === undefined ? null : v));

  let tab = $state<Cat>((cats as readonly string[]).includes(app.view.id ?? '') ? (app.view.id as Cat) : 'general');
  let query = $state('');
  let s = $state<any>(fresh());
  let base = $state<any>(fresh()); // last saved state; `s` is the draft
  let opts = $state<any>({ models: [], efforts: [], permissionModes: [] });
  let claude = $state<any>(app.status?.claude ?? {});
  let gitStatus = $state<any>({});
  let modules = $state<any[]>([]);
  let loadErr = $state<Record<string, string>>({});
  let loading = $state<Record<string, boolean>>({ modules: true });
  let busy = $state<Record<string, boolean>>({});
  let apiToken = $state('');
  let remote = $state({ name: '', url: '', username: '', token: '' });
  let privErr = $state('');
  /** Can other accounts on this computer open the library folder? null: not known, nothing is shown. */
  let openToOthers = $state<boolean | null>(null);
  let advOpen = $state<Record<string, boolean>>({});

  const isDirty = () => norm(s) !== norm(base);
  const dirty = $derived(isDirty());
  const isGit = $derived(String(s.sync.mode).startsWith('git'));
  // "Online location" and "Drive or folder" are both sent to by the app (the git modes); `sync.target` says which was chosen.
  const syncKind = $derived(isGit ? (s.sync.target === 'drive' ? 'drive' : 'git') : s.sync.mode === 'folder' ? 'folder' : 'none');
  // An online location is sent the history itself, so there the history is always kept.
  const historyOn = $derived(isGit || s.keepHistory !== false);
  /** What the saved settings say (the status lines describe what is really happening, not the draft). */
  const historySaved = $derived(String(base.sync?.mode ?? '').startsWith('git') || base.keepHistory !== false);
  const gitMissing = $derived(app.status?.git?.installed === false);
  // Whether a cloud drive syncs the library folder is the service's call (real sync folders, shortcuts resolved).
  const cloud = cloudFor(() => String(s.libraryPath ?? ''));
  const inCloud = $derived(cloud.current.synced);
  const cloudName = $derived(cloud.current.service || t('hista.cloud.someDrive'));
  /** The library was already sent to an online location from this very folder: warned about, never locked out. */
  const cloudAlready = $derived(String(base.sync?.mode ?? '').startsWith('git') && String(base.libraryPath ?? '').trim() === String(s.libraryPath ?? '').trim());
  const pm = $derived(s.ai.claude.permissionMode);
  const permValue = $derived((perms as readonly string[]).includes(realPerm(pm)) ? realPerm(pm) : '');
  /** A settings file written by a newer version is never rewritten: everything here is read-only. */
  const readOnly = $derived(!!app.status?.settingsNewer);
  // The look is saved the moment it is chosen (ThemePicker), so it is never part of the unsaved draft.
  // The same goes for the accessibility preferences (zoom changes from the keyboard too).
  $effect(() => {
    const th = app.settings?.theme, m = app.settings?.mode, ax = JSON.stringify(app.settings?.accessibility ?? null);
    untrack(() => { s.theme = th; s.mode = m; base.theme = th; base.mode = m; s.accessibility = JSON.parse(ax); base.accessibility = JSON.parse(ax); });
  });
  const ax = $derived(a11y());
  const zoomChoices = $derived([...new Set([...ZOOM_STEPS, ax.zoom])].sort((a, b) => a - b));
  /** Save an accessibility choice. When the save fails the control goes back to what is stored:
   *  the box or list was changed by the click itself, and Svelte does not reset an unchanged value. */
  const setAx = (patch: Partial<A11y>, el?: HTMLInputElement | HTMLSelectElement) => {
    setA11y(patch).catch((e) => {
      if (el instanceof HTMLInputElement) { const k = Object.keys(patch)[0] as keyof A11y; el.checked = !!a11y()[k]; }
      else if (el) el.value = String(a11y().zoom);
      failed(t('a11y.saveFailed'), e);
    });
  };
  const stepZoom = (d: -1 | 1) => { zoomStep(d).catch((e) => failed(t('a11y.saveFailed'), e)); };
  const cost = $derived(app.status?.usage?.monthCostUsd);
  const validRemote = $derived(/^(https?:\/\/|ssh:\/\/|git@)\S+$/.test(remote.url.trim()));

  const errors = $derived.by(() => {
    const e: Record<string, string> = {};
    const whole = (v: any) => Number.isInteger(v) && v >= 1;
    if (!String(s.libraryPath ?? '').trim()) e.libraryPath = t('set.errLibrary');
    if (!whole(s.idleMinutes)) e.idleMinutes = t('set.errWhole');
    if (!whole(s.wrapNagThreshold)) e.wrapNagThreshold = t('set.errWhole');
    if (permValue === '') e.permission = t('set.errPerm');
    const sp = s.spending;
    if (sp && sp.monthlyLimitUsd != null && !(Number(sp.monthlyLimitUsd) >= 1 && Number(sp.monthlyLimitUsd) <= 100000 && Number.isInteger(Number(sp.warnPercent)) && sp.warnPercent >= 50 && sp.warnPercent <= 95)) e.spending = t('runc.spend.err');
    if (syncKind === 'git' && inCloud && !s.sync.allowCloud && !cloudAlready) e.sync = t('hista.online.cloudClash');
    return e;
  });
  const invalid = $derived(Object.keys(errors).length > 0);

  // ----- search -----
  const q = $derived(fold(query.trim()));
  const hit = (k: string) => fold(t(k)).includes(q);
  const catAll = (c: Cat) => !q || fold(t('set.cat.' + c)).includes(q);
  const vis = (c: Cat, ...keys: string[]) => catAll(c) || keys.some(hit);
  const catShown = (c: Cat) => vis(c, ...basic[c], ...adv[c]);
  const shown = $derived(q ? cats.filter(catShown) : [tab]);
  const tx = (k: string, fallback: string) => { const v = t(k); return v === k ? fallback : v; };

  async function load(key: string, path: string, set: (v: any) => void) {
    loading[key] = true; loadErr[key] = '';
    try { set(await api(path)); }
    catch (e: any) { loadErr[key] = e?.message || t('set.failed'); }
    finally { loading[key] = false; }
  }
  const loadModules = () => load('modules', '/modules', (m) => (modules = m));
  const loadGit = () => load('git', '/git', (g) => {
    gitStatus = g;
    backup.health = g?.health ?? null;
    if (String(app.settings?.sync?.mode ?? '').startsWith('git')) loadSignIns();
  });

  // ----- backup locations: health, sign-ins, drives, SSH servers -----
  /** Per site the online locations sign in to: `{host, locations, saved, username, store, insecure}` (never a password). */
  let signIns = $state<any[]>([]);
  /** The "Update sign-in" form of one location, with the outcome of its check. */
  let signForm = $state<{ location: string; username: string; token: string; result: string; ok: boolean } | null>(null);
  const loadSignIns = () => api('/git/credentials').then((v) => (signIns = Array.isArray(v) ? v : [])).catch(() => (signIns = []));
  /** Fresh after every Sync (the Sync button refreshes it), so the rows here never lag behind. */
  const health = $derived(backup.health ?? gitStatus.health ?? null);
  const locHealth = (name: string) => (health?.locations ?? []).find((l: any) => l.name === name) ?? null;
  const signInOf = (name: string) => signIns.find((x) => (x.locations ?? []).includes(name)) ?? null;
  const listNames = (names: string[]) => names.map((n) => (lang() === 'ar' ? `«${n}»` : `“${n}”`)).join(lang() === 'ar' ? '، ' : ', ');
  function healthText(h: any): string {
    if (!h || !(gitStatus.remotes || []).length) return '';
    if (h.state === 'ok') return t('histb.health.ok');
    if (h.state === 'failed') return t('histb.health.failed');
    if (h.state === 'stale') return (h.locations ?? []).some((l: any) => !l.at) ? t('histb.health.never') : t('histb.health.stale', { days: h.staleDays });
    if (h.state === 'waiting') return tn('histb.health.waiting', Math.max(0, ...(h.locations ?? []).map((l: any) => Number(l.unsent) || 0)));
    return '';
  }
  function openSignIn(name: string) {
    const si = signInOf(name);
    signForm = { location: name, username: si?.username ?? '', token: '', result: '', ok: false };
  }
  const saveSignIn = () => run('signin', async () => {
    const f = signForm;
    if (!f || !f.token) return;
    const v = await post('/git/credentials', { location: f.location, username: f.username.trim(), token: f.token });
    f.token = '';
    f.ok = !!v?.works;
    f.result = v?.works ? t('histb.signin.works') : t('histb.signin.notWorks', { why: String(v?.why ?? '') });
    await loadSignIns();
    await refreshBackup();
  });
  async function forgetSignIn(si: any) {
    const ok = await confirmDialog({ title: t('histb.signin.forgetTitle', { host: si.host }), body: t('histb.signin.forgetBody'),
      note: t('histb.signin.forgetAlso'), list: si.locations ?? [], confirmLabel: t('histb.signin.forget'), danger: true });
    if (!ok) return;
    await run('forget', async () => {
      await del(`/git/credentials?host=${encodeURIComponent(si.host)}`);
      if (signForm && (si.locations ?? []).includes(signForm.location)) signForm = null;
      toast(t('histb.signin.forgotten'), { kind: 'success' });
      await loadSignIns();
    });
  }
  async function addDrive() {
    let p: string | null = null;
    if (inTauri()) {
      try { const { open } = await import('@tauri-apps/plugin-dialog'); const r = await open({ directory: true }); p = typeof r === 'string' ? r : null; }
      catch { toast(t('set.browseFail'), { kind: 'error' }); return; }
    } else {
      p = await promptDialog({ title: t('histb.drive.typeTitle'), body: t('histb.drive.typeBody'), value: '' });
    }
    const folder = p?.trim();
    if (!folder) return;
    await run('drive', async () => {
      const v = await post('/git/remotes', { name: driveName(folder, new Set((gitStatus.remotes || []).map((r: any) => String(r.name)))), folder });
      toast(t('histb.drive.added'), { kind: 'success', detail: v?.path ? t('histb.drive.where', { path: v.path }) : undefined });
      await loadGit();
      await refreshBackup();
    });
  }
  /** An SSH server this computer has never connected to: show its keys, trust the one the user compared. */
  const checkServer = (r: any) => run('host', async () => {
    const k = await api(`/git/hostkey?location=${encodeURIComponent(r.name)}`);
    const keys: { type: string; fingerprint: string }[] = k?.keys ?? [];
    const pick = await choiceDialog({
      title: t('histb.host.title'), body: t('histb.host.body', { host: String(k?.host ?? '') }),
      list: keys.map((x) => `${x.type}: ${x.fingerprint}`),
      choices: keys.map((x, i) => ({ id: x.fingerprint, label: keys.length > 1 ? `${t('histb.host.trust')} (${x.type})` : t('histb.host.trust'), primary: i === 0 })),
    });
    if (!pick) return;
    await post(`/git/hostkey?location=${encodeURIComponent(r.name)}`, { fingerprint: pick });
    toast(t('histb.host.trusted'), { kind: 'success' });
    await refreshBackup();
  });
  // Arriving from a failed Sync ("Update sign-in", "Check the server"): go to the location concerned.
  let focused = '';
  $effect(() => {
    const v = app.view, h = health;
    if (v.kind !== 'settings' || !v.path || !h || focused === v.path) return;
    untrack(async () => {
      focused = v.path!;
      tab = 'git';
      const want = v.path === 'signin' ? 'sync.why.sign_in' : v.path === 'hostkey' ? 'sync.why.host_key' : '';
      const loc = want ? (h.locations ?? []).find((l: any) => l.whyKey === want) : null;
      if (loc && v.path === 'signin') openSignIn(loc.name);
      await tick();
      document.getElementById(loc ? `st-loc-${loc.name}` : 'st-locations')?.scrollIntoView({ block: 'center' });
    });
  });

  onMount(() => {
    api('/ai/claude/options').then((o) => (opts = o)).catch(() => {});
    loadGit(); loadModules();
    libraryOpenToOthers().then((v) => (openToOthers = v));
    app.leaveGuard = leaveGuard;
  });

  // Plain headline per action; whatever the background service said goes in the detail.
  const FAIL: Record<string, string> = { save: 'ui.set.saveFailed', mod: 'ui.set.modFailed', remote: 'ui.set.remoteFailed', token: 'ui.set.keyFailed', login: 'ui.set.loginFailed', reorg: 'ui.set.reorgFailed', signin: 'histb.signin.failed', forget: 'histb.signin.forgetFailed', drive: 'histb.drive.failed', host: 'histb.host.failed' };
  /** Runs one action with a loading flag; any failure is shown to the user. */
  async function run(key: string, fn: () => Promise<unknown>) {
    if (busy[key]) return;
    busy[key] = true;
    try { await fn(); } catch (e) { failed(t(FAIL[key.split(':')[0]] ?? 'set.failed'), e); }
    finally { busy[key] = false; }
  }

  function trimmed(v: any) {
    const x = structuredClone($state.snapshot(v));
    const c = x.ai.claude;
    for (const k of ['binary', 'model', 'effort', 'permissionMode', 'miniWrapModel', 'baseUrl']) c[k] = typeof c[k] === 'string' && c[k].trim() ? c[k].trim() : null;
    x.libraryPath = String(x.libraryPath ?? '').trim();
    return x;
  }
  /** Leaf values of `draft` that differ from `from` (a list counts as one value). */
  function changes(draft: any, from: any, path: string[] = [], out: [string[], any][] = []): [string[], any][] {
    const obj = (v: any) => !!v && typeof v === 'object' && !Array.isArray(v);
    for (const k of new Set([...Object.keys(draft ?? {}), ...Object.keys(from ?? {})])) {
      const a = draft?.[k], b = from?.[k];
      if (obj(a) && obj(b)) changes(a, b, [...path, k], out);
      else if (norm(a) !== norm(b)) out.push([[...path, k], a]);
    }
    return out;
  }
  /** Only what the user changed, laid over the settings as they are now: a value nobody touched
   *  here (the theme, zoom, a mode only shown under another name) is sent back exactly as it is. */
  function payload() {
    const x = structuredClone($state.snapshot(app.settings));
    for (const [path, v] of changes(trimmed(s), trimmed(base))) {
      let o = x;
      for (const k of path.slice(0, -1)) o = o[k] ??= {};
      o[path[path.length - 1]] = v === undefined ? null : structuredClone(v);
    }
    return x;
  }

  // ----- bypass permissions: never switched on without the warning -----
  /** True once the user agreed to the warning for the draft that is being edited. */
  let bypassOk = false;
  const turnsBypassOn = () => s.ai.claude.permissionMode === BYPASS && app.settings?.ai?.claude?.permissionMode !== BYPASS;
  const askBypass = () => confirmDialog({
    title: t('perm.bypass.title'), body: `${t('perm.bypass.body')}\n\n${t('perm.bypass.bodyDefault')}`,
    note: t('perm.bypass.note'), confirmLabel: t('perm.bypass.confirm'), danger: true,
  });
  /** A click on a permission radio. Choosing bypass waits for the dialog; the radio only moves on a yes. */
  async function pickPerm(p: string, e: MouseEvent) {
    if (p !== BYPASS || permValue === BYPASS) { s.ai.claude.permissionMode = p; return; }
    e.preventDefault();
    if (!(await askBypass())) return;
    bypassOk = true;
    s.ai.claude.permissionMode = BYPASS;
    autoSave();
  }
  // ----- opening another library: chats stay with the one they were started in -----
  /** Asked before the library folder changes. False = stay; otherwise whether the old library moved here. */
  async function askLibrarySwitch(): Promise<false | { moved: boolean }> {
    const was = String(app.settings?.libraryPath ?? '').trim();
    const now = String(s.libraryPath ?? '').trim();
    if (!was || !now || was === now) return { moved: false };
    // The old folder is gone: only the person knows whether they moved it or this is another library.
    if (app.status?.libraryMissing && app.chats.length) {
      const c = await choiceDialog({
        title: t('wrapa.lib.movedTitle'), body: t('wrapa.lib.movedBody'),
        choices: [{ id: 'different', label: t('wrapa.lib.different') }, { id: 'same', label: t('wrapa.lib.same'), primary: true }],
      });
      return c ? { moved: c === 'same' } : false;
    }
    if (app.wrap.count > 0 && !(await confirmDialog({ title: t('wrapa.lib.title'), body: tn('wrapa.lib.body', app.wrap.count), confirmLabel: t('wrapa.lib.confirm') }))) return false;
    return { moved: false };
  }

  /** Write the draft. The service refuses to switch bypass on without the confirmation sent here. */
  async function persist() {
    const lib = await askLibrarySwitch();
    if (!lib) { s.libraryPath = base.libraryPath; return; }
    const confirm = turnsBypassOn();
    const sent = norm(s);
    const query = new URLSearchParams();
    if (confirm) query.set('confirm', BYPASS);
    if (lib.moved) query.set('library', 'moved');
    const qs = query.toString();
    await saveSettings(payload(), qs ? `?${qs}` : '');
    bypassOk = false;
    // A change typed while this was being saved stays in the draft and goes with the next save.
    if (norm(s) === sent) s = fresh();
    base = fresh();
    flashSaved();
  }

  // ----- every change applies at once -----
  // A change is saved when the control commits it: a switch or a choice at once, a text box when you
  // leave it or press Enter. Only invalid input, or a save the service refused, waits in the bar below.
  let queue: Promise<void> = Promise.resolve();
  let saveFailed = $state(false);
  let justSaved = $state(false);
  let savedTimer: ReturnType<typeof setTimeout> | undefined;
  function flashSaved() { justSaved = true; clearTimeout(savedTimer); savedTimer = setTimeout(() => (justSaved = false), 2500); }
  async function saveNow() {
    if (readOnly || !isDirty() || invalid) return;
    if (turnsBypassOn() && !bypassOk) return; // only ever after the warning (see pickPerm)
    busy.save = true;
    try { await persist(); saveFailed = false; }
    catch (e) { saveFailed = true; failed(t('ui.set.saveFailed'), e); }
    finally { busy.save = false; }
  }
  /** Save the draft (after any save still running). Bubbled `change` events land here. */
  function autoSave(): Promise<void> { queue = queue.then(saveNow, saveNow); return queue; }

  function discard() { s = structuredClone($state.snapshot(base)); bypassOk = false; saveFailed = false; applyTheme(); }

  /** Text typed into a field that only acts on its own button (a key, a backup address): not a setting, so not saved by itself. */
  const unsent = () => [
    ...(apiToken.trim() ? [t('set.apiKey')] : []),
    ...(provUrl.trim() !== savedUrl ? [t('set.baseUrl')] : []),
    ...(remote.url.trim() || remote.username.trim() || remote.token ? [t('shellui.set.typedRemote')] : []),
    ...(signForm?.token ? [t('histb.signin.update')] : []),
  ];

  // Leaving with unsaved edits: navigation waits for this (`app.leaveGuard`, awaited inside `go()`).
  async function leaveGuard(): Promise<boolean> {
    await queue; // a change being saved right now finishes first
    if (!(await leaveTyped())) return false;
    if (!isDirty() || readOnly) return true;
    const bad = invalid;
    const choice = await choiceDialog({
      title: t('set.leaveTitle'), body: t(bad ? 'layer.leaveInvalid' : 'set.leaveBody'), cancelLabel: t('layer.keepEditing'),
      choices: [{ id: 'discard', label: t('set.discard'), danger: true }, ...(bad ? [] : [{ id: 'save', label: t('save'), primary: true }])],
    });
    if (choice === 'discard') { discard(); return true; }
    if (choice !== 'save') return false;
    if (turnsBypassOn() && !bypassOk) { if (!(await askBypass())) return false; bypassOk = true; }
    try { await persist(); return true; }
    catch (e) { failed(t('ui.set.saveFailed'), e); return false; }
  }
  /** A typed key or backup address would be lost without a word: say so first. */
  async function leaveTyped(): Promise<boolean> {
    const typed = unsent();
    if (!typed.length || readOnly) return true;
    const choice = await choiceDialog({
      title: t('shellui.set.typedTitle'), body: t('shellui.set.typedBody'), list: typed, cancelLabel: t('layer.keepEditing'),
      choices: [{ id: 'leave', label: t('shellui.set.typedLeave'), danger: true }, ...(provDirty && !provErr ? [{ id: 'key', label: t('runa.prov.save'), primary: true }] : [])],
    });
    // The key and its address are one form (see saveProvider): saving stores both together.
    if (choice === 'key') { await saveProvider(); if (provDirty) return false; }
    else if (choice !== 'leave') return false;
    apiToken = ''; provUrl = savedUrl; remote = { name: '', url: '', username: '', token: '' }; signForm = null;
    return true;
  }
  onDestroy(() => { if (app.leaveGuard === leaveGuard) app.leaveGuard = null; clearTimeout(savedTimer); reportUnsaved('settings', null); });
  // The desktop app asks before quitting while a change is not saved or a typed key or address was not sent.
  $effect(() => { reportUnsaved('settings', !readOnly && (dirty || unsent().length > 0) ? t('settings') : null); });

  function onBeforeUnload(e: BeforeUnloadEvent) { if (isDirty() || unsent().length) { e.preventDefault(); e.returnValue = ''; } }

  // ----- notifications: the switch is only half of it; the computer has to allow them too -----
  let notif = $state<NotifState>('unknown');
  const refreshNotif = () => notifState().then((v) => (notif = v));
  onMount(() => { refreshNotif(); window.addEventListener('focus', refreshNotif); return () => window.removeEventListener('focus', refreshNotif); });
  async function onNotifySwitch(on: boolean) { if (on) notif = await askNotify(); }
  async function sendTest() {
    busy.notif = true;
    try {
      notif = await testNotify(t('shellui.notif.testTitle'), t('shellui.notif.testBody'));
      if (notif === 'unknown') toast(t('shellui.notif.cannot'), { kind: 'error' });
    } finally { busy.notif = false; }
  }

  const resets: Partial<Record<Cat, { list: string[]; apply: () => void }>> = {
    general: { list: ['theme', 'mode', 'set.notify'], apply: () => {
      s.notifications = true;
      // The look applies (and saves) at once, like choosing it in the picker.
      saveSettings({ theme: 'saudi', mode: 'system' }).catch((e) => failed(t('shell.themeNotSaved'), e));
    } },
    accessibility: { list: basic.accessibility, apply: () => setAx({ ...A11Y_DEFAULTS }) },
    ai: { list: ['set.perm', 'set.wrapNag', 'chatc.set.suggest', 'set.model', 'set.effort', 'set.binary', 'set.miniModel', 'set.idle'], apply: () => {
      const c = s.ai.claude; c.permissionMode = 'acceptEdits'; c.model = null; c.effort = null; c.binary = null; c.miniWrapModel = null; c.suggestNext = false;
      s.idleMinutes = 15; s.wrapNagThreshold = 5;
    } },
    daemon: { list: ['set.keepRunning', 'set.runAtLogin'], apply: () => { s.daemon.keepRunning = true; s.daemon.runAtLogin = false; } },
  };

  async function resetCat(c: Cat) {
    const r = resets[c];
    if (!r) return;
    const ok = await confirmDialog({ title: t('set.resetTitle', { name: t('set.cat.' + c) }), body: t('set.resetBody'), list: r.list.map((k) => t(k)), confirmLabel: t('set.reset'), danger: true });
    if (ok) { r.apply(); autoSave(); }
  }

  async function syncServerFields() {
    app.settings = await api('/settings');
    s.ai.claude.hasApiToken = base.ai.claude.hasApiToken = app.settings.ai.claude.hasApiToken;
    s.ai.claude.baseUrl = base.ai.claude.baseUrl = app.settings.ai.claude.baseUrl;
    s.modules = structuredClone($state.snapshot(app.settings.modules));
    base.modules = structuredClone($state.snapshot(app.settings.modules));
  }

  // "Check again" also forgets what a failed run said about the sign-in, and asks the CLI afresh.
  const refreshClaude = () => run('claude', async () => { await recheckSignIn(); claude = app.status?.claude ?? {}; });
  $effect(() => { const c = app.status?.claude; if (c) untrack(() => (claude = c)); });
  const signIn = () => run('login', async () => { await post('/ai/claude/login'); toast(t('set.loginToast')); });

  // ----- API key or another service -----
  // The address and its key are saved together and at once, apart from the rest of the page: the
  // service refuses an address without its key, and never sends a key to an address it was not saved for.
  let provUrl = $state<string>(app.settings?.ai?.claude?.baseUrl ?? '');
  let provTest = $state<{ ok: boolean; message?: string } | null>(null);
  const savedUrl = $derived(String(app.settings?.ai?.claude?.baseUrl ?? '').trim());
  const hasKey = $derived(!!app.settings?.ai?.claude?.hasApiToken);
  /** Where requests go when it is not Anthropic itself ('' for no address and for Anthropic's own). */
  const dest = (u: string) => (!u.trim() || /^https:\/\/api\.anthropic\.com(?:[:/?#]|$)/i.test(u.trim()) ? '' : u.trim());
  /** Who answers the assistant, as saved: the Claude account, an Anthropic key, another service, or an address with no key. */
  const provKind = $derived(dest(savedUrl) ? (hasKey ? 'service' : 'incomplete') : hasKey ? 'anthropicKey' : 'account');
  const provDirty = $derived(provUrl.trim() !== savedUrl || !!apiToken.trim());
  const provErr = $derived.by(() => {
    const u = provUrl.trim();
    if (u && !/^https?:\/\/\S+$/.test(u)) return t('set.errUrl');
    if (provDirty && dest(u) && !apiToken.trim() && !(hasKey && dest(savedUrl) === dest(u))) return t('runa.prov.needKey');
    return '';
  });

  const saveProvider = () => run('token', async () => {
    if (provErr || !provDirty) return;
    const r = await put('/ai/claude/provider', { baseUrl: provUrl.trim() || null, token: apiToken.trim() || null });
    apiToken = ''; provTest = null;
    await syncServerFields();
    provUrl = app.settings.ai.claude.baseUrl ?? '';
    refreshClaude();
    toast(t('runa.prov.saved.' + r.provider) + (r.keyRemoved ? ' ' + t('runa.prov.oldKeyRemoved') : ''), { kind: 'success' });
  });

  async function removeProvider() {
    if (provKind === 'account') return;
    if (!(await confirmDialog({ title: t('runa.prov.removeTitle'), body: t('runa.prov.removeBody'), confirmLabel: t('runa.prov.remove'), danger: true }))) return;
    await run('token', async () => {
      await del('/ai/claude/provider');
      apiToken = ''; provTest = null;
      await syncServerFields();
      provUrl = '';
      refreshClaude();
      toast(t('runa.prov.removed'));
    });
  }

  /** Can a test be made: a key is typed, or the one saved for this very address is there. */
  const canTest = $derived(!provErr && (!!apiToken.trim() || (hasKey && dest(savedUrl) === dest(provUrl))));
  // What is typed is tried as it stands, saved or not; with nothing typed, the saved set-up is.
  const testProvider = () => run('provtest', async () => {
    provTest = null;
    provTest = await post('/ai/claude/provider/test', provDirty ? { baseUrl: provUrl.trim() || null, token: apiToken.trim() || null } : undefined);
  });

  async function browseLibrary() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const p = await open({ directory: true, defaultPath: s.libraryPath || undefined });
      if (typeof p === 'string') { s.libraryPath = p; autoSave(); }
    } catch { toast(t('set.browseFail'), { kind: 'error' }); }
  }

  function setSyncKind(k: string) {
    const sends = k === 'git' || k === 'drive';
    s.sync.mode = sends ? (isGit ? s.sync.mode : 'git-manual') : k;
    if (sends) { s.sync.target = k === 'drive' ? 'drive' : 'online'; s.keepHistory = true; }
  }

  /** The history needs the free tool Git: start its installer (macOS), or show where to get it. */
  const installGit = () => run('gitInstall', async () => {
    const r = await post('/git/install');
    if (r?.url) { try { const { openUrl } = await import('@tauri-apps/plugin-opener'); await openUrl(r.url); } catch { window.open(r.url, '_blank', 'noopener'); } }
    if (r?.hint) toast(t('ob.gitInstallToast'), { detail: String(r.hint), duration: 0 }); else toast(t('ob.gitInstallToast'));
  });

  async function addRemote() {
    if (!validRemote) return;
    const ok = await confirmDialog({ title: t('set.uploadTitle'), body: t('set.uploadBody'), list: [remote.url.trim()], confirmLabel: t('set.addRemote') });
    if (!ok) return;
    await run('remote', async () => {
      await post('/git/remotes', { ...remote, name: remote.name.trim() || freeName(), url: remote.url.trim() });
      remote = { name: '', url: '', username: '', token: '' };
      await loadGit();
      await refreshBackup();
    });
  }

  /** A short name for a location the user did not name: "origin" for the first, then "backup", "backup-2"… */
  function freeName(): string {
    const taken = new Set<string>((gitStatus.remotes || []).map((r: any) => String(r.name)));
    if (!taken.size) return 'origin';
    for (let n = 1; ; n++) { const name = n === 1 ? 'backup' : `backup-${n}`; if (!taken.has(name)) return name; }
  }

  /** Receive changes from this location from now on (every location is still sent to). */
  async function makeMain(r: any) {
    s.sync.mainLocation = r.name;
    await autoSave();
    await loadGit();
  }

  async function removeRemote(r: any) {
    if (!(await confirmDialog({ title: t('set.removeRemoteTitle'), body: t('histb.loc.removeBody'), list: [r.url], confirmLabel: t('set.remove'), danger: true }))) return;
    await run('remote', async () => { await del(`/git/remotes/${encodeURIComponent(r.name)}`); if (signForm?.location === r.name) signForm = null; await loadGit(); await refreshBackup(); });
  }

  const enableModule = (id: string) => run('mod:' + id, async () => {
    const r = await post(`/modules/${id}/enable`);
    await syncServerFields();
    if (r?.chatId) go({ kind: 'chat', id: r.chatId });
  });

  /** Turning a feature off hides it; nothing the user wrote with it is deleted. */
  async function disableModule(m: any) {
    const name = (s.language === 'ar' && m.nameAr) || m.name;
    if (!(await confirmDialog({ title: t('ui.set.modOffTitle', { name }), body: t('ui.set.modOffBody'), confirmLabel: t('ui.set.modOff') }))) return;
    await run('mod:' + m.id, async () => {
      await post(`/modules/${m.id}/disable`);
      await syncServerFields();
      await loadModules();
      toast(t('ui.set.modOffDone', { name }), { kind: 'success' });
    });
  }

  async function reorganize() {
    const instructions = await promptDialog({ title: t('set.reorgTitle'), body: t('set.reorgBody'), placeholder: t('set.reorgPh'), confirmLabel: t('set.reorgStart') });
    if (instructions === null) return;
    await run('reorg', async () => { const r = await post('/reorganize', { instructions }); go({ kind: 'chat', id: r.chatId }); });
  }

  async function restart() {
    if (busy.restart) return;
    busy.restart = true;
    try { await restartService(); } finally { busy.restart = false; }
  }

  async function addPrivate() {
    privErr = '';
    const lib = String(base.libraryPath ?? '').replace(/\\/g, '/').replace(/\/+$/, '');
    let rel: string | null = null;
    if (inTauri()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const p = await open({ directory: true, defaultPath: lib || undefined });
        if (typeof p !== 'string') return;
        const abs = p.replace(/\\/g, '/').replace(/\/+$/, '');
        if (!lib || abs === lib || !abs.startsWith(lib + '/')) { privErr = t('set.privOutside'); return; }
        rel = abs.slice(lib.length + 1);
      } catch { rel = null; }
    }
    if (rel === null) {
      const typed = await promptDialog({ title: t('set.privAdd'), body: t('set.privTypeBody'), placeholder: 'clients/confidential' });
      if (typed === null) return;
      rel = typed.trim().replace(/\\/g, '/').replace(/^\.?\/+|\/+$/g, '');
      if (!rel || rel.split('/').includes('..') || /^([A-Za-z]:|~)/.test(rel)) { privErr = t('set.privOutside'); return; }
    }
    if (s.privatePaths.includes(rel)) { privErr = t('set.privDup'); return; }
    s.privatePaths = [...s.privatePaths, rel];
    await autoSave();
    // What the index already says about this folder's items stays readable: offer to remove it.
    await offerIndexCleanup(rel);
  }
  const closeLibrary = () => run('access', async () => { openToOthers = await keepLibraryToMe(); });

  async function removePrivate(p: string) {
    if (!(await confirmDialog({ title: t('set.privRemoveTitle'), body: t('set.privRemoveBody'), list: [p], confirmLabel: t('set.remove'), danger: true }))) return;
    s.privatePaths = s.privatePaths.filter((x: string) => x !== p);
    autoSave();
  }
</script>

<svelte:window onbeforeunload={onBeforeUnload} />

{#snippet err(key: string)}
  {#if errors[key]}<p class="err" id={'st-err-' + key} role="alert">{errors[key]}</p>{/if}
{/snippet}

{#snippet resetRow(c: Cat)}
  {#if resets[c] && !q}
    <div class="reset"><button type="button" class="btn ghost" onclick={() => resetCat(c)}><Icon name="rollback" size={14} /> {t('set.reset')}</button></div>
  {/if}
{/snippet}

{#snippet section(c: Cat)}
  {#if c === 'general'}
    <div class="card">
      {#if vis(c, 'language')}
        <label for="st-lang">{t('language')}</label>
        <select id="st-lang" bind:value={s.language}><option value="ar" lang="ar">العربية</option><option value="en" lang="en">English</option></select>
      {/if}
      {#if vis(c, 'set.name')}
        <label for="st-name">{t('set.name')}</label><input id="st-name" dir="auto" bind:value={s.userName} autocomplete="off" />
      {/if}
      {#if vis(c, 'ui.set.profession')}
        <label for="st-prof">{t('ui.set.profession')}</label>
        <select id="st-prof" bind:value={s.profession} aria-describedby="st-prof-hint">
          <option value={null}>{t('ui.set.professionNone')}</option>
          {#each professions as p}<option value={p}>{t('layer.prof.' + p)}</option>{/each}
          {#if s.profession && !professions.includes(s.profession)}<option value={s.profession}>{s.profession}</option>{/if}
        </select>
        <p class="muted sm" id="st-prof-hint">{t('ui.set.professionHint')}</p>
      {/if}
      {#if vis(c, 'theme', 'mode')}<ThemePicker bind:theme={s.theme} bind:mode={s.mode} />{/if}
      {#if vis(c, 'set.notify')}
        <label class="check"><input type="checkbox" bind:checked={s.notifications} onchange={(e) => onNotifySwitch(e.currentTarget.checked)} aria-describedby="st-notif-state" /> {t('set.notify')}</label>
        <div class="row wrap notif" id="st-notif-state">
          {#if s.notifications && notif === 'blocked'}
            <p class="note grow" role="status"><Icon name="warning" size={15} /> <span>{t('shellui.notif.blocked')}</span></p>
            {#if notifySettingsUrl()}<button type="button" class="btn" onclick={async () => { if (!(await openNotifySettings())) toast(t('nativea.notif.openFailed'), { kind: 'error' }); }}>{t('nativea.notif.open')}</button>{/if}
          {:else if notif === 'sent'}
            <span class="muted sm grow" role="status">{t('revui.notif.sent')}</span>
            {#if notifySettingsUrl()}<button type="button" class="btn" onclick={async () => { if (!(await openNotifySettings())) toast(t('nativea.notif.openFailed'), { kind: 'error' }); }}>{t('nativea.notif.open')}</button>{/if}
          {:else if notif === 'granted'}
            <span class="muted sm grow">{t('shellui.notif.allowed')}</span>
          {:else}<span class="grow"></span>{/if}
          <button type="button" class="btn ghost" disabled={busy.notif} aria-busy={busy.notif} onclick={sendTest}><Icon name="bell" size={14} /> {t('shellui.notif.test')}</button>
        </div>
      {/if}
      {#if vis(c, 'todob.set.title', 'todob.set.weekStart', 'todob.set.dates') && s.calendar}<CalendarPrefs bind:cal={s.calendar} />{/if}
      {#if vis(c, 'nativeb.set.title', 'nativeb.set.switch')}<QuickCapturePrefs />{/if}
    </div>
  {:else if c === 'accessibility'}
    <div class="card">
      {#if vis(c, 'a11y.zoom')}
        <label for="st-zoom">{t('a11y.zoom')}</label>
        <div class="row wrap">
          <button type="button" class="btn" disabled={ax.zoom <= ZOOM_STEPS[0]} onclick={() => stepZoom(-1)} aria-label={t('a11y.zoomOut')} title={tip(t('a11y.zoomOut'), 'zoom.out')}><Icon name="zoomOut" size={15} /></button>
          <select id="st-zoom" class="num" value={ax.zoom} onchange={(e) => setAx({ zoom: +e.currentTarget.value }, e.currentTarget)} aria-describedby="st-zoom-hint">
            {#each zoomChoices as z}<option value={z}>{fmtNum(z)}%</option>{/each}
          </select>
          <button type="button" class="btn" disabled={ax.zoom >= ZOOM_STEPS[ZOOM_STEPS.length - 1]} onclick={() => stepZoom(1)} aria-label={t('a11y.zoomIn')} title={tip(t('a11y.zoomIn'), 'zoom.in')}><Icon name="zoomIn" size={15} /></button>
          <button type="button" class="btn ghost" disabled={ax.zoom === 100} onclick={() => setAx({ zoom: 100 })}>{t('a11y.zoomReset')}</button>
        </div>
        <p class="muted sm" id="st-zoom-hint">{t('a11y.zoomHint', { in: isolatedKeys('zoom.in'), out: isolatedKeys('zoom.out'), reset: isolatedKeys('zoom.reset') })}</p>
      {/if}
      {#each [['a11y.motion', 'reduceMotion'], ['a11y.links', 'underlineLinks'], ['a11y.focus', 'strongFocus'], ['a11y.spacing', 'textSpacing']] as [key, field]}
        {#if vis(c, key)}
          <label class="check ax"><input type="checkbox" checked={!!ax[field as keyof A11y]} onchange={(e) => setAx({ [field]: e.currentTarget.checked } as Partial<A11y>, e.currentTarget)} aria-describedby={'st-' + field + '-hint'} /> {t(key)}</label>
          <p class="muted sm axhint" id={'st-' + field + '-hint'}>{t(key + 'Hint')}</p>
        {/if}
      {/each}
      {#if !q}
        <p class="muted sm row wrap gap-top"><span>{t('a11y.contrast')}</span> <button type="button" class="btn ghost" onclick={() => (tab = 'general')}>{t('a11y.contrastOpen')}</button></p>
      {/if}
    </div>
  {:else if c === 'ai'}
    <div class="card">
      <CodexSettings bind:ai={s.ai} part="which" vis={(...k) => vis(c, ...k)} {readOnly} />
      {#if vis(c, 'set.account')}
        <h3>{t('set.account')}</h3>
        <div class="row wrap">
          {#if claude.installed && (provKind === 'anthropicKey' || provKind === 'service')}
            <span class="pill accent">{t('runa.acct.' + provKind)}</span>
          {:else if claude.installed && provKind === 'incomplete'}
            <span class="pill">{t('runa.acct.incomplete')}</span>
          {:else if claude.installed}
            <span class="pill" class:accent={claude.loggedIn}>{t(claude.loggedIn ? 'set.signedIn' : 'set.notSignedIn')}</span>
            {#if !claude.loggedIn}<button type="button" class="btn primary" disabled={busy.login} onclick={signIn}>{t('set.signIn')}</button>{/if}
          {:else}
            <span class="pill">{t('set.notInstalled')}</span>
            <a href="https://claude.com/claude-code" target="_blank" rel="noreferrer">{t('set.installClaude')} <Icon name="external" size={13} /></a>
          {/if}
          <button type="button" class="btn ghost" disabled={busy.claude} aria-busy={busy.claude} onclick={refreshClaude}><Icon name="sync" size={14} /> {busy.claude ? t('set.checking') : t('set.checkAgain')}</button>
        </div>
        {#if (app.wrap as any)?.summariesFailing}<p class="err sm" role="status">{t('runc.sumFail')}</p>{/if}
        <p class="muted sm">{t(provKind === 'incomplete' ? 'runa.acct.incompleteHint' : provKind === 'account' ? 'set.accountHint' : 'runa.acct.keyHint')}</p>
        <!-- Not installed, or not signed in: the steps to get there, checked as they happen (FEAT-07). -->
        {#if !claude.installed || (provKind === 'account' && !claude.loggedIn)}<AssistantGuide bind:claude keyInstead={provKind === 'anthropicKey' || provKind === 'service'} />{/if}
      {/if}
      <CodexSettings bind:ai={s.ai} part="codex" vis={(...k) => vis(c, ...k)} {readOnly} />
      {#if vis(c, 'set.perm')}
        <fieldset class="perm" aria-describedby={errors.permission ? 'st-err-permission' : undefined}>
          <legend>{t('set.perm')}</legend>
          {#each perms as p}
            <label class="radio">
              <input type="radio" name="st-perm" value={p} checked={permValue === p} onclick={(e) => pickPerm(p, e)} />
              <span><b class:danger-text={p === BYPASS}>{t('set.perm.' + p)}</b><span class="muted sm blk">{t('set.perm.' + p + '.hint')}</span></span>
            </label>
          {/each}
        </fieldset>
        {@render err('permission')}
      {/if}
      {#if vis(c, 'set.wrapNag')}
        <label for="st-nag">{t('set.wrapNag')}</label>
        <input id="st-nag" class="num" type="number" min="1" step="1" bind:value={s.wrapNagThreshold} aria-invalid={!!errors.wrapNagThreshold} aria-describedby="st-nag-hint" />
        {@render err('wrapNagThreshold')}
        <p class="muted sm" id="st-nag-hint">{t('set.wrapDef')}</p>
      {/if}
      {#if vis(c, 'chatc.set.suggest')}
        <label class="check"><input type="checkbox" checked={!!s.ai.claude.suggestNext} onchange={(e) => (s.ai.claude.suggestNext = e.currentTarget.checked)} aria-describedby="st-suggest-hint" /> {t('chatc.set.suggest')}</label>
        <p class="muted sm" id="st-suggest-hint">{t('chatc.set.suggestHint')}</p>
      {/if}
      {#if vis(c, 'r10-chatview.set.title')}
        <label class="check"><input type="checkbox" checked={!!s.chatSteps} onchange={(e) => (s.chatSteps = e.currentTarget.checked)} aria-describedby="st-steps-hint" /> {t('r10-chatview.set.title')}</label>
        <p class="muted sm" id="st-steps-hint">{t('r10-chatview.set.hint')}</p>
      {/if}
    </div>
    {#if vis(c, ...adv.ai)}
      <details class="card adv" open={!!q || advOpen.ai} ontoggle={(e) => { if (!q) advOpen.ai = e.currentTarget.open; }}>
        <summary>{t('set.advanced')}</summary>
        {#if vis(c, 'set.model')}
          <label for="st-model">{t('set.model')}</label>
          <input id="st-model" dir="ltr" list="st-models" bind:value={s.ai.claude.model} placeholder={t('set.auto')} autocomplete="off" />
          <datalist id="st-models">{#each opts.models as m}<option value={m}></option>{/each}</datalist>
          <p class="muted sm">{t('set.modelHint')}</p>
        {/if}
        {#if vis(c, 'set.effort')}
          <label for="st-effort">{t('set.effort')}</label>
          <select id="st-effort" bind:value={s.ai.claude.effort}><option value={null}>{t('set.auto')}</option>{#each opts.efforts as m}<option value={m}>{tx('set.effort.' + m, m)}</option>{/each}</select>
        {/if}
        {#if vis(c, 'set.binary')}
          <label for="st-bin">{t('set.binary')}</label>
          <input id="st-bin" dir="ltr" bind:value={s.ai.claude.binary} placeholder={claude.path || t('set.auto')} autocomplete="off" />
          <p class="muted sm">{t('set.binaryHint')}{#if claude.version} <bdi>({claude.version})</bdi>{/if}</p>
        {/if}
        {#if vis(c, 'set.miniModel')}
          <label for="st-mini">{t('set.miniModel')}</label>
          <input id="st-mini" dir="ltr" bind:value={s.ai.claude.miniWrapModel} placeholder="haiku" autocomplete="off" />
          <p class="muted sm">{t('set.miniHint')}</p>
        {/if}
        {#if vis(c, 'set.idle')}
          <label for="st-idle">{t('set.idle')}</label>
          <input id="st-idle" class="num" type="number" min="1" step="1" bind:value={s.idleMinutes} aria-invalid={!!errors.idleMinutes} />
          {@render err('idleMinutes')}
        {/if}
        {#if vis(c, 'set.provider', 'set.baseUrl', 'set.apiKey')}
          <h3>{t('set.provider')}</h3>
          <p class="muted sm">{t('set.providerHint')} {t('runa.prov.how')}</p>
          <ProviderPresets url={provUrl} disabled={readOnly} onpick={(u) => { provUrl = u; provTest = null; }} />
          <label for="st-base">{t('set.baseUrl')}</label>
          <input id="st-base" dir="ltr" bind:value={provUrl} placeholder="https://api.anthropic.com" aria-invalid={!!provErr} aria-describedby={provErr ? 'st-err-provider' : undefined} autocomplete="off" disabled={readOnly} />
          <label for="st-key">{t('set.apiKey')} {#if hasKey}<span class="pill accent">{t('set.keyOn')}</span>{/if}</label>
          <input id="st-key" dir="ltr" type="password" bind:value={apiToken} placeholder="sk-…" aria-describedby={provErr ? 'st-err-provider' : undefined} autocomplete="off" disabled={readOnly} />
          {#if provErr}<p class="err" id="st-err-provider" role="alert">{provErr}</p>{/if}
          <div class="row wrap gap-top">
            <button type="button" class="btn primary" disabled={!provDirty || !!provErr || busy.token || readOnly} aria-busy={busy.token} onclick={saveProvider}>{t('runa.prov.save')}</button>
            <button type="button" class="btn" disabled={!canTest || busy.provtest} aria-busy={busy.provtest} onclick={testProvider}>{busy.provtest ? t('runa.prov.testing') : t('runa.prov.test')}</button>
            <button type="button" class="btn danger" disabled={provKind === 'account' || busy.token || readOnly} onclick={removeProvider}>{t('runa.prov.remove')}</button>
          </div>
          {#if provTest}<p class="sm" class:err={!provTest.ok} role="status" dir="auto">{provTest.ok ? t('runa.prov.testOk') : provTest.message}</p>{/if}
          <p class="muted sm">{t('runc.prov.testHint')}</p>
          <p class="muted sm">{t('set.apiKeyHint')}</p>
        {/if}
        <CodexSettings bind:ai={s.ai} part="advanced" vis={(...k) => vis(c, ...k)} {readOnly} />
      </details>
    {/if}
  {:else if c === 'usage'}
    <UsageSettings bind:value={s.spending} disabled={readOnly} vis={(...k) => vis(c, ...k)} />
  {:else if c === 'git'}
    <div class="card">
      {#if vis(c, 'set.libraryPath')}
        <label for="st-lib">{t('set.libraryPath')}</label>
        <div class="row"><input id="st-lib" class="grow" dir="ltr" bind:value={s.libraryPath} aria-invalid={!!errors.libraryPath} autocomplete="off" /><button type="button" class="btn" onclick={browseLibrary}><Icon name="folder" size={14} /> {t('set.browse')}</button></div>
        {@render err('libraryPath')}
        <p class="muted sm">{t('set.libraryHint')}</p>
      {/if}
      {#if vis(c, 'hista.historyTitle', 'hista.keep')}
        <h3>{t('hista.historyTitle')}</h3>
        <label class="check"><input type="checkbox" checked={historyOn} disabled={isGit} onchange={(e) => (s.keepHistory = e.currentTarget.checked)} aria-describedby="st-history-hint" /> {t('hista.keep')}</label>
        <p class="muted sm" id="st-history-hint">{t(isGit ? 'hista.keepForced' : historyOn ? 'hista.keepHint' : 'hista.state.off')}</p>
        {#if historyOn && historySaved}
          {#if gitMissing}
            <p class="note"><Icon name="warning" size={15} /> <span>{t('hista.state.noGit')}</span></p>
            <button type="button" class="btn" disabled={busy.gitInstall} onclick={installGit}>{t('ob3.installGit')}</button>
          {:else if gitStatus.repo}
            <div class="row wrap"><span class="muted sm grow">{t(gitStatus.kept ? 'hista.state.kept' : 'hista.state.own')}</span><button type="button" class="btn" onclick={() => go({ kind: 'history' })}><Icon name="rollback" size={14} /> {t('shell.openRollback')}</button></div>
          {:else if !loading.git && !loadErr.git}
            <p class="muted sm">{t('hista.state.starting')}</p>
          {/if}
        {/if}
      {/if}
      {#if vis(c, 'set.backup', 'hista.backup.online', 'histb.backup.drive')}
        <h3>{t('set.backup')}</h3>
        <label for="st-sync" class="sr-only">{t('set.backup')}</label>
        <select id="st-sync" value={syncKind} onchange={(e) => setSyncKind(e.currentTarget.value)} aria-invalid={!!errors.sync} aria-describedby="st-sync-hint">
          <option value="none">{t('hista.backup.none')}</option>
          <option value="folder">{t('hista.backup.folder')}</option>
          <option value="git">{t('hista.backup.online')}</option>
          <option value="drive">{t('histb.backup.drive')}</option>
        </select>
        {@render err('sync')}
        <p class="muted sm" id="st-sync-hint">{t(syncKind === 'none' ? 'hista.backup.noneNote' : syncKind === 'folder' ? 'hista.backup.folderNote' : syncKind === 'drive' ? 'histb.backup.driveNote' : 'hista.backup.onlineNote')}</p>
        {#if syncKind === 'folder' && String(s.libraryPath ?? '').trim()}
          {#if inCloud}<p class="muted sm">{t('hista.cloud.inside', { service: cloudName })}</p>
          {:else}<p class="note"><Icon name="warning" size={15} /> <span>{t('hista.cloud.notInside')}</span></p>{/if}
        {:else if syncKind === 'git' && inCloud}
          <p class="note"><Icon name="warning" size={15} /> <span>{t(cloudAlready && !s.sync.allowCloud ? 'hista.cloud.existing' : 'hista.cloud.clash', { service: cloudName })}</span></p>
          <label class="check"><input type="checkbox" bind:checked={s.sync.allowCloud} /> {t('hista.cloud.anyway')}</label>
        {/if}
      {/if}
      {#if isGit && vis(c, 'hista.backup.online', 'histb.backup.drive', 'hista.online.when', 'set.credStore', 'histb.loc.title', 'hista.online.locations', 'histb.signin.update')}
        {#if syncKind === 'git'}<p class="muted sm">{t('hista.online.body')}</p>{/if}
        {#if vis(c, 'hista.backup.online', 'histb.backup.drive', 'hista.online.when')}
          <label for="st-gitmode">{t('hista.online.when')}</label>
          <select id="st-gitmode" bind:value={s.sync.mode}>
            <option value="git-manual">{t('hista.online.manual')}</option>
            <option value="git-auto">{t('hista.online.auto')}</option>
          </select>
        {/if}
        {#if (syncKind === 'git' || signIns.length) && vis(c, 'hista.backup.online', 'set.credStore')}
          <label for="st-cred">{t('set.credStore')}</label>
          <select id="st-cred" bind:value={s.sync.credentialStore}>
            <option value="keychain">{t('set.credKeychain')}</option>
            <option value="file">{t('set.credFile')}</option>
          </select>
          <p class="muted sm">{t(s.sync.credentialStore === 'file' ? 'ui.set.credFileHint' : 'ui.set.credKeychainHint')}</p>
        {/if}
        {#if vis(c, 'hista.backup.online', 'histb.backup.drive', 'histb.loc.title', 'hista.online.locations', 'histb.signin.update')}
          <h3 id="st-locations">{t('histb.loc.title')}</h3>
          {#if loadErr.git}<p class="err" role="alert">{loadErr.git} <button type="button" class="btn ghost" onclick={loadGit}>{t('set.retry')}</button></p>{/if}
          {#if healthText(health)}<p class="note" class:good={health?.state === 'ok'} role="status"><Icon name={health?.state === 'ok' ? 'check' : 'warning'} size={15} /> <span>{healthText(health)}</span></p>{/if}
          {#if gitStatus.repo}<p class="muted sm">{t('set.gitState', { branch: gitStatus.branch, changed: fmtNum(gitStatus.dirty ?? 0), ahead: fmtNum(gitStatus.ahead ?? 0), behind: fmtNum(gitStatus.behind ?? 0) })}</p>{/if}
          {#each gitStatus.remotes || [] as r, i (r.name)}
            {@const h = locHealth(r.name)}
            {@const si = r.kind === 'online' ? signInOf(r.name) : null}
            <div class="remote" id="st-loc-{r.name}">
              <div class="row"><b dir="auto">{r.name}</b>{#if r.main ?? i === 0}<span class="pill">{t('set.remoteMain')}</span>{/if}<code class="grow sm path"><bdi>{r.url}</bdi></code>{#if !(r.main ?? i === 0)}<button type="button" class="btn ghost" disabled={busy.remote || busy.save} onclick={() => makeMain(r)}>{t('hista.online.makeMain')}</button>{/if}<button type="button" class="btn ghost danger" disabled={busy.remote} onclick={() => removeRemote(r)}>{t('set.remove')}</button></div>
              <p class="muted sm loc-line">
                <span>{t('histb.loc.kind.' + (r.kind ?? 'online'))}</span>
                {#if h}<span>{h.at ? t('histb.loc.at', { when: ago(h.at) }) : t('histb.loc.never')}</span>{/if}
                {#if h && h.unsent > 0}<span>{tn('histb.loc.unsent', h.unsent)}</span>{/if}
                {#if r.kind === 'folder' && h?.present === false}<span class="warn-text">{t('histb.loc.unplugged')}</span>{/if}
              </p>
              {#if h?.why}<p class="err sm" dir="auto">{t('histb.loc.failed', { why: h.why })}</p>{/if}
              {#if r.kind === 'ssh'}
                <div class="row wrap"><span class="muted sm grow">{t('histb.signin.ssh')}</span>{#if h?.whyKey === 'sync.why.host_key'}<button type="button" class="btn" disabled={busy.host} aria-busy={busy.host} onclick={() => checkServer(r)}>{t('histb.host.check')}</button>{/if}</div>
              {:else if si?.insecure}
                <p class="note sm"><Icon name="warning" size={15} /> <span>{t('histb.signin.insecure')}</span></p>
              {:else if si}
                <div class="row wrap">
                  <span class="muted sm grow" dir="auto">{si.saved ? (si.username ? t('histb.signin.as', { name: si.username }) : t('histb.signin.token')) : t('histb.signin.none')}{#if (si.locations ?? []).length > 1} · {t('histb.signin.shared', { names: listNames(si.locations.filter((n: string) => n !== r.name)) })}{/if}</span>
                  {#if signForm?.location !== r.name}<button type="button" class="btn" class:primary={h?.whyKey === 'sync.why.sign_in'} onclick={() => openSignIn(r.name)}>{t('histb.signin.update')}</button>{/if}
                  {#if si.saved}<button type="button" class="btn ghost danger" disabled={busy.forget} onclick={() => forgetSignIn(si)}>{t('histb.signin.forget')}</button>{/if}
                </div>
              {/if}
              {#if signForm && signForm.location === r.name}
                <div class="signin">
                  <div class="grid2">
                    <div><label for="st-su-{i}">{t('set.remoteUser')}</label><input id="st-su-{i}" dir="ltr" bind:value={signForm.username} autocomplete="off" /></div>
                    <div><label for="st-st-{i}">{t('set.remoteToken')}</label><input id="st-st-{i}" dir="ltr" type="password" bind:value={signForm.token} autocomplete="off" onkeydown={(e) => { if (e.key === 'Enter') saveSignIn(); }} /></div>
                  </div>
                  <p class="muted sm">{t('histb.signin.hint')}</p>
                  <div class="row wrap">
                    <button type="button" class="btn primary" disabled={!signForm.token || busy.signin} aria-busy={busy.signin} onclick={saveSignIn}>{busy.signin ? t('histb.signin.checking') : t('histb.signin.save')}</button>
                    <button type="button" class="btn ghost" onclick={() => (signForm = null)}>{signForm.result ? t('close') : t('cancel')}</button>
                  </div>
                  {#if signForm.result}<p class="sm" class:err={!signForm.ok} role="status" dir="auto">{signForm.result}</p>{/if}
                </div>
              {/if}
            </div>
          {:else}
            {#if !loading.git && !loadErr.git}<p class="muted sm">{t('set.noRemotes')}</p>{/if}
          {/each}
          {#if (gitStatus.remotes || []).length > 1}<p class="muted sm">{t('hista.online.mainHint')}</p>{/if}
          {#if !loading.git && !loadErr.git && (gitStatus.remotes || []).length}<p class="muted sm">{lastBackupText(backup.last ?? gitStatus.lastBackup ?? null) || t('hista.sync.never')}</p>{/if}
          {#if syncKind === 'drive'}
            <p class="muted sm">{t('histb.drive.hint')}</p>
            <button type="button" class="btn primary" disabled={busy.drive} aria-busy={busy.drive} onclick={addDrive}><Icon name="folder" size={14} /> {t('histb.drive.add')}</button>
          {:else}
            <div class="grid2">
              <div><label for="st-rurl">{t('hista.online.addressShort')}</label><input id="st-rurl" dir="ltr" bind:value={remote.url} placeholder="https://github.com/me/thoughts.git" autocomplete="off" /></div>
              <div><label for="st-rname">{t('set.remoteName')}</label><input id="st-rname" dir="ltr" bind:value={remote.name} placeholder="origin" autocomplete="off" /></div>
              <div><label for="st-ruser">{t('set.remoteUser')}</label><input id="st-ruser" dir="ltr" bind:value={remote.username} autocomplete="off" /></div>
              <div><label for="st-rtoken">{t('set.remoteToken')}</label><input id="st-rtoken" dir="ltr" type="password" bind:value={remote.token} autocomplete="off" /></div>
            </div>
            {#if remote.url.trim() && !validRemote}<p class="err" role="alert">{t('set.errRemote')}</p>{/if}
            <p class="note"><Icon name="warning" size={15} /> <span>{t('set.uploadBody')}</span></p>
            <div class="row wrap">
              <button type="button" class="btn" onclick={addRemote} disabled={!validRemote || busy.remote}>{t('set.addRemote')}</button>
              <button type="button" class="btn ghost" disabled={busy.drive} aria-busy={busy.drive} onclick={addDrive}><Icon name="folder" size={14} /> {t('histb.drive.addAlso')}</button>
            </div>
          {/if}
        {/if}
      {/if}
      {#if vis(c, 'reorganize')}
        <h3>{t('reorganize')}</h3>
        <div class="row wrap"><button type="button" class="btn" disabled={busy.reorg} onclick={reorganize}>{t('reorganize')}…</button><span class="muted sm grow">{t('set.reorgHint')}</span></div>
      {/if}
    </div>
  {:else if c === 'privacy'}
    {#if vis(c, 'set.privTitle', 'appr.strict.title', 'seca.access.title')}
      <div class="card">
        <h3>{t('set.privSentTitle')}</h3>
        <p class="muted sm">{t('set.privSent')}</p>
        <h3>{t('set.privTitle')}</h3>
        <p class="muted sm">{t('appr.privBody')}</p>
        {#each s.privatePaths as p (p)}
          <div class="row remote"><Icon name="shield" size={15} /><code class="grow path"><bdi>{p}</bdi></code><button type="button" class="btn ghost danger" onclick={() => removePrivate(p)}>{t('set.remove')}</button></div>
        {:else}
          <p class="muted sm">{t('set.privEmpty')}</p>
        {/each}
        {#if privErr}<p class="err" role="alert">{privErr}</p>{/if}
        <div class="row gap-top"><button type="button" class="btn" onclick={addPrivate}><Icon name="plus" size={14} /> {t('set.privAdd')}</button></div>
        <h3>{t('appr.strict.title')}</h3>
        <p class="muted sm" id="st-strict-hint">{t('appr.strict.body')}</p>
        {#if !s.privatePaths.length}<p class="muted sm">{t('appr.strict.none')}</p>{/if}
        <label class="check"><input type="checkbox" bind:checked={s.strictPrivate} disabled={readOnly} aria-describedby="st-strict-hint" /> {t('appr.strict.toggle')}</label>
        {#if openToOthers !== null}
          <h3>{t('seca.access.title')}</h3>
          {#if openToOthers}
            <p class="muted sm">{t('seca.access.open')}</p>
            <div class="row gap-top"><button type="button" class="btn" disabled={busy.access} onclick={closeLibrary}>{t('seca.access.fix')}</button></div>
          {:else}
            <p class="muted sm">{t('seca.access.closed')}</p>
          {/if}
        {/if}
      </div>
    {/if}
    {#if vis(c, 'wrapa.set.auto')}
      <div class="card">
        <h3>{t('wrapa.set.auto')}</h3>
        <p class="muted sm" id="st-autosum-hint">{t('wrapa.set.autoHint')}</p>
        <label class="check"><input type="checkbox" bind:checked={s.autoSummaries} aria-describedby="st-autosum-hint" /> {t('wrapa.set.auto')}</label>
      </div>
    {/if}
    {#if vis(c, 'wording.reports.title')}
      <!-- SEC-50: what the assistant program sends its maker on its own; off unless switched on. -->
      <div class="card">
        <h3>{t('wording.reports.title')}</h3>
        <p class="muted sm" id="st-reports-hint">{t('wording.reports.hint')}</p>
        <label class="check"><input type="checkbox" bind:checked={s.assistantReports} aria-describedby="st-reports-hint" /> {t('wording.reports.toggle')}</label>
      </div>
    {/if}
    {#if vis(c, 'layer.signalsTitle')}
      <div class="card">
        <h3>{t('layer.signalsTitle')}</h3>
        <p class="muted sm" id="st-signals-hint">{t('layer.signalsBody')}</p>
        <label class="check"><input type="checkbox" bind:checked={s.usageSignals} aria-describedby="st-signals-hint" /> {t('layer.signalsToggle')}</label>
      </div>
    {/if}
    {#if vis(c, 'set.cost')}
      <div class="card">
        <h3>{t('set.cost')}</h3>
        {#if typeof cost === 'number'}
          <p class="cost"><bdi>{fmtMoney(cost)}</bdi></p>
        {:else}
          <p class="muted sm">{t('set.costNone')}</p>
        {/if}
        <p class="muted sm">{t('set.costHint')}</p>
        {#if vis(c, 'runc.spend.title')}<SpendingGuard bind:value={s.spending} disabled={readOnly} error={errors.spending ?? ''} />{/if}
        {#if vis(c, 'ui.set.showCost')}
          <label class="check"><input type="checkbox" checked={app.showCost} onchange={(e) => setShowCost(e.currentTarget.checked)} aria-describedby="st-cost-hint" /> {t('ui.set.showCost')}</label>
          <p class="muted sm" id="st-cost-hint">{t('ui.set.showCostHint')}</p>
        {/if}
      </div>
    {/if}
    {#if vis(c, 'secb.keys.title')}<AccessKeys />{/if}
  {:else if c === 'modules'}
    <div class="card">
      <p class="muted">{t('set.modulesIntro')} {t('ui.set.modKeeps')}</p>
      {#if loading.modules}<p class="muted sm" role="status">{t('set.loading')}</p>
      {:else if loadErr.modules}<p class="err" role="alert">{loadErr.modules} <button type="button" class="btn ghost" onclick={loadModules}>{t('set.retry')}</button></p>
      {:else}
        {#each modules as m (m.id)}
          <div class="row mod">
            <div class="grow"><b>{(s.language === 'ar' && m.nameAr) || m.name}</b><div class="muted sm">{(s.language === 'ar' && m.descriptionAr) || m.description}</div></div>
            {#if s.modules?.[m.id]}<span class="pill accent">{t('enabled')}</span>
              {#if m.canDisable !== false}<button type="button" class="btn ghost" disabled={busy['mod:' + m.id]} onclick={() => disableModule(m)}>{t('ui.set.modOff')}</button>{/if}
            {:else}<button type="button" class="btn" disabled={busy['mod:' + m.id]} onclick={() => enableModule(m.id)}>{busy['mod:' + m.id] ? t('set.enabling') : t('enable')}</button>{/if}
          </div>
        {:else}
          <p class="muted sm">{t('set.noModules')}</p>
        {/each}
      {/if}
    </div>
  {:else if c === 'daemon'}
    <div class="card">
      <p class="muted">{t('set.daemonIntro')}</p>
      {#if vis(c, 'set.keepRunning')}<label class="check"><input type="checkbox" bind:checked={s.daemon.keepRunning} /> {t('set.keepRunning')}</label>{/if}
      {#if vis(c, 'set.runAtLogin')}<label class="check"><input type="checkbox" bind:checked={s.daemon.runAtLogin} /> <span>{t('set.runAtLogin')} {#if app.status?.autostart?.supported === false || app.status?.autostartSupported === false}<span class="muted">{t('set.macOnly')}</span>{/if}</span></label>{/if}
    </div>
    {#if vis(c, ...adv.daemon)}
      <details class="card adv" open={!!q || advOpen.daemon} ontoggle={(e) => { if (!q) advOpen.daemon = e.currentTarget.open; }}>
        <summary>{t('set.advanced')}</summary>
        <p class="muted sm">{t('set.restartHint')}</p>
        <button type="button" class="btn" disabled={busy.restart} onclick={restart}>{busy.restart ? t('set.restarting') : t('set.restart')}</button>
      </details>
    {/if}
  {:else if c === 'maintenance'}
    {#if vis(c, 'care.upd.title')}
      <div class="card">
        <h3>{t('care.upd.title')}</h3>
        <label class="check"><input type="checkbox" bind:checked={s.releaseCheck} aria-describedby="st-release-hint" /> {t('care.upd.toggle')}</label>
        <p class="muted sm" id="st-release-hint">{t('care.upd.hint')}</p>
        {#if app.status?.newerVersion?.version}<p class="muted sm" role="status">{t('care.upd.available', { version: app.status.newerVersion.version })}</p>{/if}
        <button type="button" class="btn" onclick={() => checkAppUpdate(true)}><Icon name="sync" size={14} /> {t('care.upd.check')}</button>
      </div>
    {/if}
    <Maintenance show={(...keys) => vis(c, ...keys)} open={(to) => { tab = to; query = ''; if (to === 'ai') advOpen.ai = true; }} />
  {:else if c === 'appLayer'}
    <!-- Customise has a page of its own (sidebar); this category points there. -->
    <div class="card">
      <p class="muted">{t('ldc.set.moved')}</p>
      <div class="row wrap" style="gap: 8px">
        <button type="button" class="btn primary" onclick={() => command('customise.new')} title={t('r10-cust-start.newTip')}><Icon name="palette" size={15} /> {t('r10-cust-start.new')}</button>
        <button type="button" class="btn" onclick={() => go({ kind: 'customise' })}>{t('ldc.set.open')}</button>
      </div>
    </div>
    {#if vis(c, ...adv.appLayer)}
      <details class="card adv" open={!!q || advOpen.appLayer} ontoggle={(e) => { if (!q) advOpen.appLayer = e.currentTarget.open; }}>
        <summary>{t('set.advanced')}</summary>
        <p class="muted sm">{t('set.safeHint')}</p>
        <button type="button" class="btn" onclick={() => reloadApp('safe=1')}>{t('set.safeMode')}</button>
      </details>
    {/if}
  {/if}
  {@render resetRow(c)}
{/snippet}

<div class="settings">
  <nav class="cats" aria-label={t('settings')}>
    <h1>{t('settings')}</h1>
    <div class="find">
      <Icon name="search" size={14} />
      <input type="search" bind:value={query} placeholder={t('set.searchPh')} aria-label={t('set.searchPh')} autocomplete="off" />
    </div>
    {#each cats as k}
      {#if !q || catShown(k)}
        <button type="button" class="cat" class:on={!q && tab === k} aria-current={!q && tab === k ? 'page' : undefined} onclick={() => { tab = k; query = ''; }}><span class="ci"><Icon name={catIcon[k]} size={15} /></span>{t('set.cat.' + k)}</button>
      {/if}
    {/each}
  </nav>
  <div class="content" onchange={() => autoSave()}>
    {#if readOnly}
      <div class="ro card" role="status"><Icon name="lock" size={16} /><span>{t('ui.settingsNewer')}</span></div>
    {/if}
    {#each shown as c, i (c)}
      <section aria-labelledby={'st-h-' + c}>
        <div class="row titlerow">
          <h2 class="title grow" id={'st-h-' + c}>{t('set.cat.' + c)}</h2>
          {#if i === 0}<span class="saved" role="status">{#if justSaved}<Icon name="check" size={14} /> {t('shellui.set.saved')}{:else if busy.save}{t('set.saving')}{/if}</span>{/if}
        </div>
        <fieldset class="lockable" disabled={readOnly}>{@render section(c)}</fieldset>
      </section>
    {:else}
      <p class="muted none" role="status">{t('set.noMatch', { q: query.trim() })}</p>
    {/each}

    {#if dirty && !readOnly && (invalid || saveFailed)}
      <div class="savebar" role="region" aria-label={t('set.unsaved')}>
        <span class="dot" aria-hidden="true"></span>
        <span class="grow" role="status">{invalid ? t('shellui.set.fixToSave') : t('shellui.set.notSaved')}</span>
        <button type="button" class="btn" disabled={busy.save} onclick={discard}>{t('shellui.set.undo')}</button>
        {#if !invalid}<button type="button" class="btn primary" disabled={busy.save} aria-busy={busy.save} onclick={() => autoSave()}>{busy.save ? t('set.saving') : t('shell.retry')}</button>{/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .settings { display: grid; grid-template-columns: 220px 1fr; min-height: calc(100vh - 26px); }
  .lockable { border: 0; padding: 0; margin: 0; min-width: 0; }
  .ro { display: flex; gap: 10px; align-items: flex-start; margin-bottom: 14px; font-size: 13px; border-color: var(--accent-2); }
  .ro :global(.icon) { flex: none; margin-top: 2px; color: var(--accent-2); }
  .cats { padding-block: 6px 20px; padding-inline: 16px 10px; border-inline-end: 1px solid var(--hairline); display: flex; flex-direction: column; gap: 2px; }
  .cats h1 { font-size: 20px; margin: 4px 6px 12px; }
  .find { position: relative; margin-bottom: 8px; color: var(--muted); }
  .find :global(.icon) { position: absolute; inset-inline-start: 9px; inset-block-start: 50%; transform: translateY(-50%); pointer-events: none; }
  .find input { padding-inline-start: 30px; font-size: 13px; }
  .cat { font: inherit; color: inherit; background: none; border: 0; text-align: start; cursor: pointer; display: flex; align-items: center; gap: 10px; padding: 5px 6px; border-radius: 7px; font-size: 13px; width: 100%; }
  .cat:hover { background: var(--fill); }
  /* A navigation list: the current row is tinted, like the sidebar (see "selected" in theme.css). */
  .cat.on { background: var(--tint); color: var(--on-tint); font-weight: 600; }
  .ci { width: 24px; height: 24px; border-radius: 6px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); flex: none; }
  .cat.on .ci { background: var(--accent); color: var(--accent-fg); }
  :global([data-theme="contrast"]) .cat.on .ci { background: var(--accent-fg); color: var(--accent); }
  .content { padding: 6px 28px 28px; max-width: 760px; min-width: 0; }
  .title { font-size: 20px; margin: 4px 0 14px; }
  .titlerow { align-items: baseline; }
  .saved { font-size: var(--fs-xs); color: var(--ok); display: inline-flex; align-items: center; gap: 4px; min-height: 1em; }
  .notif { margin-top: 4px; }
  .notif .note { margin: 0; }
  section + section { margin-top: 26px; }
  h3 { font-size: 14px; font-weight: 600; margin: 18px 0 6px; }
  .card > h3:first-child { margin-top: 0; }
  .savebar { position: sticky; inset-block-end: 12px; z-index: 5; margin-top: 16px; padding: 10px 14px; display: flex; gap: 8px; align-items: center; background: var(--glass, var(--panel)); -webkit-backdrop-filter: blur(24px); backdrop-filter: blur(24px); border: 1px solid var(--hairline); border-radius: var(--radius-lg); box-shadow: var(--shadow-lg); font-size: 13px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); flex: none; }
  .card { margin-bottom: 12px; }
  .check, .radio { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; }
  .radio { align-items: flex-start; margin: 8px 0; cursor: pointer; }
  .radio input { margin-top: 4px; }
  .blk { display: block; font-weight: 400; }
  .danger-text { color: var(--danger); }
  .ax { margin-top: 14px; }
  .axhint { margin: 2px 0 0; padding-inline-start: 24px; }
  .perm { border: 0; padding: 0; margin: 14px 0 0; min-width: 0; }
  .perm legend { font-size: var(--fs-xs); font-weight: 500; color: var(--muted); padding: 0; }
  .num { max-width: 120px; }
  .adv > summary { cursor: pointer; font-size: 13px; font-weight: 600; }
  .adv[open] > summary { margin-bottom: 6px; }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 0 10px; margin: 4px 0 8px; }
  .wrap { flex-wrap: wrap; }
  .gap-top { margin-top: 10px; }
  .mod, .remote { padding: 10px 0; border-bottom: 1px solid var(--hairline); }
  .remote > p { margin: 4px 0 0; }
  .loc-line { display: flex; flex-wrap: wrap; gap: 4px 12px; }
  .warn-text { color: var(--danger); }
  .signin { margin-top: 8px; padding: 10px 12px; border-radius: var(--radius); background: var(--fill); }
  .note.good { color: var(--ok); }
  .mod:last-child { border-bottom: 0; }
  .path { overflow-wrap: anywhere; }
  .err { color: var(--danger); font-size: var(--fs-sm); margin: 4px 0 0; }
  .note { display: flex; gap: 8px; align-items: flex-start; font-size: var(--fs-sm); margin: 8px 0; }
  .note :global(.icon) { flex: none; margin-top: 3px; color: var(--danger); }
  .cost { font-size: 26px; font-weight: 500; margin: 0 0 4px; }
  .reset { display: flex; justify-content: flex-end; }
  .none { padding: 24px 0; }
  .sm { font-size: var(--fs-xs); }
  /* The window can't be narrower than 920px, so a viewport query never fired: follow the room the view really has. */
  @container main (max-width: 700px) {
    .settings { grid-template-columns: 1fr; min-height: 0; }
    .cats { border-inline-end: 0; border-bottom: 1px solid var(--hairline); flex-direction: row; flex-wrap: wrap; align-items: center; gap: 4px; padding-block-end: 10px; }
    .cats h1, .find { flex-basis: 100%; }
    .cat { width: auto; padding-inline-end: 10px; }
    .content { padding-inline: 16px; }
    .grid2 { grid-template-columns: 1fr; }
  }
</style>
