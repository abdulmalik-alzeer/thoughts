// Saving a generated file (an exported page, the calendar file) to the user's computer.
import { inTauri } from './api';
import { toast } from './store.svelte';
import { t } from './i18n';

/** Why the desktop app refused a file (its own words, in English), as the person is told. */
const REFUSED: [RegExp, string][] = [[/too large/i, 'cln.save.tooLarge'], [/kind of file/i, 'cln.save.kind'], [/not a word document/i, 'cln.save.notWord']];

/** Text kinds go to the shell as text; anything else (a Word document) as its bytes. */
const isText = (mime: string) => /^text\/|^application\/(json|xml)|\+xml$/i.test(mime);

/** Offers `data` as a download named `name`. Resolves 'saved', 'cancelled' or 'unsupported'.
 *  In the native window the shell opens the save dialog (`save_file` command): text is sent as text,
 *  other files as raw bytes with the name in a header. When the shell refuses the file (too large, a
 *  kind it does not save) the person is told why and nothing is saved ('cancelled'); only a shell too
 *  old to have the command, or no shell at all (the browser preview), gets an ordinary download link. */
export async function saveFile(name: string, data: string | Blob, mime = 'text/plain'): Promise<'saved' | 'cancelled' | 'unsupported'> {
  const blob = typeof data === 'string' ? new Blob([data], { type: mime }) : data;
  if (inTauri()) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      if (typeof data === 'string' || isText(blob.type || mime)) {
        const contents = typeof data === 'string' ? data : await blob.text();
        return (await invoke('save_file', { name, contents })) ? 'saved' : 'cancelled';
      }
      const bytes = new Uint8Array(await blob.arrayBuffer());
      return (await invoke('save_file', bytes, { headers: { 'x-file-name': encodeURIComponent(name) } })) ? 'saved' : 'cancelled';
    } catch (e) {
      const why = String((e as any)?.message ?? e ?? '');
      // "Command save_file not found": a shell from before the command; anything else is its answer.
      if (!/\bnot found\b/i.test(why)) {
        const key = REFUSED.find(([re]) => re.test(why))?.[1];
        toast(t(key ?? 'cln.save.failed'), { kind: 'error', key: 'save-file', detail: key ? undefined : why || undefined });
        return 'cancelled';
      }
    }
  }
  try {
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url; a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 10000);
    return inTauri() ? 'unsupported' : 'saved';
  } catch { return 'unsupported'; }
}
