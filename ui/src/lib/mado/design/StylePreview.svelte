<script lang="ts">
  // A small sample of an interface drawn with one set of design settings: heading, text, buttons,
  // a card, an input, a table. The settings arrive as CSS variables on this element only
  // (`--mado-*`), so every colour here is the project's, none is the app's theme.
  // It is a picture: nothing in it can be focused or clicked.
  import { t } from '../../i18n';
  import { inlineVars } from './tokens';

  let { tokens, size = 'card' }: { tokens: unknown; size?: 'card' | 'full' } = $props();
</script>

<div class="mp" class:full={size === 'full'} style={inlineVars(tokens)} aria-hidden="true" inert>
  <div class="mp-top">
    <div>
      <div class="mp-eyebrow">{t('madoDesign.sample.eyebrow')}</div>
      <div class="mp-h">{t('madoDesign.sample.heading')}</div>
    </div>
    <span class="mp-chip accent">{t('madoDesign.sample.up')}</span>
  </div>
  <p class="mp-p">{t('madoDesign.sample.text')}</p>
  <div class="mp-row">
    <span class="mp-btn primary">{t('madoDesign.sample.primary')}</span>
    <span class="mp-btn">{t('madoDesign.sample.secondary')}</span>
    <span class="mp-link">{t('madoDesign.sample.link')}</span>
  </div>
  <div class="mp-card">
    <div class="mp-cardhead">
      <span class="mp-muted">{t('madoDesign.sample.card')}</span>
      <span class="mp-stat">{t('madoDesign.sample.stat')}</span>
    </div>
    <div class="mp-input">{t('madoDesign.sample.input')}</div>
    <div class="mp-table">
      <div class="mp-tr head"><span>{t('madoDesign.sample.colOrder')}</span><span>{t('madoDesign.sample.colStatus')}</span></div>
      <div class="mp-tr"><span class="mp-mono">#1042</span><span class="mp-dot success">{t('madoDesign.sample.paid')}</span></div>
      <div class="mp-tr"><span class="mp-mono">#1043</span><span class="mp-dot warning">{t('madoDesign.sample.waiting')}</span></div>
      {#if size === 'full'}
        <div class="mp-tr"><span class="mp-mono">#1044</span><span class="mp-dot danger">{t('madoDesign.sample.failed')}</span></div>
      {/if}
    </div>
  </div>
</div>

<style>
  /* Everything is sized from the design's own spacing, density and radius. */
  .mp {
    --u: calc(var(--mado-spacing) * var(--mado-density));
    container-type: inline-size;
    background: var(--mado-color-bg); color: var(--mado-color-text); font-family: var(--mado-font-body);
    font-size: 12px; line-height: 1.45; padding: calc(var(--u) * 1.6); display: flex; flex-direction: column; gap: var(--u);
    text-align: start; overflow: hidden; min-height: 100%;
  }
  .mp.full { font-size: 14.5px; padding: calc(var(--u) * 3); gap: calc(var(--u) * 1.6); }
  .mp-top { display: flex; justify-content: space-between; align-items: flex-start; gap: var(--u); }
  .mp-eyebrow { font-size: .78em; color: var(--mado-color-muted); letter-spacing: .04em; text-transform: uppercase; }
  .mp-h { font-family: var(--mado-font-heading); font-size: 1.55em; font-weight: 650; line-height: 1.15; letter-spacing: -.01em; }
  .mp-p { margin: 0; color: var(--mado-color-muted); }
  .mp-row { display: flex; align-items: center; gap: var(--u); flex-wrap: wrap; }
  .mp-btn {
    display: inline-flex; align-items: center; padding: calc(var(--u) * .7) calc(var(--u) * 1.5); border-radius: var(--mado-radius);
    border: 1px solid var(--mado-color-border); background: var(--mado-color-surface); font-weight: 600; white-space: nowrap;
  }
  .mp-btn.primary { background: var(--mado-color-primary); color: var(--mado-color-on-primary); border-color: var(--mado-color-primary); box-shadow: var(--mado-shadow); }
  .mp-link { color: var(--mado-color-accent); font-weight: 600; text-decoration: underline; text-underline-offset: 2px; }
  .mp-chip { font-size: .8em; font-weight: 600; padding: 2px calc(var(--u) * .9); border-radius: 99px; white-space: nowrap; border: 1px solid currentColor; }
  .mp-chip.accent { color: var(--mado-color-accent); }
  .mp-card {
    background: var(--mado-color-surface); border: 1px solid var(--mado-color-border); border-radius: calc(var(--mado-radius) * 1.25);
    box-shadow: var(--mado-shadow); padding: calc(var(--u) * 1.4); display: flex; flex-direction: column; gap: var(--u);
  }
  .mp-cardhead { display: flex; justify-content: space-between; align-items: baseline; gap: var(--u); }
  .mp-muted { color: var(--mado-color-muted); }
  .mp-stat { font-family: var(--mado-font-heading); font-weight: 650; font-size: 1.15em; }
  .mp-input { border: 1px solid var(--mado-color-border); border-radius: var(--mado-radius); padding: calc(var(--u) * .7) var(--u); color: var(--mado-color-muted); background: var(--mado-color-bg); }
  .mp-table { display: flex; flex-direction: column; }
  .mp-tr { display: flex; justify-content: space-between; gap: var(--u); padding: calc(var(--u) * .55) 0; border-top: 1px solid var(--mado-color-border); }
  .mp-tr.head { border-top: 0; color: var(--mado-color-muted); font-size: .85em; font-weight: 600; }
  .mp-mono { font-family: var(--mado-font-mono); font-size: .95em; }
  .mp-dot { display: inline-flex; align-items: center; gap: .45em; font-weight: 600; }
  .mp-dot::before { content: ''; width: .6em; height: .6em; border-radius: 50%; background: currentColor; }
  .mp-dot.success { color: var(--mado-color-success); }
  .mp-dot.warning { color: var(--mado-color-warning); }
  .mp-dot.danger { color: var(--mado-color-danger); }
</style>
