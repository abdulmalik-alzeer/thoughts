//! "Suggest automations": on the user's request only, the assistant looks at a short summary of how
//! the library and the app are used and proposes a few things worth automating — a Routine (skill),
//! a Helper (agent), a Connection (integration) or a New page (tab). Building one always goes through
//! a path the app already reviews: the routine editor, the helper editor, a guide the user follows,
//! or the customisation flow (Review › Keep / Discard › Rollback).
//!
//! Nothing here runs in the background and nothing is analysed until the user presses the button.
//! The question is one tool-less answer (`claude::complete`), asked in the app's home folder, never
//! in the library, with nothing in it but the summary below, which the user can read first
//! (`GET /api/automations/preview`). The summary never holds the contents of notes: chat titles and
//! the first line of each chat's first message, names of routines, tasks, add-ons and pages, and
//! counts. Private folders and other libraries are left out.
//!
//! Split for testing: `gather` (reads the library, chats and settings into `Facts`) is the only part
//! that touches anything; `summarise`, `render_summary`, `suggest_prompt` and `parse_suggestions`
//! are pure.

use crate::assistant::Program;
use crate::routines::Draft;
use crate::state::AppState;
use axum::extract::{Path as UrlPath, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// ---------- what is gathered ----------

/// One chat, as far as the summary may know it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatFact {
    /// "chat" | "wrap" | "customize" | "workflow" | …
    pub kind: String,
    pub title: String,
    pub created: DateTime<Utc>,
    /// The first user message, one line, at most `FIRST_CHARS` characters (ordinary chats only).
    pub first: String,
    /// It belongs to the library that is open now.
    pub in_library: bool,
    /// It attached or @-mentioned something in a private folder, or its first message names one.
    pub private: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TodoFacts { pub open: usize, pub done: usize, pub overdue: usize, pub repeating: usize, pub groups: Vec<String> }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileFacts {
    /// Top-level folder ("" = files at the top of the library) → (files, changed in the last 30 days).
    pub folders: BTreeMap<String, (usize, usize)>,
    /// Ending (lowercase, "" = none) → files.
    pub kinds: BTreeMap<String, usize>,
    pub total: usize,
    /// Counting stopped at `WALK_MAX` entries.
    pub cut: bool,
}

/// Everything the summary is made from. Every part may be empty.
#[derive(Clone, Debug, PartialEq)]
pub struct Facts {
    pub chats: Vec<ChatFact>,
    /// (title, description)
    pub routines: Vec<(String, String)>,
    /// (name, description)
    pub helpers: Vec<(String, String)>,
    /// (title, when it runs, in words)
    pub tasks: Vec<(String, String)>,
    pub addons: Vec<String>,
    pub pages: Vec<String>,
    pub chips: Vec<String>,
    pub hidden: Vec<String>,
    /// People in `people/` (None: there is no such folder).
    pub people: Option<usize>,
    pub files: FileFacts,
    pub todos: TodoFacts,
    /// `usage::summary` — only when the user left usage signals on.
    pub usage: Option<String>,
    pub program: Program,
    pub arabic: bool,
}

impl Default for Facts {
    fn default() -> Self {
        Facts { chats: vec![], routines: vec![], helpers: vec![], tasks: vec![], addons: vec![], pages: vec![], chips: vec![], hidden: vec![],
            people: None, files: FileFacts::default(), todos: TodoFacts::default(), usage: None, program: Program::Claude, arabic: false }
    }
}

const FIRST_CHARS: usize = 200;
/// The library walk stops after this many entries.
pub const WALK_MAX: usize = 20_000;
/// The summary handed to the assistant is never longer than this.
pub const SUMMARY_MAX: usize = 8_000;

/// Helpers exist only for Claude Code (its project `.claude/agents`).
pub fn helpers_offered(program: Program) -> bool { program == Program::Claude }

/// The helpers already in the library: (name, description).
// TODO(auto-agents): read them with `crate::agents::list(lib)` once `agents.rs` exists.
fn existing_helpers(_lib: &Path) -> Vec<(String, String)> { vec![] }

/// Text as one plain line: whitespace collapsed, control and invisible characters gone, cut at `max` characters.
fn clean(s: &str, max: usize) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).filter(|c| !crate::staging::validate::is_hidden_char(*c)).collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// Does `text` name the private folder `dir`, as a path (`me/finances`, `@me`, `me/…`)?
fn names_folder(text: &str, dir: &str) -> bool {
    let dir = dir.trim_matches(['/', '\\']).replace('\\', "/").to_lowercase();
    if dir.is_empty() { return false; }
    let text = text.replace('\\', "/").to_lowercase();
    let nested = dir.contains('/');
    let mut from = 0;
    while let Some(i) = text[from..].find(&dir) {
        let at = from + i;
        let before = text[..at].chars().next_back();
        let after = text[at + dir.len()..].chars().next();
        let starts = before.map_or(true, |c| !c.is_alphanumeric() && c != '-' && c != '_');
        let ends = after.map_or(true, |c| !c.is_alphanumeric() && c != '-' && c != '_');
        // A one-word folder ("me") counts only when it is written as a path: `me/…` or `@me`.
        let as_path = nested || after == Some('/') || before == Some('@') || before == Some('/');
        if starts && ends && as_path { return true; }
        from = at + dir.len().max(1);
    }
    false
}

/// What the summary may know of one chat. `root`/`private`: the library and its private folders.
pub fn chat_fact(c: &crate::chats::Chat, in_library: bool, root: &Path, private: &[String]) -> ChatFact {
    let users: Vec<&crate::chats::Msg> = c.messages.iter().filter(|m| m.role == "user").collect();
    let first_text = users.first().map(|m| m.text.as_str()).unwrap_or_default();
    let mentioned = users.iter().flat_map(|m| m.tool.as_ref().and_then(|t| t["mentions"].as_array().cloned()).unwrap_or_default())
        .filter_map(|v| v.as_str().map(String::from)).chain(users.iter().flat_map(|m| m.attachments.iter().cloned()));
    let root_text = root.to_string_lossy().replace('\\', "/");
    let private_hit = mentioned.into_iter().any(|p| crate::paths::is_private(root, &p, private))
        || private.iter().any(|d| names_folder(first_text, d) || names_folder(first_text, &format!("{root_text}/{d}")));
    ChatFact {
        kind: c.kind.clone(), title: clean(&c.title, 120), created: c.created,
        first: if c.kind == "chat" { clean(first_text, FIRST_CHARS) } else { String::new() },
        in_library, private: private_hit,
    }
}

