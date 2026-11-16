//! Updates for a copy whose releases are not public: the `gh` fallback.
//!
//! The updater asks the public address first, with no sign-in. Only when that answers a clear
//! "not found", and only when the person switched this on from a terminal
//! (`thoughts updates gh on`), the app asks the GitHub command-line tool for its sign-in and
//! fetches the newest release through GitHub's API, as that person. Their access to the
//! repository decides what they get. The updater library still checks the signature of whatever
//! is fetched before it installs anything, so the sign-in decides who may download and never
//! what may be installed.
//!
//! What this file keeps to:
//! - With the switch off, `gh` is never started.
//! - The sign-in is sent to the API host only. GitHub answers a file request with a forward to
//!   its file host; that second request is made here, without the sign-in.
//! - The sign-in lives in memory for one check or one install. It is never written anywhere,
//!   never logged, and never handed to the window or to a chat.
//!
//! The switch is its own small file in the app's folder, written only by the terminal command.
//! It is not part of `settings.json`, so the Settings page, a customisation or a chat cannot
//! flip it, and it stays in the product after the repository becomes public: from then on the
//! public address answers and this path is simply never reached.

use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the releases are (`owner/name` on GitHub).
pub const REPO: &str = thoughtsd::release::REPO;
/// GitHub's API: the only host the sign-in is ever sent to.
const API: &str = "https://api.github.com";
/// The switch file, in the app's folder.
const SWITCH: &str = "updates-gh.json";
/// The updater's manifest, a file of every release built for updates.
const MANIFEST: &str = "latest.json";
const MANIFEST_LIMIT: u64 = 1024 * 1024;
/// No installer is anywhere near this; a larger answer is not an installer.
pub const INSTALLER_LIMIT: u64 = 1024 * 1024 * 1024;

/// The API address. A rehearsal build may be pointed at a pretend GitHub on this computer.
pub fn api() -> String {
    #[cfg(feature = "rehearsal")]
    if let Some(v) = std::env::var("THOUGHTS_GITHUB_API").ok().map(|v| v.trim().trim_end_matches('/').to_string()).filter(|v| !v.is_empty()) {
        return v;
    }
    API.to_string()
}

// ---------- the switch ----------

fn switch_file() -> PathBuf { thoughtsd::paths::home().join(SWITCH) }

/// Has the person switched the fallback on (from a terminal)?
pub fn is_on() -> bool { is_on_in(&switch_file()) }

fn is_on_in(file: &Path) -> bool {
    std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v["gh"] == true)
}

fn set_on_in(file: &Path, on: bool) -> std::io::Result<()> {
    if on {
        if let Some(d) = file.parent() { thoughtsd::paths::create_private_dir(d)?; }
        return thoughtsd::paths::write_atomic(file, b"{\"gh\": true}\n", true);
    }
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

// ---------- what can go wrong, in words the window has a sentence for ----------

/// Why the fallback has nothing to offer. `code` is what the window is told.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// The fallback is switched off, so nothing was asked.
    Off,
    /// The GitHub command-line tool is not installed.
    NoGh,
    /// It is installed but not signed in.
    SignedOut,
    /// GitHub refused the sign-in (expired or withdrawn).
    Refused,
    /// The account cannot see the repository.
    NoAccess,
    /// It can, but nothing has been published.
    NoRelease,
    /// GitHub could not be reached.
    Offline(String),
    /// Anything else, with what happened.
    Unexpected(String),
}

impl Problem {
    pub fn code(&self) -> &'static str {
        match self {
            Problem::Off => "unpublished",
            Problem::NoGh => "gh-missing",
            Problem::SignedOut => "gh-signed-out",
            Problem::Refused => "gh-refused",
            Problem::NoAccess => "gh-no-access",
            Problem::NoRelease => "gh-no-release",
            Problem::Offline(_) => "offline",
            Problem::Unexpected(_) => "failed",
        }
    }
    pub fn detail(&self) -> String {
        match self {
            Problem::Offline(d) | Problem::Unexpected(d) => d.clone(),
            _ => String::new(),
        }
    }
}

// ---------- the sign-in ----------

/// A GitHub sign-in. It has no `Debug` or `Display` on purpose: it cannot end up in a log line.
pub struct SignIn(String);

