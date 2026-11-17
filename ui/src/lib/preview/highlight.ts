// A small syntax highlighter for the file preview: splits source text into tokens with a class
// (comment, string, keyword…). It only ever returns text and class names — the renderer puts the
// text into the page as text, never as markup — so a file can't inject anything by its contents.
// It is deliberately approximate (no grammar, a handful of rules per language family), fast enough
// for a few hundred kilobytes, and has no dependency.

export type TokClass = '' | 'com' | 'str' | 'num' | 'kw' | 'lit' | 'fn' | 'type' | 'tag' | 'attr' | 'key' | 'meta' | 'ins' | 'del';
export type Tok = { t: string; c: TokClass };

const words = (s: string) => new Set(s.split(' '));

const C_KW = words('abstract as async await break case catch class const continue default defer delete do else enum export extends extern final finally fn for from func function get go goto if impl implements import in instanceof interface internal is let loop match mod module move mut namespace new of open operator override package private protected pub public readonly ref return sealed set static struct super switch synchronized this throw throws trait try type typeof union unsafe use using val var virtual void volatile when where while with yield chan select range map fallthrough lazy guard init deinit extension protocol inout fileprivate typealias def object echo foreach elseif endif require include namespace self Self crate dyn sizeof typedef register auto signed unsigned inline constexpr template typename');
const C_LIT = words('true false null nil undefined None NaN Infinity iota');
const C_TYPE = words('int long short char float double bool boolean byte string String number any unknown never object void u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64 str Vec Option Result Box Some Ok Err Int Double Float Bool Any Unit');
const PY_KW = words('and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield match case print self cls');
const PY_LIT = words('True False None');
const RB_KW = words('alias and begin break case class def defined do else elsif end ensure for if in module next not or redo rescue retry return self super then undef unless until when while yield require require_relative attr_accessor attr_reader puts');
const SH_KW = words('if then else elif fi for while until do done case esac in function select time return exit export local readonly declare unset shift source alias echo cd set trap eval exec test FROM RUN CMD ENTRYPOINT COPY ADD ENV ARG WORKDIR EXPOSE USER VOLUME LABEL');
const LUA_KW = words('and break do else elseif end for function goto if in local not or repeat return then until while');
const SQL_KW = words('select from where and or not insert into values update set delete create table drop alter add column index view join left right inner outer full on as group by order having limit offset union all distinct case when then else end is null like in between exists primary key foreign references default unique check constraint begin commit rollback transaction with returning asc desc int integer text varchar boolean date timestamp');
const R_KW = words('if else repeat while function for in next break return library require');
const EX_KW = words('def defp defmodule do end if else unless case cond fn when import alias use require receive after try rescue catch raise quote unquote');

type Rule = [RegExp, TokClass | ((m: string, rest: string) => TokClass)];
type Spec = { rules: Rule[]; kw?: Set<string>; lit?: Set<string>; types?: Set<string>; ci?: boolean };