/// A schedule in words, for the summary.
pub fn trigger_words(t: &crate::workflows::Trigger) -> String {
    use crate::workflows::Trigger;
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    match t {
        Trigger::Event { on } => match on.as_str() { "wrap.done" => "after each wrap".into(), "app.start" => "when the app starts".into(), o => format!("on {}", clean(o, 40)) },
        Trigger::Schedule { every, at, weekday, day, .. } => {
            let at = clean(at, 5);
            match every.as_str() {
                "day" => format!("every day at {at}"),
                "weekdays" => format!("on working days at {at}"),
                "week" => format!("every {} at {at}", DAYS[weekday.unwrap_or(0).min(6) as usize]),
                "month" => match day {
                    Some(Value::String(s)) if s == "last" => format!("on the last day of each month at {at}"),
                    Some(d) => format!("each month on day {} at {at}", d.as_u64().unwrap_or(1)),
                    None => format!("each month at {at}"),
                },
                other => format!("every {} at {at}", clean(other, 20)),
            }
        }
    }
}

/// Count the library's files without opening any: per top-level folder and per ending. Leaves out
/// `.git`, `.claude`, `node_modules`, every other hidden folder, private folders and links.
pub fn walk_files(lib: &Path, private: &[String], now: std::time::SystemTime) -> FileFacts {
    let mut out = FileFacts::default();
    let month = std::time::Duration::from_secs(30 * 86_400);
    let mut seen = 0usize;
    let it = walkdir::WalkDir::new(lib).follow_links(false).min_depth(1).sort_by_file_name().into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        if name.starts_with('.') || name == "node_modules" || e.file_type().is_symlink() { return false; }
        let rel = e.path().strip_prefix(lib).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        !crate::paths::is_private(lib, &rel, private)
    });
    for e in it.flatten() {
        seen += 1;
        if seen > WALK_MAX { out.cut = true; break; }
        if !e.file_type().is_file() { continue; }
        let rel = e.path().strip_prefix(lib).map(Path::to_path_buf).unwrap_or_default();
        let top = if rel.components().count() > 1 { rel.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default() } else { String::new() };
        let ext = rel.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        let fresh = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|m| now.duration_since(m).ok()).map_or(false, |d| d < month);
        let f = out.folders.entry(top).or_default();
        f.0 += 1;
        if fresh { f.1 += 1; }
        *out.kinds.entry(ext).or_default() += 1;
        out.total += 1;
    }
    out
}

/// The library's todos, counted (private folders left out).
pub fn todo_facts(lib: &Path, private: &[String], today: NaiveDate) -> TodoFacts {
    let mut t = TodoFacts::default();
    for f in crate::todos::list(lib) {
        let file = f["file"].as_str().unwrap_or_default();
        if crate::paths::is_private(lib, file, private) { continue; }
        if f["idea"].is_null() && file == crate::todos::general(lib) { t.groups = f["groups"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(|g| clean(g, 60))).collect(); }
        for i in f["items"].as_array().into_iter().flatten() {
            if i["done"] == true { t.done += 1; continue; }
            t.open += 1;
            if i["every"].is_string() { t.repeating += 1; }
            if i["due"].as_str().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).map_or(false, |d| d < today) { t.overdue += 1; }
        }
    }
    t
}

/// How many people `people/` holds: one file each besides its front page (None: no such folder).
/// Only names are looked at, never the files, so a private `people/` is counted and never read.
pub fn count_people(lib: &Path) -> Option<usize> {
    let dir = lib.join("people");
    if !std::fs::symlink_metadata(&dir).map_or(false, |m| m.is_dir()) { return None; }
    Some(std::fs::read_dir(&dir).into_iter().flatten().flatten()
        .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.to_lowercase().ends_with(".md") && !n.eq_ignore_ascii_case("README.md") && !n.starts_with('.')).count())
}

/// Read everything the summary is made from. The only part of this module that touches anything.
pub async fn gather(st: &AppState, lib: &Path) -> Facts {
    let s = st.settings.read().await.clone();
    let private = s.private_paths_of(Some(lib));
    let scope = crate::wraps::Scope::of(Some(lib.to_path_buf()));
    let since = Utc::now() - Duration::days(90);
    let chats: Vec<ChatFact> = {
        let all = st.chats.chats.read().await;
        all.values().filter(|c| c.created > since && c.library.is_some())
            .map(|c| chat_fact(c, scope.holds(c.library.as_deref()), lib, &private)).collect()
    };
    let layer = crate::paths::app_layer_dir();
    let manifest = crate::applayer::manifest_of(&layer);
    let bound = crate::workflows::load_bound(&crate::paths::home());
    let tasks: Vec<(String, String)> = crate::workflows::load_files(&layer).into_iter()
        .filter(|l| bound.get(&l.file).map_or(true, |b| scope.holds(Some(b))))
        .map(|l| (clean(&l.workflow.name, 80), trigger_words(&l.workflow.trigger))).collect();
    let addons: Vec<String> = crate::applayer::addons_of(&layer, false).iter().filter(|a| a["kind"] == "plugin").filter_map(|a| a["label"].as_str().map(|l| clean(l, 60))).collect();
    let pages: Vec<String> = manifest["views"].as_array().into_iter().flatten().filter_map(|v| v["name"].as_str().map(|n| clean(&crate::applayer::item_label(&format!("views/{n}")), 60))).collect();
    let chips: Vec<String> = manifest["home"]["chips"].as_array().into_iter().flatten()
        .filter_map(|c| c["label"].as_str().or_else(|| c["prompt"].as_str()).map(|l| clean(l, 60))).collect();
    let hidden: Vec<String> = manifest["nav"]["hidden"].as_array().into_iter().flatten().filter_map(|h| h.as_str().map(|h| clean(h, 40))).collect();
    let usage = s.usage_signals.then(|| crate::usage::summary(&crate::usage::read(&crate::paths::home()), chrono::Local::now()));
    let (lib2, private2) = (lib.to_path_buf(), private.clone());
    let today = chrono::Local::now().date_naive();
    let (routines, helpers, people, files, todos) = tokio::task::spawn_blocking(move || {
        let routines: Vec<(String, String)> = crate::routines::list(&lib2).iter()
            .map(|r| (clean(r["title"].as_str().unwrap_or_default(), 80), clean(r["description"].as_str().unwrap_or_default(), 300))).collect();
        (routines, existing_helpers(&lib2), count_people(&lib2), walk_files(&lib2, &private2, std::time::SystemTime::now()), todo_facts(&lib2, &private2, today))
    }).await.unwrap_or_default();
    Facts { chats, routines, helpers, tasks, addons, pages, chips, hidden, people, files, todos, usage,
        program: crate::assistant::primary(&s), arabic: s.language == "ar" }
}

