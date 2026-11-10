// In-app replacements for window.confirm / window.prompt. Rendered by lib/Dialogs.svelte (mounted once in App).
export type DialogReq = {
  kind: 'confirm' | 'prompt';
  title: string;
  body?: string;
  list?: string[];
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  value?: string;
  placeholder?: string;
  resolve: (v: any) => void;
};

export const dialogs = $state<{ current: DialogReq | null }>({ current: null });

/** Resolves true when confirmed, false when cancelled or dismissed (Esc / backdrop). */
export function confirmDialog(o: Omit<DialogReq, 'kind' | 'resolve'>): Promise<boolean> {
  return new Promise((resolve) => { dialogs.current = { ...o, kind: 'confirm', resolve }; });
}

/** Resolves the entered string, or null when cancelled. */
export function promptDialog(o: Omit<DialogReq, 'kind' | 'resolve'>): Promise<string | null> {
  return new Promise((resolve) => { dialogs.current = { ...o, kind: 'prompt', resolve }; });
}
