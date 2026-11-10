# Privacy

This page describes what the Thoughts app does with your information. It is a description of how the software behaves, not a legal document. If you handle confidential information for clients, students or an employer, check it against your own professional and contractual obligations.

العربية: [docs/ar/PRIVACY.md](ar/PRIVACY.md)

## The short version

- Your library and your chats are stored on your computer.
- When you chat, the conversation and the files the assistant reads to answer you are sent to the AI provider you signed in with.
- Sync uploads your library to the place you chose, and nowhere else.
- The app contains no analytics, tracking or advertising.

## What stays on your computer

| What | Where |
|---|---|
| Your library: notes, ideas, todos, meeting notes, outputs | The library folder you chose during setup |
| Chat history and attachments | A folder named `.thoughts` in your home folder |
| Settings, customisations and logs | The same `.thoughts` folder |
| Saved passwords for sync and for the AI provider (if you entered any) | Your computer's password store (the Keychain on a Mac). See [Saved passwords](#saved-passwords) |

The assistant that Thoughts runs (Claude Code) also keeps its own record of sessions on your computer, in a folder named `.claude` in your home folder.

The part of Thoughts that runs in the background only accepts connections from your own computer.

### Saved passwords

By default, passwords you enter for sync or for an AI provider are kept in your computer's password store: the Keychain on a Mac, and the equivalent system store on other computers.

Settings also offers a **file** option for the sync password. If you choose it, the password is saved in a file inside the `.thoughts` folder that only your user account on this computer can read. That file is **not encrypted**: anyone who can sign in as you, or who gets a copy of your home folder (an unencrypted backup, for example), can read it. On a Linux computer with no working password store, Thoughts falls back to the same kind of file. Prefer the default unless you have a reason not to.

## What is sent to the AI provider

Thoughts works by running an AI assistant on your behalf. To answer you, the assistant sends the AI provider:

- what you type, and files or images you attach;
- the contents of the files it opens while working on your request. It works inside your library, so this can be any note it considers relevant, not only the one on screen;
- the library's own instructions and routines.

This is sent through your own account with the provider (by default, your Claude account). How the provider stores, retains and uses that data is set by the terms and privacy settings of that account, not by Thoughts. If you entered a different provider's address in Settings, the same information goes to that provider instead.

The same applies to everything the assistant does for you without a chat window in front of you: **Wrap**, automatic tasks you have approved, and customisation requests.

Nothing is sent to an AI provider when you are only reading, searching or editing your library in the app.

## What sync does

Sync is off unless you turn it on. When it is on, Thoughts uploads your whole library, including its history of earlier versions, to the location you entered, and downloads changes from it. Who can see it there depends on that service and how you set it up. Chats and settings are not part of the library and are not synced.

If you keep your library inside a folder that another service backs up (iCloud Drive, Dropbox and the like), that service copies it too. That is between you and that service.

## No analytics

Thoughts does not collect usage statistics, crash reports or any other information about you, and has no account of its own. When it checks whether a new version is available, it contacts the page the app is downloaded from; it sends nothing about you or your library.

To suggest improvements (for example the monthly tune-up described in [Making Thoughts your own](CUSTOMISING.md)), the app keeps a small record on your computer of which screens you open and which customisations you kept or discarded. That record never leaves your computer on its own. If you accept a tune-up suggestion, a short summary of it is included in the request to the assistant, like anything else you send in a chat.

The diagnostics shown under Settings are produced on your computer, with passwords and keys removed, and go nowhere unless you copy and send them yourself.

## Private folders

In **Settings → Privacy** you can mark folders in your library as private.

What is enforced, by the app's background service:

- The assistant's file tools are blocked from reading, listing, searching or changing anything in a private folder. This applies to every kind of run: chats, Wrap, automatic tasks and customisation.
- Add-ons (the small tools and views created through Customise) cannot read private folders.
- The assistant is also told, in its instructions, to leave them alone.

What it does not do:

- **It is a safeguard, not a guarantee against every route.** The block applies to the assistant's file tools. If the assistant is allowed to run a command on your computer, a command could still reach a file in a private folder. If that risk is not acceptable for some material, do not keep it in the library at all.
- **Private folders are still backed up and synced.** They are ordinary folders in your library, so sync uploads them with everything else. To keep a folder out of sync, keep it **outside the library folder** (anywhere else on your computer) and link to it from your notes if you need to remember where it is.
- A private folder cannot be used with the assistant. You cannot ask it to summarise or search something it is blocked from reading.

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
| Sync is on | The whole library with its history, private folders included | The backup location you entered |
| You export, print or share an output yourself | That file | Wherever you send it |
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

A library is a folder of text, but a folder can also carry settings and instructions that make the assistant or the sync tool run programs. When you open a library that Thoughts did not create itself — one you downloaded, or received from someone — Thoughts first lists anything in it that could run a program on your computer, or that changes the assistant's standing instructions from the ones Thoughts ships, and asks whether you trust the folder. Nothing in the folder is run until you say yes. If those settings or instructions change later, you are asked again.

Libraries created by Thoughts contain one such setting, which fetches the latest synced copy when a session starts. Only trust a library from someone you would let run software on your computer.

## Deleting everything

1. **The app and its data.** Quit Thoughts completely, remove the app, and delete the `.thoughts` folder in your home folder. This removes chats, attachments, settings and customisations. Also remove its entry from the things that open when you log in. See [Uninstall](../README.md#uninstall).
2. **Your library.** Delete the library folder. Until you do, it stays exactly as it is, readable with any text editor.
3. **Saved passwords.** In your computer's password store (Keychain Access on a Mac), delete the entries named `thoughts-git` and `thoughts-ai`. A password saved with the file option is removed with the `.thoughts` folder.
4. **Synced copies.** Delete the copy at the location you synced to, using that service.
5. **The AI provider.** Conversations already sent are handled under your account with the provider; use that account's settings to review or delete them. The assistant's own local session records are in the `.claude` folder in your home folder.

Related: [what Thoughts costs to run](COST.md) · [history and undo for your notes](HISTORY.md).
