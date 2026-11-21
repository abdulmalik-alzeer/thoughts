# MADO in Thoughts — the contract (0.3.0)

MADO is the app's way of planning and building a software project: seven steps, each with documents
and diagrams the user reviews, comments on and approves, then a build the app runs itself.
This file is the contract between the daemon and the window. `ui/src/lib/mado/types.ts` is the same
contract in TypeScript. If you must deviate, say so in your report; do not silently change a shape.

## Decisions (the owner's, 2026-11-21)

- No set of agent files. Each step is an ordinary run with a prompt the daemon holds (`mado_prompts`),
  versioned with the binary. The app never writes `.claude/` for MADO.
- Everything lives in the linked project's folder, in `mado/`. Nothing in the library, nothing in `~/.thoughts`
  except build worktrees and transient engine state.
- Seven steps: `brief`, `spec`, `tech`, `design`, `plan`, `review`, `build`.
- Diagrams are an app-owned graph JSON drawn in the window with @xyflow/svelte (interactive). Archify stays
  for figures in chats and notes; MADO does not use it.
- Screen mock-ups are free HTML and CSS written by the assistant, shown only inside a sealed frame
  (`<iframe sandbox="">`, no scripts, no network), after the daemon cleaned them.
- The parallel build engine ships in this release.

## Files in `<project>/mado/`

| File | Written by | What |
|---|---|---|
| `state.json` | daemon only | `MadoState` |
| `brief.md` | step `brief` | the project brief from the interview |
| `spec.md` | step `spec` | product spec. Features are headings `### F<n> — <name>` (ids `F1`, `F2`, …) |
| `flow.graph.json` | step `spec` | `GraphDoc` kind `flow`: screens (`kind: "screen"`), decisions, actions, journeys as edges |
| `tech.md` | step `tech` | technical design: stack, components, interfaces, risks |
| `architecture.graph.json` | step `tech` | `GraphDoc` kind `architecture` |
| `schema.graph.json` | step `tech` | `GraphDoc` kind `erd`: one node per entity, `props.fields: [{name,type,key?,note?}]`, edges are relations with `kind` `1-1`/`1-n`/`n-n` |
| `design.md` | step `design` | the design direction in words |
| `design/styles.json` | step `design` | `{version:1, styles: StyleCandidate[]}` — 3 or 4 candidates |
| `design/tokens.json` | window (direct) and step `design` | `DesignTokens` of the picked style, as edited by the user |
| `design/screens/<screenId>.html` | step `design` | one mock-up per `screen` node of the flow |
| `plan.json` | step `plan` | `PlanDoc` |
| `comments.json` | daemon only | `{version:1, comments: Comment[]}` |
| `build.json` | daemon only | `BuildState` (also the record of what was built) |

Runs of kind `mado` may write only inside `<project>/mado/` and never `state.json`, `comments.json`,
`build.json`. The daemon validates every file a run wrote before the step can be reviewed; an invalid
file is reported to the user and to the run (one automatic repair turn), never drawn.

Graph ids, task ids, screen ids, style ids: `^[a-z0-9][a-z0-9-]{0,63}$`. Feature ids: `^F[0-9]{1,3}$`.
Limits: a graph has at most 400 nodes and 1200 edges; a mock-up at most 400 kB; a doc at most 1 MB.

## Shapes

See `ui/src/lib/mado/types.ts` for the exact fields. In short:

- `MadoState { version: 1, steps: Record<StepId, StepState>, style?: string, plan_hash?: string }`
  `StepState { status: 'locked'|'ready'|'drafting'|'review'|'approved'|'stale', chatId?: string, approvedAt?: string }`
  Order is fixed. A step is `ready` when the one before it is `approved` (`design` and `tech` both open after
  `spec`; `plan` needs both). Approving again after a change marks every later approved step `stale`
  (not locked: its files stay, the user re-runs or re-approves).
- `GraphDoc { version: 1, kind, title, nodes: GraphNode[], edges: GraphEdge[] }`
  `GraphNode { id, label, kind, props?, pos?: {x,y} }`, `GraphEdge { id, from, to, label?, kind? }`.
  `pos` is presentation only: the window may save it directly; a run must keep the `pos` of nodes it keeps.
- `PlanDoc { version: 1, tasks: PlanTask[] }`
  `PlanTask { id, title, description, owns: string[], dependsOn: string[], acceptance: string[], feature: string, screens?: string[] }`
  `owns` are project-relative paths or globs (`src/auth/**`). `acceptance` are shell commands run from the project root.
