# Development

## Build from source

For Intel Macs, or if you'd rather build it yourself. The steps below are for macOS, the only
platform the app has been run on (see [RELEASING.md](RELEASING.md#known-unverified)). It takes about 10 minutes the first time,
mostly waiting.

### 1. Requirements

| Tool | Check | Install |
|---|---|---|
| Xcode command-line tools | `xcode-select -p` | `xcode-select --install` |
| git | `git --version` | included with the tools above |
| Node.js 20+ | `node --version` | [nodejs.org](https://nodejs.org) or `brew install node` |
| Rust | `cargo --version` | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh`, then open a new terminal |
| Claude Code | `claude --version` | [claude.com/claude-code](https://claude.com/claude-code) |

Then sign in to Claude Code once:

```bash
claude auth login
```

> Using an API key or another Anthropic-compatible provider (OpenRouter, DeepSeek, a local
> gateway…)? Skip the login and enter the key during onboarding instead (or later under
> Settings › Assistant). A key from Anthropic needs no address and is sent as `ANTHROPIC_API_KEY`;
> for another service enter its address too, and the key is sent there as `ANTHROPIC_AUTH_TOKEN`.
> The address and its key are saved together: an address without a key never runs, because Claude
> Code would send your Claude sign-in to it.

### 2. Get the code

```bash
git clone https://github.com/abdulmalik-alzeer/thoughts.git
cd thoughts
npm install
```

### 3. Build the app

```bash
npx tauri build --bundles app
```

The app is created at `target/release/bundle/macos/Thoughts.app`. Move it to Applications:

```bash
cp -R target/release/bundle/macos/Thoughts.app /Applications/
```

### 4. Open it

Open **Thoughts** from Applications. A build you made on this Mac usually opens straight away. A
copy that was downloaded or passed on is not code-signed yet, so macOS blocks its first launch: on
macOS 15 and later allow it under System Settings › Privacy & Security › **Open Anyway**; on older
versions Control-click the app and choose **Open**. The steps are in the README,
[Opening Thoughts the first time](../README.md#opening-thoughts-the-first-time).

> Just want to try it without installing? Run `npm run tauri dev` from the project folder.

## How it works

```
Thoughts.app
├── UI (Svelte)  ──HTTP/WebSocket──▶  background service (thoughtsd)
│                                     ├── runs `claude -p` sessions (streamed)
│                                     ├── reads/writes your library (markdown + git)
│                                     ├── mini-wraps, Wrap, Reorganize
│                                     └── app layer (git repo of your customisations)
└── Recovery menu (native; customisations can't remove it)
```

- The **background service** owns every AI session, so chats keep running through app restarts
  and updates. It listens only on `127.0.0.1` and needs a random token.
- App data lives in `~/.thoughts/`: settings, chat history, attachments, logs, trust decisions and
  the **app layer** (`~/.thoughts/app-layer`).
- Your **library** is just a folder. It works without the app: open it in any editor or run
  `claude` inside it. New libraries are copied from `starter/` (embedded in the binary):
  `CLAUDE.md` rules, skills in `.claude/skills/`, and profession presets in `presets/` (English)
  and `presets/ar/` (Arabic) that the init skill applies once and then moves to
  `.claude/presets/`. `library.rs` embeds the whole `starter/` folder with `include_dir!` and
  extracts all of it, so a file added under `starter/` is copied into new libraries without a
  code change. `START-HERE.md` and `START-HERE.ar.md` are both copied. When setup runs in the app
  the assistant is told to leave the clearing up to the app (it cannot write under `.claude/`):
  after a run that succeeded, `library::finish_setup` removes the init skill and both welcome
  pages and moves `presets/` to `.claude/presets/`. After a run that failed or was stopped
  nothing is removed, and the setup chat offers to run it again
  (`POST /api/onboarding/setup/:id`). Run in a terminal, the init skill does all of this itself.

### Trust model

- A library the app did not create can carry things that execute: Claude Code hooks and other
  non-allowlisted keys in `.claude/settings*.json`, `.mcp.json`, git hooks and risky repo-local git
  config. `trust.rs` lists these as *findings*; the daemon refuses to run the assistant or git in
  the folder until the user accepts them.
- The refusal is a typed error (`trust::Untrusted`), never a marker in an error's text. The API
  answers 403 with `code: "untrusted"` and a `trust` object: the id of the question the daemon
  holds open, the folder, each finding (`kind`, `file`, `name`, `detail`, `new`), which part
  changed and which files differ. The UI opens the trust dialog for that object only and words
  each `kind` itself (`ui/src/lib/i18n/sec-a.ts`); `file`, `name` and `detail` come from the folder
  and are shown as data. `POST /api/library/trust {id, approval}` answers that one question, and only while
  the folder is still what it listed; there is no call that trusts a path. `approval` comes from the
  desktop shell (`approve_action`, see `consent.rs` in both crates): the app token alone trusts nothing.
  The trust dialog shows what was found; its button opens the shell's own native confirmation,
  which names the folder as the daemon has it (`GET /api/library/trust?id=`) and is the yes. Bypass
  switches have no in-page dialog: the shell's native warning is the confirmation. Script in the
  page cannot answer, word or skip that dialog, and only one is open at a time. The approval is
  signed over what the dialog named (`consent::chat_subject`: the chat and its program;
  `trust_subject`: the question and its folder), and the daemon checks it against the chat or
  question as it is when the approval is used. When the shell cannot learn the subject from the
  daemon it shows no dialog and makes no approval.
- Trust is stored daemon-side in `~/.thoughts/trusted.json`, per folder, in two parts so that a
  change in one is never reported as a change in the other: the *assistant* part (its findings
  plus the bytes of every file under a `.claude` folder that the starter kit did not put there,
  and `.mcp.json`) and the *git* part (risky settings with their values, hook files). Plain git
  settings (where the library syncs to, which branch follows which) belong to neither, so adding a
  backup location never asks. A record written by 0.0.8 (one fingerprint string) is carried over
  when the folder is still exactly what that version trusted.
- `.claude` folders below the top level count too: Claude Code loads their routines as soon as
  the assistant reads or edits a file beside them. Their markdown is checked and their files are
  remembered like the top folder's; a `settings.json` there is not read by Claude Code unless that
  sub-folder is opened as a project, which is checked as a folder of its own.
- The starter's own SessionStart hook is recognised by exact JSON equality with
  `starter/.claude/settings.json`. Changing that file changes what is treated as known-safe.
  The hook pulls from the backup only in a terminal session: every assistant process the app
  starts carries `THOUGHTS_APP_RUN=1` (`claude.rs`), and the hook does nothing when it is set, so
  inside the app only the daemon's own guarded sync touches the repository. The hook as shipped
  up to 0.0.8 had no such test; it is still recognised, and `claude::refresh_starter_hook`
  replaces that one command at the daemon's start and before each run (`start_run`, and
  Reorganize before its working copy is cut). In the library this happens only with
  `library_lock` held across the read, the rewrite and the renewal of trust; while the library
  is busy it waits for a later run. The write goes through `trust::renew_after_app_write`, so a
  folder the user trusted stays trusted only when nothing else trust looks at changed around the
  write; otherwise the usual question appears.
- Markdown under `.claude/` (skills, commands, agents, and the presets once the init skill has
  moved them to `.claude/presets/`) is standing instruction for the assistant, so it is part of
  the check: each `.claude/**/*.md` file is compared byte-for-byte with the starter's version. A
  file that differs or has no starter counterpart is a finding, and its bytes are part of the
  fingerprint. Editing a starter skill therefore changes the baseline for every library. The
  fingerprint is always computed, including when nothing is found.
- No assistant run may write under `.claude/` (`claude::protective_deny`); the one exception is
  the first-time set-up run, which installs the presets. So the tune-up and wrap routines never
  adjust a skill themselves: they describe the change and send the user to Settings →
  Customisations → Routines, where the app writes the routine after the user has reviewed it and
  renews the trust record (`routines.rs`). Every run is told this as well (`claude::managed_note`),
  because libraries made before 0.0.9 carry routines that still say "edit that skill's file".
- Skills and presets are markdown only, with no `allowed-tools` or hooks in skill frontmatter.
  Nothing in the starter adds executable content.
- Reorganize works in a copy of the library; that copy is trust-checked like any other folder and
  is never trusted automatically.

### Deny rules on every run

Every assistant run the daemon starts gets the same generated deny rules through `--settings`:
chat, Wrap, mini-wrap, workflow, Reorganize and customise. They block the assistant's file tools
from writing `.claude/`, `.mcp.json`, `.git/` and the app's data folder (`~/.thoughts`), and from
reading or writing the user's private paths. Customise runs additionally make the library
read-only. Earlier versions applied these rules to customise runs only.

`permissionMode` is allow-listed to `acceptEdits` and `plan`. `default` is still read from old
settings files and treated as `plan`, because there is no approve/deny UI.

### Full access

`settings.fullAccess` (`full_access.rs`) is the owner's switch for "no app-imposed limits". While it
is on, `claude::start_run` gives every kind of run `bypassPermissions` (a run in Plan keeps Plan), no
rules file, no command check (`shell_guard`), no trust check, no strict-private asking and no
standing-file restore, and the `full` profile; Codex runs get the CLI's own
`--dangerously-bypass-approvals-and-sandbox` (found in `codex exec --help`, else the
`danger-full-access` sandbox), no outer sandbox and nothing put back. Runs are told so
(`full_access::run_note`, `staging::system_prompt_full`, `codex::full_instructions`), with the data
folder and the source folder (`fullAccess.sourceDir`). The live layer keeps hand edits as a history
entry instead of setting them aside, validation does not block Keep, add-on requests made with the
app token lose `scope=plugin` (`api::plugin_gate`; access keys never do), and `/api/machine/*`
(`machine.rs`) answers only while it is on and never for `?preview=1`.

Switching it on: `PUT /api/settings?confirm=full-access&approval=…` with the shell's approval for
`consent::Action::FullAccess` (two native dialogs, `src-tauri/src/consent.rs`); the body's
`fullAccess.on` is never read. Off: `?fullAccess=off`, which also stops every run started with it and
every bridge command (`full_access::switched_off`).

### Run profiles

What a run may use besides reading and writing the files of the folders it works in follows from
who is behind it (`claude::profile`). The rules travel in the run's private rules file and hold on
every Claude Code version (a deny rule beats every allow rule and every permission mode, bypass
included). The flags are added only when the installed CLI's `--help` lists them; each falls back
to the rules alone.

| Profile | Runs | Commands | Web | User's own settings (allow rules, hooks, plugins) | Connected tools (MCP) | Standing files | Flags added |
|---|---|---|---|---|---|---|---|
| `chat` | ordinary chat, customise chat | what the mode allows; under strict private folders every command asks (ordinary chat) or is denied (customise chat) | off | kept | kept | may change | ordinary chat: `--permission-prompts host --mcp-config <file> --permission-prompt-tool mcp__thoughtsapp__approve` (see Approvals); customise chat and bypass: `--permission-prompts none` |
| `chat`, web lookups on | an ordinary chat whose switch "Web lookups" is on (never in bypass mode) | denied | allowed: `WebSearch`, `WebFetch` | kept | denied | denied | `--permission-prompts none --strict-mcp-config` |
| `shaping` | first-time set-up, Reorganize | what accept-edits allows on its own (moving, deleting, read-only commands) | denied | not loaded | denied | may change (Reorganize's diff is reviewed) | `--permission-prompts none --setting-sources project,local --strict-mcp-config` |
| `upkeep` | Wrap (preview and apply), adding a feature | denied (`Bash`, `PowerShell`) | denied | not loaded | denied | Wrap: denied, and put back if changed anyway; adding a feature: may change | `--permission-prompts none --setting-sources project,local --restricted --strict-mcp-config` |
| `unattended` | automatic task | denied | denied | not loaded | denied | denied, and put back if changed anyway | as `upkeep`, plus `--max-budget-usd` and the time limits |
| `mado` | a MADO planning step or change request (`mado.rs`) | denied | only when its chat has web lookups on | not loaded, and neither are the project's own settings | denied | denied | `--permission-prompts none --setting-sources "" --restricted --strict-mcp-config`; mode `default` with allow rules for `mado/**` only, deny rules for every other name at the top of the project |
| `build` | one task of a MADO build (`mado_build.rs`), in its own worktree | only the task's approved acceptance and setup commands (allow rules; nobody can be asked) | denied | not loaded, and neither are the project's own settings | denied | denied | `--permission-prompts none --setting-sources "" --strict-mcp-config` |
| one-shot | titles, mini-wraps, routine drafts, the provider test | no tools (`--tools ""`) | no | none: `--safe-mode` (no CLAUDE.md, skills, plugins, hooks, MCP servers) | none | — | `--safe-mode --strict-mcp-config --disable-slash-commands --disallowedTools mcp__* --permission-prompts none` |

- **The rules are the lock that is always there; each flag is a second one.** Denied commands are
  the rules `Bash` and `PowerShell`, denied web tools `WebSearch` and `WebFetch`, denied connected
  tools `mcp__*` — whatever the user's own allow rules say. Each flag is added whenever the profile
  needs it and the CLI lists it, independently of the others (`profile_flags_never_fail_open` tests
  every profile against every mix): `--setting-sources project,local` for every profile without
  the user's own settings (`--setting-sources ""`, no source at all, for `mado` and `build`), `--restricted` on top of it when there are no commands either (it also
  removes every other code-running tool and keeps the file tools inside the run's folders), and
  `--strict-mcp-config` whenever connected tools are denied.
- **Standing files** are what every later session follows: `CLAUDE.md` and `CLAUDE.local.md` in
  any folder, and the checklists in `routines/` (`.claude/`, where the app keeps the routines the
  assistant follows, is closed to every run but first-time set-up). Rules name them as written, in
  lower case and in every letter case at once (`[Cc][Ll][Aa][Uu][Dd][Ee].[Mm][Dd]`, `claude::any_case`):
  rules follow gitignore's syntax, where `[…]` is a character class, and macOS and Windows volumes
  ignore case. The protective `.claude`, `.git` and `.mcp.json` rules and the private-folder rules
  get the same every-case spelling.
- **Backstop for runs nobody watches** (Wrap, automatic task: `Profile::restores_standing`): before
  the run the daemon keeps a copy of every standing file, everything under `.claude/` and
  `.mcp.json` (`claude::StandingSnapshot`; links are never followed, `.git` is not entered). When
  the run ends, however it ends, a changed file gets its old contents back, a removed one comes
  back, a new one goes, and the chat lists them (a `denied` message, item `what: "restored"`).
  Nothing is written through a link. A chat is never second-guessed this way: the user is there,
  and may be editing those files themselves.
- Every run is told not to say how long its own work will take unless the user asks, and then to
  answer for itself and not for a developer (`claude::estimates_note`, repeated in
  `codex::instructions`). Left alone the assistant guesses in a person's time: hours, for work it
  finishes in minutes.
- Runs that may not change the standing files are told so (`claude::standing_rules_note`); a web
  chat is also told that page text is material, never instructions, and never to put library
  content into a search or an address. The Wrap preview fences the mini-wraps between two lines
  carrying a random mark, as material to file; the mini-wrap prompt says the same about the
  chat transcript.
- Switching web lookups on asks first, in plain words: pages can carry instructions, and the
  assistant may send what it read (notes included) to the sites it opens. It is per chat and off
  by default. There is no per-fetch approval: a web chat never asks (below), and in a chat with
  the switch off a web lookup is refused without asking.
- The library's own project settings are kept by `--setting-sources project,local` (the trust
  question covers them); `--restricted` is documented to load only managed settings and
  `--settings`. Nothing in the app's runs depends on project settings.
- A user who set a gateway only in `~/.claude/settings.json` (its `env`) rather than under
  Settings › Assistant gets it in ordinary chats only. Settings › Assistant is the supported way.

**What a run was refused** is read from the `permission_denials` list of the CLI's final `result`
event (`{tool_name, tool_use_id, tool_input}`) and stored as a chat message with role `denied` and
`tool.denials: [{tool, what, target, why}]` (`claude::denials`). It is pushed after the answer and
before a run's error, so an error stays the last message. Refused `ExitPlanMode` and
`AskUserQuestion` calls are left out: nobody could have answered them. The chat words each item
itself and offers "Allow web lookups in this chat" when a web lookup was refused because the
switch is off.

### Approvals in a chat

`claude -p` has nobody to ask, so an ordinary chat's run names an MCP tool that asks for it
(`approvals.rs`): `--permission-prompt-tool mcp__thoughtsapp__approve`, with `--permission-prompts
host` in place of `none` and `--mcp-config` pointing at a 0600 file in `run-tmp`. That file names
the daemon's own endpoint, `POST /mcp/approvals` on its port (streamable HTTP, one JSON-RPC
message per request: `initialize`, `ping`, `tools/list`, `tools/call`; `GET` is 405), with a
bearer token made for that one run, and a per-server `timeout` above the wait (Claude Code gives
an HTTP server's request 60 seconds and a silent call 5 minutes otherwise). The endpoint sits
outside the API's token check, answers only a token whose run is going, and refuses any request
with an `Origin`.

- **Who asks:** `kind == "chat"`, not bypass, web lookups off, and a CLI whose `--help` shows
  `--permission-prompts host` naming `--permission-prompt-tool` (2.1.259 and later; the flag itself
  is hidden). Everything else keeps `--permission-prompts none`: Wrap, automatic tasks, adding a
  feature, set-up, Reorganize, customise chats, one-shot answers, web chats, bypass chats.
- **The request** (`{tool_name, input, tool_use_id}`) becomes a chat message with role `approval`
  and `tool: {code: "approval", status, card: {tool, what, target, why, mode, strict, private,
  always, detail, toolUseId}, expiresAt}`. The `tools/call` answer is an SSE stream (a comment
  line every 15 s, then the result) or plain JSON when the client does not take a stream. The
  result is one text block, `{"behavior":"allow"}` or `{"behavior":"deny","message":…}`, with no
  `structuredContent` — Claude Code's documented contract.
- **Answers:** `POST /api/chats/:id/approvals/:msgId {answer: once|always|deny}`. Unanswered for
  5 minutes (`approvals::WAIT`): refused, status `timeout`. The run ending (Stop, an error, the
  daemon stopping) or the program dropping the call: refused, status `ended`. The status is
  written into the message and sent again as `chat.message` with the same id.
- **Decided without a card** (`approvals::decide_now`): the app's own tool (the model can see it
  and is told not to call it), `AskUserQuestion` and `ExitPlanMode`, `WebFetch`/`WebSearch` (the
  switch is off), a file whose real path is in a private folder (`paths::is_private`, links
  resolved), a tool use already asked about or already answered, and what the chat allows always.
- **Always for this chat** is kept by the daemon in `chat-approvals.json`, never in Claude Code's
  settings: this exact command (and tool), changes to this exact file, reading this exact path,
  or this connected tool with any input. Deny rules still come first. `GET /api/chats/:id/approvals`
  lists them, `DELETE /api/chats/:id/always/:id` takes one back, deleting the chat forgets them.
  Under strict private folders a command is offered once only and the "always" list is not
  consulted for commands.
- **Characters that do not show** (the add-on check's list, `staging::validate::is_hidden_char`,
  plus control characters, here with no exceptions: bidi controls, zero-width characters, tag
  characters, variation selectors…) are written out as `[U+202E]` in the card's `target`, `tool`
  and `detail`, and the card carries `hidden: true` (`approvals::hidden_in`, `approvals::reveal`).
  Such a request is never offered "Always"; the card warns, puts Deny first and focuses it. The UI
  checks the same list again. The target is never cut; a long one scrolls and the card says so.
- Refusals the user gave on a card are left out of the `denied` list under the answer.
- `approval.asked {chatId, id, title}` and `approval.changed {chatId}` go out on the event stream
  (not to add-ons); the window shows a toast when the user is elsewhere, and a notification when
  it is in the background.

**Strict private folders** (`settings.strictPrivate`, off by default; a save that leaves it out
keeps it): while there are private folders, every chat that is not in bypass mode loses its own
commands (`approvals::strict_applies`, `Profile::strict`). An ordinary chat that asks gets the
rules `ask: ["Bash", "PowerShell"]` — an ask rule beats the user's allow rules and the built-in
read-only commands — so every command comes to a card with the private-folder warning, offered
once only. A customise chat, a web chat, or a chat whose CLI cannot ask gets `deny: ["Bash",
"PowerShell"]`, and a refusal names the reason (`why: "strict"`). Runs the app starts by itself
are unchanged: Wrap, automatic tasks and adding a feature have no commands anyway; set-up and
Reorganize keep theirs.

### Codex (the second assistant)

Settings › Assistant names the program that answers new chats and everything the app starts
itself (`ai.primary`: `claude` or `codex`); each chat keeps its own (`Chat.provider`) and can be
moved (`handoff.rs`). `claude::start_run` decides everything about a run first — trust, the
permission mode, bypass only for ordinary chats, the profile, the standing-file backstop — and then
hands Codex runs to `codex::launch` (`assistant::for_run`: a chat uses its own program; an app run
the one in Settings, recorded on its chat; a Wrap's saving step the one that made its preview).
One-shot answers go through `claude::complete` / `complete_cheap`, which call `codex::complete`
when Codex is chosen.

- **Discovery, never a list.** Sandbox modes from `codex exec --help` (`--sandbox`'s possible
  values); options (`--json`, `--image`, `--skip-git-repo-check`, the `resume` sub-command) from
  the same help; reasoning efforts from Codex's own refusal of a value it does not know (one start
  with an empty, private `CODEX_HOME`, no key and every address pointed at `127.0.0.1:9`, so it
  can cost nothing and reach no one); models from what runs report (`codex-seen.json`).
- **Modes.** The app's mode maps onto the sandbox (`codex::sandbox_for`): Plan → `read-only`,
  Accept edits → `workspace-write`, Bypass → `danger-full-access` (ordinary chats only, after the
  confirmed warning; `check_bypass` and `start_run` refuse it otherwise). App runs get the least
  that does their job (`run_sandbox`): a Wrap preview is read-only, its saving step, set-up,
  Reorganize, adding a feature and automatic tasks write in their own folder.
- **The command line** (`codex::exec_args`, from the run's profile through `codex::switches`):
  `exec [--image …] --json --skip-git-repo-check --sandbox <mode> -c history.persistence="none"
  [-c model=…] [-c model_reasoning_effort=…] [-c tools.web_search=true] … -`, the prompt on stdin.
  `-c mcp_servers={}` whenever the profile has no connected tools, `-c notify=[]` without the
  user's own set-up, `-c sandbox_workspace_write.network_access=false` unless the profile has both
  the web and the user's set-up. A profile with the web but no commands (a web chat) runs
  read-only: Codex cannot switch commands off. A project chat may also write the library
  (`sandbox_workspace_write.writable_roots`). Strict private folders refuse to run a chat on Codex
  (`codex.strict`), and Codex runs never get the approval prompt tool.
- **What a run is told** (`codex::instructions`, in front of the prompt, never on the command
  line): where the standing rules (CLAUDE.md) and routines (`.claude/skills`) are, the private
  folders by name with the plain fact that Codex cannot close them, the app's data folder, and the
  same standing-rule and web notes as Claude Code runs, worded for what Codex can enforce, and the
  same rule about time estimates.
- **Output.** Both formats are read (`codex::parse_line`): the 0.2x event stream (`{"id","msg"}`,
  after a settings line; no answer word by word, no session id, no final result line) and the later
  one (`thread.started`, `item.*`, `turn.completed`). Commands, file changes, connected tools, web
  searches and plans become the chat's `tool` / `tool_result` messages; a command the sandbox
  refused is listed as `denied` (`why: "codexSandbox"`); tokens are added to `Chat.usage_tokens`
  (Codex reports no cost; `meter::record_codex` estimates one from `prices.rs` and adds it to the month's figure with `add_usage`, never to `Chat.cost_usd`). Errors are told apart (`codex::problem`):
  signed out (`signedOut`, `provider: "codex"`), the plan's limit (`codexLimit`), offline
  (`network`), anything else in Codex's words.
- **Sessions.** A later Codex prints its session id and has `codex exec resume <id>`. Codex 0.2x
  prints neither: the session is found after the run by a mark in its prompt
  (`codex::find_session`, in `$CODEX_HOME/sessions`), and continued with
  `-c experimental_resume="<its file>"`. A session Codex no longer has is reported as
  `sessionLost`, and the chat carries on from its saved conversation. Deleting a chat deletes its
  session files by exact name; one-shot answers delete theirs at once.
- **What Codex cannot enforce, and what stands in for it.** It has no rule that closes one file or
  folder: private folders, the app's data folder and other chats' attachments can be read (the UI
  says so in every Codex chat while private folders exist, and in Settings). `.claude/` and
  `.codex/` in any folder and `.mcp.json` (and, where the run may not change them, the standing
  files: CLAUDE.md, the `routines/` checklists, and AGENTS.md / AGENTS.override.md in any folder, all
  in any letter case) are copied before every run that can write and put back afterwards
  (`codex::keep_for`, `StandingSnapshot::take_guarded`). Links under any of those names (a file's,
  or a kept folder's such as `.Codex`) are recorded by where they point and never followed; a new or
  changed one goes when the run is over and the chat says so — also in an ordinary chat, which may
  edit AGENTS.md but never put a link in its place. So does any new or changed link to a folder (or
  to nothing yet), whatever its name: the folder may hold such files. The scan looks through the
  whole tree (links are never followed, `.git` is not entered); there is no cut-off a run could hide
  a kept name behind. The Claude Code backstop does the same. Codex itself keeps a
  writable folder's top-level `.git` read-only and gives sandboxed commands no network (checked
  with `codex debug seatbelt`, which runs no model). No per-run spending ceiling: automatic tasks on Codex
  have only their time limits, though their estimated cost counts toward the month's limit.
- **Moving a chat** (`PATCH /api/chats/:id {provider}`, `handoff.rs`): the old session stays
  behind (kept in `session_ids`), the model and effort become the new program's own, the mode
  stays — except Bypass, which goes back to Accept edits (the chat says so; switching it on again
  takes the new program's own confirmed warning). The next message carries the whole transcript when it fits half of the new model's
  window (as the program reported it, else 128,000 tokens; three bytes a token), otherwise a summary
  the new program writes (kept on the chat) plus the last messages. The chat says which at the
  move, and says so again if the summary could not be made.

### Library busy and history

- While a Wrap or a Reorganize apply holds the library, every library mutation (file writes, todo
  operations, import, new/move/delete, restore), sync, and chat sends that work in the library
  return `BUSY::<message>`. While a Wrap is `interrupted`, sync and new Wraps return
  `WRAP_INTERRUPTED::<chatId>` until the user finishes or undoes it. Sync is also refused while a
  chat or workflow run is active in the library.
- **Library history** is git-backed. `GET /api/library/history` lists changes with a `kind`
  (`wrap`, `sync`, `edit`, `assistant`, `other`); `GET /api/library/version` returns one file at
  one change; `POST /api/library/restore` puts one file back and `POST /api/library/undo` reverts
  one whole change. Both create a new commit; undo returns `CONFLICT::<files>` when it cannot.
  With git off they return `NO_HISTORY::<message>`. User-facing description:
  [HISTORY.md](HISTORY.md).
- Git calls have a timeout, so a stalled call no longer locks the library until restart.

### Private paths

- `privatePaths` in settings is a list of library-relative folders the assistant must never read.
  The daemon enforces it when it starts any run (deny rules passed to the assistant, and the
  list stated in the appended system prompt, `claude::private_note`); the starter `CLAUDE.md`
  repeats it as a behavioural rule. The system prompt goes through `--append-system-prompt-file`
  (a 0600 file in `~/.thoughts/run-tmp`) whenever the CLI takes it, because every local account
  can read a process's command line; on a CLI without it the folders are not named there.
- Rules are written for the folder name as configured, in lower case and as spelled on disk.
  Pattern characters in a path (`*`, `?`, `[`, `]`) are escaped the way Claude Code escapes paths
  itself, and a path holding them also gets a second rule with each replaced by `?`.
- **Plugin scope is enforced by the daemon, not the UI.** The UI host adds `scope=plugin` to every
  library call it makes for a plugin or view. With that scope the daemon checks private folders
  itself, case-insensitively and on canonical paths, and for writes requires the path to fall
  under the `allow=<prefix>,…` list, which the host fills from the plugin's declared `write`
  folders. Writes are limited to `.md`, `.json`, `.csv` and `.txt`.
- The deny rules cover the assistant's file tools. They are not a sandbox: a shell command the
  assistant is permitted to run is not filtered by them (read-only commands such as `grep` and
  `cat` run without asking in every mode). Runs the app starts by itself have no commands at all
  (see Run profiles); ordinary chats keep them, except while web lookups are on or strict private
  folders are switched on (every command then asks first, see Approvals in a chat). PRIVACY.md
  says this in plain words; keep the two in step.
- Private folders are still part of the library on disk and are still synced.
- The file list and search leave a private folder out by where it really is (`library::Hidden`),
  not only by its name. What `INDEX.md` already says about its items is not covered by the deny
  rules: Settings offers to remove those lines when a folder is marked private
  (`/api/library/private-index`).

### App layer and updates

- The app layer is a git repo of the user's customisations (CSS, themes, plugins, views,
  workflows, and the declarative `home.json`, `nav.json` and `todos.json`). A requested change is
  prepared in a staging worktree, shown as a review with a plain-language summary, and committed
  only when the user keeps it. Every applied change can be undone on its own (`revert`,
  `undo-last`) or the whole layer rolled back; the native Recovery menu sits outside the layer
  and cannot be removed by it.
- **Built-in pages reference.** `ui/src/lib/layer/builtin-pages.json` is the one list of what a
  customisation may change on the app's own pages (each page's sections, parts, texts, whether it
  can be replaced), the sidebar ids and icons, the permissions and every Plugin API call with the
  permission it needs. The UI reads it (`lib/pages.svelte.ts`, `lib/navLayout.ts`, `lib/layer.ts`),
  the daemon validates against it (`staging/pages.rs`, `staging/validate.rs`) and fills the
  "Built-in pages reference" of the layer's README from it (`{{BUILTIN_PAGES}}` in
  `assets/LAYER.md`). Tests hold it to the code: `ui/src/lib/reference.test.ts` (each page draws
  exactly its sections, every icon and API call exists) and `section_ids_survive_releases` in
  `pages.rs` (an id may only go away through `renamed`). A page's sections are drawn by
  `views/Sections.svelte` from snippets, with `data-slot="<page>.<id>"` on each block.
- **Per-plugin manifests.** Each plugin or view declares itself in a file beside it,
  `plugins/<name>.json` or `views/<name>.json`:
  `{"apiVersion":1,"permissions":[…],"write":["folder/"],"description":"…"}`. Entries in the
  shared `manifest.json` remain a fallback. Separate files mean undoing one plugin's change no
  longer conflicts with another's. A plugin, view or pack declaring an `apiVersion` above 1 is
  listed as `unsupported` and not loaded.
- **Validation before review.** Staging validates every JSON file, manifest, workflow and path;
  problems come back on the pending object as `problems:[{path, message}]` and Keep is disabled
  while any exist. The UI also dry-loads each staged plugin in a hidden sandbox frame.
- **Apply signature.** `GET /api/layer/pending` returns a `signature`, a hash of the staged
  content. `POST /api/layer/pending/apply {signature}` fails with `CHANGED::` when the staged
  content no longer matches what the user reviewed.
- **Packs.** A full pack carries the whole layer; a feature pack (`export?paths=…`) carries a
  subset and is merged on import: files added, `home.json`/`nav.json`/`todos.json` merged
  key-wise, CSS placed in a marked `/* pack:<name> */` block. Any import is staged and reviewed,
  never applied directly.
- **Workflows** run as chats of kind `workflow`, one at a time, never during a Wrap, only after
  the user has approved each one. Schedules: day, week, weekdays, month (day 1–28 or last).
- **Learning** is local only: usage events and keep/discard/undo outcomes in
  `~/.thoughts/usage-events.jsonl`, used for suggestions and the monthly tune-up.
- **Updates** replace the app itself and never touch the library. A build that carries the
  updater's public key (`src-tauri/updater.pub`, see RELEASING.md) looks for a new version when
  the user asks, or by itself while "Check for new versions automatically" is on
  (`startAutoUpdateChecks` in `ui/src/lib/updates.ts`; off unless switched on), and installs it
  only when the user agrees. It never installs silently. `find_update`
  (`src-tauri/src/main.rs`) asks the public release's `latest.json`
  (`https://github.com/<owner>/<repo>/releases/latest/download/latest.json`), with no sign-in,
  and the updater library checks the signature of what it downloads against the build's key
  before installing it. "Not found" there means nothing is published yet (`unpublished`). Copies
  0.0.12 to 0.7.0 also had a GitHub fallback through the `gh` sign-in for the time the
  repository was private (`thoughts updates gh on`); it was removed in 0.7.1, and
  `thoughts updates …` now only says that no step is needed. A build without the key only
  points to the download page (`release.rs`). Each release ships a hash manifest;
  customised files are compared three ways (original, yours, new). Only files changed on both
  sides ask the user: keep mine, use new, or let the assistant merge (previewed like any change).
  Defaults removed or renamed upstream are reported and left in place, an unresolved conflict is
  never overwritten by a later update, and a downgrade changes nothing.

### User-facing docs

[PRIVACY.md](PRIVACY.md), [CUSTOMISING.md](CUSTOMISING.md), [HISTORY.md](HISTORY.md) and
[COST.md](COST.md) describe this behaviour for end users, with Arabic versions in `docs/ar/`.
Keep both languages true when the behaviour changes. [GLOSSARY.md](GLOSSARY.md) gives every
product term one name in each language (Wrap «تلخيص وحفظ», History «السجل», Try it · Keep ·
Discard «تجربة · اعتماد · تجاهل», automatic task «مهمة تلقائية», the app always "Thoughts"…)
and the writing rules; the UI strings, the service's messages (`tr.rs`), the native menus, the
starter kit and these guides follow it. Errors the service raises for a person go through
`tr::err`, or `tr::err_detail` when git's or a program's own words should stay available under
"Show details".

The first-run test plan for each platform and the list of things nobody has verified yet are in
[RELEASING.md](RELEASING.md#first-run-test-plan).

## Uninstall

```bash
rm -rf /Applications/Thoughts.app ~/.thoughts ~/Library/LaunchAgents/app.thoughts.daemon.plist
```

That is the macOS form: the app, the data folder and the login item (LaunchAgent). Your library
folder is never touched. Delete it yourself if you want it gone. Saved credentials are under
"thoughts-git" (sync passwords, one per backup address) and "thoughts-ai" (the AI provider key)
in Keychain Access.

What removes them for you:

- **Settings → Maintenance → Remove Thoughts from this computer** (every platform): stops the
  background service, removes the login item and the saved passwords and key, and optionally the
  data folder. It deletes a fixed list of the app's own files and folders inside the data folder,
  never the library.
- `sh scripts/uninstall-macos.sh --purge` / `uninstall-linux.sh --purge`: the same from a
  terminal, plus both keychain services ("thoughts-git" and "thoughts-ai"). Without `--purge` they
  only stop the service and remove the login item. The Linux script uses `secret-tool` when it is
  installed (untested).
- The Windows uninstaller removes the login item and, when you tick "Delete the application
  data", `%USERPROFILE%\.thoughts`. It does not remove saved passwords from Windows Credential
  Manager (entries named `thoughts-git` and `thoughts-ai`); use the in-app removal first.

## Commands & layout

```bash
npm run tauri dev          # app + hot-reloading UI
cargo test -p thoughtsd    # daemon tests
npx vite build             # UI build check
```

A development build of the daemon also answers pages served from `http://localhost:<port>` (the
Vite dev server, a browser preview). A release build answers the app's own window only; set
`THOUGHTS_DEV_ORIGINS=1` when starting one by hand to preview the UI in a browser against it.
The events WebSocket takes the token from its offered sub-protocols
(`thoughts.events, thoughts.token.<token>`), never from the address.

```
src-tauri/         Tauri shell — one binary: GUI · --daemon · git credential helper
crates/thoughtsd/  the daemon (axum): chats, Claude runner, library, todos, git, wraps, app layer
ui/                Svelte 5 frontend
starter/           starter kit for new libraries (embedded): rules, skills, profession presets (en + ar)
docs/              user docs (English, Arabic in docs/ar/), this file, RELEASING.md, the review
```

## Roadmap

- Signed and notarized releases; Intel, Windows and Linux downloads (built in CI, never run)
- Approving tool permissions from inside the chat (until then "ask before changes" is not offered;
  what was refused is listed under the answer)
- Reminders when the app is not running
- Previews for PDF, Word and Excel files
- Optional semantic-search extension for very large libraries

## License

MIT © Abdulmalik Alzeer
