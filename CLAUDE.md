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
- Verify with `cargo test -p thoughtsd`, `npx vite build` and `cargo build -p thoughts`.
