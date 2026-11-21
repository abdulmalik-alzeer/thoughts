//! Diagrams in answers and notes, drawn by Archify (https://github.com/tt-a1i/archify, Apache-2.0).
//!
//! A fenced block with the language `archify` holds one Archify JSON document. The window sends it
//! here; this module runs Archify's own renderer with the Node.js already on the machine and hands
//! back the page it wrote. The window takes the drawing out of that page (it never runs the page's
//! scripts), shows it, and offers it as an SVG and a draw.io file (`ui/src/lib/figures/`).
//!
//! Archify is a Node program, not a library, so a copy travels inside this binary (`vendor/archify`,
//! trimmed to what `render` needs) and is unpacked twice under the app's home:
//! - `archify/<version>/`, the copy that is run. The folder is closed to every assistant run like the
//!   rest of the app's own data (`paths::secret_paths_in`), and before each use every file is
//!   compared with the one in this binary: a copy that was changed is unpacked again, never run.
//! - `extensions/archify-guide/<version>/`, the part an assistant reads to learn the format (the
//!   guide, the examples, the schemas). Runs may read `extensions/` and never write it.
//!
//! What is drawn is the assistant's or a note's own text, so it is treated as hostile: a size limit,
//! a time limit, a bare environment, an empty folder to work in, and the two parts of the format
//! that reach outside the document are taken out first (a `brand` given as an address is fetched
//! from the network by Archify; `meta.repository` reads the folder it runs in).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};

use crate::state::AppState;

static ARCHIFY: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../vendor/archify");

/// The diagram types the app draws (Archify's `explainer` is a whole page, not a diagram).
pub const TYPES: [&str; 10] = ["architecture", "workflow", "sequence", "dataflow", "lifecycle", "erd", "class", "tree", "timeline", "waterfall"];
/// What an assistant reads; the rest is the program.
const GUIDE_PARTS: [&str; 4] = ["GUIDE.md", "examples", "schemas", "references"];
/// Changes whenever the unpacked copies must be replaced though Archify's own version did not change.
const REVISION: &str = "1";
/// One document may be this long (the largest of Archify's own examples is 13 KB).
pub const MAX_SOURCE: usize = 256 * 1024;
/// The page Archify writes is under 1 MB; anything far beyond that is not one of its pages.
const MAX_PAGE: u64 = 4 * 1024 * 1024;
const RENDER_TIMEOUT: Duration = Duration::from_secs(20);
/// Archify needs Node.js 18 or newer.
const MIN_NODE: u32 = 18;

/// The name of the unpacked copies: Archify's version, where it came from, and `REVISION`.
pub fn version() -> String {
    let pkg: Value = ARCHIFY.get_file("package.json").and_then(|f| serde_json::from_slice(f.contents()).ok()).unwrap_or(Value::Null);
    let upstream = ARCHIFY.get_file("UPSTREAM").and_then(|f| f.contents_utf8()).unwrap_or("").trim();
    format!("{}-{}-{REVISION}", pkg["version"].as_str().unwrap_or("0"), upstream.get(..8).unwrap_or("local"))
}

/// Where the copy that is run lives.
pub fn program_dir(home: &Path) -> PathBuf { home.join("archify").join(version()) }
/// Where the copy an assistant reads lives.
pub fn guide_dir(home: &Path) -> PathBuf { home.join("extensions").join("archify-guide").join(version()) }

static UNPACK: Mutex<()> = Mutex::new(());

fn files<'a>(dir: &'a include_dir::Dir<'a>, out: &mut Vec<&'a include_dir::File<'a>>) {
    for e in dir.entries() {
        match e {
            include_dir::DirEntry::Dir(d) => files(d, out),
            include_dir::DirEntry::File(f) => out.push(f),
        }
    }
}

/// The files of one copy: all of them for the program, the reading parts for the guide.
fn wanted(guide: bool) -> Vec<&'static include_dir::File<'static>> {
    let mut all = vec![];
    files(&ARCHIFY, &mut all);
    if guide { all.retain(|f| GUIDE_PARTS.iter().any(|p| f.path().starts_with(p))); }
    all
}

/// Is every folder from `home` down to `dir` a plain folder (not a link)?
fn plain(home: &Path, dir: &Path) -> bool {
    let Ok(rest) = dir.strip_prefix(home) else { return false };
    let mut at = home.to_path_buf();
    for c in rest.components() {
        at.push(c);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_dir() => {}
            _ => return false,
        }
    }
    true
}

