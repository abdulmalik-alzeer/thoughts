//! MADO comments: what the user said about a passage, a node, an edge, a token, a spot on a
//! mock-up or a task. Stored in `mado/comments.json` (the daemon's own file). After every run the
//! text comments are found again in the documents as they are now (by the quoted passage and the
//! words around it); a comment whose target is gone is `orphaned`. A change request turns comments
//! and the user's diagram edits into one precise instruction (`change_prompt`).

use crate::mado::{self, Comment, CommentTarget, Comments, GraphDoc, GraphOp, OpsOnFile};
use crate::state::AppState;
use std::path::Path;

const FILE: &str = "comments.json";
const MAX_COMMENTS: usize = 2000;
const MAX_BODY: usize = 10_000;
const MAX_QUOTE: usize = 2000;
/// How much text either side of a quoted passage is kept with it.
pub const CONTEXT: usize = 40;

pub fn load(mado: &Path, full: bool) -> anyhow::Result<Comments> {
    let Some(b) = mado::read_in(mado, FILE, full, 20_000_000)? else { return Ok(Comments::default()) };
    Ok(serde_json::from_slice(&b).unwrap_or_else(|e| { crate::log::warn(format!("mado: comments.json could not be read ({e})")); Comments::default() }))
}

fn save(mado: &Path, c: &Comments, full: bool) -> anyhow::Result<()> { mado::write_in(mado, FILE, &serde_json::to_vec_pretty(c)?, full) }

/// Change the comments under the project's lock (the state's lock: one change at a time) and tell the window.
async fn with_comments<T>(state: &AppState, pid: &str, f: impl FnOnce(&Path, &mut Comments, bool) -> anyhow::Result<T>) -> anyhow::Result<T> {
    let full = state.settings.read().await.full_access();
    let m = mado::mado_dir(state, pid).await?;
    let lock = lock_of(pid);
    let _held = lock.lock().await;
    if mado::load_state(&m, full)?.is_none() { return Err(crate::tr::err("mado.not_started", &[])); }
    let mut c = load(&m, full)?;
    let out = f(&m, &mut c, full)?;
    save(&m, &c, full)?;
    drop(_held);
    mado::tell(state, pid, "comments").await;
    Ok(out)
}

fn lock_of(pid: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    static L: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).entry(pid.to_string()).or_default().clone()
}

/// A graph's file from its file name or its name (`flow` → `flow.graph.json`).
pub fn graph_file_of(f: &str) -> Option<(&'static str, &'static str)> {
    mado::GRAPHS.iter().find(|(n, file, _)| *n == f || *file == f).map(|(_, file, kind)| (*file, *kind))
}
fn doc_file_of(f: &str) -> Option<&'static str> { mado::DOCS.iter().find(|(n, file)| *n == f || *file == f).map(|(_, file)| *file) }

fn last_chars(s: &str, n: usize) -> String { let c: Vec<char> = s.chars().collect(); c[c.len().saturating_sub(n)..].iter().collect() }
fn first_chars(s: &str, n: usize) -> String { s.chars().take(n).collect() }

