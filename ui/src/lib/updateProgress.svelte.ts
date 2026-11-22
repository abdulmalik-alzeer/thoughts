// How far the download of an app update is, for the bar in Settings › Maintenance. One place
// that stays put: not a notification sent again for every piece that arrives.
export const download = $state({ active: false, got: 0, total: 0 });

/** True while a check for a new version is looking (the turning icon on the "Check for updates"
 *  buttons). Not the dialog or the download that may follow. */
export const check = $state({ busy: false });

/** Run one lookup with the icon turning, for at least long enough to be seen. */
export async function looking<T>(run: () => Promise<T>): Promise<T> {
  const since = Date.now();
  check.busy = true;
  try { return await run(); }
  finally {
    const left = 600 - (Date.now() - since);
    if (left > 0) await new Promise((r) => setTimeout(r, left));
    check.busy = false;
  }
}
