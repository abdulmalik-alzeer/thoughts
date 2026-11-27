//! What changed, in a form a person can read: one entry per document — new, changed, moved or
//! removed — with the lines that were added and removed and a few lines around them. Used by
//! History ("what this change did"), by Versions (an earlier version against the text now), by a
//! sync clash (my copy against the other device's) and after a history repair (the copy kept
//! before the repair against now).
//!
//! The comparison is worked out here, line by line, from the two texts themselves: nothing a
//! person's own git settings say (colours, prefixes, outside programs) can change it, and names are
//! kept as they are (Arabic included). Each document has the shape the window's `ChangeView` reads:
//! `{path, from, change, detail, binary, added, removed, more, hunks: [{line, lines: [[kind, text]]}]}`
//! with `kind` `+` (added), `-` (removed) or ` ` (unchanged, shown around a change).

use crate::state::AppState;
use serde_json::{json, Value};
use std::path::Path;

/// Documents whose changed lines are included; any further ones are listed by name only.
pub const DETAILED: usize = 40;
/// Lines shown for one document; the rest is counted ("and 40 more lines").
pub const LINES: usize = 400;
/// Lines shown of a document that was removed: enough to recognise it.
const REMOVED_LINES: usize = 30;
/// One very long line (a pasted table, minified text) is cut here.
const LINE_CHARS: usize = 2000;
/// Unchanged lines shown before and after each change.
const CONTEXT: usize = 3;
/// A document larger than this is listed without its lines.
pub(crate) const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Past this many differing lines the two texts are not matched line by line any more: the old
/// text is shown as removed and the new one as added (the page cuts both short anyway).
const MAX_EDITS: usize = 1000;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Op { Same, Del, Add }

/// The shortest way from `a` to `b`, line by line: what stays, what goes and what comes.
fn edit_script(a: &[&str], b: &[&str]) -> Vec<Op> {
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] { pre += 1; }
    let mut suf = 0;
    while suf < a.len() - pre && suf < b.len() - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf] { suf += 1; }
    let mut ops = vec![Op::Same; pre];
    ops.extend(middle(&a[pre..a.len() - suf], &b[pre..b.len() - suf]));
    ops.extend(std::iter::repeat_n(Op::Same, suf));
    ops
}

