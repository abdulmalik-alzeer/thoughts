import { marked } from 'marked';
import DOMPurify from 'dompurify';

marked.setOptions({ gfm: true, breaks: false });

/** Render markdown to sanitized HTML (frontmatter stripped). */
export function md(src: string): string {
  const body = src.replace(/^---\n[\s\S]*?\n---\n?/, '');
  return DOMPurify.sanitize(marked.parse(body, { async: false }) as string);
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
