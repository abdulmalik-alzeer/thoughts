//! Todo files: root `TODO.md`, `tasks/TODO.md`, and `<area>/<slug>/todo.md`.
//! Items are top-level GFM task lines; edits happen in place, line by line.
//! A due date is the suffix ` @due(YYYY-MM-DD)` on the task line.

use crate::library::resolve;
use serde_json::{json, Value};
use std::path::Path;

/// The general list: `tasks/TODO.md` (the starter's place for it) or root `TODO.md`, whichever
/// exists — `tasks/TODO.md` when both do. When neither exists yet, the library's own rules decide:
/// a `CLAUDE.md` that names `tasks/TODO.md`, or an existing `tasks/` folder, means `tasks/TODO.md`.
pub fn general(root: &Path) -> String {
    const TASKS: &str = "tasks/TODO.md";
    if root.join(TASKS).is_file() { return TASKS.into(); }
    if root.join("TODO.md").is_file() { return "TODO.md".into(); }
    let rules = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap_or_default();
    if rules.contains(TASKS) || root.join("tasks").is_dir() { TASKS.into() } else { "TODO.md".into() }
}

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

/// How often a task can repeat: the ending ` @every(month)`. Ticking such a task off moves its due
/// date to the next time instead of finishing it.
pub const REPEATS: &[&str] = &["day", "week", "2weeks", "month", "month-end", "quarter", "year"];

/// `(title, repeat)` when the text ends with a known ` @every(…)`.
fn split_every(text: &str) -> Option<(&str, &str)> {
    let body = text.trim_end().strip_suffix(')')?;
    let i = body.rfind("@every(")?;
    let (before, every) = (&body[..i], &body[i + 7..]);
    (REPEATS.contains(&every) && (before.is_empty() || before.ends_with(char::is_whitespace))).then(|| (before.trim_end(), every))
}

/// Split a task's raw text into `(title, due, repeat)`. The endings may come in either order; the
/// app writes `@every(…)` before `@due(…)`, so a reader that only knows `@due` still finds it last.
pub fn split_task(text: &str) -> (&str, Option<&str>, Option<&str>) {
    let (mut title, mut due, mut every) = (text, None, None);
    loop {
        if due.is_none() {
            let (t, d) = split_due(title);
            if d.is_some() { title = t; due = d; continue; }
        }
        if every.is_none() {
            if let Some((t, e)) = split_every(title) { title = t; every = Some(e); continue; }
        }
        return (title, due, every);
    }
}

/// A task's text from its parts, in the order the app writes them.
pub fn compose(title: &str, every: Option<&str>, due: Option<&str>) -> String {
    let mut s = title.trim_end().to_string();
    if let Some(e) = every { s.push_str(&format!(" @every({e})")); }
    if let Some(d) = due { s.push_str(&format!(" @due({d})")); }
    s.trim_start().to_string()
}

fn last_of_month(y: i32, m: u32) -> Option<chrono::NaiveDate> {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    chrono::NaiveDate::from_ymd_opt(ny, nm, 1)?.pred_opt()
}

/// The next time after `from` for a repeat. A monthly task on the 31st falls on the last day of
/// shorter months; `month-end` is always the last day of a month.
pub fn next_due(from: chrono::NaiveDate, every: &str) -> Option<chrono::NaiveDate> {
    use chrono::{Datelike, Days, Months};
    match every {
        "day" => from.checked_add_days(Days::new(1)),
        "week" => from.checked_add_days(Days::new(7)),
        "2weeks" => from.checked_add_days(Days::new(14)),
        "month" => from.checked_add_months(Months::new(1)),
        "quarter" => from.checked_add_months(Months::new(3)),
        "year" => from.checked_add_months(Months::new(12)),
        "month-end" => {
            let end = last_of_month(from.year(), from.month())?;
            if from < end { Some(end) } else { let n = from.checked_add_days(Days::new(1))?; last_of_month(n.year(), n.month()) }
        }
        _ => None,
    }
}

/// The first time after today, counted on from the task's own due date (or from today without one),
/// so a monthly task that was missed for a while lands on its next real date, not on a backlog.
pub fn next_occurrence(due: Option<chrono::NaiveDate>, every: &str, today: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    // Months are counted from the task's own day each time, so a short month on the way (the 28th
    // of February for a task on the 31st) does not pull every later date to that day.
    let months = match every { "month" => Some(1), "quarter" => Some(3), "year" => Some(12), _ => None };
    if let Some(m) = months {
        let from = due.unwrap_or(today);
        for k in 1..=40_000u32 {
            let d = from.checked_add_months(chrono::Months::new(m * k))?;
            if d > today { return Some(d); }
        }
        return None;
    }
    let mut d = due.unwrap_or(today);
    // Stepping keeps the weekday or the day of the month; a century of daily steps is still quick.
    for _ in 0..40_000 {
        d = next_due(d, every)?;
        if d > today { return Some(d); }
    }
    None
}

pub fn parse(text: &str) -> Vec<Value> {
    let mut group = String::new();
    let mut items = vec![];
    for (i, l) in text.lines().enumerate() {
        if let Some(h) = l.strip_prefix("## ") { group = h.trim().to_string(); continue; }
        let (done, rest) = if let Some(r) = l.strip_prefix("- [ ] ") { (false, r) }
            else if let Some(r) = l.strip_prefix("- [x] ").or_else(|| l.strip_prefix("- [X] ")) { (true, r) }
            else { continue };
        let (title, due, every) = split_task(rest);
        items.push(json!({"line": i, "text": rest, "title": title, "due": due, "every": every, "done": done, "group": group}));
    }
    items
}

/// The `##` groups of a list, in file order.
pub fn groups(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for l in text.lines() {
        if let Some(h) = l.strip_prefix("## ") {
            let h = h.trim().to_string();
            if !h.is_empty() && !out.contains(&h) { out.push(h); }
        }
    }
    out
}

pub fn list(root: &Path) -> Vec<Value> {
    files(root).into_iter().filter_map(|f| {
        let text = std::fs::read_to_string(root.join(&f)).ok()?;
        let idea = if f.ends_with("/todo.md") && f.matches('/').count() == 2 { Some(f.trim_end_matches("/todo.md").to_string()) } else { None };
        Some(json!({"file": f, "idea": idea, "groups": groups(&text), "items": parse(&text)}))
    }).collect()
}

/// A list's lines. Only a missing file starts empty. Any other read failure (permissions, bad
/// encoding, a cloud placeholder…) must stop here — otherwise a write would wipe the real file.
fn load(p: &Path, file: &str) -> anyhow::Result<String> {
    match std::fs::read_to_string(p) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => anyhow::bail!("could not read {file}: {e}"),
    }
}

