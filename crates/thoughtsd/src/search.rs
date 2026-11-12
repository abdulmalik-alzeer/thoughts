//! Full-text search over the library's files and the chats.
//!
//! - One `fold` for every comparison: letter case, Latin accents, Arabic and Persian digits, and the
//!   Arabic spellings people type interchangeably (أ إ آ ٱ → ا, ى ی → ي, ة → ه, ؤ → و, ئ → ي, ک → ك),
//!   without tashkeel or tatweel. The UI's `fold()` (CommandPalette.svelte) follows the same rules.
//! - Every word of the query must appear somewhere in a note (its text, name or title) or a chat
//!   (its title or messages), in any order. `"quoted words"` must appear together.
//! - Results are ranked, and come with up to three short snippets whose matches are marked
//!   (`marks`: UTF-16 offsets, so the UI can slice the JavaScript string directly).
//! - File text is cached by modification time and size, and folded chat text by the chat's last
//!   change, so typing in the search box does not re-read the library on every key.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;
use walkdir::WalkDir;

/// Text files whose contents are searched. Every other file is still found by its name.
const TEXT_EXT: &[&str] = &["md", "markdown", "txt", "csv", "tsv", "json", "yaml", "yml", "toml", "log", "xml", "html", "htm", "org", "rst"];
/// Larger files are found by name only.
const TEXT_MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TERMS: usize = 8;
const MAX_TERM_CHARS: usize = 80;
/// Longest snippet, in characters.
const SNIPPET_CHARS: usize = 180;
const SNIPPETS: usize = 3;

fn latin_base(c: char) -> Option<char> {
    Some(match c {
        'À'..='Å' | 'à'..='å' | 'Ā'..='ą' => 'a',
        'Ç' | 'ç' | 'Ć'..='č' => 'c',
        'Ď' | 'ď' => 'd',
        'È'..='Ë' | 'è'..='ë' | 'Ē'..='ě' => 'e',
        'Ĝ'..='ģ' => 'g',
        'Ĥ' | 'ĥ' => 'h',
        'Ì'..='Ï' | 'ì'..='ï' | 'Ĩ'..='İ' => 'i',
        'Ĵ' | 'ĵ' => 'j',
        'Ķ' | 'ķ' => 'k',
        'Ĺ'..='ľ' => 'l',
        'Ñ' | 'ñ' | 'Ń'..='ň' => 'n',
        'Ò'..='Ö' | 'ò'..='ö' | 'Ō'..='ő' => 'o',
        'Ŕ'..='ř' => 'r',
        'Ś'..='š' => 's',
        'Ţ'..='ť' => 't',
        'Ù'..='Ü' | 'ù'..='ü' | 'Ũ'..='ų' => 'u',
        'Ŵ' | 'ŵ' => 'w',
        'Ý' | 'ý' | 'ÿ' | 'Ŷ' | 'ŷ' | 'Ÿ' => 'y',
        'Ź'..='ž' => 'z',
        _ => return None,
    })
}

/// Push the folded form of `c` onto `out` (nothing for marks that are ignored).
fn fold_into(c: char, out: &mut String) {
    let c = match c {
        // Combining accents, tashkeel, the superscript alef, Quranic signs above letters, tatweel.
        '\u{0300}'..='\u{036f}' | '\u{064b}'..='\u{065f}' | '\u{0670}' | '\u{0610}'..='\u{061a}' | '\u{0640}' => return,
        '\u{0623}' | '\u{0625}' | '\u{0622}' | '\u{0671}' => 'ا',
        '\u{0649}' | '\u{0626}' | '\u{06cc}' => 'ي',
        '\u{0629}' => 'ه',
        '\u{0624}' => 'و',
        '\u{06a9}' => 'ك',
        '\u{0660}'..='\u{0669}' => char::from(b'0' + (c as u32 - 0x0660) as u8),
        '\u{06f0}'..='\u{06f9}' => char::from(b'0' + (c as u32 - 0x06f0) as u8),
        c => match latin_base(c) { Some(b) => b, None => c },
    };
    if c.is_ascii() { out.push(c.to_ascii_lowercase()); } else { out.extend(c.to_lowercase()); }
}

