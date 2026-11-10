# Independent review of v0.0.2

Eight reviewers, each with a different role and no knowledge of the earlier review, read the code at
tag `v0.0.2`. This file records their scores and findings. Line numbers refer to that tag.

## Scores

| Reviewer | Score |
|---|---|
| Security | Private beta 7.5/10 · Public release 5/10 |
| QA and reliability | Private beta 6.5/10 · Public release 4/10 |
| Design | Usability 7 · Visual polish 7.5 · Accessibility 6.5 · Consistency 7 |
| Arabic and accessibility | Completeness 8 · Quality 8 · RTL 8.5 · Accessibility 7 |
| Product vision | About 40% of the thesis is real (trust machinery ~85%, expressiveness ~35%, learning ~10%, ecosystem ~15%) |
| Everyday professionals | Consultant 5 · Teacher 4 · Accountant 3.5 · Lawyer 3 |
| Market | Launch readiness 4/10 (product 7, distribution 2) |
| Release engineering | macOS 7 · Windows 4 · Linux 4 · Release pipeline 5 |

## Fix first (bugs and unsafe gaps)

1. **A failed or stopped Wrap can erase edits made while it ran.** Library writes, todo edits and new
   chats don't check whether a wrap holds the library (`wraps.rs:207-215`, `api.rs:613,659,742`).
2. **Sync after an interrupted Wrap pushes the half-applied wrap and removes its undo** (reproduced
   against a test daemon).
3. **Reorganize auto-trusts its working copy**, so a library whose committed `.claude/settings.json`
   differs from the one on disk can run hooks without a prompt (`wraps.rs:401-406`).
4. **Only customise runs get the deny rules** for `.claude`, `.mcp.json`, `.git` and the app's data
   folder; chat, wrap, workflow and reorganize runs don't (`staging.rs:76-101` vs `api.rs:498,537`).
5. **The trust check ignores `.claude/**/*.md`** (skills, commands, agents) and skips the fingerprint
   when nothing is found (`trust.rs:28,42,117-130,168`).
6. **Private folders can be read by a plugin** using different letter case on macOS; the check is in
   the UI only (`layer.ts:203-208`, `api.rs:585-587`). `docs/PRIVACY.md` says "blocked".
7. **Print from the command palette prints a blank page** (`App.svelte:259`, `Docs.svelte:496`).
8. **The High contrast theme has invisible badges** (accent text on an accent background,
   `theme.css:62,158`).
9. **Native "Undo last app change" does nothing** (`main.rs:535`, `applayer.rs:46-48`).
10. **A stalled git call locks the library until restart** (no timeout, `git.rs:33`).
11. **A damaged settings file makes the app look like a fresh install**; a newer-schema file is
    silently rewritten.

## Promises the app makes but doesn't keep

- "Ask before changes" is offered in Chat and Settings, but there is no approve/deny UI; changes are
  skipped.
- "Every change is kept in the history and can be undone": History only covers customisations. There
  is no in-app history or restore for notes.
- "Remind me": todos have due dates and filters, but nothing notifies.
- `docs/PRIVACY.md` says passwords live in the Keychain; the "file" option stores them in a plain
  file readable only by the user.

## The vision ("software that grows with you")

Strong: staging and review, per-change undo, sandboxed plugins with permissions, workflow approval,
failure isolation, and the library-side `tune-up` and wrap routines.

Missing:
- Plugins cannot save anything: no storage, no library write, no todo update. An intake form, a
  kanban board you can drag, or billable-hours tracking are awkward or impossible.
- Built-in views are closed: todo grouping, sidebar items and widget placement can't be changed.
- `apiVersion` is never checked; nothing validates a plugin before review.
- Learning is four fixed rules on a handful of signals; keep/discard outcomes aren't recorded.
- The three-way update covers five default files; removed or renamed defaults and API drift under
  user plugins aren't handled.
- Packs are whole-layer dumps that overwrite `manifest.json`, `home.json` and `custom.css`.
- Per-change undo is likely to conflict for plugins, because every plugin edits one shared manifest.

## Everyday professionals

Works: accurate and cautious profession presets, plain-language assistant rules, honest privacy
wording, preview before Wrap saves, meeting-notes routine, complete Arabic.

Blocks adoption:
- Install: unsigned app, plus Claude Code, git and a paid plan; Apple Silicon only.
- PDF, Word and Excel can't be previewed; the only export is print or copy.
- No due-date alerts, calendar view or calendar export.
- Confidentiality: private folders are still synced, and a private matter can't be used with the
  assistant at all; no guidance on which Claude plan suits client data.
- No price or plan recommendation; a dollar figure beside every chat.
- Navigation still says "Ideas" for clients, matters and classes; some feature descriptions leak
  file names and developer terms.
- Help has no support contact.

## Design

- First run is 13 screens (7 onboarding steps, 6 tour cards), and git is required even for
  local-only use.
- Docs can't create, rename, move or delete files.
- At the minimum window width the Docs reading column is about 320px; collapsing the sidebar removes
  all navigation.
- Chat composer: a deleted @mention is still sent, retry drops attachments, files can't be sent
  without text, drafts are lost when switching chats.
- Home shows zeros while loading or on failure; theme choice shows a false "Unsaved changes" bar.
- Features can be turned on but not off.

## Arabic and accessibility

- Daemon error messages (about 160) are English only and appear as toast headlines.
- The weekly-review suggestion is fixed to Friday 16:00, the Saudi weekend.
- Standard native menu items (Undo, Copy, Quit…) are likely English under an Arabic menu.
- Palette search doesn't fold hamza, ى/ي, ة/ه or tashkeel.
- Home stat labels aren't pluralised.
- Streaming chat re-announces per token to screen readers; the sidebar status dot is colour only.
- Starter kit, presets and docs are English only.
- A 20-item wording list and a canonical glossary were supplied by the reviewer.

## Release engineering

No definite compile errors found for Windows or Linux, but neither has ever run.

- Tauri adds a nonce to `<style>` in HTML assets, which may block inline styles in plugin frames in
  bundled builds only (unverified).
- Windows: line endings could break the plugin runtime hash (no `.gitattributes`); the npm
  `claude.cmd` shim rejects multi-line arguments; `\\?\` paths from `canonicalize`; credential helper
  path unquoted.
- Linux: the keyring backend loses secrets on reboot; AppImage paths under `/tmp` are used for
  autostart and the daemon; several native menu items are unsupported; `.deb` installs are offered
  updates they can't apply.
- CI: npm Tauri packages are 2.5.0 against 2.12.5 crates; three jobs create the draft release in
  parallel; tests run on macOS only; `svelte-check` isn't run.
- Nothing removes autostart entries, the LaunchAgent or `~/.thoughts` on uninstall.

## Market

- Nothing is signed, notarised or auto-updating.
- The presets target lawyers and accountants, but install needs a CLI and a paid plan. Suggested first
  audience: people who already think with Claude.
- The thesis has no demo, screenshots or pack gallery.
- The whole product depends on one vendor's CLI; a second backend is still on the roadmap.
- No feedback channel; no monetisation hook; the name "Thoughts" is generic and hard to search.
- Suggested positioning: "For people who think with Claude: Thoughts turns every conversation into a
  plain-file library you own, and reshapes itself around how you work, with every change previewed
  and reversible."
