<p align="center"><img src="design/icon.svg" width="112" alt="Thoughts"></p>

<h1 align="center">Thoughts</h1>

<p align="center"><b>For people who already think with Claude and want that thinking kept.</b><br>
Talk things through with AI. Thoughts keeps what matters: ideas, notes and todos, in plain files you own.</p>

<p align="center"><a href="README.ar.md">العربية</a></p>

---

## Software that grows with you

Apps have always been finished products: built once, the same for everyone, and you adapt to them.
I think AI ends that. Apps will become living things that learn how you work and reshape themselves
around you, one conversation at a time.

Thoughts is my attempt to build one. Ask it for a new view, a workflow or a look, and it builds it
for you. You see a preview before anything changes, every change is versioned and can be undone in
one click, and updates never overwrite what you've made your own.
See [Making Thoughts your own](docs/CUSTOMISING.md).

## What it does

- **Think out loud.** Chat with an AI assistant about anything: a client project, a plan, a decision.
- **Nothing evaporates.** When you're done, **Wrap** turns the conversation into tidy notes,
  ideas and todos. You choose which chats to save, read the plan, and see every change before
  it is kept.
- **Pick up anywhere.** The home screen shows what you were working on, what's open, and what's
  waiting to be wrapped.
- **Yours, in plain files.** Everything is ordinary text files in a folder on your computer. No
  lock-in, no proprietary database. Sync it with iCloud, Dropbox or git, or keep it local.
- **Nothing lost for good.** With history turned on, you can undo a Wrap or restore an earlier
  version of any document. See [History and undo](docs/HISTORY.md).
- **Who it's for.** People who already think with Claude and want that thinking kept instead of
  lost in old chats. You need to be comfortable installing Claude Code and paying for a Claude
  plan; Thoughts does not remove that step yet.
- **Profession templates.** Setup asks what you do (accounting, law, consulting, teaching,
  research, building a product) and arranges your library around clients, matters, classes or
  projects, with meeting notes and due dates. These are starting templates to adapt, not finished
  products for those professions.
- **English and Arabic.** Full right-to-left support, Arabic starter templates, and the assistant
  writes your notes in the language you work in.

## Get started

