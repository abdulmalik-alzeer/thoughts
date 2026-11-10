//! Library trust: before the app or the AI works inside a library it didn't create, list anything in
//! it that could run programs on this machine, and only proceed once the user has trusted that folder.
//! Trust decisions live on the daemon side (~/.thoughts/trusted.json), never in a client flag.

use std::path::{Path, PathBuf};

const STARTER_SETTINGS: &str = include_str!("../../../starter/.claude/settings.json");

fn store() -> PathBuf { crate::paths::home().join("trusted.json") }

fn canon(p: &Path) -> String { std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_string() }

/// Trust is recorded per folder *and* per exact set of findings, so anything new asks again.
fn load() -> std::collections::BTreeMap<String, String> {
    std::fs::read_to_string(store()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}
/// Hash of the findings *and the bytes of every file they could come from*, so editing a trusted
/// hook, config or MCP file asks again.
fn fingerprint(path: &Path, f: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(f.join("\n"));
    let mut files: Vec<PathBuf> = [".claude/settings.json", ".claude/settings.local.json", ".mcp.json"].iter().map(|x| path.join(x)).collect();
    let git = |arg: &str| std::process::Command::new(crate::claude::find_binary(&None, "git").unwrap_or_else(|| "git".into()))
        .current_dir(path).env("PATH", crate::claude::shell_path()).env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "core.fsmonitor=false", "rev-parse", arg]).output().ok().filter(|o| o.status.success())
        .map(|o| { let p = PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()); if p.is_absolute() { p } else { path.join(p) } });
    // Every non-markdown file under .claude/ (scripts a hook could call); skills' .md edits don't re-prompt.
    // Symlinks are never followed (their target string is hashed instead); depth and count are capped.
    fn walk(d: &Path, depth: usize, out: &mut Vec<PathBuf>, links: &mut Vec<String>) {
        // Over the cap: mix in a fresh value so this folder can never match a stored trust.
        if depth > 8 || out.len() > 2000 { links.push(format!("limit:{:?}", std::time::SystemTime::now())); return; }
        if let Ok(rd) = std::fs::read_dir(d) { for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                links.push(format!("{}->{}", p.display(), std::fs::read_link(&p).map(|t| t.display().to_string()).unwrap_or_default()));
                // Also hash what it resolves to, but only a regular file inside the library.
                out.push(p)
            }
            else if ft.is_dir() { walk(&p, depth + 1, out, links) }
            else if ft.is_file() && p.extension().map_or(true, |x| x != "md") { out.push(p) }
        } }
    }
    let (mut extra, mut links) = (vec![], vec![]);
    walk(&path.join(".claude"), 0, &mut extra, &mut links);
    extra.sort(); links.sort(); files.extend(extra);
    h.update(links.join("\n"));
    let root = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let git_dirs: Vec<PathBuf> = [git("--absolute-git-dir"), git("--git-common-dir")].into_iter().flatten()
        .filter_map(|d| std::fs::canonicalize(d).ok()).collect();
    for d in git_dirs.clone() {
        files.push(d.join("config")); files.push(d.join("config.worktree"));
        // Files pulled in through include.path / includeIf.
        for name in ["config", "config.worktree"] {
            if let Ok(o) = std::process::Command::new(crate::claude::find_binary(&None, "git").unwrap_or_else(|| "git".into()))
                .env("PATH", crate::claude::shell_path()).env("GIT_CONFIG_NOSYSTEM", "1")
                .args(["-c", "core.fsmonitor=false", "config", "--file"]).arg(d.join(name))
                .args(["--includes", "--list", "--show-origin", "--null"]).output()
            {
                for rec in String::from_utf8_lossy(&o.stdout).split('\0') {
                    if let Some(f) = rec.strip_prefix("file:") { files.push(PathBuf::from(f)); }
                }
            }
        }
        if let Ok(rd) = std::fs::read_dir(d.join("hooks")) {
            let mut hooks: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect(); hooks.sort(); files.extend(hooks);
        }
    }
    for p in files {
        h.update(p.to_string_lossy().as_bytes()); h.update([0]);
        // Whole file, streamed, up to 16 MB; bigger files can never match a stored trust. Symlinks are
        // read only when they resolve to a regular file inside the library (never /dev, /proc, …).
        let real = std::fs::canonicalize(&p).ok();
        let inside = real.as_ref().map_or(false, |r| r.starts_with(&root) || git_dirs.iter().any(|g| r.starts_with(g)));
        if inside && real.as_ref().and_then(|r| std::fs::metadata(r).ok()).map_or(false, |m| m.is_file()) {
            const LIMIT: u64 = 16 << 20;
            if let Ok(f) = std::fs::File::open(real.unwrap()) {
                use std::io::Read;
                let n = std::io::copy(&mut f.take(LIMIT + 1), &mut h).unwrap_or(0);
                h.update(n.to_le_bytes());
                if n > LIMIT { h.update(format!("{:?}", std::time::SystemTime::now())); }
            }
        }
        h.update([0xff]);
    }
    format!("{:x}", h.finalize())
}

pub fn trust(p: &Path) -> anyhow::Result<()> {
    let mut m = load();
    m.insert(canon(p), fingerprint(p, &findings(p)));
    std::fs::write(store(), serde_json::to_string_pretty(&m)?)?;
    Ok(())
}

/// Claude project settings are allowlisted: any key not known to be harmless is reported.
const CLAUDE_SAFE: &[&str] = &["$schema", "cleanupPeriodDays", "includeCoAuthoredBy", "outputStyle", "spinnerTipsEnabled", "language"];

