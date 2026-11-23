# Thoughts app — contributor notes

- Workspace: `crates/thoughtsd` (daemon, all logic, HTTP API in `api.rs`), `src-tauri` (thin shell),
  `ui/` (Svelte 5 runes, no SvelteKit). Product decisions live in the user's library, at
  `tools/thoughts-workstation/notes/decisions.md`.
- The UI talks to the daemon API for everything about the library, chats and settings. Tauri commands
  exist only for what needs the desktop itself (save and open dialogs, opening a file in its app,
  updates, menus and the Dock, quick capture, voice, quitting and restarting the service) and hold
  no library logic; the list is the `generate_handler!` in `src-tauri/src/main.rs`. Each command there is
  also named once in `COMMANDS` in `src-tauri/build.rs` (the app manifest) and granted as `allow-<command>`
  only to the windows that call it (`capabilities/default.json` for the app window, `capture.json` for
  quick capture; add-on frames get none). The build fails, naming the command, if the three disagree.
  Commands that matter check the caller too (`main_only` / `is_main_label`).
- Never hard-code model, effort or permission lists: they come from `claude --help` (`claude.rs`).
  Effort levels and permission modes are shown under Claude Code's own names.
- `bypassPermissions` is only for ordinary chats (`kind == "chat"`) and is only switched on after the
  user confirmed the warning, which the shell shows as a native dialog (`approve_action`); the daemon refuses the switch without that confirmation
  (`check_bypass` in `api.rs`, `?confirm=` on `PUT /api/settings`). That confirmation, like a folder trust
  answer, carries an approval from the desktop shell made with a key the window never sees
  (`consent.rs` in the daemon and in `src-tauri`): the app token alone is not a yes. Runs the app starts itself never use it,
  and a bypass default in settings is only inherited by chats the user starts by hand (`byUser`).
- Full access (`settings.fullAccess`, `full_access.rs`, Settings › Privacy & cost) is the exception the
  owner chose: while it is on, every guard the app itself adds yields — every kind of run bypasses
  permissions (Codex: its own unrestricted mode), no rules file, sandbox, command check, trust check or
  standing-file restore; the customisation layer may change or remove anything, core, Recovery and
  Rollback included (history is still recorded); add-ons get every permission and pages get the machine
  bridge (`machine.rs`, `ui/src/lib/layer/machine.ts`). Every new guard must yield to it the same way,
  behind one `full_access()` check, and stay exactly as before while it is off. Switching it ON is the
  guarded step: only through the shell's two-step native warning (`Action::FullAccess`,
  `?confirm=full-access&approval=`); the body and the token alone never switch it. OFF is
  `?fullAccess=off`, at once, and stops what was started with it. Access keys never get any of it.
- Library writes must respect the library's conventions: todos are edited line by line in place,
  and frontmatter is preserved.
- Anything the user can customise belongs in the app layer, never in core. The Recovery menu and
  Rollback must stay outside the app layer (out of reach unless full access is on).
- Diagrams and charts in answers and notes (`figures.rs`, `ui/src/lib/figures/`): a fenced block in the language `archify`
  is drawn by the copy of Archify in `vendor/archify` (run by the daemon with the machine's Node.js, from a closed
  folder, compared with the binary's own copy before every use); a `chart` block is drawn by the window. Both are shown
  as an `<img>` of a self-contained SVG and never as markup in the page; the shell saves an SVG only after parsing it
  against an allowlist (`plain_svg`; `DRAWN` in `standalone.ts` is the same list). Never show a figure by putting SVG
  into rendered markdown, and never let a document reach outside itself (`figures::prepare` takes out what would).
- MADO, planning and building a project (`mado*.rs`, `ui/src/lib/mado/`, `ui/src/views/mado/`; contract
  `docs/mado-spec.md`): the plan lives in `<project>/mado/`, whose `state.json`, `comments.json` and `build.json`
  only the daemon writes (`state.json` only through the core, `mado.rs`: the design step records its pick with
  `mado::set_style`). Nothing in the project is trusted: a `state.json` the daemon did not write (fingerprint in
  `<home>/mado/<projectId>/state.sha256`) is distrusted, and what allows commands is the approval record beside it
  (`approval.json` = plan hash + the commands the user was shown), never anything in `mado/`. Every file a run wrote
  is validated and size-limited before it is shown; mock-ups are cleaned from an allowlist and shown only in the
  sealed frame (`<iframe sandbox="" srcdoc>` with its own CSP meta), never as markup in the page. A planning run
  (kind `mado`) loads no user or project Claude Code settings and is confined by allow rules for `mado/**` only,
  with deny rules for every other top-level name (folded in every letter case; the allow is folded only on a volume
  probed to ignore case) and the stray check afterwards; a build run (`mado-build`) has no shell, edits only its own
  worktree (refused anywhere else, with no added folders) and its chat is a read-only record. Only the daemon runs the
  approved set-up and check commands (`mado_build::run_check`): cleared environment, the build's own caches, and on
  macOS `sandbox-exec` with a generated profile (setting `spending.confineCommands`, on by default). Approving the Final
  check needs the shell's yes (`Action::MadoApprove`, bound to the plan hash, `?approval=`), and so does switching
  confinement off (`Action::ConfineOff`). Links, special or hard-linked files in `mado/` are refused (`mado::odd_entry`),
  and so is a project whose own git config starts programs or includes other config (`git::program_keys`). Text from
  the project reaches prompts only as data blocks (`mado_prompts::block`/`inline`). What yields to full access: trust
  checks, link/inside-`mado/` checks, the git-config check, rules, the command check and the confinement. What does
  not: validation and cleaning, the approval record and plan hash, the clean-tree and never-the-user's-branch rules of
  the build, the approved-step lock, where a build run may work, and the build's usage governor.
  Round 0.4.0 (last section of the spec): a plan may add `verify`, `run` and task `kind`s; only such a plan gets the
  phases after the last wave (the `verify` gate after every wave, the screen check, the review and its fixes,
  `mado_build_finish.rs`), and files of `contracts`/`tests` tasks are locked on every path. The daemon starts
  `run.start` only confined like every build command, checks the address is local and answered by that program's own
  process group, and always stops it. The screens are looked at by the desktop shell (`src-tauri/src/screen_check.rs`,
  command `mado_screen_check`, main window only) in hidden windows that no capability names (no IPC), one origin only;
  what they report, the review's answer, README and conventions are untrusted text (cleaned, bounded, data blocks).
  Every planning and build run takes its model and effort from `mado::options_for_run` (the project's record outside
  the project, else Settings; never a Claude choice for Codex); `mado_e2e_tests.rs` walks a whole 0.4.0 project
  over HTTP and checks the command line of every run.
- Other providers (`providers.rs`, round 0.5.0): the app never bundles or starts a gateway (no LiteLLM), and
  subscription sign-in stays inside the unmodified CLIs. Another service is Claude Code's own `ANTHROPIC_BASE_URL`
  or Codex's own `model_providers` (`-c` per run, key only in `THOUGHTS_PROVIDER_KEY`, kept from commands); a keyless
  service on this computer gets the stand-in `NO_KEY`, never the sign-in. Only the address check yields to full access.
- Verify with `cargo test -p thoughtsd`, `npx vite build` and `cargo build -p thoughts`.
