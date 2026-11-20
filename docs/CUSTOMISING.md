# Making Thoughts your own

Most apps are the same for everyone. Thoughts is meant to change shape around the way you work, and you do not need to know anything technical to change it: you ask, look at the result, and decide whether to keep it.

العربية: [docs/ar/CUSTOMISING.md](ar/CUSTOMISING.md)

There are two things you can shape.

## 1. Your library — how your work is organised

Your library adapts through use.

- After a **Wrap**, the assistant may make one suggestion based on something it has seen you do repeatedly, for example: "You have prepared this report three months running. Shall I save the steps as a checklist?" It only suggests. Nothing changes until you say yes.
- Say **"tune-up"** in a chat whenever you like. The assistant looks at how you actually use the library and proposes up to five adjustments: a new checklist, a renamed section, a folder that is no longer needed.
- Or simply ask: "From now on, add a 'Risks' heading to every meeting note." "Call them 'files', not 'matters'." "Write todos for other people with their name first."

These changes are ordinary edits to your library. When history is turned on they appear in **History** and can be undone like any other change; see [History and undo](HISTORY.md).

## 2. The app — how it looks and what it shows

Press **New customisation**: on the **Customise** page, in Help or Settings, or from the command palette. A chat opens, just like a new chat, where you describe what you want in your own words and talk it through with the assistant until it is right. Each customisation has a chat of its own, listed with your other chats, so you can come back to it later.

### What you can ask for

- **Looks**: colours, spacing, a theme of your own.
- **Forms that save notes into a folder.** "A new-client intake form that saves each entry as a note in my clients folder."
- **Boards you can drag.** "A board of my engagements with columns for each status; dragging a card changes its status."
- **Trackers that remember data.** "A tracker for billable hours per client", "a reading log". The tracker keeps its own data between sessions.
- **Views built from your library.** "Todos due this week grouped by client", "all open questions across my matters". A page you add gets its own place in the sidebar and opens with the same motion as the app's own pages (or none, when Settings › Accessibility asks for less motion).
- **How todos are grouped.** By list, by due date or by tag; sorted by due date; showing only what is due soon or overdue; with or without finished items.
- **The sidebar.** Hide items you do not use, change their order, rename them ("call Ideas 'Matters'"), give them other icons, put them in headed groups ("a Work group with Clients and Docs"), and choose the page the app opens on. You can also do all of this yourself: press **Edit sidebar** under the page list (or find it in the command palette), drag the entries or move them with the up and down buttons, and press Save. Each save is one entry in History that you can undo. Home, Customise, Settings and Help always stay, and Customise keeps its name and icon so you can always find your way back.
- **Home.** Hide, reorder or resize its sections (the greeting and message box is one of them too), put todos and chats side by side, and place widgets where you want them: "put the hours tracker above my todos".
- **The app's own pages, inside and out.** On Home, Chats, Ideas, Todos, Docs, Folders, Inbox, Graph and Usage you can change the heading and add a line under it, reword buttons and hints, and hide small parts you never use. Each page is also made of named **sections** (for Todos: the filters, the new-todo box, the lists, the calendar) that can be put in another order, hidden, made narrower so two sit side by side, swapped for an add-on's box, or have an add-on's box placed between them: "put the lists above the filters", "hide the quick-add box", "show my hours box beside the lists". Settings' categories can be reordered and some hidden; the ones about privacy and customisation always stay.
- **A page of your own design in place of one of the app's.** Any of the pages above can be replaced as a whole by a page the assistant writes for you ("redo my Todos page as a board"). It gets the same data the usual page uses (with the permissions you approve), the app's own wording in your language, and the app's look. The usual page stays one press away (**Show the usual page**). If your page fails or is missing, the app shows its own page with a note saying why, with **Try again** and **Keep the app's page**; the latter keeps the usual page for that page alone, as an entry in History you can undo. The app's own questions, warnings and error messages always stay as they are.
- **Shortcuts.** A button on Home that starts a chat you often start, or an extra action on a todo or an idea.
- **Automatic tasks.** Something the assistant does on a schedule: every day, every week, on weekdays only, or monthly on a day you choose or on the last day of the month ("on the last day of each month, draft my month-end checklist"). Also after each Wrap or when the app starts.

