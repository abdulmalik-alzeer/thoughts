<script lang="ts">
  // Dev only (see index.html): the design step with made-up data, plus the hostile mock-up shown
  // raw in the sealed frame, with a counter of anything it managed to send out.
  import { app } from '../../../store.svelte';
  import type { Comment, CommentTarget } from '../../types';
  import { useClient } from '../client';
  import DesignStep from '../DesignStep.svelte';
  import MockFrame from '../MockFrame.svelte';
  import { FALLBACK } from '../tokens';
  import { HOSTILE as RAW, fakeClient } from './fixtures';

  // ?evil=<port>: the hostile page's addresses point at a listener of yours on that port, so its log proves nothing arrived.
  const evil = new URLSearchParams(location.search).get('evil');
  const HOSTILE = evil && /^\d+$/.test(evil) ? RAW.replaceAll('127.0.0.1:9/', `127.0.0.1:${evil}/`) : RAW;

  let lines = $state<string[]>([]);
  const log = (l: string) => { lines = [l, ...lines].slice(0, 12); };
  useClient(fakeClient(log));

  let theme = $state('saudi');
  let mode = $state('light');
  let language = $state('en');
  let locked = $state(false);
  let version = $state(0);
  let comments = $state<Comment[]>([
    { id: 'c1', target: { kind: 'screen', screenId: 'bookings', x: 0.3, y: 0.2 }, body: 'Make the heading larger', createdAt: '2026-11-21T10:00:00Z', status: 'open' },
    { id: 'c2', target: { kind: 'screen', screenId: 'bookings', x: 0.7, y: 0.6 }, body: 'This table needs a status column', createdAt: '2026-11-21T10:01:00Z', status: 'resolved' },
    { id: 'c3', target: { kind: 'token', tokenPath: 'color.primary' }, body: 'Closer to our logo, please', createdAt: '2026-11-21T10:02:00Z', status: 'open' },
  ]);
  let fromFrames = $state(0);
  let lastMessage = $state('');

  app.settings = { language: 'en' } as any;
  $effect(() => {
    const d = document.documentElement;
    d.dataset.theme = theme; d.dataset.mode = mode; d.lang = language; d.dir = language === 'ar' ? 'rtl' : 'ltr';
    app.settings = { language } as any;
  });
  $effect(() => {
    const on = (e: MessageEvent) => { if (typeof e.data === 'string' && e.data.startsWith('mado-hostile')) { fromFrames++; lastMessage = e.data; } };
    window.addEventListener('message', on);
    return () => window.removeEventListener('message', on);
  });

  function oncomment(target: Extract<CommentTarget, { kind: 'token' | 'screen' }>) {
    log(`oncomment ${JSON.stringify(target)}`);
    comments = [...comments, { id: `c${comments.length + 1}`, target, body: 'A new comment (demo)', createdAt: new Date().toISOString(), status: 'open' }];
  }
</script>

<div class="demo">
  <div class="bar">
    <strong>Design step, dev demo</strong>
    <select bind:value={theme} aria-label="Theme"><option>saudi</option><option>contrast</option><option>tech</option></select>
    <select bind:value={mode} aria-label="Mode"><option>light</option><option>dark</option></select>
    <select bind:value={language} aria-label="Language"><option value="en">English</option><option value="ar">العربية</option></select>
    <label class="chk"><input type="checkbox" bind:checked={locked} /> locked</label>
    <button class="btn" onclick={() => version++}>version++</button>
    <span id="frame-messages" class="count" class:bad={fromFrames > 0}>messages from frames: {fromFrames} {lastMessage}</span>
  </div>

  {#key language}
    <div class="page">
      <DesignStep projectId="demo" {comments} {locked} {version} {oncomment} />
    </div>
    <div class="page">
      <h2>The hostile mock-up, raw, in the sealed frame</h2>
      <p class="muted">Not cleaned at all. It must show as plain words, the counter above must stay 0, and the network list must hold no request to 127.0.0.1:9.</p>
      <div style="max-width: 760px"><MockFrame html={HOSTILE} tokens={FALLBACK} name="Hostile" device="tablet" unchecked /></div>
    </div>
  {/key}
  <pre class="log" aria-label="Log">{lines.join('\n')}</pre>
</div>

<style>
  .demo { min-height: 100vh; background: var(--bg); color: var(--fg); font-family: var(--font); }
  .bar { position: sticky; top: 0; z-index: 10; display: flex; gap: 10px; align-items: center; flex-wrap: wrap; padding: 8px 16px; background: var(--panel); border-bottom: 1px solid var(--border); }
  .bar select { width: auto; padding: 3px 8px; }
  .chk { display: inline-flex; gap: 6px; align-items: center; margin: 0; }
  .count { margin-inline-start: auto; font-family: var(--mono); font-size: 12px; }
  .count.bad { color: var(--danger); font-weight: 700; }
  .page { padding: 20px 28px; max-width: 1240px; }
  .log { margin: 0 28px 28px; padding: 10px; background: var(--code-bg); border-radius: 8px; font-size: 12px; min-height: 40px; white-space: pre-wrap; }
</style>
