// Customise › Plugins: everything the user customised as a list of named things. Each can be
// switched off and on, removed, exported to a file, changed with the assistant; a plugin file is
// added through the usual review. The service (`bundles.rs`) decides what belongs to which plugin.
import { api, post } from './api';
import { app } from './store.svelte';
import { t } from './i18n';
import { checkPending, loadLayer } from './layer';
import { fingerprintCode } from './packs';
import { asPackFile, packFileName, savePackFile } from './packfile';

export type PluginKind = 'bundle' | 'addon' | 'page' | 'task' | 'theme' | 'home' | 'sidebar' | 'todos' | 'pages' | 'look' | 'keys';
export interface PluginPart { kind: Exclude<PluginKind, 'bundle'> | 'css'; label: string; path?: string }
export interface PluginItem {
  id: string; kind: PluginKind; named: boolean; name: string; version: string; description: string; author: string;
  origin: 'made' | 'pack' | 'ready' | 'migrated' | 'loose'; on: boolean;
  state: 'ok' | 'off' | 'needsNewerApp' | 'unsupported' | 'empty'; minApp: string; message?: string;
  contains: PluginPart[]; permissions: string[]; permissionWords: string[];
  tasks: { file: string; name: string; approved: boolean }[];
  canSwitch: boolean; canRemove: boolean; canExport: boolean; updated?: string;
}

const KINDS: PluginKind[] = ['bundle', 'addon', 'page', 'task', 'theme', 'home', 'sidebar', 'todos', 'pages', 'look', 'keys'];
const STATES = ['ok', 'off', 'needsNewerApp', 'unsupported', 'empty'];
const str = (v: unknown, max: number) => (typeof v === 'string' ? v : '').slice(0, max);
const strs = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);

/** One row of the service's answer, with anything out of shape left out (the list is shown as it is). */
export function asPlugin(v: any): PluginItem | null {
  if (!v || typeof v.id !== 'string' || !v.id || !KINDS.includes(v.kind)) return null;
  return {
    id: v.id, kind: v.kind, named: v.named === true, name: str(v.name, 80) || v.id, version: str(v.version, 20), description: str(v.description, 400), author: str(v.author, 80),
    origin: ['made', 'pack', 'ready', 'migrated'].includes(v.origin) ? v.origin : 'loose', on: v.on === true,
    state: STATES.includes(v.state) ? v.state : 'ok', minApp: str(v.minApp, 20), message: str(v.message, 300) || undefined,
    contains: (Array.isArray(v.contains) ? v.contains : []).filter((p: any) => p && typeof p.kind === 'string').map((p: any) => ({ kind: p.kind, label: str(p.label, 80), path: str(p.path, 200) || undefined })),
    permissions: strs(v.permissions), permissionWords: strs(v.permissionWords),
    tasks: (Array.isArray(v.tasks) ? v.tasks : []).filter((x: any) => x && typeof x.file === 'string').map((x: any) => ({ file: x.file, name: str(x.name, 80) || x.file, approved: x.approved === true })),
    canSwitch: v.canSwitch === true, canRemove: v.canRemove === true, canExport: v.canExport === true, updated: str(v.updated, 40) || undefined,
  };
}

/** The list, and `problem`: why it cannot be shown or changed (the record of plugins is damaged), in the service's words. */
export async function loadPlugins(): Promise<{ plugins: PluginItem[]; problem: string }> {
  const r = await api('/plugins');
  return { plugins: (Array.isArray(r?.plugins) ? r.plugins : []).map(asPlugin).filter((p: PluginItem | null): p is PluginItem => !!p), problem: str(r?.problem, 600) };
}

export async function listPlugins(): Promise<PluginItem[]> { return (await loadPlugins()).plugins; }

/** The icon of a kind of plugin or of one thing a plugin contains (names from lib/Icon.svelte). */
export function kindIcon(kind: string): string {
  return ({ bundle: 'modules', addon: 'plugin', page: 'docs', task: 'clock', theme: 'palette', home: 'home', sidebar: 'sidebar', todos: 'todos', pages: 'window', look: 'palette', css: 'palette', keys: 'keyboard' } as Record<string, string>)[kind] ?? 'plugin';
}

/** What kind of thing it is, in a word or two. */
export function kindLabel(kind: string): string {
  const k = 'plg.kind.' + kind;
  const s = t(k);
  return s === k ? t('plg.kind.bundle') : s;
}

/** One thing a plugin contains, as its chip reads: "Add-on: hours", "Home: Board", "Its look". */
export function partText(p: PluginPart): string {
  const kind = kindLabel(p.kind);
  return p.label ? t('plg.part', { kind, label: p.label }) : kind;
}

