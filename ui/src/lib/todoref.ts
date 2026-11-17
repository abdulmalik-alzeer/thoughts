// Which todo an open editor, date picker or move bar belongs to. A line number alone is not enough:
// when the list reloads while one is open (the assistant adds a task above, a sync lands), another
// todo can sit on that line by then. So the open box remembers the todo's file, line and exact text,
// shows only on the row that still matches all three, and sends those back when saving — if the
// line moved, the service's text check refuses the change instead of overwriting a different todo.
import type { TodoItem } from './types';

export type TodoRef = { file: string; line: number; text: string };

/** The ref of todo `it` in list file `file`, taken now. */
export const refOf = (file: string, it: Pick<TodoItem, 'line' | 'text'>): TodoRef => ({ file, line: it.line, text: it.text });

/** True when the open box `open` belongs to this row: same file, same line, same text. */
export function isAt(open: TodoRef | null | undefined, file: string, it: Pick<TodoItem, 'line' | 'text'>): boolean {
  return !!open && open.file === file && open.line === it.line && open.text === it.text;
}

/** After a reload: where the todo `open` points at is now. Its own line when it is still there;
 *  otherwise the one line in the same file with exactly its text (moved by a line added or removed
 *  above it); otherwise null — it was changed, deleted, moved away, or has a twin with the same
 *  text, and guessing could point the box at the wrong todo. Returns `open` itself when unchanged. */
export function reanchor(open: TodoRef | null, lists: { file: string; items: Pick<TodoItem, 'line' | 'text'>[] }[]): TodoRef | null {
  if (!open) return null;
  const list = lists.find((l) => l.file === open.file);
  if (!list) return null;
  if (list.items.some((it) => it.line === open.line && it.text === open.text)) return open;
  const same = list.items.filter((it) => it.text === open.text);
  return same.length === 1 ? { ...open, line: same[0].line } : null;
}
