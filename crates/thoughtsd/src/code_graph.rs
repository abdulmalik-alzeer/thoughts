//! code-review-graph (https://code-review-graph.com/) for MADO on existing projects (round 0.5.0).
//!
//! The app never ships, installs or downloads the tool, never runs its `install` command (it writes
//! some 44 files of hooks and settings) and never starts its MCP server. When the user has installed
//! it, the daemon (never a run) builds a graph of the linked project and reads it through the tool's
//! read-only queries. MADO works fully without it: every reader returns `None` when the tool is
//! missing, old, hangs, prints too much or lies, and never waits long.
//!
//! - Where the graph lives: `<thoughts home>/mado/<projectId>/graph/` (`CRG_DATA_DIR`), never in the
//!   project. The tool's own home and registry (`HOME`, `CRG_HOME`) point inside that folder too.
//!   Only a git repository of its own gets a graph: for a plain folder the tool insists on writing
//!   `<project>/.code-review-graph/`, which the app does not allow.
//! - How it runs: after the link, trust and git-settings guards (`trust::require_git`,
//!   `git::program_keys`), from the project, with a cleared environment (git told to start no
//!   program), a time limit and an output limit, and on macOS inside `sandbox-exec`
//!   (`mado_build::graph_profile`: reads the project and the tool, writes only the graph's folder, no
//!   network) whenever a build's commands are confined (`spending.confineCommands`). The guards and
//!   the confinement yield to full access; bounding and cleaning what it prints never do.
//! - What it prints is untrusted: every path is made relative to the project and must name a real
//!   file there, every name is cut to plain characters, every text is bounded. The readers return
//!   plain text that goes into a prompt only through `mado_prompts::block`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path as UrlPath, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;

use crate::state::AppState;

/// The program's name on the PATH.
pub const PROGRAM: &str = "code-review-graph";
/// The oldest version whose commands and output the app was checked against.
pub const MIN_VERSION: [u32; 3] = [2, 4, 0];
/// What the user is shown to copy (the tool's own README at v2.4.0: "pip install code-review-graph
/// # or: pipx install code-review-graph"; pipx keeps it in a place of its own and puts it on the
/// PATH, where a plain pip install is refused by Homebrew's Python). Never run by the app.
pub const INSTALL_COMMAND: &str = "pipx install code-review-graph";
pub const HOME_PAGE: &str = "https://code-review-graph.com/";

#[cfg(not(test))] const BUILD_LIMIT: Duration = Duration::from_secs(10 * 60);
#[cfg(test)] const BUILD_LIMIT: Duration = Duration::from_secs(4);
#[cfg(not(test))] const QUERY_LIMIT: Duration = Duration::from_secs(20);
#[cfg(test)] const QUERY_LIMIT: Duration = Duration::from_secs(3);
/// The most a reader (`code_map`, `around`, `impact`) may take, all its queries together.
#[cfg(not(test))] const CALL_LIMIT: Duration = Duration::from_secs(40);
#[cfg(test)] const CALL_LIMIT: Duration = Duration::from_secs(6);
const VERSION_LIMIT: Duration = Duration::from_secs(10);
/// The most of a query's answer that is read; more and the answer is not used.
const QUERY_MAX: usize = 8 << 20;
/// What is kept of what a build printed (its end: the error, the progress).
const LOG_TAIL: usize = 16 << 10;
/// The most files `around` and `impact` look at.
const FILES_MAX: usize = 12;
const CHANGED_MAX: usize = 60;
/// The longest path and name taken from the tool.
const PATH_MAX: usize = 300;
const NAME_MAX: usize = 100;
const RECORD: &str = "built.json";

// ---------- the project ----------

/// The project a graph is about: its id (which names its record folder) and its folder.
#[derive(Clone, Debug)]
pub struct Project { pub id: String, pub dir: PathBuf }

impl Project {
    pub fn new(id: &str, dir: &Path) -> Project { Project { id: id.to_string(), dir: dir.to_path_buf() } }
    /// A project of the open library the user linked (the routes' check).
    pub async fn linked(state: &AppState, id: &str) -> anyhow::Result<Project> {
        Ok(Project::new(id, &crate::mado::project_dir(state, id).await?))
    }
}

/// `<thoughts home>/mado/<projectId>/graph/`.
pub fn graph_dir(project_id: &str) -> Option<PathBuf> { crate::mado::record_dir(project_id).map(|d| d.join("graph")) }

// ---------- what the window is told ----------

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct ToolInfo {
    pub found: bool,
    pub version: Option<String>,
    /// Found, and at least `MIN_VERSION`.
    pub usable: bool,
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Recommend { pub text: String, pub command: String, pub url: String }

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Reason { pub code: String, pub message: String }

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Progress { pub done: u64, pub total: u64 }

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GraphStatus {
    pub tool: ToolInfo,
    pub built: bool,
    pub built_at: Option<String>,
    pub files: Option<u64>,
    pub stale: bool,
    pub building: bool,
    pub progress: Option<Progress>,
    /// Why a graph cannot be built here now (`not-linked`, `untrusted`, `no-repo`, `git-programs`,
    /// `no-sandbox`, `unsafe-place`, `missing`, `old-tool`); `None` when it can.
    pub reason: Option<Reason>,
    /// How the last build failed, when it did: `{message, detail}` (the tool's own words, cleaned and cut).
    pub error: Option<Value>,
    pub recommend: Recommend,
}

/// What the daemon wrote after a build (never anything the tool wrote).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Built { built_at: String, commit: Option<String>, tool: String, files: Option<u64>, nodes: Option<u64>, edges: Option<u64>, incomplete: bool }

// ---------- builds under way, and how the last one ended ----------

#[derive(Default)]
struct Runs { building: HashMap<String, Option<Progress>>, failed: HashMap<String, Value> }

fn with_runs<T>(f: impl FnOnce(&mut Runs) -> T) -> T {
    static R: std::sync::OnceLock<Mutex<Runs>> = std::sync::OnceLock::new();
    f(&mut R.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()))
}
pub fn is_building(project_id: &str) -> bool { with_runs(|r| r.building.contains_key(project_id)) }