/// The target as it will be stored: known files, plain ids, a pin inside the mock-up, a quote of
/// sensible length with at most `CONTEXT` characters either side.
pub fn clean_target(t: CommentTarget) -> anyhow::Result<CommentTarget> {
    let bad = || crate::tr::err("mado.bad_target", &[]);
    let id_ok = crate::mado_validate::id_ok;
    Ok(match t {
        CommentTarget::Text { file, quote, prefix, suffix, heading } => {
            let file = doc_file_of(&file).ok_or_else(bad)?.to_string();
            if quote.trim().is_empty() || quote.chars().count() > MAX_QUOTE { return Err(bad()); }
            let heading = heading.map(|h| first_chars(h.trim(), 300)).filter(|h| !h.is_empty());
            CommentTarget::Text { file, quote, prefix: last_chars(&prefix, CONTEXT), suffix: first_chars(&suffix, CONTEXT), heading }
        }
        CommentTarget::Node { file, node_id } => {
            let (file, _) = graph_file_of(&file).ok_or_else(bad)?;
            if !id_ok(&node_id) { return Err(bad()); }
            CommentTarget::Node { file: file.into(), node_id }
        }
        CommentTarget::Edge { file, edge_id } => {
            let (file, _) = graph_file_of(&file).ok_or_else(bad)?;
            if !id_ok(&edge_id) { return Err(bad()); }
            CommentTarget::Edge { file: file.into(), edge_id }
        }
        CommentTarget::Token { token_path } => {
            let ok = !token_path.is_empty() && token_path.len() <= 100 && token_path.split('.').all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric()));
            if !ok { return Err(bad()); }
            CommentTarget::Token { token_path }
        }
        CommentTarget::Screen { screen_id, x, y } => {
            if !id_ok(&screen_id) || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) { return Err(bad()); }
            CommentTarget::Screen { screen_id, x, y }
        }
        CommentTarget::Task { task_id } => { if !id_ok(&task_id) { return Err(bad()); } CommentTarget::Task { task_id } }
    })
}

fn clean_body(body: &str) -> anyhow::Result<String> {
    let b = body.trim();
    if b.is_empty() || b.chars().count() > MAX_BODY { return Err(crate::tr::err("mado.bad_comment", &[])); }
    Ok(b.to_string())
}

pub async fn add(state: &AppState, pid: &str, target: CommentTarget, body: String) -> anyhow::Result<Comment> {
    let target = clean_target(target)?;
    let body = clean_body(&body)?;
    with_comments(state, pid, |m, all, full| {
        if all.comments.len() >= MAX_COMMENTS { return Err(crate::tr::err("mado.too_many_comments", &[])); }
        let mut c = Comment { id: uuid::Uuid::new_v4().to_string(), target, body, created_at: chrono::Utc::now().to_rfc3339(), status: "open".into(), run_id: None };
        // A comment on something that is not there is said to be so at once.
        if !target_exists(m, &c.target, full) { c.status = "orphaned".into(); }
        all.comments.push(c.clone());
        Ok(c)
    }).await
}

/// The user edits a comment's words, or marks it open or resolved.
pub async fn change(state: &AppState, pid: &str, id: &str, body: Option<String>, status: Option<String>) -> anyhow::Result<Comment> {
    let body = body.map(|b| clean_body(&b)).transpose()?;
    if let Some(s) = &status { if s != "open" && s != "resolved" { return Err(crate::tr::err("mado.bad_comment", &[])); } }
    with_comments(state, pid, |_, all, _| {
        let c = all.comments.iter_mut().find(|c| c.id == id).ok_or_else(|| crate::tr::err("mado.no_comment", &[]))?;
        if let Some(b) = body { c.body = b; }
        if let Some(s) = status { c.status = s; if c.status == "open" { c.run_id = None; } }
        Ok(c.clone())
    }).await
}

pub async fn remove(state: &AppState, pid: &str, id: &str) -> anyhow::Result<()> {
    with_comments(state, pid, |_, all, _| {
        let n = all.comments.len();
        all.comments.retain(|c| c.id != id);
        if all.comments.len() == n { return Err(crate::tr::err("mado.no_comment", &[])); }
        Ok(())
    }).await
}

/// The comments a change request carries go `sent`, with the run that applies them.
pub async fn mark_sent(state: &AppState, pid: &str, ids: &[String], chat_id: &str) -> anyhow::Result<()> {
    if ids.is_empty() { return Ok(()); }
    with_comments(state, pid, |_, all, _| {
        for c in all.comments.iter_mut().filter(|c| ids.contains(&c.id)) { c.status = "sent".into(); c.run_id = Some(chat_id.into()); }
        Ok(())
    }).await
}

/// The request did not start: its comments are open again.
pub async fn unsend(state: &AppState, pid: &str, ids: &[String]) -> anyhow::Result<()> {
    if ids.is_empty() { return Ok(()); }
    with_comments(state, pid, |_, all, _| {
        for c in all.comments.iter_mut().filter(|c| ids.contains(&c.id) && c.status == "sent") { c.status = "open".into(); c.run_id = None; }
        Ok(())
    }).await
}