### The review step

Nothing you ask for is applied straight away.

1. The assistant prepares the change on the side. The app you are using is not touched.
2. You see a review. It starts with **what changes**, which Thoughts reads from the change itself rather than from what the assistant says about it: each new, changed or removed add-on and page, with what it may do and, for one that changes, exactly what it gains or loses ("Now also: create, change and delete files in clients/"); each Home shortcut with the request it will put in front of the assistant; what is hidden, shown, renamed or reordered on Home and in the sidebar; how todos are grouped or shown; themes; the look; automatic tasks. Add-ons and pages the change leaves alone are folded away under **Already installed**. Then comes the assistant's own description of the change, labelled as such, and last the list of files, with **Show the exact changes** for anyone who wants every line.
3. **Try it** shows the app with the change in place so you can click around. Forms, boards and trackers work as they will, but nothing is saved: a message says what an add-on would have saved and where, and what it saved while you try is gone when you stop. Close it and the app is exactly as before.
4. Choose **Keep** or **Discard**. Discard leaves the app exactly as it was.
   Or choose **Continue chatting** when it is close but not quite right. The app is back to how it was, as with Discard, but this version is not thrown away: you return to the chat, say what to change, and the assistant builds on what it made. The next version comes back to the same review. Under the chat, **Review it again** brings back the version you set aside.

Thoughts checks every change before you see it. If it finds a problem, such as a setting it does not understand or a file that is not written correctly, the problem is listed in the review in plain words and **Keep is switched off** until it is fixed. Tell the assistant what the review says and it will correct the same change, or choose Discard.

Thoughts also test-runs every new or changed add-on and page in a sealed box with no access to your data. Keep waits until that test has finished. If a part does not start, the review says which one and why, and Keep stays off unless you tick **Keep it anyway** (that part will not work until it is fixed). Asking the assistant to fix it is usually the better choice.

The review also tells you, in plain words:

- **How the look changes.** "Colours and style settings only" means the change sets colours, shadows and corner shapes and nothing else. If it also moves, resizes or hides things, the review says so, and it warns when a style change prints words of its own, covers parts of the app, hides something, restyles the app's own questions and warnings, or tries to load a file. A theme never needs those.
- **If the assistant did not finish.** When it was stopped or ran into a problem, the change may be incomplete. Keep stays off until you tick a box saying you want it anyway. Discarding and asking again is the safer choice.
- **If the change no longer fits.** If you undo something, go back to an earlier point or update the app while a change is waiting, Thoughts carries the waiting change over, so keeping it never brings back what you undid. When the two cannot be combined, the review says the change no longer fits; discard it and ask again.
- **What a pack says about itself.** A pack's own description is shown as a quotation, marked as its maker's words. Thoughts has not checked it.
- **Hidden characters.** Text you are asked to approve, such as what an automatic task will tell the assistant, cannot contain invisible characters. Where free text has any, they are written out like `[U+202E]` so you see them.
- **Files that changed while the change was being prepared.** The assistant may only write inside its own copy of your customisations. If files in your library changed while it worked, the review lists them: under a warning when the assistant's own steps name them, and otherwise as changed in the meantime (most likely by you or another chat). None of them is part of the change.

### More than one change at a time

You can ask for something new, import a pack or let the assistant merge an update while another change is still waiting for review. The waiting change is not replaced: it is set aside, Thoughts tells you so, and it stays listed under **Customise** until you deal with it. Each change is reviewed and kept on its own; open one and the one you were looking at waits in its place. A change you set aside is carried over onto your app as it is when you come back to it, so keeping it never undoes another change you kept in the meantime. Continuing the chat that prepared a waiting change picks that change up again.

### Changes made outside the app

Only what you kept is ever loaded. If a customisation file is changed in another way (edited by hand, by another program, or by an assistant working outside its usual limits), Thoughts takes that change out before it can take effect and shows it to you as a review marked **Changed outside the app**. Keep it only if you made the change yourself or expected it. Assistant chats can read your customisations but are not allowed to write them, whatever mode they run in.

