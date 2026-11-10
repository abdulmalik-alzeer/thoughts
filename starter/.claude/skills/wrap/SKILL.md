---
name: wrap
description: End-of-session ritual — distill the discussion into the idea's files, update INDEX.md, and commit. Use when a discussion winds down or the user says wrap, save, or done for now.
---

# Wrap a session

Turn the discussion into files a future session can resume from in one short read. For **each idea touched this session**, run all steps (one commit per idea).

## Steps

1. **Distill** the session into one log entry, prepended to `log.md` (newest first):

   ```
   ## YYYY-MM-DD — <topic of the session>
   - Decided: … → [note](notes/<topic>.md)
   - Learned: …
   - Open: …
   - Next: …
   ```

   Distillation, not transcript — only lines that earn their place; drop empty categories.

2. **Merge substance into notes/.** Durable reasoning, designs, comparisons, research → create or update `notes/<topic>.md` (light frontmatter: `title`, `updated`, `summary`). Notes hold *current* thinking — rewrite rather than append contradictions. Create the `notes/` dir with the first note. Keep the README `## Files` map in sync (one-liner per note).

3. **Refresh README:** rewrite `## Current state` (≤10 lines), update `## Open questions`, bump `status:` if it changed (e.g. `seed` → `active`), set `updated:` to today. Add `## Related` links if new connections to sibling ideas emerged.

4. **INDEX.md:** update the idea's line (status · pitch · date) and move it to the top of its area section.

5. **Log rotation:** if `log.md` exceeds ~400 lines, move older entries to `log-archive.md`, keeping the 5 newest in `log.md`.

6. **Commit** — stage only that idea's folder plus INDEX.md; message `<slug>: <short session summary>` (e.g. `quarterly-close: agreed the review checklist`).

7. **Push** — once, after all ideas are committed: `git push` (skip silently if no remote is configured). If rejected because the remote moved, `git pull --rebase` then push again — never force-push. If offline or push fails, tell the user plainly; the commits will ride along on the next wrap.

8. **Confirm** to the user in 1–2 lines what was captured, the commit message(s) used, and that the push succeeded (or why it didn't).

**Spawned sub-ideas:** if the session produced a spin-off worth its own exploration, run the new-idea ritual for it as a *sibling* idea and cross-link both `## Related` sections before committing the parent.
