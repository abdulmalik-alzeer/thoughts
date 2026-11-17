// The confirmation before one chat is deleted. A chat that was never wrapped loses its thinking for
// good (the assistant's own transcript goes with it), so the dialog says so, the same way the chat
// list does when several are picked, and offers to start a Wrap instead.
import { choiceDialog } from '../../lib/dialog.svelte';
import { app, command } from '../../lib/store.svelte';
import { t } from '../../lib/i18n';

/** Whether this chat still waits to be saved into the library (the list's flag when it has the chat,
 *  else the same rule as the daemon's `is_unwrapped`). */
export function chatUnwrapped(chat: any, id: string): boolean {
  const listed = app.chats.find((c: any) => c.id === id);
  if (listed && typeof listed.unwrapped === 'boolean') return listed.unwrapped;
  if (chat?.kind !== 'chat' || !(chat.messages ?? []).some((m: any) => m.role === 'user')) return false;
  return !chat.wrappedAt || Date.parse(chat.updated) > Date.parse(chat.wrappedAt);
}

/** True when the person confirmed the delete. Choosing "Wrap first" starts the Wrap and answers false. */
export async function confirmDeleteChat(chat: any, id: string): Promise<boolean> {
  const unsaved = chatUnwrapped(chat, id);
  const body = t('chata.deleteBody') + (unsaved ? '\n\n' + t('ux-small.del.unsaved') : '');
  const choices = [
    ...(unsaved ? [{ id: 'wrap', label: t('ux-small.del.wrapFirst'), primary: true }] : []),
    { id: 'delete', label: t('delete'), danger: true, primary: !unsaved },
  ];
  const pick = await choiceDialog({ title: t('chat.deleteTitle'), body, choices });
  if (pick === 'wrap') command('wrap.start');
  return pick === 'delete';
}
