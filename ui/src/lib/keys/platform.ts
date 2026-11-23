// Which kind of computer the window runs on, for code that must not load the app's store (the
// message box keys, lib/composer/keys.ts). The registry (lib/keys/registry.ts) uses the same test.
export const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);
