//! MADO: nothing a run wrote is trusted. Every file of `mado/` a run may write is checked here
//! before it is drawn, reviewed or built from. Problems are worded for the assistant (English), so
//! the repair turn can name exactly what is wrong; the user sees them under a plain heading.
//! (The design tokens are `mado_design`'s to check; the mock-ups are cleaned when they are read.)

use crate::mado::{GraphDoc, PlanDoc, Pos};
use serde_json::Value;
use std::collections::HashSet;

pub const MAX_NODES: usize = 400;
pub const MAX_EDGES: usize = 1200;
pub const MAX_TASKS: usize = 400;
const MAX_LABEL: usize = 300;
const MAX_LIST: usize = 100;
/// The most problems listed for one file (the rest are summed up).
const MAX_PROBLEMS: usize = 40;

/// Graph, task, screen and style ids: `^[a-z0-9][a-z0-9-]{0,63}$`.
pub fn id_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 64 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Feature ids: `^F[0-9]{1,3}$`.
pub fn feature_ok(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=4).contains(&b.len()) && b[0] == b'F' && b[1..].iter().all(u8::is_ascii_digit)
}

pub fn pos_ok(p: &Pos) -> bool { p.x.is_finite() && p.y.is_finite() && p.x.abs() <= 1e6 && p.y.abs() <= 1e6 }

fn capped(mut v: Vec<String>) -> Vec<String> {
    if v.len() > MAX_PROBLEMS { let more = v.len() - MAX_PROBLEMS; v.truncate(MAX_PROBLEMS); v.push(format!("…and {more} more problems of the same kind")); }
    v
}

