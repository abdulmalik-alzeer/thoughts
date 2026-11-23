//! MADO: the app's way of planning and building a software project (`docs/mado-spec.md`).
//!
//! Seven steps — brief, spec, tech, design, plan, review, build — each with documents and diagrams
//! the user reviews, comments on and approves. Everything lives in the linked project's own folder,
//! in `mado/`. This file is the core: where the files are (and that nothing reaches outside them),
//! the step state machine, documents and diagrams, comments, the runs of the planning steps and of
//! change requests, and the routes under `/api/mado/:projectId`. Next to it:
//! `mado_validate` (nothing a run wrote is trusted), `mado_review` (the gate before the build),
//! `mado_comments` (anchors and the change-request prompt) and `mado_prompts` (what each step's run
//! is told). The design files are `mado_design`'s and the build engine is `mado_build`'s.

use crate::api::ApiErr;
use crate::state::AppState;
use axum::extract::{Path as UrlPath, State};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use crate::mado_review::{plan_hash, review, waves};

/// Round 0.5.0: a project that already has code (detection, the Read rules for its secrets, the code map).
#[path = "mado_existing.rs"]
pub mod existing;

type S = State<Arc<AppState>>;
type R = Result<Json<Value>, ApiErr>;

// ---------- shapes (ui/src/lib/mado/types.ts) ----------

