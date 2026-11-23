//! MADO for a project that already has code (round 0.5.0, `docs/mado-spec.md` "Round 0.5.0").
//!
//! - `detect`: what a linked folder holds, from a cheap, bounded walk of names (no file is opened,
//!   no program is run, no link is followed; `mado/`, `.git`, hidden, dependency and build folders
//!   are not entered). Shown as `detected` by `GET /api/mado/:project` and given to the runs of
//!   existing mode as a data block (`found_text`).
//! - `secret_read_deny`: the Read rules that keep the files holding a project's secrets (`.env`,
//!   keys, `.npmrc`…) out of an existing-mode run, which is told to read the whole project.
//! - `code_map`: the code-review-graph map for planning runs (a stub until `v050-graph` lands).
//!
//! The mode itself (`MadoState.mode`) and its checks are the core's (`mado.rs`).

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

/// The most entries the walk looks at, how deep it goes, and how long it may take.
pub const WALK_ENTRIES: usize = 20_000;
pub const WALK_DEPTH: usize = 8;
pub const WALK_TIME: Duration = Duration::from_millis(1500);
/// Source files that make a folder count as code without any package or build file.
pub const CODE_FILES: usize = 5;
/// The most package and build files named, languages listed, and top-level names given to a run.
const MANIFESTS_MAX: usize = 20;
const LANGS_MAX: usize = 12;
const TOP_MAX: usize = 60;
/// How much of the README a run is handed as data (it may read the file itself as well).
const README_CHARS: usize = 6000;

/// Folders the walk never enters (besides every hidden one and `mado/`): installed dependencies,
/// build output, caches.
const SKIP_DIRS: &[&str] = &["node_modules", "bower_components", "jspm_packages", "target", "dist", "build", "out", "vendor", "venv", "env",
    "__pycache__", "site-packages", "coverage", "Pods", "DerivedData", "obj", "tmp", "temp", "logs"];

/// Package and build files, by exact name (a `*.csproj`/`*.sln`/`*.fsproj` by extension).
const MANIFESTS: &[&str] = &["package.json", "deno.json", "Cargo.toml", "pyproject.toml", "requirements.txt", "setup.py", "Pipfile", "go.mod",
    "composer.json", "Gemfile", "pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle", "Package.swift", "pubspec.yaml", "mix.exs",
    "CMakeLists.txt", "meson.build", "build.zig", "stack.yaml", "dune-project"];

/// Source file extensions and the language they are counted under.
fn language(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "Rust",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "svelte" => "Svelte",
        "vue" => "Vue",
        "py" => "Python",
        "go" => "Go",
        "rb" => "Ruby",
        "php" => "PHP",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "m" | "mm" => "Objective-C",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "C++",
        "cs" => "C#",
        "fs" => "F#",
        "dart" => "Dart",
        "ex" | "exs" => "Elixir",
        "erl" => "Erlang",
        "scala" => "Scala",
        "clj" | "cljs" => "Clojure",
        "hs" => "Haskell",
        "ml" => "OCaml",
        "lua" => "Lua",
        "zig" => "Zig",
        "sql" => "SQL",
        "sh" | "bash" | "zsh" => "Shell",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" | "less" => "CSS",
        _ => return None,
    })
}

/// One language and how many of the files found are in it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Lang { pub lang: String, pub files: usize }

/// What `GET /api/mado/:project` says about the linked folder (`detected`).
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    /// The folder holds a project's code: a package or build file, or at least `CODE_FILES` source files.
    pub has_code: bool,
    /// The languages of the source files, most files first (at most `LANGS_MAX`).
    pub languages: Vec<Lang>,
    /// How many source files were found (counted up to the walk's limits).
    pub files: usize,
    /// There is a README at the top of the folder (a plain file).
    pub has_readme: bool,
    /// The package and build files found, project-relative (at most `MANIFESTS_MAX`).
    pub manifests: Vec<String>,
    /// The walk stopped at one of its limits: the numbers are a lower bound.
    pub capped: bool,
}

/// The walk's result with what a run is also told: the names at the top of the folder.
#[derive(Clone, Debug, Default)]
pub struct Found { pub detected: Detected, pub top: Vec<String> }