/// Does `dir` hold exactly the wanted files, byte for byte, as plain files?
fn intact(home: &Path, dir: &Path, guide: bool) -> bool {
    if !plain(home, dir) { return false; }
    wanted(guide).iter().all(|f| {
        let p = dir.join(f.path());
        std::fs::symlink_metadata(&p).map_or(false, |m| m.file_type().is_file() && m.len() == f.contents().len() as u64)
            && std::fs::read(&p).map_or(false, |have| have == f.contents())
    })
}

/// Unpack one copy to `dir`, replacing whatever is there. It is written beside its place under a
/// passing name and then takes it, so a half-written copy is never used.
fn unpack(home: &Path, dir: &Path, guide: bool) -> std::io::Result<()> {
    let bad = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, what.to_string());
    let parent = dir.parent().ok_or_else(|| bad("no parent"))?;
    // The folders above it: made here, and never a link.
    let mut at = home.to_path_buf();
    for c in parent.strip_prefix(home).map_err(|_| bad("not in the app's folder"))?.components() {
        at.push(c);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_dir() => {}
            Ok(_) => return Err(bad("a folder on the way is a link")),
            Err(_) => crate::paths::create_private_dir(&at)?,
        }
    }
    let part = parent.join(format!(".part-{}", uuid::Uuid::new_v4().simple()));
    crate::paths::create_private_dir(&part)?;
    let write = || -> std::io::Result<()> {
        for f in wanted(guide) {
            let to = part.join(f.path());
            if let Some(p) = to.parent() { std::fs::create_dir_all(p)?; }
            std::fs::write(&to, f.contents())?;
        }
        Ok(())
    };
    if let Err(e) = write() { let _ = std::fs::remove_dir_all(&part); return Err(e); }
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.file_type().is_dir() => std::fs::remove_dir_all(dir)?,
        Ok(_) => std::fs::remove_file(dir)?,
        Err(_) => {}
    }
    if let Err(e) = std::fs::rename(&part, dir) { let _ = std::fs::remove_dir_all(&part); return Err(e); }
    // Copies of earlier versions are no use to anyone.
    if let Ok(rd) = std::fs::read_dir(parent) {
        for e in rd.flatten() {
            let p = e.path();
            if p != dir && std::fs::symlink_metadata(&p).map_or(false, |m| m.is_dir()) { let _ = std::fs::remove_dir_all(&p); }
        }
    }
    Ok(())
}

/// The copy at `dir`, unpacked now when it is missing or differs from what this binary carries.
fn ensure(home: &Path, dir: PathBuf, guide: bool) -> std::io::Result<PathBuf> {
    let _one = UNPACK.lock().unwrap_or_else(|e| e.into_inner());
    if !intact(home, &dir, guide) { unpack(home, &dir, guide)?; }
    Ok(dir)
}

/// The program, ready to run (checked against this binary every time). Blocking.
pub fn program(home: &Path) -> std::io::Result<PathBuf> { ensure(home, program_dir(home), false) }
/// The guide, ready to read. Blocking.
pub fn guide(home: &Path) -> std::io::Result<PathBuf> { ensure(home, guide_dir(home), true) }

/// What every run is told about drawing. `guide`: where the guide is, when it could be unpacked.
pub fn note(guide: Option<&Path>) -> String {
    let read = match guide {
        Some(g) => format!(" Before your first diagram or chart in a conversation, read {} (the formats, and which worked example to copy for each \
            type of diagram); the examples and schemas it names are in the same folder.", g.join("GUIDE.md").display()),
        None => String::new(),
    };
    format!("Diagrams and charts: this app draws them itself, inside your answer and inside notes, and lets the user save them as SVG and \
        draw.io files. For a diagram (architecture, flowchart or workflow, sequence, data flow, state or lifecycle, entity-relationship, class, \
        tree or hierarchy, timeline, waterfall) write a fenced code block with the language `archify` holding one Archify JSON document. For a \
        chart of numbers (bar, line, area, pie, donut, scatter) write a fenced code block with the language `chart` holding one JSON document. \
        Never use Mermaid, PlantUML, Graphviz, ASCII art or raw HTML or SVG for these: they are shown as plain text.{read} Draw only when a \
        picture helps or the user asks for one.")
}

