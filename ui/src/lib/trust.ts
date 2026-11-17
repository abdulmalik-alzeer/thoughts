// The trust question: shown only for the structured `trust` object the background service attaches
// to a 403 `untrusted` answer. The text of an error message can never open it or word it.
import { t } from './i18n';
import { choiceDialog, confirmDialog } from './dialog.svelte';

/** One thing found in the folder. `kind` is a fixed word from the service; the rest is data from the folder. */
export type TrustItem = { kind: string; file: string; name: string; detail: string; new: boolean; open: string | null };
/** What the service found in a folder that needs the user's yes (`trust` in the error body). */
export type TrustInfo = {
  id: string; path: string; known: boolean; changed: string[];
  items: TrustItem[]; files: { file: string; what: string }[]; moreFiles: number;
  /** Why a yes cannot be recorded for the folder as it is (the service's own sentence), or null. */
  blocked?: string | null;
};

/** Is this the service's own trust question (and not something that merely looks like one)? */
export function isTrustInfo(x: any): x is TrustInfo {
  return !!x && typeof x.id === 'string' && !!x.id && typeof x.path === 'string' && Array.isArray(x.items);
}

/** The app's own sentence for a finding. Nothing from the folder is part of it except a setting's name. */
export function trustSentence(i: TrustItem): string {
  const known = (key: string, vars: Record<string, string> = {}) => { const s = t(key, vars); return s === key ? null : s; };
  if (i.kind === 'hook') {
    if (!i.name) return t('seca.trust.hook.any');
    return known(`seca.trust.hook.${i.name}`) ?? t('seca.trust.hook.other', { name: i.name });
  }
  if (i.kind === 'mcp' && !i.name) return t('seca.trust.kind.mcp.any');
  // For a second upload address the remote's short name is the middle of `remote.<name>.pushurl`.
  const name = i.kind === 'git_pushurl' ? i.name.replace(/^remote\./i, '').replace(/\.pushurl$/i, '') : i.name;
  return known(`seca.trust.kind.${i.kind}`, { name }) ?? t('seca.trust.kind.other');
}

/** The paragraph that says why the question is being asked. */
export function trustIntro(info: TrustInfo): string {
  if (!info.known) return t('seca.trust.first');
  const a = info.changed.includes('assistant'), g = info.changed.includes('git');
  const what = t(a && g ? 'seca.trust.changed.both' : a ? 'seca.trust.changed.assistant' : g ? 'seca.trust.changed.git' : 'seca.trust.changed.new');
  return `${what} ${t(info.items.length ? 'seca.trust.review' : 'seca.trust.plain')}`;
}

/** Ask the user. Resolves true once the service has recorded their yes; false when they declined;
 *  the service's reason when the folder cannot be trusted as it is (then there is no yes to give:
 *  the dialog shows what was found and why, with only a Close button).
 *  This dialog shows what was found; its button opens the desktop app's own confirmation, which is
 *  the yes (`answer`). `answer` resolves a newer question when the folder changed while the dialogs
 *  were open (the service then refuses the old answer), null when it is settled, false on a no. */
export async function askTrust(info: TrustInfo, answer: (id: string) => Promise<TrustInfo | null | false>): Promise<boolean | string> {
  let current: TrustInfo | null = info;
  while (current) {
    if (current.blocked) {
      await choiceDialog({ title: t('seca.trust.title'), trust: current, choices: [], cancelLabel: t('close') });
      return current.blocked;
    }
    const next = await confirmDialog({ title: t('seca.trust.title'), trust: current, confirmLabel: t('sec-shell-ipc.trust.next'), cancelLabel: t('seca.trust.cancel'), danger: true });
    if (!next) return false;
    const answered = await answer(current.id);
    if (answered === false) return false;
    current = answered;
  }
  return true;
}
