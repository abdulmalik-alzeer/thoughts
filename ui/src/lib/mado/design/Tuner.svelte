<script lang="ts">
  // The tuner: every setting of the picked style, changed live. It holds no state of its own
  // beyond what is being typed: each change is handed up (`onchange`) with a key that says which
  // control made it, so a drag is one step to undo.
  import { t, tn } from '../../i18n';
  import Icon from '../../Icon.svelte';
  import type { Comment, DesignTokens } from '../types';
  import { isHex } from './contrast';
  import { FONTS, fontStack } from './fonts';
  import { matchKey } from '../../keys/registry';
  import { COLOR_ROLES, DENSITIES, FONT_ROLES, HARMONIES, RADIUS, SHADOWS, SPACING, fixAll, fixed, harmony, lowRoles, reading, setToken, type ColorRole, type Harmony } from './tokens';

  let {
    tokens, comments = [], locked = false, canUndo = false, canRedo = false, canReset = false, status = 'saved', saveError = '',
    onchange, onundo, onredo, onreset, onretry, oncomment,
  }: {
    tokens: DesignTokens;
    comments?: Comment[];
    locked?: boolean;
    canUndo?: boolean;
    canRedo?: boolean;
    canReset?: boolean;
    status?: 'saved' | 'unsaved' | 'saving' | 'failed';
    saveError?: string;
    onchange: (next: DesignTokens, key: string | null) => void;
    onundo: () => void;
    onredo: () => void;
    onreset: () => void;
    onretry: () => void;
    oncomment?: (tokenPath: string) => void;
  } = $props();

  const low = $derived(lowRoles(tokens));
  const countFor = (path: string) => comments.filter((c) => c.target.kind === 'token' && c.target.tokenPath === path && c.status !== 'resolved').length;

  const set = (path: string, value: unknown) => onchange(setToken(tokens, path, value), path);

  // The colour code as it is being typed; taken once it is a whole `#rrggbb`.
  let typing = $state<Record<string, string>>({});
  function typed(role: ColorRole, raw: string) {
    typing[role] = raw;
    const v = raw.trim().startsWith('#') ? raw.trim() : '#' + raw.trim();
    if (isHex(v)) set(`color.${role}`, v.toLowerCase());
  }
  const doneTyping = (role: ColorRole) => { delete typing[role]; };

  function keys(e: KeyboardEvent) {
    const id = matchKey(e, 'mado-design');
    if (id !== 'mado-design.undo' && id !== 'mado-design.redo') return;
    // A text box keeps its own undo while it is being typed in.
    const el = e.target as HTMLElement;
    if (el instanceof HTMLInputElement && el.type === 'text') return;
    e.preventDefault();
    if (id === 'mado-design.redo') { if (canRedo) onredo(); } else if (canUndo) onundo();
  }

  const fontGroups = (role: string) => (['sans', 'serif', 'mono'] as const)
    .map((kind) => ({ kind, list: FONTS.filter((f) => f.kind === kind && (role !== 'mono' || f.kind === 'mono')) }))
    .filter((g) => g.list.length);
</script>

