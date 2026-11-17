//! Who is answering on the daemon's port?
//!
//! `daemon.json` (port and token) outlives a daemon that was killed, and the port it names can be
//! taken by any other program afterwards. A client must therefore never treat "something answered
//! 200" as "that is my daemon", and must never send the token to find out. Instead it sends a
//! random challenge to `GET /api/hello?nonce=…` (the one route that needs no token) and the daemon
//! answers with `HMAC-SHA256(token, nonce)`: proof that it knows the token, without revealing it.

use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

/// What `probe` found on the port.
#[derive(Debug, PartialEq, Eq)]
pub enum Probe {
    /// The daemon that owns this token, and the version it reports.
    Daemon(String),
    /// Something that answers the way a daemon from before 0.0.9 would (it has no `/api/hello`).
    /// Nothing is proven and anyone can answer like this, so it is never trusted and never sent the
    /// token. It only tells the desktop shell that an old daemon *process* may need replacing.
    Legacy,
    /// Nothing, or something that is not the daemon.
    Nobody,
}

/// HMAC-SHA256 (RFC 2104), built on the SHA-256 the daemon already uses.
pub(crate) fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK { k[..32].copy_from_slice(&Sha256::digest(key)); } else { k[..key.len()].copy_from_slice(key); }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

/// The answer to challenge `nonce` that only the holder of `token` can give. The fixed prefix keeps
/// this from ever being a valid answer to anything else the token might be used for.
pub fn proof(token: &str, nonce: &str) -> String {
    hex(&hmac_sha256(token.as_bytes(), format!("thoughts-hello-v1:{nonce}").as_bytes()))
}

/// A fresh challenge: 128 random bits as hex.
pub fn new_nonce() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    hex(&b)
}

/// A challenge is 16 to 128 letters or digits. Anything else is not answered.
pub fn valid_nonce(nonce: &str) -> bool {
    (16..=128).contains(&nonce.len()) && nonce.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The version reported in a `/api/hello` answer, if the answer proves knowledge of `token`.
pub fn check(body: &str, token: &str, nonce: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body.trim()).ok()?;
    let given = v["proof"].as_str()?.as_bytes();
    let want = proof(token, nonce);
    let same = given.len() == want.len() && given.iter().zip(want.as_bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
    same.then(|| v["version"].as_str().unwrap_or_default().to_string())
}

/// Ask whatever listens on `127.0.0.1:port` to prove it is the daemon holding `token`. The token
/// itself is never sent. Blocking, at most about two seconds.
pub fn probe(port: u16, token: &str) -> Probe {
    let wait = Duration::from_millis(800);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, wait) else { return Probe::Nobody };
    let _ = s.set_read_timeout(Some(wait));
    let _ = s.set_write_timeout(Some(wait));
    let nonce = new_nonce();
    let req = format!("GET /api/hello?nonce={nonce} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() { return Probe::Nobody; }
    // The answer is a few hundred bytes; anything much larger is not a daemon.
    let mut buf = Vec::new();
    let _ = s.take(16 * 1024).read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let status = text.strip_prefix("HTTP/1.").and_then(|r| r.split_whitespace().nth(1)).and_then(|c| c.parse::<u16>().ok());
    let body = text.split_once("\r\n\r\n").map_or("", |(_, b)| b);
    match status {
        Some(200) => check(body, token, &nonce).map_or(Probe::Nobody, Probe::Daemon),
        // Before 0.0.9 there was no such route: "not found", or "no token" from the token check.
        Some(401) | Some(404) => Probe::Legacy,
        _ => Probe::Nobody,
    }
}

/// The version of the daemon on `port`, if and only if it proved that it holds `token`. This is
/// the one test a client applies before it uses the port for anything or sends the token to it.
pub fn proven(port: u16, token: &str) -> Option<String> {
    match probe(port, token) {
        Probe::Daemon(version) => Some(version),
        Probe::Legacy | Probe::Nobody => None,
    }
}

/// The route, mounted outside the token check (see `api::router`).
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/hello", get(hello))
}

