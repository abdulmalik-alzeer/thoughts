//! A local-only log of how the app is used (`~/.thoughts/usage-events.jsonl`). It feeds the
//! suggestion heuristics and never leaves this machine.

use chrono::{DateTime, Local};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 1024 * 1024;
const KEEP_LINES: usize = 6000;

pub fn file(home: &Path) -> PathBuf { home.join("usage-events.jsonl") }

/// Event names are short labels such as `view.todos`, `home.prompt:Plan my week`, `todo.add`,
/// `todo.done`, `doc.open`, `search`, `palette.cmd:<id>` or `customize.keep:<request>`. Any label
/// is stored; the file is capped.
pub fn clean_event(event: &str) -> anyhow::Result<String> {
    let e = event.trim();
    if e.is_empty() { anyhow::bail!("empty event"); }
    if e.chars().any(char::is_control) { anyhow::bail!("invalid event"); }
    Ok(e.chars().take(120).collect())
}

/// Record how a customisation ended (`keep`, `discard` or `undo`) with the first 80 characters of
/// what the user asked for, so later suggestions can learn from it.
pub fn record_outcome(home: &Path, outcome: &str, request: &str, now: DateTime<Local>) {
    let request: String = request.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
    let _ = append(home, &format!("customize.{outcome}:{request}"), now);
}

/// A short plain-text account of the last 30 days, for the monthly tune-up suggestion. It is built
/// on this machine and goes nowhere unless the user accepts that suggestion.
pub fn summary(events: &[(DateTime<Local>, String)], now: DateTime<Local>) -> String {
    use std::collections::BTreeMap;
    let since = now - chrono::Duration::days(30);
    let mut counts: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    let mut requests: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut bump = |group: &'static str, key: &str| *counts.entry(group).or_default().entry(key.chars().take(60).collect()).or_default() += 1;
    for (t, e) in events.iter().filter(|(t, _)| *t > since && *t <= now) {
        let _ = t;
        if let Some(v) = e.strip_prefix("view.") { bump("views", v); }
        else if let Some(c) = e.strip_prefix("home.chip:") { bump("chips", c.trim()); }
        else if let Some(c) = e.strip_prefix("palette.cmd:") { bump("commands", c.trim()); }
        else if e.starts_with("home.prompt:") { bump("other", "requests typed on Home"); }
        else if e == "todo.add" { bump("other", "todos added"); }
        else if e == "todo.done" { bump("other", "todos completed"); }
        else if e == "doc.open" || e.starts_with("doc.open:") { bump("other", "documents opened"); }
        else if e == "search" || e.starts_with("search:") { bump("other", "searches"); }
        else if e == "wrap.done" { bump("other", "wraps"); }
        else if let Some(rest) = e.strip_prefix("customize.") {
            let (kind, text) = rest.split_once(':').unwrap_or((rest, ""));
            let key = match kind { "keep" => "kept", "discard" => "discarded", "undo" => "undone", _ => continue };
            let list = requests.entry(key).or_default();
            if list.len() < 8 { list.push(text.trim().to_string()); }
        }
    }
    let top = |group: &str, n: usize| -> String {
        let mut v: Vec<(&String, &usize)> = counts.get(group).into_iter().flatten().collect();
        v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        if v.is_empty() { return "none".into(); }
        v.into_iter().take(n).map(|(k, c)| format!("{k} ×{c}")).collect::<Vec<_>>().join(", ")
    };
    let asked = |key: &str| -> String {
        match requests.get(key) {
            Some(l) if !l.is_empty() => l.iter().map(|r| if r.is_empty() { "\"(no text)\"".to_string() } else { format!("\"{r}\"") }).collect::<Vec<_>>().join("; "),
            _ => "none".into(),
        }
    };
    format!(
        "- Pages opened: {}\n- Home buttons used: {}\n- Commands used: {}\n- Other activity: {}\n- Customisations kept: {}\n- Customisations discarded: {}\n- Customisations undone later: {}",
        top("views", 10), top("chips", 6), top("commands", 6), top("other", 8), asked("kept"), asked("discarded"), asked("undone"))
}

pub fn append(home: &Path, event: &str, now: DateTime<Local>) -> anyhow::Result<()> {
    use std::io::Write;
    // One writer at a time: the window reports events side by side, and trimming the file (read,
    // then replace) must not drop a line appended meanwhile.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let event = clean_event(event)?;
    let path = file(home);
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
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
    // The whole line in one write, never in pieces another line could land between.
    f.write_all(format!("{}\n", json!({"t": now.to_rfc3339(), "e": event})).as_bytes())?;
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

    #[test]
    fn no_line_is_lost_or_garbled_when_events_come_in_side_by_side() {
        let d = std::env::temp_dir().join(format!("thoughts-usage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        // Just over the size at which the file is trimmed, so the trim races the appends.
        let old = format!("{}\n", json!({"t": Local::now().to_rfc3339(), "e": "x".repeat(100)}));
        std::fs::write(file(&d), old.repeat((MAX_BYTES as usize / old.len()) + 2)).unwrap();
        let threads: Vec<_> = (0..8).map(|i| { let d = d.clone(); std::thread::spawn(move || {
            for n in 0..200 { append(&d, &format!("view.t{i}-{n}"), Local::now()).unwrap(); }
        }) }).collect();
        for t in threads { t.join().unwrap(); }
        let text = std::fs::read_to_string(file(&d)).unwrap();
        assert!(text.lines().all(|l| serde_json::from_str::<Value>(l).is_ok()), "a garbled line");
        let ours = read(&d).iter().filter(|(_, e)| e.starts_with("view.t")).count();
        assert_eq!(ours, 1600, "every event is there once");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn records_outcomes_and_summarises_the_last_month() {
        let d = std::env::temp_dir().join(format!("thoughts-usage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let now = Local::now();
        for e in ["view.todos", "view.todos", "view.docs", "home.chip:Plan my week", "todo.add", "todo.done", "doc.open", "search", "palette.cmd:new-chat"] {
            append(&d, e, now - chrono::Duration::hours(1)).unwrap();
        }
        append(&d, "view.projects", now - chrono::Duration::days(45)).unwrap();
        record_outcome(&d, "keep", &format!("Show overdue\ntodos on Home {}", "x".repeat(200)), now);
        record_outcome(&d, "discard", "Make everything pink", now);
        record_outcome(&d, "undo", "", now);
        let ev = read(&d);
        let kept = &ev.iter().find(|(_, e)| e.starts_with("customize.keep:")).unwrap().1;
        assert!(kept.starts_with("customize.keep:Show overdue todos on Home x") && kept.chars().count() == "customize.keep:".len() + 80);
        let s = summary(&ev, now);
        assert!(s.contains("Pages opened: todos ×2, docs ×1"), "{s}");
        assert!(!s.contains("projects"), "older than a month");
        assert!(s.contains("Plan my week ×1") && s.contains("new-chat ×1") && s.contains("todos added ×1") && s.contains("searches ×1"));
        assert!(s.contains("Customisations discarded: \"Make everything pink\"") && s.contains("undone later: \"(no text)\""));
        assert!(summary(&[], now).contains("Pages opened: none"));
        std::fs::remove_dir_all(d).unwrap();
    }
}
