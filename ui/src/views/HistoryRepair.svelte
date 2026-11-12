<script lang="ts">
  // The history's health check and its repair, in plain words. Checks on open; when something is
  // wrong, says what and offers one button. The repair first keeps a copy of the library exactly as
  // it is, and afterwards lists any document that differs from that copy, to compare and bring back.
  import { onMount } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import DocChanges, { type Doc } from '../lib/DocChanges.svelte';
  import { api, post } from '../lib/api';
  import { toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { errDetail, failed } from '../lib/errors';
  import { t, tn } from '../lib/i18n';

  let { onclose, onrepaired }: { onclose?: () => void; onrepaired?: () => void } = $props();

  type Problem = { id: string; stale?: boolean; branch?: string };
  let phase = $state<'checking' | 'failed' | 'ready' | 'repairing' | 'done'>('checking');
  let checkError = $state('');
  let problems = $state<Problem[]>([]);
  // After a repair: the copy kept just before it, and what differs from it now.
  let snapshot = $state('');
  let differs = $state<string[]>([]);
  let rebuilt = $state<{ fromLocation: boolean } | null>(null);
  let broughtBack = $state<Record<string, boolean>>({});
  let busy = $state('');
  let shown = $state<Record<string, { loading: boolean; error: string; files: Doc[] | null }>>({});

  async function check() {
    phase = 'checking'; checkError = '';
    try {
      const r = await api('/library/history/check?thorough=1');
      problems = Array.isArray(r?.problems) ? r.problems : [];
      phase = 'ready';
    } catch (e) { checkError = errDetail(e); phase = 'failed'; }
  }
  onMount(check);

  const has = (id: string) => problems.some((p) => p.id === id);
  // Only a lock that is still in use: nothing to do but wait.
  const waitOnly = $derived(problems.length > 0 && problems.every((p) => p.id === 'locked' && !p.stale));
  const damaged = $derived(has('damaged'));
  const key = (p: Problem) => (p.id === 'locked' ? (p.stale ? 'lockedStale' : 'lockedNow') : p.id);

  async function repair() {
    phase = 'repairing';
    try {
      const r = await post('/library/history/repair', {});
      if (r?.rebuild) { problems = r.problems ?? problems; phase = 'ready'; return; }
      snapshot = String(r?.snapshot ?? '');
      differs = Array.isArray(r?.differs) ? r.differs : [];
      problems = Array.isArray(r?.problems) ? r.problems : [];
      phase = 'done';
      toast(t(r?.ok ? 'histc.repair.doneToast' : 'histc.repair.partToast'), { kind: r?.ok ? 'success' : 'error', key: 'history-state' });
      onrepaired?.();
    } catch (e) { phase = 'ready'; failed(t('histc.repair.failed'), e); }
  }

  async function rebuild() {
    const ok = await confirmDialog({ title: t('histc.rebuild.title'), body: t('histc.rebuild.body'), note: t('histc.rebuild.note'), confirmLabel: t('histc.rebuild.go') });
    if (!ok) return;
    phase = 'repairing';
    try {
      const r = await post('/library/history/repair', { rebuild: true });
      rebuilt = { fromLocation: !!r?.fromLocation };
      problems = []; differs = []; snapshot = '';
      phase = 'done';
      toast(t('histc.repair.doneToast'), { kind: 'success', key: 'history-state' });
      onrepaired?.();
    } catch (e) { phase = 'ready'; failed(t('histc.repair.failed'), e); }
  }

  async function compare(path: string) {
    if (shown[path] && !shown[path].error) { delete shown[path]; return; }
    shown[path] = { loading: true, error: '', files: null };
    try {
      const r = await api(`/library/compare?path=${encodeURIComponent(path)}&hash=${encodeURIComponent(snapshot)}`);
      shown[path] = { loading: false, error: '', files: Array.isArray(r?.files) ? r.files : [] };
    } catch (e) { shown[path] = { loading: false, error: errDetail(e) || t('histc.repair.compareFailed'), files: null }; }
  }

  async function bringBack(path: string) {
    busy = path;
    try {
      await post('/library/restore', { path, hash: snapshot });
      broughtBack[path] = true;
      delete shown[path];
      toast(t('histc.repair.broughtBack'), { kind: 'success' });
      onrepaired?.();
    } catch (e) { failed(t('histc.repair.bringBackFailed'), e); }
    finally { busy = ''; }
  }
</script>

<section class="card repair" aria-labelledby="repair-title" aria-busy={phase === 'checking' || phase === 'repairing'}>
  <div class="row head">
    <span class="ki" aria-hidden="true"><Icon name="tool" size={16} /></span>
    <h2 id="repair-title" class="grow">{t('histc.repair.title')}</h2>
    {#if onclose && phase !== 'repairing'}<button type="button" class="btn ghost x" onclick={onclose} aria-label={t('close')} title={t('close')}><Icon name="close" size={15} /></button>{/if}
  </div>

  {#if phase === 'checking'}
    <p class="muted" role="status">{t('histc.repair.checking')}</p>
    <div class="skel"></div>
  {:else if phase === 'failed'}
    <div role="alert">
      <p>{t('histc.repair.checkFailed')}</p>
      {#if checkError}<p class="muted sm" dir="auto">{checkError}</p>{/if}
      <div class="row acts"><button type="button" class="btn" onclick={check}><Icon name="retry" size={14} /> {t('histc.repair.again')}</button></div>
    </div>
  {:else if phase === 'repairing'}
    <p class="muted" role="status">{t('histc.repair.working')}</p>
    <div class="skel"></div>
  {:else if phase === 'done'}
    <div role="status">
      {#if rebuilt}
        <p class="ok"><Icon name="check" size={15} /> {t(rebuilt.fromLocation ? 'histc.rebuild.doneOnline' : 'histc.rebuild.doneHere')}</p>
        <p class="muted sm">{t('histc.rebuild.aside')}</p>
      {:else if problems.length}
        <p><Icon name="warning" size={15} /> {t('histc.repair.notAll')}</p>
      {:else}
        <p class="ok"><Icon name="check" size={15} /> {t('histc.repair.done')}</p>
      {/if}
    </div>
    {#if differs.length}
      <p class="muted">{tn('histc.repair.differs', differs.length)}</p>
      <ul class="differs">
        {#each differs as d (d)}
          <li>
            <div class="row line">
              <span class="grow name"><bdi>{d}</bdi></span>
              {#if broughtBack[d]}
                <span class="muted sm"><Icon name="check" size={13} /> {t('histc.repair.isBack')}</span>
              {:else}
                <button type="button" class="btn ghost sm" aria-expanded={!!shown[d] && !shown[d].error} onclick={() => compare(d)}><Icon name="eye" size={14} /> {t(shown[d] && !shown[d].error ? 'histc.clash.hide' : 'histc.clash.compare')}</button>
                <button type="button" class="btn sm" disabled={!!busy} aria-busy={busy === d} onclick={() => bringBack(d)}><Icon name="rollback" size={14} /> {t('histc.repair.bringBack')}</button>
              {/if}
            </div>
            {#if shown[d] && !broughtBack[d]}
              <div class="cmp" aria-live="polite">
                {#if shown[d].loading}<div class="skel"></div>
                {:else if shown[d].error}<p class="muted sm" role="alert" dir="auto">{shown[d].error}</p>
                {:else}<DocChanges files={shown[d].files ?? []} single labels={{ added: t('histc.repair.onlyNow'), removed: t('histc.repair.onlyCopy') }} />{/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if problems.length}
      <ul class="probs">
        {#each problems as p (p.id)}<li><strong>{t(`histc.p.${key(p)}.title`)}</strong><span class="muted sm">{t(`histc.p.${key(p)}.body`)}</span></li>{/each}
      </ul>
      <div class="row acts"><button type="button" class="btn" onclick={check}><Icon name="retry" size={14} /> {t('histc.repair.again')}</button></div>
    {/if}
  {:else if !problems.length}
    <p class="ok" role="status"><Icon name="check" size={15} /> {t('histc.repair.healthy')}</p>
    <div class="row acts"><button type="button" class="btn ghost sm" onclick={check}><Icon name="retry" size={14} /> {t('histc.repair.again')}</button></div>
  {:else}
    <p>{t('histc.repair.found')}</p>
    <ul class="probs">
      {#each problems as p (p.id)}<li><strong>{t(`histc.p.${key(p)}.title`)}</strong><span class="muted sm">{t(`histc.p.${key(p)}.body`)}</span></li>{/each}
    </ul>
    {#if damaged}
      <p class="note">{t('histc.rebuild.explain')}</p>
      <div class="row acts">
        <button type="button" class="btn primary" onclick={rebuild}><Icon name="tool" size={14} /> {t('histc.rebuild.go')}</button>
      </div>
    {:else if waitOnly}
      <div class="row acts"><button type="button" class="btn primary" onclick={check}><Icon name="retry" size={14} /> {t('histc.repair.again')}</button></div>
    {:else}
      <p class="note">{t('histc.repair.safe')}</p>
      <div class="row acts"><button type="button" class="btn primary" onclick={repair}><Icon name="tool" size={14} /> {t('histc.repair.action')}</button></div>
    {/if}
  {/if}
</section>

<style>
  .repair { padding: 14px 16px; display: grid; gap: 10px; margin-bottom: 18px; }
  .head { gap: 10px; align-items: center; }
  .head h2 { margin: 0; font-size: 15px; }
  .ki { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); flex: none; }
  .x { padding: 4px 6px; }
  p { margin: 0; }
  .ok { color: var(--ok); display: flex; gap: 6px; align-items: center; }
  .probs { margin: 0; padding-inline-start: 18px; display: grid; gap: 8px; }
  .probs li { display: grid; gap: 2px; }
  .note { font-size: 13px; background: var(--fill); border-radius: var(--radius); padding: 8px 10px; }
  .acts { gap: 8px; flex-wrap: wrap; }
  .differs { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .differs li { border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 6px 10px; display: grid; gap: 6px; }
  .line { gap: 8px; flex-wrap: wrap; align-items: center; }
  .name { overflow-wrap: anywhere; min-width: 0; }
  .cmp { max-height: 50vh; overflow: auto; }
  .skel { height: 14px; width: 60%; }
  .sm { font-size: var(--fs-xs); }
</style>
