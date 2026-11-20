// The quick capture box wears the app's own look: the theme, Day / Night, language, accessibility
// and zoom the user chose (painted by `applyTheme` in lib/store, exactly as in the app window), and
// the themes added through Customise. Those live in the customisation layer's custom.css (each a
// block of tokens like `[data-theme="sand"][data-mode="dark"] { --bg: …; --accent: … }`). The box
// takes only the tokens from it — custom properties and @font-face rules, inside the @media and
// @supports blocks they sit in — never the rules that restyle the app's own pages, which could
// otherwise hide or break the box. So a custom theme's colours, accent and fonts come out the same
// as in the app, and no colour is written down twice.

/** The themes the app itself ships (theme.css). */
export const BUILT_IN = ['saudi', 'contrast', 'tech'];

/** The theme to paint: the chosen one when it exists (built in, or added through Customise),
 *  the standard one otherwise (a custom theme that was removed, or customisations that did not load). */
export function themeFor(chosen: string | undefined, custom: { id?: unknown }[] = []): string {
  const id = String(chosen ?? '');
  if (BUILT_IN.includes(id)) return id;
  if (id && custom.some((c) => c && c.id === id)) return id;
  return 'saudi';
}

type Rules = ArrayLike<CSSRule>;

function walk(rules: Rules): string {
  let out = '';
  for (const r of Array.from(rules) as any[]) {
    if (r.style && typeof r.selectorText === 'string') {
      // A style rule: only its custom properties (theme tokens), with their !important if any.
      const decls: string[] = [];
      for (const name of Array.from(r.style as CSSStyleDeclaration)) {
        if (!name.startsWith('--')) continue;
        const v = r.style.getPropertyValue(name).trim();
        if (!v) continue;
        decls.push(`${name}:${v}${r.style.getPropertyPriority(name) ? ' !important' : ''}`);
      }
      if (decls.length) out += `${r.selectorText}{${decls.join(';')}}`;
    } else if (r.constructor?.name === 'CSSFontFaceRule' || r.type === 5) {
      out += r.cssText;
    } else if (r.cssRules) {
      const inner = walk(r.cssRules);
      if (!inner) continue;
      const media = r.media?.mediaText ?? (r.type === 4 ? r.conditionText : undefined);
      if (media !== undefined && (r.type === 4 || r.constructor?.name === 'CSSMediaRule')) out += `@media ${media}{${inner}}`;
      else if (r.type === 12 || r.constructor?.name === 'CSSSupportsRule') out += `@supports ${r.conditionText}{${inner}}`;
      // Anything else that groups rules (@layer, @container…): its tokens without the condition
      // would apply where they should not, so they are left out.
    }
  }
  return out;
}

/** The theme tokens in `css` (custom.css of the customisation layer), as a style sheet of their own. */
export function themeTokens(css: string): string {
  if (!css || typeof css !== 'string') return '';
  const el = document.createElement('style');
  // Parsed for reading only: it never applies (and is removed again straight away).
  el.media = 'not all';
  el.textContent = css;
  document.head.appendChild(el);
  try {
    const sheet = el.sheet as CSSStyleSheet | null;
    return sheet ? walk(sheet.cssRules) : '';
  } catch {
    return '';
  } finally {
    el.remove();
  }
}

let tokensEl: HTMLStyleElement | null = null;

/** Put the layer's theme tokens in force (after theme.css, as custom.css is in the app window). */
export function applyTokens(css: string) {
  tokensEl ??= document.head.appendChild(Object.assign(document.createElement('style'), { id: 'layer-tokens' }));
  tokensEl.textContent = css;
}
