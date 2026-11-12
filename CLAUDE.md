# Thoughts app — contributor notes

- Workspace: `crates/thoughtsd` (daemon, all logic, HTTP API in `api.rs`), `src-tauri` (thin shell),
  `ui/` (Svelte 5 runes, no SvelteKit). Product decisions live in the user's library, at
  `tools/thoughts-workstation/notes/decisions.md`.
- The UI talks to the daemon API for everything about the library, chats and settings. Tauri commands
  exist only for what needs the desktop itself (save and open dialogs, opening a file in its app,
  updates, menus and the Dock, quick capture, voice, quitting and restarting the service) and hold
  no library logic; the list is the `generate_handler!` in `src-tauri/src/main.rs`.
- Never hard-code model, effort or permission lists: they come from `claude --help` (`claude.rs`).
  Effort levels and permission modes are shown under Claude Code's own names.
- `bypassPermissions` is only for ordinary chats (`kind == "chat"`) and is only switched on after the
  user confirmed the warning dialog; the daemon refuses the switch without that confirmation
  (`check_bypass` in `api.rs`, `?confirm=` on `PUT /api/settings`). Runs the app starts itself never use it,
  and a bypass default in settings is only inherited by chats the user starts by hand (`byUser`).
- Library writes must respect the library's conventions: todos are edited line by line in place,
  and frontmatter is preserved.
- Anything the user can customise belongs in the app layer, never in core. The Recovery menu and
  Rollback must stay outside the app layer.
- Verify with `cargo test -p thoughtsd`, `npx vite build` and `cargo build -p thoughts`.
