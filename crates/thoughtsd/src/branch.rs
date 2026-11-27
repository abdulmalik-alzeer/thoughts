//! Editing a sent message, and branching a chat.
//!
//! - **Edit the last message.** The message and everything after it (its answer) leave the chat, and
//!   the new words are sent in their place (`api::chat_edit`). The assistant's session already holds
//!   the old exchange, so the chat leaves that session behind (it stays in `session_ids`, so deleting
//!   the chat still removes it) and the conversation before the message is passed on with the new
//!   one — whole when it fits, otherwise a summary and the last messages (`handoff.rs`).
//! - **Branch from here.** A new chat carries the conversation up to and including a message, and
//!   goes on apart from it; the original is not changed. Its first message continues the original's
//!   own session where that is exact: Claude Code started with `--resume <session> --fork-session`
//!   (when its help lists the flag), the original's session still ending at that very message.
//!   Everywhere else (an earlier point, Codex, whose `exec` lists no fork, an original that has moved
//!   on or was deleted) the conversation is passed on as text, like a chat moved to the other program.
//! - **Edit an earlier message** = a branch that ends just before it, with the new words sent there.
//!
//! What a branch carries: the messages (same ids, same times), copies of their attached files in the
//! branch's own folder (deleting one chat never takes the other's files), the folder group, the
//! options (never "bypass permissions": that was confirmed for the original). Not carried: what the
//! original cost (each run's cost is counted once, by the chat that ran it), its session ids, the
//! library changes of its answers (their Undo belongs to the original). What it carried counts as
//! the original's for Wrap: the branch is "saved" up to where it begins, and its own wrap saves only
//! what is said in it afterwards.

