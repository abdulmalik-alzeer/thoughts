//! Library trust: before the app or the AI works inside a library it didn't create, list anything in
//! it that could run programs on this machine, and only proceed once the user has trusted that folder.
//! Trust decisions live on the daemon side (~/.thoughts/trusted.json), never in a client flag.

use std::path::{Path, PathBuf};

const STARTER_SETTINGS: &str = include_str!("../../../starter/.claude/settings.json");

fn store() -> PathBuf { crate::paths::home().join("trusted.json") }

fn canon(p: &Path) -> String { std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_string() }

pub fn is_trusted(p: &Path) -> bool {
    let list: Vec<String> = std::fs::read_to_string(store()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    list.contains(&canon(p))
}

pub fn trust(p: &Path) -> anyhow::Result<()> {
    let mut list: Vec<String> = std::fs::read_to_string(store()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let c = canon(p);
    if !list.contains(&c) { list.push(c); }
    std::fs::write(store(), serde_json::to_string_pretty(&list)?)?;
    Ok(())
}

/// Claude settings keys that run commands.
const CLAUDE_KEYS: &[&str] = &["hooks", "apiKeyHelper", "statusLine", "awsAuthRefresh", "awsCredentialExport", "otelHeadersHelper", "fileSuggestion"];

/// Git config keys (as listed by `git config --list`) that can run programs.
fn risky_git_key(k: &str) -> bool {
    let k = k.to_lowercase();
    k.starts_with("include.") || k.starts_with("includeif.") || k.starts_with("filter.") || k.starts_with("credential")
        || k.starts_with("gpg.") || k.starts_with("alias.") || k.ends_with(".textconv") || k.ends_with(".command")
        || k.ends_with(".driver") || k.ends_with(".program") || k.ends_with(".helper")
        || ["core.fsmonitor", "core.hookspath", "core.sshcommand", "core.pager", "core.editor", "core.askpass",
            "diff.external", "sequence.editor", "uploadpack.packobjectshook"].contains(&k.as_str())
}

/// Everything in `path` that could execute code. Empty means nothing found.
pub fn findings(path: &Path) -> Vec<String> {
    let mut out = vec![];
    // Claude Code project settings (our own starter hook is known and allowed).
    let starter: serde_json::Value = serde_json::from_str(STARTER_SETTINGS).unwrap_or_default();
    for f in [".claude/settings.json", ".claude/settings.local.json"] {
        let Ok(t) = std::fs::read_to_string(path.join(f)) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { out.push(format!("{f} could not be read")); continue };
        for k in CLAUDE_KEYS {
            if let Some(x) = v.get(*k) {
                if !(*k == "hooks" && Some(x) == starter.get("hooks")) { out.push(format!("Claude \"{k}\" in {f}")); }
            }
        }
    }
    if path.join(".mcp.json").exists() { out.push("MCP servers in .mcp.json".into()); }

    // Git: resolve the gitdir and the repo's own config the way git itself reads it.
    let gitdir = std::process::Command::new("git").current_dir(path)
        .args(["-c", "core.fsmonitor=false", "rev-parse", "--absolute-git-dir"]).output().ok()
        .filter(|o| o.status.success()).map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
    if let Some(gd) = gitdir {
        if let Ok(rd) = std::fs::read_dir(gd.join("hooks")) {
            if rd.flatten().any(|e| !e.file_name().to_string_lossy().ends_with(".sample")) { out.push("git hooks".into()); }
        }
        let cfg = gd.join("config");
        if let Ok(o) = std::process::Command::new("git").current_dir(path).env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["-c", "core.fsmonitor=false", "config", "--file"]).arg(&cfg).args(["--includes", "--list"]).output()
        {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let key = line.split('=').next().unwrap_or("");
                if risky_git_key(key) { out.push(format!("git setting {key}")); }
            }
        }
    }
    out.sort(); out.dedup();
    out
}

/// Fail with a machine-readable UNTRUSTED error when `path` needs the user's trust first.
pub fn require(path: &Path) -> anyhow::Result<()> {
    if is_trusted(path) { return Ok(()); }
    let f = findings(path);
    if f.is_empty() { Ok(()) } else { anyhow::bail!("UNTRUSTED::{}::{}", path.display(), f.join("; ")) }
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
        let f = findings(&d).join("|");
        for want in ["\"hooks\"", "apiKeyHelper", ".mcp.json", "filter.x.clean", "include.path"] {
            assert!(f.contains(want), "missing {want} in {f}");
        }
    }
}