fn rewrite<T, F: FnOnce(&mut Vec<String>) -> anyhow::Result<T>>(root: &Path, file: &str, f: F) -> anyhow::Result<T> {
    let p = resolve(root, file)?;
    let text = load(&p, file)?;
    let trailing = text.ends_with('\n') || text.is_empty();
    // A file written on Windows keeps its line endings.
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let out_value = f(&mut lines)?;
    let mut out = lines.join(eol);
    if trailing { out.push_str(eol); }
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    crate::paths::write_atomic(&p, out.as_bytes(), false)?;
    Ok(out_value)
}

fn is_task(l: &str) -> bool {
    l.starts_with("- [ ] ") || l.starts_with("- [x] ") || l.starts_with("- [X] ")
}
fn is_done(l: &str) -> bool { l.starts_with("- [x] ") || l.starts_with("- [X] ") }

/// The task the client means must still be exactly the one on that line: same line, a task, the
/// same text. An empty `expect` never matches, so it cannot be used to skip the comparison.
fn check_line(lines: &[String], line: usize, expect: &str) -> anyhow::Result<()> {
    let stale = || crate::tr::err("stale.todo", &[]);
    let l = lines.get(line).ok_or_else(stale)?;
    if !is_task(l) || expect.trim().is_empty() || l[6..].trim_end() != expect.trim_end() { return Err(stale()); }
    Ok(())
}

/// Tick or untick a task. Ticking off a repeating task (`@every(…)`) keeps it open and moves its due
/// date to the next time instead. With `to_group` the task (and its notes) then moves to the end of
/// that group, which is created when the list has none of that name ("move to Done when ticked").
/// → `{line, text, done, next?, previous?}`: where the task is now, and for a repeat the old text.
pub fn toggle_with(root: &Path, file: &str, line: usize, text: &str, to_group: Option<&str>, today: chrono::NaiveDate) -> anyhow::Result<Value> {
    rewrite(root, file, |lines| {
        check_line(lines, line, text)?;
        let l = lines[line].clone();
        let rest = l[6..].to_string();
        let open = l.starts_with("- [ ] ");
        if open {
            if let (title, due, Some(every)) = split_task(&rest) {
                let from = due.and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
                if let Some(next) = next_occurrence(from, every, today) {
                    let next = next.format("%Y-%m-%d").to_string();
                    let new_text = compose(title, Some(every), Some(&next));
                    lines[line] = format!("- [ ] {new_text}");
                    return Ok(json!({"line": line, "text": new_text, "done": false, "next": next, "previous": rest}));
                }
            }
        }
        lines[line] = if open { l.replacen("- [ ] ", "- [x] ", 1) } else { format!("- [ ] {rest}") };
        let at = match to_group.map(str::trim).filter(|g| !g.is_empty()) {
            Some(g) => move_block_within(lines, line, g),
            None => line,
        };
        Ok(json!({"line": at, "text": rest, "done": open}))
    })
}

pub fn toggle(root: &Path, file: &str, line: usize, text: &str) -> anyhow::Result<()> {
    toggle_with(root, file, line, text, None, chrono::Local::now().date_naive()).map(|_| ())
}

/// One line of task text: line breaks become spaces.
/// A task's text on one line, as it is written into the list.
pub(crate) fn one_line(text: &str) -> String { text.replace("\r\n", " ").replace(['\n', '\r'], " ").trim().to_string() }

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

/// An indented, non-empty line directly under a task belongs to it (a note or sub-step).
fn is_continuation(l: &str) -> bool { l.starts_with([' ', '\t']) && !l.trim().is_empty() }

/// The index just past task `i` and the indented lines that belong to it.
fn end_of_task(lines: &[String], i: usize) -> usize {
    let mut j = i + 1;
    while j < lines.len() && is_continuation(&lines[j]) { j += 1; }
    j
}

/// Deleting a task also removes its indented sub-lines, so they never end up under the task above.
/// Returns exactly the lines removed (the task first), so `restore` can put all of them back.
pub fn delete(root: &Path, file: &str, line: usize, text: &str) -> anyhow::Result<Vec<String>> {
    rewrite(root, file, |lines| {
        check_line(lines, line, text)?;
        let end = end_of_task(lines, line);
        Ok(lines.drain(line..end).collect())
    })
}

/// Undo a delete: put a removed task with its notes back at `line` (or the nearest place that does
/// not split another task from its notes, or the end when the list got shorter) → the line it is on.
pub fn restore(root: &Path, file: &str, line: usize, block: &[String]) -> anyhow::Result<usize> {
    let bad = || crate::tr::err("todo.restore_invalid", &[]);
    let (first, notes) = block.split_first().ok_or_else(bad)?;
    if !is_task(first) || first[6..].trim().is_empty() || !notes.iter().all(|l| is_continuation(l))
        || block.iter().any(|l| l.contains(['\n', '\r'])) { return Err(bad()); }
    rewrite(root, file, |lines| {
        let mut at = line.min(lines.len());
        while at < lines.len() && is_continuation(&lines[at]) { at += 1; }
        lines.splice(at..at, block.iter().cloned());
        Ok(at)
    })
}

fn heading_is(l: &str, group: &str) -> bool {
    l.strip_prefix("## ").is_some_and(|h| h.trim().to_lowercase() == group.trim().to_lowercase())
}

/// Where a task joins `group`: after the group's last task (and its notes), or straight under the
/// heading. `""` is the part above the first heading. A group the list does not have yet is added
/// at the end of the file.
fn group_insert_point(lines: &mut Vec<String>, group: &str) -> usize {
    let group = group.trim();
    let first_h2 = lines.iter().position(|l| l.starts_with("## ")).unwrap_or(lines.len());
    let (from, to) = if group.is_empty() {
        (0, first_h2)
    } else if let Some(h) = lines.iter().position(|l| heading_is(l, group)) {
        let next = lines.iter().enumerate().skip(h + 1).find(|(_, l)| l.starts_with("## ")).map(|(i, _)| i).unwrap_or(lines.len());
        (h + 1, next)
    } else {
        if lines.last().is_some_and(|l| !l.trim().is_empty()) { lines.push(String::new()); }
        lines.push(format!("## {group}"));
        return lines.len();
    };
    (from..to).rev().find(|&i| is_task(&lines[i])).map(|i| end_of_task(lines, i))
        .unwrap_or(if group.is_empty() { to } else { from })
}

/// Insert lines at `at`, keeping a blank line between them and a heading that would follow directly.
fn insert_block(lines: &mut Vec<String>, at: usize, block: Vec<String>) {
    let n = block.len();
    lines.splice(at..at, block);
    if lines.get(at + n).is_some_and(|l| l.starts_with("## ")) { lines.insert(at + n, String::new()); }
}

