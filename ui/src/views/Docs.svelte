<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import { api, apiBlob, inTauri, on, post, put } from '../lib/api';
  import { hasHiddenText, md, mdClick, resolveLink, revealInvisible } from '../lib/md';
  import { app, go, refreshChats, track } from '../lib/store.svelte';
  import { reportUnsaved } from '../lib/unsaved';
  import { t, tn, lang, fmtDate, fmtNum } from '../lib/i18n';
  import { choiceDialog, confirmDialog, promptDialog } from '../lib/dialog.svelte';
  import { errCode, errDetail, failed } from '../lib/errors';
  import { saveFile } from '../lib/download';
  import { noteLink } from '../lib/deeplink';
  import { MAX_FILE_BYTES, copyText, fmtSize, isImageName, notify, prettyName, rawBlobUrl, readAsDataURL } from '../lib/content/util';
  import Versions from './docs/Versions.svelte';
  import FolderPicker from './docs/FolderPicker.svelte';
  import Conflict from './docs/Conflict.svelte';
  import Editor from './docs/Editor.svelte';
  import FindBar from '../lib/FindBar.svelte';
  import SearchResults from '../lib/SearchResults.svelte';
  import { fold } from '../lib/CommandPalette.svelte';
  import Backlinks from './docs/Backlinks.svelte';
  import DocxPreview from './docs/DocxPreview.svelte';
  import { docDir, toDocx } from '../lib/content/docx';
  import ImportNotes from './ImportNotes.svelte';
  import PageExtras from './PageExtras.svelte';
  import { pageText, shows } from '../lib/pages.svelte';
  import { canOpenInApp, openLibraryFile, revealLabelKey, revealLibraryFile } from '../lib/openInApp';

  let { path }: { path?: string } = $props();

  let files = $state<any[]>([]);
  let loading = $state(true);
  let treeError = $state('');
  let current = $state<string>('');
  let doc = $state<any>(null);
  let docLoading = $state(false);
  let docError = $state('');
  let editing = $state(false);
  let draft = $state('');
  let saving = $state(false);
  let preview = $state(readPref());
  let results = $state<any[] | null>(null);
  let filter = $state('');
  let open = $state<Record<string, boolean>>(readOpen());
  let imgUrl = $state<string | null>(null);
  let pdfUrl = $state<string | null>(null);
  let pdfError = $state('');
  let showVersions = $state(false);
  let menu = $state(false);
  let menuBtn = $state<HTMLButtonElement>();
  let treeOpen = $state(false);
  let treeBtn = $state<HTMLButtonElement>();
  let treeEl = $state<HTMLElement>();
  /** Does the library keep a history (so a delete can be undone)? null = not asked yet. */
  let historyOn: boolean | null = null;
  let importing = $state(false);
  let dragging = $state(false);
  let dragDepth = 0;
  let canOpenExternal = $state(true);
  let article = $state<HTMLElement>();
  let picker = $state<HTMLInputElement>();
  // Import: single files (flat, as before), or a whole folder of notes with its folders kept (FEAT-09).
  let showImportNotes = $state(false);
  async function bringIn() {
    const c = await choiceDialog({ title: t('sb.imp.choose'), body: t('sb.imp.chooseBody'),
      choices: [{ id: 'files', label: t('sb.imp.files') }, { id: 'folder', label: t('sb.imp.folder'), primary: true }] });
    if (c === 'files') picker?.click(); else if (c === 'folder') showImportNotes = true;
  }
  async function broughtIn(into: string) { await loadTree(); setOpen(into, true); }
  /** The note's source instead of its formatted view (hidden text made visible). */
  let showSource = $state(false);
  /** The file as it is now on disk, when it changed elsewhere while being edited ({gone} when it disappeared). */
  let theirs = $state<any>(null);
  /** The "two versions" comparison, while it is open. */
  let conflict = $state<{ theirs: string; gone: boolean; resolve: (c: 'copy' | 'theirs' | 'mine' | null) => void } | null>(null);
  /** The Move folder picker, while it is open. */
  let moving = $state<{ path: string; folder: boolean } | null>(null);

  const TEXT_EXT = /\.(txt|csv|tsv|json|ya?ml|toml|log|xml|html?|css|js|ts|py|sh|ini|conf)$/i;
  /** Text files that can be edited here like notes (the rest of TEXT_EXT is shown read-only). */
  const EDIT_EXT = /\.(md|txt|csv|json)$/i;
  const isMd = (p: string) => /\.md$/i.test(p);
  const isPdf = (p: string) => /\.pdf$/i.test(p);
  const isDocx = (p: string) => /\.docx$/i.test(p);
  /** Bumped on every library change, so "Linked from" stays current. */
  let libVersion = $state(0);
  const isDir = (f: any) => f?.dir === true || f?.isDir === true || f?.type === 'dir' || f?.kind === 'dir' || String(f?.path ?? '').endsWith('/');
  const baseName = (p: string) => p.split('/').pop() || p;
  const dirName = (p: string) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '');
  const desktop = inTauri();
  const isMac = /mac/i.test(navigator.platform || navigator.userAgent);
  const dirty = $derived(editing && !!doc && draft !== doc.content);
  // The desktop app asks before quitting while this is set (the draft itself is kept: see the stash below).
  $effect(() => { reportUnsaved('docs', dirty && current ? baseName(current) : null, true); });
  const info = $derived(files.find((f) => f.path === current));

  function readOpen(): Record<string, boolean> { try { return JSON.parse(localStorage.getItem('thoughts.docs.open') || '{}') || {}; } catch { return {}; } }
  function setOpen(k: string, v: boolean) { open[k] = v; try { localStorage.setItem('thoughts.docs.open', JSON.stringify(open)); } catch { /* private mode */ } }
  function readPref() { try { return localStorage.getItem('thoughts.docs.preview') !== '0'; } catch { return true; } }
  function togglePreview() { preview = !preview; try { localStorage.setItem('thoughts.docs.preview', preview ? '1' : '0'); } catch { /* private mode */ } }

  // Safety net only: unsaved text is mirrored to localStorage, with the version it started from, so a
  // crash, a forced reload or quitting the app never loses it (the next open brings it back).
  // Leaving normally goes through `leaveOk` (Save / Discard / Keep editing), wired to `app.leaveGuard`.
  type Stash = { text: string; base?: string; at: number };
  const STASH = 'thoughts.docdraft:';
  const DAY = 86_400_000;
  const libKey = () => String(app.status?.library ?? '');
  const stashKey = (p: string) => `${STASH}${libKey()}\n${p}`;
  const oldKey = (p: string) => `thoughts.draft:${p}`; // 0.0.8 kept drafts in sessionStorage
  function stashGet(p: string): Stash | null {
    try {
      const s = JSON.parse(localStorage.getItem(stashKey(p)) || 'null');
      if (s && typeof s.text === 'string') return s;
    } catch { /* unreadable: none */ }
    try { const old = sessionStorage.getItem(oldKey(p)); if (old !== null) return { text: old, at: Date.now() }; } catch { /* ignore */ }
    return null;
  }
  function stashDrop(p: string) {
    try { localStorage.removeItem(stashKey(p)); } catch { /* ignore */ }
    try { sessionStorage.removeItem(oldKey(p)); } catch { /* ignore */ }
  }
  $effect(() => {
    if (!current || !editing) return;
    const d = draft, isDirty = dirty, base = doc?.hash;
    try {
      if (isDirty) localStorage.setItem(stashKey(current), JSON.stringify({ text: d, base, at: Date.now() } satisfies Stash));
      else stashDrop(current);
    } catch { /* storage full or unavailable */ }
  });
  /** Drafts of this library waiting in other files (older than 60 days are dropped). */
  function waitingDrafts(except: string): string[] {
    const out: string[] = [], prefix = `${STASH}${libKey()}\n`;
    try {
      for (let i = localStorage.length - 1; i >= 0; i--) {
        const k = localStorage.key(i);
        if (!k?.startsWith(STASH)) continue;
        let s: Stash | null = null;
        try { s = JSON.parse(localStorage.getItem(k) || 'null'); } catch { /* broken */ }
        if (!s || typeof s.text !== 'string' || Date.now() - (s.at || 0) > 60 * DAY) { localStorage.removeItem(k); continue; }
        if (!k.startsWith(prefix)) continue;
        const p = k.slice(prefix.length);
        if (p !== except && files.some((f) => f.path === p)) out.push(p);
      }
    } catch { /* storage unavailable */ }
    return out.sort();
  }
  /** A non-Markdown text file is shown as plain text, however it was loaded. */
  const asDoc = (p: string, d: any) => (isMd(p) ? { ...d, plain: false } : { ...d, plain: true });

  let emptyDirs = $state<string[]>([]);
  async function loadTree() {
    try {
      const all: any[] = await api('/library/tree');
      files = all.filter((f) => !isDir(f));
      emptyDirs = all.filter(isDir).map((f) => String(f.path).replace(/\/+$/, ''));
      treeError = '';
    }
    catch (e: any) { treeError = errDetail(e) || String(e); }
    finally { loading = false; }
  }

  /** Show the search page for the `?words` route; the page (SearchResults) looks the words up itself. */
  function runSearch() {
    searched = path ?? ''; results = [];
  }
  // Files at the top of the library that are there for the assistant (and the user's orientation),
  // grouped under "About this library" instead of sitting among the user's own folders.
  const SYSTEM = /^(CLAUDE|AGENTS|INDEX|START-HERE(\.[a-z]{2})?)\.md$/i;
  const isSystem = (p: string) => !p.includes('/') && SYSTEM.test(p);
  const lastKey = () => `thoughts.docs.last:${libKey()}`;
  const has = (p: string | null | undefined) => !!p && files.some((f) => f.path === p);
  /** Where Docs opens: the note opened last, else the library's welcome page, else the first own note. */
  function defaultFile(): string | undefined {
    let last: string | null = null;
    try { last = localStorage.getItem(lastKey()); } catch { /* private mode */ }
    if (has(last)) return last!;
    const welcome = lang() === 'ar' && has('START-HERE.ar.md') ? 'START-HERE.ar.md' : 'START-HERE.md';
    if (has(welcome)) return welcome;
    return files.find((f) => isMd(f.path) && !isSystem(f.path))?.path ?? files.find((f) => isMd(f.path))?.path ?? files[0]?.path;
  }
  const firstFile = () => defaultFile();

  function beforeUnload(e: BeforeUnloadEvent) { if (dirty) { e.preventDefault(); e.returnValue = ''; } }
  function onKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's' && editing) { e.preventDefault(); save(); }
  }
  // The shell leaves "Print" to this view while a document is open.
  function onCommand(e: Event) { if ((e as CustomEvent).detail?.id === 'print') printDoc(); }
  function onDocClick(e: MouseEvent) { if (menu && !(e.target as HTMLElement)?.closest?.('.menuwrap')) menu = false; }

  let off: (() => void) | undefined;
  let ready = $state(false);
  let searched = '';
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  onMount(async () => {
    window.addEventListener('beforeunload', beforeUnload);
    window.addEventListener('keydown', onKey);
    window.addEventListener('thoughts:command', onCommand);
    window.addEventListener('click', onDocClick);
    app.leaveGuard = leaveOk;
    off = on((e) => {
      if (e.type !== 'library.changed' && e.type !== 'resync') return;
      // Bursts (a wrap writes many files) settle into one refresh.
      clearTimeout(refreshTimer);
      refreshTimer = setTimeout(async () => { await loadTree(); refreshOpen(); libVersion++; }, 250);
    });
    await loadTree();
    try {
      if (treeError) return;
      if (path?.startsWith('?')) await runSearch();
      else await openFile(path || defaultFile(), false, !path);
      // Opened to be edited (an item's page: Edit, a new meeting note): straight into the editor.
      if (app.view.id === 'edit' && doc && !doc.binary && !editing && EDIT_EXT.test(current)) startEdit();
    } finally { ready = true; }
    const waiting = waitingDrafts(current);
    if (waiting.length) notify(tn('docsa.draftsWaiting', waiting.length, { name: prettyName(waiting[0]) }), { kind: 'info', key: 'docs-drafts-waiting', duration: 12000, action: { label: t('docsa.open'), run: () => go({ kind: 'docs', path: waiting[0] }) } });
  });

  // The app's route follows the open file (Back, the sidebar highlight and the palette agree), and a
  // route change made elsewhere (palette, search, Back) opens its file here without a remount.
  $effect(() => {
    const p = path;
    if (!ready) return;
    untrack(() => {
      if (treeError) return;
      if (p?.startsWith('?')) { if (results === null || searched !== p) runSearch(); }
      else if (p && p !== current) openFile(p, true);
      else if (!p) { if (current) go({ kind: 'docs', path: current }, { replace: true }); else openFile(defaultFile(), true, true); }
    });
  });
  // The shell drops the leave guard each time it navigates; this view stays mounted, so arm it again.
  $effect(() => { void app.view; untrack(() => { if (app.leaveGuard !== leaveOk) app.leaveGuard = leaveOk; }); });

  /** The open file changed on disk (the assistant, a sync, another app): show the new version, or,
   *  with unsaved edits, say so and offer the comparison. */
  async function refreshOpen() {
    const p = current;
    if (!p || !doc || doc.binary) return;
    if (saving) { setTimeout(refreshOpen, 400); return; }
    if (!files.some((f) => f.path === p)) {
      if (editing && dirty) theirs = { gone: true, content: '' };
      else if (!editing) { doc = null; docError = t('docsa.goneBody'); }
      return;
    }
    let d: any;
    try { d = await api(`/library/file?path=${encodeURIComponent(p)}`); } catch { return; }
    if (current !== p || !doc || (d.hash && d.hash === doc.hash)) return;
    if (!editing) { doc = asDoc(p, d); return; }
    if (!dirty) { doc = asDoc(p, d); draft = d.content; theirs = null; return; }
    if (d.content === draft) { doc = asDoc(p, d); theirs = null; return; }
    theirs = asDoc(p, d);
  }
  onDestroy(() => {
    window.removeEventListener('beforeunload', beforeUnload);
    window.removeEventListener('keydown', onKey);
    window.removeEventListener('thoughts:command', onCommand);
    window.removeEventListener('click', onDocClick);
    document.documentElement.classList.remove('print-doc');
    off?.();
    clearTimeout(refreshTimer);
    if (imgUrl) URL.revokeObjectURL(imgUrl);
    if (pdfUrl) URL.revokeObjectURL(pdfUrl);
    dropInline();
    if (app.leaveGuard === leaveOk) app.leaveGuard = null;
    reportUnsaved('docs', null);
  });

  /** Resolves true when it is safe to replace what is on screen. */
  async function leaveOk(): Promise<boolean> {
    if (!dirty) return true;
    const choice = await choiceDialog({
      title: t('docs.unsavedTitle'), body: t('docs.unsavedBody'), list: [current], cancelLabel: t('docs.keepEditing'),
      choices: [{ id: 'discard', label: t('docs.discard'), danger: true }, { id: 'save', label: t('save'), primary: true }],
    });
    if (choice === 'save') { const ok = await save(false, true); if (ok) { editing = false; refreshOpen(); } return ok; }
    if (choice === 'discard') { stashDrop(current); editing = false; theirs = null; return true; }
    return false;
  }

  async function loadPdf(p: string) {
    pdfError = '';
    try {
      const b = await apiBlob(`/library/raw?path=${encodeURIComponent(p)}`);
      const u = URL.createObjectURL(new Blob([b], { type: 'application/pdf' }));
      if (current === p) pdfUrl = u; else URL.revokeObjectURL(u);
    } catch (e) { if (current === p) pdfError = errDetail(e) || t('docs3.pdfFailed'); }
  }

  /** `replace`: the route is corrected in place (a default file), not added to Back. */
  async function openFile(p?: string, force = false, replace = false) {
    if (!p) return;
    if (!force && !(await leaveOk())) return;
    // A link to a folder opens the folder's front page when there is one.
    if (!files.some((f) => f.path === p) && files.some((f) => f.path === `${p}/README.md`)) p = `${p}/README.md`;
    if (p !== current) track('doc.open');
    results = null; editing = false; current = p; docError = ''; menu = false; treeOpen = false; showSource = false; theirs = null;
    if (path !== p) go({ kind: 'docs', path: p }, { replace });
    try { localStorage.setItem(lastKey(), p); } catch { /* private mode */ }
    if (imgUrl) { URL.revokeObjectURL(imgUrl); imgUrl = null; }
    if (pdfUrl) { URL.revokeObjectURL(pdfUrl); pdfUrl = null; }
    pdfError = '';
    // Make sure every folder above the file is open in the tree.
    const parts = p.split('/');
    for (let i = 1; i < parts.length; i++) { const k = parts.slice(0, i).join('/'); if (!open[k]) setOpen(k, true); }
    if (!isMd(p) && !TEXT_EXT.test(p)) {
      doc = { content: '', binary: true };
      if (isImageName(p)) rawBlobUrl(p).then((u) => { if (current === p) imgUrl = u; else if (u) URL.revokeObjectURL(u); });
      else if (isPdf(p)) loadPdf(p);
      return;
    }
    docLoading = true;
    doc = null;
    try {
      const d = await api(`/library/file?path=${encodeURIComponent(p)}`);
      if (current !== p) return;
      doc = asDoc(p, d);
      const kept = EDIT_EXT.test(p) ? stashGet(p) : null;
      if (kept && kept.text !== d.content) {
        draft = kept.text; editing = true;
        // The file moved on since the draft began: keep the draft's starting point, so Save compares.
        const moved = !!kept.base && !!d.hash && kept.base !== d.hash;
        if (moved) { doc = { ...doc, hash: kept.base }; theirs = asDoc(p, d); }
        notify(t(moved ? 'docsa.draftRestoredChanged' : 'docs.draftRestored'), { kind: 'info', key: 'docs-draft-restored' });
      } else if (kept) stashDrop(p);
    } catch (e: any) {
      if (current === p) { doc = null; docError = errDetail(e) || String(e); }
    } finally { if (current === p) docLoading = false; }
  }

  function startEdit() { draft = doc.content; editing = true; theirs = null; }

  /** Done: leave the editor (asking first about unsaved text), showing what is on disk now. */
  async function cancelEdit() {
    if (dirty && !(await leaveOk())) return;
    stashDrop(current);
    editing = false; theirs = null;
    refreshOpen();
  }

  /** Show the version on disk. The draft it replaces can be brought back for a while. */
  async function reloadTheirs(): Promise<boolean> {
    const p = current, mine = draft;
    let d: any;
    try { d = await api(`/library/file?path=${encodeURIComponent(p)}`); }
    catch (err) { failed(t('docsa.loadTheirsFailed'), err); return false; }
    if (current !== p) return true;
    stashDrop(p);
    doc = asDoc(p, d);
    editing = false; theirs = null;
    notify(t('docsa.usedTheirs'), { kind: 'info', duration: 20000, action: { label: t('docsa.bringBack'), run: () => {
      if (current !== p || editing) return;
      // Picks up from the version now shown, so saving replaces it on purpose.
      draft = mine; editing = true;
    } } });
    return true;
  }

  /** Free name next to `p` for "save mine as a copy": "plan (my version).md", "plan (my version 2).md"… */
  function copyName(p: string): string {
    const dot = baseName(p).lastIndexOf('.');
    const stem = dot > 0 ? p.slice(0, p.length - baseName(p).length + dot) : p;
    const extn = dot > 0 ? baseName(p).slice(dot) : '';
    for (let i = 1; ; i++) {
      const c = `${stem} (${t('docsa.copySuffix')}${i > 1 ? ' ' + i : ''})${extn}`;
      if (!taken(c)) return c;
    }
  }

  /** Both versions side by side; resolves true when the edit is settled (saved, copied or set aside). */
  async function resolveConflict(disk: any): Promise<boolean> {
    const p = current, mine = draft;
    const choice = await new Promise<'copy' | 'theirs' | 'mine' | null>((resolve) => {
      conflict = { theirs: disk?.gone ? '' : String(disk?.content ?? ''), gone: !!disk?.gone, resolve };
    });
    conflict = null;
    if (!choice || current !== p) return false;
    if (choice === 'mine') return save(true);
    if (choice === 'theirs') return reloadTheirs();
    // Keep both: my text goes into a new file beside this one; this one shows what is on disk.
    const copy = copyName(p);
    try { await post('/library/new', { path: copy, content: mine }); }
    catch (err) { failed(t('docsa.copyFailed'), err); return false; }
    await loadTree();
    notify(t('docsa.copySaved', { name: baseName(copy) }), { kind: 'success', duration: 12000, action: { label: t('docsa.open'), run: () => go({ kind: 'docs', path: copy }) } });
    stashDrop(p);
    editing = false; theirs = null;
    if (disk?.gone) { await openFile(copy, true); return true; }
    try { const d = await api(`/library/file?path=${encodeURIComponent(p)}`); if (current === p) doc = asDoc(p, d); } catch { /* the tree shows what is there */ }
    return true;
  }

  /** `force` saves over whatever is on disk (the user chose "Overwrite with mine"). */
  /** `closing`: the editor is being left right after (Save in the unsaved-changes question). */
  async function save(force: boolean | Event = false, closing = false): Promise<boolean> {
    force = force === true;
    if (saving) return false;
    saving = true;
    // Saving keeps the editor open (Done leaves it); text typed while the request runs stays unsaved.
    const p = current, sent = draft;
    try {
      const r = await put('/library/file', { path: p, content: sent, ...(force || !doc?.hash ? {} : { baseHash: doc.hash }) });
      if (current !== p) return true;
      doc = { ...doc, content: sent, hash: r?.hash ?? doc.hash };
      theirs = null;
      if (draft === sent) stashDrop(p);
      notify(t(editing && !closing ? 'docsa.savedStill' : 'docs.saved'));
      return true;
    } catch (e: any) {
      const msg: string = e?.message ?? String(e);
      if (!msg.startsWith('STALE::')) { failed(t('docs3.saveFailed'), e); return false; }
      // The file changed on disk after it was opened here (a sync, the assistant, another app).
      saving = false;
      let disk: any = { gone: true, content: '' };
      try { disk = await api(`/library/file?path=${encodeURIComponent(p)}`); }
      catch (err) { if ((err as any)?.status !== 404) { failed(t('docs3.saveFailed'), err); return false; } }
      if (current !== p) return false;
      return resolveConflict(disk);
    } finally { saving = false; }
  }

  // Pictures inside a note are library files: fetch their bytes (the page itself cannot load them) and swap them in.
  let inlineUrls: string[] = [];
  function dropInline() { for (const u of inlineUrls.splice(0)) URL.revokeObjectURL(u); }
  $effect(() => {
    const el = article, p = current;
    void doc?.content;
    dropInline();
    if (!el || !p || editing) return;
    tick().then(() => {
      for (const img of Array.from(el.querySelectorAll('img'))) {
        const src = img.getAttribute('src') || '';
        if (!src || /^([a-z][a-z0-9+.-]*:|\/\/|#)/i.test(src)) continue;
        img.removeAttribute('src');
        rawBlobUrl(resolveLink(p, src)).then((u) => {
          if (!u) return; // missing file, or a background service without /library/raw
          if (current !== p || !img.isConnected) { URL.revokeObjectURL(u); return; }
          inlineUrls.push(u);
          img.src = u;
        });
      }
    });
  });

  function clickLink(e: MouseEvent) {
    if (mdClick(e)) return;
    const a = (e.target as HTMLElement).closest('a');
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (/^[a-z]+:/i.test(href) || href.startsWith('#')) return;
    e.preventDefault();
    openFile(resolveLink(current, href));
  }

  /** A new chat with this file mentioned and the cursor in the message box; nothing is sent until the user does. */
  async function discuss() {
    const p = current;
    const priv = ((app.settings?.privatePaths ?? []) as string[]).some((x) => { const b = String(x).replace(/\/+$/, '').toLowerCase(); return !!b && (p.toLowerCase() === b || p.toLowerCase().startsWith(b + '/')); });
    if (priv) { notify(t('docsa.privateDiscuss'), { kind: 'error' }); return; }
    try {
      const c = await post('/chats', {});
      // The chat view picks this up as the draft of the new chat (same shape as its own drafts).
      try { localStorage.setItem(`thoughts.chatdraft:${c.id}`, JSON.stringify({ text: `@${p} `, mentions: [p], files: [] })); } catch { /* storage unavailable */ }
      await refreshChats().catch(() => {});
      go({ kind: 'chat', id: c.id });
    } catch (e) { failed(t('docsa.discussFailed'), e); }
  }

  async function openExternal() {
    try { await openLibraryFile(current); }
    catch (e: any) {
      const msg = String(e?.message ?? e);
      // An older shell without the command: stop offering the button.
      if (/not found|unknown command|not allowed|no such command/i.test(msg) && !/library/i.test(msg)) canOpenExternal = false;
      notify(t('docs.openFailed'), { kind: 'error', detail: msg });
    }
  }
  async function reveal() {
    try { await revealLibraryFile(current); }
    catch (e: any) { notify(t('docs.revealFailed'), { kind: 'error', detail: String(e?.message ?? e) }); }
  }
  /** What a reader sees, as text: the clean copy laid out off screen (a detached element has no line breaks). */
  function plainText(): string {
    const el = exportClone();
    if (!el) return '';
    el.style.cssText = 'position:fixed;inset-inline-start:-100000px;top:0;width:760px;';
    el.setAttribute('aria-hidden', 'true');
    document.body.appendChild(el);
    try { return el.innerText.trim(); } finally { el.remove(); }
  }
  async function copyDoc() {
    const text = plainText() || doc?.body || doc?.content || '';
    const ok = await copyText(text);
    notify(t(ok ? 'docs.copied' : 'docs.copyFailed'), { kind: ok ? 'info' : 'error' });
  }

  /** A clean copy of the rendered document (no copy buttons or focus helpers), for export and the clipboard. */
  function exportClone(): HTMLElement | null {
    if (!article) return null;
    const el = article.cloneNode(true) as HTMLElement;
    for (const b of Array.from(el.querySelectorAll('[data-md-copy]'))) b.remove();
    for (const n of Array.from(el.querySelectorAll('[tabindex]'))) n.removeAttribute('tabindex');
    return el;
  }

  async function copyRich() {
    const el = exportClone();
    if (!el || !article) return;
    const text = plainText();
    try {
      await navigator.clipboard.write([new ClipboardItem({
        'text/html': new Blob([el.innerHTML], { type: 'text/html' }),
        'text/plain': new Blob([text], { type: 'text/plain' }),
      })]);
      notify(t('docs3.richCopied'));
    } catch {
      const ok = await copyText(text);
      notify(t(ok ? 'docs3.richFallback' : 'docs.copyFailed'), { kind: ok ? 'info' : 'error' });
    }
  }

  const escHtml = (x: string) => x.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);
  const EXPORT_CSS = 'body{font:16px/1.6 -apple-system,"Segoe UI","Noto Sans Arabic",system-ui,sans-serif;color:#111;background:#fff;margin:0}main{max-width:760px;margin:0 auto;padding:32px 20px}img{max-width:100%}pre{background:#f4f4f4;padding:12px;border-radius:6px;white-space:pre-wrap;overflow-wrap:anywhere}code{font-family:ui-monospace,Menlo,Consolas,monospace;font-size:.92em}table{border-collapse:collapse}td,th{border:1px solid #999;padding:4px 8px;text-align:start}blockquote{margin:0;padding-inline-start:12px;border-inline-start:3px solid #999;color:#444}a{color:#0645ad}';
  async function exportHtml() {
    const el = exportClone();
    if (!el) return;
    try {
      // Pictures are shown from temporary addresses: embed them so the file stands on its own.
      for (const img of Array.from(el.querySelectorAll('img'))) {
        const src = img.getAttribute('src') || '';
        if (!src.startsWith('blob:')) continue;
        try { img.setAttribute('src', await readAsDataURL(await (await fetch(src)).blob())); } catch { img.removeAttribute('src'); }
      }
      const title = doc?.frontmatter?.title || info?.title || prettyName(current);
      const l = lang();
      const html = `<!doctype html>\n<html lang="${l}" dir="${l === 'ar' ? 'rtl' : 'ltr'}">\n<head>\n<meta charset="utf-8">\n<meta name="viewport" content="width=device-width, initial-scale=1">\n<title>${escHtml(String(title))}</title>\n<style>${EXPORT_CSS}</style>\n</head>\n<body>\n<main dir="auto">\n${el.innerHTML}\n</main>\n</body>\n</html>\n`;
      const r = await saveFile(baseName(current).replace(/\.[a-z0-9]+$/i, '') + '.html', html, 'text/html');
      if (r === 'saved') notify(t('docs3.exported'), { kind: 'success' });
      else if (r === 'unsupported') {
        const ok = await copyText(html);
        notify(t(ok ? 'docs3.exportCopied' : 'docs3.exportFailed'), { kind: ok ? 'info' : 'error' });
      }
    } catch (e) { failed(t('docs3.exportFailed'), e); }
  }

  /** Prints only the document: a copy of the rendered article is placed at the top of <body> for the print pass.
   *  The "hide everything else" print rules apply only while `html.print-doc` is set, so printing any other view is unaffected. */
  async function printDoc(title?: string | Event) {
    await tick();
    if (!article || editing) { notify(t(editing ? 'docs3.restoreUnsaved' : 'docs3.printNotDoc'), { kind: 'info' }); return; }
    document.getElementById('print-root')?.remove();
    const root = document.createElement('div');
    root.id = 'print-root';
    root.className = 'md';
    // The page runs the way most of the note reads (an Arabic note prints right to left).
    root.dir = docDir(article.textContent ?? '');
    for (const n of article.childNodes) root.appendChild(n.cloneNode(true));
    document.body.appendChild(root);
    const html = document.documentElement;
    // The window's title is the name a saved PDF is offered under.
    const was = document.title;
    if (typeof title === 'string' && title) document.title = title;
    const done = () => { root.remove(); html.classList.remove('print-doc'); document.title = was; window.removeEventListener('afterprint', done); };
    window.addEventListener('afterprint', done);
    html.classList.add('print-doc');
    try { window.print(); } catch { done(); notify(t('docs.printFailed'), { kind: 'error' }); }
  }

  const docTitle = () => String(doc?.frontmatter?.title || info?.title || prettyName(current));
  /** Where a note's exports go: the idea's `outputs/` folder (beside `notes/`), else one beside the note. */
  function outputsFolder(p: string): string {
    const parts = p.split('/').slice(0, -1);
    if (parts[parts.length - 1] === 'outputs') return parts.join('/');
    if (parts[parts.length - 1] === 'notes') parts.pop();
    return [...parts, 'outputs'].join('/');
  }
  let makingWord = false;
  /** The note as a Word document, saved with the idea's outputs (never replacing one). */
  async function exportDocx() {
    if (makingWord) return;
    if (showSource) { showSource = false; await tick(); }
    const el = exportClone();
    if (!el || editing) { notify(t('docs3.printNotDoc'), { kind: 'info' }); return; }
    makingWord = true;
    const p = current;
    try {
      const blob = await toDocx(el, { title: docTitle(), lang: lang() });
      const folder = outputsFolder(p);
      const r = await post('/library/output', { folder, name: baseName(p).replace(/\.[a-z0-9]+$/i, '') + '.docx', dataBase64: await readAsDataURL(blob) });
      await loadTree();
      const out = String(r?.path ?? '');
      notify(t('docsb.wordSaved', { folder }), { kind: 'success', duration: 12000, action: out ? { label: t('docsa.open'), run: () => openOutput(out) } : undefined });
    } catch (e) { failed(t('docsb.wordFailed'), e); }
    finally { makingWord = false; }
  }
  /** The note as a Word document, saved wherever the user picks (the system's save dialog in the
   *  desktop app, a download in a browser). */
  async function exportDocxAs() {
    if (makingWord) return;
    if (showSource) { showSource = false; await tick(); }
    const el = exportClone();
    if (!el || editing) { notify(t('docs3.printNotDoc'), { kind: 'info' }); return; }
    makingWord = true;
    try {
      const blob = await toDocx(el, { title: docTitle(), lang: lang() });
      const r = await saveFile(baseName(current).replace(/\.[a-z0-9]+$/i, '') + '.docx', blob, 'application/vnd.openxmlformats-officedocument.wordprocessingml.document');
      if (r === 'saved') notify(t('nativeb.word.saved'), { kind: 'success' });
      else if (r === 'unsupported') notify(t('nativeb.word.failed'), { kind: 'error' });
    } catch (e) { failed(t('nativeb.word.failed'), e); }
    finally { makingWord = false; }
  }
  /** A link that opens this note from a calendar event or another app (thoughts://note/…). */
  async function copyNoteLink() {
    const ok = await copyText(noteLink(current));
    notify(t(ok ? 'nativeb.link.copied' : 'nativeb.link.copyFailed'), { kind: ok ? 'success' : 'error' });
  }
  /** A saved export: in Word (or whatever opens it) on the desktop, here otherwise. */
  async function openOutput(p: string) {
    if (desktop && canOpenExternal && canOpenInApp(p)) {
      try { if (await openLibraryFile(p)) return; }
      catch { /* show it here instead */ }
    }
    go({ kind: 'docs', path: p });
  }
  /** "Save as PDF" goes through the print window, which offers it; the PDF is named after the note. */
  async function savePdf() {
    if (showSource) { showSource = false; await tick(); }
    notify(t(isMac ? 'docsb.pdfHintMac' : 'docsb.pdfHint'), { kind: 'info', duration: 15000, key: 'docs-pdf-hint' });
    await new Promise((r) => setTimeout(r, 350));
    printDoc(docTitle());
  }

  // ---------- create, rename, move, delete ----------
  /** A library-relative path typed by the user, tidied; null when it cannot be used. */
  function cleanPath(raw: string): string | null {
    const p = raw.trim().replace(/\\/g, '/').replace(/\/+/g, '/').replace(/^\/|\/$/g, '');
    if (!p) return '';
    if (p.split('/').some((seg) => !seg.trim() || seg.startsWith('.') || /[<>:"|?*\u0000-\u001f]/.test(seg))) return null;
    return p.split('/').map((x) => x.trim()).join('/');
  }
  const taken = (p: string) => files.some((f) => f.path.toLowerCase() === p.toLowerCase() || f.path.toLowerCase().startsWith(p.toLowerCase() + '/'));

  async function historyAvailable(): Promise<boolean> {
    if (historyOn !== null) return historyOn;
    try { await api('/library/history?limit=1'); historyOn = true; }
    catch (e) { if (errCode(e) === 'NO_HISTORY') historyOn = false; return false; }
    return historyOn;
  }

  async function newDoc(folder = dirName(current)) {
    const raw = await promptDialog({ title: folder ? t('docs3.newDocHere', { folder }) : t('docs3.newDocTitle'), placeholder: t('docs3.namePlaceholder') });
    if (raw === null) return;
    let rel = cleanPath(raw);
    if (!rel) { notify(t('docs3.badName'), { kind: 'error' }); return; }
    if (!/\.[a-z0-9]{1,5}$/i.test(rel)) rel += '.md';
    if (!EDIT_EXT.test(rel)) { notify(t('docs3.badName'), { kind: 'error' }); return; }
    const p = folder ? `${folder}/${rel}` : rel;
    if (taken(p)) { notify(t('docs3.exists'), { kind: 'error' }); return; }
    const title = prettyName(p);
    try {
      await post('/library/new', { path: p, content: isMd(p) ? `# ${title}\n\n` : '' });
      await loadTree();
      notify(t('docs3.created', { name: baseName(p) }), { kind: 'success' });
      await openFile(p);
      if (current === p && doc && !doc.binary) startEdit();
    } catch (e) { failed(t('docs3.createFailed'), e); }
  }

  // A folder exists once it holds a file, so a new folder starts with a short README.
  async function newFolder(parent = '') {
    const raw = await promptDialog({ title: t('docs3.newFolderTitle'), placeholder: t('docs3.folderPlaceholder') });
    if (raw === null) return;
    const rel = cleanPath(raw);
    if (!rel) { notify(t('docs3.badName'), { kind: 'error' }); return; }
    const dir = parent ? `${parent}/${rel}` : rel;
    if (taken(dir)) { notify(t('docs3.exists'), { kind: 'error' }); return; }
    const name = prettyName(dir);
    try {
      await post('/library/new', { path: `${dir}/README.md`, content: `# ${name}\n\n${t('docs3.folderStart', { name })}\n` });
      await loadTree();
      setOpen(dir, true);
      notify(t('docs3.created', { name: baseName(dir) }), { kind: 'success' });
      openFile(`${dir}/README.md`);
    } catch (e) { failed(t('docs3.createFailed'), e); }
  }

  async function createNew() {
    const c = await choiceDialog({ title: t('docs3.newTitle'), choices: [{ id: 'folder', label: t('docs3.newFolder') }, { id: 'doc', label: t('docs3.newDoc'), primary: true }] });
    if (c === 'doc') newDoc(); else if (c === 'folder') newFolder();
  }

  const inside = (p: string, what: string) => p === what || p.startsWith(what + '/');

  /** Move or rename a file or folder, then follow the open document to its new place. The library
   *  rewrites the links that pointed there; `done` gets the success message, completed with how many. */
  async function relocate(from: string, to: string, failKey: string, done: string): Promise<boolean> {
    if (to === from) return false;
    if (taken(to) && to.toLowerCase() !== from.toLowerCase()) { notify(t('docs3.exists'), { kind: 'error' }); return false; }
    const affected = !!current && inside(current, from);
    if (affected && !(await leaveOk())) return false;
    let r: any;
    try { r = await post('/library/move', { from, to }); }
    catch (e) { failed(t(failKey), e); return false; }
    for (const k of Object.keys(open)) if (inside(k, from) && open[k]) setOpen(to + k.slice(from.length), true);
    moveDrafts(from, to);
    await loadTree();
    if (affected) openFile(to + current.slice(from.length), true);
    const links = Number(r?.links) || 0, notes = Number(r?.notes) || 0;
    notify(links ? `${done} ${tn('docsb.linksUpdated', links, { notes: tn('docsb.inNotes', notes) })}` : done, { kind: 'success' });
    return true;
  }
  /** Unsaved drafts of files that moved go with them. */
  function moveDrafts(from: string, to: string) {
    const prefix = `${STASH}${libKey()}\n`;
    try {
      for (let i = localStorage.length - 1; i >= 0; i--) {
        const k = localStorage.key(i);
        if (!k?.startsWith(prefix)) continue;
        const p = k.slice(prefix.length);
        if (!inside(p, from)) continue;
        const v = localStorage.getItem(k);
        localStorage.removeItem(k);
        if (v !== null) localStorage.setItem(stashKey(to + p.slice(from.length)), v);
      }
    } catch { /* storage unavailable */ }
  }

  async function renamePath(p: string, folder = false) {
    const old = baseName(p);
    const raw = await promptDialog({ title: t('docs3.renameTitle', { name: old }), value: old });
    if (raw === null) return;
    let name = cleanPath(raw);
    if (!name || name.includes('/')) { notify(t('docs3.badName'), { kind: 'error' }); return; }
    const oldExt = /\.[a-z0-9]+$/i.exec(old)?.[0] ?? '';
    if (!folder && oldExt && !/\.[a-z0-9]{1,5}$/i.test(name)) name += oldExt;
    const to = dirName(p) ? `${dirName(p)}/${name}` : name;
    await relocate(p, to, 'docs3.renameFailed', t('docs3.renamed', { name }));
  }

  /** Every folder in the library (from the file list already loaded). */
  const allFolders = $derived.by(() => {
    const s = new Set<string>(emptyDirs);
    for (const f of files) { const parts = String(f.path).split('/'); for (let i = 1; i < parts.length; i++) s.add(parts.slice(0, i).join('/')); }
    return [...s];
  });
  function movePath(p: string, folder = false) { moving = { path: p, folder }; }
  async function moveTo(p: string, dir: string) {
    moving = null;
    if (inside(dir, p)) { notify(t('docs3.badName'), { kind: 'error' }); return; }
    const to = dir ? `${dir}/${baseName(p)}` : baseName(p);
    if (await relocate(p, to, 'docs3.moveFailed', dir ? t('docs3.moved', { folder: dir }) : t('docs3.movedTop'))) {
      if (dir) setOpen(dir, true);
    }
  }

  async function undoChange(hash: string, p: string, folder: boolean) {
    try {
      await post('/library/undo', { hash });
      await loadTree();
      notify(t('docs3.restored', { name: baseName(p) }), { kind: 'success' });
      if (!folder && !dirty) openFile(p);
    } catch (e) {
      if (errCode(e) === 'CONFLICT') notify(t('docs3.undoConflict'), { kind: 'error', detail: errDetail(e).split(';').map((x) => x.trim()).filter(Boolean).join('\n') });
      else failed(t('docs3.undoFailed'), e);
    }
  }

  /** Files go straight away with an Undo (when the library keeps a history); folders, and anything that
   *  cannot be brought back, ask first. */
  async function removePath(p: string, folder = false) {
    const name = baseName(p);
    const affected = !!current && inside(current, p);
    const hist = await historyAvailable();
    if (folder || !hist) {
      const ok = await confirmDialog({
        title: t('docs3.deleteTitle', { name }), list: [p], danger: true, confirmLabel: t('docs3.delete'),
        body: [folder ? t('docs3.deleteFolderBody') : '', hist ? (folder ? t('docs3.deleteFolderUndo') : '') : t('docs3.deleteNoUndo')].filter(Boolean).join(' '),
      });
      if (!ok) return;
    }
    // The draft of an open file inside is dropped only once the delete has really happened.
    try { await post('/library/delete', folder ? { path: p, recursive: true } : { path: p }); }
    catch (e) { failed(t('docs3.deleteFailed'), e); return; }
    if (affected) { editing = false; theirs = null; stashDrop(current); current = ''; doc = null; }
    await loadTree();
    if (affected) openFile(firstFile(), true);
    let hash = '';
    if (hist) {
      try {
        const top = (await api('/library/history?limit=1'))?.[0];
        if (top?.hash && (top.files ?? []).some((f: string) => inside(f, p))) hash = top.hash;
      } catch { /* no undo offered */ }
    }
    if (hash) notify(t('docs3.deleted', { name }), { action: { label: t('docs3.undo'), run: () => undoChange(hash, p, folder) }, duration: 12000 });
    else notify(t(hist ? 'docs3.deletedHistory' : 'docs3.deleted', { name }));
  }

  async function folderMenu(dir: string) {
    const c = await choiceDialog({
      title: t('docs3.folderTitle', { name: baseName(dir) }), list: [dir],
      choices: [
        { id: 'delete', label: t('docs3.delete'), danger: true }, { id: 'move', label: t('docs3.move') },
        { id: 'rename', label: t('docs3.rename') }, { id: 'new', label: t('docs3.newDoc'), primary: true },
      ],
    });
    if (c === 'new') newDoc(dir); else if (c === 'rename') renamePath(dir, true); else if (c === 'move') movePath(dir, true); else if (c === 'delete') removePath(dir, true);
  }

  // ---------- "More" menu for the open document ----------
  type MenuItem = { id: string; label: string; icon: string; run: () => void; danger?: boolean };
  const menuItems = $derived.by((): MenuItem[] => {
    const text = !!doc && !doc.binary;
    return [
      ...(text ? [
        { id: 'versions', label: t('docs3.versions'), icon: 'rollback', run: () => (showVersions = true) },
        ...(isMd(current) ? [{ id: 'docx', label: t('docsb.saveWord'), icon: 'download', run: exportDocx }, { id: 'docx-as', label: t(desktop ? 'nativeb.word.saveAs' : 'nativeb.word.download'), icon: 'download', run: exportDocxAs }, { id: 'pdf', label: t('docsb.savePdf'), icon: 'print', run: savePdf }] : []),
        { id: 'html', label: t('docs3.exportHtml'), icon: 'download', run: exportHtml },
        { id: 'rich', label: t('docs3.copyRich'), icon: 'copy', run: copyRich },
        { id: 'copy', label: t('docs.copyText'), icon: 'copy', run: copyDoc },
        ...(isMd(current) ? [{ id: 'source', label: t(showSource ? 'docsa.showFormatted' : 'docsa.showSource'), icon: 'eye', run: () => (showSource = !showSource) }] : []),
      ] : []),
      ...(desktop && current ? [{ id: 'reveal', label: t(revealLabelKey()), icon: 'folder', run: reveal }] : []),
      ...(desktop && current ? [{ id: 'link', label: t('nativeb.link.copy'), icon: 'copy', run: copyNoteLink }] : []),
      { id: 'rename', label: t('docs3.rename'), icon: 'edit', run: () => renamePath(current) },
      { id: 'move', label: t('docs3.move'), icon: 'arrow', run: () => movePath(current) },
      { id: 'delete', label: t('docs3.delete'), icon: 'trash', danger: true, run: () => removePath(current) },
    ];
  });
  async function toggleMenu() {
    menu = !menu;
    if (menu) { await tick(); menuBtn?.parentElement?.querySelector<HTMLElement>('[role="menuitem"]')?.focus(); }
  }
  function menuKey(e: KeyboardEvent) {
    const items = Array.from((e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[role="menuitem"]'));
    const i = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); menu = false; menuBtn?.focus(); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); items[(i + 1) % items.length]?.focus(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); items[(i - 1 + items.length) % items.length]?.focus(); }
    else if (e.key === 'Home') { e.preventDefault(); items[0]?.focus(); }
    else if (e.key === 'End') { e.preventDefault(); items[items.length - 1]?.focus(); }
    else if (e.key === 'Tab') menu = false;
  }
  function pickMenu(it: MenuItem) { menu = false; menuBtn?.focus(); it.run(); }

  // ---------- file list as an overlay (narrow windows) ----------
  async function showTree() { treeOpen = true; await tick(); treeEl?.querySelector<HTMLElement>('input, button')?.focus(); }
  function hideTree() { if (!treeOpen) return; treeOpen = false; treeBtn?.focus(); }

  async function importFiles(list: Iterable<File>) {
    const all = [...list];
    if (!all.length || importing) return;
    const ok = all.filter((f) => f.size <= MAX_FILE_BYTES);
    for (const f of all) if (f.size > MAX_FILE_BYTES) notify(t('docs.tooLarge', { name: f.name, size: fmtSize(f.size) }), { kind: 'error' });
    if (!ok.length) return;
    importing = true;
    try {
      const payload = [];
      for (const f of ok) payload.push({ name: f.name, dataBase64: (await readAsDataURL(f)).split(',').pop() || '' });
      const r = await post('/library/import', { files: payload });
      const paths: string[] = r?.paths ?? [];
      await loadTree();
      notify(tn('docs.imported', paths.length || ok.length));
      if (paths[0] && !dirty) openFile(paths[0]);
    } catch (e: any) {
      const msg = e?.message ?? String(e);
      if (/not found|404|method not allowed/i.test(msg)) notify(t('docs.importUnavailable'), { kind: 'error' }); else failed(t('docs3.importFailed'), e);
    } finally { importing = false; }
  }
  function pick(e: Event) { const el = e.target as HTMLInputElement; importFiles(el.files || []); el.value = ''; }

  const hasFiles = (e: DragEvent) => [...(e.dataTransfer?.types || [])].includes('Files');
  function dragenter(e: DragEvent) { if (!hasFiles(e)) return; e.preventDefault(); dragDepth++; dragging = true; }
  function dragover(e: DragEvent) { if (hasFiles(e)) e.preventDefault(); }
  function dragleave(e: DragEvent) { if (!hasFiles(e)) return; dragDepth = Math.max(0, dragDepth - 1); if (!dragDepth) dragging = false; }
  function drop(e: DragEvent) { if (!hasFiles(e)) return; e.preventDefault(); dragDepth = 0; dragging = false; importFiles(e.dataTransfer?.files || []); }

  const titleOf = (f: any) => f.title || prettyName(f.path);
  type Node = { name: string; path: string; dirs: Node[]; files: any[]; count: number };
  const tree = $derived.by((): Node => {
    const root: Node = { name: '', path: '', dirs: [], files: [], count: 0 };
    const q = fold(filter.trim());
    const dirAt = (parts: string[], n: number, counted: boolean) => {
      let node = root;
      for (let i = 0; i < n; i++) {
        const p = parts.slice(0, i + 1).join('/');
        let c = node.dirs.find((d) => d.path === p);
        if (!c) { c = { name: parts[i], path: p, dirs: [], files: [], count: 0 }; node.dirs.push(c); }
        if (counted) c.count++;
        node = c;
      }
      return node;
    };
    for (const f of files) {
      if (q && !fold(`${f.path} ${titleOf(f)}`).includes(q)) continue;
      const parts = String(f.path).split('/');
      if (isSystem(f.path)) { root.count++; continue; }
      dirAt(parts, parts.length - 1, true).files.push(f);
      root.count++;
    }
    if (!q) for (const d of emptyDirs) { const parts = d.split('/'); dirAt(parts, parts.length, false); }
    const sort = (n: Node) => { n.dirs.sort((x, y) => x.name.localeCompare(y.name)); n.dirs.forEach(sort); };
    sort(root);
    return root;
  });
  const isOpen = (k: string) => !!(open[k] || filter.trim());
  const systemFiles = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return files.filter((f) => isSystem(f.path) && (!q || `${f.path} ${titleOf(f)}`.toLowerCase().includes(q))).sort((a, b) => a.path.localeCompare(b.path));
  });
  const SYS = '\u0000system';
  const sysOpen = $derived(!!(open[SYS] || filter.trim() || isSystem(current)));
  const hidden = $derived(!!doc && !doc.binary && !doc.plain && isMd(current) && hasHiddenText(String(doc.content ?? '')));

  const FM = ['status', 'area', 'updated', 'level'];
  function fmValue(k: string, v: any): string {
    const s = String(v);
    if (k === 'status') { const key = `idea.status.${s}`; const x = t(key); return x === key ? s : x; }
    if (k === 'updated' && /^\d{4}-\d{2}-\d{2}/.test(s)) return fmtDate(s.slice(0, 10) + 'T12:00:00');
    return s;
  }
  const ext = (p: string) => (p.includes('.') ? p.split('.').pop()!.toUpperCase() : '');
</script>

{#snippet fileRow(f: any, depth: number)}
  <button class="f" class:active={current === f.path} aria-current={current === f.path ? 'page' : undefined} style="padding-inline-start: {6 + depth * 14}px" onclick={() => openFile(f.path)} title={f.path}>
    <span class="ft" dir="auto">{titleOf(f)}</span><bdi class="fp">{baseName(f.path)}</bdi></button>
{/snippet}
{#snippet level(n: Node, depth: number)}
  {#each n.dirs as d (d.path)}
    <div class="dirrow" style="padding-inline-start: {depth * 14}px">
      <button class="dir grow" onclick={() => setOpen(d.path, !open[d.path])} aria-expanded={isOpen(d.path)}>
        <Icon name={isOpen(d.path) ? 'down' : 'right'} size={13} class={isOpen(d.path) ? '' : 'flip-rtl'} /> <bdi class="dname">{d.name}</bdi> <span class="count muted">{fmtNum(d.count)}</span></button>
      <button class="dact" onclick={() => folderMenu(d.path)} aria-label={t('docs3.folderActions', { name: d.name })} title={t('docs3.folderActions', { name: d.name })}><Icon name="edit" size={13} /></button>
    </div>
    {#if isOpen(d.path)}{@render level(d, depth + 1)}{/if}
  {/each}
  {#each n.files as f (f.path)}{@render fileRow(f, depth)}{/each}
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="docs-c">
<div class="docs" ondragenter={dragenter} ondragover={dragover} ondragleave={dragleave} ondrop={drop}>
  {#if treeOpen}<div class="scrim" role="presentation" onclick={hideTree}></div>{/if}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <aside class="tree scroll no-print" class:open={treeOpen} aria-label={t('docs.filesLabel')} bind:this={treeEl}
    onkeydown={(e) => { if (e.key === 'Escape' && treeOpen) { e.stopPropagation(); hideTree(); } }}>
    <div class="treehead">
      <div class="row">
        {#if shows('docs', 'filter')}<input class="grow" placeholder={pageText('docs', 'filter', t('docs.filter'))} aria-label={pageText('docs', 'filter', t('docs.filter'))} bind:value={filter} dir="auto" />{:else}<span class="grow"></span>{/if}
        <button class="btn ghost ic closetree" onclick={hideTree} aria-label={t('docs3.closeFiles')} title={t('docs3.closeFiles')}><Icon name="close" size={15} /></button>
      </div>
      <div class="row">
        <button class="btn grow tb" onclick={createNew}><Icon name="plus" size={14} /> {t('docs3.new')}</button>
        <button class="btn grow tb" onclick={bringIn} disabled={importing} title={t('sb.imp.buttonTip')}><Icon name="upload" size={14} /> {t(importing ? 'docs.importing' : 'sb.imp.button')}</button>
      </div>
      <input type="file" multiple hidden bind:this={picker} onchange={pick} />
    </div>
    {#if loading}
      <div class="skel"></div><div class="skel w70"></div><div class="skel w50"></div><div class="skel w70"></div>
    {:else if !files.length && !emptyDirs.length}
      <p class="muted sm note">{t('docs.treeEmpty')}</p>
    {:else if !tree.count && filter.trim()}
      <p class="muted sm note">{t('docs.noFilterMatches')}</p>
      <button class="btn ghost sm" onclick={() => (filter = '')}>{t('docs.clearFilter')}</button>
    {/if}
    {@render level(tree, 0)}
    {#if systemFiles.length}
      <div class="dirrow sys">
        <button class="dir grow" onclick={() => setOpen(SYS, !open[SYS])} aria-expanded={sysOpen} title={t('docsa.systemTip')}>
          <Icon name={sysOpen ? 'down' : 'right'} size={13} class={sysOpen ? '' : 'flip-rtl'} /> <Icon name="info" size={13} /> <span class="dname">{t('docsa.systemGroup')}</span> <span class="count muted">{fmtNum(systemFiles.length)}</span></button>
      </div>
      {#if sysOpen}{#each systemFiles as f (f.path)}{@render fileRow(f, 1)}{/each}{/if}
    {/if}
  </aside>

  <FindBar root={() => article} editor={() => (editing ? treeEl?.parentElement?.querySelector<HTMLTextAreaElement>('textarea.editor') : null)} where={'doc:' + (path ?? '')} bottom />
  <section class="view scroll">
    <div class="narrowbar no-print">
      <button class="btn" bind:this={treeBtn} onclick={showTree} aria-expanded={treeOpen}><Icon name="folder" size={14} /> {t('docs3.files')}</button>
    </div>
    <div class="no-print"><PageExtras page="docs" intro={false} /></div>
    {#if loading}
      <div class="pad" role="status" aria-busy="true"><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div><div class="skel w50"></div></div>
    {:else if treeError}
      <div class="state" role="alert"><Icon name="warning" size={26} /><h1>{t('docs.loadFailed')}</h1><p class="muted" dir="auto">{treeError}</p>
        <button class="btn primary" onclick={() => { loading = true; loadTree().then(() => { if (!treeError && !current) openFile(firstFile()); }); }}><Icon name="retry" size={14} /> {t('docs3.retry')}</button></div>
    {:else if results && path?.startsWith('?')}
      {#key path}<SearchResults query={path.slice(1)} />{/key}
    {:else if !files.length}
      <div class="state">
        <Icon name="docs" size={30} />
        <h1>{t('docs.emptyTitle')}</h1>
        <p class="muted">{t('docs.emptyBody')}</p>
        <div class="row wrapr">
          <button class="btn primary" onclick={() => newDoc('')}><Icon name="plus" size={14} /> {t('docs3.newDoc')}</button>
          <button class="btn" onclick={bringIn} disabled={importing} title={t('sb.imp.buttonTip')}><Icon name="upload" size={14} /> {t('sb.imp.button')}</button>
          <button class="btn" onclick={() => go({ kind: 'home' })}>{t('docs.emptyAsk')}</button>
        </div>
      </div>
    {:else if docError}
      <div class="state" role="alert"><Icon name="warning" size={26} /><h1>{t('docs.fileFailed')}</h1><p class="muted"><bdi>{current}</bdi></p><p class="muted sm" dir="auto">{docError}</p>
        <button class="btn primary" onclick={() => openFile(current)}><Icon name="retry" size={14} /> {t('docs3.retry')}</button></div>
    {:else if doc}
      <div class="row bar no-print">
        <code class="grow path"><bdi>{current}</bdi></code>
        {#if editing}
          <span class="dirty" class:on={dirty} role="status"><span class="dot" aria-hidden="true"></span>{t(dirty ? 'docs.unsaved' : 'docs.noChanges')}</span>
          {#if !doc.plain}<button class="btn" onclick={togglePreview} aria-pressed={preview}><Icon name="eye" size={15} /> {t('docs.preview')}</button>{/if}
          <button class="btn" onclick={cancelEdit} title={t('docsa.doneTip')}>{t('docsa.done')}</button>
          <button class="btn primary" onclick={save} disabled={!dirty || saving} title={t('docs.saveTip')}>{t(saving ? 'docs.saving' : 'save')}</button>
        {:else}
          {#if !doc.binary}
            <button class="btn ghost ic" onclick={printDoc} title={t('docs.print')} aria-label={t('docs.print')}><Icon name="print" size={16} /></button>
            <button class="btn ghost" onclick={() => (showVersions = true)} title={t('docs3.versionsTitle')}><Icon name="rollback" size={15} /> {t('docs3.versions')}</button>
          {/if}
          <div class="menuwrap">
            <button class="btn ghost" bind:this={menuBtn} onclick={toggleMenu} aria-haspopup="menu" aria-expanded={menu} title={t('docs3.moreFor')}>{t('docs3.more')} <Icon name="down" size={13} /></button>
            {#if menu}
              <!-- svelte-ignore a11y_interactive_supports_focus -->
              <div class="menu" role="menu" aria-label={t('docs3.moreFor')} onkeydown={menuKey}>
                {#each menuItems as it (it.id)}
                  <button role="menuitem" class="mi" class:danger={it.danger} onclick={() => pickMenu(it)}><Icon name={it.icon} size={14} class={it.icon === 'arrow' ? 'flip-rtl' : ''} /> {it.label}</button>
                {/each}
              </div>
            {/if}
          </div>
          <button class="btn" onclick={discuss} title={t('docs.discussTip')}><Icon name="chat" size={15} /> {t('docs.discuss')}</button>
          {#if !doc.binary && EDIT_EXT.test(current)}<button class="btn" onclick={startEdit}><Icon name="edit" size={14} /> {t('edit')}</button>{/if}
        {/if}
      </div>

      {#if doc.binary}
        <div class="filecard surface">
          <div class="row">
            <span class="fbadge"><Icon name="docs" size={20} /></span>
            <div class="grow">
              <h1 class="fname" dir="auto">{baseName(current)}</h1>
              <div class="muted sm">{[ext(current) && t('docs.fileType', { type: ext(current) }), info ? fmtSize(info.size) : '', info?.modified ? fmtDate(info.modified) : ''].filter(Boolean).join(' · ')}</div>
            </div>
          </div>
          {#if !isImageName(current) && !isPdf(current) && !isDocx(current)}<p class="muted">{t('docs.notPreviewable')}</p>{/if}
          {#if desktop && !canOpenInApp(current)}<p class="muted">{t('sec-shell-open.notOpened')}</p>{/if}
          <div class="row wrapr">
            {#if desktop && canOpenExternal && canOpenInApp(current)}<button class="btn primary" onclick={openExternal}><Icon name="external" size={14} /> {t('docs.openDefault')}</button>
            {:else if desktop}<button class="btn primary" onclick={reveal}><Icon name="folder" size={14} /> {t(revealLabelKey())}</button>{/if}
            <button class="btn" onclick={discuss}><Icon name="chat" size={14} /> {t('docs.discussAssistant')}</button>
          </div>
        </div>
        {#if imgUrl}<img class="inline-img" src={imgUrl} alt={baseName(current)} />{/if}
        {#if isDocx(current)}<DocxPreview path={current} name={baseName(current)} />{/if}
        {#if isPdf(current)}
          {#if pdfUrl}
            <iframe class="pdf" src={pdfUrl} title={t('docs3.pdfTitle', { name: baseName(current) })}></iframe>
          {:else if pdfError}
            <div class="state" role="alert"><Icon name="warning" size={24} /><p>{t('docs3.pdfFailed')}</p><p class="muted sm" dir="auto">{pdfError}</p>
              <button class="btn" onclick={() => loadPdf(current)}><Icon name="retry" size={14} /> {t('docs3.retry')}</button></div>
          {:else}
            <div class="pad" role="status" aria-busy="true"><span class="sr-only">{t('docs3.pdfLoading')}</span><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div></div>
          {/if}
        {/if}
      {:else if editing}
        {#if theirs}
          <div class="banner no-print" role="status">
            <Icon name="warning" size={16} /><span class="grow">{t(theirs.gone ? 'docsa.goneBanner' : 'docsa.changedBanner')}</span>
            <button class="btn sm-btn" onclick={() => resolveConflict(theirs)}>{t('docsa.compare')}</button>
          </div>
        {/if}
        <Editor bind:value={draft} path={current} plain={doc.plain} preview={preview && !doc.plain} label={t('docs.editorLabel', { name: current })} />
      {:else if doc.plain}
        <h1 class="ptitle" dir="auto">{baseName(current)}</h1>
        <article bind:this={article} aria-label={t('docs3.textLabel', { name: baseName(current) })}><pre class="plain" dir="auto">{doc.content}</pre></article>
      {:else if showSource}
        <div class="banner info no-print">
          <span class="grow">{t('docsa.sourceNote')} <code class="inv">U+202E</code></span>
          <button class="btn sm-btn" onclick={() => (showSource = false)}>{t('docsa.showFormatted')}</button>
        </div>
        <article bind:this={article} aria-label={t('docsa.sourceLabel', { name: baseName(current) })}><pre class="plain src">{#each revealInvisible(String(doc.content ?? '')) as seg}{#if seg.code}<code class="inv">{seg.text}</code>{:else}{seg.text}{/if}{/each}</pre></article>
      {:else}
        {#if hidden}
          <div class="banner no-print" role="note">
            <Icon name="eye" size={16} /><span class="grow">{t('docsa.hiddenBody')}</span>
            <button class="btn sm-btn" onclick={() => (showSource = true)}>{t('docsa.showSource')}</button>
          </div>
        {/if}
        {#if doc.frontmatter && typeof doc.frontmatter === 'object'}
          <div class="row fm">
            {#each Object.entries(doc.frontmatter).filter(([k, v]) => FM.includes(k) && v != null && v !== '') as [k, v]}<span class="pill">{t(`docs.fm.${k}`)}: <bdi>{fmValue(k, v)}</bdi></span>{/each}
          </div>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <article class="md" dir="auto" bind:this={article} onclick={clickLink}>{@html md(doc.content)}</article>
        <Backlinks path={current} version={libVersion} open={(p) => openFile(p)} />
      {/if}
      {#if doc.binary}<Backlinks path={current} version={libVersion} open={(p) => openFile(p)} />{/if}
    {:else if docLoading}
      <div class="pad" role="status" aria-busy="true"><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div><div class="skel w50"></div></div>
    {/if}
    <div class="no-print"><PageExtras page="docs" place="bottom" /></div>
  </section>

  {#if dragging}<div class="dropzone" aria-hidden="true"><Icon name="upload" size={28} /><span>{t('docs.dropHere')}</span></div>{/if}
</div>
</div>

{#if moving}
  <FolderPicker item={moving.path} folder={moving.folder} folders={allFolders} clean={(raw) => cleanPath(raw) || null}
    pick={(dir) => { const m = moving; if (m) moveTo(m.path, dir); }} close={() => (moving = null)} />
{/if}
{#if conflict}
  <Conflict name={baseName(current)} theirs={conflict.theirs} mine={draft} gone={conflict.gone} done={(c) => conflict?.resolve(c)} />
{/if}

{#if showVersions && current}
  <Versions path={current} close={() => (showVersions = false)}
    restored={() => { stashDrop(current); editing = false; notify(t('docs3.restoredVersion'), { kind: 'success' }); openFile(current, true); }} />
{/if}

{#if showImportNotes}<ImportNotes close={() => (showImportNotes = false)} done={broughtIn} />{/if}

<style>
  .docs-c { container-type: inline-size; container-name: docs; }
  .docs { position: relative; display: grid; grid-template-columns: 280px minmax(0, 1fr); height: calc(100vh - 26px); }
  .tree { border-inline-end: var(--border-w) solid var(--border); padding: 0 8px 12px; display: flex; flex-direction: column; gap: 1px; }
  .treehead { position: sticky; top: 0; z-index: 1; background: var(--bg); display: grid; gap: 6px; padding-bottom: 6px; }
  .treehead input { padding: 5px 8px; font-size: 13px; }
  .treehead .row { gap: 6px; }
  .tb { font-size: var(--fs-sm); padding-inline: 8px; min-width: 0; }
  .closetree, .narrowbar, .scrim { display: none; }
  .dirrow { display: flex; align-items: center; gap: 2px; margin-top: 2px; flex: none; }
  .dname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dact { font: inherit; background: none; border: 0; padding: 0; cursor: pointer; width: 26px; height: 26px; display: grid; place-items: center; border-radius: 6px; color: var(--muted); flex: none; }
  .dact:hover { background: var(--panel-2); color: var(--fg); }
  .f, .dir { font: inherit; color: inherit; background: none; border: 0; text-align: start; cursor: pointer; font-size: 13px; padding: 4px 6px; border-radius: 6px; min-width: 0; flex: none; }
  .f { display: flex; flex-direction: column; line-height: 1.3; }
  .ft, .fp { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  .fp { font-size: var(--fs-xs); color: var(--muted); font-family: var(--mono); }
  .dir { display: flex; align-items: center; gap: 5px; font-weight: 600; }
  .dir .count { margin-inline-start: auto; font-weight: 400; font-size: var(--fs-xs); }
  .f:hover, .dir:hover { background: var(--panel-2); }
  /* Current item in a list = tint (see the "selected" note in theme.css). */
  .f.active { background: var(--tint); color: var(--on-tint); font-weight: 600; }
  .f.active .fp { color: inherit; font-weight: 400; }
  .note { margin: 8px 6px; }

  .view { padding: 0 32px 40px; min-width: 0; }
  .bar { position: sticky; top: 0; background: var(--bg); padding: 6px 0; z-index: 2; flex-wrap: wrap; }
  .path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-sm); }
  .ic { padding: 6px 8px; }
  .menuwrap { position: relative; }
  .menu { position: absolute; top: calc(100% + 4px); inset-inline-end: 0; z-index: 8; min-width: 230px; padding: 5px; display: grid; gap: 1px; background: var(--panel); border: var(--border-w) solid var(--hairline); border-radius: var(--radius-lg); box-shadow: var(--shadow-lg); }
  .mi { font: inherit; font-size: 13px; color: var(--fg); background: none; border: 0; text-align: start; cursor: pointer; display: flex; align-items: center; gap: 8px; padding: 7px 9px; border-radius: 6px; white-space: nowrap; }
  .mi:hover, .mi:focus-visible { background: var(--fill); }
  .mi :global(.icon) { color: var(--muted); flex: none; }
  .mi.danger, .mi.danger :global(.icon) { color: var(--danger); }
  .ptitle { font-size: 20px; margin: 10px 0 0; overflow-wrap: anywhere; }
  .pdf { display: block; width: 100%; height: calc(100vh - 230px); min-height: 360px; margin-top: 14px; border: var(--border-w) solid var(--border); border-radius: var(--radius); background: #fff; }
  .dirty { display: inline-flex; align-items: center; gap: 6px; font-size: var(--fs-sm); color: var(--muted); }
  .dirty .dot { width: 8px; height: 8px; border-radius: 50%; border: 1.5px solid var(--muted); }
  .dirty.on { color: var(--fg); font-weight: 500; }
  .dirty.on .dot { background: var(--accent-2); border-color: var(--accent-2); }
  .fm { flex-wrap: wrap; margin: 6px 0 10px; }
  .fm .pill { font-size: var(--fs-xs); }

  .banner { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin: 6px 0 10px; padding: 8px 12px; border-radius: var(--radius);
    border: var(--border-w) solid color-mix(in srgb, var(--accent-2) 45%, var(--border)); background: color-mix(in srgb, var(--accent-2) 10%, var(--bg)); font-size: var(--fs-sm); }
  .banner.info { border-color: var(--border); background: var(--panel); }
  .banner :global(.icon) { flex: none; color: var(--accent-2); }
  .sm-btn { font-size: var(--fs-sm); padding: 4px 10px; }
  .src { unicode-bidi: plaintext; text-align: start; }
  .inv { font-family: var(--mono); font-size: .85em; padding: 0 3px; border-radius: 4px; border: 1px dashed var(--danger); color: var(--danger); }
  .sys { margin-top: 10px; }

  /* Narrow window: the reading column takes the whole width and the file list opens over it. */
  @container docs (max-width: 760px) {
    .docs { grid-template-columns: minmax(0, 1fr); }
    .tree { display: none; position: absolute; inset-block: 0; inset-inline-start: 0; z-index: 12; width: min(320px, 88%); background: var(--bg); box-shadow: var(--shadow-lg); padding-top: 8px; }
    .tree.open { display: flex; }
    .scrim { display: block; position: absolute; inset: 0; z-index: 11; background: rgba(0,0,0,.25); }
    .closetree { display: inline-flex; }
    .narrowbar { display: flex; padding-top: 6px; }
    .view { padding-inline: 20px; }
  }


  .state { display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; gap: 6px; min-height: 60%; padding: 40px 0; color: var(--muted); }
  .state h1 { margin: 6px 0 0; color: var(--fg); font-size: 18px; letter-spacing: -.02em; }
  .state p { margin: 0; max-width: 440px; }
  .state .row, .state > .btn { margin-top: 12px; }

  .filecard { display: grid; gap: 12px; max-width: 560px; margin-top: 10px; }
  .filecard > * { position: relative; }
  .filecard p { margin: 0; }
  .fbadge { width: 40px; height: 40px; border-radius: 10px; display: grid; place-items: center; background: var(--fill); color: var(--accent-2); flex: none; }
  .fname { display: block; overflow-wrap: anywhere; font-size: 14px; font-weight: 700; letter-spacing: 0; line-height: 1.4; margin: 0; }
  .wrapr { flex-wrap: wrap; }
  .inline-img { display: block; max-width: 100%; margin-top: 16px; border-radius: var(--radius); box-shadow: var(--shadow); }
  .plain { margin-top: 16px; font-size: var(--fs-sm); white-space: pre-wrap; overflow-wrap: anywhere; unicode-bidi: plaintext; text-align: start; }

  .pad { padding-top: 16px; display: grid; gap: 12px; }
  .skel { height: 13px; margin: 5px 6px; border-radius: 8px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .skel.h { height: 26px; width: 45%; }
  .w70 { width: 70%; } .w50 { width: 50%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel { animation: none; } }

  .dropzone { position: absolute; inset: 8px; z-index: 10; pointer-events: none; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; font-weight: 500; color: var(--accent); border: 2px dashed var(--accent); border-radius: var(--radius-lg); background: color-mix(in srgb, var(--bg) 88%, transparent); }

  .sm { font-size: var(--fs-xs); }

  /* Printing a document: only the copy made by printDoc() is laid out. These rules apply ONLY while
     printDoc() has set `html.print-doc`, so "Print" in any other view prints that view normally. */
  :global(#print-root) { display: none; }
  @media print {
    :global(html.print-doc), :global(html.print-doc body) { height: auto !important; overflow: visible !important; background: #fff !important; }
    :global(html.print-doc body > *:not(#print-root)) { display: none !important; }
    :global(html.print-doc #print-root) { display: block; color: #000; background: #fff; font-size: 12pt; line-height: 1.5; padding: 0; }
    :global(html.print-doc #print-root a) { color: inherit; text-decoration: underline; }
    :global(html.print-doc #print-root pre), :global(html.print-doc #print-root table), :global(html.print-doc #print-root img), :global(html.print-doc #print-root blockquote) { break-inside: avoid; }
    :global(html.print-doc #print-root pre) { white-space: pre-wrap; border: 1px solid #ccc; padding: 8px; }
    :global(html.print-doc #print-root [data-md-copy]) { display: none; }
    :global(html.print-doc #print-root h1), :global(html.print-doc #print-root h2), :global(html.print-doc #print-root h3) { break-after: avoid; }
    :global(html.print-doc #print-root td), :global(html.print-doc #print-root th) { border: 1px solid #999; }
    /* Any other view printed while Docs is open underneath (never the case today) keeps the app layout rules in theme.css. */
    .docs { height: auto; }
  }
</style>
