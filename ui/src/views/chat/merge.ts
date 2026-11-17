// How the chat view keeps its copy of a chat in step with the service: what one event does to it,
// what a fetched copy replaces, and which fetched copies count at all. Kept free of Svelte and the
// DOM so the rules can be tested on their own (merge.test.ts); Chat.svelte carries out the effects.
//
// The service numbers every piece of text (and of thinking) within a run and keeps the pieces of
// the message in progress, so the view can join in the middle of an answer: a fetch brings what
// there is so far, and events that arrive while it is on its way are kept and gone through again
// afterwards, so whatever the fetched copy lacks is added.
import type { Chat, ChatMessage, ChatPartial, DaemonEvent } from '../../lib/types';

/** The answer being written, as the view shows it. */
export type Stream = {
  /** The text of the message being written. */
  text: string;
  /** Number of the last text piece in `text`. */
  seq: number;
  /** The assistant's reasoning so far, for the "thinking" line. */
  thinking: string;
  /** Number of the last thinking piece in `thinking`. */
  thinkSeq: number;
};

export const emptyStream = (): Stream => ({ text: '', seq: 0, thinking: '', thinkSeq: 0 });

/** What the view still has to do after an event (the merge itself has been made). */
export type Effects = {
  /** Fetch the chat again: pieces went missing, or an answer ended. */
  reload?: boolean;
  /** Follow the new output down (`force`: whatever the reader is doing, e.g. their own message). */
  scroll?: 'follow' | 'force';
  /** An answer has just ended (live, not gone through again). */
  ended?: boolean;
};

export type MergeContext = {
  /** The event arrived while the chat was being fetched and is gone through a second time, now
   *  that the fetched copy is on screen. */
  again: boolean;
  /** A fetch of this chat is on its way (a gap then needs no fetch of its own). */
  loading: boolean;
  /** A message is on its way to the service: the chat counts as working whatever an update says. */
  sending: boolean;
};

/** The fetched copy's answer in progress replaces what is on screen. */
export function applyPartial(chat: Pick<Chat, 'running'>, s: Stream, p: ChatPartial | undefined | null): void {
  if (!chat.running) { Object.assign(s, emptyStream()); return; }
  if (!p) return; // an older service does not send it: keep what is on screen
  s.text = String(p.text ?? '');
  s.seq = Number(p.seq) || 0;
  s.thinking = String(p.thinking ?? '');
  s.thinkSeq = Number(p.thinkingSeq) || 0;
}