/// Move task `line` (with its notes) to the end of `group` in the same list → its new line.
fn move_block_within(lines: &mut Vec<String>, line: usize, group: &str) -> usize {
    let end = end_of_task(lines, line);
    let block: Vec<String> = lines.drain(line..end).collect();
    let at = group_insert_point(lines, group);
    insert_block(lines, at, block);
    at
}

/// Move a task with its notes to the end of a group, in the same list or another one → its new line.
/// Into another list, it is added there first and only then removed here, so a failure never loses it.
pub fn move_to(root: &Path, file: &str, line: usize, text: &str, to_file: &str, group: &str) -> anyhow::Result<usize> {
    if to_file == file {
        return rewrite(root, file, |lines| { check_line(lines, line, text)?; Ok(move_block_within(lines, line, group)) });
    }
    let src: Vec<String> = load(&resolve(root, file)?, file)?.lines().map(String::from).collect();
    check_line(&src, line, text)?;
    let block = src[line..end_of_task(&src, line)].to_vec();
    let at = rewrite(root, to_file, |lines| {
        if lines.is_empty() { lines.push("# Todo".into()); lines.push(String::new()); }
        let at = group_insert_point(lines, group);
        insert_block(lines, at, block.clone());
        Ok(at)
    })?;
    let removed = rewrite(root, file, |lines| {
        check_line(lines, line, text)?;
        let end = end_of_task(lines, line);
        if lines[line..end] != block[..] { return Err(crate::tr::err("stale.todo", &[])); }
        lines.drain(line..end);
        Ok(())
    });
    if let Err(e) = removed {
        // The source changed in the meantime: take the copy back out of the other list.
        let _ = rewrite(root, to_file, |lines| {
            if lines.get(at..at + block.len()) == Some(&block[..]) { lines.drain(at..at + block.len()); }
            Ok(())
        });
        return Err(e);
    }
    Ok(at)
}

/// Swap a task (with its notes) with the one above or below it in the same group → its new line.
/// At the top or bottom of its group nothing moves. With `skip_done`, ticked tasks (hidden on the
/// Todos page) are stepped over, so the task passes the next one the user can see.
pub fn reorder(root: &Path, file: &str, line: usize, text: &str, up: bool, skip_done: bool) -> anyhow::Result<usize> {
    rewrite(root, file, |lines| {
        check_line(lines, line, text)?;
        let end = end_of_task(lines, line);
        let neighbour = |range: Box<dyn Iterator<Item = usize>>, lines: &[String]| {
            for i in range { if lines[i].starts_with("## ") { return None; } if is_task(&lines[i]) && !(skip_done && is_done(&lines[i])) { return Some(i); } }
            None
        };
        if up {
            let Some(p) = neighbour(Box::new((0..line).rev()), lines) else { return Ok(line) };
            let block: Vec<String> = lines.drain(line..end).collect();
            lines.splice(p..p, block);
            Ok(p)
        } else {
            let Some(n) = neighbour(Box::new(end..lines.len()), lines) else { return Ok(line) };
            let e = end_of_task(lines, n);
            let block: Vec<String> = lines.drain(line..end).collect();
            let at = e - block.len();
            lines.splice(at..at, block);
            Ok(at)
        }
    })
}

/// Move every ticked task that is not in `group` yet to the end of it, in order, notes included
/// (the group is added when the list has none) → how many moved.
pub fn archive(root: &Path, file: &str, group: &str) -> anyhow::Result<usize> {
    let group = group.trim();
    if group.is_empty() { return Err(crate::tr::err("todo.empty", &[])); }
    rewrite(root, file, |lines| {
        let (mut ranges, mut inside, mut i) = (vec![], false, 0);
        while i < lines.len() {
            if lines[i].starts_with("## ") { inside = heading_is(&lines[i], group); i += 1; continue; }
            if is_done(&lines[i]) && !inside { let e = end_of_task(lines, i); ranges.push(i..e); i = e; continue; }
            i += 1;
        }
        let n = ranges.len();
        if n == 0 { return Ok(0); }
        let mut moved: Vec<String> = vec![];
        for r in ranges.into_iter().rev() { let mut b: Vec<String> = lines.drain(r).collect(); b.append(&mut moved); moved = b; }
        let at = group_insert_point(lines, group);
        insert_block(lines, at, moved);
        Ok(n)
    })
}

/// New items go at the end of the top group (or the end of the file) → the line it was added on.
pub fn add(root: &Path, file: &str, text: &str) -> anyhow::Result<usize> { add_to(root, file, None, text) }

/// Add a task; with `group`, at the end of that group (added when the list has none of that name).
pub fn add_to(root: &Path, file: &str, group: Option<&str>, text: &str) -> anyhow::Result<usize> {
    let text = one_line(text);
    if text.is_empty() { return Err(crate::tr::err("todo.empty", &[])); }
    rewrite(root, file, |lines| {
        let item = format!("- [ ] {text}");
        if lines.is_empty() { lines.push("# Todo".into()); lines.push(String::new()); }
        if let Some(g) = group.map(str::trim).filter(|g| !g.is_empty()) {
            let at = group_insert_point(lines, g);
            insert_block(lines, at, vec![item]);
            return Ok(at);
        }
        let first_h2 = lines.iter().position(|l| l.starts_with("## "));
        let insert_at = match first_h2 {
            Some(h) => {
                let next_h2 = lines.iter().enumerate().skip(h + 1).find(|(_, l)| l.starts_with("## ")).map(|(i, _)| i).unwrap_or(lines.len());
                let last_item = (h + 1..next_h2).rev().find(|&i| lines[i].starts_with("- [")).map(|i| end_of_task(lines, i));
                last_item.unwrap_or(h + 1)
            }
            None => {
                let last = lines.iter().rposition(|l| l.starts_with("- [")).map(|i| end_of_task(lines, i));
                last.unwrap_or(lines.len())
            }
        };
        lines.insert(insert_at, item);
        Ok(insert_at)
    })
}

// ---------- quick capture ----------

const WEEKDAYS_EN: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
const WEEKDAYS_AR: [&[&str]; 7] = [&["الأحد", "الاحد"], &["الإثنين", "الاثنين"], &["الثلاثاء"], &["الأربعاء", "الاربعاء"],
    &["الخميس"], &["الجمعة"], &["السبت"]];

