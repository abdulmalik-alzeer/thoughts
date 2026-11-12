// Voice in (FEAT-55): dictation and meeting recordings. The desktop app listens, records and turns
// speech into text on this computer (macOS's own speech recognition, only ever in its on-device
// mode; see src-tauri/src/voice.rs). The window never gets the sound: it asks the desktop app to
// listen or record and gets words back. Only the Mac app has it; elsewhere nothing here is shown.
import { inTauri, post } from './api';
import { go, refreshChats, toast } from './store.svelte';
import { t, lang as appLang } from './i18n';
import { choiceDialog, confirmDialog } from './dialog.svelte';
import { failed } from './errors';
import { libs } from './libraries.svelte';

export type VoiceLang = 'en' | 'ar';
type Perm = 'granted' | 'denied' | 'restricted' | 'ask';
type LangInfo = { locale: string | null; onDevice: boolean; available: boolean };
export type Rec = { path: string; item: string; title: string; lang: VoiceLang; date: string; seconds: number; paused: boolean; level: number; ended?: string | null; library?: string };
export type Job = { path: string; meta: any; done: number; total: number };
export type Status = {
  supported: boolean; reason?: string; platform?: string; mic?: Perm; speech?: Perm; langs?: Record<VoiceLang, LangInfo>;
  dictating?: boolean; recording?: Rec | null; transcribing?: Job | null; result?: string | null;
};
/** A transcript just filed for an item: its page offers to write the meeting notes from it. */
export type Ready = { item: string; path: string; title: string; complete: boolean };

const LANG_KEY = 'thoughts.voice.lang';
function storedLang(): VoiceLang | null {
  try { const v = localStorage.getItem(LANG_KEY); return v === 'ar' || v === 'en' ? v : null; } catch { return null; }
}

export const voice = $state({
  status: null as Status | null,
  /** The language the user speaks in: their last choice on this computer, else the app's language. */
  lang: (storedLang() ?? 'en') as VoiceLang,
  /** Which dictation button is listening, if one is. */
  owner: null as string | null,
  partial: '',
  level: 0,
  /** Dictation that stopped by itself, for the button that started it. */
  ended: null as { owner: string; text: string; error: string | null } | null,
  rec: null as Rec | null,
  job: null as Job | null,
  ready: [] as Ready[],
  /** Items whose page shows the recorder right now (the floating bar is not needed there). */
  panels: [] as string[],
});

/** Only the Mac app can turn speech into text without sending it anywhere. */
export const voiceShell = () => inTauri() && (window as any).__THOUGHTS_PLATFORM__ === 'macos';

async function call<T = any>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

/** `VOICE::<code>[::detail]` from the desktop app. */
export function voiceCode(e: unknown): { code: string; detail: string } {
  const m = /VOICE::([a-z-]+)(?:::([\s\S]*))?/.exec(e instanceof Error ? e.message : String(e ?? ''));
  return m ? { code: m[1], detail: (m[2] ?? '').trim() } : { code: '', detail: e instanceof Error ? e.message : String(e ?? '') };
}

/** Say what went wrong, in the user's words. */
export function voiceError(e: unknown): void {
  const { code, detail } = voiceCode(e);
  const key = code === 'busy' ? (detail === 'recording' ? 'voice.err.busyRecording' : detail === 'transcribing' ? 'voice.err.busyTranscribing' : 'voice.err.busyDictating')
    : code === 'no-input' ? 'voice.err.noInput' : code === 'library' ? 'voice.err.library' : code === 'empty' ? 'voice.err.empty'
    : code === 'unreadable' ? 'voice.err.unreadable' : code === 'unsupported' ? 'voice.err.unsupported' : 'voice.err.failed';
  const quiet = ['busy', 'empty', 'unsupported'].includes(code);
  toast(t(key), { kind: 'error', key: 'voice-error', detail: quiet ? undefined : detail || undefined });
}