/** Where a named plugin came from, in a few words ('' for a loose one). */
export function originText(p: PluginItem): string {
  return p.origin === 'loose' ? '' : t('plg.origin.' + p.origin);
}

/** The file a plugin is saved as: its name in whatever script it is written, then `.thoughts-plugin`.
 *  Only characters a file name cannot hold are dropped. */
export function exportFileName(name: string): string { return packFileName(name, 'thoughts-plugin'); }

/** After the live layer changed here, the running app loads it again (unless a change is being tried). */
async function reload() { if (!app.preview) await loadLayer().catch(() => {}); }

/** → the new History entry (empty when nothing changed). */
export async function switchPlugin(id: string, on: boolean): Promise<string> {
  const r = await post('/plugins/switch', { id, on });
  await reload();
  return String(r?.entry ?? '');
}
export async function removePlugin(id: string): Promise<string> {
  const r = await post('/plugins/remove', { id });
  await reload();
  return String(r?.entry ?? '');
}

export type PluginWords = { name: string; version: string; author: string; description: string };

/** Name a plugin (a loose item becomes a named plugin). → its id, and the History entry. */
export async function describePlugin(id: string, who: PluginWords): Promise<{ id: string; entry: string }> {
  const r = await post('/plugins/describe', { id, ...who });
  await reload();
  return { id: String(r?.id ?? id), entry: String(r?.entry ?? '') };
}

export type Saved = { result: 'saved' | 'copied' | 'cancelled'; fingerprint: string };

/** Save the named plugin `id` as `<name>.thoughts-plugin`: a zip of its real files. → how it went and the code of its fingerprint. */
export async function exportPlugin(id: string): Promise<Saved> {
  const f = asPackFile(await api('/plugins/export?id=' + encodeURIComponent(id)));
  return { result: await savePackFile(f.name, f), fingerprint: fingerprintCode(f.fingerprint) };
}

/** Save every plugin as one `.thoughts-pack` file (with which are on and off; what add-ons saved is not in it). */
export async function exportAllPlugins(): Promise<Saved> {
  const name = t('plg.all.fileName');
  const f = asPackFile(await api('/plugins/export?all=1&name=' + encodeURIComponent(name)));
  return { result: await savePackFile(name, f), fingerprint: fingerprintCode(f.fingerprint) };
}

/** Start a customisation chat whose change updates this plugin. → the chat's id. */
export async function customisePlugin(id: string): Promise<string> {
  const r = await post('/plugins/customise', { id });
  return String(r?.chatId ?? '');
}

/** What an import would do as a plugin, as the service worked it out (`POST /layer/pack/inspect`). */
export type PluginLook = {
  id: string; name: string; version: string; minApp: string; layerFormat: number; compatible: boolean; why?: 'newerApp' | 'newerFormat' | null;
  replaces: { id: string; version: string; name?: string } | null; olderThanYours: boolean; migrates: boolean;
} | { all: { id: string; name: string; version: string; on: boolean }[] };

/** The lines the import dialog adds for a plugin file. `refused`: it cannot be added at all. */
export function importLines(look: PluginLook | null | undefined, clean: (v: unknown, max: number) => string): { lines: string[]; refused: string } {
  if (!look) return { lines: [], refused: '' };
  if ('all' in look) return { lines: look.all.length ? [t('plg.imp.all', { names: look.all.slice(0, 12).map((x) => clean(x.name, 80)).join(' · ') })] : [], refused: '' };
  const name = clean(look.name, 80);
  const version = clean(look.version, 20);
  if (!look.compatible) return { lines: [], refused: look.why === 'newerApp' ? t('plg.imp.needsNewer', { version: clean(look.minApp, 20) }) : t('layer3.packNewer') };
  const lines = [version ? t('plg.imp.is', { name, version }) : t('plg.imp.isNoVersion', { name })];
  if (look.replaces) {
    const old = clean(look.replaces.version, 20);
    lines.push(look.olderThanYours ? t('plg.imp.older', { name, yours: old, version }) : old && version ? t('plg.imp.updates', { name, from: old, to: version }) : t('plg.imp.updatesNoVersion', { name }));
  } else lines.push(t('plg.imp.new'));
  if (look.migrates) lines.push(t('plg.imp.migrates'));
  return { lines, refused: '' };
}

/** The user's choice in the review: keep the change as one new plugin called `name`, or as it is. */
export async function setPendingPlugin(target: 'none' | 'new', name = ''): Promise<void> {
  const p = await post('/layer/pending/plugin', { target, name });
  if (p?.exists) app.pending = p; else await checkPending(false);
}
