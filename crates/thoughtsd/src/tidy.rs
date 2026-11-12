//! Keeping the chat list tidy: bulk delete, the optional automatic clean-up of old chats, and the
//! removal of chats nobody ever wrote in.
//!
//! The clean-up is off until the person switches it on (`Settings::chat_retention_days`). Whatever
//! the setting, it never removes a chat that is pinned, running, cut off and not yet seen, or not
//! yet saved into the library (one the next Wrap would still save), nor a Wrap or Reorganize that
//! is waiting for the person's decision.

use crate::chats::ChatMeta;
use crate::state::AppState;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::sync::Arc;

/// The shortest time the clean-up may be set to.
pub const MIN_DAYS: u32 = 7;
/// The longest (ten years): a number beyond this is a typing mistake.
pub const MAX_DAYS: u32 = 3650;
/// The choices the Chats page offers.
pub const CHOICES: [u32; 4] = [30, 90, 180, 365];
/// A chat nobody has written in is dropped once it has been lying around this long.
const EMPTY_AFTER_HOURS: i64 = 24;

/// Has this Wrap / Reorganize reached an end, so that nobody is waiting on it?
fn finished(c: &ChatMeta) -> bool {
    match c.kind.as_str() {
        "wrap" | "reorganize" => matches!(c.meta["phase"].as_str(),
            Some("done" | "failed" | "undone" | "superseded" | "discarded" | "closed" | "merged" | "preview_failed")),
        _ => true,
    }
}

/// Could the automatic clean-up remove this chat, if it is old enough?
pub fn removable(c: &ChatMeta) -> bool {
    !c.pinned && !c.running && !c.interrupted && !c.is_unwrapped() && finished(c)
}

/// A chat nobody has written in (one made by New chat and left).
pub fn is_empty_chat(c: &ChatMeta) -> bool { c.kind == "chat" && c.message_count == 0 && !c.pinned && !c.running }

/// What a clean-up after `days` days would do right now.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Preview {
    /// Chats of the person's own that would be removed.
    pub chats: usize,
    /// Wraps, setup chats, automatic-task chats… that would be removed.
    pub system: usize,
    /// Old chats that stay because they are not saved into the library yet.
    pub unsaved: usize,
    /// Old chats that stay because they are pinned.
    pub pinned: usize,
}

/// The ids and the totals of what a clean-up after `days` days would remove at `now`.
pub fn plan(all: &[ChatMeta], days: u32, now: DateTime<Utc>) -> (Vec<String>, Preview) {
    let mut p = Preview::default();
    let mut ids = vec![];
    if days == 0 { return (ids, p); }
    let limit = now - Duration::days(days as i64);
    for c in all.iter().filter(|c| c.updated < limit && !c.running) {
        if c.pinned { p.pinned += 1; }
        else if c.is_unwrapped() { p.unsaved += 1; }
        else if removable(c) {
            if c.kind == "chat" { p.chats += 1 } else { p.system += 1 }
            ids.push(c.id.clone());
        }
    }
    (ids, p)
}

/// Delete these chats (each with its files and what the assistant kept of it). A chat that is
/// running is left alone. Returns (deleted, skipped).
pub async fn delete_chats(st: &AppState, ids: &[String]) -> (usize, usize) {
    let (mut done, mut skipped) = (0, 0);
    let mut gone = vec![];
    for id in ids {
        match st.chats.meta(id).await {
            Some(m) if m.running => skipped += 1,
            Some(_) => if crate::api::remove_chat(st, id).await { done += 1; gone.push(id.clone()) } else { skipped += 1 },
            None => skipped += 1,
        }
    }
    if !gone.is_empty() { st.emit(json!({"type": "chat.removed", "chatIds": gone})); }
    (done, skipped)
}

