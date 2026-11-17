//! The Ideas list, kept between requests. The window asks for it after every change in the
//! library, and building it reads every item's README and todo list. Here it is built again only
//! when one of those files (or the set of items) changed: each request still walks the item
//! folders and looks at the size and time of those two files per item, but reads nothing.
//!
//! A file changed within the last few seconds never lets the list be kept: some drives (a memory
//! stick, a shared folder) only keep the time to the nearest second or two, so two changes in the
//! same tick would otherwise look like one.

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

/// A file is only trusted to have stayed the same by its time and size once it is this old.
const SETTLED: Duration = Duration::from_secs(3);

/// Size and time of a file, `None` when it is not there.
type Stamp = Option<(SystemTime, u64)>;

/// Every item README the Ideas list reads, with its todo list beside it.
type Prints = Vec<(PathBuf, Stamp, Stamp)>;

struct Kept { prints: Prints, ideas: Arc<Vec<Value>> }

fn kept() -> &'static Mutex<HashMap<PathBuf, Kept>> {
    static KEPT: OnceLock<Mutex<HashMap<PathBuf, Kept>>> = OnceLock::new();
    KEPT.get_or_init(Default::default)
}

fn stamp(p: &Path) -> Stamp {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// What the Ideas list depends on, and whether all of it has settled (see `SETTLED`).
/// The same walk as `library::ideas`: every `<area>/<item>/README.md`.
/// If `library::ideas` ever reads another file of an item, it must be stamped here too.
fn prints(root: &Path) -> (Prints, bool) {
    let now = SystemTime::now();
    let mut settled = true;
    let mut out = vec![];
    for e in WalkDir::new(root).min_depth(3).max_depth(3).into_iter().filter_entry(|e| !crate::library::skip(e)).flatten() {
        if e.file_name() != "README.md" { continue; }
        let (readme, todo) = (stamp(e.path()), stamp(&e.path().with_file_name("todo.md")));
        for (t, _) in readme.iter().chain(todo.iter()) {
            if now.duration_since(*t).map_or(true, |age| age < SETTLED) { settled = false; }
        }
        out.push((e.path().to_path_buf(), readme, todo));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    (out, settled)
}

/// `library::ideas(root)`, read again only when an item's README or todo list changed, or an
/// item came or went.
pub fn ideas(root: &Path) -> Arc<Vec<Value>> {
    let (now, settled) = prints(root);
    if let Some(k) = kept().lock().unwrap_or_else(|e| e.into_inner()).get(root) {
        if k.prints == now { return k.ideas.clone(); }
    }
    let ideas = Arc::new(crate::library::ideas(root));
    let mut all = kept().lock().unwrap_or_else(|e| e.into_inner());
    // Kept under what was there before the read: anything that changes during or after it no
    // longer matches, so it is read again next time. Read while something was still settling: not kept.
    if settled { all.insert(root.to_path_buf(), Kept { prints: now, ideas: ideas.clone() }); } else { all.remove(root); }
    ideas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-listings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        dunce::canonicalize(d).unwrap()
    }

    /// Write a file and date it `ago` seconds back, as an edit made a while ago would be.
    fn put(root: &Path, rel: &str, text: &str, ago: u64) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(ago)).unwrap();
    }

    fn titles(v: &[Value]) -> Vec<String> {
        let mut t: Vec<String> = v.iter().map(|i| format!("{}:{}:{}", i["slug"].as_str().unwrap(), i["title"].as_str().unwrap(), i["openTodos"])).collect();
        t.sort();
        t
    }

    fn readme(title: &str) -> String { format!("---\ntitle: {title}\nstatus: seed\nupdated: 2026-01-01\n---\nText.\n") }

    #[test]
    fn the_ideas_list_is_kept_until_an_item_changes_and_then_read_again() {
        let d = tmp();
        put(&d, "ideas/shop/README.md", &readme("Shop"), 60);
        put(&d, "ideas/cafe/README.md", &readme("Cafe"), 60);
        let first = ideas(&d);
        assert_eq!(titles(&first), ["cafe:Cafe:0", "shop:Shop:0"]);
        assert_eq!(titles(&first), titles(&crate::library::ideas(&d)));
        // Nothing changed: the same list, not read again.
        assert!(Arc::ptr_eq(&first, &ideas(&d)));

        // A README rewritten with text of the same length, at another time: read again.
        put(&d, "ideas/shop/README.md", &readme("Shoe"), 50);
        assert_eq!(titles(&ideas(&d)), ["cafe:Cafe:0", "shop:Shoe:0"]);
        // A todo list added, then changed.
        put(&d, "ideas/cafe/todo.md", "- [ ] buy beans\n", 40);
        assert_eq!(titles(&ideas(&d)), ["cafe:Cafe:1", "shop:Shoe:0"]);
        put(&d, "ideas/cafe/todo.md", "- [ ] buy beans\n- [ ] hire\n", 30);
        assert_eq!(titles(&ideas(&d)), ["cafe:Cafe:2", "shop:Shoe:0"]);
        // An item added, renamed, removed.
        put(&d, "projects/site/README.md", &readme("Site"), 20);
        assert_eq!(titles(&ideas(&d)), ["cafe:Cafe:2", "shop:Shoe:0", "site:Site:0"]);
        std::fs::rename(d.join("projects/site"), d.join("projects/web")).unwrap();
        assert_eq!(titles(&ideas(&d)), ["cafe:Cafe:2", "shop:Shoe:0", "web:Site:0"]);
        std::fs::remove_dir_all(d.join("ideas/cafe")).unwrap();
        assert_eq!(titles(&ideas(&d)), ["shop:Shoe:0", "web:Site:0"]);
        // A todo list taken away.
        put(&d, "ideas/shop/todo.md", "- [ ] one\n", 10);
        assert_eq!(titles(&ideas(&d)), ["shop:Shoe:1", "web:Site:0"]);
        std::fs::remove_file(d.join("ideas/shop/todo.md")).unwrap();
        assert_eq!(titles(&ideas(&d)), ["shop:Shoe:0", "web:Site:0"]);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_file_changed_just_now_is_never_trusted_by_its_time_and_size() {
        let d = tmp();
        put(&d, "ideas/shop/README.md", &readme("Shop"), 0);
        assert_eq!(titles(&ideas(&d)), ["shop:Shop:0"]);
        // Same size, and on a drive that keeps times only to the second this could be the same
        // time too: still read again, because the file changed only a moment ago.
        let p = d.join("ideas/shop/README.md");
        let when = std::fs::metadata(&p).unwrap().modified().unwrap();
        std::fs::write(&p, readme("Shoe")).unwrap();
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(when).unwrap();
        assert_eq!(titles(&ideas(&d)), ["shop:Shoe:0"]);
        std::fs::remove_dir_all(d).unwrap();
    }
}
