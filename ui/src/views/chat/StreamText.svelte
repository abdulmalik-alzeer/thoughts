<script lang="ts">
  // The answer while it is being written. Pieces arrive many times a second; rendering the whole
  // answer for each one (markdown, sanitising, replacing its DOM) costs more the longer it gets and
  // throws away a selection the reader is making. So the screen catches up at most once per frame,
  // and only the block still being written is rendered again: finished paragraphs, lists and code
  // blocks keep their DOM, and a selection in them survives. Only the end of the answer is read
  // again (streamBlocks.ts), and the block still being written is shown without the marks of what
  // is still open — "**bold", a table's header alone, half a link (streamMend.ts).
  import { untrack } from 'svelte';
  import { md, mdClick } from '../../lib/md';
  import { StreamLexer } from './streamBlocks';
  import { mendTail } from './streamMend';
  import { zoomImages } from '../../lib/zoomImages';
  import { linkDefs, type LinkMap } from './linkDefs';

  let { text, onrender, onclick }: { text: string; onrender?: () => void; onclick?: (e: MouseEvent) => void } = $props();
  const lexer = new StreamLexer();

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
  /** Blocks that can hold a link. */
  const LINKY = new Set(['paragraph', 'heading', 'list', 'blockquote', 'table']);
  const blocks = $derived.by(() => {
    let tokens: { raw: string; type: string }[], links: LinkMap;
    try { ({ tokens, links } = lexer.read(shown)); } catch { lexer.reset(); return [md(shown)]; }
    // A reference link ("[text][1]") finds its address in a definition ("[1]: https://…") usually
    // written further down: each block is rendered with the definitions seen so far, so the link
    // works as soon as its definition has arrived (definitions themselves show nothing).
    const defs = linkDefs(links);
    const next = new Map<string, string>();
    const out: string[] = [];
    let last = tokens.length - 1;
    while (last >= 0 && tokens[last].type === 'space') last--;
    for (let i = 0; i <= last; i++) {
      const tok = tokens[i];
      const raw = i === last ? mendTail(tok.raw ?? '', tok.type) : tok.raw ?? '';
      if (!raw.trim()) continue;
      // (Not after code or raw HTML: a block still open there would show the lines as its own.)
      const src = defs && LINKY.has(tok.type) ? raw + '\n\n' + defs : raw;
      const html = next.get(src) ?? done.get(src) ?? md(src);
      next.set(src, html);
      out.push(html);
    }
    done = next;
    return out;
  });

  // After the screen has caught up: the chat may follow the new text down.
  $effect(() => { blocks; untrack(() => onrender?.()); });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="md" dir="auto" onclick={(e) => { if (onclick) onclick(e); else mdClick(e); }} onauxclick={(e) => onclick?.(e)} use:zoomImages>{#each blocks as html, i (i)}{@html html}{/each}</div>