/// `GET /api/hello?nonce=<16–128 letters or digits>` → `{ok, version, proof}`.
/// Only for local programs: a request a browser page made (it carries an `Origin`, or reached this
/// port under another host name) is refused.
async fn hello(State(st): State<Arc<AppState>>, headers: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let local = ["127.0.0.1", "localhost"].iter().any(|name| host == *name || host.strip_prefix(name).is_some_and(|rest| rest.starts_with(':')));
    if headers.contains_key(header::ORIGIN) || !local { return (StatusCode::FORBIDDEN, "forbidden").into_response(); }
    let Some(nonce) = q.get("nonce").filter(|n| valid_nonce(n)) else { return (StatusCode::BAD_REQUEST, "bad nonce").into_response() };
    let mut answer = json!({"ok": true, "version": env!("CARGO_PKG_VERSION"), "proof": proof(&st.token, nonce)});
    // A program with an access key names it (`key=<its id>`) and gets the same proof under that
    // key's SHA-256, which only the app that made the key has (`tokens.rs`): it sends the key only then.
    if let Some(hash) = q.get("key").and_then(|id| st.keys.proof_key(id)) { answer["keyProof"] = json!(proof(&hash, nonce)); }
    Json(answer).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_the_published_test_vectors() {
        // RFC 4231, test cases 1, 2 and 6 (a key longer than one block).
        assert_eq!(hex(&hmac_sha256(&[0x0b; 20], b"Hi There")), "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
        assert_eq!(hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        assert_eq!(hex(&hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")), "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54");
    }

    #[test]
    fn only_the_holder_of_the_token_can_answer_a_challenge() {
        let nonce = new_nonce();
        assert!(valid_nonce(&nonce) && nonce != new_nonce());
        let answer = |token: &str, nonce: &str| json!({"ok": true, "version": "9.9.9", "proof": proof(token, nonce)}).to_string();
        assert_eq!(check(&answer("secret", &nonce), "secret", &nonce).as_deref(), Some("9.9.9"));
        // Another token, an answer recorded for another challenge, or no proof at all.
        assert_eq!(check(&answer("guess", &nonce), "secret", &nonce), None);
        assert_eq!(check(&answer("secret", &new_nonce()), "secret", &nonce), None);
        assert_eq!(check(r#"{"ok":true,"version":"9.9.9"}"#, "secret", &nonce), None);
        assert_eq!(check(r#"{"ok":true,"version":"9.9.9","proof":""}"#, "secret", &nonce), None);
        assert_eq!(check("not json", "secret", &nonce), None);
        for bad in ["", "short", "has space in it 1234", "../../etc/passwd0000", &"a".repeat(129)] { assert!(!valid_nonce(bad), "{bad}"); }
    }

    /// A stand-in for "some other program now listens on the daemon's old port": it reads one
    /// request and answers with whatever `reply` makes of it.
    fn impostor(reply: impl Fn(&str) -> String + Send + 'static) -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let _ = s.write_all(reply(&String::from_utf8_lossy(&buf[..n])).as_bytes());
            }
        });
        port
    }

    fn http(status: &str, body: &str) -> String {
        format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len())
    }

    #[test]
    fn an_impostor_on_the_port_is_not_taken_for_the_daemon_and_never_sees_the_token() {
        let seen = Arc::new(std::sync::Mutex::new(String::new()));
        let log = seen.clone();
        // It answers 200 with the right version, exactly what used to be enough.
        let port = impostor(move |req| { *log.lock().unwrap() = req.to_string(); http("200 OK", r#"{"ok":true,"version":"0.0.9"}"#) });
        assert_eq!(probe(port, "the-real-token"), Probe::Nobody);
        let req = seen.lock().unwrap().clone();
        assert!(req.starts_with("GET /api/hello?nonce="), "{req}");
        assert!(!req.contains("the-real-token") && !req.to_lowercase().contains("authorization"), "the token must not be sent: {req}");
        // Guessing a proof does not work either, and a closed port is nobody.
        assert_eq!(probe(impostor(|_| http("200 OK", r#"{"ok":true,"version":"0.0.9","proof":"00"}"#)), "t"), Probe::Nobody);
        assert_eq!(probe(impostor(|_| "garbage".into()), "t"), Probe::Nobody);
        let closed = { let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); l.local_addr().unwrap().port() };
        assert_eq!(probe(closed, "t"), Probe::Nobody);
        // An older daemon does not know the route: reported as such, not trusted.
        assert_eq!(probe(impostor(|_| http("404 Not Found", "")), "t"), Probe::Legacy);
        assert_eq!(probe(impostor(|_| http("401 Unauthorized", "unauthorized")), "t"), Probe::Legacy);
        // Whoever does know the token is recognised.
        let port = impostor(|req| {
            let nonce = req.split("nonce=").nth(1).and_then(|r| r.split_whitespace().next()).unwrap_or("").to_string();
            http("200 OK", &json!({"ok": true, "version": "1.2.3", "proof": proof("the-real-token", &nonce)}).to_string())
        });
        assert_eq!(probe(port, "the-real-token"), Probe::Daemon("1.2.3".into()));
    }

    /// Like `impostor`, but it stays up, answers every request with `reply`, and keeps what it was sent.
    fn recording_impostor(reply: impl Fn(&str) -> String + Send + 'static) -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { return };
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let _ = s.write_all(reply(&req).as_bytes());
                log.lock().unwrap().push(req);
            }
        });
        (port, seen)
    }

    #[test]
    fn answering_ping_without_the_proof_earns_no_trust_and_no_token() {
        let token = "the-real-token-0123456789";
        let nothing_leaked = |seen: &Arc<std::sync::Mutex<Vec<String>>>| {
            let seen = seen.lock().unwrap();
            assert!(!seen.is_empty(), "the responder was asked");
            for req in seen.iter() {
                assert!(req.starts_with("GET /api/hello?nonce="), "only the challenge is ever sent: {req}");
                assert!(!req.contains(token) && !req.to_lowercase().contains("authorization") && !req.to_lowercase().contains("bearer"), "{req}");
            }
        };
        // What used to be enough: 200 and the current version on every path, /api/ping included.
        let (port, seen) = recording_impostor(|_| http("200 OK", r#"{"ok":true,"version":"0.0.9"}"#));
        assert_eq!(proven(port, token), None);
        nothing_leaked(&seen);
        // Posing as a daemon too old to know the challenge: "not found" for it, 200 for /api/ping.
        // It is reported as such, is not trusted, and is not asked anything else.
        let (port, seen) = recording_impostor(|req| if req.starts_with("GET /api/hello") { http("404 Not Found", "") } else { http("200 OK", r#"{"ok":true,"version":"0.0.8"}"#) });
        assert_eq!(probe(port, token), Probe::Legacy);
        assert_eq!(proven(port, token), None);
        assert_eq!(seen.lock().unwrap().len(), 2, "one challenge per check, and no follow-up request");
        nothing_leaked(&seen);
        // Replaying a proof it saw for an earlier challenge does not work: every challenge is new.
        let old_nonce = new_nonce();
        let replay = json!({"ok": true, "version": "0.0.9", "proof": proof(token, &old_nonce)}).to_string();
        let (port, seen) = recording_impostor(move |_| http("200 OK", &replay));
        assert_eq!(proven(port, token), None);
        nothing_leaked(&seen);
        // Echoing back whatever the request contained does not work either: the proof cannot be
        // learned from the challenge.
        let (port, seen) = recording_impostor(|req| {
            let nonce = req.split("nonce=").nth(1).and_then(|r| r.split_whitespace().next()).unwrap_or("").to_string();
            http("200 OK", &json!({"ok": true, "version": "0.0.9", "proof": nonce}).to_string())
        });
        assert_eq!(proven(port, token), None);
        nothing_leaked(&seen);
    }

    #[test]
    fn a_correct_proof_is_trusted() {
        let (port, seen) = recording_impostor(|req| {
            let nonce = req.split("nonce=").nth(1).and_then(|r| r.split_whitespace().next()).unwrap_or("").to_string();
            http("200 OK", &json!({"ok": true, "version": "1.2.3", "proof": proof("the-real-token", &nonce)}).to_string())
        });
        assert_eq!(proven(port, "the-real-token").as_deref(), Some("1.2.3"));
        // The same responder is nobody to a client holding a different token.
        assert_eq!(proven(port, "another-token"), None);
        assert!(seen.lock().unwrap().iter().all(|r| !r.contains("the-real-token") && !r.contains("another-token")));
    }

    #[tokio::test]
    async fn the_real_daemon_proves_itself_and_still_wants_the_token_for_everything_else() {
        let st = Arc::new(AppState::for_test());
        let token = st.token.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { let _ = axum::serve(listener, crate::api::router(st)).await; });
        let t = token.clone();
        assert_eq!(tokio::task::spawn_blocking(move || probe(port, &t)).await.unwrap(), Probe::Daemon(env!("CARGO_PKG_VERSION").into()));
        assert_eq!(tokio::task::spawn_blocking(move || probe(port, "a-stale-token")).await.unwrap(), Probe::Nobody);
        assert!(crate::ping(port, &token).await && !crate::ping(port, "a-stale-token").await);
        // One raw request, answered by the real router.
        let ask = move |request: String| tokio::task::spawn_blocking(move || {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(request.as_bytes()).unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            out
        });
        let get = |path: &str, extra: &str| format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Connection: close\r\n\r\n");
        let nonce = new_nonce();
        // No token is needed for the challenge, and none is given away by it.
        let r = ask(get(&format!("/api/hello?nonce={nonce}"), "")).await.unwrap();
        assert!(r.starts_with("HTTP/1.1 200") && r.contains(&proof(&token, &nonce)) && !r.contains(&token), "{r}");
        // Every other route is as closed as before.
        for path in ["/api/ping", "/api/status", "/api/settings", "/api/layer/history", "/api/hello/more"] {
            let r = ask(get(path, "")).await.unwrap();
            assert!(r.starts_with("HTTP/1.1 401") || r.starts_with("HTTP/1.1 404"), "{path}: {r}");
            assert!(!r.contains(&token));
        }
        assert!(ask(get("/api/status", "")).await.unwrap().starts_with("HTTP/1.1 401"));
        assert!(ask(get("/api/ping", &format!("Authorization: Bearer {token}\r\n"))).await.unwrap().starts_with("HTTP/1.1 200"));
        // A web page has no business asking, and a malformed challenge is not answered.
        assert!(ask(get(&format!("/api/hello?nonce={nonce}"), "Origin: https://example.com\r\n")).await.unwrap().starts_with("HTTP/1.1 403"));
        assert!(ask(format!("GET /api/hello?nonce={nonce} HTTP/1.1\r\nHost: rebound.example:{port}\r\nConnection: close\r\n\r\n")).await.unwrap().starts_with("HTTP/1.1 403"));
        assert!(ask(get("/api/hello?nonce=short", "")).await.unwrap().starts_with("HTTP/1.1 400"));
        assert!(ask(get("/api/hello", "")).await.unwrap().starts_with("HTTP/1.1 400"));
    }
}
