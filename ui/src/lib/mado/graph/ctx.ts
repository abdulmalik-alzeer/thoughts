// What the canvas shares with its nodes and edges (Svelte context): the state they draw and the actions they call.
import { getContext, setContext } from 'svelte';
import type { ErdField } from '../types';

export type Target = { kind: 'node' | 'edge'; key: string };
export interface GraphCtx {
  readonly editable: boolean;
  /** The node or edge whose name is being typed. */
  readonly editing: Target | null;
  /** The entity whose fields are open. */
  readonly fieldsFor: string | null;
  /** A look picked in the legend: everything else is dimmed. */
  readonly spot: string | null;
  readonly dir: 'ltr' | 'rtl';
  rename(t: Target): void;
  commit(t: Target, text: string): void;
  cancel(): void;
  remove(t: Target): void;
  restore(t: Target): void;
  comment(t: Target): void;
  fields(key: string | null): void;
  setFields(key: string, fields: ErdField[]): void;
}
const KEY = Symbol('mado-graph');
export const provideGraph = (c: GraphCtx) => setContext(KEY, c);
export const useGraph = () => getContext<GraphCtx>(KEY);
