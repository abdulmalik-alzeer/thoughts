// MADO, round 0.4.0: looking at the built product's screens when a build asks for it. While a build
// is `checking`, the service waits for one window to look; this asks what to look at, has the desktop
// shell look (lib/mado/screencheck.ts), and sends back what it saw, once. In a browser nothing can
// look: the service gives up by itself and the view says so.
import { madoApi } from './api';
import { askedScreens, clampResults, localUrl } from './plans';
import { screenCheck } from './screencheck';
import type { ScreenResult } from './types';

/** `looking`: this window is at it · `sent`: it answered · `cannot`: nothing here can look ·
 *  `failed`: the answer could not be sent · `busy`: the shell is looking at another build's screens
 *  (asked again on the next call) · `badUrl` / `tooMany`: the shell refused the address or the number
 *  of screens (the build is told at once that nothing was looked at). */
export type Look = 'looking' | 'sent' | 'cannot' | 'failed' | 'busy' | 'badUrl' | 'tooMany';

/** The shell's refusals (`src-tauri/src/screen_check.rs`), by the code it throws. */
const REFUSED: Record<string, Look> = { 'screen-check-busy': 'busy', 'screen-check-bad-url': 'badUrl', 'screen-check-too-many': 'tooMany' };
/** By project: what this window did about the build's request. */
export const looks = $state<Record<string, Look>>({});

export interface LookDeps {
  api: { buildScreens: () => Promise<{ url: string; screens: { id: string; path: string }[]; nonce: string }>; sendScreens: (nonce: string, results: ScreenResult[]) => Promise<unknown> };
  check: (url: string, screens: { id: string; path: string }[]) => Promise<ScreenResult[] | null>;
  /** Runs `job` only if no other window holds `name`; the app's windows share one origin. */
  only: (name: string, job: () => Promise<void>) => Promise<void>;
}

const busy = new Set<string>();
/** Requests (by their nonce) this window already dealt with: never answered twice. */
const answered = new Set<string>();

/** One window at a time, through the browser's own locks; where there are none, this window alone. */
export async function onlyOneWindow(name: string, job: () => Promise<void>): Promise<void> {
  const locks = typeof navigator !== 'undefined' ? (navigator as any).locks : undefined;
  if (!locks?.request) return job();
  await locks.request(name, { ifAvailable: true }, async (lock: unknown) => { if (lock) await job(); });
}

/** Ask, look, answer. Safe to call as often as the build's state changes. */
export async function lookAtScreens(projectId: string, deps?: Partial<LookDeps>): Promise<void> {
  if (busy.has(projectId)) return;
  busy.add(projectId);
  const api = deps?.api ?? madoApi(projectId);
  const check = deps?.check ?? screenCheck;
  const only = deps?.only ?? onlyOneWindow;
  try {
    await only(`thoughts-mado-screens:${projectId}`, async () => {
      let ask: Awaited<ReturnType<LookDeps['api']['buildScreens']>>;
      try { ask = await api.buildScreens(); } catch { return; } // not waiting for a window (yet, or any more)
      const nonce = typeof ask?.nonce === 'string' ? ask.nonce : '';
      if (!nonce || nonce.length > 200 || answered.has(nonce)) return;
      answered.add(nonce);
      const url = localUrl(ask.url), screens = askedScreens(ask.screens);
      if (!url || !screens.length) { looks[projectId] = 'cannot'; return; }
      looks[projectId] = 'looking';
      let seen: ScreenResult[] | null;
      try { seen = await check(url, screens); }
      catch (e) {
        const refused = REFUSED[e instanceof Error ? e.message : String(e)];
        if (refused === 'busy') { answered.delete(nonce); looks[projectId] = 'busy'; return; }
        if (refused) {
          // Nothing will be looked at here: say so to the build now, rather than have it wait.
          looks[projectId] = refused;
          try { await api.sendScreens(nonce, []); } catch { /* it gives up by itself */ }
          return;
        }
        seen = null;
      }
      if (seen === null) { looks[projectId] = 'cannot'; return; }
      try { await api.sendScreens(nonce, clampResults(seen, screens)); looks[projectId] = 'sent'; }
      catch { looks[projectId] = 'failed'; }
    });
  } finally { busy.delete(projectId); }
}

/** For tests. */
export function forgetLooks() { busy.clear(); answered.clear(); for (const k of Object.keys(looks)) delete looks[k]; }