/// Text folded for matching (see the module notes).
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() { fold_into(c, &mut out); }
    out
}

/// Fold `chars`, remembering for every byte of the result which character it came from.
fn fold_spans(chars: &[char]) -> (String, Vec<usize>) {
    let (mut text, mut from) = (String::new(), vec![]);
    for (i, &c) in chars.iter().enumerate() {
        fold_into(c, &mut text);
        from.resize(text.len(), i);
    }
    (text, from)
}

/// The words of a query, folded: quoted parts stay together, punctuation around a word is dropped,
/// repeats are removed. At most `MAX_TERMS`.
pub fn terms(q: &str) -> Vec<String> {
    let mut raw: Vec<String> = vec![];
    let (mut cur, mut quoted) = (String::new(), false);
    let flush = |cur: &mut String, quoted: bool, raw: &mut Vec<String>| {
        if quoted {
            let p = cur.split_whitespace().collect::<Vec<_>>().join(" ");
            if !p.is_empty() { raw.push(p); }
        } else {
            raw.extend(cur.split_whitespace().map(str::to_string));
        }
        cur.clear();
    };
    for ch in q.chars() {
        if matches!(ch, '"' | '“' | '”' | '«' | '»') { flush(&mut cur, quoted, &mut raw); quoted = !quoted; } else { cur.push(ch); }
    }
    flush(&mut cur, false, &mut raw);
    let mut out: Vec<String> = vec![];
    for w in raw {
        let trimmed = w.trim_matches(|c: char| c.is_ascii_punctuation() || "،؛؟…".contains(c));
        let f = fold(if trimmed.is_empty() { &w } else { trimmed });
        let f: String = f.chars().take(MAX_TERM_CHARS).collect();
        if !f.is_empty() && !out.contains(&f) { out.push(f); }
        if out.len() >= MAX_TERMS { break; }
    }
    out
}

/// A short piece of `line` around its first match, with every match marked.
fn snippet(line: &str, terms: &[String]) -> (String, Vec<[usize; 2]>) {
    let chars: Vec<char> = line.trim_end_matches('\r').trim().chars().collect();
    let (start, end) = if chars.len() <= SNIPPET_CHARS { (0, chars.len()) } else {
        let (folded, from) = fold_spans(&chars);
        let first = terms.iter().filter_map(|t| folded.find(t.as_str())).min().map_or(0, |b| from[b]);
        let s = first.saturating_sub(SNIPPET_CHARS / 3);
        let s = s.min(chars.len().saturating_sub(SNIPPET_CHARS));
        (s, (s + SNIPPET_CHARS).min(chars.len()))
    };
    let window = &chars[start..end];
    let (folded, from) = fold_spans(window);
    // Character ranges of every match, merged.
    let mut ranges: Vec<(usize, usize)> = vec![];
    for t in terms {
        for (b, m) in folded.match_indices(t.as_str()) { ranges.push((from[b], from[b + m.len() - 1] + 1)); }
    }
    ranges.sort();
    let mut merged: Vec<(usize, usize)> = vec![];
    for (a, b) in ranges {
        match merged.last_mut() { Some(l) if a <= l.1 => l.1 = l.1.max(b), _ => merged.push((a, b)) }
    }
    // Characters → UTF-16 offsets in the text that is sent.
    let lead = if start > 0 { "…" } else { "" };
    let mut u16_at = Vec::with_capacity(window.len() + 1);
    let mut pos = lead.encode_utf16().count();
    for c in window { u16_at.push(pos); pos += c.len_utf16(); }
    u16_at.push(pos);
    let marks = merged.into_iter().map(|(a, b)| [u16_at[a], u16_at[b]]).collect();
    let mut text: String = lead.to_string();
    text.extend(window.iter());
    if end < chars.len() { text.push('…'); }
    (text, marks)
}

