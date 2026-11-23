//! Slides written in markdown and presented in the app (round 0.5.0). The window does the work
//! (`ui/src/lib/slides/`: reveal.js over the app's own sanitised markdown); this side tells a run how
//! to write a deck and keeps the guide it reads (`assets/slides/GUIDE.md`) unpacked where every run
//! may read it and none may change it (`<home>/extensions/slides-guide/`, like the diagram guide).

use std::path::{Path, PathBuf};

pub const GUIDE: &str = include_str!("../assets/slides/GUIDE.md");

/// Where the guide is unpacked.
pub fn guide_dir(home: &Path) -> PathBuf { home.join("extensions").join("slides-guide") }

/// The guide, unpacked now when it is missing or differs from the one this binary carries. Blocking.
pub fn guide(home: &Path) -> std::io::Result<PathBuf> {
    let dir = guide_dir(home);
    let file = dir.join("GUIDE.md");
    let same = std::fs::symlink_metadata(&file).is_ok_and(|m| m.is_file()) && std::fs::read_to_string(&file).is_ok_and(|t| t == GUIDE);
    if !same {
        crate::paths::create_private_dir(&dir)?;
        if std::fs::symlink_metadata(&file).is_ok_and(|m| !m.is_file()) { std::fs::remove_file(&file)?; }
        crate::paths::write_atomic(&file, GUIDE.as_bytes(), false)?;
    }
    Ok(file)
}

/// What a run is told about slides. `guide`: the unpacked guide, when it could be.
pub fn note(guide: Option<&Path>) -> String {
    let read = guide.map(|g| format!(" Before your first deck in a conversation read {}.", g.display())).unwrap_or_default();
    format!("Slide decks: a note with `slides: true` in its frontmatter, slides separated by a line with only `---`, speaker notes after \
        `Note:`; in an answer use a fenced block `slides`.{read}")
}

/// The kinds of run that may write a deck: ordinary chats and customise chats.
fn for_kind(kind: &str) -> bool { matches!(kind, "chat" | "customize") }

/// What a run of `kind` is told about writing slides (`run_notes::all`); nothing for other runs.
pub async fn run_note(kind: &str) -> Option<String> {
    if !for_kind(kind) { return None; }
    let home = crate::paths::home();
    let file = tokio::task::spawn_blocking(move || guide(&home)).await.ok()
        .and_then(|r| r.map_err(|e| crate::log::warn(format!("the slides guide could not be unpacked: {e}"))).ok());
    Some(note(file.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_is_unpacked_once_and_put_back_when_changed() {
        let home = std::env::temp_dir().join(format!("thoughts-slides-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&home).unwrap();
        let f = guide(&home).unwrap();
        assert_eq!(f, home.join("extensions/slides-guide/GUIDE.md"));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), GUIDE);
        std::fs::write(&f, "changed").unwrap();
        guide(&home).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), GUIDE, "a changed copy is replaced");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn the_note_names_the_convention_and_the_guide() {
        let n = note(Some(Path::new("/x/GUIDE.md")));
        for want in ["`slides: true`", "`---`", "`Note:`", "`slides`", "/x/GUIDE.md"] { assert!(n.contains(want), "{want}: {n}"); }
        assert!(!note(None).contains("Before your first deck"));
        // Both languages, an example each, figures, right to left, and no raw HTML.
        for want in ["slides: true", "ملاحظة:", "dir: rtl", "```chart", "Never raw HTML", "```slides"] { assert!(GUIDE.contains(want), "{want}"); }
        assert!(for_kind("chat") && for_kind("customize") && !for_kind("wrap") && !for_kind("mado"));
    }
}