use crate::assistant::Program;
use crate::chats::{Chat, Msg};
use crate::handoff::How;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A message of the conversation itself (what the assistant's session holds), not a note about it.
pub fn said(m: &Msg) -> bool { matches!(m.role.as_str(), "user" | "assistant" | "tool" | "tool_result") }

/// The last message of the conversation itself.
pub fn last_said(c: &Chat) -> Option<&Msg> { c.messages.iter().rev().find(|m| said(m)) }

fn position(c: &Chat, msg: &str) -> anyhow::Result<usize> {
    c.messages.iter().position(|m| m.id == msg).ok_or_else(|| crate::tr::err("branch.no_message", &[]))
}

/// Where the last user message `msg` stands, to be edited in place. Only an ordinary chat's last
/// message of the user's can be: an earlier one is edited in a branch.
pub fn edit_at(c: &Chat, msg: &str) -> anyhow::Result<usize> {
    if c.kind != "chat" { return Err(crate::tr::err("branch.chat_only", &[])); }
    let i = position(c, msg)?;
    if c.messages[i].role != "user" { return Err(crate::tr::err("branch.no_message", &[])); }
    if c.messages[i + 1..].iter().any(|m| m.role == "user") { return Err(crate::tr::err("branch.not_last", &[])); }
    Ok(i)
}

/// The conversation is passed on with the next message (see `handoff::prefix`): whole when it fits
/// what the chat's model takes in, a summary otherwise. Nothing to pass on: nothing is set.
pub fn replay_next(c: &mut Chat) -> Option<How> {
    if !c.meta.is_object() { c.meta = json!({}); }
    if let Some(o) = c.meta.as_object_mut() { o.remove("handoff"); }
    if !c.messages.iter().any(|m| m.role == "user" || m.role == "assistant") { return None; }
    let p = crate::assistant::of_chat(&c.provider);
    let how = crate::handoff::choose(c.transcript(usize::MAX).len(), crate::handoff::window(p, c.options.model.as_deref()));
    c.meta["handoff"] = json!({"from": p.name(), "to": p.name(), "how": how.name(), "branch": true});
    Some(how)
}

/// What an edit took out of the chat, to put back if the new message cannot be sent.
pub struct Cut { pub at: usize, pub removed: Vec<Msg>, pub session: Option<String>, pub handoff: Value }

/// Take message `at` and everything after it out of the chat, leave the session behind and pass the
/// rest of the conversation on with the next message.
pub fn cut(c: &mut Chat, at: usize) -> Cut {
    let removed = c.messages.split_off(at.min(c.messages.len()));
    let session = c.session_id.take();
    if let Some(s) = &session { if !c.session_ids.contains(s) { c.session_ids.push(s.clone()); } }
    let handoff = c.meta.get("handoff").cloned().unwrap_or(Value::Null);
    replay_next(c);
    c.suggestion = None;
    Cut { at, removed, session, handoff }
}

/// Put back what `cut` took (the message could not be sent after all).
pub fn restore(c: &mut Chat, cut: Cut) {
    let at = cut.at.min(c.messages.len());
    let tail = c.messages.split_off(at);
    c.messages.extend(cut.removed);
    c.messages.extend(tail);
    c.session_id = cut.session;
    if !c.meta.is_object() { c.meta = json!({}); }
    if cut.handoff.is_null() { if let Some(o) = c.meta.as_object_mut() { o.remove("handoff"); } } else { c.meta["handoff"] = cut.handoff; }
}

/// The files of `paths` that sit in chat `chat`'s own folder, as plain names.
fn own_names(chat: &str, paths: &[String]) -> Vec<String> {
    let dir = files_dir(chat);
    paths.iter().filter_map(|p| {
        let p = Path::new(p);
        (p.parent() == Some(dir.as_path())).then(|| p.file_name()?.to_str().map(String::from)).flatten()
    }).collect()
}

/// Of the attachments the client asks to keep, those that really are the files of message `of` in
/// chat `chat` (a client never names any other file).
pub fn kept_files(chat: &str, of: &Msg, wanted: &[String]) -> Vec<String> {
    let dir = files_dir(chat);
    let named = |n: &str| wanted.iter().any(|w| w == n || Path::new(w).file_name().and_then(|x| x.to_str()) == Some(n));
    of.attachments.iter().filter(|p| {
        let p = Path::new(p);
        p.parent() == Some(dir.as_path()) && p.file_name().and_then(|n| n.to_str()).is_some_and(named)
    }).cloned().collect()
}

/// Copy `files` (attachments of chat `from`) into chat `to`'s own folder under the same names.
/// Returns old path → new path; a file that is no longer there is left out. All or nothing: on a
/// failure what was copied is removed again.
pub fn copy_files(from: &str, to: &str, files: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    let names = own_names(from, files);
    if names.is_empty() { return Ok(vec![]); }
    let (src, dst) = (crate::paths::attachments_dir().join(from), crate::paths::attachments_dir().join(to));
    let mut done: Vec<(String, String)> = vec![];
    let made: anyhow::Result<()> = (|| {
        crate::paths::create_private_dir(&dst)?;
        for n in &names {
            let s = src.join(n);
            // A plain file only, never a link out of the folder.
            if !std::fs::symlink_metadata(&s).is_ok_and(|m| m.is_file()) { continue; }
            let d = dst.join(n);
            if d.exists() { done.push((s.to_string_lossy().into(), d.to_string_lossy().into())); continue; }
            std::fs::copy(&s, &d)?;
            #[cfg(unix)]
            { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o600))?; }
            done.push((s.to_string_lossy().into(), d.to_string_lossy().into()));
        }
        Ok(())
    })();
    if let Err(e) = made {
        for (_, d) in &done { let _ = std::fs::remove_file(d); }
        let _ = std::fs::remove_dir(&dst);
        crate::log::error(format!("branch: the files of chat {from} could not be copied for chat {to}: {e}"));
        return Err(crate::tr::err("branch.files_failed", &[]));
    }
    Ok(done)
}

/// Did the original chat's assistant session end at `at`, so a copy of it is exactly the conversation
/// up to there? `help`: Claude Code's `--help` (the flag is only used where it is listed).
fn session_ends_at(orig: &Chat, at: &str, help: &str) -> Option<String> {
    let sid = orig.session_id.clone()?;
    let ok = orig.kind == "chat" && crate::assistant::of_chat(&orig.provider) == Program::Claude && !orig.running
        // A move to the other program that has not happened yet: the session is on its way out.
        && !orig.meta.get("handoff").is_some_and(Value::is_object)
        && last_said(orig).is_some_and(|m| m.id == at && m.role == "assistant")
        && crate::claude::supports(help, "--resume") && crate::claude::supports(help, "--fork-session");
    ok.then_some(sid)
}

