import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { t } from './i18n';

marked.setOptions({ gfm: true, breaks: false });


/** Render markdown to sanitized HTML (frontmatter stripped).
 *  Code blocks get a copy button and tables a horizontally scrollable wrapper, added on the sanitised
 *  DOM tree. */
export function md(src: string): string {
  const body = src.replace(/^---\n[\s\S]*?\n---\n?/, '');
  // Sanitise to a DOM fragment, then add the wrappers with DOM APIs (never by editing the HTML string).
  const frag = DOMPurify.sanitize(marked.parse(body, { async: false }) as string, { RETURN_DOM_FRAGMENT: true });
  for (const pre of Array.from(frag.querySelectorAll('pre'))) {
    const wrap = document.createElement('div');
    wrap.className = 'md-code';
    const btn = document.createElement('button');
    btn.type = 'button'; btn.className = 'md-copy'; btn.setAttribute('data-md-copy', '');
    btn.textContent = t('md.copyCode');
    pre.replaceWith(wrap);
    wrap.append(btn, pre);
  }
  for (const table of Array.from(frag.querySelectorAll('table'))) {
    const wrap = document.createElement('div');
    wrap.className = 'md-table'; wrap.tabIndex = 0;
    table.replaceWith(wrap);
    wrap.append(table);
  }
  const host = document.createElement('div');
  host.append(frag);
  return host.innerHTML;
}

/** Delegated click handler for rendered markdown: wire it on the container (`onclick={mdClick}`).
 *  Returns true when the click was a copy button and has been handled. */
export function mdClick(e: Event): boolean {
  const btn = (e.target as HTMLElement | null)?.closest?.('[data-md-copy]') as HTMLButtonElement | null;
  if (!btn) return false;
  e.preventDefault();
  const code = btn.parentElement?.querySelector('pre')?.innerText ?? '';
  const done = (ok: boolean) => {
    btn.textContent = t(ok ? 'md.copied' : 'md.copyFailed');
    setTimeout(() => { btn.textContent = t('md.copyCode'); }, 1600);
  };
  navigator.clipboard.writeText(code).then(() => done(true), () => done(false));
  return true;
}

// Styles for the two wrappers live here because rendered markdown is injected HTML (no component scope).
if (typeof document !== 'undefined' && !document.getElementById('md-extras')) {
  const s = document.createElement('style');
  s.id = 'md-extras';
  s.textContent = `
.md-code { position: relative; margin: 0 0 10px; }
.md-code pre { margin: 0; overflow-x: auto; direction: ltr; text-align: left; }
.md-copy { position: absolute; top: 6px; inset-inline-end: 6px; font: inherit; font-size: 12px; font-weight: 500; line-height: 1.4;
  color: var(--muted); background: var(--panel, var(--bg)); border: 1px solid var(--hairline); border-radius: 7px; padding: 2px 8px;
  cursor: pointer; opacity: 0; transition: opacity .15s; }
.md-code:hover .md-copy, .md-copy:focus-visible { opacity: 1; }
.md-copy:hover { color: var(--fg); }
@media (hover: none) { .md-copy { opacity: 1; } }
.md-table { overflow-x: auto; max-width: 100%; margin: 0 0 10px; }
.md-table table { min-width: max-content; }
@media print { .md-copy { display: none !important; } .md-table { overflow: visible; } }
`;
  document.head.appendChild(s);
}

/** Resolve a relative markdown link against the file it appears in. */
export function resolveLink(from: string, href: string): string {
  const base = from.split('/').slice(0, -1);
  for (const part of href.split('#')[0].split('/')) {
    if (part === '..') base.pop();
    else if (part && part !== '.') base.push(part);
  }
  return base.join('/');
}
