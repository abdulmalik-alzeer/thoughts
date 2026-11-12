# History and undo for your notes

Thoughts keeps every earlier version of your library, so a change you regret can be put back. This page is about your notes. Changes to the app itself have their own History under Customise; see [Making Thoughts your own](CUSTOMISING.md).

العربية: [docs/ar/HISTORY.md](ar/HISTORY.md)

## History is on for every library

The history of changes is its own switch. It is on from the start, whichever backup you chose (**No backup**, **Cloud folder** or **Online location**), and you can turn it off and on again with **Keep a history of changes on this computer** under Settings → Library & backup.

- **Where it is kept.** On this computer, in Thoughts' own data folder, outside your library folder. Nothing is added to your notes. A cloud drive that syncs your library (iCloud Drive, Dropbox and the like) never sees the history, so it cannot damage it. The one exception is a library that was set up with an **Online location** from the beginning: its history sits inside the library folder, which is how that kind of backup normally works.
- **It is not a backup.** The history lets you undo changes. It does not protect your notes if this computer is lost or damaged; for that, choose a backup. With **Online location** the history is sent along with the library, so there it is always kept.
- **It needs a free tool called Git.** If Git is not installed, History says so and Settings → Library & backup offers to install it. The history starts by itself once Git is there. Until then a change cannot be undone.
- **A library from before version 0.0.9** that used **No backup** or **Cloud folder** had no history. It gets one the first time the new version starts: the library as it stands becomes the first entry, labelled **Start**. Your files are not touched, and nothing from before that day can be recovered.
- **If you move or rename the library folder** and choose it again in Settings, a new history starts for it at its new place.

**If you switch it off**, History shows a message that the history is off, and the Undo and restore actions described below are not available. A change that has been saved cannot be put back by the app. Your only copies are the ones you made yourself. What was recorded before stays, and when you switch the history back on, everything you changed in between appears as one entry.

## What History shows

**History** lists the changes made to your library, newest first. Each entry has a date, a one-line description, the kind of change and the documents it touched:

- **Wrap**: what a Wrap saved from a conversation.
- **Assistant**: changes the assistant made during a chat or an automatic task.
- **Edit**: changes you made yourself in the app.
- **Sync**: changes that arrived from your online location, for example from another computer.
- **Start**: the library as it was when its history began. It is the oldest entry and cannot be undone, because nothing exists before it.

Open an entry to see which documents changed.

## Undoing a Wrap

If a Wrap saved something you did not want, open **History**, find that Wrap and choose **Undo**. Everything that Wrap changed goes back to how it was before: notes it wrote are removed, notes it edited return to their earlier text, todos it added disappear.

- The undo is itself recorded in History, so you can change your mind again.
- The conversation is not deleted. You can Wrap it again.
- If you or the assistant have since changed the same documents, Thoughts may not be able to undo the whole Wrap cleanly. It then tells you which documents are in the way and changes nothing. Restore those documents one by one instead (below).

Any other entry in History can be undone the same way.

## Restoring an earlier version of one document

Open the document, then open its **History** (document versions). You see every saved version of that document. Pick one to read it, and choose **Restore** to put that version back.

- Only that document changes. The rest of the library stays as it is.
- The version you replaced is not lost: it is in History too, and can be restored in turn.
- A document you deleted can be brought back the same way, from the History entry that removed it.

## Good to know

- History covers the library only. Chats, settings and customisations are not part of it.
- History is saved at certain moments: after a Wrap, after the assistant finishes a piece of work, and when you save an edit. Text you typed and never saved is not in it.
- With **Online location** as the backup, the history is uploaded with the library, so earlier versions are also available on your other computers. This also means that deleting something from a note does not delete it from the history or from your backup. See [Privacy](PRIVACY.md).
- The history takes disk space of its own: about the size of your library to begin with, growing slowly as you make changes.
- While a Wrap is running, the library is briefly locked and restore waits until it finishes.
