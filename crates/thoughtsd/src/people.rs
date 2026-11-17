//! The People list: the people the user writes to, who they are and how to write to them, kept as
//! plain Markdown in the library so the assistant can draft well (the "Email writing" routine reads
//! it). Not an item and not in INDEX, like `me/`.
//!
//! `people/<slug>.md` per person — frontmatter `name, email, role, organisation, relationship,
//! language, tone, updated, summary`, then `# Name` and the user's own notes — and `people/README.md`,
//! the front page, with one line per person: `- [Name](slug.md) · role, organisation · email`.
//!
//! What the app keeps when it saves: every frontmatter key it does not know, line for line, after the
//! ones it writes; the notes the user wrote (the body under the heading). The front page's list is
//! made again from the files on every save; whatever the user wrote above and below the list stays.
//! The app never reads or writes through a link, and a person's file name never changes once made
//! (renaming someone keeps their file, so links to it keep working).
//!
//! A private `people/` folder: the user still edits it here (it is their own app; private folders
//! close the assistant and add-ons out). The list says so, because the assistant then cannot read it.

use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const DIR: &str = "people";
const FRONT: &str = "README.md";

const MAX_NAME: usize = 120;
const MAX_FIELD: usize = 200;
const MAX_SUMMARY: usize = 300;
const MAX_NOTES: usize = 20_000;
/// A person's file or the front page larger than this is not read (and never rewritten).
const MAX_FILE: u64 = 256 * 1024;
/// At most this many people are listed; a new one beyond it is refused.
const MAX_PEOPLE: usize = 2000;
/// The details the app writes, in this order.
const KEYS: [&str; 9] = ["name", "email", "role", "organisation", "relationship", "language", "tone", "updated", "summary"];

/// What the user sees and edits for one person.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Person {
    pub name: String,
    pub email: String,
    pub role: String,
    pub organisation: String,
    /// How the user knows them: client, colleague, supplier, friend…
    pub relationship: String,
    /// The language to write to them in.
    pub language: String,
    /// formal, friendly…
    pub tone: String,
    /// One line: who they are. Empty: made from role, organisation and relationship.
    pub summary: String,
    /// The user's own notes (the body under the heading), Markdown.
    pub notes: String,
}

fn one_line(s: &str) -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") }

fn err(key: &str) -> anyhow::Error { crate::tr::err(key, &[]) }

// ---------- validation ----------

/// One plausible email address (or nothing): a single `@`, a sensible part before it, a domain with
/// a dot and a two-letter-or-longer ending; no spaces and none of the characters that would break an
/// "Open in Mail" link.
pub fn plausible_email(s: &str) -> bool {
    if s.is_empty() { return true; }
    if s.len() > 254 || s.matches('@').count() != 1 { return false; }
    let (local, domain) = s.split_once('@').unwrap_or_default();
    let local_ok = !local.is_empty() && local.len() <= 64 && !local.starts_with('.') && !local.ends_with('.') && !local.contains("..")
        && local.chars().all(|c| c.is_alphanumeric() || ".+-_'".contains(c));
    let labels: Vec<&str> = domain.split('.').collect();
    let domain_ok = labels.len() >= 2 && labels.iter().all(|l| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-')
        && l.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && labels.last().map_or(false, |t| t.chars().count() >= 2 && t.chars().all(char::is_alphabetic));
    local_ok && domain_ok
}

/// The person, tidied (one-line details, trimmed notes), if they can be saved as they are.
pub fn check(p: &Person) -> anyhow::Result<Person> {
    // Hidden characters are refused, not quietly dropped: line breaks in a one-line detail are only spaces.
    let hidden = |s: &str| s.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r'));
    let fields = [&p.name, &p.email, &p.role, &p.organisation, &p.relationship, &p.language, &p.tone, &p.summary, &p.notes];
    if fields.iter().any(|f| hidden(f)) { return Err(err("people.unsafe")); }
    crate::routines::no_hidden(&fields.map(|f| f.as_str()))?;
    let out = Person {
        name: one_line(&p.name), email: one_line(&p.email), role: one_line(&p.role), organisation: one_line(&p.organisation),
        relationship: one_line(&p.relationship), language: one_line(&p.language), tone: one_line(&p.tone), summary: one_line(&p.summary),
        notes: p.notes.replace("\r\n", "\n").replace('\r', "\n").trim_matches('\n').trim_end().to_string(),
    };
    if out.name.is_empty() || out.name.chars().count() > MAX_NAME { return Err(err("people.name")); }
    if !plausible_email(&out.email) { return Err(err("people.email")); }
    let long = [&out.role, &out.organisation, &out.relationship, &out.language, &out.tone].iter().any(|f| f.chars().count() > MAX_FIELD)
        || out.summary.chars().count() > MAX_SUMMARY || out.notes.len() > MAX_NOTES;
    if long { return Err(err("people.too_long")); }
    Ok(out)
}

/// "Finance manager at Najd Trading; client." — the line made when the user wrote none.
pub fn auto_summary(p: &Person, arabic: bool) -> String {
    let (at, semi) = if arabic { (" في ", "؛ ") } else { (" at ", "; ") };
    let who = match (p.role.is_empty(), p.organisation.is_empty()) {
        (false, false) => format!("{}{at}{}", p.role, p.organisation),
        (false, true) => p.role.clone(),
        (true, false) => p.organisation.clone(),
        (true, true) => String::new(),
    };
    let line = match (who.is_empty(), p.relationship.is_empty()) {
        (false, false) => format!("{who}{semi}{}", p.relationship),
        (false, true) => who,
        (true, false) => p.relationship.clone(),
        (true, true) => return String::new(),
    };
    format!("{line}.")
}

// ---------- file names ----------

/// Arabic (and Persian) letters written in Latin letters, so an Arabic name gets a readable file name
/// («سارة القحطاني» → `sara-alqhtany`).
fn latin(c: char) -> Option<&'static str> {
    Some(match c {
        'ا' | 'أ' | 'إ' | 'آ' | 'ٱ' | 'ى' | 'ة' | 'ع' => "a",
        'ب' => "b", 'ت' => "t", 'ث' => "th", 'ج' => "j", 'ح' => "h", 'خ' => "kh", 'د' => "d", 'ذ' => "dh",
        'ر' => "r", 'ز' => "z", 'س' => "s", 'ش' => "sh", 'ص' => "s", 'ض' => "d", 'ط' => "t", 'ظ' => "z",
        'غ' => "gh", 'ف' => "f", 'ق' => "q", 'ك' | 'ک' => "k", 'ل' => "l", 'م' => "m", 'ن' => "n", 'ه' => "h",
        'و' => "w", 'ي' | 'ی' => "y", 'پ' => "p", 'چ' => "ch", 'گ' => "g", 'ژ' => "zh",
        'ء' | 'ؤ' | 'ئ' => "",
        '٠' => "0", '١' => "1", '٢' => "2", '٣' => "3", '٤' => "4", '٥' => "5", '٦' => "6", '٧' => "7", '٨' => "8", '٩' => "9",
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => "a", 'é' | 'è' | 'ê' | 'ë' | 'ē' => "e", 'í' | 'ì' | 'î' | 'ï' | 'ī' => "i",
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' => "o", 'ú' | 'ù' | 'û' | 'ü' | 'ū' => "u", 'ç' => "c", 'ñ' => "n", 'ß' => "ss",
        'ş' | 'š' => "s", 'ğ' => "g", 'ı' => "i", 'ž' => "z", 'č' => "c",
        _ => return None,
    })
}

