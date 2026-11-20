// What the chat and dictation need of a text box, whether a plain <textarea> or the formatted
// message box (Composer.svelte), so both work with either.
export interface TextBox {
  readonly value: string;
  readonly selectionStart: number;
  readonly selectionEnd: number;
  focus(opts?: FocusOptions): void;
  setSelectionRange(start: number, end: number): void;
  setRangeText(text: string, start: number, end: number, mode?: SelectionMode): void;
  dispatchEvent(e: Event): boolean;
}

/** The element that holds the focus while the box is being typed in. */
export function boxElement(b: TextBox | HTMLElement | null | undefined): HTMLElement | null {
  if (!b) return null;
  if (b instanceof HTMLElement) return b;
  return (b as { element?: HTMLElement }).element ?? null;
}
