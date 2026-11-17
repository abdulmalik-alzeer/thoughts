//! "N changes waiting": a look, every so often, at whether the library's main backup location has
//! changes from the user's other computers that this one does not have yet.
//!
//! - **Only a look.** It fetches what the location has into a reference of the app's own
//!   (`refs/thoughts/incoming/<location>`) and counts what it holds that the library does not. It
//!   never merges, rebases, checks out or changes a file: the changes come in when the user presses
//!   Sync, exactly as before.
//! - **Off by default**, per library (`sync.checkRemote`, "Check for changes from your other
//!   computers"), like every other automatic use of the network. Only while a window of the app is
//!   open, and only for the open library when it is sent to a location.
//! - **Never asks anything** (`git::git_quiet`): no password prompt helper, SSH in batch mode. A
//!   location that wants a sign-in, or a computer that is offline, only leaves a quiet "couldn't check"
//!   in Settings, and the next look is put off longer each time (`delay_after`), back to every 15
//!   minutes after one that works or after a Sync.
//! - **Never in the way.** A round is skipped while the library is held (a wrap, a sync, an undo). The
//!   fetch itself holds no lock: it writes no `FETCH_HEAD` and only its own reference, which nothing
//!   else touches, so a save or a Sync started meanwhile goes ahead and is never confused by it.

use crate::state::AppState;
use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the location is looked at while things work.
pub const EVERY: Duration = Duration::from_secs(15 * 60);
/// The longest the next look is put off after failures.
pub const MAX_DELAY: Duration = Duration::from_secs(4 * 3600);
/// How often the loop wakes to see whether a look is due.
const TICK: Duration = Duration::from_secs(20);
/// A round skipped because the library was busy is tried again this soon.
const BUSY_RETRY: Duration = Duration::from_secs(60);

/// When the next look is due after `failures` failed ones in a row: every 15 minutes, then 30, 60,
/// 120, and at most every 4 hours.
pub fn delay_after(failures: u32) -> Duration {
    let d = EVERY.saturating_mul(1u32 << failures.min(8));
    d.min(MAX_DELAY)
}

/// The reference the look fetches the location `name`'s branch into. Only this module writes it.
pub fn incoming_ref(name: &str) -> String { format!("refs/thoughts/incoming/{name}") }

/// A count `git rev-list --count` printed.
pub fn parse_count(out: &str) -> Option<u64> { out.trim().parse::<u64>().ok() }

/// What is known about one library.
#[derive(Clone, Debug, Default)]
struct Seen {
    failures: u32,
    /// `None`: look at the next tick.
    due: Option<Instant>,
    waiting: u64,
    /// "unknown" (not looked yet) | "ok" | "failed"
    state: &'static str,
    /// Why the last look failed: "offline" | "sign_in" | "timeout" | "other".
    why: Option<&'static str>,
    checked: Option<chrono::DateTime<chrono::Utc>>,
    /// The location the count is about.
    location: String,
    /// The library changed since the count was made: count again (locally) at the next tick.
    recount: bool,
}