// ---------- finding the tool ----------

/// Tests name the PATH a project's tool is looked for on; nothing else ever sees the real program.
#[cfg(test)]
pub(crate) fn test_paths() -> &'static Mutex<HashMap<String, String>> {
    static P: std::sync::OnceLock<Mutex<HashMap<String, String>>> = std::sync::OnceLock::new();
    P.get_or_init(Default::default)
}

fn search_path(_project_id: &str) -> String {
    #[cfg(test)]
    return test_paths().lock().unwrap_or_else(|e| e.into_inner()).get(_project_id).cloned().unwrap_or_default();
    #[cfg(not(test))]
    crate::claude::shell_path().to_string()
}

/// The first `code-review-graph` on `path` (the PATH a terminal has, as for `claude`, `codex`, `node`).
pub fn find_on(path: &str) -> Option<PathBuf> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let dirs: Vec<&Path> = path.split(sep).filter(|d| !d.is_empty()).map(Path::new).filter(|d| d.is_absolute()).collect();
    crate::claude::search(&dirs, &[PROGRAM.to_string()], false, |p| p.is_file())
}

/// `code-review-graph 2.4.0` → "2.4.0" (digits and dots only, short).
pub fn parse_version(printed: &str) -> Option<String> {
    let v = printed.split_whitespace().last()?.trim_start_matches('v');
    let ok = !v.is_empty() && v.len() <= 20 && v.bytes().all(|b| b.is_ascii_digit() || b == b'.') && v.split('.').all(|p| !p.is_empty());
    ok.then(|| v.to_string())
}

pub fn version_ok(v: &str) -> bool {
    let parts: Vec<u32> = v.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let at = |i: usize| parts.get(i).copied().unwrap_or(0);
    [at(0), at(1), at(2)] >= MIN_VERSION
}

type Seen = HashMap<PathBuf, (u64, Option<std::time::SystemTime>, Option<String>)>;

/// The tool on the user's PATH and its version (asked once per program file, within 10 seconds).
pub async fn detect(project_id: &str) -> ToolInfo {
    static SEEN: std::sync::OnceLock<Mutex<Seen>> = std::sync::OnceLock::new();
    let Some(bin) = find_on(&search_path(project_id)) else { return ToolInfo::default() };
    let seen = SEEN.get_or_init(Default::default);
    let key = std::fs::metadata(&bin).ok().map(|m| (m.len(), m.modified().ok()));
    if let Some((len, at)) = &key {
        if let Some((l, a, v)) = seen.lock().unwrap_or_else(|e| e.into_inner()).get(&bin) {
            if l == len && a == at { return info(bin, v.clone()); }
        }
    }
    let version = ask_version(&bin).await;
    if let Some((len, at)) = key { seen.lock().unwrap_or_else(|e| e.into_inner()).insert(bin.clone(), (len, at, version.clone())); }
    info(bin, version)
}

fn info(bin: PathBuf, version: Option<String>) -> ToolInfo {
    let usable = version.as_deref().map_or(false, version_ok);
    ToolInfo { found: true, version, usable, path: Some(bin) }
}

/// `code-review-graph --version`: the user's own program, outside any project, with a bare
/// environment and a home of its own (so it reads nothing of the user's).
async fn ask_version(bin: &Path) -> Option<String> {
    let home = crate::paths::home().join("code-graph-tool");
    crate::paths::create_private_dir(&home).ok()?;
    let mut c = crate::proc::command(bin);
    c.arg("--version").env_clear().env("PATH", crate::claude::shell_path()).env("HOME", &home).env("CRG_HOME", home.join("crg"))
        .env("NO_COLOR", "1").env("PYTHONDONTWRITEBYTECODE", "1").current_dir(&home).stdin(std::process::Stdio::null()).kill_on_drop(true);
    crate::proc::own_group(&mut c);
    let out = crate::proc::output_timeout(&mut c, VERSION_LIMIT).await.ok()?;
    if !out.status.success() { return None; }
    parse_version(&String::from_utf8_lossy(&out.stdout[..out.stdout.len().min(4096)]))
}

// ---------- cleaning what the tool prints ----------

fn invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}' | '\u{061C}' | '\u{00AD}' | '\u{180E}')
}

/// One line of text from the tool: no control, direction or invisible characters, no block
/// markers, spaces collapsed, at most `max` characters.
pub fn clean_line(s: &str, max: usize) -> String {
    let t: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).filter(|c| !invisible(*c)).collect();
    let mut t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    loop { let n = t.replace("<<<", "<<").replace(">>>", ">>"); if n == t { break; } t = n; }
    t.chars().take(max).collect()
}

/// A symbol's name from the tool (`Name@L41` → `Name`): letters, digits and `_ $ . :` only, short.
pub fn clean_symbol(raw: &str) -> Option<String> {
    let name = raw.rsplit_once("@L").filter(|(_, l)| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit())).map_or(raw, |(n, _)| n).trim();
    let ok = !name.is_empty() && name.chars().count() <= NAME_MAX && name.chars().all(|c| c.is_alphanumeric() || "_$.:".contains(c)) && !name.starts_with(':');
    ok.then(|| name.to_string())
}

/// The project's folder as the tool may print it (macOS's `/private` in front of `/var` and `/tmp`, or not).
fn prefixes(dir: &Path) -> Vec<String> {
    let d = dir.to_string_lossy().trim_end_matches('/').to_string();
    let other = match d.strip_prefix("/private") { Some(rest) => rest.to_string(), None => format!("/private{d}") };
    vec![d, other]
}

