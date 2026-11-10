//! A local-only log of how the app is used (`~/.thoughts/usage-events.jsonl`). It feeds the
//! suggestion heuristics and never leaves this machine.

use chrono::{DateTime, Local};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 1024 * 1024;
const KEEP_LINES: usize = 6000;

pub fn file(home: &Path) -> PathBuf { home.join("usage-events.jsonl") }

/// Event names are short labels such as `view.todos` or `home.prompt:Plan my week`.
pub fn clean_event(event: &str) -> anyhow::Result<String> {
    let e = event.trim();
    if e.is_empty() { anyhow::bail!("empty event"); }
    if e.chars().any(char::is_control) { anyhow::bail!("invalid event"); }
    Ok(e.chars().take(120).collect())
}

pub fn append(home: &Path, event: &str, now: DateTime<Local>) -> anyhow::Result<()> {
    use std::io::Write;
    let event = clean_event(event)?;
    let path = file(home);
    if std::fs::metadata(&path).map_or(false, |m| m.len() > MAX_BYTES) {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let kept = lines[lines.len().saturating_sub(KEEP_LINES)..].join("\n") + "\n";
        crate::paths::write_atomic(&path, kept.as_bytes(), true)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.append(true).create(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; opts.mode(0o600); }
    let mut f = opts.open(&path)?;
    writeln!(f, "{}", json!({"t": now.to_rfc3339(), "e": event}))?;
    Ok(())
}

/// Every recorded event, oldest first.
pub fn read(home: &Path) -> Vec<(DateTime<Local>, String)> {
    std::fs::read_to_string(file(home)).unwrap_or_default().lines().filter_map(|l| {
        let v: Value = serde_json::from_str(l).ok()?;
        let t = DateTime::parse_from_rfc3339(v["t"].as_str()?).ok()?.with_timezone(&Local);
        Some((t, v["e"].as_str()?.to_string()))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_and_reads_back() {
        let d = std::env::temp_dir().join(format!("thoughts-usage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let now = Local::now();
        append(&d, " view.todos ", now).unwrap();
        append(&d, &"x".repeat(500), now).unwrap();
        assert!(append(&d, "a\nb", now).is_err() && append(&d, "  ", now).is_err());
        let ev = read(&d);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].1, "view.todos");
        assert_eq!(ev[1].1.len(), 120);
        std::fs::remove_dir_all(d).unwrap();
    }
}