fn seen() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Seen>> {
    static S: std::sync::OnceLock<std::sync::Mutex<HashMap<PathBuf, Seen>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn key(lib: &Path) -> PathBuf { crate::paths::canonical_lenient(lib) }

/// Windows of the app connected to the event stream right now.
static WINDOWS: AtomicUsize = AtomicUsize::new(0);

/// Held by each window's event stream (`api::pump`) while it is connected: looks happen only then.
pub struct WindowOpen;
impl WindowOpen {
    pub fn new() -> Self { WINDOWS.fetch_add(1, Ordering::SeqCst); WindowOpen }
}
impl Drop for WindowOpen {
    fn drop(&mut self) { WINDOWS.fetch_sub(1, Ordering::SeqCst); }
}
impl Default for WindowOpen { fn default() -> Self { Self::new() } }

/// How one look went.
#[derive(Debug, PartialEq)]
pub enum Round {
    /// Nothing to look at now (no location, a drive not plugged in, not on a branch…).
    Skipped,
    /// The location holds this many changes the library does not have.
    Checked { location: String, waiting: u64 },
    /// The location could not be reached or would not answer without a sign-in.
    Failed(&'static str),
}

/// How many changes `reference` holds that the library's HEAD does not (all of them when nothing is
/// recorded here yet). `None` when the reference is not there.
pub async fn waiting_in(state: &AppState, dir: &Path, reference: &str) -> Option<u64> {
    let spec = format!("{reference}^{{commit}}");
    crate::git::git(state, dir, &["rev-parse", "--verify", "-q", &spec]).await.ok()?;
    let range = if crate::git::head(state, dir).await.is_some() { format!("HEAD..{reference}") } else { reference.to_string() };
    crate::git::git(state, dir, &["rev-list", "--count", &range]).await.ok().and_then(|o| parse_count(&o))
}

/// Look once at the main location of the library at `dir`. Changes nothing but the app's own
/// reference (see the module notes).
pub async fn look(state: &AppState, dir: &Path) -> Round {
    let rs = crate::git::remotes(state, dir).await;
    let Some(main) = rs.iter().find(|r| r["main"] == true).or(rs.first()) else { return Round::Skipped };
    let (name, url, kind) = (main["name"].as_str().unwrap_or(""), main["url"].as_str().unwrap_or(""), main["kind"].as_str().unwrap_or("online"));
    if name.is_empty() || name.starts_with('-') { return Round::Skipped; }
    // A drive that is not plugged in is not a failure: there is simply nothing to look at.
    if kind == "folder" && !crate::git::folder_present(url).await { return Round::Skipped; }
    // The same check as a sync: the library's git settings must still be the ones the app trusts.
    if crate::trust::require_git(dir).is_err() { return Round::Skipped; }
    let Ok(branch) = crate::git::current_branch(state, dir).await else { return Round::Skipped };
    if crate::git::internal_branch(&branch) { return Round::Skipped; }
    let reference = incoming_ref(name);
    let refspec = format!("+refs/heads/{branch}:{reference}");
    let args = ["-c", "maintenance.auto=false", "-c", "gc.auto=0", "-c", "fetch.writeCommitGraph=false",
                "fetch", "-q", "--no-write-fetch-head", "--no-tags", "--no-recurse-submodules", "--", name, &refspec];
    match crate::git::git_quiet(state, dir, &args).await {
        Ok(_) => {}
        // The location does not have this branch yet: nothing is waiting there.
        Err(e) if e.to_string().contains("couldn't find remote ref") => return Round::Checked { location: name.into(), waiting: 0 },
        Err(e) => {
            crate::log::info(format!("incoming: could not look at {name} ({})", crate::tr::classify_in(false, &e).code));
            return Round::Failed(why_of(&e));
        }
    }
    match waiting_in(state, dir, &reference).await {
        Some(n) => Round::Checked { location: name.into(), waiting: n },
        None => Round::Failed("other"),
    }
}

/// Why a look failed, in the few kinds Settings tells apart.
fn why_of(e: &anyhow::Error) -> &'static str {
    match crate::tr::classify_in(false, e).code {
        "offline" => "offline",
        "sign_in" => "sign_in",
        "timeout" => "timeout",
        _ => "other",
    }
}

/// What is known for the library at `lib`, as the window shows it.
fn view(on: bool, s: Option<&Seen>) -> Value {
    let s = s.cloned().unwrap_or_default();
    json!({
        "on": on,
        "waiting": if on { s.waiting } else { 0 },
        "state": if !on { "off" } else if s.state.is_empty() { "unknown" } else { s.state },
        "why": s.why,
        "checkedAt": s.checked,
        "location": s.location,
    })
}

/// Is the look switched on for the library at `lib`, and is there anywhere to look?
async fn wanted(state: &AppState, lib: &Path) -> bool {
    let s = state.settings.read().await;
    let sync = s.sync_of(lib);
    sync.check_remote && sync.mode.starts_with("git")
}

fn tell(state: &AppState, lib: &Path, v: &Value) {
    let mut e = v.clone();
    e["type"] = json!("incoming.changed");
    e["library"] = json!(lib.to_string_lossy());
    state.emit(e);
}

/// One pass of the loop for the open library.
async fn round(state: &AppState) {
    let lib = { state.settings.read().await.library() };
    let Some(lib) = lib.filter(|l| l.is_dir()) else { return };
    let k = key(&lib);
    if !wanted(state, &lib).await {
        // Switched off: forget what was counted, and say so once.
        if seen().remove(&k).is_some() { tell(state, &lib, &view(false, None)); }
        return;
    }
    let now = Instant::now();
    let (due, recount) = { let s = seen(); s.get(&k).map_or((None, false), |s| (s.due, s.recount)) };
    // The library changed here (a Sync took the changes in, say): count again, without the network.
    if recount { recount_now(state, &lib).await; }
    if WINDOWS.load(Ordering::SeqCst) == 0 { return; }
    if due.map_or(false, |d| d > now) { return; }
    // A wrap, a sync, an undo or a repair is changing the library: not now.
    if state.library_busy_at(&lib) || state.library_lock.syncing_at(&lib) {
        seen().entry(k).or_default().due = Some(now + BUSY_RETRY);
        return;
    }
    let r = look(state, &lib).await;
    let shown = {
        let mut map = seen();
        let s = map.entry(k).or_default();
        let before = (s.waiting, s.state);
        match r {
            Round::Skipped => { s.due = Some(Instant::now() + EVERY); }
            Round::Checked { location, waiting } => {
                *s = Seen { failures: 0, due: Some(Instant::now() + EVERY), waiting, state: "ok", why: None,
                            checked: Some(chrono::Utc::now()), location, recount: false };
            }
            Round::Failed(why) => {
                s.failures = s.failures.saturating_add(1);
                s.due = Some(Instant::now() + delay_after(s.failures));
                s.state = "failed";
                s.why = Some(why);
            }
        }
        (before != (s.waiting, s.state)).then(|| view(true, Some(s)))
    };
    if let Some(v) = shown { tell(state, &lib, &v); }
}

/// Count what is waiting again from what was last fetched (no network).
async fn recount_now(state: &AppState, lib: &Path) {
    let k = key(lib);
    let location = { let mut m = seen(); let Some(s) = m.get_mut(&k) else { return }; s.recount = false; s.location.clone() };
    if location.is_empty() { return; }
    let Some(n) = waiting_in(state, lib, &incoming_ref(&location)).await else { return };
    let shown = {
        let mut m = seen();
        let Some(s) = m.get_mut(&k) else { return };
        let changed = s.waiting != n;
        s.waiting = n;
        changed.then(|| view(true, Some(s)))
    };
    if let Some(v) = shown { tell(state, lib, &v); }
}

/// The user synced the library at `lib`: what was waiting has come in, and a look that kept failing
/// starts again from every 15 minutes.
pub async fn synced(state: &AppState, lib: &Path) {
    {
        let mut m = seen();
        let Some(s) = m.get_mut(&key(lib)) else { return };
        s.failures = 0;
        s.due = Some(Instant::now() + EVERY);
        if s.state == "failed" { s.state = if s.location.is_empty() { "unknown" } else { "ok" }; s.why = None; }
    }
    recount_now(state, lib).await;
}

/// Start the loop.
pub fn start(state: Arc<AppState>) {
    let mut events = state.events.subscribe();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(TICK) => {}
                e = events.recv() => match e {
                    Ok(v) => match v["type"].as_str().unwrap_or("") {
                        // Switched on or off, another library opened: act on it now.
                        "settings.changed" | "library.switched" => {}
                        // The library changed: the count may have (a Sync, an undo). Done at the next tick.
                        "library.changed" | "history.changed" => {
                            if let Some(lib) = state.settings.read().await.library() {
                                if let Some(s) = seen().get_mut(&key(&lib)) { s.recount = true; }
                            }
                            continue;
                        }
                        _ => continue,
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return,
                },
            }
            round(&state).await;
        }
    });
}

