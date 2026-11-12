---
name: tune-up
description: Periodic review of how the library is actually being used, ending in a short list of proposed adjustments to its conventions, routines and skills so that it fits the user better over time. Use when the user says tune-up, asks how the library is working, wants it to fit them better, changes profession or way of working, or about once a month when offered.
---

# Tune-up

The library should fit the person, not the other way round. This routine looks at what the user really does and proposes changes to the rules — nothing is changed until the user agrees. (For checking that files are tidy and consistent, use **review-library**; this one is about fit.)

Speak in the user's language and in plain words. Never describe a proposal in terms of files or settings — say what will be different for them.

## Look — lightly

Read, in this order, and stop when you have enough:

1. `## About this library` in CLAUDE.md — what was set up, and anything listed as "Not wanted".
2. INDEX.md — how many items per area, what is active, what has not moved in a long time.
3. The todo lists — overdue items, tasks without dates, items that keep being postponed.
4. The newest 3–5 log entries of the few most active items, and the list of file names (not contents) in their `notes/`, `meetings/`, `outputs/`.
5. `routines/` — which exist, and whether recent logs show them being used.

Do not open private folders. Do not read note bodies unless a specific question needs it.

## Notice

- **Repeated manual work** — the same steps or the same kind of document produced several times → a new routine, or a template in a routine file.
- **Unused structure** — areas, README sections, routines or skills nobody touches → simplify or remove.
- **Missing structure** — things the user keeps filing awkwardly (meetings with no item, a growing pile of general notes, one area holding two different kinds of thing) → a new area, item kind or section.
- **Words** — the user calls things by different names than the library does ("files" not "matters", "sections" not "classes") → rename.
- **Dates and follow-through** — many overdue or undated todos → suggest a weekly review, or fewer, clearer todos.
- **Language** — notes drifting between languages against the user's wishes → agree on one.
- **Privacy** — confidential details that leaked into general notes, or folders that should probably be marked private → point them out and offer to tidy.
- **Changed circumstances** — a new role, a new kind of client, a new term → the matching preset in `.claude/presets/` (`.claude/presets/ar/` when the library is in Arabic) may have useful pieces; read only that one.

## Propose

Give at most **five** proposals, most useful first, each in this shape:

> **What I noticed** — one sentence with the evidence. **What I suggest** — one sentence. **What changes for you** — one sentence.

Then ask which to apply: all, some, or none. If nothing is worth changing, say so in one line — that is a good result.

## Apply — only what was agreed

- Convention changes → edit CLAUDE.md (the relevant convention, and `## About this library`).
- New or changed routine → `routines/<name>.md`.
- Changed behaviour of a routine such as wrap or meeting notes → you cannot edit those (the app keeps them, so nothing there changes unseen). Write the change in one or two plain sentences and ask the user to open **Settings → Customisations → Routines**, choose that routine, and paste them into the box that asks the assistant for a draft; they review it and keep it there.
- Renames or moves of existing folders → list exactly what will move, confirm once more, then move and fix the links and INDEX.md.
- Declined proposals → record under `## About this library` as "Not wanted: …" so they are not raised again.

Never change `.claude/settings.json`, never add anything that runs programs, and never delete user content as part of a tune-up.

Finish with a three-line summary of what is now different, and add a dated line under `## About this library`: `Last tune-up: YYYY-MM-DD — <what changed>`. Outside the Thoughts app, save as `tune-up: <summary>`.
