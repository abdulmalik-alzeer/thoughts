//! What changed between two points of a library's history, in a form a person can read: one entry
//! per document — new, changed, moved or removed — with the lines that changed and a few lines
//! around them. Used to review a Wrap or a Reorganize before it is kept, and to look back at what a
//! saved Wrap changed. Never raw diff text: names are shown as they are (Arabic included), and
//! nothing a person's own git settings say (colours, prefixes, external programs) changes the result.

use crate::state::AppState;
use serde_json::{json, Value};
use std::path::Path;

/// Documents whose changed lines are included; any further ones are listed by name only.
pub const DETAILED: usize = 80;
/// Lines shown for one document; the rest is counted ("and 40 more lines").
pub const LINES: usize = 400;
/// Lines shown of a document that was removed: enough to recognise it.
const REMOVED_LINES: usize = 30;
/// One very long line (a pasted table, minified text) is cut here.
const LINE_CHARS: usize = 2000;
/// The longest list of documents named in one command for their changed lines (bytes): far below
/// what any system allows on a command line.
const NAMES_MAX: usize = 24_000;

/// One document that differs between the two points.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: String,
    /// Where it was before, for a document that was moved or renamed.
    pub from: Option<String>,
    /// `added` | `edited` | `moved` | `deleted`.
    pub change: &'static str,
}

/// Parse `git diff --name-status -z -M` output.
pub fn parse_name_status(out: &str) -> Vec<Entry> {
    let mut it = out.split('\0');
    let mut entries = vec![];
    while let Some(status) = it.next() {
        let status = status.trim();
        let Some(letter) = status.chars().next() else { continue };
        let entry = match letter {
            // Renamed (R) or copied (C): the old name, then the new one.
            'R' | 'C' => {
                let (Some(from), Some(to)) = (it.next(), it.next()) else { break };
                if letter == 'C' { Entry { path: to.into(), from: None, change: "added" } }
                else { Entry { path: to.into(), from: Some(from.into()), change: "moved" } }
            }
            _ => {
                let Some(path) = it.next() else { break };
                Entry { path: path.into(), from: None, change: match letter { 'A' => "added", 'D' => "deleted", _ => "edited" } }
            }
        };
        if !entry.path.is_empty() { entries.push(entry); }
    }
    entries
}

/// The changed lines of one document, from its part of a unified diff.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Detail {
    pub binary: bool,
    pub added: usize,
    pub removed: usize,
    /// `(line in the new version, [(kind, text)])`, kind `+`, `-` or ` `.
    pub hunks: Vec<(usize, Vec<(char, String)>)>,
    /// Lines that were left out to keep the page readable.
    pub more: usize,
}

fn clip(s: &str) -> String {
    let s = s.trim_end_matches('\r');
    if s.chars().count() <= LINE_CHARS { return s.to_string(); }
    let mut out: String = s.chars().take(LINE_CHARS).collect();
    out.push('…');
    out
}

/// Parse one document's part of `git diff` output (from its `diff --git` line to the next one),
/// keeping at most `max` lines.
pub fn parse_chunk(chunk: &str, max: usize) -> Detail {
    let mut d = Detail::default();
    let mut shown = 0;
    let mut in_hunk = false;
    for line in chunk.lines() {
        if let Some(head) = line.strip_prefix("@@ ") {
            in_hunk = true;
            // `@@ -12,3 +14,5 @@ heading`: the new version's first line number.
            let at = head.split_whitespace().find_map(|p| p.strip_prefix('+')).and_then(|p| p.split(',').next()?.parse().ok()).unwrap_or(1);
            if shown < max { d.hunks.push((at, vec![])); }
            continue;
        }
        if !in_hunk {
            if line.starts_with("Binary files ") || line == "GIT binary patch" { d.binary = true; }
            continue;
        }
        let (kind, text) = match line.chars().next() {
            Some('+') => ('+', &line[1..]),
            Some('-') => ('-', &line[1..]),
            Some(' ') => (' ', &line[1..]),
            // "\ No newline at end of file"
            Some('\\') => continue,
            // An empty context line (git's `diff.suppressBlankEmpty`).
            None => (' ', ""),
            Some(_) => { in_hunk = false; continue; }
        };
        match kind { '+' => d.added += 1, '-' => d.removed += 1, _ => {} }
        if shown < max {
            if let Some((_, lines)) = d.hunks.last_mut() { lines.push((kind, clip(text))); shown += 1; }
        } else if kind != ' ' {
            d.more += 1;
        }
    }
    d.hunks.retain(|(_, l)| !l.is_empty());
    d
}

