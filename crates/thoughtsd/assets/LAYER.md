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
3. The app **checks your files** (see "Your change will be rejected if…" below), then shows the
   user what the change does as the app reads it from your files (each add-on and page with the
   permissions it gains or loses, each Home shortcut with its prompt, what is hidden or renamed,
   todo settings, themes, the look, automatic tasks), then your summary, then the files. So your
   summary must match what the files do. They can **Try it** (a preview), **Keep** it, or
   **Discard** it. Nothing is live until they press Keep, so never say it already is. While the
   check finds problems, Keep is switched off and the user sees the problems in plain words.
4. Every kept change is one History entry the user can undo on its own. "Rollback" restores the
   whole layer to an earlier day, and Safe mode starts the app with the layer switched off.

Do not run git. Do not create files other than the ones listed below — anything else is ignored
when the change is kept (hidden files, `README.md`, `CLAUDE.md` and `SUMMARY.md` are never part
of the layer).

## Rules that are never negotiable

- **Never weaken security.** Do not try to disable the plugin sandbox, permissions, the trust
  check, or the preview/keep step, and do not help anything get around them.
- **Never ask for, read, print or store the app's access token** (`daemon.json`), saved API keys,
  or git credentials. Plugins never receive the token and must not look for it.
- **No code from the internet.** Plugins and views must be self-contained: no remote scripts,
  styles, fonts, images, trackers or analytics, and no sending the user's notes anywhere.
- **Least permission.** A plugin declares exactly the permissions it uses, nothing more, and the
  narrowest `write` folders that work.
- **Be careful with the user's data.** Customising changes how the app looks and behaves. A plugin
  writes to the library only where it declared, only when the user does something (submits a form,
  drags a card), and never deletes or rewrites notes on its own. (A workflow may write to the
  library — that is its job — but only what its prompt says.)
- Prefer the simplest tool that works: a settings file (`home.json`, `nav.json`, `todos.json`) or
  `custom.css` before a plugin; a plugin before an HTML view.

## Layout

| Path | What it is |
|---|---|
| `custom.css` | Style overrides, loaded after the built-in theme |
| `themes.json` | Extra themes listed in the theme picker |
| `home.json` | Home screen: quick-start chips, hidden sections, order of sections and widgets |
| `nav.json` | Sidebar: hidden pages, order, labels |
| `todos.json` | Todos page: grouping, sorting, filter |
| `plugins/<name>.js` | A plugin (ES module) using the Plugin API below |
| `plugins/<name>.json` | That plugin's declaration: permissions, write folders |
| `views/<name>.html` | A static page shown in the sidebar, in a sandboxed frame |
| `views/<name>.json` | That view's declaration (only if it reads data) |
| `workflows/<name>.json` | A job the app runs on a schedule or after an event |
| `manifest.json` | Older shared list of plugin permissions — still read, do not add to it |

`addons.json` lists the add-ons and pages the user switched off (`{ "off": ["plugins/hours.js"] }`):
they stay in the layer but are not loaded. Only the app writes this file, when the user switches an
add-on off or on or removes it in Customise, Add-ons. Your changes to it are ignored, so do not
edit it; if the user asks you to switch one on or off, tell them where to do it. A plugin you
change while it is off stays off: say so in the summary.

Limits: at most 200 files, 512 KB per file, 4 MB in total. Text files only — the layer cannot
carry images, fonts or any other binary file.

### Your change will be rejected if…

The app validates the staged files before the user can keep them. Any of these blocks the change:

- a `.json` file is not valid JSON, or is not the shape described in its section below (wrong
  type, a value outside the listed choices);
- `plugins/<name>.json` or `views/<name>.json` exists without its `plugins/<name>.js` /
  `views/<name>.html` (so keep other JSON data out of the top of `plugins/` and `views/`; put it
  in a subfolder such as `plugins/data/`), is missing `"apiVersion": 1`, or has an `apiVersion`
  other than 1;
- a permission is not one of the names listed under "Permissions";
- `library:write` is declared without a `write` list, or `write` is given without
  `library:write` or `ideas:write`, or a `write` folder is absolute, contains `..`, is empty or `.`, or is or sits
  inside `.claude` or `.git`;
- `read` is given without `library:read`, `todos:read` or `ideas:read`, is empty, or names a folder
  that could not be a `write` folder either (same rules as above);
- a view declares anything but read permissions, or declares `write`;
- `nav.json` hides `home`, `settings` or `help`;
- a workflow has no `prompt`, an unknown trigger, a time that is not `"HH:MM"`, a monthly `day`
  outside 1–28 / `"last"`, or a `weekday` outside 0–6;
- a plugin or view file directly in `plugins/` or `views/` has a name that is not plain: use only
  English letters, digits, dots, dashes and underscores, 60 characters at most (`hours.js`,
  `client-intake.json`). The app knows a plugin by its file name and keeps its storage under it.
  The name the user sees can be in any language: it is the `title` you give `ui.addView`, the
  widget's title, the `description`;
- a settings file, declaration or workflow contains a character that does not show on screen
  (zero-width characters, direction overrides, tag characters and the like), in a key or a value.
  Write plain text; ordinary Arabic and emoji are fine;
