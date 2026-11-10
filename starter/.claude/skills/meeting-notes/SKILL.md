---
name: meeting-notes
description: Turn pasted, typed or dictated rough notes from a meeting, call or class into a clean dated meeting note with attendees, decisions and action items, and add the action items as todos with due dates. Use when the user pastes meeting notes or a transcript, says meeting notes, minutes, "I just had a call with…", or asks to write up a meeting.
---

# Meeting notes

Rough notes in, a clean record and a list of todos out. Write in the language of the user's notes; speak plainly.

## Steps

1. **Place it.** Work out which item the meeting belongs to (client, matter, class, project, idea) from the names mentioned and INDEX.md. Clear match → use it. No match → ask once: file it under an existing item, open a new one, or keep it as a general meeting (root `meetings/`). A meeting covering several clients gets one note per client — never mix clients in one file.
2. **Fill the gaps — one batch, only what is missing and matters:** the date (default today; "yesterday" and the like are worked out from today's date), who attended, and for action items with no owner or date, who and by when. If the user says "just save it", save with what you have and mark unknowns as "not stated".
3. **Write** `<area>/<item>/meetings/YYYY-MM-DD-short-topic.md`:

   ```markdown
   ---
   title: {Meeting title}
   date: {YYYY-MM-DD}
   attendees: [{Name (role)}, …]
   updated: {YYYY-MM-DD}
   summary: {One line: what the meeting settled.}
   ---

   # {Meeting title}

   ## Discussed
   - {Topic — the point made, in a line or two}

   ## Decisions
   - {What was decided, and by whom when it matters}

   ## Action items
   - {Owner}: {action} — by {date}

   ## Open points
   - {Unresolved questions, things to confirm}
   ```

   Headings in the user's language. Stay faithful: tidy the wording, keep the substance; do not add conclusions nobody reached. Mark anything unclear in the source as "(unclear in notes)" instead of guessing. Keep figures, names and dates exactly as given. Leave out an empty section.
4. **Todos.** Each action item becomes one line in the item's `todo.md` (root meetings: the general list): the owner's name first when it is not the user, and `@due(YYYY-MM-DD)` at the end whenever a date was given. Edit in place; no duplicates.
5. **Update the item:** add the meeting to the README `## Files` list; refresh `## Current state` and any key dates the meeting changed; set `updated:`; add a log entry of two or three lines pointing to the meeting note; update the INDEX line.
6. **Privacy:** leave out passwords, ID and bank numbers even if they were said. If the notes contain confidential third-party material and this is the first time in the conversation, give the one-line reminder from CLAUDE.md.
7. **Report back** in a few lines: where it was filed (by item name), the decisions, and the todos with owners and dates — so the user can correct a wrong date straight away. Offer, in one line, to draft a follow-up message to the attendees; if they accept, put it in the item's `outputs/` — never send anything.
8. **Save** — outside the Thoughts app only: `<item-folder>: meeting notes YYYY-MM-DD`.
