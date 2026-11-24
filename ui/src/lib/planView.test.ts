import { describe, expect, it } from 'vitest';
import { OLD_AFTER_S, planView, type WatchOne } from './planView';

const NOW = 1_800_000_000;
const win = (seenAt: number) => ({ id: 'five_hour', percent: 23, resetsAt: NOW + 3600, minutes: 300, status: 'ok', seenAt, hasReset: false });
const plan = (seenAt: number) => ({ windows: [win(seenAt)], seenAt });
const watch = (state: string, reason: string | null = null, busy = false): WatchOne => ({ state, reason, checkedAt: NOW - 60, fetchedAt: state === 'known' ? NOW - 60 : 0, busy });

describe('what the plan-limit block shows', () => {
  it('shows a placeholder, not an empty state, while the service is asking', () => {
    expect(planView('plan', null, undefined, NOW).kind).toBe('loading');
    expect(planView('plan', null, watch('', null, true), NOW).kind).toBe('loading');
    expect(planView('unknown', null, null, NOW).kind).toBe('loading');
    expect(planView('plan', { windows: [], seenAt: 0 }, watch('known', null, true), NOW).kind).toBe('loading');
  });

  it('shows the figures as current when they are fresh, and says when they are from once they are old', () => {
    expect(planView('plan', plan(NOW - 60), watch('known'), NOW)).toEqual({ kind: 'known', asOf: null });
    expect(planView('plan', plan(NOW - OLD_AFTER_S), watch('known'), NOW).asOf).toBeNull();
    expect(planView('plan', plan(NOW - OLD_AFTER_S - 1), watch('known'), NOW)).toEqual({ kind: 'known', asOf: NOW - OLD_AFTER_S - 1 });
    // From the last start, before the service has answered again: shown at once, with their time.
    expect(planView('plan', plan(NOW - 86_400), undefined, NOW)).toEqual({ kind: 'known', asOf: NOW - 86_400 });
    // Asking fails: the last figures stay, never passed off as current.
    expect(planView('plan', plan(NOW - 7200), watch('failed'), NOW)).toEqual({ kind: 'known', asOf: NOW - 7200 });
    expect(planView('plan', plan(NOW - 600), watch('failed'), NOW).asOf).toBe(NOW - 600);
    expect(planView('plan', plan(NOW - 60), watch('failed'), NOW).asOf).toBeNull();
    // An assistant that only reports with its answers (too old to be asked): always "as of" its last chat.
    expect(planView('plan', plan(NOW - 600), watch('unavailable', 'oldProgram'), NOW).asOf).toBe(NOW - 600);
    expect(planView('plan', plan(NOW - 600), watch('unavailable', 'off'), NOW).asOf).toBe(NOW - 600);
    // A refresh in flight never blanks what is there.
    expect(planView('plan', plan(NOW - 700), watch('known', null, true), NOW).kind).toBe('known');
  });

  it('says once, in plain words, that a key or another service has no plan limit', () => {
    expect(planView('key', null, watch('unavailable', 'key'), NOW).kind).toBe('key');
    expect(planView('key', plan(NOW - 60), watch('unavailable', 'key'), NOW).kind).toBe('key');
    expect(planView('plan', null, watch('unavailable', 'service'), NOW).kind).toBe('key');
    expect(planView('plan', null, watch('unavailable', 'key'), NOW).kind).toBe('key');
  });

  it('says when the assistant is not installed or not signed in', () => {
    expect(planView('missing', null, watch('unavailable', 'missing'), NOW).kind).toBe('missing');
    expect(planView('signedOut', null, undefined, NOW).kind).toBe('signedOut');
    expect(planView('plan', null, watch('unavailable', 'signedOut'), NOW).kind).toBe('signedOut');
    expect(planView('unknown', null, watch('unavailable', 'missing'), NOW).kind).toBe('missing');
  });

  it('gives the reason when a plan cannot be asked without a chat', () => {
    expect(planView('plan', null, watch('unavailable', 'oldProgram'), NOW).kind).toBe('old');
    expect(planView('plan', null, watch('unavailable', 'noReport'), NOW).kind).toBe('noReport');
    expect(planView('plan', null, watch('known'), NOW).kind).toBe('noReport');
    expect(planView('plan', null, watch('failed'), NOW).kind).toBe('failed');
    expect(planView('plan', null, watch('unavailable', 'off'), NOW).kind).toBe('waiting');
    expect(planView('plan', null, watch('unavailable', 'notice'), NOW).kind).toBe('waiting');
  });
});
