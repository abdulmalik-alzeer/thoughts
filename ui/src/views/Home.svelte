<script lang="ts">
  import { onMount } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { app, go, refreshChats } from '../lib/store.svelte';
  import { t, tn, ago, fmtDate, fmtNum } from '../lib/i18n';
  import { dueLabel, dueState, fail, normTodo } from '../lib/content/util';
  import Icon from '../lib/Icon.svelte';
  import Emblem from '../lib/Emblem.svelte';

  let { projects, newChat, startWrap }: { projects: any[]; newChat: (pid?: string | null) => void; startWrap: () => void } = $props();

  let ideas = $state<any[]>([]);
  let lists = $state<any[]>([]);
  let files = $state(0);
  let ask = $state('');
  let askEl: HTMLTextAreaElement;

  async function load() {
    const [i, td, st] = await Promise.allSettled([api('/library/ideas'), api('/todos'), api('/library/stats')]);
    if (i.status === 'fulfilled') ideas = i.value;
    if (td.status === 'fulfilled') lists = td.value;
    if (st.status === 'fulfilled') files = st.value.markdownFiles;
  }
  onMount(() => { load(); return on((e) => { if (['todos.changed', 'library.changed', 'chat.done'].includes(e.type)) load(); }); });

  const hour = new Date().getHours();
  const greeting = $derived(t(hour < 12 ? 'goodMorning' : hour < 18 ? 'goodAfternoon' : 'goodEvening'));
  const today = $derived(fmtDate(new Date(), { weekday: 'long', day: 'numeric', month: 'long' }));

  const active = $derived(ideas.filter((i) => i.status === 'active' || i.status === 'seed'));
  const recentIdeas = $derived([...ideas].sort((a, b) => String(b.updated).localeCompare(String(a.updated))).slice(0, 4));
  const openTodos = $derived(lists.flatMap((l) => l.items.filter((i: any) => !i.done).map((i: any) => ({ ...normTodo(i), file: l.file, idea: l.idea }))));
  // Next few dated todos, earliest first — so anything overdue leads the list.
  const upcoming = $derived(openTodos.filter((i) => i.due).sort((a, b) => a.due!.localeCompare(b.due!)).slice(0, 5));
  const statusLabel = (s: string) => { const k = `idea.status.${s}`; const x = t(k); return x === k ? s : x; };
  const recentChats = $derived(app.chats.filter((c) => c.kind === 'chat').slice(0, 5));

  async function submit(prefix = '') {
    const text = (prefix + ask).trim();
    if (!text) return askEl?.focus();
    try {
      const c = await post('/chats', {});
      await post(`/chats/${c.id}/send`, { text });
      ask = '';
      await refreshChats();
      go({ kind: 'chat', id: c.id });
    } catch (e) { fail(e); }
  }

  async function toggle(it: any) {
    try { await post('/todos/toggle', { file: it.file, line: it.line, text: it.text }); } catch (e) { fail(e); }
    load();
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); submit(); }
  }
</script>

