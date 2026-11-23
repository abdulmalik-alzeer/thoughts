<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { api, post } from '../lib/api';
  import { app, applyTheme, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { imeKey } from '../lib/ime';
  import { promptDialog } from '../lib/dialog.svelte';
  import { cloudFor } from '../lib/cloud.svelte';
  import { driveName } from '../lib/backup.svelte';
  import ThemePicker from './ThemePicker.svelte';
  import Logo from '../lib/Logo.svelte';
  import AssistantGuide from './AssistantGuide.svelte';
  import CodexSettings from './CodexSettings.svelte';
  import HelpTip from '../lib/HelpTip.svelte';
  import RemoteTokenHelp from './RemoteTokenHelp.svelte';
  import { isReady, primaryOf, statusOf } from '../lib/assistantReady';
  import { fmtSize } from '../lib/content/util';
  import { libs, switchLibrary } from '../lib/libraries.svelte';

  // `adding`: another library is added to an app that is already set up (the library switcher). Only
  // the library's own screens are shown; the app's language, look and assistant are not asked again.
  let { done, adding = false, cancel }: { done: (setupChatId: string | null, name?: string) => void; adding?: boolean; cancel?: () => void } = $props();

  // Four steps: Welcome · Assistant · Your notes (folder + backup) · Review (privacy + look).
  const steps = ['welcome', 'ai', 'library', 'ready'] as const;
  let step = $state(adding ? 2 : 0);
  /** The first step shown: adding a library starts at its folder. */
  const first = adding ? 2 : 0;
  let busy = $state(false);
  let checking = $state(false);
  let working = $state<Record<string, boolean>>({});
  let claude = $state<any>(app.status?.claude ?? {});
  let git = $state<any>(app.status?.git ?? { installed: true });
  let modules = $state<any[]>([]);
  let modulesLoading = $state(true);
  let modulesErr = $state('');
  let s = $state<any>(structuredClone($state.snapshot(app.settings)));
  let libraryMode = $state<'new' | 'existing'>('new');
  let seeds = $state('');
  let remoteUrl = $state('');
  let remoteUser = $state('');
  let remoteToken = $state('');
  let remoteAck = $state(false);
  let useApi = $state(false);
  let apiToken = $state('');
  let profession = $state('');
  let professionOther = $state('');
  // What is at the chosen folder, as the background service sees it (asked again whenever the path changes).
  let folder = $state<{ path: string; state: string } | null>(null);
  let probe = $state<'idle' | 'busy' | 'failed'>('idle');
  const professions = ['finance', 'legal', 'consulting', 'teaching', 'research', 'founder', 'technical', 'personal', 'other'];
  // The assistant can be connected later (FEAT-08): the library is made now, its setup waits.
  let later = $state(false);
  // Adding a library to an app whose assistant is not connected: its setup waits, as on first use.
  // (The setup runs on the program chosen in Settings › Assistant, Claude Code or Codex.)
  if (adding) { const p = primaryOf(app.settings); const c = statusOf(app.status, p); later = !(isReady(c, p) || (p === 'claude' && c.installed && app.settings?.ai?.claude?.hasApiToken)); }
  // "I already have one" on a folder of notes (FEAT-09): use it where it is (with the user's yes to
  // what is added), or keep it as it is and bring a copy into a new library.
  let adoptChoice = $state<'' | 'adopt' | 'copy'>('');
  let adoptYes = $state(false);
  let adoptPlan = $state<{ path: string; v?: any; err?: string } | null>(null);
  // A folder of notes whose copy comes into the new library, and what is in it.
  let importFrom = $state('');
  let bring = $state<{ path: string; v?: any; err?: string } | null>(null);

  // Start with the safest choice: nothing leaves this computer until the user asks for a backup.
  // The history of changes is a separate switch, and it starts on.
  s.sync.mode = 'none';
  s.keepHistory = true;
  s.libraryPath = app.status?.home ? app.status.home + '/Thoughts' : '~/Thoughts';
  // Adding a library: nothing of the open library's own comes along (its private folders, its
  // features), and its name suggests its folder until a folder is chosen.
  let libName = $state('');
  let pathTouched = false;
  const homeDir = app.status?.home ? String(app.status.home) : '~';
  if (adding) { s.privatePaths = []; s.modules = {}; }
  $effect(() => {
    const n = libName.replace(/[\\/:*?"<>|]+/g, ' ').replace(/\s+/g, ' ').trim();
    if (adding && !pathTouched) s.libraryPath = homeDir + '/' + (n || t('libs.addDefault'));
  });
  const norm = (p: string) => p.replace(/[\\/]+$/, '');
  /** The typed folder is already one of the user's libraries: it is opened, not added again. */
  const listedHere = $derived(adding ? libs.list.find((l) => norm(l.path) === norm(String(s.libraryPath ?? '').trim())) : undefined);
  function openListed() { const l = listedHere; if (!l) return; cancel?.(); if (!l.open) switchLibrary(l.id); }
  // First screen in the system's language: Arabic systems start in Arabic (an explicit choice wins on a re-run).
  const LANG_KEY = 'thoughts.ob.lang';
  const lsGet = (k: string) => { try { return localStorage.getItem(k); } catch { return null; } };
  const picked = lsGet(LANG_KEY);
  if (picked === 'ar' || picked === 'en') s.language = picked;
  else if (/^ar\b/i.test(navigator.language || '')) s.language = 'ar';
  function setLang(l: 'ar' | 'en') { s.language = l; try { localStorage.setItem(LANG_KEY, l); } catch { /* private mode */ } }
  // Only two ways of working exist: the assistant edits notes, or it only reads.
  if (s.ai?.claude && s.ai.claude.permissionMode === 'default') s.ai.claude.permissionMode = 'plan';

  // Focus follows the step: each screen's heading is announced when it appears.
  let bodyEl = $state<HTMLElement | null>(null);
  let firstStep = true;
  $effect(() => {
    step;
    if (firstStep) { firstStep = false; return; }
    tick().then(() => bodyEl?.querySelector<HTMLElement>('h1')?.focus());
  });

  const ar = $derived(s.language === 'ar');
  const isGit = $derived(String(s.sync.mode).startsWith('git'));
  // "Drive or folder" is sent to the same way as an online location (`sync.target` says which).
  const isDrive = $derived(isGit && s.sync.target === 'drive');
  /** The drive or folder chosen for the backup; added as a location once the library exists. */
  let driveFolder = $state('');
  // Whether a cloud drive syncs the chosen folder is the service's call (real sync folders, shortcuts resolved).
  const cloud = cloudFor(() => String(s.libraryPath ?? ''));
  const inCloud = $derived(cloud.current.synced);
  const cloudName = $derived(cloud.current.service || t('hista.cloud.someDrive'));
  const fullPath = (p: string) => /^(~\/|\/|[A-Za-z]:[\\/])/.test(p);
  const typedPath = $derived(String(s.libraryPath ?? '').trim());
  /** missing · empty · starter (a setup that stopped half-way) · library · files · notFolder — '' while not known. */
  const here = $derived(folder && folder.path === typedPath ? folder.state : '');
  const whatIsThere = (path: string) => post('/onboarding/folder', { path }).then((r) => String(r?.state ?? ''));
  let probeSeq = 0;
  $effect(() => {
    const p = typedPath;
    if (step !== 2) return;
    const seq = ++probeSeq;
    if (!p || !fullPath(p)) { probe = 'idle'; return; }
    probe = 'busy';
    // A short pause, so a path being typed is not looked up letter by letter.
    const timer = setTimeout(async () => {
      try { const state = await whatIsThere(p); if (seq === probeSeq) { folder = { path: p, state }; probe = 'idle'; } }
      catch { if (seq === probeSeq) probe = 'failed'; /* Finish checks the folder again */ }
    }, 300);
    return () => clearTimeout(timer);
  });
  // What adding Thoughts' files to the chosen folder of notes would add (asked when that is the choice).
  const adoptFor = $derived(libraryMode === 'existing' && here === 'files' ? typedPath : '');
  // (Which folder was asked about is kept outside the reactive state, so an answer never asks again.)
  let adoptAsked = '';
  $effect(() => {
    const p = adoptFor;
    if (!p || adoptChoice !== 'adopt') { adoptAsked = ''; return; }
    if (adoptAsked === p) return;
    adoptAsked = p;
    adoptPlan = { path: p };
    adoptYes = false;
    post('/onboarding/adopt', { path: p })
      .then((v) => { if (adoptAsked === p) adoptPlan = { path: p, v }; })
      .catch((e: any) => { if (adoptAsked === p) adoptPlan = { path: p, err: e?.message || t('sb.adopt.failed') }; });
  });
  // What a copy of the chosen folder of notes would bring into the new library.
  let bringAsked = '';
  $effect(() => {
    const p = importFrom.trim();
    if (!p || libraryMode !== 'new') { bringAsked = ''; bring = null; return; }
    if (bringAsked === p) return;
    bringAsked = p;
    bring = { path: p };
    post('/library/import-folder/preview', { path: p })
      .then((v) => { if (bringAsked === p) bring = { path: p, v }; })
      .catch((e: any) => { if (bringAsked === p) bring = { path: p, err: e?.message || t('sb.imp.previewFailed') }; });
  });
  const adopting = $derived(!!adoptFor && adoptChoice === 'adopt');
  const bringing = $derived(libraryMode === 'new' && !!importFrom.trim());
  // Which assistant program the app runs on (Settings › Assistant's choice, set here the same way):
  // the library's setup, Wrap and new chats use it.
  const onCodex = $derived(s.ai?.primary === 'codex');
  const codex = $derived<any>(app.status?.codex ?? {});
  function setProgram(p: 'claude' | 'codex') { s.ai.primary = p; }
  // Codex has no live check of its own: while it is chosen and not ready, look every few seconds
  // (only while the window is in front), so installing or signing in shows up by itself.
  $effect(() => {
    if (step !== 1 || !onCodex || isReady(codex, 'codex')) return;
    const timer = setInterval(() => {
      if (document.visibilityState !== 'visible') return;
      post('/ai/codex/recheck').then((r) => { if (r && typeof r === 'object') app.status = { ...app.status, codex: r }; }).catch(() => {});
    }, 5000);
    return () => clearInterval(timer);
  });
  /** The assistant is not connected and the user chose to connect it later. */
  const assistantReady = $derived(onCodex ? isReady(codex, 'codex') : !!claude.installed && (!!claude.loggedIn || useApi));
  const waitsForAssistant = $derived(later && !assistantReady);
  // Signing in with a Claude or ChatGPT subscription needs the user's explicit acknowledgement of the
  // terms risk (stored as `subscriptionRiskAck`, an ISO time, in settings). An API key needs none.
  const onSubscription = $derived(!later && (onCodex || !useApi));
  function ackRisk(on: boolean) { s.subscriptionRiskAck = on ? new Date().toISOString() : null; }

  async function pickNotes() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const p = await open({ directory: true });
      if (typeof p === 'string') importFrom = p;
    } catch { toast(t('sb.bring.needPick'), { kind: 'error' }); }
  }
  /** Keep the folder of notes as it is: a new library, with a copy of those notes brought in. */
  function copyInstead() {
    const from = typedPath;
    adoptChoice = 'copy';
    libraryMode = 'new';
    pathTouched = true;
    importFrom = from;
    const home = app.status?.home ? String(app.status.home) : '~';
    const fresh = home + '/Thoughts';
    s.libraryPath = from.replace(/[\\/]+$/, '') === fresh ? fresh + ' library' : fresh;
    toast(t('sb.adopt.switched'));
  }

  const validUrl = (u: string) => /^https?:\/\/\S+$/.test((u ?? '').trim());
  const validRemote = (u: string) => /^(https?:\/\/|ssh:\/\/|git@)\S+$/.test(u.trim());

  onMount(loadModules);

  async function loadModules() {
    modulesLoading = true; modulesErr = '';
    try {
      modules = await api('/modules');
      s.modules = Object.fromEntries(modules.map((m) => [m.id, s.modules?.[m.id] ?? m.defaultOn]));
    } catch (e: any) { modulesErr = t('ob3.modulesFailed'); }
    finally { modulesLoading = false; }
  }

  // Picking a language on the first step switches the whole flow (text and direction) immediately.
  $effect(() => { app.settings.language = s.language; applyTheme(); });

  async function act(key: string, fn: () => Promise<void>, failKey = 'ob.failed') {
    if (working[key]) return;
    working[key] = true;
    try { await fn(); } catch (e) { failed(t(failKey), e); }
    finally { working[key] = false; }
  }

  async function checkAgain() {
    if (checking) return;
    checking = true;
    try {
      const st = await api('/status');
      claude = st.claude ?? {};
      git = st.git ?? git;
      if (st.codex) app.status = { ...app.status, codex: st.codex };
      const ready = step === 1 ? assistantReady : git.installed;
      if (step !== 1 && git.installed && gitAsked) gitSaid = t('ux-onboarding.git.nowInstalled');
      toast(t(ready ? 'ob.checkOk' : 'ob.checkStill'));
    } catch (e) { failed(t('ob3.checkFailed'), e); }
    finally { checking = false; }
  }

  const isMac = app.status?.platform === 'macos';
  // After "Install Git", the card looks by itself every few seconds until Git is there (Apple's
  // installer takes minutes), then says so and goes away; "Check again" still works meanwhile.
  let gitAsked = $state(false);
  let gitSaid = $state('');
  $effect(() => {
    if (!gitAsked || git.installed || step !== 2) return;
    const timer = setInterval(async () => {
      if (document.visibilityState !== 'visible' || checking) return;
      try { const st = await api('/status'); if (st?.git) git = st.git; if (git.installed) gitSaid = t('ux-onboarding.git.nowInstalled'); } catch { /* the next look tries again */ }
    }, 10_000);
    return () => clearInterval(timer);
  });
  const installGit = () => act('git', async () => {
    const r = await post('/git/install');
    gitAsked = true;
    // Windows: the service hands back the download page; Linux: the command for this system.
    if (r?.url) { try { const { openUrl } = await import('@tauri-apps/plugin-opener'); await openUrl(r.url); } catch { window.open(r.url, '_blank', 'noopener'); } }
    if (r?.hint) toast(t('ob.gitInstallToast'), { detail: String(r.hint), duration: 0 }); else toast(t('ob.gitInstallToast'));
  }, 'ob3.gitInstallFailed');

  async function browse() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const p = await open({ directory: true });
      if (typeof p !== 'string') return;
      pathTouched = true;
      if (libraryMode !== 'new') { s.libraryPath = p; return; }
      // A new library goes straight into an empty folder, and into a "Thoughts" folder inside one that
      // holds other things. A folder that already holds a library is opened, not started over.
      const picked = p.replace(/[\\/]+$/, '') || p;
      const state = await whatIsThere(picked).catch(() => '');
      if (state === 'library') { libraryMode = 'existing'; s.libraryPath = picked; toast(t('sa.folder.pickedLibrary')); }
      else s.libraryPath = state === 'empty' || state === 'starter' ? picked : picked + '/Thoughts';
    } catch { toast(t('ob.browseFail'), { kind: 'error' }); }
  }

  function setUseApi(on: boolean) {
    useApi = on;
    if (on && !s.ai.claude.baseUrl) s.ai.claude.baseUrl = 'https://api.anthropic.com';
  }

  /** Backup is one of: no backup, a cloud folder, an online location or a drive or folder (sent when Sync is pressed, or after every Wrap). */
  function setBackup(kind: 'none' | 'folder' | 'online' | 'drive') {
    const sends = kind === 'online' || kind === 'drive';
    s.sync.mode = sends ? (isGit ? s.sync.mode : 'git-manual') : kind;
    if (sends) { s.sync.target = kind === 'drive' ? 'drive' : 'online'; s.keepHistory = true; }
  }
  async function pickDrive() {
    let p: string | null = null;
    if ((window as any).__TAURI_INTERNALS__) {
      try { const { open } = await import('@tauri-apps/plugin-dialog'); const r = await open({ directory: true }); p = typeof r === 'string' ? r : null; }
      catch { p = null; }
    } else {
      p = await promptDialog({ title: t('histb.drive.typeTitle'), body: t('histb.drive.typeBody'), value: driveFolder });
    }
    if (p && p.trim()) driveFolder = p.trim();
  }
  // Backup and history wait in a closed section; coming back to the step with a choice already made
  // there (and maybe something still to finish, like the online location's confirmation) shows it open.
  let backupOpen = $state(false);
  $effect(() => { if (step === 2) untrack(() => { if (s.sync.mode !== 'none' || s.keepHistory === false) backupOpen = true; }); });
  // An online location is sent the history itself, so there the history is always kept.
  const historyOn = $derived(isGit || s.keepHistory !== false);

  /** Why the Next button is disabled on the current step ('' = good to go). */
  const blocker = $derived.by(() => {
    if (step === 1 && onSubscription && !s.subscriptionRiskAck) return t('subRisk.needAck');
    if (step === 1 && !later && onCodex) {
      if (!codex.installed) return t('ux-onboarding.needCodex');
      if (!isReady(codex, 'codex')) return t('ux-onboarding.needCodexSignIn');
    } else if (step === 1 && !later) {
      if (!claude.installed) return t('ob.needClaude');
      if (useApi) {
        if (!validUrl(s.ai.claude.baseUrl)) return t('ob.needApiUrl');
        if (!apiToken.trim()) return t('ob.needApiKey');
      } else if (!claude.loggedIn) return t('ob.needSignIn');
    }
    if (step === 2) {
      const p = String(s.libraryPath ?? '').trim();
      if (!p) return t('ob.needFolder');
      if (listedHere) return t('libs.already', { name: listedHere.displayName });
      if (!fullPath(p)) return t('ob.needFullPath');
      if (probe === 'busy') return t('sa.folder.checking');
      if (libraryMode === 'new') {
        if (here === 'library') return t('sa.folder.needOther');
        if (here === 'files') return t('sa.folder.files');
        if (here === 'notFolder') return t('sa.folder.notFolder');
      } else {
        if (here === 'missing') return t('sa.folder.missing');
        if (here === 'starter') return t('sa.folder.needSetUp');
        if (here === 'files') {
          if (adoptChoice !== 'adopt') return t('sb.adopt.needChoice');
          if (!adoptPlan?.v) return adoptPlan?.err || t('sb.adopt.looking');
          if (!adoptYes) return t('sb.adopt.needYes');
        }
        if (here === 'empty' || here === 'notFolder') return t('sa.folder.notLibrary');
      }
      if (bringing) {
        if (!bring?.v && !bring?.err) return t('sb.bring.busy');
        if (bring?.err || bring?.v?.tooBig || !bring?.v?.files) return t('sb.bring.bad');
      }
    }
    if (step === 2) {
      // Git is only a must for an online location; without it the app still works, minus the history.
      if (isGit && !git.installed) return t(isDrive ? 'histb.drive.needGit' : 'hista.online.needGit');
      if (isGit && !isDrive && inCloud && !s.sync.allowCloud) return t('hista.online.cloudClash');
      if (isGit && !isDrive && remoteUrl.trim()) {
        if (!validRemote(remoteUrl)) return t('ob.needRemoteUrl');
        if (!remoteAck) return t('ob.needAck');
      }
    }
    return '';
  });

  // Screen readers: what the folder holds is said in one short line (not the cards below it, with
  // their buttons), once the typing has paused; a card with choices that appears takes the focus to
  // its first control — unless the user is typing the folder, who is never moved out of the field.
  const cardKey = $derived(listedHere ? 'listed' : libraryMode === 'new' ? (here === 'library' ? 'library' : '') : here === 'starter' || here === 'files' ? here : '');
  const folderSummary = $derived.by(() => {
    const more = cardKey ? ' ' + t('ux-onboarding.choicesBelow') : '';
    if (listedHere) return t('libs.already', { name: listedHere.displayName }) + more;
    if (!here) return '';
    const key = libraryMode === 'new'
      ? ({ missing: 'sa.folder.new', empty: 'sa.folder.empty', starter: 'sa.folder.starter', library: 'sa.folder.library', files: 'sa.folder.files', notFolder: 'sa.folder.notFolder' } as Record<string, string>)[here]
      : ({ library: 'sa.folder.found', starter: 'sa.folder.unfinished', missing: 'sa.folder.missing', files: 'sb.adopt.title' } as Record<string, string>)[here] ?? 'sa.folder.notLibrary';
    return key ? t(key) + more : '';
  });
  let folderSaid = $state('');
  $effect(() => {
    const v = folderSummary;
    if (!v) return; // keep the last line: nothing new to say while a path is being typed
    const timer = setTimeout(() => (folderSaid = v), 800);
    return () => clearTimeout(timer);
  });
  let pathEl = $state<HTMLInputElement | null>(null);
  let saysEl = $state<HTMLElement | null>(null);
  let lastCard = '';
  let cardOnStep = false;
  $effect(() => {
    const key = cardKey;
    if (step !== 2) { cardOnStep = false; lastCard = ''; return; }
    // Arriving on the step: the heading takes the focus, not a card that is already there.
    if (!cardOnStep) { cardOnStep = true; lastCard = key; return; }
    if (key === lastCard) return;
    lastCard = key;
    if (!key) return;
    tick().then(() => {
      if (document.activeElement === pathEl) return;
      saysEl?.querySelector<HTMLElement>('button:not([disabled]), input:not([disabled])')?.focus();
    });
  });

  const modName = (m: any) => (ar && m.nameAr) || m.name;
  const modDesc = (m: any) => (ar && m.descriptionAr) || m.description;
  // Why Next is greyed out, said aloud only once it has settled — never "Checking…" at every key
  // pressed — and not when the folder line has just said the same.
  const transient = ['sa.folder.checking', 'sb.bring.busy', 'sb.adopt.looking'];
  let whySaid = $state('');
  $effect(() => {
    const v = step < steps.length - 1 ? blocker : '';
    if (v && transient.some((k) => t(k) === v)) return;
    const timer = setTimeout(() => (whySaid = v && v !== folderSummary ? v : ''), 1000);
    return () => clearTimeout(timer);
  });
  const syncLabel = $derived(t(isDrive ? 'histb.backup.drive' : isGit ? 'hista.backup.online' : s.sync.mode === 'folder' ? 'hista.backup.folder' : 'hista.backup.none'));

  async function finish() {
    if (busy) return;
    busy = true;
    try {
      const settings = structuredClone($state.snapshot(s));
      // A key for another service belongs to Claude Code only: with Codex chosen, none is sent.
      const keyed = useApi && !onCodex;
      if (!keyed) settings.ai.claude.baseUrl = null;
      if (settings.ai?.claude && !['acceptEdits', 'plan'].includes(settings.ai.claude.permissionMode ?? 'acceptEdits')) settings.ai.claude.permissionMode = 'plan';
      // The app keeps the choice itself (its id, or the user's own few words): every screen reads its
      // wording from that. The name of the choice in the user's language is for the assistant only.
      const own = profession === 'other' ? professionOther.trim() : '';
      settings.profession = own || profession || null;
      const r = await post('/onboarding', {
        settings, libraryMode, seeds: seeds.split('\n').map((x) => x.trim()).filter(Boolean),
        remoteUrl: isGit && !isDrive ? remoteUrl.trim() : '', remoteUsername: isGit && !isDrive ? remoteUser : '', remoteToken: isGit && !isDrive ? remoteToken : '',
        apiToken: keyed ? apiToken.trim() : '',
        profession: own || (profession ? t('layer.prof.' + profession) : ''),
        assistantLater: waitsForAssistant,
        adopt: adopting && adoptYes,
        importFrom: bringing ? importFrom.trim() : '',
        addLibrary: adding, libraryName: adding ? libName.trim() : '',
      });
      try { app.settings = await api('/settings'); applyTheme(); } catch { /* the app reloads settings on its own */ }
      // The library exists now: the chosen drive becomes its backup location (the first sync, at
      // the end of setup, sends the library there). A drive that cannot be used is said plainly,
      // and can be chosen again in Settings.
      if (isDrive && driveFolder) {
        try { await post('/git/remotes', { name: driveName(driveFolder, new Set()), folder: driveFolder }); }
        catch (e) { failed(t('histb.drive.failed'), e); }
      }
      // A setup that waits for the assistant is offered again once one is connected; start at Home.
      done(r.waiting ? null : r.setupChatId, libName.trim());
    } catch (e) {
      failed(t('ob3.finishFailed'), e);
    } finally { busy = false; }
  }

  // Enter in a text box moves on, like the Next button (never from a checkbox, a list or the notes box).
  function onKey(e: KeyboardEvent) {
    if (adding && cancel && e.key === 'Escape' && !busy && !e.defaultPrevented) { e.preventDefault(); cancel(); return; }
    if (e.key !== 'Enter' || imeKey(e) || e.shiftKey || e.metaKey || e.ctrlKey || e.altKey || e.defaultPrevented) return;
    const el = e.target;
    if (!(el instanceof HTMLInputElement) || !['text', 'password', 'url', 'email', 'search', ''].includes(el.type)) return;
    if (step >= steps.length - 1 || blocker || busy) return;
    e.preventDefault();
    step++;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="ob" onkeydown={onKey}>
  <div class="drag" data-tauri-drag-region></div>
  <p class="prog" role="status">{t('ob3.stepOf', { n: step - first + 1, total: steps.length - first })} · <b>{t('ob.step.' + steps[step])}</b></p>
  <ol class="steps" aria-label={t('ob.stepsLabel')}>
    {#each steps.slice(first) as name, j}
      {@const i = j + first}
      <li class="st" class:on={i === step} class:past={i < step} aria-current={i === step ? 'step' : undefined}>{t('ob.step.' + name)}</li>
    {/each}
  </ol>

  <div class="card body" bind:this={bodyEl}>
    {#if step === 0}
      <div class="hero-logo"><Logo size={72} /></div>
      <h1 class="brand" tabindex="-1">Thoughts</h1>
      <p>{t('ob.welcomeBody')}</p>
      <div class="lbl" id="ob-lang">{t('language')}</div>
      <div class="row" role="group" aria-labelledby="ob-lang">
        <button type="button" class="btn" class:primary={s.language === 'ar'} aria-pressed={s.language === 'ar'} lang="ar" onclick={() => setLang('ar')}>العربية</button>
        <button type="button" class="btn" class:primary={s.language !== 'ar'} aria-pressed={s.language !== 'ar'} lang="en" onclick={() => setLang('en')}>English</button>
      </div>
      <label for="ob-name">{t('ob.nameLabel')}</label>
      <input id="ob-name" dir="auto" bind:value={s.userName} placeholder={t('ob.namePh')} autocomplete="off" />
      <label for="ob-prof">{t('layer.prof.label')}</label>
      <select id="ob-prof" bind:value={profession}>
        <option value="">{t('layer.prof.choose')}</option>
        {#each professions as p}<option value={p}>{t('layer.prof.' + p)}</option>{/each}
      </select>
      {#if profession === 'other'}
        <label for="ob-prof-other">{t('sa.prof.otherLabel')}</label>
        <input id="ob-prof-other" dir="auto" bind:value={professionOther} placeholder={t('sa.prof.otherPh')} maxlength="160" autocomplete="off" />
      {/if}
      <p class="muted sm">{t('layer.prof.hint')}</p>
    {:else if step === 1}
      <h1 tabindex="-1">{t('ob.aiTitle')}</h1>
      <p>{onCodex ? t('ux-onboarding.ai.bodyCodex') : t('ob.aiBody')}</p>
      <div class="lbl" id="ob-prog">{t('ux-onboarding.ai.which')}
        <HelpTip>
          {#if onCodex}
            <b>{t('ux-onboarding.codex.planTitle')}</b>
            <span>{t('ux-onboarding.codex.planPaid')}</span>
            <span>{t('ux-onboarding.codex.planData')}</span>
            {#if !codex.installed}<span>{t('ux-onboarding.codex.customise')}</span>{/if}
            <span>{t('ux-onboarding.codex.change')}</span>
          {:else}
            <b>{t('ob3.planTitle')}</b>
            <span>{t('ob3.planPaid')}</span>
            <span>{t('ob3.planNoBill')}</span>
            <span>{t('ob3.planData')}</span>
          {/if}
        </HelpTip>
      </div>
      <div class="opts" role="group" aria-labelledby="ob-prog">
        <button type="button" class="opt" class:sel={!onCodex} aria-pressed={!onCodex} onclick={() => setProgram('claude')}><b><bdi>Claude</bdi></b><span class="muted">{t('ux-onboarding.ai.claudeHint')}</span></button>
        <button type="button" class="opt" class:sel={onCodex} aria-pressed={onCodex} onclick={() => setProgram('codex')}><b><bdi>Codex</bdi></b><span class="muted">{t('ux-onboarding.ai.codexHint')}</span></button>
      </div>
      {#if onSubscription}
        <div class="card status risk" role="group" aria-labelledby="ob-risk">
          <p class="row"><Icon name="warning" size={16} /> <b id="ob-risk">{t('subRisk.title')}</b></p>
          <p>{t('subRisk.body')}</p>
          <label class="check"><input type="checkbox" checked={!!s.subscriptionRiskAck} onchange={(e) => ackRisk(e.currentTarget.checked)} /> {t('subRisk.ack')}</label>
        </div>
      {/if}
      {#if onCodex}
        <CodexSettings bind:ai={s.ai} part="codex" vis={() => true} />
      {:else}
        <h2>{t('sb.guide.title')}</h2>
        <AssistantGuide bind:claude keyInstead={useApi} />

        <details class="adv" open={useApi}>
          <summary>{t('ob.advApi')}</summary>
          <label class="check"><input type="checkbox" checked={useApi} onchange={(e) => setUseApi(e.currentTarget.checked)} /> {t('ob.useApi')}</label>
          {#if useApi}
            <label for="ob-base">{t('ob.apiUrl')}</label>
            <input id="ob-base" dir="ltr" bind:value={s.ai.claude.baseUrl} placeholder="https://api.anthropic.com" autocomplete="off" />
            <label for="ob-key">{t('ob.apiKey')} <HelpTip text={t('ob3.apiKeyHint')} /></label>
            <input id="ob-key" dir="ltr" type="password" bind:value={apiToken} autocomplete="off" />
          {/if}
        </details>
      {/if}

      {#if !assistantReady}
        <div class="card status later" class:sel={later}>
          <p class="row"><Icon name="eye" size={16} /> <b>{t('sb.later.title')}</b></p>
          <p class="muted">{t('sb.later.body')}</p>
          {#if later}<p class="okline" role="status"><Icon name="check" size={15} /> <span>{t('sb.later.chosen')}</span></p>
          {:else}<div class="row gap-top"><button type="button" class="btn" onclick={() => { later = true; step++; }}>{t('sb.later.button')}</button></div>{/if}
        </div>
      {/if}
    {:else if step === 2}
      {#if adding}
        <h1 tabindex="-1">{t('libs.addTitle')}</h1>
        <p>{t('libs.addBody')}</p>
        <label for="ob-libname">{t('libs.nameLabel')}</label>
        <input id="ob-libname" dir="auto" bind:value={libName} placeholder={t('libs.namePh')} maxlength="60" autocomplete="off" />
        <label for="ob-prof2">{t('layer.prof.label')}</label>
        <select id="ob-prof2" bind:value={profession}>
          <option value="">{t('layer.prof.choose')}</option>
          {#each professions as p}<option value={p}>{t('layer.prof.' + p)}</option>{/each}
        </select>
        {#if profession === 'other'}
          <label for="ob-prof2-other">{t('sa.prof.otherLabel')}</label>
          <input id="ob-prof2-other" dir="auto" bind:value={professionOther} placeholder={t('sa.prof.otherPh')} maxlength="160" autocomplete="off" />
        {/if}
        <h2>{t('ob.libTitle')}</h2>
      {:else}
        <h1 tabindex="-1">{t('ob.libTitle')}</h1>
      {/if}
      <p>{t('ob.libBody')}</p>
      <div class="opts">
        <button type="button" class="opt" class:sel={libraryMode === 'new'} aria-pressed={libraryMode === 'new'} onclick={() => (libraryMode = 'new')}><b>{t('ob.libNew')}</b><span class="muted">{t('ob.libNewHint')}</span></button>
        <button type="button" class="opt" class:sel={libraryMode === 'existing'} aria-pressed={libraryMode === 'existing'} onclick={() => (libraryMode = 'existing')}><b>{t('ob.libExisting')}</b><span class="muted">{t('ob.libExistingHint')}</span></button>
      </div>
      <label for="ob-path">{t('ob.folder')}</label>
      <div class="row"><input id="ob-path" class="grow" dir="ltr" bind:this={pathEl} bind:value={s.libraryPath} oninput={() => (pathTouched = true)} autocomplete="off" aria-describedby="ob-path-sum" /><button type="button" class="btn" onclick={browse}><Icon name="folder" size={14} /> {t('ob.browse')}</button></div>
      <p class="sr-only" id="ob-path-sum" role="status">{folderSaid}</p>
      <p class="sr-only" role="status">{gitSaid}</p>
      <div id="ob-path-says" bind:this={saysEl}>
        {#if listedHere}
          <div class="card status">
            <p class="row"><Icon name="info" size={16} /> <b>{t('libs.already', { name: listedHere.displayName })}</b></p>
            <div class="row gap-top"><button type="button" class="btn primary" onclick={openListed}>{t('libs.openIt')}</button></div>
          </div>
        {:else if libraryMode === 'new'}
          {#if here === 'missing'}<p class="muted sm">{t('sa.folder.new')}</p>
          {:else if here === 'empty'}<p class="muted sm">{t('sa.folder.empty')}</p>
          {:else if here === 'starter'}<p class="okline"><Icon name="check" size={15} /> <span>{t('sa.folder.starter')}</span></p>
          {:else if here === 'library'}
            <div class="card status">
              <p class="row"><Icon name="info" size={16} /> <b>{t('sa.folder.library')}</b></p>
              <p class="muted">{t('sa.folder.libraryHint')}</p>
              <div class="row gap-top"><button type="button" class="btn primary" onclick={() => (libraryMode = 'existing')}>{t('sa.folder.open')}</button></div>
            </div>
          {:else if here === 'files'}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sa.folder.files')}</span></p>
          {:else if here === 'notFolder'}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sa.folder.notFolder')}</span></p>
          {/if}
        {:else if here === 'library'}<p class="okline"><Icon name="check" size={15} /> <span>{t('sa.folder.found')}</span></p>
        {:else if here === 'starter'}
          <div class="card status">
            <p class="row"><Icon name="info" size={16} /> <b>{t('sa.folder.unfinished')}</b></p>
            <p class="muted">{t('sa.folder.unfinishedHint')}</p>
            <div class="row gap-top"><button type="button" class="btn primary" onclick={() => (libraryMode = 'new')}>{t('sa.folder.setUp')}</button></div>
          </div>
        {:else if here === 'missing'}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sa.folder.missing')}</span></p>
        {:else if here === 'files'}
          <div class="card status">
            <p class="row"><Icon name="info" size={16} /> <b>{t('sb.adopt.title')}</b></p>
            <div class="opts" role="group" aria-label={t('sb.adopt.title')}>
              <button type="button" class="opt" class:sel={adoptChoice === 'adopt'} aria-pressed={adoptChoice === 'adopt'} onclick={() => (adoptChoice = 'adopt')}><b>{t('sb.adopt.useTitle')}</b><span class="muted">{t('sb.adopt.useHint')}</span></button>
              <button type="button" class="opt" aria-pressed="false" onclick={copyInstead}><b>{t('sb.adopt.copyTitle')}</b><span class="muted">{t('sb.adopt.copyHint')}</span></button>
            </div>
            {#if adoptChoice === 'adopt'}
              {#if adoptPlan?.err}<p class="warn" role="alert"><Icon name="warning" size={15} /> <span>{adoptPlan.err}</span></p>
              {:else if !adoptPlan?.v}<p class="muted sm">{t('sb.adopt.looking')}</p>
              {:else}
                <p>{t('sb.adopt.what')}</p>
                <ul class="plain">
                  {#each adoptPlan.v.adds ?? [] as a}
                    <li>{t('sb.adopt.' + a.what, { n: a.files ?? 0 })} <code class="muted sm"><bdi>{a.path}</bdi></code></li>
                  {/each}
                </ul>
                <p class="muted sm">{t('sb.adopt.counts', { notes: adoptPlan.v.notes ?? 0, total: adoptPlan.v.total ?? 0, size: fmtSize(Number(adoptPlan.v.bytes) || 0) })}</p>
                {#if !adoptPlan.v.notes}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sb.adopt.noNotes')}</span></p>{/if}
                <p class="muted sm">{t(historyOn ? 'sb.adopt.unchanged' : 'sb.adopt.unchangedNoHistory')}</p>
                <label class="check"><input type="checkbox" bind:checked={adoptYes} /> {t('sb.adopt.yes')}</label>
              {/if}
            {/if}
          </div>
        {:else if here}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sa.folder.notLibrary')}</span></p>
        {/if}
      </div>
      {#if libraryMode === 'new'}
        <label for="ob-seeds">{t('ob.seeds')}</label>
        <textarea id="ob-seeds" rows="2" dir="auto" bind:value={seeds} placeholder={t('ob.seedsPh')}></textarea>

        <h2 id="ob-bring">{t('sb.bring.label')}</h2>
        <p class="muted sm">{t('sb.bring.hint')}</p>
        {#if bringing}
          <div class="card status" aria-labelledby="ob-bring">
            <p class="row"><Icon name="folder" size={16} /> <b class="grow path">{t('sb.bring.from', { path: importFrom.trim() })}</b></p>
            {#if bring?.err}<p class="warn" role="alert"><Icon name="warning" size={15} /> <span>{bring.err}</span></p>
            {:else if !bring?.v}<p class="muted sm" role="status">{t('sb.imp.looking')}</p>
            {:else if bring.v.tooBig}<p class="warn" role="alert"><Icon name="warning" size={15} /> <span>{t('sb.imp.tooBig')}</span></p>
            {:else if !bring.v.files}<p class="warn" role="alert"><Icon name="warning" size={15} /> <span>{t('sb.imp.empty')}</span></p>
            {:else}
              <p class="muted sm">{t('sb.imp.counts', { notes: bring.v.notes ?? 0, other: bring.v.other ?? 0, size: fmtSize(Number(bring.v.bytes) || 0) })}</p>
              {#if bring.v.leftCount}<p class="muted sm">{t('sb.imp.leftTitle')} {Object.entries(bring.v.leftBy ?? {}).filter(([, n]) => Number(n) > 0).map(([w, n]) => t('sb.imp.left.' + w, { n: Number(n) })).join(' · ')}</p>{/if}
              <p class="muted sm">{t(historyOn ? 'sb.imp.undo' : 'sb.imp.noUndo')}</p>
            {/if}
            <div class="row gap-top">
              <button type="button" class="btn" onclick={pickNotes}><Icon name="folder" size={14} /> {t('sb.bring.change')}</button>
              <button type="button" class="btn ghost" onclick={() => (importFrom = '')}>{t('sb.bring.remove')}</button>
            </div>
          </div>
        {:else}
          <div class="row"><button type="button" class="btn" onclick={pickNotes}><Icon name="upload" size={14} /> {t('sb.bring.pick')}</button></div>
        {/if}
      {/if}

      <!-- Backup and the history start with safe choices (no backup, history on): they wait, closed, until wanted. -->
      <details class="adv backup" bind:open={backupOpen}>
        <summary><span>{t('ux-onboarding.backup.section')}</span> <span class="muted cur">{syncLabel} · {t('hista.historyTitle')}: {t(historyOn ? 'hista.on' : 'hista.off')}{#if historyOn && !git.installed} · {t('hista.sumNeedsGit')}{/if}</span></summary>
        <h3 id="ob-history">{t('hista.historyTitle')}</h3>
        <label class="check"><input type="checkbox" checked={historyOn} disabled={isGit} onchange={(e) => (s.keepHistory = e.currentTarget.checked)} /> {t('hista.keep')}</label>
        <p class="muted sm">{t(isGit ? 'hista.keepForced' : historyOn ? 'hista.keepHint' : 'hista.keepOffHint')}</p>
        {#if gitSaid && git.installed}<p class="okline"><Icon name="check" size={15} /> <span>{gitSaid}</span></p>{/if}
        {#if historyOn && !git.installed}
          <div class="card status" role="note">
            <p class="row"><Icon name="info" size={16} /> <b>{t('hista.needsGitTitle')}</b></p>
            <p class="muted">{t(isGit ? 'hista.needsGitOnline' : 'hista.needsGitBody')}</p>
            <div class="row gap-top">
              <button type="button" class="btn" class:primary={isGit} disabled={working.git} onclick={installGit}>{working.git ? t('ob.opening') : t('ob3.installGit')}</button>
              <button type="button" class="btn" disabled={checking} aria-busy={checking} onclick={checkAgain}><Icon name="sync" size={14} class={checking ? 'spin' : ''} /> {checking ? t('ob.checking') : t('ob.checkAgain')}</button>
            </div>
            {#if isMac}<p class="muted sm gap-top">{t(isGit ? 'ux-onboarding.git.macOnline' : 'ux-onboarding.git.mac')}</p>{/if}
          </div>
        {/if}

        <h3 id="ob-backup">{t('ob3.backupTitle')}</h3>
        <p class="muted">{t('hista.backupBody')}</p>
        <div class="opts four" role="group" aria-labelledby="ob-backup">
          <button type="button" class="opt" class:sel={s.sync.mode === 'none'} aria-pressed={s.sync.mode === 'none'} onclick={() => setBackup('none')}><b>{t('hista.backup.none')}</b><span class="muted">{t('hista.backup.noneHint')}</span></button>
          <button type="button" class="opt" class:sel={s.sync.mode === 'folder'} aria-pressed={s.sync.mode === 'folder'} onclick={() => setBackup('folder')}><b>{t('hista.backup.folder')}</b><span class="muted">{t('hista.backup.folderHint')}</span></button>
          <button type="button" class="opt" class:sel={isGit && !isDrive} aria-pressed={isGit && !isDrive} onclick={() => setBackup('online')}><b>{t('hista.backup.online')}</b><span class="muted">{t('hista.backup.onlineHint')}</span></button>
          <button type="button" class="opt" class:sel={isDrive} aria-pressed={isDrive} onclick={() => setBackup('drive')}><b>{t('histb.backup.drive')}</b><span class="muted">{t('histb.backup.driveHint')}</span></button>
        </div>
        {#if s.sync.mode === 'none'}
          <p class="warn"><Icon name="warning" size={15} /> <span>{t('hista.backup.noneWarn')}</span></p>
        {:else if s.sync.mode === 'folder'}
          <div class="card status">
            <p>{t('ob.folderPick')}</p>
            <div class="row gap-top"><code class="grow path"><bdi>{s.libraryPath}</bdi></code><button type="button" class="btn" onclick={browse}><Icon name="folder" size={14} /> {t('ob.chooseFolder')}</button></div>
            {#if inCloud}<p class="okline"><Icon name="check" size={15} /> <span>{t('hista.cloud.inside', { service: cloudName })}</span></p>
            {:else}<p class="warn"><Icon name="warning" size={15} /> <span>{t('ob.folderNotCloud')}</span></p>{/if}
            <p class="muted sm">{t('ob.cloudPrivacy')}</p>
          </div>
        {:else if isDrive}
          <div class="card status">
            <p class="muted sm">{t('histb.drive.hint')}</p>
            <div class="row gap-top"><code class="grow path"><bdi>{driveFolder || '—'}</bdi></code><button type="button" class="btn" onclick={pickDrive}><Icon name="folder" size={14} /> {t('histb.drive.add')}</button></div>
            {#if !driveFolder}<p class="muted sm">{t('histb.backup.driveLater')}</p>{/if}
            <label for="ob-drivemode">{t('hista.online.when')}</label>
            <select id="ob-drivemode" bind:value={s.sync.mode}>
              <option value="git-manual">{t('hista.online.manual')}</option>
              <option value="git-auto">{t('hista.online.auto')}</option>
            </select>
          </div>
        {:else}
          <div class="card status">
            <p class="muted sm">{t('hista.online.body')}</p>
            {#if inCloud}
              <div class="warn box" role="note">
                <Icon name="warning" size={15} />
                <div>
                  <div>{t('hista.cloud.clash', { service: cloudName })}</div>
                  <label class="check"><input type="checkbox" bind:checked={s.sync.allowCloud} /> {t('hista.cloud.anyway')}</label>
                </div>
              </div>
            {/if}
            <label for="ob-gitmode">{t('hista.online.when')}</label>
            <select id="ob-gitmode" bind:value={s.sync.mode}>
              <option value="git-manual">{t('hista.online.manual')}</option>
              <option value="git-auto">{t('hista.online.auto')}</option>
            </select>
            <p class="muted sm">{t('ob.wrapDef')}</p>
            <label for="ob-remote">{t('hista.online.address')}</label>
            <input id="ob-remote" dir="ltr" bind:value={remoteUrl} placeholder="https://github.com/you/thoughts.git" autocomplete="off" />
            {#if remoteUrl.trim()}
              <div class="warn box" role="note">
                <Icon name="warning" size={15} />
                <div>
                  <b>{t('ob.uploadTitle')}</b>
                  <div>{t('ob.uploadBody')}</div>
                  <label class="check"><input type="checkbox" bind:checked={remoteAck} /> {t('ob.uploadAck')}</label>
                </div>
              </div>
              <div class="row top">
                <div class="grow"><label for="ob-ruser">{t('ob.remoteUser')}</label><input id="ob-ruser" dir="ltr" bind:value={remoteUser} autocomplete="off" /></div>
                <div class="grow"><label for="ob-rtoken">{t('ob.remoteToken')}</label><input id="ob-rtoken" dir="ltr" type="password" bind:value={remoteToken} autocomplete="off" /></div>
              </div>
              <RemoteTokenHelp />
              <label for="ob-cred">{t('ob.credStore')}</label>
              <select id="ob-cred" bind:value={s.sync.credentialStore}><option value="keychain">{t('ob.credKeychain')}</option><option value="file">{t('ob.credFile')}</option></select>
              <p class="muted sm">{t(s.sync.credentialStore === 'file' ? 'ob3.credFileNote' : 'ob3.credKeychainNote')}</p>
            {/if}
          </div>
        {/if}
      </details>

      <details class="adv">
        <summary>{t('ob3.featuresAdv')}</summary>
        <p class="muted sm">{t('ob.modBody')}</p>
        {#if modulesLoading}
          <p class="muted" role="status">{t('ob.loading')}</p>
        {:else if modulesErr}
          <p class="warn" role="alert"><Icon name="warning" size={15} /> <span>{modulesErr}</span></p>
          <button type="button" class="btn" onclick={loadModules}>{t('ob3.retry')}</button>
        {:else}
          {#each modules as m (m.id)}
            <label class="mod" class:locked={m.id === 'ideas'}>
              <input type="checkbox" bind:checked={s.modules[m.id]} disabled={m.id === 'ideas'} />
              <span class="grow"><b>{modName(m)}</b><span class="muted sm blk">{modDesc(m)}</span></span>
              {#if m.id === 'ideas'}<span class="pill accent">{t('ob.alwaysOn')}</span>
              {:else if !s.modules[m.id]}<span class="pill">{t('addLater')}</span>{/if}
            </label>
          {/each}
        {/if}
      </details>
    {:else}
      <h1 tabindex="-1">{t('ob.readyTitle')}</h1>
      <div class="card status">
        <p class="row"><Icon name="shield" size={16} /> <b>{t('ob.privTitle')}</b></p>
        <ul class="plain">
          <li>{t(onCodex ? 'ux-onboarding.privSentCodex' : 'ob3.privSent')}</li>
          <li>{t('ob3.privStays')}</li>
          <li>{t(onCodex ? 'ux-onboarding.privFoldersCodex' : 'ob3.privFolders')}</li>
        </ul>
      </div>
      <dl class="sum">
        <dt>{t('ob.sumNotes')}</dt><dd><code><bdi>{s.libraryPath}</bdi></code> · {t(libraryMode === 'new' ? 'ob.sumNew' : 'ob.sumExisting')}{#if adopting}{' · '}{t('sb.adopt.sum')}{/if}</dd>
        {#if bringing}<dt>{t('sb.bring.sum')}</dt><dd><code><bdi>{importFrom.trim()}</bdi></code></dd>{/if}
        {#if !adding}<dt>{t('ob.sumAi')}</dt><dd><bdi>{onCodex ? 'Codex' : 'Claude'}</bdi>{#if useApi && !onCodex} · <bdi>{s.ai.claude.baseUrl}</bdi>{/if}{#if waitsForAssistant}{' · '}{t('sb.later.sum')}{/if}</dd>
        {:else if libName.trim()}<dt>{t('libs.nameLabel')}</dt><dd dir="auto">{libName.trim()}</dd>{/if}
        <dt>{t('ob.sumBackup')}</dt><dd>{syncLabel}{#if isDrive && driveFolder} · <bdi>{driveFolder}</bdi>{:else if isGit && !isDrive && remoteUrl.trim()} · <bdi>{remoteUrl.trim()}</bdi>{/if}{#if s.sync.mode === 'folder' && !inCloud} · {t('sa.sum.notCloud')}{/if}</dd>
        <dt>{t('hista.historyTitle')}</dt><dd>{t(historyOn ? 'hista.on' : 'hista.off')}{#if historyOn && !git.installed} · {t('hista.sumNeedsGit')}{/if}</dd>
      </dl>
      {#if !adding}
        <h2>{t('ob.lookTitle')}</h2>
        <ThemePicker bind:theme={s.theme} bind:mode={s.mode} />
        <details class="adv">
          <summary>{t('ob3.moreOptions')}</summary>
          <label class="check"><input type="checkbox" bind:checked={s.notifications} /> {t('ob.notify')}</label>
          <label class="check"><input type="checkbox" bind:checked={s.daemon.keepRunning} /> {t('ob.keepRunning')}</label>
          <label class="check"><input type="checkbox" bind:checked={s.daemon.runAtLogin} /> {t('ob.runAtLogin')}</label>
        </details>
      {/if}
      <p class="muted">{t('ob.wrapDef')}</p>
      {#if waitsForAssistant && (libraryMode === 'new' || adopting)}<p class="muted">{t(adopting ? 'sb.later.readyAdopt' : 'sb.later.readyNew')}</p>
      {:else if libraryMode === 'new'}<p class="muted">{t('ob.readyNew')}</p>{/if}
    {/if}
  </div>

  <div class="row nav">
    {#if step > first}<button type="button" class="btn" disabled={busy} onclick={() => step--}>{t('back')}</button>{/if}
    {#if adding && cancel}<button type="button" class="btn ghost" disabled={busy} onclick={cancel}>{t('libs.cancelAdd')}</button>{/if}
    <span class="grow why" id="ob-why">{#if blocker && step < steps.length - 1}{blocker}{/if}</span>
    <span class="sr-only" role="status">{whySaid}</span>
    {#if step < steps.length - 1}
      <button type="button" class="btn primary" disabled={!!blocker} aria-describedby="ob-why" onclick={() => step++}>{t('next')}</button>
    {:else}
      <button type="button" class="btn primary" disabled={busy} aria-busy={busy} onclick={finish}>{busy ? t('ob.finishing') : t('finish')}</button>
    {/if}
  </div>
</div>

<style>
  .ob { max-width: 760px; margin: 0 auto; padding: 0 24px 24px; min-height: 100vh; display: flex; flex-direction: column; }
  .drag { height: 34px; }
  .prog { font-size: 13px; color: var(--muted); margin: 0 0 4px; }
  .prog b { color: var(--fg); }
  .body h1:focus { outline: none; }
  .body h1:focus-visible { outline: 2px solid var(--accent); outline-offset: 4px; border-radius: 4px; }
  .check { margin: 6px 0; }
  .steps { list-style: none; counter-reset: st; display: flex; flex-wrap: wrap; gap: 4px 14px; font-size: var(--fs-xs); color: var(--muted); margin: 0 0 14px; padding: 0; }
  .st { counter-increment: st; }
  .st::before { content: counter(st) '. '; }
  .st.on { color: var(--accent); font-weight: 700; }
  .st.past { color: var(--fg); }
  .body { flex: 1; padding: 24px; }
  .brand { font-size: 36px; }
  .hero-logo { margin-bottom: 14px; filter: drop-shadow(0 10px 24px rgba(0,0,0,.25)); }
  .lbl { font-size: var(--fs-xs); font-weight: 500; color: var(--muted); margin: 10px 0 4px; }
  .opts { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-bottom: 12px; }
  .opts.four { grid-template-columns: 1fr 1fr; }
  @media (max-width: 640px) { .opts.four { grid-template-columns: 1fr; } }
  .opt { font: inherit; color: inherit; text-align: start; cursor: pointer; border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 12px; display: flex; flex-direction: column; gap: 4px; background: var(--bg); }
  .opt.sel { border-color: var(--accent); outline: 2px solid var(--accent); box-shadow: var(--glow); }
  .opt:focus-visible { outline: 3px solid color-mix(in srgb, var(--accent) 60%, transparent); outline-offset: 2px; }
  .status { background: var(--bg); margin-top: 10px; }
  .status p { margin: 0 0 6px; }
  .gap-top { margin-top: 10px; }
  .row.top { align-items: flex-start; }
  .check { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; }
  .mod { display: flex; gap: 10px; align-items: center; color: var(--fg); font-size: 14px; padding: 8px 0; border-bottom: var(--border-w) solid var(--border); margin: 0; cursor: pointer; }
  .mod.locked { opacity: .8; }
  .blk { display: block; font-weight: 400; }
  .adv { margin-top: 14px; border-top: 1px solid var(--hairline); padding-top: 4px; }
  .adv > summary { cursor: pointer; font-size: 13px; font-weight: 500; color: var(--muted); padding: 6px 0; }
  .adv.backup > summary { font-size: 14px; color: var(--fg); }
  .adv.backup > summary .cur { display: block; font-size: var(--fs-xs); font-weight: 400; margin-inline-start: 18px; }
  .adv h3 { font-size: 14px; font-weight: 600; margin: 14px 0 6px; }
  .warn, .okline { display: flex; gap: 8px; align-items: flex-start; font-size: 13px; margin: 8px 0; }
  .warn :global(.icon), .okline :global(.icon) { flex: none; margin-top: 3px; }
  .warn :global(.icon) { color: var(--danger); }
  .okline :global(.icon) { color: var(--ok); }
  .warn.box { border: var(--border-w) solid var(--danger); border-radius: var(--radius); padding: 10px 12px; background: color-mix(in srgb, var(--danger) 7%, transparent); }
  .path { overflow-wrap: anywhere; }
  .later.sel { border-color: var(--accent); }
  .risk { border-color: var(--danger); }
  .risk .row :global(svg) { color: var(--danger); }
  .plain { margin: 6px 0 0; padding-inline-start: 20px; }
  .plain li { margin-bottom: 4px; }
  .sum { display: grid; grid-template-columns: max-content 1fr; gap: 6px 14px; margin: 16px 0; }
  .sum dt { color: var(--muted); font-size: 13px; }
  .sum dd { margin: 0; overflow-wrap: anywhere; }
  .nav { padding-top: 14px; }
  .why { font-size: var(--fs-sm); color: var(--muted); text-align: end; }
  .sm { font-size: var(--fs-xs); }
  :global(.icon.spin) { animation: ob-spin 1s linear infinite; }
  @keyframes ob-spin { to { transform: rotate(360deg); } }
</style>