- `Comment { id, target, body, createdAt, status: 'open'|'sent'|'resolved'|'orphaned', runId? }`
  `target` is one of
  `{kind:'text', file, quote, prefix, suffix, heading?}` (quote plus up to 40 chars either side),
  `{kind:'node', file, nodeId}`, `{kind:'edge', file, edgeId}`, `{kind:'token', tokenPath}`,
  `{kind:'screen', screenId, x, y}` (a pin, x and y in 0..1 of the mock-up), `{kind:'task', taskId}`.
- `GraphOp` (what the user did to a diagram, sent as a change request, never applied by the window itself):
  `{op:'rename', id, label}`, `{op:'add-node', label, kind, near?: id}`, `{op:'remove-node', id}`,
  `{op:'add-edge', from, to, label?}`, `{op:'remove-edge', id}`, `{op:'relabel-edge', id, label}`,
  `{op:'set-prop', id, key, value}`.
- `DesignTokens { color: {bg, surface, text, muted, primary, onPrimary, accent, border, success, warning, danger}, font: {heading, body, mono}, radius: number, spacing: number, density: 'compact'|'regular'|'roomy', shadow: 'none'|'soft'|'strong' }`
  Colours are `#rrggbb`. Fonts are names from a fixed list the app ships (system stacks only: no downloads).
  In mock-ups they are the CSS variables `--mado-color-bg` …, `--mado-font-heading` …, `--mado-radius`,
  `--mado-spacing`, `--mado-shadow`.
- `StyleCandidate { id, name, description, tokens: DesignTokens, source: 'app'|'assistant' }`
- `Review { features: [{id, name, tasks: string[]}], screens: [{id, label, tasks: string[]}], problems: Problem[], waves: Wave[], commands: string[], planHash: string }`
  `planHash` is the sha256 (lowercase hex) of the bytes of `plan.json` the review was computed from.
  `Problem { kind: 'cycle'|'missing-dep'|'overlap'|'untraced-feature'|'untraced-screen'|'unknown-feature'|'empty-acceptance'|'bad-path', message, tasks?: string[] }`
  `Wave { lanes: Lane[] }`, `Lane { id, tasks: string[] }`. A wave is one dependency level; inside a wave,
  tasks whose `owns` overlap share a lane (run one after another), others get their own lane.
- `BuildState { version: 1, status: 'idle'|'running'|'paused'|'waiting'|'failed'|'done'|'stopped', branch, base, startedAt?, waves: BuildWave[], governor: {maxParallel, running, holding: boolean, reason?}, plan_hash }`
  `BuildWave { status, lanes: [{id, status, tasks: [{id, status: 'pending'|'running'|'checking'|'done'|'failed'|'conflict'|'skipped', chatId?, commit?, note?}]}] }`

## API (all under `/api/mado/:projectId`, app token required like every route)

