// Svelte action that moves an element's content into a shadow root, so the page's stylesheets —
// including the staged `custom.css` a user is being asked to review — cannot restyle or hide it.
// Used by the Review panel and the preview banner: the two places where the user decides whether a
// customisation is kept must always look the way the app drew them.
//
// * Nothing from the document's CSS applies inside (no .btn, no .modal): everything is in CSS below.
// * CSS custom properties DO inherit into a shadow tree, so the colours are pinned to fixed values
//   (AA contrast, light and dark) on the inner wrapper instead of reading the theme's variables.
// * The host's own box is set with inline `!important` declarations, which page CSS cannot override.
// * Focus: `document.activeElement` is the host for anything focused inside, so the focus trap here
//   uses the shadow root's own `activeElement`.

const CSS = `
:host { all: initial; }
.sh {
  --bg: #ffffff; --fg: #1a1a1a; --muted: #555555; --line: #c4c4c4; --fill: #f0f0f2; --fill2: #e2e2e6;
  --accent: #0b5cad; --on-accent: #ffffff; --warn: #8a5a00; --danger: #b3261e; --ok: #1b6e3c;
  color: var(--fg); font: 14px/1.55 -apple-system, BlinkMacSystemFont, "Segoe UI", "SF Arabic", "Noto Sans Arabic", system-ui, sans-serif;
  text-align: start; letter-spacing: 0;
}
.sh.dark {
  --bg: #1c1c1e; --fg: #f2f2f2; --muted: #b8b8b8; --line: #55555a; --fill: #2c2c2e; --fill2: #3a3a3d;
  --accent: #8ab4ff; --on-accent: #0b1730; --warn: #f0c060; --danger: #ff8a80; --ok: #6fd69a;
}
* { box-sizing: border-box; }
.bg { position: fixed; inset: 0; background: rgba(0,0,0,.45); display: grid; place-items: center; }
.modal { background: var(--bg); color: var(--fg); border: 1px solid var(--line); border-radius: 16px; padding: 22px; width: min(640px, 92vw); max-height: 86vh; overflow: auto; box-shadow: 0 20px 50px rgba(0,0,0,.3); display: grid; gap: 10px; outline: none; }
h1 { font-size: 22px; font-weight: 600; line-height: 1.2; }
h1, h2, p, ul { margin: 0; }
h2 { font-size: 14px; font-weight: 600; margin-top: 6px; }
.row { display: flex; gap: 8px; align-items: center; }
.wrap, .end { flex-wrap: wrap; }
.end { margin-top: 8px; }
.grow { flex: 1; min-width: 0; }
.muted { color: var(--muted); }
.icon { flex: none; }
.pill { font-size: 12px; font-weight: 500; padding: 1px 8px; border-radius: 99px; background: var(--fill); color: var(--muted); white-space: nowrap; }
.btn { font: inherit; font-size: 13px; font-weight: 500; color: var(--fg); background: var(--fill); border: 1px solid var(--line); border-radius: 8px; padding: 6px 12px; min-height: 30px; cursor: pointer; display: inline-flex; gap: 6px; align-items: center; justify-content: center; white-space: nowrap; }
.btn:hover { background: var(--fill2); }
.btn.primary { background: var(--accent); color: var(--on-accent); border-color: transparent; }
.btn.danger { color: var(--danger); }
.btn.ghost { background: transparent; border-color: transparent; }
.btn:disabled { opacity: .5; cursor: default; }
:focus-visible { outline: 3px solid var(--accent); outline-offset: 2px; }
.sum { white-space: pre-wrap; overflow-wrap: anywhere; }
.warn { display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--warn); background: var(--fill); font-size: 13px; }
.warn.bad { border-color: var(--danger); border-width: 2px; }
.warn > .icon { margin-top: 2px; color: var(--warn); }
.warn.bad > .icon { color: var(--danger); }
.warn p { margin-top: 4px; }
.warn ul { padding-inline-start: 20px; margin-top: 2px; }
.pl { font-weight: 600; }
.sub { font-weight: 400; }
pre { font-family: "SF Mono", ui-monospace, Menlo, Consolas, monospace; background: var(--bg); border: 1px solid var(--line); border-radius: 8px; margin: 4px 0 0; max-height: 160px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12.5px; padding: 8px; text-align: start; }
.allow { display: flex; gap: 8px; align-items: center; margin: 8px 0 0; font-size: 13px; font-weight: 600; }
input[type="checkbox"] { width: 16px; height: 16px; accent-color: var(--accent); }
.files { list-style: none; padding: 0; display: grid; gap: 4px; font-size: 13px; max-height: 180px; overflow: auto; }
.files bdi, .mono { font-family: "SF Mono", ui-monospace, Menlo, Consolas, monospace; font-size: 12.5px; overflow-wrap: anywhere; }
.st.added { color: var(--ok); }
.st.deleted { color: var(--danger); }
.err { color: var(--danger); }
.spin { width: 14px; height: 14px; border-radius: 50%; border: 2px solid var(--fill2); border-top-color: var(--accent); animation: spin .7s linear infinite; flex: none; }
@keyframes spin { to { transform: rotate(360deg); } }
.banner { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin: 4px 28px 0; padding: 10px 14px; border-radius: 12px; background: var(--bg); color: var(--fg); border: 2px solid var(--accent); box-shadow: 0 4px 16px rgba(0,0,0,.18); font-size: 13px; }
.banner > .icon { color: var(--accent); }
.banner .grow { min-width: 220px; }
@media (prefers-reduced-motion: reduce) { * { animation-duration: 1ms !important; } }
`;

