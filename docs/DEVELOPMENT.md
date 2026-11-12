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

Open **Thoughts** from Applications. The build isn't code-signed yet, so macOS may block the first
launch. If it does, right-click the app, choose **Open**, then confirm.

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
  and are shown as data. `POST /api/library/trust {id}` answers that one question, and only while
  the folder is still what it listed; there is no call that trusts a path.
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

### Run profiles

What a run may use besides reading and writing the files of the folders it works in follows from
who is behind it (`claude::profile`). The rules travel in the run's private rules file and hold on
every Claude Code version (a deny rule beats every allow rule and every permission mode, bypass
included). The flags are added only when the installed CLI's `--help` lists them; each falls back
to the rules alone.

| Profile | Runs | Commands | Web | User's own settings (allow rules, hooks, plugins) | Connected tools (MCP) | Standing files | Flags added |
|---|---|---|---|---|---|---|---|
| `chat` | ordinary chat, customise chat | what the mode allows | off | kept | kept | may change | `--permission-prompts none` |
| `chat`, web lookups on | an ordinary chat whose switch "Web lookups" is on (never in bypass mode) | denied | allowed: `WebSearch`, `WebFetch` | kept | denied | denied | `--permission-prompts none --strict-mcp-config` |
| `shaping` | first-time set-up, Reorganize | what accept-edits allows on its own (moving, deleting, read-only commands) | denied | not loaded | denied | may change (Reorganize's diff is reviewed) | `--permission-prompts none --setting-sources project,local --strict-mcp-config` |
| `upkeep` | Wrap (preview and apply), adding a feature | denied (`Bash`, `PowerShell`) | denied | not loaded | denied | Wrap: denied, and put back if changed anyway; adding a feature: may change | `--permission-prompts none --setting-sources project,local --restricted --strict-mcp-config` |
| `unattended` | automatic task | denied | denied | not loaded | denied | denied, and put back if changed anyway | as `upkeep`, plus `--max-budget-usd` and the time limits |
| one-shot | titles, mini-wraps, routine drafts, the provider test | no tools (`--tools ""`) | no | none: `--safe-mode` (no CLAUDE.md, skills, plugins, hooks, MCP servers) | none | — | `--safe-mode --strict-mcp-config --disable-slash-commands --disallowedTools mcp__* --permission-prompts none` |

- **The rules are the lock that is always there; each flag is a second one.** Denied commands are
  the rules `Bash` and `PowerShell`, denied web tools `WebSearch` and `WebFetch`, denied connected
  tools `mcp__*` — whatever the user's own allow rules say. Each flag is added whenever the profile
  needs it and the CLI lists it, independently of the others (`profile_flags_never_fail_open` tests
  every profile against every mix): `--setting-sources project,local` for every profile without
  the user's own settings, `--restricted` on top of it when there are no commands either (it also
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
- Runs that may not change the standing files are told so (`claude::standing_rules_note`); a web
  chat is also told that page text is material, never instructions, and never to put library
  content into a search or an address. The Wrap preview fences the mini-wraps between two lines
  carrying a random mark, as material to file; the mini-wrap prompt says the same about the
  chat transcript.
- Switching web lookups on asks first, in plain words: pages can carry instructions, and the
  assistant may send what it read (notes included) to the sites it opens. It is per chat and off
  by default; the CLI cannot ask per page in a `-p` run, so there is no per-fetch approval.
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
  (see Run profiles); ordinary chats keep them, except while web lookups are on. PRIVACY.md says
  this in plain words; keep the two in step.
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
- **Updates** replace the app itself and never touch the library. The app checks for a new
  version and tells the user; it does not install silently. Each release ships a hash manifest;
  customised files are compared three ways (original, yours, new). Only files changed on both
  sides ask the user: keep mine, use new, or let the assistant merge (previewed like any change).
  Defaults removed or renamed upstream are reported and left in place, an unresolved conflict is
  never overwritten by a later update, and a downgrade changes nothing.

### User-facing docs

[PRIVACY.md](PRIVACY.md), [CUSTOMISING.md](CUSTOMISING.md), [HISTORY.md](HISTORY.md) and
[COST.md](COST.md) describe this behaviour for end users, with Arabic versions in `docs/ar/`.
Keep both languages true when the behaviour changes. The Arabic glossary (Wrap «تلخيص وحفظ»,
History «السجل», Keep / Discard / Try it «اعتماد / تجاهل / تجربة», add-on «إضافة», pack «حزمة»,
automatic task «مهمة تلقائية») must match the UI strings.

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
- Codex as a second assistant (switch per chat; history carries over)
- Optional semantic-search extension for very large libraries

## License

MIT © Abdulmalik Alzeer