/// `note` for a run starting now: the guide is unpacked first if need be (file work, off the runtime).
pub async fn run_note() -> String {
    let home = crate::paths::home();
    let dir = tokio::task::spawn_blocking(move || guide(&home)).await.ok().and_then(|r| r.map_err(|e| crate::log::warn(format!("the diagram guide could not be unpacked: {e}"))).ok());
    note(dir.as_deref())
}

/// Why a document was not drawn. `code` is what the window translates; `detail` is Archify's own
/// message (English), shown under it when there is one.
#[derive(Debug, PartialEq)]
pub struct Refused { pub code: &'static str, pub detail: String }
fn refused(code: &'static str) -> Refused { Refused { code, detail: String::new() } }

/// The document as it is handed to Archify, with its type: checked to be one of the app's diagram
/// types, given the output name Archify insists on, drawn without motion, and without the two
/// parts that reach outside it.
pub fn prepare(source: &str) -> Result<(String, Value), Refused> {
    if source.len() > MAX_SOURCE { return Err(refused("too-long")); }
    let mut doc: Value = serde_json::from_str(source).map_err(|e| Refused { code: "not-json", detail: e.to_string() })?;
    let Some(obj) = doc.as_object_mut() else { return Err(refused("not-json")) };
    let kind = obj.get("diagram_type").and_then(Value::as_str).unwrap_or("").to_string();
    if !TYPES.contains(&kind.as_str()) { return Err(Refused { code: "unknown-type", detail: kind }); }
    let meta = obj.entry("meta").or_insert_with(|| json!({}));
    if let Some(m) = meta.as_object_mut() {
        m.insert("output".into(), json!("diagram.html"));
        m.insert("animation".into(), json!("none"));
        m.remove("repository");
    }
    // A brand given as an object carries an address Archify would fetch; a plain name is its own list.
    for list in obj.values_mut().filter_map(Value::as_array_mut) {
        for item in list.iter_mut().filter_map(Value::as_object_mut) {
            if item.get("brand").map_or(false, |b| !b.is_string()) { item.remove("brand"); }
        }
    }
    Ok((kind, doc))
}

/// `v22.14.0` → 22.
fn node_major(printed: &str) -> Option<u32> {
    printed.trim().trim_start_matches('v').split('.').next()?.parse().ok()
}

/// The machine's Node.js, when it is new enough. `Err`: "no-node" or "old-node" (with its version).
async fn node() -> Result<PathBuf, Refused> {
    static FOUND: Mutex<Option<PathBuf>> = Mutex::new(None);
    if let Some(p) = FOUND.lock().unwrap_or_else(|e| e.into_inner()).clone().filter(|p| p.is_file()) { return Ok(p); }
    let bin = crate::claude::find_binary(&None, "node").ok_or_else(|| refused("no-node"))?;
    let mut c = crate::proc::command(&bin);
    c.arg("--version").env("PATH", crate::claude::shell_path()).stdin(std::process::Stdio::null());
    let out = crate::proc::output_timeout(&mut c, Duration::from_secs(10)).await.map_err(|_| refused("no-node"))?;
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match node_major(&printed) {
        Some(n) if n >= MIN_NODE => { *FOUND.lock().unwrap_or_else(|e| e.into_inner()) = Some(bin.clone()); Ok(bin) }
        Some(_) => Err(Refused { code: "old-node", detail: printed }),
        None => Err(refused("no-node")),
    }
}

/// What Archify printed, as it is shown under a diagram that was refused: its own lines, without
/// Node's stack traces and without the folders of this machine.
fn tidy(printed: &str, hide: &[&Path]) -> String {
    let mut text = printed.to_string();
    for p in hide { text = text.replace(&p.to_string_lossy().to_string(), "…"); }
    let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty())
        .filter(|l| { let t = l.trim_start(); !(t.starts_with("at ") || t.starts_with("node:") || t.starts_with("Node.js v") || t == "^") })
        .collect();
    let mut out = lines.join("\n");
    if out.len() > 4000 { let mut cut = 4000; while !out.is_char_boundary(cut) { cut -= 1; } out.truncate(cut); out.push('…'); }
    out
}

/// At most this many diagrams are drawn at once (each is a Node process).
static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// Draw one document: the page Archify wrote for it.
pub async fn render(source: &str) -> Result<String, Refused> {
    let (kind, doc) = prepare(source)?;
    let node = node().await?;
    let home = crate::paths::home();
    let h = home.clone();
    let program = tokio::task::spawn_blocking(move || program(&h)).await.ok().and_then(Result::ok).ok_or_else(|| refused("failed"))?;
    let _slot = SLOTS.acquire().await.map_err(|_| refused("failed"))?;
    let work = crate::paths::run_tmp_dir().join(format!("figure-{}", uuid::Uuid::new_v4().simple()));
    let mut doc = doc;
    let made = crate::paths::create_private_dir(&crate::paths::run_tmp_dir()).and_then(|_| crate::paths::create_private_dir(&work));
    if let Err(e) = made { let _ = std::fs::remove_dir_all(&work); return Err(Refused { code: "failed", detail: e.to_string() }); }
    let mut repairs = 0;
    let result = loop {
        if let Err(e) = std::fs::write(work.join("diagram.json"), serde_json::to_vec(&doc).unwrap_or_default()) { break Err(Refused { code: "failed", detail: e.to_string() }); }
        match run(&node, &program, &work, &kind).await {
            // A caption that sits on a box, or a name wider than its box: Archify says how far to
            // move or widen, so that is done and the drawing tried again, a few times at most.
            Err(r) if r.code == "invalid" && repairs < MAX_REPAIRS && repair(&mut doc, &r.detail) => repairs += 1,
            Err(r) => break Err(Refused { code: r.code, detail: if r.code == "invalid" { tidy(&r.detail, &[&work, &program, &home]) } else { r.detail } }),
            Ok(page) => break Ok(page),
        }
    };
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// How often one document is corrected and drawn again.
const MAX_REPAIRS: usize = 3;

/// Apply the corrections Archify itself suggests for an architecture diagram's layout, as it
/// printed them: `Label "SQL" overlaps component … Suggested fix: … set labelDy 24 (below)` moves
/// that caption, `widen component "worker" to at least 124px` widens that box. True when
/// something was changed (the document is then worth drawing again). Nothing else is touched: a
/// document that is wrong in what it says is shown with its message, not guessed at.
pub fn repair(doc: &mut Value, printed: &str) -> bool {
    use std::sync::OnceLock;
    static MOVE: OnceLock<regex::Regex> = OnceLock::new();
    static WIDEN: OnceLock<regex::Regex> = OnceLock::new();
    let mv = MOVE.get_or_init(|| regex::Regex::new(r#"(?m)^- Label "([^"\n]+)" overlaps[^\n]*\n(?:[^\n]*\n){0,3}?\s*Suggested fix:[^\n]*?set labelDy (-?\d{1,4})\b"#).unwrap());
    let wd = WIDEN.get_or_init(|| regex::Regex::new(r#"widen component "([^"\n]+)" to at least (\d{2,4})px"#).unwrap());
    if doc["diagram_type"] != "architecture" { return false; }
    let mut changed = false;
    for c in mv.captures_iter(printed) {
        let (label, dy) = (&c[1], c[2].parse::<i64>().unwrap_or(0));
        for conn in doc["connections"].as_array_mut().into_iter().flatten().filter_map(Value::as_object_mut) {
            if conn.get("label").and_then(Value::as_str) == Some(label) && conn.get("labelDy").and_then(Value::as_i64) != Some(dy) && !conn.contains_key("labelAt") {
                conn.insert("labelDy".into(), json!(dy));
                changed = true;
            }
        }
    }
    for c in wd.captures_iter(printed) {
        let (id, w) = (&c[1], c[2].parse::<i64>().unwrap_or(0).min(400) + 8);
        for comp in doc["components"].as_array_mut().into_iter().flatten().filter_map(Value::as_object_mut) {
            if comp.get("id").and_then(Value::as_str) != Some(id) { continue; }
            let h = comp.get("size").and_then(|s| s.get(1)).and_then(Value::as_i64).unwrap_or(60);
            if comp.get("size").and_then(|s| s.get(0)).and_then(Value::as_i64).map_or(true, |have| have < w) {
                comp.insert("size".into(), json!([w, h]));
                changed = true;
            }
        }
    }
    changed
}

/// One try. A refusal of the document comes back with what Archify printed, as printed.
async fn run(node: &Path, program: &Path, work: &Path, kind: &str) -> Result<String, Refused> {
    let mut c = crate::proc::command(node);
    c.arg(program.join("bin").join("archify.mjs")).args(["render", kind, "diagram.json", "diagram.html"])
        .current_dir(work).env_clear()
        .env("PATH", crate::claude::shell_path()).env("HOME", work).env("USERPROFILE", work).env("TMPDIR", work).env("TEMP", work).env("TMP", work)
        // Archify may ask its site whether a newer version exists: never from here.
        .env("ARCHIFY_UPDATE_CHECK_DISABLED", "1").env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    for k in ["SystemRoot", "SYSTEMROOT", "windir", "PATHEXT"] { if let Ok(v) = std::env::var(k) { c.env(k, v); } }
    let out = match crate::proc::output_timeout(&mut c, RENDER_TIMEOUT).await {
        Ok(o) => o,
        Err(e) if crate::proc::timed_out(&e) => return Err(refused("timeout")),
        Err(e) => return Err(Refused { code: "failed", detail: e.to_string() }),
    };
    if !out.status.success() {
        return Err(Refused { code: "invalid", detail: String::from_utf8_lossy(&out.stderr).to_string() });
    }
    let page = work.join("diagram.html");
    match std::fs::symlink_metadata(&page) {
        Ok(m) if m.file_type().is_file() && m.len() <= MAX_PAGE => {}
        _ => return Err(refused("failed")),
    }
    std::fs::read_to_string(&page).map_err(|e| Refused { code: "failed", detail: e.to_string() })
}

#[derive(serde::Deserialize)]
struct DrawBody { source: String }

/// `POST /api/figures/archify {source}` → `{ok: true, html}` or `{ok: false, code, detail}`. A
/// document that cannot be drawn is an answer, not an error: the window shows why under the block.
async fn draw(State(_st): State<Arc<AppState>>, Json(b): Json<DrawBody>) -> Json<Value> {
    match render(&b.source).await {
        Ok(html) => Json(json!({"ok": true, "html": html})),
        Err(r) => Json(json!({"ok": false, "code": r.code, "detail": r.detail})),
    }
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/figures/archify", post(draw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-figures-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        crate::paths::canonical(&d).unwrap()
    }

    #[test]
    fn the_copy_carried_is_the_program_and_its_guide() {
        for f in ["bin/archify.mjs", "package.json", "LICENSE", "NOTICE", "GUIDE.md", "UPSTREAM", "schemas/architecture.schema.json"] {
            assert!(ARCHIFY.get_file(f).is_some(), "{f} is carried");
        }
        // Every type the app draws has a schema, and the guide names a worked example that exists.
        let guide = ARCHIFY.get_file("GUIDE.md").unwrap().contents_utf8().unwrap();
        for t in TYPES {
            assert!(ARCHIFY.get_file(format!("schemas/{t}.schema.json")).is_some(), "{t} has a schema");
            let named = guide.lines().find(|l| l.starts_with(&format!("| `{t}`"))).unwrap_or_else(|| panic!("the guide lists {t}"));
            let example = named.rsplit('`').nth(1).unwrap();
            assert!(example.ends_with(&format!(".{t}.json")) && ARCHIFY.get_file(example).is_some(), "{example} exists");
        }
        assert!(version().starts_with("3."), "{}", version());
    }

    #[test]
    fn a_copy_is_unpacked_once_and_again_when_it_was_changed() {
        let home = tmp();
        let dir = program(&home).unwrap();
        assert_eq!(dir, program_dir(&home));
        assert!(intact(&home, &dir, false));
        let main = dir.join("bin/archify.mjs");
        let before = std::fs::metadata(&main).unwrap().modified().unwrap();
        // Untouched: left as it is.
        assert_eq!(program(&home).unwrap(), dir);
        assert_eq!(std::fs::metadata(&main).unwrap().modified().unwrap(), before);
        // Changed (same length, other bytes; or an extra line): never run as found.
        let mut bytes = std::fs::read(&main).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&main, &bytes).unwrap();
        assert!(!intact(&home, &dir, false));
        program(&home).unwrap();
        assert_eq!(std::fs::read(&main).unwrap(), ARCHIFY.get_file("bin/archify.mjs").unwrap().contents());
        // A file that went missing comes back.
        std::fs::remove_file(dir.join("package.json")).unwrap();
        program(&home).unwrap();
        assert!(intact(&home, &dir, false));
        // A copy of an earlier version beside it goes.
        let old = dir.parent().unwrap().join("0.0.1-old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::remove_file(dir.join("LICENSE")).unwrap();
        program(&home).unwrap();
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_the_copy_is_replaced_not_followed() {
        let home = tmp();
        let elsewhere = tmp();
        std::fs::write(elsewhere.join("keep.txt"), "mine").unwrap();
        std::fs::create_dir_all(program_dir(&home).parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, program_dir(&home)).unwrap();
        let dir = program(&home).unwrap();
        assert!(std::fs::symlink_metadata(&dir).unwrap().file_type().is_dir());
        assert!(elsewhere.join("keep.txt").exists() && !elsewhere.join("bin").exists(), "nothing was written through the link");
        // A link higher up is refused outright.
        let home2 = tmp();
        std::os::unix::fs::symlink(&elsewhere, home2.join("archify")).unwrap();
        assert!(program(&home2).is_err());
        for d in [home, home2, elsewhere] { let _ = std::fs::remove_dir_all(d); }
    }

    #[test]
    fn the_guide_copy_holds_only_what_is_read() {
        let home = tmp();
        let dir = guide(&home).unwrap();
        assert!(dir.starts_with(home.join("extensions")));
        assert!(dir.join("GUIDE.md").is_file() && dir.join("examples/orders.erd.json").is_file() && dir.join("schemas/erd.schema.json").is_file());
        assert!(!dir.join("bin").exists() && !dir.join("renderers").exists());
        let n = note(Some(&dir));
        assert!(n.contains("`archify`") && n.contains("`chart`") && n.contains(&dir.join("GUIDE.md").display().to_string()));
        assert!(!note(None).contains("GUIDE.md"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_document_is_prepared_before_it_is_drawn() {
        let src = r#"{"schema_version":1,"diagram_type":"architecture","meta":{"title":"T","output":"../../x.html","animation":"trace","repository":{"root":"/"}},
            "components":[{"id":"a","brand":{"url":"http://169.254.169.254/","sha256":"0"}},{"id":"b","brand":"redis"}],"connections":[]}"#;
        let (kind, doc) = prepare(src).unwrap();
        assert_eq!(kind, "architecture");
        assert_eq!(doc["meta"]["output"], "diagram.html");
        assert_eq!(doc["meta"]["animation"], "none");
        assert!(doc["meta"].get("repository").is_none());
        assert!(doc["components"][0].get("brand").is_none(), "a brand with an address is not fetched");
        assert_eq!(doc["components"][1]["brand"], "redis");
        // No meta at all: the output name is still there (Archify then says what else is missing).
        assert_eq!(prepare(r#"{"diagram_type":"tree"}"#).unwrap().1["meta"]["output"], "diagram.html");
        assert_eq!(prepare("not json").unwrap_err().code, "not-json");
        assert_eq!(prepare("[1]").unwrap_err().code, "not-json");
        assert_eq!(prepare(r#"{"diagram_type":"explainer"}"#).unwrap_err(), Refused { code: "unknown-type", detail: "explainer".into() });
        assert_eq!(prepare(r#"{"diagram_type":"../bin"}"#).unwrap_err().code, "unknown-type");
        assert_eq!(prepare(&format!("{{\"diagram_type\":\"tree\",\"x\":\"{}\"}}", "a".repeat(MAX_SOURCE))).unwrap_err().code, "too-long");
    }

    #[test]
    fn what_archify_printed_is_tidied() {
        assert_eq!(node_major("v22.14.0\n"), Some(22));
        assert_eq!(node_major("16.3.0"), Some(16));
        assert_eq!(node_major("nonsense"), None);
        let work = Path::new("/home/u/.thoughts/run-tmp/figure-1");
        let printed = "architecture schema validation failed:\n  /components/0/type must be one of the allowed values\n\n    at file:///x/y.mjs:4:65\nnode:fs:442\n        ^\nError: ENOENT, open '/home/u/.thoughts/run-tmp/figure-1/diagram.json'\nNode.js v22.14.0\n";
        let t = tidy(printed, &[work]);
        assert_eq!(t, "architecture schema validation failed:\n  /components/0/type must be one of the allowed values\nError: ENOENT, open '…/diagram.json'");
        assert!(tidy(&"é".repeat(5000), &[]).ends_with('…'));
    }

    #[test]
    fn a_caption_on_a_box_and_a_name_too_wide_are_corrected_as_archify_suggests() {
        let printed = "Architecture layout validation failed:\n- Label \"Notifications worker\" (~132px) is wider than component \"worker\" (120px) — shorten the label or widen size.\n- Label \"SQL\" overlaps component \"orders\" — adjust labelDx/labelDy/labelSegment or set labelAt.\n  label rect: [530, 230, 30, 14]\n  component \"orders\" rect: [480, 190, 130, 60]\n  Suggested fix: set labelAt [545, 264] or set labelDy 24 (below); or set labelAt [545, 182] or set labelDy -58 (above)\n[architecture/component-label-overflow] Fix: shorten component \"worker\" label; or widen component \"worker\" to at least 124px.\n";
        let mut doc = json!({"diagram_type": "architecture", "components": [{"id": "worker", "label": "Notifications worker"}, {"id": "orders", "size": [130, 70]}],
            "connections": [{"id": "a", "label": "SQL"}, {"id": "b", "label": "REST"}, {"id": "c", "label": "SQL", "labelAt": [1, 2]}]});
        assert!(repair(&mut doc, printed));
        assert_eq!(doc["connections"][0]["labelDy"], 24);
        assert!(doc["connections"][1].get("labelDy").is_none() && doc["connections"][2].get("labelDy").is_none());
        assert_eq!(doc["components"][0]["size"], json!([132, 60]));
        assert_eq!(doc["components"][1]["size"], json!([130, 70]));
        // The same advice a second time changes nothing more, so the tries end.
        assert!(!repair(&mut doc, printed));
        // Other kinds of diagram, and other messages, are left alone.
        assert!(!repair(&mut json!({"diagram_type": "tree", "connections": [{"label": "SQL"}]}), printed));
        assert!(!repair(&mut doc, "architecture schema validation failed:\n  /components/0/type must be equal to one of the allowed values"));
    }

    /// The whole way, with the Node.js of this machine (skipped where there is none): every worked
    /// example the guide names is drawn, and a document with a mistake comes back with Archify's words.
    #[tokio::test]
    async fn the_examples_are_drawn_with_this_machines_node() {
        if node().await.is_err() { eprintln!("no Node.js 18+ here: skipped"); return; }
        let guide = ARCHIFY.get_file("GUIDE.md").unwrap().contents_utf8().unwrap();
        for t in TYPES {
            let named = guide.lines().find(|l| l.starts_with(&format!("| `{t}`"))).unwrap();
            let example = ARCHIFY.get_file(named.rsplit('`').nth(1).unwrap()).unwrap().contents_utf8().unwrap();
            let html = render(example).await.unwrap_or_else(|r| panic!("{t}: {} {}", r.code, r.detail));
            assert!(html.contains("<svg") && html.contains("data-node-id"), "{t} is drawn");
        }
        let bad = ARCHIFY.get_file("examples/web-app.architecture.json").unwrap().contents_utf8().unwrap().replacen("\"external\"", "\"nonsense\"", 1);
        let r = render(&bad).await.unwrap_err();
        assert_eq!(r.code, "invalid");
        assert!(r.detail.contains("allowed values") && !r.detail.contains("run-tmp"), "{}", r.detail);
        // A caption that lands on a box is moved as Archify suggests, and the diagram is drawn.
        let near = r#"{"schema_version":1,"diagram_type":"architecture","meta":{"title":"Small SaaS"},"layout":{"mode":"grid","gapX":110,"gapY":70},
            "components":[{"id":"gateway","type":"backend","label":"API gateway","row":0,"col":0},{"id":"auth","type":"security","label":"Auth service","row":1,"col":0},
              {"id":"orders","type":"backend","label":"Orders","row":0,"col":1}],
            "connections":[{"id":"a","from":"gateway","to":"auth","label":"verify","variant":"security"},{"id":"b","from":"gateway","to":"orders","label":"REST"}]}"#;
        assert!(render(near).await.unwrap_or_else(|r| panic!("{} {}", r.code, r.detail)).contains("data-node-id"));
        // Nothing is left behind.
        let left: Vec<_> = std::fs::read_dir(crate::paths::run_tmp_dir()).map(|d| d.flatten().collect()).unwrap_or_default();
        assert!(left.is_empty(), "{left:?}");
    }
}
