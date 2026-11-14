<script lang="ts">
  // The Usage page: each assistant's plan limits, and the tokens used — in total, by day, by chat and
  // by model — with what they would cost at API prices.
  import { onMount, untrack } from 'svelte';
  import Icon from '../../lib/Icon.svelte';
  import { app, go } from '../../lib/store.svelte';
  import { ago, fmtDate, fmtNum, t, tn } from '../../lib/i18n';
  import { costText, followMeter, fmtTokens, loadMeter, meter, providerName, providerState, PROVIDERS, type Use } from '../../lib/meter.svelte';
  import PlanBars from './PlanBars.svelte';

  const PERIODS = [7, 30, 90] as const;
  let days = $state<number>(30);
  let data = $state<any>(null);
  let loading = $state(true);
  let error = $state('');
  let measure = $state<'tokens' | 'cost'>('tokens');
  let asTable = $state(false);
  let hover = $state<number | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function load() {
    try { data = await loadMeter(days); error = ''; }
    catch (e: any) { error = e?.message ?? String(e); }
    finally { loading = false; }
  }
  onMount(() => { followMeter(); return () => clearTimeout(timer); });
  $effect(() => { days; app.settings?.spending?.wholeComputer; untrack(() => { loading = !data; load(); }); });
  let seen = meter.tick;
  $effect(() => { const n = meter.tick; if (n !== seen) { seen = n; clearTimeout(timer); timer = setTimeout(load, 800); } });

  const totals = $derived<Record<string, Use>>(data?.totals ?? {});
  const any = $derived(Object.values(totals).some((u) => u?.count > 0));
  const everything = $derived(data?.scope === 'computer');
  const shownProviders = $derived(PROVIDERS.filter((p) => providerState(p) !== 'missing' || (totals[p]?.count ?? 0) > 0));
  const plansShown = $derived(shownProviders.length ? shownProviders : PROVIDERS);

  // ----- by day -----
  const dayVal = (u: Use | undefined) => (!u ? 0 : measure === 'cost' ? u.costUsd : u.totalTokens);
  const dayList = $derived<any[]>(data?.days ?? []);
  const dayMax = $derived(Math.max(0, ...dayList.map((d) => PROVIDERS.reduce((s, p) => s + dayVal(d.usage?.[p]), 0))));
  const dayLabel = (d: string) => fmtDate(d + 'T12:00:00', { day: 'numeric', month: 'short' });
  const dayLong = (d: string) => fmtDate(d + 'T12:00:00', { weekday: 'long', day: 'numeric', month: 'long' });
  const valText = (u: Use | undefined) => (measure === 'cost' ? costText(u) : t('r10-usage.tokensN', { n: fmtTokens(u?.totalTokens ?? 0) }));
  const tickEvery = $derived(dayList.length > 31 ? 14 : dayList.length > 10 ? 7 : 1);

  // ----- by chat -----
  function rowTitle(r: any): string {
    if (r.kind === 'chat') return r.title || t('newChat');
    if (r.kind === 'deleted') return t('r10-usage.row.deleted');
    if (r.kind === 'outside') return r.folder ? t('r10-usage.row.outsideIn', { folder: r.folder }) : t('r10-usage.row.outside');
    return t('r10-usage.row.background');
  }
  const kindLabel = (k: string) => (k && k !== 'chat' ? t(`chat.kind.${k}`) : '');
</script>

