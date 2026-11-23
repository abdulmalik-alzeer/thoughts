# Slide decks in Thoughts

Thoughts presents a markdown note as slides (with reveal.js, inside the app) when its frontmatter
says `slides: true`. The user presses **Present** in Docs, in a file preview, on a deck in a chat
answer, or runs "Present the open slide deck" from the command palette.

## The format

````markdown
---
slides: true
title: Quarterly review
dir: ltr            # optional: rtl or ltr; otherwise taken from the deck's first letters
transition: slide   # optional: none, fade or slide (default slide)
theme: app          # optional: app, light or dark (default app: the user's own look)
---

# Quarterly review

Three things went well, two did not.

Note: Start with the numbers, not the story.

---

## What went well

- Sales up 12%
- Two new clients
- The launch was on time

---

## Revenue by month

```chart
{ "type": "bar", "title": "Revenue", "labels": ["Jan", "Feb", "Mar"], "series": [{ "name": "2026", "values": [12, 15, 19] }] }
```
````

- **Slides are separated by a line holding exactly `---`, with a blank line before and after.**
  A `---` inside a fenced code block is not a separator. Without `slides: true` in the
  frontmatter, `---` stays an ordinary horizontal rule.
- **Speaker notes:** a line starting with `Note:` (or `ملاحظة:`) on a slide; everything after it on
  that slide is notes. The presenter panel (key `S`) shows them, with the next slide's title, a timer
  and "slide 3 of 12".
- One idea per slide: a heading, then at most five short bullets or one picture, chart or diagram.
  The first slide is the title slide (`#`), the others start with `##`.
- **Diagrams and charts** are ordinary `archify` and `chart` blocks, exactly as in notes and answers;
  they are drawn on the slide. Pictures are ordinary markdown images of files in the library.
- **Never raw HTML, `<script>`, `<style>`, `<iframe>` or reveal.js attributes**: the app shows only
  plain markdown, and removes everything else. Pictures from the internet are not shown.
- At most 300 slides and 200 KB per deck. No vertical slides and no step-by-step fragments.
- Save a deck as a note (for example `<idea>/notes/<topic>-slides.md`). The user can save it as one
  HTML file into the idea's `outputs/` folder (Docs › More › Save slides as HTML).

## In a chat answer

Write the deck in a fenced block with the language `slides` (the frontmatter is optional there). The
app shows it as a card with **Present** and **Save as note**:

````markdown
```slides
# Onboarding

---

## Day one

- Laptop and accounts
- Meet the team
```
````

## Arabic decks

Write the deck in Arabic and it runs right to left by itself (or set `dir: rtl`). Keep English
product names and numbers as they are; the app keeps them in the right order.

```markdown
---
slides: true
title: مراجعة الربع
dir: rtl
---

# مراجعة الربع

ثلاثة أمور سارت جيداً، واثنان لم يسيرا كما أردنا.

ملاحظة: ابدأ بالأرقام لا بالقصة.

---

## ما سار جيداً

- ارتفعت المبيعات ١٢٪
- عميلان جديدان
- الإطلاق في موعده
```
