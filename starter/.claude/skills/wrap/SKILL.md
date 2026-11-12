---
name: wrap
description: End-of-session routine — turn the discussion into the item's notes, log and todos (with due dates), update INDEX.md, and finish with at most one suggestion for improving how the library works. Use when a discussion winds down or the user says wrap, save, or done for now.
---

# Wrap a session

Turn the discussion into files a future session can resume from in one short read.

**Language:** write every title, note, log entry and todo in the user's language — the language of the conversation, or of the existing file when adding to one. **Plain words:** when reporting to the user, say what was kept, not which files changed.

For **each item touched this session** (idea or work item), do steps 1–7. Talk that belongs to no item: todos go to the general list; anything worth keeping becomes a new item (new-idea routine) or, for a meeting, a note in root `meetings/`.

## Steps

1. **Log entry**, added at the top of `log.md`:

   ```
   ## YYYY-MM-DD — <topic of the session>
   - Decided: … → [note](notes/<topic>.md)
   - Learned: …
   - Open: …
   - Next: …
   ```

   A distillation, not a transcript — only lines that earn their place; leave out empty categories.

2. **Notes.** Lasting reasoning, designs, comparisons, research → create or update `notes/<topic>.md` (note details `title, updated, summary`). Notes hold *current* thinking — rewrite instead of appending contradictions. If the session was a meeting, write it as a meeting note instead (meeting-notes routine). Anything produced for someone else goes in `outputs/`. Keep the README `## Files` list in step.

3. **Action items → todos.** Go through the session for anything someone agreed or needs to do. Each becomes one line in the item's `todo.md` (general tasks: the general list):
   - Start with the owner's name when it is not the user.
   - When a date was mentioned or implied ("by Thursday", "before month-end", "next week's class"), work out the actual date from today's date and end the line with `@due(YYYY-MM-DD)`. No date mentioned → no due date; do not invent one.
   - Do not duplicate a todo that already exists; tick off (`- [x]`) those the session shows are finished.
   - Edit lines in place; never rewrite the file.

4. **README:** rewrite `## Current state` (10 lines at most), update `## Open questions` and any work-item sections that changed (key dates, contacts, scope), change `status:` if it moved, set `updated:` to today. Add `## Related` links for new connections.

5. **INDEX.md:** update the item's line (status · summary · date) and move it to the top of its area.

6. **Privacy check** before saving: no secrets or ID/bank numbers in what you wrote; no client's details in another item's folder or in `me/`.

7. **Long logs:** past about 400 lines, move older entries to `log-archive.md`, keeping the 5 newest.

8. **Save and sync** — only when working outside the Thoughts app (inside it, the app saves after showing the user a preview). One history entry per item — that item's folder plus INDEX.md — with the message `<item-folder>: <short summary>`; then sync once at the end if a backup location exists. If the backup has moved on, bring its changes in first and try again; never force. If sync fails, say plainly that everything is saved on this computer and will sync next time.

9. **Confirm** in two or three lines: what was kept, the todos added (with their dates, so the user can correct a wrong date), and whether it synced.

10. **Grow with the user — at most one suggestion.** Look back over this session and the last few log entries of the items touched. If you see a *recurring* pattern that the library does not yet support, propose **one** small improvement, in one or two plain sentences, and ask whether to apply it. Examples:
    - The same steps were done again by hand → "You have done this reconciliation three months running. Shall I save it as a routine so it becomes a ready checklist?"
    - A README section is always empty, or the user keeps adding the same extra one → adjust the work-item sections.
    - Meeting notes always need the same extra heading → add it to the meeting-notes routine.
    - The user corrects the same thing each time (tone, date format, a term) → record the preference.

    Rules: one suggestion or none — say nothing when no real pattern exists, and never repeat a suggestion the user declined (note declined ones under `## About this library` in CLAUDE.md as "Not wanted: …"). Apply only after the user says yes; then make the change to CLAUDE.md or the routine file, and tell them in one line what will be different from now on. A change to a routine such as meeting notes or wrap itself is not yours to make (the app keeps those): describe it in one or two sentences and ask the user to open Settings → Customisations → Routines, choose the routine and paste it into the box that asks for a draft, then review and keep it. When wrapping without the user present (the app wraps several chats at once), write the suggestion as the last line of your summary instead of asking, and change nothing.

**Spin-offs:** if the session produced something worth its own exploration, create it as a separate item (new-idea routine) and link both `## Related` sections.