/// The title of a branch of a chat called `title`.
pub fn title(title: &str, ar: bool) -> String {
    let ends = |k: bool| crate::tr::text_in(k, "branch.title", &[("title", "")]);
    if title.ends_with(ends(false).trim()) || title.ends_with(ends(true).trim()) { return title.to_string(); }
    crate::tr::text_in(ar, "branch.title", &[("title", title)])
}

/// How a branch's conversation reaches the assistant: its own copy of the original's session
/// ("fork"), or passed on as text ("replay", "summary"), or nothing to pass on ("none").
pub fn how_name(fork: bool, how: Option<How>) -> &'static str { if fork { "fork" } else { how.map_or("none", How::name) } }

/// A new chat with `orig`'s conversation up to and including message `at` (`before`: up to just
/// before it). The files are not copied here (see `copy_files`). `help`: Claude Code's `--help`.
pub fn make(orig: &Chat, at: &str, before: bool, help: &str, ar: bool) -> anyhow::Result<Chat> {
    if orig.kind != "chat" { return Err(crate::tr::err("branch.chat_only", &[])); }
    let i = position(orig, at)?;
    let end = if before { i } else { i + 1 };
    let now = chrono::Utc::now();
    let mut b = Chat {
        title: title(&orig.title, ar), project_id: orig.project_id.clone(), library: orig.library.clone(),
        provider: orig.provider.clone(), options: orig.options.clone(), created: now, updated: now,
        // What it carries belongs to the original's wrap; only what is said here from now on is the branch's.
        wrapped_at: Some(now),
        ..Default::default()
    };
    // The changes an answer made to the library, and the questions a run asked, belong to that run.
    b.messages = orig.messages[..end].iter().filter(|m| !matches!(m.role.as_str(), "changes" | "approval")).cloned().collect();
    // The point the branch starts from: the last message of the conversation it carries.
    let point = b.messages.iter().rev().find(|m| said(m)).map(|m| m.id.clone());
    let fork = if before { None } else { session_ends_at(orig, at, help) };
    let bypass = b.options.permission_mode.as_deref() == Some(crate::settings::BYPASS);
    if bypass { b.options.permission_mode = Some(crate::settings::DEFAULT_PERMISSION_MODE.into()); }
    let how = replay_next(&mut b);
    let at_text: String = orig.messages[i].text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
    b.meta["branch"] = json!({
        "from": orig.id, "fromTitle": orig.title, "at": orig.messages[i].id, "atRole": orig.messages[i].role, "atText": at_text,
        "before": before, "point": point, "forkSession": fork, "how": how_name(fork.is_some(), how), "created": now,
    });
    if bypass {
        let mut m = Msg::new("system", crate::tr::text_in(ar, "branch.bypass_off", &[]));
        m.tool = Some(json!({"code": "branchBypassOff"}));
        b.messages.push(m);
    }
    Ok(b)
}

/// Point the copied messages at the branch's own copies of their files.
pub fn repoint(b: &mut Chat, copied: &[(String, String)]) {
    for m in b.messages.iter_mut() {
        m.attachments = m.attachments.iter().filter_map(|p| copied.iter().find(|(o, _)| o == p).map(|(_, n)| n.clone())).collect();
    }
}

/// The original remembers its branches (newest last), so it can list them.
pub fn note_branch(orig: &mut Chat, b: &Chat) {
    if !orig.meta.is_object() { orig.meta = json!({}); }
    let mut list = orig.meta["branches"].as_array().cloned().unwrap_or_default();
    list.push(json!({"id": b.id, "at": b.meta["branch"]["at"], "created": b.created}));
    // A chat branched very often keeps the newest ones.
    let n = list.len();
    if n > 200 { list.drain(..n - 200); }
    orig.meta["branches"] = Value::Array(list);
}

/// The session a branch's next message continues from, as its own copy (`--fork-session`): only
/// while the branch has said nothing of its own, and the original's session still ends where the
/// branch begins. `None`: the conversation is passed on as text instead (`replay_next` set that up).
pub fn fork_from(orig: Option<&Chat>, b: &Chat, help: &str) -> Option<String> {
    let info = b.meta.get("branch")?;
    let sid = info["forkSession"].as_str()?;
    if b.session_id.is_some() || crate::assistant::of_chat(&b.provider) != Program::Claude { return None; }
    let point = info["point"].as_str()?;
    if last_said(b).map(|m| m.id.as_str()) != Some(point) { return None; }
    let found = session_ends_at(orig?, point, help)?;
    (found == sid).then_some(found)
}