<div class="page usage">
  <div class="row head">
    <div class="grow">
      <h1>{t('r10-usage.title')}</h1>
      <p class="muted sub">{t('r10-usage.sub')}</p>
    </div>
    <div class="seg" role="group" aria-label={t('r10-usage.period')}>
      {#each PERIODS as p (p)}<button type="button" class="segb" aria-pressed={days === p} onclick={() => (days = p)}>{tn('r10-usage.days', p)}</button>{/each}
    </div>
  </div>

  <section aria-labelledby="u-plans">
    <h2 id="u-plans">{t('r10-usage.plans')}</h2>
    <p class="muted sm">{t('r10-usage.plansHint')}</p>
    <div class="grid2">
      {#each plansShown as p (p)}
        <div class="card"><PlanBars provider={p} plan={data?.plans?.[p] ?? null} /></div>
      {/each}
    </div>
  </section>

  {#if loading && !data}
    <div class="skel big" aria-busy="true"><span class="sr-only" role="status">{t('shell.loading')}</span></div>
  {:else if error && !data}
    <div class="state" role="alert"><Icon name="warning" size={24} /><p>{t('r10-usage.loadFailed')}</p><p class="muted sm" dir="auto">{error}</p>
      <button class="btn primary" onclick={() => { loading = true; load(); }}><Icon name="retry" size={14} /> {t('ui.retry')}</button></div>
  {:else}
    <section aria-labelledby="u-cost">
      <div class="row sh">
        <h2 id="u-cost" class="grow">{t('r10-usage.costTitle')}</h2>
        <span class="pill">{everything ? t('r10-usage.scope.computer') : t('r10-usage.scope.thoughts')}</span>
        <button type="button" class="btn ghost sm" onclick={() => go({ kind: 'settings', id: 'usage' })}>{t('r10-usage.change')}</button>
      </div>
      <p class="muted sm">{t('r10-usage.costHint')}</p>
      {#if everything && data?.scan && !data.scan.claudeFound && !data.scan.codexFound}
        <p class="note sm" role="status"><Icon name="info" size={14} /> {t('r10-usage.scan.none')}</p>
      {:else if everything && data?.scan?.unreadable > 0}
        <p class="note sm" role="status"><Icon name="info" size={14} /> {t('r10-usage.scan.unreadable', { n: fmtNum(data.scan.unreadable) })}</p>
      {/if}

      {#if !any}
        <div class="state">
          <Icon name="usage" size={28} />
          <h3>{t('r10-usage.empty.title')}</h3>
          <p>{everything ? t('r10-usage.empty.computer') : t('r10-usage.empty.thoughts')}</p>
        </div>
      {:else}
        <div class="grid2">
          {#each PROVIDERS.filter((p) => totals[p]?.count) as p (p)}
            {@const u = totals[p]}
            <div class="card tot">
              <div class="row"><b class="grow">{providerName(p)}</b>{#if u.estimated}<span class="pill" title={t('r10-usage.estimateNote')}>{t('r10-usage.estimate')}</span>{/if}</div>
              <div class="big"><bdi>{costText(u)}</bdi></div>
              <p class="muted sm">{t('r10-usage.atApi')}</p>
              <dl>
                <dt>{t('r10-usage.tok.input')}</dt><dd>{fmtTokens(u.tokens.input)}</dd>
                <dt>{t('r10-usage.tok.output')}</dt><dd>{fmtTokens(u.tokens.output)}</dd>
                <dt>{t('r10-usage.tok.cacheRead')}</dt><dd>{fmtTokens(u.tokens.cacheRead)}</dd>
                <dt>{t('r10-usage.tok.cacheWrite')}</dt><dd>{fmtTokens(u.tokens.cacheWrite)}</dd>
                <dt class="tt">{t('r10-usage.tok.total')}</dt><dd class="tt">{fmtTokens(u.totalTokens)}</dd>
              </dl>
              {#if u.unpricedTokens > 0}<p class="muted sm">{t('r10-usage.unpriced', { n: fmtTokens(u.unpricedTokens) })}</p>{/if}
            </div>
          {/each}
        </div>

        <section aria-labelledby="u-days" class="sub2">
          <div class="row sh">
            <h3 id="u-days" class="grow">{t('r10-usage.byDay')}</h3>
            <div class="seg" role="group" aria-label={t('r10-usage.measure')}>
              <button type="button" class="segb" aria-pressed={measure === 'tokens'} onclick={() => (measure = 'tokens')}>{t('r10-usage.m.tokens')}</button>
              <button type="button" class="segb" aria-pressed={measure === 'cost'} onclick={() => (measure = 'cost')}>{t('r10-usage.m.cost')}</button>
            </div>
            <button type="button" class="btn ghost sm" aria-pressed={asTable} onclick={() => (asTable = !asTable)}>{asTable ? t('r10-usage.asChart') : t('r10-usage.asTable')}</button>
          </div>
          <ul class="legend" aria-hidden={asTable}>
            {#each PROVIDERS as p (p)}<li><span class="sw {p}"></span>{providerName(p)}</li>{/each}
          </ul>
          {#if asTable}
            <table class="tbl">
              <thead><tr><th scope="col">{t('r10-usage.day')}</th>{#each PROVIDERS as p (p)}<th scope="col">{providerName(p)}</th>{/each}</tr></thead>
              <tbody>{#each [...dayList].reverse() as d (d.day)}<tr><th scope="row">{dayLong(d.day)}</th>{#each PROVIDERS as p (p)}<td>{d.usage?.[p] ? valText(d.usage[p]) : '–'}</td>{/each}</tr>{/each}</tbody>
            </table>
          {:else}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="chart" onmouseleave={() => (hover = null)} role="img" aria-label={t('r10-usage.chartLabel', { n: fmtNum(dayList.length) })}>
              {#each dayList as d, i (d.day)}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div class="col" onmouseenter={() => (hover = i)}>
                  <div class="stack">
                    {#each PROVIDERS as p (p)}
                      {@const v = dayVal(d.usage?.[p])}
                      {#if v > 0 && dayMax > 0}<span class="seg-{p}" style="block-size: {Math.max(2, (v / dayMax) * 100)}%"></span>{/if}
                    {/each}
                  </div>
                  <span class="tick">{#if (dayList.length - 1 - i) % tickEvery === 0}{dayLabel(d.day)}{/if}</span>
                </div>
              {/each}
              {#if hover != null && dayList[hover]}
                {@const d = dayList[hover]}
                <div class="tip surface" style="inset-inline-start: {Math.min(Math.max((hover + 0.5) / dayList.length * 100, 12), 88)}%">
                  <b>{dayLong(d.day)}</b>
                  {#each PROVIDERS as p (p)}<div class="row"><span class="sw {p}"></span><span class="grow">{providerName(p)}</span><bdi>{d.usage?.[p] ? valText(d.usage[p]) : '–'}</bdi></div>{/each}
                </div>
              {/if}
            </div>
          {/if}
        </section>

        <section aria-labelledby="u-chats" class="sub2">
          <h3 id="u-chats">{t('r10-usage.byChat')}</h3>
          <ul class="rows">
            {#each data.chats as r, i (i)}
              <li class="urow">
                {#if r.kind === 'chat'}
                  <button type="button" class="name ell" dir="auto" onclick={() => go({ kind: 'chat', id: r.chatId })}>{rowTitle(r)}</button>
                {:else}
                  <span class="name ell muted" dir="auto">{rowTitle(r)}</span>
                {/if}
                {#if kindLabel(r.chatKind)}<span class="pill">{kindLabel(r.chatKind)}</span>{/if}
                {#each r.providers as p (p)}<span class="pill">{providerName(p)}</span>{/each}
                <span class="muted sm when">{ago(r.last)}</span>
                <span class="num">{fmtTokens(r.usage.totalTokens)}</span>
                <span class="num cost"><bdi>{costText(r.usage)}</bdi></span>
              </li>
            {/each}
          </ul>
        </section>

        <section aria-labelledby="u-models" class="sub2">
          <h3 id="u-models">{t('r10-usage.byModel')}</h3>
          <table class="tbl">
            <thead><tr><th scope="col">{t('r10-usage.model')}</th><th scope="col">{t('r10-usage.tok.input')}</th><th scope="col">{t('r10-usage.tok.output')}</th><th scope="col">{t('r10-usage.tok.cache')}</th><th scope="col">{t('r10-usage.m.cost')}</th></tr></thead>
            <tbody>
              {#each data.models as m, i (i)}
                <tr>
                  <th scope="row"><span class="muted sm">{providerName(m.provider)}</span> <bdi dir="ltr" class="mono">{m.model || t('r10-usage.modelUnknown')}</bdi></th>
                  <td>{fmtTokens(m.usage.tokens.input)}</td><td>{fmtTokens(m.usage.tokens.output)}</td>
                  <td>{fmtTokens(m.usage.tokens.cacheRead + m.usage.tokens.cacheWrite)}</td>
                  <td>{#if m.priced}<bdi>{costText(m.usage)}</bdi>{:else}<span class="muted" title={t('r10-usage.noPrice')}>{t('r10-usage.unknown')}</span>{/if}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </section>
      {/if}
      <p class="muted sm foot">
        {t('r10-usage.footApi')}
        {#if !everything && data?.firstRecord} {t('r10-usage.footSince', { when: fmtDate(data.firstRecord) })}{/if}
      </p>
    </section>
  {/if}
</div>

<style>
  .usage { display: flex; flex-direction: column; gap: 22px; }
  .head { align-items: flex-start; gap: 16px; flex-wrap: wrap; }
  .head h1 { margin: 0; }
  .sub { margin: 4px 0 0; }
  h2 { font-size: 17px; margin: 0 0 4px; }
  h3 { font-size: 15px; margin: 0; }
  section > p.sm { margin: 0 0 12px; }
  .sh { flex-wrap: wrap; margin-bottom: 4px; }
  .sub2 { margin-top: 22px; }
  .grid2 { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 14px; }
  .card { padding: 16px; }
  .tot .big { font-size: 26px; font-weight: 650; margin-top: 6px; font-variant-numeric: tabular-nums; }
  .tot p { margin: 2px 0 10px; }
  dl { margin: 0; display: grid; grid-template-columns: 1fr auto; gap: 4px 12px; font-size: 13px; }
  dt { color: var(--muted); }
  dd { margin: 0; text-align: end; font-variant-numeric: tabular-nums; }
  .tt { border-top: var(--border-w, 1px) solid var(--hairline); padding-top: 4px; font-weight: 600; color: var(--fg); }
  .sm { font-size: var(--fs-xs); }
  .btn.sm { font-size: 12px; padding: 4px 10px; }
  .note { display: flex; gap: 6px; align-items: center; margin: 0 0 12px; }
  .legend { list-style: none; display: flex; gap: 16px; margin: 6px 0 8px; padding: 0; font-size: 12px; color: var(--muted); }
  .legend li { display: inline-flex; align-items: center; gap: 6px; }
  .sw { inline-size: 10px; block-size: 10px; border-radius: 3px; display: inline-block; flex: none; }
  .sw.claude, .seg-claude { background: var(--accent); }
  .sw.codex, .seg-codex { background: color-mix(in srgb, var(--fg) 50%, transparent); }
  .chart { position: relative; display: flex; align-items: stretch; gap: 2px; block-size: 170px; padding-block-end: 18px; border-block-end: var(--border-w, 1px) solid var(--hairline); }
  .col { flex: 1; min-inline-size: 0; position: relative; display: flex; flex-direction: column; justify-content: flex-end; }
  .col:hover .stack { background: var(--fill); }
  .stack { flex: 1; display: flex; flex-direction: column-reverse; justify-content: flex-start; gap: 2px; border-radius: 4px 4px 0 0; }
  .stack span { display: block; inline-size: 100%; max-inline-size: 22px; margin-inline: auto; }
  .stack span:last-child { border-radius: 4px 4px 0 0; }
  .tick { position: absolute; inset-block-end: -18px; inset-inline-start: 0; font-size: 10px; color: var(--muted); white-space: nowrap; }
  .tip { position: absolute; inset-block-start: 4px; transform: translateX(-50%); padding: 8px 10px; font-size: 12px; display: flex; flex-direction: column; gap: 4px; min-inline-size: 170px; pointer-events: none; z-index: 2; box-shadow: var(--shadow); }
  :global([dir="rtl"]) .tip { transform: translateX(50%); }
  .tbl { inline-size: 100%; border-collapse: collapse; font-size: 13px; }
  .tbl th, .tbl td { text-align: start; padding: 6px 8px; border-block-end: var(--border-w, 1px) solid var(--hairline); font-weight: 400; }
  .tbl thead th { color: var(--muted); font-size: 12px; }
  .tbl td { font-variant-numeric: tabular-nums; }
  .mono { font-family: var(--mono); font-size: 12px; }
  .rows { list-style: none; margin: 8px 0 0; padding: 0; display: flex; flex-direction: column; }
  .urow { display: flex; align-items: center; gap: 8px; padding: 8px 2px; border-block-end: var(--border-w, 1px) solid var(--hairline); flex-wrap: wrap; }
  .name { flex: 1 1 200px; min-inline-size: 0; font: inherit; text-align: start; background: none; border: 0; padding: 0; color: inherit; }
  button.name { cursor: pointer; }
  button.name:hover { color: var(--accent); text-decoration: underline; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .num { font-variant-numeric: tabular-nums; min-inline-size: 60px; text-align: end; font-size: 13px; }
  .num.cost { min-inline-size: 90px; }
  .when { min-inline-size: 70px; text-align: end; }
  .state { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 8px; padding: 32px 0; color: var(--muted); }
  .state h3 { color: var(--fg); margin: 4px 0 0; }
  .state p { margin: 0; max-inline-size: 460px; }
  .skel.big { block-size: 220px; border-radius: var(--radius-lg); background: var(--fill); }
  .foot { margin-top: 18px; }
  @container main (max-width: 620px) { .when { display: none; } }
</style>
