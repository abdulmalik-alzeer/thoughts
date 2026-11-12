// Quick capture: a message that starts with "todo: …", "new idea: …" or "new client: …" (or matter,
// engagement, class, course, project) is filed straight into the library by the assistant's
// routines. The routines look for the English words, so the Arabic forms («مهمة: …»,
// «فكرة جديدة: …», «عميل جديد: …») are rewritten to them before the message is sent.
const WORK_AR: [RegExp, string][] = [
  [/عميل/, 'client'], [/قضي[ةه]|ملف/, 'matter'], [/مشروع\s+استشاري/, 'engagement'], [/فصل|صف/, 'class'],
  [/مقرر/, 'course'], [/مشروع/, 'project'],
];
const TODO = /^\s*(?:مهم[ةه](?:\s+جديد[ةه])?|todo)\s*[:：]\s*/i;
const IDEA = /^\s*(?:فكر[ةه](?:\s+جديد[ةه])?|new\s+idea|idea)\s*[:：]\s*/i;
const WORK_EN = /^\s*new\s+(client|matter|engagement|class|course|project)\s*[:：]\s*/i;
const WORK_ARABIC = /^\s*(عميل|قضي[ةه]|ملف|مشروع\s+استشاري|مشروع|فصل|صف|مقرر)\s+جديد[ةه]?\s*[:：]\s*/;

/** 'todo' | 'idea' when the text starts with a capture prefix (English or Arabic), else null.
 *  A new work item ("new client: …") counts as 'idea': it is filed by the same routine. */
export function captureKind(text: string): 'todo' | 'idea' | null {
  if (TODO.test(text)) return 'todo';
  if (IDEA.test(text) || WORK_EN.test(text) || WORK_ARABIC.test(text)) return 'idea';
  return null;
}

/** The text with an Arabic (or loosely typed) capture prefix replaced by the canonical English one. */
export function normalizeCapture(text: string): string {
  if (TODO.test(text)) return 'todo: ' + text.replace(TODO, '');
  if (IDEA.test(text)) return 'new idea: ' + text.replace(IDEA, '');
  const en = WORK_EN.exec(text);
  if (en) return `new ${en[1].toLowerCase()}: ` + text.replace(WORK_EN, '');
  const ar = WORK_ARABIC.exec(text);
  if (ar) {
    const word = WORK_AR.find(([re]) => re.test(ar[1]))?.[1] ?? 'project';
    return `new ${word}: ` + text.replace(WORK_ARABIC, '');
  }
  return text;
}

/** What to put in a box when the todo chip is pressed with nothing typed, in the app language.
 *  (The new-item chip uses `newItemTrigger` from terms.ts, which follows the user's line of work.) */
export const capturePrefix = (kind: 'todo' | 'idea', lang: string) =>
  lang === 'ar' ? (kind === 'todo' ? 'مهمة: ' : 'فكرة جديدة: ') : kind === 'todo' ? 'todo: ' : 'new idea: ';