/// Is `rel` a plain project-relative path (no `..`, no leading `-`, nothing invisible, short)?
fn plain_rel(rel: &str) -> bool {
    !rel.is_empty() && rel.chars().count() <= PATH_MAX && !rel.starts_with('-') && !rel.starts_with('/') && !rel.contains('\\')
        && !rel.contains("<<<") && !rel.contains(">>>")
        && rel.chars().all(|c| !c.is_control() && !invisible(c)) && rel.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

/// A path the tool printed, as a path relative to the project that names a real file in it,
/// reached without a link (the tool's word is never taken for it). An absolute path must be inside
/// the project; a relative one is taken only when `relative_ok` (some answers print module names
/// such as `react` where a path would be).
pub fn rel_path(dir: &Path, raw: &str, relative_ok: bool) -> Option<String> {
    let rel = if raw.starts_with('/') {
        prefixes(dir).iter().find_map(|p| raw.strip_prefix(p.as_str()).and_then(|r| r.strip_prefix('/')).map(str::to_string))?
    } else if relative_ok { raw.trim_start_matches("./").to_string() } else { return None };
    if !plain_rel(&rel) { return None; }
    let mut p = dir.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
        if std::fs::symlink_metadata(&p).map_or(true, |m| m.file_type().is_symlink()) { return None; }
    }
    p.is_file().then_some(rel)
}

/// `lines` within `budget` characters, whole lines only, with a note of what was left out.
fn fit(lines: &[String], budget: usize) -> Option<String> {
    let len = |l: &String| l.chars().count() + 1;
    let total: usize = lines.iter().map(len).sum();
    let mut keep = lines.len();
    if total > budget {
        // Room for the note too: lines are dropped from the end until both fit.
        let note = |left: usize| format!("… ({left} more lines left out)").chars().count() + 1;
        let mut used: usize = total;
        while keep > 0 && used + note(lines.len() - keep) > budget { keep -= 1; used -= len(&lines[keep]); }
    }
    if keep == 0 { return None; }
    let mut out: String = lines[..keep].iter().map(|l| format!("{l}\n")).collect();
    if keep < lines.len() { out.push_str(&format!("… ({} more lines left out)\n", lines.len() - keep)); }
    Some(out)
}

// ---------- the guards, and where it runs ----------

struct Ctx { bin: PathBuf, dir: PathBuf, work: PathBuf, cache: PathBuf, confined: bool, port: u16, version: String }

