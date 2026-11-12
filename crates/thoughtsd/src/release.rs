//! "Is there a newer version?" for copies whose built-in updater is off (no updater key in the
//! build, a hand-shared download, a Linux package).
//!
//! It asks GitHub, where the app is published, for the latest release number: one HTTPS GET with
//! no cookies, no user configuration file, nothing about the user or the library, and a plain
//! `User-Agent: Thoughts` (GitHub refuses requests without one). It runs only when the user asks
//! ("Check for updates") or, if they switched it on in Settings, at most once a week.
//! Whenever the answer is not a clear version number (no network, the repository not public yet,
//! a rate limit) the result is "cannot tell" and nothing is shown.

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where releases are published (`owner/name` on GitHub).
pub const REPO: &str = "abdulmalik-alzeer/thoughts";
/// The automatic check runs at most this often.
pub const EVERY: chrono::Duration = chrono::Duration::days(7);

pub fn api_url() -> String { format!("https://api.github.com/repos/{REPO}/releases/latest") }

/// A release tag safe to put in a link: `v1.2.3` and the like.
pub fn valid_tag(tag: &str) -> bool {
    !tag.is_empty() && tag.len() <= 40 && tag.chars().all(|c| c.is_ascii_alphanumeric() || ".-_+".contains(c))
}

/// The download page for `tag`. Always built here, never taken from the answer.
pub fn page_url(tag: &str) -> String { format!("https://github.com/{REPO}/releases/tag/{tag}") }

/// The numbers of a version or tag (`v0.1.10` → [0, 1, 10]); `None` when it is not one.
pub fn numbers(v: &str) -> Option<Vec<u64>> {
    let core = v.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
    let parts: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty() && p.len() <= 4)
}

/// Is release `tag` newer than the running `current` version?
pub fn is_newer(current: &str, tag: &str) -> bool {
    match (numbers(current), numbers(tag)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            b > a
        }
        _ => false,
    }
}

/// The tag of the latest published release in GitHub's answer, or `None` when the answer does not
/// say (not found, refused, a draft or pre-release, anything unexpected).
pub fn parse_answer(status: u16, body: &str) -> Option<String> {
    if status != 200 { return None; }
    let v: Value = serde_json::from_str(body).ok()?;
    if v["draft"] == true || v["prerelease"] == true { return None; }
    let tag = v["tag_name"].as_str()?.trim();
    (valid_tag(tag) && numbers(tag).is_some()).then(|| tag.to_string())
}

/// Ask GitHub. `None` means "cannot tell".
async fn fetch() -> Option<String> {
    let mut c = crate::proc::command("curl");
    // -q first: never read the user's ~/.curlrc (it could add headers, cookies or a proxy).
    c.args(["-q", "--silent", "--max-time", "15", "--proto", "=https", "--tlsv1.2",
            "-H", "Accept: application/vnd.github+json", "-A", "Thoughts", "-w", "\n%{http_code}"])
        .arg(api_url())
        .stdin(std::process::Stdio::null());
    let out = crate::proc::output_timeout(&mut c, std::time::Duration::from_secs(25)).await.ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, code) = text.rsplit_once('\n')?;
    parse_answer(code.trim().parse().ok()?, body)
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Stored {
    pub checked_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The latest release tag seen, if the last check could tell.
    pub latest: Option<String>,
    /// The tag the window was last told about (it says so once per version).
    pub announced: Option<String>,
}

fn file(home: &Path) -> PathBuf { home.join("release-check.json") }

