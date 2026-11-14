//! The review cycle of the open change, and "Continue chatting".
//!
//! A change made in a customise chat goes round this cycle until the user keeps or discards it:
//!
//! ```text
//!   Running ──run ends──▶ Pending ──Continue chatting──▶ Continued ──next message starts──▶ Running …
//!                           │  ▲                            │
//!                           │  └────────Review again────────┘
//!                           └─ Keep / Discard: final (also from Continued; Discard also while Running is refused)
//! ```
//!
//! *Pending* is a version ready for the review (Try it, Keep, Discard, Continue chatting).
//! *Continued* means the user tried that version and went back to the chat to ask for changes: it is
//! not in use anywhere (a preview is the app's own and ends before this), its files stay in the
//! staging copy so the next run builds on them, and the review does not open by itself. Only the
//! flag `Meta::continued` is written, atomically, in the file that already describes the open change:
//! so the phase survives the app closing, travels with the change when it is set aside, and no step
//! here can leave anything half done. *Running* is read from the chat itself.

use super::{chat_running, load_meta, save_meta, staged, Dirs, Git, Meta};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase { Running, Pending, Continued }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event { Continue, ReviewAgain, RunStarted, RunEnded, Keep, Discard }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next { To(Phase), Final }

/// The whole cycle: where `event` takes a change that is in `from`, or `None` when it does not apply.
pub fn next(from: Phase, event: Event) -> Option<Next> {
    use {Event::*, Next::*, Phase::*};
    match (from, event) {
        (Pending, Continue) => Some(To(Continued)),
        (Continued, ReviewAgain) => Some(To(Pending)),
        (Pending | Continued, RunStarted) => Some(To(Running)),
        (Running, RunEnded) => Some(To(Pending)),
        (Pending | Continued, Keep | Discard) => Some(Final),
        _ => None,
    }
}

/// The phase of a change described by `m`, whose chat is `running` or not.
pub fn phase_of(m: &Meta, running: bool) -> Phase {
    if running { Phase::Running } else if m.continued { Phase::Continued } else { Phase::Pending }
}

/// Write the phase `event` leads to (callers have checked it applies). Nothing else changes.
fn record(d: &Dirs, mut m: Meta, to: Phase) -> anyhow::Result<()> {
    let continued = to == Phase::Continued;
    if m.continued == continued { return Ok(()); }
    m.continued = continued;
    save_meta(d, &m)
}

async fn open(g: Git<'_>, d: &Dirs) -> anyhow::Result<(Meta, Phase)> {
    let m = load_meta(d).filter(|_| staged(d)).ok_or_else(|| crate::tr::err("layer.no_pending", &[]))?;
    let running = chat_running(g, &m).await;
    let p = phase_of(&m, running);
    Ok((m, p))
}

/// "Continue chatting": set the open version aside for more conversation in the chat that made it.
/// Returns that chat's id. The files are not touched.
pub async fn continue_chat(g: Git<'_>, d: &Dirs) -> anyhow::Result<String> {
    let (m, p) = open(g, d).await?;
    if p == Phase::Running { return Err(crate::tr::err("layer.assistant_working", &[])); }
    let chat = m.chat_id.clone().filter(|id| !id.is_empty()).ok_or_else(|| crate::tr::err("layer.no_chat", &[]))?;
    if p == Phase::Continued { return Ok(chat); } // pressed twice: already there
    let Some(Next::To(to)) = next(p, Event::Continue) else { return Err(crate::tr::err("layer.no_pending", &[])) };
    record(d, m, to)?;
    Ok(chat)
}

/// "Review it again": a version set aside for more conversation goes back to the review as it is.
pub async fn review_again(g: Git<'_>, d: &Dirs) -> anyhow::Result<()> {
    let (m, p) = open(g, d).await?;
    if p != Phase::Continued { return Ok(()); }
    if let Some(Next::To(to)) = next(p, Event::ReviewAgain) { record(d, m, to)?; }
    Ok(())
}

/// A run of `chat_id` started on the open change: from here it is the run's (Running), and a run
/// cut off by the app closing comes back as an unfinished version for the review, not as Continued.
pub fn run_started(d: &Dirs, chat_id: &str) {
    let Some(m) = load_meta(d).filter(|m| m.continued && m.chat_id.as_deref() == Some(chat_id)) else { return };
    if let Err(e) = record(d, m, Phase::Running) { crate::log::warn(format!("layer: could not note that the assistant started on the change: {e}")); }
}