/// Ask the GitHub command-line tool for its sign-in. Only ever called with the switch on.
pub fn sign_in() -> Result<SignIn, Problem> {
    // A rehearsal build asks a stand-in, so the person's real sign-in never goes to a pretend GitHub.
    #[cfg(feature = "rehearsal")]
    if let Some(stand_in) = std::env::var_os("THOUGHTS_REHEARSAL_GH") {
        return sign_in_with(Path::new(&stand_in));
    }
    let Some(gh) = thoughtsd::claude::find_binary(&None, "gh") else { return Err(Problem::NoGh) };
    sign_in_with(&gh)
}

fn sign_in_with(gh: &Path) -> Result<SignIn, Problem> {
    let mut c = thoughtsd::proc::std_command(gh);
    c.args(["auth", "token", "--hostname", "github.com"])
        .env("PATH", thoughtsd::claude::shell_path())
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");
    // A token left in the environment is not the person's own sign-in.
    for k in ["GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN", "GH_HOST", "GH_DEBUG"] { c.env_remove(k); }
    let out = thoughtsd::proc::std_output_timeout(&mut c, Duration::from_secs(8)).map_err(|e| Problem::Unexpected(format!("the GitHub command-line tool did not answer ({e})")))?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || token.is_empty() { return Err(Problem::SignedOut); }
    if token.len() > 512 || !token.chars().all(|c| c.is_ascii_graphic()) {
        return Err(Problem::Unexpected("the GitHub command-line tool printed something that is not a sign-in".into()));
    }
    Ok(SignIn(token))
}

// ---------- GitHub's API ----------

/// One file of a release, as the API lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    /// The file's address in the API (`…/releases/assets/<id>`), never the public download address.
    pub url: String,
    pub size: u64,
}

/// The newest published release.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

/// Redirects are never followed for us: every request made with the sign-in ends at the API.
/// `quiet` is how long the other side may say nothing before the request is given up: short for
/// a question to the API, longer while a file is coming in.
fn agent(quiet: u64) -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0).timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(quiet)).user_agent("Thoughts").build()
}

/// One request to the API, with the sign-in. `url` must be an address of the API itself.
fn api_get(api: &str, url: &str, who: &SignIn, accept: &str) -> Result<ureq::Response, Problem> {
    if !url.starts_with(&format!("{api}/")) {
        return Err(Problem::Unexpected("an address outside GitHub's API was refused".into()));
    }
    match agent(20).get(url).set("Authorization", &format!("Bearer {}", who.0)).set("Accept", accept).set("X-GitHub-Api-Version", "2022-11-28").call() {
        Ok(r) => Ok(r),
        Err(ureq::Error::Status(401, _)) => Err(Problem::Refused),
        Err(ureq::Error::Status(404, _)) => Err(Problem::NoAccess),
        Err(ureq::Error::Status(code, _)) => Err(Problem::Unexpected(format!("GitHub answered {code}"))),
        Err(ureq::Error::Transport(t)) => Err(Problem::Offline(t.to_string())),
    }
}

fn read_all(r: ureq::Response, expect: Option<u64>, limit: u64, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>, Problem> {
    let mut reader = r.into_reader().take(limit + 1);
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut chunk).map_err(|e| Problem::Offline(e.to_string()))?;
        if n == 0 { break; }
        out.extend_from_slice(&chunk[..n]);
        if out.len() as u64 > limit { return Err(Problem::Unexpected("the download is larger than any release file".into())); }
        progress(out.len() as u64, expect);
    }
    if expect.is_some_and(|e| e != out.len() as u64) {
        return Err(Problem::Offline("the download was cut short".into()));
    }
    Ok(out)
}

fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 200 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// The release in GitHub's answer. Files whose API address is not on this API, under this
/// repository, are left out: nothing else is ever asked with the sign-in.
fn parse_release(api: &str, repo: &str, v: &Value) -> Option<Release> {
    if v["draft"] == true || v["prerelease"] == true { return None; }
    let tag = v["tag_name"].as_str()?.trim();
    if !thoughtsd::release::valid_tag(tag) || thoughtsd::release::numbers(tag).is_none() { return None; }
    let prefix = format!("{api}/repos/{repo}/releases/assets/");
    let assets = v["assets"].as_array()?.iter().filter_map(|a| {
        let (name, url, size) = (a["name"].as_str()?, a["url"].as_str()?, a["size"].as_u64()?);
        let id = url.strip_prefix(&prefix)?;
        (safe_name(name) && !id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then(|| Asset { name: name.to_string(), url: url.to_string(), size })
    }).collect();
    Some(Release { tag: tag.to_string(), assets })
}

/// The newest published release of `repo`, asked as the signed-in person.
pub fn latest_release(api: &str, repo: &str, who: &SignIn) -> Result<Release, Problem> {
    const JSON: &str = "application/vnd.github+json";
    let r = match api_get(api, &format!("{api}/repos/{repo}/releases/latest"), who, JSON) {
        // "Not found" is GitHub's answer both for a repository this account cannot see and for
        // one with nothing published. Asking for the repository itself tells the two apart.
        Err(Problem::NoAccess) => return Err(match api_get(api, &format!("{api}/repos/{repo}"), who, JSON) {
            Ok(_) => Problem::NoRelease,
            Err(e) => e,
        }),
        other => other?,
    };
    if r.status() != 200 { return Err(Problem::Unexpected(format!("GitHub answered {}", r.status()))); }
    let body = read_all(r, None, MANIFEST_LIMIT, |_, _| {})?;
    serde_json::from_slice::<Value>(&body).ok().and_then(|v| parse_release(api, repo, &v))
        .ok_or_else(|| Problem::Unexpected("GitHub's answer about the newest release could not be read".into()))
}

/// The name GitHub knows the signed-in account by (for the terminal command's report).
fn account(api: &str, who: &SignIn) -> Option<String> {
    let r = api_get(api, &format!("{api}/user"), who, "application/vnd.github+json").ok()?;
    let body = read_all(r, None, MANIFEST_LIMIT, |_, _| {}).ok()?;
    let v: Value = serde_json::from_slice(&body).ok()?;
    v["login"].as_str().filter(|l| safe_name(l)).map(str::to_string)
}

/// May a download be forwarded to `to`? Only to an https address (a pretend GitHub on this
/// computer, in a test or a rehearsal, forwards to itself over http).
fn forward_allowed(api: &str, to: &str) -> bool {
    to.starts_with("https://") || (api.starts_with("http://127.0.0.1:") && to.starts_with("http://127.0.0.1:"))
}

/// Fetch one file of a release. The API is asked with the sign-in and answers with a forward to
/// GitHub's file host; that request, and any forward after it, is made without the sign-in.
pub fn fetch(api: &str, who: &SignIn, asset: &Asset, limit: u64, progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>, Problem> {
    if asset.size > limit { return Err(Problem::Unexpected(format!("{} is larger than any release file", asset.name))); }
    let mut r = api_get(api, &asset.url, who, "application/octet-stream")?;
    for _hop in 0..4 {
        match r.status() {
            200 => return read_all(r, Some(asset.size), limit, progress),
            301 | 302 | 303 | 307 | 308 => {
                let to = r.header("Location").unwrap_or("").to_string();
                if !forward_allowed(api, &to) {
                    return Err(Problem::Unexpected("GitHub forwarded the download to an address that is not https".into()));
                }
                r = match agent(60).get(&to).set("Accept", "application/octet-stream").call() {
                    Ok(next) => next,
                    Err(ureq::Error::Status(code, _)) => return Err(Problem::Unexpected(format!("the file host answered {code}"))),
                    Err(ureq::Error::Transport(t)) => return Err(Problem::Offline(t.to_string())),
                };
            }
            other => return Err(Problem::Unexpected(format!("GitHub answered {other}"))),
        }
    }
    Err(Problem::Unexpected("the download was forwarded too many times".into()))
}

/// The file of this release that an address in the manifest names (its last part).
pub fn asset_for<'a>(release: &'a Release, download_url: &str) -> Option<&'a Asset> {
    let name = download_url.split(['?', '#']).next()?.rsplit('/').next()?;
    release.assets.iter().find(|a| a.name == name)
}

// ---------- one check ----------

/// A newer release, found through the fallback, with the sign-in that found it.
pub struct Offer {
    pub release: Release,
    /// The updater's manifest of that release, exactly as published.
    pub manifest: Value,
    who: SignIn,
}

