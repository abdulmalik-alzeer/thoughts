// Other providers (Settings › Assistant): the address rules the background service applies, mirrored
// here only to say things before saving. The service decides (`settings::check_base_url_for`,
// `keyless_allowed`); nothing here is a guard.

function hostOf(u: string): { https: boolean; host: string } | null {
  const m = /^(https?):\/\/([^/?#\s]*)/i.exec(u.trim());
  if (!m || m[2].includes('@')) return null;
  const host = m[2].replace(/:\d+$/, '');
  return host ? { https: m[1].toLowerCase() === 'https', host } : null;
}

const thisComputer = (h: string) => h === 'localhost' || h === '127.0.0.1';
function privateIPv4(h: string): boolean {
  const p = h.split('.');
  if (p.length !== 4 || !p.every((x) => /^\d{1,3}$/.test(x) && Number(x) <= 255)) return false;
  const [a, b] = p.map(Number);
  return a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168);
}

/** Can the service at `u` be used with "No key needed"? On this computer, or (full access on) a private address. */
export function keylessAllowed(u: string, full: boolean): boolean {
  const h = hostOf(u);
  return !!h && (thisComputer(h.host) || (full && privateIPv4(h.host)));
}

/** Is `u` a plain http:// address on the home network, accepted only because full access is on? */
export function onHomeNetwork(u: string, full: boolean): boolean {
  const h = hostOf(u);
  return !!h && full && !h.https && privateIPv4(h.host);
}

// ---------- the "Use another service" page (round 0.7.2) ----------
// One short list of ready-made services (`GET /ai/services`, `providers::SERVICES`) and one form for
// everything else. What the page shows and may send is worked out here, so it can be tested without
// a window; the background service still decides every save.

/** The two assistant programs. A service reaches Claude Code only at an address that speaks Anthropic's
 *  format, and Codex only at one that speaks OpenAI's: which of them a service works with is a fact
 *  about the service, said on the page. */
export type Program = 'claude' | 'codex';
export const PROGRAM_NAME: Record<Program, string> = { claude: 'Claude Code', codex: 'Codex' };
/** A ready-made service: its address for each program (`null` = not for that one; `''` for Claude
 *  Code = Anthropic itself, a key and no address). */
export type Service = { id: string; claude: string | null; codex: string | null; noKey: boolean; keysUrl: string };
/** What is saved for one program (`settings.ai.claude` / `.codex`; owned by the background service). */
export type Saved = { baseUrl?: string | null; hasApiToken?: boolean; keyless?: boolean } | undefined;
/** The entry for any service that is not ready-made: address, key and model typed by the user. */
export const CUSTOM = 'custom';

const tidy = (u: string | null | undefined) => String(u ?? '').trim();
const ANTHROPIC = /^https:\/\/api\.anthropic\.com(?:[:/?#]|$)/i;
/** Where a program's requests go when it is not its maker's own service ('' for that). For Claude
 *  Code, Anthropic's own address counts as none, exactly as the background service reads it. */
export function destination(program: Program, u: string | null | undefined): string {
  const s = tidy(u);
  return program === 'claude' && ANTHROPIC.test(s) ? '' : s;
}
/** The same address, whatever the letter case or a slash at the end. */
export const sameAddress = (a: string, b: string) => tidy(a).replace(/\/+$/, '').toLowerCase() === tidy(b).replace(/\/+$/, '').toLowerCase();

export function programsOf(s: Service): Program[] {
  return (['codex', 'claude'] as Program[]).filter((p) => s[p] !== null && s[p] !== undefined);
}

/** Which entry of the page a program's saved set-up is: a ready-made service's id, `CUSTOM` for any
 *  other address (one saved from a preset the page no longer lists included), or '' when the program
 *  uses its own sign-in. */
export function savedAs(program: Program, saved: Saved, services: Service[]): string {
  const d = destination(program, saved?.baseUrl);
  if (!d) return program === 'claude' && saved?.hasApiToken ? (services.find((s) => s.claude === '')?.id ?? CUSTOM) : '';
  return services.find((s) => !!s[program] && sameAddress(s[program] as string, d))?.id ?? CUSTOM;
}

/** Is the saved set-up one that runs: a service with its key (or none needed), or a key missing? */
export function savedKind(program: Program, saved: Saved): 'own' | 'service' | 'incomplete' {
  if (!destination(program, saved?.baseUrl)) return program === 'claude' && saved?.hasApiToken ? 'service' : 'own';
  return saved?.hasApiToken || saved?.keyless ? 'service' : 'incomplete';
}

/** The program a newly opened entry is for: the only one it works with; else the one already using
 *  it; else the assistant the user chose, if it is installed; else the installed one. The generic
 *  entry starts with Codex (OpenAI-compatible is what most services are). */
export function startProgram(id: string, services: Service[], saved: Record<Program, Saved>, primary: string | undefined, installed: Record<Program, boolean>): Program {
  const svc = services.find((s) => s.id === id);
  const can = svc ? programsOf(svc) : (['codex', 'claude'] as Program[]);
  if (can.length === 1) return can[0];
  const using = can.find((p) => savedAs(p, saved[p], services) === id);
  if (using) return using;
  if (!svc) return 'codex';
  const chosen = can.find((p) => p === primary && installed[p]);
  return chosen ?? can.find((p) => installed[p]) ?? can[0];
}

/** The address a newly opened entry starts with: the saved one when this entry is what the program
 *  uses now (as saved, so an unchanged form never looks edited); else the ready-made one; else blank. */
export function startAddress(id: string, program: Program, services: Service[], saved: Saved): string {
  if (savedAs(program, saved, services) === id) return tidy(saved?.baseUrl);
  return services.find((s) => s.id === id)?.[program] ?? '';
}

export type Form = { url: string; token: string; noKey: boolean };
/** What a form may do, given what is saved for its program:
 *  - `noKey`: sent as "no key needed" (only an address on this computer, and only with no key typed);
 *  - `needKey`: nothing can be saved yet because the key for this address is missing (a key saved
 *    for one address is never reused for another);
 *  - `canSave`: something changed and it is complete; `canTest`: the saved set-up is this very form;
 *  - `body`: what `PUT /ai/<program>/provider` gets. */
export function formState(program: Program, f: Form, saved: Saved, full: boolean) {
  const url = tidy(f.url), token = f.token.trim();
  const d = destination(program, url), sd = destination(program, saved?.baseUrl);
  const badUrl = !!url && !/^https?:\/\/\S+$/.test(url);
  const noKey = !!d && f.noKey && !token && keylessAllowed(url, full);
  const same = d === sd;
  const keeps = same && !!saved?.hasApiToken && !noKey;
  const needKey = !token && !keeps && !noKey;
  // Codex has no "own service with a key" spelling: without an address there is nothing to save.
  const complete = !badUrl && (program === 'claude' || !!d);
  const dirty = url !== tidy(saved?.baseUrl) || !!token || noKey !== (!!sd && !!saved?.keyless);
  return {
    badUrl, noKey, needKey, dirty,
    /** The key field is asked for again because the address changed. */
    keyMoved: needKey && !same && !!saved?.hasApiToken,
    canSave: dirty && complete && !needKey,
    canTest: !dirty && complete && (!!saved?.hasApiToken || (!!d && !!saved?.keyless)),
    body: { baseUrl: url || null, token: token || null, noKey },
  };
}

/** `s` kept left-to-right inside a sentence of either direction (an address in an Arabic line):
 *  wrapped in the two isolate marks, written by number so no invisible character sits in the source. */
export const ltr = (s: string) => String.fromCharCode(0x2066) + s + String.fromCharCode(0x2069);