/// What the next run is told when the user went back to the chat from a version they tried.
pub fn prompt_note(d: &Dirs) -> &'static str {
    if load_meta(d).map_or(false, |m| m.continued) {
        "\n\nThe staging folder holds the version of this change the user just tried in the app. They chose to keep talking \
         instead of keeping it: they want changes. Build on those files rather than starting again (unless they ask to start \
         over), and rewrite SUMMARY.md so it describes the change as it will be after this turn."
    } else { "" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{apply, discard, pending, put_aside, open_draft, recreate, drafts};
    use crate::applayer::testutil;
    use Phase::*;

    #[test]
    fn the_cycle_goes_round_until_keep_or_discard() {
        use Event::*;
        assert_eq!(next(Pending, Continue), Some(Next::To(Continued)));
        assert_eq!(next(Continued, RunStarted), Some(Next::To(Running)));
        assert_eq!(next(Running, RunEnded), Some(Next::To(Pending)));
        assert_eq!(next(Continued, ReviewAgain), Some(Next::To(Pending)));
        for p in [Pending, Continued] { for e in [Keep, Discard] { assert_eq!(next(p, e), Some(Next::Final)); } }
        // Nothing is decided while the assistant is writing, and there is no going back from Running by hand.
        for e in [Continue, ReviewAgain, Keep, Discard, RunStarted] { assert_eq!(next(Running, e), None, "{e:?}"); }
        assert_eq!(next(Continued, Continue), None);
        assert_eq!(next(Pending, ReviewAgain), None);
        assert_eq!(next(Pending, RunEnded), None);
    }

    #[tokio::test]
    async fn continuing_keeps_the_version_to_build_on_and_survives_a_restart() {
        let (st, d) = testutil::layer("continue").await;
        recreate(st, &d, "customize", Some("chat-a")).await.unwrap();
        std::fs::write(d.staging.join("custom.css"), ":root { --radius: 2px }\n").unwrap();
        std::fs::write(d.staging.join("SUMMARY.md"), "Rounder corners\n").unwrap();
        let before = pending(st, &d, false).await;
        assert_eq!((&before["phase"], &before["continued"], &before["canContinue"]), (&json!("pending"), &json!(false), &json!(true)));
        let live = std::fs::read_to_string(d.layer.join("custom.css")).unwrap_or_default();

        assert_eq!(continue_chat(st, &d).await.unwrap(), "chat-a");
        assert_eq!(continue_chat(st, &d).await.unwrap(), "chat-a", "pressing it twice is harmless");
        // Written down: a fresh read (as after a restart) sees the same phase, and the same version.
        let after = pending(st, &d, false).await;
        assert_eq!((&after["phase"], &after["continued"]), (&json!("continued"), &json!(true)));
        assert_eq!(after["signature"], before["signature"], "the version is still there to build on");
        assert_eq!(std::fs::read_to_string(d.layer.join("custom.css")).unwrap_or_default(), live, "the app is as it was");
        assert!(prompt_note(&d).contains("just tried"));

        // The next message starts a run: Running, then a new version for the review.
        run_started(&d, "chat-other");
        assert!(load_meta(&d).unwrap().continued, "another chat's run changes nothing");
        // A message whose run never starts (signed out, say) leaves it waiting for the next one.
        super::super::run_start_result(st, &d, "chat-a", false).await;
        assert_eq!(pending(st, &d, false).await["phase"], "continued");
        super::super::run_start_result(st, &d, "chat-a", true).await;
        assert!(!load_meta(&d).unwrap().continued, "a run cut off now comes back as an unfinished version, not as continued");
        assert_eq!(prompt_note(&d), "");
        std::fs::write(d.staging.join("custom.css"), ":root { --radius: 3px }\n").unwrap();
        let mut m = load_meta(&d).unwrap();
        m.run = "done".into();
        save_meta(&d, &m).unwrap();
        let again = pending(st, &d, false).await;
        assert_eq!(again["phase"], "pending");
        assert_ne!(again["signature"], before["signature"]);
        // …and round again.
        continue_chat(st, &d).await.unwrap();
        review_again(st, &d).await.unwrap();
        assert_eq!(pending(st, &d, false).await["phase"], "pending");
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn a_continued_change_set_aside_comes_back_continued_and_keep_or_discard_end_it() {
        let (st, d) = testutil::layer("continue-aside").await;
        recreate(st, &d, "customize", Some("chat-a")).await.unwrap();
        std::fs::write(d.staging.join("custom.css"), ":root { --radius: 2px }\n").unwrap();
        continue_chat(st, &d).await.unwrap();
        // Another request sets it aside; opening it again brings it back as it was.
        let a = put_aside(st, &d).await.unwrap();
        drop(a);
        recreate(st, &d, "customize", Some("chat-b")).await.unwrap();
        let id = drafts(&d)[0].id.clone();
        open_draft(st, &d, &id).await.unwrap();
        assert_eq!(pending(st, &d, false).await["phase"], "continued");
        // Keep is final from here too (the user looked at it again and decided).
        apply(st, &d, false, None).await.unwrap();
        assert_eq!(pending(st, &d, false).await["exists"], false);
        assert!(continue_chat(st, &d).await.is_err());

        recreate(st, &d, "customize", Some("chat-c")).await.unwrap();
        std::fs::write(d.staging.join("custom.css"), ":root { --radius: 5px }\n").unwrap();
        continue_chat(st, &d).await.unwrap();
        discard(st, &d).await.unwrap();
        assert_eq!(pending(st, &d, false).await["exists"], false);
        assert!(load_meta(&d).is_none());
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[tokio::test]
    async fn only_a_change_made_in_a_chat_can_continue() {
        let (st, d) = testutil::layer("continue-nochat").await;
        assert!(continue_chat(st, &d).await.is_err(), "nothing open");
        recreate(st, &d, "import", None).await.unwrap();
        std::fs::write(d.staging.join("custom.css"), ":root { --radius: 2px }\n").unwrap();
        let p = pending(st, &d, false).await;
        assert_eq!(p["canContinue"], false);
        assert!(continue_chat(st, &d).await.is_err());
        assert_eq!(pending(st, &d, false).await["phase"], "pending");
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    use serde_json::json;
}
