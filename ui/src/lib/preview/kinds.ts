// Which preview a file gets, from its name alone (pure: no I/O, no DOM). FilePreview.svelte asks this
// first, then loads the bytes and hands them to the renderer registered for the kind (renderers.ts).

export type PreviewKind =
  | 'image' | 'svg' | 'pdf' | 'text' | 'code' | 'json' | 'yaml' | 'xml' | 'html' | 'audio' | 'video' | 'zip'
  | 'docx' | 'xlsx' | 'csv' | 'pptx' | 'odt' | 'ods' | 'odp' | 'legacy' | 'none';

const MB = 1024 * 1024;
/** The background service hands out no file bigger than this (`RAW_MAX_BYTES` in library.rs). */
export const SERVED_MAX = 25 * MB;

/** Largest file each kind is previewed for. Over it the preview says the file is too big to show
 *  here (and offers to show it in its folder) instead of loading it. Text kinds are lower than the
 *  service's cap because the whole file is decoded in the window. */
export const LIMITS: Record<PreviewKind, number> = {
  image: SERVED_MAX, svg: 5 * MB, pdf: SERVED_MAX, text: 10 * MB, code: 5 * MB, json: 5 * MB, yaml: 5 * MB,
  xml: 5 * MB, html: 5 * MB, audio: SERVED_MAX, video: SERVED_MAX, zip: SERVED_MAX,
  docx: SERVED_MAX, xlsx: SERVED_MAX, csv: 10 * MB, pptx: SERVED_MAX, odt: SERVED_MAX, ods: SERVED_MAX, odp: SERVED_MAX,
  legacy: SERVED_MAX, none: 0,
};

const EXT: Record<string, PreviewKind> = {};
const add = (kind: PreviewKind, exts: string) => { for (const e of exts.split(' ')) EXT[e] = kind; };
// heic/heif/tif/tiff: shown only where the window's engine decodes them (Safari's does); otherwise the
// picture fails to load and the preview says it can't be shown.
add('image', 'png jpg jpeg jpe jfif gif webp bmp avif ico heic heif tif tiff');
add('svg', 'svg');
add('pdf', 'pdf');
add('text', 'txt text log md markdown mdown rst org nfo adoc asciidoc textile jsonl ndjson ics vcf eml out err lst');
add('json', 'json jsonc json5 geojson webmanifest har ipynb');
add('yaml', 'yaml yml');
add('xml', 'xml plist xsd xsl xslt rss atom kml gpx opml xliff xlf csproj resx storyboard xib');
add('html', 'html htm xhtml');
add('audio', 'mp3 m4a aac wav ogg oga opus flac caf aif aiff');
add('video', 'mp4 m4v mov webm ogv');
add('zip', 'zip');
// Macro-enabled Office files (docm, xlsm, pptm) are only read as data here: nothing in them runs.
add('docx', 'docx docm dotx dotm');
add('xlsx', 'xlsx xlsm xltx xltm');
add('csv', 'csv tsv tab');
add('pptx', 'pptx pptm ppsx ppsm potx potm');
add('odt', 'odt ott');
add('ods', 'ods ots');
add('odp', 'odp otp');
add('legacy', 'doc xls ppt rtf pps dot xlt pot wpd wps');

/** Kinds of file this app has no way to show and that are certainly not text: they belong to
 *  another program (Apple's and other office formats, e-books, design files), are packed, or are
 *  programs. The preview says to open the file in its own app rather than trying anything.
 *  Nothing in such a file is ever read or run here. */
const APP_ONLY = new Set(('xlsb numbers pages key one pub vsd vsdx mpp accdb mdb epub mobi azw3 djvu xps '
  + 'psd psb ai eps indd sketch fig xd afdesign afphoto dwg dxf blend raw cr2 nef arw dng '
  + '7z rar tar gz tgz bz2 xz zst lz4 cab iso dmg pkg deb rpm msi exe dll so dylib app apk ipa jar war class o a bin '
  + 'sqlite sqlite3 db ttf otf woff woff2 mkv avi wmv flv wma mid midi').split(' '));

/** Is this a file that can only be opened in its own app (see `APP_ONLY`)? */
export function appOnly(name: string): boolean {
  return APP_ONLY.has(extOf(name));
}

/** The largest file with a name that says nothing which is tried as text. */
export const TRIAL_MAX = 2 * MB;

/** What the preview does with a file. `kind`: the renderer's kind. `trial`: the name says nothing
 *  about the file (no extension, or one not known here), so it is shown as text if its bytes turn
 *  out to be text (text.ts `looksLikeText`) and as "can't be shown" otherwise. `appOnly`: see above. */
export function previewPlan(name: string, size?: number): { kind: PreviewKind; trial: boolean; appOnly: boolean } {
  const kind = previewKind(name, size);
  if (kind !== 'none' || (size !== undefined && size > SERVED_MAX)) return { kind, trial: false, appOnly: false };
  if (appOnly(name)) return { kind: 'none', trial: false, appOnly: true };
  if (!baseOf(name) || (size !== undefined && size > TRIAL_MAX)) return { kind: 'none', trial: false, appOnly: false };
  return { kind: 'text', trial: true, appOnly: false };
}