let listening = false;
let loading: Promise<Status | null> | null = null;

/** The desktop app's voice status (permissions, languages, what is running). */
export async function loadVoice(): Promise<Status | null> {
  if (!voiceShell()) return null;
  loading ??= call<Status>('voice_status').then((s) => {
    voice.status = s;
    voice.rec = s.recording ?? null;
    if (s.transcribing) voice.job = s.transcribing; else if (voice.job && !s.transcribing) voice.job = null;
    return s;
  }).catch(() => null).finally(() => { loading = null; });
  return loading;
}

/** Once per window: listen to the desktop app, pick up a recording or transcript already under way. */
export async function initVoice(): Promise<void> {
  if (listening || !voiceShell()) return;
  listening = true;
  if (!storedLang()) voice.lang = appLang();
  try {
    const { listen } = await import('@tauri-apps/api/event');
    await listen<any>('voice', (e) => onEvent(e.payload ?? {}));
  } catch { listening = false; return; }
  const s = await loadVoice();
  if (s?.result) fileResult();
  // A recording that stopped by itself while no window was listening (the page reloaded).
  if (s?.recording?.ended) recEnded(s.recording.ended, s.recording);
}

function onEvent(p: any) {
  switch (p.type) {
    case 'level': voice.level = Number(p.level) || 0; break;
    case 'partial': voice.partial = String(p.text ?? ''); break;
    case 'dictation-ended':
      if (voice.owner) voice.ended = { owner: voice.owner, text: String(p.text ?? ''), error: p.error ? String(p.error) : null };
      voice.owner = null;
      break;
    case 'rec': voice.rec = p.recording ?? null; break;
    case 'rec-ended': recEnded(p.reason, p.recording); break;
    case 'progress':
      if (voice.job && voice.job.path === p.path) { voice.job.done = Number(p.done) || 0; voice.job.total = Number(p.total) || voice.job.total; }
      else voice.job = { path: String(p.path), meta: lastJob && lastJob.path === p.path ? lastJob.meta : null, done: Number(p.done) || 0, total: Number(p.total) || 0 };
      break;
    case 'transcribed': voice.job = null; fileResult(); break;
    case 'transcribe-failed':
      voice.job = null;
      toast(t('voice.job.failed'), { kind: 'error', key: 'voice-job', detail: voiceCode(p.error).detail || undefined,
        action: lastJob?.path === p.path ? { label: t('ui.retry'), run: () => { const j = lastJob!; transcribe(j.path, j.lang, j.meta); } } : undefined });
      break;
    case 'transcribe-cancelled': voice.job = null; toast(t('voice.job.cancelled'), { key: 'voice-job' }); break;
  }
}

// ---------- the user's language ----------

export function setVoiceLang(l: VoiceLang) {
  voice.lang = l;
  try { localStorage.setItem(LANG_KEY, l); } catch { /* storage unavailable: kept for this window */ }
}
export const langName = (l: VoiceLang) => t('voice.lang.' + l);
const works = (l: VoiceLang) => { const i = voice.status?.langs?.[l]; return !!(i?.onDevice && i.available); };

/** Speech in `l` must be turned into text on this Mac. When it cannot be, say why (never fall back
 *  to Apple's servers) and offer the other language if that one works — and, for a meeting, to
 *  record now and turn it into text later. Resolves the language to use, 'later', or null. */
export async function ensureLang(l: VoiceLang, recording = false): Promise<VoiceLang | 'later' | null> {
  if (works(l)) return l;
  const other: VoiceLang = l === 'ar' ? 'en' : 'ar';
  const choices = [
    ...(works(other) ? [{ id: other, label: t('voice.lang.useOther', { lang: langName(other) }), primary: true }] : []),
    ...(recording ? [{ id: 'later', label: t('voice.lang.recordAnyway') }] : []),
  ];
  const pick = await choiceDialog({
    title: t('voice.lang.missingTitle', { in: t('voice.lang.in.' + l) }),
    body: t('voice.lang.missingBody', { in: t('voice.lang.in.' + l), lang: langName(l) }),
    note: recording ? t('voice.lang.recordOnlyNote') : undefined,
    choices, cancelLabel: t('close'),
  });
  if (pick === other) { setVoiceLang(other); return other; }
  return pick === 'later' ? 'later' : null;
}

