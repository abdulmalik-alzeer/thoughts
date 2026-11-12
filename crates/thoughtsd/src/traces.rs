//! What the assistant program (Claude Code) keeps on disk about a conversation, outside the app's
//! own folder — and removing it when the user deletes that chat.
//!
//! Claude Code writes one transcript per session, `<config>/projects/<folder>/<session id>.jsonl`.
//! A session that ran in plan mode (every Wrap preview does) also leaves a plan file,
//! `<config>/plans/<slug>.md`; the slug is written on the lines of the session's own transcript.
//! `<config>` is `~/.claude`, or `$CLAUDE_CONFIG_DIR` when that is set.
//!
//! Deleting here is deliberately narrow. Only regular files are removed, each under one exact name
//! built from a session id the app itself was given by the CLI for that chat (or from a slug read
//! out of that session's transcript). Never a folder, never a link, never a name found by
//! matching a pattern — and nothing at all when an id or a slug does not have the expected shape.

use std::collections::BTreeSet;
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// Claude Code's own folder: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
#[cfg(not(test))]
pub fn config_dir() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).filter(|p| p.is_absolute()) {
        Some(dir) => Some(dir),
        None => dirs::home_dir().map(|h| h.join(".claude")),
    }
}

/// Under test this is always a folder inside the throwaway test home, never the real `~/.claude`.
#[cfg(test)]
pub fn config_dir() -> Option<PathBuf> { Some(crate::paths::home().join("claude-config")) }

/// A session id exactly as Claude Code makes them: a UUID written `8-4-4-4-12` in hex digits.
pub fn is_session_id(s: &str) -> bool {
    s.len() == 36 && s.bytes().enumerate().all(|(i, b)| if matches!(i, 8 | 13 | 18 | 23) { b == b'-' } else { b.is_ascii_hexdigit() })
}

/// A plan slug as Claude Code makes them: lowercase words and digits joined by single dashes
/// (`wrap-the-user-elegant-hippo`).
fn is_plan_slug(s: &str) -> bool {
    (1..=200).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-') && !s.ends_with('-') && !s.contains("--")
}

/// A regular file — not a folder and not a link to anything.
fn is_plain_file(p: &Path) -> bool { std::fs::symlink_metadata(p).map_or(false, |m| m.file_type().is_file()) }
/// A real folder — not a link to one.
fn is_real_dir(p: &Path) -> bool { std::fs::symlink_metadata(p).map_or(false, |m| m.file_type().is_dir()) }

/// How many files were deleted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Removed { pub transcripts: usize, pub plans: usize }

/// The plan slugs a transcript names on lines that belong to session `sid`.
fn plan_slugs(transcript: &Path, sid: &str) -> BTreeSet<String> {
    #[derive(serde::Deserialize)]
    struct Line {
        #[serde(default, rename = "sessionId")]
        session_id: Option<String>,
        #[serde(default)]
        slug: Option<String>,
    }
    let mut out = BTreeSet::new();
    let Ok(f) = std::fs::File::open(transcript) else { return out };
    for line in std::io::BufReader::new(f).split(b'\n').map_while(Result::ok) {
        // Cheap test first: transcripts can be many megabytes.
        if !line.windows(6).any(|w| w == b"\"slug\"") { continue; }
        let Ok(l) = serde_json::from_slice::<Line>(&line) else { continue };
        if l.session_id.as_deref() != Some(sid) { continue; }
        if let Some(slug) = l.slug.filter(|s| is_plan_slug(s)) { out.insert(slug); }
    }
    out
}

