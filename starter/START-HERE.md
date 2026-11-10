# Welcome to your library

This folder is your library: everything you think through with the assistant, kept as ordinary text files that belong to you. You can read them in the app, open them with any text editor, copy them, or back them up like any other folder.

## How it works

1. **Talk.** Open a chat and think out loud: a client question, a plan, a lesson, a decision. Write in whatever language you prefer; the assistant answers and keeps notes in the same language.
2. **Wrap.** When you are done, press **Wrap**. The assistant turns the conversation into tidy notes, decisions and todos, and shows you what it will save before anything is saved. Nothing you discussed is lost, and nothing is saved without you seeing it.
3. **Come back.** Next time, the home screen shows what you were working on. Pick any item and the assistant recaps where you left off.

## Everyday things

- **Capture an idea.** Say "new idea: ..." in a chat, or use the capture button. One line is enough; you can develop it later.
- **Start a client, matter, class or project.** Say "new client: ..." (or whichever word fits your work). It gets its own page with notes, todos and meeting notes.
- **Save a meeting.** Paste or dictate your rough notes and say "meeting notes". You get a clean, dated note, and the action items become todos.
- **Add a todo with a due date.** Add the task in the Todos view and pick a date. Or just tell the assistant: "remind me to send the report by Thursday" — it adds the todo with that date. You can also type `todo: ...` (or «مهمة: ...») in quick capture.
- **See what is due.** The Calendar view shows your todos on their due dates. While Thoughts is running, it shows a notification when a todo falls due. It cannot notify you when it is not running, so for dates that matter, export your due dates to your own calendar app from the Todos view.
- **Find things.** Use Search for any word or name. Or ask: "what did we decide about ...?"
- **Undo.** If you chose to keep a history when setting up (you can turn it on later in Settings), every save is kept. Open History to see what changed and undo a whole change, such as a Wrap. To go back on a single document, open its versions and restore an earlier one. Without history turned on, saved changes cannot be undone from the app.

## Making it yours

The library adapts to how you work. After a wrap, the assistant may suggest one small improvement, for example a checklist for something you do every month. It only suggests; nothing changes until you agree. You can also say "tune-up" at any time for a short review of how the library is working for you.

## Keeping things private

Your files stay in this folder. What you say in a chat, and the notes the assistant opens to answer you, are sent to the AI provider you signed in with. In Settings, under Privacy, you can mark folders the assistant is blocked from reading; they are still included in your backup. Avoid putting passwords, bank details or ID numbers in notes.

---

*For developers:* the library is plain markdown with optional git history. You can also run the assistant from a terminal inside this folder; `CLAUDE.md` holds the rules it follows and `.claude/skills/` holds the routines.
