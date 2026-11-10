# Preset — accounting & finance

For accountants, bookkeepers, finance managers and controllers, in practice (many clients) or in-house (one or several entities). Used once by init-library; later by tune-up.

## Words
- Work item: **client** (practice) or **entity** (in-house). `kind: client`
- Ask which applies if it is not obvious from what the user said.

## Areas
- `clients/` — one item per client or entity
- `periods/` — optional: one item per close or year-end that spans several clients
- `reference/` — policies, chart-of-accounts notes, recurring technical questions and their answers
- `ideas/` — improvements to process, tools, the practice itself

## Work item sections
Overview (what the business does, legal form, group structure) · Key contacts · Accounting basics (financial year-end, reporting framework as stated by the user, bookkeeping system, currencies, tax registrations held — recorded, not assumed) · Recurring obligations (what, how often, who prepares, who reviews) · Key dates · Current state · Open questions · Files · Related

## Routines

### month-end-close
Checklist; steps apply in this order unless the user's own differs. Remove steps that do not apply to the client.
1. Confirm cut-off: all sales invoices, purchase invoices and expense claims for the period entered
2. Bank and cash: reconcile every account to statements; investigate and clear old reconciling items
3. Receivables: ledger agrees to control account; review aged balances; consider doubtful-debt allowance
4. Payables: ledger agrees to control account; reconcile key supplier statements; goods received not invoiced
5. Payroll: post the payroll journal; reconcile payroll liabilities to amounts paid
6. Inventory: counts or valuation updates; provisions for slow-moving stock
7. Fixed assets: additions and disposals recorded; depreciation run; register agrees to ledger
8. Accruals and prepayments: update the schedule; reverse last month's where needed
9. Revenue: deferred and accrued income; long-running contracts
10. Loans and leases: interest and repayments posted; balances agree to statements
11. Intercompany: balances agreed with the other side; differences resolved
12. Taxes: indirect-tax and other tax accounts reconciled to returns and payments
13. Clear suspense and unallocated accounts to nil
14. Review: trial balance scan; variances against prior period and budget explained in writing
15. Management pack prepared and reviewed → `outputs/`
16. Lock the period; note unresolved points for next month in the log

### filing-calendar
A single table kept current: Obligation · Client/entity · Period covered · Due date · Preparer · Reviewer · Status.
- Fill it only from dates the user gives or confirms; filing dates differ by country, entity type and year-end, so never assume one.
- Each row becomes a todo with `@due(...)` on the due date and, where the user wants, a second todo for the internal review date before it.
- Review it at every month-end and at the weekly check-in.

### year-end-pack
Trial balance agreed and locked · reconciliations for every balance-sheet account with supporting schedules · fixed-asset register · debtors and creditors listings · inventory records · loan and lease confirmations · payroll summaries · tax computations and workings · related-party and post-period-end events noted · list of judgements and estimates with the reasoning · queries log for the auditor or reviewer (question, asked, answered).

### client-onboarding
Engagement terms agreed and signed · identity and acceptance checks done as the firm's own procedures require (record that they were done, not the documents) · handover from previous accountant · system access arranged · opening balances agreed · recurring obligations and filing calendar filled in · first-month priorities as todos.

## Worked example
Client "Example Trading Co.": a small trading company, year-end 31 December, monthly management accounts due by the 10th working day. README filled in; todos "Reconcile the two bank accounts", "Chase missing purchase invoices for the month", "Send management pack"; one meeting note "Monthly review call" with two decisions and two action items.

## Care points
- Figures are quoted exactly as given, with their source and date; never round or estimate silently.
- Tax and reporting rules differ by place and change: flag what must be checked against current rules, and say when a professional judgement is the user's to make.
- No bank account numbers, tax identification numbers or payroll details of named employees in notes — refer to the source document instead.
- One client's information never appears in another client's folder.
