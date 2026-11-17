// Suggest automations: the pure rules the Automate tab relies on (lib/automate.ts).
import { describe, it, expect } from 'vitest';
import {
  blockedKey, chatDraft, draftFrom, draftReady, draftUnsafe, emptyPerson, guideSteps, normGuide, normPeople,
  normState, personBody, personLine, personProblem, phaseOf, runsCommands, samePerson, saveBody, tidyDraft,
} from './automate';

const sug = (id: string, extra: Record<string, unknown> = {}) => ({ id, kind: 'skill', title: 'Email writing', what: 'Drafts emails.', why: '14 of 31', build: 'A routine', request: 'r', template: null, people: false, ...extra });

describe('normState', () => {
  it('is safe for nothing at all', () => {
    const s = normState(undefined);
    expect(s).toMatchObject({ running: false, error: null, generatedAt: null, suggestions: [], nothing: '', program: 'claude', canSuggest: false, blocked: null });
  });
  it('keeps known kinds in order and drops unknown, empty and repeated ones', () => {
    const s = normState({ suggestions: [sug('a'), sug('b', { kind: 'robot' }), sug('c', { title: ' ' }), sug('a'), sug('d', { kind: 'tab' }), null], canSuggest: true });
    expect(s.suggestions.map((x) => x.id)).toEqual(['a', 'd']);
    expect(s.canSuggest).toBe(true);
  });
  it('never lets Suggest be pressed while blocked or running', () => {
    expect(normState({ canSuggest: true, blocked: 'spend' }).canSuggest).toBe(false);
    expect(normState({ canSuggest: true, running: true }).canSuggest).toBe(false);
    expect(normState({ blocked: 'weird' }).blocked).toBe(null);
  });
  it('drops a date it cannot read and an empty error', () => {
    const s = normState({ generatedAt: 'not a date', error: '  ' });
    expect(s.generatedAt).toBe(null);
    expect(s.error).toBe(null);
  });
  it('takes template and people only in their own shapes', () => {
    const [x] = normState({ suggestions: [sug('a', { template: 'email-writing', people: 'yes' })] }).suggestions;
    expect(x.template).toBe('email-writing');
    expect(x.people).toBe(false);
  });
});

describe('phaseOf', () => {
  const base = normState({});
  it('shows the explanation first', () => expect(phaseOf(base, false)).toBe('before'));
  it('running wins over everything', () => expect(phaseOf({ ...base, running: true, error: 'x' }, true)).toBe('running'));
  it('Suggest again goes back to the explanation', () => expect(phaseOf({ ...base, suggestions: normState({ suggestions: [sug('a')] }).suggestions }, true)).toBe('before'));
  it('an error with nothing to show is the error state', () => expect(phaseOf({ ...base, error: 'Could not read' }, false)).toBe('error'));
  it('an error keeps the earlier suggestions on screen', () => expect(phaseOf({ ...base, error: 'x', suggestions: normState({ suggestions: [sug('a')] }).suggestions }, false)).toBe('result'));
  it('nothing yet, and all used up', () => {
    const at = '2026-11-17T10:00:00Z';
    expect(phaseOf({ ...base, generatedAt: at, nothing: 'Too early.' }, false)).toBe('nothing');
    expect(phaseOf({ ...base, generatedAt: at }, false)).toBe('done');
  });
});

describe('blockedKey', () => {
  it('names the reason', () => {
    expect(blockedKey(normState({ blocked: 'signedOut' }))).toBe('auto-ui.blocked.signedOut');
    expect(blockedKey(normState({ running: true }))).toBe('auto-ui.blocked.running');
    expect(blockedKey(normState({ canSuggest: true }))).toBe('');
  });
});

