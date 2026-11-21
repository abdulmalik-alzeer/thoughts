// MADO's own chats: the step chats (kind `mado`) and the build's runs (kind `mado-build`). They are
// opened from the Plan view, never listed with the user's chats or a project's chats (like the
// other kinds the app runs itself, they are not ordinary chats; unlike them they come in dozens).
export const PLAN_CHAT_KINDS: readonly string[] = ['mado', 'mado-build'];
export const isPlanChat = (c: { kind?: string } | null | undefined): boolean => !!c && PLAN_CHAT_KINDS.includes(String(c.kind ?? ''));
/** The chats to list anywhere outside the Plan view. */
export const listedChats = <T extends { kind?: string }>(chats: readonly T[]): T[] => chats.filter((c) => !isPlanChat(c));