struct Refused { code: &'static str, err: anyhow::Error }
fn refused(code: &'static str, err: anyhow::Error) -> Refused { Refused { code, err } }

fn min_version() -> String { MIN_VERSION.iter().map(u32::to_string).collect::<Vec<_>>().join(".") }

/// The guards every run of the tool passes, in this order, and the places it works in. The link
/// check always holds; the trust, git-settings and confinement checks yield to full access. No git
/// command runs before the trust check.
async fn ready(state: &AppState, project: &Project) -> Result<Ctx, Refused> {
    let full = state.settings.read().await.full_access();
    let linked = crate::mado::any_project_dir_in(&*state.settings.read().await, &project.id).ok();
    let dir = match (linked, crate::paths::canonical(&project.dir).ok()) {
        (Some(l), Some(d)) if l == d => d,
        _ => return Err(refused("not-linked", crate::tr::err("not_found.project", &[]))),
    };
    if !full { crate::trust::require_git(&dir).map_err(|e| refused("untrusted", e))?; }
    // A repository of its own (a plain folder would get the tool's folder written into it).
    let top = crate::git::git(state, &dir, &["rev-parse", "--show-toplevel"]).await.ok().and_then(|t| crate::paths::canonical(Path::new(t.trim())).ok());
    if top.as_deref() != Some(dir.as_path()) { return Err(refused("no-repo", crate::tr::err("code_graph.no_repo", &[]))); }
    if !full {
        let keys = crate::git::program_keys(state, &dir).await.map_err(|e| refused("git-programs", e))?;
        if !keys.is_empty() {
            let shown: Vec<String> = keys.iter().take(6).map(|k| k.chars().take(80).collect()).collect();
            return Err(refused("git-programs", crate::tr::err("mado_build.git_programs", &[("keys", &shown.join(", "))])));
        }
    }
    let how = crate::mado_build::confinement(state).await;
    if how.confined {
        if !Path::new(crate::mado_build::SANDBOX_EXEC).is_file() { return Err(refused("no-sandbox", crate::tr::err("code_graph.no_sandbox", &[]))); }
        // Reading the project must not open the user's whole home, nor the app's own folder.
        let app = crate::paths::canonical(&crate::paths::home()).unwrap_or_else(|_| crate::paths::home());
        let user = crate::paths::user_home().ok().and_then(|h| crate::paths::canonical(&h).ok());
        if app.starts_with(&dir) || dir.starts_with(&app) || user.as_deref().map_or(false, |h| h.starts_with(&dir)) {
            return Err(refused("unsafe-place", crate::tr::err("code_graph.unsafe_place", &[])));
        }
    }
    let tool = detect(&project.id).await;
    let Some(bin) = tool.path.clone() else { return Err(refused("missing", crate::tr::err("code_graph.missing", &[]))) };
    if !tool.usable {
        let v = tool.version.clone().unwrap_or_else(|| "?".into());
        return Err(refused("old-tool", crate::tr::err("code_graph.old", &[("version", &v), ("min", &min_version())])));
    }
    let graph = graph_dir(&project.id).ok_or_else(|| refused("not-linked", crate::tr::err("not_found.project", &[])))?;
    let (work, cache) = (graph.join("work"), graph.join("cache"));
    for d in [graph.clone(), work.clone(), work.join("db"), work.join("home"), work.join("crg"), work.join("tmp"), cache.clone()] {
        // Plain folders of the app's own, never links (made private; anything else is refused).
        let plain = std::fs::symlink_metadata(&d).map_or(true, |m| m.file_type().is_dir()) && crate::paths::create_private_dir(&d).is_ok()
            && std::fs::symlink_metadata(&d).map_or(false, |m| m.file_type().is_dir());
        if !plain { return Err(refused("unsafe-place", crate::tr::err("code_graph.unsafe_place", &[]))); }
    }
    Ok(Ctx { bin, dir, work, cache, confined: how.confined, port: how.port, version: tool.version.unwrap_or_default() })
}

/// The folders of the tool's installation the sandbox must let it read: its own environment and the
/// Python it runs on (from the script's first line and the environment's `pyvenv.cfg`). Never the
/// home itself, a folder above it, or the root.
pub fn tool_roots(bin: &Path) -> Vec<PathBuf> {
    let root_of = |p: &Path| -> Option<PathBuf> {
        let real = crate::paths::canonical(p).ok()?;
        let parent = real.parent()?;
        Some(if parent.file_name().map_or(false, |n| n == "bin") { parent.parent()?.to_path_buf() } else { parent.to_path_buf() })
    };
    let mut cands: Vec<PathBuf> = vec![];
    if let Some(r) = root_of(bin) {
        let cfg = std::fs::read(r.join("pyvenv.cfg")).map(|b| String::from_utf8_lossy(&b[..b.len().min(8192)]).to_string()).unwrap_or_default();
        for l in cfg.lines().take(50) {
            if let Some(("home", v)) = l.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
                let home = Path::new(v);
                if home.is_absolute() {
                    if let Some(h) = crate::paths::canonical(home).ok().map(|h| if h.file_name().map_or(false, |n| n == "bin") { h.parent().map(Path::to_path_buf).unwrap_or(h) } else { h }) { cands.push(h); }
                }
            }
        }
        cands.push(r);
    }
    let mut head = vec![];
    if let Ok(f) = std::fs::File::open(bin) { use std::io::Read; let _ = f.take(512).read_to_end(&mut head); }
    if let Some(rest) = head.strip_prefix(b"#!") {
        let line = String::from_utf8_lossy(rest.split(|b| *b == b'\n').next().unwrap_or_default()).to_string();
        let mut parts = line.split_whitespace();
        let first = parts.next().unwrap_or_default();
        let interp = if first.ends_with("/env") { parts.next().and_then(|n| crate::claude::find_binary(&None, n)) } else { Some(PathBuf::from(first)) };
        if let Some(r) = interp.filter(|p| p.is_absolute()).and_then(|p| root_of(&p)) { cands.push(r); }
    }
    let user = crate::paths::user_home().ok().and_then(|h| crate::paths::canonical(&h).ok());
    let mut out: Vec<PathBuf> = vec![];
    for c in cands {
        let too_wide = c.parent().is_none() || user.as_deref().map_or(false, |h| h.starts_with(&c));
        if !too_wide && !out.contains(&c) { out.push(c); }
    }
    out.truncate(6);
    out
}

/// The environment of a run of the tool: the build commands' cleared one, its home, data and
/// temporary folders inside the graph's folder, no optional network features, and git told to
/// start no program of the repository's choosing and to take no lock.
fn tool_env(cx: &Ctx, read_only: bool) -> Vec<(String, String)> {
    let s = |p: PathBuf| p.to_string_lossy().to_string();
    let mut env: Vec<(String, String)> = crate::mado_build::command_env(&cx.cache).into_iter()
        .filter(|(k, _)| !matches!(k.as_str(), "HOME" | "TMPDIR" | "CI") && !k.starts_with("PYTHON") && !k.starts_with("CRG_") && !k.starts_with("GIT_") && !k.starts_with("XDG_"))
        .collect();
    let home = cx.work.join("home");
    let mut own: Vec<(&str, String)> = vec![
        ("HOME", s(home.clone())), ("CRG_HOME", s(cx.work.join("crg"))), ("CRG_DATA_DIR", s(cx.work.join("db"))), ("TMPDIR", s(cx.work.join("tmp"))),
        ("XDG_CONFIG_HOME", s(home.join(".config"))), ("XDG_DATA_HOME", s(home.join(".local/share"))), ("XDG_STATE_HOME", s(home.join(".local/state"))),
        ("XDG_CACHE_HOME", s(cx.cache.join("xdg"))),
        ("CRG_PARSE_EXECUTOR", "thread".into()), ("CRG_DISABLE_ENRICHMENT", "1".into()), ("CRG_FOLLOW_SYMLINKS", "0".into()),
        ("NO_COLOR", "1".into()), ("PYTHONDONTWRITEBYTECODE", "1".into()), ("PYTHONNOUSERSITE", "1".into()), ("PYTHONIOENCODING", "utf-8".into()),
        ("HF_HUB_OFFLINE", "1".into()), ("TRANSFORMERS_OFFLINE", "1".into()),
        ("GIT_CONFIG_GLOBAL", "/dev/null".into()), ("GIT_CONFIG_NOSYSTEM", "1".into()), ("GIT_ATTR_NOSYSTEM", "1".into()), ("GIT_TERMINAL_PROMPT", "0".into()),
        ("GIT_OPTIONAL_LOCKS", "0".into()),
    ];
    if read_only { own.push(("CRG_READ_ONLY", "1".into())); }
    if let Some(p) = cx.dir.parent() { own.push(("GIT_CEILING_DIRECTORIES", s(p.to_path_buf()))); }
    // (Not `diff.external`: blanking it makes every `git diff` fail; a repository that sets it is refused by `program_keys`.)
    let git = ["core.fsmonitor=false", "core.hooksPath=/dev/null", "core.pager=cat", "core.editor=true", "core.askPass=", "credential.helper=",
               "core.sshCommand=false", "protocol.allow=never", "commit.gpgSign=false", "gpg.program=false", "core.quotePath=false", "core.untrackedCache=false"];
    own.push(("GIT_CONFIG_COUNT", git.len().to_string()));
    env.extend(own.into_iter().map(|(k, v)| (k.to_string(), v)));
    for (i, pair) in git.iter().enumerate() {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        env.push((format!("GIT_CONFIG_KEY_{i}"), k.into()));
        env.push((format!("GIT_CONFIG_VALUE_{i}"), v.into()));
    }
    env
}

#[derive(Debug, PartialEq)]
enum Fail { NoSandbox, Timeout, TooLarge, Exit(String) }

/// `INFO: Progress: 65/120 files parsed` → 65 of 120.
pub fn parse_progress(line: &str) -> Option<Progress> {
    let rest = line.split("Progress:").nth(1)?.trim();
    let (a, b) = rest.split_whitespace().next()?.split_once('/')?;
    let (done, total) = (a.parse().ok()?, b.parse().ok()?);
    (done <= total && total < 10_000_000).then_some(Progress { done, total })
}

/// One run of the tool from the project: its answer (at most `max_out` bytes: more ends it) within
/// `limit`. `progress_of` names the project whose build progress its lines report.
async fn run(cx: &Ctx, args: &[String], limit: Duration, max_out: usize, read_only: bool, progress_of: Option<&str>) -> Result<Vec<u8>, Fail> {
    let mut c = if cx.confined {
        if !Path::new(crate::mado_build::SANDBOX_EXEC).is_file() { return Err(Fail::NoSandbox); }
        let mut read = vec![cx.dir.clone()];
        read.extend(tool_roots(&cx.bin));
        let profile = crate::mado_build::graph_profile(&cx.work, &cx.dir, &cx.cache, crate::paths::user_home().ok().as_deref(), cx.port, &read);
        let mut c = crate::proc::command(crate::mado_build::SANDBOX_EXEC);
        c.arg("-p").arg(profile).arg(&cx.bin);
        c
    } else { crate::proc::command(&cx.bin) };
    c.args(args).env_clear().envs(tool_env(cx, read_only)).current_dir(&cx.dir)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    crate::proc::own_group(&mut c);
    let mut child = c.spawn().map_err(|e| if cx.confined { Fail::NoSandbox } else { Fail::Exit(e.to_string()) })?;
    let id = child.id();
    let (out, err) = (child.stdout.take(), child.stderr.take());
    let read_out = async move {
        let mut buf = vec![];
        let Some(out) = out else { return Ok(buf) };
        let _ = out.take(max_out as u64 + 1).read_to_end(&mut buf).await;
        if buf.len() > max_out {
            if let Some(id) = id { crate::proc::end_tree(id).await; }
            return Err(Fail::TooLarge);
        }
        Ok(buf)
    };
    let read_err = async move {
        use tokio::io::AsyncBufReadExt;
        let mut tail = std::collections::VecDeque::<u8>::new();
        let Some(err) = err else { return String::new() };
        let mut lines = tokio::io::BufReader::new(err).split(b'\n');
        while let Ok(Some(line)) = lines.next_segment().await {
            let text = String::from_utf8_lossy(&line[..line.len().min(4096)]).to_string();
            if let (Some(pid), Some(p)) = (progress_of, parse_progress(&text)) {
                with_runs(|r| if let Some(slot) = r.building.get_mut(pid) { *slot = Some(p); });
            }
            tail.extend(text.bytes().chain(std::iter::once(b'\n')));
            while tail.len() > LOG_TAIL { tail.pop_front(); }
        }
        String::from_utf8_lossy(&tail.into_iter().collect::<Vec<u8>>()).to_string()
    };
    let all = async { let (o, e) = tokio::join!(read_out, read_err); (o, e, child.wait().await) };
    match tokio::time::timeout(limit, all).await {
        Ok((o, e, status)) => {
            let o = o?;
            if cx.confined && e.trim_start().starts_with("sandbox-exec:") {
                crate::log::warn(format!("code graph: the sandbox did not start: {}", clean_line(&e, 300)));
                return Err(Fail::NoSandbox);
            }
            if status.as_ref().map_or(false, |s| s.success()) { return Ok(o); }
            let code = status.ok().and_then(|s| s.code()).map(|c| c.to_string()).unwrap_or_else(|| "?".into());
            Err(Fail::Exit(format!("(exit {code}) {}", tidy(&e, cx))))
        }
        Err(_) => {
            if let Some(id) = id { crate::proc::end_tree(id).await; }
            let _ = child.kill().await;
            Err(Fail::Timeout)
        }
    }
}

/// The end of what the tool printed, as a detail to show: the app's and the project's folders
/// named in words, its own lines only (not its INFO lines), cleaned and short.
fn tidy(printed: &str, cx: &Ctx) -> String {
    let mut t = printed.to_string();
    for p in prefixes(&cx.work) { t = t.replace(&p, "<graph>"); }
    for p in prefixes(&cx.dir) { t = t.replace(&p, "<project>"); }
    let lines: Vec<String> = t.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("INFO:")).map(|l| clean_line(l, 300)).collect();
    let s = lines[lines.len().saturating_sub(8)..].join("\n");
    let n = s.chars().count();
    if n > 1500 { format!("…{}", s.chars().skip(n - 1500).collect::<String>()) } else { s }
}