/// Repo-local git config is allowlisted: anything outside these plain keys is reported.
fn risky_git_key(k: &str) -> bool {
    let k = k.to_lowercase();
    const SAFE: &[&str] = &["core.repositoryformatversion", "core.bare", "core.filemode", "core.logallrefupdates",
        "core.ignorecase", "core.precomposeunicode", "core.symlinks", "core.autocrlf", "pull.rebase", "init.defaultbranch",
        "user.name", "user.email"];
    // section = before the first dot, variable = after the last dot; anything between is a subsection
    // name, which may itself contain dots or '=' and is never interpreted.
    let (section, var) = match (k.split_once('.'), k.rsplit_once('.')) { (Some((a, _)), Some((_, b))) => (a, b), _ => return true };
    let has_sub = k.matches('.').count() >= 2;
    let sub_ok = has_sub && match section {
        "remote" => ["url", "fetch", "pushurl", "prune"].contains(&var),
        "branch" => ["remote", "merge", "rebase"].contains(&var),
        _ => false,
    };
    !(SAFE.contains(&k.as_str()) || sub_ok)
}

/// Everything in `path` that could execute code. Empty means nothing found.
pub fn findings(path: &Path) -> Vec<String> {
    let mut out = vec![];
    // Claude Code project settings (our own starter hook is known and allowed).
    let starter: serde_json::Value = serde_json::from_str(STARTER_SETTINGS).unwrap_or_default();
    for f in [".claude/settings.json", ".claude/settings.local.json"] {
        let Ok(t) = std::fs::read_to_string(path.join(f)) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { out.push(format!("{f} could not be read")); continue };
        let Some(obj) = v.as_object() else { out.push(format!("{f} is not a settings object")); continue };
        for (k, x) in obj {
            if CLAUDE_SAFE.contains(&k.as_str()) || (k == "hooks" && Some(x) == starter.get("hooks")) { continue; }
            out.push(format!("Claude \"{k}\" in {f}"));
        }
    }
    if path.join(".mcp.json").exists() { out.push("MCP servers in .mcp.json".into()); }

    // Git: resolve both the worktree gitdir and the common dir with the same git the app uses; fail closed.
    if path.join(".git").exists() {
        let bin = crate::claude::find_binary(&None, "git").unwrap_or_else(|| "git".into());
        let rev = |arg: &str| std::process::Command::new(&bin).current_dir(path).env("PATH", crate::claude::shell_path())
            .env("GIT_CONFIG_NOSYSTEM", "1").args(["-c", "core.fsmonitor=false", "rev-parse", arg]).output().ok()
            .filter(|o| o.status.success()).map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
        let (Some(gd), Some(common)) = (rev("--absolute-git-dir"), rev("--git-common-dir")) else {
            out.push("a .git folder that could not be inspected".into());
            out.sort(); out.dedup(); return out;
        };
        let common = if common.is_absolute() { common } else { path.join(common) };
        for d in [&gd, &common] {
            if let Ok(rd) = std::fs::read_dir(d.join("hooks")) {
                if rd.flatten().any(|e| !e.file_name().to_string_lossy().ends_with(".sample")) { out.push("git hooks".into()); }
            }
            for name in ["config", "config.worktree"] {
                let cfg = d.join(name);
                if !cfg.exists() { continue; }
                match std::process::Command::new(&bin).env("PATH", crate::claude::shell_path()).env("GIT_CONFIG_NOSYSTEM", "1")
                    .args(["-c", "core.fsmonitor=false", "config", "--file"]).arg(&cfg).args(["--includes", "--list", "--null", "--name-only"]).output()
                {
                    Ok(o) if o.status.success() => for key in String::from_utf8_lossy(&o.stdout).split('\0').filter(|k| !k.is_empty()) {
                        if risky_git_key(key) { out.push(format!("git setting {}", key.replace('\n', " "))); }
                    },
                    _ => out.push(format!("git {name} that could not be read")),
                }
            }
        }
    }
    out.sort(); out.dedup();
    out
}

/// Fail with a machine-readable UNTRUSTED error when `path` needs the user's trust first.
pub fn require(path: &Path) -> anyhow::Result<()> {
    let f = findings(path);
    if f.is_empty() || load().get(&canon(path)) == Some(&fingerprint(path, &f)) { return Ok(()); }
    anyhow::bail!("UNTRUSTED::{}::{}", path.display(), f.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-trust-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".claude")).unwrap();
        d
    }

    #[test]
    fn starter_hook_is_allowed() {
        let d = dir("starter");
        std::fs::write(d.join(".claude/settings.json"), STARTER_SETTINGS).unwrap();
        assert!(findings(&d).is_empty());
    }

    #[test]
    fn foreign_claude_and_git_settings_are_flagged() {
        let d = dir("foreign");
        std::fs::write(d.join(".claude/settings.json"), r#"{"hooks":{"x":[]},"apiKeyHelper":"evil"}"#).unwrap();
        std::fs::write(d.join(".mcp.json"), "{}").unwrap();
        let g = |a: &[&str]| assert!(std::process::Command::new("git").current_dir(&d).args(a).output().unwrap().status.success());
        g(&["init", "-q"]);
        g(&["config", "filter.x.clean", "evil"]);
        g(&["config", "include.path", "/tmp/elsewhere"]);
        g(&["config", "remote.x=y.uploadpack", "evil"]);
        let f = findings(&d).join("|");
        for want in ["\"hooks\"", "apiKeyHelper", ".mcp.json", "filter.x.clean", "include.path", "uploadpack"] {
            assert!(f.contains(want), "missing {want} in {f}");
        }
    }
}
