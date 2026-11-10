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
- App data lives in `~/.thoughts/`: settings, chat history, attachments and the **app layer**
  (`~/.thoughts/app-layer`). The app layer is a git repo of custom CSS, themes, plugins and views.
- Your **library** is just a folder. It works without the app: open it in any editor or run
  `claude` inside it.
- **Updates** replace the app itself. Each release ships a hash manifest; files you customised are
  compared three ways (original, yours, new), and only real conflicts ask you to migrate, keep or reset.

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
starter/           starter kit for new libraries (embedded)
updates/           update manifests (embedded)
```

## Roadmap

- Codex as a second assistant (switch per chat; history carries over)
- Windows and Linux builds
- Approving tool permissions from inside the chat
- Optional semantic-search extension for very large libraries
- Signed and notarized releases; Intel, Windows and Linux downloads

## License

MIT © Abdulmalik Alzeer
