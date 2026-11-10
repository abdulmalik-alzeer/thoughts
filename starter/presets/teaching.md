# Preset — teaching

For school teachers, lecturers, tutors and trainers. Used once by init-library; later by tune-up.

## Words
- Work item: **class** (a group you teach in a term or year) — or **course** for lecturers and trainers. `kind: class`

## Areas
- `classes/` — one item per class or course you teach
- `units/` — optional: units or schemes of work reused across classes and years
- `resources/` — worksheets, reading lists, activities, question banks worth keeping
- `admin/` — meetings, duties, reports, professional development
- `ideas/` — things to try, projects, improvements

## Work item sections
Overview (subject, level or year group, group size, when and where it meets) · Term dates and key dates (assessments, reports due, trips, parent meetings) · Curriculum map (units in order with planned weeks) · Assessment plan (what, when, how it is marked) · Where we are (the resume point: last lesson taught, what comes next) · Things to remember about this group (general needs and what works — no sensitive detail) · Open questions · Files · Related

## Routines

### lesson-plan
One note per lesson or lesson sequence in `notes/`:
1. Learning objective — what students will know or be able to do by the end
2. Where it sits — unit, previous lesson, what they should already know
3. Opening — a short task that recalls prior learning or raises the question
4. Teaching — the explanation or model, with the examples to use
5. Guided practice — done together, with checks for understanding (the actual questions to ask)
6. Independent practice — the task, with support for those who need it and stretch for those who are ready
7. Closing check — how you will know who has got it
8. Materials and preparation
9. Homework or follow-up
10. Afterwards, two lines: what worked, what to change — this is what makes next year easier

### unit-plan
Big questions and end goals · what is assessed and how · sequence of lessons with one line each · common misconceptions · key vocabulary · resources · links to earlier and later units.

### assessment-cycle
Before: what is being assessed, mark scheme, date announced. After: results summary for the group (patterns, not a list of names), the three most common errors, what will be retaught, feedback given. Reports due → todos with due dates.

### term-calendar
One table of the term: week · dates · what each class is covering · assessments · reports and meetings · school events. Each deadline becomes a todo.

### contact-log
For conversations with parents, guardians or colleagues about a student: date · who · what was discussed · what was agreed · follow-up. Kept inside the class item; follow-ups become todos.

## Worked example
Class "Example — Year 8 Science, Group B": three lessons a week, current unit "Forces", end-of-unit test in three weeks. README filled in with a short curriculum map and Where we are; one lesson plan note ("Balanced and unbalanced forces"); todos "Prepare practical equipment list", "Write end-of-unit test", "Reports due" with dates.

## Care points
- Keep information about students to the minimum the teaching needs. Prefer first names or initials; no health, family or safeguarding details in notes — those belong in the school's own systems.
- Do not keep full mark lists with names here unless the user decides to; summaries by pattern are usually enough.
- Suggest marking a folder private if it holds anything about individual students.
- Follow the curriculum and assessment rules the user states; do not assume a particular country's system.
- Materials for students go in `outputs/`, checked for level and language before use.