/// The file name a name gives, before it is made unique: lowercase Latin letters, digits and dashes;
/// `person` when nothing is left.
pub fn base_slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() { s.push(c); continue; }
        // Arabic short vowels and the stretching line are not letters of the name.
        if ('\u{064B}'..='\u{065F}').contains(&c) || c == '\u{0670}' || c == 'ـ' { continue; }
        match latin(c) {
            Some(l) => s.push_str(l),
            None => if !s.is_empty() && !s.ends_with('-') { s.push('-') },
        }
    }
    let s: String = s.trim_matches('-').chars().take(60).collect::<String>().trim_end_matches('-').to_string();
    if s.is_empty() || s == "readme" { "person".into() } else { s }
}

/// A file name (without `.md`) that is safe to open: no folders, no hidden files, not the front page.
pub fn valid_slug(s: &str) -> bool {
    !s.is_empty() && s.len() <= 120 && !s.starts_with('.') && !s.eq_ignore_ascii_case("readme")
        && !s.chars().any(|c| c.is_control() || "/\\:".contains(c))
}

/// The lowercase names already in `people/` (on a Mac `Sara.md` and `sara.md` are the same file).
fn taken_names(dir: &Path) -> std::collections::HashSet<String> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect()
}

/// A free file name for a new person.
pub fn new_slug(dir: &Path, name: &str) -> String {
    let base = base_slug(name);
    let taken = taken_names(dir);
    let free = |s: &str| !taken.contains(&format!("{s}.md"));
    if free(&base) { return base; }
    (2..).map(|n| format!("{base}-{n}")).find(|s| free(s)).unwrap_or(base)
}

// ---------- reading and writing one file ----------

/// The frontmatter block as written and the body after it (no block: `None` and the whole text).
fn split(text: &str) -> (Option<&str>, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) {
        if let Some(end) = rest.find("\n---") {
            return (Some(&rest[..end]), rest[end + 4..].trim_start_matches(['\r', '\n']));
        }
    }
    (None, text)
}

/// The frontmatter's own lines for every key the app does not write, in their order and exactly as
/// they were (a key's indented or listed lines stay with it; comments stay where they are).
fn other_lines(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut keep = true;
    for line in yaml.lines() {
        let starts_key = !line.is_empty() && !line.starts_with([' ', '\t', '-', '#']) && line.contains(':');
        if starts_key {
            let key = line.split(':').next().unwrap_or("").trim().trim_matches(['"', '\'']);
            keep = !KEYS.contains(&key);
        } else if line.starts_with('#') {
            keep = true;
        }
        if keep && !(line.trim().is_empty() && out.is_empty()) { out.push(line.trim_end_matches('\r').to_string()); }
    }
    while out.last().map_or(false, |l: &String| l.trim().is_empty()) { out.pop(); }
    out
}

/// A detail's value as text, whatever YAML made of it.
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => one_line(s),
        Value::Null => String::new(),
        Value::Array(a) => a.iter().map(text_of).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "),
        Value::Object(_) => String::new(),
        other => other.to_string(),
    }
}

/// A value written plainly when YAML reads it back as exactly this text (`Sara Al-Qahtani`), quoted otherwise.
fn scalar(s: &str) -> String {
    if s.is_empty() { return String::new(); }
    let plain = !s.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c)) && !s.contains(": ") && !s.contains(" #") && !s.ends_with(':')
        && yaml_serde::from_str::<yaml_serde::Value>(&format!("k: {s}")).ok().and_then(|y| serde_json::to_value(y).ok())
            .map_or(false, |v| v["k"].as_str() == Some(s));
    if plain { format!(" {s}") } else { format!(" {}", serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())) }
}

/// The notes under the heading (the heading itself is the name and is written by the app).
fn notes_of(body: &str) -> String {
    let body = body.replace("\r\n", "\n");
    let body = body.trim_start_matches('\n');
    let rest = match body.strip_prefix("# ") { Some(r) => r.split_once('\n').map_or("", |(_, s)| s), None => body };
    rest.trim_matches('\n').trim_end().to_string()
}

