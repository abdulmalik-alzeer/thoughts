//! GIT_ASKPASS mode: git runs this executable with a prompt such as
//! "Username for 'https://github.com': " and reads the answer from stdout.
//!
//! This mode never reads the keychain itself. Anyone can start the app's executable with any
//! environment and any prompt, and the keychain trusts that executable, so a lookup here would hand
//! the saved sync password to every program on the computer. Instead the daemon gives each git
//! command it starts a fresh random code (`Ticket`), valid only while that command runs; this mode
//! sends the code and git's prompt back to the daemon over the loopback address, and the daemon
//! answers only for a code of a git command it is running right now (`routes`).

use axum::extract::DefaultBodyLimit;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::Arc;

/// The variable that switches the executable into this mode, holding the git command's code.
pub const CODE_VAR: &str = "THOUGHTS_ASKPASS";
/// The daemon's port on 127.0.0.1, where the answer is asked for.
pub const PORT_VAR: &str = "THOUGHTS_ASKPASS_PORT";
/// The daemon's route for it (outside the app token: the code is the only key it takes).
const ROUTE: &str = "/api/askpass";
/// A prompt is one short line; a request is a code and a prompt.
const BODY_LIMIT: usize = 8 * 1024;

pub fn main() -> i32 {
    let prompt = std::env::args().nth(1).unwrap_or_default();
    // Asked about nothing the daemon would answer: no need to ask it.
    if host_asked(&prompt).is_none() { return 1; }
    let code = std::env::var(CODE_VAR).unwrap_or_default();
    let Some(port) = std::env::var(PORT_VAR).ok().and_then(|p| p.parse::<u16>().ok()).filter(|p| *p != 0) else { return 1 };
    if !valid_code(&code) { return 1; }
    match ask(port, &code, &prompt) {
        Some(s) => { println!("{s}"); 0 }
        None => 1,
    }
}

/// Ask the daemon on `127.0.0.1:port` for the answer to `prompt`, showing it `code`. Blocking, a
/// few seconds at most (the daemon may have to wait for the keychain).
fn ask(port: u16, code: &str, prompt: &str) -> Option<String> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = s.set_write_timeout(Some(std::time::Duration::from_secs(5)));
    let body = json!({"code": code, "prompt": prompt}).to_string();
    let req = format!("POST {ROUTE} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    s.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    let _ = s.take(64 * 1024).read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let status = text.strip_prefix("HTTP/1.").and_then(|r| r.split_whitespace().nth(1)).and_then(|c| c.parse::<u16>().ok());
    if status != Some(200) { return None; }
    let body = text.split_once("\r\n\r\n").map_or("", |(_, b)| b);
    let v: Value = serde_json::from_str(body.trim()).ok()?;
    v["answer"].as_str().map(String::from)
}

/// A code is 64 hex digits (256 random bits).
fn valid_code(code: &str) -> bool { code.len() == 64 && code.bytes().all(|b| b.is_ascii_hexdigit()) }

/// The codes of the git commands running right now.
static LIVE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn live() -> std::sync::MutexGuard<'static, Vec<String>> { LIVE.lock().unwrap_or_else(|e| e.into_inner()) }

/// The code of one git command the daemon starts: given to that git in its environment, valid
/// until this is dropped (when the command has ended, however it ended).
pub struct Ticket { code: String }

impl Ticket {
    pub fn new() -> Ticket {
        use rand::RngCore;
        let mut b = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut b);
        let code: String = b.iter().map(|x| format!("{x:02x}")).collect();
        live().push(code.clone());
        Ticket { code }
    }
    pub fn code(&self) -> &str { &self.code }
}

impl Drop for Ticket {
    fn drop(&mut self) { live().retain(|c| c != &self.code); }
}

/// Compare without leaking, through timing, how much of a code matched.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Is `code` the code of a git command running right now? Every live code is compared in full.
fn is_live(code: &str) -> bool {
    if !valid_code(code) { return false; }
    live().iter().fold(false, |found, c| found | same(c.as_bytes(), code.as_bytes()))
}

/// The answer to git's `prompt` from the saved sign-ins: the username or the password.
fn lookup(prompt: &str) -> Option<String> {
    let host = host_asked(prompt)?;
    // The same store the daemon writes to: the system keyring, or the private file when a Linux
    // machine has no working keyring.
    let raw = keys_for(&host).iter().find_map(|k| crate::secrets::get("thoughts-git", k))?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let field = if prompt.to_lowercase().starts_with("username") { "username" } else { "secret" };
    v[field].as_str().map(String::from)
}

