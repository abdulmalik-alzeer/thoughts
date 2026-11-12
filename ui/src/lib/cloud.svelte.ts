// Is a folder kept in sync by a cloud drive? The background service answers (it knows the real
// sync folders of this computer and resolves shortcuts); the app never guesses from the path's name.
import { api } from './api';

export type Cloud = { synced: boolean; service: string | null };
const NONE: Cloud = { synced: false, service: null };

/** Follows a folder path as it is typed or picked: `current` is the answer for the latest path,
 *  asked a moment after the path stops changing. Call inside a component's script. */
export function cloudFor(path: () => string): { readonly current: Cloud } {
  let current = $state<Cloud>(NONE);
  $effect(() => {
    const p = path().trim();
    if (!p) { current = NONE; return; }
    const timer = setTimeout(() => {
      api<Cloud>(`/library/cloud?path=${encodeURIComponent(p)}`)
        .then((c) => { if (path().trim() === p) current = { synced: !!c?.synced, service: c?.service ?? null }; })
        .catch(() => { /* unknown: say nothing rather than something wrong */ });
    }, 250);
    return () => clearTimeout(timer);
  });
  return { get current() { return current; } };
}
