// One sidebar change made straight from an entry's right-click menu, without opening the editor
// (hide this page, open the app on it): written at once like the editor's own changes, with a
// message that offers to put it back.
import { app, toast, type NavConfig } from './store.svelte';
import { put } from './api';
import { t } from './i18n';
import { failed } from './errors';
import { cleanNav, emptyNav, sameNav } from './navEdit';
import { fullAccessOn } from './fullAccess';

const write = async (nav: NavConfig) => {
  const full = fullAccessOn();
  await put('/layer/nav', cleanNav(nav, full));
  if (!app.preview) app.nav = { ...emptyNav(), ...cleanNav(nav, full) } as NavConfig;
};

/** Save `next` as the sidebar and say `text`; the message's button writes the earlier one back. */
export async function quickNav(next: NavConfig, text: string) {
  const was = ($state.snapshot(app.nav) as NavConfig | null) ?? emptyNav();
  if (sameNav(was, next, fullAccessOn())) return;
  try {
    await write(next);
    toast(text, { kind: 'success', key: 'sidebar', action: { label: t('sb72.t.back'), run: () => { write(was).catch((e) => failed(t('sb72.saveFailed'), e)); } } });
  } catch (e) { failed(t('sb72.saveFailed'), e); }
}
