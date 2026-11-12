// "What's new": shown once after the app was updated, and always available from Help.
import { APP_VERSION, go, toast } from './store.svelte';
import { t } from './i18n';
import { changelogSource } from './guides';

const KEY = 'thoughts.whatsNewSeen';
const seen = {
  get: () => { try { return localStorage.getItem(KEY); } catch { return null; } },
  set: (v: string) => { try { localStorage.setItem(KEY, v); } catch { /* ignore */ } },
};

/** The notes for one version: the part of the change log under its "## <version>" heading. */
export function notesFor(version: string, src = changelogSource()): string {
  const parts = src.split(/^(?=## )/m);
  const part = parts.find((p) => new RegExp(`^##\\s+v?${version.replace(/\./g, '\\.')}(?![\\w.])`).test(p));
  return part ? part.replace(/^##[^\n]*\n/, '').trim() : '';
}

/** A fresh install has nothing "new": remember this version without announcing it. */
export function markWhatsNewSeen() { seen.set(APP_VERSION); }

/** After an update, say once that there are release notes for this version. */
export function announceWhatsNew() {
  if (seen.get() === APP_VERSION) return;
  seen.set(APP_VERSION);
  if (!notesFor(APP_VERSION)) return;
  toast(t('help.new.toast', { v: APP_VERSION }), { kind: 'info', duration: 0, key: 'whatsnew', action: { label: t('help.new.see'), run: () => go({ kind: 'help', id: 'whatsnew' }) } });
}
