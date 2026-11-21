// MADO: what the Plan view knows about one project, and the only place that talks to the service
// for it. The view reads; every change goes through a method here and is fetched back (the service
// also says so over the events socket, and `onEvent` fetches what the event names: never its payload).
import { ApiError } from '../api';
import { madoApi, type DocName, type MadoApi } from './api';
import { changeRequest, GRAPH_FILES, STEP_DOCS, STEP_GRAPHS, type PendingOps } from './comments/batch';
import type {
  BuildGuide, BuildView, Comment, CommentStatus, CommentTarget, GraphDoc, GraphName, GraphOp, MadoEvent, MadoOverview, PlanDoc,
  Pos, Review, StepId, StepState, StepStatus,
} from './types';
import { STEPS } from './types';

/** Kinds of problem that make a plan impossible to build: Approve stays off while one exists.
 *  The same list as the service's (`mado_review::blocks`), which refuses the approval anyway. */
export const BLOCKING: readonly string[] = ['cycle', 'missing-dep', 'bad-path', 'bad-command', 'empty-acceptance', 'unknown-feature', 'no-repo', 'no-branch'];

// ---------- the rules of the steps (plain functions, tested on their own) ----------

export const statusOf = (o: MadoOverview | null, step: StepId): StepStatus => o?.state?.steps?.[step]?.status ?? 'locked';
/** Steps the assistant writes. Review is worked out by the app; Build is the building itself. */
export const isDrafted = (step: StepId) => step !== 'review' && step !== 'build';
export const canRun = (step: StepId, s: StepStatus) => isDrafted(step) && (s === 'ready' || s === 'review' || s === 'stale');
export const canReopen = (step: StepId, s: StepStatus) => step !== 'build' && s === 'approved';
/** Comments, edits and diagram changes are possible only while the step is with the user. */
export const isEditable = (s: StepStatus) => s === 'ready' || s === 'review' || s === 'stale';
export const blockingProblems = (r: Review | null) => (r?.problems ?? []).filter((p) => BLOCKING.includes(p.kind));

/** May this step be approved now? The review step also needs a plan that can be built; no step
 *  while its last run changed files outside the plan folder that the user has not looked at (`strayed`),
 *  or that check could not be made (`unchecked`). */
export function canApprove(step: StepId, s: StepStatus, review: Review | null = null, strayed: string[] | undefined = undefined, unchecked = false): boolean {
  if (step === 'build') return false;
  if (strayed?.length || unchecked) return false;
  if (step === 'review') return (s === 'ready' || s === 'review' || s === 'stale') && !!review && !!review.planHash && blockingProblems(review).length === 0;
  return s === 'review' || s === 'stale';
}

/** The step to open first: the earliest one that still needs the user. */
export function firstOpenStep(o: MadoOverview | null): StepId {
  for (const s of STEPS) { const st = statusOf(o, s); if (st !== 'approved' && st !== 'locked') return s; }
  let last: StepId = 'brief';
  for (const s of STEPS) if (statusOf(o, s) === 'approved') last = s;
  return last;
}

const gone = (e: unknown) => e instanceof ApiError && e.status === 404;
/** The service refused an approval because plan.json is not the plan that was shown. */
export const planChanged = (e: unknown) => e instanceof ApiError && (e.status === 409 || e.status === 412 || /^(PLAN_CHANGED|plan_changed|CHANGED|CONFLICT)$/.test(e.code));

export class MadoStore {
  readonly projectId: string;
  readonly api: MadoApi;

  overview = $state<MadoOverview | null>(null);
  /** 'loading' until the first answer; 'error' with `error` when the project could not be read. */
  phase = $state<'loading' | 'ready' | 'error'>('loading');
  error = $state('');
  /** undefined: not fetched yet · null: the file is not there yet. */
  docs = $state<Partial<Record<DocName, string | null>>>({});
  graphs = $state<Partial<Record<GraphName, GraphDoc | null>>>({});
  plan = $state<PlanDoc | null | undefined>(undefined);
  review = $state<Review | null | undefined>(undefined);
  build = $state<BuildView | null | undefined>(undefined);
  /** `mado/conventions.md` (round 0.4.0; older plans have none). */
  conventions = $state<string | null | undefined>(undefined);
  /** How to run what was built (`GET /build/guide`), fetched once a build is landed. */
  guide = $state<BuildGuide | null | undefined>(undefined);
  comments = $state<Comment[]>([]);
  /** Diagram edits the user made and has not sent, per diagram file. Kept across tabs and steps. */
  pendingOps = $state<PendingOps>({});
  /** A document the user is editing by hand and has not saved: its text so far. */
  edits = $state<Partial<Record<DocName, string>>>({});
  /** Goes up whenever the service says the design changed (DesignStep fetches again). */
  designVersion = $state(0);

