# Drawing diagrams and charts in Thoughts

Thoughts draws diagrams and charts itself, inside your answer and inside notes. You write a fenced
code block; the app draws it and offers it as an SVG file and a draw.io file. Never use Mermaid,
PlantUML, Graphviz, ASCII art, or raw HTML/SVG for these: they show as plain text.

There are two kinds of block.

## 1. Diagrams: a block with the language `archify`

The block holds exactly one JSON document in the Archify format (version 3.2.0, vendored in this
folder). Nothing but the JSON goes inside the block.

Pick the type by what the reader needs to see:

| `diagram_type` | Use it for | Worked example to read first |
|---|---|---|
| `architecture` | systems, services, boundaries, who talks to whom | `examples/web-app.architecture.json` |
| `workflow` | flowcharts, processes, decisions, swim lanes | `examples/holiday-planning.workflow.json` |
| `sequence` | messages between participants over time | `examples/cache-miss-request.sequence.json` |
| `dataflow` | data moving through stages | `examples/event-stream.dataflow.json` |
| `lifecycle` | states and transitions (state machines) | `examples/agent-run.lifecycle.json` |
| `erd` | database tables and relationships | `examples/orders.erd.json` |
| `class` | types, interfaces, inheritance | `examples/payments.class.json` |
| `tree` | hierarchies, org charts, folder trees, outlines | `examples/payment-platform.tree.json` |
| `timeline` | dated events, roadmaps | `examples/payment-incident.timeline.json` |
| `waterfall` | nested spans of time (traces, build steps, Gantt-like) | `examples/checkout-request.waterfall.json` |

How to write one that draws the first time:

1. Read the worked example for the type (the files are beside this guide, in `examples/`) and copy
   its shape. The exact rules are in `schemas/<type>.schema.json` (shared lists such as node types
   are in `schemas/common.schema.json`); unknown fields are refused, so do not invent any.
2. Every document starts with `schema_version` (the number in the table below), `diagram_type`,
   and `meta` with a `title` and `"output": "diagram.html"` (the app ignores the name).
3. The examples are finished showpieces. Copy their structure, not their decoration: leave out
   `meta.viewBox`, `meta.subtitle`, `meta.column_fit`, `meta.quality_profile`, `meta.views`,
   `meta.animation`, `meta.repository`, `cards`, `sources`, `via` and `labelAt` unless you need
   them. Leave out any `brand` given as an object with a `url` (the app removes it).
4. Ids are short lowercase words joined by hyphens or underscores, unique in the document; every
   edge, message or relationship names ids that exist.
5. Keep labels short: two or three words in a box, one or two on a line between boxes.
6. Write labels in the language you are answering in, and set `meta.locale` (`en`, `ar`, `es`,
   `ko`, `zh-CN` or `zh-TW`).

What each type needs beyond its example:

| Type | `schema_version` | Remember |
|---|---|---|
| `architecture` | 1 | Let Archify place the boxes: `"layout": {"mode": "grid", "gapX": 110, "gapY": 80}` and a `row` and `col` on every component (the main path along row 0, left to right; stores and side services in the rows below, under the box that uses them). Box `type` is one of `frontend`, `backend`, `database`, `cloud`, `security`, `messagebus`, `external` (people and outside systems are `external`). A box is 120 wide: a longer name goes partly into `sublabel`. |
| `workflow` | 2 | There is no special decision box: a decision is a node whose label is the question, with one labelled edge per answer. Columns are 0 to 5. A step back to an earlier node is an edge with `"role": "return"`. |
| `sequence` | 1 | Each message has its own `y`, starting at 160 and about 25 to 30 apart, top to bottom in time. No message from a participant to itself. `segments` and `activations` are optional; a segment's `from` and `to` must be at least 4 away from any message's `y`. |
| `dataflow` | 1 | Two to five stages; rows 0 to 4. |
| `lifecycle` | 3 | `mainPath` is required: the ids of the normal path in order, each joined to the next by a transition. State `type` has its own list (in the schema), not the box types above. |
| `erd` | 1 | Cardinality is `one` or `many`. Many-to-many needs a junction entity with two foreign keys. `references` names the other entity by its `id` (`"book.id"`). |
| `class` | 1 | At most three columns of types. |
| `tree` | 1 | Exactly one root (the node without `parent`); every other node has one `parent`. |
| `timeline` | 1 | `meta.evidence` is required: `"observed"` for real dated events, `"illustrative"` when you chose the dates. Dates are full ISO times with an offset (`2027-03-01T00:00:00+03:00`); when the user gave only a month, use its first day. |
| `waterfall` | 1 | Each span has a `start` and an `end` or a `duration`; a child names its `parent`. |

The app corrects two small layout slips of an architecture diagram itself (a line's caption
sitting on a box, a name slightly wider than its box). Anything else that is wrong is shown to the
user with Archify's message. If the user passes that message back to you, fix exactly what it
names and send the whole block again.

The two files in `references/` say more about layout and wording. They were written for Archify's
own command-line workflow: ignore what they say about `finalize`, `deliver`, `--repo-root`,
receipts, quality profiles and the Viewer.

## 2. Charts: a block with the language `chart`

For numbers (bar, line, area, pie, donut, scatter). The block holds one JSON document:

```chart
{
  "type": "bar",
  "title": "Revenue by quarter",
  "labels": ["Q1", "Q2", "Q3", "Q4"],
  "series": [
    { "name": "2025", "values": [12, 15, 14, 19] },
    { "name": "2026", "values": [14, 18, 21, 24] }
  ],
  "x": { "label": "Quarter" },
  "y": { "label": "SAR, millions" }
}
```

Fields:

- `type`: `bar`, `line`, `area`, `pie`, `donut` or `scatter`. Required.
- `title`, `subtitle`: optional text above the chart.
- `labels`: the categories along the axis (bar, line, area) or the slice names (pie, donut).
- `series`: one or more `{ "name", "values" }`; each `values` list is as long as `labels`. Use
  `null` for a missing value. Pie and donut take exactly one series. At most 8 series.
- `scatter` takes `series` of `{ "name", "points": [[x, y], …] }` and no `labels`.
- `x`, `y`: `{ "label": "…" }` axis titles. `y` may also carry `"min"` and `"max"`.
- `stacked`: `true` stacks bars or areas. `horizontal`: `true` lays bars sideways (good for long
  category names).
- `unit`: a short text put after the values, such as `"%"` (written `12%`) or `"SAR"` (written `12 SAR`). Put a
  currency or measure in the axis label instead when the numbers are large.
- A pie's or donut's series `name` is not shown; its `labels` are.

Rules: numbers are plain JSON numbers (no thousands separators, no units inside them). Up to 60
labels and 2,000 points in all. Bars start at zero. Say in a sentence what the chart shows; do not
repeat its numbers in a table unless asked.

## Which one

A picture of *things and how they connect* is a diagram (`archify`). A picture of *numbers* is a
chart (`chart`). A simple comparison of a few words is a table, not either.