// ---------- permission ----------

const PRIVACY: Record<'mic' | 'speech', string> = {
  mic: 'x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone',
  speech: 'x-apple.systempreferences:com.apple.preference.security?Privacy_SpeechRecognition',
};

/** The system must let Thoughts use speech recognition (and, with `mic`, the microphone). The
 *  first time, the system asks; a refusal is explained, with the way to change it. */
export async function ensurePermission(mic: boolean): Promise<boolean> {
  let s = await loadVoice();
  if (!s?.supported) { toast(t('voice.err.unsupported'), { kind: 'error', key: 'voice-error' }); return false; }
  if (s.speech === 'ask' || (mic && s.mic === 'ask')) {
    try { s = voice.status = await call<Status>('voice_permit', { mic }); } catch { /* answered below from the old status */ }
  }
  const which = s.speech !== 'granted' ? 'speech' : mic && s.mic !== 'granted' ? 'mic' : null;
  if (!which) return true;
  if (s[which] === 'restricted') {
    toast(t(`voice.perm.${which}.title`) + ' ' + t('voice.perm.restricted'), { kind: 'error', key: 'voice-perm' });
    return false;
  }
  const open = await confirmDialog({ title: t(`voice.perm.${which}.title`), body: t(`voice.perm.${which}.body`), confirmLabel: t('voice.perm.open'), cancelLabel: t('close') });
  if (open) {
    try { await (await import('@tauri-apps/plugin-opener')).openUrl(PRIVACY[which]); }
    catch { toast(t('voice.perm.openFailed'), { kind: 'error' }); }
  }
  return false;
}

// ---------- dictation ----------

export async function startDictation(l: VoiceLang, owner: string): Promise<void> {
  voice.partial = '';
  voice.level = 0;
  voice.ended = null;
  await call('voice_dictate_start', { lang: l });
  voice.owner = owner;
}

/** Stop listening: what was said. */
export async function stopDictation(): Promise<string> {
  try { return await call<string>('voice_dictate_stop'); }
  finally { voice.owner = null; voice.level = 0; }
}

export function cancelDictation(): void {
  voice.owner = null;
  voice.level = 0;
  call('voice_dictate_cancel').catch(() => {});
}

/** Put dictated words into a text box at `at` (the selection when listening started), as one
 *  edit the text box's Undo takes back, with a space on either side where the words need one. */
export function insertText(el: HTMLTextAreaElement, text: string, at?: [number, number] | null): void {
  let words = text.trim();
  if (!words) return;
  const len = el.value.length;
  const [s, e] = at ? [Math.min(at[0], len), Math.min(at[1], len)] : [el.selectionStart, el.selectionEnd];
  const before = el.value.slice(0, s), after = el.value.slice(e);
  if (before && !/\s$/.test(before)) words = ' ' + words;
  if (after && !/^[\s.,;:!?)\]،؛؟]/.test(after)) words += ' ';
  el.focus();
  el.setSelectionRange(s, e);
  let ok = false;
  try { ok = document.execCommand('insertText', false, words); } catch { /* not supported */ }
  if (!ok) {
    el.setRangeText(words, s, e, 'end');
    el.dispatchEvent(new Event('input', { bubbles: true }));
  }
}

// ---------- meeting recordings ----------

export const isAudio = (p: string) => /\.(m4a|wav|mp3|aac|caf|aiff?)$/i.test(String(p ?? ''));
const stem = (p: string) => String(p).replace(/\.[^./]+$/, '');