/// How many of `terms` each line contains; the best `SNIPPETS` line numbers, in reading order.
fn best_lines<'a>(folded_lines: impl Iterator<Item = &'a str>, terms: &[String]) -> (Vec<usize>, bool) {
    let mut scored: Vec<(usize, usize)> = vec![];
    for (i, l) in folded_lines.enumerate() {
        let n = terms.iter().filter(|t| l.contains(t.as_str())).count();
        if n > 0 { scored.push((n, i)); }
    }
    let all_on_one = scored.iter().any(|(n, _)| *n == terms.len());
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut keep: Vec<usize> = scored.into_iter().take(SNIPPETS).map(|(_, i)| i).collect();
    keep.sort();
    (keep, all_on_one)
}

fn occurrences(hay: &str, t: &str, cap: usize) -> usize { hay.match_indices(t).take(cap).count() }

// ---------- notes ----------

struct FileEntry { mtime: Option<SystemTime>, len: u64, text: Arc<String>, folded: Arc<String>, title: Option<String> }

fn file_cache() -> &'static Mutex<HashMap<PathBuf, FileEntry>> {
    static C: OnceLock<Mutex<HashMap<PathBuf, FileEntry>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// A note's title: `title:` in its frontmatter, else its first `# ` heading.
fn title_of(text: &str) -> Option<String> {
    let clean = |s: &str| { let s = s.trim().trim_matches(['"', '\'']).trim(); (!s.is_empty()).then(|| s.chars().take(200).collect::<String>()) };
    let mut lines = text.lines();
    if text.starts_with("---") {
        lines.next();
        for l in lines.by_ref() {
            if l.trim_end() == "---" { break; }
            if let Some(v) = l.strip_prefix("title:") { if let Some(t) = clean(v) { return Some(t); } }
        }
    }
    lines.take(40).find_map(|l| l.strip_prefix("# ").and_then(clean))
}

