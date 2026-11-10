// Quick capture: a message that starts with "todo: …" or "new idea: …" is filed straight into the
// library by the assistant's routines. The routines look for the English words, so the Arabic
// forms («مهمة: …», «فكرة جديدة: …») are rewritten to them before the message is sent.
const FORMS: [RegExp, string][] = [
  [/^\s*(?:مهم[ةه](?:\s+جديد[ةه])?|todo)\s*[:：]\s*/i, 'todo: '],
  [/^\s*(?:فكر[ةه](?:\s+جديد[ةه])?|new\s+idea|idea)\s*[:：]\s*/i, 'new idea: '],
];

/** 'todo' | 'idea' when the text starts with a capture prefix (English or Arabic), else null. */
export function captureKind(text: string): 'todo' | 'idea' | null {
  if (FORMS[0][0].test(text)) return 'todo';
  if (FORMS[1][0].test(text)) return 'idea';
  return null;
}

/** The text with an Arabic (or loosely typed) capture prefix replaced by the canonical English one. */
export function normalizeCapture(text: string): string {
  for (const [re, canon] of FORMS) if (re.test(text)) return canon + text.replace(re, '');
  return text;
}

/** What to put in a box when a capture chip is pressed with nothing typed, in the app language. */
export const capturePrefix = (kind: 'todo' | 'idea', lang: string) =>
  lang === 'ar' ? (kind === 'todo' ? 'مهمة: ' : 'فكرة جديدة: ') : kind === 'todo' ? 'todo: ' : 'new idea: ';
