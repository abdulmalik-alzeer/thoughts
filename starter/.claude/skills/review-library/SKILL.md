---
name: review-library
description: Audit the whole library and report prioritized improvement feedback — hygiene drift, stale ideas, convention violations, structural gaps. Use when the user asks to review the library, check project health, or wants feedback on how to improve it.
---

# Review the library

Sweep the whole repo and produce **feedback, not fixes** — a prioritized report the user can act on. Apply changes only if the user asks afterwards. This is the one ritual allowed to read broadly; still prefer frontmatter and file maps over full bodies.

## Pass 1 — mechanical hygiene

Check every idea folder (`*/*/`), `INDEX.md`, and `me/`:

- **Index drift:** INDEX lines vs. README frontmatter (status, summary, date, area vs. actual folder); ideas missing from INDEX; orphan INDEX lines.
- **Frontmatter:** missing or malformed fields (`title, status, area, tags, created, updated, summary`); `updated:` far behind the folder's last git touch (`git log -1 --format=%as -- <folder>`).
- **README shape:** `## Current state` over 10 lines; missing sections; `## Files` map disagreeing with actual `notes/` contents.
- **Logs:** `log.md` over ~400 lines without rotation; entries not newest-first.
- **Files & links:** stray files that aren't README / log / log-archive / notes; broken relative links; wiki-links (`[[…]]`) anywhere.
- **me/:** README file map vs. actual files; `interview-map.md` coverage levels contradicting what topic files actually contain.

## Pass 2 — editorial judgment

This is the valuable half — read READMEs (and top log entries where needed) and ask of each idea:

- **Lying statuses:** `active` but untouched for 30+ days → suggest `parked`; `seed` never developed after several sessions → develop, park, or drop?
- **Stuck questions:** the same open question sitting across 3+ log entries — name it and suggest how to unblock (research, a decision, a check-in question).
- **Missing connections:** ideas that mention each other (or clearly share ground) without `## Related` links; ideas so overlapping they should merge or one should absorb the other.
- **Resumability:** could a future session actually resume from `## Current state` in one short read? Flag vague ones.
- **me/ gaps:** ideas that lean on personal facts (skills, time, money, goals) not recorded in `me/` — feed these to the interview map as suggested threads.
- **Portfolio view:** one step back — too many actives for the user's attention? An area growing that deserves splitting? Anything the library's structure itself makes hard?

## Report

Deliver as a single message, grouped and ranked:

1. **Fix now** — drift and violations that corrupt trust in the library (index lies, lying statuses, broken links).
2. **Worth doing** — editorial improvements with a one-line why and the concrete fix each.
3. **Nice to have** — polish.

One line per item with the file path. End with: offer to apply all Pass-1 mechanical fixes in one go (commit `library review: hygiene fixes`), leaving editorial calls to the user.
