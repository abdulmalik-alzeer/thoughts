# Development

## Build from source

For Intel Macs, or if you'd rather build it yourself. It takes about 10 minutes the first time,
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
> gateway…)? Skip the login and enter the base URL and token during onboarding instead.

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
  `CLAUDE.md` rules, skills in `.claude/skills/`, and profession presets in `presets/` that the
  init skill applies once and then moves to `.claude/presets/`.

### Trust model

- A library the app did not create can carry things that execute: Claude Code hooks and other
  non-allowlisted keys in `.claude/settings*.json`, `.mcp.json`, git hooks and risky repo-local git
  config. `trust.rs` lists these as *findings*; the daemon refuses to run the assistant or git in
  the folder (`UNTRUSTED::<path>::<findings>`) until the user accepts them.
- Trust is stored daemon-side in `~/.thoughts/trusted.json`, per folder and per fingerprint (the
  findings plus the bytes of every file they can come from), so any later change asks again.
- The starter's own SessionStart hook is recognised by exact JSON equality with
  `starter/.claude/settings.json`. Changing that file changes what is treated as known-safe.
- Skills and presets are markdown only. Nothing in the starter adds executable content.

### Private paths

- `privatePaths` in settings is a list of library-relative folders the assistant must never read.
  The daemon enforces it when it starts a run (read-deny rules passed to the assistant, and the
  list stated in the system prompt); the starter `CLAUDE.md` repeats it as a behavioural rule.
- Private folders are still part of the library on disk and are still synced.

### App layer and updates

- The app layer is a git repo of the user's customisations (CSS, themes, plugins, views).
  A requested change is staged, shown as a preview with a plain-language summary, and committed
  only when the user keeps it. Every applied change can be rolled back; the native Recovery menu
  sits outside the layer and cannot be removed by it.
- Customisations can be exported and imported as a single pack file; an imported pack goes
  through the same preview.
- **Updates** replace the app itself and never touch the library. The app checks for a new
  version and tells the user; it does not install silently. Each release ships a hash manifest;
  customised files are compared three ways (original, yours, new). Only files changed on both
  sides ask the user: keep mine, use new, or let the assistant merge (previewed like any change).

### User-facing docs

[PRIVACY.md](PRIVACY.md) and [CUSTOMISING.md](CUSTOMISING.md) describe this behaviour for end
users. Keep them true when the behaviour changes.

## Uninstall

```bash
rm -rf /Applications/Thoughts.app ~/.thoughts ~/Library/LaunchAgents/app.thoughts.daemon.plist
```

Your library folder is never touched. Delete it yourself if you want it gone. Saved git
credentials are under "thoughts-git" in Keychain Access.

## Commands & layout

```bash
npm run tauri dev          # app + hot-reloading UI
cargo test -p thoughtsd    # daemon tests
npx vite build             # UI build check
```

```
src-tauri/         Tauri shell — one binary: GUI · --daemon · git credential helper
crates/thoughtsd/  the daemon (axum): chats, Claude runner, library, todos, git, wraps, app layer
ui/                Svelte 5 frontend
starter/           starter kit for new libraries (embedded): rules, skills, profession presets
updates/           update manifests (embedded)
```

## Roadmap

- Professional setup end to end: onboarding asks profession and working language and passes
  them to the init skill (presets exist in `starter/presets/`)
- Due dates, meeting notes and outputs as first-class views
- Signed and notarized releases; Intel, Windows and Linux downloads
- Approving tool permissions from inside the chat
- Codex as a second assistant (switch per chat; history carries over)
- Optional semantic-search extension for very large libraries

## License

MIT © Abdulmalik Alzeer
