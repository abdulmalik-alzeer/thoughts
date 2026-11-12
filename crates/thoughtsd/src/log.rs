//! A small rotating file log at `~/.thoughts/logs/daemon.log` (the daemon's stdout/stderr go
//! nowhere). Every line is passed through `redact` first, so tokens and passwords never reach disk.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 1_000_000;
/// Rotated files kept next to the live one: daemon.log.1 … daemon.log.3.
const KEEP: usize = 3;

struct Logger { dir: PathBuf, secrets: Vec<String> }

fn logger() -> &'static Mutex<Option<Logger>> {
    static L: OnceLock<Mutex<Option<Logger>>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(None))
}

pub fn file_in(dir: &Path) -> PathBuf { dir.join("daemon.log") }

/// Start logging into `dir`. Until this is called every log call is a no-op.
pub fn init(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    *logger().lock().unwrap_or_else(|e| e.into_inner()) = Some(Logger { dir: dir.to_path_buf(), secrets: vec![] });
}

/// A literal value (the daemon's own bearer token) that must never appear in the log.
pub fn add_secret(secret: &str) {
    if secret.len() < 8 { return; }
    if let Some(l) = logger().lock().unwrap_or_else(|e| e.into_inner()).as_mut() { l.secrets.push(secret.to_string()); }
}

/// Remove anything that looks like a credential.
pub fn redact(s: &str) -> String {
    static RES: OnceLock<Vec<(regex::Regex, &'static str)>> = OnceLock::new();
    let res = RES.get_or_init(|| [
        // Authorization headers (whole value), then bare bearer tokens.
        (r"(?i)(authorization\s*[:=]\s*)[^\r\n]+", "${1}[redacted]"),
        (r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]+", "Bearer [redacted]"),
        // token=…, api_key=…, password=… in query strings, env dumps and JSON.
        (r#"(?i)((?:[a-z_]*token|api[_-]?key|password|secret)["']?\s*[=:]\s*["']?)[^\s&"',;}]+"#, "${1}[redacted]"),
        // Whoever a URL signs in as: https://user:pass@host, and https://token@host just the same
        // (an access token is often given as the user name alone).
        (r"(?i)\b([a-z][a-z0-9+.-]*://)[^/\s@]+@", "${1}[redacted]@"),
        // Provider keys wherever they appear.
        (r"\bsk-[A-Za-z0-9_-]{16,}", "[redacted]"),
        (r"\bgh[pousr]_[A-Za-z0-9]{20,}", "[redacted]"),
        (r"\bgithub_pat_[A-Za-z0-9_]{20,}", "[redacted]"),
        (r"\bglpat-[A-Za-z0-9_-]{16,}", "[redacted]"),
        // The app's access keys for other programs (`tokens.rs`).
        (r"\bthoughts_[0-9a-f]{8}_[A-Za-z0-9]{20,}", "[redacted]"),
    ].into_iter().map(|(re, rep)| (regex::Regex::new(re).unwrap(), rep)).collect());
    let mut out = s.to_string();
    for (re, rep) in res { out = re.replace_all(&out, *rep).into_owned(); }
    out
}

fn rotate(dir: &Path) {
    let live = file_in(dir);
    if std::fs::metadata(&live).map(|m| m.len()).unwrap_or(0) < MAX_BYTES { return; }
    let n = |i: usize| dir.join(format!("daemon.log.{i}"));
    let _ = std::fs::remove_file(n(KEEP));
    for i in (1..KEEP).rev() { let _ = std::fs::rename(n(i), n(i + 1)); }
    let _ = std::fs::rename(&live, n(1));
}

fn write_line(l: &Logger, level: &str, msg: &str) {
    let mut msg = redact(msg);
    for s in &l.secrets { msg = msg.replace(s, "[redacted]"); }
    rotate(&l.dir);
    let line = format!("{} {level:5} {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), msg.replace('\n', "\n    "));
    let mut o = std::fs::OpenOptions::new();
    o.create(true).append(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    if let Ok(mut f) = o.open(file_in(&l.dir)) { let _ = f.write_all(line.as_bytes()); }
}

fn write(level: &str, msg: &str) {
    if let Some(l) = logger().lock().unwrap_or_else(|e| e.into_inner()).as_ref() { write_line(l, level, msg); }
}

pub fn info(msg: impl AsRef<str>) { write("INFO", msg.as_ref()) }
pub fn warn(msg: impl AsRef<str>) { write("WARN", msg.as_ref()) }
pub fn error(msg: impl AsRef<str>) { write("ERROR", msg.as_ref()) }

/// Log a failure that the caller otherwise ignores. Returns the value for chaining.
pub fn ok_or_warn<T, E: std::fmt::Display>(what: &str, r: Result<T, E>) -> Option<T> {
    match r { Ok(v) => Some(v), Err(e) => { warn(format!("{what}: {e}")); None } }
}

/// Panics are written to the log before the default handler runs.
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let short: String = bt.lines().take(40).collect::<Vec<_>>().join("\n");
        error(format!("PANIC {info}\n{short}"));
        prev(info);
    }));
}