/** The recording a meeting note was made from: same name (or the note is `<name>-2.md`…). */
export function recordingOf(note: string, files: { path: string }[]): string | null {
  const s = stem(note);
  const audio = files.filter((f) => isAudio(f.path));
  const exact = audio.find((f) => stem(f.path) === s);
  if (exact) return exact.path;
  const m = /^(.*)-\d+$/.exec(s);
  return (m && audio.find((f) => stem(f.path) === m[1])?.path) || null;
}

/** Recordings in a meetings list that have no note made from them yet. */
export function unpaired(files: { path: string }[]): any[] {
  const notes = files.filter((f) => /\.md$/i.test(f.path));
  return files.filter((f) => isAudio(f.path) && !notes.some((n) => recordingOf(n.path, files) === f.path));
}

/** `05:12`, or `1:05:12` from an hour on. */
export function clock(secs: number): string {
  const s = Math.max(0, Math.round(Number(secs) || 0));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
  const two = (n: number) => String(n).padStart(2, '0');
  return h ? `${h}:${two(m)}:${two(r)}` : `${two(m)}:${two(r)}`;
}

/** What a recording's own name says: its date and its topic. */
export function recordingMeta(path: string, item: string): { item: string; title: string; date: string; seconds: number } {
  const base = String(path).split('/').pop()!.replace(/\.[^.]+$/, '');
  const m = /^(\d{4}-\d{2}-\d{2})-?(.*)$/.exec(base);
  const topic = (m ? m[2] : base).replace(/-\d+$/, '').replace(/[-_]+/g, ' ').trim();
  return { item, title: topic ? topic[0].toUpperCase() + topic.slice(1) : '', date: m ? m[1] : '', seconds: 0 };
}

/** Start recording a meeting of `item` into its meetings folder. */
export async function startRecording(item: string, title: string, l: VoiceLang): Promise<boolean> {
  try {
    if (!(await ensurePermission(true))) return false;
    const use = await ensureLang(l, true);
    if (!use) return false;
    voice.rec = await call<Rec>('voice_record_start', { item, title: title.trim() || t('voice.rec.meetingDefault'), lang: use === 'later' ? l : use });
    return true;
  } catch (e) { voiceError(e); return false; }
}

export async function pauseRecording(paused: boolean): Promise<void> {
  try { voice.rec = await call<Rec>('voice_record_pause', { paused }); } catch (e) { voiceError(e); }
}

/** Finish the recording (the file is complete), then turn it into text. */
export async function stopRecording(): Promise<void> {
  let r: Rec;
  try { r = await call<Rec>('voice_record_stop'); } catch (e) { voice.rec = null; voiceError(e); return; }
  voice.rec = null;
  // The library it was recorded in goes along: another one may be open by the time it is text.
  await transcribe(r.path, r.lang, { item: r.item, title: r.title, date: r.date, seconds: r.seconds, library: r.library });
}

/** Stop and delete the recording, after asking. */
export async function discardRecording(): Promise<void> {
  const r = voice.rec;
  if (!r) return;
  const ok = await confirmDialog({ title: t('voice.rec.discardTitle'), body: t('voice.rec.discardBody', { time: clock(r.seconds) }), confirmLabel: t('voice.rec.discardOk'), danger: true });
  if (!ok) return;
  try { await call('voice_record_discard'); voice.rec = null; } catch (e) { voiceError(e); }
}

/** A recording stopped by itself (four hours reached, the microphone gone): keep it, and turn it into text. */
async function recEnded(reason: string, r: Rec | null) {
  if (!r) return;
  toast(t(reason === 'limit' ? 'voice.rec.limit' : 'voice.rec.interrupted'), { kind: reason === 'limit' ? 'info' : 'error', key: 'voice-rec-ended', duration: 0 });
  await stopRecording();
}

let lastJob: { path: string; lang: VoiceLang; meta: any } | null = null;

