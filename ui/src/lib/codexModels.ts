// What the screen says about Codex's models, from `GET /ai/codex/options` (the daemon's
// `codex_models`): the list comes from the installed Codex, never from the app.

export interface CodexModelOpts {
  efforts?: string[];
  modelInfo?: { id: string; efforts?: string[] }[];
  modelsSource?: string;
  modelsDetected?: boolean;
  modelsTooOld?: boolean;
  modelsRefreshing?: boolean;
  codexVersion?: string;
}

/** The version's number as `codex --version` prints it (`codex-cli 0.27.0` → `0.27.0`). */
export function versionNumber(v: string | null | undefined): string {
  return String(v ?? '').trim().split(/\s+/).pop() ?? '';
}

/** The efforts `model` takes where Codex's catalog says, else the program's own list. */
export function effortsFor(o: CodexModelOpts, model: string | null | undefined): string[] {
  const own = model ? o.modelInfo?.find((m) => m.id === model)?.efforts : undefined;
  return own?.length ? own : (o.efforts ?? []);
}

/** The key of the plain note shown when Codex could not list its models (none: '' — listed, still
 *  being asked, or the options not in yet). An old Codex is told an update will list them. */
export function modelsNoteKey(o: CodexModelOpts, looking = false): '' | 'codex.models.tooOld' | 'codex.models.notListed' {
  if (o.modelsDetected || looking || o.modelsRefreshing || !o.modelsSource) return '';
  return o.modelsTooOld ? 'codex.models.tooOld' : 'codex.models.notListed';
}