// ---------- the summary (pure) ----------

/// A kind of request the summary counts, with the words that give it away (English and Arabic).
/// A Latin word matches at the start of a word (`email` in "emails", not in "re-email…" — the
/// dash counts as a break); one ending in `$` only as a whole word. Arabic words match anywhere, as
/// they take prefixes (`بالبريد`).
struct Lex { id: &'static str, label: &'static str, words: &'static [&'static str] }

const LEXICON: &[Lex] = &[
    Lex { id: "email", label: "writing email", words: &["email", "e-mail", "mail", "reply to", "write to", "بريد", "إيميل", "ايميل", "رسالة إلى", "رسالة الى", "رد على"] },
    Lex { id: "meeting", label: "meetings", words: &["meeting", "minutes of", "agenda", "اجتماع", "محضر"] },
    Lex { id: "summary", label: "summaries", words: &["summary", "summaries", "summarise", "summarize", "sum up", "tl;dr", "ملخص", "لخص", "تلخيص"] },
    Lex { id: "translate", label: "translation", words: &["translate", "translation", "ترجم"] },
    Lex { id: "report", label: "reports", words: &["report", "تقرير", "تقارير"] },
    Lex { id: "invoice", label: "invoices or bills", words: &["invoice", "bill$", "bills$", "billing", "receipt", "فاتورة", "فواتير"] },
    Lex { id: "post", label: "social media posts", words: &["social media", "social post", "linkedin", "tweet", "instagram", "post about", "post for", "منشور", "تغريدة"] },
    Lex { id: "plan", label: "plans or schedules", words: &["plan$", "plans$", "planning", "schedule", "itinerary", "خطة", "خطط", "جدول"] },
    Lex { id: "letter", label: "letters", words: &["letter", "خطاب"] },
    Lex { id: "cv", label: "CVs or résumés", words: &["cv$", "cvs$", "resume$", "résumé", "curriculum vitae", "سيرة ذاتية", "السيرة الذاتية"] },
];

fn has_word(text: &str, word: &str) -> bool {
    let (word, whole) = match word.strip_suffix('$') { Some(w) => (w, true), None => (word, false) };
    if !word.is_ascii() { return text.contains(word); }
    let mut from = 0;
    while let Some(i) = text[from..].find(word) {
        let at = from + i;
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        if before.map_or(true, |c| !c.is_alphanumeric()) && (!whole || after.map_or(true, |c| !c.is_alphanumeric())) { return true; }
        from = at + word.len();
    }
    false
}

/// The ids of the kinds of request `text` is about.
pub fn kinds_of(text: &str) -> Vec<&'static str> {
    let t = text.to_lowercase();
    LEXICON.iter().filter(|l| l.words.iter().any(|w| has_word(&t, w))).map(|l| l.id).collect()
}

const ADDRESS_BREAKS: &str = "<>()[]{},;:\"'«»“”‘’";

/// Is `tok` (punctuation at its ends aside) an email address?
fn address(tok: &str) -> Option<&str> {
    let tok = tok.trim_matches(|c: char| c == '.' || c == '!' || c == '?');
    let (local, domain) = tok.split_once('@')?;
    let local_ok = !local.is_empty() && local.chars().all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c));
    let labels: Vec<&str> = domain.split('.').collect();
    let domain_ok = labels.len() >= 2 && labels.iter().all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && labels.last().map_or(false, |l| l.len() >= 2 && l.chars().all(|c| c.is_ascii_alphabetic()));
    (local_ok && domain_ok).then_some(tok)
}

/// The distinct email addresses written in `texts` (lowercase).
pub fn addresses_in<'a>(texts: impl IntoIterator<Item = &'a str>) -> HashSet<String> {
    let mut out = HashSet::new();
    for t in texts {
        for tok in t.split(|c: char| c.is_whitespace() || ADDRESS_BREAKS.contains(c)) {
            if let Some(a) = address(tok) { out.insert(a.to_lowercase()); }
        }
    }
    out
}

/// `text` with every email address in it written as "[email address]": the summary counts them, it never names them.
pub fn mask_addresses(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut tok = String::new();
    let flush = |tok: &mut String, out: &mut String| {
        match address(tok) {
            Some(a) => { let start = tok.find(a).unwrap_or(0); out.push_str(&tok[..start]); out.push_str("[email address]"); out.push_str(&tok[start + a.len()..]); }
            None => out.push_str(tok),
        }
        tok.clear();
    };
    for c in text.chars() {
        if c.is_whitespace() || ADDRESS_BREAKS.contains(c) { flush(&mut tok, &mut out); out.push(c); } else { tok.push(c); }
    }
    flush(&mut tok, &mut out);
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct KindCount { pub id: &'static str, pub label: &'static str, pub n30: usize, pub n90: usize }

/// What the summary says, before it is written out.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub today: NaiveDate,
    pub chats30: usize,
    pub chats90: usize,
    /// Kinds of request seen at least once, most frequent first.
    pub kinds: Vec<KindCount>,
    /// Openings (first three words) used in 3 or more chats: (opening, chats).
    pub openings: Vec<(String, usize)>,
    /// Distinct email addresses in titles and first messages (only the number).
    pub addresses: usize,
    /// Runs of other kinds in the last 90 days: (what, how many).
    pub other: Vec<(String, usize)>,
    /// (date, title, first line), newest first, email addresses masked.
    pub recent: Vec<(NaiveDate, String, String)>,
    pub facts: Facts,
}

const MAX_LIST: usize = 30;
const MAX_FOLDERS: usize = 25;
const MAX_KINDS: usize = 15;
const MAX_RECENT: usize = 150;

fn other_label(kind: &str) -> String {
    match kind {
        "wrap" => "wraps".into(), "customize" => "customisation chats".into(), "workflow" => "automatic task runs".into(),
        "setup" => "set-up runs".into(), "module" => "feature set-ups".into(), "reorganize" => "reorganisations".into(),
        "update" => "app update runs".into(), o => format!("{} runs", clean(o, 20)),
    }
}

