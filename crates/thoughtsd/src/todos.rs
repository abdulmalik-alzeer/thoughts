//! Todo files: root `TODO.md`, `tasks/TODO.md`, and `<area>/<slug>/todo.md`.
//! Items are top-level GFM task lines; edits happen in place, line by line.

use crate::paths::safe_join;
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
            out.push(e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
    out
}

pub fn parse(text: &str) -> Vec<Value> {
    let mut group = String::new();
    let mut items = vec![];
    for (i, l) in text.lines().enumerate() {
        if let Some(h) = l.strip_prefix("## ") { group = h.trim().to_string(); continue; }
        let (done, rest) = if let Some(r) = l.strip_prefix("- [ ] ") { (false, r) }
            else if let Some(r) = l.strip_prefix("- [x] ").or_else(|| l.strip_prefix("- [X] ")) { (true, r) }
            else { continue };
        items.push(json!({"line": i, "text": rest, "done": done, "group": group}));
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
    let p = safe_join(root, file)?;
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    let trailing = text.ends_with('\n') || text.is_empty();
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    f(&mut lines)?;
    let mut out = lines.join("\n");
    if trailing { out.push('\n'); }
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(p, out)?;
    Ok(())
}

fn check_line(lines: &[String], line: usize, expect: &str) -> anyhow::Result<()> {
    let l = lines.get(line).ok_or_else(|| anyhow::anyhow!("todo moved — reload"))?;
    if !l.ends_with(expect) { anyhow::bail!("todo changed on disk — reload"); }
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
}