/// The working week from the library's own rules: the `Week:` line under `## About this library`
/// ("Week: Sunday–Thursday; weekly review on Thursday", or the Arabic «الأسبوع: الأحد–الخميس…»)
/// → `(first day, working days)`, days counted from Sunday = 0.
pub fn library_week(root: &Path) -> Option<(u32, Vec<u32>)> {
    let rules = std::fs::read_to_string(root.join("CLAUDE.md")).ok()?;
    let line = rules.lines().find(|l| {
        let l = l.trim_start_matches([' ', '-', '*', '\t']).to_lowercase();
        l.starts_with("week:") || l.starts_with("الأسبوع:") || l.starts_with("الاسبوع:") || l.starts_with("أسبوع العمل:")
    })?.to_lowercase();
    let mut found: Vec<(usize, u32)> = vec![];
    for d in 0..7u32 {
        for name in std::iter::once(WEEKDAYS_EN[d as usize]).chain(WEEKDAYS_AR[d as usize].iter().copied()) {
            found.extend(line.match_indices(name).map(|(i, _)| (i, d)));
        }
    }
    found.sort();
    let start = found.first()?.1;
    let end = found.iter().map(|&(_, d)| d).find(|&d| d != start).unwrap_or(start);
    let mut working = vec![start];
    let mut d = start;
    while d != end { d = (d + 1) % 7; working.push(d); }
    Some((start, working))
}

fn weekday_num(d: chrono::NaiveDate) -> u32 { use chrono::Datelike; d.weekday().num_days_from_sunday() }

/// The next `day` (Sunday = 0) after `today`.
fn next_weekday(today: chrono::NaiveDate, day: u32) -> chrono::NaiveDate {
    let k = (7 + day - weekday_num(today)) % 7;
    today + chrono::Duration::days(if k == 0 { 7 } else { k as i64 })
}

/// Read a date said at the end of a quick todo — "Call the bank tomorrow", "Send the draft by
/// Thursday", «ادفع الإيجار نهاية الشهر» — and take it off the text → `(title, due)`. Only the very
/// end counts, and never when nothing would be left. `week_start` (Sunday = 0) decides "next week".
pub fn quick_due(text: &str, today: chrono::NaiveDate, week_start: u32) -> (String, Option<chrono::NaiveDate>) {
    use chrono::Datelike;
    let trimmed = text.trim_end_matches(|c: char| c.is_whitespace() || ".,!،؛".contains(c));
    let day = |n: i64| today + chrono::Duration::days(n);
    let next_week = next_weekday(today, week_start % 7);
    let this_week_end = { let e = next_week - chrono::Duration::days(3); if e < today { e + chrono::Duration::days(7) } else { e } };
    let month_end = last_of_month(today.year(), today.month()).unwrap_or(today);
    let mut phrases: Vec<(String, chrono::NaiveDate)> = vec![];
    let mut put = |names: &[&str], d: chrono::NaiveDate| for n in names { phrases.push((n.to_string(), d)); };
    put(&["today", "tonight", "اليوم", "الليلة"], today);
    put(&["tomorrow", "غدا", "غداً", "غدًا", "بكرة", "بكره"], day(1));
    put(&["next week", "الأسبوع القادم", "الاسبوع القادم", "الأسبوع المقبل", "الاسبوع المقبل", "الأسبوع الجاي", "الاسبوع الجاي"], next_week);
    put(&["end of the week", "end of week", "نهاية الأسبوع", "نهاية الاسبوع", "آخر الأسبوع", "آخر الاسبوع"], this_week_end);
    put(&["end of the month", "end of month", "month end", "month-end", "نهاية الشهر", "آخر الشهر", "اخر الشهر"], month_end);
    for d in 0..7u32 {
        let when = next_weekday(today, d);
        phrases.push((WEEKDAYS_EN[d as usize].to_string(), when));
        phrases.push((format!("next {}", WEEKDAYS_EN[d as usize]), when));
        for ar in WEEKDAYS_AR[d as usize] {
            phrases.push((ar.to_string(), when));
            for after in ["القادم", "المقبل", "الجاي"] { phrases.push((format!("{ar} {after}"), when)); }
        }
    }
    phrases.sort_by_key(|(p, _)| std::cmp::Reverse(p.chars().count()));
    const LEAD: &[&str] = &["by", "on", "due", "before", "for", "until", "this", "يوم", "في", "قبل", "بحلول", "حتى", "خلال"];
    for (p, d) in phrases {
        let n = p.chars().count();
        let Some((cut, _)) = trimmed.char_indices().rev().nth(n - 1) else { continue };
        if trimmed[cut..].to_lowercase() != p { continue; }
        let before = &trimmed[..cut];
        if !(before.is_empty() || before.ends_with(char::is_whitespace)) { continue; }
        let mut title = before.trim_end();
        if let Some(i) = title.rfind(char::is_whitespace) {
            if LEAD.contains(&title[i..].trim().to_lowercase().as_str()) { title = title[..i].trim_end(); }
        } else if LEAD.contains(&title.to_lowercase().as_str()) { title = ""; }
        if title.trim().is_empty() { break; }
        return (title.to_string(), Some(d));
    }
    (text.trim().to_string(), None)
}

/// A quick note, saved as its own file in `inbox/` without the assistant → its library path.
/// The assistant (or the user) files it properly later.
pub fn inbox_note(root: &Path, text: &str, now: chrono::DateTime<chrono::Local>) -> anyhow::Result<String> {
    let text = text.trim().replace("\r\n", "\n");
    if text.is_empty() { return Err(crate::tr::err("todo.empty", &[])); }
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let title: String = first.trim_start_matches(['#', ' ']).chars().take(80).collect();
    let words: String = first.split_whitespace().take(6).collect::<Vec<_>>().join("-").to_lowercase()
        .chars().filter(|c| c.is_alphanumeric() || *c == '-').take(40).collect();
    let words = words.trim_matches('-');
    let base = format!("inbox/{}-{}", now.format("%Y-%m-%d-%H%M"), if words.is_empty() { "note" } else { words });
    let quoted = title.replace('\\', "\\\\").replace('"', "\\\"");
    let content = format!("---\ntitle: \"{quoted}\"\ncaptured: {}\nsource: quick capture\n---\n\n{text}\n", now.format("%Y-%m-%dT%H:%M:%S%:z"));
    for n in 1..100 {
        let rel = if n == 1 { format!("{base}.md") } else { format!("{base}-{n}.md") };
        if resolve(root, &rel)?.exists() { continue; }
        crate::library::create(root, &rel, &content)?;
        return Ok(rel);
    }
    Err(crate::tr::err("exists", &[("path", &base)]))
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

/// Calendar export choices: a reminder at this time of day on the due day (`None` = no reminder).
#[derive(Clone, Copy, Debug, Default)]
pub struct CalOpts { pub alarm: Option<(u32, u32)> }

/// What earlier exports put in the user's calendar, so the next one can take back what is no longer
/// open (ticked off, deleted, renamed): `uid → {date, summary, gone?}`. Kept in the app's own folder.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Exported { pub date: String, pub summary: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub gone: Option<String> }
pub type CalRecord = std::collections::BTreeMap<String, Exported>;

/// How long a cancelled event is still sent after its task stopped being open.
const CANCEL_DAYS: i64 = 60;

fn rrule(every: &str) -> Option<&'static str> {
    Some(match every {
        "day" => "FREQ=DAILY", "week" => "FREQ=WEEKLY", "2weeks" => "FREQ=WEEKLY;INTERVAL=2",
        "month" => "FREQ=MONTHLY", "quarter" => "FREQ=MONTHLY;INTERVAL=3", "year" => "FREQ=YEARLY",
        "month-end" => "FREQ=MONTHLY;BYMONTHDAY=-1", _ => return None,
    })
}