const NUM: Rule = [/0[xob][\da-f_]+|(?:\d[\d_]*\.?[\d_]*|\.\d[\d_]*)(?:e[+-]?\d+)?[a-z]*/iy, 'num'];
const DQ: Rule = [/"(?:[^"\\\n]|\\[\s\S])*"?/y, 'str'];
const SQ: Rule = [/'(?:[^'\\\n]|\\[\s\S])*'?/y, 'str'];
const BT: Rule = [/`(?:[^`\\]|\\[\s\S])*`?/y, 'str'];
const BLOCK: Rule = [/\/\*[\s\S]*?(?:\*\/|$)/y, 'com'];
const SLASH: Rule = [/\/\/.*/y, 'com'];
const HASH: Rule = [/#.*/y, 'com'];
const DASH: Rule = [/--.*/y, 'com'];

const cLike = (extra: Rule[] = []): Spec => ({ rules: [BLOCK, SLASH, ...extra, DQ, SQ, BT, NUM], kw: C_KW, lit: C_LIT, types: C_TYPE });
const SPECS: Record<string, Spec> = {
  c: cLike([[/#\s*\w+/y, 'meta']]),
  js: cLike(), ts: cLike(), java: cLike([[/@\w+/y, 'meta']]), kotlin: cLike([[/@\w+/y, 'meta']]), cs: cLike(), go: cLike(),
  rust: cLike([[/#!?\[[^\]\n]*\]?/y, 'meta'], [/'[a-z_]\w*(?!')/y, 'meta']]), swift: cLike([[/@\w+/y, 'meta']]), dart: cLike(), scala: cLike(), php: cLike([HASH]),
  objc: cLike([[/#\s*\w+/y, 'meta']]), zig: cLike(), proto: cLike(), gradle: cLike(), graphql: { rules: [HASH, DQ, NUM], kw: words('query mutation subscription fragment on type input enum interface union scalar schema extend directive implements'), lit: C_LIT, types: C_TYPE },
  py: { rules: [HASH, [/(?:[rbuf]{0,2})(?:"""[\s\S]*?(?:"""|$)|'''[\s\S]*?(?:'''|$))/iy, 'str'], [/@[\w.]+/y, 'meta'], DQ, SQ, NUM], kw: PY_KW, lit: PY_LIT },
  rb: { rules: [HASH, DQ, SQ, [/:[a-z_]\w*/iy, 'lit'], NUM], kw: RB_KW, lit: C_LIT },
  sh: { rules: [HASH, DQ, SQ, [/\$\{[^}\n]*\}?|\$\w+/y, 'key'], NUM], kw: SH_KW, lit: words('true false') },
  ps: { rules: [HASH, DQ, SQ, [/\$\w+/y, 'key'], NUM], kw: words('function param if else elseif foreach for while do switch return begin process end try catch finally') },
  bat: { rules: [[/(?:rem\s|::).*/iy, 'com'], DQ, [/%\w+%/y, 'key'], NUM], kw: words('echo set if else goto call exit for in do not exist'), ci: true },
  perl: { rules: [HASH, DQ, SQ, [/[$@%]\w+/y, 'key'], NUM], kw: words('my our sub if elsif else unless while for foreach return use package last next') },
  lua: { rules: [[/--\[\[[\s\S]*?(?:\]\]|$)/y, 'com'], DASH, DQ, SQ, NUM], kw: LUA_KW, lit: words('true false nil') },
  sql: { rules: [DASH, BLOCK, SQ, DQ, NUM], kw: SQL_KW, lit: words('true false null'), ci: true },
  r: { rules: [HASH, DQ, SQ, NUM], kw: R_KW, lit: words('TRUE FALSE NULL NA Inf NaN') },
  elixir: { rules: [HASH, DQ, SQ, [/:[a-z_]\w*/iy, 'lit'], NUM], kw: EX_KW, lit: words('true false nil') },
  haskell: { rules: [[/\{-[\s\S]*?(?:-\}|$)/y, 'com'], DASH, DQ, NUM], kw: words('module where import qualified data type newtype class instance deriving let in if then else case of do') },
  clojure: { rules: [[/;.*/y, 'com'], DQ, [/:[\w-]+/y, 'lit'], NUM], kw: words('def defn defmacro let fn if do when cond loop recur ns require') },
  nim: { rules: [HASH, DQ, NUM], kw: words('proc func var let const type object if elif else for while case of return import from include') },
  julia: { rules: [HASH, DQ, NUM], kw: words('function end if elseif else for while return module using import struct mutable let begin') },
  make: { rules: [HASH, [/\$[({][^)}\n]*[)}]?/y, 'key'], [/^[\w./%-]+(?=\s*:)/my, 'fn'], DQ, SQ], kw: words('ifeq ifneq ifdef ifndef else endif include define endef export') },
  tex: { rules: [[/%.*/y, 'com'], [/\\[a-z@]+/iy, 'kw'], [/\$[^$\n]*\$?/y, 'str']] },
  css: { rules: [BLOCK, [/\/\/.*/y, 'com'], DQ, SQ, [/@[\w-]+/y, 'kw'], [/#[\da-f]{3,8}\b/iy, 'num'], [/[\w-]+(?=\s*:[^:])/y, 'attr'], NUM] },
  toml: { rules: [HASH, [/^\s*\[\[?[^\]\n]*\]?\]?/my, 'tag'], [/"""[\s\S]*?(?:"""|$)/y, 'str'], DQ, SQ, [/^\s*[\w.-]+(?=\s*=)/my, 'key'], NUM], lit: words('true false') },
  ini: { rules: [[/^\s*[;#].*/my, 'com'], [/^\s*\[[^\]\n]*\]?/my, 'tag'], [/^\s*[\w.-]+(?=\s*[=:])/my, 'key'], DQ, SQ, NUM], lit: words('true false yes no on off') },
  json: { rules: [[/"(?:[^"\\\n]|\\.)*"?(?=\s*:)/y, 'key'], DQ, NUM, SLASH, BLOCK], lit: words('true false null') },
  yaml: { rules: [HASH, [/^---|^\.\.\./my, 'meta'], [/[&*][\w-]+/y, 'meta'], [/!!?[\w-]*/y, 'meta'],
    [/[^\s#:'"\-\[\]{},][^:#\n]*?(?=:(?:\s|$))/y, 'key'], DQ, SQ, NUM], lit: words('true false null yes no on off ~ True False Null') },
  md: { rules: [[/^#{1,6}\s.*/my, 'kw'], [/^```.*/my, 'meta'], [/`[^`\n]*`?/y, 'str'], [/^\s*(?:[-*+]|\d+\.)\s/my, 'meta'], [/^>.*/my, 'com']] },
  srt: { rules: [[/^\d+$/my, 'num'], [/^[\d:.,]+ --> [\d:.,]+.*/my, 'meta'], [/^WEBVTT.*/my, 'kw']] },
  diff: { rules: [[/^(?:\+\+\+|---).*/my, 'meta'], [/^@@.*/my, 'meta'], [/^\+.*/my, 'ins'], [/^-.*/my, 'del'], [/^(?:diff|index) .*/my, 'meta']] },
};
SPECS.vue = SPECS.js; // the script parts; markup is handled below for html/xml only

const IDENT = /[A-Za-z_$][\w$]*/y;
// Never across a line end, so rules anchored to a line start (`^`, `m` flag) see every line.
const OTHER = /[^\S\n]+|[\s\S]/y;

function push(out: Tok[], t: string, c: TokClass) {
  const last = out[out.length - 1];
  if (last && last.c === c) last.t += t; else out.push({ t, c });
}

function sticky(re: RegExp, s: string, at: number): string | null {
  re.lastIndex = at;
  const m = re.exec(s);
  return m && m[0].length ? m[0] : null;
}

function generic(s: string, spec: Spec): Tok[] {
  const out: Tok[] = [];
  let i = 0;
  while (i < s.length) {
    let hit = false;
    for (const [re, cls] of spec.rules) {
      // Rules anchored to a line start (`m` flag with `^`) only apply there.
      if (re.multiline && i > 0 && s[i - 1] !== '\n' && re.source.startsWith('^')) continue;
      const m = sticky(re, s, i);
      if (m) { push(out, m, typeof cls === 'function' ? cls(m, s.slice(i + m.length, i + m.length + 2)) : cls); i += m.length; hit = true; break; }
    }
    if (hit) continue;
    const id = sticky(IDENT, s, i);
    if (id) {
      const w = spec.ci ? id.toLowerCase() : id;
      let c: TokClass = '';
      if (spec.kw?.has(w)) c = 'kw';
      else if (spec.lit?.has(w)) c = 'lit';
      else if (spec.types?.has(id) || (spec.types && /^[A-Z][a-z]\w*$/.test(id))) c = 'type';
      else if (spec.types && s[i + id.length] === '(') c = 'fn';
      push(out, id, c);
      i += id.length;
      continue;
    }
    const o = sticky(OTHER, s, i)!;
    push(out, o, '');
    i += o.length;
  }
  return out;
}

const ATTR = /\s+|[^\s=>/"']+|=|"[^"]*"?|'[^']*'?|\/?>|\//y;
function markup(s: string): Tok[] {
  const out: Tok[] = [];
  let i = 0;
  while (i < s.length) {
    const lt = s.indexOf('<', i);
    const amp = s.indexOf('&', i);
    const next = Math.min(lt < 0 ? s.length : lt, amp < 0 ? s.length : amp);
    if (next > i) { push(out, s.slice(i, next), ''); i = next; continue; }
    if (s[i] === '&') {
      const e = sticky(/&#?[\w]+;/y, s, i);
      push(out, e ?? '&', e ? 'num' : ''); i += e ? e.length : 1; continue;
    }
    const com = sticky(/<!--[\s\S]*?(?:-->|$)/y, s, i);
    if (com) { push(out, com, 'com'); i += com.length; continue; }
    const cdata = sticky(/<!\[CDATA\[[\s\S]*?(?:\]\]>|$)/y, s, i);
    if (cdata) { push(out, cdata, 'str'); i += cdata.length; continue; }
    const decl = sticky(/<[!?][^>]*>?/y, s, i);
    if (decl) { push(out, decl, 'meta'); i += decl.length; continue; }
    const open = sticky(/<\/?[A-Za-z_][\w:.-]*/y, s, i);
    if (!open) { push(out, '<', ''); i++; continue; }
    push(out, open, 'tag'); i += open.length;
    // Inside the tag: attribute names, values, up to the closing `>`.
    while (i < s.length) {
      const m = sticky(ATTR, s, i);
      if (!m) break;
      i += m.length;
      if (m === '>' || m === '/>') { push(out, m, 'tag'); break; }
      push(out, m, /^\s+$|^=$|^\/$/.test(m) ? '' : /^["']/.test(m) ? 'str' : 'attr');
    }
  }
  return out;
}

/** Tokens for `text` in `lang` (kinds.ts `codeLang`); an unknown or empty language is one plain token. */
export function highlight(text: string, lang: string): Tok[] {
  if (!text) return [];
  if (lang === 'xml' || lang === 'html') return markup(text);
  const spec = SPECS[lang];
  return spec ? generic(text, spec) : [{ t: text, c: '' }];
}

/** Tokens split into lines (a token spanning lines — a block comment — is cut at each line end). */
export function lines(toks: Tok[]): Tok[][] {
  const out: Tok[][] = [[]];
  for (const tk of toks) {
    const parts = tk.t.split('\n');
    parts.forEach((p, n) => {
      if (n > 0) out.push([]);
      if (p) out[out.length - 1].push({ t: p, c: tk.c });
    });
  }
  return out;
}

export const knownLang = (lang: string) => lang === 'xml' || lang === 'html' || lang in SPECS;
