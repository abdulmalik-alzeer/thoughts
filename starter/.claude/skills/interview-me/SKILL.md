---
name: interview-me
description: Tree-structured interview that deepens the me/ profile — high-level root questions per life domain branching into ever more specific follow-ups. Use when the user says interview me, or wants to fill out, deepen, or complete their personal profile.
---

# Interview me

Goal: make `me/` comprehensive enough that any future need — generating a CV, choosing a course, scoping an income project — can be served from the files without re-asking anything. The interview is a **tree**: each life domain is a root with one high-level question; every answer spawns deeper follow-ups; follow-ups not yet asked persist as *threads* in `me/interview-map.md` so the tree survives across sessions.

## Steps

1. **Load** `me/README.md`, `me/interview-map.md`, and the topic file(s) of the domain being explored. Never ask anything a topic file already answers — deepen past it instead.
2. **Pick a branch.** The user's choice if they named one; otherwise recommend one domain — weakest coverage or highest-value open threads — and start there. Don't offer a menu of everything.
3. **Open.** Untouched domain → ask its root question from the map. Returning to a domain → resume its open threads, briefly reminding the user where the tree left off.
4. **Drill.** From each answer, generate the natural deeper questions and ask them in batches of 2–4 — never a wall of ten, never one-at-a-time trivia. Chase **record-grade specifics**: dates, employers, titles, technologies, numbers, outcomes, names. One clarifying push on a vague answer, then move on. It's an interview, not a form — react to what's interesting, challenge inconsistencies with what's already on file.
5. **Prune, don't chase.** A promising side-branch that would derail the current line → write it into the map as a thread and stay on course.
6. **Stop when signal drops** — answers thin out or the user tires. Never ask "for completeness."

## Save (every session, even short ones)

7. **Topic files:** fold answers into `me/<topic>.md` (light frontmatter: `title, updated, summary`). Current truth only — rewrite contradictions rather than appending; superseded facts just disappear. Keep `me/README.md`'s file map in sync.
8. **Map:** update the domain's coverage level, delete answered threads, add newly spawned ones, bump `updated:`.
9. **Commit** — `me: interview — <domain>`.

## Graduation

When most domains reach `covered`, say so and propose starting the **check-in ritual** (daily/weekly reviews — see `check-in` skill): the profile is built, now it needs maintaining.

## Coverage levels

`untouched` → `seeded` (root question answered) → `covered` (main threads closed) → `deep` (exhausted for now). Levels can regress — life changes reopen domains.
