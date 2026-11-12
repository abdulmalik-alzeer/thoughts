# Capture from your phone

Ideas and calls happen away from your desk. With a small shortcut on your iPhone, a thought you type, dictate or share from another app lands in your library's **inbox**, and shows up in Thoughts on your computer, ready to be filed.

العربية: [docs/ar/PHONE-CAPTURE.md](ar/PHONE-CAPTURE.md)

## Read this first: when it works

Thoughts does not run a server for your phone, and nothing is sent over the internet by the app. The shortcut simply saves a text file in your library's `inbox` folder. So it only works when **your library is in a folder your phone can reach**, most simply **iCloud Drive** (or another cloud drive that appears in the Files app on your iPhone, such as Dropbox).

- If your library sits only on your computer, or is backed up with an **Online location** (Git) and nothing else, the phone cannot write into it, and this guide does not apply. To use this, keep your library in a cloud folder: choose **Cloud folder** when you set a library up, or pick that folder under Settings → Library & backup.
- Your computer has to be on and online for the capture to arrive. It appears once the cloud drive has finished syncing, usually within a minute.
- Captures are plain text, stored in your library like everything else. Whoever runs the cloud drive keeps a copy, as with the rest of your notes.

## Where captures land

Every capture is one file in the `inbox` folder of your library, for example `2026-11-12-0930-phone.md`. The part of the name before the time tells Thoughts when it was captured. Thoughts shows the number waiting on **Home** and in the sidebar, and lists them under **Inbox**.

## Make the shortcut (about five minutes)

You need the **Shortcuts** app, which comes with the iPhone. Action names can differ slightly between iOS versions. Follow the meaning, and try it once at the end.

### 1. A shortcut you open yourself

1. Open **Shortcuts**, tap **+** to make a new shortcut, and name it **Thoughts**.
2. Add **Ask for Input**. Set the type to **Text** and the question to *What is on your mind?* (To speak instead of type, use **Dictate Text** here.)
3. Add **Format Date** on **Current Date**. Choose **Custom** and write `yyyy-MM-dd-HHmm`.
4. Add **Text** and write the formatted date followed by `-phone.md`, so it reads like `2026-11-12-0930-phone.md`.
5. Add **Set Name**: set the name of the **Provided Input from step 2** (the text you typed) to the text from step 4. Set it so the name keeps its ending `.md`.
6. Add **Save File**. Turn **Ask Where to Save** off, choose **iCloud Drive**, and for the destination pick your library's **inbox** folder. Turn **Overwrite If File Exists** off.
7. Tap **Done**. Then add it to your Home Screen, or give it to the **Action Button** or Siri ("Hey Siri, Thoughts").

### 2. A share-sheet action, for text and links in other apps

1. Make a second shortcut the same way, and call it **Send to Thoughts**.
2. Instead of **Ask for Input**, begin with **Shortcut Input**, the thing being shared. Open the shortcut's settings (the **i** button) and turn on **Show in Share Sheet**. Let it accept **Text**, **URLs** and **Safari web pages**.
3. Keep steps 3 to 6 above (name the file with the date, then **Save File** into the same **inbox** folder).
4. Now **Share → Send to Thoughts** appears in Safari, Notes, Messages and most other apps.

### 3. Test it

Run the shortcut and type "test". On your computer, open Thoughts: a number appears on **Home** and in the sidebar within a minute (or at once when you switch back to the window), and **Inbox** shows your test. Delete the test there.

If nothing appears, check in the **Files** app on the iPhone that the file is inside your library's **inbox** folder, and that your computer is online. A file whose name starts with a dot has not finished arriving from the cloud yet; Thoughts shows it as soon as it has.

## Dealing with the inbox

Open **Inbox** from the sidebar or from the number on Home. Each capture can be:

- **Made a todo**: it goes to your general todo list. A date at the end ("tomorrow", "by Friday") becomes its due date.
- **Filed under** one of your items: it is moved into that item's notes.
- **Deleted**. You can undo that right after.
- Or press **Process with the assistant**: one conversation files every capture where it belongs (todos with dates, notes, new items), asking you only about the unclear ones, and deletes each capture once it is filed.

See also: [Your privacy](PRIVACY.md) for what stays on your computer.
