# Customising Thoughts — the contract

This folder is the **customisation layer** of the Thoughts desktop app: everything that makes this
install the user's own. You (the assistant) change the app by changing files here. This file is
written by the app on every start; do not edit it. If something is not described here, it is not
supported — say so instead of guessing.

The user is probably **not a developer**. Keep changes small, explain them in plain words, and never
make them read code.

## How a change reaches the user

1. You are working in a **staging copy** of the layer, not the live one. Write only inside the
   folder you were started in. The user's library is readable for context but read-only for you.
2. When you are done, write **`SUMMARY.md`** in this folder: 2–5 plain sentences, in the user's
   language, saying what will change and how to try it ("Open Home — the new box is under your
   chats"). The first line becomes the title of the change in the user's History, so make it a short
   description such as "Show overdue todos on Home". No jargon; file names only when they help.
3. The app shows the user your summary and the list of changed files. They can **Try it** (a
   preview), **Apply** it, or **Discard** it. Nothing is live until they apply, so never say it
   already is.
4. Every applied change is one History entry the user can undo on its own. "Rollback" restores the
   whole layer to an earlier day, and Safe mode starts the app with the layer switched off.

Do not run git. Do not create files other than the ones listed below — anything else is ignored
when the change is applied (hidden files, `README.md`, `CLAUDE.md` and `SUMMARY.md` are never part
of the layer).

## Rules that are never negotiable

- **Never weaken security.** Do not try to disable the plugin sandbox, permissions, the trust
  check, or the preview/apply step, and do not help anything get around them.
- **Never ask for, read, print or store the app's access token** (`daemon.json`), saved API keys,
  or git credentials. Plugins never receive the token and must not look for it.
- **No code from the internet.** Plugins and views must be self-contained: no remote scripts,
  styles, fonts, trackers or analytics, and no sending the user's notes anywhere.
- **Least permission.** A plugin declares exactly the permissions it uses, nothing more.
- **Do not touch the user's data.** Customising changes how the app looks and behaves. It never
  deletes, moves or rewrites notes. (A workflow may write to the library — that is its job — but
  only what its prompt says.)
- Prefer the simplest tool that works: `home.json` or `custom.css` before a plugin; a plugin before
  an HTML view.

## Layout

| Path | What it is |
|---|---|
| `manifest.json` | Declares plugins and their permissions |
| `custom.css` | Style overrides, loaded after the built-in theme |
| `themes.json` | Extra themes listed in the theme picker |
| `home.json` | Home screen: quick-start chips, hidden sections, section order |
| `plugins/*.js` | Plugins (ES modules) using the Plugin API below |
| `views/*.html` | Static pages shown in the sidebar, in a sandboxed frame |
| `workflows/*.json` | Jobs the app runs on a schedule or after an event |

Limits: at most 200 files, 512 KB per file, 4 MB in total. Text files only.

### manifest.json

```json
{
  "apiVersion": 1,
  "plugins": {
    "due-today.js": { "permissions": ["todos:read"] }
  }
}
```

Keys of `plugins` are file names inside `plugins/`. A plugin that is not listed runs with **no**
permissions. Whenever you add, rename or remove a plugin, or it starts using another part of the
API, update this file in the same change. The user is told when a change touches plugins or
permissions.

Permissions: `library:read`, `todos:read`, `todos:write`, `ideas:read`, `chat:start`.
Everything under `ui.*` and `on(...)` needs none.

### custom.css

Loaded after the built-in theme. Override design tokens rather than restyling components, so the
app stays consistent and readable in every theme, in light and dark mode, and in Arabic
(right-to-left).

Tokens you may override:

- Colours: `--bg`, `--panel`, `--panel-2`, `--sidebar`, `--fg`, `--muted`, `--accent`,
  `--accent-fg`, `--accent-2`, `--border`, `--danger`, `--ok`, `--code-bg`
- Shape and type: `--radius`, `--radius-lg`, `--border-w`, `--sidebar-w`, `--font`, `--font-ar`,
  `--mono`
- Depth and surfaces: `--shadow`, `--shadow-lg`, `--hairline`, `--fill`, `--fill-2`, `--tint`,
  `--glass`, `--sidebar-glass`, `--surface`, `--edge`, `--edge-strong`, `--spot`, `--spot-strong`,
  `--aura`, `--glow`, `--grain-o`, `--pattern`

Scope colour changes to a theme and a mode:
`[data-theme="saudi"][data-mode="light"] { --accent: #0a5c8a; }`.
Built-in themes are `saudi`, `contrast` (high contrast — keep text contrast at 7:1 or better) and
`tech`; modes are `light` and `dark`. Use logical properties (`margin-inline`, `inset-inline`,
`text-align: start`) so right-to-left keeps working. Never hide focus outlines.

### themes.json

A JSON array of extra themes for the picker: `[{ "id": "sand", "name": "Sand" }]`. `id` is
lowercase letters, digits and dashes. Each theme needs its tokens in `custom.css` for **both**
modes: `[data-theme="sand"][data-mode="light"] { … }` and `[data-theme="sand"][data-mode="dark"] { … }`,
each defining at least the thirteen colour tokens above. Keep existing entries.

### home.json

```json
{
  "chips": [{ "label": "Plan my week", "prompt": "Plan my week: " }],
  "hidden": ["stats"],
  "order": ["todos", "upcoming", "chats", "ideas", "projects", "stats"]
}
```

- `chips` — one-click starters under the Home prompt box. `prompt` is what gets filled in; end it
  with a space when the user is meant to finish the sentence.
- `hidden` — Home sections not to show. Sections: `stats`, `ideas`, `todos`, `chats`, `projects`,
  `upcoming`.
- `order` — the order of the sections; ones you leave out keep their usual place after the listed
  ones.

All three keys are optional. Keep what is already there unless the user asked to remove it.

### plugins/*.js — Plugin API v1

A plugin is one ES module with a default export. It receives a single object and nothing else: no
network access to the app's internals, no token.

```js
export default function (thoughts) {
  // thoughts.version === 1
}
```

| Call | Returns / does | Permission |
|---|---|---|
| `thoughts.library.list()` | Promise of the library's files (paths relative to the library) | `library:read` |
| `thoughts.library.read(path)` | Promise of a file's text | `library:read` |
| `thoughts.library.search(q)` | Promise of matches: `[{ path, matches: [{ text }] }]` | `library:read` |
| `thoughts.todos.list()` | Promise of `[{ file, idea, items: [{ title, text, due, done, group, line }] }]`; `due` is `"YYYY-MM-DD"` or `null` | `todos:read` |
| `thoughts.todos.add(text)` | Adds a todo to the general list; append ` @due(YYYY-MM-DD)` for a due date | `todos:write` |
| `thoughts.ideas.list()` | Promise of `[{ path, area, slug, title, status, summary, tags, updated }]` | `ideas:read` |
| `thoughts.chat.start(prompt)` | Opens a new chat with that prompt | `chat:start` |
| `thoughts.ui.toast(msg)` | Shows a short message | none |
| `thoughts.ui.go(view)` | Opens a page: `"home"`, `"todos"`, `"ideas"`, `"docs"`, `"projects"`, `"settings"`, or the `id` of a view you added | none |
| `thoughts.ui.addView({ id, title, icon?, render(el) })` | Adds a page to the sidebar; `render` fills the element `el` | none |
| `thoughts.ui.addCommand({ id, title, run })` | Adds a command to the command palette | none |
| `thoughts.ui.addHomeWidget({ id, title, render(el) })` | Adds a box to the Home screen | none |
| `thoughts.ui.addHomeChip({ id, label, prompt })` | Adds a quick-start chip on Home (prefer `home.json` when it is static) | none |
| `thoughts.ui.addItemAction({ target, id, label, run(item) })` | Adds an action to each todo (`target: "todo"`) or idea (`target: "idea"`) | none |
| `thoughts.on(event, cb)` | Calls `cb` when something happens; returns a function that stops listening | none |

Events for `on`: `layer.changed` (customisations were applied or undone), `todos.changed`,
`library.changed`, `wrap.done` (a wrap finished saving chats into the library), `chat.done` (the
assistant finished answering).

A call the plugin has no permission for is rejected with an error — it does not prompt the user.
Declare the permission in `manifest.json` instead.

Writing a good plugin:

- Build the interface with DOM calls (`document.createElement`, `textContent`). Never put library
  text into `innerHTML`.
- Use the tokens (`var(--panel)`, `var(--fg)`, `var(--muted)`, `var(--accent)`, `var(--radius)`)
  so it matches every theme. Put `dir="auto"` on elements showing the user's own text.
- Write visible text in the user's language. No emojis as icons.
- Everything the plugin adds is removed automatically when the layer reloads; keep no global state.
- A plugin that throws is switched off and the user is told, so handle empty lists and failed
  calls (`try`/`catch` around `await`).
- `id`s are lowercase with dashes and unique within the plugin.

### views/*.html

A complete static HTML page shown in the sidebar (the file name without `.html` is its title). It
runs in a sandboxed frame: it cannot reach the app, the user's files or the network on its own. It
can ask for **read-only** data by message:

```js
const call = (method, ...args) => new Promise((resolve, reject) => {
  const id = Math.random().toString(36).slice(2);
  const onMsg = (e) => {
    if (!e.data || e.data.id !== id) return;
    removeEventListener('message', onMsg);
    'error' in e.data ? reject(new Error(e.data.error)) : resolve(e.data.result);
  };
  addEventListener('message', onMsg);
  parent.postMessage({ id, method, args }, '*');
});
const todos = await call('todos.list');
```

Available methods: `library.list`, `library.read`, `library.search`, `todos.list`, `ideas.list`.
Prefer a plugin with `ui.addView` when the page needs to do more than display things.

### workflows/*.json

One file per workflow:

```json
{
  "name": "Weekly review",
  "trigger": { "type": "schedule", "every": "week", "at": "16:00", "weekday": 5 },
  "prompt": "Write a short review of this week into the library: …"
}
```

- Schedule trigger: `every` is `"day"` or `"week"`; `at` is `"HH:MM"`, 24-hour, the user's local
  time; `weekday` (weekly only) is 0–6 where **0 is Sunday** and 5 is Friday.
- Event trigger: `{ "type": "event", "on": "wrap.done" }` (after each wrap) or
  `{ "type": "event", "on": "app.start" }` (when the app starts).
- `prompt` is what the assistant is asked to do, inside the user's library, with nobody watching.
  Make it complete on its own: what to read, what to write and where, and how long the result
  should be. Follow the conventions in the library's own `CLAUDE.md`.

The app runs one workflow at a time, never during a wrap, and each run appears as a chat the user
can read. If the computer was off at the scheduled time the workflow runs once when the app is
next open. A file with an unknown trigger or a missing prompt is ignored.

## Worked examples

### 1. A Home widget listing overdue todos

`plugins/overdue.js`

```js
export default function (thoughts) {
  thoughts.ui.addHomeWidget({
    id: 'overdue-todos',
    title: 'Overdue',
    async render(el) {
      const draw = async () => {
        const today = new Date().toLocaleDateString('en-CA'); // YYYY-MM-DD, local
        let items = [];
        try {
          const files = await thoughts.todos.list();
          items = files.flatMap((f) => f.items).filter((i) => !i.done && i.due && i.due < today);
        } catch { /* show the empty state */ }
        items.sort((a, b) => a.due.localeCompare(b.due));
        el.replaceChildren();
        if (!items.length) {
          const p = document.createElement('p');
          p.textContent = 'Nothing overdue.';
          p.style.color = 'var(--muted)';
          el.append(p);
          return;
        }
        for (const i of items.slice(0, 7)) {
          const row = document.createElement('button');
          row.dir = 'auto';
          row.textContent = `${i.title} — ${i.due}`;
          row.style.cssText = 'display:block;width:100%;text-align:start;font:inherit;color:inherit;background:none;border:0;padding:4px 0;cursor:pointer';
          row.onclick = () => thoughts.ui.go('todos');
          el.append(row);
        }
      };
      await draw();
      thoughts.on('todos.changed', draw);
    },
  });
}
```

`manifest.json` gains `"overdue.js": { "permissions": ["todos:read"] }` under `plugins`.

`SUMMARY.md`: "Show overdue todos on Home. A new box called Overdue lists up to seven todos whose
date has passed, oldest first. Open Home to see it; click an item to jump to Todos."

### 2. A weekly-review workflow

`workflows/weekly-review.json`

```json
{
  "name": "Weekly review",
  "trigger": { "type": "schedule", "every": "week", "at": "16:00", "weekday": 5 },
  "prompt": "Write my weekly review. Read the notes and logs changed in the last 7 days and my todo lists. Then add one dated entry, at most 15 lines, where this library keeps reviews or logs (follow CLAUDE.md): what I decided, what is still open, and the todos due in the next 7 days. Do not change anything else."
}
```

`SUMMARY.md`: "Add a weekly review. Every Friday at 4 pm the assistant writes a short review of
your week into your library: decisions, open questions, and what is due next week. You will find
each run in your chats."

### 3. A theme

`themes.json` gains `{ "id": "sand", "name": "Sand" }` (keeping existing entries), and
`custom.css` gains:

```css
[data-theme="sand"][data-mode="light"] {
  --bg: #f7f1e5; --panel: #fffaf0; --panel-2: #efe5d2; --sidebar: #ede2cc; --fg: #2b2118; --muted: #6b5c4a;
  --accent: #a4571b; --accent-fg: #ffffff; --accent-2: #2f6f6a; --border: #e0d3bb;
  --danger: #a3261c; --ok: #2f6b3a; --code-bg: #f1e8d6;
}
[data-theme="sand"][data-mode="dark"] {
  --bg: #19140e; --panel: #221b13; --panel-2: #2c2419; --sidebar: #1d1710; --fg: #f3eadb; --muted: #b3a48d;
  --accent: #e39a4f; --accent-fg: #1b1208; --accent-2: #6fc2b9; --border: #3a2f21;
  --danger: #ff8b7a; --ok: #7fd18f; --code-bg: #1e1811;
}
```

`SUMMARY.md`: "Add a warm Sand theme. It comes in a light and a dark version. To try it, open the
theme picker and choose Sand; your current theme stays as it is until you switch."
