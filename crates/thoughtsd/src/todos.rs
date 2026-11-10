//! Todo files: root `TODO.md`, `tasks/TODO.md`, and `<area>/<slug>/todo.md`.
//! Items are top-level GFM task lines; edits happen in place, line by line.
//! A due date is the suffix ` @due(YYYY-MM-DD)` on the task line.

use crate::library::resolve;
use serde_json::{json, Value};
use std::path::Path;

pub fn files(root: &Path) -> Vec<String> {
    let mut out = vec![];
    for cand in ["TODO.md", "tasks/TODO.md"] {
        if root.join(cand).is_file() { out.push(cand.to_string()); }
    }
    for e in walkdir::WalkDir::new(root).min_depth(3).max_depth(3).into_iter()
        .filter_entry(|e| e.file_name() != ".git").flatten()
    {
        if e.file_name() == "todo.md" {
            if let Ok(rel) = e.path().strip_prefix(root) { out.push(rel.to_string_lossy().replace('\\', "/")); }
        }
    }
    out
}

/// Split a task's raw text into `(title, due)`. Only a real calendar date at the very end counts.
pub fn split_due(text: &str) -> (&str, Option<&str>) {
    let t = text.trim_end();
    if let Some(body) = t.strip_suffix(')') {
        if let Some(i) = body.rfind("@due(") {
            let date = &body[i + 5..];
            let before = &body[..i];
            let well_formed = date.len() == 10 && date.bytes().all(|b| b.is_ascii_digit() || b == b'-')
                && chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok();
            if well_formed && (before.is_empty() || before.ends_with(char::is_whitespace)) {
                return (before.trim_end(), Some(date));
            }
        }
    }
    (text, None)
}

pub fn parse(text: &str) -> Vec<Value> {
    let mut group = String::new();
    let mut items = vec![];
    for (i, l) in text.lines().enumerate() {
        if let Some(h) = l.strip_prefix("## ") { group = h.trim().to_string(); continue; }
        let (done, rest) = if let Some(r) = l.strip_prefix("- [ ] ") { (false, r) }
            else if let Some(r) = l.strip_prefix("- [x] ").or_else(|| l.strip_prefix("- [X] ")) { (true, r) }
            else { continue };
        let (title, due) = split_due(rest);
        items.push(json!({"line": i, "text": rest, "title": title, "due": due, "done": done, "group": group}));
    }
    items
}

pub fn list(root: &Path) -> Vec<Value> {
    files(root).into_iter().filter_map(|f| {
        let text = std::fs::read_to_string(root.join(&f)).ok()?;
        let idea = if f.ends_with("/todo.md") && f.matches('/').count() == 2 { Some(f.trim_end_matches("/todo.md").to_string()) } else { None };
        Some(json!({"file": f, "idea": idea, "items": parse(&text)}))
    }).collect()
}

fn rewrite<F: FnOnce(&mut Vec<String>) -> anyhow::Result<()>>(root: &Path, file: &str, f: F) -> anyhow::Result<()> {
    let p = resolve(root, file)?;
    // Only a missing file starts empty. Any other read failure (permissions, bad encoding, a
    // cloud placeholder…) must stop here — otherwise the write below would wipe the real file.
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => anyhow::bail!("could not read {file}: {e}"),
    };
    let trailing = text.ends_with('\n') || text.is_empty();
    // A file written on Windows keeps its line endings.
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    f(&mut lines)?;
    let mut out = lines.join(eol);
    if trailing { out.push_str(eol); }
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    crate::paths::write_atomic(&p, out.as_bytes(), false)?;
    Ok(())
}

fn is_task(l: &str) -> bool {
    l.starts_with("- [ ] ") || l.starts_with("- [x] ") || l.starts_with("- [X] ")
}

/// The task the client means must still be exactly the one on that line: same line, a task, the
/// same text. An empty `expect` never matches, so it cannot be used to skip the comparison.
fn check_line(lines: &[String], line: usize, expect: &str) -> anyhow::Result<()> {
    let stale = || crate::tr::err("stale.todo", &[]);
    let l = lines.get(line).ok_or_else(stale)?;
    if !is_task(l) || expect.trim().is_empty() || l[6..].trim_end() != expect.trim_end() { return Err(stale()); }
    Ok(())
}

