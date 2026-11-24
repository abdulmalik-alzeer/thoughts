// Plugin and pack files (0.7.0): a zip of the customisation's real files with `plugin.json` at its
// root, made by the service (`pack_file.rs`). `.thoughts-plugin` holds one plugin, `.thoughts-pack`
// everything else (Export all, a feature pack, a full pack). The window only carries the bytes: it
// saves what the service made and sends a chosen file as it is; the service reads and checks it
// (an archive, or a legacy `.json` pack or plugin file).
import { inTauri } from './api';
import { t } from './i18n';

export type PackKind = 'thoughts-plugin' | 'thoughts-pack';
/** What an export route answers: the file as base64, the extension it is saved under, and what is shown afterwards. */
export type PackFile = { extension: PackKind; name: string; kind: 'full' | 'feature'; fingerprint: string; file: string };

/** What the "Add a file…" pickers accept: both kinds, a zip someone made by hand, and legacy `.json` files. */
export const PICK_ACCEPT = '.thoughts-plugin,.thoughts-pack,.zip,.json,application/zip,application/json';
/** A chosen file, at most (the service's own limit). */
export const MAX_PICK_BYTES = 5 * 1024 * 1024;

/** The service's answer, or the refusal when it is not one. */
export function asPackFile(v: any): PackFile {
  if (!v || (v.extension !== 'thoughts-plugin' && v.extension !== 'thoughts-pack') || typeof v.file !== 'string' || !v.file) throw new Error(t('layer.packInvalid'));
  return { extension: v.extension, name: String(v.name ?? '').slice(0, 80), kind: v.kind === 'feature' ? 'feature' : 'full', fingerprint: String(v.fingerprint ?? ''), file: v.file };
}

export function fromBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function toBase64(bytes: Uint8Array): string {
  let s = '';
  // In slices: one call with millions of arguments would overflow the stack.
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

/** `<name>.<extension>`: the name in whatever script it is written; only characters a file name cannot hold are dropped. */
export function packFileName(name: string, extension: PackKind, fallback = 'plugin'): string {
  const base = String(name ?? '').normalize('NFC').replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-+|-+$/g, '').slice(0, 60);
  return `${base || fallback}.${extension}`;
}

/** Hand the file the service made to the user. `base`: the file's name without its extension. */
export async function savePackFile(base: string, f: PackFile): Promise<'saved' | 'cancelled'> {
  const name = packFileName(base, f.extension, f.extension === 'thoughts-plugin' ? 'plugin' : 'thoughts-pack');
  const bytes = fromBase64(f.file);
  if (inTauri()) {
    // The native window has no download bar: the shell opens the save dialog itself and writes the bytes.
    const { invoke } = await import('@tauri-apps/api/core');
    return (await invoke('save_pack', bytes, { headers: { 'x-file-name': encodeURIComponent(name), 'x-file-kind': f.extension } })) ? 'saved' : 'cancelled';
  }
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: 'application/zip' }));
  const a = document.createElement('a');
  a.href = url; a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
  return 'saved';
}

/** A chosen file as the service takes it (`{file}`: its bytes, base64), whatever kind it is. */
export async function fileBody(file: File): Promise<{ file: string }> {
  if (file.size > MAX_PICK_BYTES) throw new Error(t('layer.packTooBig'));
  return { file: toBase64(new Uint8Array(await file.arrayBuffer())) };
}