/** One event about this chat, merged into `chat` and `s` (both changed in place). */
export function applyEvent(chat: Chat, s: Stream, e: DaemonEvent, ctx: MergeContext): Effects {
  const { again } = ctx;
  switch (e.type) {
    case 'chat.message': {
      // A message that arrives again under the same id replaces the one shown: a failure the
      // service recovered from by itself becomes a quiet note.
      const at = chat.messages.findIndex((m: ChatMessage) => m.id === e.message.id);
      // The fetched copy already had it. (It is only ever replaced by the note that follows a
      // recovered failure, never the reverse.)
      if (at >= 0 && again) { if (e.message.tool?.code !== 'sessionLost') chat.messages[at] = e.message; return {}; }
      if (at >= 0) chat.messages[at] = e.message; else chat.messages.push(e.message);
      if (e.message.role === 'assistant') s.text = '';
      if (e.message.role !== 'user') s.thinking = '';
      if (e.message.role === 'user') chat.suggestion = undefined;
      return { scroll: e.message.role === 'user' ? 'force' : 'follow' };
    }
    case 'chat.suggestion':
      // What the user might say next, from the assistant program; offered under the answer.
      if (!again) chat.suggestion = e.text;
      return {};
    case 'chat.message.changed': {
      // A message that only changed state (an answer's changes undone or put back): replaced where it is.
      const at = chat.messages.findIndex((m: ChatMessage) => m.id === e.message.id);
      if (at >= 0) chat.messages[at] = e.message;
      return {};
    }
    case 'chat.message.removed':
      // A message whose answer could not be started is taken back: its words are in the box again.
      chat.messages = chat.messages.filter((m: ChatMessage) => m.id !== e.messageId);
      return {};
    case 'chat.delta': {
      const seq = typeof e.seq === 'number' ? e.seq : s.seq + 1;
      if (seq === s.seq + 1) { s.text += e.text; s.seq = seq; return {}; }
      // Pieces in between never arrived: take the service's copy rather than show a text with holes.
      if (seq > s.seq + 1 && !again && !ctx.loading) return { reload: true };
      return {};
    }
    case 'chat.thinking': {
      // Numbered like the text: a piece is added once, whether it came live or is gone through again.
      // (Pieces in between that went missing only shorten the line, so they are not fetched.)
      const seq = e.seq;
      if (typeof seq === 'number') { if (seq > s.thinkSeq) { s.thinking += e.text; s.thinkSeq = seq; } }
      else if (!again) s.thinking += e.text; // an older service does not number them
      return {};
    }
    case 'chat.done':
      if (again && !chat.running) return {}; // the fetched copy is from after the end
      Object.assign(s, emptyStream());
      return { reload: true, ended: !again };
    case 'chat.updated':
      if (again || !e.chat) return {};
      chat.running = e.chat.running || ctx.sending;
      chat.title = e.chat.title;
      // A wrap or reorganize says where it stands (preview, saving, ready to review).
      if (e.chat.phase && chat.meta && typeof chat.meta === 'object') chat.meta.phase = e.chat.phase;
      return {};
    default:
      return {};
  }
}

/** The fetches of one chat that are on their way, and the events that arrived meanwhile.
 *
 *  Fetches can overlap (a gap, the end of an answer, a reconnect, a panel's own refresh) and their
 *  replies can come back in any order. A reply older than the one already on screen is dropped:
 *  otherwise a copy from before the end of an answer could put back "still working" after it ended.
 *  The held events are kept until no fetch is on its way any more, and each copy that is used goes
 *  through all of them (doing so twice is harmless: every rule above adds a thing only once). */
export class Fetches {
  private started = 0;
  private shown = 0;
  private open = 0;
  private held: DaemonEvent[] = [];

  /** Is a fetch on its way? Events that arrive now must be held. */
  get loading(): boolean { return this.open > 0; }

  /** The ticket of the fetch started last. */
  get newest(): number { return this.started; }

  /** A fetch starts; its ticket. */
  begin(): number { this.open++; return ++this.started; }

  /** An event arrived: kept while a fetch is on its way. */
  hold(e: DaemonEvent): void { if (this.open) this.held.push(e); }

  /** The reply of fetch `ticket` arrived: true when it is newer than the copy on screen, so it is
   *  used (and becomes the copy on screen); false when it is stale and must be dropped. */
  accept(ticket: number): boolean {
    if (ticket <= this.shown) return false;
    this.shown = ticket;
    return true;
  }

  /** The events to go through again over a copy just put on screen. */
  replay(): DaemonEvent[] { return [...this.held]; }

  /** Fetch done (used, dropped or failed). */
  end(): void { if (this.open > 0) this.open--; if (!this.open) this.held = []; }
}

/** After a reconnect, has the chat's last answer finished well, so a message waiting for it may go?
 *  (Live, only a successful `chat.done` sends it; one that was stopped or failed leaves it waiting,
 *  and the chat offers to send it now.) `asked`: the user asked to send it as soon as the answer ended. */
export function readyForQueued(chat: Pick<Chat, 'running' | 'interrupted' | 'messages'>, asked: boolean): boolean {
  if (chat.running) return false;
  if (asked) return true;
  if (chat.interrupted) return false;
  // The answer's last word (its tool calls aside): a reply, not a "stopped" note, an error, or nothing yet.
  for (let i = chat.messages.length - 1; i >= 0; i--) {
    const role = chat.messages[i].role;
    if (role === 'tool' || role === 'tool_result') continue;
    return role === 'assistant';
  }
  return false;
}
