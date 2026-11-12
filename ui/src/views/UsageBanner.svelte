<script lang="ts">
  // Two plain notices at the top of the window: the monthly spending limit the user set (close to it,
  // reached), and the plan's own usage limit as the assistant reports it, with when it resets.
  import { app, go } from '../lib/store.svelte';
  import { fmtDate, fmtMoney, t } from '../lib/i18n';
  import Icon from '../lib/Icon.svelte';

  let hidden = $state<string[]>([]);
  const usage = $derived(app.status?.usage);
  const plan = $derived(app.status?.planLimit);

  const spendKey = $derived(usage && (usage.level === 'warn' || usage.level === 'reached') ? `spend:${usage.level}:${usage.limitUsd}` : '');
  const planKey = $derived(plan ? `plan:${plan.reached ? 'r' : 'w'}:${plan.resetsAt ?? ''}` : '');
  const showSpend = $derived(!!spendKey && !hidden.includes(spendKey));
  const showPlan = $derived(!!planKey && !hidden.includes(planKey));

  /** "today at 3:00 PM", or the date and time, in the app's language. */
  function when(epoch: number | null | undefined): string {
    if (!epoch) return '';
    const d = new Date(epoch * 1000);
    const today = new Date();
    const same = d.getFullYear() === today.getFullYear() && d.getMonth() === today.getMonth() && d.getDate() === today.getDate();
    return same ? t('runc.plan.today', { time: fmtDate(d, { timeStyle: 'short' }) }) : t('runc.plan.later', { date: fmtDate(d, { dateStyle: 'medium', timeStyle: 'short' }) });
  }
  const planText = $derived.by(() => {
    if (!plan) return '';
    const w = when(plan.resetsAt);
    return plan.reached ? (w ? t('runc.plan.reached', { when: w }) : t('runc.plan.reachedUnknown')) : (w ? t('runc.plan.warn', { when: w }) : t('runc.plan.warnUnknown'));
  });
</script>

{#if showPlan}
  <div class="safe no-print" role={plan.reached ? 'alert' : 'status'}>
    <Icon name="warning" size={16} />
    <span class="grow" dir="auto">{planText}</span>
    <button type="button" class="btn ghost" onclick={() => hidden.push(planKey)}>{t('runc.banner.hide')}</button>
  </div>
{/if}
{#if showSpend}
  <div class="safe no-print" role={usage.level === 'reached' ? 'alert' : 'status'}>
    <Icon name="warning" size={16} />
    <span class="grow" dir="auto">{usage.level === 'reached'
      ? t('runc.banner.reached', { limit: fmtMoney(usage.limitUsd) })
      : t('runc.banner.warn', { percent: usage.percent ?? 0, spent: fmtMoney(usage.monthCostUsd), limit: fmtMoney(usage.limitUsd) })}</span>
    <button type="button" class="btn" onclick={() => go({ kind: 'settings', id: 'privacy' })}>{t('runc.banner.change')}</button>
    <button type="button" class="btn ghost" onclick={() => hidden.push(spendKey)}>{t('runc.banner.hide')}</button>
  </div>
{/if}

<style>
  .safe { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin: 4px 28px 0; padding: 10px 14px; border-radius: var(--radius-lg); background: var(--panel); border: var(--border-w) solid var(--accent-2); font-size: 13px; }
  .safe > :global(.icon) { flex: none; color: var(--accent-2); }
  .safe .grow { flex: 1; min-width: 220px; }
</style>
