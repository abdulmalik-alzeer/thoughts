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
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    f(&mut lines)?;
    let mut out = lines.join("\n");
    if trailing { out.push('\n'); }
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    crate::paths::write_atomic(&p, out.as_bytes(), false)?;
    Ok(())
}

fn is_task(l: &str) -> bool {
    l.starts_with("- [ ] ") || l.starts_with("- [x] ") || l.starts_with("- [X] ")
}

fn check_line(lines: &[String], line: usize, expect: &str) -> anyhow::Result<()> {
    let l = lines.get(line).ok_or_else(|| anyhow::anyhow!("todo moved — reload"))?;
    if !is_task(l) || !l.ends_with(expect) { anyhow::bail!("todo changed on disk — reload"); }
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

pub fn edit(root: &Path, file: &str, line: usize, old: &str, new: &str) -> anyhow::Result<()> {
    rewrite(root, file, |lines| {
        check_line(lines, line, old)?;
        let prefix = lines[line][..6].to_string();
        lines[line] = format!("{prefix}{}", new.replace('\n', " "));
        Ok(())
    })
}

pub fn delete(root: &Path, file: &str, line: usize, text: &str) -> anyhow::Result<()> {
    rewrite(root, file, |lines| { check_line(lines, line, text)?; lines.remove(line); Ok(()) })
}

/// New items go at the end of the top group (or the end of the file).
pub fn add(root: &Path, file: &str, text: &str) -> anyhow::Result<()> {
    rewrite(root, file, |lines| {
        let item = format!("- [ ] {}", text.replace('\n', " "));
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

#[cfg(test)]
mod tests {
    use super::*;
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
