<script lang="ts">
  // Development only: the frame the Plan view sits in inside the app (a scrolling main area that is
  // a size container), plus the app's dialogs and toasts, and a strip to change what is shown.
  import Dialogs from '../../../lib/Dialogs.svelte';
  import Toasts from '../../../lib/Toasts.svelte';
  import { app } from '../../../lib/store.svelte';
  import MadoView from '../MadoView.svelte';
  import type { Fake } from './fake';

  let { fake, flags }: { fake: Fake; flags: { scenario: string; lang: string; mode: string; theme: string; motion: string } } = $props();
  const link = (change: Record<string, string>) => '?' + new URLSearchParams({ ...flags, ...change }).toString();
  const SCENARIOS = ['fresh', 'early', 'mid', 'review', 'blocked', 'building', 'done'];
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
  </div>
  <div class="mainscroll">
    {#if app.view.kind === 'home' || app.view.kind === 'mado'}
      <div class="view"><MadoView projectId={fake.projectId} /></div>
    {:else}
      <div class="left">The app would now show: <code>{JSON.stringify(app.view)}</code> <button type="button" class="btn" onclick={() => (app.view = { kind: 'mado', id: fake.projectId })}>back to the plan</button></div>
    {/if}
  </div>
</div>
<Dialogs />
<Toasts />

<style>
  .main { position: fixed; inset: 0; display: flex; flex-direction: column; background: var(--aura), var(--bg); background-repeat: no-repeat; container: main / inline-size; }
  .mainscroll { flex: 1; min-height: 0; overflow: auto; }
  .strip { flex: none; display: flex; gap: 10px; align-items: center; flex-wrap: wrap; padding: 4px 12px; font-size: 12px; background: var(--panel-2); border-bottom: 1px solid var(--hairline); }
  .strip a { color: var(--muted); text-decoration: none; }
  .strip a.on { color: var(--fg); font-weight: 700; }
  .strip button { font: inherit; }
  .left { padding: 60px; display: grid; gap: 12px; justify-items: start; }
</style>
