// Printing a note, and saving it as a PDF, from any view that shows rendered markdown (Docs, an
// item's front page and its log). The view hands over its rendered note; a clean copy of it is put
// in `#print-root` at the end of <body>, and while `html.print-doc` is set only that copy is laid out
// on paper, in a light print style whatever the theme (printPdf.css). Then:
// - Print: the system print window (`window.print()`).
// - Save as PDF: in the desktop app on macOS the shell asks where to save and prints the window
//   straight into that file, page by page (`save_pdf`, src-tauri/src/pdf.rs). Where it cannot (Windows,
//   Linux, the browser preview, an older shell) the print window opens with a hint on how to save
//   it as a PDF there.
import './printPdf.css';
import { inTauri } from './api';
import { toast } from './store.svelte';
import { t } from './i18n';
import { docDir } from './content/docx';
import { unmarkImages } from './zoomImages';

export type PrintSource = {
  /** The rendered markdown: an element whose children are copied, or several nodes in order. */
  content: Node | Node[];
  /** The note's title (the PDF's suggested name, and the window title while printing). */
  title: string;
  /** A heading put above the content (when the view shows the title outside the rendered note). */
  heading?: string;
};

/** The note's title for a PDF: its frontmatter `title:`, else its first heading, else `fallback`. */
export function titleOf(src: string, fallback = ''): string {
  const text = String(src ?? '').replace(/^\uFEFF/, '');
  const fm = /^---\r?\n([\s\S]*?)\r?\n---(\r?\n|$)/.exec(text);
  if (fm) {
    const m = /^title:\s*(.+?)\s*$/m.exec(fm[1]);
    const v = m?.[1].replace(/^(["'])(.*)\1$/, '$2').trim();
    if (v) return v;
  }
  const body = fm ? text.slice(fm[0].length) : text;
  let fenced = false;
  for (const line of body.split(/\r?\n/)) {
    if (/^\s*(```|~~~)/.test(line)) { fenced = !fenced; continue; }
    if (fenced) continue;
    const h = /^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$/.exec(line);
    if (h) {
      // Inline markup is not part of the name: **bold**, `code`, [text](link).
      const plain = h[1].replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1').replace(/[*_`~]+/g, '').trim();
      if (plain) return plain;
    }
  }
  return fallback;
}

/** A file name every system accepts, from a title: no path separators or reserved characters, no
 *  invisible direction marks or control characters, no leading/trailing dots or spaces, at most 100
 *  characters, ending in `.pdf`. Letters of any script stay (an Arabic title gives an Arabic name). */
export function pdfFileName(title: string): string {
  let s = String(title ?? '')
    .replace(/\.pdf$/i, '')
    .replace(/[\u0000-\u001f\u007f\u200b-\u200f\u202a-\u202e\u2066-\u2069\ufeff]/g, ' ')
    .replace(/[\\/:*?"<>|]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/^[.\s]+|[.\s]+$/g, '');
  if (Array.from(s).length > 100) s = Array.from(s).slice(0, 100).join('').replace(/[.\s]+$/, '');
  // Names Windows keeps for devices.
  if (/^(con|prn|aux|nul|com\d|lpt\d)$/i.test(s)) s = `${s} note`;
  return `${s || 'document'}.pdf`;
}

const isExternal = (href: string) => /^(https?:|mailto:)/i.test(href);

/** The copy that is printed: the rendered note without the app's controls (copy buttons, focus
 *  helpers, zoom marks), every image loaded at once, and the address shown after an external link
 *  whose text is not already that address. Runs the way most of the note reads. */
export function buildPrintRoot(src: PrintSource, doc: Document = document): HTMLElement {
  const root = doc.createElement('div');
  root.id = 'print-root';
  root.className = 'md print-md';
  if (src.heading) {
    const h = doc.createElement('h1');
    h.className = 'print-title';
    h.setAttribute('dir', 'auto');
    h.textContent = src.heading;
    root.appendChild(h);
  }
  const nodes = Array.isArray(src.content) ? src.content : Array.from(src.content.childNodes);
  for (const n of nodes) root.appendChild(n.cloneNode(true));
  for (const b of Array.from(root.querySelectorAll('[data-md-copy], button, input, select, textarea, script, style'))) b.remove();
  unmarkImages(root);
  for (const n of Array.from(root.querySelectorAll('[tabindex]'))) n.removeAttribute('tabindex');
  for (const n of Array.from(root.querySelectorAll('[id]'))) n.removeAttribute('id');
  // Folded sections are printed unfolded: paper has nothing to click.
  for (const d of Array.from(root.querySelectorAll('details'))) d.setAttribute('open', '');
  for (const img of Array.from(root.querySelectorAll('img'))) { img.setAttribute('loading', 'eager'); img.removeAttribute('decoding'); }
  for (const a of Array.from(root.querySelectorAll('a[href]'))) {
    const href = a.getAttribute('href') || '';
    const text = (a.textContent || '').trim();
    const bare = (x: string) => x.replace(/^(https?:\/\/|mailto:)/i, '').replace(/\/$/, '');
    if (isExternal(href) && text && bare(text) !== bare(href)) a.classList.add('print-url');
  }
  root.dir = docDir(root.textContent ?? '');
  return root;
}

/** Put the copy in the page and switch the print rules on; returns how to undo it. */
function mount(src: PrintSource): () => void {
  document.getElementById('print-root')?.remove();
  const root = buildPrintRoot(src);
  document.body.appendChild(root);
  const html = document.documentElement;
  html.classList.add('print-doc');
  const was = document.title;
  // The window's title is the name the print window offers a PDF under.
  if (src.title) document.title = src.title;
  return () => { root.remove(); html.classList.remove('print-doc'); document.title = was; };
}

/** Wait (a little) for the copied images to load, so they are on the paper. */
async function imagesReady(timeoutMs = 4000) {
  const root = document.getElementById('print-root');
  const imgs = Array.from(root?.querySelectorAll('img') ?? []).filter((i) => !i.complete);
  if (!imgs.length) return;
  await Promise.race([
    Promise.all(imgs.map((i) => new Promise<void>((r) => { i.addEventListener('load', () => r(), { once: true }); i.addEventListener('error', () => r(), { once: true }); }))),
    new Promise((r) => setTimeout(r, timeoutMs)),
  ]);
}

const isMac = () => ((window as any).__THOUGHTS_PLATFORM__ || '') === 'macos' || /mac/i.test(navigator.platform || navigator.userAgent);

/** Print through the system print window. The copy is removed once the window has closed. */
export async function printNote(src: PrintSource): Promise<void> {
  const undo = mount(src);
  await imagesReady();
  let done = false;
  const finish = () => { if (done) return; done = true; window.removeEventListener('afterprint', finish); undo(); };
  window.addEventListener('afterprint', finish);
  try { window.print(); } catch { finish(); toast(t('docs.printFailed'), { kind: 'error' }); }
}

/** Print the view as it is (its own print rules), e.g. a whole conversation, with the hint on
 *  saving a PDF from the print window. */
export function printView(title: string, hintKey = 'print-pdf-hint') {
  toast(t(isMac() ? 'docsb.pdfHintMac' : 'docsb.pdfHint'), { kind: 'info', duration: 15000, key: hintKey });
  const was = document.title;
  if (title) document.title = title;
  const done = () => { document.title = was; window.removeEventListener('afterprint', done); };
  window.addEventListener('afterprint', done);
  setTimeout(() => { try { window.print(); } catch { done(); toast(t('docs.printFailed'), { kind: 'error' }); } }, 300);
}

let busy = false;
/** Whether a PDF is being saved right now. */
export const pdfBusy = () => busy;

/** Save the note as a PDF. Resolves when done: 'saved', 'cancelled', 'printed' (the print window was
 *  used instead), 'busy' (another one is being saved) or 'failed'; the person is told the outcome. */
export async function saveNotePdf(src: PrintSource): Promise<'saved' | 'cancelled' | 'printed' | 'busy' | 'failed'> {
  if (busy) return 'busy';
  if (inTauri()) {
    busy = true;
    const undo = mount(src);
    try {
      await imagesReady();
      const { invoke } = await import('@tauri-apps/api/core');
      const r = await invoke<any>('save_pdf', { name: pdfFileName(src.title) });
      if (r && typeof r === 'object' && typeof r.id === 'number') {
        const id = r.id as number;
        toast(t('libf-pdf.saved', { file: String(r.file || pdfFileName(src.title)) }), {
          kind: 'success', key: 'pdf-save', duration: 12000,
          action: { label: t('libf-pdf.open'), run: () => openSaved(id) },
        });
        return 'saved';
      }
      if (!r || typeof r !== 'object') return 'cancelled';
      // `{unsupported: true}`: this computer prints instead (below).
    } catch (e) {
      const why = String((e as any)?.message ?? e ?? '');
      if (/^busy$/.test(why)) return 'busy';
      // A shell from before the command prints instead; anything else is a real failure.
      if (!/\bnot found\b|not allowed/i.test(why)) {
        toast(t('libf-pdf.failed'), { kind: 'error', key: 'pdf-save', detail: why || undefined });
        return 'failed';
      }
    } finally {
      undo();
      busy = false;
    }
  }
  toast(t(isMac() ? 'docsb.pdfHintMac' : 'docsb.pdfHint'), { kind: 'info', duration: 15000, key: 'pdf-save' });
  await new Promise((r) => setTimeout(r, 350));
  await printNote(src);
  return 'printed';
}

/** Open a PDF that was just saved; when nothing on this computer opens PDFs, show it in its folder. */
export async function openSaved(id: number) {
  const { invoke } = await import('@tauri-apps/api/core');
  let why = '';
  for (const reveal of [false, true]) {
    try { await invoke('open_saved_pdf', { id, reveal }); return; }
    catch (e) { why = String((e as any)?.message ?? e ?? ''); if (/gone|unknown/.test(why)) break; }
  }
  toast(t(/gone|unknown/.test(why) ? 'libf-pdf.gone' : 'libf-pdf.openFailed'), { kind: 'error', key: 'pdf-open' });
}
