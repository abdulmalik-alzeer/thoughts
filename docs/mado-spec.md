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
| `state.json` | daemon only (the core, `mado.rs`) | `MadoState`; `style` is the style picked at the design step, set through `mado::set_style` (the design routes never write this file) |
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

What allows commands to run is never taken from the project folder (a build run, or a cloned repository
arriving with a ready-made `mado/`, could write it). The daemon keeps, outside the project, in
`<thoughts home>/mado/<projectId>/` (private, written atomically, mode 0600):
- `approval.json` = `{planHash, commands: string[], approvedAt}`: written when the user approves the review
  (`commands` = `Review.commands`, setup first), removed as soon as the review is no longer approved (reopened,
  stale, an earlier step approved again). `mado::approved_plan(state, projectId) -> Option<Approval>` (async) and
  `mado::approval_matches(&Approval, plan_bytes) -> bool` are what every command gate checks (the `mado-build`
  allowance, the engine) — never `state.plan_hash`, which is display only.
- `state.sha256`: the fingerprint of the last `state.json` the daemon wrote. A `state.json` the daemon did not write
  is not believed: steps with a run past a draft go back to `review`, `review` and `build` are closed, chat ids and
  `plan_hash` are dropped. `review` shows `approved` only with an approval record matching `plan.json` now (else `stale`).

Nothing in `mado/` may be a link: before every run of kind `mado` or `mado-build`, at `POST /start`, and before the
engine copies `mado/` into a worktree (and when a build starts), the daemon walks `mado/` (at most 5000 entries and 12
levels; anything beyond either is refused too) and refuses, naming the entry, any symbolic link, any entry that is not a
plain folder or file, and any file with more than one hard link (`mado::odd_entry`). Yields to full access.

