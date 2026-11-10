---
name: new-idea
description: Add a new item to the library — an idea, or a work item such as a client, matter, engagement, class or project — create its folder, list it in INDEX.md, and start the discussion. Use when the user wants to capture a new idea or open a new client, matter, class or project.
---

# New item (idea or work item)

Capture fast, then talk. Write everything in the user's language and speak plainly (CLAUDE.md "Always" rules).

First decide what it is: a **work item** if the user names a client, matter, engagement, class or project (or the word recorded under `## About this library` in CLAUDE.md); otherwise an **idea**.

- **Quick (default):** a title and one line are enough. If the message already has both, ask nothing — create it and go. For a work item, at most one batch of questions for facts you cannot guess (for example the main contact, or the key date), and skip it if the user is in a hurry.
- **Deep** (the user asks to go deeper): before creating, ask 3–5 shaping questions in one batch — for an idea: the problem, why it matters now, what success looks like, the closest alternative, the biggest unknown; for a work item: the sections listed under `## About this library`.

## Steps

1. **Folder name + area.** Make a short folder name from the title, following the naming already used in the library. Choose the area from the `## <area>` headings in INDEX.md; ideas go in the ideas area, work items in the area for their kind. If none fits, propose one short new area. Ask only when truly unclear.
2. **Duplicate check.** Look in INDEX.md for the name and title words — if something close exists, ask whether to continue that one instead.
3. **Create** `<area>/<item>/`:
   - `README.md` from the template below — today's date; status `seed` for an idea, `active` for a work item.
   - `log.md` with a heading and a first entry: `## YYYY-MM-DD — captured` (or "opened") and the one-line summary.
   - Nothing else yet. `todo.md`, `notes/`, `meetings/` and `outputs/` appear with their first content.
4. **Index.** Add the item's line at the top of its `## <area>` section in INDEX.md (create the heading in alphabetical position if new; remove the "Nothing here yet" line).
5. **Save** — when working outside the Thoughts app: one history entry `new item: <item-folder>`, then sync if a backup exists. Inside the app, skip this; the app saves.
6. **Talk.** Start at once. Idea: ask what prompted it and where they want to take it. Work item: ask what needs doing first, and turn the answer into todos with due dates.

## README template — idea

```markdown
---
title: {Title}
kind: idea
status: seed
area: {area}
tags: []
created: {YYYY-MM-DD}
updated: {YYYY-MM-DD}
summary: {One line.}
---

# {Title}

## What & why

{The idea and the motivation — mostly stable once written.}

## Current state

{10 lines at most. What a future session must know to resume. Always fresh.}

## Open questions

- {…}

## Files

_None yet._

## Related

_None yet._
```

## README template — work item

Same note details with `kind:` set to the item word and `status: active`. Sections: an opening `## Overview`, then the work item sections recorded under `## About this library` in CLAUDE.md, and always `## Current state`, `## Open questions`, `## Files`, `## Related`. Leave a section out when there is nothing to put in it yet — do not fill it with placeholders. Translate headings into the working language; keep the note-details keys and status values in English.

Privacy: record only what the work needs. No ID numbers, no full bank details, no passwords.
