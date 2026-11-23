<script lang="ts">
  // Ready-made services for Settings › Assistant: pick one and its address is filled in. The list
  // comes from the background service, with how each one takes its key.
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { lang, t } from '../lib/i18n';
  import Icon from '../lib/Icon.svelte';

  // `program`: Claude Code's services (Anthropic-compatible) or Codex's (OpenAI-compatible). `noKey`:
  // the service runs on this computer and takes no key.
  let { url = '', disabled = false, program = 'claude', onpick }: { url?: string; disabled?: boolean; program?: 'claude' | 'codex'; onpick: (baseUrl: string, noKey: boolean) => void } = $props();
  type Preset = { id: string; name: string; nameAr: string; baseUrl: string; hint?: string; hintAr?: string; keysUrl: string; noKey?: boolean };
  let presets = $state<Preset[]>([]);
  onMount(() => { api<Preset[]>(`/ai/${program}/presets`).then((p) => (presets = p)).catch(() => {}); });

  const same = (a: string, b: string) => a.trim().replace(/\/+$/, '').toLowerCase() === b.trim().replace(/\/+$/, '').toLowerCase();
  const anthropic = (u: string) => !u.trim() || /^https:\/\/api\.anthropic\.com(?:[:/?#]|$)/i.test(u.trim());
  const chosen = $derived(presets.find((p) => (p.baseUrl ? same(url, p.baseUrl) : anthropic(url))));
  const name = (p: Preset) => (lang() === 'ar' ? p.nameAr : p.name);
  const hint = (p: Preset) => (program === 'codex' ? t('prov5.codex.preset.' + p.id) : (lang() === 'ar' ? p.hintAr : p.hint) ?? '');
</script>

{#if presets.length}
  <div class="presets" role="group" aria-label={t('runc.preset.title')}>
    <span class="muted sm">{t('runc.preset.pick')}</span>
    <div class="row wrap">
      {#each presets as p (p.id)}
        <button type="button" class="btn" class:primary={chosen?.id === p.id} aria-pressed={chosen?.id === p.id} {disabled} onclick={() => onpick(p.baseUrl, !!p.noKey)}>{name(p)}</button>
      {/each}
    </div>
    {#if chosen}
      <p class="muted sm" dir="auto">{hint(chosen)}
        {#if chosen.keysUrl}<a href={chosen.keysUrl} target="_blank" rel="noreferrer">{t('runc.preset.getKey')} <Icon name="external" size={13} /></a>{/if}</p>
    {/if}
  </div>
{/if}

<style>
  .presets { display: grid; gap: 6px; margin: 8px 0; }
  .row { display: flex; gap: 8px; flex-wrap: wrap; }
  p { margin: 0; }
</style>