/// The route, mounted outside the app token's check (see `api::router`): its only key is a live code.
pub fn routes() -> Router<Arc<crate::state::AppState>> {
    Router::new().route(ROUTE, post(answer)).layer(DefaultBodyLimit::max(BODY_LIMIT))
}

/// `POST /api/askpass` `{code, prompt}` → `{answer}`. Only for git's prompt helper: a request from
/// a web page (it carries an `Origin`, or reached this port under another host name) is refused,
/// and so is every request without the code of a git command the daemon is running right now.
async fn answer(headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let local = ["127.0.0.1", "localhost"].iter().any(|name| host == *name || host.strip_prefix(name).is_some_and(|rest| rest.starts_with(':')));
    if headers.contains_key(header::ORIGIN) || !local { return (StatusCode::FORBIDDEN, "forbidden").into_response(); }
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let (Some(code), Some(prompt)) = (v["code"].as_str(), v["prompt"].as_str()) else { return (StatusCode::BAD_REQUEST, "bad request").into_response() };
    if !is_live(code) { return (StatusCode::UNAUTHORIZED, "unauthorized").into_response(); }
    let prompt = prompt.to_string();
    let host = host_asked(&prompt);
    match tokio::task::spawn_blocking(move || lookup(&prompt)).await.ok().flatten() {
        Some(s) => { if let Some(h) = host { note_answered(&h); } Json(json!({"answer": s})).into_response() }
        None => (StatusCode::NOT_FOUND, "nothing saved").into_response(),
    }
}

/// Hosts whose saved sign-in was read and given to git during this run of the daemon: this program
/// can read them without the system asking the user anything (`incoming.rs` relies on it).
static ANSWERED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn note_answered(host: &str) {
    let mut a = ANSWERED.lock().unwrap_or_else(|e| e.into_inner());
    if !a.iter().any(|h| h == host) { a.push(host.to_string()); }
}

/// Was the saved sign-in for `host` (see `git::host_of`) given to git during this run?
pub fn answered(host: &str) -> bool {
    let a = ANSWERED.lock().unwrap_or_else(|e| e.into_inner());
    keys_for(host).iter().any(|k| a.iter().any(|h| h == k)) || a.iter().any(|h| keys_for(h).iter().any(|k| k == host))
}

/// The host (with its port) a prompt asks about — only when git asks for an `https://` address.
/// A saved password is never handed out for anything else: an `http://` address (which a library's
/// own settings can name) would send it across the network unencrypted, and no other kind of
/// address the app sets up signs in with one.
pub(crate) fn host_asked(prompt: &str) -> Option<String> {
    let url = prompt.split('\'').nth(1)?;
    if !url.get(..8).map_or(false, |s| s.eq_ignore_ascii_case("https://")) { return None; }
    crate::git::host_of(url)
}

