# Agent Client Protocol (ACP) in Thoughts

The daemon speaks ACP (agentclientprotocol.com) to the official adapters over their stdio.

- **Adapters** (`crates/thoughtsd/src/acp_install.rs`): `@agentclientprotocol/claude-agent-acp` and
  `@agentclientprotocol/codex-acp`, each pinned to one exact version in `PINS` (`CLAUDE`, `CODEX`).
  Installed on demand with the machine's npm into `<home>/acp/<name>-<version>/` (`--ignore-scripts
  --omit=optional`, staged then renamed); never vendored. The adapters' own copies of the CLIs are
  left out: `CLAUDE_CODE_EXECUTABLE` / `CODEX_PATH` point them at the user's installed program, so
  sign-in stays inside the unmodified CLI. Raise a pin only after checking it against the app.
- **Client** (`acp.rs`): a small JSON-RPC client on tokio (not the `agent-client-protocol` crate: its
  connection runs on its own executor and keeps session config options, resume and fork behind
  unstable features). `catalog` reads models / effort levels / modes from `configOptions`
  (categories `model`, `thought_level`, `mode`), else the older `modes` / `models` fields.
  `Translator` turns `session/update` into the `stream-json` events `claude::handle_event` already
  shows. `pick_option` answers permission requests from an allowlist only: the plain yes/no
  answers, and options that switch the mode only to the chat's checked mode or a stricter one
  (`acp::mode_within`: plan < default < acceptEdits < auto < bypassPermissions; unknown = never).
  Full access lifts it.
- **Detection** (`acp_agent::catalog` / `probe`): `session/options` (else `session/new`) in an empty
  folder of the app's own, cached per program and provider address, read in the background;
  `acp.options` is announced when it is in. `GET /api/ai/{claude,codex}/options` add `models`
  (+ `modelNames`), `efforts` (+ `effortNames`) and, for Claude Code, `permissionModes` (+ `modeNames`)
  — only modes `settings::PERMISSION_MODES` accepts. Until the report is in, the old lists
  (`claude --help`, Codex's catalog) are served.
- **Runs**: ordinary Claude Code chats (kind `chat`) run over ACP once the adapter is installed
  (`claude::start_acp_chat`; `ai.classicChats` keeps the command line). Every guard of the
  command-line path travels: the rules file (deny/allow/ask rules + the `shell_guard` hooks) as the
  SDK `settings`, the app's notes appended to Claude Code's system prompt, the spending limit,
  `allowDangerouslySkipPermissions` only for a confirmed bypass, the profile's flag guards
  (`settingSources` for `--setting-sources`; `strictMcpConfig` + no servers + `disallowedTools: mcp__*`
  for `--strict-mcp-config`; a profile needing `--restricted` stays on the command line,
  `acp_agent::carries`), the mode set over ACP and checked (a mode not offered refuses the turn; a
  switch mid-turn — `current_mode_update` or `config_option_update` — to any mode doing more than
  the checked one ends it). Permission
  requests become the chat's approval cards (`approvals::ask`) or are refused when nobody can be
  asked. Resume is `session/resume` of the stored session id (the same id the CLI path uses), a
  branch is `session/fork`. Providers: `ANTHROPIC_*` env for Claude Code; Codex's
  `model_providers` as `CODEX_CONFIG` JSON with the key in `THOUGHTS_PROVIDER_KEY`.
- **Still on the command line**: everything the app starts itself (wrap, reorganize, set-up,
  modules, workflows, customise), MADO planning and build runs (their `--setting-sources ""`,
  `--restricted` and allow-only confinement have no checked equivalent over ACP yet), one-shot
  completions, and all Codex runs (their sandbox, writable roots and macOS outer sandbox are set
  per `codex exec`; codex-acp is used for detection only).
