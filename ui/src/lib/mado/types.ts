// MADO: the shapes shared by the daemon and the window. The contract is docs/mado-spec.md.

export const STEPS = ['brief', 'spec', 'tech', 'design', 'plan', 'review', 'build'] as const;
export type StepId = (typeof STEPS)[number];
export type StepStatus = 'locked' | 'ready' | 'drafting' | 'review' | 'approved' | 'stale';
export interface StepState { status: StepStatus; chatId?: string; approvedAt?: string; /** paths the last run changed outside mado/; blocks approval until dismissed */ strayed?: string[] }
export interface MadoState { version: 1; steps: Record<StepId, StepState>; style?: string; plan_hash?: string }
export interface MadoOverview { project: { id: string; name: string; path: string }; exists: boolean; state: MadoState | null; files: string[] }

export type GraphKind = 'flow' | 'architecture' | 'erd' | 'tasks';
export type GraphName = 'flow' | 'architecture' | 'schema' | 'tasks';
export interface Pos { x: number; y: number }
export interface GraphNode { id: string; label: string; kind: string; props?: Record<string, unknown>; pos?: Pos }
export interface GraphEdge { id: string; from: string; to: string; label?: string; kind?: string }
export interface GraphDoc { version: 1; kind: GraphKind; title: string; nodes: GraphNode[]; edges: GraphEdge[] }
export interface ErdField { name: string; type: string; key?: 'pk' | 'fk'; note?: string }

// In add-edge.from/to, set-prop.id and add-node.near a node added in the same ops list may be named `new:<n>`
// (the n-th add-node of that list, 1-based).
export type GraphOp =
  | { op: 'rename'; id: string; label: string }
  | { op: 'add-node'; label: string; kind: string; near?: string }
  | { op: 'remove-node'; id: string }
  | { op: 'add-edge'; from: string; to: string; label?: string; kind?: string }
  | { op: 'remove-edge'; id: string }
  | { op: 'relabel-edge'; id: string; label: string }
  | { op: 'set-prop'; id: string; key: string; value: unknown };

export interface PlanTask { id: string; title: string; description: string; owns: string[]; dependsOn: string[]; acceptance: string[]; feature: string; screens?: string[] }
export interface PlanDoc { version: 1; tasks: PlanTask[]; /** run once in each fresh working copy before any task */ setup?: string[] }

export type CommentTarget =
  | { kind: 'text'; file: string; quote: string; prefix: string; suffix: string; heading?: string }
  | { kind: 'node'; file: string; nodeId: string }
  | { kind: 'edge'; file: string; edgeId: string }
  | { kind: 'token'; tokenPath: string }
  | { kind: 'screen'; screenId: string; x: number; y: number }
  | { kind: 'task'; taskId: string };
export type CommentStatus = 'open' | 'sent' | 'resolved' | 'orphaned';
export interface Comment { id: string; target: CommentTarget; body: string; createdAt: string; status: CommentStatus; runId?: string }

export interface DesignTokens {
  color: { bg: string; surface: string; text: string; muted: string; primary: string; onPrimary: string; accent: string; border: string; success: string; warning: string; danger: string };
  font: { heading: string; body: string; mono: string };
  radius: number;
  spacing: number;
  density: 'compact' | 'regular' | 'roomy';
  shadow: 'none' | 'soft' | 'strong';
}
export interface StyleCandidate { id: string; name: string; description: string; tokens: DesignTokens; source: 'app' | 'assistant' }
export interface ScreenInfo { id: string; label: string; hasMock: boolean }

export type ProblemKind = 'cycle' | 'missing-dep' | 'overlap' | 'untraced-feature' | 'untraced-screen' | 'unknown-feature' | 'empty-acceptance' | 'bad-path' | 'bad-command';
export interface Problem { kind: ProblemKind; message: string; tasks?: string[] }
export interface Lane { id: string; tasks: string[] }
export interface Wave { lanes: Lane[] }
export interface Review {
  features: { id: string; name: string; tasks: string[] }[];
  screens: { id: string; label: string; tasks: string[] }[];
  problems: Problem[];
  waves: Wave[];
  commands: string[];
  /** sha256 of the plan.json bytes this review was computed from; sent back when approving the review. */
  planHash: string;
}

export type BuildStatus = 'idle' | 'running' | 'paused' | 'waiting' | 'failed' | 'done' | 'stopped';
export type TaskStatus = 'pending' | 'running' | 'checking' | 'done' | 'failed' | 'conflict' | 'skipped';
/** `attempts`: tries started so far. */
export interface BuildTask { id: string; status: TaskStatus; chatId?: string; commit?: string; note?: string; attempts?: number }
/** `fixChatId`: the run that settled (or tried to settle) a clash when the lane was merged. */
export interface BuildLane { id: string; status: TaskStatus; tasks: BuildTask[]; fixChatId?: string }
/** `checked`: every task's checks passed again in the build's own copy after the lanes were merged;
 *  `fixChatId`: the run that fixed what those checks found. */
export interface BuildWave { status: TaskStatus; lanes: BuildLane[]; checked?: boolean; fixChatId?: string }
export interface BuildState {
  version: 1;
  status: BuildStatus;
  branch: string;
  base: string;
  startedAt?: string;
  waves: BuildWave[];
  governor: { maxParallel: number; running: number; holding: boolean; reason?: string };
  plan_hash: string;
  /** The build's number (`mado/build-<n>`). */
  number?: number;
  /** The commit of the user's branch the build started from. */
  baseCommit?: string;
  /** Why the build waits or failed, in words for the user. */
  message?: string;
  /** When the finished build was brought into the user's branch. */
  landedAt?: string;
}
/** `GET /build`: the build, plus (while none is under way) why a start would be refused now. */
export interface BuildView extends BuildState { blocked?: string }

export interface MadoEvent { type: 'mado'; projectId: string; what: 'state' | 'doc' | 'graph' | 'comments' | 'design' | 'build' }
