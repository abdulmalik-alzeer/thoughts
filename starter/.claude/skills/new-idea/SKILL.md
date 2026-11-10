---
name: new-idea
description: Capture a new idea into the library — scaffold its folder, add it to INDEX.md, commit, and start the discussion. Use when the user wants to start or capture a new idea or project thought.
---

# New idea

Capture fast, then talk. Two modes:

- **Quick (default):** all that's needed is a working title and a one-line pitch. If the user's message already contains both, ask nothing — scaffold and go.
- **Deep** (user says "go deep" or similar): before scaffolding, ask 3–5 shaping questions in one batch — the problem it solves, why it matters to them / why now, what success looks like, the closest existing alternative, the biggest unknown. Seed the README sections with the answers.

## Steps

1. **Slug + area.** Kebab-case the title into a slug. Pick the area from the existing `## <area>` headings in INDEX.md; if none fits, propose one new short lowercase word. Ask only when genuinely ambiguous.
2. **Duplicate guard.** Grep INDEX.md for the slug and title words — if something close already exists, ask whether to continue that idea instead.
3. **Scaffold** `<area>/<slug>/`:
   - `README.md` from the template below — today's date, status `seed`.
   - `log.md` with heading `# Log — <Title>` and one first entry: `## YYYY-MM-DD — captured` followed by the one-line pitch.
   - Do **not** create `notes/` yet — it appears with the first real note.
4. **Index.** Add the idea's line at the top of its `## <area>` section in INDEX.md (create the heading in alphabetical position if new; remove the `_No ideas yet._` placeholder if present).
5. **Commit** — message `new idea: <slug>` — then `git push` if a remote is configured (if it fails, say so; it rides along on the next push).
6. **Talk.** Start the discussion immediately: ask what triggered the idea and where they want to take it. In deep mode, reflect the seeded README back and probe the biggest unknown first.

## README template

```markdown
---
title: {Title}
status: seed
area: {area}
tags: []
created: {YYYY-MM-DD}
updated: {YYYY-MM-DD}
summary: {One-line pitch.}
---

# {Title}

## What & why

{The pitch and the motivation — mostly stable once written.}

## Current state

{≤10 lines. What a future session must know to resume. Always fresh.}

## Open questions

- {…}

## Files

_None yet._

## Related

_None yet._
```
