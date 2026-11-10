<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  let { close }: { close: () => void } = $props();
  const cards = [
    ['home', 'Your home', 'Home shows what matters today: open todos, ideas in progress, recent chats. Ask anything from the box at the top.'],
    ['chats', 'Just talk', 'Every chat runs your AI inside your library, so it knows your notes and history. Say “new idea: …”, “continue …”, or anything at all.'],
    ['wrap', 'Wrap to save', 'When you finish, press Wrap. You see how your chats fold into your ideas, notes and todos before anything is written.'],
    ['docs', 'Plain files you own', 'Ideas, Docs and Todos are views over ordinary markdown files. Open them here, in any editor, or let the AI edit them.'],
    ['projects', 'Work in any folder', 'Link a folder — client files, a report, a code repo — and chat with your AI inside it.'],
    ['palette', 'Make it yours', 'Settings → App layer: describe any change to the app itself. If something breaks, Rollback restores any earlier version.'],
  ];
  let i = $state(0);
</script>

<div class="modal-bg">
  <div class="modal tour">
    <div class="ic"><Icon name={cards[i][0]} size={26} /></div>
    <h1>{cards[i][1]}</h1>
    <p>{cards[i][2]}</p>
    <div class="dots">{#each cards as _, j}<i class:on={j === i}></i>{/each}</div>
    <div class="row"><button class="btn ghost" onclick={close}>Skip</button><span class="grow"></span>
      {#if i > 0}<button class="btn" onclick={() => i--}>Back</button>{/if}
      {#if i < cards.length - 1}<button class="btn primary" onclick={() => i++}>Next</button>{:else}<button class="btn primary" onclick={close}>Get started</button>{/if}
    </div>
  </div>
</div>

<style>
  .tour { max-width: 460px; text-align: center; padding: 28px; }
  .ic { width: 56px; height: 56px; border-radius: 16px; margin: 0 auto 14px; display: grid; place-items: center; background: var(--tint); color: var(--accent); }
  p { color: var(--muted); margin: 0 0 18px; }
  .dots { display: flex; justify-content: center; gap: 6px; margin-bottom: 18px; }
  .dots i { width: 6px; height: 6px; border-radius: 50%; background: var(--fill-2); }
  .dots i.on { background: var(--accent); }
</style>