/// After a run of `chat_id`: what it was sent is resolved when it ended well (open again when not),
/// and every comment is held against the files as they are now.
pub async fn after_run(state: &AppState, pid: &str, chat_id: &str, ok: bool) -> anyhow::Result<()> {
    with_comments(state, pid, |m, all, full| {
        for c in all.comments.iter_mut().filter(|c| c.status == "sent" && c.run_id.as_deref() == Some(chat_id)) {
            c.status = if ok { "resolved" } else { "open" }.into();
            if !ok { c.run_id = None; }
        }
        reanchor_all(m, all, full);
        Ok(())
    }).await
}

/// Every comment held against the files as they are now (after the user's own edit, too).
pub async fn reanchor_project(state: &AppState, pid: &str) -> anyhow::Result<()> {
    with_comments(state, pid, |m, all, full| { reanchor_all(m, all, full); Ok(()) }).await
}

fn graph(m: &Path, file: &str, full: bool) -> Option<GraphDoc> {
    let (file, kind) = graph_file_of(file)?;
    mado::read_graph(m, file, kind, full).ok().flatten()
}

/// Is a node, edge, task or screen comment's target still there? (Text is found by `anchor`.)
fn target_exists(m: &Path, t: &CommentTarget, full: bool) -> bool {
    match t {
        CommentTarget::Text { file, quote, prefix, suffix, heading } => {
            mado::read_text(m, file, full).ok().flatten().map_or(false, |text| anchor(&text, quote, prefix, suffix, heading.as_deref()).is_some())
        }
        CommentTarget::Node { file, node_id } => graph(m, file, full).map_or(false, |g| g.nodes.iter().any(|n| &n.id == node_id)),
        CommentTarget::Edge { file, edge_id } => graph(m, file, full).map_or(false, |g| g.edges.iter().any(|e| &e.id == edge_id)),
        CommentTarget::Screen { screen_id, .. } => graph(m, "flow.graph.json", full).map_or(false, |g| g.nodes.iter().any(|n| &n.id == screen_id && n.kind == "screen")),
        CommentTarget::Task { task_id } => mado::read_plan(m, full).ok().flatten().map_or(false, |p| p.tasks.iter().any(|t| &t.id == task_id)),
        CommentTarget::Token { .. } => true,
    }
}

/// Re-anchor every comment that is not resolved: a text comment follows its passage (quote,
/// context, heading updated to where it is now); one whose target is gone goes `orphaned`; an
/// orphaned one whose target came back is open again.
pub fn reanchor_all(m: &Path, all: &mut Comments, full: bool) {
    let mut texts: std::collections::HashMap<String, Option<String>> = Default::default();
    for c in all.comments.iter_mut().filter(|c| c.status != "resolved") {
        let found = match &mut c.target {
            CommentTarget::Text { file, quote, prefix, suffix, heading } => {
                let text = texts.entry(file.clone()).or_insert_with(|| mado::read_text(m, file, full).ok().flatten());
                match text.as_deref().and_then(|t| anchor(t, quote, prefix, suffix, heading.as_deref())) {
                    Some(a) => { *quote = a.quote; *prefix = a.prefix; *suffix = a.suffix; if a.heading.is_some() { *heading = a.heading; } true }
                    None => false,
                }
            }
            t => target_exists(m, t, full),
        };
        if !found && c.status != "orphaned" { c.status = "orphaned".into(); c.run_id = None; }
        else if found && c.status == "orphaned" { c.status = "open".into(); }
    }
}

/// Where a quoted passage is now.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor { pub start: usize, pub quote: String, pub prefix: String, pub suffix: String, pub heading: Option<String> }

/// The heading a position in a markdown text is under (the nearest `#` line above it).
pub fn heading_at(text: &str, pos: usize) -> Option<String> {
    let mut fenced = false;
    let mut last = None;
    let mut at = 0usize;
    for line in text.split_inclusive('\n') {
        if at > pos { break; }
        if line.trim_start().starts_with("```") { fenced = !fenced; }
        else if !fenced && line.starts_with('#') { last = Some(line.trim_start_matches('#').trim().to_string()); }
        at += line.len();
    }
    last.filter(|h| !h.is_empty())
}

