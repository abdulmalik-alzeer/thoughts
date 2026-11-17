// Which component shows each kind of file (kinds.ts). Each renderer is loaded only when a file of its
// kind is first shown, so it (and whatever it parses with) is a chunk of its own.
//
// The contract every renderer follows (FilePreview.svelte is the only caller):
// - props: RendererProps below. `data` is the whole file, already loaded and under the kind's size
//   limit; a renderer never fetches anything itself and never loads remote content.
// - it shows its own "working…" state while it parses, and its own content when done;
// - when it finds it cannot show this particular file (not really that format, damaged, a codec
//   the window can't play), it calls `unsupported()` once: FilePreview then shows the shared
//   "can't be shown here" state with "Show in Finder";
// - every object URL it creates it revokes on destroy (and when `data` changes).
import type { Component } from 'svelte';
import type { PreviewKind } from './kinds';

export type RendererProps = {
  /** The file's bytes, typed (kinds.ts `mimeFor`). `await data.arrayBuffer()` for raw bytes. */
  data: Blob;
  /** The file's name (for labels, and the language of code). */
  name: string;
  kind: PreviewKind;
  /** The highlighter's language for code and structured text ('' = plain). */
  lang: string;
  /** Fill the available height (a viewer dialog) rather than a page-sized frame. */
  fill: boolean;
  /** Call when this file turns out not to be showable after all. `detail`: a plain reason, optional. */
  unsupported: (detail?: string) => void;
  /** Open a picture in the app's picture viewer (zoom and move); FilePreview always passes it.
   *  Pictures inside rendered content can use `use:zoomImages` (lib/zoomImages.ts) instead. */
  zoom?: (src: string, alt: string) => void;
};

export type Loader = () => Promise<{ default: Component<RendererProps> }>;

/** One line per kind. `null`: no renderer (yet): the "can't be shown here" state. */
export const renderers: Record<PreviewKind, Loader | null> = {
  image: () => import('./renderers/Image.svelte'),
  svg: () => import('./renderers/Image.svelte'),
  pdf: () => import('./renderers/Pdf.svelte'),
  text: () => import('./renderers/Code.svelte'),
  code: () => import('./renderers/Code.svelte'),
  json: () => import('./renderers/Json.svelte'),
  yaml: () => import('./renderers/Code.svelte'),
  xml: () => import('./renderers/Code.svelte'),
  html: () => import('./renderers/Html.svelte'),
  audio: () => import('./renderers/Media.svelte'),
  video: () => import('./renderers/Media.svelte'),
  zip: () => import('./renderers/Zip.svelte'),
  docx: () => import('./renderers/Docx.svelte'),
  xlsx: () => import('./renderers/Sheet.svelte'),
  csv: () => import('./renderers/Sheet.svelte'),
  pptx: () => import('./renderers/Slides.svelte'),
  odt: () => import('./renderers/Docx.svelte'),
  ods: () => import('./renderers/Sheet.svelte'),
  odp: () => import('./renderers/Slides.svelte'),
  legacy: () => import('./renderers/Docx.svelte'), // RTF; old binary Office files say they can't be shown
  none: null,
};
