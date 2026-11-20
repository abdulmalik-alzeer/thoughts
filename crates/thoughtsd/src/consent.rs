//! The user's yes, proven by the desktop shell — not merely claimed by whoever holds the token.
//!
//! Four things may only happen after the user agreed to a warning in the app window: switching a
//! chat to "bypass permissions", making that the default for new chats, trusting a folder, and
//! switching full access on (`settings::FullAccess`). The
//! app token (daemon.json) is a plain file any program running as the user may read, so a request
//! carrying it proves nothing about a person having said yes. Each of those requests must also carry
//! an *approval*: a short-lived, single-use code bound to that one action, made with a key only the
//! desktop shell and this daemon know.
//!
//! The key is made by the shell, in memory, and handed to the daemon it starts on its standard input
//! (`--consent-key-stdin`): never in daemon.json, an environment variable or an argument, and no
//! route returns it. The window gets one approval at a time from the shell (`approve_action`), right
//! after the user confirmed. A daemon started any other way (at login, by hand) has no key and
//! accepts no approval; the shell replaces it with one of its own when it is idle.
//!
//! An approval is `<issued ms>.<nonce>.<mac>`, the mac an HMAC-SHA256 over the action, its subject
//! (the chat id, the trust question id, nothing for the settings), the time and the nonce. It is
//! good for two minutes and once.

use crate::state::AppState;
use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// The argument that tells `--daemon` to read its key from standard input.
pub const KEY_FLAG: &str = "--consent-key-stdin";
/// How long an approval stays good.
pub const TTL_MS: u64 = 120_000;
/// How far ahead of this clock an approval may be dated (the two clocks are the same one).
const SKEW_MS: u64 = 5_000;

/// What an approval allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Switch one chat (subject: its id) to bypass permissions.
    ChatBypass,
    /// Make bypass permissions the default for chats started by hand (no subject).
    SettingsBypass,
    /// Trust the folder of one trust question (subject: the question id).
    Trust,
    /// Switch full access on (no subject): every guard the app adds yields (`settings::FullAccess`).
    FullAccess,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self { Action::ChatBypass => "chat-bypass", Action::SettingsBypass => "settings-bypass", Action::Trust => "trust", Action::FullAccess => "full-access" }
    }
    pub fn from_name(s: &str) -> Option<Action> {
        [Action::ChatBypass, Action::SettingsBypass, Action::Trust, Action::FullAccess].into_iter().find(|a| a.name() == s)
    }
    /// Is `subject` the kind of thing this action names? Checked by the shell before it signs.
    pub fn subject_ok(self, subject: &str) -> bool {
        match self {
            Action::ChatBypass => crate::chats::valid_id(subject),
            Action::SettingsBypass | Action::FullAccess => subject.is_empty(),
            Action::Trust => (16..=64).contains(&subject.len()) && subject.bytes().all(|b| b.is_ascii_hexdigit()),
        }
    }
}

pub type Key = [u8; 32];

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A fresh random key (the shell makes one per launch).
pub fn new_key() -> Key {
    use rand::RngCore;
    let mut k = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut k);
    k
}

pub fn key_hex(k: &Key) -> String { hex(k) }

fn parse_key(s: &str) -> Option<Key> {
    let s = s.trim();
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() { *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?; }
    Some(k)
}

fn message(action: Action, subject: &str, issued: u64, nonce: &str) -> String {
    format!("thoughts-approval-v1\n{}\n{subject}\n{issued}\n{nonce}", action.name())
}

fn valid_nonce(n: &str) -> bool { n.len() == 32 && n.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) }

/// An approval for `action` on `subject`, dated `issued` (ms since 1970).
pub fn mint(key: &Key, action: Action, subject: &str, issued: u64) -> String {
    let nonce = crate::hello::new_nonce();
    format!("{issued}.{nonce}.{}", hex(&crate::hello::hmac_sha256(key, message(action, subject, issued, &nonce).as_bytes())))
}

/// The shell's check that a daemon holds its key, without the key or an approval ever crossing:
/// a different prefix, so no answer here is an approval or the other way round.
pub fn proof(key: &Key, nonce: &str) -> String {
    hex(&crate::hello::hmac_sha256(key, format!("thoughts-consent-proof-v1:{nonce}").as_bytes()))
}

