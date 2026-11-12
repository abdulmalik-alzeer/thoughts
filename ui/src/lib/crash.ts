// Errors that escape a view or an event handler: written to the background service's log (so a
// diagnostics report shows them), never shown as a toast on their own. A burst is capped so one
// broken render loop cannot flood the log.
import { post } from './api';

let sent = 0;
let last = '';

/** The text a person can copy and send: message plus the first lines of the stack. */
export function crashText(err: unknown): string {
  if (err instanceof Error) return [`${err.name}: ${err.message}`, ...(err.stack ?? '').split('\n').slice(1, 12)].join('\n').trim();
  try { return typeof err === 'string' ? err : JSON.stringify(err); } catch { return String(err); }
}

export function reportError(where: string, err: unknown) {
  const text = crashText(err).slice(0, 2000);
  console.error(`[${where}]`, err);
  if (text === last || sent >= 20) return;
  last = text;
  sent++;
  try { post('/ui-error', { where: where.slice(0, 60), message: text }).catch(() => {}); } catch { /* not connected */ }
}
