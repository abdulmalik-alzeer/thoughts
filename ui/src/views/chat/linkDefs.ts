// The link definitions of an answer being written, as markdown to put after each of its blocks
// (StreamText renders block by block; a "[text][1]" link needs its "[1]: …" line in the same source).

/** What marked's lexer collects as `tokens.links`: label (normalised) → address and title. */
export type LinkMap = Record<string, { href?: string | null; title?: string | null }>;

/** The definitions written again as markdown lines, or '' when there are none. */
export function linkDefs(links: LinkMap | undefined | null): string {
  if (!links) return '';
  const out: string[] = [];
  for (const [label, def] of Object.entries(links)) {
    const href = String(def?.href ?? '');
    if (!label || !href) continue;
    // In angle brackets an address may hold spaces; the brackets themselves cannot be in it.
    const dest = '<' + href.replace(/[<>\n]/g, (c) => encodeURIComponent(c)) + '>';
    const title = def?.title ? ' "' + String(def.title).replace(/[\\"]/g, '\\$&').replace(/\n/g, ' ') + '"' : '';
    // The label is kept as it was written (escapes and all), only lower-cased: written back as is.
    out.push(`[${label}]: ${dest}${title}`);
  }
  return out.join('\n');
}