/// The saved entries that can answer for `host` (`host_of` of the address git asks about), best
/// first: the host with its port, then — for an address that names the usual port 443, which is
/// the same place as the bare host — the bare host. A password saved for `host` is never offered
/// to that host on another port: a different port can be a different service, run by someone else.
pub(crate) fn keys_for(host: &str) -> Vec<String> {
    let mut keys = vec![host.to_string()];
    if let Some((bare, "443")) = host.rsplit_once(':') { if !bare.is_empty() { keys.push(bare.to_string()); } }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "127.0.0.1:1".parse().unwrap());
        h
    }
    fn req(code: &str, prompt: &str) -> axum::body::Bytes { json!({"code": code, "prompt": prompt}).to_string().into() }
    async fn status(h: HeaderMap, body: axum::body::Bytes) -> u16 { answer(h, body).await.status().as_u16() }

    /// The app's executable started by anyone, with any prompt, gets nothing: only a git command the
    /// daemon is running right now is answered, and only while it runs.
    #[tokio::test]
    async fn a_password_is_given_only_to_a_git_command_the_daemon_is_running() {
        crate::secrets::set("thoughts-git", "askpass-test.example.com", r#"{"username":"sara","secret":"s3cret"}"#).unwrap();
        let prompt = "Password for 'https://sara@askpass-test.example.com': ";
        let ticket = Ticket::new();
        let code = ticket.code().to_string();
        assert!(valid_code(&code) && code != Ticket::new().code());
        // A live code is served, the username as well as the password.
        let r = answer(local(), req(&code, prompt)).await;
        assert_eq!(r.status(), StatusCode::OK);
        let body = axum::body::to_bytes(r.into_body(), 4096).await.unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["answer"], "s3cret");
        assert_eq!(lookup("Username for 'https://askpass-test.example.com': ").as_deref(), Some("sara"));
        // No code, a made-up one, one that is nearly right, one of the wrong shape.
        let mut near = code.clone().into_bytes();
        near[63] = if near[63] == b'0' { b'1' } else { b'0' };
        for bad in ["", "1", &"0".repeat(64), std::str::from_utf8(&near).unwrap(), &format!("{code}0"), &code.to_uppercase()] {
            assert_eq!(status(local(), req(bad, prompt)).await, 401, "{bad}");
        }
        assert_eq!(status(local(), "not json".into()).await, 400);
        // A web page, or a request that reached the port under another name, is refused even with the code.
        let mut page = local();
        page.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert_eq!(status(page, req(&code, prompt)).await, 403);
        let mut renamed = HeaderMap::new();
        renamed.insert(header::HOST, "evil.example:1".parse().unwrap());
        assert_eq!(status(renamed, req(&code, prompt)).await, 403);
        // Nothing saved, or an address a password is never given for.
        assert_eq!(status(local(), req(&code, "Password for 'https://nothing-saved.example.com': ")).await, 404);
        assert_eq!(status(local(), req(&code, "Password for 'http://askpass-test.example.com': ")).await, 404);
        // The command has ended: its code is no longer answered.
        drop(ticket);
        assert!(!is_live(&code));
        assert_eq!(status(local(), req(&code, prompt)).await, 401);
        crate::secrets::delete("thoughts-git", "askpass-test.example.com").unwrap();
    }

    /// The prompt helper as git starts it, against a daemon on a real port.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_prompt_helper_asks_the_daemon_and_never_answers_by_itself() {
        crate::secrets::set("thoughts-git", "askpass-wire.example.com:8443", r#"{"username":"sara","secret":"wire-secret"}"#).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().route(ROUTE, post(answer)).layer(DefaultBodyLimit::max(BODY_LIMIT));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let prompt = "Password for 'https://askpass-wire.example.com:8443': ";
        let ticket = Ticket::new();
        let code = ticket.code().to_string();
        let asked = |code: String| tokio::task::spawn_blocking(move || ask(port, &code, prompt));
        assert_eq!(asked(code.clone()).await.unwrap().as_deref(), Some("wire-secret"));
        assert_eq!(asked("ab".repeat(32)).await.unwrap(), None);
        drop(ticket);
        assert_eq!(asked(code).await.unwrap(), None);
        crate::secrets::delete("thoughts-git", "askpass-wire.example.com:8443").unwrap();
    }

    #[test]
    fn a_password_is_looked_up_under_the_host_and_port_git_asks_for() {
        let host = |prompt: &str| super::host_asked(prompt).unwrap_or_default();
        assert_eq!(super::keys_for(&host("Password for 'https://sara@git.example.com:3000': ")), ["git.example.com:3000"]);
        assert_eq!(super::keys_for(&host("Username for 'https://github.com': ")), ["github.com"]);
    }

    #[test]
    fn a_password_saved_for_the_bare_host_is_only_offered_on_the_usual_port() {
        // 443 is where the bare host already is: the same place, so its password answers.
        assert_eq!(super::keys_for("github.example.com:443"), ["github.example.com:443", "github.example.com"]);
        // Any other port may be someone else's service on the same machine.
        for host in ["github.example.com:9999", "github.example.com:8443", "github.example.com:4430", "github.example.com:44"] {
            assert_eq!(super::keys_for(host), [host], "{host}");
        }
        assert_eq!(super::keys_for(":443"), [":443"]);
    }

    #[test]
    fn a_password_is_only_given_for_an_encrypted_address() {
        assert_eq!(super::host_asked("Password for 'HTTPS://sara@github.com': ").as_deref(), Some("github.com"));
        for prompt in ["Password for 'http://sara@github.com': ", "Username for 'http://github.com': ", "Password for 'ssh://git@github.com': ",
                       "Password for 'httpsx://github.com': ", "Password: ", "Enter passphrase for key '/Users/sara/.ssh/id_ed25519': "] {
            assert_eq!(super::host_asked(prompt), None, "{prompt}");
        }
    }
}
