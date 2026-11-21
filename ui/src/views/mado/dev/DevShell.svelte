<script lang="ts">
  // Development only: the frame the Plan view sits in inside the app (a scrolling main area that is
  // a size container), plus the app's dialogs and toasts, and a strip to change what is shown.
  import Dialogs from '../../../lib/Dialogs.svelte';
  import Toasts from '../../../lib/Toasts.svelte';
  import { app } from '../../../lib/store.svelte';
  import MadoView from '../MadoView.svelte';
  import PlansGroup from '../PlansGroup.svelte';
  import type { Fake } from './fake';

  let { fake, flags }: { fake: Fake; flags: { scenario: string; lang: string; mode: string; theme: string; motion: string } } = $props();
  const link = (change: Record<string, string>) => '?' + new URLSearchParams({ ...flags, ...change }).toString();
  const SCENARIOS = ['fresh', 'early', 'mid', 'review', 'blocked', 'building', 'checking', 'reviewing', 'reviewed', 'done', 'landed'];
  let side = $state(true);
</script>

<div class="main">
  <div class="strip" dir="ltr">
    <b>dev preview</b>
    {#each SCENARIOS as s}<a href={link({ scenario: s })} class:on={flags.scenario === s}>{s}</a>{/each}
    <span>·</span>
    <a href={link({ lang: flags.lang === 'ar' ? 'en' : 'ar' })}>{flags.lang === 'ar' ? 'English' : 'العربية'}</a>
    <a href={link({ mode: flags.mode === 'dark' ? 'light' : 'dark' })}>{flags.mode === 'dark' ? 'light' : 'dark'}</a>
    {#each ['saudi', 'contrast', 'tech'] as th}<a href={link({ theme: th })} class:on={flags.theme === th}>{th}</a>{/each}
    <span>·</span>
    <button type="button" onclick={() => fake.finishRuns()}>finish runs</button>
    <button type="button" onclick={() => fake.rewritePlan()}>change plan.json</button>
    <button type="button" onclick={() => fake.lastRound()}>last round done</button>
    <button type="button" onclick={() => (side = !side)}>sidebar</button>
  </div>
  <div class="body">
  {#if side}<aside class="devside" aria-label="Sidebar (preview)"><div class="grp">Chats</div><PlansGroup projects={[{ id: fake.projectId, name: 'Corner Bakery' }]} /></aside>{/if}
  <div class="mainscroll">
    {#if app.view.kind === 'home' || app.view.kind === 'mado'}
      {#if app.view.kind === 'mado' && app.view.id !== fake.projectId}
        <div class="left">The app would now show the plan of project <code>{app.view.id}</code> <button type="button" class="btn" onclick={() => (app.view = { kind: 'mado', id: fake.projectId })}>back to the plan</button></div>
      {:else}
      <div class="view"><MadoView projectId={fake.projectId} /></div>
      {/if}
    {:else}
      <div class="left">The app would now show: <code>{JSON.stringify(app.view)}</code> <button type="button" class="btn" onclick={() => (app.view = { kind: 'mado', id: fake.projectId })}>back to the plan</button></div>
    {/if}
  </div>
  </div>
</div>
<Dialogs />
<Toasts />

<style>
  .main { position: fixed; inset: 0; display: flex; flex-direction: column; background: var(--aura), var(--bg); background-repeat: no-repeat; container: main / inline-size; }
  .body { flex: 1; min-height: 0; display: flex; }
  .devside { flex: none; width: 250px; padding: 10px 12px; overflow: auto; background: var(--sidebar, var(--panel-2)); border-inline-end: 1px solid var(--hairline); }
  .grp { font-size: var(--fs-xs); font-weight: 600; color: var(--muted); padding: 0 6px; }
  .mainscroll { flex: 1; min-width: 0; min-height: 0; overflow: auto; container: main / inline-size; }
  .strip { flex: none; display: flex; gap: 10px; align-items: center; flex-wrap: wrap; padding: 4px 12px; font-size: 12px; background: var(--panel-2); border-bottom: 1px solid var(--hairline); }
  .strip a { color: var(--muted); text-decoration: none; }
  .strip a.on { color: var(--fg); font-weight: 700; }
  .strip button { font: inherit; }
  .left { padding: 60px; display: grid; gap: 12px; justify-items: start; }
</style>