/// The seven steps, in their fixed order.
pub const STEPS: [&str; 7] = ["brief", "spec", "tech", "design", "plan", "review", "build"];
/// The steps that have a run of kind `mado` (review is the gate; the build is the engine's).
pub const RUN_STEPS: [&str; 5] = ["brief", "spec", "tech", "design", "plan"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct StepState {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<String>,
    /// What the step's last run changed outside `mado/` (project-relative), which keeps the step
    /// from being approved until the user dismisses it (`mado_stray`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strayed: Option<Vec<String>>,
    /// The last run's check for changes outside `mado/` could not be made (its kept "before" was
    /// missing or not the one recorded): approval waits for a dismiss, as for `strayed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stray_unchecked: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MadoState {
    pub version: u32,
    pub steps: BTreeMap<String, StepState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_hash: Option<String>,
    /// Round 0.5.0: `"existing"` when the plan documents a project that is already built (its
    /// documents are written from the code, its plan is a change). Absent: a new project, exactly as
    /// before 0.5.0 (a plan of 0.3.x/0.4.x has none). `normalize` keeps no other value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Round 0.5.0: when the technical design of an existing project was last approved (its documents
    /// then describe the code as it was). Display only: `distrust` drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documented: Option<Documented>,
}

/// When the documents of an existing project were approved as its description (`MadoState.documented`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Documented { pub at: String }

/// The one value `MadoState.mode` may hold.
pub const EXISTING: &str = "existing";

impl MadoState {
    /// Does this plan document a project that is already built (round 0.5.0)?
    pub fn existing(&self) -> bool { self.mode.as_deref() == Some(EXISTING) }
}

impl Default for MadoState {
    fn default() -> Self {
        let mut s = MadoState { version: 1, steps: BTreeMap::new(), style: None, plan_hash: None, mode: None, documented: None };
        normalize(&mut s);
        s
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Pos { pub x: f64, pub y: f64 }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<serde_json::Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<Pos>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GraphEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GraphDoc {
    pub version: u32,
    pub kind: String,
    pub title: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanTask {
    pub id: String,
    pub title: String,
    pub description: String,
    pub owns: Vec<String>,
    pub depends_on: Vec<String>,
    pub acceptance: Vec<String>,
    pub feature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screens: Option<Vec<String>>,
    /// Round 0.4.0: `contracts`, `tests` or `feature` (absent = feature). Checked by `mado_validate::plan`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl PlanTask {
    /// A `contracts` or `tests` task: what it owns is locked once it is done (`mado_build`).
    pub fn locks(&self) -> bool { matches!(self.kind.as_deref(), Some("contracts" | "tests")) }
}

/// One screen of the flow at a concrete address of the running product (`path` starts with `/`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlanScreen { pub id: String, pub path: String }

/// How to start the product and where each screen is (round 0.4.0).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlanRun {
    pub start: String,
    pub url: String,
    #[serde(default)]
    pub screens: Vec<PlanScreen>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlanDoc {
    pub version: u32,
    pub tasks: Vec<PlanTask>,
    /// Commands the build runs once in each fresh working copy before any task (installing the
    /// dependencies, say). Checked like acceptance commands, shown first in the review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<Vec<String>>,
    /// Round 0.4.0: commands run in the build's merged copy after every wave (checked like acceptance commands).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<Vec<String>>,
    /// Round 0.4.0: how to start the product, and one address per screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<PlanRun>,
}

impl PlanDoc {
    /// Every command of the plan the build may run, in the order the review lists them: set-up,
    /// acceptance, `verify`, then `run.start` (blank ones left out, untrimmed).
    pub fn all_commands(&self) -> Vec<&String> {
        self.setup.iter().flatten().chain(self.tasks.iter().flat_map(|t| t.acceptance.iter())).chain(self.verify.iter().flatten())
            .chain(self.run.as_ref().map(|r| &r.start)).filter(|c| !c.trim().is_empty()).collect()
    }
}

/// What a comment is about.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CommentTarget {
    #[serde(rename_all = "camelCase")]
    Text { file: String, quote: String, prefix: String, suffix: String, #[serde(default, skip_serializing_if = "Option::is_none")] heading: Option<String> },
    #[serde(rename_all = "camelCase")]
    Node { file: String, node_id: String },
    #[serde(rename_all = "camelCase")]
    Edge { file: String, edge_id: String },
    #[serde(rename_all = "camelCase")]
    Token { token_path: String },
    #[serde(rename_all = "camelCase")]
    Screen { screen_id: String, x: f64, y: f64 },
    #[serde(rename_all = "camelCase")]
    Task { task_id: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub target: CommentTarget,
    pub body: String,
    pub created_at: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Comments { pub version: u32, pub comments: Vec<Comment> }
impl Default for Comments { fn default() -> Self { Comments { version: 1, comments: vec![] } } }

/// What the user did to a diagram, sent as a change request (never applied by the window itself).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum GraphOp {
    Rename { id: String, label: String },
    AddNode { label: String, kind: String, #[serde(default)] near: Option<String> },
    RemoveNode { id: String },
    AddEdge { from: String, to: String, #[serde(default)] label: Option<String>, #[serde(default)] kind: Option<String> },
    RemoveEdge { id: String },
    RelabelEdge { id: String, label: String },
    SetProp { id: String, key: String, value: Value },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Problem {
    pub kind: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Lane { pub id: String, pub tasks: Vec<String> }
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Wave { pub lanes: Vec<Lane> }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FeatureTrace { pub id: String, pub name: String, pub tasks: Vec<String> }
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ScreenTrace { pub id: String, pub label: String, pub tasks: Vec<String> }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Review {
    pub features: Vec<FeatureTrace>,
    pub screens: Vec<ScreenTrace>,
    pub problems: Vec<Problem>,
    pub waves: Vec<Wave>,
    pub commands: Vec<String>,
    /// sha256 of the bytes of `plan.json` this review was computed from (filled in by `GET /review`;
    /// empty from the pure `review`, which sees the parsed plan only). Approving the review must send
    /// it back: the approval is bound to the plan the user was shown.
    #[serde(rename = "planHash", default)]
    pub plan_hash: String,
}

/// Round 0.5.0: what a plan of an existing project changes (`plan.json` `change`, optional).
/// Read apart from `PlanDoc` (`plan_change`), so every `PlanDoc` stays as it was; checked by
/// `mado_validate::plan` whenever it is there.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlanChange {
    pub title: String,
    /// The spec's features the change touches or adds (`F2`, `F9`).
    #[serde(default)]
    pub features: Vec<String>,
    /// Project-relative files the change is expected to touch (each passes `owned_path_ok`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

/// The `change` of the bytes of a `plan.json` that `mado_validate::plan` accepted; `None` when it has none.
pub fn plan_change(bytes: &[u8]) -> Option<PlanChange> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    serde_json::from_value(v.get("change").filter(|c| !c.is_null())?.clone()).ok()
}

// ---------- the files ----------

/// The folder everything lives in, inside the project.
pub const DIR: &str = "mado";
/// Files only the daemon writes: no run may change them.
pub const DAEMON_FILES: [&str; 3] = ["state.json", "comments.json", "build.json"];
/// The documents (`GET/PUT /doc/:name`).
/// `conventions` is the tech step's and optional: a plan made before 0.4.0 has none (`step_optional`).
/// `change` is the plan step's in existing mode (round 0.5.0: what the user wants changed), optional too.
pub const DOCS: [(&str, &str); 6] = [("brief", "brief.md"), ("spec", "spec.md"), ("tech", "tech.md"), ("design", "design.md"), ("conventions", "conventions.md"), ("change", "change.md")];
/// The diagrams (`GET /graph/:name`): name, file, kind. `tasks` is derived from `plan.json`.
pub const GRAPHS: [(&str, &str, &str); 3] = [("flow", "flow.graph.json", "flow"), ("architecture", "architecture.graph.json", "architecture"), ("schema", "schema.graph.json", "erd")];
pub const PLAN: &str = "plan.json";
/// A document's largest size (1 MB), a mock-up's (400 kB).
pub const DOC_MAX: usize = 1_000_000;
pub const MOCK_MAX: usize = 400_000;

pub fn doc_file(name: &str) -> Option<&'static str> { DOCS.iter().find(|(n, _)| *n == name).map(|(_, f)| *f) }
pub fn graph_file(name: &str) -> Option<(&'static str, &'static str)> { GRAPHS.iter().find(|(n, _, _)| *n == name).map(|(_, f, k)| (*f, *k)) }

/// The files a step's run must leave behind (the mock-ups of the design step are added per screen).
pub fn step_outputs(step: &str) -> &'static [&'static str] {
    match step {
        "brief" => &["brief.md"],
        "spec" => &["spec.md", "flow.graph.json"],
        "tech" => &["tech.md", "architecture.graph.json", "schema.graph.json"],
        "design" => &["design.md", "design/styles.json"],
        "plan" => &["plan.json"],
        _ => &[],
    }
}

/// The files a step's run writes that a step may be approved without: plans approved before they
/// existed stay approved and valid. When one is there it is checked and shown like the others.
pub fn step_optional(step: &str) -> &'static [&'static str] {
    match step {
        "tech" => &["conventions.md"],
        // Round 0.5.0: the change a plan of an existing project is for.
        "plan" => &["change.md"],
        _ => &[],
    }
}

/// The step a file belongs to.
pub fn step_of_file(rel: &str) -> Option<&'static str> {
    RUN_STEPS.iter().copied().find(|s| step_outputs(s).contains(&rel) || step_optional(s).contains(&rel)).or_else(|| (rel.starts_with("design/")).then_some("design"))
}

/// The linked project's folder, as it really is on disk.
pub fn project_dir_in(s: &crate::settings::Settings, project_id: &str) -> anyhow::Result<PathBuf> {
    let p = s.projects.iter().find(|p| p.id == project_id).ok_or_else(|| crate::tr::err("not_found.project", &[]))?;
    let dir = PathBuf::from(&p.path);
    if !dir.is_absolute() || !dir.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", &p.path)])); }
    Ok(crate::paths::canonical(&dir)?)
}

/// Every project linked to any listed library: the open one's first, then those of the libraries
/// that are not open (their parked settings). For the build engine, which goes on with a build
/// whatever library is open; the routes keep to the open library's projects (`project_dir`).
pub fn all_linked_projects(s: &crate::settings::Settings) -> Vec<crate::settings::Project> {
    let mut out: Vec<crate::settings::Project> = s.projects.clone();
    for p in s.libraries.iter().filter_map(|l| l.parked.as_ref()).flat_map(|l| l.projects.iter()) {
        if !out.iter().any(|q| q.id == p.id) { out.push(p.clone()); }
    }
    out
}

/// `project_dir_in` over every linked project of every listed library (`all_linked_projects`).
pub fn any_project_dir_in(s: &crate::settings::Settings, project_id: &str) -> anyhow::Result<PathBuf> {
    let p = all_linked_projects(s).into_iter().find(|p| p.id == project_id).ok_or_else(|| crate::tr::err("not_found.project", &[]))?;
    let dir = PathBuf::from(&p.path);
    if !dir.is_absolute() || !dir.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", &p.path)])); }
    Ok(crate::paths::canonical(&dir)?)
}

/// The project's folder, only for a project the user linked (`Settings::projects`).
pub async fn project_dir(state: &AppState, project_id: &str) -> anyhow::Result<PathBuf> {
    project_dir_in(&*state.settings.read().await, project_id)
}

/// `<project>/mado`, checked: never a link to somewhere else (unless full access is on).
pub async fn mado_dir(state: &AppState, project_id: &str) -> anyhow::Result<PathBuf> {
    let p = project_dir(state, project_id).await?;
    let full = state.settings.read().await.full_access();
    mado_dir_of(&p, full)
}

pub fn mado_dir_of(project: &Path, full: bool) -> anyhow::Result<PathBuf> {
    let d = project.join(DIR);
    if let Ok(m) = std::fs::symlink_metadata(&d) {
        if m.file_type().is_symlink() && !full { return Err(crate::tr::err("mado.path", &[("path", DIR)])); }
        if !m.is_dir() && !m.file_type().is_symlink() { return Err(crate::tr::err("mado.path", &[("path", DIR)])); }
    }
    Ok(d)
}

/// The most entries `odd_entry` looks through (a `mado/` folder holds a few dozen files).
pub const ENTRY_CAP: usize = 5000;
/// The deepest `odd_entry` looks; anything deeper is refused, never taken as plain.
pub const ENTRY_DEPTH: usize = 12;

/// The first entry of `mado/` that is not a plain folder or a plain file of its own: a symbolic link,
/// a device, pipe or socket, or a file with more than one hard link (it is also a file somewhere
/// else, say `~/.zshrc`). A folder too large or too deep to look through all of counts too (it fails
/// closed: what was not looked at is never taken as plain). `None`: every entry is plain.
pub fn odd_entry(mado: &Path) -> Option<String> {
    let Ok(m) = std::fs::symlink_metadata(mado) else { return None };
    if !m.is_dir() { return Some(DIR.into()); }
    let mut n = 0usize;
    // One level more than allowed: any entry found there is refused.
    for e in walkdir::WalkDir::new(mado).follow_links(false).min_depth(1).max_depth(ENTRY_DEPTH + 1) {
        n += 1;
        let rel = |p: &Path| format!("{DIR}/{}", p.strip_prefix(mado).unwrap_or(p).to_string_lossy().replace('\\', "/"));
        if n > ENTRY_CAP { return Some(format!("{DIR}/… ({ENTRY_CAP}+)")); }
        let e = match e { Ok(e) => e, Err(err) => return Some(err.path().map(rel).unwrap_or_else(|| DIR.into())) };
        if e.depth() > ENTRY_DEPTH { return Some(rel(e.path())); }
        let ft = e.file_type();
        if ft.is_dir() { continue; }
        if !ft.is_file() { return Some(rel(e.path())); }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            match std::fs::symlink_metadata(e.path()) { Ok(m) if m.nlink() <= 1 => {} _ => return Some(rel(e.path())) }
        }
    }
    None
}

/// `odd_entry` as a guard: refused with the entry named, unless full access is on.
pub fn require_plain(mado: &Path, full: bool) -> anyhow::Result<()> {
    if full { return Ok(()); }
    match odd_entry(mado) {
        Some(p) => Err(crate::tr::err("mado.odd_entry", &[("path", &p.chars().take(200).collect::<String>())])),
        None => Ok(()),
    }
}

/// A name inside `mado/` is plain: segments of letters, digits, `.`, `_` and `-`, none starting with a dot.
pub fn clean_rel(rel: &str) -> bool {
    !rel.is_empty() && rel.len() <= 200 && rel.split('/').all(|seg| {
        !seg.is_empty() && !seg.starts_with('.') && seg.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    })
}

/// `rel` inside `mado`, resolved: no `..`, no absolute part, and (unless full access is on) no link
/// anywhere on the way, and its real location inside the real `mado/`.
pub fn resolve_in(mado: &Path, rel: &str, full: bool) -> anyhow::Result<PathBuf> {
    if !clean_rel(rel) { return Err(crate::tr::err("mado.path", &[("path", rel)])); }
    let mut p = mado.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
        if full { continue; }
        if let Ok(m) = std::fs::symlink_metadata(&p) {
            if m.file_type().is_symlink() { return Err(crate::tr::err("mado.path", &[("path", rel)])); }
        }
    }
    if !full && p.exists() {
        let (real, root) = (crate::paths::canonical(&p)?, crate::paths::canonical(mado)?);
        if !real.starts_with(&root) { return Err(crate::tr::err("mado.path", &[("path", rel)])); }
    }
    Ok(p)
}

/// Read a file of `mado/`; `None` when it is not there. Larger than `max` is refused.
pub fn read_in(mado: &Path, rel: &str, full: bool, max: usize) -> anyhow::Result<Option<Vec<u8>>> {
    let p = resolve_in(mado, rel, full)?;
    match std::fs::metadata(&p) {
        Err(_) => Ok(None),
        Ok(m) if !m.is_file() => Err(crate::tr::err("mado.path", &[("path", rel)])),
        Ok(m) if m.len() as usize > max => Err(crate::tr::err("mado.too_large", &[("path", rel)])),
        Ok(_) => Ok(Some(std::fs::read(&p)?)),
    }
}

/// Write a file of `mado/` in one piece (never half a file), its folders made as needed.
pub fn write_in(mado: &Path, rel: &str, bytes: &[u8], full: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(mado)?;
    let p = resolve_in(mado, rel, full)?;
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    // Made just now: still nothing on the way may be a link.
    let p = resolve_in(mado, rel, full)?;
    crate::paths::write_atomic(&p, bytes, false)?;
    Ok(())
}

/// One change to a project's MADO files at a time (state, comments).
fn lock_of(project_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    static L: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).entry(project_id.to_string()).or_default().clone()
}

/// Tell the window that something of a project's MADO changed (`{type:'mado', projectId, what}`).
/// The window refetches; it does not trust the payload.
pub async fn tell(state: &AppState, project_id: &str, what: &str) {
    state.emit(json!({"type": "mado", "projectId": project_id, "what": what}));
}

// ---------- the step state machine ----------

/// The steps that must be approved before `step` opens.
pub fn prereqs(step: &str) -> &'static [&'static str] {
    match step {
        "spec" => &["brief"],
        "tech" | "design" => &["spec"],
        "plan" => &["tech", "design"],
        "review" => &["plan"],
        "build" => &["review"],
        _ => &[],
    }
}

/// Every step that builds on `step`, directly or not.
pub fn later_steps(step: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = vec![];
    for s in STEPS {
        if prereqs(s).iter().any(|p| *p == step || out.contains(p)) { out.push(s); }
    }
    out
}

fn is_approved(st: &MadoState, step: &str) -> bool { st.steps.get(step).map_or(false, |s| s.status == "approved") }

/// Put the state right: every step present with a known status, and `locked` / `ready` following
/// from the steps before (a step that is further along keeps its status and its files).
pub fn normalize(st: &mut MadoState) {
    st.version = 1;
    // Absent is a new project; nothing but "existing" is kept (a stored "new" is the same as none).
    if !st.existing() { st.mode = None; st.documented = None; }
    st.steps.retain(|k, _| STEPS.contains(&k.as_str()));
    for s in STEPS {
        let e = st.steps.entry(s.to_string()).or_default();
        if !["locked", "ready", "drafting", "review", "approved", "stale"].contains(&e.status.as_str()) { e.status = "locked".into(); }
        if RUN_STEPS.iter().all(|r| *r != s) { e.chat_id = None; e.strayed = None; e.stray_unchecked = None; }
        if e.strayed.as_ref().map_or(false, |l| l.is_empty()) { e.strayed = None; }
        if e.stray_unchecked == Some(false) { e.stray_unchecked = None; }
        // The review and the build have no run: nothing for them to be drafting.
        if (s == "review" || s == "build") && e.status == "drafting" { e.status = "ready".into(); }
    }
    for s in STEPS {
        let open = prereqs(s).iter().all(|p| is_approved(st, p));
        let e = st.steps.get_mut(s).expect("every step is present");
        if e.status == "locked" || e.status == "ready" { e.status = if open { "ready" } else { "locked" }.into(); }
    }
}

/// Approve `step`: every later step that was approved goes `stale` (its files stay; the user runs it
/// again or approves it again).
pub fn approve_in(st: &mut MadoState, step: &str, now: &str) {
    if let Some(e) = st.steps.get_mut(step) { e.status = "approved".into(); e.approved_at = Some(now.into()); }
    for later in later_steps(step) {
        if let Some(e) = st.steps.get_mut(later) { if e.status == "approved" { e.status = "stale".into(); } }
    }
    normalize(st);
}

/// Open an approved step again for changes.
pub fn reopen_in(st: &mut MadoState, step: &str) {
    if let Some(e) = st.steps.get_mut(step) { if e.status == "approved" || e.status == "stale" { e.status = "review".into(); e.approved_at = None; } }
    if step == "review" { st.plan_hash = None; }
    // The documents of an existing project are no longer the approved description of it.
    if step == "tech" { st.documented = None; }
    normalize(st);
}

/// May the user approve `step` in its present status?
pub fn approvable(st: &MadoState, step: &str) -> bool {
    let Some(e) = st.steps.get(step) else { return false };
    let open = prereqs(step).iter().all(|p| is_approved(st, p));
    match step {
        "build" => false,
        "review" => open && matches!(e.status.as_str(), "ready" | "review" | "stale"),
        _ => open && matches!(e.status.as_str(), "review" | "stale" | "drafting"),
    }
}

fn state_path(mado: &Path, full: bool) -> anyhow::Result<PathBuf> { resolve_in(mado, "state.json", full) }

// ---------- what the daemon keeps outside the project ----------
//
// Anything that can write the project (a build run, a cloned repository that arrives with a
// ready-made `mado/`) could write `state.json`. So what allows commands to run lives in the app's
// own data folder, `<thoughts home>/mado/<projectId>/`, which no run may reach: the approval of the
// review (`approval.json`: the plan's hash and its commands), and the fingerprint of the last
// `state.json` the daemon wrote (`state.sha256`). A state.json the daemon did not write is not
// believed: every step in it that is further than a draft needs the user again.

/// The user's approval of the review, kept by the daemon: the plan it approved and the commands it showed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Approval { pub plan_hash: String, pub commands: Vec<String>, pub approved_at: String }

/// `<thoughts home>/mado/<projectId>/`, for a well-formed project id only.
pub fn record_dir(project_id: &str) -> Option<PathBuf> {
    let ok = !project_id.is_empty() && project_id.len() <= 64 && project_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    ok.then(|| crate::paths::home().join(DIR).join(project_id))
}

fn write_record(project_id: &str, name: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let d = record_dir(project_id).ok_or_else(|| crate::tr::err("not_found.project", &[]))?;
    crate::paths::create_private_dir(&d)?;
    crate::paths::write_atomic(&d.join(name), bytes, true)?;
    Ok(())
}

/// The review's approval as the daemon recorded it, if there is one.
pub fn approved_plan_in(project_id: &str) -> Option<Approval> {
    let f = record_dir(project_id)?.join("approval.json");
    if std::fs::symlink_metadata(&f).ok()?.file_type().is_symlink() { return None; }
    serde_json::from_slice(&std::fs::read(f).ok()?).ok()
}

/// `approved_plan_in`, for callers holding the state (the build engine).
pub async fn approved_plan(_state: &AppState, project_id: &str) -> Option<Approval> { approved_plan_in(project_id) }

/// Is `plan_bytes` (plan.json now) the very plan the user approved?
pub fn approval_matches(a: &Approval, plan_bytes: &[u8]) -> bool { !a.plan_hash.is_empty() && a.plan_hash == plan_hash(plan_bytes) }

pub fn write_approval(project_id: &str, a: &Approval) -> anyhow::Result<()> { write_record(project_id, "approval.json", &serde_json::to_vec_pretty(a)?) }

/// The approval is gone (the review reopened, or no longer approved).
pub fn clear_approval(project_id: &str) {
    if let Some(d) = record_dir(project_id) { let _ = std::fs::remove_file(d.join("approval.json")); }
}

fn state_seen(project_id: &str) -> Option<String> {
    std::fs::read_to_string(record_dir(project_id)?.join("state.sha256")).ok().map(|s| s.trim().to_string())
}

/// A state.json the daemon did not write: nothing past a draft is believed. Steps with a run go
/// back to review (their files stay), the review and the build are closed, chats, the picked style and the plan hash
/// are forgotten.
pub fn distrust(st: &mut MadoState) {
    for (k, e) in st.steps.iter_mut() {
        e.chat_id = None;
        e.approved_at = None;
        if RUN_STEPS.contains(&k.as_str()) {
            if matches!(e.status.as_str(), "approved" | "stale" | "drafting") { e.status = "review".into(); }
        } else { e.status = "locked".into(); }
    }
    st.plan_hash = None;
    st.style = None;
    // The mode stays (it only changes what the runs are told and which shapes are accepted); the
    // record of when the documents were approved is display only and goes with the approvals.
    st.documented = None;
    normalize(st);
}

/// The state as stored, put right; `None` when `mado/` has not been started. A state.json this
/// computer's daemon did not write is `distrust`ed, and the review counts as approved only with
/// the daemon's own record of it for the plan as it is now.
pub fn load_state(project_id: &str, mado: &Path, full: bool) -> anyhow::Result<Option<MadoState>> {
    let p = state_path(mado, full)?;
    if !p.is_file() { return Ok(None); }
    let bytes = read_in(mado, "state.json", full, DOC_MAX)?.unwrap_or_default();
    // A damaged state file is started over (the documents are what matters, and they stay).
    let mut st: MadoState = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        crate::log::warn(format!("mado: state.json could not be read ({e}); starting from the files"));
        MadoState::default()
    });
    if state_seen(project_id).as_deref() != Some(plan_hash(&bytes).as_str()) { distrust(&mut st); }
    normalize(&mut st);
    if is_approved(&st, "review") {
        let now = read_in(mado, PLAN, full, DOC_MAX).ok().flatten();
        let ok = approved_plan_in(project_id).zip(now).map_or(false, |(a, b)| approval_matches(&a, &b));
        if !ok { if let Some(e) = st.steps.get_mut("review") { e.status = "stale".into(); } normalize(&mut st); }
    }
    Ok(Some(st))
}