/// A name a prompt or the window may carry: short, printable, no control or direction characters.
fn plain_name(n: &str) -> bool {
    !n.is_empty() && n.chars().count() <= 120 && n.chars().all(|c| !c.is_control() && !format_char(c))
}

/// Bidi controls, zero-width and other format characters that change how a line reads.
pub fn format_char(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{feff}' | '\u{061c}' | '\u{00ad}')
}

fn is_manifest(name: &str) -> bool {
    MANIFESTS.contains(&name) || [".csproj", ".fsproj", ".sln"].iter().any(|x| name.ends_with(x) && name.len() > x.len())
}

/// Is there a README at the top of `project` (a plain file, not a link)?
fn has_readme(project: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(project) else { return false };
    rd.flatten().any(|e| {
        let n = e.file_name().to_string_lossy().to_ascii_lowercase();
        (n == "readme" || n.starts_with("readme.")) && e.file_type().map_or(false, |t| t.is_file())
    })
}

/// Walk `project` and tell what it holds. Bounded (`WALK_ENTRIES`, `WALK_DEPTH`, `WALK_TIME`); follows
/// no link (a link is neither entered nor counted); never opens a file. `mado/` (in any letter case),
/// hidden folders and `SKIP_DIRS` are not entered.
pub fn detect(project: &Path) -> Found {
    let started = Instant::now();
    let mut langs: BTreeMap<&'static str, usize> = BTreeMap::new();
    let (mut files, mut seen, mut capped) = (0usize, 0usize, false);
    let mut manifests: Vec<String> = vec![];
    let skip = |e: &walkdir::DirEntry| {
        if e.depth() == 0 || !e.file_type().is_dir() { return false; }
        let n = e.file_name().to_string_lossy();
        n.starts_with('.') || SKIP_DIRS.contains(&n.as_ref()) || (e.depth() == 1 && n.eq_ignore_ascii_case(crate::mado::DIR))
    };
    let mut walk = walkdir::WalkDir::new(project).follow_links(false).max_depth(WALK_DEPTH).sort_by_file_name().into_iter();
    while let Some(entry) = walk.next() {
        let Ok(e) = entry else { continue };
        if e.depth() == 0 { continue; }
        if skip(&e) { walk.skip_current_dir(); continue; }
        seen += 1;
        if seen > WALK_ENTRIES || started.elapsed() > WALK_TIME { capped = true; break; }
        // A link is not followed, and not taken for what it points at.
        if !e.file_type().is_file() { continue; }
        let name = e.file_name().to_string_lossy();
        if e.depth() <= 3 && is_manifest(&name) && manifests.len() < MANIFESTS_MAX {
            let rel = e.path().strip_prefix(project).unwrap_or(e.path()).to_string_lossy().replace('\\', "/");
            if rel.split('/').all(plain_name) && rel.len() <= 200 { manifests.push(rel); }
        }
        let ext = name.rsplit_once('.').map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_default();
        if let Some(l) = language(&ext) { files += 1; *langs.entry(l).or_default() += 1; }
    }
    let mut languages: Vec<Lang> = langs.into_iter().map(|(l, n)| Lang { lang: l.to_string(), files: n }).collect();
    languages.sort_by(|a, b| b.files.cmp(&a.files).then(a.lang.cmp(&b.lang)));
    languages.truncate(LANGS_MAX);
    // Markup and styles alone are not a program (a folder of web pages is still documented, though).
    let program_files: usize = languages.iter().filter(|l| l.lang != "HTML" && l.lang != "CSS").map(|l| l.files).sum();
    let has_code = !manifests.is_empty() || program_files >= CODE_FILES;
    let mut top: Vec<String> = std::fs::read_dir(project).map(|rd| rd.flatten().filter_map(|e| {
        let n = e.file_name().to_string_lossy().to_string();
        let dir = e.file_type().map_or(false, |t| t.is_dir());
        (plain_name(&n) && !n.eq_ignore_ascii_case(crate::mado::DIR)).then(|| if dir { format!("{n}/") } else { n })
    }).collect()).unwrap_or_default();
    top.sort();
    top.truncate(TOP_MAX);
    Found { detected: Detected { has_code, languages, files, has_readme: has_readme(project), manifests, capped }, top }
}