- a file is larger than 512 KB, is not plain text, or the change has more than 200 files or 4 MB.

File names anywhere in the layer may not contain invisible characters either; such a file is
left out of the change.

The app also test-loads each plugin and page in a hidden sandbox. A new or changed one that throws
while loading, or never gives control back (an endless loop), is stopped and shown to the user
with its error, and Keep stays off unless they tick that they want to keep it anyway. A change that
once made the app stop responding is not run again until the user chooses to. So: write the exact
shapes below, never wait in a loop, and do not leave half-finished files behind.

### plugins/<name>.json — a plugin's declaration

Every plugin that uses a permission has its own small file beside it, with the same name:

`plugins/due-today.json`

```json
{
  "apiVersion": 1,
  "permissions": ["todos:read"],
  "description": "Shows today's todos on Home"
}
```

- `apiVersion` — always `1`. (A plugin, view or pack that asks for a higher number is listed as
  unsupported and not loaded.)
- `permissions` — see the list below. A plugin with no declaration runs with **no** permissions.
- `write` — only with `library:write` or `ideas:write`: the library folders the plugin may write
  into (or whose items' status it may change), relative to the library, e.g. `["clients/"]`.
  Anything elsewhere is refused by the app.
- `read` — optional, with `library:read`, `todos:read` or `ideas:read`: the only library folders
  the plugin may read in, e.g. `["clients/"]`. Then `library.list`, `library.read`,
  `library.search`, `todos.list` and `ideas.list` give it nothing outside them (a link inside such
  a folder that leads elsewhere does not count). Left out, it reads the whole library except
  private folders. **Give `read` whenever a plugin only needs some folders** — a client-intake form
  has no reason to read the journal — and the user sees the folders in the review. (The general
  todo list sits at the top of the library, so a plugin with `read` folders does not see it.)
  Every todo call stays inside them too: `todos.add`, `setDone`, `edit` and `remove` only change
  lists inside the `read` folders, and the status `ideas.setStatus` sets only of items inside them.
- `description` — one plain sentence; the user sees it when reviewing.

Because each plugin has its own file, adding or removing one plugin never touches another, and the
user can undo one without disturbing the rest. **Do not add new plugins to `manifest.json`.** If a
plugin you are changing is still declared there, leave that entry alone and give the plugin its
own file — the plugin's own file wins. When you add, rename or remove a plugin, add, rename or
remove its `.json` in the same change. The user is told when a change touches plugins or
permissions.

#### Permissions

| Permission | Allows |
|---|---|
| `library:read` | `library.list`, `library.read`, `library.search` (only inside the `read` folders, when given) |
| `library:write` | `library.write`, `library.remove` — only inside the `write` folders, only `.md`, `.json`, `.csv`, `.txt` |
| `todos:read` | `todos.list` |
| `todos:write` | `todos.add`, `todos.setDone`, `todos.edit`, `todos.remove` |
| `ideas:read` | `ideas.list` |
| `ideas:write` | `ideas.setStatus` — only for items inside the folders declared in `write` (none declared: no item) |
| `chat:start` | `chat.start` |
| `storage` | `storage.get`, `storage.set` — the plugin's own private document |
| `files:save` | `ui.saveFile` — hand the user a `.csv`, `.txt`, `.md` or `.json` file in the app's save dialog |
| `notify` | `ui.notify` — a short notice under the plugin's name, also a system notification while the app is in the background |
| `assistant:ask` | `assistant.ask` — a question for the assistant, whose answer comes back as text |

Every other call under `ui.*` and `on(...)` needs none. Folders the user marked private are never
listed, read, searched or written, whatever a plugin declares. The app checks every permission
itself, from the plugin's own declaration, for every call; asking for one in the code changes
nothing.

### custom.css

Loaded after the built-in theme. Override design tokens rather than restyling components, so the
app stays consistent and readable in every theme, in light and dark mode, and in Arabic
(right-to-left).

Tokens you may override:

- Colours: `--bg`, `--panel`, `--panel-2`, `--sidebar`, `--fg`, `--muted`, `--accent`,
  `--accent-fg`, `--accent-2`, `--border`, `--danger`, `--ok`, `--code-bg`
- Shape and spacing: `--radius`, `--radius-lg`, `--border-w`, `--sidebar-w`
- Type: `--font`, `--font-ar`, `--mono` — these may only name fonts that are **already installed
  on the user's computer** (system fonts). The layer cannot ship or download a font.
- Depth and surfaces: `--shadow`, `--shadow-lg`, `--hairline`, `--fill`, `--fill-2`, `--tint`,
  `--glass`, `--sidebar-glass`, `--surface`, `--edge`, `--edge-strong`, `--spot`, `--spot-strong`,
  `--aura`, `--glow`, `--grain-o`, `--pattern`

Scope colour changes to a theme and a mode:
`[data-theme="saudi"][data-mode="light"] { --accent: #0a5c8a; }`.
Built-in themes are `saudi`, `contrast` (high contrast — keep text contrast at 7:1 or better) and
`tech`; modes are `light` and `dark`. Use logical properties (`margin-inline`, `inset-inline`,
`text-align: start`) so right-to-left keeps working. Never hide focus outlines.

**The user is told what your CSS does.** Before they can keep a change, the app reads the rules
you added and says so in plain words: "colours and style settings only" when they only set tokens,
colours, shadows and corner shapes; otherwise that the change also moves, resizes or hides things.
It warns specifically when CSS

- prints text (`content:` with anything but an empty string),
- places things on top of the app (`position: fixed`, `absolute` or `sticky`),
- hides something (`display: none`, `visibility: hidden`, zero opacity or size, transparent text,
  clipping or masking),
- names the app's own questions, warnings and notices in a selector (dialogs, modals, toasts), or
- loads a file (`@import`, `url(...)`).

None of these belongs in a theme. Do not restyle or cover the app's dialogs, warnings, review
panel or preview banner, and never use CSS to show wording of your own. To hide a page or a Home
section use `nav.json` / `home.json`; to show something use a plugin.

**Branding — what is and is not possible.** The layer is text only. When a user asks for "our
logo" or "our company font", say plainly that the app cannot load image or font files, and offer
what does work:

- their brand **colours** (accent, backgrounds, text) as a theme, in light and dark;
- their **shape**: corner radius, border width, sidebar width;
- a **text wordmark** — the firm's name set in an installed font, in the brand colour, as a small
  plugin Home widget or as a renamed page label in `nav.json`;
- an installed system font by name in `--font` / `--font-ar`, with sensible fallbacks.

Do not embed images as `data:` URLs or draw a logo out of CSS to get around this.

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
  "order": ["widget:hours-month", "todos", "upcoming", "suggestions", "chats", "ideas", "projects"]
}
```

- `chips` — one-click starters under the Home prompt box. Clicking one puts `prompt` in the box
  (above anything already typed); it is never sent until the user presses Send. End it with a space
  when the user is meant to finish the sentence.
- `hidden` — Home sections not to show. Sections: `stats`, `ideas`, `todos`, `chats`, `projects`,
  `upcoming`.
- `order` — the order of things on Home. Besides the six sections it may name `suggestions` (the
  box of proposals the app makes) and **`widget:<id>`** — a widget a plugin added with
  `ui.addHomeWidget({ id })`. That is how you place a widget above or between the built-in
  sections. Anything you leave out keeps its usual place after the listed ones.

All three keys are optional. Keep what is already there unless the user asked to remove it.

### nav.json

The sidebar. All keys optional; create the file when you first need it.

```json
{
  "hidden": ["projects"],
  "order": ["home", "todos", "chats", "ideas", "docs", "client-intake"],
  "labels": { "ideas": { "en": "Clients", "ar": "العملاء" } }
}
```

- Page ids: `home`, `chats`, `ideas`, `todos`, `docs`, `projects`, `me`, plus the `id` of any page
  a plugin added with `ui.addView`. A plugin page can also be named `<plugin file>:<id>`
  (`hours.js:board`), which stays exact when two plugins use the same `id`; an `id` that is also an
  app page's (`todos`) always means the app's page.
- `hidden` — pages not shown in the sidebar. `home` can never be hidden, and Settings and Help
  always stay. Hiding a page deletes nothing; the user's data is untouched.
- `order` — pages in this order first; the rest follow in their usual order.
- `labels` — rename a page, e.g. "Ideas" → "Clients" for a lawyer or "Classes" for a teacher.
  Give both `en` and `ar` when you can (at least one), 40 characters at most.

To hide or rename a page always use `nav.json` — never CSS.

### todos.json

How the Todos page is laid out. All keys optional.

```json
{ "groupBy": "due", "sort": "due", "filter": "all", "showDone": false }
```

- `groupBy` — `"list"` (by the file each todo lives in — the default), `"due"` (by date: overdue
  first, then what is coming up, then todos with no date), `"tag"`, or `"none"` (one flat list).
- `sort` — `"list"` (the order in the files) or `"due"` (earliest date first).
- `filter` — what the page shows when it opens: `"all"`, `"soon"` (due soon) or `"overdue"`.
- `showDone` — `true` to show finished todos too.
- `moveDone` — `true` to move a todo to its list's Done group when it is ticked off (only in lists
  that have groups; a repeating todo moves on to its next date instead).

### plugins/*.js — Plugin API v1

A plugin is one ES module with a default export. It receives a single object and nothing else: no
network access, no access to the app's internals, no token.

**What a plugin can and cannot send out.** A plugin (or view) has no way out of the app: web
requests and sockets, forms, images, fonts and styles from the network, links and new windows,
frames inside its own page, and peer connections (WebRTC) are all refused or removed before its
code runs. What it reads with its permissions it can show on screen, keep in its own `storage`, or
write into the library folders it declared — nothing else. Do not try to work around this (no
frames, no links that leave the page, no waiting loops): such code is stopped or switched off, and
the user is told the add-on cannot send their notes anywhere.

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
| `thoughts.library.write(path, text)` | Creates or replaces a text file. `path` must be inside one of the plugin's `write` folders and end in `.md`, `.json`, `.csv` or `.txt` | `library:write` |
| `thoughts.library.remove(path)` | Deletes a file inside the `write` folders (the user can get it back from the library's history) | `library:write` |
| `thoughts.todos.list()` | Promise of `[{ file, idea, items: [{ title, text, due, done, group, line }] }]`; `due` is `"YYYY-MM-DD"` or `null` | `todos:read` |
| `thoughts.todos.add(text, where?)` | Adds a todo; append ` @due(YYYY-MM-DD)` for a due date. Without `where`, to the general list. `where` = `{ list, group }`: `list` is a list from `todos.list()` (or its `file`, e.g. `"clients/acme/todo.md"`) that already exists and the plugin can see; `group` one of that list's `groups`. Add-ons never start a new list or group | `todos:write` |
| `thoughts.todos.setDone(item, done)` | Ticks (`true`) or unticks (`false`) a todo | `todos:write` |
| `thoughts.todos.edit(item, text)` | Replaces a todo's text (keep or change its ` @due(…)` suffix) | `todos:write` |
| `thoughts.todos.remove(item)` | Deletes a todo | `todos:write` |
| `thoughts.ideas.list()` | Promise of `[{ path, area, slug, title, status, summary, tags, updated }]` | `ideas:read` |
| `thoughts.ideas.setStatus(path, status)` | Sets an item's status (`path` exactly as given by `ideas.list()`, inside a folder declared in `write`); statuses are `seed`, `active`, `parked`, `done`, `dropped` | `ideas:write` |
| `thoughts.storage.get()` | Promise of the plugin's own saved object (`{}` the first time) | `storage` |
| `thoughts.storage.set(obj)` | Saves `obj` (a plain JSON object, at most 256 KB) in place of what was saved before | `storage` |
| `thoughts.chat.start(prompt)` | Opens a new chat with that prompt. Only right after the user pressed something of this plugin (a button in its page or widget, one of its commands or item actions), a few seconds apart, at most 20 an hour | `chat:start` |
| `thoughts.assistant.ask(question)` | Promise of the assistant's answer, as plain text, shown wherever the plugin wants it ("summarise this client"). The assistant has no tools and sees **only the question**: put in it the text it needs (what the plugin read). Only right after the user pressed something of this plugin, one question at a time, at most 20 an hour and 100 a day, never once the month's spending limit is reached; it costs money like a chat. Up to 40,000 characters | `assistant:ask` |
| `thoughts.ui.toast(msg)` | Shows a short message, under the plugin's name (at most three in ten seconds) | none |
| `thoughts.ui.confirm(text, { confirm?, danger? })` | Promise of `true` (the user said yes) or `false`. The app asks in its own dialog, under the plugin's name; `confirm` is the yes button's label, `danger: true` for something that deletes. Only right after the user pressed something of this plugin. Use it instead of `confirm()`, which add-ons cannot call | none |
| `thoughts.ui.notify(text)` | A short notice under the plugin's name: in the app, and as a system notification while the app is in the background and the user has notifications on ("Timer finished"). At most one every 20 seconds and 10 an hour | `notify` |
| `thoughts.ui.saveFile(name, text)` | Opens the save dialog for a file named `name` (`.csv`, `.txt`, `.md` or `.json`, 5 MB at most); the user chooses where. Promise of `{ saved }`. In a `.csv`, a cell that starts with `=`, `+`, `-` or `@` and is not a number is saved with a `'` in front (so no spreadsheet runs it as a formula); in a `.md`, pictures become links and HTML tags are written as text. Only right after the user pressed something of this plugin | `files:save` |
| `thoughts.ui.print()` | Prints the plugin's page as it is, whole (only from a page, not a Home widget). Only right after the user pressed something of this plugin | none |
| `thoughts.ui.go(view)` | Opens a page: `"home"`, `"todos"`, `"ideas"`, `"docs"`, `"projects"`, `"settings"`, or the `id` of a view you added. Only right after the user pressed something of this plugin | none |
| `thoughts.ui.addView({ id, title, icon?, render(el) })` | Adds a page to the sidebar; `render` fills the element `el` | none |
| `thoughts.ui.addCommand({ id, title, run })` | Adds a command to the command palette | none |
| `thoughts.ui.addHomeWidget({ id, title, render(el) })` | Adds a box to the Home screen (place it with `widget:<id>` in `home.json`) | none |
| `thoughts.ui.addHomeChip({ id, label, prompt })` | Adds a quick-start chip on Home that fills the prompt box (prefer `home.json` when it is static) | chat:start |
| `thoughts.ui.addItemAction({ target, id, label, run })` | Adds an action to each todo (`target: "todo"`) or idea (`target: "idea"`); see below | none |
| `thoughts.on(event, cb)` | Calls `cb` when something happens; returns a function that stops listening | none |

