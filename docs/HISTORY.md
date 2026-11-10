# History and undo for your notes

Thoughts can keep every earlier version of your library, so a change you regret can be put back. This page is about your notes. Changes to the app itself have their own History under Customise; see [Making Thoughts your own](CUSTOMISING.md).

العربية: [docs/ar/HISTORY.md](ar/HISTORY.md)

## It needs backup with history turned on

History for notes is kept by the same mechanism that backs up your library (a tool called git). It is on when you chose to keep a history during setup, or turned it on later in Settings. It works on this computer alone; an online backup location is optional.

**Without it**, History shows a message that there is no history for this library, and the Undo and restore actions described below are not available. A change that has been saved cannot be put back by the app. Your only copies are the ones you made yourself. Turning history on later starts from that day; it cannot recover earlier versions.

## What History shows

**History** lists the changes made to your library, newest first. Each entry has a date, a one-line description, the kind of change and the documents it touched:

- **Wrap**: what a Wrap saved from a conversation.
- **Assistant**: changes the assistant made during a chat or an automatic task.
- **Edit**: changes you made yourself in the app.
- **Sync**: changes that arrived from your backup location, for example from another computer.

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
- If sync is on, the history is uploaded with the library, so earlier versions are also available on your other computers. This also means that deleting something from a note does not delete it from the history or from your backup. See [Privacy](PRIVACY.md).
- While a Wrap is running, the library is briefly locked and restore waits until it finishes.