  private tickets: Record<string, number> = {};

  constructor(projectId: string, api: MadoApi = madoApi(projectId)) {
    this.projectId = projectId;
    this.api = api;
  }

  get exists(): boolean { return !!this.overview?.exists; }
  status(step: StepId): StepStatus { return statusOf(this.overview, step); }
  step(step: StepId): StepState { return this.overview?.state?.steps?.[step] ?? { status: 'locked' }; }
  has(file: string): boolean { return !!this.overview?.files?.includes(file); }

  /** Run one fetch; of two fetches of the same thing, only the one started last is used. */
  private async latest<T>(key: string, get: () => Promise<T>, use: (v: T | null) => void): Promise<void> {
    const ticket = (this.tickets[key] = (this.tickets[key] ?? 0) + 1);
    let v: T | null;
    try { v = await get(); }
    catch (e) { if (!gone(e)) throw e; v = null; }
    if (this.tickets[key] === ticket) use(v);
  }

  /** `first`: called once the plan is known, in the same tick it is first shown (to pick the step to open). */
  async load(first?: () => void): Promise<void> {
    try {
      await this.refreshOverview();
      if (this.exists) await this.refreshComments().catch(() => {});
      this.error = '';
      if (this.phase !== 'ready') first?.();
      this.phase = 'ready';
    } catch (e) {
      if (this.overview) throw e; // keep what is on screen; the caller says the refresh failed
      this.error = e instanceof Error ? e.message : String(e);
      this.phase = 'error';
    }
  }

  /** Unlike a document, the overview is never "not there yet": a project the service does not know is an error. */
  async refreshOverview(): Promise<void> {
    const ticket = (this.tickets.overview = (this.tickets.overview ?? 0) + 1);
    const v = await this.api.overview();
    if (this.tickets.overview === ticket && v) this.overview = v;
  }
  refreshComments() { return this.latest('comments', () => this.api.comments(), (v) => { this.comments = v ?? []; }); }
  refreshDoc(name: DocName) { return this.latest('doc:' + name, () => this.api.doc(name), (v) => { this.docs[name] = v ? String(v.text ?? '') : null; }); }
  refreshGraph(name: GraphName) { return this.latest('graph:' + name, () => this.api.graph(name), (v) => { this.graphs[name] = v; }); }
  refreshPlan() { return this.latest('plan', () => this.api.plan(), (v) => { this.plan = v; }); }
  refreshReview() { return this.latest('review', () => this.api.review(), (v) => { this.review = v; }); }
  refreshBuild() { return this.latest('build', () => this.api.build(), (v) => { this.build = v; }); }
  refreshConventions() { return this.latest('conventions', () => this.api.conventions(), (v) => { this.conventions = v ? String(v.text ?? '') : null; }); }
  /** A service from before 0.4.0 has no guide: then there is simply none (null). */
  refreshGuide() {
    return this.latest('guide', () => this.api.buildGuide(), (v) => {
      const s = (x: unknown, max: number) => (typeof x === 'string' && x.trim() ? x.slice(0, max) : undefined);
      this.guide = v && typeof v === 'object' ? { start: s(v.start, 2000), url: s(v.url, 200), readme: s(v.readme, 8000) } : null;
    }).catch(() => { this.guide = null; });
  }

  /** Fetch what a step shows. `force`: again, even if it is already here. */
  async ensureStep(step: StepId, force = false): Promise<void> {
    if (!this.exists) return;
    const jobs: Promise<void>[] = [];
    for (const d of STEP_DOCS[step]) if (force || this.docs[d] === undefined) jobs.push(this.refreshDoc(d));
    for (const g of STEP_GRAPHS[step]) if (force || this.graphs[g] === undefined) jobs.push(this.refreshGraph(g));
    if (step === 'tech' && this.has('conventions.md') && (force || this.conventions === undefined)) jobs.push(this.refreshConventions().catch(() => { this.conventions = null; }));
    if (step === 'plan' && (force || this.plan === undefined)) jobs.push(this.refreshPlan());
    if (step === 'review') { jobs.push(this.refreshReview()); if (force || this.plan === undefined) jobs.push(this.refreshPlan()); }
    if (step === 'build') { jobs.push(this.refreshBuild()); if (force || this.plan === undefined) jobs.push(this.refreshPlan()); }
    await Promise.all(jobs);
  }