1. Install [Claude Code](https://claude.com/claude-code) and sign in once. This needs a Claude
   plan that includes Claude Code, or an API key; see [what it costs to run](docs/COST.md).
2. Get Thoughts (see below). On a Mac, open the `.dmg` and drag Thoughts into Applications.
3. Open Thoughts. A short setup creates your library and walks you through the basics.

### Where it runs today

Thoughts is in early testing. It is open source: the code and every release are public on
GitHub, and the installers are on the
[Releases page](https://github.com/abdulmalik-alzeer/thoughts/releases).

- **macOS** is where Thoughts is used today, on Apple Silicon Macs (M1 or later). Each release is
  set up to make one app for Apple Silicon and Intel Macs, but it has not been run on an Intel Mac
  yet.
- **Windows** (an installer) and **Linux** (a `.deb` package and an AppImage) come out of the same
  release but have never been run on a real computer. Treat them as experiments.

### Opening Thoughts the first time

Thoughts is not signed by Apple or Microsoft yet, so the first launch of a copy you installed by
hand needs one extra confirmation. Updates from inside the app do not ask this again.

**On a Mac with macOS 15 (Sequoia) or later**

1. Drag Thoughts into Applications and open it from there, not from the `.dmg` or Downloads
   (started from there, Thoughts cannot start at login and cannot update itself).
2. Open Thoughts. macOS says it cannot check the app and does not open it. Choose **Done**, not
   **Move to Trash**.
3. Open **System Settings › Privacy & Security** and scroll down to **Security**. A line says that
   Thoughts was blocked: choose **Open Anyway**. (If the line is not there, open Thoughts once more
   and come back.)
4. macOS asks once more: choose **Open Anyway**, and give your Mac's password or Touch ID if it
   asks. Thoughts opens, and from then on it opens like any other app.

**On macOS 14 (Sonoma) or earlier:** in Applications, Control-click (or right-click) Thoughts,
choose **Open**, then **Open** again in the message.

**Questions macOS may ask later**

- **"Thoughts wants to use … in your keychain".** Thoughts keeps the sign-in for an online backup,
  and an API key if you give one, in your Mac's password store (the Keychain), unless you chose
  the file instead. This is Thoughts asking for its own saved sign-in: type your Mac login
  password and choose **Always Allow**. macOS sees every unsigned version as a new app, so the
  question can come back once after an update.
- **"Background items added".** This only appears if you switch on **Start Thoughts in the
  background when I log in** (in setup under More options, or Settings › Background service). It
  is that switch: Thoughts' background service then starts when you log in. You can see it under
  System Settings › General › Login Items, and switching it off in Thoughts removes it.
- **Microphone, speech recognition and notifications** are asked for the first time you dictate
  or Thoughts sends a notification. Each is optional.

**On Windows** (not yet tried on a real computer): if Windows says it protected your PC, choose
**More info**, then **Run anyway**.

### What setup will ask you

Four short steps. Only the folder is required; everything else can be changed later in Settings.

1. **Welcome:** the language (English or Arabic), what to call you, and what you mainly use
   Thoughts for, which decides how your library is arranged.
2. **Assistant:** Claude (recommended) or Codex, with help to install it and sign in. With Claude
   you can use an API key instead, under Advanced. Or choose **Set up the assistant later**.
3. **Your notes:** start a new library or open one you already have, and the folder it lives in.
   Optionally, a few things on your mind (they become your first ideas) and a folder of notes to
   bring in. **Backup and history** is a closed, optional section: no backup unless you choose one,
   and a history of changes on this computer.
4. **Finish:** what is sent to the assistant and what stays on your computer, a summary
   of your choices, and the look of the app. Under More options: notifications, letting long tasks
   continue after you close the window, and starting at login. When you press **Finish** for a new
   library, the assistant sets it up in your first chat.

## Updates

Your library is never touched by an update. Choose **Check for Updates…** in the menu, or switch
on **Check for new versions automatically** under Settings › Maintenance (off unless you switch
it on). When a newer version is out, Thoughts says what changed and, when you agree, downloads
it, checks that it is genuine and restarts as the new version. It never installs anything by
itself. Version 0.0.11 and older cannot do this: install the next version by hand once, the way
you installed this one.

Checking for updates needs no account and no sign-in: Thoughts asks the public Releases page.

If you've customised the app, the new version compares what you changed with the original and
only asks where both you and the new version changed the same thing: keep mine, use new, or let
the assistant merge the two.

## Privacy

Your files stay on your computer. Thoughts talks to the AI when you chat or ask it to do
something, and once more after a chat goes quiet, to write the short summary that Wrap works
from (you can switch that off in Settings › Privacy & cost). It syncs only where you tell it to.
Thoughts itself has no analytics or tracking, and it asks the assistant program it runs (Claude Code)
not to send Anthropic usage statistics and error reports of its own, unless you allow that under
Settings › Privacy & cost. You can mark folders the assistant must not read. The details in plain language, including what to check before using it with
confidential client data: [docs/PRIVACY.md](docs/PRIVACY.md).

## More to read

- [Making Thoughts your own](docs/CUSTOMISING.md): what you can ask the app to become.
- [History and undo](docs/HISTORY.md): undo a Wrap, restore an earlier version of a document.
- [What it costs to run](docs/COST.md): what you need, and what the "estimated cost" figure means.
- [Privacy](docs/PRIVACY.md).

## Uninstall

The simplest way: **Settings › Maintenance › Remove Thoughts from this computer**. It removes the
data folder, the saved passwords and key, and the login item, then quits. Then drag the app to
the Trash.

By hand: quit Thoughts completely first (from its menu, not only by closing the window). Then
remove three things:

1. **The app** itself, from Applications.
2. **The data folder** `.thoughts` in your home folder (`~/.thoughts`): chats, settings,
   customisations and logs.
3. **The login item**: the entry that starts Thoughts' background service when you log in. On a
   Mac, remove Thoughts under System Settings → General → Login Items.

If you have the source, `sh scripts/uninstall-macos.sh` (or `uninstall-linux.sh`) does steps 2 and 3
for you; add `--purge` to also remove the data folder. On Windows the uninstaller removes the login
entry and offers to delete the data folder.

**Your library folder is never touched.** It stays where you put it, readable with any text
editor, until you delete it yourself. Saved passwords and synced copies are covered in
[Privacy › Deleting everything](docs/PRIVACY.md#deleting-everything).

---

<sub>For building from source and architecture notes, see [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).</sub>