fn common_suffix(a: &str, b: &str) -> usize { a.chars().rev().zip(b.chars().rev()).take_while(|(x, y)| x == y).count() }
fn common_prefix(a: &str, b: &str) -> usize { a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count() }

/// Find the passage `quote` in `text` again: every place it appears (exactly, else with its spacing
/// changed) is scored by how much of the words before and after still match and whether it is
/// under the same heading; the best one wins. `None` when it is gone.
pub fn anchor(text: &str, quote: &str, prefix: &str, suffix: &str, heading: Option<&str>) -> Option<Anchor> {
    // Every place it appears, its spacing changed or not (an exact copy is one of them).
    let words: Vec<String> = quote.split_whitespace().map(regex::escape).collect();
    if words.is_empty() { return None; }
    let re = regex::Regex::new(&words.join(r"\s+")).ok()?;
    let spots: Vec<(usize, usize)> = re.find_iter(text).map(|m| (m.start(), m.end())).collect();
    let best = spots.into_iter().max_by_key(|(s, e)| {
        let before = &text[..*s];
        let after = &text[*e..];
        let mut score = common_suffix(before, prefix) + common_prefix(after, suffix);
        if let Some(h) = heading { if heading_at(text, *s).as_deref() == Some(h) { score += 2 * CONTEXT; } }
        // Among equals, the first one.
        (score, std::cmp::Reverse(*s))
    })?;
    let (s, e) = best;
    Some(Anchor { start: s, quote: text[s..e].to_string(), prefix: last_chars(&text[..s], CONTEXT), suffix: first_chars(&text[e..], CONTEXT), heading: heading_at(text, s) })
}

// ---------- the change request ----------

fn node_desc(g: Option<&GraphDoc>, id: &str) -> String {
    match g.and_then(|g| g.nodes.iter().find(|n| n.id == id)) {
        Some(n) => format!("`{id}` ({:?}, kind {})", n.label, n.kind),
        None => format!("`{id}`"),
    }
}

fn edge_desc(g: Option<&GraphDoc>, id: &str) -> String {
    match g.and_then(|gr| gr.edges.iter().find(|e| e.id == id).map(|e| (gr, e))) {
        Some((gr, e)) => format!("`{id}` (from {} to {}{})", node_desc(Some(gr), &e.from), node_desc(Some(gr), &e.to),
            e.label.as_ref().map(|l| format!(", labelled {l:?}")).unwrap_or_default()),
        None => format!("`{id}`"),
    }
}

fn quoted(text: &str) -> String { text.lines().map(|l| format!("   > {l}")).collect::<Vec<_>>().join("\n") }

/// One comment as an instruction line.
fn comment_line(m: &Path, full: bool, c: &Comment) -> String {
    let body = c.body.trim();
    match &c.target {
        CommentTarget::Text { file, quote, prefix, suffix, heading } => {
            let under = heading.as_ref().map(|h| format!(", under the heading {h:?}")).unwrap_or_default();
            format!("On `{file}`{under}, the passage:\n{}\n   (in context: {:?} [passage] {:?})\n   Comment: {body}", quoted(quote), format!("…{prefix}"), format!("{suffix}…"))
        }
        CommentTarget::Node { file, node_id } => format!("On node {} of `{file}`:\n   Comment: {body}", node_desc(graph(m, file, full).as_ref(), node_id)),
        CommentTarget::Edge { file, edge_id } => format!("On edge {} of `{file}`:\n   Comment: {body}", edge_desc(graph(m, file, full).as_ref(), edge_id)),
        CommentTarget::Token { token_path } => format!("On the design token `{token_path}` (design/tokens.json and the style it came from in design/styles.json):\n   Comment: {body}"),
        CommentTarget::Screen { screen_id, x, y } => format!("On the mock-up of screen {} (`design/screens/{screen_id}.html`), at the spot {:.0}% from the left and {:.0}% from the top:\n   Comment: {body}",
            node_desc(graph(m, "flow.graph.json", full).as_ref(), screen_id), x * 100.0, y * 100.0),
        CommentTarget::Task { task_id } => {
            let title = mado::read_plan(m, full).ok().flatten().and_then(|p| p.tasks.into_iter().find(|t| &t.id == task_id)).map(|t| format!(" ({:?})", t.title)).unwrap_or_default();
            format!("On task `{task_id}`{title} of `plan.json`:\n   Comment: {body}")
        }
    }
}

