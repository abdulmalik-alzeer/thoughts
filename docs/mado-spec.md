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

- Needs the project to be a git repository of its own, on a branch that is not `mado/*`. `GET /review` adds what it finds to `problems`: `no-repo` and `no-branch` (both block approval), and `uncommitted` (told only). `start` first commits the user's pending changes on their branch (`git add -A` without `mado/`, message “Before the MADO build”; a repository with no commit gets its first one), then branches from that commit. The build view's `blocked` no longer names pending changes.
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

## Round 0.4.0: better projects, and the plan in reach (as built)

Why: the first real build (mini-crm) passed 227 tests with three broken screens. Every fault was where one
task's work met another's, and nothing looked at the joined product. Everything below is additive: a plan
or build written by 0.3.x stays valid and builds exactly as before (see "Old plans").

### `plan.json` additions (version stays 1; all optional)

- `verify: string[]` — commands the engine runs in the build's merged copy **after every wave is merged** (the
  whole test suite, the type check, the linter). At most 100, each one line of at most 1000 characters, and
  plain like every command (`mado_validate::command_ok`); part of the review's `commands`, so approved with them.
- `run: { start, url, screens: [{ id, path }] }` — how to start the product and one concrete address per screen.
  `start` is a plain command and part of `commands`. `url` is exactly `http://localhost:<port>` or
  `http://127.0.0.1:<port>` (an optional trailing `/`; port 1–65535 without a leading zero; no path, query or user).
  `path` (`mado_validate::screen_path_ok`): one leading `/` (never `//`), at most 300 printable ASCII characters
  without spaces or backslashes, no `.`/`..` part (also percent-encoded) and never the shell's report address
  `/__thoughts_screen_check__` — exactly what the desktop shell will open. At most 200 screens, ids unique.
- task `kind?: "contracts" | "tests" | "feature"` (absent = feature). Files owned by a `contracts` or `tests` task
  are **locked** once it is done: a later commit that changes one is undone by the daemon (put back from the last
  commit, new files removed) and the task goes back to the assistant like a failed check, naming the files. A file
  the task's own `owns` names exactly (not a pattern) is that task's, so a feature's marker file stays writable.
  The rule holds on every fix path (wave fix, `verify` fix, review fix: undone before their checks; a merge-clash
  fix that changes another lane's locked file fails and the merge is undone).
- Review problems, all non-blocking ("worth a look"): `no-verify`, `no-run` (once), `no-contracts` (more than one
  task, none of kind contracts), `no-tests`, `unchecked-screen` (a flow screen without a `run.screens` entry; only
  when `run` exists). A 0.3.x plan now gets the first four as advice.

### Planning

- **Tech step** also writes `mado/conventions.md` (names, a screen, calling the API, errors, a table of shared
  helpers with the exact call and what comes back, tests, static checks). Optional (`mado::step_optional("tech")`):
  never required, so a 0.3.x tech step stays approved; when present it is validated, size-limited, listed in
  `files`, served and saved by `GET/PUT /doc/conventions` (`{text, exists}`), commentable, and an edit reopens the
  tech step. Static checks by default: a type check and a linter as build-only tools, unless the brief rules them
  out (then tech.md and conventions.md say so).
- **Plan step** (`mado_prompts::PLAN`): `verify`, `run` with one concrete path per screen, a first `contracts`
  task (an example file of every answer of every endpoint), one `tests` task per feature written from spec.md's
  Acceptance lists before the feature tasks (which depend on it), provider before consumer (unless a consumer
  builds against the contract files only), and a last task that walks whole journeys. **Tests first vs. `verify`
  after every wave** is settled in the prompt, not the engine: tests written first skip while a marker file of the
  feature (`tests/built/F2`) is missing; the feature's last task owns and creates the marker and its acceptance is
  `test -f tests/built/F2` then the feature's tests. The example plan in the prompt is checked by a test against
  the engine's validation, review and lock rules.
- The user's comments reach the plan step as data blocks (settled ones must be named in a task or in the summary;
  unsettled ones listed in the summary): at most 80, 1500 characters each.

