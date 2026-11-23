<script lang="ts">
  // A small "?" button that shows explanatory text in a popover. Click again, Escape, or a click
  // outside closes it. Use for "how it works" prose; keep warnings, errors and status visible.
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import { t } from './i18n';

  let { text = '', label = '', children, size = 14 }: {
    text?: string; label?: string; children?: Snippet; size?: number;
  } = $props();

  let open = $state(false);
  let root: HTMLSpanElement | undefined = $state();
  let btn: HTMLButtonElement | undefined = $state();
  const id = `helptip-${Math.random().toString(36).slice(2, 9)}`;
  // Flip the popover to the other side when it would run off the window edge.
  let alignEnd = $state(false);

  function toggle() {
    open = !open;
    if (open && btn) {
      const r = btn.getBoundingClientRect();
      const rtl = getComputedStyle(btn).direction === 'rtl';
      const room = rtl ? r.right : window.innerWidth - r.left;
      alignEnd = room < Math.min(300, window.innerWidth - 32);
    }
  }
  function onDoc(e: MouseEvent) { if (open && root && !root.contains(e.target as Node)) open = false; }
  function onKey(e: KeyboardEvent) { if (open && e.key === 'Escape') { e.stopPropagation(); open = false; btn?.focus(); } }
</script>

<svelte:document onmousedown={onDoc} onkeydown={onKey} />

<span class="helptip" bind:this={root}>
  <button
    bind:this={btn}
    type="button"
    class="helptip-btn"
    aria-label={label || t('helpTip.label')}
    aria-expanded={open}
    aria-controls={id}
    onclick={(e) => { e.stopPropagation(); e.preventDefault(); toggle(); }}
  ><Icon name="help" {size} /></button>
  {#if open}
    <span class="helptip-pop" class:end={alignEnd} {id} role="note">
      {#if text}<span class="helptip-text">{text}</span>{/if}
      {#if children}{@render children()}{/if}
    </span>
  {/if}
</span>

<style>
  .helptip { position: relative; display: inline-flex; vertical-align: middle; margin-inline-start: 4px; }
  .helptip-btn {
    display: inline-flex; align-items: center; justify-content: center; padding: 2px; border: 0;
    background: transparent; color: var(--muted); border-radius: 999px; cursor: pointer; line-height: 0;
  }
  .helptip-btn:hover, .helptip-btn[aria-expanded="true"] { color: var(--fg); background: var(--fill); }
  .helptip-btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .helptip-pop {
    position: absolute; top: calc(100% + 6px); inset-inline-start: -6px; z-index: 50;
    width: max-content; max-width: min(320px, calc(100vw - 32px));
    padding: 10px 12px; background: var(--panel); color: var(--fg);
    border: var(--border-w) solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg);
    font-size: var(--fs-sm); font-weight: 400; line-height: 1.5; text-align: start; white-space: normal;
    text-transform: none; letter-spacing: normal; display: flex; flex-direction: column; gap: 6px;
  }
  .helptip-pop.end { inset-inline-start: auto; inset-inline-end: -6px; }
</style>
