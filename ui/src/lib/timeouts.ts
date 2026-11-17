// How long a request to the background service may go unanswered before the window gives up on it.
//
// A service that accepts the connection but never answers (stuck after the computer slept, or
// behind something that never ends) would otherwise leave every waiting page on its skeleton and
// every busy button spinning for good. Most requests answer in well under a second; a short edit
// waits at most 20 seconds for a sync to finish (`EDIT_WAIT` in the daemon), so 30 seconds is the
// default. Requests that can legitimately take minutes are listed in `LONG` and have no limit.

/** The default limit, in milliseconds. */
export const DEFAULT_TIMEOUT_MS = 30_000;
/** Previews of library files (a PDF, a picture) and the calendar file: larger, so a little longer. */
export const BLOB_TIMEOUT_MS = 120_000;

/** `METHOD /path` (no `/api`, no query) of requests that can take minutes: no time limit. They do
 *  real work while the person waits for it — a sync over the network, an import, a Wrap, the
 *  assistant answering — and cutting them off would only report a failure that did not happen. */
export const LONG: RegExp[] = [
  // Sending a message (its pictures and files ride along; a customise chat prepares its copy first).
  /^POST \/chats\/[^/]+\/send$/,
  // Sync and backup: they talk to the other computer or the backup service.
  /^POST \/git\/(sync|resolve|join|remotes|hostkey|credentials)$/,
  /^GET \/git\/hostkey$/,
  // Wrap and reorganize: start, plan, save, undo.
  /^POST \/wrap(\/[^/]+\/[a-z-]+)?$/,
  /^POST \/reorganize(\/[^/]+\/[a-z-]+)?$/,
  // Bringing notes and files in, saving outputs, recordings and their transcripts.
  /^POST \/library\/(import|import-folder|import-folder\/preview|output|picture|item\/meeting|item\/transcript)$/,
  /^POST \/onboarding(\/adopt|\/setup\/[^/]+)?$/,
  // History repair and undoing or restoring through git.
  /^(GET|POST) \/library\/history\/(check|repair)$/,
  /^POST \/library\/(restore|undo)$/,
  // Moving to another computer, sharing and installing customisations.
  /^POST \/move\/(export|inspect|import)$/,
  /^GET \/layer\/export$/,
  /^POST \/layer\/(import|pack\/inspect|gallery\/stage|customize|rollback|revert|undo-last)$/,
  /^POST \/layer\/pending\/(apply|accept|continue|review-again)$/,
  // An add-on asking the assistant, a routine run by hand.
  /^POST \/layer\/addon\/ask$/,
  /^POST \/workflows\/run$/,
  // Updates (the check reaches the internet) and the self-check, repairs and removal.
  /^POST \/(updates\/resolve|release-check|health\/fix|remove)$/,
  /^GET \/(health|diagnostics|remove\/plan)$/,
  // The assistants: signing in, testing a connection, asking the command line what it offers. A
  // key saved into the system keychain can wait for the person to answer the keychain's question.
  /^POST \/ai\/(claude|codex)\/(login|recheck)$/,
  /^POST \/ai\/claude\/(provider\/test|token)$/,
  /^(PUT|DELETE) \/ai\/claude\/provider$/,
  /^GET \/ai\/(claude|codex)\/(options|status)$/,
  // Opening or adding another library.
  /^POST \/libraries\/(switch|add)$/,
];

/** The limit for one request: `override` when the caller gave one (0 = none), otherwise none for
 *  the `LONG` requests and `DEFAULT_TIMEOUT_MS` for everything else. */
export function timeoutFor(method: string, path: string, override?: number): number {
  if (override !== undefined) return Math.max(0, override);
  const p = path.split('?')[0].split('#')[0];
  const key = `${method.toUpperCase()} ${p}`;
  return LONG.some((re) => re.test(key)) ? 0 : DEFAULT_TIMEOUT_MS;
}

/** A signal that aborts after `ms` (never when 0), and whether it was the clock that aborted. */
export function deadline(ms: number): { signal?: AbortSignal; timedOut: () => boolean; clear: () => void } {
  if (!ms) return { signal: undefined, timedOut: () => false, clear: () => {} };
  const c = new AbortController();
  let fired = false;
  const timer = setTimeout(() => { fired = true; c.abort(); }, ms);
  return { signal: c.signal, timedOut: () => fired, clear: () => clearTimeout(timer) };
}
