---
name: reindex
description: Rebuild INDEX.md from the front pages of every idea and work item, and report anything that had drifted. Use when the index looks stale or wrong, or after folders were moved or renamed by hand.
---

# Reindex

INDEX.md is rebuilt from item READMEs, which are the source of truth.

## Steps

1. **Collect.** Find every `*/*/README.md` — exactly two levels deep (`area/item`). Skip `me/`, `routines/`, `meetings/`, `tasks/`, folders starting with a dot, and folders the app marks private.
2. **Read the note details only** (the first lines between `---`): `title`, `kind`, `status`, `area`, `updated`, `summary`.
3. **Rebuild INDEX.md** in the standard format — `## <area>` headings alphabetical; within an area, most recently updated first; keep the explanatory header at the top.
4. **Tell the user what was out of step**, in plain words and their language: items that were missing from the index, index lines whose folder no longer exists, items whose recorded area differs from where they actually are, and items with missing details.
5. **Save** — outside the Thoughts app only: `reindex`.