/// Save the state, note its fingerprint outside the project, and drop the approval record once the
/// review is no longer approved.
pub fn save_state(project_id: &str, mado: &Path, st: &MadoState, full: bool) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(st)?;
    write_in(mado, "state.json", &bytes, full)?;
    write_record(project_id, "state.sha256", plan_hash(&bytes).as_bytes())?;
    if !is_approved(st, "review") { clear_approval(project_id); }
    Ok(())
}

/// Change the state under the project's lock and tell the window.
pub async fn with_state<T>(state: &AppState, project_id: &str, f: impl FnOnce(&mut MadoState) -> anyhow::Result<T>) -> anyhow::Result<T> {
    let lock = lock_of(project_id);
    let _held = lock.lock().await;
    let full = state.settings.read().await.full_access();
    let mado = mado_dir(state, project_id).await?;
    let mut st = load_state(project_id, &mado, full)?.ok_or_else(|| crate::tr::err("mado.not_started", &[]))?;
    let out = f(&mut st)?;
    normalize(&mut st);
    save_state(project_id, &mado, &st, full)?;
    drop(_held);
    tell(state, project_id, "state").await;
    Ok(out)
}

/// The style picked at the design step, kept in `state.json` (`style`): the design step calls this,
/// so the core stays the only writer of `state.json`. `id` must be a well-formed id.
pub async fn set_style(state: &AppState, project_id: &str, id: &str) -> anyhow::Result<()> {
    if !crate::mado_validate::id_ok(id) { return Err(crate::tr::err("mado.bad_request", &[])); }
    with_state(state, project_id, |st| { st.style = Some(id.to_string()); Ok(()) }).await
}

// ---------- reading what a run wrote ----------

pub fn read_text(mado: &Path, rel: &str, full: bool) -> anyhow::Result<Option<String>> {
    Ok(read_in(mado, rel, full, DOC_MAX)?.map(|b| String::from_utf8_lossy(&b).into_owned()))
}

/// A diagram file, validated. `Ok(None)` when it is not there; an invalid one is an error naming what is wrong.
pub fn read_graph(mado: &Path, file: &str, kind: &str, full: bool) -> anyhow::Result<Option<GraphDoc>> {
    let Some(bytes) = read_in(mado, file, full, DOC_MAX)? else { return Ok(None) };
    match crate::mado_validate::graph(&bytes, kind) {
        Ok(g) => Ok(Some(g)),
        Err(list) => Err(crate::tr::err_detail("mado.invalid", &[("path", file)], list.join("\n"))),
    }
}

pub fn read_plan(mado: &Path, full: bool) -> anyhow::Result<Option<PlanDoc>> {
    let Some(bytes) = read_in(mado, PLAN, full, DOC_MAX)? else { return Ok(None) };
    match crate::mado_validate::plan(&bytes) {
        Ok(p) => Ok(Some(p)),
        Err(list) => Err(crate::tr::err_detail("mado.invalid", &[("path", PLAN)], list.join("\n"))),
    }
}

/// The `tasks` diagram, made from the plan: one node per task, an edge per dependency; `props`
/// carry the feature, what the task owns, its wave and its lane.
pub fn tasks_graph(plan: &PlanDoc) -> GraphDoc {
    let ws = waves(plan);
    let mut place: HashMap<&str, (usize, &str)> = HashMap::new();
    for (i, w) in ws.iter().enumerate() { for l in &w.lanes { for t in &l.tasks { place.insert(t.as_str(), (i + 1, l.id.as_str())); } } }
    let ids: std::collections::HashSet<&str> = plan.tasks.iter().map(|t| t.id.as_str()).collect();
    let nodes = plan.tasks.iter().map(|t| {
        let mut props = serde_json::Map::new();
        props.insert("feature".into(), json!(t.feature));
        props.insert("owns".into(), json!(t.owns));
        if let Some((w, l)) = place.get(t.id.as_str()) { props.insert("wave".into(), json!(w)); props.insert("lane".into(), json!(l)); }
        if let Some(s) = &t.screens { props.insert("screens".into(), json!(s)); }
        GraphNode { id: t.id.clone(), label: t.title.clone(), kind: "task".into(), props: Some(props), pos: None }
    }).collect();
    let mut edges = vec![];
    for t in &plan.tasks {
        for d in t.depends_on.iter().filter(|d| ids.contains(d.as_str())) {
            edges.push(GraphEdge { id: format!("{d}--{}", t.id), from: d.clone(), to: t.id.clone(), label: None, kind: Some("depends".into()) });
        }
    }
    GraphDoc { version: 1, kind: "tasks".into(), title: "Tasks".into(), nodes, edges }
}

/// The screens of the flow diagram (`kind: "screen"`), in its order.
pub fn screens_of(flow: &GraphDoc) -> Vec<(String, String)> {
    flow.nodes.iter().filter(|n| n.kind == "screen").map(|n| (n.id.clone(), n.label.clone())).collect()
}

/// Everything wrong with the files of `step` as they are now, in words for the assistant (English).
/// `missing` lists the files that are not there. A step whose files are all missing is still a
/// conversation (the brief's interview), not a mistake.
/// In existing mode (round 0.5.0) the design step's mock-ups are optional (checked when there) and
/// its `styles.json` may hold one to four candidates (`mado_validate::file_in`).
pub fn check_step(mado: &Path, step: &str, full: bool) -> (Vec<String>, Vec<String>) {
    let (mut missing, mut wrong) = (vec![], vec![]);
    let existing = existing_in(mado, full);
    let mut outputs: Vec<String> = step_outputs(step).iter().map(|s| s.to_string()).collect();
    let mut mocks: Vec<String> = vec![];
    if step == "design" {
        if let Ok(Some(flow)) = read_graph(mado, "flow.graph.json", "flow", full) {
            for (id, _) in screens_of(&flow) { mocks.push(format!("design/screens/{id}.html")); }
        }
        if !existing { outputs.append(&mut mocks); }
    }
    for rel in outputs.iter().chain(mocks.iter()) {
        let max = if rel.ends_with(".html") { MOCK_MAX } else { DOC_MAX };
        let bytes = match read_in(mado, rel, full, max) {
            Ok(Some(b)) => b,
            // (A mock-up of existing mode is checked only when it is there.)
            Ok(None) => { if !mocks.contains(rel) { missing.push(rel.clone()); } continue; }
            Err(e) => { wrong.push(format!("{rel}: {}", crate::tr::classify_in(false, &e).message)); continue; }
        };
        let problems = crate::mado_validate::file_in(rel, &bytes, existing);
        wrong.extend(problems.into_iter().map(|p| format!("{rel}: {p}")));
    }
    // A file a step may do without (`step_optional`) is held to its shape whenever it is there.
    for f in RUN_STEPS.iter().flat_map(|s| step_optional(s).iter().copied()) {
        match read_in(mado, f, full, DOC_MAX) {
            Ok(Some(b)) => wrong.extend(crate::mado_validate::file(f, &b).into_iter().map(|p| format!("{f}: {p}"))),
            Ok(None) => {}
            Err(e) => wrong.push(format!("{f}: {}", crate::tr::classify_in(false, &e).message)),
        }
    }
    // Every file a run may have touched besides its own is still checked (a change request edits several).
    for (_, f, k) in GRAPHS {
        if outputs.iter().any(|o| o == f) { continue; }
        if let Ok(Some(b)) = read_in(mado, f, full, DOC_MAX) {
            if let Err(list) = crate::mado_validate::graph(&b, k) { wrong.extend(list.into_iter().map(|p| format!("{f}: {p}"))); }
        }
    }
    if step != "plan" {
        if let Ok(Some(b)) = read_in(mado, PLAN, full, DOC_MAX) {
            if let Err(list) = crate::mado_validate::plan(&b) { wrong.extend(list.into_iter().map(|p| format!("{PLAN}: {p}"))); }
        }
    }
    // The plan is also held against the spec and the flow (traceability, dependencies, paths).
    if step == "plan" && wrong.is_empty() && missing.is_empty() {
        if let Ok(Some(plan)) = read_plan(mado, full) {
            let spec = read_text(mado, "spec.md", full).ok().flatten().unwrap_or_default();
            let flow = read_graph(mado, "flow.graph.json", "flow", full).ok().flatten();
            // (Only what would keep the review from being approved: an untraced feature is the user's to judge there.)
            for p in crate::mado_review::review_in(false, &spec, flow.as_ref(), &plan).problems.iter().filter(|p| crate::mado_review::blocks(p)) {
                wrong.push(format!("plan.json: {}", p.message));
            }
            // What a change says it touches must be features of the spec.
            if let Some(change) = read_in(mado, PLAN, full, DOC_MAX).ok().flatten().as_deref().and_then(plan_change) {
                let known: Vec<String> = crate::mado_validate::features(&spec).into_iter().map(|(id, _)| id).collect();
                for f in change.features.iter().filter(|f| !known.contains(f)) {
                    wrong.push(format!("plan.json: change.features names {f}, which is not a feature heading of spec.md (add the new feature to spec.md as \"### {f} — Name\" first)"));
                }
            }
        }
    }
    (missing, wrong)
}

/// The mode `state.json` records, read as written (`distrust` keeps the mode too, so a state file
/// the daemon did not write says the same). For the checks and the prompts, which only read `mado/`.
pub fn existing_in(mado: &Path, full: bool) -> bool {
    read_in(mado, "state.json", full, DOC_MAX).ok().flatten()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .map_or(false, |v| v["mode"] == EXISTING)
}