impl Offer {
    /// Fetch the file the manifest names at `download_url`.
    pub fn download(&self, api: &str, download_url: &str, progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>, Problem> {
        let asset = asset_for(&self.release, download_url).ok_or_else(|| Problem::Unexpected("the release does not hold the file its update list names".into()))?;
        fetch(api, &self.who, asset, INSTALLER_LIMIT, progress)
    }
}

/// What a check does once the public address has answered "not found". With the switch off it
/// asks nothing and `sign_in` is not called. `Ok(None)`: the newest release is not newer than
/// this copy.
pub fn look(api: &str, repo: &str, on: bool, current: &str, sign_in: impl FnOnce() -> Result<SignIn, Problem>) -> Result<Option<Offer>, Problem> {
    if !on { return Err(Problem::Off); }
    let who = sign_in()?;
    let release = latest_release(api, repo, &who)?;
    if !thoughtsd::release::is_newer(current, &release.tag) { return Ok(None); }
    let asset = release.assets.iter().find(|a| a.name == MANIFEST)
        .ok_or_else(|| Problem::Unexpected(format!("release {} was not built for updates (it has no {MANIFEST})", release.tag)))?;
    let bytes = fetch(api, &who, asset, MANIFEST_LIMIT, |_, _| {})?;
    let manifest = serde_json::from_slice(&bytes).map_err(|_| Problem::Unexpected(format!("the {MANIFEST} of release {} could not be read", release.tag)))?;
    Ok(Some(Offer { release, manifest, who }))
}

// ---------- why the public address had nothing ----------

/// What the public address says when it is asked plainly.
#[derive(Debug, PartialEq)]
pub enum Public {
    /// A clear "not found": the only answer that leads to the fallback.
    NotFound,
    /// No answer at all (no connection, a time-out).
    Unreachable(String),
    /// It answered, but with something else.
    Other(String),
}

/// Ask the public address once more, with no sign-in, to learn why the updater found no release
/// there. GitHub answers "not found" for a private repository and for one with no release.
pub fn probe_public(url: &str) -> Public {
    let agent = ureq::AgentBuilder::new().redirects(5).timeout(Duration::from_secs(10)).user_agent("Thoughts").build();
    match agent.get(url).set("Accept", "application/json").call() {
        Ok(r) => Public::Other(format!("the update address answered {} with something that is not a list of updates", r.status())),
        Err(ureq::Error::Status(404, _)) => Public::NotFound,
        Err(ureq::Error::Status(code, _)) => Public::Other(format!("the update address answered {code}")),
        Err(ureq::Error::Transport(t)) => Public::Unreachable(t.to_string()),
    }
}

// ---------- the terminal command ----------

const USAGE: &str = "Usage: thoughts updates gh on | off | status

