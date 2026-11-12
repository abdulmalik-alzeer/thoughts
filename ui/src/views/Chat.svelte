<script lang="ts" module>
  type Attachment = { name: string; data: string; size: number; image: boolean; /** bytes were lost on restart: must be attached again */ missing?: boolean };
  // Kept for the session, per chat: attachments still waiting in the composer (their bytes cannot
  // go into localStorage) and the files of the last message sent (so Retry can send them again).
  const pendingFiles = new Map<string, Attachment[]>();
  const lastSent = new Map<string, { text: string; files: Attachment[] }>();
  const DRAFT_MAX = 20000;
  /** What one message may carry in files, all together (the service refuses more: ATTACHMENTS_MAX_BYTES). */
  const MAX_TOTAL_BYTES = 50 * 1024 * 1024;
  const draftKey = (id: string) => `thoughts.chatdraft:${id}`;
</script>

<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onMount, tick } from 'svelte';
  import FindBar from '../lib/FindBar.svelte';
  import { api, del, on, patch, post, ApiError } from '../lib/api';
  import { md, mdClick } from '../lib/md';
  import { app, costVisible, go, refreshChats } from '../lib/store.svelte';
  import { t, tn, fmtMoney, fmtNum } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { signIn } from '../lib/auth';
  import { normalizeCapture } from '../lib/capture';
  import { confirmDialog, promptDialog } from '../lib/dialog.svelte';
  import { openReview } from '../lib/layer';
  import WrapPanel from './WrapPanel.svelte';
  import StreamText from './chat/StreamText.svelte';
  import ChatOptions from './chat/ChatOptions.svelte';
  import DictateButton from '../lib/voice/DictateButton.svelte';
  import Attachments from './chat/Attachments.svelte';
  import QueuedCard from './chat/QueuedCard.svelte';
  import ExportMenu from './chat/ExportMenu.svelte';
  import RunChanges from './chat/RunChanges.svelte';
  import { MAX_FILE_BYTES, copyText, fmtSize, isImageName, notify, prettyName, readAsDataURL } from '../lib/content/util';
  import { imeKey } from '../lib/ime';
  import SetupCard from './SetupCard.svelte';
  import { foldMap } from '../lib/search';
  import { enqueue, restored, sendNowOrAfterStop, take, waiting as queuedBy, type Mention } from '../lib/chatQueue.svelte';
  import ApprovalCard from './ApprovalCard.svelte';
  import ChatAllowed from './ChatAllowed.svelte';

  let { id, projects }: { id: string; projects: any[] } = $props();


  let chat = $state<any>(null);
  let loadState = $state<'loading' | 'ok' | 'missing' | 'error'>('loading');
  let loadError = $state('');
  let streaming = $state('');
  let thinking = $state('');
  let text = $state('');
  let files = $state<Attachment[]>([]);
  let mentions = $state<Mention[]>([]);
  let opts = $state<any>({ models: [], efforts: [], permissionModes: [] });
  let mentionQ = $state<string | null>(null);
  let mentionIdx = $state(0);
  let openTools = $state<Record<string, boolean>>({});
  let stick = $state(true);      // user is at (or near) the bottom, so new output may scroll the view
  let unseen = $state(false);    // new output arrived while the user was reading further up
  let dragging = $state(false);
  let dragDepth = 0;
  let scroller = $state<HTMLElement>();
  let input = $state<HTMLTextAreaElement>();
  let escHint = $state(false);   // Esc was pressed once while the assistant is working
  let escTimer: ReturnType<typeof setTimeout> | undefined;
  let announce = $state('');     // one polite line for screen readers when an answer completes
  let announceTimer: ReturnType<typeof setTimeout> | undefined;

  // ---- draft: what was typed (and which files were waiting) survives chat switches and restarts ----
  {
    let d: any = null;
    try { d = JSON.parse(localStorage.getItem(draftKey(id)) || 'null'); } catch { /* no draft */ }
    if (d && typeof d === 'object') {
      text = String(d.text ?? '').slice(0, DRAFT_MAX);
      // (A draft from before mentions had names holds bare paths, shown as written.)
      const asMention = (x: any): Mention | null => typeof x === 'string' ? { path: x, label: x }
        : x && typeof x.path === 'string' ? { path: x.path, label: String(x.label || x.path) } : null;
      mentions = Array.isArray(d.mentions) ? d.mentions.map(asMention).filter((m: Mention | null): m is Mention => !!m).slice(0, 30) : [];
      const mem = pendingFiles.get(id) ?? [];
      const meta: any[] = Array.isArray(d.files) ? d.files.slice(0, 20) : [];
      files = meta.map((m) => mem.find((f) => f.name === m.name && f.size === m.size) ?? { name: String(m.name ?? ''), size: Number(m.size) || 0, image: !!m.image, data: '', missing: true });
    }
  }
  $effect(() => {
    const draft = { text: text.slice(0, DRAFT_MAX), mentions: [...mentions], files: files.map((f) => ({ name: f.name, size: f.size, image: f.image })) };
    const live = files.filter((f) => !f.missing).map((f) => ({ ...f }));
    if (live.length) pendingFiles.set(id, live); else pendingFiles.delete(id);
    try {
      if (draft.text || draft.mentions.length || draft.files.length) localStorage.setItem(draftKey(id), JSON.stringify(draft));
      else localStorage.removeItem(draftKey(id));
    } catch { /* storage full or unavailable: the draft just isn't kept */ }
  });

  // A mention lives only as long as its "@name" is still in the text.
  $effect(() => {
    const keep = mentions.filter((m) => text.includes('@' + m.label));
    if (keep.length !== mentions.length) mentions = keep;
  });

  // The box grows with the text up to about 40% of the window, then scrolls.
  function grow() {
    if (!input) return;
    input.style.height = 'auto';
    const max = Math.round(window.innerHeight * 0.4);
    input.style.height = Math.min(input.scrollHeight + 2, max) + 'px';
    input.style.overflowY = input.scrollHeight + 2 > max ? 'auto' : 'hidden';
  }
  $effect(() => { text; input; grow(); });

  function say(msg: string) {
    clearTimeout(announceTimer);
    announce = msg;
    announceTimer = setTimeout(() => (announce = ''), 4000);
  }

  // ---- the answer being written ----
  // The service numbers every piece of text within a run and keeps the pieces of the message in
  // progress, so this view can join in the middle of an answer: `load` brings what there is so
  // far, and events that arrive while it is fetching are kept and gone through again afterwards.
  let streamSeq = 0;          // number of the last piece in `streaming`
  let loads = 0;              // fetches of this chat on their way
  let heldEvents: any[] = []; // what arrived meanwhile

  function applyPartial(p: any) {
    if (!chat.running) { streaming = ''; thinking = ''; streamSeq = 0; return; }
    if (!p) return; // an older service does not send it: keep what is on screen
    streaming = String(p.text ?? ''); streamSeq = Number(p.seq) || 0; thinking = String(p.thinking ?? '');
  }

  async function load(initial = false) {
    if (initial) loadState = 'loading';
    loads++;
    try {
      const c = await api(`/chats/${id}`);
      c.options ??= {};
      // "Ask before changes" no longer exists: an older chat that stored it is read-only.
      if (c.options.permissionMode === 'default') c.options.permissionMode = 'plan';
      // A chat from before every chat stored its mode: it runs with accept-edits, so show that.
      if (!c.options.permissionMode) c.options.permissionMode = 'acceptEdits';
      savedPerm = c.options.permissionMode ?? null;
      // A mode picked while its warning is still on screen stays picked.
      if (pendingPerm !== undefined) c.options.permissionMode = pendingPerm;
      chat = c;
      loadOpts(c.provider);
      applyPartial(c.partial);
      // Files named in sent messages are shown by their titles.
      if (c.messages.some((m: any) => m.tool?.mentions?.length)) loadMentionables();
      // What arrived while this copy was on its way: anything it does not have yet is added.
      const again = heldEvents;
      heldEvents = [];
      for (const e of again) onEvent(e, true);
      loadState = 'ok';
      // Seen: the "cut off" marker stays on screen for this visit but no longer follows the chat around.
      if (c.interrupted && initial) post(`/chats/${id}/ack-interrupted`).catch(() => {});
      await scrollDown(initial);
      if (initial) {
        const missing = files.filter((f) => f.missing).length;
        if (missing) notify(tn('chat3.reattachNote', missing), { kind: 'info', key: 'reattach:' + id });
        // A message that was waiting to be sent when the app closed is back in the box, unsent.
        if (restored.delete(id)) notify(t('chatc.queue.restored'), { kind: 'info', key: 'requeue:' + id });
        // The cursor goes straight into the message box, so typing can start at once (the shell
        // announces the chat's title). Not while a dialog is open, nor away from where the user is typing.
        if (!['wrap', 'reorganize', 'setup'].includes(c.kind)) {
          await tick();
          const a = document.activeElement as HTMLElement | null;
          const elsewhere = !!a && a !== document.body && a !== input && !!a.closest('input, textarea, select, [contenteditable="true"]');
          if (!document.querySelector('[aria-modal="true"]') && !elsewhere) input?.focus({ preventScroll: true });
        }
      }
    } catch (e: any) {
      if (chat && !initial) return failed(t('chat3.refreshFailed'), e); // keep what is on screen; a refresh failed
      loadError = e?.message ?? String(e);
      loadState = e instanceof ApiError && e.status === 404 ? 'missing' : 'error';
    } finally {
      loads--;
      if (!loads) heldEvents = [];
    }
  }

  /** One event about this chat. `again`: it arrived while the chat was being fetched and is gone
   *  through a second time, now that the fetched copy is on screen. */
  function onEvent(e: any, again = false) {
    if (e.type === 'chat.message') {
      // A message that arrives again under the same id replaces the one shown: a failure the
      // service recovered from by itself becomes a quiet note.
      const at = chat.messages.findIndex((m: any) => m.id === e.message.id);
      // The fetched copy already had it. (It is only ever replaced by the note that follows a recovered failure, never the reverse.)
      if (at >= 0 && again) { if (e.message.tool?.code !== 'sessionLost') chat.messages[at] = e.message; return; }
      if (at >= 0) chat.messages[at] = e.message; else chat.messages.push(e.message);
      if (e.message.role === 'assistant') streaming = '';
      if (e.message.role !== 'user') thinking = '';
      if (e.message.role === 'user') chat.suggestion = null;
      scrollDown(e.message.role === 'user');
    } else if (e.type === 'chat.suggestion') {
      // What the user might say next, from the assistant program; offered under the answer.
      if (!again) chat.suggestion = e.text;
    } else if (e.type === 'chat.message.changed') {
      // A message that only changed state (an answer's changes undone or put back): replaced where it is.
      const at = chat.messages.findIndex((m: any) => m.id === e.message.id);
      if (at >= 0) chat.messages[at] = e.message;
    } else if (e.type === 'chat.message.removed') {
      // A message whose answer could not be started is taken back: its words are in the box again.
      chat.messages = chat.messages.filter((m: any) => m.id !== e.messageId);
    } else if (e.type === 'chat.delta') {
      const seq = typeof e.seq === 'number' ? e.seq : streamSeq + 1;
      if (seq === streamSeq + 1) { streaming += e.text; streamSeq = seq; }
      // Pieces in between never arrived: take the service's copy rather than show a text with holes.
      else if (seq > streamSeq + 1 && !again && !loads) load();
    } else if (e.type === 'chat.thinking') {
      if (!again) thinking += e.text;
    } else if (e.type === 'chat.done') {
      if (again && !chat.running) return; // the fetched copy is from after the end
      streaming = ''; thinking = ''; streamSeq = 0;
      escHint = false;
      if (!again) say(t('chat3.answerDone'));
      load();
    } else if (e.type === 'chat.updated') {
      if (again) return;
      // While a send is on its way the chat counts as working, whatever an update in between says.
      chat.running = e.chat.running || sending;
      chat.title = e.chat.title;
      // A wrap or reorganize says where it stands (preview, saving, ready to review).
      if (e.chat.phase && chat.meta && typeof chat.meta === 'object') chat.meta.phase = e.chat.phase;
    }
  }

  async function scrollDown(force = false) {
    // While the reader is selecting text in the conversation, nothing moves under the mouse.
    if (!force && (!stick || selecting())) { unseen = true; return; }
    await tick();
    if (scroller) { scroller.scrollTop = scroller.scrollHeight; lastTop = scroller.scrollTop; }
    stick = true; unseen = false;
  }

  /** Is part of the conversation selected (or being selected)? */
  function selecting(): boolean {
    const sel = window.getSelection();
    return !!sel && !sel.isCollapsed && !!scroller && !!sel.anchorNode && scroller.contains(sel.anchorNode);
  }

  // The view follows new text only while the reader is at the very bottom. Any move up lets go at
  // once — a wheel turn, a swipe, a key — so reading further up is never pulled back down; reaching
  // the bottom again takes hold again.
  let lastTop = 0;
  function onscroll() {
    if (!scroller) return;
    const top = scroller.scrollTop;
    const atBottom = scroller.scrollHeight - top - scroller.clientHeight < 8;
    if (atBottom) { stick = true; unseen = false; }
    else if (top < lastTop - 1) stick = false;
    lastTop = top;
  }
  function letGo() { if (scroller && scroller.scrollHeight > scroller.clientHeight) stick = false; }
  function onwheel(e: WheelEvent) { if (e.deltaY < 0) letGo(); }
  function onscrollkey(e: KeyboardEvent) { if (['ArrowUp', 'PageUp', 'Home'].includes(e.key)) letGo(); }
  let touchY = 0;
  function ontouchmove(e: TouchEvent) { const y = e.touches[0]?.clientY ?? 0; if (y > touchY) letGo(); touchY = y; }

  onMount(() => {
    load(true);
    window.addEventListener('resize', grow);
    const off = on((e) => {
      if (e.type === 'resync') { load(); return; }
      if (e.chatId !== id && e.chat?.id !== id) return;
      // While the chat is being fetched, events are kept: the copy on its way may or may not include them.
      if (loads) heldEvents.push(e);
      if (chat) onEvent(e);
    });
    return () => { off(); window.removeEventListener('resize', grow); clearTimeout(escTimer); clearTimeout(announceTimer); };
  });

  // ---- which assistant program answers this chat (Claude Code or Codex), and its own choices ----
  let optsFor = '';
  function loadOpts(provider: string) {
    const p = provider === 'codex' ? 'codex' : 'claude';
    if (optsFor === p) return;
    optsFor = p;
    api(`/ai/${p}/options`).then((o) => { if (optsFor === p) opts = { models: [], efforts: [], permissionModes: [], ...o }; }).catch(() => { if (optsFor === p) optsFor = ''; });
  }
  const onCodex = $derived(chat?.provider === 'codex');
  const PROGRAMS = [{ id: 'claude', label: 'Claude Code' }, { id: 'codex', label: 'Codex' }];
  /** The two programs, offered in the chat's control once Codex is installed (or the chat already uses it). */
  const programs = $derived(chat?.kind === 'chat' && (app.status?.codex?.installed || onCodex)
    ? PROGRAMS.map((p) => ({ ...p, ok: !!app.status?.[p.id]?.installed })) : []);

  /** Move this chat to the other assistant program. Its conversation goes along with the next
   *  message — whole when it fits, otherwise a summary with the last messages — and the chat says which. */
  async function moveProgram(to: string) {
    if (!chat || chat.provider === to || chat.running || sending) return;
    const name = PROGRAMS.find((p) => p.id === to)?.label ?? to;
    const said = chat.messages.some((m: any) => m.role === 'assistant');
    if (!(await confirmDialog({ title: t('codex.move.title', { to: name }), body: t(said ? 'codex.move.body' : 'codex.move.bodyEmpty', { to: name }), confirmLabel: t('codex.move.confirm') }))) return;
    try {
      const v = await patch(`/chats/${id}`, { provider: to });
      chat.provider = v?.provider ?? to;
      if (v?.options) { chat.options = { ...chat.options, ...v.options }; savedPerm = chat.options.permissionMode ?? savedPerm; }
      loadOpts(chat.provider);
      if (v?.handoff) notify(t(v.handoff === 'summary' ? 'codex.move.bodySummary' : 'codex.move.bodyReplay', { to: name }), { kind: 'info', key: 'move:' + id });
      refreshChats().catch(() => {});
    } catch (e) { failed(t('codex.move.failed'), e); }
  }

  let sending = false;   // a message is on its way to the service and its answer has not come back yet
  async function sendBody(body: any, failKey = 'chat3.sendFailed') {
    if (sending) return false;
    if (body.options?.permissionMode === 'default') body.options = { ...body.options, permissionMode: 'plan' };
    chat.running = true;
    sending = true;
    stick = true;
    try { await post(`/chats/${id}/send`, body); return true; }
    catch (e: any) {
      // The service refuses files in private folders (or ones that no longer exist) as mentions.
      if (e instanceof ApiError && e.status === 400 && body.mentions?.length) notify(t('layer.mentionRejected'), { kind: 'error', detail: e.message });
      else failed(t(failKey), e);
      // "Still working" means a run this view did not know about: ask the service how things stand.
      if (e instanceof ApiError && e.status === 409 && e.code === 'busy') load(); else chat.running = false;
      return false;
    } finally { sending = false; }
  }

  async function send() {
    if (!canSend || chat.running || sending) return;
    const keep = { text, files: [...files], mentions: [...mentions] };
    const ready = files.filter((f) => !f.missing);
    // Files may go on their own; the assistant still needs a sentence to answer. A quick-capture
    // prefix ("idea: …", «مهمة: …») only means a capture as the chat's first message: later in a
    // conversation it is just how a sentence starts, and is sent as typed.
    const first = !chat.messages.some((m: any) => m.role === 'user');
    const words = text.trim() ? (first ? normalizeCapture(text) : text) : tn('chat3.seeAttached', ready.length);
    const body = { text: words, attachments: ready.map(({ name, data }) => ({ name, data })), mentions: mentions.map((m) => m.path), options: chat.options };
    text = ''; files = []; mentions = []; mentionQ = null;
    // Nothing the user typed is lost when sending fails.
    if (!(await sendBody(body))) { text = keep.text; files = keep.files; mentions = keep.mentions; }
    else lastSent.set(id, { text: words, files: ready.map((f) => ({ ...f })) });
  }

  /** The suggested next message goes into the box (after anything typed), to send or change. */
  function useSuggestion() {
    const s = String(chat?.suggestion ?? '');
    if (!s) return;
    text = text.trim() ? text.replace(/\s*$/, '') + '\n' + s : s;
    tick().then(() => { input?.focus(); input?.setSelectionRange(text.length, text.length); });
  }

  // ---- the next message, written while the assistant is still answering ----
  const queued = $derived(queuedBy[id]);
  /** It waits, visible above the box, and goes as soon as the answer has finished. */
  function queueMsg() {
    if (!canSend) return;
    const ready = files.filter((f) => !f.missing);
    enqueue(id, {
      text: text.trim() ? text : tn('chat3.seeAttached', ready.length),
      files: ready.map(({ name, data, size, image }) => ({ name, data, size, image })),
      mentions: mentions.map((m) => ({ ...m })),
    });
    text = ''; files = files.filter((f) => f.missing); mentions = []; mentionQ = null;
    say(t('chatc.queue.waits'));
    input?.focus();
  }
  /** Back into the box, ahead of anything typed since. */
  function editQueued() {
    const q = take(id);
    if (!q) return;
    text = [q.text, text].filter((x) => x.trim()).join('\n\n');
    files = [...q.files, ...files];
    mentions = [...q.mentions, ...mentions.filter((m) => !q.mentions.some((x) => x.path === m.path))];
    input?.focus();
  }
  const canSend = $derived(!!text.trim() || files.some((f) => !f.missing));

  // (In a chat that began as first-run setup, the setup's own "Set up my library." is never sent again.)
  const lastUser = $derived(chat ? chat.messages.slice(Number(chat.meta?.doneAt) || 0).reverse().find((m: any) => m.role === 'user') : null);
  const lastAnswerId = $derived(chat ? [...chat.messages].reverse().find((m: any) => m.role === 'assistant' || m.role === 'error')?.id : null);
  const canCompose = $derived(!!chat && !['wrap', 'reorganize', 'setup'].includes(chat.kind));

  async function retry() {
    if (!lastUser || chat.running || sending) return;
    // The files of that message go again too (they are kept in memory for this session).
    const prev = lastSent.get(id);
    const again = prev && prev.text === lastUser.text ? prev.files : [];
    if (!again.length && lastUser.attachments?.length) notify(t('chat3.retryNoFiles'), { kind: 'info' });
    await sendBody({ text: lastUser.text, attachments: again.map(({ name, data }) => ({ name, data })), mentions: lastUser.tool?.mentions ?? [], options: chat.options }, 'chat3.retryFailed');
  }

  async function stop() {
    escHint = false;
    try { await post(`/chats/${id}/stop`); } catch (e) { failed(t('chat3.stopFailed'), e); }
  }

  async function copyMsg(s: string) {
    const ok = await copyText(s);
    notify(t(ok ? 'chat.copied' : 'chat.copyFailed'), { kind: ok ? 'info' : 'error' });
  }

  function keydown(e: KeyboardEvent) {
    if (mentionHits.length) {
      if (e.key === 'ArrowDown') { e.preventDefault(); mentionIdx = (mentionIdx + 1) % mentionHits.length; return; }
      if (e.key === 'ArrowUp') { e.preventDefault(); mentionIdx = (mentionIdx - 1 + mentionHits.length) % mentionHits.length; return; }
      if ((e.key === 'Enter' || e.key === 'Tab') && !imeKey(e)) { e.preventDefault(); pickMention(mentionHits[mentionIdx]); return; }
    }
    if (e.key === 'Escape') {
      if (mentionQ !== null) { e.preventDefault(); mentionQ = null; }
      else if (chat?.running) {
        // One stray Esc must not throw away an answer: the second press within 2 seconds stops it.
        e.preventDefault();
        if (escHint) { clearTimeout(escTimer); stop(); }
        else { escHint = true; clearTimeout(escTimer); escTimer = setTimeout(() => (escHint = false), 2000); }
      }
      return;
    }
    // Enter sends, Shift+Enter makes a new line (Cmd/Ctrl+Enter also sends). Never while an IME is composing.
    // While an answer is being written, Enter puts the message in line for when it has finished.
    if (e.key === 'Enter' && !e.shiftKey && !imeKey(e)) { e.preventDefault(); if (chat?.running || sending) queueMsg(); else send(); }
  }

  // ---- @-mentions: ideas, notes, other files and folders, found by the name people know them by ----
  type Mentionable = { path: string; kind: string; label: string; tf: string; pf: string };
  let mentionables = $state<Mentionable[]>([]);
  let mentionablesAt = 0;
  const fold = (s: string) => foldMap(s).s;
  /** Fetched when "@" is first typed, and again when the list is older than half a minute. */
  function loadMentionables() {
    if (Date.now() - mentionablesAt < 30_000) return;
    mentionablesAt = Date.now();
    api('/library/mentions').then((x: any[]) => {
      mentionables = x.map((m) => {
        const label = String(m.title || prettyName(m.path)).replace(/\s+/g, ' ').trim() || m.path;
        return { path: m.path, kind: m.kind, label, tf: fold(label), pf: fold(m.path) };
      });
    }).catch(() => { mentionablesAt = 0; });
  }
  // "@" at the start or after a space, then what is typed so far (spaces allowed: titles have them).
  const MENTION_AT = /(?:^|[\s(\[«"'])@([^\s@][^@\n]{0,39})?$/u;
  function oninput() {
    const m = text.slice(0, input!.selectionStart).match(MENTION_AT);
    mentionQ = m ? (m[1] ?? '') : null;
    if (mentionQ !== null) loadMentionables();
    mentionIdx = 0;
  }

  function pickMention(h: Mentionable) {
    const pos = input!.selectionStart;
    const before = text.slice(0, pos).replace(/@([^\s@][^@\n]{0,39})?$/u, '@' + h.label + ' ');
    text = before + text.slice(pos);
    if (!mentions.some((m) => m.path === h.path)) mentions.push({ path: h.path, label: h.label });
    mentionQ = null;
    tick().then(() => { input?.focus(); input?.setSelectionRange(before.length, before.length); });
  }

  function dropMention(m: Mention) {
    text = text.split('@' + m.label + ' ').join('').split('@' + m.label).join('');
    mentions = mentions.filter((x) => x.path !== m.path);
    input?.focus();
  }

  const KIND_RANK: Record<string, number> = { idea: 0, note: 1, folder: 2, file: 3 };
  const mentionIcon = (k: string) => (k === 'idea' ? 'ideas' : k === 'note' ? 'docs' : k === 'folder' ? 'folder' : 'attach');
  // Ideas first, then notes, folders and other files; a name that starts with what was typed before
  // one that only contains it, and either before a match on where the file is.
  const mentionHits = $derived.by(() => {
    if (mentionQ === null) return [];
    const q = fold(mentionQ);
    const hits: { h: Mentionable; s: number }[] = [];
    for (const h of mentionables) {
      const s = !q || h.tf.startsWith(q) ? 0 : h.tf.includes(q) ? 1 : h.pf.includes(q) ? 2 : -1;
      if (s >= 0) hits.push({ h, s: s * 10 + (KIND_RANK[h.kind] ?? 3) });
    }
    return hits.sort((a, b) => a.s - b.s || a.h.label.length - b.h.label.length).slice(0, 8).map((x) => x.h);
  });
  /** A mentioned file's name in a sent message: its title when the list has it. */
  const mentionName = (p: string) => mentionables.find((m) => m.path === p)?.label ?? prettyName(p);

  async function addFiles(list: Iterable<File>) {
    for (const f of list) {
      if (f.size > MAX_FILE_BYTES) { notify(t('chat.tooLarge', { name: f.name, size: fmtSize(f.size) }), { kind: 'error' }); continue; }
      const name = f.name || t('chat.pastedName');
      // One message carries 50 MB of files at most. (A file that fills a placeholder is not counted twice.)
      const waiting = files.reduce((sum, x) => sum + (x.missing ? 0 : x.size), 0);
      if (waiting + f.size > MAX_TOTAL_BYTES) { notify(t('chata.tooLargeTogether', { name }), { kind: 'error', key: 'attach-total' }); continue; }
      try {
        const data = await readAsDataURL(f);
        const att = { name, data, size: f.size, image: f.type.startsWith('image/') || isImageName(name) };
        // Attaching a file again fills the placeholder a restored draft left for it.
        const gap = files.findIndex((x) => x.missing && x.name === name);
        if (gap >= 0) files[gap] = att; else files.push(att);
      } catch { notify(t('chat.readFailed', { name: f.name }), { kind: 'error' }); }
    }
  }

  async function attach(e: Event) {
    const el = e.target as HTMLInputElement;
    await addFiles(el.files || []);
    el.value = '';
  }

  function onpaste(e: ClipboardEvent) {
    const fs = [...(e.clipboardData?.files || [])];
    if (!fs.length) return;
    e.preventDefault();
    addFiles(fs);
  }

  const hasFiles = (e: DragEvent) => [...(e.dataTransfer?.types || [])].includes('Files');
  function dragenter(e: DragEvent) { if (!canCompose || !hasFiles(e)) return; e.preventDefault(); dragDepth++; dragging = true; }
  function dragover(e: DragEvent) { if (canCompose && hasFiles(e)) e.preventDefault(); }
  function dragleave(e: DragEvent) { if (!hasFiles(e)) return; dragDepth = Math.max(0, dragDepth - 1); if (!dragDepth) dragging = false; }
  function drop(e: DragEvent) {
    if (!canCompose || !hasFiles(e)) return;
    e.preventDefault();
    dragDepth = 0; dragging = false;
    addFiles(e.dataTransfer?.files || []);
    input?.focus();
  }

  async function saveOpts(confirmBypass = false): Promise<boolean> {
    try { await patch(`/chats/${id}`, { options: chat.options, confirmBypass }); return true; }
    catch (e) { failed(t('chat3.optionsFailed'), e); return false; }
  }

  // The model menu lists what the assistant program and past answers name; "Other…" takes any name
  // the person knows (a full model name, a service's own).
  const MODEL_OTHER = '__other__';
  async function modelChanged(e: Event & { currentTarget: HTMLSelectElement }) {
    const el = e.currentTarget, v = el.value, before = chat.options.model ?? '';
    if (v !== MODEL_OTHER) { chat.options.model = v || null; await saveOpts(); return; }
    const typed = (await promptDialog({ title: t('runc.model.otherTitle'), body: t('runc.model.otherBody'), placeholder: t('runc.model.otherPlaceholder'), confirmLabel: t('save') }))?.trim();
    el.value = before; // until a name is accepted the menu shows what was there
    if (!typed) return;
    if (!/^[A-Za-z0-9][A-Za-z0-9._:/@\[\]-]{0,119}$/.test(typed)) { notify(t('runc.model.otherBad'), { kind: 'error', key: 'model-name' }); return; }
    chat.options.model = typed;
    if (await saveOpts()) el.value = typed;
  }

  // The permission mode as the service has it. "Bypass permissions" is only switched on after the
  // user agreed to the warning (the service refuses it otherwise).
  const BYPASS = 'bypassPermissions';
  let savedPerm = $state<string | null>(null);
  let pendingPerm: string | null | undefined; // picked, with its warning still on screen
  async function permChanged() {
    const want: string | null = chat.options.permissionMode ?? null;
    const turnsOn = want === BYPASS && savedPerm !== BYPASS;
    if (turnsOn) {
      pendingPerm = want;
      const yes = await confirmDialog({
        title: t('perm.bypass.title'), body: `${t(onCodex ? 'codex.bypass.body' : 'perm.bypass.body')}\n\n${t('perm.bypass.chat')}`,
        note: t(onCodex ? 'codex.bypass.note' : 'perm.bypass.note'), confirmLabel: t(onCodex ? 'codex.perm.bypassPermissions' : 'perm.bypass.confirm'), danger: true,
      });
      pendingPerm = undefined;
      if (!yes) { chat.options.permissionMode = savedPerm; return; }
      // The chat may have been fetched afresh while the warning was up (an answer finished):
      // what gets saved is the mode that was confirmed, not whatever the fresh copy holds.
      chat.options.permissionMode = want;
    }
    try {
      const saved = await patch(`/chats/${id}`, { options: chat.options, confirmBypass: turnsOn });
      savedPerm = saved?.options?.permissionMode ?? want;
    } catch (e) { failed(t('chat3.optionsFailed'), e); }
    // Whatever happened, the picker shows the mode the service has.
    chat.options.permissionMode = savedPerm;
  }

  /** From the warning banner: back to the ordinary mode, without needing the picker. */
  async function bypassOff() {
    chat.options.permissionMode = 'acceptEdits';
    await permChanged();
  }

  /** Web lookups for this chat: an ordinary chat's own choice, off until switched on. */
  async function setWeb(on: boolean) {
    const before = !!chat.options.web;
    // Switching it on says first, plainly, what it lets in and what it may send out.
    if (on && !before && !(await confirmDialog({ title: t('runb.web.confirmTitle'), body: t(onCodex ? 'codex.web.confirmBody' : 'runb.web.confirmBody'), confirmLabel: t('runb.web.confirm') }))) return;
    chat.options.web = on;
    try {
      const saved = await patch(`/chats/${id}`, { options: chat.options });
      chat.options.web = !!saved?.options?.web;
    } catch (e) { chat.options.web = before; failed(t('runb.web.failed'), e); }
  }

  /** One thing the assistant was not allowed to do, in plain words. What it named (a command, an
   *  address, a file) is kept apart from the sentence around it, so it reads the right way in Arabic too. */
  const DENIED = ['command', 'page', 'search', 'open', 'look', 'change', 'connected', 'tool', 'restored'];
  const ISOLATE = [String.fromCharCode(0x2068), String.fromCharCode(0x2069)]; // first-strong isolate … pop
  function deniedText(d: any): string {
    if (d?.why === 'appData') return t('runb.denied.appData');
    const what = DENIED.includes(d?.what) ? d.what : 'tool';
    const target = String(d?.target || (what === 'tool' ? d?.tool ?? '' : ''));
    if (!target) return t(['open', 'look', 'change'].includes(what) ? `runb.denied.${what}.none` : 'runb.denied.tool', { target: '' });
    return t(`runb.denied.${what}`, { target: ISOLATE[0] + target + ISOLATE[1] });
  }

  async function moveTo(pid: string) {
    try {
      await patch(`/chats/${id}`, { projectId: pid || null });
      chat.projectId = pid || null;
      refreshChats().catch(() => {});
    } catch (e) { failed(t('chat3.moveFailed'), e); }
  }

  async function rename() {
    const title = (await promptDialog({ title: t('chat.renameTitle'), value: chat.title, placeholder: t('chat.renamePlaceholder'), confirmLabel: t('save') }))?.trim();
    if (!title || title === chat.title) return;
    try { await patch(`/chats/${id}`, { title }); chat.title = title; refreshChats().catch(() => {}); } catch (e) { failed(t('chat3.renameFailed'), e); }
  }

  // Pin the chat to the top of the lists, or put it away (archived: out of the side list, still kept).
  async function flag(key: 'pinned' | 'archived') {
    try {
      const v = await patch(`/chats/${id}`, { [key]: !chat[key] });
      chat[key] = v[key];
      refreshChats().catch(() => {});
      if (key === 'archived' && v.archived) { notify(t('chatb.archivedToast'), { kind: 'info' }); go({ kind: 'chats' }); }
    } catch (e) { failed(t('chatb.failChange'), e); }
  }

  async function remove() {
    if (!(await confirmDialog({ title: t('chat.deleteTitle'), body: t('chata.deleteBody'), confirmLabel: t('delete'), danger: true }))) return;
    try {
      // A chat that is already gone (deleted in another window) counts as deleted.
      await del(`/chats/${id}`).catch((e) => { if (!(e instanceof ApiError && e.status === 404)) throw e; });
      try { localStorage.removeItem(draftKey(id)); } catch { /* ignore */ }
      pendingFiles.delete(id); lastSent.delete(id);
      await refreshChats().catch(() => {});
      go({ kind: 'chats' });
    } catch (e) { failed(t('chat3.deleteFailed'), e); }
  }

  const phase = $derived(chat?.meta?.phase as string | undefined);

  const resultFor = $derived.by(() => {
    const map: Record<string, any> = {};
    for (const m of chat?.messages ?? []) if (m.role === 'tool_result' && m.tool?.toolUseId) map[m.tool.toolUseId] = m;
    return map;
  });
  const toolKey = (m: any) => String(m.tool?.id ?? m.id);

  /** Translated label when we have one, the raw value otherwise (models, future modes). */
  const label = (prefix: string, v: string) => { const k = `${prefix}.${v}`; const s = t(k); return s === k ? v : s; };
  const usd = fmtMoney;
  // Claude Code's modes, as far as the service allows them. Bypass is for ordinary chats only.
  const permModes = $derived((opts.permissionModes as string[]).filter((m) => m === 'acceptEdits' || m === 'plan' || (m === BYPASS && chat?.kind === 'chat')));
</script>

{#if loadState === 'loading'}
  <div class="wrap" aria-busy="true">
    <span class="sr" role="status">{t('chat.loading')}</span>
    <div class="skel line title-sk"></div>
    <div class="skel-col">
      <div class="skel bubble-sk end"></div>
      <div class="skel line w80"></div><div class="skel line w60"></div><div class="skel line w70"></div>
      <div class="skel bubble-sk end short"></div>
      <div class="skel line w60"></div><div class="skel line w40"></div>
    </div>
  </div>
{:else if loadState !== 'ok' || !chat}
  <div class="wrap state" role="alert">
    <Icon name={loadState === 'missing' ? 'chats' : 'warning'} size={28} />
    <h2>{t(loadState === 'missing' ? 'chat.missingTitle' : 'chat.errorTitle')}</h2>
    <p class="muted">{t(loadState === 'missing' ? 'chat.missingBody' : 'chat.errorBody')}</p>
    {#if loadState === 'error' && loadError}<p class="muted detail" dir="auto">{loadError}</p>{/if}
    <div class="row">
      <button class="btn" onclick={() => go({ kind: 'chats' })}><Icon name="arrow" size={14} class="flip-rtl back" /> {t('chat.backToChats')}</button>
      {#if loadState === 'error'}<button class="btn primary" onclick={() => load(true)}><Icon name="retry" size={14} /> {t('chat.tryAgain')}</button>{/if}
    </div>
  </div>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="wrap" ondragenter={dragenter} ondragover={dragover} ondragleave={dragleave} ondrop={drop}>
    <header class="row">
      <h1 class="h grow"><button class="title ell" onclick={rename} title={t('chat.rename')} aria-label={t('chat.renameAria', { title: chat.title })} dir="auto">{chat.title}</button></h1>
      {#if chat.kind === 'chat'}
        <select value={chat.projectId ?? ''} onchange={(e) => moveTo((e.target as HTMLSelectElement).value)} title={t('chat.group')} aria-label={t('chat.group')}>
          <option value="">{t('library')}</option>
          {#each projects as p}<option value={p.id}>{p.name}</option>{/each}
        </select>
      {:else}
        <span class="pill accent">{label('chat.kind', chat.kind)}</span>
      {/if}
      {#if chat.usageTokens > 0 && !(chat.costUsd > 0)}
        <span class="pill cost" title={t('codex.chat.tokensTip')}><bdi>{t('codex.chat.tokens', { n: fmtNum(chat.usageTokens) })}</bdi></span>
      {/if}
      {#if chat.costUsd > 0 && costVisible()}
        <button class="pill cost" type="button" title={t('chat.costTip')} aria-label="{t('chat.costLabel', { cost: usd(chat.costUsd) })}. {t('chat.costTip')}"
          onclick={() => notify(t('chat.costTip'), { kind: 'info' })}><bdi>{usd(chat.costUsd)}</bdi> <Icon name="info" size={12} /></button>
      {/if}
      {#if chat.kind === 'chat'}
        <button class="btn ghost" class:on={chat.pinned} aria-pressed={!!chat.pinned} onclick={() => flag('pinned')} title={chat.pinned ? t('chatb.unpin') : t('chatb.pin')} aria-label={chat.pinned ? t('chatb.unpin') : t('chatb.pin')}><Icon name="pin" /></button>
        <button class="btn ghost" onclick={() => flag('archived')} title={t('chatb.archive')} aria-label={t('chatb.archive')}><Icon name="archive" /></button>
      {/if}
      <ExportMenu {chat} />
      <button class="btn ghost" onclick={remove} title={t('chat.deleteChat')} aria-label={t('chat.deleteChat')}><Icon name="trash" /></button>
    </header>

    {#if chat.kind === 'customize'}
      <div class="banner row">
        <span class="grow">{t('layer.chatBanner')}</span>
        {#if app.pending?.exists && !app.pending.running}<button class="btn primary" onclick={openReview}>{t('layer.review')}</button>{/if}
      </div>
    {/if}
    {#if chat.interrupted}
      <div class="banner row" role="status"><Icon name="warning" size={14} /> <span class="grow">{t('layer.chat.interrupted')}</span></div>
    {/if}
    <!-- Always on screen while every permission check is off: it does not depend on the picker below being there. -->
    {#if savedPerm === BYPASS}
      <div class="banner alert row" role="status"><Icon name="warning" size={15} /> <span class="grow">{t('chata.bypassOn')}</span>
        <button class="btn" onclick={bypassOff}>{t('chata.bypassOff')}</button></div>
    {/if}
    {#if onCodex && (app.settings?.privatePaths ?? []).length}
      <div class="banner alert row" role="note"><Icon name="warning" size={15} /> <span class="grow">{t('codex.chat.private')}</span>
        {#if chat.kind === 'chat' && app.status?.claude?.installed}<button class="btn" onclick={() => moveProgram('claude')}>{t('codex.chat.privateMove')}</button>{/if}</div>
    {/if}
    {#if chat.kind === 'chat' && chat.options.web && savedPerm !== BYPASS}
      <div class="banner row" role="status"><Icon name="search" size={14} /> <span class="grow">{t(onCodex ? 'codex.web.on' : 'runb.web.on')}</span>
        <button class="btn" onclick={() => setWeb(false)}>{t('runb.web.off')}</button></div>
    {/if}
    {#if chat.kind === 'chat'}<ChatAllowed chatId={id} />{/if}

    <div class="msgs-wrap">
      <FindBar root={() => scroller} where={'chat:' + id} anchors=":scope > .msg.user, :scope > .msg.ai" />
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="msgs scroll" bind:this={scroller} {onscroll} {onwheel} onkeydown={onscrollkey} ontouchstart={(e) => (touchY = e.touches[0]?.clientY ?? 0)} {ontouchmove}
        role="group" aria-label={t('chat.conversation')}>
        {#each chat.messages as m (m.id)}
          {#if m.role === 'user'}
            <div class="msg user">
              <div class="bubble" dir="auto">{m.text}
                {#if m.attachments?.length}<Attachments chatId={id} paths={m.attachments} />{/if}{#if m.tool?.mentions?.length}<div class="sm att refs">{#each m.tool.mentions as p (p)}<button type="button" class="ref" onclick={() => go({ kind: 'docs', path: p })} title={t('chatc.mention.open', { path: p })}>@<bdi>{mentionName(p)}</bdi></button>{/each}</div>{/if}
              </div>
              <div class="acts">
                <button class="act" onclick={() => copyMsg(m.text)} aria-label={t('chat.copyMessage')} title={t('chat.copyMessage')}><Icon name="copy" size={14} /></button>
              </div>
            </div>
          {:else if m.role === 'assistant'}
            <div class="msg ai">
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <div class="md" dir="auto" onclick={mdClick}>{@html md(m.text)}</div>
              <div class="acts" class:show={m.id === lastAnswerId}>
                <button class="act" onclick={() => copyMsg(m.text)} aria-label={t('chat.copyMessage')} title={t('chat.copyMessage')}><Icon name="copy" size={14} /><span>{t('chat.copy')}</span></button>
                {#if m.id === lastAnswerId && !chat.running && lastUser && canCompose}
                  <button class="act" onclick={retry} title={t('chat.retryTip')}><Icon name="retry" size={14} /><span>{t('chat.retry')}</span></button>
                {/if}
              </div>
            </div>
          {:else if m.role === 'tool'}
            {@const k = toolKey(m)}
            {@const res = resultFor[k]}
            <div class="toolrow">
              <button class="tool" onclick={() => (openTools[k] = !openTools[k])} aria-expanded={!!openTools[k]} title={t(openTools[k] ? 'chat.hideDetails' : 'chat.showDetails')}>
                <Icon name="tool" size={13} /> <span class="ell" dir="auto">{m.text}</span>
                {#if res?.tool?.isError}<span class="pill bad">{t('chat.toolFailed')}</span>{/if}
                <Icon name={openTools[k] ? 'down' : 'right'} size={13} class={openTools[k] ? '' : 'flip-rtl'} />
              </button>
              {#if openTools[k]}
                <div class="tooldetail">
                  <div class="eyebrow">{t('chat.toolInput')}</div>
                  <pre class="sm" dir="ltr">{JSON.stringify(m.tool?.input, null, 2)}</pre>
                  <div class="eyebrow">{t('chat.toolResult')}</div>
                  {#if res}<pre class="sm" dir="auto">{res.text || t('chat.toolEmpty')}</pre>
                  {:else}<p class="muted sm">{t(chat.running ? 'chat.toolPending' : 'chat.toolNoResult')}</p>{/if}
                </div>
              {/if}
            </div>
          {:else if m.role === 'tool_result'}
            <!-- shown inside its tool call above -->
          {:else if m.role === 'approval'}
            <ApprovalCard {m} chatId={id} running={!!chat.running} />
          {:else if m.role === 'changes'}
            <!-- What this answer changed in the library, with Undo (FEAT-06). -->
            <RunChanges {m} chatId={id} busy={!!chat.running} onchange={(x) => { const i = chat.messages.findIndex((y: any) => y.id === x.id); if (i >= 0) chat.messages[i] = x; }} />
          {:else if m.role === 'denied'}
            <div class="msg denied" role="note">
              <div class="deniedhead"><Icon name="shield" size={13} /> <span>{t('runb.denied.title')}</span></div>
              {#if m.tool?.denials?.length}
                <ul>
                  {#each m.tool.denials as d}
                    <li><span dir="auto">{deniedText(d)}</span>{#if d.why !== 'appData'}<span class="muted why" dir="auto">{t('runb.why.' + (d.why || 'none'))}</span>{/if}</li>
                  {/each}
                </ul>
                {#if chat.kind === 'chat' && !chat.options.web && savedPerm !== BYPASS && m.tool.denials.some((d: any) => d.why === 'webOff')}
                  <button class="act show" onclick={() => setWeb(true)}><Icon name="search" size={14} /><span>{t('runb.web.allowHere')}</span></button>
                {/if}
              {:else}<div class="sm" dir="auto">{m.text}</div>{/if}
            </div>
          {:else if m.role === 'error' && m.tool?.code !== 'sessionLost'}
            <div class="msg err" role="alert">
              <div class="errline"><Icon name="warning" size={14} /> <span dir="auto">{m.text}</span></div>
              {#if m.tool?.code === 'signedOut' && m.id === lastAnswerId}
                <button class="act show" onclick={() => signIn(m.tool?.provider === 'codex' ? 'codex' : 'claude')}><Icon name="external" size={14} /><span>{t(m.tool?.provider === 'codex' ? 'codex.set.signIn' : 'auth.signIn')}</span></button>
              {:else if m.tool?.code === 'providerKey' && m.id === lastAnswerId}
                <!-- The service refused the saved key: it is changed in Settings, under Assistant. -->
                <button class="act show" onclick={() => go({ kind: 'settings', id: 'ai' })}><Icon name="settings" size={14} /><span>{t('chatc.openSettings')}</span></button>
              {/if}
              {#if m.id === lastAnswerId && !chat.running && lastUser && canCompose}
                <button class="act show" onclick={retry}><Icon name="retry" size={14} /><span>{t('chat.tryAgain')}</span></button>
              {/if}
            </div>
          {:else}
            <div class="msg sys muted" dir="auto">{m.text}</div>
          {/if}
        {:else}
          {#if !chat.running}<div class="hint muted"><Icon name="chat" size={22} /><span>{t('chat.emptyHint')}</span></div>{/if}
        {/each}
        {#if chat.suggestion && !chat.running && canCompose}
          <div class="suggest"><button type="button" class="sugg" onclick={useSuggestion} title={t('chatc.suggest.tip')} aria-label={t('chatc.suggest.aria', { text: chat.suggestion })}>
            <Icon name="wrap" size={13} /><span dir="auto">{chat.suggestion}</span></button></div>
        {/if}
        {#if thinking}<div class="msg sys muted ell think"><Icon name="thinking" size={13} /> <span class="ell" dir="auto">{thinking.slice(-160)}</span></div>{/if}
        {#if streaming}
          <div class="msg ai" aria-busy="true"><StreamText text={streaming} onrender={() => scrollDown()} /></div>
        {/if}
        {#if chat.running && !streaming}<div class="msg sys muted working" role="status"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span> {t('chat.working')}</div>{/if}
      </div>
      <span class="sr" role="status" aria-live="polite">{announce}</span>
      {#if !stick}
        <button class="jump btn" onclick={() => scrollDown(true)}><Icon name="arrowDown" size={14} /> {t(unseen ? 'chat.jumpNew' : 'chat.jumpLatest')}</button>
      {/if}
    </div>

    {#if chat.kind === 'wrap'}<WrapPanel {chat} {id} reload={() => load()} />{/if}
    {#if chat.kind === 'reorganize' && !chat.running && (phase === 'interrupted' || phase === 'failed')}
      <div class="approve card row" role="alert"><Icon name="warning" size={15} /> <span class="grow">{t(phase === 'failed' ? 'layer.reorg.failed' : 'layer.reorg.interrupted')}</span></div>
    {/if}
    {#if chat.kind === 'reorganize' && !chat.running && chat.meta?.phase === 'review'}
      <div class="approve card row"><span class="grow">{t('chat.reorgReady')}</span>
        <button class="btn primary" onclick={() => go({ kind: 'reorganize', id })}>{t('chat.reviewChanges')}</button></div>
    {/if}

    <!-- Wrap, Reorganize and Setup have no message box, so their Stop lives here (setup's is on its own card below). -->
    {#if !canCompose && chat.running && chat.kind !== 'setup'}
      <div class="runbar card row" role="status">
        <span class="dots" aria-hidden="true"><i></i><i></i><i></i></span>
        <span class="grow">{t(chat.kind === 'wrap' ? (phase === 'applying' ? (chat.meta?.inCopy ? 'wrapb.run.writing' : 'chata.run.wrapApply') : 'chata.run.wrapPreview') : chat.kind === 'reorganize' ? 'chata.run.reorganize' : 'chata.run.setup')}</span>
        <button class="btn danger" onclick={stop} title={t('chata.stopRun')}><Icon name="stop" size={13} /> {t('stop')}</button>
      </div>
    {/if}

    {#if chat.kind === 'setup' || chat.meta?.setup}
      <!-- First run: setup at work, setup that did not finish, or "you're set up" with a reply ready to send. -->
      <SetupCard {chat} {stop} reload={() => load()} reply={(s) => { if (!text.trim()) text = s; tick().then(() => input?.focus()); }} />
    {/if}
    {#if canCompose}
      <div class="composer card">
        {#if mentionHits.length}
          <div class="mentions" role="listbox" id="mention-list" aria-label={t('chat.mentionList')}>
            {#each mentionHits as h, i (h.path)}
              <button type="button" role="option" id="mention-{i}" aria-selected={i === mentionIdx} class:sel={i === mentionIdx} tabindex="-1"
                onmousedown={(e) => e.preventDefault()} onclick={() => pickMention(h)} onmouseenter={() => (mentionIdx = i)}>
                <Icon name={mentionIcon(h.kind)} size={14} />
                <span class="mt"><bdi class="ell">{h.label}</bdi><bdi class="ell mp" dir="ltr">{h.path}</bdi></span>
              </button>
            {/each}
          </div>
        {/if}
        {#if files.length}
          <ul class="files">
            {#each files as f, i (f.name + i)}
              <li class="file" class:gone={f.missing} title={f.missing ? t('chat3.reattachTip') : undefined}>
                {#if f.image && !f.missing}<img src={f.data} alt="" />{:else}<span class="ficon"><Icon name={f.missing ? 'warning' : 'docs'} size={16} /></span>{/if}
                <span class="fmeta"><bdi class="ell fname">{f.name}</bdi><span class="muted">{f.missing ? t('chat3.reattach') : fmtSize(f.size)}</span></span>
                <button class="x" type="button" onclick={() => files.splice(i, 1)} aria-label={t('chat.removeFile', { name: f.name })} title={t('chat.removeFile', { name: f.name })}><Icon name="close" size={13} /></button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if mentions.length}
          <ul class="files chips" aria-label={t('chat3.mentioned')}>
            {#each mentions as m (m.path)}
              <li class="chip" title={m.path}><span aria-hidden="true">@</span><bdi class="ell">{m.label}</bdi>
                <button class="x" type="button" onclick={() => dropMention(m)} aria-label={t('chat3.removeMention', { path: m.label })} title={t('chat3.removeMention', { path: m.label })}><Icon name="close" size={13} /></button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if queued}
          <QueuedCard q={queued} running={chat.running} onedit={editQueued} oncancel={() => take(id)} onsend={() => sendNowOrAfterStop(id, chat.running)} />
        {/if}
        <textarea bind:this={input} bind:value={text} {oninput} onkeydown={keydown} {onpaste} rows="2" dir="auto"
          placeholder={chat.running ? t('chatc.queue.placeholder') : t('chatc.placeholder')} aria-label={t('chat.messageLabel')}
          aria-controls={mentionHits.length ? 'mention-list' : undefined} aria-activedescendant={mentionHits.length ? `mention-${mentionIdx}` : undefined}></textarea>
        <div class="row controls">
          <label class="btn ghost" title={t('chat.attachTip')}><Icon name="attach" /><span class="sr">{t('chat.attachTip')}</span><input type="file" multiple class="sr" onchange={attach} /></label>
          <DictateButton target={() => input} />
          <!-- Model, effort and what the assistant may do: one control; the mode is always named on it. -->
          <ChatOptions options={chat.options} models={opts.models} efforts={opts.efforts} modes={permModes} {label}
            permPrefix={onCodex ? 'codex.perm' : 'chat3.perm'} hintPrefix={onCodex ? 'codex.perm' : 'set.perm'} effortPrefix={onCodex ? 'codex.effort' : 'chat.effort'}
            {programs} program={chat.provider} onprogram={programs.every((p) => p.ok) && !chat.running ? moveProgram : undefined}
            onmodel={modelChanged} extraModel={{ value: MODEL_OTHER, label: t('runc.model.other') }} oneffort={(v) => { chat.options.effort = v; saveOpts(); }}
            onperm={(v) => { chat.options.permissionMode = v; permChanged(); }} />
          {#if chat.kind === 'chat' && savedPerm !== BYPASS}
            <label class="btn ghost webtoggle" class:on={!!chat.options.web} title={t('runb.web.tip')}>
              <input type="checkbox" checked={!!chat.options.web} onchange={async (e) => { const box = e.currentTarget; await setWeb(box.checked); box.checked = !!chat.options.web; }} />
              <Icon name="search" size={14} /><span>{t('runb.web.label')}</span>
            </label>
          {/if}
          <span class="grow"></span>
          <span class="esc muted" role="status" aria-live="polite">{#if escHint}{t('chat3.escHint')}{/if}</span>
          {#if chat.running}
            {#if canSend}<button class="btn" onclick={queueMsg} title={t('chatc.queue.tip')}><Icon name="clock" size={13} /> {t('chatc.queue.add')}</button>{/if}
            <button class="btn danger" onclick={stop} title={t('chat.stopTip')}><Icon name="stop" size={13} /> {t('stop')}</button>
          {:else}
            <button class="btn primary send" onclick={send} disabled={!canSend} title={t('chat.sendTip')} aria-label={t('send')}><Icon name="send" size={16} stroke={2.25} /></button>
          {/if}
        </div>
      </div>
    {/if}

    {#if dragging}<div class="dropzone" aria-hidden="true"><Icon name="upload" size={26} /><span>{t('chat.dropHere')}</span></div>{/if}
  </div>
{/if}

<style>
  .wrap { position: relative; display: flex; flex-direction: column; height: calc(100vh - 26px); padding: 0 24px 16px; max-width: 980px; margin: 0 auto; }
  header { padding-bottom: 8px; border-bottom: var(--border-w) solid var(--border); }
  header select { width: auto; }
  .h { margin: 0; font-size: inherit; font-weight: inherit; letter-spacing: 0; line-height: inherit; min-width: 0; display: flex; border-radius: 6px; }
  .title { max-width: 100%; font: inherit; color: inherit; background: none; border: 0; padding: 2px 4px; margin-inline-start: -4px; border-radius: 6px; text-align: start; font-weight: 600; font-size: 16px; cursor: text; }
  .title:hover { background: var(--fill); }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; padding: 0; border: 0; }
  .cost { font: inherit; font-size: var(--fs-xs); font-weight: 500; border: 0; cursor: help; display: inline-flex; align-items: center; gap: 4px; }
  .banner { margin-top: 8px; font-size: var(--fs-sm); padding: 6px 10px; border-radius: var(--radius); background: var(--panel-2); border: var(--border-w) dashed var(--accent); }

  .state { align-items: center; justify-content: center; text-align: center; gap: 6px; }
  .state h2 { margin: 6px 0 0; }
  .state p { margin: 0; max-width: 420px; }
  .state .detail { font-size: var(--fs-xs); font-family: var(--mono); overflow-wrap: anywhere; }
  .state .row { margin-top: 12px; }
  .state :global(.back) { transform: scaleX(-1); }
  :global([dir="rtl"]) .state :global(.back) { transform: none; }

  .skel { border-radius: 10px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .skel.line { height: 13px; }
  .title-sk { width: 220px; height: 20px; margin: 4px 0 14px; }
  .skel-col { display: flex; flex-direction: column; gap: 12px; padding-top: 18px; }
  .bubble-sk { width: 46%; height: 44px; border-radius: 16px; }
  .bubble-sk.short { width: 30%; }
  .end { align-self: flex-end; }
  .w80 { width: 80%; } .w70 { width: 70%; } .w60 { width: 60%; } .w40 { width: 40%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel, .dots i { animation: none; } }

  .msgs-wrap { flex: 1; min-height: 0; position: relative; display: flex; }
  .msgs { flex: 1; padding: 16px 0; display: flex; flex-direction: column; gap: 10px; }
  .msg.user { display: flex; flex-direction: column; align-items: flex-end; gap: 2px; }
  .send { width: 32px; height: 32px; padding: 0; border-radius: 50%; }
  .think, .att, .errline, .working { display: flex; gap: 6px; align-items: center; }
  .think { min-width: 0; }
  .bubble { background: var(--accent); color: var(--accent-fg); padding: 9px 14px; max-width: 78%; white-space: pre-wrap; overflow-wrap: anywhere; box-shadow: var(--shadow); border-radius: 16px; }
  .bubble .att { opacity: .85; margin-top: 4px; }
  .msg.ai { max-width: 100%; min-width: 0; }
  .msg.ai .md { overflow-wrap: anywhere; }
  .msg.err { color: var(--danger); font-size: 13px; display: flex; flex-direction: column; align-items: flex-start; gap: 4px; }
  .errline { white-space: pre-wrap; align-items: flex-start; }
  .msg.sys { font-size: var(--fs-sm); }
  .msg.denied { font-size: var(--fs-sm); padding: 6px 10px; border-radius: var(--radius); border: var(--border-w) dashed var(--border); display: flex; flex-direction: column; align-items: flex-start; gap: 4px; }
  .deniedhead { display: flex; gap: 6px; align-items: center; font-weight: 500; }
  .msg.denied ul { margin: 0; padding-inline-start: 20px; display: grid; gap: 4px; overflow-wrap: anywhere; }
  .msg.denied .why { display: block; font-size: var(--fs-xs); }
  .controls .webtoggle { display: inline-flex; align-items: center; gap: 5px; font-size: var(--fs-xs); padding: 4px 8px; cursor: pointer; }
  .controls .webtoggle input { margin: 0; }
  .controls .webtoggle.on { color: var(--accent); }
  .hint { margin: auto; display: flex; flex-direction: column; align-items: center; gap: 8px; font-size: 13.5px; }

  .acts { display: flex; gap: 2px; opacity: 0; transition: opacity .15s; }
  .msg:hover .acts, .acts:focus-within, .acts.show { opacity: 1; }
  @media (hover: none) { .acts { opacity: 1; } }
  .act { font: inherit; font-size: var(--fs-xs); color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 3px 7px; cursor: pointer; display: inline-flex; align-items: center; gap: 5px; }
  .act:hover { color: var(--fg); background: var(--fill); }
  .act.show { opacity: 1; }

  .toolrow { min-width: 0; }
  .tool { font: inherit; background: none; border: 0; padding: 2px 4px; margin-inline-start: -4px; border-radius: 6px; text-align: start; cursor: pointer; font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); display: flex; align-items: center; gap: 6px; max-width: 100%; }
  .tool:hover { color: var(--accent); }
  .tooldetail { margin: 4px 0 6px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); display: grid; gap: 4px; min-width: 0; }
  .tooldetail pre { margin: 0 0 4px; max-height: 260px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
  .tooldetail p { margin: 0; }
  .pill.bad { color: var(--danger); }

  .dots { display: inline-flex; gap: 3px; }
  .dots i { width: 5px; height: 5px; border-radius: 50%; background: currentColor; animation: blink 1.2s infinite both; }
  .dots i:nth-child(2) { animation-delay: .2s; } .dots i:nth-child(3) { animation-delay: .4s; }
  @keyframes blink { 0%, 80%, 100% { opacity: .25; } 40% { opacity: 1; } }

  .jump { position: absolute; bottom: 10px; inset-inline-start: 50%; transform: translateX(-50%); border-radius: 99px; box-shadow: var(--shadow-lg); background: var(--panel); border: 1px solid var(--hairline); font-size: var(--fs-sm); z-index: 3; }
  :global([dir="rtl"]) .jump { transform: translateX(50%); }

  .composer { position: relative; padding: 8px; border-radius: 16px; }
  .composer textarea { border: 0; resize: none; background: transparent; box-shadow: none; display: block; min-height: 56px; max-height: 40vh; overflow-y: hidden; }
  .esc { font-size: var(--fs-xs); }
  .composer:focus-within { box-shadow: var(--shadow), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent); }
  .controls { flex-wrap: wrap; }
  .controls label { position: relative; margin: 0; color: var(--fg); font-size: 14px; }
  .controls label:focus-within { box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 35%, transparent); }

  .mentions { position: absolute; bottom: 100%; inset-inline-start: 8px; margin-bottom: 4px; padding: 4px; background: var(--panel); border: var(--border-w) solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg); display: flex; flex-direction: column; min-width: 320px; max-width: calc(100% - 16px); z-index: 5; }
  .mentions button { font: inherit; color: inherit; background: none; border: 0; border-radius: 6px; text-align: start; padding: 5px 10px; cursor: pointer; font-size: var(--fs-sm);
    display: flex; align-items: center; gap: 8px; min-width: 0; }
  .mentions button > :global(.icon) { flex: none; color: var(--muted); }
  .mentions button.sel > :global(.icon) { color: inherit; }
  .mt { display: flex; flex-direction: column; min-width: 0; line-height: 1.3; }
  .mp { font-size: var(--fs-xs); opacity: .7; text-align: start; }
  .mt .ell { display: block; }
  .refs { flex-wrap: wrap; gap: 4px 8px; }
  .suggest { display: flex; }
  .sugg { font: inherit; font-size: var(--fs-sm); color: var(--fg); background: var(--panel); border: var(--border-w) solid var(--border); border-radius: 14px;
    padding: 6px 12px; cursor: pointer; display: inline-flex; align-items: flex-start; gap: 7px; text-align: start; max-width: 100%; box-shadow: var(--shadow); }
  .sugg:hover { border-color: var(--accent); }
  .sugg > :global(.icon) { color: var(--accent); flex: none; margin-top: 3px; }
  .ref { font: inherit; color: inherit; background: none; border: 0; padding: 0; cursor: pointer; text-decoration: underline; text-underline-offset: 2px; opacity: .9; }
  .mentions button.sel { background: var(--accent); color: var(--accent-fg); }

  .files { list-style: none; margin: 0 0 6px; padding: 0; display: flex; flex-wrap: wrap; gap: 6px; }
  .file { display: flex; align-items: center; gap: 8px; padding: 4px 6px 4px 4px; border-radius: 10px; background: var(--fill); max-width: 240px; }
  .file img, .ficon { width: 36px; height: 36px; border-radius: 7px; object-fit: cover; flex: none; }
  .ficon { display: grid; place-items: center; background: var(--fill-2); color: var(--muted); }
  .fmeta { display: flex; flex-direction: column; min-width: 0; font-size: var(--fs-xs); line-height: 1.35; }
  .fname { display: block; font-weight: 500; }
  .file.gone { outline: 1px dashed var(--muted); outline-offset: -1px; }
  .file.gone .fname { color: var(--muted); }
  .chips { margin-bottom: 4px; }
  .chip { display: inline-flex; align-items: center; gap: 4px; max-width: 100%; padding: 0 2px 0 8px; padding-inline: 8px 2px; border-radius: 99px; background: var(--tint); color: var(--on-tint); font-size: var(--fs-xs); }
  .chip .x { color: inherit; }
  .x { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 6px; padding: 4px; min-width: 24px; min-height: 24px; flex: none; cursor: pointer; display: grid; place-items: center; }
  .x:hover { color: var(--fg); background: var(--fill-2); }

  .dropzone { position: absolute; inset: 0 12px 8px; z-index: 10; pointer-events: none; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; font-weight: 500; color: var(--accent); border: 2px dashed var(--accent); border-radius: var(--radius-lg); background: color-mix(in srgb, var(--bg) 88%, transparent); }

  .approve { margin-bottom: 8px; }
  .runbar { margin-bottom: 8px; gap: 10px; font-size: var(--fs-sm); }
  .banner.alert { border-style: solid; border-color: var(--danger); }
  .banner.alert > :global(.icon) { color: var(--danger); flex: none; }
  .sm { font-size: var(--fs-xs); }

  /* On paper: the whole conversation from the first message, not the screenful the window shows.
     The list stops being a scroll box, and everything that is only for working in the chat goes. */
  @media print {
    .wrap { display: block; height: auto; max-width: none; padding: 0; }
    .msgs-wrap, .msgs { display: block; overflow: visible; }
    .msgs { padding: 12px 0 0; }
    .msgs > :global(*) { margin-bottom: 10px; }
    .bubble { max-width: 100%; box-shadow: none; }
    header select, header .btn, .cost, .acts, .jump, .composer, .runbar, .approve, .dropzone, .banner .btn, .working, .think, .hint, .suggest { display: none !important; }
    .tooldetail pre { max-height: none; overflow: visible; }
  }
</style>