If the change includes an automatic task, you are asked to approve the task separately before it can ever run.

### Permissions, in plain words

An add-on can do only what it has asked for, and the review shows the list before you choose Keep.

| The review says | What it means |
|---|---|
| Read your library | It can open and search your notes. Never folders you marked private. |
| Save into: *folder names* | It can create, change and remove text files, but only inside the folders named. Nowhere else in your library. |
| Read your todos / Change your todos | It can list todos; or also add, tick, edit and remove them. |
| Read your ideas / Change an idea's status | It can list your ideas and items; or also move one between statuses. |
| Remember its own data | It keeps one small private store for itself, such as the hours you logged. It is not part of your library. |
| Start a chat | It can open a chat with a prepared message. You see the chat like any other. |
| Sees only these folders: *folder names* | Whatever else it may read (notes, todos, ideas), it sees only what is inside the folders named. It also changes or deletes only files inside them; where else it saves, it can only add new files. |
| Hand you files to save | It can offer you a spreadsheet, text or data file, such as this month's hours. The app's save dialog opens and you choose where; nothing is saved unless you do. |
| Show notifications | It can show a short notice under its name, also on your computer when Thoughts is in the background and notifications are on. |
| Ask your assistant | It can ask the assistant a question and show you the answer, such as a summary of a client. This uses your allowance. The assistant sees only what the add-on sends it. |

An add-on that asks for nothing can still add pages, boxes on Home, commands, and buttons on your todos and ideas, show short messages, ask you yes-or-no questions in the app's own dialog and print its page. It sees a todo or an idea only when you use its button on it, and it reads nothing else. No add-on can reach the internet, read files outside your library, see your passwords, or change the app's own settings.

### Undo, any time

Every change you keep is recorded. Under **Customise › History** you can undo any single change, or go back to how the app was at an earlier point, however long ago: **Show older changes** at the end of the list goes all the way back to where your app started. Undoing is itself recorded, so you can change your mind again. The starting point and the entries the app makes for itself (when an update refreshes its own files) cannot be undone on their own; you can still go back to them.

If a customisation ever leaves the app unusable, even with a blank or frozen window, the **Recovery** menu in the menu bar still works, because the desktop app does it itself: undo the last change, go back to an earlier version, discard a change waiting for review, switch customisations off (they stay off until you switch them back on), reload, or open a fresh window. Each step says what it will change and asks first, and afterwards offers to put things back. Customisations cannot remove that menu.

### Your add-ons, one at a time

Under **Customise › Add-ons** every add-on and page you have is listed with what it may do.

- **Switch off** stops one from loading without removing it, and **Switch on** brings it back. Useful when one misbehaves.
- **Remove** takes it out of the app.
- **Take away** beside a permission withdraws just that permission. The add-on may then stop working in the parts that needed it.

Each of these is one entry in History and can be undone, even when later changes built on the add-on.