Text from the project reaches a prompt only as data: file names only if they pass `clean_rel`, ids only if they match
their patterns, and free text (comment bodies, quoted passages, labels, task titles and descriptions, a check's output)
inside a block opened by `<<<DATA-<code>` and closed by `DATA-<code>>>>` with a fresh random code per block, control
characters and `<<<`/`>>>` taken out of the content, and a note saying it is the user's or the files' text, not instructions.

Graph ids, task ids, screen ids, style ids: `^[a-z0-9][a-z0-9-]{0,63}$`. Feature ids: `^F[0-9]{1,3}$`.
Limits: a graph has at most 400 nodes and 1200 edges; a mock-up at most 400 kB; a doc at most 1 MB.

## Shapes

See `ui/src/lib/mado/types.ts` for the exact fields. In short:

- `MadoState { version: 1, steps: Record<StepId, StepState>, style?: string, plan_hash?: string }`
  `StepState { status: 'locked'|'ready'|'drafting'|'review'|'approved'|'stale', chatId?: string, approvedAt?: string, strayed?: string[], strayUnchecked?: boolean }`
  `strayed`: project-relative paths the step's last run changed outside `mado/` (see Runs); while it is there the step
  cannot be approved; `POST /steps/:step/strayed/dismiss` clears it, and so does the next run that strays nowhere.
  `strayUnchecked`: that check could not be made (the kept "before" is missing, unreadable, over 64 MB or not the one
  whose SHA-256 the run's chat recorded in `meta.mado.strayCheck`): it fails closed, blocking approval the same way
  until dismissed.
  Order is fixed. A step is `ready` when the one before it is `approved` (`design` and `tech` both open after
  `spec`; `plan` needs both). Approving again after a change marks every later approved step `stale`
  (not locked: its files stay, the user re-runs or re-approves).
- `GraphDoc { version: 1, kind, title, nodes: GraphNode[], edges: GraphEdge[] }`
  `GraphNode { id, label, kind, props?, pos?: {x,y} }`, `GraphEdge { id, from, to, label?, kind? }`.
  `pos` is presentation only: the window may save it directly; a run must keep the `pos` of nodes it keeps.
- `PlanDoc { version: 1, tasks: PlanTask[], setup?: string[] }`
  `setup`: commands the build runs once in each fresh working copy before any task (installing dependencies);
  checked like acceptance commands, listed first in `Review.commands`, kept in the approval record.
  `PlanTask { id, title, description, owns: string[], dependsOn: string[], acceptance: string[], feature: string, screens?: string[] }`
  `owns` are project-relative paths or globs (`src/auth/**`). `acceptance` are shell commands run from the project root.
- `Comment { id, target, body, createdAt, status: 'open'|'sent'|'resolved'|'orphaned', runId? }`
  `target` is one of
  `{kind:'text', file, quote, prefix, suffix, heading?}` (quote plus up to 40 chars either side),
  `{kind:'node', file, nodeId}`, `{kind:'edge', file, edgeId}`, `{kind:'token', tokenPath}`,
  `{kind:'screen', screenId, x, y}` (a pin, x and y in 0..1 of the mock-up), `{kind:'task', taskId}`.
- `GraphOp` (what the user did to a diagram, sent as a change request, never applied by the window itself):
  `{op:'rename', id, label}`, `{op:'add-node', label, kind, near?: id}`, `{op:'remove-node', id}`,
  `{op:'add-edge', from, to, label?, kind?}`, `{op:'remove-edge', id}`, `{op:'relabel-edge', id, label}`,
  `{op:'set-prop', id, key, value}`.
  A node the user added in the same list has no id yet: `add-edge.from`/`to`, `set-prop.id` and `add-node.near`
  may name it as `new:<n>`, the n-th `add-node` of that list (1-based, n ≤ the number of `add-node`s). Every other
  value there, and every other id, must be a real id; anything else is refused (422). The run is told to name such
  nodes by their label and give each a proper id.
- `DesignTokens { color: {bg, surface, text, muted, primary, onPrimary, accent, border, success, warning, danger}, font: {heading, body, mono}, radius: number, spacing: number, density: 'compact'|'regular'|'roomy', shadow: 'none'|'soft'|'strong' }`
  Colours are `#rrggbb`. Fonts are names from a fixed list the app ships (system stacks only: no downloads).
  In mock-ups they are the CSS variables `--mado-color-bg` …, `--mado-font-heading` …, `--mado-radius`,
  `--mado-spacing`, `--mado-shadow`.
- `StyleCandidate { id, name, description, tokens: DesignTokens, source: 'app'|'assistant' }`
- `Review { features: [{id, name, tasks: string[]}], screens: [{id, label, tasks: string[]}], problems: Problem[], waves: Wave[], commands: string[], planHash: string }`
  `planHash` is the sha256 (lowercase hex) of the bytes of `plan.json` the review was computed from.
  `Problem { kind: 'cycle'|'missing-dep'|'overlap'|'untraced-feature'|'untraced-screen'|'unknown-feature'|'empty-acceptance'|'bad-path'|'bad-command', message, tasks?: string[] }`
  `bad-command`: an acceptance or setup command that is not one plain command — it has rule or pattern syntax
  (`* ? [ ] ( )`, `:*`) or shell syntax (`; | & \` $ > < \ { } ! #`, line breaks), or any character outside printable
  ASCII (0x20–0x7E: no bidi overrides, zero-width or other format characters). The review shows such a command with
  those characters escaped (`\u{202e}`), in the problem and in `commands`. It blocks approval, like
  `cycle`, `missing-dep`, `bad-path`, `empty-acceptance` and `unknown-feature`; the others are shown, not blocking.
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
  It also needs the desktop shell's approval (`?approval=`, `consent::Action::MadoApprove`): the window calls
  `approve_action('mado-approve', '<projectId>/<planHash>')`; the shell reads the project's name and the review's commands
  and hash from the service itself, refuses when the hash is not the one the window named, shows a native dialog naming
  the project, the number of commands and the commands (twelve, then "and N more — see the list in the window"), and
  signs over `consent::mado_subject(projectId, planHash)`. Without it (a plain browser, the token alone) the approval
  is refused (403), unless full access is on.
- `GET /comments` · `POST /comments {target, body}` · `PATCH /comments/:id {body?, status?}` · `DELETE /comments/:id`
- `POST /changes {step, commentIds: string[], graphOps?: {file, ops: GraphOp[]}[], note?}` → `{chatId}`
  One run applies everything, to the diagram and to the passages of the documents that describe it. Comments go
  `sent`, then `resolved` when the run ends well; text anchors are found again afterwards, lost ones go `orphaned`.
- `GET /review` → `Review`

Design (`mado_design.rs`):
- `GET /design/styles` → `{styles: StyleCandidate[], picked?: string, dropped: number}` (the app's own examples first, then the assistant's; `picked` is `state.style`)
- `POST /design/style {id}` → `{picked, tokens}`: writes `design/tokens.json` and records the pick in `state.json` `style` through the core (`mado::set_style`)
- `GET /design/tokens` (`null` before a pick) · `PUT /design/tokens DesignTokens` (direct, no run). Both changes are refused (409) while the design step is approved, as the core's state says.
- `GET /design/screens` → `[{id, label, hasMock}]` · `GET /design/screens/:id` → `{html}` (cleaned, see below)

Build (`mado_build.rs`):
- `GET /build` → `BuildState` plus `blocked?` and `confined: boolean` (whether the build's own commands run confined now) · `POST /build/start {maxParallel?}` · `POST /build/pause` · `/build/resume` · `/build/stop`
- `POST /build/tasks/:id/retry` · `POST /build/land` (brings the finished build branch into the user's branch)

Every change tells the window over the existing events socket: `{type: 'mado', projectId, what: 'state'|'doc'|'graph'|'comments'|'design'|'build'}`.
The window refetches; it does not trust the event's payload.

## Runs

- Kind `mado` (planning steps and change requests): cwd is the project, may read the project, may write only
  `mado/**` minus the daemon-only files; no shell, no MCP, no web unless the chat has it on. Writes are confined by
  allowing, not by denying: the run uses Claude Code's `default` mode (an edit is not accepted by itself), nobody can
  be asked, and the only allow rules are `Edit(<project>/mado/**)`; the run loads none of the user's or the project's
  own Claude Code settings (`--setting-sources ""`), so no allow rule of theirs widens it. Second layer: deny rules
  for every name at the top of the project that is not `mado` (each entry there, `*.*`, names of 1–3 and 5+ characters,
  four-letter names differing from `mado` in a letter; `mado::write_deny`), and for the daemon files. The deny rules
  are always also given in every letter case (fail closed), except those naming another spelling of `mado`, which stay
  exact. Whether the project's volume ignores case is probed (`mado::ignores_case`); only when it positively does is
  the allow rule folded too and other spellings of `mado` left alone (they are the same folder); otherwise (case-telling,
  or the probe could not tell) the allow stays exact and every other spelling of `mado` is denied. As a third, the
  daemon notes what changed outside `mado/` during the run (`git status` in a repository, else a walk of sizes and
  times capped at 20 000 entries, said when the cap is hit), tells the user in the step's chat and puts it on the step
  as `strayed`. Nothing is put back. The "before" is kept beside the approval record
  (`<home>/mado/<projectId>/stray-before-<chatId>.json`), so a run a daemon restart cut off is checked on the next
  start (`mado::check_cut_off_runs`), found from the chats still marked `meta.mado.strayCheck` (cleared once the
  result is reported), so a run whose kept file was deleted is reported as unchecked; kept files no chat names are
  dropped. The run is denied Read and Edit on that record folder. Always Claude Code (unless full access). One chat per step,
  kept in `state.steps[step].chatId`, so the user can talk in it (the brief is an interview: the assistant asks,
  the user answers in that chat). Never bypass, except under full access like every other kind.
- Kind `mado-build` (one task of a lane): cwd is the lane's worktree, may read and edit only there, and has **no shell**
  (unless full access): the shell would be a child of the assistant, which holds the sign-in, running code the run wrote.
  The daemon alone runs the plan's set-up and acceptance commands (see Build engine). Nobody can be asked. Its cwd must
  be one of the engine's worktrees (`<home>/mado/<projectId>/build` or `…/lane-<w>-<l>`, canonical) with no added
  folders, or the run is refused (this holds under full access too). Edits of the user's own project folder and of the
  approval records are denied, and so are the git subcommands that change branches or history. Like `mado` runs, it
  loads none of the user's or the project's own Claude Code settings. The chat carries `meta.mado = {projectId, taskId}`
  and is a read-only record: `chat_send` (and so editing and resending) refuses a `mado-build` chat, and the window shows
  no message box for it. Always Claude Code (unless full access).
- Every guard MADO adds yields to `full_access()` exactly like the existing ones, and behaves as written here while it is off.

## Build engine

- Needs the project to be a git repository with a clean working tree; the view says so and offers nothing else.
- Refuses to start unless the daemon's approval record exists and matches `plan.json` now (`approval_matches`).
  The review step shows every acceptance command verbatim; approving it is what allows the daemon to run them.
- Works on a branch of its own (`mado/build-<n>`, from the user's current branch) in worktrees under
  `<thoughts home>/mado/<projectId>/`, one per lane. Never checks out, commits to or merges into the user's
  branch during a build. Merges are done by the daemon, never by a run.
- A wave: start its lanes (at most `maxParallel` at once, default 3), each lane runs its tasks in order; a task is
  done when its run ended, its acceptance commands pass in the lane's worktree and a commit exists. Then the
  daemon merges the lanes into the build branch one by one; a merge conflict marks the task `conflict` and
  starts one fix run in the build worktree; if that fails the build goes `waiting` for the user.
- A failed task is tried up to three more times (`MAX_ATTEMPTS` 4): each try goes on in the same task chat (its session
  resumed) with the failing command and its capped output, then the build goes `waiting`; tasks that depend on it are not started.
- The daemon's own commands (set-up and checks, `run_check`): run with the user's shell from a cleared environment
  (PATH, HOME, USER, LOGNAME, SHELL, LANG, `LC_*`, TMPDIR, TERM and the toolchain homes, never a name with TOKEN, KEY,
  SECRET or PASSWORD, nor `ANTHROPIC_*`, `CLAUDE*`, `THOUGHTS_*`), `CI=1`, git reading no global or system settings, and
  every package tool's cache pointed at the build's own `<home>/mado/<projectId>/cache/` (removed with the worktrees).
  On macOS, while `spending.confineCommands` is on (default; off only with the shell's approval,
  `?confirm=confine-off&approval=`, `Action::ConfineOff`; on is free) and full access is off, each runs under
  `/usr/bin/sandbox-exec` with a generated profile: default allow; writes only inside the worktree and the build's cache
  (their contents: the folders themselves, and the folder they are in, cannot be removed, renamed or replaced), the
  temporary folders and the null devices; reads of the user's home and the app's folder closed except the worktree, the
  cache, the project's `.git`, `RUSTUP_HOME` and the toolchains on the PATH; credential places and the user's own
  folders (`.ssh`, `.claude*`, `.aws`, `.npmrc`, `Documents`, `Library/Keychains`, …) and the app's records closed last;
  no connection to the daemon's port; no keychain. Before every command the worktree and cache must be plain folders,
  not links. If the sandbox is missing or does not start, or a folder is a link, the command is not run and the build
  goes `waiting` with a plain message naming the setting. Elsewhere there is no confinement (`confined: false`).
- Git: before a build starts, before `land`, and right before and after each of the engine's own git changes in a
  worktree (worktree add, merge, commit, reset, checkout), the repository's own configuration (`git config --local
  --no-includes --list`) must hold no key that starts a program (`filter.*.clean/smudge/process` other than git-lfs's
  standard ones, `merge.*.driver`, `diff.*.textconv/command`, `diff.external`, editors, pagers, `credential.*helper`,
  `gpg.*program`, `*.gpgsign` on, `core.sshCommand`, `core.fsmonitor`, `core.hooksPath`, …), no `include.path` or
  `includeIf.*.path`, no `extensions.worktreeConfig` and no `config.worktree` file; otherwise it refuses (the build goes
  `waiting`), naming the keys. Every git call of the app also overrides hooks, fsmonitor, ssh command, editor,
  sequence editor, pager, signing and credential helper on the command line. Yields to full access.
- Governor: before starting a lane, look at the usage the app already meters. Above the threshold
  (setting, default 80 % of the current window) it holds new lanes (`holding: true`, with the reason) and resumes by itself.
- Survives a restart of the daemon: `build.json` plus git are the truth; a task that was `running` is started again.
  Every linked project of every listed library is picked up, whichever library is open, and the build goes on
  while another library is open; starting, stopping and landing stay with the open library's projects.
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