/// A fingerprint of every file of `mado/` a run may write (to tell whether a run changed anything).
pub fn fingerprint(mado: &Path) -> BTreeMap<String, String> {
    use sha2::Digest;
    let mut out = BTreeMap::new();
    for e in walkdir::WalkDir::new(mado).follow_links(false).max_depth(4).into_iter().flatten() {
        if !e.file_type().is_file() { continue; }
        let Ok(rel) = e.path().strip_prefix(mado) else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if DAEMON_FILES.contains(&rel.as_str()) { continue; }
        let Ok(b) = std::fs::read(e.path()) else { continue };
        out.insert(rel, format!("{:x}", sha2::Sha256::digest(&b)));
    }
    out
}

// ---------- runs ----------

/// What a chat of kind `mado` is for, from its `meta.mado`.
#[derive(Clone, Debug, Default)]
pub struct ChatRole { pub project_id: String, pub step: String }

pub fn role_of(meta: &Value) -> Option<ChatRole> {
    let m = &meta["mado"];
    Some(ChatRole { project_id: m["projectId"].as_str()?.to_string(), step: m["step"].as_str().unwrap_or_default().to_string() })
}

/// The permission mode a run of kind `mado` uses: Claude Code's own `default`, where an edit is
/// not accepted by itself. Nobody can be asked in the app (`--permission-prompts none`, no
/// `approvals` for this kind), so an edit no allow rule covers is refused: the run's writes are
/// confined by what is allowed (`write_rules`), not by listing what is not. Full access keeps the
/// mode it gives every run; every other kind keeps its own.
pub fn run_mode(kind: &str, mode: String, full: bool) -> String {
    // (Bypass is left as asked, so that it is refused for this kind like for any other.)
    if kind == "mado" && !full && mode != "plan" && mode != crate::settings::BYPASS { "default".into() } else { mode }
}

/// The rules of a run of kind `mado`, as `(deny, allow)`, in three layers:
/// 1. allow: edits under `mado/` only (in the default mode with nobody to ask, anything else is refused);
///    the run loads none of the user's or the project's own Claude Code settings (`Profile::setting_sources`),
///    so no allow rule of theirs can widen this;
/// 2. deny: everything at the top of the project that is not `mado` (`write_deny`) — a deny wins over any
///    allow, wherever that allow comes from — and the daemon's own files inside `mado/`;
/// 3. afterwards, the stray check (`mado_stray`) tells the user what changed outside `mado/` anyway.
/// `.claude`, `.git` and `.mcp.json` stay closed by the rules every run gets (`claude::protective_deny`).
pub fn write_rules(project: &Path) -> (Vec<String>, Vec<String>) { write_rules_for(project, ignores_case(project)) }

/// `write_rules` for a volume that `ignores` letter case (`Some(true)`, probed positively), tells cases
/// apart (`Some(false)`) or could not be probed (`None`). Fails closed: the deny list is always given in
/// every letter case too (as for every other run) — except the rules naming another spelling of `mado`
/// itself, which stay exact so they never close `mado/`; the allow list is folded only on a volume
/// positively known to ignore case (otherwise a folded allow would open a sibling `MADO/`).
pub fn write_rules_for(project: &Path, ignores: Option<bool>) -> (Vec<String>, Vec<String>) {
    let p = crate::claude::rule_path(project);
    let mut allow = vec![format!("Edit({p}/{DIR}/**)"), format!("Edit(./{DIR}/**)")];
    let mut deny = write_deny(project, ignores == Some(true));
    for f in DAEMON_FILES {
        deny.push(format!("Edit({p}/{DIR}/{f})"));
        deny.push(format!("Edit(./{DIR}/{f})"));
    }
    // Fold every deny rule but those whose first name is a spelling of `mado`.
    let names_mado = |r: &str| {
        let rest = r.strip_prefix(&format!("Edit({p}/")).or_else(|| r.strip_prefix("Edit(./")).unwrap_or("");
        let top = rest.trim_end_matches(')').split('/').next().unwrap_or("");
        top.eq_ignore_ascii_case(DIR)
    };
    let (exact, mut foldable): (Vec<String>, Vec<String>) = deny.into_iter().partition(|r| names_mado(r) && !r.starts_with(&format!("Edit({p}/{DIR}/")) && !r.starts_with(&format!("Edit(./{DIR}/")));
    crate::claude::every_case(&mut foldable);
    let mut deny = foldable;
    deny.extend(exact);
    if ignores == Some(true) { crate::claude::every_case(&mut allow); }
    let mut seen = std::collections::HashSet::new();
    deny.retain(|r| seen.insert(r.clone()));
    (deny, allow)
}

/// Does the volume holding `dir` ignore letter case (macOS and Windows by default)? Probed: an entry
/// of `dir` (or `dir` itself) looked up with its letters' case flipped. `Some(true)`: the same file;
/// `Some(false)`: not found, or another file; `None`: nothing to probe with.
pub fn ignores_case(dir: &Path) -> Option<bool> {
    fn flip(n: &str) -> String { n.chars().map(|c| if c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }).collect() }
    fn same(a: &Path, b: &Path) -> Option<bool> {
        let x = std::fs::symlink_metadata(a).ok()?;
        let Ok(y) = std::fs::symlink_metadata(b) else { return Some(false) };
        #[cfg(unix)] { use std::os::unix::fs::MetadataExt; Some(x.dev() == y.dev() && x.ino() == y.ino()) }
        #[cfg(not(unix))] { let _ = (x, y); Some(crate::paths::canonical(a).ok()? == crate::paths::canonical(b).ok()?) }
    }
    let lettered = |n: &str| n.chars().any(|c| c.is_ascii_alphabetic());
    let first = std::fs::read_dir(dir).ok()?.flatten().filter_map(|e| e.file_name().into_string().ok()).find(|n| lettered(n));
    if let Some(n) = first { return same(&dir.join(&n), &dir.join(flip(&n))); }
    let n = dir.file_name()?.to_str().filter(|n| lettered(n))?;
    same(dir, &dir.parent()?.join(flip(n)))
}

/// Deny rules closing every name at the top of the project that is not `mado`: each entry there now,
/// and, for names made during the run, `*.*`, every name of 1–3 and of 5 or more characters, and every
/// four-letter name differing from `mado` in one of its letters. Each also with `/**` (everything under
/// it, should it be a folder), as absolute paths only (see below). Unless the volume is known to
/// ignore case (`ignores`), only the entry spelled exactly `mado` is left open, and every other
/// spelling of it (`Mado`, `MADO`…) is denied too; on one that ignores case those are the same folder.
pub fn write_deny(project: &Path, ignores: bool) -> Vec<String> {
    use crate::claude::{glob_escape, rule_path};
    let p = rule_path(project);
    let mut names: Vec<String> = ["*.*", "?", "??", "???", "?????*"].iter().map(|s| s.to_string()).collect();
    let is_ours = |n: &str| if ignores { n.eq_ignore_ascii_case(DIR) } else { n == DIR };
    if let Ok(rd) = std::fs::read_dir(project) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if is_ours(&name) { continue; }
            names.push(glob_escape(&name));
        }
    }
    // A new four-letter name that is not `mado`: one of its letters differs. (Written with plain
    // classes: a negated one such as `[!m]` is not read the same way by every rule reader.)
    const NAME_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_ -.";
    for (i, c) in DIR.chars().enumerate() {
        let class: String = NAME_CHARS.chars().filter(|x| !x.eq_ignore_ascii_case(&c)).collect();
        let pat: String = (0..4).map(|j| if j == i { format!("[{class}]") } else { "?".to_string() }).collect();
        names.push(pat);
    }
    if !ignores {
        // Every other letter case of `mado`, which the classes above leave open.
        let chars: Vec<char> = DIR.chars().collect();
        for mask in 1..(1u32 << chars.len()) {
            names.push(chars.iter().enumerate().map(|(i, c)| if mask & (1 << i) != 0 { c.to_ascii_uppercase() } else { *c }).collect());
        }
    }
    // Absolute forms only. Claude Code reads a rule written relative to the project (`./name`) like a
    // gitignore line with no folder in it: it matches at any depth, so `./*.*` and `./?????*` closed
    // `mado/brief.md` itself and no step could save anything (0.3.0 and 0.3.1).
    let mut out: Vec<String> = names.iter().flat_map(|n| [format!("Edit({p}/{n})"), format!("Edit({p}/{n}/**)")]).collect();
    let mut seen = std::collections::HashSet::new();
    out.retain(|r| seen.insert(r.clone()));
    out
}

/// The git commands a build run may not use: the engine alone commits, merges and changes branches.
pub const BUILD_GIT_DENY: &[&str] = &["commit", "push", "checkout", "switch", "reset", "merge", "rebase", "branch", "worktree", "tag", "stash", "update-ref",
    "cherry-pick", "revert", "am", "pull", "fetch", "remote", "config", "clean"];


