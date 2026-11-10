# Thoughts app — contributor notes

- Workspace: `crates/thoughtsd` (daemon, all logic, HTTP API in `api.rs`), `src-tauri` (thin shell),
  `ui/` (Svelte 5 runes, no SvelteKit). Product decisions live in the user's library, at
  `tools/thoughts-workstation/notes/decisions.md`.
- The UI talks only to the daemon API. Tauri commands are just `daemon_info` and `restart_daemon`.
- Never hard-code model, effort or permission lists: they come from `claude --help` (`claude.rs`).
- Library writes must respect the library's conventions: todos are edited line by line in place,
  and frontmatter is preserved.
- Anything the user can customise belongs in the app layer, never in core. The Recovery menu and
  Rollback must stay outside the app layer.
- Verify with `cargo test -p thoughtsd`, `npx vite build` and `cargo build -p thoughts`.