/// An iCalendar file with one all-day event per open task that has a due date (a repeating task
/// repeats in the calendar too). Each event's UID is derived from the list and the task's title, so
/// importing the file again updates the same events rather than adding duplicates — and moving a
/// due date moves the event. Events from an earlier export whose task is no longer open come back
/// as cancelled, so a re-import clears them; `record` is updated to match.
pub fn calendar(root: &Path, now: chrono::DateTime<chrono::Utc>, opts: CalOpts, record: &mut CalRecord) -> String {
    use sha2::{Digest, Sha256};
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let today = now.date_naive();
    // Later exports must win over earlier ones in the calendar app: minutes since 2020.
    let sequence = ((now.timestamp() - 1_577_836_800) / 60).max(0);
    let mut lines: Vec<String> = vec!["BEGIN:VCALENDAR".into(), "VERSION:2.0".into(), "PRODID:-//Thoughts//Todos//EN".into(),
        "CALSCALE:GREGORIAN".into(), "METHOD:PUBLISH".into(), "X-WR-CALNAME:Thoughts".into()];
    let event = |lines: &mut Vec<String>, uid: &str, day: chrono::NaiveDate, summary: &str| {
        lines.push("BEGIN:VEVENT".into());
        lines.push(format!("UID:{uid}@thoughts.app"));
        lines.push(format!("DTSTAMP:{stamp}"));
        lines.push(format!("SEQUENCE:{sequence}"));
        lines.push(format!("DTSTART;VALUE=DATE:{}", day.format("%Y%m%d")));
        lines.push(format!("DTEND;VALUE=DATE:{}", day.succ_opt().unwrap_or(day).format("%Y%m%d")));
        lines.push(format!("SUMMARY:{}", ics_escape(summary)));
    };
    let mut seen = std::collections::HashSet::new();
    for list in list(root) {
        let file = list["file"].as_str().unwrap_or_default().to_string();
        for item in list["items"].as_array().into_iter().flatten() {
            if item["done"] == true { continue; }
            let (Some(due), Some(title)) = (item["due"].as_str(), item["title"].as_str()) else { continue };
            let Ok(day) = chrono::NaiveDate::parse_from_str(due, "%Y-%m-%d") else { continue };
            if day.succ_opt().is_none() { continue; }
            let mut uid = format!("{:x}", Sha256::digest(format!("{file}\n{title}").as_bytes()))[..32].to_string();
            // Two identical tasks in one list still need different ids.
            let mut n = 1;
            while !seen.insert(uid.clone()) { n += 1; uid = format!("{:x}", Sha256::digest(format!("{file}\n{title}\n{n}").as_bytes()))[..32].to_string(); }
            event(&mut lines, &uid, day, title);
            lines.push("STATUS:CONFIRMED".into());
            if let Some(r) = item["every"].as_str().and_then(rrule) { lines.push(format!("RRULE:{r}")); }
            let group = item["group"].as_str().unwrap_or_default();
            let place = match (list["idea"].as_str(), group.is_empty()) {
                (Some(idea), _) => idea.to_string(),
                (None, false) => group.to_string(),
                (None, true) => String::new(),
            };
            if !place.is_empty() { lines.push(format!("DESCRIPTION:{}", ics_escape(&place))); }
            lines.push("TRANSP:TRANSPARENT".into());
            if let Some((h, m)) = opts.alarm {
                // All-day events start at midnight: the reminder is that many hours into the day.
                lines.extend(["BEGIN:VALARM".into(), "ACTION:DISPLAY".into(), format!("DESCRIPTION:{}", ics_escape(title)),
                    format!("TRIGGER:PT{h}H{m}M"), "END:VALARM".into()]);
            }
            lines.push("END:VEVENT".into());
            record.insert(uid, Exported { date: day.format("%Y-%m-%d").to_string(), summary: title.to_string(), gone: None });
        }
    }
    let keep_from = today - chrono::Duration::days(CANCEL_DAYS);
    record.retain(|uid, e| {
        if seen.contains(uid) { return true; }
        let gone = *e.gone.get_or_insert_with(|| today.format("%Y-%m-%d").to_string()) >= keep_from.format("%Y-%m-%d").to_string();
        if gone {
            if let Ok(day) = chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") {
                event(&mut lines, uid, day, &e.summary);
                lines.push("STATUS:CANCELLED".into());
                lines.push("TRANSP:TRANSPARENT".into());
                lines.push("END:VEVENT".into());
            }
        }
        gone
    });
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
        let ics = calendar(&d, now, CalOpts::default(), &mut CalRecord::new());
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
        assert_eq!(uids(&ics), uids(&calendar(&d, now + chrono::Duration::days(3), CalOpts::default(), &mut CalRecord::new())));
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
    fn the_general_list_is_the_one_that_exists() {
        let d = std::env::temp_dir().join(format!("todo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        // Nothing yet: a plain library gets a root list, a starter library (its rules name tasks/TODO.md) the tasks one.
        assert_eq!(general(&d), "TODO.md");
        std::fs::write(d.join("CLAUDE.md"), "General list: `tasks/TODO.md` (or root `TODO.md` if that is what exists).").unwrap();
        assert_eq!(general(&d), "tasks/TODO.md");
        std::fs::write(d.join("TODO.md"), "- [ ] a\n").unwrap();
        assert_eq!(general(&d), "TODO.md");
        std::fs::create_dir_all(d.join("tasks")).unwrap();
        std::fs::write(d.join("tasks/TODO.md"), "- [ ] b\n").unwrap();
        assert_eq!(general(&d), "tasks/TODO.md");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn indented_sub_lines_stay_with_their_task() {
        let d = std::env::temp_dir().join(format!("todo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("TODO.md"), "# Todo\n\n## Now\n- [ ] Prepare report\n  - draft\n\t- send\n\n## Later\n- [ ] z\n").unwrap();
        add(&d, "TODO.md", "new").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("TODO.md")).unwrap(), "# Todo\n\n## Now\n- [ ] Prepare report\n  - draft\n\t- send\n- [ ] new\n\n## Later\n- [ ] z\n");
        delete(&d, "TODO.md", 3, "Prepare report").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("TODO.md")).unwrap(), "# Todo\n\n## Now\n- [ ] new\n\n## Later\n- [ ] z\n");
        // Without groups too.
        std::fs::write(d.join("TODO.md"), "- [ ] a\n    note\n").unwrap();
        add(&d, "TODO.md", "b").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("TODO.md")).unwrap(), "- [ ] a\n    note\n- [ ] b\n");
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

    fn tmp(files: &[(&str, &str)]) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("todo-test-{}", uuid::Uuid::new_v4()));
        for (f, c) in files { let p = d.join(f); std::fs::create_dir_all(p.parent().unwrap()).unwrap(); std::fs::write(p, c).unwrap(); }
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn read(d: &Path, f: &str) -> String { std::fs::read_to_string(d.join(f)).unwrap() }
    fn day(s: &str) -> chrono::NaiveDate { chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap() }

    #[test]
    fn repeat_markers_and_next_dates() {
        assert_eq!(split_task("Pay VAT @every(quarter) @due(2026-12-31)"), ("Pay VAT", Some("2026-12-31"), Some("quarter")));
        // Either order, and without a date; an unknown repeat is just text.
        assert_eq!(split_task("Pay VAT @due(2026-12-31) @every(month)"), ("Pay VAT", Some("2026-12-31"), Some("month")));
        assert_eq!(split_task("ادفع الإيجار @every(month-end)"), ("ادفع الإيجار", None, Some("month-end")));
        assert_eq!(split_task("a @every(fortnight)"), ("a @every(fortnight)", None, None));
        assert_eq!(split_task("a@every(week)"), ("a@every(week)", None, None));
        assert_eq!(compose("Pay VAT", Some("month"), Some("2026-12-01")), "Pay VAT @every(month) @due(2026-12-01)");
        // Older readers that only know `@due` at the end still see the date.
        assert_eq!(split_due("Pay VAT @every(month) @due(2026-12-01)"), ("Pay VAT @every(month)", Some("2026-12-01")));
        let t = day("2026-11-12");
        for (due, every, next) in [
            ("2026-11-12", "day", "2026-11-13"), ("2026-11-12", "week", "2026-11-19"), ("2026-11-12", "2weeks", "2026-11-26"),
            ("2026-11-12", "month", "2026-12-12"), ("2026-10-31", "month", "2026-11-30"), ("2026-11-12", "quarter", "2027-02-12"),
            ("2026-11-12", "year", "2027-11-12"), ("2026-11-12", "month-end", "2026-11-30"), ("2026-11-30", "month-end", "2026-12-31"),
            // Missed for months: the next date after today, on the same weekday or day of the month.
            ("2026-01-05", "week", "2026-11-16"), ("2025-03-15", "month", "2026-11-15"), ("2026-11-20", "month", "2026-12-20"),
            // …counted from the task's own day: a short month on the way does not move it to that month's last day.
            ("2026-01-31", "month", "2026-11-30"), ("2025-08-31", "quarter", "2026-11-30"),
        ] {
            assert_eq!(next_occurrence(Some(day(due)), every, t), Some(day(next)), "{due} {every}");
        }
        assert_eq!(next_occurrence(None, "week", t), Some(day("2026-11-19")));
        let items = parse("- [ ] Pay VAT @every(month) @due(2026-12-01)\n");
        assert_eq!((items[0]["title"].as_str(), items[0]["due"].as_str(), items[0]["every"].as_str()), (Some("Pay VAT"), Some("2026-12-01"), Some("month")));
    }

    #[test]
    fn ticking_a_repeating_task_moves_it_on_and_ticking_can_move_to_done() {
        let d = tmp(&[("TODO.md", "# Todo\r\n\r\n## الآن\r\n- [ ] ادفع الإيجار @every(month) @due(2026-11-01)\r\n  - الحوالة\r\n- [ ] اتصل بالبنك\r\n\r\n## منجز\r\n- [x] قديم\r\n")]);
        let t = day("2026-11-12");
        let v = toggle_with(&d, "TODO.md", 3, "ادفع الإيجار @every(month) @due(2026-11-01)", Some("منجز"), t).unwrap();
        assert_eq!(v["next"], "2026-12-01");
        assert_eq!(v["previous"], "ادفع الإيجار @every(month) @due(2026-11-01)");
        assert_eq!(v["done"], false);
        // Still open, still in its place (a repeat never goes to Done), the Windows line endings kept.
        assert_eq!(read(&d, "TODO.md"), "# Todo\r\n\r\n## الآن\r\n- [ ] ادفع الإيجار @every(month) @due(2026-12-01)\r\n  - الحوالة\r\n- [ ] اتصل بالبنك\r\n\r\n## منجز\r\n- [x] قديم\r\n");
        // Undo is an edit back to the old text.
        edit(&d, "TODO.md", 3, v["text"].as_str().unwrap(), v["previous"].as_str().unwrap()).unwrap();
        assert!(read(&d, "TODO.md").contains("@due(2026-11-01)"));
        // A plain task ticked with a group goes there, at the end; unticking with its old group brings it back.
        let v = toggle_with(&d, "TODO.md", 5, "اتصل بالبنك", Some("منجز"), t).unwrap();
        assert_eq!((v["line"].as_u64(), v["done"].as_bool()), (Some(8), Some(true)));
        assert_eq!(read(&d, "TODO.md"), "# Todo\r\n\r\n## الآن\r\n- [ ] ادفع الإيجار @every(month) @due(2026-11-01)\r\n  - الحوالة\r\n\r\n## منجز\r\n- [x] قديم\r\n- [x] اتصل بالبنك\r\n");
        let v = toggle_with(&d, "TODO.md", 8, "اتصل بالبنك", Some("الآن"), t).unwrap();
        assert_eq!(v["line"], 5);
        assert_eq!(read(&d, "TODO.md"), "# Todo\r\n\r\n## الآن\r\n- [ ] ادفع الإيجار @every(month) @due(2026-11-01)\r\n  - الحوالة\r\n- [ ] اتصل بالبنك\r\n\r\n## منجز\r\n- [x] قديم\r\n");
        // A Done group the list does not have yet is added at the end.
        std::fs::write(d.join("TODO.md"), "- [ ] a\n- [ ] b\n").unwrap();
        toggle_with(&d, "TODO.md", 0, "a", Some("Done"), t).unwrap();
        assert_eq!(read(&d, "TODO.md"), "- [ ] b\n\n## Done\n- [x] a\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tasks_move_between_groups_and_lists_with_their_notes() {
        let d = tmp(&[("TODO.md", "# Todo\n\n## Now\n- [ ] a\n  note a\n- [ ] b\n\n## Later\n- [ ] z\n")]);
        assert_eq!(list(&d)[0]["groups"], json!(["Now", "Later"]));
        assert_eq!(move_to(&d, "TODO.md", 3, "a", "TODO.md", "later").unwrap(), 7);
        assert_eq!(read(&d, "TODO.md"), "# Todo\n\n## Now\n- [ ] b\n\n## Later\n- [ ] z\n- [ ] a\n  note a\n");
        // Into an empty group, and into the part above the first heading.
        std::fs::write(d.join("TODO.md"), "# Todo\n\n## Now\n- [ ] a\n\n## Next\n\n## Later\n").unwrap();
        move_to(&d, "TODO.md", 3, "a", "TODO.md", "Next").unwrap();
        assert_eq!(read(&d, "TODO.md"), "# Todo\n\n## Now\n\n## Next\n- [ ] a\n\n## Later\n");
        move_to(&d, "TODO.md", 5, "a", "TODO.md", "").unwrap();
        assert_eq!(read(&d, "TODO.md"), "# Todo\n\n- [ ] a\n\n## Now\n\n## Next\n\n## Later\n");
        // Another list, created with its first task; a stale task is never moved.
        assert!(move_to(&d, "TODO.md", 2, "b", "clients/acme/todo.md", "").is_err());
        assert!(!d.join("clients/acme/todo.md").exists());
        std::fs::create_dir_all(d.join("clients/acme")).unwrap();
        move_to(&d, "TODO.md", 2, "a", "clients/acme/todo.md", "").unwrap();
        assert_eq!(read(&d, "clients/acme/todo.md"), "# Todo\n\n- [ ] a\n");
        assert_eq!(read(&d, "TODO.md"), "# Todo\n\n\n## Now\n\n## Next\n\n## Later\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn reorder_stays_inside_the_group() {
        let d = tmp(&[("TODO.md", "## Now\r\n- [ ] a\r\n  - a1\r\n- [ ] ب\r\n\r\n## Later\r\n- [ ] z\r\n")]);
        // Down: a (with its note) goes below ب; again: already last in its group, nothing moves.
        assert_eq!(reorder(&d, "TODO.md", 1, "a", false, false).unwrap(), 2);
        assert_eq!(read(&d, "TODO.md"), "## Now\r\n- [ ] ب\r\n- [ ] a\r\n  - a1\r\n\r\n## Later\r\n- [ ] z\r\n");
        assert_eq!(reorder(&d, "TODO.md", 2, "a", false, false).unwrap(), 2);
        assert_eq!(reorder(&d, "TODO.md", 6, "z", true, false).unwrap(), 6);
        assert_eq!(reorder(&d, "TODO.md", 2, "a", true, false).unwrap(), 1);
        assert_eq!(read(&d, "TODO.md"), "## Now\r\n- [ ] a\r\n  - a1\r\n- [ ] ب\r\n\r\n## Later\r\n- [ ] z\r\n");
        assert!(reorder(&d, "TODO.md", 1, "b", true, false).is_err());
        // Hidden ticked tasks are stepped over when asked.
        std::fs::write(d.join("TODO.md"), "- [ ] a\n- [ ] b\n- [x] done\n- [ ] c\n").unwrap();
        assert_eq!(reorder(&d, "TODO.md", 3, "c", true, true).unwrap(), 1);
        assert_eq!(read(&d, "TODO.md"), "- [ ] a\n- [ ] c\n- [ ] b\n- [x] done\n");
        assert_eq!(reorder(&d, "TODO.md", 2, "b", false, true).unwrap(), 2);
        assert_eq!(reorder(&d, "TODO.md", 0, "a", false, true).unwrap(), 1);
        assert_eq!(read(&d, "TODO.md"), "- [ ] c\n- [ ] a\n- [ ] b\n- [x] done\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn finished_tasks_are_archived_into_done() {
        let d = tmp(&[("TODO.md", "# Todo\n\n## Now\n- [x] a\n  - a1\n- [ ] b\n- [X] ج\n\n## Done\n- [x] old\n")]);
        assert_eq!(archive(&d, "TODO.md", "done").unwrap(), 2);
        assert_eq!(read(&d, "TODO.md"), "# Todo\n\n## Now\n- [ ] b\n\n## Done\n- [x] old\n- [x] a\n  - a1\n- [X] ج\n");
        assert_eq!(archive(&d, "TODO.md", "Done").unwrap(), 0);
        assert!(archive(&d, "TODO.md", " ").is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn delete_and_restore_round_trip() {
        let original = "# Todo\r\n\r\n## الآن\r\n- [ ] راجع العقد\r\n  - البند ٣\r\n\t- البند ٧\r\n- [ ] b\r\n";
        let d = tmp(&[("TODO.md", original)]);
        let removed = delete(&d, "TODO.md", 3, "راجع العقد").unwrap();
        assert_eq!(removed, ["- [ ] راجع العقد", "  - البند ٣", "\t- البند ٧"]);
        assert_eq!(restore(&d, "TODO.md", 3, &removed).unwrap(), 3);
        assert_eq!(read(&d, "TODO.md"), original);
        // The list changed meanwhile: it never lands between another task and its notes, or past the end.
        std::fs::write(d.join("TODO.md"), "- [ ] x\n  x note\n").unwrap();
        assert_eq!(restore(&d, "TODO.md", 1, &removed).unwrap(), 2);
        assert_eq!(restore(&d, "TODO.md", 99, &["- [ ] y".to_string()]).unwrap(), 5);
        assert_eq!(read(&d, "TODO.md"), "- [ ] x\n  x note\n- [ ] راجع العقد\n  - البند ٣\n\t- البند ٧\n- [ ] y\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn adding_to_a_named_group() {
        let d = tmp(&[("TODO.md", "# Todo\n\n## Now\n- [ ] a\n\n## Later\n- [ ] z\n")]);
        assert_eq!(add_to(&d, "TODO.md", Some("Later"), "y").unwrap(), 7);
        assert_eq!(add_to(&d, "TODO.md", Some("Someday"), "q").unwrap(), 10);
        assert_eq!(add(&d, "TODO.md", "b").unwrap(), 4);
        assert_eq!(read(&d, "TODO.md"), "# Todo\n\n## Now\n- [ ] a\n- [ ] b\n\n## Later\n- [ ] z\n- [ ] y\n\n## Someday\n- [ ] q\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn dates_said_at_the_end_of_a_quick_todo() {
        let t = day("2026-11-12"); // a Thursday
        let q = |s: &str, ws: u32| { let (a, b) = quick_due(s, t, ws); (a, b.map(|d| d.format("%Y-%m-%d").to_string())) };
        let some = |a: &str, b: &str| (a.to_string(), Some(b.to_string()));
        assert_eq!(q("Call the bank tomorrow", 1), some("Call the bank", "2026-11-13"));
        assert_eq!(q("Call the bank Tomorrow.", 1), some("Call the bank", "2026-11-13"));
        assert_eq!(q("Send the draft by Sunday", 1), some("Send the draft", "2026-11-15"));
        assert_eq!(q("Prepare slides for next Monday", 1), some("Prepare slides", "2026-11-16"));
        assert_eq!(q("Review on Thursday", 1), some("Review", "2026-11-19"));
        assert_eq!(q("Plan next week", 1), some("Plan", "2026-11-16"));
        assert_eq!(q("Plan next week", 0), some("Plan", "2026-11-15"));
        assert_eq!(q("Report end of the week", 0), some("Report", "2026-11-12"));
        assert_eq!(q("Report end of week", 1), some("Report", "2026-11-13"));
        assert_eq!(q("Close the books end of month", 1), some("Close the books", "2026-11-30"));
        assert_eq!(q("اتصل بالبنك غداً", 0), some("اتصل بالبنك", "2026-11-13"));
        assert_eq!(q("اتصل بالبنك بكرة", 0), some("اتصل بالبنك", "2026-11-13"));
        assert_eq!(q("أرسل المسودة يوم الأحد", 0), some("أرسل المسودة", "2026-11-15"));
        assert_eq!(q("أرسل المسودة قبل الخميس القادم", 0), some("أرسل المسودة", "2026-11-19"));
        assert_eq!(q("ادفع الإيجار نهاية الشهر", 0), some("ادفع الإيجار", "2026-11-30"));
        assert_eq!(q("جهّز التقرير الأسبوع القادم", 0), some("جهّز التقرير", "2026-11-15"));
        // Nothing left, glued to a word, or not at the end: unchanged.
        for s in ["tomorrow", "by Friday", "Read yesterday's notes", "Buy groceries", "Fridays are busy", "غداً", "todayish"] {
            assert_eq!(q(s, 1), (s.to_string(), None), "{s}");
        }
    }

    #[test]
    fn the_week_comes_from_the_library_rules() {
        let d = tmp(&[("CLAUDE.md", "# Rules\n\n## About this library\n- Week: Sunday–Thursday; weekly review on Thursday\n")]);
        assert_eq!(library_week(&d), Some((0, vec![0, 1, 2, 3, 4])));
        std::fs::write(d.join("CLAUDE.md"), "## About this library\r\n- الأسبوع: من الأحد إلى الخميس؛ المراجعة الأسبوعية يوم الخميس\r\n").unwrap();
        assert_eq!(library_week(&d), Some((0, vec![0, 1, 2, 3, 4])));
        std::fs::write(d.join("CLAUDE.md"), "- Week: Monday–Friday; weekly review on Friday\n").unwrap();
        assert_eq!(library_week(&d), Some((1, vec![1, 2, 3, 4, 5])));
        std::fs::write(d.join("CLAUDE.md"), "The week starts whenever.\n").unwrap();
        assert_eq!(library_week(&d), None);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn quick_notes_go_to_the_inbox() {
        let d = tmp(&[]);
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-12T09:30:00+03:00").unwrap().with_timezone(&chrono::Local);
        let a = inbox_note(&d, "Idea: a \"tiny\" shop\r\nwith details", now).unwrap();
        let b = inbox_note(&d, "Idea: a \"tiny\" shop", now).unwrap();
        assert!(a.starts_with("inbox/2026-11-12-") && a.ends_with("-idea-a-tiny-shop.md"), "{a}");
        assert_eq!(b, a.replace(".md", "-2.md"));
        let text = read(&d, &a);
        assert!(text.starts_with("---\ntitle: \"Idea: a \\\"tiny\\\" shop\"\ncaptured: 2026-11-12T"), "{text}");
        assert!(text.ends_with("---\n\nIdea: a \"tiny\" shop\nwith details\n"));
        assert!(inbox_note(&d, " \n ", now).is_err());
        assert_eq!(inbox_note(&d, "!!!", now).unwrap().rsplit('-').next(), Some("note.md"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_calendar_reminds_repeats_and_takes_back_what_is_gone() {
        let d = tmp(&[("TODO.md", "- [ ] Pay VAT @every(quarter) @due(2026-12-31)\n- [ ] Call the bank @due(2026-11-20)\n")]);
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-10T08:00:00Z").unwrap().with_timezone(&chrono::Utc);
        let mut record = CalRecord::new();
        let ics = calendar(&d, now, CalOpts { alarm: Some((9, 0)) }, &mut record).replace("\r\n ", "");
        assert_eq!(ics.matches("BEGIN:VALARM").count(), 2);
        assert!(ics.contains("TRIGGER:PT9H0M\r\n") && ics.contains("RRULE:FREQ=MONTHLY;INTERVAL=3\r\n"));
        assert!(ics.contains("SUMMARY:Pay VAT\r\n") && !ics.contains("STATUS:CANCELLED"));
        assert_eq!(record.len(), 2);
        // The bank call is ticked off: the next export cancels its event, with its old date and a later sequence.
        toggle(&d, "TODO.md", 1, "Call the bank @due(2026-11-20)").unwrap();
        let later = now + chrono::Duration::days(1);
        let ics2 = calendar(&d, later, CalOpts::default(), &mut record).replace("\r\n ", "");
        assert!(!ics2.contains("VALARM"));
        assert_eq!(ics2.matches("BEGIN:VEVENT").count(), 2);
        let cancelled = ics2.split("BEGIN:VEVENT").find(|e| e.contains("STATUS:CANCELLED")).unwrap();
        assert!(cancelled.contains("SUMMARY:Call the bank\r\n") && cancelled.contains("DTSTART;VALUE=DATE:20261120"));
        let seq = |s: &str| s.lines().find_map(|l| l.strip_prefix("SEQUENCE:")).unwrap().parse::<i64>().unwrap();
        assert!(seq(&ics2) > seq(&ics));
        assert_eq!(record.values().filter(|e| e.gone.is_some()).count(), 1);
        // After a while it is no longer sent at all.
        let ics3 = calendar(&d, later + chrono::Duration::days(61), CalOpts::default(), &mut record);
        assert!(!ics3.contains("CANCELLED"));
        assert_eq!(record.len(), 1);
        std::fs::remove_dir_all(d).unwrap();
    }

}
