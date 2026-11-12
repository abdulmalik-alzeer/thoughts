/** Is this key press part of typing with an input method (Japanese, Chinese, Korean and others)?
 *  Enter then accepts the suggested word and must never send a message or submit a dialog.
 *  `isComposing` alone is not enough: WebKit — the app's web view on macOS — ends the composition
 *  just *before* that Enter's keydown, so the flag is already false and only the legacy key code
 *  229 ("handled by the input method") still says so. */
export const imeKey = (e: KeyboardEvent): boolean => e.isComposing || e.keyCode === 229;
