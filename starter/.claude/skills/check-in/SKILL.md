---
name: check-in
description: Daily or weekly review meeting — what happened, what was learned, progress against goals — distilled into me/log.md. Use when the user says check-in, daily review, weekly review, or wants a recurring personal debrief.
---

# Check-in

The maintenance ritual once the `me/` profile is built: short recurring meetings that keep it current and track progress against goals. Two cadences — infer from the user's words and the date of the last entry in `me/log.md`; ask only if genuinely ambiguous.

## Daily (default) — ~5 minutes

1. Ask in **one batch**: What did you do today (work + projects)? What did you learn? Did anything change — money, plans, people, health?
2. Probe the single most consequential answer with one or two follow-ups. That's it — a debrief, not an interrogation.

## Weekly — zoom out

1. Read the week's daily entries in `me/log.md` first, plus `me/goals.md` if it exists — open with a 3-line recap of the week as recorded, so the user reacts instead of recalling.
2. Ask: wins and misses against goals? What moved (or stalled) on active ideas? How were energy and habits? What's the one focus for next week?
3. Push back where the record disagrees with the self-assessment — that's the point of the meeting.

## Save

4. Prepend to `me/log.md` (create with heading `# Log — <user's name>` on first use; newest first, idea-log conventions apply, rotate to `log-archive.md` past ~400 lines):

   ```
   ## YYYY-MM-DD — daily | weekly review
   - Did: …
   - Learned: …
   - Changed: …
   - Next: …
   ```

   Distillation, not transcript; drop empty categories.

5. **Durable facts** (new skill, changed situation, revised goal) → fold into the relevant `me/` topic file, not the log. Reopen the domain in `interview-map.md` (regress coverage, add threads) if something big shifted.
6. **Idea spillover:** if the check-in surfaces real movement on a library idea, offer to update that idea's log/README too — separate commit per repo rules.
7. **Commit** — `me: check-in YYYY-MM-DD` (or `me: weekly review YYYY-MM-DD`).
