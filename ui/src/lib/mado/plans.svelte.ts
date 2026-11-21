// MADO, round 0.4.0: the plans the sidebar lists (`GET /api/mado`), kept current by the events the
// Plan view listens to (`mado` state and build changes) and by the reconnect event.
import { on } from '../api';
import { listPlans } from './api';
import { cleanPlans } from './plans';
import { lookAtScreens } from './screenwatch.svelte';
import type { PlanEntry } from './types';

export const plans = $state<{ list: PlanEntry[] }>({ list: [] });

let following = false;
let timer: ReturnType<typeof setTimeout> | undefined;
let ticket = 0;

export async function refreshPlans(): Promise<void> {
  const mine = ++ticket;
  let list: PlanEntry[];
  try { list = cleanPlans(await listPlans()); } catch { return; } // an older service has no such list: nothing shown
  if (mine !== ticket) return;
  plans.list = list;
  // A build that wants its screens looked at is answered from wherever the app is, not only from its own page.
  const checking = list.filter((p) => p.build === 'checking');
  for (const p of checking) lookAtScreens(p.projectId).catch(() => {});
  if (checking.length) soon(5000);
}
function soon(ms = 250) { clearTimeout(timer); timer = setTimeout(() => { refreshPlans(); }, ms); }

/** Start following (once for the whole window). */
export function followPlans(): void {
  if (following) { soon(0); return; }
  following = true;
  refreshPlans();
  on((e) => {
    if (e?.type === 'resync' || (e?.type === 'mado' && (e.what === 'state' || e.what === 'build'))) soon();
  });
}
