import { describe, expect, it } from 'vitest';
import { CUSTOM, destination, formState, keylessAllowed, onHomeNetwork, programsOf, sameAddress, savedAs, savedKind, startAddress, startProgram, type Service } from './providers';
import strings from './i18n/providers72';

describe('other providers', () => {
  it('allows no key only on this computer, or the home network with full access', () => {
    expect(keylessAllowed('http://localhost:11434', false)).toBe(true);
    expect(keylessAllowed('http://127.0.0.1:4000/v1', false)).toBe(true);
    expect(keylessAllowed('https://api.deepseek.com/anthropic', true)).toBe(false);
    expect(keylessAllowed('http://192.168.1.5:11434', false)).toBe(false);
    expect(keylessAllowed('http://192.168.1.5:11434', true)).toBe(true);
    expect(keylessAllowed('http://user@localhost:1', false)).toBe(false);
    expect(keylessAllowed('http://172.32.0.1', true)).toBe(false);
    expect(keylessAllowed('http://localhost.evil.com', false)).toBe(false);
  });
  it('notes a plain address on the home network only while full access is on', () => {
    expect(onHomeNetwork('http://10.0.0.2/v1', true)).toBe(true);
    expect(onHomeNetwork('http://10.0.0.2/v1', false)).toBe(false);
    expect(onHomeNetwork('https://10.0.0.2/v1', true)).toBe(false);
  });
});

// What `GET /ai/services` sends (`providers::SERVICES`): the page's whole list.
const SERVICES: Service[] = [
  { id: 'openai', claude: null, codex: 'https://api.openai.com/v1', noKey: false, keysUrl: 'https://platform.openai.com/api-keys' },
  { id: 'anthropic', claude: '', codex: null, noKey: false, keysUrl: 'https://console.anthropic.com/settings/keys' },
  { id: 'openrouter', claude: 'https://openrouter.ai/api', codex: 'https://openrouter.ai/api/v1', noKey: false, keysUrl: 'https://openrouter.ai/keys' },
  { id: 'deepseek', claude: 'https://api.deepseek.com/anthropic', codex: 'https://api.deepseek.com', noKey: false, keysUrl: 'https://platform.deepseek.com/api_keys' },
  { id: 'ollama', claude: 'http://localhost:11434', codex: 'http://localhost:11434/v1', noKey: true, keysUrl: '' },
];
const both = { claude: true, codex: true };