/** Source code: extension → language hint for the highlighter (highlight.ts). */
const CODE: Record<string, string> = {};
const code = (lang: string, exts: string) => { for (const e of exts.split(' ')) CODE[e] = lang; };
code('js', 'js mjs cjs jsx');
code('ts', 'ts mts cts tsx');
code('py', 'py pyw pyi');
code('rb', 'rb rake gemspec');
code('rust', 'rs');
code('go', 'go');
code('c', 'c h');
code('cpp', 'cpp cc cxx hpp hh hxx ino');
code('cs', 'cs');
code('java', 'java');
code('kotlin', 'kt kts');
code('swift', 'swift');
code('objc', 'm mm');
code('scala', 'scala sc');
code('dart', 'dart');
code('php', 'php');
code('sh', 'sh bash zsh fish ksh command');
code('ps', 'ps1 psm1');
code('bat', 'bat cmd');
code('css', 'css scss sass less');
code('sql', 'sql');
code('toml', 'toml');
code('ini', 'ini cfg conf env properties editorconfig gitconfig');
code('lua', 'lua');
code('r', 'r');
code('perl', 'pl pm');
code('vue', 'vue svelte astro');
code('graphql', 'graphql gql');
code('diff', 'diff patch');
code('make', 'mk mak');
code('elixir', 'ex exs');
code('haskell', 'hs');
code('clojure', 'clj cljs edn');
code('zig', 'zig');
code('nim', 'nim');
code('julia', 'jl');
code('tex', 'tex sty bib');
code('proto', 'proto');
code('gradle', 'gradle');
code('srt', 'srt vtt');

/** Files known by their whole name (no extension). */
const NAMES: Record<string, [PreviewKind, string?]> = {
  makefile: ['code', 'make'], gnumakefile: ['code', 'make'], dockerfile: ['code', 'sh'], containerfile: ['code', 'sh'],
  gemfile: ['code', 'rb'], rakefile: ['code', 'rb'], podfile: ['code', 'rb'], brewfile: ['code', 'rb'], procfile: ['code', 'sh'],
  '.gitignore': ['code', 'sh'], '.gitattributes': ['code', 'sh'], '.dockerignore': ['code', 'sh'], '.npmrc': ['code', 'ini'],
  '.env': ['code', 'ini'], '.editorconfig': ['code', 'ini'], '.bashrc': ['code', 'sh'], '.zshrc': ['code', 'sh'], '.profile': ['code', 'sh'],
  license: ['text'], licence: ['text'], copying: ['text'], readme: ['text'], changelog: ['text'], authors: ['text'], notice: ['text'], todo: ['text'],
};

const baseOf = (name: string) => (name.split(/[\\/]/).pop() || name).trim();

/** The file's extension, lower case, without the dot ('' when it has none). */
export function extOf(name: string): string {
  const b = baseOf(name);
  const i = b.lastIndexOf('.');
  return i > 0 ? b.slice(i + 1).toLowerCase() : '';
}

/** The preview a file gets. `size` (bytes, when known): a file bigger than the service hands out
 *  can't be loaded at all, so it is `none`. */
export function previewKind(name: string, size?: number): PreviewKind {
  if (size !== undefined && size > SERVED_MAX) return 'none';
  const b = baseOf(name).toLowerCase();
  const named = NAMES[b];
  if (named) return named[0];
  const e = extOf(b);
  if (!e) return 'none';
  return EXT[e] ?? (CODE[e] ? 'code' : 'none');
}

/** The highlighter's language for a file ('' = plain text). Set for code, and for the text kinds
 *  that are shown as highlighted source (json, yaml, xml, html). */
export function codeLang(name: string): string {
  const b = baseOf(name).toLowerCase();
  const named = NAMES[b];
  if (named) return named[1] ?? '';
  const e = extOf(b);
  if (CODE[e]) return CODE[e];
  const k = EXT[e];
  if (k === 'json') return 'json';
  if (k === 'yaml') return 'yaml';
  if (k === 'xml' || k === 'svg') return 'xml';
  if (k === 'html') return 'html';
  if (e === 'md' || e === 'markdown' || e === 'mdown') return 'md';
  return '';
}

/** Is `size` over what this kind is previewed for? Unknown size: not (yet). */
export function tooLarge(kind: PreviewKind, size?: number): boolean {
  return size !== undefined && size > LIMITS[kind];
}

/** Kinds whose bytes are text, decoded in the window (the text renderers). */
export const TEXT_KINDS: ReadonlySet<PreviewKind> = new Set(['text', 'code', 'json', 'yaml', 'xml', 'html', 'csv']);

/** The content type a kind's bytes are wrapped in, so the window's engine treats them right (an
 *  SVG only ever as a picture, a PDF only as a PDF). Office kinds stay generic: they are parsed, never handed to the engine. */
export function mimeFor(name: string, kind: PreviewKind = previewKind(name)): string {
  const e = extOf(name);
  switch (kind) {
    case 'svg': return 'image/svg+xml';
    case 'pdf': return 'application/pdf';
    case 'image': return ({ jpg: 'image/jpeg', jpeg: 'image/jpeg', jpe: 'image/jpeg', jfif: 'image/jpeg', ico: 'image/x-icon', tif: 'image/tiff', svg: 'image/svg+xml' } as Record<string, string>)[e] ?? `image/${e}`;
    case 'audio': return ({ mp3: 'audio/mpeg', m4a: 'audio/mp4', aac: 'audio/aac', wav: 'audio/wav', ogg: 'audio/ogg', oga: 'audio/ogg', opus: 'audio/ogg', flac: 'audio/flac', caf: 'audio/x-caf', aif: 'audio/aiff', aiff: 'audio/aiff' } as Record<string, string>)[e] ?? 'audio/mpeg';
    case 'video': return ({ mp4: 'video/mp4', m4v: 'video/mp4', mov: 'video/quicktime', webm: 'video/webm', ogv: 'video/ogg' } as Record<string, string>)[e] ?? 'video/mp4';
    default: return TEXT_KINDS.has(kind) ? 'text/plain' : 'application/octet-stream';
  }
}
