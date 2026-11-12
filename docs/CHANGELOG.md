# What's new in Thoughts

## 0.0.9

A large release: every finding of the 0.0.8 review is fixed or built, plus Codex, several libraries and voice.

### Safer by default
- **History is on for every library**, not only with Git sync. Undo, versions and rolling back a Wrap work for everyone. Backup is now a separate choice: none, a cloud folder, a drive or folder, or an online location.
- Every assistant run starts with an explicit permission mode. The default is **Accept edits**, and saving Settings no longer changes it.
- Each kind of run gets only the tools it needs. Runs Thoughts starts by itself never use Bypass, never load your own add-ons to Claude Code, and cannot rewrite CLAUDE.md, AGENTS.md or routines.
- **Approve requests inside a chat:** when the assistant needs a permission, a card asks Allow once, Always for this chat, or Deny.
- **Strict private folders** (Settings › Privacy): the assistant cannot run commands while private folders exist.
- Private folders are judged by where files really are (links and letter case included), and every run is told which folders are private.
- The trust question says which folder and what would run, and is only asked when something really changed.
- A setting to stop the assistant programs sending their own usage statistics and error reports (off by default).

### Wrap
- **A Wrap you can review:** choose which chats to save, correct what is saved from each, see every change per document, then keep, change or discard it.
- Wraps read whole chats, continue from where the last Wrap stopped, and never mark a chat saved when only part of it was read.
- A cut-off Wrap shows Finish, Undo and Keep, and says what each costs.
- Under each answer that changed your notes: **"Changed 3 notes"**, with the changes and an Undo for just that answer.

### Several libraries
- Add, switch and remove libraries. Chats, history, trust, private folders, automatic tasks and the inbox belong to their library.

### Codex
- Codex can be used next to Claude Code, chosen in Settings or per chat. A chat can move between the two (the whole conversation when it fits, a summary otherwise).

### Finding and capturing
- One search across note text and chat text, with words in any order and Arabic spellings folded. Find in the open note or chat (⌘F / Ctrl+F). Open any note from the command palette.
- **Quick capture from anywhere:** a system-wide shortcut (⌃⌥⌘Space on Mac), the tray or the File menu. Todos and quick notes are written straight to your library, without a chat.
- **Inbox:** captures, including ones saved from your phone into the library's inbox folder, wait in one place to become todos, be filed, or be processed together.
- `thoughts://` links to notes, chats and capture.
- **Voice (Mac):** dictation in chats and notes, and meeting recordings turned into text on your Mac. The audio never leaves the computer.

### Notes, ideas and todos
- A new note editor: toolbar and shortcuts for bold, headings, links, lists and checklists; pasted pictures saved beside the note; preview in step.
- Links and the index follow a rename or move. "Linked from" on every note. Save as Word.
- **One page per item** (client, matter, class or project): overview, todos, meetings, notes, outputs and log.
- Todos: groups, moving and reordering, repeats, reminders, Hijri dates and a week-start setting. Change an idea's status from the Ideas page.

### Chats
- One compact control for model, effort and permission. Queue a message while the assistant works. Export a chat. Mention any library file; pictures show as thumbnails.
- Pin, archive, filter and bulk-delete chats; an optional clean-up of old chats.
- Web lookups in a chat, off by default and switched on per chat.

### Getting started and help
- Guided install and sign-in. Set up the assistant later and use your library straight away. Bring an existing folder of notes.
- The user guides open inside the app, in English and Arabic. Report a problem. What's new after each update.

### Customise and add-ons
- The review says what a change does, in plain words, before the file list. Several changes can be in progress, each kept or discarded on its own.
- Switch one add-on off or remove it, back up add-on data, pack name, author, version and fingerprint, and a small gallery of ready-made packs.
- Add-ons can save in Try it, hand you a file, show notifications, add todos to a chosen list and ask the assistant — each a permission the review names.
- An add-on can no longer freeze the app, reach the app's own commands, send data out, or cover the review.

### Settings and the app
- Settings apply as you change them. Toasts no longer cover Send, Stop or the save bar.
- Settings › Maintenance: a start-up check-up with fixes, move Thoughts to another computer, remove Thoughts, and a check for a newer version (off by default).
- Recovery works from the native menu even when the window is broken. Quitting asks when chats are running or edits are unsaved.
- A monthly spending limit, provider presets with a connection test, and keys for other programs (add only or read only).
- Backup sign-ins can be updated or forgotten; a backup health indicator; edits wait for a sync instead of being refused.

### Good to know when updating
- Libraries that hold AGENTS.md or a .codex folder are asked for trust once.
- A library on "No backup" that already has its own Git repository now records history in that repository.
- Going back to 0.0.8 afterwards opens the new settings read-only instead of erasing them.
- The installers are still unsigned; on macOS, open the app with right-click › Open the first time.
