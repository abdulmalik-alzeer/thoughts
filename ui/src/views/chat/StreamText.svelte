<script lang="ts">
  // The answer while it is being written. Pieces arrive many times a second; rendering the whole
  // answer for each one (markdown, sanitising, replacing its DOM) costs more the longer it gets and
  // throws away a selection the reader is making. So the screen catches up at most once per frame,
  // and only the block still being written is rendered again: finished paragraphs, lists and code
  // blocks keep their DOM, and a selection in them survives.
  import { untrack } from 'svelte';
  import { marked } from 'marked';
  import { md, mdClick } from '../../lib/md';

  let { text, onrender }: { text: string; onrender?: () => void } = $props();

  let shown = $state('');
  let frame = 0;
  $effect(() => {
    const now = text;
    // A new answer (or none): at once, never a frame of the old one.
    if (!now || !now.startsWith(shown)) { cancelAnimationFrame(frame); frame = 0; shown = now; return; }
    if (frame || now === shown) return;
    frame = requestAnimationFrame(() => { frame = 0; shown = text; });
  });
  $effect(() => () => cancelAnimationFrame(frame));

  // Rendered blocks by their source, kept only while they are still part of the answer.
  let done = new Map<string, string>();
  const blocks = $derived.by(() => {
    let tokens: { raw: string }[];
    try { tokens = marked.lexer(shown) as unknown as { raw: string }[]; } catch { return [md(shown)]; }
    const next = new Map<string, string>();
    const out: string[] = [];
    for (const tok of tokens) {
      const raw = tok.raw ?? '';
      if (!raw.trim()) continue;
      const html = next.get(raw) ?? done.get(raw) ?? md(raw);
      next.set(raw, html);
      out.push(html);
    }
    done = next;
    return out;
  });

  // After the screen has caught up: the chat may follow the new text down.
  $effect(() => { blocks; untrack(() => onrender?.()); });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="md" dir="auto" onclick={mdClick}>{#each blocks as html, i (i)}{@html html}{/each}</div>