/// One pass: the automatic clean-up when it is on, and the chats nobody wrote in. Returns how many chats went.
pub async fn sweep(st: &AppState) -> usize {
    let now = Utc::now();
    let all = st.chats.list().await;
    let days = st.settings.read().await.chat_retention_days;
    let (mut ids, _) = plan(&all, days, now);
    let stale = now - Duration::hours(EMPTY_AFTER_HOURS);
    let empty: Vec<String> = all.iter().filter(|c| is_empty_chat(c) && c.updated < stale && !ids.contains(&c.id)).map(|c| c.id.clone()).collect();
    ids.extend(empty);
    if ids.is_empty() { return 0; }
    let (done, _) = delete_chats(st, &ids).await;
    if done > 0 { crate::log::info(format!("tidy: {done} old or empty chat(s) removed")); }
    done
}

/// Run `sweep` shortly after start and then every hour.
pub fn start(st: Arc<AppState>) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(90)).await;
        loop {
            sweep(&st).await;
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });
}

/// The state of the setting plus what a clean-up after `days` days (default: the current setting) would do now.
pub async fn describe(st: &AppState, days: Option<u32>) -> Value {
    let current = st.settings.read().await.chat_retention_days;
    let asked = days.unwrap_or(current);
    let (ids, p) = plan(&st.chats.list().await, asked, Utc::now());
    json!({"days": current, "choices": CHOICES, "preview": {"days": asked, "total": ids.len(), "chats": p.chats, "system": p.system, "unsaved": p.unsaved, "pinned": p.pinned}})
}

