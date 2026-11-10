# Preset — legal

For lawyers, in-house counsel, paralegals and legal assistants. Used once by init-library; later by tune-up.

## Words
- Work item: **matter**. `kind: matter`. Clients may also get a page of their own when they have several matters.

## Areas
- `matters/` — one item per matter
- `clients/` — optional: one item per client with several matters (contacts, billing arrangements, list of matters)
- `knowledge/` — research notes, clause language, checklists, lessons learned, with client details removed
- `ideas/` — practice development, process improvements

## Work item sections
Overview (client, what the matter is about, the client's objective) · Parties and representatives · People on the matter (responsible lawyer, team) · Scope of the engagement (what is and is not included; fee arrangement in a line) · Key dates (table: date · what · source document · who confirmed it) · Current state · Strategy and open issues · Open questions · Files · Related

## Routines

### matter-intake
1. Client and all other parties identified, with related companies and individuals
2. Conflict check carried out under the firm's procedure — record the date and outcome
3. Client identification and acceptance steps the firm requires — record that they were completed, not the documents
4. Scope, fee arrangement and responsible lawyer agreed; engagement letter sent and signed
5. The client's objective and what a good result looks like, in their words
6. Key dates gathered from the documents and confirmed by the lawyer (limitation or other time limits, hearing dates, notice periods, contractual deadlines) → Key dates table and todos with due dates
7. Documents received listed, with what is still missing
8. First actions and who owns each → todos

### chronology
A note `notes/chronology.md`: Date · Event · Source document · Disputed? One line per fact; add to it whenever a new document comes in. Facts only — arguments go in a separate note.

### key-dates-review
Weekly: every open matter's Key dates table against the todo list — each date has a todo, each todo has an owner, anything within 14 days is mentioned to the user.

### hearing-or-meeting-prep
Purpose and desired outcome · the three points that matter · documents to bring · questions to ask · risks and fallback positions · afterwards: a meeting note the same day.

### matter-closing
Final bill and outstanding costs · client informed of the outcome and any continuing obligations or future dates · original documents returned · file archived under the firm's retention policy · lessons for `knowledge/` with client details removed.

## Worked example
Matter "Example — supply contract review": a client asks for review of a supplier's draft agreement before signing. README with parties, scope ("review and mark-up; negotiation not included") and one key date (the client's signing target, confirmed by the client); todos "Send first mark-up", "Call client on liability clause"; one meeting note "Kick-off call".

## Care points
- Never work out a limitation period, court deadline or notice period: record the date the lawyer gives, with its source, and remind. Say so if asked to calculate one.
- Do not state what the law is as fact. Research notes say which sources were consulted and what remains to be verified by the lawyer.
- Matter content is confidential and may be privileged: it stays inside the matter's folder; nothing is copied to other matters, to `knowledge/` without removing client details, or to the personal profile.
- Suggest marking `matters/` private for any matter the client or firm requires to be kept away from AI tools.
- Quote contract wording exactly; summarise separately and label the summary as such.