/// Called by `claude::start_run` for every run, once its kind is known: a run of kind `mado` gets its
/// step's instructions, works in the project only, and (unless full access is on) may write only
/// `mado/` minus the daemon's own files (the returned allow rules, with `run_mode`); its step goes `drafting`. A run of kind `mado-build` may run
/// its own task's acceptance commands (the returned allow rules) and may not write the project's own
/// folder. Every other run passes unchanged.
pub async fn prepare_run(state: &AppState, kind: &str, mut spec: crate::claude::RunSpec, full: bool) -> anyhow::Result<(crate::claude::RunSpec, Vec<String>)> {
    if kind != "mado" && kind != "mado-build" { return Ok((spec, vec![])); }
    let meta = state.chats.with(&spec.chat_id, |c| c.meta.clone()).await.unwrap_or(Value::Null);
    let Some(role) = role_of(&meta) else { return Err(crate::tr::err("mado.not_a_step", &[])) };
    // A build goes on whatever library is open (the engine started it); a planning step is the open library's.
    let project = if kind == "mado-build" { any_project_dir_in(&*state.settings.read().await, &role.project_id)? } else { project_dir(state, &role.project_id).await? };
    if kind == "mado-build" {
        // Only ever in one of the engine's own worktrees (`<home>/mado/<projectId>/build` or
        // `…/lane-<w>-<l>`), with no other folder: never the user's project, never the records beside them.
        // (Not a guard of the app's: the engine always runs there, so it holds under full access too.)
        let engine = record_dir(&role.project_id).and_then(|d| crate::paths::canonical(&d).ok());
        let cwd = crate::paths::canonical(&spec.cwd).ok();
        let inside = match (&engine, &cwd) {
            (Some(e), Some(c)) => c.strip_prefix(e).ok().and_then(|r| r.components().next()).map_or(false, |first| {
                let n = first.as_os_str().to_string_lossy();
                n == "build" || n.strip_prefix("lane-").map_or(false, |r| r.split('-').count() == 2 && r.split('-').all(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit())))
            }),
            _ => false,
        };
        if !inside || !spec.add_dirs.is_empty() { return Err(crate::tr::err("mado.build_run_place", &[])); }
        if !full {
            // Git commands that change branches or history, here too (the engine adds them as well).
            for g in BUILD_GIT_DENY { spec.extra_deny.push(format!("Bash(git {g}:*)")); }
            let p = crate::claude::rule_path(&project);
            spec.extra_deny.extend([format!("Edit({p})"), format!("Edit({p}/**)")]);
            crate::claude::every_case(&mut spec.extra_deny);
            // The approval records outside the project are not the runs' to change either.
            if let Some(d) = record_dir(&role.project_id) {
                let r = crate::claude::rule_path(&d);
                spec.extra_deny.extend(["approval.json", "state.sha256"].iter().flat_map(|f| [format!("Edit({r}/{f})"), format!("Read({r}/{f})")]));
            }
            crate::claude::every_case(&mut spec.extra_deny);
            // No link or shared file in `mado/` (its documents are copied into the worktree).
            require_plain(&mado_dir_of(&project, full)?, full)?;
        }
        // No commands at all: the run has no shell (`claude::profile`); the daemon runs the checks.
        return Ok((spec, vec![]));
    }
    if !RUN_STEPS.contains(&role.step.as_str()) { return Err(crate::tr::err("mado.not_a_step", &[])); }
    // No link or shared file in `mado/` that a write of the run would go through.
    require_plain(&project.join(DIR), full)?;
    let mut allowed = vec![];
    // Round 0.5.0: a plan of a project that is already built (its runs read the whole project).
    let existing = existing_in(&project.join(DIR), full);
    if !full {
        // Its folder is the project, and nothing besides it.
        if crate::paths::canonical(&spec.cwd).ok().as_deref() != Some(project.as_path()) { return Err(crate::tr::err("mado.path", &[("path", &spec.cwd.display().to_string())])); }
        spec.add_dirs.clear();
        let (deny, allow) = write_rules(&project);
        spec.extra_deny.extend(deny);
        allowed = allow;
        // Reading stays as for every planning run (the project, nothing outside it); an existing-mode
        // run is told to read all of it, so the files that hold its secrets are closed to it.
        if existing { spec.extra_deny.extend(existing::secret_read_deny(&project)); }
        // The records beside the approval (the approval itself, the fingerprint, the strays check's
        // "before") are not the run's to read or change.
        if let Some(d) = record_dir(&role.project_id) {
            let mut rules = vec![];
            for r in [crate::claude::rule_path(&d), crate::claude::rule_path(&crate::paths::canonical(&d).unwrap_or_else(|_| d.clone()))] {
                for tool in ["Edit", "Read"] { for at in [r.clone(), format!("{r}/**")] { let rule = format!("{tool}({at})"); if !rules.contains(&rule) { rules.push(rule); } } }
            }
            crate::claude::every_case(&mut rules);
            spec.extra_deny.extend(rules);
        }
        // The backstop: what changes outside `mado/` while it runs is told to the user afterwards.
        // The run's chat records that it was watched, and what was kept: the check fails closed.
        watch_run(state, &role.project_id, &spec.chat_id, &project).await;
    }
    let mado = mado_dir_of(&project, full)?;
    std::fs::create_dir_all(&mado)?;
    let mut ctx = crate::mado_prompts::Context::read(&mado, full);
    if existing {
        // What is in the folder, and the code map when the graph tool has one: data, never instructions.
        let found = existing::detect_trusted(&project, full).await;
        let readme = { let p = project.clone(); tokio::task::spawn_blocking(move || crate::mado_build::finish::readme(&p, full)).await.ok().flatten() };
        ctx.found = found.map(|f| existing::found_text(&f, readme.as_deref()));
        ctx.code_map = existing::code_map(state, &role.project_id).await;
    }
    let step_text = crate::mado_prompts::system(&role.step, &ctx);
    spec.append_system = Some(match spec.append_system.take() { Some(a) => format!("{step_text}\n\n{a}"), None => step_text });
    // The model and the effort are the plan's own (`/options`), whatever the step's chat was made with.
    let (model, effort) = options_for_run(state, &role.project_id).await;
    spec.options.model = model.clone();
    spec.options.effort = effort.clone();
    state.chats.update(&spec.chat_id, |c| { c.options.model = model; c.options.effort = effort; }).await;
    // Drafting from now on; how things stood is kept, for a run that ends having changed nothing.
    let before = { let st = load_state(&role.project_id, &mado, full)?.unwrap_or_default(); st.steps.get(&role.step).map(|s| s.status.clone()).unwrap_or_default() };
    let prints = fingerprint(&mado);
    state.chats.update(&spec.chat_id, |c| { c.meta["mado"]["before"] = json!({"status": before, "files": prints}); }).await;
    let step = role.step.clone();
    let chat = spec.chat_id.clone();
    with_state(state, &role.project_id, |st| {
        if let Some(e) = st.steps.get_mut(&step) { e.status = "drafting".into(); e.chat_id = Some(chat); }
        Ok(())
    }).await?;
    Ok((spec, allowed))
}

/// Called by `claude::finish` after every run: a run of kind `mado` has its files checked. Invalid
/// output gets one automatic repair turn in the same chat naming exactly what is wrong; still
/// invalid, the user is told in the chat. Valid, the step is up for review. The comments it was
/// sent are resolved (or open again, when the run did not end well), and every text comment is
/// found again in the documents as they are now.
pub fn after_run(state: Arc<AppState>, chat_id: String, ok: bool) -> futures::future::BoxFuture<'static, ()> {
    Box::pin(async move {
        let Some((kind, meta)) = state.chats.with(&chat_id, |c| (c.kind.clone(), c.meta.clone())).await else { return };
        if kind != "mado" { return; }
        let Some(role) = role_of(&meta) else { return };
        if let Err(e) = settle(&state, &chat_id, &role, &meta, ok).await {
            crate::log::warn(format!("mado: after the run of {chat_id}: {e:#}"));
        }
    })
}

/// What a planning run changed outside `mado/` (the deny rules should have made that impossible):
/// said in the chat and kept on the step. Nothing is put back. A repair turn adds to its first turn's list.
async fn report_strays(state: &AppState, chat_id: &str, role: &ChatRole, repairing: bool, found: Option<crate::mado_stray::Found>) -> anyhow::Result<()> {
    if found == Some(crate::mado_stray::Found::Unchecked) {
        crate::claude::push(state, chat_id, crate::chats::Msg::new("error", crate::tr::text("mado.stray_unchecked", &[]))).await;
        let step = role.step.clone();
        return with_state(state, &role.project_id, |st| { if let Some(e) = st.steps.get_mut(&step) { e.stray_unchecked = Some(true); } Ok(()) }).await;
    }
    if let Some(crate::mado_stray::Found::Checked(list, capped)) = found {
        if !list.is_empty() || capped {
            let mut text = String::new();
            if !list.is_empty() {
                let shown = list.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n");
                text.push_str(&format!("{}\n\n{shown}", crate::tr::text("mado.strayed", &[])));
            }
            if capped { if !text.is_empty() { text.push_str("\n\n"); } text.push_str(&crate::tr::text("mado.strayed_capped", &[("n", &crate::mado_stray::WALK_CAP.to_string())])); }
            crate::claude::push(state, chat_id, crate::chats::Msg::new("error", &text)).await;
        }
        let step = role.step.clone();
        with_state(state, &role.project_id, |st| {
            if let Some(e) = st.steps.get_mut(&step) {
                let mut all = if repairing { e.strayed.clone().unwrap_or_default() } else { vec![] };
                for p in list { if !all.contains(&p) { all.push(p); } }
                all.truncate(crate::mado_stray::LIST_CAP);
                e.strayed = (!all.is_empty()).then_some(all);
                // A check that could be made clears an earlier one that could not (a repair turn keeps it).
                if !repairing { e.stray_unchecked = None; }
            }
            Ok(())
        }).await?;
    }
    Ok(())
}

/// Keep how the project stands before a planning run, and record in its chat that it was watched
/// and what was kept (`mado_stray::META_KEY`), so its check fails closed.
async fn watch_run(state: &AppState, project_id: &str, chat_id: &str, project: &Path) {
    // Marked first, so the file is never there without a chat naming it (`check_cut_off_runs`).
    state.chats.update(chat_id, |c| { c.meta["mado"][crate::mado_stray::META_KEY] = json!({"sha256": ""}); }).await;
    let kept = crate::mado_stray::before_run(state, project_id, chat_id, project).await;
    state.chats.update(chat_id, |c| { c.meta["mado"][crate::mado_stray::META_KEY] = json!({"sha256": kept}); }).await;
}

/// The hash of the "before" a run's chat recorded (`None`: the run was not watched).
fn kept_hash(meta: &Value) -> Option<String> {
    let v = meta["mado"].get(crate::mado_stray::META_KEY)?;
    Some(v["sha256"].as_str().unwrap_or_default().to_string())
}

/// The run's check is done (reported): its chat no longer says it is waiting for one.
async fn unmark(state: &AppState, chat_id: &str) {
    state.chats.update(chat_id, |c| { if let Some(o) = c.meta["mado"].as_object_mut() { o.remove(crate::mado_stray::META_KEY); } }).await;
}

/// At the start of the daemon: planning runs a restart cut off. They are found from their chats,
/// which still carry the mark a watched run gets (`mado_stray::META_KEY`), so a run whose kept
/// "before" was deleted is found too (and is then reported as one that could not be checked).
/// Each is compared with the project as it is now, told and kept on its step as after any run, and
/// unmarked. (What changed includes what anyone did while the daemon was down: the list says what
/// to look at, never who did it.) A run going on is left alone; one whose project is not among the
/// open library's is checked on a later start. Kept files no chat refers to are dropped.
/// `only`: one project's runs (the tests, which share one home).
pub async fn check_cut_off_runs(state: &AppState, only: Option<&str>) {
    let mut named: std::collections::HashSet<(String, String)> = Default::default();
    for c in state.chats.list().await {
        if c.kind != "mado" || c.meta["mado"].get(crate::mado_stray::META_KEY).is_none() { continue; }
        let Some(role) = role_of(&c.meta) else { continue };
        if only.is_some_and(|o| o != role.project_id) { continue; }
        named.insert((role.project_id.clone(), c.id.clone()));
        if c.running || state.runs.lock().await.contains_key(&c.id) { continue; }
        let Ok(project) = project_dir(state, &role.project_id).await else { continue };
        let repairing = c.meta["mado"]["repair"].as_bool().unwrap_or(false);
        let found = crate::mado_stray::after_run(state, &role.project_id, &c.id, &project, kept_hash(&c.meta).as_deref()).await;
        match report_strays(state, &c.id, &role, repairing, found).await {
            Ok(()) => unmark(state, &c.id).await,
            Err(e) => crate::log::warn(format!("mado: the check of a run cut off by a restart ({}): {e:#}", c.id)),
        }
    }
    for (pid, chat) in crate::mado_stray::kept_runs() {
        if only.is_some_and(|o| o != pid) || named.contains(&(pid.clone(), chat.clone())) { continue; }
        // Marked since the list above was made: a run just starting.
        if state.chats.with(&chat, |c| c.meta["mado"].get(crate::mado_stray::META_KEY).is_some()).await == Some(true) { continue; }
        crate::mado_stray::discard(&pid, &chat);
    }
}

async fn settle(state: &Arc<AppState>, chat_id: &str, role: &ChatRole, meta: &Value, ok: bool) -> anyhow::Result<()> {
    let full = state.settings.read().await.full_access();
    let project = project_dir(state, &role.project_id).await?;
    let mado = mado_dir_of(&project, full)?;
    let repairing = meta["mado"]["repair"].as_bool().unwrap_or(false);
    let found = crate::mado_stray::after_run(state, &role.project_id, chat_id, &project, kept_hash(meta).as_deref()).await;
    report_strays(state, chat_id, role, repairing, found).await?;
    unmark(state, chat_id).await;
    let before = meta["mado"]["before"]["status"].as_str().unwrap_or_default().to_string();
    let before_files: BTreeMap<String, String> = serde_json::from_value(meta["mado"]["before"]["files"].clone()).unwrap_or_default();
    let changed = fingerprint(&mado) != before_files;
    let (missing, wrong) = check_step(&mado, &role.step, full);
    let produced = step_outputs(&role.step).iter().any(|f| !missing.iter().any(|m| m == f));
    state.chats.update(chat_id, |c| { if let Some(o) = c.meta["mado"].as_object_mut() { o.remove("repair"); } }).await;
    // Partly written (something there, something missing) is as wrong as a bad file.
    let mut wrong = wrong;
    if produced && !missing.is_empty() { wrong.extend(missing.iter().map(|m| format!("{m}: this file is missing"))); }
    let status = if !changed && matches!(before.as_str(), "approved" | "stale" | "review") { before.clone() }
        else if !produced && wrong.is_empty() { "drafting".to_string() }
        else { "review".to_string() };
    if ok && !wrong.is_empty() && !repairing && changed {
        state.chats.update(chat_id, |c| { c.meta["mado"]["repair"] = json!(true); }).await;
        let text = crate::mado_prompts::repair(&wrong);
        let (st, id) = (state.clone(), chat_id.to_string());
        // (Started off this run's end: this one is over by now.)
        tokio::spawn(async move {
            if let Err(e) = crate::api::send_text(st.clone(), id.clone(), text).await {
                crate::log::warn(format!("mado: the repair turn of {id} did not start: {e:#}"));
            }
        });
        return Ok(());
    }
    if !wrong.is_empty() {
        let list = wrong.iter().map(|w| format!("- {w}")).collect::<Vec<_>>().join("\n");
        let text = format!("{}\n\n{list}", crate::tr::text("mado.still_invalid", &[]));
        crate::claude::push(state, chat_id, crate::chats::Msg::new("error", &text)).await;
    }
    let step = role.step.clone();
    with_state(state, &role.project_id, |st| {
        if let Some(e) = st.steps.get_mut(&step) { if e.status == "drafting" || status != "drafting" { e.status = status.clone(); } }
        Ok(())
    }).await?;
    crate::mado_comments::after_run(state, &role.project_id, chat_id, ok && wrong.is_empty()).await?;
    tell(state, &role.project_id, "doc").await;
    tell(state, &role.project_id, "graph").await;
    if role.step == "design" { tell(state, &role.project_id, "design").await; }
    Ok(())
}

