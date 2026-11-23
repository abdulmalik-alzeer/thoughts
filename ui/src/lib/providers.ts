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

/** Who answers Codex, as saved: its own set-up, another service, or an address without its key. */
export function codexProviderKind(c: { baseUrl?: string | null; hasApiToken?: boolean; keyless?: boolean } | undefined): 'own' | 'service' | 'incomplete' {
  if (!c || !String(c.baseUrl ?? '').trim()) return 'own';
  return c.hasApiToken || c.keyless ? 'service' : 'incomplete';
}