/// The summary of `f` at `now`. Only chats of the open library, made in the last 90 days, without
/// a private folder in them, count.
pub fn summarise(f: &Facts, now: DateTime<Utc>) -> Summary {
    let (d30, d90) = (now - Duration::days(30), now - Duration::days(90));
    let mut kept: Vec<&ChatFact> = f.chats.iter().filter(|c| c.in_library && !c.private && c.created > d90 && c.created <= now).collect();
    kept.sort_by(|a, b| b.created.cmp(&a.created).then(a.title.cmp(&b.title)).then(a.first.cmp(&b.first)));
    let chats: Vec<&ChatFact> = kept.iter().copied().filter(|c| c.kind == "chat").collect();
    let mut other: BTreeMap<String, usize> = BTreeMap::new();
    for c in kept.iter().filter(|c| c.kind != "chat") { *other.entry(other_label(&c.kind)).or_default() += 1; }

    let mut kinds: Vec<KindCount> = LEXICON.iter().map(|l| KindCount { id: l.id, label: l.label, n30: 0, n90: 0 }).collect();
    let mut openings: BTreeMap<String, usize> = BTreeMap::new();
    for c in &chats {
        for id in kinds_of(&format!("{} {}", c.title, c.first)) {
            let k = kinds.iter_mut().find(|k| k.id == id).expect("a lexicon kind");
            k.n90 += 1;
            if c.created > d30 { k.n30 += 1; }
        }
        let p = crate::suggestions::prompt_prefix(&c.first);
        if p.chars().count() >= 3 { *openings.entry(p.to_lowercase()).or_default() += 1; }
    }
    let order = |id: &str| LEXICON.iter().position(|l| l.id == id).unwrap_or(usize::MAX);
    kinds.retain(|k| k.n90 > 0);
    kinds.sort_by(|a, b| b.n30.cmp(&a.n30).then(b.n90.cmp(&a.n90)).then(order(a.id).cmp(&order(b.id))));
    let mut openings: Vec<(String, usize)> = openings.into_iter().filter(|(_, n)| *n >= 3).collect();
    openings.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    openings.truncate(10);
    let addresses = addresses_in(chats.iter().flat_map(|c| [c.title.as_str(), c.first.as_str()])).len();
    let recent = chats.iter().take(MAX_RECENT).map(|c| (c.created.date_naive(), mask_addresses(&c.title), mask_addresses(&c.first))).collect();

    let mut facts = f.clone();
    facts.chats.clear();
    for list in [&mut facts.addons, &mut facts.pages, &mut facts.chips, &mut facts.hidden] { list.truncate(MAX_LIST); }
    for list in [&mut facts.routines, &mut facts.helpers, &mut facts.tasks] { list.truncate(MAX_LIST); }
    Summary {
        today: now.date_naive(), chats30: chats.iter().filter(|c| c.created > d30).count(), chats90: chats.len(),
        kinds, openings, addresses,
        other: other.into_iter().collect(), recent, facts,
    }
}

fn plural(n: usize, one: &str, many: &str) -> String { format!("{n} {}", if n == 1 { one } else { many }) }

/// The summary as plain English lines (what the assistant reads, and what "See what will be sent"
/// shows). Never longer than `SUMMARY_MAX`: what matters most comes first, so the cut takes the
/// chat titles first and the counts last. The same summary always gives the same text.
pub fn render_summary(s: &Summary) -> String {
    let f = &s.facts;
    let mut sections: Vec<Vec<String>> = vec![];
    sections.push(vec![
        format!("How this person uses Thoughts (gathered on their computer on {}):", s.today.format("%Y-%m-%d")),
        format!("Assistant program: {}. App language: {}.", f.program.label(), if f.arabic { "Arabic" } else { "English" }),
        format!("Chats in this library: {} in the last 30 days, {} in the last 90 days.", s.chats30, s.chats90),
    ]);
    if !s.kinds.is_empty() {
        let mut v = vec!["What the chats were about:".to_string()];
        for k in &s.kinds { v.push(format!("- {} of {} chats in the last 30 days were about {} ({} of {} in the last 90 days).", k.n30, s.chats30, k.label, k.n90, s.chats90)); }
        sections.push(v);
    }
    if !s.openings.is_empty() {
        let mut v = vec!["Chats that start with the same words:".to_string()];
        for (o, n) in &s.openings { v.push(format!("- \"{o}\": {}", plural(*n, "chat", "chats"))); }
        sections.push(v);
    }
    if s.addresses > 0 {
        sections.push(vec![format!("{} were written in their messages (the addresses are not shown here).", plural(s.addresses, "distinct email address", "distinct email addresses"))]);
    }
    if !s.other.is_empty() {
        sections.push(vec![format!("Other work in the last 90 days: {}.", s.other.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", "))]);
    }
    let t = &f.todos;
    let mut todo = format!("Todos: {} open ({} overdue, {} repeating), {} done.", t.open, t.overdue, t.repeating, t.done);
    if !t.groups.is_empty() { todo.push_str(&format!(" Groups in the general list: {}.", t.groups.join(", "))); }
    sections.push(vec![todo]);
    sections.push(vec![match f.people {
        None => "People list: none yet.".to_string(),
        Some(n) => format!("People list: {}.", plural(n, "person", "people")),
    }]);
    let named = |head: &str, list: &[(String, String)], sep: &str| -> Vec<String> {
        if list.is_empty() { return vec![format!("{head}: none.")]; }
        std::iter::once(format!("{head}:")).chain(list.iter().map(|(a, b)| if b.is_empty() { format!("- {a}") } else { format!("- {a}{sep}{}", b.chars().take(160).collect::<String>()) })).collect()
    };
    sections.push(named("Routines", &f.routines, " — "));
    if helpers_offered(f.program) { sections.push(named("Helpers", &f.helpers, " — ")); }
    sections.push(named("Automatic tasks", &f.tasks, ", "));
    let names = |head: &str, list: &[String]| if list.is_empty() { format!("{head}: none.") } else { format!("{head}: {}.", list.join(", ")) };
    sections.push(vec![names("Add-ons", &f.addons), names("Pages added to the app", &f.pages), names("Home buttons", &f.chips), names("Hidden from the sidebar", &f.hidden)]);
    let fl = &f.files;
    let mut files = vec![format!("Library files: {}{}.", fl.total, if fl.cut { format!(" (counting stopped after {WALK_MAX} entries)") } else { String::new() })];
    let mut folders: Vec<(&String, &(usize, usize))> = fl.folders.iter().collect();
    folders.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
    for (name, (n, fresh)) in folders.into_iter().take(MAX_FOLDERS) {
        let name = if name.is_empty() { "(top of the library)".to_string() } else { clean(name, 60) };
        files.push(format!("- {name}: {}, {fresh} changed in the last 30 days", plural(*n, "file", "files")));
    }
    let mut kinds: Vec<(&String, &usize)> = fl.kinds.iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    if !kinds.is_empty() {
        files.push(format!("By type: {}.", kinds.into_iter().take(MAX_KINDS).map(|(k, n)| format!("{} {n}", if k.is_empty() { "no ending".to_string() } else { format!(".{}", clean(k, 12)) })).collect::<Vec<_>>().join(", ")));
    }
    sections.push(files);
    if let Some(u) = &f.usage {
        sections.push(std::iter::once("How the app was used in the last 30 days:".to_string()).chain(u.lines().map(|l| l.chars().take(600).collect())).collect());
    }
    if !s.recent.is_empty() {
        let mut v = vec!["Recent chats, newest first (date · title — first line of the first message):".to_string()];
        for (d, title, first) in &s.recent {
            v.push(if first.is_empty() { format!("- {} · {title}", d.format("%Y-%m-%d")) } else { format!("- {} · {title} — {first}", d.format("%Y-%m-%d")) });
        }
        sections.push(v);
    }
    let mut out = String::new();
    'all: for sec in sections {
        for line in sec {
            if out.len() + line.len() + 1 > SUMMARY_MAX { break 'all; }
            out.push_str(&line);
            out.push('\n');
        }
        if out.len() + 1 <= SUMMARY_MAX { out.push('\n'); }
    }
    out.trim_end().to_string()
}