/// Split a whole `git diff` into each document's part and give each to the entry it belongs to
/// (matched on the name, not on the order: a document whose name git had to quote simply gets no
/// detail). Returns one `Option<Detail>` per entry.
pub fn details(entries: &[Entry], patch: &str, detailed: usize) -> Vec<Option<Detail>> {
    let mut out: Vec<Option<Detail>> = vec![None; entries.len()];
    let mut chunks: Vec<&str> = vec![];
    let mut start: Option<usize> = None;
    let mut pos = 0;
    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            if let Some(s) = start { chunks.push(&patch[s..pos]); }
            start = Some(pos);
        }
        pos += line.len();
    }
    if let Some(s) = start { chunks.push(&patch[s..]); }
    let mut next = 0;
    let head_of = |e: &Entry| format!("diff --git a/{} b/{}", e.from.as_deref().unwrap_or(&e.path), e.path);
    for chunk in chunks {
        let head = chunk.lines().next().unwrap_or("");
        let Some(i) = (next..entries.len()).find(|&i| head == head_of(&entries[i])) else { continue };
        // A document whose kind changed (a file became a link) comes as two parts: keep the first.
        if out[i].is_none() && i < detailed {
            let max = if entries[i].change == "deleted" { REMOVED_LINES } else { LINES };
            out[i] = Some(parse_chunk(chunk, max));
        }
        next = i;
    }
    out
}

/// Every document that differs between `from` and `to` (commits or trees) in the repository of
/// `dir`, for a person to read: `{files: [{path, from, change, binary, added, removed, hunks:
/// [{line, lines: [[kind, text]]}], more, detail}], total, counts: {added, edited, moved, deleted}}`.
/// The first `DETAILED` documents carry their changed lines (`detail: true`).
pub async fn between(state: &AppState, dir: &Path, from: &str, to: &str) -> anyhow::Result<Value> {
    if !crate::git::valid_hash(from) || !crate::git::valid_hash(to) { anyhow::bail!("invalid snapshot"); }
    // Settings of the person's own git that would change what comes out are overruled here.
    const PLAIN: &[&str] = &["--no-color", "--no-textconv", "--no-relative", "--src-prefix=a/", "--dst-prefix=b/", "-M"];
    let mut args = vec!["diff", "--name-status", "-z"];
    args.extend_from_slice(PLAIN);
    args.extend_from_slice(&[from, to]);
    let entries = parse_name_status(&crate::git::git(state, dir, &args).await?);
    // How large each side is, looked at before anything is read: a document too large to show line
    // by line (`diffs::MAX_BYTES`, as for History) is listed by name and its text never read.
    let (before, after) = if entries.is_empty() { Default::default() } else { (sizes(state, dir, from).await, sizes(state, dir, to).await) };
    let large: Vec<bool> = entries.iter().map(|e| {
        let was = e.from.as_deref().unwrap_or(&e.path);
        (e.change != "added" && before.get(was).is_some_and(|n| *n > crate::diffs::MAX_BYTES))
            || (e.change != "deleted" && after.get(&e.path).is_some_and(|n| *n > crate::diffs::MAX_BYTES))
    }).collect();
    // Only the documents shown line by line are read, each by its exact name (both names for a move).
    let mut names: Vec<String> = vec![];
    let mut room = NAMES_MAX;
    for (e, big) in entries.iter().zip(&large).take(DETAILED) {
        if *big { continue; }
        let these: Vec<String> = std::iter::once(&e.path).chain(e.from.as_ref()).map(|n| format!(":(literal){n}")).collect();
        let len: usize = these.iter().map(|n| n.len() + 1).sum();
        if len > room { break; }
        room -= len;
        names.extend(these);
    }
    let detail = if names.is_empty() { vec![None; entries.len()] } else {
        let mut args = vec!["diff", "-U3"];
        args.extend_from_slice(PLAIN);
        args.extend_from_slice(&[from, to, "--"]);
        args.extend(names.iter().map(String::as_str));
        match crate::git::git(state, dir, &args).await {
            Ok(patch) => details(&entries, &patch, DETAILED),
            Err(e) => { crate::log::warn(format!("changes: could not read the changed lines ({e}); listing names only")); vec![None; entries.len()] }
        }
    };
    let mut counts = json!({"added": 0, "edited": 0, "moved": 0, "deleted": 0});
    let files: Vec<Value> = entries.iter().zip(detail).zip(&large).map(|((e, d), big)| {
        counts[e.change] = json!(counts[e.change].as_u64().unwrap_or(0) + 1);
        let d = d.as_ref();
        json!({
            "path": e.path, "from": e.from, "change": e.change, "detail": d.is_some(), "tooLarge": big,
            "binary": d.is_some_and(|d| d.binary),
            "added": d.map(|d| d.added), "removed": d.map(|d| d.removed), "more": d.map_or(0, |d| d.more),
            "hunks": d.map(|d| d.hunks.iter().map(|(line, lines)| json!({
                "line": line, "lines": lines.iter().map(|(k, t)| json!([k.to_string(), t])).collect::<Vec<_>>(),
            })).collect::<Vec<_>>()).unwrap_or_default(),
        })
    }).collect();
    Ok(json!({"files": files, "total": entries.len(), "counts": counts}))
}