What an add-on saved (an hours tracker's entries, for example) stays on this computer when you remove the add-on, so undoing the removal brings it back with its data. Thoughts also keeps up to twelve earlier copies of each add-on's saved data (the newest few, then one a day), so one bad save never loses it: open **Earlier copies** to put one back. The saved data travels with you in the move file (**Settings › Maintenance › Move to another computer**).

### Ready-made packs

Under **Customise › Ready-made** are packs that come with Thoughts: billable hours, a status board, a new-client form, overdue todos on Home, a weekly review, todos grouped by date and a sand theme. Choosing one opens the same review as any other pack. Nothing is switched on until you keep it, and nothing is downloaded.

### The monthly tune-up

About once a month, Home may show one suggestion titled as an app tune-up. If you accept it, the assistant looks at a short summary of how you used the app that month (which screens you opened, which shortcuts you used, which changes you kept or discarded) and proposes **one** customisation. It goes through the same review: Try it, then Keep or Discard.

The summary is kept on your computer and is only sent to the assistant when you accept the suggestion. You can dismiss the suggestion; a dismissed one does not come back.

### Suggest automations

Thoughts can look at how you use it and suggest things that would save you work. Open **Customise › Automate** (Home and the command palette also have **Suggest automations**) and press **Suggest automations**. Nothing is looked at until you press the button, nothing runs in the background, and you are never interrupted with suggestions.

**What your assistant is sent.** One short summary of how you use Thoughts in the library that is open. You can read it word for word before anything goes: press **See what will be sent**. It holds:

- the titles of your chats from the last three months and the beginning of your first message in each, as one line of at most 200 characters (email addresses in it are replaced by "[email address]");
- how often certain kinds of request come up (writing email, meetings, summaries, translations, reports, invoices, posts, plans, letters, CVs), and openings you use again and again;
- how many other runs there were (Wraps, customisations, automatic tasks), as numbers only;
- the names of your routines, helpers, automatic tasks, add-ons and added pages, your Home buttons and the pages you hid;
- how many people your People list holds, and how many email addresses your chats mention (a number, never the addresses);
- how many files each top-level folder holds, of which kinds, and how many changed in the last month (folder names only);
- how many todos are open, done, overdue or repeating, and the headings of your general list;
- which screens and shortcuts you use, only while **Keep these notes and show suggestions** is on (Settings › Privacy & cost);
- which assistant program you use, and the app's language.

**What it is not sent:** the contents of your notes, documents or todos, the rest of your messages, the assistant's answers, email addresses, and anything from other libraries. **Private folders are left out completely**: they are not counted, and a chat that mentions or attaches anything in a private folder is not in the summary at all.

**It counts toward your usage.** Each press is one request to your assistant, so it costs the same as a short question and counts toward your plan or spending limit. The button is unavailable while the month's spending limit or your plan's limit is reached, while you are signed out, or while suggestions are already being made. You can leave the page meanwhile; the suggestions appear when they are ready.

**What you get.** Up to five suggestions, the most useful first. Each says what it would do, why you are seeing it (the numbers it came from) and what building it involves. **Not for me** removes one (with Undo); **Build it** starts building it. A suggestion you built or turned down does not come back, and your assistant is told not to suggest it again. If there is not enough to go on yet, Thoughts says so. Suggestions are kept on this computer, not in your library, so they are not synced.

**The four kinds, and how each is built and undone:**

- **Routine** — steps the assistant follows whenever you ask for that kind of work. Building one shows you its steps in the routine editor first (for the app's own ready-made routines at once; otherwise your assistant drafts them, which is one more request, and you are told before it runs). Nothing is saved until you press **Keep**. It then appears under **Customise › Automatic tasks**, where you can change or remove it, and the change is in your library's History.
- **Helper** — a specialist your assistant can hand one part of a chat's work to, for example checking an email draft for tone. You review its instructions before keeping it, exactly like a routine. A helper never gets more permissions than the chat it works in. Helpers exist only with Claude Code.
- **Connection** — linking Thoughts to another service, such as your email account. Thoughts never signs in, stores a password or connects anything by itself: you get plain steps to follow, and **Walk me through it in a chat** opens a new chat with a ready-written request that you read and send yourself.
- **New page** — a page of its own in the sidebar. Building one starts a customisation chat that goes through the usual review: Try it, then Keep or Discard, and it can be undone from **Customise › History**.

**Example: someone who writes a lot of email.** If many of your chats are "write an email to …", you are likely to see:

1. **Email writing**, a ready-made routine. When you ask for an email, the assistant finds the person in your **People** list, writes in their language and in the tone that suits them, shows you the subject and the text, and ends with an **Open in Mail** link. That link opens your own mail app with the message filled in; you check it and press Send there. Thoughts never sends email and never signs in to your mail. If someone is not in your list yet, the assistant asks once who they are and offers to add them.

   While you build this routine you can fill in your **People** list: for each person their name, email address, role and organisation, how you know them, the language to write in, the tone (formal, friendly …) and any notes. You can add more people, change them or remove them later. The list is ordinary notes in your library, in a folder of its own, one page per person plus a front page that lists everyone; it is not an item and does not appear in your index. If you make that folder private, your assistant cannot read it and will ask you instead.
2. **Your email account**, a connection, which first explains that the **Open in Mail** links already work with any mail app and need nothing connected.

### Sharing and moving customisations: packs

Under **Customise** you can export what you have made as a pack file.

- **A feature pack** holds one thing or a few: a board, a tracker, a theme. Importing a feature pack **merges** it with what you already have. Its add-ons and automatic tasks are added, its Home and sidebar choices are combined with yours, and its styling is added alongside your own. It does not replace your other customisations. Importing a newer version of the same pack updates that pack's part only.
- **A full pack** holds everything you have changed, for moving to another computer or reinstalling.

When you share a pack you give it a name, and if you like your name and a version number. Every pack also has a **fingerprint**: a short code such as `3F9A-12C4-77B0`, calculated from exactly what the pack holds. Whoever opens the file sees the same code before anything is staged, so you can check over the phone that it is the pack you sent. If the code written in the file does not match what it holds, Thoughts says the pack was changed after it was made. The fingerprint and the pack's version are kept with its entry in History.

An imported pack is never applied directly. It goes through the same review, with Try it, Keep and Discard. A pack made for a newer version of Thoughts than yours is not applied; the review says so.

Only import packs from people you trust: a pack can change how the app behaves.

### What happens when Thoughts is updated

An update never touches your library.

To get a new version, choose **Check for Updates…** in the menu, or switch on **Check for new
versions automatically** under **Settings › Maintenance** to be told when one is out. Thoughts
says what changed and, when you agree, downloads the new version, checks that it is genuine and
restarts as it. It never installs anything by itself. While the project's repository is private,
testers need one step in a terminal first: see "Updates" in the README.

For your customisations, Thoughts compares three things: the original, your version, and the new version.

- Parts only you changed stay as you made them.
- Parts only the new version changed are updated.
- Where both you and the new version changed the same thing, Thoughts tells you once and lists those parts together under **Customise › App updates**. For each one it says what the new version changes (for example "Adds the Home shortcut Weekly review"), and you choose:
  - **Keep mine** — your version stays; you do not get the new behaviour for that part. The part stays listed, so you can still take the new version or merge later.
  - **Use the new version** — the new version replaces yours.
  - **Let the assistant merge** — the assistant combines the two and shows you a review first, like any other change.
- If a new version drops something you were using, Thoughts tells you and leaves your copy in place.

Whichever you choose can be undone from History.

### What is not possible yet

- **Changing where customisations are undone.** History, the review of a change, Customise, Rollback and the Recovery menu always stay as the app made them, so a customisation can never take away the way back. Settings cannot be replaced as a whole, and Help is not customisable. After an update, anything a customisation names that the new version no longer has is left out and the page says so; renamed sections keep working.
- **Fonts and logos.** The app's typefaces and its icon are fixed. Colours and spacing can change.
- **Anything outside the customisation layer.** A customisation cannot reach the internet, connect to other apps or services, read files outside your library, change how the assistant itself works, or change sync and privacy settings.
- **Automatic tasks while the app is closed.** They run when Thoughts is open (its background service is running), one at a time.
- A customisation cannot change your library on its own, except an add-on saving into the folders you approved, and it cannot remove the Recovery menu.

If you ask for something outside these limits, the assistant should say so instead of producing something that half works.

## Tips

- Ask for one thing at a time. Small changes are easier to judge in a review.
- Say what you want to see, not how to build it: "I want to see overdue todos in red at the top".
- If a result is close but not right, say what is off. The assistant adjusts the same change.
- Show instead of describing: in the customise chat, attach a screenshot of what you want changed, a picture whose colours you like, or a document to work from (the paperclip, pasting, or dragging a file in). The assistant can look at what you attach but not change it, and only this chat's files. Pictures guide the change; they cannot become part of it, because the app cannot load image files.
- Use Try it before Keep, especially for boards and forms: click the buttons and check, in the messages that appear, that things would land where you expect.
