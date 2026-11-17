// Editing a sent message and branching a chat: what the message rows offer, and what a branch
// shows about where it came from. The service does the work (`POST /chats/:id/edit`,
// `POST /chats/:id/branch`, crates/thoughtsd/src/branch.rs); this is the pure part, tested in branch.test.ts.

type Msg = { id: string; role: string; text?: string; ts?: string; attachments?: string[]; tool?: Record<string, any> };
type ChatLike = { kind?: string; provider?: string; messages: Msg[]; wrappedAt?: string | null; meta?: any };
type Listed = { id: string; title: string };

/** Where a branch came from (`meta.branch`, written by the service). */
export type BranchInfo = { from: string; fromTitle: string; at: string; atRole: string; atText: string; before: boolean; how: string };

/** How editing message `id` works: in place (the user's last message), in a branch (an earlier one
 *  of theirs), or not at all (not an ordinary chat, not the user's message, not there). */
export function editKind(chat: ChatLike | null | undefined, id: string): 'inplace' | 'branch' | null {
  if (!chat || chat.kind !== 'chat') return null;
  const i = chat.messages.findIndex((m) => m.id === id);
  if (i < 0 || chat.messages[i].role !== 'user') return null;
  return chat.messages.slice(i + 1).some((m) => m.role === 'user') ? 'branch' : 'inplace';
}

/** "Branch from here" is offered on the user's messages and the assistant's answers of an ordinary chat. */
export function canBranch(chat: ChatLike | null | undefined, m: Msg): boolean {
  return !!chat && chat.kind === 'chat' && (m.role === 'user' || m.role === 'assistant');
}

/** What an edit should say before it is sent: the message was already saved by a wrap (editing it
 *  does not change what was saved), and its answer changed files in the library (they stay). */
export function editNotes(chat: ChatLike, id: string): { saved: boolean; changes: boolean } {
  const i = chat.messages.findIndex((m) => m.id === id);
  if (i < 0) return { saved: false, changes: false };
  const m = chat.messages[i];
  const saved = !!chat.wrappedAt && !!m.ts && Date.parse(m.ts) <= Date.parse(chat.wrappedAt);
  const changes = chat.messages.slice(i + 1).some((x) => x.role === 'changes' && !x.tool?.undone && Number(x.tool?.total ?? x.tool?.files?.length ?? 0) > 0);
  return { saved, changes };
}

/** A file's name from the path the service keeps. */
export const fileName = (p: string) => p.split(/[\\/]/).pop() || p;

/** What a branch says about itself; null for a chat that is not one. */
export function branchInfo(meta: any): BranchInfo | null {
  const b = meta?.branch;
  if (!b || typeof b !== 'object' || typeof b.from !== 'string') return null;
  return {
    from: b.from, fromTitle: String(b.fromTitle ?? ''), at: String(b.at ?? ''), atRole: String(b.atRole ?? ''),
    atText: String(b.atText ?? ''), before: !!b.before, how: String(b.how ?? 'none'),
  };
}

/** The original of a branch as the chat list has it now: its current title, or gone. */
export function origin(info: BranchInfo, chats: Listed[]): { exists: boolean; title: string } {
  const o = chats.find((c) => c.id === info.from);
  return o ? { exists: true, title: o.title } : { exists: false, title: info.fromTitle };
}

/** The branches of a chat that still exist, newest first, with their current titles. */
export function branchesOf(meta: any, chats: Listed[]): Listed[] {
  const list: any[] = Array.isArray(meta?.branches) ? meta.branches : [];
  const out: Listed[] = [];
  const seen = new Set<string>();
  for (const b of [...list].reverse()) {
    const id = String(b?.id ?? '');
    const c = chats.find((x) => x.id === id);
    if (c && !seen.has(id)) { seen.add(id); out.push({ id, title: c.title }); }
  }
  return out;
}

/** The note shown once a branch is made: how its conversation reaches the assistant. */
export function madeKey(how: string, provider: string): string {
  if (how === 'fork') return 'chatfBranch.made.fork';
  if (how === 'summary') return 'chatfBranch.made.summary';
  if (how === 'none') return 'chatfBranch.made.none';
  return provider === 'codex' ? 'chatfBranch.made.replayCodex' : 'chatfBranch.made.replay';
}

// A message of the original to show once it opens (the branch's "from this message" link).
const jumps = new Map<string, string>();
export function jumpLater(chatId: string, msgId: string) { jumps.set(chatId, msgId); }
export function takeJump(chatId: string): string | undefined {
  const m = jumps.get(chatId);
  jumps.delete(chatId);
  return m;
}