Events for `on`: `layer.changed` (customisations were kept or undone), `todos.changed`,
`library.changed`, `wrap.done` (a wrap finished saving chats into the library), `chat.done` (the
assistant finished answering).

A call the plugin has no permission for is rejected with an error — it does not prompt the user.
Declare the permission in the plugin's `.json` instead.

**The `item` in todo calls.** `todos.setDone`, `todos.edit` and `todos.remove` take a todo exactly
as the app gave it to you — an entry of `items` from `todos.list()` together with its `file`, or
the `item` an item action received: `{ file, line, text, title, due, done }`. Pass it back
unchanged; the app uses `file`, `line` and `text` to find the right line. From `todos.list()`, build it as `{ ...item, file: group.file }`.

**Item actions.** `run` is called with one argument, `{ target, item }`:

- `target: "todo"` → `item` is `{ file, line, text, title, due, done }`
- `target: "idea"` → `item` is `{ path, title, status, area, slug, summary, tags, updated }`

```js
thoughts.ui.addItemAction({
  target: 'todo', id: 'tomorrow', label: 'Move to tomorrow',
  async run({ item }) {
    const d = new Date(); d.setDate(d.getDate() + 1);
    const due = d.toLocaleDateString('en-CA');            // YYYY-MM-DD, local
    await thoughts.todos.edit(item, `${item.title} @due(${due})`);   // needs todos:write
  },
});
thoughts.ui.addItemAction({
  target: 'idea', id: 'park', label: 'Park this',
  run: ({ item }) => thoughts.ideas.setStatus(item.path, 'parked'),   // needs ideas:write
});
```

