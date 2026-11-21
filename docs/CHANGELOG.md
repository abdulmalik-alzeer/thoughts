# What's new in Thoughts

## 0.3.2

One repair: planning a project (MADO) could not save anything in 0.3.0 and 0.3.1. Take this update before you try it.

### Fixed
- **A planning step can save its work.** In 0.3.0 and 0.3.1 every step ended with the assistant saying it was not allowed to write in the `mado` folder, so no brief, document or diagram ever appeared. The rules that keep a planning run out of the rest of your project were also closing the `mado` folder itself. They no longer do. A planning run still cannot change anything outside `mado`, and still cannot change the app's own files inside it.

### Before you rely on it
- **Found in the first real use.** The fix was tried with the real Claude Code outside the app: files inside `mado` are written, and files anywhere else in the project are refused. It has not yet been seen from inside the app window, and the steps after the Brief have still never been run with a real assistant. Everything under "Before you rely on it" in 0.3.0 still applies.

## 0.3.1

Small repairs to planning and building a project (MADO), and stricter checks before a release. Nothing new to learn. As with 0.3.0, the MADO parts have still not been seen in the real app window or run with a real assistant.

### Fixed
- **A planning run cut off by a restart is still checked.** When Thoughts starts again, it looks for files the run changed outside the `mado` folder and lists them in the step, as it does after any run. If it could not check, the step says so, and it cannot be approved until you have looked.
- **A build continues after a restart or a change of library.** A build of a project in a library that is not the open one used to stall; now it goes on. You see it, stop it or bring it into your branch after switching to that library.
- **Removed build copies leave nothing behind.** When a build's copy of the project is removed, the record that Thoughts trusted that folder is removed with it. Records 0.3.0 left for copies that are gone are cleared the next time a copy of that project is removed.

### Behind the scenes
- The checks run before a release now include the UI's own tests, and they stop early when the installed packages do not match the lock file.
- A test that failed at random was fixed. The fault was in the test, not in the app.

### Before you rely on it
- **Still not seen for real.** These fixes were tested automatically only. MADO has still not been seen in the real app window, and no plan or build has yet been run with a real assistant. Everything under "Before you rely on it" in 0.3.0 still applies.

## 0.3.0

Plan and build a project: Thoughts can take a software idea from a first conversation to working code in seven steps, with you reading and approving each one. Read "Before you rely on it" below: the new parts have been tested automatically and in a browser, but not yet in the real app window or with a real assistant run.

### New
- **Plan and build a project (MADO).** Open **Projects**, pick a project folder you linked, and choose **Plan**. Seven steps, in order: **Brief**, **What it does**, **How it's built**, **Look and screens**, **Work plan**, **Final check**, **Build**. Each step has its own chat, and a step opens once the steps before it are approved. Everything the plan says is kept in a `mado` folder inside the project.
- **Comments are how you ask for changes.** Comment on a passage of a document, a box or a line of a diagram, a colour or another design setting, or a spot on a screen. Collect as many as you like, then **Send changes**: the assistant gets them all as one request and changes the documents and the diagrams together.
- **Diagrams you can edit.** The screens and the paths between them, the parts of the system, the data tables and the work plan are drawn as diagrams you can move around and zoom. In the first three you can rename a box, add one, draw a line, delete something or fill in a table's fields; your edits go to the assistant with **Send changes**. The work plan's diagram is drawn from the plan and is for reading.
- **The design step.** Eight ready looks, and the assistant's own suggestions. Pick one, then change any colour, font, corner or spacing and see it at once. Thoughts warns when text becomes hard to read, and **Fix contrast** repairs it. A first picture of every screen is shown in a sealed frame that runs nothing and loads nothing from outside, and you can pin a comment to a spot on it.
- **The Final check.** The app checks that every feature and every screen has work planned for it, and lists every command the build would run. Approving it allows those commands to run on this computer, so Thoughts asks you itself, in a dialog of its own that names the project and lists the commands. If the plan changed after you looked, the approval is refused.
- **The build.** The app does the work on a separate copy of the project, on a branch of its own, a few tasks side by side; your own branch is not touched while it works. The assistant only reads and edits files; the app runs each task's check itself and sends a failed check back to the assistant, up to three times. The build holds by itself when your usage is high (Settings › Privacy & cost sets where). You can pause or stop it at any time. When everything is done, **Bring the finished build into my branch** merges it.
- Help › "Working with notes, chats and files" has a section, "Plan and build a project (MADO)", on all of this.

