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
| `tree` | hierarchies, org charts, folder trees, mind maps | `examples/payment-platform.tree.json` |
| `timeline` | dated events, roadmaps | `examples/payment-incident.timeline.json` |
| `waterfall` | nested spans of time (traces, build steps, Gantt-like) | `examples/checkout-request.waterfall.json` |

How to write one that draws the first time:

1. Read the worked example for the type (the files are beside this guide, in `examples/`), and copy
   its shape. The exact rules are in `schemas/<type>.schema.json`; unknown fields are refused, so
   do not invent any.
2. Every document needs `schema_version`, `diagram_type`, and `meta` with a `title` and an
   `output` (write `"output": "diagram.html"`; the app ignores the name).
3. Types that take positions (`pos`, `x`, `y`, `viewBox`) need them: lay the nodes out on a tidy
   grid with room between them, as the example does, and keep labels short (two or three words).
4. Ids are lowercase words joined by hyphens, unique in the document; every edge, message or
   relationship names ids that exist.
5. Leave out `brand` objects with a `url`, `meta.repository`, and `meta.animation`: the app removes
   them. A `brand` given as a plain name from Archify's built-in list is fine but never needed.
6. Write labels in the language you are answering in. `meta.locale` may be `en`, `ar`, `es`, `ko`,
   `zh-CN` or `zh-TW`.

More detail, when a diagram is refused and the message is not enough:
`references/authoring-defaults.md` and `references/authoring-contract.md`.

When the app cannot draw a block it shows Archify's own message under it. If the user passes that
message back to you, fix exactly what it names and send the whole block again.

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
- `unit`: a short text put after values in labels, such as `"%"`.

Rules: numbers are plain JSON numbers (no thousands separators, no units inside them). Up to 60
labels and 2,000 points in all. Bars start at zero. Say in a sentence what the chart shows; do not
repeat its numbers in a table unless asked.

## Which one

A picture of *things and how they connect* is a diagram (`archify`). A picture of *numbers* is a
chart (`chart`). A simple comparison of a few words is a table, not either.