**Storage.** `storage` is one JSON object per plugin, kept on this computer outside the library
(it is not synced and not part of packs; it does travel in the move file to another computer, and
the app keeps a few earlier copies the user can put back). Read it, change it, write the whole
object back:

```js
const data = await thoughts.storage.get();          // {} the first time
data.entries = [...(data.entries || []), entry];
await thoughts.storage.set(data);
```

Use `storage` for the plugin's own settings and small records. Use `library:write` instead when
the result is something the user should own as a note (a client file, a log) — that lives in
their library, syncs with it, and the assistant can read it.

Writing a good plugin:

- Build the interface with DOM calls (`document.createElement`, `textContent`). Never put library
  text into `innerHTML`.
- Use the tokens (`var(--panel)`, `var(--fg)`, `var(--muted)`, `var(--accent)`, `var(--radius)`)
  so it matches every theme. Put `dir="auto"` on elements showing the user's own text.
- Write visible text in the user's language. No emojis as icons.
- Each page and each Home widget is drawn by its own fresh copy of the plugin: they share **no
  variables**. Anything two of them must both know goes through `storage` or the library.
- Everything the plugin adds is removed automatically when the layer reloads; keep no global state.
- A plugin that throws is switched off and the user is told, so handle empty lists and failed
  calls (`try`/`catch` around `await`).
