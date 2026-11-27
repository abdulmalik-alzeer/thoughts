//! What the chat's @-list offers: every idea, note, other file and folder of the library, each with
//! the name a person knows it by (an idea's title, a note's title) next to where it is. Private
//! folders are left out wherever they really are, as are hidden and protected files.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;
use walkdir::WalkDir;

/// The most entries one list holds (the composer narrows it down as the user types).
pub const MAX_ENTRIES: usize = 20_000;
/// How much of a note is read to find its title: its details and first lines are at the top.
const HEAD_BYTES: u64 = 8 * 1024;

type Seen = HashMap<PathBuf, (Option<SystemTime>, u64, Option<String>)>;
/// Titles already read, by file (and the version of it they were read from).
fn cache() -> &'static Mutex<Seen> {
    static C: OnceLock<Mutex<Seen>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn title_of(p: &Path, mtime: Option<SystemTime>, len: u64, cache: &mut Seen) -> Option<String> {
    if let Some((m, l, t)) = cache.get(p) { if *m == mtime && *l == len { return t.clone(); } }
    let mut head = Vec::new();
    let _ = std::fs::File::open(p).and_then(|f| f.take(HEAD_BYTES).read_to_end(&mut head));
    let t = crate::search::title_of(&String::from_utf8_lossy(&head));
    cache.insert(p.to_path_buf(), (mtime, len, t.clone()));
    t
}

fn is_markdown(rel: &str) -> bool {
    rel.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// `[{path, kind, title}]`, `kind` one of `idea` (an idea's front page), `note` (any other markdown
/// file), `file` or `folder`. `title` is null when the file names none; an idea's folder carries the
/// idea's title. `exclude` = the private folders.
pub fn list(root: &Path, exclude: &[String]) -> Vec<Value> {
    let hidden = crate::library::Hidden::new(root, exclude);
    let mut out: Vec<Value> = vec![];
    let mut idea_titles: HashMap<String, String> = HashMap::new();
    let mut read: HashSet<PathBuf> = HashSet::new();
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    let walk = WalkDir::new(root).min_depth(1).sort_by_file_name().into_iter()
        .filter_entry(|e| !crate::library::skip(e) && !hidden.folder(e) && !e.file_name().to_string_lossy().starts_with('.'));
    for e in walk.flatten() {
        if out.len() >= MAX_ENTRIES { break; }
        let Some(rel) = e.path().strip_prefix(root).ok().map(|p| p.to_string_lossy().replace('\\', "/")) else { continue };
        if crate::paths::is_under(&rel, exclude) || crate::library::denied(&rel) { continue; }
        // Links are not followed: what one leads to is reached by its own name, or not at all.
        if e.file_type().is_dir() { out.push(json!({"path": rel, "kind": "folder", "title": null})); continue; }
        if !e.file_type().is_file() { continue; }
        let parts: Vec<&str> = rel.split('/').collect();
        let idea = parts.len() == 3 && parts[2] == "README.md" && !matches!(parts[0], "me" | "inbox");
        let title = if is_markdown(&rel) {
            let meta = e.metadata().ok();
            read.insert(e.path().to_path_buf());
            title_of(e.path(), meta.as_ref().and_then(|m| m.modified().ok()), meta.map_or(0, |m| m.len()), &mut cache)
        } else { None };
        if let (true, Some(t)) = (idea, &title) { idea_titles.insert(format!("{}/{}", parts[0], parts[1]), t.clone()); }
        out.push(json!({"path": rel, "kind": if idea { "idea" } else if is_markdown(&rel) { "note" } else { "file" }, "title": title}));
    }
    cache.retain(|p, _| read.contains(p));
    // An idea's folder goes by the idea's title.
    for v in out.iter_mut().filter(|v| v["kind"] == "folder") {
        if let Some(t) = v["path"].as_str().and_then(|p| idea_titles.get(p)) { v["title"] = json!(t); }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn everything_by_title_but_nothing_private_hidden_or_protected() {
        let root = std::env::temp_dir().join(format!("thoughts-mentions-{}", uuid::Uuid::new_v4()));
        write(&root, "clients/acme/README.md", "---\ntitle: \"Acme Trading\"\nstatus: active\n---\nBody\n");
        write(&root, "clients/acme/notes/pricing.md", "# Pricing for 2027\n\ntext\n");
        write(&root, "clients/acme/notes/plain.md", "no title here\n");
        write(&root, "clients/acme/outputs/offer.pdf", "%PDF-1.4");
        write(&root, "ideas/قهوة/README.md", "---\ntitle: مقهى في الحي\n---\n");
        write(&root, "me/README.md", "---\ntitle: About me\n---\n");
        write(&root, "secret/plan.md", "# The secret plan\n");
        write(&root, "Secret2/x.md", "# also private\n");
        write(&root, ".obsidian/app.json", "{}");
        write(&root, ".claude/settings.json", "{}");
        write(&root, "clients/.DS_Store", "x");
        write(&root, "CLAUDE.md", "# Library rules\n");
        let all = list(&root, &["secret".into(), "secret2".into()]);
        let get = |p: &str| all.iter().find(|v| v["path"] == p).cloned();
        assert_eq!(get("clients/acme/README.md").unwrap(), json!({"path": "clients/acme/README.md", "kind": "idea", "title": "Acme Trading"}));
        assert_eq!(get("clients/acme").unwrap()["title"], "Acme Trading", "an idea's folder goes by the idea's name");
        assert_eq!(get("clients/acme/notes/pricing.md").unwrap()["title"], "Pricing for 2027");
        assert_eq!(get("clients/acme/notes/plain.md").unwrap()["title"], Value::Null);
        assert_eq!(get("clients/acme/outputs/offer.pdf").unwrap()["kind"], "file");
        assert_eq!(get("ideas/قهوة/README.md").unwrap()["title"], "مقهى في الحي");
        assert_eq!(get("me/README.md").unwrap()["kind"], "note", "the profile is not an idea");
        assert_eq!(get("clients").unwrap()["kind"], "folder");
        assert!(get("CLAUDE.md").is_some());
        for gone in ["secret", "secret/plan.md", "Secret2", "Secret2/x.md", ".obsidian", ".obsidian/app.json", ".claude", ".claude/settings.json", "clients/.DS_Store"] {
            assert!(get(gone).is_none(), "{gone} is offered");
        }
        // A title read once is read again only when the note changes.
        write(&root, "clients/acme/notes/pricing.md", "# Pricing for 2028\n");
        let again = list(&root, &[]);
        assert_eq!(again.iter().find(|v| v["path"] == "clients/acme/notes/pricing.md").unwrap()["title"], "Pricing for 2028");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_private_folder_is_not_a_way_in() {
        let root = std::env::temp_dir().join(format!("thoughts-mentions-{}", uuid::Uuid::new_v4()));
        write(&root, "private/diary.md", "# Diary\n");
        write(&root, "notes/a.md", "# A\n");
        std::os::unix::fs::symlink(root.join("private"), root.join("notes/shortcut")).unwrap();
        let all = list(&root, &["private".into()]);
        assert!(all.iter().all(|v| !v["path"].as_str().unwrap().contains("diary")), "{all:?}");
        assert!(all.iter().any(|v| v["path"] == "notes/a.md"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