/// Read a person's file: what the user edits, when it was last updated, and the details the app does
/// not show (kept as they are on save).
pub fn parse(slug: &str, text: &str) -> (Person, String, Vec<String>) {
    let (_, body) = split(text);
    let fm = crate::library::note_details(text).map(|(v, _)| v).unwrap_or(Value::Null);
    let get = |k: &str| text_of(&fm[k]);
    let mut name = get("name");
    if name.is_empty() {
        name = body.trim_start().strip_prefix("# ").and_then(|r| r.lines().next()).map(one_line).unwrap_or_else(|| slug.replace('-', " "));
    }
    let p = Person {
        name, email: get("email"), role: get("role"), organisation: get("organisation"), relationship: get("relationship"),
        language: get("language"), tone: get("tone"), summary: get("summary"), notes: notes_of(body),
    };
    let mut extra: Vec<String> = fm.as_object().map(|o| o.keys().filter(|k| !KEYS.contains(&k.as_str())).cloned().collect()).unwrap_or_default();
    extra.sort();
    (p, get("updated"), extra)
}

/// The file for a person: the app's details in a fixed order, then every other detail the file
/// already had (`old`), then the name as a heading and the notes.
pub fn render(p: &Person, updated: &str, old: Option<&str>) -> String {
    let mut s = String::from("---\n");
    let values = [&p.name, &p.email, &p.role, &p.organisation, &p.relationship, &p.language, &p.tone, &updated.to_string(), &p.summary];
    for (k, v) in KEYS.iter().zip(values) { s.push_str(&format!("{k}:{}\n", scalar(v))); }
    if let Some(yaml) = old.and_then(|t| split(t).0) {
        for l in other_lines(yaml) { s.push_str(&l); s.push('\n'); }
    }
    s.push_str("---\n\n# ");
    s.push_str(&p.name);
    s.push('\n');
    if !p.notes.is_empty() { s.push('\n'); s.push_str(&p.notes); s.push('\n'); }
    s
}

// ---------- the folder ----------

fn is_link(p: &Path) -> bool { std::fs::symlink_metadata(p).map_or(false, |m| m.file_type().is_symlink()) }

/// A plain regular file (never a link), small enough: its bytes.
fn read_plain(p: &Path) -> Option<Vec<u8>> {
    let m = std::fs::symlink_metadata(p).ok()?;
    if !m.is_file() || m.len() > MAX_FILE { return None; }
    std::fs::read(p).ok()
}

/// `people/` as a real folder (not a link), if there is one.
fn folder(lib: &Path) -> Option<PathBuf> {
    let d = lib.join(DIR);
    std::fs::symlink_metadata(&d).ok().filter(|m| m.is_dir()).map(|_| d)
}

/// One listed person.
#[derive(Clone, Debug)]
pub struct Entry { pub slug: String, pub person: Person, pub updated: String, pub extra: Vec<String>, pub hash: String }

impl Entry {
    pub fn to_json(&self) -> Value {
        let p = &self.person;
        json!({
            "slug": self.slug, "name": p.name, "email": p.email, "role": p.role, "organisation": p.organisation,
            "relationship": p.relationship, "language": p.language, "tone": p.tone, "summary": p.summary,
            "notes": p.notes, "updated": self.updated, "extra": self.extra, "hash": self.hash,
        })
    }
}

/// Everyone in `people/`: one plain `.md` file each besides the front page, sorted by name. Links,
/// folders, hidden files and files too large are left out.
pub fn list(lib: &Path) -> Vec<Entry> {
    let Some(dir) = folder(lib) else { return vec![] };
    let mut slugs: Vec<String> = std::fs::read_dir(&dir).into_iter().flatten().flatten()
        .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".md")).map(str::to_string))
        .filter(|s| valid_slug(s)).collect();
    slugs.sort();
    let mut out: Vec<Entry> = slugs.into_iter().take(MAX_PEOPLE).filter_map(|slug| {
        let bytes = read_plain(&dir.join(format!("{slug}.md")))?;
        let (person, updated, extra) = parse(&slug, &String::from_utf8_lossy(&bytes));
        Some(Entry { slug, person, updated, extra, hash: crate::workflows::content_hash(&bytes) })
    }).collect();
    out.sort_by(|a, b| a.person.name.to_lowercase().cmp(&b.person.name.to_lowercase()).then_with(|| a.slug.cmp(&b.slug)));
    out
}

/// The front page's line for one person.
pub fn line(e: &Entry) -> String {
    let p = &e.person;
    let name = p.name.replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]");
    let who = [&p.role, &p.organisation].iter().filter(|s| !s.is_empty()).map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
    let mut parts = vec![format!("- [{name}]({}.md)", e.slug)];
    if !who.is_empty() { parts.push(who); }
    if !p.email.is_empty() { parts.push(p.email.clone()); }
    parts.join(" · ")
}

fn is_person_line(l: &str) -> bool {
    let t = l.trim_start();
    (t.starts_with("- [") || t.starts_with("* [")) && t.contains("](")
}

/// The front page with its list made from `people`: the first run of list lines (blank lines between
/// them included) is replaced; everything above and below it stays as the user wrote it. No list yet:
/// it goes at the end. No front page yet: a new one with a heading and one line saying what it is.
pub fn front_page(old: Option<&str>, people: &[Entry], arabic: bool) -> String {
    let list: Vec<String> = people.iter().map(line).collect();
    let Some(old) = old else {
        let intro = if arabic { "الأشخاص الذين تراسلهم: لكل منهم صفحة هنا يقرؤها المساعد حين يكتب إليه." }
            else { "The people you write to. Each has a page here that the assistant reads when it writes to them." };
        let mut s = format!("# People\n\n{intro}\n");
        if !list.is_empty() { s.push('\n'); s.push_str(&list.join("\n")); s.push('\n'); }
        return s;
    };
    let lines: Vec<&str> = old.lines().collect();
    let Some(start) = lines.iter().position(|l| is_person_line(l)) else {
        if list.is_empty() { return old.to_string(); }
        let mut s = old.trim_end().to_string();
        if !s.is_empty() { s.push_str("\n\n"); }
        s.push_str(&list.join("\n"));
        s.push('\n');
        return s;
    };
    let mut end = start;
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if is_person_line(l) { end = i; } else if !l.trim().is_empty() { break; }
    }
    let mut out: Vec<String> = lines[..start].iter().map(|s| s.to_string()).collect();
    if list.is_empty() {
        // Nothing left to list: do not leave two blank lines where the list was.
        while out.last().map_or(false, |l| l.trim().is_empty()) && lines.get(end + 1).map_or(true, |l| l.trim().is_empty()) { out.pop(); }
    } else {
        out.extend(list);
    }
    out.extend(lines[end + 1..].iter().map(|s| s.to_string()));
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Neither `people/`, the front page nor the person's file may be a link, nor anything but a file.
fn no_links(lib: &Path, slug: Option<&str>) -> anyhow::Result<()> {
    let dir = lib.join(DIR);
    let mut check = vec![dir.clone(), dir.join(FRONT)];
    if let Some(s) = slug { check.push(dir.join(format!("{s}.md"))); }
    if check.iter().any(|p| is_link(p)) { return Err(err("people.link")); }
    if std::fs::symlink_metadata(&dir).map_or(false, |m| !m.is_dir()) { return Err(err("people.link")); }
    if std::fs::symlink_metadata(dir.join(FRONT)).map_or(false, |m| !m.is_file() || m.len() > MAX_FILE) { return Err(err("people.link")); }
    Ok(())
}

