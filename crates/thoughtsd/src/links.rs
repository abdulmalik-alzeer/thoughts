//! Relative markdown links between library files: finding them, keeping them right when a file or
//! folder is moved or renamed in the app, and the "Linked from" index built from the same parsing.
//!
//! Only the link targets are ever rewritten: every other byte of a file (front matter, line endings,
//! the link text, anchors after `#`) stays exactly as it was.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One link destination in a markdown text: the byte range of the destination as written (inside
/// the `<…>` when it has them) and the line it is on (1-based).
#[derive(Debug, Clone, PartialEq)]
pub struct Link { pub start: usize, pub end: usize, pub angle: bool, pub line: usize }

/// Where front matter ends (0 when there is none).
fn frontmatter_end(text: &str) -> usize {
    let t = text.strip_prefix('\u{feff}').unwrap_or(text);
    let off = text.len() - t.len();
    let first = t.split_inclusive('\n').next().unwrap_or("");
    if first.trim_end_matches(['\r', '\n']) != "---" { return 0; }
    let mut pos = off + first.len();
    for l in t[first.len()..].split_inclusive('\n') {
        pos += l.len();
        let s = l.trim_end_matches(['\r', '\n']);
        if s == "---" || s == "..." { return pos; }
    }
    0 // never closed: not front matter
}

/// A fence opener/closer: up to three spaces, then three or more backticks or tildes.
fn fence(line: &str) -> Option<(char, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 { return None; }
    let rest = &line[indent..];
    let c = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = rest.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// Every inline link / image destination (`[text](dest)`, `![alt](<dest>)`) and every link
/// definition (`[id]: dest`) outside code, in order.
pub fn find_links(text: &str) -> Vec<Link> {
    let mut out = vec![];
    let mut pos = frontmatter_end(text);
    let mut line_no = text[..pos].matches('\n').count();
    let mut open_fence: Option<(char, usize)> = None;
    for raw in text[pos..].split_inclusive('\n') {
        line_no += 1;
        let line = raw.trim_end_matches(['\r', '\n']);
        let base = pos;
        pos += raw.len();
        if let Some((c, n)) = open_fence {
            if let Some((c2, n2)) = fence(line) { if c2 == c && n2 >= n && line.trim_start().trim_start_matches(c).trim().is_empty() { open_fence = None; } }
            continue;
        }
        if let Some(f) = fence(line) { open_fence = Some(f); continue; }
        // Indented code (four spaces or a tab, not inside a list) is too ambiguous to tell apart from a
        // list continuation, so it is treated as text: a link in it is still a link to a reader of the source.
        if let Some(def) = definition(line) { out.push(Link { start: base + def.0, end: base + def.1, angle: def.2, line: line_no }); continue; }
        inline_links(line, base, line_no, &mut out);
    }
    out
}

/// `[label]: dest` → (start, end, angle) of `dest` within the line.
fn definition(line: &str) -> Option<(usize, usize, bool)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 || !line[indent..].starts_with('[') || line[indent..].starts_with("[^") { return None; }
    let b = line.as_bytes();
    let mut i = indent + 1;
    while i < b.len() && b[i] != b']' { if b[i] == b'\\' { i += 1; } i += 1; }
    if i + 1 >= b.len() || b[i + 1] != b':' || i == indent + 1 { return None; }
    i += 2;
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') { i += 1; }
    if i >= b.len() { return None; }
    let (s, e, angle) = destination(line, i)?;
    // Only a title (or nothing) may follow.
    let rest = line[e + usize::from(angle)..].trim();
    if !(rest.is_empty() || rest.starts_with(['"', '\'', '('])) { return None; }
    Some((s, e, angle))
}

