# Privacy

This page describes what the Thoughts app does with your information. It is a description of how the software behaves, not a legal document. If you handle confidential information for clients, students or an employer, check it against your own professional and contractual obligations.

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
| Saved passwords for sync and for the AI provider (if you entered any) | Your Mac's Keychain |

The assistant that Thoughts runs (Claude Code) also keeps its own record of sessions on your computer, in a folder named `.claude` in your home folder.

The part of Thoughts that runs in the background only accepts connections from your own computer.

## What is sent to the AI provider

Thoughts works by running an AI assistant on your behalf. To answer you, the assistant sends the AI provider:

- what you type, and files or images you attach;
- the contents of the files it opens while working on your request. It works inside your library, so this can be any note it considers relevant, not only the one on screen;
- the library's own instructions and routines.

This is sent through your own account with the provider (by default, your Claude account). How the provider stores, retains and uses that data is set by the terms and privacy settings of that account, not by Thoughts. If you entered a different provider's address in Settings, the same information goes to that provider instead.

Nothing is sent to an AI provider when you are only reading, searching or editing your library in the app.

## What sync does

Sync is off unless you turn it on. When it is on, Thoughts uploads your whole library, including its history of earlier versions, to the location you entered, and downloads changes from it. Who can see it there depends on that service and how you set it up. Chats and settings are not part of the library and are not synced.

If you keep your library inside a folder that another service backs up (iCloud Drive, Dropbox and the like), that service copies it too. That is between you and that service.

## No analytics

Thoughts does not collect usage statistics, crash reports or any other information about you, and has no account of its own. When it checks whether a new version is available, it contacts the page the app is downloaded from; it sends nothing about you or your library.

The diagnostics shown under Settings are produced on your computer, with passwords and keys removed, and go nowhere unless you copy and send them yourself.

## Keeping folders away from the assistant

In **Settings → Privacy** you can mark folders in your library as private. The assistant is told not to open, list or search them and is blocked from reading them, so their contents are not sent to the AI provider. They remain ordinary folders on your computer, and they are still included in sync.

Good candidates: anything a client or employer has asked you not to put through AI tools, and records about named individuals.

Some habits help regardless of settings:

- Do not paste passwords, bank or card numbers, or national ID numbers into a chat. The assistant is instructed not to save them in notes, but whatever you type has already been sent to the AI provider.
- The assistant keeps each client's details inside that client's own folder.
- The first time you paste something that looks confidential, the assistant reminds you that what gets saved will also go wherever your library is backed up.

## The trust question when opening a library

A library is a folder of text, but a folder can also carry settings that make the assistant or the sync tool run programs. When you open a library that Thoughts did not create itself — one you downloaded, or received from someone — Thoughts first lists anything in it that could run a program on your computer and asks whether you trust the folder. Nothing in the folder is run until you say yes. If those settings change later, you are asked again.

Libraries created by Thoughts contain one such setting, which fetches the latest synced copy when a session starts. Only trust a library from someone you would let run software on your computer.

## Deleting everything

1. **The app and its data.** Quit Thoughts, move it from Applications to the Trash, and delete the `.thoughts` folder in your home folder. This removes chats, attachments, settings and customisations.
2. **Your library.** Delete the library folder. Until you do, it stays exactly as it is, readable with any text editor.
3. **Saved passwords.** In Keychain Access, delete the entries named `thoughts-git` and `thoughts-ai`.
4. **Synced copies.** Delete the copy at the location you synced to, using that service.
5. **The AI provider.** Conversations already sent are handled under your account with the provider; use that account's settings to review or delete them. The assistant's own local session records are in the `.claude` folder in your home folder.

Exact commands for step 1 are in [DEVELOPMENT.md](DEVELOPMENT.md#uninstall).