let sheet: CSSStyleSheet | null = null;

export type ShieldOpts = {
  /** Inline box for the host element, e.g. 'position:fixed;inset:0;z-index:60'. Applied with !important. */
  box?: string;
  /** Keep keyboard focus inside (a dialog) and restore it on close. */
  trap?: boolean;
  onEscape?: () => void;
  /** A click on the element marked `data-backdrop` itself (not its children). */
  onBackdrop?: () => void;
};

const FOCUSABLE = 'a[href],button:not([disabled]),input:not([disabled]),textarea:not([disabled]),select:not([disabled]),[tabindex]:not([tabindex="-1"])';

export function shield(node: HTMLElement, opts: ShieldOpts = {}) {
  const prev = document.activeElement as HTMLElement | null;
  const root = node.attachShadow({ mode: 'open' });
  try {
    sheet ??= (() => { const s = new CSSStyleSheet(); s.replaceSync(CSS); return s; })();
    root.adoptedStyleSheets = [sheet];
  } catch {
    const st = document.createElement('style'); st.textContent = CSS; root.appendChild(st);
  }
  const inner = document.createElement('div');
  inner.className = 'sh';
  while (node.firstChild) inner.appendChild(node.firstChild);
  root.appendChild(inner);

  for (const d of ('display:block;visibility:visible;opacity:1;transform:none;filter:none;clip-path:none;pointer-events:auto;' + (opts.box ?? '')).split(';')) {
    const i = d.indexOf(':');
    if (i > 0) node.style.setProperty(d.slice(0, i).trim(), d.slice(i + 1).trim(), 'important');
  }

  const html = document.documentElement;
  const look = () => inner.classList.toggle('dark', html.dataset.mode === 'dark');
  look();
  const mo = new MutationObserver(look);
  mo.observe(html, { attributes: true, attributeFilter: ['data-mode'] });

  const items = () => Array.from(inner.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => el.getClientRects().length > 0);
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && opts.onEscape) { e.stopPropagation(); opts.onEscape(); return; }
    if (!opts.trap || e.key !== 'Tab') return;
    const list = items();
    if (!list.length) { e.preventDefault(); return; }
    const cur = root.activeElement as HTMLElement | null;
    const first = list[0], last = list[list.length - 1];
    if (!cur || !inner.contains(cur)) { e.preventDefault(); first.focus(); }
    else if (e.shiftKey && cur === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && cur === last) { e.preventDefault(); first.focus(); }
  }
  function onClick(e: MouseEvent) {
    const el = e.target as HTMLElement | null;
    if (el?.hasAttribute?.('data-backdrop')) opts.onBackdrop?.();
  }
  // Focus that leaves the dialog (a click on the page behind is impossible, but Tab from the
  // browser chrome is not) is pulled back in.
  function onDocFocus(e: FocusEvent) {
    if (!opts.trap || node.contains(e.target as Node) || e.target === node) return;
    if ((e.target as HTMLElement | null)?.closest?.('.modal-bg.top, .toast-wrap')) return; // a confirm dialog or toast on top
    (inner.querySelector<HTMLElement>('[data-autofocus]') ?? items()[0])?.focus();
  }
  root.addEventListener('keydown', onKey as EventListener);
  root.addEventListener('click', onClick as EventListener);
  if (opts.trap) {
    document.addEventListener('focusin', onDocFocus);
    queueMicrotask(() => (inner.querySelector<HTMLElement>('[data-autofocus]') ?? items()[0])?.focus());
  }
  return {
    update(o: ShieldOpts) { opts = { ...opts, ...o }; },
    destroy() {
      mo.disconnect();
      document.removeEventListener('focusin', onDocFocus);
      if (opts.trap) prev?.focus?.();
    },
  };
}
