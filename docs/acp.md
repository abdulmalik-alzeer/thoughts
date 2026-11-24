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
  branch is `session/fork`. Providers: `ANTHROPIC_*` env for Claude Code. The turn's tokens come
  from the answer's `usage` (with the cost from `usage_update`) for the usage page.
- **Codex chats** (kind `chat`, `codex_acp.rs`, round 0.7.0): `codex::launch` decides everything as
  for `codex exec` (it builds the same command line), then hands the turn to codex-acp when every
  guard travels (table below); otherwise, or when the adapter fails before the turn is sent, the
  same turn goes on through `codex exec` and the log says why (`codex_acp::Stay`). The adapter
  starts `codex app-server`, so it is only used with a Codex at least as new as its own
  `@openai/codex` dependency (read from its `package.json`, `codex_acp::required_codex`) that lists
  `app-server` (`codex_fits`; detection is gated the same way: an older Codex takes the word for a
  prompt and starts its terminal screen). `GET /api/ai/codex/options` says `acpChats: {on, why,
  need, have}`; Settings shows why when the user can act on it. `ai.classicChats` covers both
  programs. Updates are shown as `codex exec`'s events (`codex_acp::View` → `codex::handle`), with
  the text streaming in; usage is the answer's `usage` (`meter_of` → `meter::record_codex`). Resume
  is `session/resume` of the stored id (the thread id `codex exec` stores too); a branch of a Codex
  chat replays its history as before (`branch::fork_from` forks Claude Code sessions only).

| Guard on `codex exec` | Over ACP |
|---|---|
| Trust, strict private folders, plain-password file, customise never on Codex, bypass only for chats, sign-in, access keys, subscription acknowledgement | Unchanged: checked before the way of running is chosen (`claude::start_run`, `codex::launch`, the API) |
| Sandbox (`read-only` / `workspace-write` / `danger-full-access`, web lookups read only) | The adapter's mode of the same name (`agent-full-access` for no sandbox), from the start (`INITIAL_AGENT_MODE`) and set over ACP; a mode not offered → `codex exec`; a switch mid-turn to one doing more (incl. `agent`, where a model approves) ends the turn. Tested |
| Writable roots (`sandbox_workspace_write.writable_roots`) | The session's `additionalDirectories`, which the adapter adds to every turn's sandbox. Tested |
| `-c` settings: no MCP servers / notifier where the profile says so, network off for commands, web search, `history.persistence=none`, analytics off, model, effort | The same keys, dotted, as the session config (`CODEX_CONFIG`, `session_config` reads them off the `codex exec` arguments); the adapter's per-turn sandbox has the network off in any case. Tested |
| Never asks, never leaves its sandbox | Every permission request (they are all about leaving the sandbox) is refused from an allowlist of plain noes (`decline`, `reject_permissions`, `cancel`) and listed under the answer as refused by the sandbox. Tested |
| macOS outer sandbox around a Bypass run (`outer_sandbox`) | No checked equivalent around node + `codex app-server`: such a turn stays on `codex exec`. Tested |
| The folder's own Codex settings are not loaded | The adapter marks the session's folders trusted, so Codex would load a `.codex/config.toml` there or above: such a turn stays on `codex exec` (`own_settings`). Tested |
| Another provider: address via `-c`, key only in `THOUGHTS_PROVIDER_KEY`, inherited OpenAI keys removed | The provider table in the session config (no key), the key in the environment only (`adapter_command`, `providers::apply_codex_env`). Tested |
| The app's notes in front of the prompt on stdin | The first text block of the prompt. Tested |
| Kept files put back afterwards (`keep_for`, snapshots) | The same snapshots, taken before, put back by the same end (`codex::after_run`) |
| Pictures (`--image`) | Image blocks, where the adapter says it takes them; else `codex exec`. Tested |
| Spending: no per-run limit on Codex (said in the log), the month's figure from what it used | The same: the answer's tokens priced and added (`record_codex`). Tested |
| Stop, one run per chat, full access switched off while starting | `session/cancel`, then the adapter's process group is ended; the same switch and checks. Tested |
| Full access yields everything | The same: no snapshots or outer sandbox, the folder-settings check and the mode check yield, Bypass is `agent-full-access` |

- **Still on the command line**: everything the app starts itself (wrap, reorganize, set-up,
  modules, workflows, customise), MADO planning and build runs (their `--setting-sources ""`,
  `--restricted` and allow-only confinement have no checked equivalent over ACP yet), one-shot
  completions, Codex chats in Bypass on macOS or in a folder with its own Codex settings, and Codex
  chats with a Codex too old for the adapter (0.27.0, for one, has no `app-server`).