<div class="home">
  <section class="hero">
    <div class="lead">
      <div class="eyebrow">{today}</div>
      <h1 class="display">{greeting}{app.settings?.userName ? `, ${app.settings.userName}` : ''}.</h1>
      <p class="sub">{t('heroSub')}</p>
      <div class="ask surface">
        <textarea bind:this={askEl} bind:value={ask} onkeydown={keydown} rows="2" placeholder={t('askPlaceholder')} aria-label={t('askPlaceholder')} dir="auto"></textarea>
        <div class="row">
          <button class="chip" onclick={() => submit('new idea: ')}><Icon name="ideas" size={14} />{t('home.captureIdea')}</button>
          <button class="chip" onclick={() => submit('todo: ')}><Icon name="todos" size={14} />{t('addTodoChip')}</button>
          {#if app.settings?.modules?.checkin}<button class="chip" onclick={() => submit('check-in')}><Icon name="calendar" size={14} />{t('checkIn')}</button>{/if}
          <span class="grow"></span>
          <button class="btn primary round" onclick={() => submit()} disabled={!ask.trim()} aria-label={t('send')}><Icon name="send" size={16} stroke={2.25} /></button>
        </div>
      </div>
    </div>
    <div class="art"><Emblem size={250} /></div>
  </section>

  <section class="stats">
    <button class="stat" onclick={() => go({ kind: 'ideas' })}><b>{fmtNum(active.length)}</b><span>{t('activeIdeas')}</span></button>
    <button class="stat" onclick={() => go({ kind: 'todos' })}><b>{fmtNum(openTodos.length)}</b><span>{t('openTodos')}</span></button>
    <button class="stat" class:attn={app.wrap.count > 0} onclick={() => (app.wrap.count ? startWrap() : go({ kind: 'chats' }))}>
      <b>{fmtNum(app.wrap.count)}</b><span>{app.wrap.count ? t('toWrapAction') : t('allSaved')}</span></button>
    <button class="stat" onclick={() => go({ kind: 'docs' })}><b>{fmtNum(files)}</b><span>{t('notesInLibrary')}</span></button>
  </section>

  <section class="block">
    <div class="bh">
      <div><h2>{t('continueWhere')}</h2><p class="muted">{t('continueSub')}</p></div>
      <button class="more" onclick={() => go({ kind: 'ideas' })}>{t('seeAll')} <Icon name="right" size={14} class="flip-rtl" /></button>
    </div>
    <div class="ideas">
      {#each recentIdeas as i (i.path)}
        <button class="idea surface lit" onclick={() => go({ kind: 'docs', path: i.path })}>
          <span class="badge"><Icon name="ideas" size={12} />{statusLabel(i.status)}</span>
          <span class="glyph" aria-hidden="true">{(i.title || '?').trim()[0]}</span>
          <b class="ell" dir="auto">{i.title}</b>
          <span class="muted clamp" dir="auto">{i.summary}</span>
        </button>
      {:else}
        <div class="empty surface"><Icon name="ideas" size={22} /><span>{t('noIdeasYet')}</span></div>
      {/each}
    </div>
  </section>

  {#if upcoming.length}
    <section class="block up">
      <div class="surface panel">
        <div class="ph"><div class="phl"><div class="eyebrow">{t('home.upcomingEyebrow')}</div><h2>{t('home.upcoming')}</h2></div>
          <button class="more" onclick={() => go({ kind: 'todos' })}>{t('seeAll')} <Icon name="right" size={14} class="flip-rtl" /></button></div>
        {#each upcoming as it (it.file + it.line)}
          {@const s = dueState(it.due!)}
          <label class="todo">
            <input type="checkbox" onchange={() => toggle(it)} />
            <span class="grow" dir="auto">{it.title}</span>
            {#if it.idea}<span class="muted when ell"><bdi>{it.idea.split('/').pop()}</bdi></span>{/if}
            <span class="due {s}"><Icon name={s === 'overdue' ? 'warning' : 'calendar'} size={12} />{#if s === 'overdue'}<b>{t('todo.overdue')}</b> ·{/if} {dueLabel(it.due!)}</span>
          </label>
        {/each}
      </div>
    </section>
  {/if}

  <section class="cols">
    <div class="surface panel">
      <div class="ph"><h2>{t('todos')}</h2><button class="more" onclick={() => go({ kind: 'todos' })}>{t('seeAll')} <Icon name="right" size={14} class="flip-rtl" /></button></div>
      {#each openTodos.slice(0, 7) as it (it.file + it.line)}
        <label class="todo">
          <input type="checkbox" onchange={() => toggle(it)} />
          <span class="grow" dir="auto">{it.title}</span>
          {#if it.idea}<span class="muted when ell"><bdi>{it.idea.split('/').pop()}</bdi></span>{/if}
        </label>
      {:else}
        <div class="empty"><Icon name="check" size={20} /><span>{t('nothingOpen')}</span></div>
      {/each}
    </div>
    <div class="surface panel">
      <div class="ph"><h2>{t('recentChats')}</h2><button class="more" onclick={() => newChat()}><Icon name="plus" size={14} /> {t('newChat')}</button></div>
      {#each recentChats as c (c.id)}
        <button class="rowbtn" onclick={() => go({ kind: 'chat', id: c.id })}>
          <Icon name="chat" size={15} />
          <span class="grow ell" dir="auto">{c.title}</span>
          {#if c.unwrapped}<span class="dot" role="img" aria-label={t('home.notSaved')} title={t('home.notSaved')}></span>{/if}
          <span class="muted when">{ago(c.updated)}</span>
        </button>
      {:else}
        <div class="empty"><Icon name="chats" size={20} /><span>{t('noChats')}</span></div>
      {/each}
    </div>
  </section>

  {#if app.settings?.modules?.builds !== false}
    <section class="block">
      <div class="bh">
        <div><h2>{t('projects')}</h2><p class="muted">{t('noProjects')}</p></div>
        <button class="more" onclick={() => go({ kind: 'projects' })}>{t('manage')} <Icon name="right" size={14} class="flip-rtl" /></button>
      </div>
      <div class="projects">
        {#each projects.slice(0, 4) as p (p.id)}
          <button class="proj surface lit" onclick={() => newChat(p.id)}>
            <Icon name="folder" size={18} />
            <b class="ell" dir="auto">{p.name}</b>
            <span class="muted when">{tn('home.chatCount', app.chats.filter((c) => c.projectId === p.id).length)}</span>
          </button>
        {/each}
        <button class="proj add" onclick={() => go({ kind: 'projects' })}><Icon name="plus" size={18} /><span>{t('linkFolder')}</span></button>
      </div>
    </section>
  {/if}
</div>

<style>
  .home { max-width: 1080px; margin: 0 auto; padding: 8px 36px 48px; }
  .hero { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 32px; min-height: 330px; padding-top: 18px; }
  .lead { min-width: 0; }
  .eyebrow { color: var(--accent-2); margin-bottom: 10px; }
  .sub { color: var(--muted); font-size: 15px; max-width: 520px; margin: 12px 0 22px; }
  .art { padding: 10px; }

  .ask { border-radius: 18px; padding: 12px 12px 10px 16px; transition: box-shadow .2s; }
  .ask:focus-within { box-shadow: var(--shadow-lg), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent); }
  .ask textarea { position: relative; background: transparent; border: 0; box-shadow: none; padding: 4px 0; resize: none; font-size: 15px; }
  .ask textarea:focus { background: transparent; box-shadow: none; }
  .ask .row { position: relative; }
  .chip { font: inherit; background: none; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; font-size: 12.5px; font-weight: 500; color: var(--muted); padding: 5px 11px; border-radius: 99px; border: 1px solid var(--hairline); }
  .chip:hover { color: var(--fg); border-color: color-mix(in srgb, var(--fg) 25%, transparent); }
  .round { width: 32px; height: 32px; padding: 0; border-radius: 50%; }

  .stats { display: flex; gap: 56px; padding: 26px 2px 8px; flex-wrap: wrap; }
  .stat { font: inherit; color: inherit; background: none; border: 0; padding: 0; text-align: start; cursor: pointer; display: flex; flex-direction: column; gap: 2px; }
  .stat b { font-size: 30px; font-weight: 500; letter-spacing: -.04em; line-height: 1.1; }
  .stat span { font-size: 12.5px; color: var(--muted); }
  .stat:hover b { color: var(--accent); }
  .stat.attn b, .stat.attn span { color: var(--accent-2); }

  .block { margin-top: 44px; }
  .bh { display: flex; align-items: flex-end; gap: 16px; margin-bottom: 16px; }
  .bh > div { flex: 1; }
  .bh h2 { font-size: 24px; margin: 0 0 4px; letter-spacing: -.03em; }
  .bh p { margin: 0; font-size: 13.5px; }

  .ideas { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; }
  .idea { font: inherit; color: inherit; text-align: start; box-sizing: border-box; cursor: pointer; position: relative; height: 196px; padding: 14px; display: flex; flex-direction: column; gap: 3px; transition: transform .2s; }
  .idea:hover { transform: translateY(-2px); }
  .badge { position: relative; align-self: flex-start; display: inline-flex; align-items: center; gap: 5px; font-size: 12px; font-weight: 500; color: var(--muted); }
  .glyph { position: relative; flex: 1; display: grid; place-items: center; font-size: 46px; font-weight: 300; letter-spacing: -.05em; color: color-mix(in srgb, var(--fg) 85%, transparent); }
  .idea b { position: relative; font-size: 13.5px; font-weight: 600; }
  .clamp { position: relative; font-size: 12px; line-height: 1.4; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }

  .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-top: 44px; }
  .panel { padding: 6px; min-width: 0; overflow: hidden; }
  .ph { position: relative; display: flex; align-items: center; padding: 10px 10px 8px; }
  .ph h2 { margin: 0; font-size: 15px; flex: 1; }
  .rowbtn { font: inherit; background: none; border: 0; text-align: start; position: relative; box-sizing: border-box; width: 100%; cursor: pointer; display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: 9px; min-width: 0; color: var(--fg); }
  .rowbtn :global(.icon) { color: var(--muted); flex: none; }
  .rowbtn:hover { background: var(--fill); }
  .todo { position: relative; display: flex; align-items: center; gap: 10px; padding: 8px 10px; margin: 0; border-radius: 9px; font-size: 13.5px; color: var(--fg); cursor: pointer; }
  .todo:hover { background: var(--fill); }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .when { font-size: 12px; white-space: nowrap; max-width: 140px; }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--accent-2); }
  .up { margin-top: 28px; }
  .phl { flex: 1; }
  .phl .eyebrow { color: var(--accent-2); margin-bottom: 2px; }
  .due { font-size: 12px; font-weight: 500; color: var(--muted); background: var(--fill); border: 1px solid transparent; border-radius: 99px; padding: 2px 9px; display: inline-flex; align-items: center; gap: 5px; white-space: nowrap; }
  .due.today, .due.soon { color: var(--accent); background: var(--tint); }
  .due.overdue { color: var(--danger); border-color: currentColor; background: color-mix(in srgb, var(--danger) 10%, transparent); }
  .empty { position: relative; display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 24px 10px; color: var(--muted); font-size: 13px; }
  .ideas .empty { grid-column: 1 / -1; }

  .projects { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; }
  .proj { font: inherit; color: inherit; text-align: start; box-sizing: border-box; cursor: pointer; padding: 14px; display: flex; flex-direction: column; gap: 6px; min-height: 104px; justify-content: flex-end; }
  .proj :global(.icon) { position: relative; color: var(--accent-2); margin-bottom: auto; }
  .proj b, .proj span { position: relative; }
  .proj.add { background: transparent; box-shadow: none; border: 1px dashed var(--hairline); border-radius: var(--radius-lg); color: var(--muted); align-items: center; justify-content: center; font-size: 13px; }
  .proj.add:hover { color: var(--fg); border-color: color-mix(in srgb, var(--fg) 30%, transparent); }
  .proj.add :global(.icon) { margin: 0; color: inherit; }

  @media (max-width: 1150px) { .art { display: none; } .hero { grid-template-columns: 1fr; min-height: 0; padding-top: 30px; } .ideas, .projects { grid-template-columns: repeat(2, 1fr); } }
</style>