/// A read-only query's JSON object, or `None` for anything else.
async fn query(cx: &Ctx, args: &[&str]) -> Option<Value> {
    let mut a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    a.push("--repo".into());
    a.push(cx.dir.to_string_lossy().to_string());
    let out = run(cx, &a, QUERY_LIMIT, QUERY_MAX, true, None).await.ok()?;
    let v: Value = serde_json::from_slice(&out).ok()?;
    v.is_object().then_some(v)
}

// ---------- the record of the last build ----------

fn read_built(project_id: &str) -> Option<Built> {
    let p = graph_dir(project_id)?.join(RECORD);
    let m = std::fs::symlink_metadata(&p).ok()?;
    if !m.is_file() || m.len() > 64 << 10 { return None; }
    let b: Built = serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?;
    let sane = b.built_at.len() <= 40 && b.tool.len() <= 20 && b.commit.as_deref().map_or(true, |c| c.len() <= 64 && c.bytes().all(|x| x.is_ascii_hexdigit()));
    sane.then_some(b)
}

/// A graph is there: the daemon's record, and the tool's database as a plain file.
fn built_graph(project_id: &str) -> Option<Built> {
    let b = read_built(project_id)?;
    let db = graph_dir(project_id)?.join("work/db/graph.db");
    std::fs::symlink_metadata(&db).map_or(false, |m| m.is_file()).then_some(b)
}