// ---------- the question (pure) ----------

/// The built-in templates the assistant may name: (id, kind, what it is).
pub const TEMPLATES: &[(&str, &str, &str)] = &[
    ("email-writing", "skill", "the app's own ready-made routine \"Email writing\": it drafts an email in the recipient's language and tone from the person's People list, ends with an Open in Mail link, and never sends anything. Use it for a person who writes email in their chats (set \"people\": true)."),
    ("email-account", "integration", "the app's own guide to connecting an email account to the assistant program. Use it for connecting a mail account."),
];

/// What the assistant is asked. `dismissed`: titles the person turned down; `existing`: what they
/// already have (routines, helpers, tasks, add-ons, pages, and what was built from earlier suggestions).
pub fn suggest_prompt(summary: &str, dismissed: &[String], existing: &[String], program: Program, arabic: bool) -> String {
    let helpers = helpers_offered(program);
    let mut kinds = String::from("- \"skill\" (shown to them as a Routine): step-by-step instructions saved in their library that the assistant follows whenever they ask for it, for example \"meeting notes\". A routine works only with the files in the library (it reads and writes notes, todos and logs) and can end with links such as an Open in Mail (mailto:) link.\n");
    if helpers { kinds.push_str("- \"agent\" (shown as a Helper): a specialist with its own instructions that the assistant can hand part of the work to in their chats, for example a careful proofreader. It gets no extra permissions.\n"); }
    kinds.push_str("- \"integration\" (shown as a Connection): a guide that walks them through connecting an outside service (email, calendar, storage) to their assistant program themselves. Thoughts never signs in, stores a password or sends anything; it only shows the steps.\n");
    kinds.push_str("- \"tab\" (shown as a New page): a new page or Home widget inside the Thoughts app, built through the app's customisation flow, which they review before keeping. It can show their notes and todos.\n");
    let list = |l: &[String]| if l.is_empty() { "(none)".to_string() } else { l.iter().map(|t| format!("- {}", clean(t, 100))).collect::<Vec<_>>().join("\n") };
    let templates = TEMPLATES.iter().map(|(id, kind, what)| format!("- \"{id}\" (kind \"{kind}\"): {what}")).collect::<Vec<_>>().join("\n");
    let kind_names = if helpers { "\"skill\" | \"agent\" | \"integration\" | \"tab\"" } else { "\"skill\" | \"integration\" | \"tab\"" };
    let no_helpers = if helpers { "" } else { "\n- Do not suggest helpers (\"agent\"): their assistant program (Codex) does not have them." };
    let language = if arabic { "Arabic" } else { "English" };
    format!("You help a person who is not a developer get more out of Thoughts, an app that keeps their notes, todos and chats with an AI assistant in a library of plain files on their computer. \
They pressed \"Suggest automations\". Below is a short summary of how they use it, gathered by the app. You have no tools and cannot open any file: work only from what is written here.\n\n\
What Thoughts can build for them, by kind:\n{kinds}\n\
Built-in templates you may name in \"template\":\n{templates}\n\n\
<<<SUMMARY\n{summary}\nSUMMARY>>>\n\n\
What they already have (never suggest these again, or anything that does the same):\n{}\n\n\
Suggestions they turned down (never suggest these again, or anything close to them):\n{}\n\n\
Reply with ONE JSON object and nothing else:\n\
{{\"suggestions\": [{{\"kind\": {kind_names}, \"title\": \"short name, at most 60 characters\", \"what\": \"what it would do for them, 1-2 sentences\", \"why\": \"the behaviour it came from, quoting the summary's numbers\", \"build\": \"what building it involves, 1 sentence, plain words\", \"request\": \"detailed instructions for building it, at most 3000 characters\", \"template\": \"email-writing\" | \"email-account\" | null, \"people\": true | false}}], \"nothing\": \"if nothing is worth automating yet: one short sentence why; else empty\"}}\n\n\
Rules:\n\
- 3 to 5 suggestions, most useful first, and only ones the summary supports: every \"why\" cites a number from it. Fewer, or none, is fine. Never invent behaviour.\n\
- Write title, what, why and build in plain {language} words a non-developer understands: no file names, code, commands or jargon.\n\
- Never include shell commands, lines starting with \"!\", or anything that installs or runs programs. Nothing that signs in, asks for a password or sends anything on their behalf.{no_helpers}\n\
- Prefer a template when one fits: \"email-writing\" for a person who writes email in their chats, \"email-account\" for connecting a mail account.\n\
- Set \"people\": true when a routine would work better with a list of the people they write to (names, addresses, roles, tone).",
        list(existing), list(dismissed))
}

// ---------- reading the answer (pure) ----------

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Suggestion {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub what: String,
    pub why: String,
    pub build: String,
    pub request: String,
    pub template: Option<String>,
    pub people: bool,
}

pub const KINDS: &[&str] = &["skill", "agent", "integration", "tab"];
pub const MAX_SUGGESTIONS: usize = 5;

/// What the answer is checked against.
pub struct Ctx<'a> {
    pub program: Program,
    /// Ids already turned down or built.
    pub skip: &'a HashSet<String>,
    /// Titles of the library's routines and helpers.
    pub routines: &'a [String],
    pub helpers: &'a [String],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed { pub suggestions: Vec<Suggestion>, pub nothing: String }

/// A title as compared: lowercase letters and digits only (any script).
pub fn normalise(title: &str) -> String { title.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect() }

/// A suggestion's id: its kind and 8 hex digits of its normalised title.
pub fn suggestion_id(kind: &str, title: &str) -> String {
    use sha2::{Digest, Sha256};
    let h = format!("{:x}", Sha256::digest(normalise(title).as_bytes()));
    format!("{kind}-{}", &h[..8])
}

