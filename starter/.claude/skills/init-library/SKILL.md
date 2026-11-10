---
name: init-library
description: One-time setup of a fresh library — ask what the user does and which language they work in, shape the library with the matching profession preset and one worked example, then remove this skill. Use when the user says init, initialize, set up the library, or on the first session of a library that is not set up yet.
---

# Set up the library

This runs **once**, on a fresh copy of the starter kit. Its job: learn who the owner is and how they will use the library, shape it to fit, and then remove itself.

If the library already looks set up (CLAUDE.md has an `## About this library` section, or INDEX.md has real entries), stop and say it is already set up; offer **tune-up** instead.

Follow CLAUDE.md's four "Always" rules throughout — in particular, speak plainly and in the user's language from the first sentence.

## Step 1 — interview

Welcome the user in a sentence or two, then ask **in one batch** (use a question form where available):

1. **What kind of work will you use this for?** (one, or the closest)
   - Accounting & finance
   - Legal
   - Consulting
   - Teaching
   - Research / study
   - Founder / product
   - Personal
   - Other (ask them to describe it in a line)
2. **Which language do you want to work in?** — notes, titles and todos will be written in it. Default: the language they are writing in now.
3. **What else should it do?** (pick any) — capture and develop ideas · todo lists · daily or weekly check-ins · a personal profile so the assistant never asks twice.
4. **What should I call you?**
5. **What is on your plate right now?** — optional; one to three clients, matters, classes, projects or ideas, one line each.
6. **Keep a backup copy online?** — yes / not now.

**If the working language is Arabic**, add one short question to the same batch — and only this one: **folder and file names in Arabic, or in Latin letters?** («أسماء المجلدات والملفات: بالعربية أم بالحروف اللاتينية؟»). Default when not answered: Arabic. Ask it once; never raise it again later.

Do not ask more — the library shapes itself through use.

**When the answers are supplied for you** (the Thoughts app passes them and says not to ask): ask nothing. For anything not supplied, decide quietly: language = the language the app says the interface uses, else the language of the name and of the "on their mind" lines, else English; kind of work = whatever those lines clearly indicate, else Personal; names = Arabic when the working language is Arabic; weekly-review day as in Step 2. Say in the hand-over what you assumed and that one sentence changes it ("set this library up for legal work").

## Step 2 — shape the library

1. **Read one preset.** For the chosen work (`accounting-finance`, `legal`, `consulting`, `teaching`, `research-study`, `founder-product`): when the working language is Arabic open `presets/ar/<profession>.md`; otherwise `presets/<profession>.md`. The Arabic presets have the same sections in the same order (الكلمات = Words, المجالات = Areas, أقسام عنصر العمل = Work item sections, طرق العمل = Routines, مثال تطبيقي = Worked example, نقاط العناية = Care points) and are already worded for the profession in Arabic — use their wording as it stands; do not translate the English preset yourself. For any other working language, read the English preset and translate. Personal and Other have no preset: use an ideas area (`الأفكار` / `ideas`) plus, for Other, one area and item word that fit the description they gave. Read only that one file.
2. **Record the choices** at the bottom of CLAUDE.md:

   ```
   ## About this library
   - Used for: <kind of work, in a few words>
   - Working language: <language>
   - Work items are called: <client / matter / class / … in the working language> (`kind: <word>`)
   - Areas: <area> — <what goes in it>; …
   - Work item sections: <the README sections from the preset>
   - Names: <Arabic / Latin letters> — folder and file names follow this everywhere
   - Week: <first working day>–<last working day>; weekly review on <day>
   - Care points: <the preset's care points, one line each>
   ```

   Write the item word, section names and care points in the working language (from the Arabic preset as written, when Arabic).

   **Names.** One choice, applied consistently to every area folder, item folder, routine file, note and meeting file created now and later:
   - *Arabic names* (the default for an Arabic library): area folders take the Arabic name the preset gives (`العملاء/`, `القضايا/` …); item folders, routine files and note files are short Arabic names with hyphens instead of spaces (`شركة-المثال-التجارية`, `routines/إقفال-نهاية-الشهر.md`), without diacritics, tatweel or punctuation.
   - *Latin letters*: area folders and routine files take the English names the preset gives in backticks (`clients/`, `routines/month-end-close.md`); item folders are a simple lowercase transliteration or the English equivalent, with hyphens (`sharikat-al-mithal`).
   - Either way the titles inside files are natural Arabic, and the names the app reads stay in English exactly as CLAUDE.md rule 1 lists them: `README.md`, `log.md`, `todo.md`, `INDEX.md`, `tasks/TODO.md`, `notes`, `meetings`, `outputs`, `routines`, `me`, the note-details keys, `kind:` and status values, and `@due(...)`.
   - Never mix the two styles in one library.

   **Week and weekly review.** Arabic working language (or the user says they are in Saudi Arabia or keep a Sunday–Thursday week): the week starts on Sunday, the working week is Sunday–Thursday, and the weekly review is on **Thursday**. Otherwise: Monday–Friday, weekly review on **Friday**. If the user names a different day or week, use theirs. Record it on the `Week:` line; every routine with a weekly rhythm (weekly review, weekly status, key-dates review, check-ins) and every due date you work out for "end of the week" follows it.
