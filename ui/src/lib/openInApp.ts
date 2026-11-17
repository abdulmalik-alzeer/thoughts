// Opening a library or chat file in its own app, and showing it in its folder, through the desktop
// app's own checks (src-tauri/src/openfile.rs). The list below is the shell's `SAFE_TO_OPEN`, kept
// in step by a test there: kinds that may carry macros or programs (old Word/Excel/PowerPoint
// files, Rich Text, OpenDocument…) are offered "Show in Finder" instead.
import { inTauri } from './api';

export const OPEN_IN_APP = ['pdf', 'docx', 'xlsx', 'pptx', 'csv', 'tsv', 'txt', 'md', 'png', 'jpg', 'jpeg', 'gif', 'webp', 'heic', 'tiff', 'bmp', 'mp3', 'm4a', 'wav', 'mp4', 'mov', 'epub', 'aac', 'caf', 'aiff', 'aif'];

/** Whether the desktop app opens this file in its usual app (otherwise it can only show it in its folder). */
export function canOpenInApp(name: string): boolean {
  const ext = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase() ?? '';
  return inTauri() && OPEN_IN_APP.includes(ext);
}

/** The label of "show in its folder" for this computer: Finder, File Explorer or the file manager. */
export function revealLabelKey(): string {
  const os = (window as any).__THOUGHTS_PLATFORM__ || (/Mac/i.test(navigator.platform || navigator.userAgent) ? 'macos' : /Win/i.test(navigator.platform || navigator.userAgent) ? 'windows' : 'linux');
  return os === 'macos' ? 'sec-shell-open.reveal.mac' : os === 'windows' ? 'sec-shell-open.reveal.windows' : 'sec-shell-open.reveal.linux';
}

async function invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const core = await import('@tauri-apps/api/core');
  return core.invoke<T>(cmd, args);
}

/** Open a library file in its usual app. `false`: the user chose not to (a file in a private folder). */
export const openLibraryFile = (path: string) => invoke<boolean>('open_library_file', { path });
/** Show a library file in its folder. `false`: the user chose not to (a file in a private folder). */
export const revealLibraryFile = (path: string) => invoke<boolean>('reveal_library_file', { path });
/** Show a file sent in a chat in its folder. */
export const revealChatAttachment = (chatId: string, name: string) => invoke<void>('reveal_chat_attachment', { chatId, name });