/// Write the front page again from the files, if it changed.
fn write_front(lib: &Path, arabic: bool) -> anyhow::Result<()> {
    let dir = lib.join(DIR);
    let path = dir.join(FRONT);
    let old = read_plain(&path).map(|b| String::from_utf8_lossy(&b).to_string());
    let text = front_page(old.as_deref(), &list(lib), arabic);
    if old.as_deref() != Some(text.as_str()) { crate::paths::write_atomic(&path, text.as_bytes(), false)?; }
    Ok(())
}

fn today() -> String { chrono::Local::now().format("%Y-%m-%d").to_string() }

/// Keep a person as the user saw them. `slug`/`base` name the person being changed and the hash of
/// the file the user opened; both `None` for someone new. Returns `(slug, hash of the new file)`.
pub fn save(lib: &Path, slug: Option<&str>, base: Option<&str>, p: &Person, arabic: bool) -> anyhow::Result<(String, String)> {
    let mut p = check(p)?;
    let dir = lib.join(DIR);
    let (slug, old) = match slug {
        Some(s) => {
            if !valid_slug(s) { return Err(err("people.gone")); }
            no_links(lib, Some(s))?;
            let cur = read_plain(&dir.join(format!("{s}.md"))).ok_or_else(|| err("people.gone"))?;
            if base.map_or(true, |b| b != crate::workflows::content_hash(&cur)) { return Err(err("people.changed")); }
            (s.to_string(), Some(String::from_utf8_lossy(&cur).to_string()))
        }
        None => {
            no_links(lib, None)?;
            if list(lib).len() >= MAX_PEOPLE { return Err(err("people.full")); }
            (new_slug(&dir, &p.name), None)
        }
    };
    no_links(lib, Some(&slug))?;
    // A summary the app made earlier follows the details; one the user wrote stays.
    let before = old.as_deref().map(|t| parse(&slug, t).0);
    let made = |q: &Person| [auto_summary(q, false), auto_summary(q, true)].contains(&p.summary);
    if p.summary.is_empty() || before.as_ref().map_or(false, |b| made(b)) { p.summary = auto_summary(&p, arabic); }
    let text = render(&p, &today(), old.as_deref());
    std::fs::create_dir_all(&dir)?;
    crate::paths::write_atomic(&dir.join(format!("{slug}.md")), text.as_bytes(), false)?;
    write_front(lib, arabic)?;
    Ok((slug, crate::workflows::content_hash(text.as_bytes())))
}

/// Take a person off the list, only as the user saw them. Their file is removed and the front page
/// made again; the library's history (when it is kept) still holds the file.
pub fn remove(lib: &Path, slug: &str, base: &str, arabic: bool) -> anyhow::Result<()> {
    if !valid_slug(slug) { return Err(err("people.gone")); }
    no_links(lib, Some(slug))?;
    let file = lib.join(DIR).join(format!("{slug}.md"));
    let cur = read_plain(&file).ok_or_else(|| err("people.gone"))?;
    if crate::workflows::content_hash(&cur) != base { return Err(err("people.changed")); }
    std::fs::remove_file(&file)?;
    write_front(lib, arabic)
}

/// Is the People list inside a private folder (so the assistant cannot read it)?
pub fn is_private(lib: &Path, private: &[String]) -> bool {
    crate::paths::is_private(lib, &format!("{DIR}/{FRONT}"), private)
}

// ---------- HTTP ----------

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/people", get(api_list))
        .route("/api/people/save", post(api_save))
        .route("/api/people/remove", post(api_remove))
}

type R = Result<Json<Value>, crate::api::ApiErr>;

