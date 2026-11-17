//! A run is never started from an empty or failed `claude --help`: it is asked again, then the
//! last good help stands in, else the run is refused (`run_help`).

use super::*;

const GOOD: &str = "Options:\n\
  --disable-slash-commands              Disable all skills\n\
  --disallowedTools, --disallowed-tools <tools...>\n      Comma or space-separated list of tool names to deny\n\
  --max-budget-usd <amount>             Maximum dollar amount to spend on API\n\
  --permission-prompts <target>         Who answers permission prompts\n\
  -p, --print                           Print response and exit\n\
  --restricted                          Restricted mode\n\
  --safe-mode                           Start with all customizations disabled\n\
  --setting-sources <sources>           Comma-separated list of setting sources\n\
  --strict-mcp-config                   Only use MCP servers from --mcp-config\n";

/// An `ask` that answers each call from `answers` in turn ("" once they run out), counting calls.
fn asker(answers: Vec<&'static str>, calls: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> impl FnMut() -> std::future::Ready<String> {
    move || {
        let n = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::future::ready(answers.get(n).copied().unwrap_or("").to_string())
    }
}

fn counter() -> std::sync::Arc<std::sync::atomic::AtomicUsize> { Default::default() }

#[test]
fn only_a_real_help_counts() {
    assert!(usable_help(GOOD));
    for bad in ["", "   \n", "error: unexpected end of input", "Updating Claude Code…\n"] {
        assert!(!usable_help(bad), "{bad:?}");
    }
}

#[tokio::test]
async fn empty_help_twice_and_nothing_kept_refuses() {
    let calls = counter();
    assert_eq!(resolve_help(asker(vec!["", ""], calls.clone()), None, || None).await, None);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2, "asked once more before giving up, no more");
}

#[tokio::test]
async fn an_empty_answer_is_asked_again() {
    let calls = counter();
    assert_eq!(resolve_help(asker(vec!["", GOOD], calls.clone()), None, || None).await, Some((GOOD.to_string(), true)));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    // A good first answer is not asked twice.
    let calls = counter();
    assert_eq!(resolve_help(asker(vec![GOOD], calls.clone()), None, || panic!("not read")).await, Some((GOOD.to_string(), true)));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn empty_help_uses_the_last_good_one() {
    // The one this daemon read before comes first.
    assert_eq!(resolve_help(asker(vec![], counter()), Some(GOOD.into()), || panic!("not read")).await, Some((GOOD.to_string(), false)));
    // Else the one kept on disk.
    assert_eq!(resolve_help(asker(vec![], counter()), None, || Some(GOOD.into())).await, Some((GOOD.to_string(), false)));
    // A remembered answer that is itself unusable is no stand-in.
    assert_eq!(resolve_help(asker(vec![], counter()), Some(String::new()), || None).await, None);
}

#[test]
fn the_kept_help_survives_on_disk() {
    let dir = std::env::temp_dir().join(format!("thoughts-help-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("assistant-help.json");
    assert_eq!(read_saved_help(&file), None);
    write_saved_help(&file, Path::new("/usr/local/bin/claude"), GOOD);
    assert_eq!(read_saved_help(&file).as_deref(), Some(GOOD));
    // A broken or unusable file is no stand-in.
    std::fs::write(&file, "{\"help\": \"\"}").unwrap();
    assert_eq!(read_saved_help(&file), None);
    std::fs::write(&file, "not json").unwrap();
    assert_eq!(read_saved_help(&file), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn good_help_gives_every_safety_flag() {
    let p = profile("workflow", "acceptEdits", false);
    let a = profile_flags(&p, GOOD);
    for f in ["--permission-prompts", "--setting-sources", "--restricted", "--strict-mcp-config"] {
        assert!(a.iter().any(|x| x == f), "{f} in {a:?}");
    }
    let g = one_shot_guard(GOOD);
    for f in ["--safe-mode", "--strict-mcp-config", "--disable-slash-commands", "--disallowedTools", "--permission-prompts"] {
        assert!(g.iter().any(|x| x == f), "{f} in {g:?}");
    }
}

#[test]
fn a_spending_limit_that_cannot_be_held_refuses_the_run() {
    assert_eq!(budget_arg(Some(1.5), GOOD).unwrap().as_deref(), Some("1.50"));
    assert_eq!(budget_arg(None, GOOD).unwrap(), None);
    assert_eq!(budget_arg(None, "").unwrap(), None, "no limit asked: nothing to hold");
    let old = "Options:\n  -p, --print  Print response and exit\n";
    let e = budget_arg(Some(2.0), old).unwrap_err();
    assert!(e.to_string().contains("spending limit"), "{e}");
    assert!(budget_arg(Some(2.0), "").is_err());
}