Core (`mado.rs`):
- `GET ''` → `{project, exists, state, files: string[]}` · `POST /start` scaffolds `mado/` (trust required)
- `GET /doc/:name` → `{text}` · `PUT /doc/:name {text}` (the user's own edit; names: brief, spec, tech, design)
- `GET /graph/:name` → `GraphDoc` (names: flow, architecture, schema, tasks; `tasks` is derived from `plan.json`, read-only)
  `PUT /graph/:name/positions {positions: Record<id,{x,y}>}`
- `GET /plan` → `PlanDoc`
- `POST /steps/:step/run {note?}` → `{chatId}` starts or continues the step's run (status `drafting`, then `review` when it ends)
  `POST /steps/:step/approve` · `POST /steps/:step/reopen`
  Approving `review` takes `{planHash}` (the one its `GET /review` carried). It is refused, with a plain message, when the hash
  is missing or differs from the hash of `plan.json` now (the plan changed after the user saw it); the matching hash is kept as `state.plan_hash`.
- `GET /comments` · `POST /comments {target, body}` · `PATCH /comments/:id {body?, status?}` · `DELETE /comments/:id`
- `POST /changes {step, commentIds: string[], graphOps?: {file, ops: GraphOp[]}[], note?}` → `{chatId}`
  One run applies everything, to the diagram and to the passages of the documents that describe it. Comments go
  `sent`, then `resolved` when the run ends well; text anchors are found again afterwards, lost ones go `orphaned`.
- `GET /review` → `Review`

Design (`mado_design.rs`):
- `GET /design/styles` → `{styles: StyleCandidate[], picked?: string}` (the app's own examples first, then the assistant's)
- `POST /design/style {id}` picks one and writes `design/tokens.json`
- `GET /design/tokens` · `PUT /design/tokens DesignTokens` (direct, no run)
- `GET /design/screens` → `[{id, label, hasMock}]` · `GET /design/screens/:id` → `{html}` (cleaned, see below)

Build (`mado_build.rs`):
- `GET /build` → `BuildState` · `POST /build/start {maxParallel?}` · `POST /build/pause` · `/build/resume` · `/build/stop`
- `POST /build/tasks/:id/retry` · `POST /build/land` (brings the finished build branch into the user's branch)

Every change tells the window over the existing events socket: `{type: 'mado', projectId, what: 'state'|'doc'|'graph'|'comments'|'design'|'build'}`.
The window refetches; it does not trust the event's payload.

## Runs

- Kind `mado` (planning steps and change requests): cwd is the project, may read the project, may write only
  `mado/**` minus the daemon-only files; no shell, no MCP, no web unless the chat has it on. One chat per step,
  kept in `state.steps[step].chatId`, so the user can talk in it (the brief is an interview: the assistant asks,
  the user answers in that chat). Never bypass, except under full access like every other kind.
- Kind `mado-build` (one task of a lane): cwd is the lane's worktree, may write only there, shell as the app's
  rules for unattended work allow (questions surface in the build view). Never bypass, except under full access.
- Every guard MADO adds yields to `full_access()` exactly like the existing ones, and behaves as written here while it is off.

## Build engine

- Needs the project to be a git repository with a clean working tree; the view says so and offers nothing else.
- Refuses to start unless `review` is approved and `state.plan_hash` equals the hash of `plan.json` now.
  The review step shows every acceptance command verbatim; approving it is what allows the daemon to run them.
- Works on a branch of its own (`mado/build-<n>`, from the user's current branch) in worktrees under
  `<thoughts home>/mado/<projectId>/`, one per lane. Never checks out, commits to or merges into the user's
  branch during a build. Merges are done by the daemon, never by a run.
- A wave: start its lanes (at most `maxParallel` at once, default 3), each lane runs its tasks in order; a task is
  done when its run ended, its acceptance commands pass in the lane's worktree and a commit exists. Then the
  daemon merges the lanes into the build branch one by one; a merge conflict marks the task `conflict` and
  starts one fix run in the build worktree; if that fails the build goes `waiting` for the user.
- A failed task is retried once with the failure output, then the build goes `waiting`; tasks that depend on it are not started.
- Governor: before starting a lane, look at the usage the app already meters. Above the threshold
  (setting, default 80 % of the current window) it holds new lanes (`holding: true`, with the reason) and resumes by itself.
- Survives a restart of the daemon: `build.json` plus git are the truth; a task that was `running` is started again.
- `land`: only when `done`, only into a clean working tree, fast-forward if possible, otherwise a merge commit;
  on conflict nothing is changed and the user is told.

## Mock-ups (the sealed frame)

- The daemon cleans a mock-up when it is read: parsed as HTML; `script`, `iframe`, `object`, `embed`, `link`,
  `base`, `meta`, `form` actions, every `on*` attribute, every `javascript:` and every URL that is not a
  `data:` image are taken out; in CSS `@import`, `url()` that is not `data:`, `expression(` and `-moz-binding` are taken out.
- The window shows it only as `srcdoc` of `<iframe sandbox="" referrerpolicy="no-referrer">` with a first-line
  `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:">`
  and a `<style>` of the token variables, both put there by the window. Never as markup in the page.
- Colours, fonts and sizes change live by rebuilding `srcdoc` with new variables; no run.
- The user comments by dropping a pin: a transparent layer over the frame takes the click (the frame itself gets no events).

## Window

- Projects › a project › **Plan**: a stepper of the seven steps; per step the documents, the diagrams, the step's chat,
  the comments, and Run / Send changes / Approve. Everything in English and Arabic, RTL, theme tokens only.
- Components with a fixed interface (the Plan view mounts them; their owners build them):
  `ui/src/lib/mado/graph/MadoGraph.svelte` and `ui/src/lib/mado/design/DesignStep.svelte` (props in the files).
