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

Open **Customise** and describe what you want in your own words.

### What you can ask for

- **Looks**: colours, spacing, a theme of your own.
- **Forms that save notes into a folder.** "A new-client intake form that saves each entry as a note in my clients folder."
- **Boards you can drag.** "A board of my engagements with columns for each status; dragging a card changes its status."
- **Trackers that remember data.** "A tracker for billable hours per client", "a reading log". The tracker keeps its own data between sessions.
- **Views built from your library.** "Todos due this week grouped by client", "all open questions across my matters".
- **How todos are grouped.** By list, by due date or by tag; sorted by due date; showing only what is due soon or overdue; with or without finished items.
- **The sidebar.** Hide items you do not use, change their order, rename them ("call Ideas 'Matters'"). Home, Settings and Help always stay.
- **Home.** Hide or reorder its sections, and place widgets where you want them: "put the hours tracker above my todos".
- **Shortcuts.** A button on Home that starts a chat you often start, or an extra action on a todo or an idea.
- **Automatic tasks.** Something the assistant does on a schedule: every day, every week, on weekdays only, or monthly on a day you choose or on the last day of the month ("on the last day of each month, draft my month-end checklist"). Also after each Wrap or when the app starts.

### The review step

Nothing you ask for is applied straight away.

1. The assistant prepares the change on the side. The app you are using is not touched.
2. You see a review: a plain-language summary of what will be different, the list of what was added or changed, and the permissions any new add-on asks for.
3. **Try it** shows the app with the change in place so you can click around. Try it never saves anything: close it and the app is exactly as before.
4. Choose **Keep** or **Discard**. Discard leaves the app exactly as it was.

Thoughts checks every change before you see it. If it finds a problem, such as an add-on that does not load or a setting it does not understand, the problem is listed in the review in plain words and **Keep is switched off** until it is fixed. Tell the assistant what the review says and it will correct the same change, or choose Discard.

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

An add-on that asks for nothing can only show things on screen. No add-on can reach the internet, read files outside your library, see your passwords, or change the app's own settings.

### Undo, any time

Every change you keep is recorded. Under **Customise → History** you can undo any single change, or go back to how the app was at an earlier point, however long ago. Undoing is itself recorded, so you can change your mind again.

If a customisation ever leaves the app unusable, the **Recovery** entry in the menu bar restores the standard app. Customisations cannot remove that menu.

### The monthly tune-up

About once a month, Home may show one suggestion titled as an app tune-up. If you accept it, the assistant looks at a short summary of how you used the app that month (which screens you opened, which shortcuts you used, which changes you kept or discarded) and proposes **one** customisation. It goes through the same review: Try it, then Keep or Discard.

The summary is kept on your computer and is only sent to the assistant when you accept the suggestion. You can dismiss the suggestion; a dismissed one does not come back.

### Sharing and moving customisations: packs

Under **Customise** you can export what you have made as a pack file.

- **A feature pack** holds one thing or a few: a board, a tracker, a theme. Importing a feature pack **merges** it with what you already have. Its add-ons and automatic tasks are added, its Home and sidebar choices are combined with yours, and its styling is added alongside your own. It does not replace your other customisations. Importing a newer version of the same pack updates that pack's part only.
- **A full pack** holds everything you have changed, for moving to another computer or reinstalling.

An imported pack is never applied directly. It goes through the same review, with Try it, Keep and Discard. A pack made for a newer version of Thoughts than yours is not applied; the review says so.

Only import packs from people you trust: a pack can change how the app behaves.

### What happens when Thoughts is updated

Thoughts tells you when a new version is ready. An update never touches your library.

For your customisations, Thoughts compares three things: the original, your version, and the new version.

- Parts only you changed stay as you made them.
- Parts only the new version changed are updated.
- Where both you and the new version changed the same thing, Thoughts asks you, item by item:
  - **Keep mine** — your version stays; you do not get the new behaviour for that part.
  - **Use new** — the new version replaces yours.
  - **Let the assistant merge** — the assistant combines the two and shows you a review first, like any other change.
- If a new version drops something you were using, Thoughts tells you and leaves your copy in place.

Whichever you choose can be undone from History.

### What is not possible yet

- **Changing the built-in screens beyond the options above.** Chats, Docs, Todos and Settings can be arranged through the documented options (todo grouping, the sidebar, Home), and you can add your own views beside them, but you cannot redesign the screens themselves.
- **Fonts and logos.** The app's typefaces and its icon are fixed. Colours and spacing can change.
- **Anything outside the customisation layer.** A customisation cannot reach the internet, connect to other apps or services, read files outside your library, change how the assistant itself works, or change sync and privacy settings.
- **Automatic tasks while the app is closed.** They run when Thoughts is open (its background service is running), one at a time.
- A customisation cannot change your library on its own, except an add-on saving into the folders you approved, and it cannot remove the Recovery menu.

If you ask for something outside these limits, the assistant should say so instead of producing something that half works.

## Tips

- Ask for one thing at a time. Small changes are easier to judge in a review.
- Say what you want to see, not how to build it: "I want to see overdue todos in red at the top".
- If a result is close but not right, say what is off. The assistant adjusts the same change.
- Use Try it before Keep, especially for boards and forms: click the buttons and check that things land where you expect.