{#snippet commentBtn(path: string, name: string)}
  {#if oncomment}
    {@const n = countFor(path)}
    <button type="button" class="cbtn" class:has={n > 0} aria-label={t('madoDesign.tune.commentOn', { name })} title={n ? tn('madoDesign.tune.comments', n) : t('madoDesign.tune.commentOn', { name })}
      onclick={() => oncomment?.(path)}>
      <Icon name="chat" size={14} />{#if n}<span class="cn">{n}</span>{:else}<span class="ct">{t('madoDesign.tune.comment')}</span>{/if}
    </button>
  {/if}
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="tuner" onkeydown={keys}>
  <div class="bar" role="toolbar" aria-label={t('madoDesign.tab.tune')}>
    <button type="button" class="btn ghost" disabled={locked || !canUndo} onclick={onundo} title={t('madoDesign.tune.undo')}><Icon name="undo" size={15} /> {t('madoDesign.tune.undo')}</button>
    <button type="button" class="btn ghost" disabled={locked || !canRedo} onclick={onredo} title={t('madoDesign.tune.redo')}><Icon name="undo" size={15} class="mirror" /> {t('madoDesign.tune.redo')}</button>
    <button type="button" class="btn ghost" disabled={locked || !canReset} onclick={onreset}><Icon name="retry" size={15} /> {t('madoDesign.tune.reset')}</button>
    <span class="grow"></span>
    <span class="status" class:bad={status === 'failed'} role="status" aria-live="polite">
      {#if status === 'failed'}
        <Icon name="warning" size={14} /> {t('madoDesign.tune.saveFailed')}{saveError ? `: ${saveError}` : ''}
        <button type="button" class="more" onclick={onretry}>{t('madoDesign.retry')}</button>
      {:else if status === 'saving'}{t('madoDesign.tune.saving')}
      {:else if status === 'unsaved'}{t('madoDesign.tune.unsaved')}
      {:else}<Icon name="check" size={14} /> {t('madoDesign.tune.saved')}{/if}
    </span>
  </div>

  <fieldset class="group" disabled={locked}>
    <legend>{t('madoDesign.tune.colors')}</legend>
    {#if low.length}
      <button type="button" class="btn fixall" onclick={() => onchange(fixAll(tokens), null)}><Icon name="warning" size={14} /> {tn('madoDesign.tune.fixAll', low.length)}</button>
    {/if}
    <ul class="rows">
      {#each COLOR_ROLES as role (role)}
        {@const name = t(`madoDesign.color.${role}`)}
        {@const r = reading(tokens, role)}
        <li class="rowx">
          <input type="color" class="well" value={tokens.color[role]} aria-label={t('madoDesign.tune.pickColor', { name })}
            oninput={(e) => set(`color.${role}`, e.currentTarget.value.toLowerCase())} />
          <div class="what">
            <span class="lbl">{name}</span>
            {#if r}
              <span class="ratio" class:low={!r.ok} title={r.ok ? t(r.level === 'AAA' ? 'madoDesign.tune.veryReadable' : 'madoDesign.tune.readable', { n: r.ratio }) : t('madoDesign.tune.low', { n: r.ratio, t: r.target })}>
                {#if !r.ok}<Icon name="warning" size={12} />{/if}
                <span dir="ltr">{t('madoDesign.tune.ratio', { n: r.ratio })}</span>
                {#if r.ok}<span class="lvl">{r.level}</span>{/if}
                <span class="sr-only">{r.ok ? t('madoDesign.tune.readable', { n: r.ratio }) : t('madoDesign.tune.low', { n: r.ratio, t: r.target })}</span>
              </span>
              {#if !r.ok}<button type="button" class="fix" onclick={() => onchange(fixed(tokens, role), null)}>{t('madoDesign.tune.fix')}</button>{/if}
            {/if}
          </div>
          <input type="text" class="hex" dir="ltr" spellcheck="false" autocomplete="off" maxlength="7" aria-label={t('madoDesign.tune.hex', { name })}
            value={typing[role] ?? tokens.color[role]} aria-invalid={typing[role] !== undefined && !isHex(typing[role].startsWith('#') ? typing[role] : '#' + typing[role])}
            oninput={(e) => typed(role, e.currentTarget.value)} onblur={() => doneTyping(role)} />
          {@render commentBtn(`color.${role}`, name)}
        </li>
      {/each}
    </ul>
  </fieldset>

  <fieldset class="group" disabled={locked}>
    <legend>{t('madoDesign.tune.ideas')}</legend>
    <p class="hint muted">{t('madoDesign.tune.ideasHint')}</p>
    <div class="ideas">
      {#each HARMONIES as h (h)}
        {@const out = harmony(tokens, h as Harmony)}
        <button type="button" class="idea" onclick={() => onchange(out, null)}>
          <span class="chips" aria-hidden="true">
            {#each ['bg', 'surface', 'primary', 'accent'] as c (c)}<span style="background:{(out.color as any)[c]}"></span>{/each}
          </span>
          {t(`madoDesign.harmony.${h}`)}
        </button>
      {/each}
    </div>
  </fieldset>

  <fieldset class="group" disabled={locked}>
    <legend>{t('madoDesign.tune.fonts')}</legend>
    <ul class="rows">
      {#each FONT_ROLES as role (role)}
        {@const name = t(`madoDesign.font.${role}`)}
        <li class="rowx">
          <span class="aa" aria-hidden="true" style="font-family:{fontStack(tokens.font[role], role)}">Aa</span>
          <label class="what lbl" for="mdt-font-{role}">{name}</label>
          <select id="mdt-font-{role}" class="pick" value={tokens.font[role]} onchange={(e) => set(`font.${role}`, e.currentTarget.value)}>
            {#each fontGroups(role) as g (g.kind)}
              <optgroup label={t(`madoDesign.fontKind.${g.kind}`)}>
                {#each g.list as f (f.name)}<option value={f.name}>{t(`madoDesign.fontName.${f.name}`)}</option>{/each}
              </optgroup>
            {/each}
          </select>
          {@render commentBtn(`font.${role}`, name)}
        </li>
      {/each}
    </ul>
  </fieldset>

  <fieldset class="group" disabled={locked}>
    <legend>{t('madoDesign.tune.shape')}</legend>
    <ul class="rows">
      <li class="rowx">
        <label class="what lbl" for="mdt-radius">{t('madoDesign.radius')}</label>
        <input id="mdt-radius" type="range" min={RADIUS.min} max={RADIUS.max} step="1" value={tokens.radius} oninput={(e) => set('radius', +e.currentTarget.value)} />
        <output class="num" for="mdt-radius">{t('madoDesign.px', { n: tokens.radius })}</output>
        {@render commentBtn('radius', t('madoDesign.radius'))}
      </li>
      <li class="rowx">
        <label class="what lbl" for="mdt-spacing">{t('madoDesign.spacing')}</label>
        <input id="mdt-spacing" type="range" min={SPACING.min} max={SPACING.max} step="1" value={tokens.spacing} oninput={(e) => set('spacing', +e.currentTarget.value)} />
        <output class="num" for="mdt-spacing">{t('madoDesign.px', { n: tokens.spacing })}</output>
        {@render commentBtn('spacing', t('madoDesign.spacing'))}
      </li>
      <li class="rowx">
        <span class="what lbl" id="mdt-density">{t('madoDesign.density')}</span>
        <div class="seg" role="group" aria-labelledby="mdt-density">
          {#each DENSITIES as d (d)}
            <button type="button" class="segb" aria-pressed={tokens.density === d} onclick={() => set('density', d)}>{t(`madoDesign.density.${d}`)}</button>
          {/each}
        </div>
        {@render commentBtn('density', t('madoDesign.density'))}
      </li>
      <li class="rowx">
        <span class="what lbl" id="mdt-shadow">{t('madoDesign.shadow')}</span>
        <div class="seg" role="group" aria-labelledby="mdt-shadow">
          {#each SHADOWS as s (s)}
            <button type="button" class="segb" aria-pressed={tokens.shadow === s} onclick={() => set('shadow', s)}>{t(`madoDesign.shadow.${s}`)}</button>
          {/each}
        </div>
        {@render commentBtn('shadow', t('madoDesign.shadow'))}
      </li>
    </ul>
  </fieldset>
</div>

<style>
  .tuner { display: flex; flex-direction: column; gap: 14px; min-width: 0; }
  .bar { display: flex; align-items: center; gap: 4px; flex-wrap: wrap; position: sticky; top: 0; z-index: 2; background: var(--glass); -webkit-backdrop-filter: blur(18px); backdrop-filter: blur(18px); padding: 6px; border-radius: var(--radius-lg); border: 1px solid var(--hairline); }
  .grow { flex: 1; }
  .status { display: inline-flex; align-items: center; gap: 5px; font-size: var(--fs-sm); color: var(--muted); padding-inline: 8px; }
  .status.bad { color: var(--danger); }
  .group { border: var(--border-w) solid var(--border); border-radius: var(--radius-lg); background: var(--panel); margin: 0; padding: 6px 14px 12px; min-width: 0; }
  .group legend { font-size: var(--fs-sm); font-weight: 600; color: var(--fg); padding: 0 6px; }
  .group:disabled { opacity: .7; }
  .rows { list-style: none; margin: 0; padding: 0; }
  .rowx { display: flex; align-items: center; gap: 10px; padding: 7px 0; border-top: 1px solid var(--hairline); min-height: 44px; flex-wrap: wrap; }
  .rowx:first-child { border-top: 0; }
  .what { flex: 1 1 120px; min-width: 0; display: flex; flex-direction: column; gap: 1px; margin: 0; }
  .lbl { font-size: 13px; font-weight: 500; color: var(--fg); }
  label.lbl { margin: 0; }
  /* The picker shows the project's colour: the swatch is data, its frame is the theme's. */
  .well { flex: none; width: 34px; height: 34px; padding: 2px; border-radius: 50%; border: var(--border-w) solid var(--control-border); background: var(--panel); cursor: pointer; overflow: hidden; }
  .well::-webkit-color-swatch-wrapper { padding: 0; }
  .well::-webkit-color-swatch { border: 0; border-radius: 50%; }
  .well::-moz-color-swatch { border: 0; border-radius: 50%; }
  .well:disabled { cursor: default; }
  .hex { flex: none; width: 92px; font-family: var(--mono); font-size: var(--fs-sm); padding: 5px 8px; text-align: start; }
  .hex[aria-invalid='true'] { border-color: var(--danger); }
  .ratio { display: inline-flex; align-items: center; gap: 4px; font-size: var(--fs-xs); color: var(--muted); }
  .ratio.low { color: var(--danger); font-weight: 600; }
  .lvl { font-size: var(--fs-xxs); font-weight: 600; padding: 0 5px; border-radius: 99px; background: var(--fill); }
  .fix { font: inherit; font-size: var(--fs-xs); font-weight: 600; color: var(--on-tint); background: var(--tint); border: 0; border-radius: 99px; padding: 2px 9px; cursor: pointer; align-self: flex-start; margin-top: 2px; }
  .fix:hover { filter: brightness(.96); }
  .fixall { margin: 4px 0 6px; color: var(--danger); }
  .cbtn { flex: none; min-width: 84px; justify-content: center; font: inherit; font-size: var(--fs-xs); color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 5px 7px; cursor: pointer; display: inline-flex; align-items: center; gap: 4px; }
  .cbtn:hover { background: var(--fill); color: var(--fg); }
  .cbtn.has { background: var(--tint); color: var(--on-tint); }
  .cn { font-weight: 700; }
  .aa { flex: none; width: 34px; height: 34px; display: grid; place-items: center; border-radius: 8px; background: var(--fill); font-size: 16px; font-weight: 600; }
  .pick { flex: 0 1 190px; width: auto; }
  input[type='range'] { flex: 1 1 120px; padding: 0; accent-color: var(--accent); background: none; border: 0; }
  .num { flex: none; min-width: 52px; text-align: end; font-size: var(--fs-sm); color: var(--muted); font-variant-numeric: tabular-nums; }
  .hint { margin: 0 0 8px; font-size: var(--fs-sm); }
  .ideas { display: flex; flex-wrap: wrap; gap: 8px; }
  .idea { font: inherit; font-size: var(--fs-sm); font-weight: 500; color: var(--fg); background: var(--fill); border: var(--border-w) solid transparent; border-radius: 99px; padding: 5px 12px 5px 6px; cursor: pointer; display: inline-flex; align-items: center; gap: 8px; }
  :global([dir='rtl']) .idea { padding: 5px 6px 5px 12px; }
  .idea:hover { background: var(--fill-2); }
  .idea:disabled { cursor: default; }
  .chips { display: inline-flex; }
  .chips span { width: 16px; height: 16px; border-radius: 50%; border: 1px solid var(--control-border); margin-inline-end: -5px; }
  .chips span:last-child { margin-inline-end: 0; }
  .tuner :global(.mirror) { transform: scaleX(-1); }
</style>