pub fn valid_suggestion_id(id: &str) -> bool {
    id.split_once('-').map_or(false, |(k, h)| KINDS.contains(&k) && h.len() == 8 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}

/// The request text: control and invisible characters gone except line breaks, cut at `max` characters.
fn clean_block(s: &str, max: usize) -> String {
    let t: String = s.replace("\r\n", "\n").chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .filter(|c| !crate::staging::validate::is_hidden_char(*c)).collect();
    t.trim().chars().take(max).collect()
}

/// The suggestions inside the assistant's answer (which may wrap them in prose or a code block),
/// checked one by one; see the rules in the code.
pub fn parse_suggestions(answer: &str, ctx: &Ctx) -> anyhow::Result<Parsed> {
    let unreadable = || crate::tr::err("automate.unreadable", &[]);
    let (start, end) = (answer.find('{').ok_or_else(unreadable)?, answer.rfind('}').ok_or_else(unreadable)?);
    if end <= start { return Err(unreadable()); }
    let v: Value = serde_json::from_str(&answer[start..=end]).map_err(|_| unreadable())?;
    if !v.is_object() { return Err(unreadable()); }
    let routines: HashSet<String> = ctx.routines.iter().map(|t| normalise(t)).collect();
    let helpers: HashSet<String> = ctx.helpers.iter().map(|t| normalise(t)).collect();
    let mut out: Vec<Suggestion> = vec![];
    for item in v["suggestions"].as_array().into_iter().flatten() {
        let raw = |k: &str| item[k].as_str().unwrap_or_default();
        let kind = raw("kind").trim().to_lowercase();
        if !KINDS.contains(&kind.as_str()) { continue; }
        if kind == "agent" && !helpers_offered(ctx.program) { continue; }
        let s = Suggestion {
            id: String::new(), kind: kind.clone(),
            title: clean(raw("title"), 80), what: clean(raw("what"), 500), why: clean(raw("why"), 500), build: clean(raw("build"), 500),
            request: clean_block(raw("request"), 4000),
            template: item["template"].as_str().map(str::trim).filter(|t| TEMPLATES.iter().any(|(id, k, _)| id == t && *k == kind)).map(String::from),
            people: item["people"] == true,
        };
        if s.title.is_empty() || s.what.is_empty() || normalise(&s.title).is_empty() { continue; }
        // Anything the assistant program would run when it loads it: the item goes.
        if [&s.title, &s.what, &s.why, &s.build, &s.request].iter().any(|t| crate::routines::runs_commands(t)) { continue; }
        let id = suggestion_id(&kind, &s.title);
        if ctx.skip.contains(&id) || out.iter().any(|o| o.id == id) { continue; }
        let n = normalise(&s.title);
        if (kind == "skill" && routines.contains(&n)) || (kind == "agent" && helpers.contains(&n)) { continue; }
        out.push(Suggestion { id, ..s });
        if out.len() == MAX_SUGGESTIONS { break; }
    }
    let nothing = if out.is_empty() { clean(v["nothing"].as_str().unwrap_or_default(), 300) } else { String::new() };
    Ok(Parsed { suggestions: out, nothing })
}

// ---------- building (pure) ----------

/// The app's own "Email writing" routine, in the app's language.
pub fn email_writing(arabic: bool) -> Draft {
    let text = if arabic { include_str!("automations/email-writing.ar.md") } else { include_str!("automations/email-writing.en.md") };
    crate::routines::parse("email-writing", text).0
}

/// A guide for connecting something: what the UI shows, and the unsent chat draft behind its button.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Guide { pub title: String, pub intro: String, pub steps: Vec<String>, pub chat_prompt: String }

/// The app's own guide for connecting an email account, in the app's language.
pub fn email_account(arabic: bool) -> Guide {
    let text = if arabic { include_str!("automations/email-account.ar.json") } else { include_str!("automations/email-account.en.json") };
    serde_json::from_str(text).expect("the email-account guide is valid JSON")
}

/// The guide for a Connection suggestion: the template's, else steps taken from its request.
pub fn guide_for(s: &Suggestion, arabic: bool) -> Guide {
    if s.template.as_deref() == Some("email-account") { return email_account(arabic); }
    let mut steps: Vec<String> = s.request.lines().map(|l| {
        let l = l.trim().trim_start_matches(['-', '*', '•']).trim_start();
        let l = l.trim_start_matches(|c: char| c.is_ascii_digit());
        let l = l.strip_prefix('.').or_else(|| l.strip_prefix(')')).unwrap_or(l);
        clean(l, 300)
    }).filter(|l| !l.is_empty()).take(10).collect();
    if steps.is_empty() { steps.push(s.what.clone()); }
    let chat_prompt = if arabic {
        format!("أرشدني خطوة بخطوة إلى ربط «{}» ببرنامج المساعد لديّ. ما أريده منه: {} اشرح كل خطوة بكلمات بسيطة وانتظرني بعد كل خطوة. سأسجّل الدخول بنفسي من صفحة الخدمة نفسها. لا تطلب مني كلمة المرور أو أي رمز أبداً، ولا تغيّر أي إعدادات أو ملفات بنفسك.", s.title, s.what)
    } else {
        format!("Walk me through connecting \"{}\" to my assistant program, step by step. What I want it for: {} Explain each step in plain words and wait for me after each one. I will do every sign-in myself, on the service's own page. Never ask me for my password or any code, and do not change any settings or files yourself.", s.title, s.what)
    };
    Guide { title: s.title.clone(), intro: s.what.clone(), steps, chat_prompt }
}

/// The customisation request for a New page suggestion.
pub fn tab_prompt(s: &Suggestion) -> String {
    format!("Add a new page to my Thoughts app: \"{}\".\n\nWhat it should do for me: {}\n\nDetails:\n{}\n\n\
        This came from the app's \"Suggest automations\". Build it as a page (or as a Home widget, if that fits better), with only the permissions it really needs. \
        Do not change anything else. In the summary, say in plain words what you made and where to find it.",
        s.title, s.what, if s.request.is_empty() { s.what.as_str() } else { s.request.as_str() })
}

// ---------- state ----------

/// One suggestion turned down or built.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Mark {
    pub id: String,
    pub title: String,
    pub at: String,
    /// A dismissed suggestion itself and where it was in the list, so Undo can put it back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<Suggestion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct LibState {
    pub generated_at: Option<String>,
    pub suggestions: Vec<Suggestion>,
    pub nothing: String,
    pub dismissed: Vec<Mark>,
    pub built: Vec<Mark>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Store { pub libraries: BTreeMap<String, LibState> }

/// Most marks of each sort kept per library (the oldest go).
pub const MAX_MARKS: usize = 200;

pub fn store_file(home: &Path) -> PathBuf { home.join("automations.json") }

pub fn load(home: &Path) -> Store {
    std::fs::read(store_file(home)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(home: &Path, s: &Store) -> anyhow::Result<()> {
    crate::paths::write_atomic(&store_file(home), serde_json::to_string_pretty(s)?.as_bytes(), true)?;
    Ok(())
}

/// One read-change-write of the file at a time.
static FILE: Mutex<()> = Mutex::new(());

/// Change one library's part of the file and return it.
pub fn change<T>(home: &Path, lib: &str, f: impl FnOnce(&mut LibState) -> T) -> anyhow::Result<(T, LibState)> {
    let _one = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load(home);
    let l = s.libraries.entry(lib.to_string()).or_default();
    let out = f(l);
    for marks in [&mut l.dismissed, &mut l.built] {
        if marks.len() > MAX_MARKS { let extra = marks.len() - MAX_MARKS; marks.drain(..extra); }
    }
    let l = l.clone();
    save(home, &s)?;
    Ok((out, l))
}

pub fn library_state(home: &Path, lib: &str) -> LibState { load(home).libraries.get(lib).cloned().unwrap_or_default() }

fn now_text() -> String { Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true) }

/// Take a suggestion out of the list as turned down. False when it is not in the list.
pub fn dismiss(l: &mut LibState, id: &str) -> bool {
    let Some(i) = l.suggestions.iter().position(|s| s.id == id) else { return false };
    let s = l.suggestions.remove(i);
    l.dismissed.retain(|m| m.id != id);
    l.dismissed.push(Mark { id: s.id.clone(), title: s.title.clone(), at: now_text(), item: Some(s), index: Some(i) });
    true
}

/// Undo a dismissal: the suggestion goes back where it was. False when there is nothing to put back.
pub fn restore(l: &mut LibState, id: &str) -> bool {
    let Some(i) = l.dismissed.iter().position(|m| m.id == id) else { return false };
    let m = l.dismissed.remove(i);
    let Some(item) = m.item else { return false };
    if !l.suggestions.iter().any(|s| s.id == item.id) && !l.built.iter().any(|b| b.id == item.id) {
        let at = m.index.unwrap_or(0).min(l.suggestions.len());
        l.suggestions.insert(at, item);
    }
    true
}

/// Take a suggestion out of the list as built. False when it is not in the list (already built, or unknown).
pub fn mark_built(l: &mut LibState, id: &str) -> bool {
    let Some(i) = l.suggestions.iter().position(|s| s.id == id) else { return false };
    let s = l.suggestions.remove(i);
    l.built.retain(|m| m.id != id);
    l.built.push(Mark { id: s.id, title: s.title, at: now_text(), item: None, index: None });
    true
}

/// While a library's suggestions are being made, and how the last attempt failed (kept while the service runs).
#[derive(Clone, Debug, Default)]
struct Run { running: bool, error: Option<String> }

fn runs() -> &'static Mutex<HashMap<String, Run>> {
    static R: std::sync::OnceLock<Mutex<HashMap<String, Run>>> = std::sync::OnceLock::new();
    R.get_or_init(Default::default)
}

fn run_of(lib: &str) -> Run { runs().lock().unwrap_or_else(|e| e.into_inner()).get(lib).cloned().unwrap_or_default() }

/// A library's place as "running", given up however the run ends.
struct Running(String);
impl Drop for Running {
    fn drop(&mut self) { if let Some(r) = runs().lock().unwrap_or_else(|e| e.into_inner()).get_mut(&self.0) { r.running = false; } }
}

// ---------- HTTP ----------

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/automations", get(api_state))
        .route("/api/automations/preview", get(api_preview))
        .route("/api/automations/suggest", post(api_suggest))
        .route("/api/automations/:id/dismiss", post(api_dismiss))
        .route("/api/automations/:id/restore", post(api_restore))
        .route("/api/automations/:id/build", post(api_build))
        .route("/api/automations/:id/built", post(api_built))
}

