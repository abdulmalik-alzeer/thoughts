# History and undo for your notes

Thoughts keeps every earlier version of your library, so a change you regret can be put back. This page is about your notes. Changes to the app itself have their own History under Customise; see [Making Thoughts your own](CUSTOMISING.md).

العربية: [docs/ar/HISTORY.md](ar/HISTORY.md)

## History is on for every library

The history of changes is its own switch. It is on from the start, whichever backup you chose (**No backup**, **Cloud folder**, **Online location** or **Drive or folder**), and you can turn it off and on again with **Keep a history of changes on this computer** under Settings → Library & backup.

- **Where it is kept.** On this computer, in Thoughts' own data folder, outside your library folder. Nothing is added to your notes. A cloud drive that syncs your library (iCloud Drive, Dropbox and the like) never sees the history, so it cannot damage it. The one exception is a library that was set up with an **Online location** from the beginning: its history sits inside the library folder, which is how that kind of backup normally works.
- **It is not a backup.** The history lets you undo changes. It does not protect your notes if this computer is lost or damaged; for that, choose a backup. With **Online location** or **Drive or folder** the history is sent along with the library, so there it is always kept.
- **It needs a free tool called Git.** If Git is not installed, History says so and Settings → Library & backup offers to install it. The history starts by itself once Git is there. Until then a change cannot be undone.
- **A library from before version 0.0.9** that used **No backup** or **Cloud folder** had no history. It gets one the first time the new version starts: the library as it stands becomes the first entry, labelled **Start**. Your files are not touched, and nothing from before that day can be recovered.
- **If you move or rename the library folder** and choose it again in Settings, a new history starts for it at its new place.

**If you switch it off**, History shows a message that the history is off, and the Undo and restore actions described below are not available. A change that has been saved cannot be put back by the app. Your only copies are the ones you made yourself. What was recorded before stays, and when you switch the history back on, everything you changed in between appears as one entry.

## What History shows

**History** lists the changes made to your library, newest first. Each entry has a date, a one-line description, the kind of change and the documents it touched:

- **Wrap**: what a Wrap saved from a conversation.
- **Assistant**: changes the assistant made during a chat or an automatic task.
- **Edit**: changes you made yourself in the app.
- **Sync**: changes that arrived from your backup location, for example from another computer. Edits you made here and had not saved to the history yet are recorded as your own edits before anything arrives.
- **Combined**: changes from another computer combined with yours after a clash (see below). This entry cannot be undone in one step; restore the documents you need one by one instead.
- **Start**: the library as it was when its history began. It is the oldest entry and cannot be undone, because nothing exists before it.

Each entry lists the documents it touched. Choose **What changed** under it to see, document by document, the lines it added and removed, with a few unchanged lines around them; a document's name opens it. The newest changes are shown first; **Show older changes** at the bottom goes further back, all the way to the first entry.

## Before a Wrap is kept

When your library keeps a history, a Wrap does not write into your library straight away. You tick the chats to save, read the plan (and can correct what is saved from a chat, or ask for a different plan), and after you approve it the assistant writes the changes into a copy of your library. You then see every document it changed, line by line, and choose **Keep these changes**, **Change this** (say what should be different) or **Discard**. You can go on working meanwhile; if you change the same lines of a document before keeping, Thoughts says so and offers to write the Wrap's changes again on your library as it is now.

## Undoing a Wrap

If a Wrap saved something you did not want, choose **Undo this Wrap** on the Wrap's own page, or open **History**, find that Wrap and choose **Undo**. Everything that Wrap changed goes back to how it was before: notes it wrote are removed, notes it edited return to their earlier text, todos it added disappear.

- The undo is itself recorded in History, so you can change your mind again.
- The conversation is not deleted. You can Wrap it again.
- If you or the assistant have since changed the same documents, Thoughts may not be able to undo the whole Wrap cleanly. It then tells you which documents are in the way and changes nothing. Restore those documents one by one instead (below).

Any other entry in History can be undone the same way.

## Undoing what one answer changed

When the assistant changes your library during a chat or an automatic task, a line under its answer says so, for example **Changed 3 notes**. Choose it to see every document that answer changed, line by line.

- **Undo these changes** puts back only that answer's documents. Anything you or another chat changed since, in other lines of the same documents or in other documents, stays as it is.
- If a later change touched the same lines, Thoughts tells you which documents are in the way and changes nothing. Edit those documents yourself, or restore an earlier version (below).
- The undo is recorded in History too, and **Put them back** makes the answer's changes again.
- If you saved a document in the app while the answer was being written, or another chat was working in your library at the same time, those changes are never counted as the answer's: the line under the answer names them, and Undo leaves them alone. Changes made outside the app at the same time (in another editor, for example) cannot be told apart from the answer's.

## Restoring an earlier version of one document

Open the document, then open its **History** (document versions). You see every saved version of that document. Pick one: **What's different** shows how it differs from the text as it is now, and **Whole text** shows it in full. Choose **Restore** to put that version back.