fn op_line(g: Option<&GraphDoc>, op: &GraphOp) -> String {
    match op {
        GraphOp::Rename { id, label } => format!("rename node {} to {label:?}", node_desc(g, id)),
        GraphOp::AddNode { label, kind, near } => format!("add a node {label:?} of kind {kind}{}", near.as_ref().map(|n| format!(", next to {}", node_desc(g, n))).unwrap_or_default()),
        GraphOp::RemoveNode { id } => format!("remove node {} and the edges that touch it", node_desc(g, id)),
        GraphOp::AddEdge { from, to, label } => format!("add an edge from {} to {}{}", node_desc(g, from), node_desc(g, to), label.as_ref().map(|l| format!(" labelled {l:?}")).unwrap_or_default()),
        GraphOp::RemoveEdge { id } => format!("remove edge {}", edge_desc(g, id)),
        GraphOp::RelabelEdge { id, label } => format!("relabel edge {} to {label:?}", edge_desc(g, id)),
        GraphOp::SetProp { id, key, value } => format!("set the property {key:?} of {} to {}", node_desc(g, id), serde_json::to_string(value).unwrap_or_default()),
    }
}

/// The instruction a change request sends: every comment (with its anchor and the quoted passage)
/// and every diagram edit, applied together to the diagram AND to the passages of the documents
/// that describe the same thing, ids kept stable, the positions of kept nodes kept.
pub fn change_prompt(m: &Path, full: bool, comments: &[Comment], ops: &[OpsOnFile], note: Option<&str>) -> String {
    let mut out = String::from("Apply the change requests below to this project's MADO files (in `mado/`), all in this one turn.\n\n\
        How to apply them:\n\
        - Apply each change where it is made (the diagram file, the document, the mock-up or the plan it names) AND to every passage of the \
        other MADO files that describes the same thing — brief.md, spec.md, tech.md, design.md, the three .graph.json diagrams, plan.json and \
        the mock-ups in design/screens/ — so that the documents and the diagrams agree with each other afterwards.\n\
        - Keep every id that still names the same thing (nodes, edges, features F<n>, tasks, screens). Something new gets a new id in the pattern \
        ^[a-z0-9][a-z0-9-]{0,63}$; a screen added to the flow also gets its mock-up and its place in the plan if those files exist.\n\
        - Keep the \"pos\" of every node you keep exactly as it is. Give new nodes no \"pos\" (the app places them).\n\
        - Change nothing that the requests do not call for. Never write state.json, comments.json or build.json.\n\
        - Every file must still have the exact shape your instructions give for it.\n\
        - When you are done, answer with one short line per request: what you changed and where (file and section or id). If a request cannot \
        be applied as asked, say why instead of guessing.\n");
    if !comments.is_empty() {
        out.push_str("\n## Comments\n\n");
        for (i, c) in comments.iter().enumerate() { out.push_str(&format!("{}. {}\n\n", i + 1, comment_line(m, full, c))); }
    }
    let ops: Vec<&OpsOnFile> = ops.iter().filter(|o| !o.ops.is_empty()).collect();
    if !ops.is_empty() {
        out.push_str("\n## Diagram edits the user made in the window (not yet in the files: make them)\n");
        for o in ops {
            let g = graph(m, &o.file, full);
            out.push_str(&format!("\nIn `{}`:\n", o.file));
            for op in &o.ops { out.push_str(&format!("- {}\n", op_line(g.as_ref(), op))); }
        }
    }
    if let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) { out.push_str(&format!("\n## The user's note\n\n{n}\n")); }
    out
}