/** Turn the recording at `path` into text on this Mac; it is filed as a meeting note when done. */
export async function transcribe(path: string, l: VoiceLang, meta: { item: string; title: string; date: string; seconds: number; library?: string }): Promise<boolean> {
  try {
    if (!(await ensurePermission(false))) return false;
    const use = await ensureLang(l);
    if (!use || use === 'later') return false;
    lastJob = { path, lang: use, meta };
    await call('voice_transcribe', { path, lang: use, meta });
    voice.job = { path, meta, done: 0, total: meta.seconds || 0 };
    return true;
  } catch (e) { voiceError(e); return false; }
}

export function cancelTranscription(): void { call('voice_transcribe_cancel').catch(() => {}); }

/** File the finished transcript as the item's meeting note, beside its recording. */
async function fileResult(): Promise<void> {
  let r: any;
  try { r = await call('voice_result'); } catch { return; }
  if (!r?.path) return;
  const meta = r.meta && typeof r.meta === 'object' ? r.meta : recordingMeta(r.path, String(r.path).split('/meetings/')[0]);
  const item = String(meta.item || String(r.path).split('/meetings/')[0]);
  try {
    const out = await post('/library/item/transcript', { path: item, recording: r.path, title: meta.title || '', date: meta.date || '',
      paragraphs: r.paragraphs ?? [], seconds: Number(r.seconds) || 0, complete: r.complete !== false, library: typeof meta.library === 'string' ? meta.library : '' });
    call('voice_result_clear', { path: r.path }).catch(() => {});
    const title = String(meta.title || t('voice.rec.meetingDefault'));
    // Recorded in a library that is no longer the open one: the note is there, not here.
    const norm = (p: unknown) => String(p ?? '').replace(/[\\/]+$/, '');
    const open = libs.list.find((l) => l.open);
    if (meta.library && open && norm(meta.library) !== norm(open.path)) {
      const name = libs.list.find((l) => norm(l.path) === norm(meta.library))?.displayName ?? '';
      toast(t('cln.voice.savedElsewhere', { name }), { kind: r.complete !== false ? 'success' : 'info', key: 'voice-job', duration: 12000 });
      return;
    }
    voice.ready = [{ item, path: out.path, title, complete: r.complete !== false }, ...voice.ready.filter((x) => x.item !== item)].slice(0, 10);
    toast(t(r.complete !== false ? 'voice.done.saved' : 'voice.done.partial'), { kind: r.complete !== false ? 'success' : 'info', key: 'voice-job', duration: 12000,
      action: { label: t('voice.done.open'), run: () => { go({ kind: 'docs', path: out.path }); } } });
  } catch (e) {
    failed(t('voice.done.saveFailed'), e, { key: 'voice-job', duration: 0, action: { label: t('ui.retry'), run: () => { fileResult(); } } });
  }
}

/** A chat in which the assistant writes the meeting note from its transcript (the starter's
 *  meeting-notes routine). Never for a private item: the assistant may not read it. */
export async function writeMeetingNotes(notePath: string, privateItem: string | null): Promise<void> {
  if (privateItem) {
    toast(privateItem, { kind: 'error', key: 'item-private', action: { label: t('item.privacySettings'), run: () => { go({ kind: 'settings', id: 'privacy' }); } } });
    return;
  }
  try {
    const c = await post('/chats', { byUser: false });
    await post(`/chats/${c.id}/send`, { text: t('voice.notes.prompt', { mention: '@' + notePath }), mentions: [notePath] });
    voice.ready = voice.ready.filter((r) => r.path !== notePath);
    await refreshChats().catch(() => {});
    go({ kind: 'chat', id: c.id });
  } catch (e) { failed(t('voice.notes.failed'), e); }
}

/** Play a recording in the Mac's own player. */
export async function playRecording(path: string): Promise<void> {
  try { await call('open_library_file', { path }); }
  catch (e: any) { toast(t('docs.openFailed'), { kind: 'error', detail: String(e?.message ?? e) }); }
}
