// The blocks of an answer being written, read without going over the whole answer each time.
// Text only ever arrives at the end, and a finished block cannot change once two more blocks have
// started after it: so those are kept, and only the end of the answer is read again (from the
// start of a block, where markdown needs nothing that came before it but link definitions, which
// are kept too). A text that does not continue the last one is read from the start.
import { marked } from 'marked';
import type { LinkMap } from './linkDefs';

export type Tok = { raw: string; type: string };

/** Blocks still read again each time, besides blank space (the last may still grow into the one before). */
const KEEP = 2;

export class StreamLexer {
  private stableText = '';
  private stable: Tok[] = [];
  private stableLinks: LinkMap = {};

  /** The answer's blocks so far, with the link definitions seen in it. */
  read(text: string): { tokens: Tok[]; links: LinkMap } {
    if (!text.startsWith(this.stableText)) this.reset();
    const rest = text.slice(this.stableText.length);
    const lexed = marked.lexer(rest) as unknown as Tok[] & { links?: LinkMap };
    const tail = Array.from(lexed, (x) => ({ raw: String(x.raw ?? ''), type: String(x.type) }));
    const links = { ...this.stableLinks, ...(lexed.links ?? {}) };
    const tokens = [...this.stable, ...tail];
    // Keep what can no longer change. Each kept block is found in the text in turn: what lies
    // between two of them may only be blank lines or link definitions (which marked takes out of
    // the blocks); anything else and nothing is kept this time.
    let solid = 0, seen = 0;
    for (let i = tail.length - 1; i >= 0; i--) {
      if (tail[i].type !== 'space' && ++seen > KEEP) { solid = i + 1; break; }
    }
    let pos = 0, ok = solid > 0;
    for (let i = 0; ok && i < solid; i++) {
      const at = tail[i].raw ? rest.indexOf(tail[i].raw, pos) : pos;
      if (at < 0 || !GAP.test(rest.slice(pos, at))) ok = false;
      else pos = at + tail[i].raw.length;
    }
    if (ok) {
      const kept = rest.slice(0, pos);
      this.stable.push(...tail.slice(0, solid));
      this.stableText += kept;
      this.stableLinks = { ...this.stableLinks, ...defsIn(kept) };
    }
    return { tokens, links };
  }

  reset() { this.stableText = ''; this.stable = []; this.stableLinks = {}; }
}

/** What may lie between two blocks besides them: blank space and link definitions. */
const GAP = /^(?:\s|[ ]{0,3}\[[^\]\n]+\]:[^\n]*(?:\n|$))*$/;

/** Link definitions written in this text. */
function defsIn(src: string): LinkMap {
  if (!/^\s{0,3}\[[^\]]+\]:/m.test(src)) return {};
  try { return ((marked.lexer(src) as { links?: LinkMap }).links) ?? {}; } catch { return {}; }
}