pub fn toggle(root: &Path, file: &str, line: usize, text: &str) -> anyhow::Result<()> {
    rewrite(root, file, |lines| {
        check_line(lines, line, text)?;
        let l = &lines[line];
        lines[line] = if l.starts_with("- [ ] ") { l.replacen("- [ ] ", "- [x] ", 1) } else { format!("- [ ] {}", &l[6..]) };
        Ok(())
    })
}

/// One line of task text: line breaks become spaces.
fn one_line(text: &str) -> String { text.replace("\r\n", " ").replace(['\n', '\r'], " ").trim().to_string() }

pub fn edit(root: &Path, file: &str, line: usize, old: &str, new: &str) -> anyhow::Result<()> {
    let new = one_line(new);
    if new.is_empty() { return Err(crate::tr::err("todo.empty", &[])); }
    rewrite(root, file, |lines| {
        check_line(lines, line, old)?;
        let prefix = lines[line][..6].to_string();
        lines[line] = format!("{prefix}{new}");
        Ok(())
    })
}

pub fn delete(root: &Path, file: &str, line: usize, text: &str) -> anyhow::Result<()> {
    rewrite(root, file, |lines| { check_line(lines, line, text)?; lines.remove(line); Ok(()) })
}

/// New items go at the end of the top group (or the end of the file).
pub fn add(root: &Path, file: &str, text: &str) -> anyhow::Result<()> {
    let text = one_line(text);
    if text.is_empty() { return Err(crate::tr::err("todo.empty", &[])); }
    rewrite(root, file, |lines| {
        let item = format!("- [ ] {text}");
        if lines.is_empty() { lines.push("# Todo".into()); lines.push(String::new()); }
        let first_h2 = lines.iter().position(|l| l.starts_with("## "));
        let insert_at = match first_h2 {
            Some(h) => {
                let next_h2 = lines.iter().enumerate().skip(h + 1).find(|(_, l)| l.starts_with("## ")).map(|(i, _)| i).unwrap_or(lines.len());
                let last_item = (h + 1..next_h2).rev().find(|&i| lines[i].starts_with("- [")).map(|i| i + 1);
                last_item.unwrap_or(h + 1)
            }
            None => {
                let last = lines.iter().rposition(|l| l.starts_with("- [")).map(|i| i + 1);
                last.unwrap_or(lines.len())
            }
        };
        lines.insert(insert_at, item);
        Ok(())
    })
}

/// Fold one iCalendar content line at 75 bytes (never inside a character), as the format requires.
fn ics_fold(line: &str) -> String {
    let mut out = String::new();
    let mut len = 0;
    for ch in line.chars() {
        if len + ch.len_utf8() > 75 { out.push_str("\r\n "); len = 1; }
        out.push(ch);
        len += ch.len_utf8();
    }
    out
}

fn ics_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace(['\r', '\n'], " ")
}