/// Does the `/api/consent/proof` answer `body` show that the daemon holds `key`?
pub fn proof_ok(body: &str, key: &Key, nonce: &str) -> bool {
    serde_json::from_str::<Value>(body.trim()).ok()
        .and_then(|v| v["proof"].as_str().map(|p| same(p.as_bytes(), proof(key, nonce).as_bytes())))
        .unwrap_or(false)
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Approvals already used, with when they would have run out anyway.
pub type Used = HashMap<String, u64>;

/// Is `approval` a good, unused approval for exactly `action` on `subject` at `now`? A good one is
/// spent: it never passes again.
pub fn verify_with(key: Option<&Key>, used: &mut Used, approval: Option<&str>, action: Action, subject: &str, now: u64) -> bool {
    let (Some(key), Some(approval)) = (key, approval) else { return false };
    let mut parts = approval.splitn(3, '.');
    let (Some(issued), Some(nonce), Some(mac)) = (parts.next(), parts.next(), parts.next()) else { return false };
    let Ok(issued) = issued.parse::<u64>() else { return false };
    if !valid_nonce(nonce) || issued > now + SKEW_MS || now > issued.saturating_add(TTL_MS) { return false; }
    let want = hex(&crate::hello::hmac_sha256(key, message(action, subject, issued, nonce).as_bytes()));
    if !same(mac.as_bytes(), want.as_bytes()) { return false; }
    used.retain(|_, until| *until >= now);
    if used.contains_key(nonce) { return false; }
    used.insert(nonce.to_string(), issued.saturating_add(TTL_MS));
    true
}

fn key() -> &'static OnceLock<Key> {
    static K: OnceLock<Key> = OnceLock::new();
    &K
}

fn used() -> &'static Mutex<Used> {
    static U: OnceLock<Mutex<Used>> = OnceLock::new();
    U.get_or_init(Default::default)
}

/// What a chat-bypass approval is signed over: the chat and the assistant program the shell's dialog
/// named. An approval made while the chat was a Claude chat no longer passes once it is a Codex one.
/// (The chat id is checked by the shell, `Action::subject_ok`, so it holds no line break.)
pub fn chat_subject(id: &str, provider: &str) -> String {
    format!("{id}\nprovider:{provider}")
}

/// What a trust approval is signed over: the question and the folder the shell's dialog named, as
/// `GET /api/library/trust?id=` gives it.
pub fn trust_subject(id: &str, path: &str) -> String {
    format!("{id}\npath:{path}")
}

/// The folder of trust question `id` the way the shell is shown it and the approval names it.
pub fn trust_path_shown(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The daemon's check: a good approval from the shell for exactly this action, used up by passing.
pub fn verify(approval: Option<&str>, action: Action, subject: &str) -> bool {
    let mut u = used().lock().unwrap_or_else(|e| e.into_inner());
    verify_with(key().get(), &mut u, approval, action, subject, now_ms())
}

/// At daemon start: when the shell asked for it (`KEY_FLAG`), take the key from standard input —
/// one line, which the shell writes and closes at once. Anything else leaves the daemon without one.
pub fn take_key_if_asked() {
    if !std::env::args().any(|a| a == KEY_FLAG) { return; }
    let mut line = String::new();
    let read = std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line);
    match read.ok().and_then(|_| parse_key(&line)) {
        Some(k) => { let _ = key().set(k); }
        None => crate::log::warn("the app's confirmation key did not arrive; bypass and folder trust cannot be confirmed"),
    }
}

/// Tests: the daemon's key, the same fixed one in every test.
#[cfg(test)]
pub fn test_key() -> &'static Key {
    key().get_or_init(|| [7u8; 32])
}

/// Tests: an approval the shell would give right now.
#[cfg(test)]
pub fn test_approval(action: Action, subject: &str) -> String {
    mint(test_key(), action, subject, now_ms())
}

/// Tests: the shell's yes to switching Claude chat `id` to bypass (`Chat::default()` is a Claude chat).
#[cfg(test)]
pub fn test_chat_approval(id: &str) -> String {
    test_approval(Action::ChatBypass, &chat_subject(id, "claude"))
}

/// Tests: the shell's yes to trust question `id`, naming the folder the daemon asked about.
#[cfg(test)]
pub fn test_trust_approval(id: &str) -> String {
    let path = crate::trust::asked_about(id).map(|p| trust_path_shown(&p)).unwrap_or_default();
    test_approval(Action::Trust, &trust_subject(id, &path))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/consent/proof", get(proof_route))
}