fn text_ok(s: &str, max: usize) -> bool { !s.trim().is_empty() && s.chars().count() <= max && !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') }

/// A diagram of `kind` (`flow`, `architecture`, `erd`), parsed and checked.
pub fn graph(bytes: &[u8], kind: &str) -> Result<GraphDoc, Vec<String>> {
    if bytes.len() > crate::mado::DOC_MAX { return Err(vec!["the file is larger than 1 MB".into()]); }
    let g: GraphDoc = serde_json::from_slice(bytes).map_err(|e| vec![format!("not valid GraphDoc JSON: {e}")])?;
    let mut p = vec![];
    if g.version != 1 { p.push(format!("\"version\" must be 1, not {}", g.version)); }
    if g.kind != kind { p.push(format!("\"kind\" must be \"{kind}\", not \"{}\"", g.kind)); }
    if !text_ok(&g.title, 200) { p.push("\"title\" must be a short non-empty text".into()); }
    if g.nodes.len() > MAX_NODES { p.push(format!("{} nodes: at most {MAX_NODES} are allowed", g.nodes.len())); }
    if g.edges.len() > MAX_EDGES { p.push(format!("{} edges: at most {MAX_EDGES} are allowed", g.edges.len())); }
    let mut ids = HashSet::new();
    for (i, n) in g.nodes.iter().enumerate() {
        if !id_ok(&n.id) { p.push(format!("nodes[{i}].id \"{}\" must match ^[a-z0-9][a-z0-9-]{{0,63}}$", n.id)); }
        if !ids.insert(n.id.as_str()) { p.push(format!("node id \"{}\" is used twice", n.id)); }
        if !text_ok(&n.label, MAX_LABEL) { p.push(format!("node \"{}\" needs a non-empty \"label\" (at most {MAX_LABEL} characters)", n.id)); }
        if !id_ok(&n.kind) { p.push(format!("node \"{}\" has kind \"{}\": a kind is a short lowercase word (letters, digits, dashes)", n.id, n.kind)); }
        if let Some(pos) = &n.pos { if !pos_ok(pos) { p.push(format!("node \"{}\" has a position that is not a pair of finite numbers", n.id)); } }
        if kind == "erd" { p.extend(erd_fields(&n.id, n.props.as_ref())); }
    }
    let mut eids = HashSet::new();
    for (i, e) in g.edges.iter().enumerate() {
        if !id_ok(&e.id) { p.push(format!("edges[{i}].id \"{}\" must match ^[a-z0-9][a-z0-9-]{{0,63}}$", e.id)); }
        if !eids.insert(e.id.as_str()) { p.push(format!("edge id \"{}\" is used twice", e.id)); }
        for (end, v) in [("from", &e.from), ("to", &e.to)] {
            if !ids.contains(v.as_str()) { p.push(format!("edge \"{}\": \"{end}\" names \"{v}\", which is not a node", e.id)); }
        }
        if let Some(l) = &e.label { if l.chars().count() > MAX_LABEL { p.push(format!("edge \"{}\" has a label longer than {MAX_LABEL} characters", e.id)); } }
        if kind == "erd" && !matches!(e.kind.as_deref(), Some("1-1" | "1-n" | "n-n")) {
            p.push(format!("relation \"{}\" needs \"kind\": \"1-1\", \"1-n\" or \"n-n\"", e.id));
        }
    }
    if kind == "flow" && !g.nodes.iter().any(|n| n.kind == "screen") { p.push("the flow has no node of kind \"screen\"".into()); }
    if p.is_empty() { Ok(g) } else { Err(capped(p)) }
}

fn erd_fields(id: &str, props: Option<&serde_json::Map<String, Value>>) -> Vec<String> {
    let Some(fields) = props.and_then(|p| p.get("fields")) else { return vec![format!("entity \"{id}\" needs props.fields: [{{name, type, key?, note?}}]")] };
    let Some(list) = fields.as_array() else { return vec![format!("entity \"{id}\": props.fields must be a list")] };
    let mut p = vec![];
    if list.is_empty() { p.push(format!("entity \"{id}\" has no fields")); }
    for (i, f) in list.iter().enumerate() {
        let (name, ty) = (f["name"].as_str().unwrap_or_default(), f["type"].as_str().unwrap_or_default());
        if !text_ok(name, 100) || !text_ok(ty, 100) { p.push(format!("entity \"{id}\": fields[{i}] needs a \"name\" and a \"type\"")); }
        if !f["key"].is_null() && !matches!(f["key"].as_str(), Some("pk" | "fk")) { p.push(format!("entity \"{id}\": fields[{i}].key must be \"pk\" or \"fk\"")); }
        if !f["note"].is_null() && !f["note"].is_string() { p.push(format!("entity \"{id}\": fields[{i}].note must be text")); }
    }
    p
}

/// Is `path` a usable entry of `owns` (project-relative, inside the project, not MADO's or git's)?
pub fn owned_path_ok(path: &str) -> bool {
    let p = path.trim();
    !p.is_empty() && p.len() <= 300 && p == path && !p.starts_with('/') && !p.starts_with('\\') && !p.contains('\\') && !p.contains(':')
        && !p.split('/').any(|s| s == ".." || s == "." || s.is_empty())
        && !p.chars().any(char::is_control)
        && !matches!(p.split('/').next().map(str::to_ascii_lowercase).as_deref(), Some("mado" | ".git"))
}

/// Is an acceptance command one plain command, safe to become a permission rule that allows exactly
/// it? No rule or pattern syntax (`*`, `?`, `[`, `]`, `(`, `)`, a trailing `:*`) and no shell syntax
/// that runs more than one command or redirects (`;`, `|`, `&`, backticks, `$`, `>`, `<`, `\`,
/// line breaks, `{`, `}`, `!`, `#`). One command per entry: several are separate entries.
pub fn command_ok(c: &str) -> bool {
    let c = c.trim();
    !c.is_empty() && c.len() <= 1000 && !c.chars().any(|ch| ch.is_control() || "*?[]();|&`$><\\{}!#".contains(ch))
}

/// The plan, parsed and checked for its shape (dependencies, traceability and the like are the
/// review's: `mado_review`).
pub fn plan(bytes: &[u8]) -> Result<PlanDoc, Vec<String>> {
    if bytes.len() > crate::mado::DOC_MAX { return Err(vec!["the file is larger than 1 MB".into()]); }
    let plan: PlanDoc = serde_json::from_slice(bytes).map_err(|e| vec![format!("not valid PlanDoc JSON: {e}")])?;
    let mut p = vec![];
    if plan.version != 1 { p.push(format!("\"version\" must be 1, not {}", plan.version)); }
    if plan.tasks.is_empty() { p.push("the plan has no tasks".into()); }
    if plan.tasks.len() > MAX_TASKS { p.push(format!("{} tasks: at most {MAX_TASKS} are allowed", plan.tasks.len())); }
    let setup = plan.setup.as_deref().unwrap_or_default();
    if setup.len() > MAX_LIST { p.push(format!("\"setup\" has more than {MAX_LIST} entries")); }
    if setup.iter().any(|c| c.trim().is_empty() || c.len() > 1000 || c.contains(['\n', '\r'])) { p.push("every \"setup\" command is one non-empty line (at most 1000 characters)".into()); }
    let mut ids = HashSet::new();
    for (i, t) in plan.tasks.iter().enumerate() {
        if !id_ok(&t.id) { p.push(format!("tasks[{i}].id \"{}\" must match ^[a-z0-9][a-z0-9-]{{0,63}}$", t.id)); }
        if !ids.insert(t.id.as_str()) { p.push(format!("task id \"{}\" is used twice", t.id)); }
        if !text_ok(&t.title, MAX_LABEL) { p.push(format!("task \"{}\" needs a non-empty \"title\"", t.id)); }
        if t.description.trim().is_empty() { p.push(format!("task \"{}\" needs a \"description\"", t.id)); }
        if t.description.len() > 20_000 { p.push(format!("task \"{}\" has a description longer than 20000 characters", t.id)); }
        if !feature_ok(&t.feature) { p.push(format!("task \"{}\": \"feature\" \"{}\" must be a feature id like F1", t.id, t.feature)); }
        for (name, list) in [("owns", &t.owns), ("dependsOn", &t.depends_on), ("acceptance", &t.acceptance)] {
            if list.len() > MAX_LIST { p.push(format!("task \"{}\": \"{name}\" has more than {MAX_LIST} entries", t.id)); }
        }
        for d in &t.depends_on { if !id_ok(d) { p.push(format!("task \"{}\": dependsOn \"{d}\" is not a task id", t.id)); } }
        for c in &t.acceptance {
            if c.trim().is_empty() || c.len() > 1000 || c.contains(['\n', '\r']) { p.push(format!("task \"{}\": every acceptance command is one non-empty line (at most 1000 characters)", t.id)); break; }
        }
        for s in t.screens.iter().flatten() { if !id_ok(s) { p.push(format!("task \"{}\": screen \"{s}\" is not a screen id", t.id)); } }
    }
    if p.is_empty() { Ok(plan) } else { Err(capped(p)) }
}

/// The features of a spec, from its headings `### F<n> — <name>`, in order.
pub fn features(spec: &str) -> Vec<(String, String)> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"^#{2,4}\s*(F[0-9]{1,3})\s*(?:[—–:\-]+|\.)\s*(.+?)\s*#*\s*$").unwrap());
    let mut out: Vec<(String, String)> = vec![];
    let mut fenced = false;
    for line in spec.lines() {
        if line.trim_start().starts_with("```") { fenced = !fenced; continue; }
        if fenced { continue; }
        if let Some(c) = re.captures(line.trim_end()) {
            out.push((c[1].to_string(), c[2].to_string()));
        }
    }
    out
}