/// The destination starting at byte `i`: `<…>` or a run without spaces and with balanced parentheses.
/// Returns (start, end, angle); for `<…>` the range is inside the brackets.
fn destination(s: &str, i: usize) -> Option<(usize, usize, bool)> {
    let b = s.as_bytes();
    if b.get(i) == Some(&b'<') {
        let mut j = i + 1;
        while j < b.len() && b[j] != b'>' { if b[j] == b'<' { return None; } if b[j] == b'\\' { j += 1; } j += 1; }
        if j >= b.len() { return None; }
        return Some((i + 1, j, true));
    }
    let (mut j, mut depth) = (i, 0i32);
    while j < b.len() {
        match b[j] {
            b'\\' => { j += 2; continue; }
            b'(' => depth += 1,
            b')' => { if depth == 0 { break; } depth -= 1; }
            b' ' | b'\t' => break,
            c if c < 0x20 => break,
            _ => {}
        }
        j += 1;
    }
    let j = j.min(b.len());
    (j > i && depth == 0).then_some((i, j, false))
}

/// Inline links on one line, skipping code spans and escaped brackets.
fn inline_links(line: &str, base: usize, line_no: usize, out: &mut Vec<Link>) {
    let b = line.as_bytes();
    let mut i = 0;
    let mut opened = 0usize; // `[` seen and not yet closed (link text may hold brackets)
    while i < b.len() {
        match b[i] {
            b'\\' => { i += 2; continue; }
            b'`' => {
                let n = b[i..].iter().take_while(|c| **c == b'`').count();
                let ticks = &line[i..i + n];
                // The span ends at the next run of exactly as many backticks; unclosed = literal.
                let mut k = i + n;
                let mut close = None;
                while let Some(off) = line[k..].find(ticks) {
                    let at = k + off;
                    let run = b[at..].iter().take_while(|c| **c == b'`').count();
                    if run == n { close = Some(at + n); break; }
                    k = at + run;
                }
                i = close.unwrap_or(i + n);
                continue;
            }
            b'[' => opened += 1,
            b']' if opened > 0 => {
                opened -= 1;
                if b.get(i + 1) == Some(&b'(') {
                    let mut j = i + 2;
                    while j < b.len() && (b[j] == b' ' || b[j] == b'\t') { j += 1; }
                    if let Some((s, e, angle)) = destination(line, j) {
                        // After the destination: an optional title, then `)`.
                        let mut k = e + usize::from(angle);
                        while k < b.len() && (b[k] == b' ' || b[k] == b'\t') { k += 1; }
                        if k < b.len() && matches!(b[k], b'"' | b'\'' | b'(') {
                            let close = if b[k] == b'(' { b')' } else { b[k] };
                            k += 1;
                            while k < b.len() && b[k] != close { if b[k] == b'\\' { k += 1; } k += 1; }
                            k += 1;
                            while k < b.len() && (b[k] == b' ' || b[k] == b'\t') { k += 1; }
                        }
                        if k < b.len() && b[k] == b')' {
                            out.push(Link { start: base + s, end: base + e, angle, line: line_no });
                            opened = 0;
                            i = k + 1;
                            continue;
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

// ---------- targets ----------

/// A destination that points at a library file: its path part (decoded, still relative as written),
/// the `#…`/`?…` suffix as written, and how it was spelled.
#[derive(Debug)]
struct Target<'a> { path: String, suffix: &'a str, rooted: bool, dot: bool, slash: bool, encoded: bool, raw_unicode: bool }

fn target(dest: &str) -> Option<Target<'_>> {
    if dest.is_empty() || dest.starts_with('#') || dest.starts_with("//") { return None; }
    // Any scheme (`https:`, `mailto:`, `file:`, `C:`) is not a library link.
    let scheme = dest.find(':').map_or(false, |c| {
        let s = &dest[..c];
        !s.is_empty() && s.chars().next().map_or(false, |x| x.is_ascii_alphabetic()) && s.chars().all(|x| x.is_ascii_alphanumeric() || "+.-".contains(x))
    });
    if scheme { return None; }
    let cut = dest.find(['#', '?']).unwrap_or(dest.len());
    let (p, suffix) = dest.split_at(cut);
    if p.is_empty() { return None; }
    let unescaped = unescape(p);
    let path = percent_decode(&unescaped)?;
    Some(Target {
        rooted: p.starts_with('/'), dot: p.starts_with("./"), slash: p.ends_with('/') && p.len() > 1,
        encoded: has_escape(p), raw_unicode: !p.is_ascii(), path, suffix,
    })
}

/// Backslash escapes of ASCII punctuation (`\(`) → the character.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' { if let Some(n) = it.peek().copied().filter(|n| n.is_ascii_punctuation()) { out.push(n); it.next(); continue; } }
        out.push(c);
    }
    out
}

fn has_escape(s: &str) -> bool {
    let b = s.as_bytes();
    (0..b.len()).any(|i| b[i] == b'%' && i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit())
}

/// `%D8%AE` → UTF-8 text; a `%` not followed by two hex digits is kept as written. None when the
/// decoded bytes are not text.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// The library-relative path a link in `from` (a library-relative file) points at, normalised.
/// None when it climbs out of the library.
fn resolve(from: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = if path.starts_with('/') { vec![] } else { from.split('/').collect() };
    if !path.starts_with('/') { parts.pop(); }
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => { parts.pop()?; }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// Where `p` is after moving `from` to `to` (a file, or a folder with everything in it).
fn moved(p: &str, from: &str, to: &str) -> Option<String> {
    if p == from { return Some(to.to_string()); }
    p.strip_prefix(from).filter(|r| r.starts_with('/')).map(|r| format!("{to}{r}"))
}

/// The relative path from the folder of file `from` to `target` (both library-relative).
fn relative(from: &str, target: &str) -> String {
    let dir: Vec<&str> = from.split('/').collect();
    let dir = &dir[..dir.len() - 1];
    let tgt: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let common = dir.iter().zip(&tgt).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = std::iter::repeat("..").take(dir.len() - common).collect();
    parts.extend(&tgt[common..]);
    if parts.is_empty() { ".".into() } else { parts.join("/") }
}

/// Spell `path` the way the original destination was spelled: percent-encoded when it was, and
/// always so that the destination still parses (spaces outside `<…>`, `#`, `?`, `%`, `<`, `>`).
fn spell(path: &str, t: &Target, angle: bool) -> String {
    let mut out = String::with_capacity(path.len());
    let balanced = path.matches('(').count() == path.matches(')').count();
    for ch in path.chars() {
        let must = match ch {
            '%' | '#' | '?' | '<' | '>' => true,
            ' ' => !angle || t.encoded,
            '(' | ')' => !angle && !balanced,
            c if c.is_control() => true,
            c if !c.is_ascii() => t.encoded && !t.raw_unicode,
            _ => false,
        };
        if must { let mut buf = [0u8; 4]; for b in ch.encode_utf8(&mut buf).bytes() { out.push_str(&format!("%{b:02X}")); } }
        else { out.push(ch); }
    }
    out
}

/// The new destination for a link `t` in a file now at `new_src`, whose target is now at `new_t`.
fn rewrite(t: &Target, angle: bool, new_src: &str, new_t: &str) -> String {
    let mut p = if t.rooted { format!("/{new_t}") } else { relative(new_src, new_t) };
    if t.dot && !p.starts_with("..") { p = if p == "." { "./".into() } else { format!("./{p}") }; }
    if t.slash && !p.ends_with('/') { p.push('/'); }
    format!("{}{}", spell(&p, t, angle), t.suffix)
}

// ---------- the library's markdown files ----------

pub fn is_markdown(rel: &str) -> bool {
    let l = rel.to_lowercase();
    l.ends_with(".md") || l.ends_with(".markdown")
}

/// Every markdown file in the library (library-relative), with the file API's own exclusions.
fn markdown_files(root: &Path) -> Vec<(String, PathBuf)> {
    crate::library::walk_files(root).into_iter().filter(|(rel, _)| is_markdown(rel)).collect()
}

/// What moving `from` to `to` changes in the library's links, computed before the move.
#[derive(Debug, Default)]
pub struct MovePlan {
    /// (library-relative path AFTER the move, new content) for every file whose links change.
    pub writes: Vec<(String, String)>,
    /// How many links were rewritten.
    pub links: usize,
}

/// Work out every link that has to change when `from` (a file or folder) becomes `to`. Must run
/// before the move: a link is only rewritten when it pointed at something that exists. Files for
/// which `may_write(old_path)` is false are left alone (an add-on may only change its own folders).
pub fn plan_move(root: &Path, from: &str, to: &str, may_write: &dyn Fn(&str) -> bool) -> MovePlan {
    let from = from.trim_matches('/');
    let to = to.trim_matches('/');
    let mut plan = MovePlan::default();
    let mut exists: HashMap<String, bool> = HashMap::new();
    for (rel, abs) in markdown_files(root) {
        let new_src = moved(&rel, from, to).unwrap_or_else(|| rel.clone());
        if !may_write(&rel) { continue; }
        let Ok(text) = std::fs::read_to_string(&abs) else { continue };
        let mut edits: Vec<(usize, usize, String)> = vec![];
        for l in find_links(&text) {
            let dest = &text[l.start..l.end];
            let Some(t) = target(dest) else { continue };
            let Some(old_t) = resolve(&rel, &t.path) else { continue };
            if old_t.is_empty() { continue; }
            let new_t = moved(&old_t, from, to);
            if new_t.is_none() && new_src == rel { continue; }
            // Only links that work today are kept working; a broken one is left as it is.
            let ok = *exists.entry(old_t.clone()).or_insert_with(|| {
                crate::library::resolve(root, &old_t).map_or(false, |p| std::fs::symlink_metadata(p).is_ok())
            });
            if !ok { continue; }
            let new_t = new_t.unwrap_or(old_t);
            let new_dest = rewrite(&t, l.angle, &new_src, &new_t);
            if new_dest != dest { edits.push((l.start, l.end, new_dest)); }
        }
        if edits.is_empty() { continue; }
        plan.links += edits.len();
        let mut out = String::with_capacity(text.len() + 64);
        let mut at = 0;
        for (s, e, d) in &edits { out.push_str(&text[at..*s]); out.push_str(d); at = *e; }
        out.push_str(&text[at..]);
        plan.writes.push((new_src, out));
    }
    plan
}

/// Write a move plan's files (after the move). Returns how many files were changed.
pub fn apply(root: &Path, plan: &MovePlan) -> anyhow::Result<usize> {
    for (rel, content) in &plan.writes {
        let p = crate::library::resolve(root, rel)?;
        crate::paths::write_atomic(&p, content.as_bytes(), false)?;
    }
    Ok(plan.writes.len())
}

// ---------- the index line of an idea ----------

/// After an idea's status changed: the INDEX.md line that links to `readme` gets the new status
/// (and date, when the line ends with one). Everything else in the file stays byte for byte.
pub fn index_with_status(index: &str, readme: &str, status: &str, date: &str) -> Option<String> {
    const STATUSES: &[&str] = &["seed", "active", "parked", "done", "dropped"];
    let mut out = String::with_capacity(index.len());
    let mut changed = false;
    let links = find_links(index);
    let mut pos = 0;
    for raw in index.split_inclusive('\n') {
        let (start, end) = (pos, pos + raw.len());
        pos = end;
        let hit = links.iter().filter(|l| l.start >= start && l.end <= end).any(|l| {
            target(&index[l.start..l.end]).and_then(|t| resolve("INDEX.md", &t.path)).map_or(false, |p| p == readme)
        });
        if !hit || changed { out.push_str(raw); continue; }
        let eol = &raw[raw.trim_end_matches(['\r', '\n']).len()..];
        let line = &raw[..raw.len() - eol.len()];
        let mut parts: Vec<String> = line.split(" · ").map(str::to_string).collect();
        if parts.len() >= 3 && STATUSES.contains(&parts[1].trim()) {
            parts[1] = status.to_string();
            let last = parts.len() - 1;
            let l = parts[last].trim();
            if l.len() == 10 && l.as_bytes()[4] == b'-' && l.as_bytes()[7] == b'-' && l.bytes().filter(u8::is_ascii_digit).count() == 8 {
                parts[last] = parts[last].replace(l, date);
            }
            out.push_str(&parts.join(" · "));
            out.push_str(eol);
            changed = true;
        } else { out.push_str(raw); }
    }
    (changed && out != index).then_some(out)
}

// ---------- "Linked from" ----------

#[derive(Clone)]
struct Outgoing { target: String, line: usize, text: String }
struct Cached { modified: Option<std::time::SystemTime>, len: u64, title: String, links: Vec<Outgoing> }

/// Parsed links per file, reused while the file's size and time are unchanged, so a lookup over a
/// few thousand notes reads only the ones that changed since the last one.
fn cache() -> &'static Mutex<HashMap<PathBuf, Cached>> {
    static C: std::sync::OnceLock<Mutex<HashMap<PathBuf, Cached>>> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

fn parse_outgoing(rel: &str, text: &str) -> Vec<Outgoing> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<Outgoing> = vec![];
    for l in find_links(text) {
        let Some(t) = target(&text[l.start..l.end]) else { continue };
        let Some(p) = resolve(rel, &t.path) else { continue };
        if p.is_empty() || out.iter().any(|o| o.target == p && o.line == l.line) { continue; }
        let snippet: String = lines.get(l.line - 1).map_or("", |s| s.trim()).chars().take(200).collect();
        out.push(Outgoing { target: p, line: l.line, text: snippet });
    }
    out
}

/// The notes that link to `target` (a library-relative file; a folder's README also counts links to
/// the folder itself): `[{path, title, line, text}]`, one per linking note, in path order. Notes
/// under `exclude` are left out, exactly as the file list and search leave them out.
pub fn backlinks(root: &Path, target: &str, exclude: &[String]) -> Vec<Value> {
    let target = target.trim_matches('/');
    // Links to a folder open its README, so they count as links to the README.
    let folder = match target.rsplit_once('/') { Some((d, f)) if f.eq_ignore_ascii_case("README.md") => Some(d.to_string()), _ => None };
    let files = markdown_files(root);
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if c.len() > 50_000 { c.clear(); }
    let mut out = vec![];
    // Private folders by where they really are as well as by name, as in the file list and search.
    let hidden = crate::library::Hidden::new(root, exclude);
    let mut hidden_dirs: HashMap<PathBuf, bool> = HashMap::new();
    for (rel, abs) in files {
        if crate::paths::is_under(&rel, exclude) || rel == target { continue; }
        if !exclude.is_empty() && abs.parent().map_or(false, |p| *hidden_dirs.entry(p.to_path_buf()).or_insert_with(|| hidden.dir(p))) { continue; }
        let Ok(meta) = std::fs::metadata(&abs) else { continue };
        let (modified, len) = (meta.modified().ok(), meta.len());
        let fresh = c.get(&abs).map_or(false, |e| e.modified == modified && e.len == len && modified.is_some());
        if !fresh {
            let Ok(text) = std::fs::read_to_string(&abs) else { continue };
            let title = crate::library::frontmatter(&text).0.get("title").and_then(Value::as_str).unwrap_or("").to_string();
            c.insert(abs.clone(), Cached { modified, len, title, links: parse_outgoing(&rel, &text) });
        }
        let e = &c[&abs];
        let to_it = |o: &&Outgoing| o.target == target || folder.as_deref() == Some(o.target.as_str());
        if let Some(o) = e.links.iter().find(to_it) {
            let count = e.links.iter().filter(to_it).count();
            out.push(json!({"path": rel, "title": e.title, "line": o.line, "text": o.text, "count": count}));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-links-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn get(root: &Path, rel: &str) -> String { std::fs::read_to_string(root.join(rel)).unwrap() }
    /// Plan, move, apply — what the API does.
    fn mv(root: &Path, from: &str, to: &str) -> usize {
        let plan = plan_move(root, from, to, &|_| true);
        crate::library::rename(root, from, to).unwrap();
        apply(root, &plan).unwrap();
        plan.links
    }
    fn dests(text: &str) -> Vec<&str> { find_links(text).iter().map(|l| &text[l.start..l.end]).collect() }

    #[test]
    fn links_are_found_outside_code_and_front_matter() {
        let t = "---\nrelated: [x](a.md)\n---\n[one](a.md) and ![pic](img/p.png \"t\") `[no](b.md)`\n\n```\n[no](c.md)\n```\n[def]: <my note.md>\n[two](<x y.md#part>) [three](plan%20(2).md) \\[no](d.md) [](e.md)\n~~~md\n[no](f.md)\n~~~\n[four](g.md)";
        assert_eq!(dests(t), vec!["a.md", "img/p.png", "my note.md", "x y.md#part", "plan%20(2).md", "e.md", "g.md"]);
        // Not links: a space in a plain destination, an unclosed bracket, a footnote definition.
        assert!(dests("[a](my note.md) [b](c.md\n[^1]: d.md").is_empty());
    }

    #[test]
    fn a_moved_file_keeps_its_links_from_siblings_and_from_deeper_folders() {
        let r = tmp();
        put(&r, "apps/one/README.md", "---\ntitle: One\n---\n# One\nSee [notes](notes/plan.md#costs) and [the other](../two/README.md).\n");
        put(&r, "apps/one/notes/plan.md", "# Plan\nBack to [one](../README.md), [sib](other.md).\n");
        put(&r, "apps/one/notes/other.md", "[plan](plan.md) [plan again](./plan.md) [not it](plan.md.bak) [nor](plans.md) [root](/apps/one/notes/plan.md)\n");
        put(&r, "apps/two/README.md", "[deep](../one/notes/plan.md)\n");
        put(&r, "INDEX.md", "# Index\n- [Plan](apps/one/notes/plan.md) · seed · x · 2026-01-01\n");
        let n = mv(&r, "apps/one/notes/plan.md", "apps/two/notes/budget.md");
        assert_eq!(get(&r, "apps/one/README.md"), "---\ntitle: One\n---\n# One\nSee [notes](../two/notes/budget.md#costs) and [the other](../two/README.md).\n");
        assert_eq!(get(&r, "apps/one/notes/other.md"), "[plan](../../two/notes/budget.md) [plan again](../../two/notes/budget.md) [not it](plan.md.bak) [nor](plans.md) [root](/apps/two/notes/budget.md)\n");
        assert_eq!(get(&r, "apps/two/README.md"), "[deep](notes/budget.md)\n");
        assert_eq!(get(&r, "INDEX.md"), "# Index\n- [Plan](apps/two/notes/budget.md) · seed · x · 2026-01-01\n");
        // The moved file's own links follow its new place.
        assert_eq!(get(&r, "apps/two/notes/budget.md"), "# Plan\nBack to [one](../../one/README.md), [sib](../../one/notes/other.md).\n");
        assert_eq!(n, 1 + 3 + 1 + 1 + 2);
    }

    #[test]
    fn a_moved_folder_keeps_inside_links_and_fixes_outside_ones() {
        let r = tmp();
        put(&r, "apps/one/README.md", "[n](notes/a.md) [up](../../INDEX.md) [sib](../two/README.md)\n");
        put(&r, "apps/one/notes/a.md", "[home](../README.md)\n");
        put(&r, "apps/two/README.md", "[one](../one/README.md) [folder](../one/) [lookalike](../one-more/README.md) [bare](../one)\n");
        put(&r, "apps/one-more/README.md", "x\n");
        put(&r, "INDEX.md", "- [One](apps/one/README.md) · active · pitch · 2026-02-02\n- [More](apps/one-more/README.md) · seed · x · 2026-01-01\n");
        let n = mv(&r, "apps/one", "business/uno");
        assert_eq!(get(&r, "business/uno/README.md"), "[n](notes/a.md) [up](../../INDEX.md) [sib](../../apps/two/README.md)\n");
        assert_eq!(get(&r, "business/uno/notes/a.md"), "[home](../README.md)\n");
        assert_eq!(get(&r, "apps/two/README.md"), "[one](../../business/uno/README.md) [folder](../../business/uno/) [lookalike](../one-more/README.md) [bare](../../business/uno)\n");
        assert_eq!(get(&r, "INDEX.md"), "- [One](business/uno/README.md) · active · pitch · 2026-02-02\n- [More](apps/one-more/README.md) · seed · x · 2026-01-01\n");
        assert_eq!(n, 1 + 3 + 1);
    }

    #[test]
    fn arabic_and_spaced_names_keep_their_spelling() {
        let r = tmp();
        put(&r, "ملاحظات/خطة.md", "x\n");
        put(&r, "a b/c.md", "x\n");
        put(&r, "index.md", "[plain](ملاحظات/خطة.md) [enc](%D9%85%D9%84%D8%A7%D8%AD%D8%B8%D8%A7%D8%AA/%D8%AE%D8%B7%D8%A9.md) [angle](<a b/c.md>) [pct](a%20b/c.md#x)\n");
        mv(&r, "ملاحظات/خطة.md", "أفكار/خطة جديدة.md");
        mv(&r, "a b", "x y");
        assert_eq!(get(&r, "index.md"), "[plain](أفكار/خطة%20جديدة.md) [enc](%D8%A3%D9%81%D9%83%D8%A7%D8%B1/%D8%AE%D8%B7%D8%A9%20%D8%AC%D8%AF%D9%8A%D8%AF%D8%A9.md) [angle](<x y/c.md>) [pct](x%20y/c.md#x)\n");
    }

    #[test]
    fn windows_line_endings_and_everything_else_stay_byte_for_byte() {
        let r = tmp();
        put(&r, "a.md", "x\r\n");
        let before = "---\r\ntitle: \"T\"\r\nlink: a.md\r\n---\r\n\u{200f}نص [a](a.md \"title\")  \r\n\r\n```\r\n[a](a.md)\r\n```\r\n[ref]: a.md 'x'\r\n";
        put(&r, "b.md", before);
        assert_eq!(mv(&r, "a.md", "n/a.md"), 2);
        assert_eq!(get(&r, "b.md"), before.replacen("[a](a.md \"title\")", "[a](n/a.md \"title\")", 1).replacen("[ref]: a.md", "[ref]: n/a.md", 1));
    }

    #[test]
    fn broken_external_and_untouched_links_are_left_alone() {
        let r = tmp();
        put(&r, "a.md", "x");
        put(&r, "b.md", "[gone](missing.md) [web](https://x.y/a.md) [mail](mailto:a@b.c) [anchor](#a) [up](../../out.md) [a](a.md)");
        let plan = plan_move(&r, "zzz.md", "q.md", &|_| true);
        assert!(plan.writes.is_empty() && plan.links == 0);
        let plan = plan_move(&r, "a.md", "c/a.md", &|p| p != "b.md");
        assert!(plan.writes.is_empty(), "files the mover may not write are left alone");
    }

    #[test]
    fn the_index_line_follows_a_status_change() {
        let idx = "# Index\r\n\r\n## apps\r\n- [One](apps/one/README.md) · seed · pitch · with · dots · 2026-01-01\r\n- [Two](apps/two/README.md) · seed · p · 2026-01-01\r\n";
        let out = index_with_status(idx, "apps/one/README.md", "active", "2026-11-12").unwrap();
        assert_eq!(out, "# Index\r\n\r\n## apps\r\n- [One](apps/one/README.md) · active · pitch · with · dots · 2026-11-12\r\n- [Two](apps/two/README.md) · seed · p · 2026-01-01\r\n");
        assert_eq!(index_with_status(idx, "apps/three/README.md", "done", "2026-11-12"), None);
    }

    #[test]
    fn backlinks_list_linking_notes_and_respect_exclusions() {
        let r = tmp();
        put(&r, "apps/one/README.md", "# One");
        put(&r, "apps/two/README.md", "---\ntitle: Two\n---\nSee [one](../one/README.md) and [again](../one/)\n");
        put(&r, "me/finances/x.md", "[one](../../apps/one/README.md)");
        put(&r, "INDEX.md", "- [One](apps/one/README.md)");
        put(&r, "apps/one/notes/n.md", "[self-ish](../README.md#top) `[code](../README.md)`");
        let paths = |v: Vec<Value>| v.iter().map(|x| x["path"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let mut all = paths(backlinks(&r, "apps/one/README.md", &[]));
        all.sort();
        assert_eq!(all, vec!["INDEX.md", "apps/one/notes/n.md", "apps/two/README.md", "me/finances/x.md"]);
        let b = backlinks(&r, "apps/one/README.md", &["me/finances".into()]);
        assert!(!paths(b.clone()).contains(&"me/finances/x.md".to_string()));
        let two = b.iter().find(|x| x["path"] == "apps/two/README.md").unwrap();
        assert_eq!((two["title"].as_str(), two["count"].as_u64(), two["line"].as_u64()), (Some("Two"), Some(2), Some(4)));
        // Edited afterwards: the cache notices.
        std::thread::sleep(std::time::Duration::from_millis(20));
        put(&r, "apps/two/README.md", "nothing here any more, longer text");
        assert!(!paths(backlinks(&r, "apps/one/README.md", &[])).contains(&"apps/two/README.md".to_string()));
    }

    /// SEC-32: a private folder named through a link to it is left out just the same.
    #[cfg(unix)]
    #[test]
    fn backlinks_leave_out_a_private_folder_under_another_name() {
        let r = tmp();
        put(&r, "apps/one/README.md", "# One");
        put(&r, "me/finances/x.md", "[one](../../apps/one/README.md) salary");
        put(&r, "ideas/a.md", "[one](../apps/one/README.md)");
        std::os::unix::fs::symlink("../me/finances", r.join("ideas/money")).unwrap();
        let paths = |v: Vec<Value>| v.iter().map(|x| x["path"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(paths(backlinks(&r, "apps/one/README.md", &["ideas/money".into()])), ["ideas/a.md"]);
        std::fs::remove_dir_all(r).unwrap();
    }

    #[test]
    fn backlinks_stay_quick_on_a_few_thousand_notes() {
        let r = tmp();
        for i in 0..3000 { put(&r, &format!("area{}/n{i}.md", i % 30), &format!("# Note {i}\n\nSome text, then [a link](../hub.md) and [another](n{}.md).\n", i + 1)); }
        put(&r, "hub.md", "# Hub");
        let t = std::time::Instant::now();
        assert_eq!(backlinks(&r, "hub.md", &[]).len(), 3000);
        let first = t.elapsed();
        let t = std::time::Instant::now();
        assert_eq!(backlinks(&r, "hub.md", &[]).len(), 3000);
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "cached lookup took {:?} (first {first:?})", t.elapsed());
    }
}
