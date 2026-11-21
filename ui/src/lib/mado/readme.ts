// MADO, round 0.4.0: text from the project (its README, its conventions) drawn through the app's own
// markdown renderer (lib/md.ts: sanitised, nothing interactive). One more thing is taken out here:
// pictures, so that a file a build wrote cannot make the window fetch an address of its choosing.
import { md } from '../md';

export function projectDocHtml(text: string, max = 8000): string {
  const html = md(String(text ?? '').slice(0, max));
  if (!/<img\b/i.test(html)) return html;
  // A template's content is inert: nothing in it is loaded while it is edited.
  const box = document.createElement('template');
  box.innerHTML = html;
  for (const img of Array.from(box.content.querySelectorAll('img'))) {
    const alt = img.getAttribute('alt')?.trim();
    if (alt) img.replaceWith(document.createTextNode(`[${alt}]`)); else img.remove();
  }
  return box.innerHTML;
}
