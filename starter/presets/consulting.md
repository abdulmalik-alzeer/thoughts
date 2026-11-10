# Preset — consulting

For independent consultants, advisers and small firms delivering projects for clients. Used once by init-library; later by tune-up.

## Words
- Work item: **engagement** (a piece of work for a client). `kind: engagement`. A client with several engagements may also get a client page.

## Areas
- `engagements/` — one item per engagement, live or finished
- `clients/` — optional: one item per client (people, history, how they like to work)
- `proposals/` — opportunities and proposals in progress, won or lost
- `playbook/` — your reusable methods, frameworks, templates and lessons learned
- `ideas/` — offers, articles, business development

## Work item sections
Overview (client, sponsor, the problem in the client's words) · Objectives and success measures · Scope (in / out / assumptions) · Deliverables (table: deliverable · due · status) · Stakeholders (name · role · what they care about) · Commercials (fee basis, budget or days agreed, days used — in a line or two) · Risks and issues · Decisions (date · decision · who) · Current state · Open questions · Files · Related

## Routines

### engagement-kickoff
Signed scope and commercial terms on file · sponsor and day-to-day contact confirmed · objectives and success measures agreed in writing · stakeholder list drafted · information and access requested · working rhythm set (status update day, steering meetings) · deliverable dates into todos · risks noted on day one.

### weekly-status
A one-page update in `outputs/`: done this week · planned next week · deliverables against dates · risks and issues with owner · decisions needed from the client · days used against days agreed. Drafted from the week's log, meetings and todos; the user edits before sending.

### scope-change
When the client asks for something outside scope: what was asked and by whom · effect on time, cost and other deliverables · options · decision and date → Decisions section, and a short written confirmation in `outputs/`.

### proposal
Client's situation and need, in their words · proposed approach and what makes it credible · deliverables and timeline · team · fees and assumptions · what is excluded · next step and date. Status in the README: drafting, sent, won, lost (with the reason — the reasons are worth a yearly review).

### engagement-close
Deliverables accepted · final invoice · handover of materials and access · thank-you and a request for feedback or a reference · lessons learned into `playbook/` with client details removed · date for a follow-up contact as a todo.

## Worked example
Engagement "Example — operations review for a retail client": eight-week review with three deliverables (current-state findings, options paper, implementation plan). README filled in with scope in/out and a deliverables table; todos "Request sales and stock data", "Schedule stakeholder interviews", "Send week-1 status update"; one meeting note "Kick-off with sponsor".

## Care points
- Client information stays in that client's engagement; lessons moved to `playbook/` are generalised first.
- Deliverables in `outputs/` contain only what that client should see — no internal notes, no other clients' names.
- Keep recommendations and evidence apart: say what the data shows, what is inferred, and what is opinion.
- Days and fees are recorded as the user states them; never estimate billing figures.