/// Delete what Claude Code kept under `config` for these sessions: each session's transcript and
/// the plan file that transcript names. Ids that are not session ids are ignored. See the module
/// notes for what is, and is never, removed.
pub fn remove_sessions(config: &Path, session_ids: &[String]) -> Removed {
    let mut out = Removed::default();
    let ids: BTreeSet<&str> = session_ids.iter().map(String::as_str).filter(|s| is_session_id(s)).collect();
    let projects = config.join("projects");
    if ids.is_empty() || !is_real_dir(&projects) { return out; }
    // One folder per working folder the assistant has run in; a session's transcript is in one of them.
    let folders: Vec<PathBuf> = std::fs::read_dir(&projects).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| is_real_dir(p)).collect();
    let mut slugs = BTreeSet::new();
    for sid in &ids {
        let name = format!("{sid}.jsonl");
        for folder in &folders {
            let file = folder.join(&name);
            if !is_plain_file(&file) { continue; }
            slugs.extend(plan_slugs(&file, sid));
            match std::fs::remove_file(&file) {
                Ok(()) => out.transcripts += 1,
                Err(e) => crate::log::warn(format!("the assistant's transcript of a deleted chat could not be removed: {e}")),
            }
        }
    }
    let plans = config.join("plans");
    if !slugs.is_empty() && is_real_dir(&plans) {
        for slug in &slugs {
            let file = plans.join(format!("{slug}.md"));
            if !is_plain_file(&file) { continue; }
            match std::fs::remove_file(&file) {
                Ok(()) => out.plans += 1,
                Err(e) => crate::log::warn(format!("the assistant's plan file of a deleted chat could not be removed: {e}")),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid() -> String { uuid::Uuid::new_v4().to_string() }
    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    /// A transcript the way Claude Code writes one: one JSON object per line, each naming its session
    /// and (once the session has one) its plan slug.
    fn transcript(sid: &str, slug: Option<&str>) -> String {
        let slug = slug.map(|s| format!(r#","slug":"{s}""#)).unwrap_or_default();
        format!("{{\"type\":\"file-history-snapshot\",\"messageId\":\"m1\"}}\n\
                 {{\"parentUuid\":null,\"sessionId\":\"{sid}\",\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"client text\"}}{slug}}}\n\
                 {{\"parentUuid\":\"u1\",\"sessionId\":\"{sid}\",\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[]}}{slug}}}\n")
    }
    /// A pretend home folder with a `.claude` inside; returns (home, config).
    fn home() -> (PathBuf, PathBuf) {
        let home = std::env::temp_dir().join(format!("thoughts-traces-home-{}", uuid::Uuid::new_v4()));
        let config = home.join(".claude");
        std::fs::create_dir_all(config.join("projects")).unwrap();
        std::fs::create_dir_all(config.join("plans")).unwrap();
        (home, config)
    }
    /// Every file and link under `root`, relative, sorted.
    fn listing(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = walkdir::WalkDir::new(root).into_iter().flatten().filter(|e| !e.file_type().is_dir())
            .map(|e| e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")).collect();
        v.sort();
        v
    }

    #[test]
    fn ids_and_slugs_must_have_the_expected_shape() {
        assert!(is_session_id("0b8f1c2e-3a4d-4e5f-8a9b-0c1d2e3f4a5b") && is_session_id(&sid()));
        for bad in ["", "..", "../x", "0b8f1c2e-3a4d-4e5f-8a9b-0c1d2e3f4a5", "0b8f1c2e-3a4d-4e5f-8a9b-0c1d2e3f4a5b0", "0b8f1c2e/3a4d-4e5f-8a9b-0c1d2e3f4a5b",
                    "0b8f1c2e-3a4d-4e5f-8a9b-0c1d2e3f4a5g", "0b8f1c2e33a4d-4e5f-8a9b-0c1d2e3f4a5b", "*", "????????-????-????-????-????????????"] {
            assert!(!is_session_id(bad), "{bad}");
        }
        for ok in ["wrap-the-user-elegant-hippo", "quirky-hopping-starfish", "a", "plan-2"] { assert!(is_plan_slug(ok), "{ok}"); }
        for bad in ["", "..", "../x", "a/b", "a.b", "A-b", "-a", "a-", "a--b", "a b", "*", "a\\b", &"a".repeat(201)] { assert!(!is_plan_slug(bad), "{bad}"); }
    }

    #[test]
    fn only_the_sessions_own_transcript_and_plan_are_removed() {
        let (home, config) = home();
        let (mine, resumed, other) = (sid(), sid(), sid());
        // This chat: a first session with a plan (a Wrap preview), and a later session in another working folder.
        write(&config.join(format!("projects/-Users-sara-Notes/{mine}.jsonl")), &transcript(&mine, Some("wrap-the-user-elegant-hippo")));
        write(&config.join("plans/wrap-the-user-elegant-hippo.md"), "# Plan\nclient text\n");
        write(&config.join(format!("projects/-Users-sara-Work/{resumed}.jsonl")), &transcript(&resumed, None));
        // Everything else under ~/.claude belongs to someone else and must survive.
        write(&config.join(format!("projects/-Users-sara-Notes/{other}.jsonl")), &transcript(&other, Some("quirky-hopping-starfish")));
        write(&config.join("plans/quirky-hopping-starfish.md"), "another session's plan\n");
        write(&config.join("plans/wrap-the-user-elegant-hippo-agent-a1b2.md"), "a helper's plan\n");
        write(&config.join(format!("projects/-Users-sara-Notes/{mine}/subagents/agent-1.jsonl")), "{}\n");
        write(&config.join(format!("projects/-Users-sara-Notes/{mine}.jsonl.bak")), "a copy the user made\n");
        write(&config.join(format!("todos/{mine}-agent-{mine}.json")), "[]");
        write(&config.join("settings.json"), "{}");
        write(&config.join(".credentials.json"), "{}");
        write(&config.join("CLAUDE.md"), "my instructions\n");
        write(&home.join("Documents/keep.txt"), "keep");
        let before = listing(&home);

        let removed = remove_sessions(&config, &[mine.clone(), resumed.clone()]);
        assert_eq!(removed, Removed { transcripts: 2, plans: 1 });
        let gone = [format!(".claude/projects/-Users-sara-Notes/{mine}.jsonl"), format!(".claude/projects/-Users-sara-Work/{resumed}.jsonl"),
            ".claude/plans/wrap-the-user-elegant-hippo.md".to_string()];
        let expected: Vec<String> = before.iter().filter(|p| !gone.contains(p)).cloned().collect();
        assert_eq!(listing(&home), expected, "exactly the two transcripts and the one plan file go");
        // No folder is ever removed, not even one left empty.
        assert!(config.join("projects/-Users-sara-Work").is_dir() && config.join(format!("projects/-Users-sara-Notes/{mine}/subagents")).is_dir());
        // Doing it again finds nothing.
        assert_eq!(remove_sessions(&config, &[mine, resumed]), Removed::default());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn hostile_ids_slugs_and_links_remove_nothing() {
        let (home, config) = home();
        let real = sid();
        write(&config.join(format!("projects/-p/{real}.jsonl")), &transcript(&real, None));
        write(&config.join("plans/keep.md"), "keep");
        write(&config.join("settings.json"), "{}");
        write(&home.join("outside/notes.jsonl"), "keep");
        write(&home.join("outside.md"), "keep");
        let before = listing(&home);
        // Ids that are not session ids: traversal, patterns, a bare name, an absolute path.
        let hostile: Vec<String> = ["", ".", "..", "../..", "*", "../../outside/notes", "../settings", "-p", &format!("-p/{real}"), &format!("../projects/-p/{real}"),
            &format!("{real}.jsonl"), &format!("{real}/"), &real[..35], &real.to_uppercase().replace('-', "_"), &home.join("outside/notes").to_string_lossy()]
            .iter().map(|s| s.to_string()).collect();
        assert_eq!(remove_sessions(&config, &hostile), Removed::default());
        assert_eq!(listing(&home), before);

        // A transcript whose lines name slugs that would point elsewhere: none of them is followed.
        let evil = sid();
        let lines: String = ["../settings", "../../outside", "..", "keep.md", "plans/keep", "/etc/hosts", "KEEP", "", "a/../keep"].iter()
            .map(|s| format!("{{\"sessionId\":\"{evil}\",\"type\":\"user\",\"slug\":{}}}\n", serde_json::to_string(s).unwrap())).collect();
        // A slug on a line of some *other* session is not this session's plan either.
        let lines = format!("{lines}{{\"sessionId\":\"{}\",\"type\":\"user\",\"slug\":\"keep\"}}\n{{\"type\":\"user\",\"slug\":\"keep\"}}\nnot json \"slug\":\"keep\"\n", sid());
        write(&config.join(format!("projects/-p/{evil}.jsonl")), &lines);
        assert_eq!(remove_sessions(&config, &[evil.clone()]), Removed { transcripts: 1, plans: 0 });
        assert_eq!(listing(&home), before);

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            // A link named like a transcript, a linked project folder, and a linked plan are all left alone,
            // and so is whatever they point at.
            let (a, b, c) = (sid(), sid(), sid());
            symlink(home.join("outside/notes.jsonl"), config.join(format!("projects/-p/{a}.jsonl"))).unwrap();
            std::fs::create_dir_all(home.join("elsewhere")).unwrap();
            write(&home.join(format!("elsewhere/{b}.jsonl")), &transcript(&b, None));
            symlink(home.join("elsewhere"), config.join("projects/-linked")).unwrap();
            write(&config.join(format!("projects/-p/{c}.jsonl")), &transcript(&c, Some("linked-plan")));
            symlink(home.join("outside.md"), config.join("plans/linked-plan.md")).unwrap();
            // A folder named like a transcript is not a file.
            let d = sid();
            std::fs::create_dir_all(config.join(format!("projects/-p/{d}.jsonl"))).unwrap();
            write(&config.join(format!("projects/-p/{d}.jsonl/inner.txt")), "keep");
            assert_eq!(remove_sessions(&config, &[a.clone(), b.clone(), c.clone(), d.clone()]), Removed { transcripts: 1, plans: 0 });
            assert!(home.join("outside/notes.jsonl").exists() && home.join("outside.md").exists());
            assert!(home.join(format!("elsewhere/{b}.jsonl")).exists());
            assert!(std::fs::symlink_metadata(config.join(format!("projects/-p/{a}.jsonl"))).is_ok() && std::fs::symlink_metadata(config.join("plans/linked-plan.md")).is_ok());
            assert!(config.join(format!("projects/-p/{d}.jsonl/inner.txt")).exists());

            // `projects` or `plans` being links themselves: nothing is done through them.
            let (home2, config2) = self::home();
            std::fs::remove_dir(config2.join("projects")).unwrap();
            std::fs::remove_dir(config2.join("plans")).unwrap();
            let e = sid();
            write(&home2.join(format!("real-projects/-p/{e}.jsonl")), &transcript(&e, Some("some-plan")));
            write(&home2.join("real-plans/some-plan.md"), "keep");
            symlink(home2.join("real-projects"), config2.join("projects")).unwrap();
            symlink(home2.join("real-plans"), config2.join("plans")).unwrap();
            assert_eq!(remove_sessions(&config2, &[e.clone()]), Removed::default());
            assert!(home2.join(format!("real-projects/-p/{e}.jsonl")).exists() && home2.join("real-plans/some-plan.md").exists());
            std::fs::remove_dir_all(home2).unwrap();
        }
        // The one real transcript was never asked for and is still there.
        assert!(config.join(format!("projects/-p/{real}.jsonl")).exists());
        // No config folder at all is not an error.
        assert_eq!(remove_sessions(&home.join("no-such-folder"), &[real]), Removed::default());
        std::fs::remove_dir_all(home).unwrap();
    }
}