fn num(v: &Value, k: &str) -> Option<u64> { v.get(k).and_then(Value::as_u64).filter(|n| *n < 100_000_000) }

// ---------- status ----------

fn recommend(tool: &ToolInfo) -> Recommend {
    let text = if !tool.found { crate::tr::text("code_graph.recommend_missing", &[]) }
        else if !tool.usable { crate::tr::text("code_graph.recommend_old", &[("version", tool.version.as_deref().unwrap_or("?")), ("min", &min_version())]) }
        else { crate::tr::text("code_graph.recommend_found", &[]) };
    Recommend { text, command: INSTALL_COMMAND.into(), url: HOME_PAGE.into() }
}

/// What the Plan view's card shows. Cheap and never failing: it runs the guards' git checks but
/// never the tool on the project (only, once per program file, `--version`).
pub async fn status(state: &AppState, project: &Project) -> GraphStatus {
    let tool = detect(&project.id).await;
    let (building, progress, error) = with_runs(|r| (r.building.contains_key(&project.id), r.building.get(&project.id).copied().flatten(), r.failed.get(&project.id).cloned()));
    let reason = match ready(state, project).await { Ok(_) => None, Err(r) => Some(Reason { code: r.code.into(), message: crate::tr::classify(&r.err).message }) };
    let built = built_graph(&project.id);
    // HEAD only once the guards before the tool passed (the tool itself may be missing or old).
    let guarded = reason.as_ref().map_or(true, |r| r.code == "missing" || r.code == "old-tool");
    let head = if built.is_some() && guarded { crate::git::head(state, &project.dir).await } else { None };
    let stale = built.as_ref().map_or(false, |b| b.incomplete || (head.is_some() && b.commit != head) || tool.version.as_deref().map_or(false, |v| v != b.tool));
    GraphStatus {
        recommend: recommend(&tool), tool, built: built.is_some(), built_at: built.as_ref().map(|b| b.built_at.clone()), files: built.as_ref().and_then(|b| b.files),
        stale, building, progress, reason: if building { None } else { reason }, error,
    }
}

// ---------- building ----------

/// Start building (or updating) the project's graph in the background; the window hears
/// `{type:'mado', what:'graph'}` when it starts and ends. Refused with the guard's own message when
/// a guard does not let it run; a build already under way is simply reported.
pub async fn start_build(state: &Arc<AppState>, project: &Project) -> anyhow::Result<GraphStatus> {
    if is_building(&project.id) { return Ok(status(state, project).await); }
    let cx = ready(state, project).await.map_err(|r| r.err)?;
    let fresh = with_runs(|r| {
        if r.building.contains_key(&project.id) { return false; }
        r.building.insert(project.id.clone(), None);
        r.failed.remove(&project.id);
        true
    });
    if fresh {
        let (st, p) = (state.clone(), project.clone());
        tokio::spawn(async move {
            let res = build_now(&st, &p, &cx).await;
            with_runs(|r| { r.building.remove(&p.id); if let Err(e) = res { r.failed.insert(p.id.clone(), e); } });
            crate::mado::tell(&st, &p.id, "graph").await;
        });
        crate::mado::tell(state, &project.id, "graph").await;
    }
    Ok(status(state, project).await)
}

fn failure(f: Fail) -> Value {
    let (key, detail) = match f {
        Fail::NoSandbox => ("code_graph.no_sandbox", String::new()),
        Fail::Timeout => ("code_graph.timeout", String::new()),
        Fail::TooLarge => ("code_graph.too_large", String::new()),
        Fail::Exit(d) => ("code_graph.failed", d),
    };
    let minutes = (BUILD_LIMIT.as_secs() / 60).max(1).to_string();
    json!({"message": crate::tr::text(key, &[("minutes", &minutes)]), "detail": detail})
}

async fn build_now(state: &AppState, project: &Project, cx: &Ctx) -> Result<(), Value> {
    let repo = cx.dir.to_string_lossy().to_string();
    let had_local = std::fs::symlink_metadata(cx.dir.join(".code-review-graph")).is_ok();
    let update = read_built(&project.id).map_or(false, |b| b.tool == cx.version) && built_graph(&project.id).is_some();
    let args = |cmd: &str| vec![cmd.to_string(), "--repo".into(), repo.clone(), "--no-follow-symlinks".into()];
    let mut res = run(cx, &args(if update { "update" } else { "build" }), BUILD_LIMIT, LOG_TAIL, false, Some(&project.id)).await;
    if update && matches!(res, Err(Fail::Exit(_))) { res = run(cx, &args("build"), BUILD_LIMIT, LOG_TAIL, false, Some(&project.id)).await; }
    if !had_local && std::fs::symlink_metadata(cx.dir.join(".code-review-graph")).is_ok() {
        crate::log::warn("code graph: the tool made a .code-review-graph folder in the project");
    }
    res.map_err(failure)?;
    if !graph_dir(&project.id).map_or(false, |g| g.join("work/db/graph.db").is_file()) { return Err(failure(Fail::Exit(String::new()))); }
    let st = query(cx, &["status", "--json"]).await.unwrap_or(Value::Null);
    let b = Built {
        built_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true), commit: crate::git::head(state, &cx.dir).await, tool: cx.version.clone(),
        files: num(&st, "files"), nodes: num(&st, "nodes"), edges: num(&st, "edges"), incomplete: st.get("build_incomplete").and_then(Value::as_bool).unwrap_or(false),
    };
    let path = graph_dir(&project.id).ok_or(Value::Null)?.join(RECORD);
    crate::paths::write_atomic(&path, &serde_json::to_vec_pretty(&b).unwrap_or_default(), true).map_err(|e| failure(Fail::Exit(e.to_string())))
}

