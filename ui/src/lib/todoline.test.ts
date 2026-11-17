// Building a task line from its parts, and reading the "finished" group (todoline.ts).
import { describe, it, expect } from 'vitest';
import { compose, REPEATS, isDoneGroup, doneGroupOf, inSentence } from './todoline';

describe('compose', () => {
  it.each<[string, string | null | undefined, string | null | undefined, string]>([
    ['Pay rent', null, null, 'Pay rent'],
    ['  Pay rent  ', undefined, undefined, 'Pay rent'],
    ['Pay rent', 'month', null, 'Pay rent @every(month)'],
    ['Pay rent', null, '2026-11-30', 'Pay rent @due(2026-11-30)'],
    // The repeat first, so a reader that only knows @due still finds the date at the end.
    ['Pay rent', 'month', '2026-11-30', 'Pay rent @every(month) @due(2026-11-30)'],
    ['Pay rent', '', '', 'Pay rent'],
    ['', 'week', '2026-01-05', '@every(week) @due(2026-01-05)'],
    ['', null, null, ''],
    ['دفع الإيجار', '2weeks', '2026-12-01', 'دفع الإيجار @every(2weeks) @due(2026-12-01)'],
  ])('%j every=%j due=%j', (title, every, due, want) => {
    expect(compose(title, every, due)).toBe(want);
  });
});

describe('REPEATS', () => {
  it('matches the service list', () => {
    expect([...REPEATS]).toEqual(['day', 'week', '2weeks', 'month', 'month-end', 'quarter', 'year']);
  });
});

describe('isDoneGroup', () => {
  it.each<[string, boolean]>([
    ['Done', true], ['done', true], [' Completed ', true], ['Finished', true], ['Archive', true], ['Archived', true],
    ['منجز', true], ['المنجزة', true], ['المنجزات', true], ['تم', true], ['تمت', true], ['مكتملة', true], ['المكتملة', true],
    ['منتهي', true], ['المنتهية', true],
    ['Today', false], ['Done soon', false], ['Undone', false], ['', false], ['قيد التنفيذ', false],
  ])('%j → %s', (g, want) => {
    expect(isDoneGroup(g)).toBe(want);
  });
  it('copes with a missing group', () => {
    expect(isDoneGroup(undefined as unknown as string)).toBe(false);
    expect(isDoneGroup(null as unknown as string)).toBe(false);
  });
});

describe('doneGroupOf', () => {
  it('finds the first finished group', () => {
    expect(doneGroupOf({ groups: ['Now', 'Later', 'Done'] })).toBe('Done');
    expect(doneGroupOf({ groups: ['الآن', 'المنجزة'] })).toBe('المنجزة');
    expect(doneGroupOf({ groups: ['Now'] })).toBeNull();
    expect(doneGroupOf({})).toBeNull();
  });
});

describe('inSentence', () => {
  it.each([['Today', 'today'], ['Tomorrow', 'tomorrow'], ['Dec 31', 'Dec 31'], ['اليوم', 'اليوم']])('%s → %s', (a, b) => {
    expect(inSentence(a)).toBe(b);
  });
});