#[cfg(test)]
thread_local! {
    /// Claude Code's `--help` as a test says it reads.
    pub static TEST_HELP: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Claude Code's `--help`, for the choice above.
pub async fn help(state: &crate::state::AppState) -> String {
    #[cfg(test)]
    if let Some(h) = TEST_HELP.with(|h| h.borrow().clone()) { return h; }
    crate::claude::help_text(state).await
}

/// Where a chat's attachments are kept.
pub fn files_dir(chat: &str) -> PathBuf { crate::paths::attachments_dir().join(chat) }

#[cfg(test)]
mod tests {
    use super::*;

    pub const HELP: &str = "Options:\n  -p, --print                 Print response and exit\n  -r, --resume [value]        Resume a conversation\n  --fork-session              When resuming, create a new session ID\n";
    const OLD_HELP: &str = "Options:\n  -p, --print                 Print response and exit\n  -r, --resume [value]        Resume a conversation\n";

    fn convo() -> Chat {
        let mut c = Chat { title: "Candle shop".into(), session_id: Some("s-1".into()), project_id: Some("p1".into()), cost_usd: 1.25, ..Default::default() };
        for (r, t) in [("user", "What should the shop sell?"), ("assistant", "Candles."), ("user", "And the price?"), ("tool", "Read prices.md"), ("tool_result", "…"), ("assistant", "Ten riyals."), ("changes", "")] {
            c.messages.push(Msg::new(r, t));
        }
        c
    }

    #[test]
    fn only_the_last_user_message_is_edited_in_place() {
        let c = convo();
        assert_eq!(edit_at(&c, &c.messages[2].id).unwrap(), 2);
        assert_eq!(crate::tr::classify_in(false, &edit_at(&c, &c.messages[0].id).unwrap_err()).code, "conflict");
        assert_eq!(crate::tr::classify_in(false, &edit_at(&c, &c.messages[1].id).unwrap_err()).code, "not_found", "an answer is not edited");
        assert_eq!(crate::tr::classify_in(false, &edit_at(&c, "nope").unwrap_err()).code, "not_found");
        let wrap = Chat { kind: "wrap".into(), ..convo() };
        assert_eq!(crate::tr::classify_in(false, &edit_at(&wrap, &wrap.messages[2].id).unwrap_err()).code, "forbidden");
    }

    #[test]
    fn an_edit_cuts_the_message_and_its_answer_and_leaves_the_session_behind() {
        let mut c = convo();
        let before = c.messages.clone();
        let k = cut(&mut c, 2);
        assert_eq!(c.messages.len(), 2);
        assert_eq!(k.removed.len(), 5);
        assert!(c.session_id.is_none());
        assert_eq!(c.all_sessions(), ["s-1"], "kept, so deleting the chat still removes it");
        assert_eq!(c.meta["handoff"]["how"], "replay");
        assert_eq!(c.meta["handoff"]["branch"], true);
        assert_eq!(c.cost_usd, 1.25, "what was spent stays spent");
        // Put back as it was when the new message could not be sent.
        restore(&mut c, k);
        assert_eq!(c.messages.iter().map(|m| &m.id).collect::<Vec<_>>(), before.iter().map(|m| &m.id).collect::<Vec<_>>());
        assert_eq!(c.session_id.as_deref(), Some("s-1"));
        assert!(c.meta.get("handoff").is_none());
        // The first message edited: nothing is left to pass on.
        let mut c = convo();
        cut(&mut c, 0);
        assert!(c.messages.is_empty() && c.meta.get("handoff").is_none());
    }

    #[test]
    fn a_branch_at_the_last_answer_continues_a_copy_of_the_session_where_the_help_lists_it() {
        let c = convo();
        let last = c.messages[5].id.clone();
        let b = make(&c, &last, false, HELP, false).unwrap();
        assert_eq!(b.title, "Candle shop (branch)");
        assert_eq!(make(&c, &last, false, HELP, true).unwrap().title, "Candle shop (فرع)");
        assert_eq!(title("Candle shop (branch)", true), "Candle shop (branch)", "never twice");
        assert_eq!(b.messages.len(), 6, "the library changes stay with the original");
        assert_eq!(b.messages[5].id, last, "same ids: the original message can be found from the branch");
        assert_eq!((b.project_id.as_deref(), b.session_id.as_deref(), b.cost_usd), (Some("p1"), None, 0.0));
        assert!(b.session_ids.is_empty(), "deleting the branch never deletes the original's session");
        assert_eq!(b.meta["branch"]["forkSession"], "s-1");
        assert_eq!(b.meta["branch"]["how"], "fork");
        assert_eq!(fork_from(Some(&c), &b, HELP).as_deref(), Some("s-1"));
        // An older assistant without the flag: the conversation is passed on as text.
        let old = make(&c, &last, false, OLD_HELP, false).unwrap();
        assert_eq!((old.meta["branch"]["forkSession"].is_null(), old.meta["branch"]["how"].as_str()), (true, Some("replay")));
        assert_eq!(fork_from(Some(&c), &b, OLD_HELP), None, "and checked again when it is sent");
        // The carried part is the original's to save: nothing in the branch waits for Wrap yet.
        assert!(!b.meta_only().is_unwrapped());
    }

    #[test]
    fn anywhere_else_the_conversation_is_passed_on_as_text() {
        let c = convo();
        // An earlier point: the session holds more than the branch.
        let b = make(&c, &c.messages[1].id, false, HELP, false).unwrap();
        assert_eq!(b.messages.len(), 2);
        assert!(b.meta["branch"]["forkSession"].is_null());
        assert_eq!(b.meta["handoff"]["how"], "replay");
        assert_eq!(fork_from(Some(&c), &b, HELP), None);
        // Editing an earlier message: up to just before it.
        let e = make(&c, &c.messages[2].id, true, HELP, false).unwrap();
        assert_eq!(e.messages.len(), 2);
        assert_eq!(e.meta["branch"]["before"], true);
        // Codex: its exec lists no fork.
        let x = Chat { provider: "codex".into(), ..convo() };
        let b = make(&x, &x.messages[5].id, false, HELP, false).unwrap();
        assert!(b.meta["branch"]["forkSession"].is_null() && b.meta["handoff"]["to"] == "codex");
        // The original moved on (or is running, or was deleted) before the branch's first message.
        let b = make(&c, &c.messages[5].id, false, HELP, false).unwrap();
        let mut on = c.clone();
        on.messages.push(Msg::new("user", "Anything else?"));
        on.messages.push(Msg::new("assistant", "Soap."));
        assert_eq!(fork_from(Some(&on), &b, HELP), None);
        assert_eq!(fork_from(Some(&Chat { running: true, ..c.clone() }), &b, HELP), None);
        assert_eq!(fork_from(None, &b, HELP), None);
        // Once the branch has a session (or a message) of its own, never again.
        assert_eq!(fork_from(Some(&c), &Chat { session_id: Some("s-2".into()), ..b.clone() }, HELP), None);
        let mut said_more = b.clone();
        said_more.messages.push(Msg::new("user", "Next"));
        assert_eq!(fork_from(Some(&c), &said_more, HELP), None);
    }

    #[test]
    fn bypass_is_never_carried_into_a_branch() {
        let mut c = convo();
        c.options.permission_mode = Some(crate::settings::BYPASS.into());
        let b = make(&c, &c.messages[5].id, false, HELP, false).unwrap();
        assert_eq!(b.options.permission_mode.as_deref(), Some(crate::settings::DEFAULT_PERMISSION_MODE));
        assert_eq!(b.messages.last().unwrap().tool.as_ref().unwrap()["code"], "branchBypassOff");
        // A note, not part of the conversation: the branch still starts from the same point.
        assert_eq!(fork_from(Some(&c), &b, HELP).as_deref(), Some("s-1"));
    }

    #[test]
    fn the_original_lists_its_branches() {
        let mut c = convo();
        let b = make(&c, &c.messages[1].id, false, HELP, false).unwrap();
        note_branch(&mut c, &b);
        note_branch(&mut c, &b);
        assert_eq!(c.meta["branches"].as_array().unwrap().len(), 2);
        assert_eq!(c.meta["branches"][0]["id"], b.id.as_str());
        assert_eq!(c.meta["branches"][0]["at"], c.messages[1].id.as_str());
        let wrap = Chat { kind: "customize".into(), ..convo() };
        assert!(make(&wrap, &wrap.messages[1].id, false, HELP, false).is_err());
    }
}
