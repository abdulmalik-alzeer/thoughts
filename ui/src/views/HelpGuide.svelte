<script lang="ts">
  // A guide (or the release notes) read inside the app: rendered markdown, links between guides work.
  import Icon from '../lib/Icon.svelte';
  import { md, mdClick } from '../lib/md';
  import { t } from '../lib/i18n';
  import { guideLink, slug } from '../lib/guides';

  let { src, anchor = '', back, open }: { src: string; anchor?: string; back: () => void; open: (name: string, anchor: string) => void } = $props();
  let box: HTMLElement | undefined = $state();
  const html = $derived(md(src));

  function scrollTo(a: string) {
    if (!box || !a) return;
    const want = slug(a);
    const h = Array.from(box.querySelectorAll('h1,h2,h3,h4,h5,h6')).find((x) => slug(x.textContent ?? '') === want);
    h?.scrollIntoView({ block: 'start' });
  }
  $effect(() => { void html; if (box) queueMicrotask(() => (anchor ? scrollTo(anchor) : undefined)); });

  function click(e: MouseEvent) {
    if (mdClick(e)) return;
    const a = (e.target as HTMLElement).closest('a');
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (href.startsWith('#')) { e.preventDefault(); let h = href.slice(1); try { h = decodeURIComponent(h); } catch { /* as written */ } scrollTo(h); return; }
    const g = guideLink(href);
    if (g) { e.preventDefault(); open(g.name, g.anchor); return; }
    // A link to a file that is not in the app does nothing; web links are left to the window's own handling.
    if (!/^(https?|mailto):/i.test(href)) e.preventDefault();
  }
</script>

<p><button class="btn" onclick={back}><Icon name="left" size={15} /> {t('help.back')}</button></p>
<article class="md guide" dir="auto" bind:this={box} onclick={click}>{@html html}</article>
