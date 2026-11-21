// MADO: the shapes shared by the daemon and the window. The contract is docs/mado-spec.md.

export const STEPS = ['brief', 'spec', 'tech', 'design', 'plan', 'review', 'build'] as const;
export type StepId = (typeof STEPS)[number];
export type StepStatus = 'locked' | 'ready' | 'drafting' | 'review' | 'approved' | 'stale';
export interface StepState { status: StepStatus; chatId?: string; approvedAt?: string; /** paths the last run changed outside mado/; blocks approval until dismissed */ strayed?: string[]; /** that check could not be made; blocks approval until dismissed too */ strayUnchecked?: boolean }
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

export type TaskKind = 'contracts' | 'tests' | 'feature';
export interface PlanTask { id: string; title: string; description: string; owns: string[]; dependsOn: string[]; acceptance: string[]; feature: string; screens?: string[]; kind?: TaskKind }
export interface PlanRun { start: string; url: string; screens: { id: string; path: string }[] }
export interface PlanDoc { version: 1; tasks: PlanTask[]; /** run once in each fresh working copy before any task */ setup?: string[]; /** run in the merged copy after every wave */ verify?: string[]; run?: PlanRun }

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

export type ProblemKind = 'cycle' | 'missing-dep' | 'overlap' | 'untraced-feature' | 'untraced-screen' | 'unknown-feature' | 'empty-acceptance' | 'bad-path' | 'bad-command' | 'no-repo' | 'no-branch' | 'uncommitted' | 'no-verify' | 'no-run' | 'no-contracts' | 'no-tests' | 'unchecked-screen';
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

export type BuildStatus = 'idle' | 'running' | 'paused' | 'waiting' | 'failed' | 'done' | 'stopped' | 'checking' | 'reviewing';
/** What one screen showed when the app looked at it (docs/mado-spec.md, round 0.4.0). Untrusted text. */
export interface ScreenResult { id: string; path: string; ok: boolean; status?: number; errors: string[]; text: string; fields: { label: string; value: string }[] }
/** `count`/`failed`: screens whose address the daemon asked itself, and those that answered with an error or that a
 *  looking window found broken; also set when skipped after the daemon asked (no window looked). `reason`: why skipped. */
export interface BuildScreens { status: 'waiting' | 'running' | 'done' | 'skipped'; reason?: string; count?: number; failed?: number }
/** `GET …/build/screens` while the build waits for a window to look (409 otherwise). */
export interface ScreenOffer { url: string; screens: { id: string; path: string }[]; nonce: string }
export interface ReviewFinding { severity: 'must' | 'should'; where?: string; text: string }
/** `round`: the review under way or last made, 1 to 3 (the first and at most two after fixes).
 *  `finished`: nothing more will be done. `note`: why the review or its fix did not go as planned (the daemon's words). */
export interface BuildReview { status: 'running' | 'passed' | 'changes' | 'failed'; round: number; chatId?: string; fixChatId?: string; findings: ReviewFinding[]; finished?: boolean; note?: string }
export interface MadoOptions { model: string | null; effort: string | null }
/** `step`: the first step not approved ('build' once all are); `build`: absent while idle; `updatedAt`: RFC 3339, UTC. */
export interface PlanEntry { projectId: string; name: string; step: string; stepStatus: string; build?: BuildStatus; updatedAt: string }
export interface BuildGuide { start?: string; url?: string; readme?: string }
export type TaskStatus = 'pending' | 'running' | 'checking' | 'done' | 'failed' | 'conflict' | 'skipped';
/** `attempts`: tries started so far. */
export interface BuildTask { id: string; status: TaskStatus; chatId?: string; commit?: string; note?: string; attempts?: number }
/** `fixChatId`: the run that settled (or tried to settle) a clash when the lane was merged. */
export interface BuildLane { id: string; status: TaskStatus; tasks: BuildTask[]; fixChatId?: string }
/** `checked`: every task's checks passed again in the build's own copy after the lanes were merged;
 *  `fixChatId`: the run that fixed what those checks found. */
export interface BuildWave { status: TaskStatus; lanes: BuildLane[]; checked?: boolean; fixChatId?: string; verified?: boolean; gateFixChatId?: string }
export interface BuildState {
  version: 1;
  status: BuildStatus;
  branch: string;
  base: string;
  startedAt?: string;
  waves: BuildWave[];
  governor: { maxParallel: number; running: number; holding: boolean; reason?: string };
  plan_hash: string;
  screens?: BuildScreens;
  review?: BuildReview;
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
/** `confined`: whether the build's own commands run confined (macOS only; false elsewhere, and while full access is on or the setting is off). */
export interface BuildView extends BuildState { blocked?: string; confined?: boolean }

export interface MadoEvent { type: 'mado'; projectId: string; what: 'state' | 'doc' | 'graph' | 'comments' | 'design' | 'build' }