pub fn load(home: &Path) -> Stored {
    std::fs::read_to_string(file(home)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn store(home: &Path, s: &Stored) {
    if let Ok(t) = serde_json::to_string_pretty(s) {
        crate::log::ok_or_warn("could not remember the version check", crate::paths::write_atomic(&file(home), t.as_bytes(), true));
    }
}

/// `{version, url}` when a newer release than this one is known, else null.
pub fn newer_value(s: &Stored, current: &str) -> Value {
    match s.latest.as_deref().filter(|t| valid_tag(t) && is_newer(current, t)) {
        Some(t) => json!({"version": t.trim_start_matches(['v', 'V']), "url": page_url(t)}),
        None => Value::Null,
    }
}

/// Check now and remember the answer. A check that cannot tell keeps the last answer that could.
pub async fn check_now(home: &Path) -> (Stored, bool) {
    let mut s = load(home);
    let found = fetch().await;
    s.checked_at = Some(chrono::Utc::now());
    let told = found.is_some();
    if found.is_some() { s.latest = found; }
    store(home, &s);
    (s, told)
}

/// Is an automatic check due?
pub fn due(s: &Stored, now: chrono::DateTime<chrono::Utc>) -> bool {
    s.checked_at.map_or(true, |t| now - t >= EVERY || t > now)
}

/// The weekly check, for users who switched it on. Waits a little after start, then looks every
/// few hours whether a week has passed. Tells the window once per new version.
pub async fn weekly(st: Arc<AppState>) {
    tokio::time::sleep(std::time::Duration::from_secs(120)).await;
    loop {
        let (on, onboarded) = { let s = st.settings.read().await; (s.release_check, s.onboarded) };
        let home = crate::paths::home();
        if on && onboarded && due(&load(&home), chrono::Utc::now()) {
            let (mut s, _) = check_now(&home).await;
            let v = newer_value(&s, env!("CARGO_PKG_VERSION"));
            if !v.is_null() && s.announced != s.latest {
                s.announced = s.latest.clone();
                store(&home, &s);
                st.emit(json!({"type": "release.newer", "version": v["version"], "url": v["url"]}));
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.0.9", "v0.0.10"));
        assert!(is_newer("0.0.9", "v0.1.0"));
        assert!(is_newer("0.0.9", "1.0"));
        assert!(!is_newer("0.0.9", "v0.0.9"));
        assert!(!is_newer("0.0.9", "0.0.9.0"));
        assert!(!is_newer("0.1.0", "v0.0.99"));
        assert!(!is_newer("0.0.9", "nightly"));
        assert!(!is_newer("0.0.9", ""));
        assert_eq!(numbers("v1.2.3-beta.1"), Some(vec![1, 2, 3]));
    }

    #[test]
    fn only_a_clear_answer_counts() {
        let ok = r#"{"tag_name":"v0.1.0","draft":false,"prerelease":false,"html_url":"https://evil.example/"}"#;
        assert_eq!(parse_answer(200, ok).as_deref(), Some("v0.1.0"));
        // The private repository today: "not found". Rate limits, outages, anything odd: cannot tell.
        assert_eq!(parse_answer(404, r#"{"message":"Not Found"}"#), None);
        assert_eq!(parse_answer(403, ok), None);
        assert_eq!(parse_answer(200, "<html>"), None);
        assert_eq!(parse_answer(200, r#"{"tag_name":"v0.2.0","prerelease":true}"#), None);
        assert_eq!(parse_answer(200, r#"{"tag_name":"v0.2.0\"><script>"}"#), None);
        assert_eq!(parse_answer(200, r#"{"tag_name":"latest"}"#), None);
        // The link is always the app's own release page, whatever the answer says.
        let s = Stored { latest: Some("v0.1.0".into()), ..Default::default() };
        assert_eq!(newer_value(&s, "0.0.9"), json!({"version": "0.1.0", "url": format!("https://github.com/{REPO}/releases/tag/v0.1.0")}));
        assert_eq!(newer_value(&s, "0.1.0"), Value::Null);
        assert_eq!(newer_value(&Stored::default(), "0.0.9"), Value::Null);
    }

    #[test]
    fn the_automatic_check_runs_weekly() {
        let now = chrono::Utc::now();
        assert!(due(&Stored::default(), now));
        let at = |d: i64| Stored { checked_at: Some(now - chrono::Duration::days(d)), ..Default::default() };
        assert!(!due(&at(1), now) && !due(&at(6), now));
        assert!(due(&at(7), now) && due(&at(30), now));
        // A clock that went back does not stop it for good.
        assert!(due(&at(-3), now));
    }

    #[test]
    fn what_was_seen_is_remembered() {
        let d = std::env::temp_dir().join(format!("thoughts-release-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(load(&d).latest.is_none());
        store(&d, &Stored { latest: Some("v1.0.0".into()), ..Default::default() });
        assert_eq!(load(&d).latest.as_deref(), Some("v1.0.0"));
        std::fs::write(file(&d), "garbage").unwrap();
        assert!(load(&d).latest.is_none());
        std::fs::remove_dir_all(d).unwrap();
    }
}