describe('use another service: what the page shows', () => {
  it('says which assistant each service works with', () => {
    expect(SERVICES.map((s) => programsOf(s).join('+'))).toEqual(['codex', 'claude', 'codex+claude', 'codex+claude', 'codex+claude']);
  });

  it('reads a saved set-up as one of its entries', () => {
    expect(savedAs('codex', undefined, SERVICES)).toBe('');
    expect(savedAs('codex', { baseUrl: ' ' }, SERVICES)).toBe('');
    expect(savedAs('codex', { baseUrl: 'https://api.openai.com/v1', hasApiToken: true }, SERVICES)).toBe('openai');
    // Typed by hand with another case or a slash at the end: still the same service.
    expect(savedAs('codex', { baseUrl: 'https://OpenRouter.ai/api/v1/', hasApiToken: true }, SERVICES)).toBe('openrouter');
    expect(savedAs('claude', { baseUrl: 'https://openrouter.ai/api', hasApiToken: true }, SERVICES)).toBe('openrouter');
    // An address belongs to the program it was saved for: Codex's OpenRouter address is not Claude Code's.
    expect(savedAs('claude', { baseUrl: 'https://openrouter.ai/api/v1', hasApiToken: true }, SERVICES)).toBe(CUSTOM);
    expect(savedAs('codex', { baseUrl: 'http://localhost:11434/v1', keyless: true }, SERVICES)).toBe('ollama');
    // Claude Code: a key with no address (or Anthropic's own) is Anthropic; no key is the Claude account.
    expect(savedAs('claude', { hasApiToken: true }, SERVICES)).toBe('anthropic');
    expect(savedAs('claude', { baseUrl: 'https://api.anthropic.com', hasApiToken: true }, SERVICES)).toBe('anthropic');
    expect(savedAs('claude', {}, SERVICES)).toBe('');
    // Codex has no such spelling: a key flag without an address is its own sign-in.
    expect(savedAs('codex', { hasApiToken: true }, SERVICES)).toBe('');
  });

  it('keeps a service saved from a preset that is gone, as the generic entry with its own address', () => {
    for (const u of ['https://api.moonshot.ai/anthropic', 'https://api.z.ai/api/anthropic', 'https://api.minimax.io/anthropic', 'http://localhost:4000']) {
      const saved = { baseUrl: u, hasApiToken: true };
      expect(savedAs('claude', saved, SERVICES)).toBe(CUSTOM);
      expect(savedKind('claude', saved)).toBe('service');
      // Opened for editing, the form holds the saved address and is not edited: the saved key stays.
      const url = startAddress(CUSTOM, 'claude', SERVICES, saved);
      expect(url).toBe(u);
      const st = formState('claude', { url, token: '', noKey: false }, saved, false);
      expect([st.dirty, st.needKey, st.canSave, st.canTest]).toEqual([false, false, false, true]);
    }
    const gateway = { baseUrl: 'http://localhost:4000/v1', hasApiToken: true };
    expect(savedAs('codex', gateway, SERVICES)).toBe(CUSTOM);
    expect(startProgram(CUSTOM, SERVICES, { claude: undefined, codex: gateway }, 'claude', both)).toBe('codex');
    // The list could not be fetched: everything saved is still shown, as the generic entry.
    expect(savedAs('codex', { baseUrl: 'https://api.openai.com/v1', hasApiToken: true }, [])).toBe(CUSTOM);
  });

  it('reads whether a saved service can run', () => {
    expect(savedKind('codex', undefined)).toBe('own');
    expect(savedKind('codex', { baseUrl: 'https://x/v1', hasApiToken: true })).toBe('service');
    expect(savedKind('codex', { baseUrl: 'http://localhost:11434/v1', keyless: true })).toBe('service');
    expect(savedKind('codex', { baseUrl: 'https://x/v1' })).toBe('incomplete');
    expect(savedKind('claude', { hasApiToken: true })).toBe('service');
    expect(savedKind('claude', {})).toBe('own');
    expect(savedKind('claude', { baseUrl: 'https://x/anthropic' })).toBe('incomplete');
  });

  it('opens an entry for the assistant it can only work with, else the one in use, else the chosen one', () => {
    const none = { claude: undefined, codex: undefined };
    expect(startProgram('openai', SERVICES, none, 'claude', both)).toBe('codex');
    expect(startProgram('anthropic', SERVICES, none, 'codex', both)).toBe('claude');
    expect(startProgram('openrouter', SERVICES, none, 'claude', both)).toBe('claude');
    expect(startProgram('openrouter', SERVICES, none, 'codex', both)).toBe('codex');
    // The chosen assistant is not installed: the installed one.
    expect(startProgram('ollama', SERVICES, none, 'codex', { claude: true, codex: false })).toBe('claude');
    // Already connected for Codex: "OpenRouter" opens on that, whatever the chosen assistant.
    expect(startProgram('openrouter', SERVICES, { claude: undefined, codex: { baseUrl: 'https://openrouter.ai/api/v1', hasApiToken: true } }, 'claude', both)).toBe('codex');
    // The generic entry is the OpenAI-compatible one.
    expect(startProgram(CUSTOM, SERVICES, none, 'claude', both)).toBe('codex');
  });

  it('fills in the address of a ready-made service, and keeps the saved spelling of one in use', () => {
    expect(startAddress('openrouter', 'codex', SERVICES, undefined)).toBe('https://openrouter.ai/api/v1');
    expect(startAddress('openrouter', 'claude', SERVICES, undefined)).toBe('https://openrouter.ai/api');
    expect(startAddress('anthropic', 'claude', SERVICES, undefined)).toBe('');
    expect(startAddress(CUSTOM, 'codex', SERVICES, undefined)).toBe('');
    expect(startAddress('openrouter', 'codex', SERVICES, { baseUrl: 'https://openrouter.ai/api/v1/', hasApiToken: true })).toBe('https://openrouter.ai/api/v1/');
    expect(sameAddress('https://A.example/v1/', 'https://a.example/v1')).toBe(true);
    expect(destination('claude', 'https://api.anthropic.com/')).toBe('');
    expect(destination('codex', 'https://api.anthropic.com/')).toBe('https://api.anthropic.com/');
  });
});