/// One change to the People list at a time.
static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// → `{people: [person], private: bool}`.
async fn api_list(State(st): State<Arc<AppState>>) -> R {
    let lib = crate::api::lib(&st).await?;
    let private = st.settings.read().await.private_paths_of(Some(&lib));
    let lib2 = lib.clone();
    let people = tokio::task::spawn_blocking(move || list(&lib2)).await?;
    Ok(Json(json!({"people": people.iter().map(Entry::to_json).collect::<Vec<_>>(), "private": is_private(&lib, &private)})))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SaveReq { slug: Option<String>, hash: Option<String>, person: Person }

/// `{slug?, hash?, person}` → `{slug, hash}`.
async fn api_save(State(st): State<Arc<AppState>>, Json(r): Json<SaveReq>) -> R {
    let lib = crate::api::lib(&st).await?;
    // The library's lock is held from the write until the history entry after it.
    let _lib = st.library_lock.of(&lib).try_lock_owned().map_err(|_| crate::tr::err("busy", &[]))?;
    crate::wraps::require_no_interrupted(&st).await?;
    let _g = LOCK.lock().await;
    // With the assistant at work in the library, this change is the user's, never the run's (FEAT-06).
    let _f = crate::changesets::fence(&st).await;
    let (slug, hash) = save(&lib, r.slug.as_deref(), r.hash.as_deref(), &r.person, crate::tr::arabic())?;
    crate::log::info(format!("people: {} in the app", if r.slug.is_some() { "a person changed" } else { "a person added" }));
    let touched = [format!("{DIR}/{slug}.md"), format!("{DIR}/{FRONT}")];
    crate::claude::record_app_write(&st, &lib, &crate::git::subject::edit(&format!("person {}", one_line(&r.person.name))), &touched, true).await;
    st.emit(json!({"type": "library.changed"}));
    Ok(Json(json!({"slug": slug, "hash": hash})))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RemoveReq { slug: String, hash: String }

/// `{slug, hash}` → `{ok: true}`. The file is recorded in the library's history just before it goes.
async fn api_remove(State(st): State<Arc<AppState>>, Json(r): Json<RemoveReq>) -> R {
    let lib = crate::api::lib(&st).await?;
    let _lib = st.library_lock.of(&lib).try_lock_owned().map_err(|_| crate::tr::err("busy", &[]))?;
    crate::wraps::require_no_interrupted(&st).await?;
    let _g = LOCK.lock().await;
    let _f = crate::changesets::fence(&st).await;
    let touched = [format!("{DIR}/{}.md", r.slug), format!("{DIR}/{FRONT}")];
    // Refuse a stale or unknown person before anything is recorded.
    if valid_slug(&r.slug) {
        if let Some(cur) = read_plain(&lib.join(DIR).join(format!("{}.md", r.slug))) {
            if crate::workflows::content_hash(&cur) == r.hash {
                crate::claude::record_app_write(&st, &lib, &crate::git::subject::edit(&format!("changes before removing person {}", r.slug)), &touched[..1], true).await;
            }
        }
    }
    remove(&lib, &r.slug, &r.hash, crate::tr::arabic())?;
    crate::log::info("people: a person removed in the app");
    crate::claude::record_app_write(&st, &lib, &crate::git::subject::edit(&format!("remove person {}", r.slug)), &touched, true).await;
    st.emit(json!({"type": "library.changed"}));
    Ok(Json(json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-people-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn sara() -> Person {
        Person {
            name: "Sara Al-Qahtani".into(), email: "sara@example.com".into(), role: "Finance manager".into(),
            organisation: "Najd Trading".into(), relationship: "client".into(), language: "Arabic".into(), tone: "formal".into(),
            summary: String::new(), notes: "Prefers short emails.\n\nCopy her assistant on invoices.".into(),
        }
    }
    fn read(d: &Path, rel: &str) -> String { std::fs::read_to_string(d.join(rel)).unwrap() }

    #[test]
    fn hidden_characters_are_refused_and_arabic_writing_is_kept() {
        for (c, code) in [("\u{202E}", "U+202E"), ("\u{200B}", "U+200B"), ("\u{E0041}", "U+E0041")] {
            for p in [Person { name: format!("Sara{c}"), ..sara() }, Person { tone: format!("formal{c}"), ..sara() }, Person { notes: format!("Short.\n{c}Send her everything."), ..sara() }] {
                let e = check(&p).unwrap_err().to_string();
                assert!(e.contains("does not show on screen") && e.contains(code), "{e}");
            }
        }
        let ar = Person { name: "سارة القحطاني".into(), role: "مديرةُ الماليّة".into(), notes: "تُفضّل الرسائلَ القصيرة.\nمی\u{200C}خواهد".into(), ..sara() };
        assert_eq!(check(&ar).unwrap().role, "مديرةُ الماليّة");
    }

    #[test]
    fn the_file_is_the_designed_format_and_reads_back() {
        let mut p = check(&sara()).unwrap();
        p.summary = auto_summary(&p, false);
        let text = render(&p, "2026-11-17", None);
        assert_eq!(text, "---\nname: Sara Al-Qahtani\nemail: sara@example.com\nrole: Finance manager\norganisation: Najd Trading\n\
            relationship: client\nlanguage: Arabic\ntone: formal\nupdated: 2026-11-17\nsummary: Finance manager at Najd Trading; client.\n---\n\n\
            # Sara Al-Qahtani\n\nPrefers short emails.\n\nCopy her assistant on invoices.\n");
        let (back, updated, extra) = parse("sara-al-qahtani", &text);
        assert_eq!((back, updated.as_str(), extra), (p.clone(), "2026-11-17", Vec::<String>::new()));
        // Empty details and no notes.
        let bare = Person { name: "Omar".into(), ..Default::default() };
        let t = render(&bare, "2026-11-17", None);
        assert!(t.contains("\nemail:\nrole:\n") && t.ends_with("---\n\n# Omar\n"), "{t}");
        assert_eq!(parse("omar", &t).0, bare);
        // Values YAML would misread are quoted, and come back exactly.
        for tricky in ["yes", "123", "Client: Acme", "- dash", "#1 fan", "@home", "\"quoted\"", "null", "a # b", "سارة: مديرة"] {
            let q = Person { name: "X".into(), role: tricky.into(), ..Default::default() };
            assert_eq!(parse("x", &render(&q, "2026-11-17", None)).0.role, tricky, "{tricky}");
        }
        assert_eq!(auto_summary(&p, true), "Finance manager في Najd Trading؛ client.");
        assert_eq!(auto_summary(&Person { relationship: "friend".into(), ..Default::default() }, false), "friend.");
        assert_eq!(auto_summary(&Person::default(), false), "");
    }

    #[test]
    fn details_the_app_does_not_know_and_the_users_notes_are_kept() {
        let d = lib("keep");
        std::fs::create_dir_all(d.join("people")).unwrap();
        let theirs = "---\nname: Sara\n# met at the fair\nphone: \"+966 5\"\ntags:\n  - vip\n  - riyadh\nrole: Old role\nbirthday: 1990-01-02\n---\n\n# Sara\n\nShe likes **bold** subjects.\n\n- one\n- two\n";
        std::fs::write(d.join("people/sara.md"), theirs).unwrap();
        let e = &list(&d)[0];
        assert_eq!(e.extra, ["birthday", "phone", "tags"]);
        assert_eq!(e.person.notes, "She likes **bold** subjects.\n\n- one\n- two");
        let p = Person { role: "Finance manager".into(), ..e.person.clone() };
        save(&d, Some("sara"), Some(&e.hash), &p, false).unwrap();
        let after = read(&d, "people/sara.md");
        assert!(after.contains("role: Finance manager\n") && !after.contains("Old role"), "{after}");
        assert!(after.contains("# met at the fair\nphone: \"+966 5\"\ntags:\n  - vip\n  - riyadh\nbirthday: 1990-01-02\n---\n"), "{after}");
        assert!(after.ends_with("# Sara\n\nShe likes **bold** subjects.\n\n- one\n- two\n"), "{after}");
        let (_, _, extra) = parse("sara", &after);
        assert_eq!(extra, ["birthday", "phone", "tags"]);
        // A file without details or heading, written by hand, gets them; its text is the notes.
        std::fs::write(d.join("people/omar.md"), "Omar runs the print shop.\n").unwrap();
        let o = list(&d).into_iter().find(|e| e.slug == "omar").unwrap();
        assert_eq!((o.person.name.as_str(), o.person.notes.as_str()), ("omar", "Omar runs the print shop."));
        save(&d, Some("omar"), Some(&o.hash), &Person { name: "Omar Haddad".into(), ..o.person.clone() }, false).unwrap();
        assert!(read(&d, "people/omar.md").ends_with("# Omar Haddad\n\nOmar runs the print shop.\n"));
        assert!(d.join("people/omar.md").exists(), "renaming keeps the file name");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn what_can_be_saved() {
        assert!(check(&sara()).is_ok());
        let with = |f: fn(&mut Person)| { let mut p = sara(); f(&mut p); check(&p) };
        let msg = |r: anyhow::Result<Person>| r.unwrap_err().to_string();
        assert!(msg(with(|p| p.name = "  ".into())).contains("name"));
        assert!(with(|p| p.name = "x".repeat(120)).is_ok());
        assert!(with(|p| p.name = "x".repeat(121)).is_err());
        assert!(with(|p| p.name = "س".repeat(120)).is_ok(), "characters, not bytes");
        for ok in ["", "a@b.co", "first.last+tag@mail.example.org", "o'neil@x.ie", "سارة@مثال.السعودية"] { assert!(with_email(ok), "{ok}"); }
        for bad in ["sara", "a@b", "a@@b.co", "a b@c.co", "a@b.c", "a@-b.co", ".a@b.co", "a..b@c.co", "a@b.co, c@d.co", "<a@b.co>", "a?subject=x@b.co", "a@b.co&body", "a@b..co"] {
            assert!(!with_email(bad), "{bad}");
        }
        assert!(msg(with(|p| p.email = "sara".into())).contains("email address"));
        assert!(with(|p| p.notes = "a\u{7}b".into()).is_err());
        assert!(with(|p| p.role = "a\u{1b}[31m".into()).is_err());
        assert!(with(|p| p.notes = "line\n\tindented".into()).is_ok());
        assert!(with(|p| p.role = "r".repeat(201)).is_err() && with(|p| p.notes = "n".repeat(20_001)).is_err());
        assert!(with(|p| p.summary = "s".repeat(301)).is_err());
        let tidy = with(|p| { p.name = " Sara \n Al-Qahtani ".into(); p.notes = "\r\n\nhi\r\nthere  \n\n".into(); }).unwrap();
        assert_eq!((tidy.name.as_str(), tidy.notes.as_str()), ("Sara Al-Qahtani", "hi\nthere"));
    }
    fn with_email(e: &str) -> bool { check(&Person { email: e.into(), ..sara() }).is_ok() }

    #[test]
    fn file_names_come_from_the_name_and_never_collide() {
        assert_eq!(base_slug("Sara Al-Qahtani"), "sara-al-qahtani");
        assert_eq!(base_slug("  José   Müller (ACME) "), "jose-muller-acme");
        assert_eq!(base_slug("سارة القحطاني"), "sara-alqhtany");
        assert_eq!(base_slug("عَبْدُ اللّٰه"), "abd-allh");
        assert_eq!(base_slug("محمد ٢"), "mhmd-2");
        assert_eq!(base_slug("李雷"), "person");
        assert_eq!(base_slug("README"), "person");
        assert_eq!(base_slug(&"a".repeat(100)).len(), 60);
        let d = lib("slugs");
        std::fs::create_dir_all(d.join("people")).unwrap();
        let (a, _) = save(&d, None, None, &sara(), false).unwrap();
        let (b, _) = save(&d, None, None, &sara(), false).unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("sara-al-qahtani", "sara-al-qahtani-2"));
        // A file named differently in case is the same file on a Mac: never written over.
        std::fs::write(d.join("people/Omar.md"), "---\nname: Omar\n---\n").unwrap();
        let (c, _) = save(&d, None, None, &Person { name: "omar".into(), ..Default::default() }, false).unwrap();
        assert_eq!(c, "omar-2");
        let (z, _) = save(&d, None, None, &Person { name: "李雷".into(), ..Default::default() }, false).unwrap();
        assert_eq!(z, "person");
        for bad in ["", "../x", "a/b", ".hidden", "README", "readme", "a\\b"] { assert!(!valid_slug(bad), "{bad}"); }
        assert!(save(&d, Some("../x"), Some("h"), &sara(), false).unwrap_err().to_string().contains("no longer"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_front_page_is_made_from_the_files_and_keeps_the_users_text() {
        let d = lib("front");
        save(&d, None, None, &sara(), false).unwrap();
        save(&d, None, None, &Person { name: "Adam".into(), email: "adam@x.org".into(), ..Default::default() }, false).unwrap();
        save(&d, None, None, &Person { name: "Bo [Li]".into(), organisation: "Acme".into(), ..Default::default() }, false).unwrap();
        let front = read(&d, "people/README.md");
        assert_eq!(front, "# People\n\nThe people you write to. Each has a page here that the assistant reads when it writes to them.\n\n\
            - [Adam](adam.md) · adam@x.org\n- [Bo \\[Li\\]](bo-li.md) · Acme\n- [Sara Al-Qahtani](sara-al-qahtani.md) · Finance manager, Najd Trading · sara@example.com\n");
        // The user's own text around the list stays; a line the assistant added by hand is folded in.
        std::fs::write(d.join("people/README.md"), "# My contacts\n\nWork people only.\n\n- [Adam](adam.md)\n\n- [Old](gone.md) · x\n\n## Remember\n\nAsk before adding.\n").unwrap();
        let adam = list(&d).into_iter().find(|e| e.slug == "adam").unwrap();
        remove(&d, "adam", &adam.hash, false).unwrap();
        assert_eq!(read(&d, "people/README.md"), "# My contacts\n\nWork people only.\n\n- [Bo \\[Li\\]](bo-li.md) · Acme\n\
            - [Sara Al-Qahtani](sara-al-qahtani.md) · Finance manager, Najd Trading · sara@example.com\n\n## Remember\n\nAsk before adding.\n");
        // Everyone gone: the list goes, the rest stays.
        for e in list(&d) { remove(&d, &e.slug, &e.hash, false).unwrap(); }
        assert_eq!(read(&d, "people/README.md"), "# My contacts\n\nWork people only.\n\n## Remember\n\nAsk before adding.\n");
        // A front page with no list yet gets it at the end.
        assert_eq!(front_page(Some("# Folks\n"), &[], false), "# Folks\n");
        let e = Entry { slug: "a".into(), person: Person { name: "A".into(), ..Default::default() }, updated: String::new(), extra: vec![], hash: String::new() };
        assert_eq!(front_page(Some("# Folks\n"), &[e.clone()], false), "# Folks\n\n- [A](a.md)\n");
        assert!(front_page(None, &[e], true).starts_with("# People\n\nالأشخاص"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn changes_only_to_what_the_user_saw() {
        let d = lib("hash");
        let (slug, hash) = save(&d, None, None, &sara(), false).unwrap();
        assert_eq!(list(&d)[0].hash, hash, "the hash returned is the one listed");
        let changed = |r: anyhow::Result<(String, String)>| r.unwrap_err().to_string().contains("changed after you opened");
        assert!(changed(save(&d, Some(&slug), Some("stale"), &sara(), false)));
        assert!(changed(save(&d, Some(&slug), None, &sara(), false)));
        assert!(remove(&d, &slug, "stale", false).unwrap_err().to_string().contains("changed after you opened"));
        assert!(d.join("people/sara-al-qahtani.md").exists());
        assert!(save(&d, Some("nobody"), Some(&hash), &sara(), false).unwrap_err().to_string().contains("no longer"));
        // Someone edits the file by hand meanwhile: the app refuses instead of writing over it.
        std::fs::write(d.join("people/sara-al-qahtani.md"), read(&d, "people/sara-al-qahtani.md") + "More.\n").unwrap();
        assert!(changed(save(&d, Some(&slug), Some(&hash), &sara(), false)));
        let fresh = list(&d)[0].hash.clone();
        let (_, h2) = save(&d, Some(&slug), Some(&fresh), &Person { tone: "friendly".into(), ..sara() }, false).unwrap();
        remove(&d, &slug, &h2, false).unwrap();
        assert!(list(&d).is_empty() && !d.join("people/sara-al-qahtani.md").exists());
        assert!(remove(&d, &slug, &h2, false).unwrap_err().to_string().contains("no longer"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_summary_the_app_made_follows_the_details_one_the_user_wrote_stays() {
        let d = lib("summary");
        let (slug, h) = save(&d, None, None, &sara(), false).unwrap();
        let e = list(&d).remove(0);
        assert_eq!(e.person.summary, "Finance manager at Najd Trading; client.");
        // The editor sends back what it got, with the role changed: the made summary follows.
        let (_, h) = save(&d, Some(&slug), Some(&h), &Person { role: "CFO".into(), ..e.person.clone() }, false).unwrap();
        assert_eq!(list(&d)[0].person.summary, "CFO at Najd Trading; client.");
        let (_, h) = save(&d, Some(&slug), Some(&h), &Person { summary: "Our main contact for payments.".into(), ..list(&d)[0].person.clone() }, false).unwrap();
        save(&d, Some(&slug), Some(&h), &Person { role: "CEO".into(), ..list(&d)[0].person.clone() }, false).unwrap();
        assert_eq!(list(&d)[0].person.summary, "Our main contact for payments.");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn links_odd_files_and_big_files_are_left_alone() {
        let d = lib("odd");
        std::fs::create_dir_all(d.join("people/folder.md")).unwrap();
        std::fs::write(d.join("people/big.md"), "x".repeat(300 * 1024)).unwrap();
        std::fs::write(d.join("people/.hidden.md"), "---\nname: H\n---\n").unwrap();
        std::fs::write(d.join("people/notes.txt"), "x").unwrap();
        std::fs::write(d.join("people/readme.md"), "# People\n").unwrap();
        std::fs::write(d.join("people/ok.md"), "---\nname: OK\n---\n").unwrap();
        assert_eq!(list(&d).iter().map(|e| e.slug.as_str()).collect::<Vec<_>>(), ["ok"]);
        #[cfg(unix)]
        {
            let elsewhere = lib("elsewhere");
            std::fs::write(elsewhere.join("target.md"), "---\nname: Theirs\n---\n").unwrap();
            std::os::unix::fs::symlink(elsewhere.join("target.md"), d.join("people/linked.md")).unwrap();
            assert_eq!(list(&d).len(), 1, "a link is not listed");
            assert!(save(&d, Some("linked"), Some("x"), &sara(), false).unwrap_err().to_string().contains("link"));
            assert!(remove(&d, "linked", "x", false).unwrap_err().to_string().contains("link"));
            assert_eq!(read(&elsewhere, "target.md"), "---\nname: Theirs\n---\n");
            std::fs::remove_file(d.join("people/linked.md")).unwrap();
            // A linked front page or folder: nothing written through it.
            std::fs::remove_file(d.join("people/readme.md")).unwrap();
            std::os::unix::fs::symlink(elsewhere.join("target.md"), d.join("people/README.md")).unwrap();
            assert!(save(&d, None, None, &sara(), false).unwrap_err().to_string().contains("link"));
            let other = lib("linked-dir");
            std::os::unix::fs::symlink(&elsewhere, other.join("people")).unwrap();
            assert!(list(&other).is_empty());
            assert!(save(&other, None, None, &sara(), false).is_err());
            assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 1, "nothing written through the link");
            assert_eq!(read(&elsewhere, "target.md"), "---\nname: Theirs\n---\n");
            std::fs::remove_dir_all(other).unwrap();
            std::fs::remove_dir_all(elsewhere).unwrap();
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_email_routine_describes_this_format() {
        // The routine the app ships tells the assistant to read and write exactly these files.
        for t in [include_str!("automations/email-writing.en.md"), include_str!("automations/email-writing.ar.md")] {
            assert!(t.contains("people/README.md"));
            for k in KEYS { assert!(t.contains(&format!("\n   {k}: ")), "{k}"); }
            assert!(t.contains("- [{"), "the front page line");
        }
        // A file written by the assistant as the routine says reads back here.
        let by_assistant = "---\nname: Sara Al-Qahtani\nemail: \nrole: Finance manager\norganisation: Najd Trading\nrelationship: client\nlanguage: Arabic\ntone: formal\nupdated: 2026-11-17\nsummary: Finance manager at Najd Trading; client.\n---\n\n# Sara Al-Qahtani\n\nPrefers short emails.\n";
        let (p, updated, extra) = parse("sara-al-qahtani", by_assistant);
        assert_eq!((p.email.as_str(), p.notes.as_str(), updated.as_str(), extra.len()), ("", "Prefers short emails.", "2026-11-17", 0));
        assert_eq!(base_slug(&p.name), "sara-al-qahtani", "the example file name in the routine");
    }

    #[tokio::test]
    async fn the_endpoints_save_list_and_remove_with_history() {
        let (st, lib) = crate::wraps::testutil::library().await;
        let r = api_save(State(st.clone()), Json(SaveReq { person: sara(), ..Default::default() })).await.unwrap().0;
        assert_eq!(r["slug"], "sara-al-qahtani");
        let last = crate::wraps::testutil::sh(&lib, &["log", "-1", "--name-only", "--format=%s"]);
        assert_eq!(last.lines().filter(|l| !l.is_empty()).collect::<Vec<_>>(), ["edit: person Sara Al-Qahtani", "people/README.md", "people/sara-al-qahtani.md"]);
        let listed = api_list(State(st.clone())).await.unwrap().0;
        assert_eq!(listed["private"], false);
        let p = &listed["people"][0];
        assert_eq!((p["slug"].as_str(), p["email"].as_str(), p["hash"].as_str()), (Some("sara-al-qahtani"), Some("sara@example.com"), r["hash"].as_str()));
        // Stale: a plain message, nothing changed.
        use axum::response::IntoResponse;
        let e = api_remove(State(st.clone()), Json(RemoveReq { slug: "sara-al-qahtani".into(), hash: "old".into() })).await.unwrap_err();
        let resp = e.into_response();
        assert_eq!(resp.status(), 409);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("changed after you opened"));
        let _ = api_remove(State(st.clone()), Json(RemoveReq { slug: "sara-al-qahtani".into(), hash: p["hash"].as_str().unwrap().into() })).await.unwrap();
        assert!(!lib.join("people/sara-al-qahtani.md").exists());
        assert_eq!(crate::wraps::testutil::sh(&lib, &["log", "-1", "--format=%s"]), "edit: remove person sara-al-qahtani");
        // The file can be brought back from the history.
        assert!(crate::wraps::testutil::sh(&lib, &["show", "HEAD~1:people/sara-al-qahtani.md"]).contains("name: Sara Al-Qahtani"));
        // A private People list: still the user's to edit here, and the list says the assistant cannot read it.
        st.settings.write().await.private_paths = vec!["people".into()];
        let _ = api_save(State(st.clone()), Json(SaveReq { person: sara(), ..Default::default() })).await.unwrap();
        let listed = api_list(State(st.clone())).await.unwrap().0;
        assert_eq!((listed["private"].as_bool(), listed["people"].as_array().map(Vec::len)), (Some(true), Some(1)));
        // The suggestion summary only counts it.
        assert_eq!(crate::automations::count_people(&lib), Some(1));
        // Busy library: refused.
        let held = st.library_lock.of(&lib).try_lock_owned().unwrap();
        assert!(api_save(State(st.clone()), Json(SaveReq { person: sara(), ..Default::default() })).await.is_err());
        drop(held);
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn a_person_saved_during_an_answer_is_the_users() {
        let (st, lib) = crate::wraps::testutil::library().await;
        let chat = crate::wraps::testutil::chat(&st, "Plan").await;
        let w = crate::changesets::begin(&st, &chat, &lib, &lib).await.expect("watched");
        crate::wraps::testutil::write(&lib, "notes/plan.md", "the answer's, half written\n");
        let _ = api_save(State(st.clone()), Json(SaveReq { person: sara(), ..Default::default() })).await.unwrap();
        let last = crate::wraps::testutil::sh(&lib, &["log", "-1", "--name-only", "--format=%s"]);
        assert_eq!(last.lines().filter(|l| !l.is_empty()).collect::<Vec<_>>(), ["edit: person Sara Al-Qahtani", "people/README.md", "people/sara-al-qahtani.md"]);
        assert!(crate::changesets::finish(&st, &chat, "assistant: Plan").await);
        drop(w);
        let c = st.chats.with(&chat, |c| c.messages.iter().find(|m| m.role == "changes").and_then(|m| m.tool.clone())).await.flatten().unwrap();
        assert_eq!(c["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect::<Vec<_>>(), ["notes/plan.md"]);
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }
}