/// The size of every document at `rev` (a commit or a tree), by its name, read from the history's
/// list of files (nothing of their text is read).
async fn sizes(state: &AppState, dir: &Path, rev: &str) -> std::collections::HashMap<String, u64> {
    let out = crate::git::git(state, dir, &["ls-tree", "-r", "-l", "-z", "--full-tree", rev]).await.unwrap_or_default();
    // `<mode> blob <id> <size>\t<name>`
    out.split('\0').filter_map(|e| {
        let (meta, name) = e.split_once('\t')?;
        Some((name.to_string(), meta.split_whitespace().nth(3)?.parse().ok()?))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wraps::testutil as t;

    #[test]
    fn names_are_read_exactly_as_they_are() {
        let out = "M\0README.md\0A\0ideas/متجر صغير/README.md\0D\0old.md\0R087\0notes/a b.md\0areas/apps/a b.md\0C100\0x.md\0y.md\0";
        let e = parse_name_status(out);
        assert_eq!(e, vec![
            Entry { path: "README.md".into(), from: None, change: "edited" },
            Entry { path: "ideas/متجر صغير/README.md".into(), from: None, change: "added" },
            Entry { path: "old.md".into(), from: None, change: "deleted" },
            Entry { path: "areas/apps/a b.md".into(), from: Some("notes/a b.md".into()), change: "moved" },
            Entry { path: "y.md".into(), from: None, change: "added" },
        ]);
        assert!(parse_name_status("").is_empty());
    }

    #[test]
    fn a_chunk_becomes_lines_a_person_can_read() {
        let chunk = "diff --git a/TODO.md b/TODO.md\nindex 1..2 100644\n--- a/TODO.md\n+++ b/TODO.md\n@@ -1,3 +1,4 @@ # Todo\n # Todo\n\n - [ ] call the bank\n+- [ ] order the sign\n@@ -10,2 +11,2 @@\n-old line\r\n+new line\n\\ No newline at end of file\n";
        let d = parse_chunk(chunk, 100);
        assert_eq!((d.added, d.removed, d.binary, d.more), (2, 1, false, 0));
        assert_eq!(d.hunks[0], (1, vec![(' ', "# Todo".into()), (' ', "".into()), (' ', "- [ ] call the bank".into()), ('+', "- [ ] order the sign".into())]));
        assert_eq!(d.hunks[1], (11, vec![('-', "old line".into()), ('+', "new line".into())]));
        // Too long to show whole: the rest is counted, never silently dropped.
        let d = parse_chunk(chunk, 3);
        assert_eq!((d.hunks.len(), d.hunks[0].1.len(), d.added, d.more), (1, 3, 2, 3));
        let bin = parse_chunk("diff --git a/p.png b/p.png\nnew file mode 100644\nindex 0..1\nBinary files /dev/null and b/p.png differ\n", 100);
        assert!(bin.binary && bin.hunks.is_empty());
    }

    #[test]
    fn each_part_of_a_diff_goes_to_its_own_document() {
        let entries = parse_name_status("M\0a.md\0M\0\"odd\".md\0M\0b c.md\0");
        let patch = "diff --git a/a.md b/a.md\n@@ -1 +1 @@\n-x\n+y\ndiff --git \"a/\\\"odd\\\".md\" \"b/\\\"odd\\\".md\"\n@@ -1 +1 @@\n-1\n+2\ndiff --git a/b c.md b/b c.md\n@@ -1 +1,2 @@\n z\n+diff --git a/x b/x\n";
        let d = details(&entries, patch, 10);
        assert_eq!(d[0].as_ref().unwrap().hunks[0].1, vec![('-', "x".into()), ('+', "y".into())]);
        // git quoted that name: no detail rather than the wrong one.
        assert!(d[1].is_none());
        // A line that merely looks like the start of a new document stays a line of this one.
        assert_eq!(d[2].as_ref().unwrap().hunks[0].1, vec![(' ', "z".into()), ('+', "diff --git a/x b/x".into())]);
        // Only the first documents carry their lines.
        assert!(details(&entries, patch, 1)[2].is_none());
    }

    #[tokio::test]
    async fn the_changes_between_two_points_are_listed_per_document() {
        let (st, lib) = t::library().await;
        let before = t::sh(&lib, &["rev-parse", "HEAD"]);
        t::write(&lib, "TODO.md", "# Todo\n\n- [ ] call the bank\n- [ ] order the sign\n");
        t::write(&lib, "ideas/متجر/README.md", "---\ntitle: متجر\n---\nفكرة جديدة\n");
        std::fs::remove_file(lib.join("README.md")).unwrap();
        std::fs::create_dir_all(lib.join("areas/shop")).unwrap();
        std::fs::rename(lib.join("ideas/shop/README.md"), lib.join("areas/shop/README.md")).unwrap();
        std::fs::write(lib.join("logo.png"), [0u8, 159, 146, 150, 0, 1, 2]).unwrap();
        // Settings in the person's own git that would change the output are overruled.
        t::sh(&lib, &["config", "diff.noprefix", "true"]);
        t::sh(&lib, &["config", "color.ui", "always"]);
        t::sh(&lib, &["add", "-A"]);
        t::sh(&lib, &["commit", "-q", "-m", "second"]);
        let after = t::sh(&lib, &["rev-parse", "HEAD"]);
        let v = between(&st, &lib, &before, &after).await.unwrap();
        assert_eq!(v["total"], 5);
        assert_eq!(v["counts"], json!({"added": 2, "edited": 1, "moved": 1, "deleted": 1}));
        let by = |p: &str| v["files"].as_array().unwrap().iter().find(|f| f["path"] == p).cloned().unwrap_or_else(|| panic!("{p}: {v}"));
        let todo = by("TODO.md");
        assert_eq!((todo["change"].as_str(), todo["added"].as_u64(), todo["removed"].as_u64()), (Some("edited"), Some(1), Some(0)));
        assert!(todo["hunks"][0]["lines"].as_array().unwrap().contains(&json!(["+", "- [ ] order the sign"])));
        let new = by("ideas/متجر/README.md");
        assert_eq!((new["change"].as_str(), new["added"].as_u64()), (Some("added"), Some(4)));
        assert_eq!(new["hunks"][0]["lines"][3], json!(["+", "فكرة جديدة"]));
        let moved = by("areas/shop/README.md");
        assert_eq!((moved["change"].as_str(), moved["from"].as_str()), (Some("moved"), Some("ideas/shop/README.md")));
        assert_eq!(by("README.md")["change"], "deleted");
        assert_eq!(by("logo.png")["binary"], true);
        // From the very start (an empty library) everything is new.
        let first = between(&st, &lib, crate::wraps::EMPTY_TREE, &before).await.unwrap();
        assert_eq!(first["counts"]["added"], 3);
        assert!(between(&st, &lib, "not a point", &after).await.is_err());
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn a_document_too_large_to_show_is_named_and_never_read() {
        let (st, lib) = t::library().await;
        let big = "a line of an exported table, repeated many times over\n".repeat(60_000);
        assert!(big.len() as u64 > crate::diffs::MAX_BYTES);
        t::write(&lib, "export.csv", &big);
        t::sh(&lib, &["add", "-A"]);
        t::sh(&lib, &["commit", "-q", "-m", "an export"]);
        let before = t::sh(&lib, &["rev-parse", "HEAD"]);
        t::write(&lib, "export.csv", &big.replace("exported", "imported"));
        t::write(&lib, "TODO.md", "# Todo\n\n- [ ] call the bank\n- [ ] order the sign\n");
        t::sh(&lib, &["add", "-A"]);
        t::sh(&lib, &["commit", "-q", "-m", "both"]);
        let after = t::sh(&lib, &["rev-parse", "HEAD"]);
        let v = between(&st, &lib, &before, &after).await.unwrap();
        let by = |p: &str| v["files"].as_array().unwrap().iter().find(|f| f["path"] == p).cloned().unwrap();
        let export = by("export.csv");
        assert_eq!((export["detail"].as_bool(), export["tooLarge"].as_bool(), export["hunks"].as_array().map(Vec::len)), (Some(false), Some(true), Some(0)));
        // The small document beside it is shown line by line as usual.
        let todo = by("TODO.md");
        assert_eq!((todo["detail"].as_bool(), todo["tooLarge"].as_bool(), todo["added"].as_u64()), (Some(true), Some(false), Some(1)));
        // Added whole, it is too large as well.
        let first = between(&st, &lib, crate::wraps::EMPTY_TREE, &before).await.unwrap();
        assert_eq!(first["files"].as_array().unwrap().iter().find(|f| f["path"] == "export.csv").unwrap()["tooLarge"], true);
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }
}