/// The last `lines` lines of the live log in `dir` (plus the previous file when the live one is short).
pub fn tail_in(dir: &Path, lines: usize) -> String {
    let mut text = std::fs::read_to_string(file_in(dir)).unwrap_or_default();
    if text.lines().count() < lines {
        if let Ok(prev) = std::fs::read_to_string(dir.join("daemon.log.1")) { text = prev + &text; }
    }
    let all: Vec<&str> = text.lines().collect();
    redact(&all[all.len().saturating_sub(lines)..].join("\n"))
}

pub fn tail(lines: usize) -> String { tail_in(&crate::paths::logs_dir(), lines) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials() {
        let cases = [
            ("GET /api/events?token=abc123DEF456&x=1", "abc123DEF456"),
            ("Authorization: Bearer s3cr3tTokenValue99", "s3cr3tTokenValue99"),
            ("header bearer s3cr3tTokenValue99 sent", "s3cr3tTokenValue99"),
            ("git push https://alice:hunter2pass@github.com/a/b.git failed", "hunter2pass"),
            ("ANTHROPIC_AUTH_TOKEN=sk-ant-REDACTED", "sk-ant-REDACTED"),
            (r#"{"token":"abcdefgh12345678","port":1}"#, "abcdefgh12345678"),
            ("key ghp_abcdefghijklmnopqrstuvwxyz0123 leaked", "ghp_abcdefghijklmnopqrstuvwxyz0123"),
        ];
        for (line, secret) in cases {
            let r = redact(line);
            assert!(!r.contains(secret), "{line} -> {r}");
            assert!(r.contains("[redacted]"), "{r}");
        }
        assert_eq!(redact("git push https://alice:hunter2pass@github.com/a/b.git"), "git push https://[redacted]@github.com/a/b.git");
        assert_eq!(redact("GET /x?token=abc&y=1"), "GET /x?token=[redacted]&y=1");
        // A token used as the URL's user name, and the newer token shapes on their own (SEC-34).
        assert_eq!(redact("fatal: unable to access 'https://glpat-AbCdEf0123456789xyzz@gitlab.com/me/lib.git/'"), "fatal: unable to access 'https://[redacted]@gitlab.com/me/lib.git/'");
        assert_eq!(redact("push to ssh://git@example.com/me/lib.git failed"), "push to ssh://[redacted]@example.com/me/lib.git failed");
        for token in ["glpat-AbCdEf0123456789xyzz", "github_pat_11ABCDEFG0abcdefghijkl_mnopqrstuvwxyz0123456789"] {
            assert_eq!(redact(&format!("using {token} now")), "using [redacted] now");
        }
        // A plain address, and an @ further along a path, are not sign-in details.
        assert_eq!(redact("see https://example.com/users/@sara and write to sara@example.com"), "see https://example.com/users/@sara and write to sara@example.com");
        // Ordinary lines are left alone.
        for plain in ["sync: pulled origin, pushed origin", "wrap 12 applying (3 chats)", "https://github.com/a/b.git"] { assert_eq!(redact(plain), plain); }
    }

    #[test]
    fn writes_rotates_and_tails() {
        let d = std::env::temp_dir().join(format!("thoughts-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let l = Logger { dir: d.clone(), secrets: vec!["daemonTokenXYZ123".into()] };
        write_line(&l, "INFO", "hello token=abc daemonTokenXYZ123");
        let t = std::fs::read_to_string(file_in(&d)).unwrap();
        assert!(t.contains("INFO") && t.contains("hello token=[redacted] [redacted]") && !t.contains("daemonTokenXYZ123"), "{t}");
        // Four oversized generations: only the live file and KEEP rotated ones survive.
        for i in 0..5 {
            std::fs::write(file_in(&d), vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
            write_line(&l, "WARN", &format!("after rotation {i}"));
        }
        let mut names: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        assert_eq!(names, ["daemon.log", "daemon.log.1", "daemon.log.2", "daemon.log.3"]);
        assert_eq!(std::fs::read_to_string(file_in(&d)).unwrap().lines().count(), 1);
        std::fs::write(file_in(&d), (0..1000).map(|i| format!("line {i}\n")).collect::<String>()).unwrap();
        let tail = tail_in(&d, 400);
        assert_eq!(tail.lines().count(), 400);
        assert!(tail.ends_with("line 999") && tail.starts_with("line 600"));
        std::fs::remove_dir_all(d).unwrap();
    }
}
