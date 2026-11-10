// In-app replacements for window.confirm / window.prompt. Rendered by lib/Dialogs.svelte (mounted once in App).
export type DialogChoice = { id: string; label: string; primary?: boolean; danger?: boolean };
export type DialogReq = {
  kind: 'confirm' | 'prompt' | 'choice';
  title: string;
  body?: string;
  list?: string[];
  /** Small reassuring line shown under the list (e.g. "a backup is always kept"). */
  note?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  value?: string;
  placeholder?: string;
  choices?: DialogChoice[];
  resolve: (v: any) => void;
};

export const dialogs = $state<{ current: DialogReq | null }>({ current: null });

function open<T>(req: Omit<DialogReq, 'resolve'>): Promise<T> {
  // Only one dialog at a time: a new one dismisses the one it replaces.
  dismissDialog();
  return new Promise((resolve) => { dialogs.current = { ...req, resolve }; });
}

/** Resolves true when confirmed, false when cancelled or dismissed (Esc / backdrop). */
export function confirmDialog(o: Omit<DialogReq, 'kind' | 'resolve' | 'choices'>): Promise<boolean> {
  return open<boolean>({ ...o, kind: 'confirm' });
}

/** Resolves the entered string, or null when cancelled. */
export function promptDialog(o: Omit<DialogReq, 'kind' | 'resolve' | 'choices'>): Promise<string | null> {
  return open<string | null>({ ...o, kind: 'prompt' });
}

/** Several named answers plus Cancel. Resolves the chosen `id`, or null when cancelled. */
export function choiceDialog(o: Omit<DialogReq, 'kind' | 'resolve'> & { choices: DialogChoice[] }): Promise<string | null> {
  return open<string | null>({ ...o, kind: 'choice' });
}

/** Cancel whatever dialog is open (used by the global Esc handler). Returns true if one was open. */
export function dismissDialog(): boolean {
  const d = dialogs.current;
  if (!d) return false;
  dialogs.current = null;
  d.resolve(d.kind === 'confirm' ? false : null);
  return true;
}