/// `detect` for the window and the runs: only for a folder the user trusted (`trust::require`,
/// which yields to full access, like every other guard), off the threads that answer requests.
/// `None`: the folder is not trusted (yet), or could not be looked at.
pub async fn detect_trusted(project: &Path, full: bool) -> Option<Found> {
    let p = project.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if !full && crate::trust::require(&p).is_err() { return None; }
        Some(detect(&p))
    }).await.ok().flatten()
}

/// What a run of existing mode is told the app found, as one text (it goes into a data block:
/// every name in it came from the project). The README's start is added when it is a plain file.
pub fn found_text(found: &Found, readme: Option<&str>) -> String {
    let d = &found.detected;
    let mut t = String::from("What the app found in the project folder (it read names only, and the README):\n");
    t.push_str(&format!("- Source files: {}{}\n", d.files, if d.capped { " (the count stopped at the app's limit; there are more)" } else { "" }));
    if !d.languages.is_empty() {
        t.push_str(&format!("- Languages: {}\n", d.languages.iter().map(|l| format!("{} ({} files)", l.lang, l.files)).collect::<Vec<_>>().join(", ")));
    }
    if !d.manifests.is_empty() { t.push_str(&format!("- Package and build files: {}\n", d.manifests.join(", "))); }
    if !found.top.is_empty() { t.push_str(&format!("- At the top of the folder: {}\n", found.top.join(", "))); }
    match readme.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => {
            let cut: String = r.chars().take(README_CHARS).collect();
            let more = if r.chars().count() > README_CHARS { "\n(… the README goes on; read the file for the rest)" } else { "" };
            t.push_str(&format!("\nThe README, as written:\n{cut}{more}"));
        }
        None => t.push_str("- No README at the top of the folder.\n"),
    }
    t
}

/// Names of files that hold secrets, closed to Read in every existing-mode run: `.env` files (the
/// examples too — the code that reads a setting names it), keys and certificates, package-registry
/// and network passwords, git's own config (a remote address can carry a token).
pub const SECRET_NAMES: &[&str] = &[".env", ".env.*", "*.pem", "*.key", "*.p12", "*.pfx", "*.jks", "*.keystore", "id_rsa*", "id_dsa*", "id_ecdsa*",
    "id_ed25519*", ".npmrc", ".yarnrc.yml", ".pypirc", ".netrc", ".git-credentials", ".htpasswd", ".pgpass", ".dockercfg", "*.tfvars"];

/// The Read rules for `SECRET_NAMES` at any depth of `project` (absolute, and relative to it, which
/// Claude Code reads like a gitignore line: a name without a folder matches at any depth), plus
/// `.git/config`, each in every letter case too.
pub fn secret_read_deny(project: &Path) -> Vec<String> {
    let p = crate::claude::rule_path(project);
    let mut deny = vec![];
    for n in SECRET_NAMES {
        for at in [format!("./{n}"), format!("./**/{n}"), format!("{p}/{n}"), format!("{p}/**/{n}")] { deny.push(format!("Read({at})")); }
    }
    for at in ["./.git/config".to_string(), format!("{p}/.git/config")] { deny.push(format!("Read({at})")); }
    crate::claude::every_case(&mut deny);
    let mut seen = std::collections::HashSet::new();
    deny.retain(|r| seen.insert(r.clone()));
    deny
}

/// The code-review-graph map of the project for a planning run (files, main symbols, how they
/// connect), at most `budget` characters, or `None` (no tool, no graph, anything failed).
// v050-graph: swap for crate::code_graph::code_map
async fn graph_code_map(_state: &crate::state::AppState, _project: &crate::settings::Project, _budget: usize) -> Option<String> { None }

/// The most characters of the code map a planning run is handed (about six thousand tokens).
pub const CODE_MAP_CHARS: usize = 24_000;

/// The code map for the project `project_id` (a linked project of the open library), bounded.
pub async fn code_map(state: &crate::state::AppState, project_id: &str) -> Option<String> {
    let project = state.settings.read().await.projects.iter().find(|p| p.id == project_id).cloned()?;
    let map = graph_code_map(state, &project, CODE_MAP_CHARS).await?;
    let map: String = map.chars().take(CODE_MAP_CHARS).collect();
    (!map.trim().is_empty()).then_some(map)
}