  /** The service said something changed (or the connection came back): fetch what it names. */
  async onEvent(e: Pick<MadoEvent, 'projectId' | 'what'> | { type: 'resync' }): Promise<void> {
    if ('type' in e && e.type === 'resync') return this.refreshAll();
    const ev = e as MadoEvent;
    if (ev.projectId !== this.projectId) return;
    const jobs: Promise<void>[] = [];
    const loadedDocs = () => (Object.keys(this.docs) as DocName[]).map((d) => this.refreshDoc(d));
    const loadedGraphs = () => (Object.keys(this.graphs) as GraphName[]).map((g) => this.refreshGraph(g));
    const derived = () => {
      if (this.plan !== undefined) jobs.push(this.refreshPlan());
      if (this.review !== undefined) jobs.push(this.refreshReview());
    };
    switch (ev.what) {
      case 'state': jobs.push(this.refreshOverview()); derived(); if (this.build !== undefined) jobs.push(this.refreshBuild()); break;
      case 'doc': jobs.push(this.refreshOverview(), ...loadedDocs()); if (this.conventions !== undefined) jobs.push(this.refreshConventions().catch(() => {})); derived(); break;
      case 'graph': jobs.push(this.refreshOverview(), ...loadedGraphs()); derived(); break;
      case 'comments': jobs.push(this.refreshComments()); break;
      case 'design': this.designVersion++; jobs.push(this.refreshOverview()); if (this.docs.design !== undefined) jobs.push(this.refreshDoc('design')); break;
      case 'build': jobs.push(this.refreshBuild()); if (this.guide !== undefined) jobs.push(this.refreshGuide()); break;
      default: return;
    }
    await Promise.all(jobs);
  }

  /** Everything that is on screen, again (after the connection dropped). */
  async refreshAll(): Promise<void> {
    await this.refreshOverview();
    if (!this.exists) return;
    this.designVersion++;
    const jobs: Promise<void>[] = [this.refreshComments()];
    for (const d of Object.keys(this.docs) as DocName[]) jobs.push(this.refreshDoc(d));
    for (const g of Object.keys(this.graphs) as GraphName[]) jobs.push(this.refreshGraph(g));
    if (this.plan !== undefined) jobs.push(this.refreshPlan());
    if (this.review !== undefined) jobs.push(this.refreshReview());
    if (this.build !== undefined) jobs.push(this.refreshBuild());
    if (this.conventions !== undefined) jobs.push(this.refreshConventions().catch(() => {}));
    if (this.guide !== undefined) jobs.push(this.refreshGuide());
    await Promise.all(jobs);
  }

  // ---------- what the user does ----------

  async start(): Promise<void> { await this.api.start(); await this.load(); }

  private setStep(step: StepId, change: Partial<StepState>) {
    const steps = this.overview?.state?.steps;
    if (steps?.[step]) steps[step] = { ...steps[step], ...change };
  }

  /** Draft (or draft again) a step. Shown as "being written" at once; the service confirms. */
  async run(step: StepId, note = ''): Promise<string> {
    const r = await this.api.run(step, note);
    this.setStep(step, { status: 'drafting', ...(r?.chatId ? { chatId: r.chatId } : {}) });
    this.refreshOverview().catch(() => {});
    return r?.chatId ?? '';
  }
  async approve(step: StepId): Promise<void> {
    if (step === 'review') throw new Error('MADO: the review is approved with approveReview');
    await this.api.approve(step);
    await this.refreshOverview();
  }

  /** The review as the service has it NOW, for showing right before it is approved. */
  async freshReview(): Promise<Review | null> {
    const ticket = (this.tickets.review = (this.tickets.review ?? 0) + 1);
    const r = await this.api.review();
    if (this.tickets.review === ticket) this.review = r;
    return r ?? null;
  }