- `id`s are lowercase with dashes and unique within the plugin.
- Forms work as usual: listen for `submit` and call `event.preventDefault()`. Nothing is ever sent
  anywhere by the browser, and links do not open other pages: a plugin page that replaces itself
  with another page is switched off.
- Wherever the plugin shows up, the app says which plugin it is: its toasts, its pages and its
  commands carry the plugin's name. Do not imitate the app's own messages or settings.
- Never wait for something in a loop (`while (!ready) {}`): code that does not give control back
  freezes the app. Use `await` and events.
- The app's keyboard shortcuts (Cmd/Ctrl with a key, Escape) keep working while the user is in the
  plugin's page or widget. A key the plugin handles itself (Escape to close its own menu, Ctrl+Enter
  to save its form): call `event.preventDefault()` in its `keydown` listener and the app leaves it alone.
- **Try it.** While the user tries a change, the plugin runs as it will once kept, but nothing is
  saved: what it writes to the library, removes, saves in `storage` or sets as an item's status is
  kept aside and read back to it until the preview ends; todo changes are answered as done. The user
  is told each time what would have been saved and where. `chat.start` and `assistant.ask` are
  refused while trying (error `code: "preview"`). So a form or a board can be tried end to end.

### views/*.html

A complete static HTML page shown in the sidebar (the file name without `.html` is its title). It
runs in a sandboxed frame: it cannot reach the app, the user's files or the network on its own. It
can ask for **read-only** data by message, but only what it declares in its own
`views/<name>.json` — for example `views/calendar.json`:
`{ "apiVersion": 1, "permissions": ["todos:read"] }` (a `read` list of folders narrows it, as for a
plugin). With no declaration a view gets no data at all, and a view can never write:

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

Available methods: `library.list`, `library.read`, `library.search`, `todos.list`, `ideas.list`
(permissions `library:read`, `todos:read`, `ideas:read`). Anything that saves, edits or reacts —
a form, a board, a tracker — must be a plugin with `ui.addView`, not an HTML view.

### workflows/*.json

One file per workflow:

```json
{
  "name": "Weekly review",
  "trigger": { "type": "schedule", "every": "week", "at": "16:00", "weekday": 5 },
  "prompt": "Write a short review of this week into the library: …"
}
```

Schedule triggers — `at` is always `"HH:MM"`, 24-hour, the user's local time:

| `every` | Extra field | Runs |
|---|---|---|
| `"day"` | — | every day |
| `"weekdays"` | `days` (optional) | Monday to Friday; or exactly the days in `days`, e.g. `[0, 1, 2, 3, 4]` for Sunday to Thursday |
| `"week"` | `weekday` | once a week; 0–6 where **0 is Sunday**, 4 is Thursday, 5 is Friday |
| `"month"` | `day` (required) | once a month, on day `1`–`28`, or `"last"` for the last day of the month |

