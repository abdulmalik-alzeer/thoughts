// Saving a generated file (an exported page, the calendar file) to the user's computer.
import { inTauri } from './api';

/** Offers `data` as a download named `name`. Resolves 'saved', 'cancelled' or 'unsupported'.
 *  In the native window the shell opens the save dialog (`save_file` command) — when the shell is
 *  too old to have it we fall back to an ordinary download link. */
export async function saveFile(name: string, data: string | Blob, mime = 'text/plain'): Promise<'saved' | 'cancelled' | 'unsupported'> {
  const blob = typeof data === 'string' ? new Blob([data], { type: mime }) : data;
  if (inTauri()) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const contents = typeof data === 'string' ? data : await blob.text();
      return (await invoke('save_file', { name, contents })) ? 'saved' : 'cancelled';
    } catch { /* older shell: try a plain download */ }
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
