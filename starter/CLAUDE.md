# Thoughts — Personal Thinking Library

> **⚠️ Not initialized yet?** If `.claude/skills/init-library/` still exists, this library has not been set up. Before doing anything else, tell the user to run `/init-library` (or offer to run it now). Do not create ideas or files until initialization is done.

A long-lived library of ideas, projects, and personal notes, written as markdown and developed through discussions with Claude across many sessions. There is no application code — the files are the product. In any session the job is: help think, then leave files a future session can resume from in one short read.

## Map

```
Thoughts/
├── CLAUDE.md            ← auto-loaded (this file)
├── INDEX.md             ← master index: every idea, one line, grouped by area
├── me/                  ← personal profile: general facts about the user
├── tasks/               ← running todo list (TODO.md), if the user tracks tasks here
└── <area>/              ← broad areas (work/, clients/, home/, …), created lazily
    └── <idea-slug>/
        ├── README.md    ← front door: status, pitch, current state, file map
        ├── log.md       ← dated session entries, newest first
        └── notes/       ← topic docs holding current thinking, created as needed
```

## Reading protocol — token discipline

This library is designed to grow large. Read the minimum:

1. Discovery = `INDEX.md`, nothing else. Never `ls`-crawl or bulk-read idea folders "for context."
2. The resume package for an idea = its `README.md` + the top 2–3 entries of its `log.md`. That is all.
3. Open a `notes/` file only when the README's `## Files` map (or the user) makes it relevant to the current topic.
4. Cross-idea search: grep `INDEX.md` and README frontmatter (`summary:`, `tags:`) first, note bodies second; read matches selectively.
5. Never load several ideas in full. Follow `## Related` links one hop at a time, README first.
6. Personal context lives in `me/` — when an idea touches the user's situation (money, time, skills), read `me/README.md` and only the relevant topic file. Never re-ask what's recorded there.

## Rituals

Skills encode the rituals; plain language triggers them just the same.

| Trigger | Ritual |
|---|---|
| "new idea: …" / `/new-idea` | Scaffold + index + commit + discuss — `.claude/skills/new-idea/SKILL.md` |
| "continue …" / `/continue-idea` | Minimal resume + recap — `.claude/skills/continue-idea/SKILL.md` |
| "wrap" / `/wrap` | Distill session into files + commit — `.claude/skills/wrap/SKILL.md` |
| "which ideas …?" | Search per reading protocol rule 4 |
| `/reindex` | Rebuild INDEX.md from READMEs — `.claude/skills/reindex/SKILL.md` |
| "review the library" / `/review-library` | Full-library audit + prioritized feedback — `.claude/skills/review-library/SKILL.md` |
| "interview me" / `/interview-me` | Tree interview deepening `me/` — `.claude/skills/interview-me/SKILL.md` |
| "check-in" / `/check-in` | Daily/weekly debrief into `me/log.md` — `.claude/skills/check-in/SKILL.md` |

When a discussion winds down — topic exhausted, user signs off, or attention shifts to another idea — offer a wrap. Never let a substantial discussion evaporate unsaved.

## Conventions

**Naming.** Ideas are kebab-case slugs; folder name = slug. Areas are short lowercase words (`work`, `clients`, `home`). Areas are created lazily by the first idea that needs them — never pre-create empty ones.

**Statuses.** `seed` just captured · `active` under discussion · `parked` deliberately paused · `done` concluded · `dropped` abandoned, kept for reference.

**Idea README.md.** Frontmatter `title, status, area, tags, created, updated, summary` (summary = one-line pitch, reused by INDEX and search). Sections: `## What & why` (stable), `## Current state` (≤10 lines, always fresh — the resume point), `## Open questions`, `## Files` (map of notes/ with one-liners), `## Related` (links to sibling ideas).

**log.md.** Newest entry first. Entry shape:

```
## YYYY-MM-DD — topic of the session
- Decided: … → [note](notes/topic.md)
- Learned: …
- Open: …
- Next: …
```

Distillation, not transcript; drop empty categories. Over ~400 lines, rotate older entries to `log-archive.md` (keep the 5 newest).

**notes/\*.md.** Light frontmatter (`title, updated, summary`). Hold current thinking only — rewrite rather than append contradictions; the past lives in log.md and git history.

**INDEX.md.** One line per idea under `## <area>` headings (areas alphabetical, ideas most-recently-updated first):

```
- [Title](area/slug/README.md) · status · one-line pitch · YYYY-MM-DD
```

INDEX is derived — READMEs are the source of truth; on disagreement trust READMEs and run `/reindex`.

**Links.** Plain relative markdown links (`[pricing](notes/pricing.md)`; cross-idea `[x](../../area/slug/README.md)`). No wiki-links.

**Personal profile (`me/`).** Not an idea and not in INDEX.md — general facts about the user (basics, finances, constraints) so sessions never re-ask. `me/README.md` is the front door with a file map; topic files carry light frontmatter like notes. Fold new durable personal facts into `me/` at wrap, or immediately when the user asks. The profile grows through the **interview-me** ritual (`me/interview-map.md` tracks the question tree and coverage) and stays current through **check-in** debriefs (`me/log.md`, idea-log conventions).

**Tasks (`tasks/TODO.md`).** If present, a single running todo list — sections `## Now`, `## Next`, `## Later`, `## Done` (Done keeps only the last ~20, dated). Add and check off items whenever they come up in conversation; sweep it during check-ins. Tasks that grow into real projects graduate to an idea folder.

**Sub-ideas.** A spin-off worth its own exploration becomes a *sibling* idea (own folder + INDEX line, in whatever area fits), cross-linked in both `## Related` sections. Never nest idea folders.

## Discussion style

Be a sparring partner: build on and develop the user's ideas, but surface weaknesses, counter-examples, and unstated assumptions as they appear — don't save critique for the end, and don't soften it away. Match scrutiny to maturity: a `seed` gets curious exploration; an `active` idea approaching a decision gets rigor.

## Git

- The library lives in git. If a GitHub remote (`origin`) is configured, it syncs so it can be used from any machine.
- Session start: a SessionStart hook runs `git pull --rebase --autostash` when a remote exists. If its output shows a conflict or failure, surface it and resolve before touching files.
- `/wrap` → one commit per idea touched: `<slug>: <short session summary>`, then a single `git push` at the end (skip the push if no remote is configured).
- `/new-idea` → `new idea: <slug>` (commit + push). `/reindex` → `reindex`.
- A commit means idea files + INDEX agree — never commit half-updated state.
- If push is rejected because the remote moved, `git pull --rebase` then push again. Never force-push. If offline, say so — the commit will ride along on the next wrap's push.

## Hygiene

- Bump `updated:` on meaningful edits; keep the idea's INDEX line in sync.
- Keep `## Current state` ≤10 lines; history belongs in log.md.
- No stray files in idea folders — everything fits README / log / notes.
