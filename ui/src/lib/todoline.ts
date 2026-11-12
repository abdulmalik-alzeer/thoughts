// Building a task line from its parts, the same way the background service writes it:
// `title @every(month) @due(2026-11-30)` — the repeat first, so a reader that only knows `@due`
// (the phone web app, older versions) still finds the date at the very end.

/** How often a task can repeat (the service's `todos::REPEATS`). */
export const REPEATS = ['day', 'week', '2weeks', 'month', 'month-end', 'quarter', 'year'] as const;
export type Repeat = (typeof REPEATS)[number];

export function compose(title: string, every: string | null | undefined, due: string | null | undefined): string {
  let s = title.trim();
  if (every) s += ` @every(${every})`;
  if (due) s += ` @due(${due})`;
  return s.trim();
}

/** A group that holds finished tasks, in the words the setup routine and people use for it. */
const DONE = /^(done|completed|finished|archived?|منجز[ةه]?|المنجز[ةه]?|المنجزات|تم|تمت|مكتمل[ةه]?|المكتملة|منته[يى]?[ةه]?|المنتهية)$/i;
export const isDoneGroup = (g: string) => DONE.test(String(g ?? '').trim());
/** The list's own group for finished tasks, if it has one. */
export const doneGroupOf = (l: { groups?: string[] }) => (l.groups ?? []).find(isDoneGroup) ?? null;

/** A due label ("Today", "Tomorrow", "Dec 31") as it reads inside a sentence: "… due tomorrow." */
export const inSentence = (label: string) => (/^(Today|Tomorrow)$/.test(label) ? label.toLowerCase() : label);
