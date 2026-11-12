// A conversation as Markdown, for saving or passing on. It holds what was said — the user's
// messages (with the names of attached and mentioned files), the answers, and failures — and
// leaves out the assistant's working steps (tool calls and their output) and the app's own notes.

type Tr = (k: string, vars?: Record<string, string | number>) => string;

const base = (p: string) => p.split(/[\\/]/).pop() || p;

export function chatMarkdown(chat: any, { t, date }: { t: Tr; date: (d: string) => string }): string {
  const out: string[] = [`# ${String(chat.title || t('chatc.export.untitled')).replace(/\n/g, ' ')}`, ''];
  if (chat.created) out.push(`*${t('chatc.export.started', { date: date(chat.created) })}*`, '');
  for (const m of chat.messages ?? []) {
    // Who speaks is in bold, and each turn starts after a line: the answers have headings of their own.
    if (m.role === 'user') {
      out.push('---', '', `**${t('chatc.export.you')}:**`, '', String(m.text ?? '').trim(), '');
      const files = (m.attachments ?? []).map(base);
      if (files.length) out.push(`*${t('chatc.export.attached', { files: files.join(', ') })}*`, '');
      const named = (m.tool?.mentions ?? []).map(String);
      if (named.length) out.push(`*${t('chatc.export.mentioned', { files: named.join(', ') })}*`, '');
    } else if (m.role === 'assistant') {
      out.push(`**${t('chatc.export.assistant')}:**`, '', String(m.text ?? '').trim(), '');
    } else if (m.role === 'error' && m.tool?.code !== 'sessionLost') {
      out.push(`> ${t('chatc.export.failed')}: ${String(m.text ?? '').trim().replace(/\n/g, '\n> ')}`, '');
    }
  }
  return out.join('\n').replace(/\n{3,}/g, '\n\n').trimEnd() + '\n';
}

/** A file name made from the chat's title: no characters a file system refuses, never empty. */
export function exportName(title: string): string {
  const s = String(title ?? '').replace(/[\\/:*?"<>|\u0000-\u001f]+/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 80).trim();
  return s.replace(/^\.+/, '') || 'chat';
}