fn is_text(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map_or(false, |e| TEXT_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

/// Notes (any library file) containing every term. `exclude` = folders left out (private folders).
pub fn notes(root: &Path, terms: &[String], limit: usize, exclude: &[String]) -> Vec<Value> {
    if terms.is_empty() { return vec![]; }
    let mut found: Vec<(i64, Option<SystemTime>, String, FileEntry)> = vec![];
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut cache = file_cache().lock().unwrap_or_else(|e| e.into_inner());
    for e in WalkDir::new(root).into_iter().filter_entry(|e| !crate::library::skip(e)).flatten() {
        if !e.file_type().is_file() { continue; }
        let Some(rel) = e.path().strip_prefix(root).ok().map(|p| p.to_string_lossy().replace('\\', "/")) else { continue };
        if crate::paths::is_under(&rel, exclude) { continue; }
        let meta = e.metadata().ok();
        let (mtime, len) = (meta.as_ref().and_then(|m| m.modified().ok()), meta.as_ref().map_or(0, |m| m.len()));
        let path = e.path().to_path_buf();
        seen.insert(path.clone());
        let fresh = cache.get(&path).map_or(false, |c| c.mtime == mtime && c.len == len);
        if !fresh {
            let text = if is_text(&path) && len <= TEXT_MAX_BYTES { std::fs::read_to_string(&path).unwrap_or_default() } else { String::new() };
            let title = title_of(&text);
            let folded = fold(&text);
            cache.insert(path.clone(), FileEntry { mtime, len, text: Arc::new(text), folded: Arc::new(folded), title });
        }
        let c = &cache[&path];
        let name = fold(&format!("{rel} {}", c.title.as_deref().unwrap_or("")));
        if !terms.iter().all(|t| name.contains(t.as_str()) || c.folded.contains(t.as_str())) { continue; }
        let mut score = 0i64;
        for t in terms {
            if name.contains(t.as_str()) { score += 40; }
            score += occurrences(&c.folded, t, 10) as i64;
        }
        if terms.len() > 1 && c.folded.contains(&terms.join(" ")) { score += 10; }
        found.push((score, mtime, rel, FileEntry { mtime, len, text: c.text.clone(), folded: c.folded.clone(), title: c.title.clone() }));
    }
    cache.retain(|p, _| seen.contains(p));
    drop(cache);
    // "All words on one line" counts toward the rank; snippets only for the notes that are returned.
    let mut ranked: Vec<(i64, Option<SystemTime>, String, FileEntry, Vec<usize>)> = found.into_iter().map(|(mut score, mtime, rel, c)| {
        let (keep, all_on_one) = best_lines(c.folded.split('\n'), terms);
        if all_on_one && terms.len() > 1 { score += 15; }
        (score, mtime, rel, c, keep)
    }).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    ranked.into_iter().take(limit).map(|(_, mtime, rel, c, keep)| {
        let lines: Vec<&str> = c.text.split('\n').collect();
        let matches: Vec<Value> = keep.iter().filter_map(|&i| lines.get(i).map(|l| {
            let (text, marks) = snippet(l, terms);
            json!({"line": i + 1, "text": text, "marks": marks})
        })).collect();
        let name = fold(&format!("{rel} {}", c.title.as_deref().unwrap_or("")));
        let name_hit = terms.iter().any(|t| name.contains(t.as_str()));
        let modified = mtime.map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
        json!({"path": rel, "title": c.title, "matches": matches, "nameHit": name_hit, "modified": modified})
    }).collect()
}

// ---------- chats ----------

struct ChatEntry { updated: chrono::DateTime<chrono::Utc>, count: usize, title: String, title_folded: String, msgs: Vec<(usize, String)> }

fn chat_cache() -> &'static Mutex<HashMap<String, ChatEntry>> {
    static C: OnceLock<Mutex<HashMap<String, ChatEntry>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// What the chat view shows as a message (in order): the person's messages and the assistant's answers.
fn shown(role: &str) -> bool { role == "user" || role == "assistant" }

/// Ordinary chats whose title or messages contain every term. Each match names the message by
/// its position among the shown messages (`msg`), so the chat view can jump to it.
pub fn chats<'a>(all: impl Iterator<Item = &'a crate::chats::Chat>, terms: &[String], limit: usize) -> Vec<Value> {
    if terms.is_empty() { return vec![]; }
    let mut cache = chat_cache().lock().unwrap_or_else(|e| e.into_inner());
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<(i64, chrono::DateTime<chrono::Utc>, Value)> = vec![];
    for c in all.filter(|c| c.kind == "chat") {
        seen.insert(c.id.clone());
        let fresh = cache.get(&c.id).map_or(false, |e| e.updated == c.updated && e.count == c.messages.len() && e.title == c.title);
        if !fresh {
            let msgs = c.messages.iter().filter(|m| shown(&m.role)).enumerate().map(|(i, m)| (i, fold(&m.text))).collect();
            cache.insert(c.id.clone(), ChatEntry { updated: c.updated, count: c.messages.len(), title: c.title.clone(), title_folded: fold(&c.title), msgs });
        }
        let e = &cache[&c.id];
        if !terms.iter().all(|t| e.title_folded.contains(t.as_str()) || e.msgs.iter().any(|(_, m)| m.contains(t.as_str()))) { continue; }
        let mut score = 0i64;
        let title_hit = terms.iter().any(|t| e.title_folded.contains(t.as_str()));
        for t in terms {
            if e.title_folded.contains(t.as_str()) { score += 40; }
            score += e.msgs.iter().filter(|(_, m)| m.contains(t.as_str())).take(10).count() as i64;
        }
        // The messages with the most of the words, then the earliest.
        let mut ranked: Vec<(usize, usize)> = e.msgs.iter()
            .map(|(i, m)| (terms.iter().filter(|t| m.contains(t.as_str())).count(), *i)).filter(|(n, _)| *n > 0).collect();
        if ranked.iter().any(|(n, _)| *n == terms.len()) && terms.len() > 1 { score += 15; }
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        ranked.truncate(SNIPPETS);
        ranked.sort_by_key(|r| r.1);
        let texts: Vec<(&str, &str)> = c.messages.iter().filter(|m| shown(&m.role)).map(|m| (m.role.as_str(), m.text.as_str())).collect();
        let matches: Vec<Value> = ranked.iter().filter_map(|&(_, i)| {
            let (role, text) = *texts.get(i)?;
            let folded = &e.msgs.iter().find(|(j, _)| *j == i)?.1;
            let (lines, _) = best_lines(folded.split('\n'), terms);
            let line = text.split('\n').nth(*lines.first()?)?;
            let (snip, marks) = snippet(line, terms);
            Some(json!({"msg": i, "role": role, "text": snip, "marks": marks}))
        }).collect();
        out.push((score, c.updated, json!({"id": c.id, "title": c.title, "projectId": c.project_id, "updated": c.updated,
            "titleHit": title_hit, "matches": matches})));
    }
    cache.retain(|id, _| seen.contains(id));
    out.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    out.into_iter().take(limit).map(|(_, _, v)| v).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chats::{Chat, Msg};

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-search-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn put(d: &Path, rel: &str, text: &str) {
        let p = d.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn paths(v: &[Value]) -> Vec<String> { v.iter().map(|x| x["path"].as_str().unwrap().to_string()).collect() }

    #[test]
    fn folding_matches_the_spellings_people_mix() {
        assert_eq!(fold("أحمد"), fold("احمد"));
        assert_eq!(fold("إسلام آمنة"), "اسلام امنه");
        assert_eq!(fold("مستشفى"), fold("مستشفي"));
        assert_eq!(fold("مدرسة"), "مدرسه");
        assert_eq!(fold("مُحَمَّد"), "محمد");
        assert_eq!(fold("جمـــيل"), "جميل");
        assert_eq!(fold("سؤال شئ"), "سوال شي");
        assert_eq!(fold("Café RÉSUMÉ"), "cafe resume");
        assert_eq!(fold("٢٠٢٦ ۱۴"), "2026 14");
        assert_eq!(fold("ی ک"), "ي ك");
        assert_eq!(fold("a\nb"), "a\nb");
    }

    #[test]
    fn queries_split_into_words_and_quoted_phrases() {
        assert_eq!(terms("  Fees   decided? "), ["fees", "decided"]);
        assert_eq!(terms("\"late fees\" client"), ["late fees", "client"]);
        assert_eq!(terms("«رسوم التأخير» العميل"), ["رسوم التاخير", "العميل"]);
        assert_eq!(terms("a a A"), ["a"]);
        assert_eq!(terms("?"), ["?"]);
        assert!(terms("   ").is_empty());
        assert_eq!(terms(&"w ".repeat(40)).len(), 1);
        assert_eq!(terms("1 2 3 4 5 6 7 8 9 10").len(), MAX_TERMS);
    }

    #[test]
    fn snippets_mark_matches_in_utf16_offsets() {
        let t = terms("احمد");
        let (text, marks) = snippet("  قال أحمد كلاماً", &t);
        assert_eq!(text, "قال أحمد كلاماً");
        let u: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(String::from_utf16(&u[marks[0][0]..marks[0][1]]).unwrap(), "أحمد");
        // Characters outside the basic plane count twice, as in JavaScript.
        let (text, marks) = snippet("😀 fee", &terms("FEE"));
        assert_eq!(marks, [[3, 6]]);
        assert_eq!(&text[5..], "fee");
        // A long line is cut around the first match.
        let long = format!("{} needle {}", "x".repeat(500), "y".repeat(500));
        let (text, marks) = snippet(&long, &terms("needle"));
        assert!(text.starts_with('…') && text.ends_with('…') && text.chars().count() <= SNIPPET_CHARS + 2);
        let u: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(String::from_utf16(&u[marks[0][0]..marks[0][1]]).unwrap(), "needle");
        // Overlapping words are one mark.
        assert_eq!(snippet("abcd", &terms("abc bcd")).1, [[0, 4]]);
    }

    #[test]
    fn notes_match_every_word_in_any_order_ranked_with_snippets() {
        let d = tmp();
        put(&d, "clients/acme.md", "---\ntitle: Acme retainer\n---\n# Acme\nWe decided the late fees are 2%.\nOther line\n");
        put(&d, "clients/beta.md", "Fees were discussed.\nNothing was decided.\n");
        put(&d, "clients/gamma.md", "only fees here\n");
        put(&d, "imports/fees-2026.pdf", "%PDF binary");
        put(&d, "data/rates.csv", "kind,fee\nlate,2%\n");
        put(&d, "ar/note.md", "اجتماع مع أحمد حول الرسوم\n");
        let r = notes(&d, &terms("decided fees"), 20, &[]);
        assert_eq!(paths(&r), ["clients/acme.md", "clients/beta.md"], "every word, any order; one line with both ranks first");
        assert_eq!(r[0]["title"], "Acme retainer");
        assert_eq!(r[0]["matches"][0]["line"], 5);
        assert_eq!(r[0]["matches"][0]["text"], "We decided the late fees are 2%.");
        assert_eq!(r[0]["matches"][0]["marks"], json!([[3, 10], [20, 24]]));
        // Names and titles count, other file types too (by name, or by text for text files).
        assert_eq!(paths(&notes(&d, &terms("retainer"), 20, &[])), ["clients/acme.md"]);
        assert!(paths(&notes(&d, &terms("fees 2026"), 20, &[])).contains(&"imports/fees-2026.pdf".to_string()));
        assert_eq!(paths(&notes(&d, &terms("late fee 2%"), 20, &[])).first().map(String::as_str), Some("clients/acme.md"));
        assert!(paths(&notes(&d, &terms("kind late"), 20, &[])).contains(&"data/rates.csv".to_string()));
        // Arabic spellings fold on both sides.
        assert_eq!(paths(&notes(&d, &terms("احمد"), 20, &[])), ["ar/note.md"]);
        // Quoted words must be together.
        assert!(notes(&d, &terms("\"fees decided\""), 20, &[]).is_empty());
        // The limit applies after ranking, not in folder order.
        assert_eq!(paths(&notes(&d, &terms("fees"), 1, &[])).len(), 1);
        // A changed file is read again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        put(&d, "clients/gamma.md", "only fees here, decided at last, and more text so the size changes\n");
        assert_eq!(notes(&d, &terms("decided fees"), 20, &[]).len(), 3);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn notes_leave_out_private_and_protected_folders() {
        let d = tmp();
        put(&d, "Me/Finances/tax.md", "salary");
        put(&d, "ideas/a.md", "salary idea");
        put(&d, ".claude/x.md", "salary");
        put(&d, ".git/config", "salary");
        let private = vec!["me/finances".to_string()];
        assert_eq!(paths(&notes(&d, &terms("salary"), 10, &private)), ["ideas/a.md"]);
        let mut all = paths(&notes(&d, &terms("salary"), 10, &[]));
        all.sort();
        assert_eq!(all, ["Me/Finances/tax.md", "ideas/a.md"]);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn chats_are_searched_inside_their_messages() {
        let mut a = Chat { title: "Pricing".into(), ..Default::default() };
        a.messages = vec![Msg::new("user", "What should the late fee be?"), Msg::new("tool", "fee lookup decided"),
            Msg::new("assistant", "Intro\nWe decided on a 2% late fee.\nMore")];
        let mut b = Chat { title: "Fees and أحمد".into(), ..Default::default() };
        b.messages = vec![Msg::new("user", "hello")];
        let wrap = Chat { kind: "wrap".into(), title: "fee wrap".into(), ..Default::default() };
        let all = [a.clone(), b.clone(), wrap];
        let r = chats(all.iter(), &terms("decided FEE"), 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["id"], a.id.as_str());
        // Tool output is not searched; message numbers count only the messages the chat view shows.
        assert_eq!(r[0]["matches"][0]["msg"], 0);
        assert_eq!(r[0]["matches"].as_array().unwrap().len(), 2);
        assert_eq!(r[0]["matches"][1]["msg"], 1);
        assert_eq!(r[0]["matches"][1]["text"], "We decided on a 2% late fee.");
        assert_eq!(r[0]["matches"][1]["role"], "assistant");
        // Titles count, with folding; wraps and other runs are left out.
        let r = chats(all.iter(), &terms("احمد"), 10);
        assert_eq!((r.len(), r[0]["titleHit"].as_bool()), (1, Some(true)));
        assert!(chats(all.iter(), &terms("wrap"), 10).is_empty());
        // A new message is picked up.
        a.messages.push(Msg::new("user", "zebra"));
        a.updated = chrono::Utc::now();
        assert_eq!(chats([a].iter(), &terms("zebra"), 10).len(), 1);
    }
}