describe('use another service: what the form may save', () => {
  const f = (url: string, token = '', noKey = false) => ({ url, token, noKey });

  it('a ready-made service needs only its key', () => {
    const blank = formState('codex', f('https://api.openai.com/v1'), undefined, false);
    expect([blank.canSave, blank.needKey, blank.canTest]).toEqual([false, true, false]);
    const st = formState('codex', f('https://api.openai.com/v1', ' sk-1 '), undefined, false);
    expect(st.canSave).toBe(true);
    expect(st.body).toEqual({ baseUrl: 'https://api.openai.com/v1', token: 'sk-1', noKey: false });
  });

  it('Anthropic is a key with no address, for Claude Code only', () => {
    expect(formState('claude', f(''), {}, false).canSave).toBe(false);
    const st = formState('claude', f('', 'sk-ant'), {}, false);
    expect([st.canSave, st.body.baseUrl, st.body.token]).toEqual([true, null, 'sk-ant']);
    // Saved: nothing to save again, and it can be tested.
    const again = formState('claude', f(''), { hasApiToken: true }, false);
    expect([again.dirty, again.canSave, again.canTest]).toEqual([false, false, true]);
    // Codex never saves a key without an address.
    expect(formState('codex', f('', 'sk-1'), undefined, false).canSave).toBe(false);
  });

  it('a service on this computer can be saved without a key, and no other can', () => {
    const st = formState('codex', f('http://localhost:11434/v1', '', true), undefined, false);
    expect([st.canSave, st.noKey, st.body.noKey, st.body.token]).toEqual([true, true, true, null]);
    const remote = formState('codex', f('https://api.deepseek.com', '', true), undefined, false);
    expect([remote.noKey, remote.needKey, remote.canSave]).toEqual([false, true, false]);
    // A typed key wins over the box.
    expect(formState('codex', f('http://localhost:4000/v1', 'k', true), undefined, false).body).toEqual({ baseUrl: 'http://localhost:4000/v1', token: 'k', noKey: false });
    // Saved keyless and untouched: not edited, and it can be tested.
    const kept = formState('claude', f('http://localhost:11434', '', true), { baseUrl: 'http://localhost:11434', keyless: true }, false);
    expect([kept.dirty, kept.canTest]).toEqual([false, true]);
    // The home network counts only while full access is on.
    expect(formState('codex', f('http://192.168.1.5:11434/v1', '', true), undefined, false).noKey).toBe(false);
    expect(formState('codex', f('http://192.168.1.5:11434/v1', '', true), undefined, true).noKey).toBe(true);
  });

  it('a saved key is never reused for another address', () => {
    const saved = { baseUrl: 'https://a.example/v1', hasApiToken: true };
    const moved = formState('codex', f('https://b.example/v1'), saved, false);
    expect([moved.dirty, moved.needKey, moved.keyMoved, moved.canSave]).toEqual([true, true, true, false]);
    expect(formState('codex', f('https://b.example/v1', 'k2'), saved, false).canSave).toBe(true);
    // Replacing a service with a ready-made one asks for that one's key too.
    expect(formState('claude', f('https://openrouter.ai/api'), { baseUrl: 'https://api.minimax.io/anthropic', hasApiToken: true }, false).needKey).toBe(true);
    // A new key for the same address is a change; the test waits for the save.
    const rekey = formState('codex', f('https://a.example/v1', 'k3'), saved, false);
    expect([rekey.canSave, rekey.canTest]).toEqual([true, false]);
  });

  it('an address that is not a web address is said, and nothing is saved', () => {
    const st = formState('codex', f('api.example.com/v1', 'k'), undefined, false);
    expect([st.badUrl, st.canSave]).toEqual([true, false]);
  });
});

describe('use another service: the words', () => {
  const en = strings.en as Record<string, string>, ar = strings.ar as Record<string, string>;
  it('every service and both assistants have a name, a line about them and help in both languages', () => {
    for (const id of [...SERVICES.map((s) => s.id), CUSTOM]) {
      for (const k of [`prov72.svc.${id}`, `prov72.about.${id}`]) { expect(en[k], k).toBeTruthy(); expect(ar[k], k).toBeTruthy(); }
    }
    for (const k of ['works.codex', 'works.claude', 'works.both', 'addressHelp.codex', 'addressHelp.claude', 'format.codex', 'format.claude', 'state.key', 'state.noKey', 'state.missing']) expect(en['prov72.' + k], k).toBeTruthy();
  });
  it('a sentence names the same things in both languages', () => {
    const holes = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort().join();
    for (const k of Object.keys(en)) expect(holes(ar[k] ?? ''), k).toBe(holes(en[k]));
  });
  it('the page keeps its words plain: what a term means is behind a "?"', () => {
    const onPage = Object.entries(en).filter(([k]) => !/help/i.test(k)).map(([, v]) => v).join(' ');
    for (const jargon of ['API', 'endpoint', 'base URL', 'wire', 'gateway', 'LiteLLM', 'token', 'provider']) expect(onPage, jargon).not.toContain(jargon);
  });
});
