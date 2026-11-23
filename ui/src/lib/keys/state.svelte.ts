// The keyboard shortcuts the user changed (the layer's `keys.json`, set by lib/layer.ts when the
// layer loads), kept reactive so every tooltip and the Keyboard page follow a change at once.
export const keyState = $state({
  /** id -> the keys that replace its defaults (`[]` = no key). Empty in safe mode. */
  overrides: {} as Record<string, string[]>,
  /** The `keys.json` exactly as it was loaded (to notice when undo, rollback or a kept change altered it). */
  loaded: '',
});