/// The chat of `step`, made when it has none yet (one chat per step, kept in the state).
async fn step_chat(state: &AppState, project_id: &str, step: &str) -> anyhow::Result<String> {
    let full = state.settings.read().await.full_access();
    let mado = mado_dir(state, project_id).await?;
    let st = load_state(project_id, &mado, full)?.ok_or_else(|| crate::tr::err("mado.not_started", &[]))?;
    if let Some(id) = st.steps.get(step).and_then(|s| s.chat_id.clone()) {
        if state.chats.get(&id).await.is_some() { return Ok(id); }
    }
    let s = state.settings.read().await.clone();
    let project = s.projects.iter().find(|p| p.id == project_id).map(|p| p.name.clone()).unwrap_or_default();
    let step_name = crate::tr::text(&format!("mado.step.{step}"), &[]);
    let mut chat = crate::chats::Chat { kind: "mado".into(), project_id: Some(project_id.to_string()), ..Default::default() };
    chat.title = crate::tr::text("mado.chat_title", &[("step", &step_name), ("project", &project)]);
    // Planning runs are Claude Code's (`assistant::for_run`); their files are edited without asking.
    chat.provider = crate::assistant::Program::Claude.name().into();
    let (model, effort) = crate::assistant::chat_defaults(&s, crate::assistant::Program::Claude);
    chat.options = crate::chats::RunOptions { model, effort, permission_mode: Some(crate::settings::DEFAULT_PERMISSION_MODE.into()), ..Default::default() };
    chat.meta = json!({"mado": {"projectId": project_id, "step": step}});
    let id = chat.id.clone();
    state.chats.insert(chat).await;
    let sid = step.to_string();
    let cid = id.clone();
    with_state(state, project_id, |st| { if let Some(e) = st.steps.get_mut(&sid) { e.chat_id = Some(cid); } Ok(()) }).await?;
    Ok(id)
}

/// Start (or continue) the run of `step` with `text` as the user's message.
async fn run_step(state: &Arc<AppState>, project_id: &str, step: &str, text: String) -> anyhow::Result<String> {
    if !RUN_STEPS.contains(&step) { return Err(crate::tr::err("mado.no_run", &[])); }
    let full = state.settings.read().await.full_access();
    let project = project_dir(state, project_id).await?;
    if !full { crate::trust::require(&project)?; }
    let mado = mado_dir_of(&project, full)?;
    let st = load_state(project_id, &mado, full)?.ok_or_else(|| crate::tr::err("mado.not_started", &[]))?;
    if st.steps.get(step).map_or(true, |s| s.status == "locked") { return Err(crate::tr::err("mado.locked", &[])); }
    let id = step_chat(state, project_id, step).await?;
    crate::api::send_text(state.clone(), id.clone(), text).await?;
    Ok(id)
}

// ---------- routes ----------

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/mado", get(plans))
        .route("/api/mado/:project", get(overview))
        .route("/api/mado/:project/options", get(options_get).put(options_put))
        .route("/api/mado/:project/start", post(start_route))
        .route("/api/mado/:project/doc/:name", get(doc_get).put(doc_put))
        .route("/api/mado/:project/graph/:name", get(graph_get))
        .route("/api/mado/:project/graph/:name/positions", put(graph_positions))
        .route("/api/mado/:project/plan", get(plan_get))
        .route("/api/mado/:project/steps/:step/run", post(step_run))
        .route("/api/mado/:project/steps/:step/approve", post(step_approve))
        .route("/api/mado/:project/steps/:step/reopen", post(step_reopen))
        .route("/api/mado/:project/steps/:step/strayed/dismiss", post(strayed_dismiss))
        .route("/api/mado/:project/comments", get(comments_get).post(comments_add))
        .route("/api/mado/:project/comments/:id", patch(comments_patch).delete(comments_delete))
        .route("/api/mado/:project/changes", post(changes))
        .route("/api/mado/:project/review", get(review_get))
}

async fn ctx(st: &AppState, project_id: &str) -> anyhow::Result<(PathBuf, PathBuf, bool)> {
    let full = st.settings.read().await.full_access();
    let project = project_dir(st, project_id).await?;
    let mado = mado_dir_of(&project, full)?;
    Ok((project, mado, full))
}

/// Every file of `mado/` that is there, relative (the daemon's own included).
fn files_of(mado: &Path) -> Vec<String> {
    let mut out = vec![];
    for e in walkdir::WalkDir::new(mado).follow_links(false).max_depth(4).into_iter().flatten() {
        if !e.file_type().is_file() { continue; }
        if let Ok(rel) = e.path().strip_prefix(mado) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if clean_rel(&rel) { out.push(rel); }
        }
    }
    out.sort();
    out
}

/// The review is no longer approved when the plan changed since (its hash is what allows the build).
/// (`load_state` already made the review stale where its approval record does not match the plan,
/// and distrusted a state.json the daemon did not write: the file is rewritten when it says otherwise.)
fn reconcile(_pid: &str, mado: &Path, st: &MadoState, full: bool) -> bool {
    let raw = read_in(mado, "state.json", full, DOC_MAX).ok().flatten().and_then(|b| serde_json::from_slice::<MadoState>(&b).ok());
    raw.map_or(true, |mut r| { normalize(&mut r); r != *st })
}

/// `GET /api/mado/:project` → `{project, exists, state, files}`.
async fn overview(State(st): S, UrlPath(pid): UrlPath<String>) -> R {
    let (project, mado, full) = ctx(&st, &pid).await?;
    let name = st.settings.read().await.projects.iter().find(|p| p.id == pid).map(|p| p.name.clone()).unwrap_or_default();
    // Round 0.5.0: what the folder holds (names only, bounded; only for a folder the user trusted).
    let detected = existing::detect_trusted(&project, full).await.map(|f| f.detected);
    let lock = lock_of(&pid);
    let _held = lock.lock().await;
    let state = load_state(&pid, &mado, full)?;
    if let Some(s) = state.as_ref() { if reconcile(&pid, &mado, s, full) { save_state(&pid, &mado, s, full)?; } }
    let files = if mado.is_dir() { files_of(&mado) } else { vec![] };
    Ok(Json(json!({"project": {"id": pid, "name": name, "path": project.to_string_lossy()}, "exists": state.is_some(), "state": state, "files": files, "detected": detected})))
}

/// `POST /start`: make `mado/` and its state (trust required, as for linking the project).
/// (The tests' way in; the route is `start_route`.)
#[cfg(test)]
async fn start(State(st): S, UrlPath(pid): UrlPath<String>) -> R { Ok(Json(start_in(&st, &pid, None).await?)) }

/// `POST /start {mode?: "new" | "existing"}` (round 0.5.0). No body, or no `mode`: as before 0.5.0 (a
/// new plan is a new project's; a started plan keeps its mode). Anything else in the body is refused.
async fn start_route(State(st): S, UrlPath(pid): UrlPath<String>, body: axum::body::Bytes) -> R {
    let mode = parse_mode(&body)?;
    Ok(Json(start_in(&st, &pid, mode).await?))
}

/// The mode a `POST /start` body asks for: `None` (nothing asked), `Some(None)` (a new project),
/// `Some(Some("existing"))`.
pub fn parse_mode(body: &[u8]) -> anyhow::Result<Option<Option<String>>> {
    if body.iter().all(u8::is_ascii_whitespace) { return Ok(None); }
    if body.len() > 4096 { return Err(crate::tr::err("mado.bad_mode", &[])); }
    let v: Value = serde_json::from_slice(body).map_err(|_| crate::tr::err("mado.bad_mode", &[]))?;
    let Some(o) = v.as_object() else { return Err(crate::tr::err("mado.bad_mode", &[])) };
    if o.keys().any(|k| k != "mode") { return Err(crate::tr::err("mado.bad_mode", &[])); }
    match o.get("mode") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(m)) if m == "new" => Ok(Some(None)),
        Some(Value::String(m)) if m == EXISTING => Ok(Some(Some(EXISTING.into()))),
        _ => Err(crate::tr::err("mado.bad_mode", &[])),
    }
}

/// Start the plan (or answer with the one there is), in `mode` when one is asked. The mode of a
/// started plan changes only while none of its steps is approved (or stale) and no run of its brief
/// is under way: an approved step was written for the mode it has.
pub async fn start_in(st: &AppState, pid: &str, mode: Option<Option<String>>) -> anyhow::Result<Value> {
    let (project, mado, full) = ctx(st, pid).await?;
    if !full { crate::trust::require(&project)?; }
    require_plain(&mado, full)?;
    let lock = lock_of(pid);
    let held = lock.lock().await;
    let state = match load_state(pid, &mado, full)? {
        Some(mut s) => {
            if let Some(m) = mode.filter(|m| *m != s.mode) {
                let brief = s.steps.get("brief").cloned().unwrap_or_default();
                if s.steps.values().any(|e| matches!(e.status.as_str(), "approved" | "stale")) {
                    return Err(crate::tr::err("mado.mode_locked", &[]));
                }
                if let Some(chat) = brief.chat_id.as_deref() {
                    if st.chats.with(chat, |c| c.running).await == Some(true) { return Err(crate::tr::err("mado.busy", &[])); }
                }
                s.mode = m;
                normalize(&mut s);
                save_state(pid, &mado, &s, full)?;
            }
            s
        }
        None => {
            let mut s = MadoState::default();
            s.mode = mode.flatten();
            normalize(&mut s);
            save_state(pid, &mado, &s, full)?;
            s
        }
    };
    drop(held);
    tell(st, pid, "state").await;
    Ok(json!({"ok": true, "state": state}))
}

async fn doc_get(State(st): S, UrlPath((pid, name)): UrlPath<(String, String)>) -> R {
    let (_, mado, full) = ctx(&st, &pid).await?;
    let file = doc_file(&name).ok_or_else(|| crate::tr::err("mado.unknown_name", &[("name", &name)]))?;
    let text = read_text(&mado, file, full)?;
    Ok(Json(json!({"text": text.clone().unwrap_or_default(), "exists": text.is_some()})))
}

#[derive(Deserialize)]
struct DocBody { text: String }

/// `PUT /doc/:name {text}`: the user's own edit. An approved step goes back to review (approving it
/// again marks what builds on it stale); its text comments are found again.
async fn doc_put(State(st): S, UrlPath((pid, name)): UrlPath<(String, String)>, Json(b): Json<DocBody>) -> R {
    let (_, mado, full) = ctx(&st, &pid).await?;
    let file = doc_file(&name).ok_or_else(|| crate::tr::err("mado.unknown_name", &[("name", &name)]))?;
    if b.text.len() > DOC_MAX { return Err(crate::tr::err("mado.too_large", &[("path", file)]).into()); }
    let state = load_state(&pid, &mado, full)?.ok_or_else(|| crate::tr::err("mado.not_started", &[]))?;
    // The step the document belongs to (`conventions` is the tech step's).
    let step = step_of_file(file).unwrap_or(name.as_str()).to_string();
    if let Some(chat) = state.steps.get(&step).and_then(|s| s.chat_id.clone()) {
        if st.chats.with(&chat, |c| c.running).await == Some(true) { return Err(crate::tr::err("mado.busy", &[]).into()); }
    }
    write_in(&mado, file, b.text.as_bytes(), full)?;
    with_state(&st, &pid, |s| {
        if let Some(e) = s.steps.get_mut(&step) { if e.status == "approved" || e.status == "stale" { e.status = "review".into(); e.approved_at = None; } }
        Ok(())
    }).await?;
    crate::mado_comments::reanchor_project(&st, &pid).await?;
    tell(&st, &pid, "doc").await;
    Ok(Json(json!({"ok": true})))
}

