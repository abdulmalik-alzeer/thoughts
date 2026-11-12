---
name: review-library
description: Health check of the whole library with prioritised feedback — out-of-step index, stale items, overdue todos, untidy files, privacy slips, structural gaps. Use when the user asks to review the library, check its health, or wants feedback on how to keep it in good order.
---

# Review the library

Go through the whole library and produce **feedback, not fixes** — a prioritised report the user can act on. Change things only if the user asks afterwards. This is the one routine allowed to read broadly; still prefer note details and file lists over full bodies, and never open folders the app marks private.

Report in the user's language and in plain words: describe each finding by what it means for them ("the client list shows this one as open but its page says closed"), with the item's name.

## Pass 1 — tidiness

Check every item folder (`*/*/`), `INDEX.md`, the todo lists, `routines/` and `me/`:

- **Index:** INDEX lines against README details (status, summary, date, area against actual folder); items missing from INDEX; lines with no folder.
- **Note details:** missing or malformed fields (`title, kind, status, area, tags, created, updated, summary`); `updated:` far behind the folder's last change.
- **README shape:** `## Current state` over 10 lines; missing sections; `## Files` disagreeing with what is actually in `notes/`, `meetings/`, `outputs/`.
- **Todos:** lines that are not one task per line; malformed `@due(...)` dates; overdue items; finished work still unticked.
- **Meetings:** file names without a date; action items that never became todos.
- **Logs:** over about 400 lines without archiving; entries not newest-first.
- **Files and links:** stray files that fit none of README / log / todo / notes / meetings / outputs (an `images/` folder next to a note holds that note's pictures and is not stray; a picture in it that no note links to may be mentioned, never deleted); broken links; working notes sitting in `outputs/` or deliverables sitting in `notes/`.
- **Language:** files that mix languages against the working language recorded in CLAUDE.md.
- **Privacy:** anything that looks like a password, an ID, bank or card number; one client's details inside another item or inside `me/`.

## Pass 2 — judgement

The valuable half. Read READMEs (and the newest log entries where needed) and ask of each item:

- **Statuses that are no longer true:** `active` but untouched for 30+ days → on hold? A work item whose work is plainly finished → close it? An idea captured and never developed → develop, pause or drop?
- **Stuck questions:** the same open question across 3+ log entries — name it and suggest how to unblock it.
- **Missing connections:** items that mention each other without `## Related` links; items so overlapping they should merge.
- **Resumability:** could a future session really resume from `## Current state` in one short read? Flag vague ones.
- **Key dates:** work items with deadlines recorded in the README but no matching todo.
- **The big picture:** too many open items for one person's attention? An area that should be split? Anything the structure itself makes hard? (For reshaping the library around how it is used, suggest **tune-up**.)

## Report

One message, grouped and ranked:

1. **Fix now** — things that make the library untrustworthy (wrong index, untrue statuses, privacy slips, missed deadlines).
2. **Worth doing** — improvements, each with a one-line reason and the concrete fix.
3. **Nice to have** — polish.

One line per finding. End by offering to apply all Pass 1 tidiness fixes in one go (outside the app, saved as `library review: tidy-up`) — except privacy findings, which are confirmed one by one — leaving the judgement calls to the user.