// ---------- reading ----------

/// The guards and a built graph, quickly: `None` while a build runs, when the tool is missing or
/// old, when a guard refuses, or when no graph is there.
async fn reader(state: &AppState, project: &Project, budget: usize) -> Option<Ctx> {
    if budget < 80 || is_building(&project.id) { return None; }
    built_graph(&project.id)?;
    ready(state, project).await.ok()
}

/// Files, their main symbols and how they connect, for planning runs: plain text (put it in a
/// prompt only through `mado_prompts::block`) of at most `budget_chars` characters.
pub async fn code_map(state: &AppState, project: &Project, budget_chars: usize) -> Option<String> {
    let cx = reader(state, project, budget_chars).await?;
    tokio::time::timeout(CALL_LIMIT, async {
        let arch = match query(&cx, &["architecture", "--detail-level", "standard"]).await {
            Some(v) => v,
            None => query(&cx, &["architecture", "--detail-level", "minimal"]).await?,
        };
        map_text(&cx.dir, &arch, budget_chars)
    }).await.ok().flatten()
}

/// `code_map` from the tool's `architecture` answer.
pub fn map_text(dir: &Path, arch: &Value, budget: usize) -> Option<String> {
    if arch.get("status").and_then(Value::as_str) != Some("ok") { return None; }
    let comms = arch.get("communities")?.as_array()?;
    let mut lines = vec!["Code map, from the code-review-graph tool (groups of code it found, their files and main symbols, then how files connect):".to_string()];
    for c in comms.iter().take(200) {
        let name = clean_line(c.get("name").and_then(Value::as_str).unwrap_or("?"), 60);
        let size = num(c, "size").unwrap_or(0);
        let lang = clean_line(c.get("dominant_language").and_then(Value::as_str).unwrap_or(""), 20);
        let (mut files, mut symbols): (Vec<String>, Vec<String>) = (vec![], vec![]);
        for m in c.get("members").and_then(Value::as_array).into_iter().flatten().take(5000).filter_map(Value::as_str) {
            let (file, sym) = m.split_once("::").map_or((m, None), |(f, s)| (f, Some(s)));
            if let Some(f) = rel_path(dir, file, false) { if !files.contains(&f) { files.push(f); } }
            if let Some(s) = sym.and_then(clean_symbol) { if symbols.len() < 10 && !symbols.contains(&s) { symbols.push(s); } }
        }
        let mut line = format!("- group {}: {size} symbols{}", quoted(&name), if lang.is_empty() { String::new() } else { format!(", {lang}") });
        if !files.is_empty() {
            let more = files.len().saturating_sub(8);
            line.push_str(&format!("; files: {}{}", files[..files.len().min(8)].join(", "), if more > 0 { format!(" (+{more} more)") } else { String::new() }));
        }
        if !symbols.is_empty() { line.push_str(&format!("; main symbols: {}", symbols.join(", "))); }
        lines.push(line);
    }
    let mut pairs: HashMap<(String, String), (u64, String)> = HashMap::new();
    for e in arch.get("cross_community_edges").and_then(Value::as_array).into_iter().flatten().take(20_000) {
        let file = |k: &str| e.get(k).and_then(Value::as_str).map(|s| s.split_once("::").map_or(s, |(f, _)| f)).and_then(|f| rel_path(dir, f, false));
        let (Some(a), Some(b)) = (file("source"), file("target")) else { continue };
        if a == b { continue; }
        let kind = clean_symbol(e.get("edge_kind").and_then(Value::as_str).unwrap_or("")).unwrap_or_else(|| "uses".into()).to_lowercase();
        pairs.entry((a, b)).or_insert((0, kind)).0 += 1;
    }
    let mut pairs: Vec<_> = pairs.into_iter().collect();
    pairs.sort_by(|x, y| y.1 .0.cmp(&x.1 .0).then(x.0.cmp(&y.0)));
    if !pairs.is_empty() {
        lines.push("How files connect (the most links first):".into());
        for ((a, b), (n, kind)) in pairs.into_iter().take(40) { lines.push(format!("- {a} → {b} ({kind} ×{n})")); }
    }
    if lines.len() == 1 { return None; }
    fit(&lines, budget)
}

/// A group's name as the tool made it (from folder and symbol names), quoted.
fn quoted(name: &str) -> String { format!("{name:?}") }

/// The entries of `files` that are plain project paths, at most `max`, each once.
fn asked(files: &[String], max: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for f in files {
        let f = f.trim().trim_start_matches("./");
        if plain_rel(f) && !out.iter().any(|o| o == f) { out.push(f.to_string()); }
        if out.len() >= max { break; }
    }
    out
}

/// Callers, callees and tests around `files` (project-relative), for task briefs and fix runs:
/// plain text for a data block, at most `budget_chars` characters.
pub async fn around(state: &AppState, project: &Project, files: &[String], budget_chars: usize) -> Option<String> {
    let files = asked(files, FILES_MAX);
    if files.is_empty() { return None; }
    let cx = reader(state, project, budget_chars).await?;
    let sem = tokio::sync::Semaphore::new(4);
    let (cx, sem) = (&cx, &sem);
    let work = files.iter().map(|f| async move {
        let mut got = vec![];
        for pattern in ["file_summary", "importers_of", "imports_of", "tests_for"] {
            let _slot = sem.acquire().await.ok();
            got.push(query(cx, &["query", pattern, f.as_str()]).await);
        }
        (f.clone(), got)
    });
    let answers = tokio::time::timeout(CALL_LIMIT, futures::future::join_all(work)).await.ok()?;
    around_text(&cx.dir, &answers, budget_chars)
}

