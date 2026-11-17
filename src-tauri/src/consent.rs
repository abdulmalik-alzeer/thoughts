// The shell's half of `thoughtsd::consent`: the user's yes to bypassing permissions or to trusting a
// folder, proven to the daemon with a key the window never sees.
//
// The key is made here, once per launch, kept in memory and handed only to a daemon this shell
// starts (on its standard input, see `spawn_daemon`). A daemon this shell did not start (one started
// at login, or left running by an earlier launch) has no key: `ensure_daemon` replaces it with one
// of its own as soon as it is idle. The window asks for one approval at a time, right after the user
// confirmed the warning, with `approve_action`; the approval it gets back is good for one action,
// once, for two minutes.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use thoughtsd::consent::{self, Action, Key};
use thoughtsd::paths::DaemonInfo;

/// This launch's key.
pub fn key() -> &'static Key {
    static K: OnceLock<Key> = OnceLock::new();
    K.get_or_init(consent::new_key)
}

/// The daemon (pid, token) last proven to hold this launch's key.
static PROVEN: Mutex<Option<(u32, String)>> = Mutex::new(None);

/// Does the daemon `i` hold this launch's key? Asked with a fresh challenge; the key never crosses.
pub fn holds_key(i: &DaemonInfo) -> bool {
    let mut proven = PROVEN.lock().unwrap_or_else(|e| e.into_inner());
    if proven.as_ref().is_some_and(|(pid, token)| *pid == i.pid && *token == i.token) {
        return true;
    }
    let nonce = thoughtsd::hello::new_nonce();
    let ok = ureq::get(&format!("http://127.0.0.1:{}/api/consent/proof?nonce={nonce}", i.port))
        .set("Authorization", &format!("Bearer {}", i.token))
        .timeout(Duration::from_secs(2))
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .is_some_and(|body| consent::proof_ok(&body, key(), &nonce));
    if ok {
        *proven = Some((i.pid, i.token.clone()));
    }
    ok
}

fn arabic() -> bool {
    thoughtsd::settings::Settings::load().language.starts_with("ar")
}

/// `approve_action(action, subject)` → an approval for exactly that action, which the window sends
/// with the request it confirms. `action`: "chat-bypass" (subject: the chat id), "settings-bypass"
/// (no subject) or "trust" (subject: the trust question id). Call it only after the user agreed.
#[tauri::command]
pub async fn approve_action(action: String, subject: String) -> Result<String, String> {
    let ar = arabic();
    let Some(what) = Action::from_name(&action).filter(|a| a.subject_ok(&subject)) else {
        return Err(if ar { "تعذّر تأكيد هذا." } else { "This could not be confirmed." }.into());
    };
    tauri::async_runtime::spawn_blocking(move || {
        if crate::daemon_alive().is_some_and(|(i, _)| holds_key(&i)) {
            return Ok(consent::mint(key(), what, &subject, consent::now_ms()));
        }
        // A background service this launch did not start cannot be told the user's yes. When it is
        // idle it is replaced now (the window reconnects on its own); otherwise it has to finish first.
        let replaced = crate::ensure_daemon().is_ok_and(|(i, _)| holds_key(&i));
        Err(match (replaced, ar) {
            (true, false) => "Thoughts restarted its background service so that it can confirm this. Try again.",
            (true, true) => "أعاد Thoughts تشغيل خدمة الخلفية ليتمكّن من تأكيد هذا. حاول مرة أخرى.",
            (false, false) => "Thoughts cannot confirm this while its background service finishes work it started before the app opened. Try again once that is done.",
            (false, true) => "لا يستطيع Thoughts تأكيد هذا ما دامت خدمة الخلفية تُنهي عملاً بدأته قبل فتح التطبيق. حاول مرة أخرى بعد انتهائه.",
        }
        .to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