type R = Result<Json<Value>, crate::api::ApiErr>;

async fn library(st: &AppState) -> anyhow::Result<PathBuf> {
    st.settings.read().await.library().ok_or_else(|| crate::tr::err("library.none", &[]))
}

fn key(lib: &Path) -> String { lib.to_string_lossy().to_string() }

/// Why suggesting cannot start now, if it cannot (`noLibrary`, `spend`, `planLimit`, `signedOut`).
async fn blocked(st: &AppState) -> Option<&'static str> {
    use std::sync::atomic::Ordering;
    let s = st.settings.read().await.clone();
    if s.library().is_none() { return Some("noLibrary"); }
    if crate::spend::reached(st).await { return Some("spend"); }
    if crate::spend::plan_limit_json(st)["reached"] == true { return Some("planLimit"); }
    let out = match crate::assistant::primary(&s) { Program::Claude => st.auth_failed.load(Ordering::Relaxed), Program::Codex => st.codex.auth_failed.load(Ordering::Relaxed) };
    out.then_some("signedOut")
}

/// `{running, error, generatedAt, suggestions, nothing, program, canSuggest, blocked}`.
pub async fn state_json(st: &AppState) -> Value {
    let program = crate::assistant::primary(&*st.settings.read().await);
    let blocked = blocked(st).await;
    let (l, run) = match st.settings.read().await.library() {
        Some(lib) => (library_state(&crate::paths::home(), &key(&lib)), run_of(&key(&lib))),
        None => (LibState::default(), Run::default()),
    };
    json!({
        "running": run.running, "error": run.error, "generatedAt": l.generated_at, "suggestions": l.suggestions, "nothing": l.nothing,
        "program": program.name(), "canSuggest": blocked.is_none() && !run.running, "blocked": blocked,
    })
}

async fn api_state(State(st): State<Arc<AppState>>) -> R { Ok(Json(state_json(&st).await)) }

/// `{text}`: exactly the summary a suggestion run would send now.
async fn api_preview(State(st): State<Arc<AppState>>) -> R {
    let lib = library(&st).await?;
    let facts = gather(&st, &lib).await;
    Ok(Json(json!({"text": render_summary(&summarise(&facts, Utc::now()))})))
}

/// What is already there, for the prompt, and what to compare the answer with.
fn existing(f: &Facts, l: &LibState) -> Vec<String> {
    let mut v: Vec<String> = f.routines.iter().map(|(t, _)| format!("Routine: {t}")).collect();
    v.extend(f.helpers.iter().map(|(t, _)| format!("Helper: {t}")));
    v.extend(f.tasks.iter().map(|(t, _)| format!("Automatic task: {t}")));
    v.extend(f.addons.iter().map(|t| format!("Add-on: {t}")));
    v.extend(f.pages.iter().map(|t| format!("Page: {t}")));
    v.extend(l.built.iter().rev().take(50).map(|m| format!("Built from an earlier suggestion: {}", m.title)));
    v
}