  on      When no public release is found, look for updates with the sign-in of the GitHub
          command-line tool (gh). For copies of a release that is not public.
  off     Ask only the public address (the default).
  status  Say whether it is on, and whether gh can see the releases.";

/// `thoughts updates gh on | off | status`, run from a terminal before any window opens.
/// Returns the exit code: 0 fine, 1 switched on but not usable yet, 2 not understood.
pub fn cli(args: &[String]) -> i32 {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["gh", "on"] => {
            if let Err(e) = set_on_in(&switch_file(), true) {
                eprintln!("Could not switch the GitHub fallback on: {e}");
                return 1;
            }
            report(&switch_file(), &api())
        }
        ["gh", "off"] => {
            if let Err(e) = set_on_in(&switch_file(), false) {
                eprintln!("Could not switch the GitHub fallback off: {e}");
                return 1;
            }
            println!("The GitHub fallback is off. Thoughts asks only the public address for updates.");
            0
        }
        ["gh", "status"] => report(&switch_file(), &api()),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn report(switch: &Path, api: &str) -> i32 {
    if !is_on_in(switch) {
        // Off means off: gh is not even looked for.
        println!("The GitHub fallback is off. Thoughts asks only the public address for updates.");
        println!("To switch it on: thoughts updates gh on");
        return 0;
    }
    println!("The GitHub fallback is on. It is used only when no public release is found.");
    let who = match sign_in() {
        Ok(who) => who,
        Err(Problem::NoGh) => {
            println!("The GitHub command-line tool (gh) is not installed. Install it (https://cli.github.com), then run: gh auth login");
            return 1;
        }
        Err(Problem::SignedOut) => {
            println!("The GitHub command-line tool is installed but not signed in. Run: gh auth login");
            return 1;
        }
        Err(e) => {
            println!("The GitHub command-line tool could not be asked: {}", e.detail());
            return 1;
        }
    };
    match account(api, &who) {
        Some(name) => println!("Signed in to GitHub as {name}."),
        None => println!("Signed in to GitHub."),
    }
    match latest_release(api, REPO, &who) {
        Ok(r) => {
            let this = env!("CARGO_PKG_VERSION");
            let newer = if thoughtsd::release::is_newer(this, &r.tag) { "newer than this copy" } else { "not newer than this copy" };
            println!("This account can see Thoughts' releases. The newest is {} ({newer}, which is {this}).", r.tag);
            0
        }
        Err(Problem::NoRelease) => {
            println!("This account can see the repository, and nothing has been published there yet.");
            0
        }
        Err(Problem::NoAccess) => {
            println!("This account cannot see Thoughts' releases ({REPO}). Ask for access, or sign in to gh with the account that has it.");
            1
        }
        Err(Problem::Refused) => {
            println!("GitHub refused the sign-in. Run: gh auth login");
            1
        }
        Err(Problem::Offline(d)) => {
            println!("GitHub could not be reached ({d}). The fallback stays on; try again when there is a connection.");
            1
        }
        Err(e) => {
            println!("GitHub could not be asked about the releases: {}", e.detail());
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug)]
    struct Req { path: String, auth: Option<String>, accept: Option<String> }
    struct Ans { status: u16, location: Option<String>, body: Vec<u8> }
    fn ok(body: impl Into<Vec<u8>>) -> Ans { Ans { status: 200, location: None, body: body.into() } }
    fn status(status: u16) -> Ans { Ans { status, location: None, body: b"{}".to_vec() } }
    fn forward(to: String) -> Ans { Ans { status: 302, location: Some(to), body: vec![] } }

    /// A pretend GitHub on this computer: answers every request with what `answer` returns
    /// (it is given the request and the server's own address) and remembers what it was asked.
    struct Pretend { addr: String, seen: Arc<Mutex<Vec<Req>>> }
    impl Pretend {
        fn seen(&self) -> Vec<Req> { self.seen.lock().unwrap().clone() }
    }
    fn pretend(answer: impl Fn(&Req, &str) -> Ans + Send + 'static) -> Pretend {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", l.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(vec![]));
        let (log, own) = (seen.clone(), addr.clone());
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { continue };
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                if r.read_line(&mut first).unwrap_or(0) == 0 { continue; }
                let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
                let (mut auth, mut accept) = (None, None);
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" { break; }
                    let Some((k, v)) = line.split_once(':') else { continue };
                    match k.to_ascii_lowercase().as_str() {
                        "authorization" => auth = Some(v.trim().to_string()),
                        "accept" => accept = Some(v.trim().to_string()),
                        _ => {}
                    }
                }
                let req = Req { path, auth, accept };
                let a = answer(&req, &own);
                log.lock().unwrap().push(req);
                let mut head = format!("HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n", a.status, a.body.len());
                if let Some(to) = &a.location { head.push_str(&format!("Location: {to}\r\n")); }
                head.push_str("\r\n");
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&a.body);
            }
        });
        Pretend { addr, seen }
    }

    const R: &str = "someone/thoughts";
    fn who() -> SignIn { SignIn("gho_test".into()) }
    fn release_json(api: &str, tag: &str, files: &[(&str, u64, u64)]) -> String {
        let assets: Vec<Value> = files.iter().map(|(name, id, size)| serde_json::json!({"name": name, "size": size, "url": format!("{api}/repos/{R}/releases/assets/{id}"),
            "browser_download_url": format!("https://github.com/{R}/releases/download/{tag}/{name}")})).collect();
        serde_json::json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": assets}).to_string()
    }
    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-ghfeed-{}-{:?}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_switch_is_a_file_of_its_own_and_off_until_it_is_written() {
        let d = tmp();
        let f = d.join("home").join(SWITCH);
        assert!(!is_on_in(&f), "off when nothing was ever written");
        set_on_in(&f, true).unwrap();
        assert!(is_on_in(&f));
        // Only a clear yes counts: a damaged or emptied file is "off".
        std::fs::write(&f, "{\"gh\": \"yes\"}").unwrap();
        assert!(!is_on_in(&f));
        std::fs::write(&f, "garbage").unwrap();
        assert!(!is_on_in(&f));
        set_on_in(&f, true).unwrap();
        set_on_in(&f, false).unwrap();
        assert!(!is_on_in(&f) && !f.exists());
        set_on_in(&f, false).unwrap(); // switching off twice is fine
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn with_the_switch_off_nothing_is_asked_and_gh_is_never_started() {
        let api = pretend(|_, _| status(500));
        let started = std::cell::Cell::new(false);
        let r = look(&api.addr, R, false, "0.0.1", || { started.set(true); Ok(who()) });
        assert!(matches!(r, Err(Problem::Off)));
        assert!(!started.get(), "gh was started although the fallback is off");
        assert!(api.seen().is_empty(), "GitHub was asked although the fallback is off");
        assert_eq!(Problem::Off.code(), "unpublished");
    }

    #[test]
    fn a_newer_release_is_found_as_the_signed_in_person() {
        let api = pretend(|req, own| match req.path.as_str() {
            "/repos/someone/thoughts/releases/latest" => ok(release_json(own, "v0.0.13", &[("latest.json", 7, 20), ("Thoughts_0.0.13_universal.app.tar.gz", 8, 5)])),
            "/repos/someone/thoughts/releases/assets/7" => ok(r#"{"version":"0.0.13"}"#),
            _ => status(404),
        });
        let offer = look(&api.addr, R, true, "0.0.12", || Ok(who())).unwrap().expect("0.0.13 is newer than 0.0.12");
        assert_eq!(offer.release.tag, "v0.0.13");
        assert_eq!(offer.manifest["version"], "0.0.13");
        let seen = api.seen();
        assert!(seen.iter().all(|r| r.auth.as_deref() == Some("Bearer gho_test")), "every question to the API carries the sign-in: {seen:?}");
        assert_eq!(seen[1].accept.as_deref(), Some("application/octet-stream"), "the manifest is asked for as a file");
        // The same release is nothing new for a copy that already runs it, and then the manifest is not even fetched.
        let before = api.seen().len();
        assert!(look(&api.addr, R, true, "0.0.13", || Ok(who())).unwrap().is_none());
        assert_eq!(api.seen().len(), before + 1);
    }

    #[test]
    fn a_file_is_fetched_in_two_steps_and_the_sign_in_stays_with_the_api() {
        let files = pretend(|req, _| if req.path == "/signed/app.tar.gz?sig=1" { ok(b"12345".to_vec()) } else { status(404) });
        let host = files.addr.clone();
        let api = pretend(move |req, own| match req.path.as_str() {
            "/repos/someone/thoughts/releases/latest" => ok(release_json(own, "v0.0.13", &[("latest.json", 7, 20), ("Thoughts_0.0.13_universal.app.tar.gz", 8, 5)])),
            "/repos/someone/thoughts/releases/assets/7" => ok(r#"{"version":"0.0.13"}"#),
            "/repos/someone/thoughts/releases/assets/8" => forward(format!("{host}/signed/app.tar.gz?sig=1")),
            _ => status(404),
        });
        let offer = look(&api.addr, R, true, "0.0.12", || Ok(who())).unwrap().unwrap();
        let mut steps = vec![];
        let bytes = offer.download(&api.addr, "https://github.com/someone/thoughts/releases/download/v0.0.13/Thoughts_0.0.13_universal.app.tar.gz", |got, of| steps.push((got, of))).unwrap();
        assert_eq!(bytes, b"12345");
        assert_eq!(steps.last(), Some(&(5, Some(5))), "progress ends at the whole file");
        let at_host = files.seen();
        assert_eq!(at_host.len(), 1);
        assert_eq!(at_host[0].auth, None, "the sign-in was sent to the file host");
        assert!(api.seen().iter().all(|r| r.auth.is_some()));
        // A file the release does not hold cannot be asked for, whatever the manifest says.
        assert!(matches!(offer.download(&api.addr, "https://evil.example/other.tar.gz", |_, _| {}), Err(Problem::Unexpected(_))));
        assert_eq!(files.seen().len(), 1);
    }

    #[test]
    fn only_addresses_of_this_api_and_repository_are_ever_asked_with_the_sign_in() {
        let api = "http://127.0.0.1:1";
        let v: Value = serde_json::from_str(&format!(r#"{{"tag_name":"v1.0.0","assets":[
            {{"name":"latest.json","size":1,"url":"https://evil.example/repos/{R}/releases/assets/1"}},
            {{"name":"a.dmg","size":1,"url":"{api}/repos/other/repo/releases/assets/2"}},
            {{"name":"b.dmg","size":1,"url":"{api}/repos/{R}/releases/assets/3/../../../user"}},
            {{"name":"../c.dmg","size":1,"url":"{api}/repos/{R}/releases/assets/4"}},
            {{"name":"good.dmg","size":1,"url":"{api}/repos/{R}/releases/assets/5"}}]}}"#)).unwrap();
        let r = parse_release(api, R, &v).unwrap();
        assert_eq!(r.assets.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["good.dmg"]);
        // A direct attempt is refused before anything is sent.
        let outside = Asset { name: "x".into(), url: "https://evil.example/x".into(), size: 1 };
        assert!(matches!(fetch(api, &who(), &outside, 10, |_, _| {}), Err(Problem::Unexpected(_))));
        // Drafts, pre-releases and tags that are not versions are not releases to offer.
        for bad in [r#"{"tag_name":"v1.0.0","draft":true,"assets":[]}"#, r#"{"tag_name":"v1.0.0","prerelease":true,"assets":[]}"#, r#"{"tag_name":"nightly","assets":[]}"#, r#"{"assets":[]}"#] {
            assert!(parse_release(api, R, &serde_json::from_str(bad).unwrap()).is_none(), "{bad}");
        }
        // A forward may only go to an https address (or, for a pretend GitHub on this computer, back to this computer).
        assert!(forward_allowed("https://api.github.com", "https://release-assets.githubusercontent.com/x"));
        assert!(!forward_allowed("https://api.github.com", "http://release-assets.githubusercontent.com/x"));
        assert!(!forward_allowed("https://api.github.com", "http://127.0.0.1:9/x"));
        assert!(forward_allowed("http://127.0.0.1:1", "http://127.0.0.1:9/x"));
    }

    #[test]
    fn what_github_answers_becomes_a_plain_reason() {
        let ask = |latest: u16, repo: u16| {
            let api = pretend(move |req, _| if req.path.ends_with("/releases/latest") { status(latest) } else { status(repo) });
            latest_release(&api.addr, R, &who())
        };
        assert_eq!(ask(401, 200), Err(Problem::Refused));
        assert_eq!(ask(404, 404), Err(Problem::NoAccess), "the account cannot see the repository");
        assert_eq!(ask(404, 200), Err(Problem::NoRelease), "it can, and nothing is published");
        assert!(matches!(ask(500, 200), Err(Problem::Unexpected(_))));
        // Nobody listening: no connection.
        let closed = { let l = TcpListener::bind("127.0.0.1:0").unwrap(); format!("http://{}", l.local_addr().unwrap()) };
        assert!(matches!(latest_release(&closed, R, &who()), Err(Problem::Offline(_))));
        for (p, code) in [(Problem::NoGh, "gh-missing"), (Problem::SignedOut, "gh-signed-out"), (Problem::Refused, "gh-refused"), (Problem::NoAccess, "gh-no-access"),
            (Problem::NoRelease, "gh-no-release"), (Problem::Offline("x".into()), "offline"), (Problem::Unexpected("x".into()), "failed")] {
            assert_eq!(p.code(), code);
        }
    }

    #[test]
    fn a_download_that_is_cut_short_or_too_large_is_refused() {
        let api = pretend(|req, _| if req.path.ends_with("/assets/8") { ok(b"123".to_vec()) } else { status(404) });
        let asset = |size| Asset { name: "a.tar.gz".into(), url: format!("{}/repos/{R}/releases/assets/8", api.addr), size };
        assert_eq!(fetch(&api.addr, &who(), &asset(3), 10, |_, _| {}).unwrap(), b"123");
        assert!(matches!(fetch(&api.addr, &who(), &asset(5), 10, |_, _| {}), Err(Problem::Offline(_))), "fewer bytes than the release lists");
        assert!(matches!(fetch(&api.addr, &who(), &asset(2), 10, |_, _| {}), Err(Problem::Offline(_))), "more bytes than the release lists");
        let before = api.seen().len();
        assert!(matches!(fetch(&api.addr, &who(), &asset(11), 10, |_, _| {}), Err(Problem::Unexpected(_))), "larger than the limit: not even asked for");
        assert_eq!(api.seen().len(), before);
    }

    #[test]
    fn only_a_clear_not_found_from_the_public_address_leads_to_the_fallback() {
        let answers = |code: u16| { let p = pretend(move |_, _| status(code)); probe_public(&format!("{}/latest.json", p.addr)) };
        assert_eq!(answers(404), Public::NotFound);
        assert!(matches!(answers(500), Public::Other(_)));
        assert!(matches!(answers(403), Public::Other(_)));
        assert!(matches!(answers(200), Public::Other(_)), "an answer the updater could not read is not a 'not found'");
        let closed = { let l = TcpListener::bind("127.0.0.1:0").unwrap(); format!("http://{}/latest.json", l.local_addr().unwrap()) };
        assert!(matches!(probe_public(&closed), Public::Unreachable(_)), "no connection is not a 'not found'");
        // The public address is asked with no sign-in at all.
        let p = pretend(|_, _| status(404));
        probe_public(&format!("{}/latest.json", p.addr));
        assert_eq!(p.seen()[0].auth, None);
    }

    #[test]
    fn the_file_to_fetch_is_the_one_the_manifest_names() {
        let r = Release { tag: "v1.0.0".into(), assets: vec![
            Asset { name: "latest.json".into(), url: "u1".into(), size: 1 },
            Asset { name: "Thoughts_1.0.0_universal.app.tar.gz".into(), url: "u2".into(), size: 2 }] };
        assert_eq!(asset_for(&r, "https://github.com/o/r/releases/download/v1.0.0/Thoughts_1.0.0_universal.app.tar.gz").map(|a| a.url.as_str()), Some("u2"));
        assert_eq!(asset_for(&r, "https://github.com/o/r/releases/download/v1.0.0/Thoughts_1.0.0_universal.app.tar.gz?x=1#y").map(|a| a.url.as_str()), Some("u2"));
        assert!(asset_for(&r, "https://github.com/o/r/releases/download/v1.0.0/Other.app.tar.gz").is_none());
        assert!(asset_for(&r, "").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn the_sign_in_comes_from_gh_or_the_reason_it_cannot() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp();
        let gh = |name: &str, script: &str| {
            let p = d.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        // What it is asked, and that a token lying in the environment is not handed on.
        let asked = gh("asked", r#"[ "$*" = "auth token --hostname github.com" ] && [ -z "$GH_TOKEN$GITHUB_TOKEN" ] && echo gho_fromgh"#);
        std::env::set_var("GH_TOKEN", "from-the-environment");
        let got = sign_in_with(&asked);
        std::env::remove_var("GH_TOKEN");
        assert_eq!(got.ok().map(|s| s.0).as_deref(), Some("gho_fromgh"));
        assert!(matches!(sign_in_with(&gh("out", "echo 'no oauth token found' >&2; exit 1")), Err(Problem::SignedOut)));
        assert!(matches!(sign_in_with(&gh("empty", "exit 0")), Err(Problem::SignedOut)));
        assert!(matches!(sign_in_with(&gh("odd", "echo 'two words'")), Err(Problem::Unexpected(_))));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_command_understands_three_words_and_nothing_else() {
        let run = |words: &[&str]| cli(&words.iter().map(|w| w.to_string()).collect::<Vec<_>>());
        for bad in [&[][..], &["gh"], &["gh", "maybe"], &["on"], &["gh", "on", "extra"], &["github", "on"]] {
            assert_eq!(run(bad), 2, "{bad:?}");
        }
        // With the switch off the report says so and looks no further (no gh, no network).
        let d = tmp();
        assert_eq!(report(&d.join(SWITCH), "http://127.0.0.1:1"), 0);
        std::fs::remove_dir_all(d).unwrap();
    }
}