3. **Routines.** For each routine in the preset, create `routines/<name>.md` (note details `title, updated, summary`, then the checklist), written in the working language.
4. **Items from question 5.** For each line, run the **new-idea** routine (quick mode, without saving to history in between): a work item if it names a client, matter, class or project; an idea otherwise. Things that are plainly tasks go to the todo list instead.
5. **One worked example.** If question 5 produced no work item, create the preset's worked example as a real item so the user sees the shape: title prefixed "Example" (in the working language), a filled-in README, one log entry, two or three todos with due dates in the coming weeks, and one meeting note if the preset has one. Say in the hand-over that it can be deleted or renamed.
6. **Always:** create `me/README.md` — note details (`title`, `updated`, `summary`), the user's name, working language, kind of work, weekly-review day, and an empty `## Files` list.
7. **Chosen extras:**
   - Todo lists → `tasks/TODO.md` with `## Now` / `## Next` / `## Later` / `## Done` headings (translated).
   - Check-ins → `me/log.md` with a heading and no entries.
   - Personal profile → `me/interview-map.md` listing the standard domains (background & education, work history, skills, finances, health & habits, goals, people & relationships), each `untouched`.
8. **Trim what was not chosen** — a lean library beats a full one:
   - No check-ins **and** no profile → delete `.claude/skills/check-in/` and `.claude/skills/interview-me/` if they are still there, and remove their rows from the routines table in CLAUDE.md. (Profile without check-ins → keep both.)
   - No todo lists → keep the Todos convention (items still have their own todo lists) but do not create `tasks/TODO.md`.
   - Remove the "Not set up yet?" block from the top of CLAUDE.md.
9. **Tidy the presets.** Move the whole `presets/` folder, including `presets/ar/`, to `.claude/presets/` unchanged, so it is out of the user's way but still available if they change profession later. Do not edit the preset files.
10. **INDEX.md** lists every item created; remove the "Nothing here yet" line if any exist.

## Step 3 — history and backup

1. If the folder has no history yet: `git init -b main`.
2. If an online backup was wanted: ask for the address if they have one; otherwise say it can be connected later from Settings. Never block on this.

## Step 4 — remove this skill and save

1. **Welcome page.** The kit ships `START-HERE.md` (English) and `START-HERE.ar.md` (Arabic). Delete both, and delete `.claude/skills/init-library/`. Nothing in them needs keeping: the hand-over below covers the same ground in the user's language. Until setup runs, a user who opens the folder reads the one in their language; if the user asks for the welcome page during setup, show the Arabic one when the working language is Arabic.
2. Save everything as one entry: `init: library set up for <Name>`. Sync if a backup location was set.

## Step 5 — hand over

Close with a short tour in the user's language, fitted to their choices — four or five lines, no technical words. For example, for an accountant:

> You are set up. I created an example client so you can see how things are kept — delete it whenever you like. Say **"new client: …"** to add a real one, paste rough notes and say **"meeting notes"** after a call, and **"start month-end close for March"** when the time comes. Press **Wrap** when we finish talking so nothing is lost.

An Arabic hand-over is written in Arabic from the start, with the app's own words: «تلخيص وحفظ» for Wrap, «المكتبة», «المهام», «السجل», «الرئيسية»; the trigger phrases are given in Arabic («عميل جديد: …», «محضر اجتماع», «مهمة: …», «فكرة جديدة: …»). Do not promise reminders or undo beyond what CLAUDE.md says the app does.

Mention anything you assumed (including the names style and the weekly-review day). Then start the first real conversation: offer to open the most pressing thing from question 5, or to replace the example with a real item.
