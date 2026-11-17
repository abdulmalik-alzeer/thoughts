# Privacy

This page describes what the Thoughts app does with your information. It is a description of how the software behaves, not a legal document. If you handle confidential information for clients, students or an employer, check it against your own professional and contractual obligations.

العربية: [docs/ar/PRIVACY.md](ar/PRIVACY.md)

## The short version

- Your library and your chats are stored on your computer.
- When you chat, the conversation and the files the assistant reads to answer you are sent to the AI provider you signed in with.
- Sync uploads your library to the place you chose, and nowhere else.
- Dictation and meeting recordings are turned into text on your Mac; the sound is never sent anywhere.
- The app contains no analytics, tracking or advertising. The assistant program it runs is made by another company and can send that company usage statistics and error reports of its own; Thoughts asks it not to unless you allow it: see [What the assistant program sends on its own](#what-the-assistant-program-sends-on-its-own).

## What stays on your computer

| What | Where |
|---|---|
| Your library: notes, ideas, todos, meeting notes, outputs | The library folder you chose during setup |
| Meeting recordings and their transcripts (Mac app) | The item's `meetings` folder in your library. See [Voice](#voice-dictation-and-meeting-recordings) |
| Chat history and attachments | A folder named `.thoughts` in your home folder |
| Settings, customisations and logs | The same `.thoughts` folder |
| Saved passwords for sync and for the AI provider (if you entered any) | Your computer's password store (the Keychain on a Mac). See [Saved passwords](#saved-passwords) |

The assistant that Thoughts runs (Claude Code) also keeps its own record of sessions on your computer, in a folder named `.claude` in your home folder.

When you delete a chat in Thoughts, the assistant's transcript of that conversation is deleted from the `.claude` folder with it, and so is the plan file a Wrap preview leaves there (in `.claude/plans`). So are the other working files the assistant keeps for that conversation's sessions: its helper records, its backup copies of files it edited and its to-do lists. Thoughts removes only items named after that conversation's own sessions, never anything else in `.claude`. Chats can also be deleted in bulk, or automatically once they are old (an option you switch on in the Chats page; off by default); the same applies to them. Deleting a chat does not recall anything already sent to the AI provider.

The part of Thoughts that runs in the background only accepts connections from your own computer.

A library that Thoughts creates can be opened only by your own user account on the computer. A folder you already had keeps whatever access it had: on a computer with several accounts, **Settings › Privacy & cost** tells you when other accounts can open your library folder, and offers to close it to them.

### Saved passwords

By default, passwords you enter for sync or for an AI provider are kept in your computer's password store: the Keychain on a Mac, and the equivalent system store on other computers.

Settings also offers a **file** option for the sync password. If you choose it, the password is saved in a file inside the `.thoughts` folder that only your user account on this computer can read. That file is **not encrypted**: anyone who can sign in as you, or who gets a copy of your home folder (an unencrypted backup, for example), can read it. On a Linux computer with no working password store, Thoughts falls back to the same kind of file. Prefer the default unless you have a reason not to.

## What is sent to the AI provider

Thoughts works by running an AI assistant on your behalf. To answer you, the assistant sends the AI provider:

- what you type, and files or images you attach;
- the contents of the files it opens while working on your request. It works inside your library, so this can be any note it considers relevant, not only the one on screen;
- the library's own instructions and routines.

This is sent through your own account with the provider (by default, your Claude account). How the provider stores, retains and uses that data is set by the terms and privacy settings of that account, not by Thoughts. If you entered a different provider's address in Settings, the same information goes to that provider instead.

The same applies to everything the assistant does for you without a chat window in front of you: **Wrap**, automatic tasks you have approved, and customisation requests. Those runs work with your library's files only: they cannot use the web, or use tools you connected to Claude Code yourself, and none of your own Claude Code settings apply to them. Wrap and automatic tasks cannot run commands on your computer at all. A customisation can run only commands that just look at files (such as listing a folder); any command that would change something is refused. Wrap and automatic tasks also cannot change the library's standing rules (CLAUDE.md) or its routines; should such a change get through anyway, it is undone when the run ends and the chat says so.

**Web lookups.** In a chat you can switch on **Web lookups** (next to the permission mode; off for every new chat, and the app asks first). The assistant may then search the web and open web pages for that chat without asking. What it searches for, and the addresses of the pages it opens, go to the search service and to those websites; the search goes through your AI provider. Pages can contain instructions written to mislead the assistant, and it may send what it has read in that chat, your notes included, to the sites it opens or searches. So while web lookups are on, it cannot run commands on your computer, use tools you connected to Claude Code, or change the library's standing rules (CLAUDE.md) and routines in that chat. Wrap, automatic tasks and summaries never use the web.

**Automatic summaries.** So that Wrap is quick, Thoughts prepares a short summary of each chat in the background: once the chat has been quiet for a while (15 minutes unless you change it), or when you start another chat, the text of that chat is sent to the AI provider a second time and a few lines come back. This happens without you pressing anything, for every chat in your library, including ones you never mean to wrap. The summary is kept on your computer with the chat. To stop it, switch off **Settings › Privacy & cost → Summarise chats automatically**: a chat is then only read again when you press Wrap.

**Codex.** If you choose Codex under Settings › Assistant (or move a chat to it), the same kinds of text go to OpenAI instead, through your ChatGPT plan or OpenAI key, under the terms of that account. Codex is not told about the chats that stay with Claude Code, and the other way round; moving a chat passes its conversation (or a summary of it) on to the other one.

Nothing is sent to an AI provider when you are only reading, searching or editing your library in the app.

## Voice: dictation and meeting recordings

In the Mac app, Thoughts can turn speech into text: the microphone button in the message box and in the note editor (dictation), and **Record a meeting** on the Meetings tab of a client, matter, class, project or idea.

- **Speech is turned into text on your Mac, never anywhere else.** Thoughts uses the speech recognition built into macOS and always requires its on-device mode, in which macOS keeps the audio on the computer. If a language cannot be recognised on your Mac (on many Macs Arabic cannot yet), Thoughts says so and does not use it: it never falls back to sending your voice to Apple's servers. Thoughts itself makes no network connection for any of this.
- **The first time**, macOS asks whether Thoughts may use the microphone and speech recognition. You can change your answer at any time in System Settings → Privacy & Security → Microphone, and → Speech Recognition.
- **Dictation** keeps no recording: the words go into the text box and nothing else is kept.
- **A meeting recording** is saved as a sound file in that item's `meetings` folder in your library, and its transcript as a dated meeting note beside it. The sound file stays on this computer: it never goes into the library's history or your backup, while the transcript note does, like your other notes (if your library is in a folder that a cloud drive syncs, such as iCloud Drive or Dropbox, that drive still copies the sound file). Because it is not in the history, a deleted sound file cannot be brought back from History; delete it once you no longer need it. A recording stops by itself after four hours.
- **Write meeting notes** starts a chat: as in any chat, the assistant reads the transcript note, so the transcript is sent to your AI provider. The sound itself is never sent. For an item in a private folder, the button does not start a chat.
- Ask the people in a meeting before you record it. The rules on recording conversations differ from place to place.
- On Windows and Linux there is no voice button yet: there is no speech recognition there that Thoughts can keep on the computer.
- The Dictation key of macOS itself (which types into any text box) is a feature of macOS, not of Thoughts; System Settings → Keyboard → Dictation says how it handles your voice.

## What sync does

Sync is off unless you turn it on. When it is on, Thoughts uploads your whole library, including its history of earlier versions, to the location you entered, and downloads changes from it. Who can see it there depends on that service and how you set it up. Chats and settings are not part of the library and are not synced.

To take them to another computer, use **Settings › Maintenance › Move to another computer**. It saves one file in your Downloads folder with your chats (and their attachments), settings, customisations and what your add-ons saved (an hours tracker's entries, for example). With several libraries it holds only the open one's chats and settings: the others, and where their folders are, stay on this computer. The file never contains passwords, your provider key or anything that unlocks an account, but your chats are in it as plain text: keep it somewhere safe and delete it once you have imported it.

If you keep your library inside a folder that another service backs up (iCloud Drive, Dropbox and the like), that service copies it too. That is between you and that service.

## No analytics

Thoughts does not collect usage statistics, crash reports or any other information about you, and has no account of its own.

Checking for a newer version is the one time Thoughts contacts the internet on its own behalf, and only in these cases:

- **When you choose "Check for Updates…"** (in the menu, under Customise › App updates, or under Settings › Maintenance).
- **At most once a day, if you switch on "Check for new versions automatically"** under Settings › Maintenance. It is off unless you switch it on. (A copy that cannot update itself, such as one installed from a `.deb` package, checks once a week instead.)

Either way it asks GitHub, where Thoughts is published (`github.com`, or `api.github.com` for a copy that cannot update itself), whether a newer version exists. The request carries nothing about you, your library or your settings: no account, no cookie, no identifier, only the address it asks for and the name of the program asking. Like any connection, it lets GitHub see your internet address.

Thoughts never downloads or installs anything by itself. When a newer version exists it tells you what changed. Only when you choose to install it does it download the new version from GitHub, check that it was signed by the makers of Thoughts, and restart as that version. A copy that cannot update itself shows you a link to the download page instead.

**The GitHub fallback, for testers.** While a release is not public, the address above answers "not found". A tester can let Thoughts look for updates as them instead, by running `thoughts updates gh on` in a terminal. From then on, and only when the public address answers "not found", a check asks the GitHub command-line tool (`gh`) on your computer for its sign-in and asks GitHub with it, so GitHub knows which account asked. The sign-in is used for that check, that download, or the check of the “Updates for testers” card in Settings › Maintenance, and nothing else: it is not stored, not shown, not given to the assistant, and sent to GitHub's own address for programs (`api.github.com`) only. This is off unless you run that command, there is no switch for it in the app (that card only shows the line to type, and while it is off it only looks whether `gh` is on your computer, without starting it), and `thoughts updates gh off` turns it off again.

To suggest improvements (for example the monthly tune-up described in [Making Thoughts your own](CUSTOMISING.md)), the app keeps a small record on your computer of which screens you open and which customisations you kept or discarded. That record never leaves your computer on its own. If you accept a tune-up suggestion, a short summary of it is included in the request to the assistant, like anything else you send in a chat.

### What the assistant program sends on its own

The assistant is a separate program made by another company: Claude Code by Anthropic and, if you
choose it, Codex by OpenAI. Besides your conversations, it can send its maker some information of
its own, under that company's terms and privacy policy, not Thoughts':

- **Claude Code** can send Anthropic usage statistics: how fast and how reliably it works, and which of
  its features are used, never your notes, your messages or file names. When you sign in with a
  Claude Pro, Max or Ultra plan it can also send reports of its own errors, with personal details
  removed before they leave. Now and then it may ask you to rate a session; a rating sends only the
  rating. Anthropic describes all of this in
  [Claude Code data usage](https://code.claude.com/docs/en/data-usage), and its privacy policy is at
  [anthropic.com/legal/privacy](https://www.anthropic.com/legal/privacy).
- **Codex** can send OpenAI usage statistics of its own; its settings have a switch for them. OpenAI's
  privacy policy is at [openai.com/policies/privacy-policy](https://openai.com/policies/privacy-policy/).

What Thoughts does about it:

- **Settings › Privacy & cost › Send the assistant program's usage statistics and error reports** is
  off unless you switch it on. While it is off, every Claude Code that Thoughts starts (chats, Wrap,
  automatic summaries, automatic tasks, setup and Reorganize) gets `DISABLE_TELEMETRY` and
  `DISABLE_ERROR_REPORTING`, which Anthropic documents as switching off its usage statistics (and with
  them the session ratings) and its error reports; and every Codex run gets `analytics.enabled =
  false`, which OpenAI documents as switching off its usage statistics. Claude Code still updates
  itself as usual. A change applies from the next answer.
- Switched on, Thoughts adds nothing: each program then does what its own settings say.
- Thoughts cannot see what these programs send, so it relies on them keeping to those switches.
  Claude Code or Codex that you run yourself, outside Thoughts, follows its own settings, not this
  one; to switch it off there too, see Anthropic's and OpenAI's pages above.

The diagnostics shown under Settings are produced on your computer and go nowhere unless you copy and send them yourself. Passwords and keys are removed from them, your home and library folders are replaced by placeholders, and the address of a backup location keeps only the name of the site. Read them through before you post them anywhere public all the same.

## Private folders

In **Settings › Privacy & cost** you can mark folders in your library as private.

What is enforced, by the app's background service:

- The assistant's file tools are blocked from reading, listing, searching or changing anything in a private folder. This applies to every kind of run: chats, Wrap, automatic tasks and customisation.
- Add-ons (the small tools and views created through Customise) cannot read private folders.
- The assistant is told, in its instructions for every run, which folders are private and to leave them alone, also when a note or a web page asks otherwise. Those instructions are handed over in a file only your account can read, so other people using this computer cannot see your private folders' names.
- Wrap and automatic tasks cannot run commands at all, so for them the block has no way around it.
- **Strict private folders** (Settings › Privacy & cost, off unless you switch it on) closes the remaining route in chats. While you have private folders, no chat runs a command on its own: an ordinary chat asks you before each command, with a warning that a command is not checked against your private folders, and other chats cannot run commands at all. The cost: the assistant cannot move, rename or delete files, run scripts or install anything without asking you each time. Chats in Bypass permissions are not affected.

What it does not do:

- **It is a safeguard, not a guarantee against every route.** The block applies to the assistant's file tools. Unless strict private folders is on, the assistant may still run some commands in an ordinary chat (looking through files, moving them), and such a command could reach a file in a private folder; it is told not to. A command you allow yourself, and anything in a chat in Bypass permissions, is not checked against private folders either. If that risk is not acceptable for some material, do not keep it in the library at all.
- **The same goes for the app's own data and your attachments.** A chat's assistant can open only the files attached to that chat, and its file tools cannot open other chats' attachments or the app's own data. Commands that only look at files (listing or showing them) run without asking, in every permission mode. The app refuses a command that names its data folder, but that is an extra check and can be got around; it does not block every command that reads. To stop the assistant from running commands on its own, switch on **Strict private folders** (it applies while you have private folders).
- **Private folders are still backed up and synced.** They are ordinary folders in your library, so sync uploads them with everything else. To keep a folder out of sync, keep it **outside the library folder** (anywhere else on your computer) and link to it from your notes if you need to remember where it is.
- **It does not go back over what is already written elsewhere.** An item's title and one-line summary in your library's index, and anything from the folder that was copied into other notes, stay where they are, and the assistant reads the index in most chats. When you mark a folder private, the app tells you if the index still mentions its items and offers to remove those lines. Text in other notes is yours to tidy.
- A private folder cannot be used with the assistant. You cannot ask it to summarise or search something it is blocked from reading.

**With Codex, private folders are only an instruction.** Codex has no way to close a single folder: it is told which folders are private and to leave them alone, but nothing stops it from reading them, or other chats' files and the app's own data. Each Codex chat says so while you have private folders. If that is not acceptable, keep those chats with Claude Code. What Codex does enforce is its sandbox: in Read only it changes nothing, in Edit in this folder it changes only that folder and its commands cannot reach the internet; changes it makes to the app's routines (and, for Wrap and automatic tasks, to CLAUDE.md) are put back when it finishes. Because Codex can read the app's own data, it does not run at all while your sign-in details are kept in a plain file (Settings lets you keep them in the system password store instead); use Claude Code then. On a Mac, a chat in the mode with no sandbox runs Codex inside a sandbox of the app's own that closes the app's data folder.

Good candidates: anything a client or employer has asked you not to put through AI tools, and records about named individuals.

Some habits help regardless of settings:

- Do not paste passwords, bank or card numbers, or national ID numbers into a chat. The assistant is instructed not to save them in notes, but whatever you type has already been sent to the AI provider.
- The assistant keeps each client's details inside that client's own folder.
- The first time you paste something that looks confidential, the assistant reminds you that what gets saved will also go wherever your library is backed up.

## Using Thoughts with confidential client data

This section is practical guidance, not legal advice. Thoughts cannot tell you whether your professional rules allow a particular use; only you, your firm or your regulator can.

### What leaves your device, and when

| When | What leaves | Where it goes |
|---|---|---|
| You send a chat message, press Wrap, or an automatic task runs | Your message and attachments, and the contents of any library file the assistant opens for that request | The AI provider, through your own account |
| Web lookups are on in a chat and the assistant uses them | What it searches for, and the addresses of the pages it opens | The search service and those websites |
| A chat goes quiet, or you start another one (unless "Summarise chats automatically" is off) | The text of that chat, once more, to be summarised | The AI provider, through your own account |
| Sync is on | The whole library with its history, private folders included | The backup location you entered |
| You export, print or share an output yourself | That file | Wherever you send it |
| The assistant program runs (Claude Code; Codex if you chose it) | Its own usage statistics and error reports, only if you allowed them under Settings › Privacy & cost | Anthropic (or OpenAI) and the services they use |
| You dictate or record a meeting in the Mac app | Nothing: speech is turned into text on your Mac | — |
| You read, search or edit in the app without chatting | Nothing | — |

Thoughts has no servers of its own. Nothing passes through the people who make the app.

### Whose terms apply

The data handling terms are those of **your own Claude plan** (or of the other provider you chose). Thoughts does not change them and cannot promise anything on the provider's behalf. Read the provider's current published terms and privacy policy for the plan you are on; for Claude these are on Anthropic's website, at [anthropic.com/legal](https://www.anthropic.com/legal).

Before using client material, check:

1. **Training.** Whether conversations on your plan may be used to train models, and whether there is a setting in your account that controls it.
2. **Retention.** How long conversations are kept, and whether and how you can delete them.
3. **Your own rules.** Whether your professional rules, your engagement letters or your employer's policies require client consent before client information is put through an AI service, or require an enterprise or business agreement with the provider instead of a personal plan.

If you cannot answer these for the plan you are on, treat client material as not cleared for use.

### Practical habits

- **Keep identifiers out of chats where you can.** "The client" or initials often work as well as a full name; refer to documents instead of pasting account or ID numbers.
- **Use private folders** for material the assistant should not read, and remember they are still synced.
- **Keep the library on an encrypted disk.** Turn on your computer's disk encryption (FileVault on a Mac, BitLocker on Windows, or your Linux distribution's equivalent). Thoughts stores your library as ordinary files and does not encrypt them itself.
- **Choose a backup location you control**, and that your obligations allow: know who can open it and in which country it is held.
- Outputs you prepare for one client should contain only what that client should see. Read them before sending.

## The trust question when opening a library

A library is a folder of text, but a folder can also carry settings and instructions that make the assistant or the sync tool run programs. When you open a library that Thoughts did not create itself — one you downloaded, or received from someone — Thoughts first lists anything in it that could run a program on your computer, or that changes the assistant's standing instructions from the ones Thoughts ships (in this version or an earlier one, so an update alone never asks), and asks whether you trust the folder. Nothing in the folder is run until you say yes. If those settings or instructions change later, you are asked again.

Libraries created by Thoughts contain one such setting. It fetches the latest synced copy when you start the assistant in the library yourself, outside the app; inside Thoughts it does nothing, because the app does the syncing. Only trust a library from someone you would let run software on your computer.

## Keys for other programs

**Settings › Privacy & cost › Keys for other programs** makes a key for a shortcut, a script or another program on this computer (a launcher such as Raycast, a Shortcuts action on a Mac, a small script). Each key is made for one purpose: adding todos and notes, reading the library, or both.

- A key can never change your settings, start the assistant, trust a folder, install an add-on, sync, or make other keys. Those stay with the app itself.
- A key that reads sees what an add-on sees: your private folders are left out, under any spelling and through a link. A key that adds cannot write into a private folder either.
- Keys work only on this computer and only while Thoughts is running. A web page cannot use one.
- A key is shown once, when it is made. Thoughts keeps only a fingerprint of it, in `tokens.json` in the `.thoughts` folder, so that file never holds a usable key. Remove a key in Settings, and anything using it stops at once.
- A program finds Thoughts through the file `port` in the `.thoughts` folder (the port number and nothing else), and can ask Thoughts to prove it made the key before it sends it. `scripts/thoughts.sh` in the source code is a small command-line client that does this.

Give a key only to a program you trust with what the key allows: a reading key lets that program read everything outside your private folders.

## Deleting everything

1. **The app and its data.** The quickest way is **Settings › Maintenance › Remove Thoughts from this computer**: it removes chats, attachments, settings, customisations, the saved passwords and key, and the login item, then quits; your library stays. Then remove the app itself. By hand: quit Thoughts completely, remove the app, and delete the `.thoughts` folder in your home folder. This removes chats, attachments, settings and customisations. Also remove its entry from the things that open when you log in. See [Uninstall](../README.md#uninstall).
2. **Your library.** Delete the library folder. Until you do, it stays exactly as it is, readable with any text editor.
3. **Saved passwords.** In your computer's password store (Keychain Access on a Mac), delete the entries named `thoughts-git` and `thoughts-ai`. A password saved with the file option is removed with the `.thoughts` folder.
4. **Synced copies.** Delete the copy at the location you synced to, using that service.
5. **The AI provider.** Conversations already sent are handled under your account with the provider; use that account's settings to review or delete them. The assistant's own local session records are in the `.claude` folder in your home folder.

Related: [what Thoughts costs to run](COST.md) · [history and undo for your notes](HISTORY.md).
