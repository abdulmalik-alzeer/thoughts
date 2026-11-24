// What one assistant's plan-limit block shows, decided in one place (no component needed to test
// it): the bars, a placeholder while the service asks the assistant, or one plain sentence saying
// why there is nothing to show. The service asks without a run (`plan_watch.rs`); `watch` is what it
// reports about that asking.
import type { Plan } from './meter.svelte';

/** How the assistant is set up, as the status says (`providerState`). */
export type Setup = 'missing' | 'signedOut' | 'key' | 'plan' | 'unknown';
/** What the service says about asking one assistant for its plan's usage. */
export type WatchOne = { state: string; reason?: string | null; checkedAt: number; fetchedAt: number; busy: boolean } | null | undefined;

export type PlanKind =
  /** Bars. `asOf` is set when the figures are old enough that the time must be said. */
  | 'known'
  /** Being asked right now, nothing to show yet: a placeholder of the same size as the bars. */
  | 'loading'
  /** The assistant is not installed / not signed in. */
  | 'missing' | 'signedOut'
  /** An API key or another service: there is no plan limit (said once, not as an error). */
  | 'key'
  /** The installed assistant is too old to be asked without a chat. */
  | 'old'
  /** Asked, and the account has no plan limits to report. */
  | 'noReport'
  /** Could not be asked just now; it is tried again by itself. */
  | 'failed'
  /** Not asked (switched off, or the note about subscriptions was not read yet): shown after the next chat. */
  | 'waiting';

/** Figures older than this say when they are from: two rounds of asking have gone by without a newer one. */
export const OLD_AFTER_S = 25 * 60;

export function planView(setup: Setup, plan: Plan, watch: WatchOne, nowS: number): { kind: PlanKind; asOf: number | null } {
  const has = !!plan?.windows?.length;
  // An API key or another service never has a plan limit, whatever an earlier sign-in left behind.
  if (setup === 'key') return { kind: 'key', asOf: null };
  if (has) {
    const seen = plan!.seenAt || 0;
    return { kind: 'known', asOf: seen && nowS - seen > OLD_AFTER_S ? seen : null };
  }
  if (setup === 'missing' || setup === 'signedOut') return { kind: setup, asOf: null };
  // Not heard from the service yet, or it is asking right now.
  if (!watch || watch.busy || !watch.state) return { kind: 'loading', asOf: null };
  if (watch.state === 'failed') return { kind: 'failed', asOf: null };
  switch (watch.reason) {
    case 'missing': return { kind: 'missing', asOf: null };
    case 'signedOut': return { kind: 'signedOut', asOf: null };
    case 'key': case 'service': return { kind: 'key', asOf: null };
    case 'oldProgram': return { kind: 'old', asOf: null };
    case 'off': case 'notice': return { kind: 'waiting', asOf: null };
    default: return { kind: 'noReport', asOf: null };
  }
}