```json
{ "type": "schedule", "every": "month", "at": "09:00", "day": "last" }
{ "type": "schedule", "every": "weekdays", "at": "08:30", "days": [0, 1, 2, 3, 4] }
```

There is no day 29, 30 or 31: use `"last"` for month-end work. Ask the user which days their
working week has before using `"weekdays"` (in Saudi Arabia and much of the Gulf it is Sunday to
Thursday).

Event triggers: `{ "type": "event", "on": "wrap.done" }` (after each wrap) or
`{ "type": "event", "on": "app.start" }` (when the app starts).

`prompt` is what the assistant is asked to do, inside the user's library, with nobody watching.
Make it complete on its own: what to read, what to write and where, and how long the result
should be. Follow the conventions in the library's own `CLAUDE.md`.

What the user can rely on (say it this way in the summary, do not promise more):

- A new or changed workflow runs only after the user has allowed it, in exactly the form they saw.
- The app runs one workflow at a time, never during a wrap, and each run appears as a chat the
  user can read.
- If the computer was off or asleep at the scheduled time, the workflow runs **once** when the
  app is next open, provided that is within 24 hours of the scheduled time. Older missed runs are
  skipped, not piled up.
- A run that fails is tried once more ten minutes later.

## Sharing: packs

The user can export their customisations as a pack file and import someone else's. A pack that
carries only some files (a "feature pack" — say, one plugin with its declaration and a bit of CSS)
is **merged** on import: plugin, view and workflow files are added; `home.json`, `nav.json`,
`todos.json` and `themes.json` are merged key by key with lists combined; the pack's CSS goes into
`custom.css` inside a block marked `/* pack:<name> <id> */ … /* end pack:<name> <id> */` (the id is
a short code the pack carries, so that two packs with the same name keep their own blocks; older
packs have none). Leave those
marked blocks as they are unless the user asks to change that pack's look, and keep your own CSS
outside them. A plugin with its own `.json` declaration travels cleanly in a pack — one more
reason not to use `manifest.json`.

## Worked examples

Each example is a complete change: every file, and the summary to write.

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

`plugins/overdue.json`

```json
{ "apiVersion": 1, "permissions": ["todos:read"], "description": "Lists overdue todos on Home" }
```

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

### 4. A client-intake form that saves a new note

A page with a form; submitting it writes one markdown file into `clients/`.

`plugins/client-intake.json`

```json
{
  "apiVersion": 1,
  "permissions": ["library:read", "library:write"],
  "write": ["clients/"],
  "read": ["clients/"],
  "description": "A form that saves each new client as a note in the clients folder"
}
```

`plugins/client-intake.js`

