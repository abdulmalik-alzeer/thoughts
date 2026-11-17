import { t } from './i18n';

/** The fixed descriptions the service gives its own history entries (`git::subject` and its
 *  callers), each with the words the reader sees instead. Anything else (a wrap's title, a moved
 *  file's name, an assistant's own summary) is shown as it was written. */
export const FIXED: Record<string, string> = {
  'edit: your changes': 'final-walk-a.subj.yourChanges',
  'edit: changes before undoing an earlier change': 'final-walk-a.subj.beforeUndo',
  'edit: the library as it was before the history was repaired': 'final-walk-a.subj.beforeRepair',
  "edit: changes before undoing an assistant's changes": 'final-walk-a.subj.beforeUndoAssistant',
  'edit: routine process-inbox': 'final-walk-a.subj.inbox',
  'edit: changes before wrap': 'final-walk-a.subj.beforeWrap',
  'edit: changes before reorganize': 'final-walk-a.subj.beforeReorganize',
  'edit: changes before applying reorganize': 'final-walk-a.subj.beforeApplyReorganize',
  'edit: Thoughts added its rules, index and routines': 'final-walk-a.subj.adopted',
  'edit: changes before bringing in files': 'final-walk-a.subj.beforeImport',
  'sync: changes from another device combined with yours': 'final-walk-a.subj.combined',
};

/** A history entry's subject in the reader's language when it is one of the app's own, else null. */
export function appSubject(subject: string | null | undefined): string | null {
  const key = FIXED[(subject ?? '').trim()];
  return key ? t(key) : null;
}
