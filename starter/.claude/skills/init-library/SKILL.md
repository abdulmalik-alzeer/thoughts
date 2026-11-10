---
name: init-library
description: One-time setup of a fresh thoughts library — ask what the user will use it for, tailor the structure and rituals to their answers, make the first commit, then delete this skill. Use when the user says init, initialize, set up the library, or on the first session of an uninitialized library.
---

# Initialize the library

This runs **once**, on a fresh copy of the starter kit. Its job: learn how the new owner intends to use the library, shape the library to that, commit the result, and then remove itself so it can never run again.

If the library already looks initialized (`me/` exists, or INDEX.md has real entries, or git history has an `init:` commit), stop and tell the user it's already set up.

## Step 1 — interview

Welcome the user in a sentence or two ("this is a personal thinking library; a few questions and I'll shape it around you"), then ask **in one batch** (use AskUserQuestion where available):

1. **What will you use this for?** (multi-select, they can pick several)
   - **Project & business ideas** — capture ideas, develop them over many sessions
   - **Daily briefs / journaling** — recurring check-ins on what happened and what's next
   - **Todo lists & tasks** — a running task list swept regularly
   - **Personal profile** — build a record of skills, history, goals (powers CVs, decisions)
2. **What should I call you?** — first name, used in logs and greetings.
3. **A few things on your mind right now?** — optional; 1–3 seed ideas or standing concerns, one line each.
4. **Sync to GitHub?** — *yes, I'll connect a remote* / *no, local only for now*.

Don't interrogate further — the library shapes itself through use.

## Step 2 — shape the library

Based on the answers:

- **Always:** create `me/README.md` — frontmatter (`title: About <Name>`, `updated`, `summary`), the user's name, and an empty `## Files` map. This is the profile's front door regardless of chosen uses.
- **Ideas chosen:** for each seed idea from question 3, run the **new-idea** ritual (quick mode, status `seed`, commits included). No seeds → leave INDEX.md as is.
- **Daily briefs chosen:** create `me/log.md` with heading `# Log — <Name>` and no entries. Mention that "check-in" starts a debrief any time.
- **Tasks chosen:** create `tasks/TODO.md` with `## Now` / `## Next` / `## Later` / `## Done` headings (seed `## Now` with any task-shaped items from question 3 instead of making them ideas).
- **Profile chosen:** create `me/interview-map.md` — list the standard domains (background & education, work history, skills, finances, health & habits, goals, people & relationships), each `untouched`, per the interview-me skill. Mention "interview me" as the way to build it.

**Trim what wasn't chosen** — a lean library beats a full one:

- No daily briefs **and** no profile → delete `.claude/skills/check-in/` and `.claude/skills/interview-me/`, and remove their rows from the rituals table in CLAUDE.md. (Profile without briefs → keep both; check-in maintains the profile.)
- No tasks → remove the **Tasks** paragraph from CLAUDE.md's Conventions.
- Ideas are the backbone — never remove the idea skills.
- Remove the "Not initialized yet?" warning block from the top of CLAUDE.md.

## Step 3 — git

1. If the folder is not a git repo: `git init -b main`.
2. If GitHub sync was chosen: ask for the remote URL if they have one ready, `git remote add origin <url>`; if they don't have one yet, tell them how to create an empty GitHub repo and say the SessionStart hook will start syncing once `origin` exists. Never block on this.
3. Stage everything.

## Step 4 — self-destruct + commit

1. Delete this skill's folder: `.claude/skills/init-library/`.
2. Delete `START-HERE.md` — it has served its purpose.
3. Commit everything as one commit: `init: library configured for <Name>`. Push if a remote was set.

## Step 5 — hand over

Close with a short tour tailored to their choices — 3–5 lines, e.g.:

> You're set up. Say **"new idea: …"** to capture a thought, **"continue <idea>"** to pick one back up, **"check-in"** for a daily debrief, and **"wrap"** when we finish a discussion so nothing evaporates. Everything lives in plain markdown — read it, edit it, it's yours.

Then start the first real conversation: if they gave seed ideas, offer to dig into the most interesting one now.
