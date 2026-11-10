# Thoughts Starter Kit

A personal thinking library you run with [Claude Code](https://claude.com/claude-code). Your ideas, projects, daily reviews, and tasks live as plain markdown files, developed through conversations with Claude across many sessions. There is no application code — **the files are the product**.

## Requirements

- Claude Code (CLI or desktop app)
- git

## Setup — one time

1. Unzip this folder wherever you keep long-lived things (e.g. `~/Thoughts`). Rename the folder if you like.
2. Open a terminal in the folder and run `claude`.
3. Say: **"init the library"** (or run `/init-library`).

Claude will ask a few questions — what you'll use the library for (project ideas, daily briefs, todo lists, a personal profile…), your name, whether to sync with GitHub — then tailor the library to your answers, make the first commit, and delete the init command so it can never run twice.

After that, just talk:

- *"new idea: a faster month-end close"* — captures and files it
- *"continue faster-month-end-close"* — resumes where you left off
- *"check-in"* — daily/weekly debrief
- *"wrap"* — saves the session's thinking into files and commits

Everything else is explained in `CLAUDE.md`, which Claude reads automatically every session.

## What's in the box

```
CLAUDE.md            ← the library's rules (Claude reads this every session)
INDEX.md             ← master index of your ideas (starts empty)
.claude/skills/      ← the rituals: init-library, new-idea, continue-idea,
                       wrap, check-in, interview-me, reindex, review-library
```