/// `GET /api/git/incoming` → `{on, waiting, state: off|unknown|ok|failed, why, checkedAt, location}`
/// for the open library.
async fn get_incoming(State(st): State<Arc<AppState>>) -> Json<Value> {
    let lib = st.settings.read().await.library();
    let Some(lib) = lib else { return Json(view(false, None)) };
    let on = wanted(&st, &lib).await;
    let s = seen().get(&key(&lib)).cloned();
    Json(view(on, s.as_ref()))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/git/incoming", get(get_incoming))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain git for arranging test repositories (the code under test goes through `git`).
    mod t {
        pub fn sh(dir: &std::path::Path, args: &[&str]) -> String {
            let o = std::process::Command::new("git").current_dir(dir)
                .args(["-c", "user.name=Tester", "-c", "user.email=tester@example.com", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
                .args(args).output().unwrap();
            assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
    }

    #[test]
    fn looks_are_put_off_longer_after_each_failure_and_never_past_four_hours() {
        let m = |n: u64| Duration::from_secs(n * 60);
        assert_eq!(delay_after(0), m(15));
        assert_eq!(delay_after(1), m(30));
        assert_eq!(delay_after(2), m(60));
        assert_eq!(delay_after(3), m(120));
        assert_eq!(delay_after(4), m(240));
        assert_eq!(delay_after(5), m(240));
        assert_eq!(delay_after(u32::MAX), MAX_DELAY);
    }

    #[test]
    fn counts_are_read_plainly() {
        assert_eq!(parse_count("3\n"), Some(3));
        assert_eq!(parse_count(" 0 "), Some(0));
        assert_eq!(parse_count(""), None);
        assert_eq!(parse_count("x"), None);
        assert_eq!(incoming_ref("origin"), "refs/thoughts/incoming/origin");
    }

    #[test]
    fn the_check_is_off_unless_switched_on() {
        let s = crate::settings::Settings::default();
        assert!(!s.sync.check_remote);
        assert!(!crate::settings::SyncSettings::default().check_remote);
        // A settings file from before the switch existed reads as off too.
        let old: crate::settings::SyncSettings = serde_json::from_str(r#"{"mode":"git-manual"}"#).unwrap();
        assert!(!old.check_remote);
        let v = view(false, None);
        assert_eq!((v["on"].as_bool(), v["state"].as_str(), v["waiting"].as_u64()), (Some(false), Some("off"), Some(0)));
    }

    /// A temp library with a bare location holding two changes the library does not have.
    async fn two_ahead() -> (crate::state::AppState, PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("thoughts-incoming-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let (remote, lib, other) = (root.join("remote.git"), root.join("lib"), root.join("other"));
        t::sh(&root, &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()]);
        std::fs::create_dir_all(&lib).unwrap();
        t::sh(&lib, &["init", "-q", "-b", "main"]);
        std::fs::write(lib.join("a.md"), "a\n").unwrap();
        t::sh(&lib, &["add", "-A"]);
        t::sh(&lib, &["commit", "-q", "-m", "first"]);
        t::sh(&lib, &["remote", "add", "origin", remote.to_str().unwrap()]);
        t::sh(&lib, &["push", "-q", "origin", "main"]);
        t::sh(&root, &["clone", "-q", remote.to_str().unwrap(), other.to_str().unwrap()]);
        for n in ["b", "c"] {
            std::fs::write(other.join(format!("{n}.md")), "x\n").unwrap();
            t::sh(&other, &["add", "-A"]);
            t::sh(&other, &["commit", "-q", "-m", n]);
        }
        t::sh(&other, &["push", "-q", "origin", "main"]);
        let st = crate::state::AppState::for_test();
        (st, lib, other, root)
    }

    #[tokio::test]
    async fn a_look_counts_what_the_other_computer_sent_and_changes_nothing_here() {
        let (st, lib, _other, _tmp) = two_ahead().await;
        let head = t::sh(&lib, &["rev-parse", "HEAD"]);
        let r = look(&st, &lib).await;
        assert_eq!(r, Round::Checked { location: "origin".into(), waiting: 2 });
        // Only a look: the branch, the files and what git last fetched are as they were.
        assert_eq!(t::sh(&lib, &["rev-parse", "HEAD"]), head);
        assert!(!lib.join("b.md").exists());
        assert!(!lib.join(".git/FETCH_HEAD").exists(), "the look must not write FETCH_HEAD");
        assert!(t::sh(&lib, &["status", "--porcelain"]).trim().is_empty());
        // Once the changes are taken in, nothing is waiting any more.
        t::sh(&lib, &["merge", "-q", "--ff-only", "refs/thoughts/incoming/origin"]);
        assert_eq!(waiting_in(&st, &lib, &incoming_ref("origin")).await, Some(0));
    }

    #[tokio::test]
    async fn a_location_that_cannot_be_reached_is_a_quiet_failure() {
        let (st, lib, _other, root) = two_ahead().await;
        t::sh(&lib, &["remote", "set-url", "origin", root.join("gone.git").to_str().unwrap()]);
        // A folder location that is not there is not even tried (an unplugged drive).
        assert_eq!(look(&st, &lib).await, Round::Skipped);
        // An address that answers nothing fails, without asking anyone anything.
        t::sh(&lib, &["remote", "set-url", "origin", "https://127.0.0.1:9/nothing.git"]);
        assert!(matches!(look(&st, &lib).await, Round::Failed(_)));
    }

    #[tokio::test]
    async fn a_busy_library_is_skipped_and_tried_again_soon() {
        let (st, lib, _other, _tmp) = two_ahead().await;
        {
            let mut s = st.settings.write().await;
            s.library_path = Some(lib.to_string_lossy().to_string());
            s.sync.mode = "git-manual".into();
            s.sync.check_remote = true;
        }
        let _window = WindowOpen::new();
        let held = st.library_lock.of(&lib);
        let g = held.lock().await;
        round(&st).await;
        {
            let m = seen();
            let s = m.get(&key(&lib)).expect("noted");
            assert_eq!(s.state, "", "nothing was looked at while the library was held");
            assert!(s.due.unwrap() > Instant::now() + Duration::from_secs(30));
        }
        drop(g);
        seen().get_mut(&key(&lib)).unwrap().due = None;
        round(&st).await;
        let m = seen();
        let s = m.get(&key(&lib)).unwrap();
        assert_eq!((s.state, s.waiting), ("ok", 2));
    }
}