/// Switch the clean-up on (a number of days) or off (0), and run it at once. Returns how many chats went.
pub async fn set_retention(st: &AppState, days: u32) -> anyhow::Result<usize> {
    if days != 0 && !(MIN_DAYS..=MAX_DAYS).contains(&days) { return Err(crate::tr::err("chat.retention_range", &[("min", &MIN_DAYS.to_string())])); }
    {
        let mut s = st.settings.write().await;
        let before = s.chat_retention_days;
        s.chat_retention_days = days;
        if let Err(e) = s.save() { s.chat_retention_days = before; return Err(e.into()); }
    }
    Ok(if days > 0 { sweep(st).await } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chats::{Chat, Msg};

    fn old(days: i64, mut c: Chat) -> Chat {
        c.updated = Utc::now() - Duration::days(days);
        c
    }
    fn said(text: &str) -> Chat { let mut c = Chat::default(); c.messages.push(Msg::new("user", text)); c.messages.push(Msg::new("assistant", "ok")); c }
    fn saved(c: Chat) -> Chat { let mut c = old(100, c); c.wrapped_at = Some(Utc::now() - Duration::days(99)); c }
    async fn put(st: &AppState, c: Chat) -> String { let id = c.id.clone(); st.chats.insert(c).await; id }
    fn kept(st: &AppState, id: &str) -> bool { st.chats.chats.try_read().unwrap().contains_key(id) }

    #[tokio::test]
    async fn the_clean_up_leaves_what_is_not_safely_in_the_library() {
        let st = AppState::for_test();
        // Goes: a saved chat, an old finished wrap, an old automatic-task chat.
        let saved_chat = put(&st, saved(said("old and saved"))).await;
        let mut w = old(100, Chat { kind: "wrap".into(), ..Default::default() }); w.meta = json!({"phase": "done"});
        let done_wrap = put(&st, w).await;
        let task = put(&st, old(100, Chat { kind: "workflow".into(), ..Default::default() })).await;
        // Stays: never saved, pinned, running, cut off, a wrap awaiting approval, a wrap applying, recent, and a chat changed after its wrap.
        let unsaved = put(&st, old(100, said("never saved"))).await;
        let mut p = saved(said("pinned")); p.pinned = true;
        let pinned = put(&st, p).await;
        let mut r = saved(said("running")); r.running = true;
        let running = put(&st, r).await;
        let mut i = saved(said("cut off")); i.interrupted = true;
        let cut = put(&st, i).await;
        let mut w = old(100, Chat { kind: "wrap".into(), ..Default::default() }); w.meta = json!({"phase": "preview", "previewOk": true});
        let waiting = put(&st, w).await;
        let mut w = old(100, Chat { kind: "wrap".into(), ..Default::default() }); w.meta = json!({"phase": "applying"});
        let applying = put(&st, w).await;
        let mut w = old(100, Chat { kind: "reorganize".into(), ..Default::default() }); w.meta = json!({"phase": "review"});
        let review = put(&st, w).await;
        let recent = put(&st, said("recent")).await;
        let mut later = saved(said("talked on after the wrap"));
        later.updated = Utc::now() - Duration::days(95); later.wrapped_at = Some(Utc::now() - Duration::days(99));
        let later = put(&st, later).await;

        // Off by default: nothing happens, however old.
        assert_eq!(st.settings.read().await.chat_retention_days, 0);
        assert_eq!(sweep(&st).await, 0);
        assert!(kept(&st, &saved_chat) && kept(&st, &done_wrap) && kept(&st, &task));

        // What it says it will do is what it does.
        let v = describe(&st, Some(30)).await;
        assert_eq!((v["preview"]["chats"].as_u64(), v["preview"]["system"].as_u64(), v["preview"]["total"].as_u64()), (Some(1), Some(2), Some(3)));
        assert_eq!((v["preview"]["unsaved"].as_u64(), v["preview"]["pinned"].as_u64()), (Some(2), Some(1)));
        assert_eq!(v["days"], 0, "looking does not switch it on");

        assert_eq!(set_retention(&st, 30).await.unwrap(), 3);
        for gone in [&saved_chat, &done_wrap, &task] { assert!(!kept(&st, gone)); }
        for stays in [&unsaved, &pinned, &running, &cut, &waiting, &applying, &review, &recent, &later] { assert!(kept(&st, stays)); }
        assert_eq!(st.settings.read().await.chat_retention_days, 30);
        // Pinning protects, unpinning does not rescue an old chat from the next pass.
        st.chats.update(&pinned, |c| c.pinned = false).await;
        assert_eq!(sweep(&st).await, 1);
        assert!(!kept(&st, &pinned));
        // Out-of-range choices are refused and change nothing.
        for bad in [1, 6, 3651, 100_000] { assert!(set_retention(&st, bad).await.is_err(), "{bad}"); }
        assert_eq!(st.settings.read().await.chat_retention_days, 30);
        assert_eq!(set_retention(&st, 0).await.unwrap(), 0);
        assert_eq!(st.settings.read().await.chat_retention_days, 0);
    }

    #[tokio::test]
    async fn bulk_delete_skips_running_chats_and_takes_the_assistant_s_files_with_it() {
        let st = AppState::for_test();
        let (a, b) = (uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string());
        let mut c = said("one"); c.session_id = Some(a.clone());
        let one = put(&st, c).await;
        let mut c = said("two"); c.running = true;
        let two = put(&st, c).await;
        let config = crate::traces::config_dir().unwrap();
        let transcript = config.join(format!("projects/-p/{a}.jsonl"));
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(&transcript, "{}\n").unwrap();
        let other = config.join(format!("projects/-p/{b}.jsonl"));
        std::fs::write(&other, "{}\n").unwrap();
        let (done, skipped) = delete_chats(&st, &[one.clone(), two.clone(), "../x".into(), uuid::Uuid::new_v4().to_string()]).await;
        assert_eq!((done, skipped), (1, 3));
        assert!(!kept(&st, &one) && kept(&st, &two));
        assert!(!transcript.exists() && other.exists());
        std::fs::remove_file(other).unwrap();
    }

    #[tokio::test]
    async fn chats_nobody_wrote_in_are_dropped_after_a_day_but_never_pinned_or_used_ones() {
        let st = AppState::for_test();
        let fresh = put(&st, Chat::default()).await;
        let stale = put(&st, old(2, Chat::default())).await;
        let mut p = old(2, Chat::default()); p.pinned = true;
        let pinned = put(&st, p).await;
        let used = put(&st, old(2, said("hello"))).await;
        let sys = put(&st, old(2, Chat { kind: "setup".into(), ..Default::default() })).await;
        assert_eq!(sweep(&st).await, 1);
        assert!(!kept(&st, &stale));
        for k in [&fresh, &pinned, &used, &sys] { assert!(kept(&st, k)); }
    }
}