```js
export default function (thoughts) {
  const FIELDS = [
    { key: 'name', label: 'Client name', required: true },
    { key: 'contact', label: 'Phone or email' },
    { key: 'matter', label: 'What they need', long: true },
  ];

  // A safe file name: letters and digits (any language) joined by dashes.
  const slug = (s) => s.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-+|-+$/g, '').slice(0, 60);

  thoughts.ui.addView({
    id: 'client-intake',
    title: 'New client',
    render(el) {
      const form = document.createElement('form');
      form.style.cssText = 'display:grid;gap:12px;max-inline-size:520px;padding:16px';
      const inputs = {};
      for (const f of FIELDS) {
        const label = document.createElement('label');
        label.style.cssText = 'display:grid;gap:4px;color:var(--fg)';
        label.append(f.label);
        const input = document.createElement(f.long ? 'textarea' : 'input');
        input.dir = 'auto';
        input.required = !!f.required;
        if (f.long) input.rows = 4;
        input.style.cssText = 'font:inherit;color:var(--fg);background:var(--panel);border:var(--border-w,1px) solid var(--border);border-radius:var(--radius);padding:8px';
        label.append(input);
        form.append(label);
        inputs[f.key] = input;
      }
      const save = document.createElement('button');
      save.type = 'submit';
      save.textContent = 'Save client';
      save.style.cssText = 'font:inherit;justify-self:start;padding:8px 16px;border:0;border-radius:var(--radius);background:var(--accent);color:var(--accent-fg);cursor:pointer';
      form.append(save);

      form.addEventListener('submit', async (e) => {
        e.preventDefault();
        const name = inputs.name.value.trim();
        const base = slug(name);
        if (!base) { thoughts.ui.toast('Please enter the client name.'); return; }
        save.disabled = true;
        try {
          // Never overwrite an existing client: pick a free file name.
          const existing = new Set((await thoughts.library.list()).map((f) => (typeof f === 'string' ? f : f.path)));
          let path = `clients/${base}.md`;
          for (let n = 2; existing.has(path); n++) path = `clients/${base}-${n}.md`;
          const today = new Date().toLocaleDateString('en-CA'); // YYYY-MM-DD, local
          const text = [
            `# ${name}`,
            '',
            `- Added: ${today}`,
            `- Contact: ${inputs.contact.value.trim() || '—'}`,
            '',
            '## What they need',
            '',
            inputs.matter.value.trim() || '—',
            '',
          ].join('\n');
          await thoughts.library.write(path, text);
          form.reset();
          thoughts.ui.toast(`Saved ${name}.`);
        } catch (err) {
          thoughts.ui.toast('Could not save the client. Nothing was changed.');
        } finally {
          save.disabled = false;
        }
      });
      el.replaceChildren(form);
    },
  });
}
```

`SUMMARY.md`: "Add a New client form. A new page called New client appears in the sidebar. Fill in
the name, contact and what they need, press Save client, and a note for that client is created in
your clients folder. It never overwrites an existing client. This add-on may write only inside the
clients folder."

### 5. A board you can drag: items by status

Columns for each status; dragging a card to another column (or choosing from the card's menu, for
keyboard users) changes that item's status.

`plugins/status-board.json`

```json
{
  "apiVersion": 1,
  "permissions": ["ideas:read", "ideas:write"],
  "write": ["clients/", "projects/"],
  "description": "A board of your items by status; drag a card to change its status"
}
```

`write` names the areas (top folders) whose items the board may change: list the user's own
(from `INDEX.md` or `ideas.list()`). An item elsewhere is shown but its status stays as it is.

`plugins/status-board.js`

```js
export default function (thoughts) {
  const COLUMNS = [
    { status: 'seed', title: 'New' },
    { status: 'active', title: 'In progress' },
    { status: 'parked', title: 'On hold' },
    { status: 'done', title: 'Done' },
  ];

  thoughts.ui.addView({
    id: 'status-board',
    title: 'Board',
    async render(el) {
      const board = document.createElement('div');
      board.style.cssText = `display:grid;grid-template-columns:repeat(${COLUMNS.length},minmax(180px,1fr));gap:12px;padding:16px;align-items:start;overflow-x:auto`;
      el.replaceChildren(board);

      const move = async (path, status) => {
        try {
          await thoughts.ideas.setStatus(path, status);
        } catch (err) {
          thoughts.ui.toast('Could not move that item.');
        }
        await draw();
      };

      const card = (idea) => {
        const box = document.createElement('div');
        box.draggable = true;
        box.style.cssText = 'display:grid;gap:6px;padding:8px;background:var(--panel);border:var(--border-w,1px) solid var(--border);border-radius:var(--radius);cursor:grab';
        box.addEventListener('dragstart', (e) => {
          e.dataTransfer.setData('text/plain', idea.path);
          e.dataTransfer.effectAllowed = 'move';
        });
        const title = document.createElement('div');
        title.dir = 'auto';
        title.textContent = idea.title;
        // The same move without a mouse.
        const pick = document.createElement('select');
        pick.setAttribute('aria-label', `Status of ${idea.title}`);
        pick.style.cssText = 'font:inherit;color:var(--fg);background:var(--panel-2);border:var(--border-w,1px) solid var(--border);border-radius:var(--radius)';
        for (const c of COLUMNS) {
          const o = document.createElement('option');
          o.value = c.status;
          o.textContent = c.title;
          o.selected = c.status === idea.status;
          pick.append(o);
        }
        pick.addEventListener('change', () => move(idea.path, pick.value));
        box.append(title, pick);
        return box;
      };

      async function draw() {
        let ideas = [];
        try { ideas = await thoughts.ideas.list(); } catch { /* show empty columns */ }
        board.replaceChildren();
        for (const c of COLUMNS) {
          const col = document.createElement('section');
          col.style.cssText = 'display:grid;gap:8px;align-content:start;min-block-size:120px;padding:8px;background:var(--panel-2);border-radius:var(--radius-lg, var(--radius))';
          const h = document.createElement('h3');
          const mine = ideas.filter((i) => i.status === c.status);
          h.textContent = `${c.title} (${mine.length})`;
          h.style.cssText = 'margin:0;font-size:0.9em;color:var(--muted)';
          col.append(h, ...mine.map(card));
          col.addEventListener('dragover', (e) => { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; });
          col.addEventListener('drop', (e) => {
            e.preventDefault();
            const path = e.dataTransfer.getData('text/plain');
            const idea = ideas.find((i) => i.path === path);
            if (idea && idea.status !== c.status) move(path, c.status);
          });
          board.append(col);
        }
      }

      await draw();
      thoughts.on('library.changed', draw);
    },
  });
}
```

Items whose status is not one of the columns (here `dropped`) are simply not shown. Rename the
column titles to the user's own words ("Enquiry", "Open matter", "Waiting", "Closed") — the
`status` values stay as they are.

`SUMMARY.md`: "Add a Board page. A new page called Board shows your items in four columns: New, In
progress, On hold and Done. Drag a card to another column to change its status, or use the menu on
the card. Nothing else about the item changes."

### 6. A billable-hours tracker with a monthly total on Home

Entries are kept in the plugin's own `storage`; a page adds and lists them and a Home widget shows
this month's total. The page and the widget share nothing but `storage`.

`plugins/hours.json`

```json
{ "apiVersion": 1, "permissions": ["storage"], "description": "Tracks billable hours and shows this month's total on Home" }
```

`plugins/hours.js`

```js
export default function (thoughts) {
  const today = () => new Date().toLocaleDateString('en-CA'); // YYYY-MM-DD, local
  const load = async () => {
    try {
      const data = await thoughts.storage.get();
      return Array.isArray(data.entries) ? data.entries : [];
    } catch { return []; }
  };
  const monthTotal = (entries) => {
    const month = today().slice(0, 7);
    return entries.filter((e) => e.date.startsWith(month)).reduce((sum, e) => sum + e.hours, 0);
  };
  const fmt = (n) => (Math.round(n * 100) / 100).toString();

  thoughts.ui.addHomeWidget({
    id: 'hours-month',
    title: 'Billable hours this month',
    async render(el) {
      const total = document.createElement('button');
      total.style.cssText = 'font:inherit;font-size:1.6em;color:var(--fg);background:none;border:0;padding:0;cursor:pointer';
      total.textContent = `${fmt(monthTotal(await load()))} h`;
      total.setAttribute('aria-label', `${total.textContent} — open the hours page`);
      total.onclick = () => thoughts.ui.go('hours');
      el.replaceChildren(total);
    },
  });

  thoughts.ui.addView({
    id: 'hours',
    title: 'Hours',
    async render(el) {
      let entries = await load();
      const wrap = document.createElement('div');
      wrap.style.cssText = 'display:grid;gap:16px;max-inline-size:640px;padding:16px';
      const field = 'font:inherit;color:var(--fg);background:var(--panel);border:var(--border-w,1px) solid var(--border);border-radius:var(--radius);padding:8px';

      const form = document.createElement('form');
      form.style.cssText = 'display:flex;flex-wrap:wrap;gap:8px;align-items:end';
      const date = Object.assign(document.createElement('input'), { type: 'date', value: today(), required: true });
      const client = Object.assign(document.createElement('input'), { type: 'text', placeholder: 'Client', required: true, dir: 'auto' });
      const hours = Object.assign(document.createElement('input'), { type: 'number', min: '0.25', max: '24', step: '0.25', placeholder: 'Hours', required: true });
      date.setAttribute('aria-label', 'Date');
      client.setAttribute('aria-label', 'Client');
      hours.setAttribute('aria-label', 'Hours');
      for (const i of [date, client, hours]) i.style.cssText = field;
      const add = document.createElement('button');
      add.type = 'submit';
      add.textContent = 'Add';
      add.style.cssText = 'font:inherit;padding:8px 16px;border:0;border-radius:var(--radius);background:var(--accent);color:var(--accent-fg);cursor:pointer';
      form.append(date, client, hours, add);

      const total = document.createElement('p');
      total.style.cssText = 'margin:0;color:var(--muted)';
      const list = document.createElement('div');
      list.style.cssText = 'display:grid;gap:4px';

      const save = async (next) => {
        try {
          await thoughts.storage.set({ entries: next });
          entries = next;
        } catch (err) {
          thoughts.ui.toast('Could not save. Your earlier entries are unchanged.');
        }
        draw();
      };

      function draw() {
        total.textContent = `This month: ${fmt(monthTotal(entries))} h`;
        list.replaceChildren();
        const sorted = [...entries].sort((a, b) => b.date.localeCompare(a.date));
        if (!sorted.length) {
          const empty = document.createElement('p');
          empty.textContent = 'No hours yet. Add your first entry above.';
          empty.style.color = 'var(--muted)';
          list.append(empty);
        }
        for (const e of sorted.slice(0, 200)) {
          const row = document.createElement('div');
          row.style.cssText = 'display:flex;gap:12px;align-items:center;padding:6px 0;border-block-end:var(--border-w,1px) solid var(--border)';
          const text = document.createElement('span');
          text.dir = 'auto';
          text.style.flex = '1';
          text.textContent = `${e.date} · ${e.client} · ${fmt(e.hours)} h`;
          const del = document.createElement('button');
          del.textContent = 'Remove';
          del.setAttribute('aria-label', `Remove ${fmt(e.hours)} h for ${e.client} on ${e.date}`);
          del.style.cssText = 'font:inherit;color:var(--danger);background:none;border:0;cursor:pointer';
          del.onclick = () => save(entries.filter((x) => x.id !== e.id));
          row.append(text, del);
          list.append(row);
        }
      }

      form.addEventListener('submit', (ev) => {
        ev.preventDefault();
        const h = Number(hours.value);
        if (!client.value.trim() || !(h > 0)) return;
        const entry = { id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`, date: date.value || today(), client: client.value.trim(), hours: h };
        client.value = '';
        hours.value = '';
        save([...entries, entry]);
      });

      wrap.append(form, total, list);
      el.replaceChildren(wrap);
      draw();
    },
  });
}
```

`home.json` gains `"widget:hours-month"` at the front of `order` (keeping what is already there),
so the total sits at the top of Home.

`SUMMARY.md`: "Track billable hours. A new page called Hours lets you add the date, client and
hours worked, and lists your entries. Home now shows your total for this month at the top; click
it to open the page. The entries are kept on this computer, not in your notes."

### 7. Group todos by due date

No plugin needed — one settings file.

`todos.json`

```json
{ "groupBy": "due", "sort": "due", "filter": "all", "showDone": false }
```

If `todos.json` already exists, change only the keys the user asked about and keep the rest.

`SUMMARY.md`: "Group your todos by date. The Todos page now groups todos by when they are due
instead of one list per file, with overdue ones first. Open Todos to see it."