/// Ask for suggestions and store what comes back. The answer's text is never logged.
async fn suggest_now(st: &AppState, lib: &Path) -> anyhow::Result<usize> {
    let facts = gather(st, lib).await;
    let summary = summarise(&facts, Utc::now());
    let text = render_summary(&summary);
    let home = crate::paths::home();
    let l = library_state(&home, &key(lib));
    let dismissed: Vec<String> = l.dismissed.iter().rev().take(100).map(|m| m.title.clone()).collect();
    let prompt = suggest_prompt(&text, &dismissed, &existing(&facts, &l), facts.program, facts.arabic);
    let model = crate::assistant::one_shot_model(&*st.settings.read().await, false);
    let answer = crate::claude::complete(st, &prompt, model).await;
    crate::spend::after_cost(st).await;
    let answer = answer?;
    // Compared with the file as it is now: something dismissed or built while the assistant thought stays so.
    let l = library_state(&home, &key(lib));
    let skip: HashSet<String> = l.dismissed.iter().chain(l.built.iter()).map(|m| m.id.clone()).collect();
    let routines: Vec<String> = facts.routines.iter().map(|(t, _)| t.clone()).collect();
    let helpers: Vec<String> = facts.helpers.iter().map(|(t, _)| t.clone()).collect();
    let parsed = parse_suggestions(&answer, &Ctx { program: facts.program, skip: &skip, routines: &routines, helpers: &helpers })?;
    let n = parsed.suggestions.len();
    change(&home, &key(lib), |l| { l.generated_at = Some(now_text()); l.suggestions = parsed.suggestions; l.nothing = parsed.nothing; })?;
    crate::log::info(format!("automations: {n} suggestion(s) from a summary of {} chat(s), {} character(s)", summary.chats90, text.chars().count()));
    Ok(n)
}

/// `{running: true}`: suggestions are being made in the background; the result arrives with an
/// `automations.changed` event. One run per library at a time; never past a spending or plan limit.
async fn api_suggest(State(st): State<Arc<AppState>>) -> R {
    let lib = library(&st).await?;
    if crate::spend::reached(&st).await { return Err(crate::tr::err("automate.spend", &[]).into()); }
    if crate::spend::plan_limit_json(&st)["reached"] == true { return Err(crate::tr::err("plan.limit_unknown", &[]).into()); }
    // The stand-in for the assistant in tests needs no sign-in.
    if st.complete_hook.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
        let program = crate::assistant::primary(&*st.settings.read().await);
        crate::assistant::require_signed_in(&st, program).await?;
    }
    let k = key(&lib);
    {
        let mut r = runs().lock().unwrap_or_else(|e| e.into_inner());
        let run = r.entry(k.clone()).or_default();
        if run.running { return Err(crate::tr::err("automate.running", &[]).into()); }
        *run = Run { running: true, error: None };
    }
    let guard = Running(k.clone());
    let st2 = st.clone();
    tokio::spawn(async move {
        let _guard = guard;
        let result = suggest_now(&st2, &lib).await;
        if let Err(e) = &result {
            crate::log::warn("automations: no suggestions this time (the error is shown in the app)");
            let msg = crate::tr::explain(e).0.message;
            if let Some(r) = runs().lock().unwrap_or_else(|e| e.into_inner()).get_mut(&k) { r.error = Some(msg); }
        }
        drop(_guard);
        st2.emit(json!({"type": "automations.changed"}));
    });
    Ok(Json(json!({"running": true})))
}

fn checked(id: &str) -> anyhow::Result<()> {
    if valid_suggestion_id(id) { Ok(()) } else { Err(crate::tr::err("automate.gone", &[])) }
}

async fn api_dismiss(State(st): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> R {
    checked(&id)?;
    let lib = library(&st).await?;
    change(&crate::paths::home(), &key(&lib), |l| dismiss(l, &id))?;
    st.emit(json!({"type": "automations.changed"}));
    Ok(Json(state_json(&st).await))
}

async fn api_restore(State(st): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> R {
    checked(&id)?;
    let lib = library(&st).await?;
    change(&crate::paths::home(), &key(&lib), |l| restore(l, &id))?;
    st.emit(json!({"type": "automations.changed"}));
    Ok(Json(state_json(&st).await))
}

/// The user kept the routine or helper, or closed the guide: the suggestion leaves the list for good.
async fn api_built(State(st): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> R {
    checked(&id)?;
    let lib = library(&st).await?;
    change(&crate::paths::home(), &key(&lib), |l| mark_built(l, &id))?;
    st.emit(json!({"type": "automations.changed"}));
    Ok(Json(state_json(&st).await))
}

/// What the UI needs to build a suggestion, per kind (see the module notes). Only a New page is
/// built here (a customisation chat is started, and the suggestion is marked built once it has
/// started); the others are marked built by the UI once the user kept what they reviewed.
async fn api_build(State(st): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> R {
    checked(&id)?;
    let lib = library(&st).await?;
    let home = crate::paths::home();
    let s = library_state(&home, &key(&lib)).suggestions.into_iter().find(|s| s.id == id).ok_or_else(|| crate::tr::err("automate.gone", &[]))?;
    let arabic = st.settings.read().await.language == "ar";
    let base = json!({"id": s.id, "kind": s.kind, "title": s.title});
    let mut out = match s.kind.as_str() {
        "tab" => {
            let chat = {
                let _g = crate::layer_api::LAYER_LOCK.lock().await;
                crate::api::start_customize(st.clone(), tab_prompt(&s), "automate", Some(s.title.clone())).await?
            };
            change(&home, &key(&lib), |l| mark_built(l, &id))?;
            st.emit(json!({"type": "automations.changed"}));
            json!({"chat": chat})
        }
        "skill" => match s.template.as_deref() {
            Some("email-writing") => json!({"draft": email_writing(arabic), "template": "email-writing", "people": s.people}),
            _ => json!({"request": s.request, "what": s.what, "people": s.people}),
        },
        "agent" => json!({"request": s.request, "what": s.what}),
        "integration" => json!({"guide": guide_for(&s, arabic)}),
        _ => return Err(crate::tr::err("automate.gone", &[]).into()),
    };
    for (k, v) in base.as_object().into_iter().flatten() { out[k.as_str()] = v.clone(); }
    Ok(Json(out))
}

#[cfg(test)]
#[path = "automations_tests.rs"]
mod tests;
