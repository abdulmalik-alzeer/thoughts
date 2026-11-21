<script lang="ts">
  // Development only. One fixture per kind of diagram, in both languages and every theme, with a log of what the
  // component reports. The page plays the parent: it keeps the pending edits and hands them back.
  import { untrack } from 'svelte';
  import MadoGraph from '../MadoGraph.svelte';
  import type { Comment, GraphOp, Pos } from '../../types';
  import { app } from '../../../store.svelte';
  import { FIXTURES, type FixtureName } from './fixtures';

  const q = new URLSearchParams(location.search);
  let name = $state<FixtureName>((q.get('f') as FixtureName) in FIXTURES ? (q.get('f') as FixtureName) : 'flow');
  let arabic = $state(q.get('labels') === 'ar');
  let language = $state(q.get('lang') === 'ar' ? 'ar' : 'en');
  let theme = $state(q.get('theme') ?? 'saudi');
  let mode = $state(q.get('mode') ?? 'light');
  let editable = $state(q.get('ro') !== '1');
  let controlled = $state(true);
  let ops = $state.raw<GraphOp[]>([]);
  let comments = $state.raw<Comment[]>([]);
  let saved = $state.raw<Record<string, Pos>>({});
  let log = $state<string[]>([]);
  const say = (what: string, v: unknown) => { log = [`${what} ${JSON.stringify(v)}`, ...log].slice(0, 40); };

  const doc = $derived.by(() => {
    const d = FIXTURES[name](arabic);
    return { ...d, nodes: (d.nodes ?? []).map((n: any) => (n && saved[n.id] ? { ...n, pos: saved[n.id] } : n)) };
  });
  $effect(() => { void name; void arabic; ops = []; comments = name === 'flow' ? [
    { id: 'c1', target: { kind: 'node', file: 'flow.graph.json', nodeId: 'sign-in' }, body: 'x', createdAt: '', status: 'open' },
    { id: 'c2', target: { kind: 'node', file: 'flow.graph.json', nodeId: 'home' }, body: 'x', createdAt: '', status: 'sent' },
    { id: 'c3', target: { kind: 'edge', file: 'flow.graph.json', edgeId: 'e3' }, body: 'x', createdAt: '', status: 'open' },
  ] : []; saved = {}; });
  $effect(() => {
    const l = language;
    untrack(() => { (app as any).settings = { ...(app.settings ?? {}), language: l }; });
    const r = document.documentElement;
    r.lang = language; r.dir = language === 'ar' ? 'rtl' : 'ltr'; r.dataset.theme = theme; r.dataset.mode = mode;
  });
</script>

<main>
  <header>
    <label>Diagram <select bind:value={name}>{#each Object.keys(FIXTURES) as f (f)}<option>{f}</option>{/each}</select></label>
    <label><input type="checkbox" bind:checked={arabic} /> Arabic labels</label>
    <label>App language <select bind:value={language}><option>en</option><option>ar</option></select></label>
    <label>Theme <select bind:value={theme}><option>saudi</option><option>contrast</option><option>tech</option></select></label>
    <label>Mode <select bind:value={mode}><option>light</option><option>dark</option></select></label>
    <label><input type="checkbox" bind:checked={editable} /> editable</label>
    <label><input type="checkbox" bind:checked={controlled} /> hand pendingOps back</label>
    <button class="btn" onclick={() => (ops = [])}>Parent clears pending (as after "send")</button>
  </header>
  <div class="stage">
    {#key name + arabic}
      <MadoGraph {doc} {comments} {editable} pendingOps={controlled ? ops : undefined}
        onops={(o) => { ops = o; say('onops', o); }}
        onpositions={(p) => { saved = { ...saved, ...p }; say('onpositions', p); }}
        oncomment={(t) => {
          say('oncomment', t);
          comments = [...comments, { id: `c${Date.now()}`, target: { ...t, file: 'x.graph.json' } as Comment['target'], body: 'x', createdAt: '', status: 'open' }];
        }}
        onselect={(s) => say('onselect', s)} />
    {/key}
  </div>
  <section class="log" dir="ltr">
    <b>Pending ops ({ops.length})</b>
    <pre data-testid="ops">{JSON.stringify(ops, null, 1)}</pre>
    <b>Events (newest first)</b>
    <pre data-testid="log">{log.join('\n')}</pre>
  </section>
</main>

<style>
  main { display: grid; grid-template-rows: auto minmax(0, 1fr) 110px; gap: 10px; height: 100vh; padding: 12px; box-sizing: border-box; }
  header { display: flex; gap: 14px; flex-wrap: wrap; align-items: center; font-size: 13px; }
  header select { width: auto; }
  .stage { min-height: 0; }
  .log { display: grid; grid-template-columns: 1fr 1fr; grid-template-rows: auto 1fr; gap: 4px 12px; font-size: 12px; min-height: 0; }
  .log b:nth-of-type(2) { grid-column: 2; grid-row: 1; }
  .log pre { margin: 0; overflow: auto; font-size: 11px; padding: 6px; min-height: 0; }
</style>
