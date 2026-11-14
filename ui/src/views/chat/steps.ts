// Folding a chat's steps (commands run, files read and edited, searches…) into one quiet row, the
// way the Claude desktop app shows them. Only the assistant's own work is folded: its words,
// errors, approvals, refused steps and changes always stand on their own (they end a run of steps).
import { t, tn } from '../../lib/i18n';

/** What a step does, as far as the user is concerned. */
export type StepKind = 'command' | 'read' | 'edit' | 'search' | 'web' | 'agent' | 'plan' | 'other';
/** One step: the call and, once it came back, what it returned. */
export type Step = { call: any; result?: any; kind: StepKind };

const KIND: Record<string, StepKind> = {
  // Claude Code's tools
  Bash: 'command', BashOutput: 'command', KillShell: 'command', KillBash: 'command',
  Read: 'read', NotebookRead: 'read',
  Edit: 'edit', MultiEdit: 'edit', Write: 'edit', NotebookEdit: 'edit',
  Grep: 'search', Glob: 'search', LS: 'search', ToolSearch: 'search',
  WebFetch: 'web', WebSearch: 'web',
  Task: 'agent', Agent: 'agent',
  TodoWrite: 'plan', TodoRead: 'plan', ExitPlanMode: 'plan',
  // Codex's, as the service names them
  Command: 'command', Change: 'edit', Plan: 'plan',
};

export function stepKind(name: unknown): StepKind {
  return KIND[String(name ?? '')] ?? 'other';
}

/** Messages that sit inside a run of steps without ending it: a call's result (shown with its call)
 *  and the hidden note left by a failure the service recovered from by itself. */
const quiet = (m: any) => m.role === 'tool_result' || (m.role === 'error' && m.tool?.code === 'sessionLost');

/** The runs of consecutive steps in a chat, keyed by the id of each run's first call. */
export function groupSteps(messages: any[]): Map<string, Step[]> {
  const results: Record<string, any> = {};
  for (const m of messages) if (m.role === 'tool_result' && m.tool?.toolUseId) results[String(m.tool.toolUseId)] = m;
  const runs = new Map<string, Step[]>();
  let run: Step[] | null = null;
  for (const m of messages) {
    if (m.role === 'tool') {
      const step: Step = { call: m, result: results[String(m.tool?.id ?? m.id)], kind: stepKind(m.tool?.name) };
      if (run) run.push(step);
      else { run = [step]; runs.set(String(m.id), run); }
    } else if (!quiet(m)) run = null;
  }
  return runs;
}

/** The files a step touched (for "edited 2 files", counted once each). */
function files(s: Step): string[] {
  const i = s.call?.tool?.input ?? {};
  if (Array.isArray(i.files)) return i.files.map(String);
  const p = i.file_path ?? i.notebook_path ?? i.path;
  return typeof p === 'string' && p ? [p] : [];
}

const ORDER: StepKind[] = ['command', 'read', 'edit', 'search', 'web', 'agent', 'plan', 'other'];

/** "Ran 3 commands, edited 2 files" — what a run of steps did, in a few words. */
export function summarize(steps: Step[]): string {
  const parts: string[] = [];
  for (const kind of ORDER) {
    const these = steps.filter((s) => s.kind === kind);
    if (!these.length) continue;
    let n = these.length;
    if (kind === 'read' || kind === 'edit') {
      const named = new Set(these.flatMap(files));
      const unnamed = these.filter((s) => !files(s).length).length;
      n = named.size + unnamed || these.length;
    }
    parts.push(tn(`r10-chatview.did.${kind}`, n));
  }
  // Each part starts with a capital in English only where it starts the line.
  const line = parts.join(t('r10-chatview.sep'));
  return line ? line.charAt(0).toLocaleUpperCase() + line.slice(1) : '';
}

/** How many of the steps failed. */
export const failures = (steps: Step[]) => steps.filter((s) => s.result?.tool?.isError).length;

/** "Running a command…": the one line shown while the assistant is at work, when not every step is shown. */
export function liveLine(messages: any[], thinking: boolean): string {
  if (thinking) return t('r10-chatview.live.thinking');
  // The run of steps the chat ends with: the latest call still waiting for its result, if any.
  const back = new Set<string>();
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role === 'tool_result') { if (m.tool?.toolUseId) back.add(String(m.tool.toolUseId)); continue; }
    if (quiet(m)) continue;
    if (m.role !== 'tool') break;
    if (!back.has(String(m.tool?.id ?? m.id))) return t(`r10-chatview.live.${stepKind(m.tool?.name)}`);
  }
  return t('chat.working');
}