- Only that document changes. The rest of the library stays as it is.
- The version you replaced is not lost: it is in History too, and can be restored in turn.
- A document you deleted can be brought back the same way, from the History entry that removed it.
- A document you moved or renamed keeps all its versions, including those from before the move. Versions from before show the name the document had then, and restoring one puts its text back into the document where it is now, with its links pointed again at what they meant from the new place.

## When the same document changed on two computers

If a Sync finds that a document was changed here and on another computer in the same places, it stops and asks you about each such document:

- **Keep mine**: your version wins where the two disagree.
- **Use the other**: the other computer's version wins where they disagree.
- **Keep both**: your version stays as it is, and the other computer's is saved next to it, with "(other device)" at the end of its name.

**Compare** shows the two versions side by side in one view. **For all** sets every document at once. Changes made to different parts of a document are kept either way, and a backup of this computer's version is always kept first.

## Repairing the history

Sometimes the history is left in a state where nothing can be saved or synced: a sync or a combination of changes that stopped halfway (the computer was turned off, the app was closed), a lock left behind by a program that stopped, or a history moved off its usual line by another program. When that happens, History shows a **History check** at the top and errors offer **Repair history**. You can also run the check at any time: **Check the history for problems**, at the bottom of History.

- The check says what is wrong in plain words.
- **Repair history** first keeps a copy of your library exactly as it is in the folder, then puts the history right.
- Afterwards it lists every document that is different from that copy. **Compare** shows the difference and **Bring back** restores that document from the copy (as a new entry, which can itself be undone).
- If part of the history is damaged and cannot be read, it can **start again**: from your backup location if you have one, otherwise from your library as it is now. Your notes are not touched, and the damaged history is set aside on this computer, not deleted.

## Backup locations

With **Online location** or **Drive or folder** as the backup (Settings → Library & backup), Thoughts sends your library, with its history, to every location in the list: when you press **Sync**, or after every Wrap if you chose that. You can mix both kinds. Changes from your other computers are received from the location marked **main**.

**A drive or folder.** Choose **Drive or folder**, then **Choose a drive or folder…** and pick a folder on an external drive, a USB stick or a network folder. Thoughts keeps the backup there in one folder named after your library (for example `Notes.git`). It is not a copy you open and read: it is the library and its history in the form Thoughts can bring back.

- **When the drive is not plugged in**, Sync says so plainly and sends the library to your other locations, if you have any. Plug the drive in and press Sync again. The location shows **Not connected right now** until then.
- **On a new computer**, or after reinstalling, set up an empty library, choose **Drive or folder** and pick the same drive: Thoughts finds the earlier backup of this library and the next Sync brings your notes back.
- A drive that already holds the backup of a *different* library with the same name is refused, so two libraries are never mixed. A folder inside the library, or one a cloud drive keeps in sync, is refused too.
- Removing a location stops sending to it. The copy kept there is not deleted.

**How the backup stands.** Under **Backup locations** each location shows when it last received your library, how many changes it does not have yet and, when the last try did not work, why. A line above the list sums it up. The Sync button gets a dot when something needs you: a location that failed (red), or changes that have waited more than seven days, or a backup that has never happened (yellow).

**Sign-ins.** An online location that starts with `https://` signs in with a username and a password or access token, saved in this computer's password store (or, if you chose it, a private file in Thoughts' own folder — never in the library). Beside each such location you see who it signs in as, with **Update sign-in** (for a token that expired: paste the new one; Thoughts checks it with the location straight away) and **Forget sign-in**. When you remove the last location on a site, its sign-in is forgotten too. A saved password is only ever sent to an `https://` address; an address that starts with `http://` is not encrypted and never gets one. When a Sync fails because a sign-in is needed, its message has an **Update sign-in** button that takes you there.

**Locations that sign in with a key** (`ssh://…` or `git@…`) use this computer's own SSH key. Thoughts cannot type a key's passphrase: if your key has one, add it to your computer's key agent first. The first time this computer connects to a server it does not know, Sync stops and the location offers **Check the server…**: it shows the server's key fingerprints, to compare with the ones the server's owner publishes (GitHub, GitLab and others list theirs), and trusts the one you pick, exactly as answering "yes" in a terminal would. If a server's identity has *changed* since the last connection, nothing is sent and nothing is offered: ask whoever runs the server.

**Editing while a Sync runs.** You can keep working. A Sync only holds your files for the moments when it records them or puts in what arrived; a save, a ticked todo or an import made meanwhile waits those moments out and goes through, and is included in the backup. (While a Wrap or a Reorganize is being applied, an edit is still refused until it has finished.)

## Good to know

- History covers the library only. Chats, settings and customisations are not part of it.
- History is saved at certain moments: after a Wrap, after the assistant finishes a piece of work, and a few seconds after you save a document, change a todo or an idea's status. Saves made one right after another become one entry. Text you typed and never saved is not in it.
- With **Online location** or **Drive or folder** as the backup, the history is sent with the library, so earlier versions are also available on your other computers. This also means that deleting something from a note does not delete it from the history or from your backup. See [Privacy](PRIVACY.md).
- The history takes disk space of its own: about the size of your library to begin with, growing slowly as you make changes.
- While a Wrap's changes are being kept (or, in a library without a copy to write into, while it saves), the library is briefly locked and restore waits until it finishes.