describe('drafts', () => {
  it('reads routine and helper drafts', () => {
    expect(draftFrom({ title: 'A', description: 'B', steps: 'C' }, 'skill')).toEqual({ title: 'A', description: 'B', body: 'C' });
    expect(draftFrom({ title: 'A', description: 'B', instructions: 'I' }, 'agent').body).toBe('I');
    expect(draftFrom(null, 'agent')).toEqual({ title: '', description: '', body: '' });
  });
  it('tidies what is saved and needs every part', () => {
    expect(tidyDraft({ title: ' Email \n writing ', description: 'when  I ask', body: '\n1. x\n' })).toEqual({ title: 'Email writing', description: 'when I ask', body: '1. x' });
    expect(draftReady({ title: 'a', description: ' ', body: 'b' })).toBe(false);
  });
  it('refuses lines that would run a program, as the service does', () => {
    expect(runsCommands('1. Say hello\n!ls')).toBe(true);
    expect(runsCommands('Run !`date`')).toBe(true);
    expect(runsCommands('![picture](a.png)\n!')).toBe(false);
    expect(draftUnsafe({ title: 'x', description: 'y', body: '  !rm' })).toBe(true);
  });
  it('builds the save body per kind', () => {
    const d = { title: ' T ', description: 'D', body: 'B ' };
    expect(saveBody(d, 'skill')).toEqual({ id: null, hash: null, routine: { title: 'T', description: 'D', steps: 'B' } });
    expect(saveBody(d, 'agent')).toEqual({ agent: { title: 'T', description: 'D', instructions: 'B' } });
  });
});

describe('guides', () => {
  it('shows steps as single plain lines, at most ten', () => {
    expect(guideSteps(['  Open\nMail ', '', 3, ...Array(20).fill('x')]).length).toBe(10);
    expect(guideSteps(['  Open\nMail '])).toEqual(['Open Mail']);
    expect(guideSteps('nope')).toEqual([]);
  });
  it('needs a title and something to do', () => {
    expect(normGuide({ title: 'Mail', steps: [] })).toBe(null);
    expect(normGuide({ title: 'Mail', steps: [], chatPrompt: 'Walk me through' })?.chatPrompt).toBe('Walk me through');
  });
  it('writes the chat draft in the chat view’s own shape', () => {
    expect(JSON.parse(chatDraft('Hello'))).toEqual({ text: 'Hello', mentions: [], files: [] });
  });
});

describe('people', () => {
  it('reads {people, private} and an older bare list', () => {
    const p = { slug: 'sara', name: 'Sara', email: 's@x.org', hash: 'h', extra: ['phone'] };
    expect(normPeople({ people: [p, { slug: '', name: 'x' }, { slug: 'n', name: '' }], private: true })).toMatchObject({ private: true, people: [{ slug: 'sara', extra: ['phone'], role: '' }] });
    expect(normPeople([p]).people.length).toBe(1);
    expect(normPeople(null)).toEqual({ people: [], private: false });
  });
  it('checks name and address before sending', () => {
    const p = emptyPerson();
    expect(personProblem(p)).toBe('auto-ui.ppl.needName');
    expect(personProblem({ ...p, name: 'Sara' })).toBe('');
    expect(personProblem({ ...p, name: 'Sara', email: 'sara@najd.sa' })).toBe('');
    expect(personProblem({ ...p, name: 'Sara', email: 'سارة@مثال.السعودية' })).toBe('');
    for (const bad of ['sara', 'sara@x', 'a b@x.org', 'a@x.org?cc=b', 'a@b@x.org']) expect(personProblem({ ...p, name: 'S', email: bad })).toBe('auto-ui.ppl.badEmail');
    expect(personProblem({ ...p, name: 'x'.repeat(121) })).toBe('auto-ui.ppl.longName');
  });
  it('sends one-line details and keeps notes and summary as written', () => {
    const b = personBody({ ...emptyPerson(), name: ' Sara \n Q ', role: 'Finance  manager', summary: 'mine', notes: 'Line 1\n\nLine 2\n\n' });
    expect(b).toMatchObject({ name: 'Sara Q', role: 'Finance manager', summary: 'mine', notes: 'Line 1\n\nLine 2' });
  });
  it('compares what would be saved', () => {
    expect(samePerson({ ...emptyPerson(), name: 'Sara ' }, { ...emptyPerson(), name: 'Sara' })).toBe(true);
    expect(samePerson({ ...emptyPerson(), name: 'Sara' }, { ...emptyPerson(), name: 'Sara', tone: 'formal' })).toBe(false);
  });
  it('joins role and organisation', () => {
    expect(personLine({ ...emptyPerson(), role: 'Manager', organisation: 'Najd' })).toBe('Manager, Najd');
    expect(personLine({ ...emptyPerson(), organisation: 'Najd' })).toBe('Najd');
  });
});