  /** Approve the review the user was just shown. Only that one: its plan hash goes along, a review
   *  with problems that block the build is never sent, and when the service says the plan is no
   *  longer the one shown, the new review is fetched and 'changed' comes back (nothing approved). */
  async approveReview(shown: Review, approval?: string): Promise<'approved' | 'changed' | 'blocked'> {
    if (!shown?.planHash || blockingProblems(shown).length) return 'blocked';
    try { await this.api.approve('review', shown.planHash, approval); }
    catch (e) {
      if (!planChanged(e)) throw e;
      await this.freshReview().catch(() => {});
      this.refreshPlan().catch(() => {});
      return 'changed';
    }
    await this.refreshOverview();
    return 'approved';
  }
  async reopen(step: StepId): Promise<void> { await this.api.reopen(step); await this.refreshOverview(); }
  /** The user looked at the files the step's last run changed outside the plan folder. */
  async dismissStrayed(step: StepId): Promise<void> { await this.api.dismissStrayed(step); await this.refreshOverview(); }

  /** The user's own edit of a document. */
  async saveDoc(name: DocName, text: string): Promise<void> {
    await this.api.saveDoc(name, text);
    this.docs[name] = text;
    this.refreshOverview().catch(() => {});
    this.refreshComments().catch(() => {});
  }

  async addComment(target: CommentTarget, body: string): Promise<Comment | null> {
    const made = await this.api.addComment(target, body.trim());
    if (made?.id && !this.comments.some((c) => c.id === made.id)) this.comments = [...this.comments, made];
    else await this.refreshComments();
    return made ?? null;
  }
  async editComment(id: string, change: { body?: string; status?: CommentStatus }): Promise<void> {
    const saved = await this.api.editComment(id, change);
    this.comments = this.comments.map((c) => (c.id === id ? { ...c, ...change, ...(saved?.id === id ? saved : {}) } : c));
  }
  async removeComment(id: string): Promise<void> {
    await this.api.removeComment(id);
    this.comments = this.comments.filter((c) => c.id !== id);
  }

  /** The diagram's own list of edits not sent yet (it hands over the full list each time). */
  setOps(name: GraphName, ops: GraphOp[]): void {
    const file = GRAPH_FILES[name];
    if (ops.length) this.pendingOps[file] = [...ops];
    else delete this.pendingOps[file];
  }
  opsOf(name: GraphName): GraphOp[] { return this.pendingOps[GRAPH_FILES[name]] ?? []; }

  /** Where the user put the nodes: presentation only, saved directly. Never for the tasks diagram,
   *  which the service makes from the plan each time (it refuses positions for it). */
  async savePositions(name: GraphName, positions: Record<string, Pos>): Promise<void> {
    if (name === 'tasks') return;
    const g = this.graphs[name];
    if (g) for (const n of g.nodes) if (positions[n.id]) n.pos = { ...positions[n.id] };
    await this.api.savePositions(name, positions);
  }

  /** Every open comment of the step and its diagram edits, as ONE change request. */
  async sendChanges(step: StepId, note = ''): Promise<boolean> {
    const req = changeRequest(step, this.comments, this.pendingOps, note);
    if (!req) return false;
    const r = await this.api.changes(req);
    for (const g of req.graphOps ?? []) delete this.pendingOps[g.file];
    const sent = new Set(req.commentIds);
    this.comments = this.comments.map((c) => (sent.has(c.id) ? { ...c, status: 'sent' as const } : c));
    this.setStep(step, { status: 'drafting', ...(r?.chatId ? { chatId: r.chatId } : {}) });
    this.refreshOverview().catch(() => {});
    this.refreshComments().catch(() => {});
    return true;
  }

  async buildStart(maxParallel: number): Promise<void> { await this.api.buildStart(maxParallel); await this.refreshBuild(); this.refreshOverview().catch(() => {}); }
  async buildPause(): Promise<void> { await this.api.buildPause(); await this.refreshBuild(); }
  async buildResume(): Promise<void> { await this.api.buildResume(); await this.refreshBuild(); }
  async buildStop(): Promise<void> { await this.api.buildStop(); await this.refreshBuild(); }
  async retryTask(id: string): Promise<void> { await this.api.retryTask(id); await this.refreshBuild(); }
  async land(): Promise<void> { await this.api.land(); await this.refreshBuild(); this.refreshOverview().catch(() => {}); }
}
