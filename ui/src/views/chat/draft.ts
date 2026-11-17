// A chat's saved draft (what was typed, which library files were named, which files were waiting),
// as it is kept in the browser's storage between visits.

export type DraftFile = { name: string; size: number; image: boolean };
export type SavedDraft = { text: string; mentions: { path: string; label: string }[]; files: DraftFile[] };

/** A message that could not be sent goes back into the saved draft, ahead of whatever was written
 *  there since (the chat may have been opened again and written in). `stored` is the raw saved
 *  value, or null. */
export function joinDraft(back: SavedDraft, stored: string | null, max: number): SavedDraft {
  let d: any = null;
  try { d = JSON.parse(stored || 'null'); } catch { /* unreadable: replaced */ }
  if (!d || typeof d !== 'object') return back;
  return {
    text: [back.text, String(d.text ?? '')].filter((x) => x.trim()).join('\n\n').slice(0, max),
    mentions: [...back.mentions, ...(Array.isArray(d.mentions) ? d.mentions : []).filter((m: any) => !back.mentions.some((x) => x.path === (typeof m === 'string' ? m : m?.path)))],
    files: [...back.files, ...(Array.isArray(d.files) ? d.files : [])],
  };
}
