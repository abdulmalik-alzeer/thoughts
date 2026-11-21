// The design step's calls to the daemon (/api/mado/:projectId/design/*). One object, so the
// dev-only demo and the tests can stand in for the daemon.
import { api, post, put } from '../../api';
import type { DesignTokens, ScreenInfo, StyleCandidate } from '../types';

export interface StylesAnswer { styles: StyleCandidate[]; picked?: string; dropped?: number }

export interface DesignClient {
  styles(projectId: string): Promise<StylesAnswer>;
  pick(projectId: string, id: string): Promise<{ picked: string; tokens: DesignTokens }>;
  tokens(projectId: string): Promise<DesignTokens | null>;
  saveTokens(projectId: string, tokens: DesignTokens): Promise<DesignTokens>;
  screens(projectId: string): Promise<ScreenInfo[]>;
  screen(projectId: string, id: string): Promise<{ html: string }>;
}

const base = (projectId: string) => `/api/mado/${encodeURIComponent(projectId)}/design`;

const daemon: DesignClient = {
  styles: (p) => api(`${base(p)}/styles`),
  pick: (p, id) => post(`${base(p)}/style`, { id }),
  tokens: (p) => api(`${base(p)}/tokens`),
  saveTokens: (p, tokens) => put(`${base(p)}/tokens`, tokens),
  screens: (p) => api(`${base(p)}/screens`),
  screen: (p, id) => api(`${base(p)}/screens/${encodeURIComponent(id)}`),
};

let current: DesignClient = daemon;
export const client = (): DesignClient => current;
/** Dev demo and tests only. Returns a function that puts the daemon back. */
export function useClient(c: DesignClient): () => void { current = c; return () => { current = daemon; }; }
