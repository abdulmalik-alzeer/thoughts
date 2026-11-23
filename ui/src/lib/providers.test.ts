import { describe, expect, it } from 'vitest';
import { codexProviderKind, keylessAllowed, onHomeNetwork } from './providers';

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
  it('reads who answers Codex', () => {
    expect(codexProviderKind(undefined)).toBe('own');
    expect(codexProviderKind({ baseUrl: ' ' })).toBe('own');
    expect(codexProviderKind({ baseUrl: 'https://x/v1', hasApiToken: true })).toBe('service');
    expect(codexProviderKind({ baseUrl: 'http://localhost:11434/v1', keyless: true })).toBe('service');
    expect(codexProviderKind({ baseUrl: 'https://x/v1' })).toBe('incomplete');
  });
});