### Safer
- **The build's commands are kept to their copy of the project** on a Mac: they can change files only there, in temporary files and in the build's own download cache, and cannot read your own files, your saved sign-ins or keys, or the app's data. They can still use the network. The switch is in Settings › Privacy & cost, on unless you turn it off; Thoughts asks before it goes off.
- The commands run code the assistant wrote during the build (a test command runs the project's tests, whatever they contain). Read the list before you approve it.
- A planning run may write only the `mado` folder. Anything it changes elsewhere is listed in the step, and the step cannot be approved until you have looked.
- A build does not start while the `mado` folder holds a link or a special file, or while the project's own git settings would start other programs; the message names what it found.

### Before you rely on it
- **Not yet seen for real.** The new parts of this version have been tested automatically and looked at in a browser. They have not yet been seen in the real app window, and no plan or build has yet been run with a real assistant. Expect rough edges, and try it first on a project you can afford to redo.
- Planning and build runs use Claude Code only, not Codex.
- The build needs the project to be a git repository with nothing uncommitted.
- While the build's commands are kept to their copy, installing packages from a private registry with your saved credentials fails.
- Thoughts is released for macOS only, and the build's commands are kept to their copy only there.

## 0.2.0

Chats read your Excel, Word and PowerPoint files, answers can hold diagrams and charts that you save as SVG or draw.io files, and the built-in pages can be rearranged. This version also brings a dangerous **Full access** switch, off unless you turn it on.

### New
- **The assistant reads office files.** Attach an Excel, Word or PowerPoint file (also OpenDocument, Rich Text and the older doc, xls and ppt) and ask about it: the assistant now reads what is in it. Spreadsheets are read sheet by sheet. Very large files are read up to a limit, and the assistant is told where it was cut.
- **Every attached file opens in the preview** with one click, also one still waiting in the message box. Files only their own program can show, such as Numbers, Pages and Keynote, say so.
- **Diagrams in answers and notes.** Ask for one ("draw how the checkout talks to the bank") and it is drawn inside the answer by [Archify](https://github.com/tt-a1i/archify), which comes with Thoughts: architecture, workflows, sequences, data flows, lifecycles, database tables, classes, trees, timelines and waterfalls. Diagrams need Node.js 18 or newer on your computer; without it Thoughts says so.
- **Charts** of numbers: bars, lines, areas, pies, donuts and scatter plots, drawn by Thoughts itself.
- **Save a picture as SVG, a diagram also as draw.io.** The draw.io file opens in draw.io (diagrams.net) with every box as a shape you can move and the lines attached. **Source** shows what a picture was drawn from. Help › "Working with notes, chats and files" has a section on all of this.
- **Markdown while you type and while an answer arrives.** The message box shows bold, lists and code as you write, and can be made larger (⌘⇧E). Answers are formatted as they stream.
- **Built-in pages can be rearranged.** Ask Customise to move, hide or replace the sections of Home, Todos and the other built-in pages, or to replace a whole page. **Edit sidebar** lets you drag the sidebar's items into place.
- **Quick capture** is a small bar in your chosen theme that grows as you type and can be dragged.
- **Codex models** are read from the Codex installed on your computer. An older Codex that cannot list its models takes a model name you type.
- **Full access (dangerous)**, in Settings › Privacy & cost. While it is on, the assistant, add-ons and pages can do anything on this computer, and every protection Thoughts adds is switched off. It is off unless you switch it on, behind two warnings. Read [Privacy › Full access](PRIVACY.md#full-access) first.

### Better
- **Full screen** is ⌃⌘F, and Esc no longer leaves it.

### Safer
- A picture saved from Thoughts is checked to be only a picture: nothing in it can run or fetch anything.
- Office files are read with limits on size and time, so a file built to exhaust the computer cannot.

## 0.1.0

A big one: pictures and files open inside the app, chats can be edited and branched, a Graph of your notes, suggestions for automating your work, and a long list of fixes and safety improvements. Help has a new guide, "Working with notes, chats and files", that explains the new tools.

### New
- **Pictures open large.** Click any picture, in a chat, a note, a Word document or an answer, to see it large. Zoom with the scroll wheel, a pinch or + and −, drag to move, and press Esc or X to close.
- **Files preview inside the app.** Docs and chat attachments now show pictures, PDFs, Word, Excel, PowerPoint, CSV, OpenDocument and Rich Text files, text and code with colours, web pages (safely, without their scripts), sound, video and the contents of a ZIP. Spreadsheets show their sheets as tables and presentations go slide by slide with their notes. Files in the older Word, Excel and PowerPoint formats (doc, xls, ppt) are not shown, and the preview says so.
- **Print or save as PDF.** Any note, an item's front page or its log can be printed or saved as a PDF, in a clean light layout with Arabic laid out correctly. On a Mac, "Save as PDF…" asks where to save and writes the file straight there; on Windows and Linux the print window opens.
- **A note beside the chat.** Click a note an answer changed, a link to a note or an @-mention to read it in a pane beside the chat. It follows the note as the assistant changes it. ⌘-click (Ctrl-click) still opens Docs.
- **Edit and send again, and branches.** Edit a message you sent and send it again. Editing an earlier message, or **Branch** on any message, starts a new chat from that point and leaves the original as it was.
- **A second window.** "Open in new window" puts a note or a chat in a window of its own, to keep two notes, or a chat and a note, side by side.
- **Graph.** A new page shows your notes as a map joined by their links, with search, filters and a list view. **Links** on an item's page shows what is around it.
- **Changes waiting.** Switch on "Check for changes from your other computers" in Settings › Library & backup (it is off until you do) and Thoughts shows "3 changes waiting" beside Sync. It only looks: nothing changes until you press Sync.
- **Changed outside Thoughts.** Edits made in another program, on your phone or by iCloud now refresh what you see and appear in History as their own entry, "Changed outside Thoughts".
- **Suggest automations** (Customise › Automate). When you ask, your assistant looks at a short summary of how you use Thoughts and suggests routines, helpers, connections or new pages. You see exactly what is sent first, and review everything before it is kept. It includes a ready-made "Email writing" routine whose drafts end with an "Open in Mail" link (Thoughts never sends email itself).
- **Helpers:** specialists your assistant can hand part of a chat's work to. You review every helper before it is kept, and a helper never gets more permissions than the chat itself (Claude Code only).
- **A People list:** the people you write to, who they are and how to write to them, kept as ordinary notes in your library, so the email routine writes in each person's language and tone. You can add, change and remove people in the app.

### Better
- **You notice when the assistant needs you.** A chat waiting for your OK is marked in the sidebar, the Dock shows a count while Thoughts is in the background, and the reminder stays on screen until you answer. Clicking a Thoughts notification opens the chat it is about.
- **First-run setup** lets you choose Codex instead of Claude, keeps backup in a closed optional section, explains Apple's Git installer on a Mac, and shows how to get an access token for an online backup.
- **Deleting a chat that was never wrapped** warns you and offers to wrap it first.
- Settings › Maintenance has an **"Updates for testers"** card that walks you through getting updates while Thoughts is not public, with each line ready to copy.
- The cost setting now says what it does, **"Show cost on Wrap, Reorganize and setup chats"**, and moves with you to another computer.
- **Codex's estimated cost** now counts toward the month's total and the spending limit, so automatic tasks on Codex stop at the limit too. The cost guide shows where to see your usage.
- **Bringing a large folder** into a library with a backup asks first, and can bring in only the notes. Undo right after bringing notes in takes them back out completely while they have not been sent.
- **Faster:** sending a message in a chat with many attached files, saving long chats with many file edits (their files also stay smaller), refreshing long chats, and the Ideas page in large libraries. Opening a document no longer reloads the whole Docs page.
- The update window now speaks only in the app's own words. To see what a new version changes, open its release page on GitHub, or Help › What's new once it is installed.
- Undo arrows point the right way in Arabic, Home's "See all" buttons tell screen readers which list they open, and many Arabic spellings were corrected.
- The README explains how to open Thoughts the first time on current macOS (Privacy & Security › Open Anyway) and what setup will ask. While Thoughts is not signed by Apple, see [Opening Thoughts the first time](../README.md#opening-thoughts-the-first-time) before installing by hand.

### Fixed
- **Todos:** a todo you are editing, dating or moving now stays with that todo when the list refreshes. Before, a refresh could make the change land on a different todo.
- **Chats:** a finished answer no longer looks stuck as "running" when the connection blips, thinking text is no longer lost, and a message waiting to be sent goes out after a reconnect. A message that fails to send after you leave the chat is kept as its draft. Links written as references work while the answer is still being written.
- Stopping a chat right after it answered no longer marks the answer as stopped; a chat can no longer start two runs that stop each other; and programs a finished run left running in the background are now ended.
- Permission requests from before a restart no longer look as if they can still be answered.
- The app no longer waits forever when its background service stops answering, and quick changes to zoom or theme no longer undo each other.
- **Sync:** if a backup location already holds a copy of your library that this computer's copy did not come from, Sync now asks how to put the two together, and nothing is deleted. A backup location no longer starts refusing changes for good, and a clash you settled is not asked about again. Libraries that keep files with Git LFS are no longer sent with stand-ins in place of those files: Sync says why and sends nothing.
- **Your text stays exactly as it was:** moving a file no longer undoes edits made to linking notes while the move ran, and status changes, inbox filing and todo edits keep byte-order marks, line endings and unusual text intact.
- Undoing an answer's changes, or rolling back a Wrap, now either finishes completely or changes nothing. Keeping a Wrap's changes just before the app stops no longer wraps the same chats twice, and a Wrap no longer clears changes you staged by hand in your own repository.
- Saving and ticking todos keep working in the library you switched to while a Wrap finishes in the one you left, and a Wrap, undo or repair now waits for a change you just made instead of risking it.
- If the app quits or crashes while a Wrap, an automatic task or a Codex chat is running, the files those runs may not change are put back, right away or the next time the app starts.
- The customisation review no longer lets you keep a change that could not be tested; it asks you to try again. Opening another library in a second window no longer throws away unsaved Settings changes without warning.

### Safer
- **Confirmed by the system.** Switching a chat to bypass permissions, making that the default, and trusting a folder are now confirmed once, in a dialog that the Thoughts app shows itself. It names the chat or the folder, and nothing running inside the window, nor another program that gets hold of the app's access file, can answer it.
- **Files that can carry macros**, old Word, Excel and PowerPoint files, Rich Text and OpenDocument files, are no longer opened in another program with one click; Thoughts offers "Show in Finder" instead. Opening a file from a private folder in another program asks first.
- Your saved sync password can only be used by the app's own sync. Other programs on your computer can no longer get it by starting Thoughts in a special way.
- A library whose sync settings are read from a file inside the library is no longer trusted, because anything synced in could change them.
- Chats and automatic tasks can no longer read the app's release signing key or keychain passwords. Codex no longer runs while your sign-in details are kept in a plain file, and on a Mac its no-sandbox mode is kept out of the app's own data.
- The assistant never starts without its safety settings, and a task with a spending limit never runs without that limit.
- An "always allow" you take back in a chat stays taken back after the app restarts.
- Each part of the app can now use only the app features it needs.

## 0.0.14

A calmer update.

- **One progress bar for an update's download.** When you agree to an update, Thoughts opens Settings › Maintenance and shows a single bar with how much has arrived. Before, a new notification appeared every few percent. You will see the bar the first time this version downloads the one after it.

## 0.0.13

A fix for attachments in customisation chats.

- **Customisation chats can read what you attach again.** Since 0.0.11 a screenshot, picture or document attached in a customisation chat was refused when the assistant tried to open it, so it could not see what you meant. The same happened in an ordinary chat with web lookups switched on, and other chats asked for permission first. Now every chat reads the files attached to it without asking. Files attached to other chats stay closed, as before.

## 0.0.12

Thoughts can now update itself, and tells you what changed before it does.

### Updates
- **Updates from inside the app:** choose Check for Updates… in the menu. When a newer version is out, Thoughts shows what changed and, when you agree, downloads it, checks that it is genuine and restarts as the new version. Your library, chats and customisations are not touched. This version still has to be installed by hand; the ones after it arrive this way.
- **You decide when it looks:** Thoughts asks about new versions by itself only if you switch on Settings › Maintenance › "Check for new versions automatically" (at most once a day). It never installs anything by itself.
- **For testers while the project is private:** one command in a terminal lets Thoughts look for updates with your GitHub sign-in. The README has it under "Updates".
- On a Mac, macOS may ask again for permission to use the keychain after an update, because this version is not signed with an Apple certificate.
- A copy opened straight from its disk image is told to move to the Applications folder first.

### The assistant
- **No more guessed times:** the assistant no longer says how long its own work will take unless you ask. When you do ask, it answers for itself, usually minutes, not the hours a person would need.

### Setting up
- **A choice for technical work:** "What do you mainly use this for?" now offers Software & technical work, in setup and in Settings › General.

## 0.0.11

A privacy fix.

- An assistant run can no longer read files attached to another chat. Before, a chat that got its first attachment while another chat's assistant was working could be read by that run. Now every run is closed to all attachments and reads only copies of its own chat's files, which are removed when the run ends. This covers ordinary chats and customisation chats, with Claude and Codex.
- The assistant's file tools now respect blocked folders whatever letter case a path is typed in (macOS treats `Chats` and `chats` as the same folder).
- Shell commands that name the app's own folder are refused in every mode. This is an extra guard, not a wall: commands that only read files still run without asking, as in Claude Code itself. To turn the shell off, use Settings › Privacy › Strict private folders.

## 0.0.10

Twelve changes asked for after using 0.0.9: a calmer chat, usage and cost at a glance, and customising that feels like a conversation.

### Chats
- **Calmer chats:** the assistant's commands and file reads fold into one line you can open, like the Claude desktop app. To see every step as before, turn on Settings › Assistant › "Show every step in chats".
- **Usage and cost:** see how much of your Claude and Codex plans you've used — a usage ring in every chat, a Usage tile on Home, Settings › Usage, and a new Usage page with tokens and what they would cost at API prices, by day, chat and model. Choose whether costs cover only chats in Thoughts (the default) or everything on this computer.

### Customising
- Customising now starts with a **New customisation** button that opens a chat of its own, where you describe the change and talk it through.
- Customisation chats can look at what you attach — a screenshot of what to change, a picture to match, a document to work from.
- Customisations can change details of the app's own pages: headings, button wording, hiding parts you never use, add-on boxes on any page, even your own page in place of Todos, Ideas, Chats or Folders (the usual page is always one press away).
- Trying a customisation that is nearly right? Choose **Continue chatting**: the app goes back to how it was, and you tell the assistant what to change — it builds on what it made.
- Closing a customisation's review no longer hides the change you were trying, and a waiting customisation always shows a "Choose" strip so you can come back to it.
- Themes you add show their three colours in the theme picker, like the built-in ones.
- Pages and boxes you add open with the same smooth motion as the app's own.

### The window
- Notifications fade away by themselves after a few seconds; hover over one to keep it. Missed one? Its button is still in Recent messages.
- Only the page scrolls — the sidebar and top bar stay put — and the sidebar slides smoothly when you hide or show it. Back and Forward return you to where you were on a page.
- With the sidebar collapsed, the window buttons sit in a proper title bar on macOS.

### Known limits
- macOS only for now; Windows and Linux installers follow later. The app is unsigned: right-click › Open the first time.
- Plan-limit figures arrive with each run, so they show "as of" the last chat. Codex costs are estimates.

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
