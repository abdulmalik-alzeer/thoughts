// The fonts a project's design may name. Each stands for a stack of fonts that are already on
// people's computers: nothing is ever downloaded. The daemon holds the same list
// (crates/thoughtsd/src/mado_design_styles.rs, FONTS) and a test there keeps the two in step:
// keep one font per line, written exactly like this.

export type FontKind = 'sans' | 'serif' | 'mono';
export interface FontChoice { name: string; kind: FontKind; stack: string }

export const FONTS: FontChoice[] = [
  { name: 'System', kind: 'sans', stack: 'system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif' },
  { name: 'Humanist', kind: 'sans', stack: 'Seravek, "Gill Sans Nova", Ubuntu, Calibri, "DejaVu Sans", sans-serif' },
  { name: 'Geometric', kind: 'sans', stack: 'Avenir, Montserrat, Corbel, "URW Gothic", sans-serif' },
  { name: 'Grotesque', kind: 'sans', stack: 'Inter, Roboto, "Helvetica Neue", "Arial Nova", "Nimbus Sans", Arial, sans-serif' },
  { name: 'Classical', kind: 'sans', stack: 'Optima, Candara, "Noto Sans", sans-serif' },
  { name: 'Rounded', kind: 'sans', stack: 'ui-rounded, "Hiragino Maru Gothic ProN", Quicksand, Comfortaa, "Arial Rounded MT Bold", Calibri, sans-serif' },
  { name: 'Condensed', kind: 'sans', stack: 'Bahnschrift, "DIN Alternate", "Franklin Gothic Medium", "Nimbus Sans Narrow", sans-serif' },
  { name: 'Old Style', kind: 'serif', stack: '"Iowan Old Style", "Palatino Linotype", "URW Palladio L", P052, serif' },
  { name: 'Transitional', kind: 'serif', stack: 'Charter, "Bitstream Charter", "Sitka Text", Cambria, serif' },
  { name: 'Didone', kind: 'serif', stack: 'Didot, "Bodoni MT", "Noto Serif Display", "URW Palladio L", P052, Sylfaen, serif' },
  { name: 'Slab', kind: 'serif', stack: 'Rockwell, "Rockwell Nova", "Roboto Slab", "DejaVu Serif", "Sitka Small", serif' },
  { name: 'Mono', kind: 'mono', stack: 'ui-monospace, "SF Mono", "Cascadia Code", "Source Code Pro", Menlo, Consolas, "DejaVu Sans Mono", monospace' },
  { name: 'Typewriter', kind: 'mono', stack: '"Nimbus Mono PS", "Courier New", Courier, monospace' },
];

const BY_NAME = new Map(FONTS.map((f) => [f.name, f]));

/** The stack a font name stands for; an unknown name gets the system font (never the name itself:
 *  only stacks from this list are ever written into a page). */
export function fontStack(name: string, role: 'heading' | 'body' | 'mono' = 'body'): string {
  return (BY_NAME.get(name) ?? BY_NAME.get(role === 'mono' ? 'Mono' : 'System')!).stack;
}
export const isFont = (name: string) => BY_NAME.has(name);