/// Myers' algorithm on what is left once the common beginning and end are set aside.
fn middle(a: &[&str], b: &[&str]) -> Vec<Op> {
    let (n, m) = (a.len() as i64, b.len() as i64);
    if n == 0 { return vec![Op::Add; m as usize]; }
    if m == 0 { return vec![Op::Del; n as usize]; }
    let limit = ((n + m) as usize).min(MAX_EDITS) as i64;
    let off = limit + 1;
    let mut v = vec![0i64; (2 * limit + 3) as usize];
    // What `v` held before each step, for k in -(d+1)..=d+1: enough to walk back.
    let mut trace: Vec<Vec<i64>> = vec![];
    let mut end: Option<i64> = None;
    'outer: for d in 0..=limit {
        trace.push(v[(off - d - 1) as usize..=(off + d + 1) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let at = |k: i64| (k + off) as usize;
            let mut x = if k == -d || (k != d && v[at(k - 1)] < v[at(k + 1)]) { v[at(k + 1)] } else { v[at(k - 1)] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] { x += 1; y += 1; }
            v[at(k)] = x;
            if x >= n && y >= m { end = Some(d); break 'outer; }
            k += 2;
        }
    }
    let Some(dmax) = end else {
        let mut ops = vec![Op::Del; n as usize];
        ops.extend(vec![Op::Add; m as usize]);
        return ops;
    };
    let (mut x, mut y) = (n, m);
    let mut rev: Vec<Op> = vec![];
    for d in (0..=dmax).rev() {
        let saved = &trace[d as usize];
        let get = |k: i64| saved[(k + d + 1) as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && get(k - 1) < get(k + 1)) { k + 1 } else { k - 1 };
        let prev_x = if d == 0 { 0 } else { get(prev_k) };
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y { rev.push(Op::Same); x -= 1; y -= 1; }
        if d > 0 { rev.push(if x == prev_x { Op::Add } else { Op::Del }); }
        x = prev_x;
        y = prev_y;
    }
    while x > 0 && y > 0 { rev.push(Op::Same); x -= 1; y -= 1; }
    rev.reverse();
    rev
}

fn clip(s: &str) -> String {
    let s = s.trim_end_matches('\r');
    if s.chars().count() <= LINE_CHARS { return s.to_string(); }
    let mut out: String = s.chars().take(LINE_CHARS).collect();
    out.push('…');
    out
}

/// Does this look like something other than text (a picture, a PDF…)?
pub fn is_binary(bytes: &[u8]) -> bool { bytes.iter().take(8000).any(|b| *b == 0) }

/// The changed lines between two texts: `(added, removed, hunks, more)`, at most `max` lines shown.
/// Each hunk is `(line in the new text, [(kind, text)])`.
#[allow(clippy::type_complexity)]
pub fn lines_between(old: &str, new: &str, max: usize) -> (usize, usize, Vec<(usize, Vec<(char, String)>)>, usize) {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let ops = edit_script(&a, &b);
    // Each step with the line it shows and where it is in the new text.
    let mut steps: Vec<(Op, &str, usize)> = Vec::with_capacity(ops.len());
    let (mut i, mut j) = (0, 0);
    for op in &ops {
        match op {
            Op::Same => { steps.push((Op::Same, a[i], j)); i += 1; j += 1; }
            Op::Del => { steps.push((Op::Del, a[i], j)); i += 1; }
            Op::Add => { steps.push((Op::Add, b[j], j)); j += 1; }
        }
    }
    let added = steps.iter().filter(|s| s.0 == Op::Add).count();
    let removed = steps.iter().filter(|s| s.0 == Op::Del).count();
    // Which steps are shown: every change, and `CONTEXT` unchanged lines on either side of one.
    let mut show = vec![false; steps.len()];
    for (n, s) in steps.iter().enumerate() {
        if s.0 != Op::Same {
            for x in show.iter_mut().take((n + CONTEXT + 1).min(steps.len())).skip(n.saturating_sub(CONTEXT)) { *x = true; }
        }
    }
    let mut hunks: Vec<(usize, Vec<(char, String)>)> = vec![];
    let (mut shown, mut more) = (0, 0);
    let mut open = false;
    for (n, s) in steps.iter().enumerate() {
        if !show[n] { open = false; continue; }
        let kind = match s.0 { Op::Same => ' ', Op::Del => '-', Op::Add => '+' };
        if shown >= max {
            if kind != ' ' { more += 1; }
            continue;
        }
        if !open { hunks.push((s.2 + 1, vec![])); open = true; }
        if let Some((_, lines)) = hunks.last_mut() { lines.push((kind, clip(s.1))); shown += 1; }
    }
    hunks.retain(|(_, l)| !l.is_empty());
    (added, removed, hunks, more)
}

/// One document for the window: its two versions (None where it does not exist) compared.
pub fn document(path: &str, from: Option<&str>, change: &str, old: Option<&[u8]>, new: Option<&[u8]>) -> Value {
    let binary = old.is_some_and(is_binary) || new.is_some_and(is_binary);
    if binary {
        return json!({"path": path, "from": from, "change": change, "binary": true, "detail": false, "added": null, "removed": null, "more": 0, "hunks": []});
    }
    let text = |b: Option<&[u8]>| b.map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    let max = if new.is_none() { REMOVED_LINES } else { LINES };
    let (added, removed, hunks, more) = lines_between(&text(old), &text(new), max);
    let hunks: Vec<Value> = hunks.iter().map(|(line, lines)| json!({
        "line": line, "lines": lines.iter().map(|(k, t)| json!([k.to_string(), t])).collect::<Vec<_>>(),
    })).collect();
    json!({"path": path, "from": from, "change": change, "binary": false, "detail": true, "added": added, "removed": removed, "more": more, "hunks": hunks})
}

/// A document listed by name only (too many to show each one, or too large).
fn named(path: &str, from: Option<&str>, change: &str) -> Value {
    json!({"path": path, "from": from, "change": change, "binary": false, "detail": false, "added": null, "removed": null, "more": 0, "hunks": []})
}

/// A document listed by name only because it is too large to show line by line (`tooLarge`).
pub(crate) fn too_large(path: &str, from: Option<&str>, change: &str) -> Value {
    let mut v = named(path, from, change);
    v["tooLarge"] = json!(true);
    v
}

/// `{files, total, counts: {added, edited, moved, deleted}}` around a list of documents.
fn listing(files: Vec<Value>) -> Value {
    let mut counts = json!({"added": 0, "edited": 0, "moved": 0, "deleted": 0});
    for f in &files {
        if let Some(c) = f["change"].as_str() { counts[c] = json!(counts[c].as_u64().unwrap_or(0) + 1); }
    }
    json!({"total": files.len(), "counts": counts, "files": files})
}

/// One line of `git diff-tree --raw -z`: the two blobs, the change and the name(s).
#[derive(Debug, PartialEq)]
struct Raw { old: String, new: String, special: bool, change: &'static str, path: String, from: Option<String> }

fn parse_raw(out: &str) -> Vec<Raw> {
    let mut it = out.split('\0');
    let mut list = vec![];
    while let Some(meta) = it.next() {
        // `:100644 100644 <old> <new> M`
        let Some(meta) = meta.trim_start_matches('\n').strip_prefix(':') else { continue };
        let p: Vec<&str> = meta.split(' ').collect();
        if p.len() < 5 { continue; }
        let letter = p[4].chars().next().unwrap_or('M');
        // A link or a folder of another repository is not a document that can be read.
        let special = [p[0], p[1]].iter().any(|m| m.starts_with("120") || m.starts_with("160"));
        let (path, from, change) = match letter {
            'R' => { let (Some(f), Some(t)) = (it.next(), it.next()) else { break }; (t.to_string(), Some(f.to_string()), "moved") }
            'C' => { let (Some(_), Some(t)) = (it.next(), it.next()) else { break }; (t.to_string(), None, "added") }
            _ => { let Some(path) = it.next() else { break }; (path.to_string(), None, match letter { 'A' => "added", 'D' => "deleted", _ => "edited" }) }
        };
        if !path.is_empty() { list.push(Raw { old: p[2].to_string(), new: p[3].to_string(), special, change, path, from }); }
    }
    list
}

/// The id git gives a side that does not exist.
fn null(id: &str) -> bool { id.is_empty() || id.bytes().all(|b| b == b'0') }

/// One side of a document: not there (or unreadable), too large to show, or its bytes.
enum Blob { None, TooLarge, Bytes(Vec<u8>) }

impl Blob {
    fn bytes(&self) -> Option<&[u8]> { if let Blob::Bytes(b) = self { Some(b) } else { None } }
}

/// How large `spec` (a blob id, or `<rev>:<path>`) is, without reading it. None: there is no such thing.
pub(crate) async fn size_of(state: &AppState, dir: &Path, spec: &str) -> Option<u64> {
    crate::git::git(state, dir, &["cat-file", "-s", spec]).await.ok()?.trim().parse().ok()
}

/// The bytes of `spec` (see `size_of`) — its size is looked at first, so one too large to show is never read.
async fn blob(state: &AppState, dir: &Path, spec: &str) -> Blob {
    if null(spec) { return Blob::None; }
    match size_of(state, dir, spec).await {
        None => Blob::None,
        Some(n) if n > MAX_BYTES => Blob::TooLarge,
        Some(_) => crate::git::git_raw(state, dir, &["cat-file", "blob", spec]).await.map_or(Blob::None, Blob::Bytes),
    }
}

/// What history entry `hash` changed, compared with the entry before it (the first one before it,
/// for an entry that combined two devices' changes: what arrived). The first entry shows every
/// document as new.
pub async fn of_entry(state: &AppState, dir: &Path, hash: &str) -> anyhow::Result<Value> {
    if !crate::git::valid_hash(hash) { return Err(crate::tr::err("history.bad_entry", &[])); }
    let commit = format!("{hash}^{{commit}}");
    let line = crate::git::git(state, dir, &["rev-list", "--parents", "-n", "1", "--end-of-options", &commit]).await
        .map_err(|_| crate::tr::err("history.bad_entry", &[]))?;
    let ids: Vec<&str> = line.split_whitespace().collect();
    let Some(me) = ids.first().copied() else { return Err(crate::tr::err("history.bad_entry", &[])) };
    let parent = ids.get(1).copied();
    let mut args = vec!["diff-tree", "-r", "-z", "--raw", "-M", "--no-commit-id", "--no-abbrev"];
    match parent { Some(p) => args.extend([p, me]), None => args.extend(["--root", me]) }
    let raw = parse_raw(&crate::git::git(state, dir, &args).await?);
    let mut files = vec![];
    for (n, r) in raw.iter().enumerate() {
        if n >= DETAILED || r.special { files.push(named(&r.path, r.from.as_deref(), r.change)); continue; }
        let old = if r.change == "added" { Blob::None } else { blob(state, dir, &r.old).await };
        let new = if r.change == "deleted" { Blob::None } else { blob(state, dir, &r.new).await };
        // Too large to read: named only, and said so.
        if matches!(old, Blob::TooLarge) || matches!(new, Blob::TooLarge) {
            files.push(too_large(&r.path, r.from.as_deref(), r.change));
            continue;
        }
        // Could not be read: named only.
        if (r.change != "added" && old.bytes().is_none() && !null(&r.old)) || (r.change != "deleted" && new.bytes().is_none() && !null(&r.new)) {
            files.push(named(&r.path, r.from.as_deref(), r.change));
            continue;
        }
        files.push(document(&r.path, r.from.as_deref(), r.change, old.bytes(), new.bytes()));
    }
    let mut v = listing(files);
    v["hash"] = json!(me);
    v["parent"] = json!(parent);
    Ok(v)
}

/// Document `rel` as it was at `hash` (under the name `was` it had then, if it was moved since)
/// against the text in the library now. A document that is gone now is shown as removed.
pub async fn with_now(state: &AppState, dir: &Path, rel: &str, hash: &str, was: Option<&str>) -> anyhow::Result<Value> {
    let big = || Ok(listing(vec![too_large(rel, was.filter(|w| *w != rel), "edited")]));
    // Both sizes are looked at before either side is read.
    if crate::git::valid_hash(hash) {
        let then_at = format!("{hash}:{}", was.unwrap_or(rel).trim_start_matches("./").trim_start_matches('/').replace('\\', "/"));
        if size_of(state, dir, &then_at).await.is_some_and(|n| n > MAX_BYTES) { return big(); }
    }
    let target = crate::library::resolve(dir, rel)?;
    if std::fs::metadata(&target).is_ok_and(|m| m.is_file() && m.len() > MAX_BYTES) { return big(); }
    let then = crate::git::file_at(state, dir, was.unwrap_or(rel), hash).await?;
    let now = match std::fs::metadata(&target) {
        Ok(m) if m.is_file() && m.len() > MAX_BYTES => return big(),
        Ok(m) if m.is_file() => Some(std::fs::read(&target)?),
        _ => None,
    };
    if then.len() as u64 > MAX_BYTES { return big(); }
    let change = if now.is_none() { "deleted" } else { "edited" };
    Ok(listing(vec![document(rel, was.filter(|w| *w != rel), change, Some(&then), now.as_deref())]))
}

/// A document that clashed in a sync: this computer's version (`mine`, as last recorded) against the
/// other device's (`theirs`, as last received). Either may be missing (removed on that side).
pub async fn clash(state: &AppState, dir: &Path, rel: &str) -> anyhow::Result<Value> {
    // The size of each side, before anything is read (none: that side does not have it — a removal, not an error).
    let size = |rev: &'static str| async move { size_of(state, dir, &format!("{rev}:{rel}")).await };
    let side = |rev: &'static str, there: bool| async move {
        if !there { return Ok(None); }
        crate::git::git_raw(state, dir, &["cat-file", "blob", &format!("{rev}:{rel}")]).await.map(Some)
    };
    if crate::git::git(state, dir, &["rev-parse", "--verify", "-q", "FETCH_HEAD^{commit}"]).await.is_err() {
        return Err(crate::tr::err("not_found.version", &[]));
    }
    let (mine_size, theirs_size) = (size("HEAD").await, size("FETCH_HEAD").await);
    if mine_size.is_none() && theirs_size.is_none() { return Err(crate::tr::err("not_found.version", &[])); }
    if mine_size.is_some_and(|n| n > MAX_BYTES) || theirs_size.is_some_and(|n| n > MAX_BYTES) {
        return Ok(listing(vec![too_large(rel, None, "edited")]));
    }
    let (mine, theirs) = (side("HEAD", mine_size.is_some()).await?, side("FETCH_HEAD", theirs_size.is_some()).await?);
    let change = match (&mine, &theirs) { (None, _) => "added", (_, None) => "deleted", _ => "edited" };
    Ok(listing(vec![document(rel, None, change, mine.as_deref(), theirs.as_deref())]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(a: &str, b: &str) -> String {
        let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
        edit_script(&a, &b).iter().map(|o| match o { Op::Same => '=', Op::Del => '-', Op::Add => '+' }).collect()
    }

    #[test]
    fn the_shortest_set_of_changed_lines_is_found() {
        assert_eq!(script("a\nb\nc\n", "a\nb\nc\n"), "===");
        assert_eq!(script("", "x\ny\n"), "++");
        assert_eq!(script("x\ny\n", ""), "--");
        assert_eq!(script("a\nb\nc\n", "a\nx\nc\n"), "=-+=");
        assert_eq!(script("a\nb\nc\nd\n", "a\nc\nd\ne\n"), "=-==+");
        // The classic example: ABCABBA → CBABAC takes 5 changes.
        let s = script("A\nB\nC\nA\nB\nB\nA", "C\nB\nA\nB\nA\nC");
        assert_eq!(s.chars().filter(|c| *c != '=').count(), 5, "{s}");
        // Applying the script to the old text gives the new one, on any pair.
        let cases = [("1\n2\n3\n4\n5\n6\n", "0\n2\n3\n5\n6\n7\n8\n"), ("same\n", "other\n"), ("a\na\na\n", "a\nb\na\n")];
        for (a, b) in cases {
            let (x, y): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
            let ops = edit_script(&x, &y);
            let (mut i, mut j, mut out) = (0, 0, vec![]);
            for op in ops { match op { Op::Same => { assert_eq!(x[i], y[j]); out.push(x[i]); i += 1; j += 1; } Op::Del => i += 1, Op::Add => { out.push(y[j]); j += 1; } } }
            assert_eq!((out, i, j), (y.clone(), x.len(), y.len()));
        }
    }

    #[test]
    fn far_too_many_differences_are_shown_as_replaced_whole() {
        let a: String = (0..4000).map(|i| format!("old {i}\n")).collect();
        let b: String = (0..4000).map(|i| format!("new {i}\n")).collect();
        let t = std::time::Instant::now();
        let (added, removed, hunks, more) = lines_between(&a, &b, LINES);
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
        assert_eq!((added, removed), (4000, 4000));
        assert_eq!(hunks.iter().map(|h| h.1.len()).sum::<usize>(), LINES);
        assert_eq!(more, 8000 - LINES, "what is not shown is counted");
    }

    #[test]
    fn changes_come_with_a_few_lines_around_them_and_their_line_number() {
        let old: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        let new = old.replace("line 5\n", "line five\n").replace("line 18\n", "") + "سطر جديد\n";
        let (added, removed, hunks, more) = lines_between(&old, &new, 100);
        assert_eq!((added, removed, more), (2, 2, 0));
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].0, 2, "the first line shown is line 2 of the new text");
        assert_eq!(hunks[0].1, vec![(' ', "line 2".into()), (' ', "line 3".into()), (' ', "line 4".into()), ('-', "line 5".into()), ('+', "line five".into()), (' ', "line 6".into()), (' ', "line 7".into()), (' ', "line 8".into())]);
        assert_eq!(hunks[1].1.last().unwrap(), &('+', "سطر جديد".to_string()));
        assert!(hunks[1].1.contains(&('-', "line 18".to_string())));
        // Windows line endings are not a change.
        assert_eq!(lines_between("a\r\nb\r\n", "a\nb\n", 10).0, 0);
        // A document's view is cut short, and the rest is counted.
        let (_, _, hunks, more) = lines_between(&old, &new, 3);
        assert_eq!((hunks.iter().map(|h| h.1.len()).sum::<usize>(), more), (3, 4));
    }

    #[test]
    fn a_document_for_the_window() {
        let d = document("notes/a.md", None, "edited", Some(b"one\ntwo\n"), Some(b"one\n2\n"));
        assert_eq!((d["added"].as_u64(), d["removed"].as_u64(), d["detail"].as_bool()), (Some(1), Some(1), Some(true)));
        assert_eq!(d["hunks"][0]["lines"], json!([[" ", "one"], ["-", "two"], ["+", "2"]]));
        let pic = document("logo.png", None, "added", None, Some(&[0u8, 1, 2]));
        assert_eq!((pic["binary"].as_bool(), pic["detail"].as_bool(), pic["hunks"].as_array().map(Vec::len)), (Some(true), Some(false), Some(0)));
        let gone: String = (0..100).map(|i| format!("{i}\n")).collect();
        let gone = document("old.md", None, "deleted", Some(gone.as_bytes()), None);
        assert_eq!((gone["removed"].as_u64(), gone["more"].as_u64()), (Some(100), Some(100 - REMOVED_LINES as u64)));
    }

    #[test]
    fn raw_lines_name_each_document_exactly() {
        let z = "0000000000000000000000000000000000000000";
        let out = format!(":100644 100644 {z}1 {z}2 M\0README.md\0:000000 100644 {z} {z}3 A\0ideas/متجر صغير/README.md\0:100644 000000 {z}4 {z} D\0old.md\0:100644 100644 {z}5 {z}6 R087\0notes/a b.md\0areas/a b.md\0:120000 120000 {z}7 {z}8 M\0link\0");
        let r = parse_raw(&out);
        assert_eq!(r.iter().map(|x| (x.path.as_str(), x.change, x.from.as_deref(), x.special)).collect::<Vec<_>>(), vec![
            ("README.md", "edited", None, false), ("ideas/متجر صغير/README.md", "added", None, false), ("old.md", "deleted", None, false),
            ("areas/a b.md", "moved", Some("notes/a b.md"), false), ("link", "edited", None, true),
        ]);
        assert!(parse_raw("").is_empty());
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git").current_dir(dir)
            .args(["-c", "user.name=Tester", "-c", "user.email=tester@example.com", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
            .args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }
    fn write(dir: &Path, rel: &str, text: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[tokio::test]
    async fn a_document_too_large_to_show_is_said_so_in_history_versions_and_clashes() {
        let d = std::env::temp_dir().join(format!("thoughts-diffs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        sh(&d, &["init", "-q", "-b", "main"]);
        let st = AppState::for_test();
        let big = "a row of a large export, again and again\n".repeat(60_000);
        assert!(big.len() as u64 > MAX_BYTES);
        write(&d, "export.csv", big.as_bytes());
        write(&d, "note.md", b"small\n");
        sh(&d, &["add", "-A"]);
        sh(&d, &["commit", "-q", "-m", "first"]);
        let first = sh(&d, &["rev-parse", "HEAD"]);
        write(&d, "export.csv", big.replace("again", "once more").as_bytes());
        write(&d, "note.md", b"small, edited\n");
        sh(&d, &["commit", "-q", "-am", "second"]);
        let second = sh(&d, &["rev-parse", "HEAD"]);
        let v = of_entry(&st, &d, &second).await.unwrap();
        let by = |p: &str| v["files"].as_array().unwrap().iter().find(|f| f["path"] == p).cloned().unwrap();
        assert_eq!((by("export.csv")["tooLarge"].as_bool(), by("export.csv")["detail"].as_bool()), (Some(true), Some(false)));
        assert_eq!((by("note.md")["tooLarge"].as_bool(), by("note.md")["detail"].as_bool()), (None, Some(true)));
        // An earlier version against the text now.
        let v = with_now(&st, &d, "export.csv", &first, None).await.unwrap();
        assert_eq!(v["files"][0]["tooLarge"], true);
        assert_eq!(with_now(&st, &d, "note.md", &first, None).await.unwrap()["files"][0]["detail"], true);
        // A clash with what arrived.
        std::fs::write(d.join(".git/FETCH_HEAD"), format!("{first}\t\tbranch 'main' of elsewhere\n")).unwrap();
        assert_eq!(clash(&st, &d, "export.csv").await.unwrap()["files"][0]["tooLarge"], true);
        assert_eq!(clash(&st, &d, "note.md").await.unwrap()["files"][0]["detail"], true);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn an_entry_a_version_and_a_clash_are_shown_document_by_document() {
        let d = std::env::temp_dir().join(format!("thoughts-diffs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        sh(&d, &["init", "-q", "-b", "main"]);
        let st = AppState::for_test();
        write(&d, "TODO.md", b"# Todo\n\n- [ ] call the bank\n");
        write(&d, "ideas/shop/README.md", b"# Shop\n\nSell things.\n");
        write(&d, "README.md", b"hello\n");
        sh(&d, &["add", "-A"]);
        sh(&d, &["commit", "-q", "-m", "first"]);
        let first = sh(&d, &["rev-parse", "HEAD"]);
        write(&d, "TODO.md", b"# Todo\n\n- [ ] call the bank\n- [ ] order the sign\n");
        write(&d, "ideas/\u{645}\u{62a}\u{62c}\u{631}/README.md", "فكرة جديدة\n".as_bytes());
        std::fs::remove_file(d.join("README.md")).unwrap();
        std::fs::create_dir_all(d.join("areas/shop")).unwrap();
        std::fs::rename(d.join("ideas/shop/README.md"), d.join("areas/shop/README.md")).unwrap();
        write(&d, "logo.png", &[0u8, 159, 146, 150, 0, 1, 2]);
        // Settings in the person's own git that would change what a comparison looks like are not used.
        sh(&d, &["config", "diff.noprefix", "true"]);
        sh(&d, &["config", "color.ui", "always"]);
        sh(&d, &["add", "-A"]);
        sh(&d, &["commit", "-q", "-m", "second"]);
        let second = sh(&d, &["rev-parse", "HEAD"]);

        let v = of_entry(&st, &d, &second).await.unwrap();
        assert_eq!(v["counts"], json!({"added": 2, "edited": 1, "moved": 1, "deleted": 1}), "{v}");
        let by = |p: &str| v["files"].as_array().unwrap().iter().find(|f| f["path"] == p).cloned().unwrap_or_else(|| panic!("{p}: {v}"));
        let todo = by("TODO.md");
        assert_eq!((todo["added"].as_u64(), todo["removed"].as_u64()), (Some(1), Some(0)));
        assert!(todo["hunks"][0]["lines"].as_array().unwrap().contains(&json!(["+", "- [ ] order the sign"])));
        assert_eq!(by("ideas/متجر/README.md")["hunks"][0]["lines"], json!([["+", "فكرة جديدة"]]));
        assert_eq!(by("areas/shop/README.md")["from"], "ideas/shop/README.md");
        assert_eq!(by("README.md")["hunks"][0]["lines"], json!([["-", "hello"]]));
        assert_eq!(by("logo.png")["binary"], true);
        // The first entry: everything is new.
        let v = of_entry(&st, &d, &first).await.unwrap();
        assert_eq!((v["counts"]["added"].as_u64(), v["parent"].is_null()), (Some(3), true));
        assert!(of_entry(&st, &d, "not-a-hash").await.is_err());
        assert!(of_entry(&st, &d, "0123456789abcdef").await.is_err());

        // A version against the text now: under the name it had then, and a document gone since.
        write(&d, "areas/shop/README.md", b"# Shop\n\nSell things.\nAnd more.\n");
        let v = with_now(&st, &d, "areas/shop/README.md", &first, Some("ideas/shop/README.md")).await.unwrap();
        assert_eq!(v["files"][0]["hunks"][0]["lines"].as_array().unwrap().last().unwrap(), &json!(["+", "And more."]));
        assert_eq!(v["files"][0]["from"], "ideas/shop/README.md");
        let v = with_now(&st, &d, "README.md", &first, None).await.unwrap();
        assert_eq!(v["files"][0]["change"], "deleted");
        assert!(with_now(&st, &d, "never.md", &first, None).await.is_err());

        // A clash: mine against what arrived. Nothing has arrived yet: there is nothing to show.
        assert!(clash(&st, &d, "TODO.md").await.is_err());
        std::fs::write(d.join(".git/FETCH_HEAD"), format!("{first}\t\tbranch 'main' of elsewhere\n")).unwrap();
        let v = clash(&st, &d, "TODO.md").await.unwrap();
        assert_eq!(v["files"][0]["hunks"][0]["lines"].as_array().unwrap().last().unwrap(), &json!(["-", "- [ ] order the sign"]));
        assert_eq!(clash(&st, &d, "README.md").await.unwrap()["files"][0]["change"], "added", "only the other side has it");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