### Model and effort

`GET/PUT /api/mado/:project/options` → `{ model: string | null, effort: string | null }`, kept in the app's own
record (`<home>/mado/<projectId>/options.json`, private file), never in the project. PUT: a field left out keeps
what is on record, `null` or `""` clears it; values pass `settings::check_cli_value`; the effort must be one of the
levels the installed `claude --help` lists (when it lists any); the model is any plain name (the app's list is open).
`mado::run_options` returns what was chosen; `mado::options_for_run` is what every `mado` and `mado-build` run is
started with: the choice, else Settings' Claude model/effort at the time the run starts; under full access with
another program chosen, that program's own Settings defaults (a Claude choice is never handed to Codex). The run's
chat records what it used.

### Build: three phases after the last wave

Only for a plan with any 0.4.0 field (`verify`, `run` or a task `kind`). `BuildStatus` gains `checking` and
`reviewing` (under way: pausable, stoppable, recovered after a restart). `BuildState` gains (all optional):

```ts
gateFixChatId?: string                         // on BuildWave: the run fixing a failed `verify`
verified?: boolean                             // on BuildWave: `verify` passed after this wave
screens?: { status: 'waiting' | 'running' | 'done' | 'skipped'; reason?: string; count?: number; failed?: number }
review?:  { status: 'running' | 'passed' | 'changes' | 'failed'; round: number; chatId?: string; fixChatId?: string;
            findings: { severity: 'must' | 'should'; where?: string; text: string }[]; finished?: boolean; note?: string }
```

0. **The `verify` gate**, after every merged wave: up to 3 fix runs in the build copy (one chat, its session
   resumed); still failing, the fixes are undone and the build waits; Resume runs the gate again with 3 fresh tries.
1. **Screen check** (`checking`), only with `run`. The address is checked again (never the daemon's port); nothing
   may already answer there; the daemon starts `run.start` in the build copy with the same clean environment and
   confinement as every build command, waits up to 60 s for `url`, and requires the listener to be in the started
   program's process group (`lsof`; where that cannot be told the check is skipped, unless full access is on). It
   asks each path itself (only 127.0.0.1/::1, no redirects, status line only) — `count`/`failed` are set from this
   even when no window looks. Then, if a window listens on the events socket, it offers the screens for 120 s:
   - `GET  …/build/screens` → `{ url, screens: [{id, path}], nonce }` while it waits, else 409.
   - `POST …/build/screens` `{ nonce, results: ScreenResult[] }` → `{ ok: true, taken }`. Wrong nonce 403 (compared in
     constant time), single use (then 409), body ≤ 4 MB; only offered ids at the offered path, once each; every field
     cleaned (control, direction and zero-width characters) and bounded (text 4000, fields 60, errors 20, line 500).
   - "Nobody looked" (no listener, 120 s passed, or an empty `results`): `screens.status = 'skipped'` with the reason
     (the daemon's words); the build goes on. The started program's whole process group is stopped at the end, on
     pause and stop, when the daemon exits and after a restart (`<home>/mado/<projectId>/app.pid` with its start time).
   Results are kept for the review (`screens.json` in the app's record).
2. **Review** (`reviewing`): a fresh run in Claude Code's `plan` mode (read-only, also under full access) with deny
   rules on the build copy; anything it changed anyway is put back. It reads spec.md's pass/fail lists,
   conventions.md, the comments and the screen results (as data between random delimiters). Findings come from the
   last fenced `json` block of its answer (`{"findings":[…]}`, at most 50, cleaned and bounded); none readable →
   `failed`.
3. **Fix**: `must` findings go to a fix run in the build copy; locked files are put back, `verify` runs again, and a
   failing fix round is undone and ends the review (`finished`, `note`). At most 3 reviews (the first and two after
   fixes). Whatever is left stays in `review.findings`; the build ends `done` and can be landed.

The desktop shell does the looking: `screenCheck(url, screens)` (`ui/src/lib/mado/screencheck.ts`) → command
`mado_screen_check` (`src-tauri/src/screen_check.rs`, main window only). Each screen opens in a hidden, unfocusable,
incognito window of its own (`screencheck-<n>`, no capability names it, so no IPC), allowed only that origin;
new windows, downloads and permissions refused, dialogs and pickers stubbed, the app's own `tauri://` answered 403.
It reads the text, every field with its label (passwords as "(filled in)", hidden inputs skipped), the page's
errors and failed requests, then reports through a cancelled navigation, re-cut and cleaned in Rust. One check at a
time; at most 40 screens looked at within a 100 s budget (the rest "Not checked: …"); ~10 s per screen. The shell
refuses, by error code: `screen-check-bad-url` (not a local address, or the daemon's port, or in a developer build
the dev server's), `screen-check-busy`, `screen-check-too-many` (over 200). `status` is optional and in practice
absent on macOS (WebKit does not give it); the daemon's own status per path stands in. In a browser and in a
second window it returns `null` and nothing is posted.

The window (`lib/mado/screenwatch.svelte.ts`) asks every 4 s while `checking`, one window at a time (a Web Lock per
project, plus a per-window busy flag and the nonces it dealt with), keeps only well-formed local addresses and paths
(at most 60 screens), cuts results to the contract's sizes and posts them once. `busy` is asked again on the next
turn; `bad-url` / `too-many` post an empty `results` at once (the build skips with its reason) and the window says
why, in English and Arabic.

### After the build

`GET …/build/guide` → `{ start?, url?, readme? }`: `start`/`url` from the build's approved plan snapshot (else the
project's validated plan), each only when it passes the review's rules; `readme` the project's own `README.md` (a
plain, not hard-linked file in the project folder; ≤ 8000 characters, cleaned). Once landed, the Build view shows
what was merged where, how many findings remain (and how many are `must`), **How to run it** (command with Copy, the
address as a link only when it is local, the README through `lib/md.ts` with pictures removed) and "The app does not
start your project for you"; **Plan a change** opens the features step; **Build the plan again** when allowed.

### Window

- The button on a project is **Build with MADO**.
- `GET /api/mado` → `[{ projectId, name, step, stepStatus, build?, updatedAt }]`, newest first: linked projects whose
  `state.json` is the one this daemon last wrote (its fingerprint; no program is run to tell trust). `step` is the
  first step not approved (`build` once all are); `build` is the engine's status word, absent while idle;
  `updatedAt` RFC 3339 UTC. The sidebar shows them under **Plans** (refetched on `mado` events), with a ring while
  something works and a dot when a build waits for the user; hidden when empty.
- The Plan view's header has the model and effort (the chats' picker, lists from `GET /api/ai/claude/options`, never
  hard-coded) and the chats' usage ring (plan limits only). `conventions.md` is a read-only **Conventions** tab.
- Work plan: "Agreed examples" / "Tests first" pills and a "How the build checks itself" block (verify, start,
  address, screen paths). Build view: per round "Whole project checked" or not, a link to the gate's fix chat; the
  screen check card (with the daemon's reason when skipped, and the screens it asked itself); the review card (round
  "n of at most 3", its note, its chats, findings as text).

### Old plans

A plan with none of `verify`, `run`, a task `kind` builds exactly as in 0.3.x: no gate, no screen check, no review,
`done` after the last wave, the same `build.json` shape. The window then shows no phase cards and, once landed,
says the plan was made before builds were checked and reviewed; the guide (USING.md) says the phases apply to plans
made with this version.

## Round 0.5.0: documenting a project that is already built (as built, daemon side)

Why: the owner's words, "make MADO read already built projects to document it; it should not be plan only when
linked to a folder." A plan now has a **mode**. A new project's plan (no mode) is exactly what it was in 0.4.x; a
built project's plan (`mode: "existing"`) has the same seven steps, the same files and the same shapes, but its
documents are written **from the code**, and its work-plan step plans **a change** on top of it. The Final check and
the build are unchanged. Everything below is additive; a `state.json` or `plan.json` of 0.3.x/0.4.x is read, shown,
checked and built exactly as before.

### `GET /api/mado/:projectId` gains `detected`

```ts
detected: { hasCode: boolean; languages: { lang: string; files: number }[]; files: number;
            hasReadme: boolean; manifests: string[]; capped: boolean } | null
```

- What the linked folder holds, from a walk of **names only** (`mado::existing::detect`): no file is opened, no program
  (git included) is run, no link is followed (a link is neither entered nor counted). Not entered: `mado/` (any letter
  case), every hidden folder (`.git`, `.venv`…), and `node_modules`, `bower_components`, `jspm_packages`, `target`,
  `dist`, `build`, `out`, `vendor`, `venv`, `env`, `__pycache__`, `site-packages`, `coverage`, `Pods`, `DerivedData`,
  `obj`, `tmp`, `temp`, `logs`. Bounded: 20 000 entries, 8 levels, 1.5 s; `capped: true` when a limit stopped it (the
  numbers are then a lower bound).
- `files`: source files counted by extension; `languages`: per language, most files first (at most 12; HTML and CSS
  are listed but do not count as a program). `manifests`: package and build files at most 3 levels down
  (`package.json`, `Cargo.toml`, `pyproject.toml`, `requirements.txt`, `go.mod`, `composer.json`, `Gemfile`, `pom.xml`,
  `build.gradle*`, `Package.swift`, `pubspec.yaml`, `*.csproj`, …), project-relative, at most 20, names with control or
  direction characters left out. `hasReadme`: a plain `README*` file at the top.
- **`hasCode`** is what the window decides on: true when there is a package or build file, or at least 5 program
  source files. It means "this folder looks like a project that is already built".
- `detected` is `null` when the app may not look: the folder is not trusted (`trust::require`, which yields to full
  access) or could not be read. Detection needs no plan (`exists: false` still has it).

### `POST /api/mado/:projectId/start` takes an optional body

`{ mode?: "new" | "existing" }`. No body, `{}` or `mode: null`: as before (a new plan is a new project's; a started plan
keeps its mode). `"new"` stores no mode at all. Anything else — another value, another field, not an object — is
refused (422 `mado.bad_mode`) and starts nothing. On a started plan the mode changes only while **no step is approved or
stale** and no brief run is under way; otherwise 409 `mado.mode_locked` (asking for the mode it already has is fine).
Answer as before: `{ ok: true, state }`.

### `MadoState` gains (both optional, absent in every plan before 0.5.0)

- `mode?: "existing"` — `normalize` keeps no other value. A `state.json` the daemon did not write keeps its mode
  (`distrust`): the mode only changes what runs are told and which shapes are accepted, never what may run.
- `documented?: { at: string }` — RFC 3339 time the tech step was last approved in existing mode: the documents are
  then the approved description of the code. Cleared when tech is reopened, when the mode is not `existing`, and by
  `distrust`. Display only. (No commit is recorded yet: a later "the code moved since" needs git and the rule-15 guards.)

### The steps in existing mode

Same steps, same files, same validation, except:

| Step | What the run is told (`mado_prompts::system_existing`) | Checks |
|---|---|---|
| brief | Draft the whole brief from the code first, then ask at most five questions about what code cannot tell (who, what hurts, what next). Headings: Problem, People and roles, **What it does today**, **What it does not do**, **What we want next**, Success, Constraints, Feel, Assumptions, Open questions. | `brief.md` required |
| spec | **As built**: one `### F<n> — Name` per feature the product has, a `- Status: built` / `partial` line, acceptance as the code behaves; real screens, each flow screen node with its real `route` and `props.source: "path:line"`. | as before |
| tech | **As is**: stack and versions from the manifests, real structure, every real route, data from migrations and models, the project's real check commands (missing ones "(proposed)"); `conventions.md` from the code with real examples; architecture/schema nodes carry `props.files`. | as before |
| design | **Current look**: `design.md` describes the look the product has; `design/styles.json` holds **1 to 4** candidates, the first `id: "current"` (tokens from the project's styles); mock-ups **optional** (a mock-up that is there is checked as always). | `styles.json` 1–4 (new mode still 3–4); mock-ups not required |
| plan | **Plan a change**: the change is what the user writes in the step's chat (the Run box note); vague → ask first. Writes `mado/change.md` (`# <title> — Change`: What the user asked, What changes, What must not change, Features, Files likely touched, Assumptions, Open questions) and `plan.json` with `change`. A new feature is first added to `spec.md` as `### F<n>` with `- Status: planned` (and its screens to the flow) — the one edit a plan run makes to an earlier step's files. Tasks only for the change; `verify` is the project's whole existing suite. | `plan.json` as before + `change`; `change.features` must be headings of `spec.md` |
| review, build | unchanged | unchanged |

Every statement in the documents ends with where it comes from: "(found in the code: path[:line])", "(from the
README)" or "(assumed)", so the user can check claims against the code. Comments, change requests, approve and reopen
are as before.

- `plan.json` `change?: { title: string; features: string[]; files?: string[] }` (`mado::PlanChange`, read with
  `mado::plan_change(bytes)` beside `PlanDoc`, which is unchanged). `title` 1–300 characters; `features` 1–100 distinct
  feature ids; `files` at most 100 paths that pass `owned_path_ok`; no other field. Absent or `null`: a plan as before.
  The review (`mado_review`, round C) may narrow `untraced-feature` to `change.features`.
- `mado/change.md` is a document of the plan step (`step_optional("plan")`): never required, checked when there, listed
  in `files`, served and saved by `GET/PUT /doc/change` (`{text, exists}`; an edit reopens the plan step), commentable.
- `GET /design/styles` adds `current: "current"` in existing mode when the assistant wrote that candidate; it is then
  listed first (before the app's own examples). Picking stays the user's (`POST /design/style`).

### What an existing-mode run may read, and what it never gets

- Reading is what every planning run had already: Claude Code's `default` mode reads inside its working folder (the
  project) without asking; the file tools stay inside it (`--restricted`), the app's records are denied. No rule was
  widened. Writing is unchanged: only `mado/**`, never the daemon's files, the same deny layers and stray check.
- Added for existing mode only (`mado::existing::secret_read_deny`, every letter case, absolute and project-relative,
  at any depth): Read is denied for `.env`, `.env.*` (examples too), `.envrc`, `secrets.*`, `credentials*`, `*.pem`,
  `*.key`, `*.p12`, `*.pfx`, `*.jks`, `*.keystore`, `*.kdbx`, `*-key.json`, `id_rsa*`, `id_dsa*`, `id_ecdsa*`,
  `id_ed25519*`, `.npmrc`, `.yarnrc.yml`, `.pypirc`, `.netrc`, `.git-credentials`, `.htpasswd`, `.pgpass`, `.dockercfg`,
  `*.tfvars`, `.docker/config.json`, and the whole of `.git/`, `.aws/` and `.ssh/` (nested repositories and
  `.git/modules` included). The search tool obeys the same rules.
- **A deny list is never complete.** So the run is also told not to open further, quote or repeat anything that looks
  like a secret, to write setting names only, and every file of `mado/` that holds an obvious key or token (a private-key
  header; AWS, GitHub, GitLab, Slack, Stripe, Anthropic, OpenAI, Google and npm token prefixes — `mado_validate::looks_secret`,
  a bounded plain check) is refused: the run is sent it back like any invalid file, and `GET /doc/:name`, `GET /graph/:name`
  and `GET /plan` answer 422 `mado.secret` ("… is not shown: it seems to hold a secret …") instead of the text. This check
  holds in both modes.
- Nothing it reads can leave: no shell, no connected tools (as for every planning run) and, in existing mode, **no web
  even when the step's chat has web lookups on** (`WebFetch`/`WebSearch` denied).
- Project text reaches the prompt only as data blocks: "What the app found in the folder" (`existing::found_text`:
  the counts, languages, manifests, the names at the top, and the first 6000 characters of a plain, not hard-linked
  `README.md`), and the code map when the code-review-graph tool has one (`existing::code_map`, at most 24 000
  characters; `None` until `v050-graph` is merged, then `crate::code_graph::code_map`).
- Proven against the installed Claude Code (2.1.295) with the rules a real existing-mode run is started with
  (`real_claude_code_keeps_an_existing_project_run_to_reading_code_and_writing_mado`, run by hand with `--ignored`):
  a source file is read; `.env`, `sub/.env.local`, `.git/config` and a nested `lib/x/.git/config` are refused; a search
  of the project finds nothing in them; a write outside `mado/` and a write of `mado/state.json` are refused;
  `mado/brief.md` is written.

### For the Plan view (round C)

- No plan yet (`exists: false`) and `detected?.hasCode`: offer two choices — "Document this project as it is"
  (`POST /start {mode:"existing"}`) and "Plan a new project here" (`POST /start {mode:"new"}`). `detected` null (not
  trusted yet): offer both too; the start then asks for trust as today. `hasCode` false: the usual start (no body).
  Show what was found in a line: the languages (`lang` and `files`), "and more" when `capped`.
- Header: the mode ("Documenting an existing project" / "New project"). While nothing is approved the user may switch
  (start again with the other mode; a 409 `mado.mode_locked` is shown as its message).
- Step labels in existing mode: brief "Brief, from the code", spec "What it does (as built)", tech "How it's built",
  design "How it looks now", plan "Plan a change" (its Run box asks "What do you want to change?" and its note is the
  change request), review and build as before. The plan step shows `change.md` as a tab ("The change") when it exists
  (`GET /doc/change`), and `plan.change.title`/`features` above the tasks.
- Once tech is approved in existing mode (`state.documented`): "Documented. Plan a change, or keep these as the
  project's documentation" — no further step is required.
- Design step in existing mode: `current` first and pre-selected in the picker when nothing is picked (picking still
  calls `POST /design/style`); no "missing mock-up" state for screens without one.
- `mado.secret` on a document: show its message in place of the document, like `mado.invalid`.

### Round C: the code graph in the build and the Final check, and the Plan view (`v050-madoui`)

- Planning runs of existing mode get `code_graph::code_map` (the stub of `mado_existing.rs` is swapped).
- Build runs, when the user's code-review-graph tool has a graph of the project (`mado_build::has_graph`): every task brief,
  the wave fix, the clash fix, the `verify` fix and the review fix get `code_graph::around` for the files they work on
  (`mado_build::files_of`: plain owned paths, patterns matched by a bounded walk of names); the review gets
  `code_graph::impact` of the files the build changed (`git diff <baseCommit>..HEAD`, after `git_safe`). Each is a data
  block (`mado_build_prompts::code_around`/`code_impact`). Without the tool or a graph nothing is asked and every prompt is
  byte for byte what it was (pinned by `without_the_code_graph_every_build_prompt_is_byte_for_byte_what_it_was`).
- `GET /review` (`mado_review::final_check`): in existing mode with `plan.change`, `untraced-feature` is kept to
  `change.features`. With a graph, three findings that never block: `untouched-impact` (files that use what the tasks
  change and that no task owns), `hidden-dependency` (task B owns a file that uses task A's files, with no order between
  them; `tasks: [B, A]`), `change-outside-scope` (a change with `files`: a task owning paths the change does not name and
  that do not use the files it names). At most 12 tasks are asked about one by one.
- Plan view: the two choices (`startChoice`), the mode pill and switch in the header (only while `modeSwitchable`), step
  names and words of existing mode (`mado-v050.step.*`), the change request box at the plan step, the `change.md` tab
  ("The change") and `plan.change` above the tasks, the documented banner, the current look first and marked in the design
  picker, a 422 on a document/diagram/plan shown in its place, and the Code graph card (header button with its state; open by
  default for a built project when the tool is there; one recommending line instead when it is not; dismissible per project).