async fn graph_get(State(st): S, UrlPath((pid, name)): UrlPath<(String, String)>) -> R {
    let (_, mado, full) = ctx(&st, &pid).await?;
    if name == "tasks" {
        let plan = read_plan(&mado, full)?.ok_or_else(|| crate::tr::err("mado.missing", &[("path", PLAN)]))?;
        return Ok(Json(serde_json::to_value(tasks_graph(&plan))?));
    }
    let (file, kind) = graph_file(&name).ok_or_else(|| crate::tr::err("mado.unknown_name", &[("name", &name)]))?;
    let g = read_graph(&mado, file, kind, full)?.ok_or_else(|| crate::tr::err("mado.missing", &[("path", file)]))?;
    Ok(Json(serde_json::to_value(g)?))
}

#[derive(Deserialize)]
struct Positions { positions: HashMap<String, Pos> }

/// `PUT /graph/:name/positions`: where the nodes are drawn, nothing else. Unknown ids are ignored.
async fn graph_positions(State(st): S, UrlPath((pid, name)): UrlPath<(String, String)>, Json(b): Json<Positions>) -> R {
    let (_, mado, full) = ctx(&st, &pid).await?;
    if name == "tasks" { return Err(crate::tr::err("mado.read_only", &[]).into()); }
    let (file, kind) = graph_file(&name).ok_or_else(|| crate::tr::err("mado.unknown_name", &[("name", &name)]))?;
    if b.positions.len() > crate::mado_validate::MAX_NODES { return Err(crate::tr::err("mado.too_large", &[("path", file)]).into()); }
    for p in b.positions.values() {
        if !crate::mado_validate::pos_ok(p) { return Err(crate::tr::err("mado.bad_request", &[]).into()); }
    }
    let lock = lock_of(&pid);
    let _held = lock.lock().await;
    let mut g = read_graph(&mado, file, kind, full)?.ok_or_else(|| crate::tr::err("mado.missing", &[("path", file)]))?;
    for n in g.nodes.iter_mut() { if let Some(p) = b.positions.get(&n.id) { n.pos = Some(p.clone()); } }
    write_in(&mado, file, &serde_json::to_vec_pretty(&g)?, full)?;
    drop(_held);
    tell(&st, &pid, "graph").await;
    Ok(Json(json!({"ok": true})))
}

