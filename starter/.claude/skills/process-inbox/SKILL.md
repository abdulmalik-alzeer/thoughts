---
name: process-inbox
description: File everything waiting in the inbox — quick notes saved in the app or sent from the phone — into the library: todos with dates, notes and log entries in the right item, new items, meeting notes. Each one is deleted once it is filed. Use when the user says process the inbox, file my inbox, check the inbox, or after captures arrived from the phone.
---

# Process the inbox

Quick captures come in as one file each in `inbox/`: saved in the app, or sent from the phone. Turn them into properly filed library content, then clear them out. Write in the user's language and speak plainly (CLAUDE.md "Always" rules).

## Steps

1. **Look.** List the files directly inside `inbox/` (`.md`, `.markdown` or `.txt`). Ignore names starting with a dot: those are phone captures that have not finished arriving yet. If there are none, say the inbox is empty and stop. If some are still arriving, tell the user to try again in a minute.
2. **Read each capture.** The text after the `---` details block is what was captured. The `captured:` date, when there is one, is when it was said: work "tomorrow" or "Friday" out from that date, not from today, and say which date you used.
3. **Decide where each one belongs** — use INDEX.md and the item README summaries, never browse folders. One of:
   - **A todo** → a task line in the item's `todo.md` (or the general list if it belongs to no item), with `@due(YYYY-MM-DD)` when a date is given or implied. Follow the Todos rules in CLAUDE.md exactly.
   - **A thought, fact or decision about an existing item** → a dated entry at the top of that item's `log.md`, or a note in its `notes/` folder when it is longer than a few lines; add the file to the README `## Files` list.
   - **A new idea or work item** → create it the way the new-idea routine does (folder, README, log, INDEX line).
   - **A meeting or call** → a dated meeting note, the way the meeting-notes routine writes it.
   - **Something about the user themselves** → `me/`, if that folder exists. Never client or student details.
   - **A reference** (a procedure, a template, a link worth keeping) → `docs/` if the library has one, otherwise the closest item.
   - A capture that holds several things is split: each part goes where it belongs.
4. **Ask once for the unclear ones.** Gather every capture you cannot place with confidence into one short batch of questions (what it seems to be, where you would put it). Do not guess for client or other confidential material: it goes only into that client's own folder.
5. **Write, then delete.** For each capture, first write what it becomes, check it is really there, and only then delete the capture file from `inbox/`. A capture you could not file stays where it is. Keep the user's own words and figures exactly; tidy spelling and nothing else. Never copy a secret (password, PIN, full account or ID number) into the library: leave it out and say so in one line.
6. **Keep the library in step.** Update `updated:` in each README you changed and add INDEX.md lines for new items.
7. **Report in a few lines**, in the user's language: how many captures were filed and where each went (item names, not file paths), what was left in the inbox and why, and any date you worked out.
8. **Save** — when working outside the Thoughts app: one history entry `inbox: <number> captures filed`, then sync if a backup exists. Inside the app, skip this; the app saves.