fn doc(rel: &str, bytes: &[u8]) -> Vec<String> {
    let mut p = vec![];
    let Ok(text) = std::str::from_utf8(bytes) else { return vec!["the file is not UTF-8 text".into()] };
    if text.trim().is_empty() { p.push("the file is empty".into()); }
    if rel == "spec.md" {
        let f = features(text);
        if f.is_empty() { p.push("no feature headings: every feature is a heading \"### F1 — Name\"".into()); }
        let mut seen = HashSet::new();
        for (id, _) in &f { if !seen.insert(id.clone()) { p.push(format!("feature {id} has two headings")); } }
    }
    p
}

fn styles(bytes: &[u8]) -> Vec<String> {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else { return vec!["not valid JSON".into()] };
    let mut p = vec![];
    if v["version"] != 1 { p.push("\"version\" must be 1".into()); }
    let Some(list) = v["styles"].as_array() else { p.push("\"styles\" must be a list".into()); return p };
    if !(3..=4).contains(&list.len()) { p.push(format!("{} style candidates: write 3 or 4", list.len())); }
    let mut seen = HashSet::new();
    for (i, s) in list.iter().enumerate() {
        let id = s["id"].as_str().unwrap_or_default();
        if !id_ok(id) { p.push(format!("styles[{i}].id \"{id}\" must match ^[a-z0-9][a-z0-9-]{{0,63}}$")); }
        if !seen.insert(id.to_string()) { p.push(format!("style id \"{id}\" is used twice")); }
        if !s["name"].as_str().map_or(false, |n| text_ok(n, 100)) { p.push(format!("style \"{id}\" needs a \"name\"")); }
        if !s["tokens"].is_object() { p.push(format!("style \"{id}\" needs \"tokens\" (DesignTokens)")); }
        if s["source"] != "assistant" { p.push(format!("style \"{id}\": \"source\" must be \"assistant\"")); }
    }
    p
}

/// What the mock-up rules say a screen's HTML may not have (the daemon takes it out anyway when it is
/// read; this is so the assistant writes it right in the first place).
fn mock(bytes: &[u8]) -> Vec<String> {
    if bytes.len() > crate::mado::MOCK_MAX { return vec!["the mock-up is larger than 400 kB".into()]; }
    let Ok(text) = std::str::from_utf8(bytes) else { return vec!["the file is not UTF-8 text".into()] };
    let low = text.to_ascii_lowercase();
    let mut p = vec![];
    if !low.contains("<") || low.trim().is_empty() { p.push("the mock-up is empty".into()); }
    if low.contains("<script") { p.push("it has a <script>: mock-ups are HTML and CSS only".into()); }
    if low.contains("http://") || low.contains("https://") || low.contains("//cdn") { p.push("it refers to an address on the web: a mock-up is self-contained".into()); }
    if low.contains("@import") || low.contains("@font-face") { p.push("it loads a stylesheet or a font: use only the --mado-* variables and system fonts".into()); }
    if !low.contains("var(--mado-") { p.push("it does not use the --mado-* variables for colour, font, radius and spacing".into()); }
    p
}

/// Everything wrong with file `rel` (relative to `mado/`) holding `bytes`.
pub fn file(rel: &str, bytes: &[u8]) -> Vec<String> {
    let flow = crate::mado::GRAPHS.iter().find(|(_, f, _)| *f == rel).map(|(_, _, k)| *k);
    if let Some(kind) = flow { return graph(bytes, kind).err().unwrap_or_default(); }
    match rel {
        crate::mado::PLAN => plan(bytes).err().unwrap_or_default(),
        "design/styles.json" => styles(bytes),
        r if r.starts_with("design/screens/") && r.ends_with(".html") => mock(bytes),
        r if r.ends_with(".md") => if bytes.len() > crate::mado::DOC_MAX { vec!["the file is larger than 1 MB".into()] } else { doc(r, bytes) },
        _ => vec![],
    }
}