async fn plan_get(State(st): S, UrlPath(pid): UrlPath<String>) -> R {
    let (_, mado, full) = ctx(&st, &pid).await?;
    let plan = read_plan(&mado, full)?.ok_or_else(|| crate::tr::err("mado.missing", &[("path", PLAN)]))?;
    Ok(Json(serde_json::to_value(plan)?))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RunBody { note: Option<String> }

/// `POST /steps/:step/run {note?}` → `{chatId}`.
async fn step_run(State(st): S, UrlPath((pid, step)): UrlPath<(String, String)>, body: Option<Json<RunBody>>) -> R {
    let note = body.and_then(|b| b.0.note).filter(|n| !n.trim().is_empty());
    if note.as_ref().map_or(false, |n| n.len() > 20_000) { return Err(crate::tr::err("mado.too_large", &[("path", "note")]).into()); }
    let (_, mado, full) = ctx(&st, &pid).await?;
    let state = load_state(&pid, &mado, full)?.ok_or_else(|| crate::tr::err("mado.not_started", &[]))?;
    let again = state.steps.get(&step).map_or(false, |s| s.status != "ready");
    let text = crate::mado_prompts::run_message(&step, again, note.as_deref());
    let id = run_step(&st, &pid, &step, text).await?;
    Ok(Json(json!({"chatId": id})))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ApproveBody { plan_hash: Option<String> }

/// `POST /steps/:step/approve`: only with every file of the step there and valid. Approving the
/// review takes `{planHash}`, the hash its `GET /review` answer carried: it must be the hash of
/// `plan.json` now (else the plan changed after the user saw it, and nothing is approved), and it is
/// kept as `state.plan_hash` (what allows the build to run the commands the user was shown).
async fn step_approve(State(st): S, UrlPath((pid, step)): UrlPath<(String, String)>, q: axum::extract::Query<HashMap<String, String>>, body: Option<Json<ApproveBody>>) -> R {
    let shown = body.and_then(|b| b.0.plan_hash).filter(|h| !h.is_empty());
    let (project, mado, full) = ctx(&st, &pid).await?;
    if !STEPS.contains(&step.as_str()) { return Err(crate::tr::err("mado.unknown_name", &[("name", &step)]).into()); }
    let state = load_state(&pid, &mado, full)?.ok_or_else(|| crate::tr::err("mado.not_started", &[]))?;
    if !approvable(&state, &step) { return Err(crate::tr::err("mado.not_approvable", &[]).into()); }
    if let Some(list) = state.steps.get(&step).and_then(|s| s.strayed.clone()).filter(|l| !l.is_empty()) {
        return Err(crate::tr::err_detail("mado.strayed_block", &[], list.join("\n")).into());
    }
    if state.steps.get(&step).and_then(|s| s.stray_unchecked) == Some(true) {
        return Err(crate::tr::err("mado.stray_unchecked_block", &[]).into());
    }
    if let Some(chat) = state.steps.get(&step).and_then(|s| s.chat_id.clone()) {
        if st.chats.with(&chat, |c| c.running).await == Some(true) { return Err(crate::tr::err("mado.busy", &[]).into()); }
    }
    let mut hash = None;
    if step == "review" {
        let bytes = read_in(&mado, PLAN, full, DOC_MAX)?.ok_or_else(|| crate::tr::err("mado.missing", &[("path", PLAN)]))?;
        let plan = crate::mado_validate::plan(&bytes).map_err(|l| crate::tr::err_detail("mado.invalid", &[("path", PLAN)], l.join("\n")))?;
        let spec = read_text(&mado, "spec.md", full)?.unwrap_or_default();
        let flow = read_graph(&mado, "flow.graph.json", "flow", full).ok().flatten();
        let mut r = review(&spec, flow.as_ref(), &plan);
        r.problems.extend(repo_problems(&st, &project).await);
        let blocking: Vec<String> = r.problems.iter().filter(|p| crate::mado_review::blocks(p)).map(|p| p.message.clone()).collect();
        if !blocking.is_empty() { return Err(crate::tr::err_detail("mado.review_blocked", &[], blocking.join("\n")).into()); }
        let now = plan_hash(&bytes);
        if shown.as_deref() != Some(now.as_str()) { return Err(crate::tr::err("mado.plan_changed", &[]).into()); }
        // Approving lets the build run these commands: the user's yes in the desktop shell's own
        // dialog, which listed them, for exactly this plan (`?approval=`). The app token alone is not a yes.
        if !full && !crate::consent::verify(q.get("approval").map(String::as_str), crate::consent::Action::MadoApprove, &crate::consent::mado_subject(&pid, &now)) {
            return Err(crate::tr::err("mado.approve_unconfirmed", &[]).into());
        }
        hash = Some((now, r.commands.clone()));
    } else {
        let (missing, wrong) = check_step(&mado, &step, full);
        if !missing.is_empty() || !wrong.is_empty() {
            let list: Vec<String> = missing.iter().map(|m| format!("{m}: missing")).chain(wrong).collect();
            return Err(crate::tr::err_detail("mado.not_ready", &[], list.join("\n")).into());
        }
    }
    let now = chrono::Utc::now().to_rfc3339();
    let s = step.clone();
    let pid2 = pid.clone();
    let out = with_state(&st, &pid, move |state| {
        approve_in(state, &s, &now);
        // Round 0.5.0: the documents of an existing project now describe it as approved.
        if s == "tech" && state.existing() { state.documented = Some(Documented { at: now.clone() }); }
        if let Some((h, commands)) = hash.clone() {
            // What allows the build to run commands: kept outside the project, never in state.json.
            write_approval(&pid2, &Approval { plan_hash: h.clone(), commands, approved_at: now.clone() })?;
            state.plan_hash = Some(h);
        }
        Ok(state.clone())
    }).await?;
    Ok(Json(json!({"ok": true, "state": out})))
}

async fn step_reopen(State(st): S, UrlPath((pid, step)): UrlPath<(String, String)>) -> R {
    if !STEPS.contains(&step.as_str()) || step == "build" { return Err(crate::tr::err("mado.unknown_name", &[("name", &step)]).into()); }
    let s = step.clone();
    let out = with_state(&st, &pid, move |state| { reopen_in(state, &s); Ok(state.clone()) }).await?;
    Ok(Json(json!({"ok": true, "state": out})))
}

/// `POST /steps/:step/strayed/dismiss`: the user has seen what the run changed outside `mado/`.
async fn strayed_dismiss(State(st): S, UrlPath((pid, step)): UrlPath<(String, String)>) -> R {
    if !RUN_STEPS.contains(&step.as_str()) { return Err(crate::tr::err("mado.unknown_name", &[("name", &step)]).into()); }
    let out = with_state(&st, &pid, move |state| { if let Some(e) = state.steps.get_mut(&step) { e.strayed = None; e.stray_unchecked = None; } Ok(state.clone()) }).await?;
    Ok(Json(json!({"ok": true, "state": out})))
}

async fn comments_get(State(st): S, UrlPath(pid): UrlPath<String>) -> R {
    let (_, mado, full) = ctx(&st, &pid).await?;
    Ok(Json(serde_json::to_value(crate::mado_comments::load(&mado, full)?)?))
}

#[derive(Deserialize)]
struct NewComment { target: CommentTarget, body: String }

async fn comments_add(State(st): S, UrlPath(pid): UrlPath<String>, Json(b): Json<NewComment>) -> R {
    let c = crate::mado_comments::add(&st, &pid, b.target, b.body).await?;
    Ok(Json(serde_json::to_value(c)?))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CommentPatch { body: Option<String>, status: Option<String> }

async fn comments_patch(State(st): S, UrlPath((pid, id)): UrlPath<(String, String)>, Json(b): Json<CommentPatch>) -> R {
    let c = crate::mado_comments::change(&st, &pid, &id, b.body, b.status).await?;
    Ok(Json(serde_json::to_value(c)?))
}

async fn comments_delete(State(st): S, UrlPath((pid, id)): UrlPath<(String, String)>) -> R {
    crate::mado_comments::remove(&st, &pid, &id).await?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize, Clone, Debug)]
pub struct OpsOnFile { pub file: String, pub ops: Vec<GraphOp> }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangesBody { step: String, #[serde(default)] comment_ids: Vec<String>, #[serde(default)] graph_ops: Vec<OpsOnFile>, #[serde(default)] note: Option<String> }

/// `POST /changes {step, commentIds, graphOps?, note?}` → `{chatId}`: one run applies everything,
/// to the diagram and to the passages of the documents that describe the same thing.
async fn changes(State(st): S, UrlPath(pid): UrlPath<String>, Json(b): Json<ChangesBody>) -> R {
    if !RUN_STEPS.contains(&b.step.as_str()) { return Err(crate::tr::err("mado.no_run", &[]).into()); }
    let (_, mado, full) = ctx(&st, &pid).await?;
    let all = crate::mado_comments::load(&mado, full)?;
    let mut chosen = vec![];
    for id in &b.comment_ids {
        let c = all.comments.iter().find(|c| &c.id == id).ok_or_else(|| crate::tr::err("mado.no_comment", &[]))?;
        chosen.push(c.clone());
    }
    let mut ops = vec![];
    for g in &b.graph_ops {
        if !GRAPHS.iter().any(|(_, f, _)| *f == g.file) { return Err(crate::tr::err("mado.unknown_name", &[("name", &g.file)]).into()); }
        if g.ops.len() > 500 { return Err(crate::tr::err("mado.too_large", &[("path", &g.file)]).into()); }
        check_ops(&g.ops)?;
        ops.push(g.clone());
    }
    if chosen.is_empty() && ops.iter().all(|g| g.ops.is_empty()) && b.note.as_deref().map_or(true, |n| n.trim().is_empty()) {
        return Err(crate::tr::err("mado.nothing_to_send", &[]).into());
    }
    let text = crate::mado_comments::change_prompt(&mado, full, &chosen, &ops, b.note.as_deref());
    if text.len() > 200_000 { return Err(crate::tr::err("mado.too_large", &[("path", "changes")]).into()); }
    let chat = step_chat(&st, &pid, &b.step).await?;
    let ids: Vec<String> = chosen.iter().map(|c| c.id.clone()).collect();
    crate::mado_comments::mark_sent(&st, &pid, &ids, &chat).await?;
    match run_step(&st, &pid, &b.step, text).await {
        Ok(id) => Ok(Json(json!({"chatId": id}))),
        Err(e) => { let _ = crate::mado_comments::unsend(&st, &pid, &ids).await; Err(e.into()) }
    }
}

/// A node an op names: a real id, or `new:<n>` — the n-th `add-node` of the same list (1-based),
/// for a node the user added in the window, which has no id yet. Only where an op may name such a
/// node (`add-edge` ends, `set-prop`, `add-node.near`); everything else takes a real id.
pub fn new_ref(r: &str, added: usize) -> Option<usize> {
    let n: usize = r.strip_prefix("new:")?.parse().ok()?;
    (n >= 1 && n <= added && r == format!("new:{n}")).then_some(n)
}

pub fn check_ops(ops: &[GraphOp]) -> anyhow::Result<()> {
    let added = ops.iter().filter(|o| matches!(o, GraphOp::AddNode { .. })).count();
    let bad = |id: &str| crate::tr::err("mado.bad_op", &[("id", &id.chars().take(80).collect::<String>())]);
    let node = |id: &str, new_ok: bool| -> anyhow::Result<()> {
        if crate::mado_validate::id_ok(id) || (new_ok && new_ref(id, added).is_some()) { Ok(()) } else { Err(bad(id)) }
    };
    let text = |t: &str, max: usize| -> anyhow::Result<()> { if t.chars().count() <= max { Ok(()) } else { Err(crate::tr::err("mado.bad_request", &[])) } };
    for op in ops {
        match op {
            GraphOp::Rename { id, label } => { node(id, false)?; text(label, 300)?; }
            GraphOp::AddNode { label, kind, near } => { text(label, 300)?; node(kind, false)?; if let Some(n) = near { node(n, true)?; } }
            GraphOp::RemoveNode { id } | GraphOp::RemoveEdge { id } => node(id, false)?,
            GraphOp::AddEdge { from, to, label, kind } => { node(from, true)?; node(to, true)?; text(label.as_deref().unwrap_or(""), 300)?; if let Some(k) = kind { if !k.is_empty() { text(k, 40)?; if k.chars().any(char::is_control) { return Err(bad(k)); } } } }
            GraphOp::RelabelEdge { id, label } => { node(id, false)?; text(label, 300)?; }
            GraphOp::SetProp { id, key, value } => { node(id, true)?; text(key, 100)?; if serde_json::to_string(value).map_or(true, |v| v.len() > 10_000) { return Err(crate::tr::err("mado.bad_request", &[])); } }
        }
    }
    Ok(())
}

// ---------- the model and the effort of a project's runs ----------

/// What the user chose for every run of this project's plan and build (`null` = the app's default).
/// Kept beside the approval record, in the app's own folder: never in the project.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Options { pub model: Option<String>, pub effort: Option<String> }

const OPTIONS_FILE: &str = "options.json";

/// A value of the options as it may be kept and used: a plain token (`settings::check_cli_value`,
/// the check every chat's model and effort pass), nothing for an empty one.
fn option_value(what: &str, v: Option<String>) -> anyhow::Result<Option<String>> {
    let v = v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    crate::settings::check_cli_value(what, v.as_deref()).map_err(|_| crate::tr::err("mado.options.value", &[]))?;
    Ok(v)
}

/// The options on record. Anything that is not the plain file the daemon wrote, or holds a value
/// that would not be accepted today, counts as not chosen.
pub fn options_in(project_id: &str) -> Options {
    let Some(f) = record_dir(project_id).map(|d| d.join(OPTIONS_FILE)) else { return Options::default() };
    let plain = std::fs::symlink_metadata(&f).map_or(false, |m| m.is_file() && m.len() <= 4096);
    if !plain { return Options::default(); }
    let o: Options = std::fs::read(&f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    Options { model: option_value("model", o.model).unwrap_or(None), effort: option_value("effort", o.effort).unwrap_or(None) }
}

/// The model and the effort every run of this project's plan and build uses (`None` = the app's
/// default): what `PUT /options` recorded. The build engine calls this for its runs.
pub async fn run_options(_state: &AppState, project_id: &str) -> (Option<String>, Option<String>) {
    let o = options_in(project_id);
    (o.model, o.effort)
}

/// The model and effort a planning or build run of this project is started with: the project's
/// choice (`run_options`, a Claude Code model and effort), with the app's default (Settings) wherever
/// nothing was chosen. MADO's runs are Claude Code's (`assistant::for_run`), except under full access,
/// where they go to the program chosen in Settings: the project's Claude Code choice is never handed
/// to another program, which gets that program's own defaults from Settings instead.
pub(crate) async fn options_for_run(state: &AppState, project_id: &str) -> (Option<String>, Option<String>) {
    let other = {
        let s = state.settings.read().await;
        let p = crate::assistant::primary(&s);
        (s.full_access() && p != crate::assistant::Program::Claude).then(|| crate::assistant::chat_defaults(&s, p))
    };
    if let Some(d) = other { return d; }
    let (model, effort) = run_options(state, project_id).await;
    let (dm, de) = crate::assistant::chat_defaults(&*state.settings.read().await, crate::assistant::Program::Claude);
    (model.or(dm), effort.or(de))
}

async fn options_get(State(st): S, UrlPath(pid): UrlPath<String>) -> R {
    project_dir(&st, &pid).await?;
    Ok(Json(serde_json::to_value(options_in(&pid))?))
}

/// A field of `PUT /options`: left out = keep what is on record, `null` (or "") = the app's default.
fn given<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> { Option::<String>::deserialize(d).map(Some) }

#[derive(Deserialize, Default)]
#[serde(default)]
struct OptionsBody {
    #[serde(deserialize_with = "given")] model: Option<Option<String>>,
    #[serde(deserialize_with = "given")] effort: Option<Option<String>>,
}

/// `PUT /options {model?, effort?}` → the options as kept. Checked like a chat's (plain tokens), and
/// the effort against the levels the installed assistant program lists, when it lists any: no list
/// of models or levels is written into the app.
async fn options_put(State(st): S, UrlPath(pid): UrlPath<String>, Json(b): Json<OptionsBody>) -> R {
    project_dir(&st, &pid).await?;
    let lock = lock_of(&pid);
    let _held = lock.lock().await;
    let mut o = options_in(&pid);
    if let Some(m) = b.model { o.model = option_value("model", m)?; }
    if let Some(e) = b.effort {
        o.effort = option_value("effort", e)?;
        if let Some(e) = &o.effort {
            let known = crate::claude::options(&st).await;
            let levels: Vec<&str> = known["efforts"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            if !effort_known(e, &levels) { return Err(crate::tr::err("mado.options.effort", &[("levels", &levels.join(", "))]).into()); }
        }
    }
    write_record(&pid, OPTIONS_FILE, &serde_json::to_vec_pretty(&o)?)?;
    Ok(Json(serde_json::to_value(o)?))
}

/// Is `effort` one of the `levels` the assistant program lists? With no list to hold it against
/// (the program is not installed, or its help names none) a plain token is all that can be asked.
pub fn effort_known(effort: &str, levels: &[&str]) -> bool { levels.is_empty() || levels.contains(&effort) }

// ---------- the plans there are ----------

/// `GET /api/mado` → `[{projectId, name, step, stepStatus, build?, updatedAt}]`: every linked project
/// whose plan was started, newest first. A project that is gone, broken or not this app's to believe
/// is left out; the list itself never fails. Only files are read: no git, no other program.
async fn plans(State(st): S) -> R {
    let s = st.settings.read().await.clone();
    let full = s.full_access();
    let mut out: Vec<Value> = s.projects.iter().filter_map(|p| plan_entry(&s, p, full)).collect();
    out.sort_by(|a, b| b["updatedAt"].as_str().cmp(&a["updatedAt"].as_str()));
    Ok(Json(Value::Array(out)))
}

/// One project's line of the list, or `None` when it has no plan this app started. "Trusted" is
/// told without looking through the folder (that would run git): the plan's `state.json` is the
/// very one this daemon last wrote (its fingerprint, kept outside the project), and a plan is only
/// ever started in a folder the user trusted.
fn plan_entry(s: &crate::settings::Settings, p: &crate::settings::Project, full: bool) -> Option<Value> {
    let dir = project_dir_in(s, &p.id).ok()?;
    let mado = mado_dir_of(&dir, full).ok()?;
    let bytes = read_in(&mado, "state.json", full, DOC_MAX).ok()??;
    if state_seen(&p.id).as_deref() != Some(plan_hash(&bytes).as_str()) { return None; }
    let state = load_state(&p.id, &mado, full).ok()??;
    let step = STEPS.iter().copied().find(|k| !is_approved(&state, k)).unwrap_or("build");
    let status = state.steps.get(step).map(|e| e.status.clone()).unwrap_or_default();
    // The build's status as the engine recorded it: one plain word, or nothing.
    let build = read_in(&mado, "build.json", full, DOC_MAX).ok().flatten()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["status"].as_str().map(String::from))
        .filter(|w| !w.is_empty() && w.len() <= 20 && w.bytes().all(|c| c.is_ascii_lowercase()) && w != "idle");
    let changed = ["state.json", "build.json"].iter()
        .filter_map(|f| std::fs::metadata(mado.join(f)).ok()?.modified().ok()).max()?;
    let mut v = json!({"projectId": p.id, "name": p.name.chars().take(200).collect::<String>(), "step": step, "stepStatus": status,
        "updatedAt": chrono::DateTime::<chrono::Utc>::from(changed).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)});
    if let Some(b) = build { v["build"] = json!(b); }
    Some(v)
}

/// What the Final check says about the project's git repository (`mado_build::repo_findings`), as
/// problems of the review: the build needs a repository, and says what it will commit first.
async fn repo_problems(st: &AppState, project: &Path) -> Vec<Problem> {
    crate::mado_build::repo_findings(st, project).await.into_iter()
        .map(|(kind, key, files)| crate::mado_review::problem(crate::tr::arabic(), kind, key, &[("files", &files)], vec![])).collect()
}

async fn review_get(State(st): S, UrlPath(pid): UrlPath<String>) -> R {
    let (project, mado, full) = ctx(&st, &pid).await?;
    // The hash is of the very bytes the review is computed from.
    let bytes = read_in(&mado, PLAN, full, DOC_MAX)?.ok_or_else(|| crate::tr::err("mado.missing", &[("path", PLAN)]))?;
    let plan = crate::mado_validate::plan(&bytes).map_err(|l| crate::tr::err_detail("mado.invalid", &[("path", PLAN)], l.join("\n")))?;
    let spec = read_text(&mado, "spec.md", full)?.unwrap_or_default();
    let flow = read_graph(&mado, "flow.graph.json", "flow", full).ok().flatten();
    let mut r = review(&spec, flow.as_ref(), &plan);
    r.problems.extend(repo_problems(&st, &project).await);
    r.plan_hash = plan_hash(&bytes);
    Ok(Json(serde_json::to_value(r)?))
}

#[cfg(test)]
#[path = "mado_tests.rs"]
mod tests;