/// `around` from the tool's answers, per file: `[file_summary, importers_of, imports_of, tests_for]`.
pub fn around_text(dir: &Path, answers: &[(String, Vec<Option<Value>>)], budget: usize) -> Option<String> {
    let mut lines = vec!["Code around these files, from the code-review-graph tool (what they define, which files use them, what they use, their tests; it can miss some):".to_string()];
    let mut any = false;
    for (file, got) in answers {
        let ok = |i: usize| got.get(i).and_then(|v| v.as_ref()).filter(|v| v.get("status").and_then(Value::as_str) == Some("ok"));
        if (0..4).all(|i| ok(i).is_none()) { continue; }
        any = true;
        lines.push(format!("## {}", clean_line(file, PATH_MAX)));
        let mut defs: Vec<String> = vec![];
        for r in ok(0).and_then(|v| v.get("results")).and_then(Value::as_array).into_iter().flatten().take(400) {
            let kind = r.get("kind").and_then(Value::as_str).unwrap_or("");
            if kind == "File" { continue; }
            let Some(name) = r.get("name").and_then(Value::as_str).and_then(clean_symbol) else { continue };
            let at = num(r, "line_start").map(|l| format!(" line {l}")).unwrap_or_default();
            let kind = clean_symbol(kind).unwrap_or_default().to_lowercase();
            if defs.len() < 25 { defs.push(format!("{name} ({kind}{at})")); }
        }
        if !defs.is_empty() { lines.push(format!("Defines: {}", defs.join(", "))); }
        let edge_files = |i: usize, end: &str| -> Vec<String> {
            let mut out: Vec<String> = vec![];
            for e in ok(i).and_then(|v| v.get("edges")).and_then(Value::as_array).into_iter().flatten().take(2000) {
                let Some(f) = e.get(end).and_then(Value::as_str).and_then(|s| rel_path(dir, s.split_once("::").map_or(s, |(f, _)| f), false)) else { continue };
                if &f != file && !out.contains(&f) && out.len() < 25 { out.push(f); }
            }
            out
        };
        let users = edge_files(1, "source");
        if !users.is_empty() { lines.push(format!("Used by (keep what they rely on working): {}", users.join(", "))); }
        let uses = edge_files(2, "target");
        if !uses.is_empty() { lines.push(format!("Uses: {}", uses.join(", "))); }
        let mut tests: Vec<String> = vec![];
        for r in ok(3).and_then(|v| v.get("results")).and_then(Value::as_array).into_iter().flatten().take(400) {
            let Some(f) = r.get("file_path").and_then(Value::as_str).and_then(|s| rel_path(dir, s, false)) else { continue };
            let t = format!("{f}{}", num(r, "line_start").map(|l| format!(" line {l}")).unwrap_or_default());
            if !tests.contains(&t) && tests.len() < 15 { tests.push(t); }
        }
        if !tests.is_empty() { lines.push(format!("Tests: {}", tests.join(", "))); }
        if lines.last().map_or(false, |l| l.starts_with("## ")) { lines.push("(the graph knows nothing more about this file)".into()); }
    }
    if !any { return None; }
    fit(&lines, budget)
}

/// Files that depend on `changed` (project-relative) and are not among them, for the review: plain
/// text for a data block, at most `budget_chars` characters. An empty answer still says how many
/// calls the tool could not follow, so it is never read as "nothing depends on it".
pub async fn impact(state: &AppState, project: &Project, changed: &[String], budget_chars: usize) -> Option<String> {
    let changed = asked(changed, CHANGED_MAX);
    if changed.is_empty() { return None; }
    let cx = reader(state, project, budget_chars).await?;
    let mut args: Vec<&str> = vec!["impact", "--depth", "2", "--detail-level", "compact", "--max-files", "60", "--files"];
    args.extend(changed.iter().map(String::as_str));
    let v = tokio::time::timeout(CALL_LIMIT, query(&cx, &args)).await.ok()??;
    impact_text(&cx.dir, &v, &changed, budget_chars)
}

/// `impact` from the tool's `impact --detail-level compact` answer.
pub fn impact_text(dir: &Path, v: &Value, changed: &[String], budget: usize) -> Option<String> {
    if v.get("status").and_then(Value::as_str) != Some("ok") { return None; }
    let list = v.get("impacted_files")?.as_array()?;
    let mut files: Vec<String> = vec![];
    for f in list.iter().take(500).filter_map(Value::as_str) {
        let Some(rel) = rel_path(dir, f, true) else { continue };
        if !changed.contains(&rel) && !files.contains(&rel) { files.push(rel); }
    }
    let unresolved = num(v, "unresolved_call_sites").unwrap_or(0);
    let mut lines = vec![if files.is_empty() {
        "The code-review-graph tool found no other file that depends on the changed files.".to_string()
    } else {
        "Files that depend on the changed files and were not changed, from the code-review-graph tool (most affected first):".to_string()
    }];
    lines.extend(files.iter().map(|f| format!("- {f}")));
    if unresolved > 0 { lines.push(format!("(The tool could not follow {unresolved} calls, so files may be missing from this list.)")); }
    fit(&lines, budget)
}

// ---------- routes ----------

type S = State<Arc<AppState>>;
type R = Result<Json<Value>, crate::api::ApiErr>;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/mado/:project/graph", get(graph_get))
        .route("/api/mado/:project/graph/build", post(graph_build))
}

/// `GET /api/mado/:project/graph`: the code graph card (`GraphStatus`).
async fn graph_get(State(st): S, UrlPath(pid): UrlPath<String>) -> R {
    let p = Project::linked(&st, &pid).await?;
    Ok(Json(serde_json::to_value(status(&st, &p).await)?))
}

/// `POST /api/mado/:project/graph/build`: build or update it (by the daemon, in the background).
async fn graph_build(State(st): S, UrlPath(pid): UrlPath<String>) -> R {
    let p = Project::linked(&st, &pid).await?;
    Ok(Json(serde_json::to_value(start_build(&st, &p).await?)?))
}

#[cfg(test)]
#[path = "code_graph_tests.rs"]
mod tests;