/// `GET /api/consent/proof?nonce=` → `{proof}`, null without a key: lets the shell tell whether this
/// daemon is one it started.
async fn proof_route(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let nonce = q.get("nonce").map(String::as_str).unwrap_or_default();
    let p = key().get().filter(|_| crate::hello::valid_nonce(nonce)).map(|k| proof(k, nonce));
    Json(json!({"proof": p}))
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: Key = [1u8; 32];

    #[test]
    fn only_a_fresh_unused_approval_for_exactly_that_action_passes() {
        let now = now_ms();
        let mut used = Used::new();
        let good = mint(&K, Action::ChatBypass, "abc", now);
        // No approval, no key, a made-up one, another key's.
        assert!(!verify_with(Some(&K), &mut used, None, Action::ChatBypass, "abc", now));
        assert!(!verify_with(None, &mut used, Some(&good), Action::ChatBypass, "abc", now));
        assert!(!verify_with(Some(&K), &mut used, Some("1.00000000000000000000000000000000.00"), Action::ChatBypass, "abc", now));
        assert!(!verify_with(Some(&K), &mut used, Some(&mint(&[2u8; 32], Action::ChatBypass, "abc", now)), Action::ChatBypass, "abc", now));
        // Bound to the action and its subject.
        assert!(!verify_with(Some(&K), &mut used, Some(&good), Action::ChatBypass, "other", now));
        assert!(!verify_with(Some(&K), &mut used, Some(&good), Action::Trust, "abc", now));
        assert!(!verify_with(Some(&K), &mut used, Some(&good), Action::SettingsBypass, "", now));
        // Tampered time: the mac no longer matches.
        let (_, rest) = good.split_once('.').unwrap();
        assert!(!verify_with(Some(&K), &mut used, Some(&format!("{}.{rest}", now + 1)), Action::ChatBypass, "abc", now));
        // The real one passes once, then never again.
        assert!(verify_with(Some(&K), &mut used, Some(&good), Action::ChatBypass, "abc", now + 1000));
        assert!(!verify_with(Some(&K), &mut used, Some(&good), Action::ChatBypass, "abc", now + 2000));
    }

    #[test]
    fn an_approval_is_bound_to_what_the_dialog_named() {
        let now = now_ms();
        let mut used = Used::new();
        let id = "0b0f6c2e-1d2a-4f8e-9c1e-5c9f2d7e8a10";
        let claude = mint(&K, Action::ChatBypass, &chat_subject(id, "claude"), now);
        // The chat became a Codex chat after the dialog said Claude: refused.
        assert!(!verify_with(Some(&K), &mut used, Some(&claude), Action::ChatBypass, &chat_subject(id, "codex"), now));
        assert!(!verify_with(Some(&K), &mut used, Some(&claude), Action::ChatBypass, id, now));
        assert!(verify_with(Some(&K), &mut used, Some(&claude), Action::ChatBypass, &chat_subject(id, "claude"), now));
        // A trust approval for one folder does not trust another under the same question id.
        let q = "0123456789abcdef0123456789abcdef";
        let shown = mint(&K, Action::Trust, &trust_subject(q, "/Users/me/Thoughts"), now);
        assert!(!verify_with(Some(&K), &mut used, Some(&shown), Action::Trust, &trust_subject(q, "/Users/me/Thoughts-evil"), now));
        assert!(!verify_with(Some(&K), &mut used, Some(&shown), Action::Trust, q, now));
        assert!(verify_with(Some(&K), &mut used, Some(&shown), Action::Trust, &trust_subject(q, "/Users/me/Thoughts"), now));
    }

    #[test]
    fn approvals_run_out() {
        let now = now_ms();
        let mut used = Used::new();
        let old = mint(&K, Action::SettingsBypass, "", now - TTL_MS - 1);
        assert!(!verify_with(Some(&K), &mut used, Some(&old), Action::SettingsBypass, "", now));
        let ahead = mint(&K, Action::SettingsBypass, "", now + 60_000);
        assert!(!verify_with(Some(&K), &mut used, Some(&ahead), Action::SettingsBypass, "", now));
        let fine = mint(&K, Action::SettingsBypass, "", now - TTL_MS + 1000);
        assert!(verify_with(Some(&K), &mut used, Some(&fine), Action::SettingsBypass, "", now));
        // A spent approval is forgotten only once it would have run out anyway.
        assert!(used.len() == 1);
        let later = mint(&K, Action::SettingsBypass, "", now + TTL_MS);
        assert!(verify_with(Some(&K), &mut used, Some(&later), Action::SettingsBypass, "", now + TTL_MS));
        assert!(used.len() == 1);
    }

    #[test]
    fn the_proof_shows_the_key_without_being_an_approval() {
        let n = crate::hello::new_nonce();
        let body = json!({"proof": proof(&K, &n)}).to_string();
        assert!(proof_ok(&body, &K, &n));
        assert!(!proof_ok(&body, &[2u8; 32], &n));
        assert!(!proof_ok(r#"{"proof":null}"#, &K, &n));
        assert!(!proof_ok("not json", &K, &n));
        assert_eq!(parse_key(&key_hex(&K)), Some(K));
        assert_eq!(parse_key("short"), None);
        assert!(Action::Trust.subject_ok("0123456789abcdef0123456789abcdef"));
        assert!(!Action::Trust.subject_ok("../x"));
        assert!(Action::SettingsBypass.subject_ok("") && !Action::SettingsBypass.subject_ok("x"));
        assert_eq!(Action::from_name("trust"), Some(Action::Trust));
        assert_eq!(Action::from_name("anything"), None);
        assert_eq!(Action::from_name("full-access"), Some(Action::FullAccess));
        assert!(Action::FullAccess.subject_ok("") && !Action::FullAccess.subject_ok("x"));
    }
}
