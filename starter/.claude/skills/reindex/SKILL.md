---
name: reindex
description: Rebuild INDEX.md from idea README frontmatter and report drift. Use when the index looks stale or wrong, or after manual file moves.
---

# Reindex

INDEX.md is derived data; idea READMEs are the source of truth.

## Steps

1. **Collect.** Glob `*/*/README.md` (every idea README — exactly two levels: `area/slug`).
2. **Read frontmatter only** (first ~15 lines of each): `title`, `status`, `area`, `updated`, `summary`.
3. **Rebuild INDEX.md** in the standard format — `## <area>` headings alphabetical; within an area, most recently `updated:` first; keep the explanatory header at the top of the file.
4. **Report drift:** ideas that were missing from the old index, orphan index lines whose folder no longer exists, `area:` frontmatter that disagrees with the folder's actual location, and missing or malformed frontmatter.
5. **Commit** — message `reindex`.