/// An iCalendar file with one all-day event per open task that has a due date. Each event's UID
/// is derived from the list and the task's title, so importing the file again updates the same
/// events rather than adding duplicates — and moving a due date moves the event.
pub fn calendar(root: &Path, now: chrono::DateTime<chrono::Utc>) -> String {
    use sha2::{Digest, Sha256};
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let mut lines: Vec<String> = vec!["BEGIN:VCALENDAR".into(), "VERSION:2.0".into(), "PRODID:-//Thoughts//Todos//EN".into(),
        "CALSCALE:GREGORIAN".into(), "METHOD:PUBLISH".into(), "X-WR-CALNAME:Thoughts".into()];
    let mut seen = std::collections::HashSet::new();
    for list in list(root) {
        let file = list["file"].as_str().unwrap_or_default().to_string();
        for item in list["items"].as_array().into_iter().flatten() {
            if item["done"] == true { continue; }
            let (Some(due), Some(title)) = (item["due"].as_str(), item["title"].as_str()) else { continue };
            let Ok(day) = chrono::NaiveDate::parse_from_str(due, "%Y-%m-%d") else { continue };
            let Some(next) = day.succ_opt() else { continue };
            let mut uid = format!("{:x}", Sha256::digest(format!("{file}\n{title}").as_bytes()))[..32].to_string();
            // Two identical tasks in one list still need different ids.
            let mut n = 1;
            while !seen.insert(uid.clone()) { n += 1; uid = format!("{:x}", Sha256::digest(format!("{file}\n{title}\n{n}").as_bytes()))[..32].to_string(); }
            lines.push("BEGIN:VEVENT".into());
            lines.push(format!("UID:{uid}@thoughts.app"));
            lines.push(format!("DTSTAMP:{stamp}"));
            lines.push(format!("DTSTART;VALUE=DATE:{}", day.format("%Y%m%d")));
            lines.push(format!("DTEND;VALUE=DATE:{}", next.format("%Y%m%d")));
            lines.push(format!("SUMMARY:{}", ics_escape(title)));
            let group = item["group"].as_str().unwrap_or_default();
            let place = match (list["idea"].as_str(), group.is_empty()) {
                (Some(idea), _) => idea.to_string(),
                (None, false) => group.to_string(),
                (None, true) => String::new(),
            };
            if !place.is_empty() { lines.push(format!("DESCRIPTION:{}", ics_escape(&place))); }
            lines.push("TRANSP:TRANSPARENT".into());
            lines.push("END:VEVENT".into());
        }
    }
    lines.push("END:VCALENDAR".into());
    lines.iter().map(|l| ics_fold(l)).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_must_name_the_exact_task_and_leave_something() {
        let d = std::env::temp_dir().join(format!("todo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let original = "# T\n- [ ] call the bank\n- [x] pay rent\n";
        std::fs::write(d.join("TODO.md"), original).unwrap();
        // An empty "old text" used to match any line; so did a mere suffix of the real text.
        for (line, old) in [(1, ""), (1, "  "), (1, "bank"), (1, "pay rent"), (0, "T"), (9, "call the bank")] {
            assert!(edit(&d, "TODO.md", line, old, "x").is_err(), "{line} {old:?}");
            assert!(delete(&d, "TODO.md", line, old).is_err(), "{line} {old:?}");
            assert!(toggle(&d, "TODO.md", line, old).is_err(), "{line} {old:?}");
        }
        // An edit that would leave nothing is refused — after the comparison, never instead of it.
        for blank in ["", "   ", "\n"] { assert!(edit(&d, "TODO.md", 1, "call the bank", blank).is_err()); }
        assert!(add(&d, "TODO.md", " \n ").is_err());
        assert_eq!(std::fs::read_to_string(d.join("TODO.md")).unwrap(), original);
        edit(&d, "TODO.md", 1, "call the bank", "call the bank\nabout the loan").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("TODO.md")).unwrap(), "# T\n- [ ] call the bank about the loan\n- [x] pay rent\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn windows_line_endings_are_kept() {
        let d = std::env::temp_dir().join(format!("todo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("TODO.md"), "# Todo\r\n\r\n## Now\r\n- [ ] a\r\n").unwrap();
        add(&d, "TODO.md", "b").unwrap();
        toggle(&d, "TODO.md", 3, "a").unwrap();
        edit(&d, "TODO.md", 4, "b", "c").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("TODO.md")).unwrap(), "# Todo\r\n\r\n## Now\r\n- [x] a\r\n- [ ] c\r\n");
        delete(&d, "TODO.md", 4, "c").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("TODO.md")).unwrap(), "# Todo\r\n\r\n## Now\r\n- [x] a\r\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn calendar_lists_open_dated_tasks_as_all_day_events() {
        let d = std::env::temp_dir().join(format!("todo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("clients/acme")).unwrap();
        std::fs::write(d.join("TODO.md"), "## Now\n- [ ] File VAT return; Q3, final @due(2026-12-31)\n- [x] done @due(2026-11-01)\n- [ ] no date\n").unwrap();
        let long = "راجع عقد التوريد مع الشركة وأرسل الملاحظات إلى المحامي قبل نهاية الأسبوع القادم";
        std::fs::write(d.join("clients/acme/todo.md"), format!("- [ ] {long} @due(2027-02-28)\n")).unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-10T08:00:00Z").unwrap().with_timezone(&chrono::Utc);
        let ics = calendar(&d, now);
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:") && ics.ends_with("END:VCALENDAR\r\n"));
        assert_eq!(ics.matches("BEGIN:VEVENT").count(), 2);
        assert_eq!(ics.matches("END:VEVENT").count(), 2);
        assert!(ics.contains("DTSTART;VALUE=DATE:20261231\r\nDTEND;VALUE=DATE:20270101\r\n"));
        assert!(ics.contains("DTSTART;VALUE=DATE:20270228\r\nDTEND;VALUE=DATE:20270301\r\n"));
        assert!(ics.contains("SUMMARY:File VAT return\\; Q3\\, final\r\n"));
        assert!(ics.contains("DESCRIPTION:Now\r\n") && ics.contains("DESCRIPTION:clients/acme\r\n"));
        assert!(ics.contains("DTSTAMP:20261110T080000Z"));
        // Every line ends in CRLF, none is longer than 75 bytes, and long ones continue with a space.
        for l in ics.split("\r\n") { assert!(l.len() <= 75, "{l}"); assert!(!l.contains('\n')); }
        let unfolded = ics.replace("\r\n ", "");
        assert!(unfolded.contains(&format!("SUMMARY:{long}\r\n")));
        // The same tasks give the same ids every time; a different task gives a different one.
        let uids = |s: &str| s.replace("\r\n ", "").lines().filter(|l| l.starts_with("UID:")).map(String::from).collect::<Vec<_>>();
        assert_eq!(uids(&ics), uids(&calendar(&d, now + chrono::Duration::days(3))));
        assert_eq!(uids(&ics).len(), 2);
        assert_ne!(uids(&ics)[0], uids(&ics)[1]);
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn add_toggle() {
        let d = std::env::temp_dir().join(format!("todo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("TODO.md"), "# Todo\n\n## Now\n- [ ] a\n\n## Later\n- [ ] z\n").unwrap();
        add(&d, "TODO.md", "b").unwrap();
        let t = std::fs::read_to_string(d.join("TODO.md")).unwrap();
        assert_eq!(t, "# Todo\n\n## Now\n- [ ] a\n- [ ] b\n\n## Later\n- [ ] z\n");
        toggle(&d, "TODO.md", 4, "b").unwrap();
        assert!(std::fs::read_to_string(d.join("TODO.md")).unwrap().contains("- [x] b"));
        assert_eq!(parse(&std::fs::read_to_string(d.join("TODO.md")).unwrap()).len(), 3);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn due_dates() {
        assert_eq!(split_due("pay rent @due(2026-12-01)"), ("pay rent", Some("2026-12-01")));
        assert_eq!(split_due("pay rent @due(2026-12-01)  "), ("pay rent", Some("2026-12-01")));
        assert_eq!(split_due("ادفع الإيجار @due(2026-12-01)"), ("ادفع الإيجار", Some("2026-12-01")));
        // Not a real date, not at the end, or glued to a word: left alone.
        for raw in ["a @due(2026-13-40)", "a @due(tomorrow)", "a @due(2026-12-01) b", "a@due(2026-12-01)", "a @due(2026-1-1)", "plain"] {
            assert_eq!(split_due(raw), (raw, None), "{raw}");
        }
        let items = parse("## Now\n- [ ] call bank @due(2026-11-20)\n- [x] done thing\n");
        assert_eq!(items[0]["title"], "call bank");
        assert_eq!(items[0]["due"], "2026-11-20");
        assert_eq!(items[0]["text"], "call bank @due(2026-11-20)");
        assert_eq!(items[0]["group"], "Now");
        assert!(items[1]["due"].is_null());
        assert_eq!(items[1]["title"], "done thing");
    }

    #[test]
    fn failed_read_never_overwrites() {
        let d = std::env::temp_dir().join(format!("todo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        // Not valid UTF-8: reading as text fails, so every edit must fail and leave the bytes alone.
        let raw = b"# Todo\n- [ ] keep me \xff\xfe\n".to_vec();
        std::fs::write(d.join("TODO.md"), &raw).unwrap();
        assert!(add(&d, "TODO.md", "new").is_err());
        assert!(toggle(&d, "TODO.md", 1, "keep me").is_err());
        assert_eq!(std::fs::read(d.join("TODO.md")).unwrap(), raw);
        // A missing file is still created on first add.
        add(&d, "tasks/TODO.md", "first @due(2026-01-02)").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("tasks/TODO.md")).unwrap(), "# Todo\n\n- [ ] first @due(2026-01-02)\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn edits_refuse_non_task_lines_and_protected_files() {
        let d = std::env::temp_dir().join(format!("todo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("TODO.md"), "# T\n- [ ] a\n").unwrap();
        assert!(edit(&d, "TODO.md", 0, "T", "x").is_err());
        assert!(toggle(&d, "TODO.md", 0, "").is_err());
        assert!(add(&d, ".git/TODO.md", "x").is_err());
        assert!(add(&d, ".claude/skills/x/todo.md", "x").is_err());
        edit(&d, "TODO.md", 1, "a", "b @due(2026-03-04)").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("TODO.md")).unwrap(), "# T\n- [ ] b @due(2026-03-04)\n");
        std::fs::remove_dir_all(d).unwrap();
    }
}
