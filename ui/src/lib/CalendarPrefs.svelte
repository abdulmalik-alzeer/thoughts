<script lang="ts">
  // Settings › General › Calendar: first day of the week, the weekend, Hijri dates, and the
  // reminder time for todos sent to the user's own calendar. Edits the Settings draft (`cal`), so
  // it is saved with the rest of the page.
  import { t, fmtHijri } from './i18n';
  import { autoWeekStart, autoWeekend, loadLibraryWeek } from './weekPrefs.svelte';

  type Cal = { weekStart: number | null; weekend: number[] | null; dates: string; alarm: string | null };
  let { cal = $bindable() }: { cal: Cal } = $props();
  if (!cal) cal = { weekStart: null, weekend: null, dates: 'gregorian', alarm: '09:00' };

  const DAYS = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'];
  // Stored sorted (as the service keeps them); shown Friday-first so "Saturday and Sunday" reads naturally.
  const WEEKENDS = ['5,6', '0,6', '5', '0'];
  const dayName = (d: number) => t('todob.day.' + DAYS[d]);
  const daysName = (ds: number[]) => [...ds].sort((a, b) => ((a + 2) % 7) - ((b + 2) % 7)).map(dayName).join(t('todob.and'));
  const weekendName = (v: string) => daysName(v.split(',').map(Number));
  // What "Automatic" currently means, so the choice is never a mystery.
  loadLibraryWeek();
  const autoStart = $derived(dayName(autoWeekStart()));
  const autoEnd = $derived(daysName(autoWeekend()));
  // Derived, so switching the language on this page shows the example in the new language.
  const example = $derived(fmtHijri(new Date(), { day: 'numeric', month: 'long', year: 'numeric' }));

  let lastAlarm = cal.alarm || '09:00';
</script>

<fieldset class="calp">
  <legend>{t('todob.set.title')}</legend>
  <label for="st-wstart">{t('todob.set.weekStart')}</label>
  <select id="st-wstart" value={cal.weekStart ?? ''} onchange={(e) => { const v = e.currentTarget.value; cal.weekStart = v === '' ? null : +v; }}>
    <option value="">{t('todob.set.auto', { value: autoStart })}</option>
    {#each [6, 0, 1] as d (d)}<option value={d}>{dayName(d)}</option>{/each}
  </select>

  <label for="st-wend">{t('todob.set.weekend')}</label>
  <select id="st-wend" value={cal.weekend ? cal.weekend.join(',') : ''} onchange={(e) => { const v = e.currentTarget.value; cal.weekend = v === '' ? null : v.split(',').map(Number).sort((a, b) => a - b); }}>
    <option value="">{t('todob.set.auto', { value: autoEnd })}</option>
    {#each WEEKENDS as w (w)}<option value={w}>{weekendName(w)}</option>{/each}
  </select>

  <label for="st-dates">{t('todob.set.dates')}</label>
  <select id="st-dates" bind:value={cal.dates} aria-describedby="st-dates-hint">
    <option value="gregorian">{t('todob.set.gregorian')}</option>
    <option value="both">{t('todob.set.both')}</option>
  </select>
  {#if example}<p class="muted sm" id="st-dates-hint">{t('todob.set.datesHint', { date: example })}</p>{/if}

  <label class="check"><input type="checkbox" checked={!!cal.alarm} onchange={(e) => { cal.alarm = e.currentTarget.checked ? lastAlarm : null; }} aria-describedby="st-alarm-hint" /> {t('todob.set.alarm')}</label>
  {#if cal.alarm}
    <div class="row">
      <label class="inl" for="st-alarm-time">{t('todob.set.alarmAt')}</label>
      <input id="st-alarm-time" type="time" value={cal.alarm} onchange={(e) => { const v = e.currentTarget.value; if (v) { cal.alarm = v; lastAlarm = v; } }} />
    </div>
  {/if}
  <p class="muted sm" id="st-alarm-hint">{t('todob.set.alarmHint')}</p>
</fieldset>

<style>
  .calp { border: 0; padding: 0; margin: 14px 0 0; min-width: 0; }
  .calp legend { font-weight: 600; padding: 0; margin-bottom: 4px; }
  .calp select { width: auto; max-width: 100%; }
  .calp input[type="time"] { width: auto; }
  .inl { margin: 0; }
  .sm { font-size: var(--fs-xs); margin: 4px 0 0; }
</style>
