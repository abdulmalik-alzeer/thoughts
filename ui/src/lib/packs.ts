// Customise › Add-ons, Ready-made and Share: one add-on at a time (switch off / on, remove, take a
// permission away, its saved data and earlier copies), the ready-made packs that come with the app,
// and packs that say who made them, which version they are and what their fingerprint is.
import { api, post } from './api';
import { app } from './store.svelte';
import { t } from './i18n';
import { checkPending, loadLayer, packId } from './layer';

export type DataInfo = { bytes: number | null; saved: string | null; versions: { id: string; saved: string | null; bytes: number }[] };
export type AddOn = {
  path: string; name: string; label: string; kind: 'plugin' | 'view'; on: boolean; manageable: boolean;
  permissions: string[]; write: string[]; description: string; unsupported: boolean; message?: string;
  data: DataInfo | null;
  /** The only folders it reads in (none: the whole library but private folders). */
  read?: string[];
};
export type Leftover = { name: string; label: string; data: DataInfo };

export async function addOns(): Promise<{ items: AddOn[]; leftover: Leftover[] }> {
  const r = await api('/layer/addons');
  return { items: Array.isArray(r?.items) ? r.items : [], leftover: Array.isArray(r?.leftover) ? r.leftover : [] };
}

/** After the live layer changed here, the running app loads it again (unless a change is being tried). */
async function reload() { if (!app.preview) await loadLayer().catch(() => {}); }

/** → the new history entry (empty when nothing changed). */
export async function switchAddOn(path: string, on: boolean): Promise<string> {
  const r = await post('/layer/addons/switch', { path, on });
  await reload();
  return String(r?.hash ?? '');
}
export async function removeAddOn(path: string): Promise<string> {
  const r = await post('/layer/addons/remove', { path });
  await reload();
  return String(r?.hash ?? '');
}
export async function withdrawPermission(path: string, permission: string): Promise<string> {
  const r = await post('/layer/addons/permission', { path, permission });
  await reload();
  return String(r?.hash ?? '');
}
export async function restoreData(name: string, version: string) {
  await post('/layer/addons/data/restore', { name, version });
  await reload();
}
export async function clearData(name: string) {
  await post('/layer/addons/data/clear', { name });
  await reload();
}
/** Delete, for good, what an add-on that is no longer in the app saved, with its earlier copies. */
export async function forgetData(name: string) {
  await post('/layer/addons/data/forget', { name });
}

// ---------- ready-made packs ----------
export type ReadyPack = {
  id: string; name: string; about: string; author: string; version: string; fingerprint: string; code: string;
  files: string[]; permissions: string[]; write: string[]; workflow: boolean; theme: boolean; added: boolean;
};
export async function readyPacks(): Promise<ReadyPack[]> {
  const r = await api('/layer/gallery');
  return Array.isArray(r) ? r : [];
}
/** Stage a ready-made pack for the usual review (it opens). Nothing is switched on here. */
export async function stageReady(id: string) {
  await post('/layer/gallery/stage', { id });
  await checkPending(true);
}

// ---------- packs: identity and fingerprint ----------
/** `fingerprint` is the full one (what every comparison uses); `code` is its first 128 bits in
 *  groups of four, for people to compare by eye; `claimed` is the code of what the file says. */
export type PackLook = {
  name: string; author: string; version: string; description: string; kind: 'full' | 'feature';
  files: string[]; fingerprint: string; code: string; claimed: string | null; matches: boolean | null; changes: number;
  earlier: { version: string; author: string; code: string; same: boolean } | null; appVersion: string;
};
/** Ask the service what a pack is before anything is staged: it checks the whole pack and
 *  calculates the fingerprint itself (the one written in the file is only compared). */
export async function inspectPack(pack: unknown): Promise<PackLook> {
  return await post('/layer/pack/inspect', { pack });
}

export type Identity = { name: string; author: string; version: string; description: string };

/** The code people compare by eye: the fingerprint's first 128 bits in eight groups of four (as the service shows it). */
export function fingerprintCode(full: string): string {
  return (String(full ?? '').match(/[0-9a-f]/gi) ?? []).slice(0, 32).join('').toUpperCase().match(/.{1,4}/g)?.join('-') ?? '';
}

/** A file name made from the pack's name: letters and digits of any language, dashes between. */
function fileName(name: string): string {
  const base = name.normalize('NFC').replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-+|-+$/g, '').slice(0, 60);
  return base || 'thoughts-pack';
}

/** Save a pack of the layer (everything, or `paths`) as a file. → how it went and the code of the pack's fingerprint. */
export async function savePack(who: Identity, paths?: string[]): Promise<{ result: 'saved' | 'copied' | 'cancelled'; fingerprint: string }> {
  const q = new URLSearchParams({ name: who.name, author: who.author, version: who.version, description: who.description });
  if (paths?.length) q.set('paths', paths.join(','));
  const pack = await api('/layer/export?' + q.toString());
  if (pack?.format !== 'thoughts-pack') throw new Error(t('layer.packInvalid'));
  // The same id for every export under this name from this computer: its look replaces only its own block.
  pack.id = packId(who.name);
  const fingerprint = fingerprintCode(String(pack.fingerprint ?? ''));
  const text = JSON.stringify(pack, null, 2);
  const name = fileName(who.name) + (who.version ? '-' + fileName(who.version) : '');
  if ((window as any).__TAURI_INTERNALS__) {
    // The native window has no download bar: the shell opens the save dialog itself and writes the file.
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      return { result: (await invoke('save_pack', { name, contents: text })) ? 'saved' : 'cancelled', fingerprint };
    } catch {
      try { await navigator.clipboard.writeText(text); return { result: 'copied', fingerprint }; } catch { /* fall through to a download */ }
    }
  }
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url; a.download = name + '.json';
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
  return { result: 'saved', fingerprint };
}

/** A size in words the user knows (bytes → "12 KB"). */
export function sizeText(bytes: number | null | undefined, fmt: (n: number) => string): string {
  const b = Number(bytes ?? 0);
  if (b < 1024) return t('ldc.size.b', { n: fmt(b) });
  if (b < 1024 * 1024) return t('ldc.size.kb', { n: fmt(Math.round(b / 102.4) / 10) });
  return t('ldc.size.mb', { n: fmt(Math.round(b / (1024 * 102.4)) / 10) });
}
