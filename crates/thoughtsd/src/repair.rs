//! The health of a library's history, and its repair.
//!
//! Some states of the history stop the app from recording or syncing: a combine of changes
//! (a sync's rebase or merge, an undo) that stopped halfway, a history that is not on a branch, a
//! lock left behind by a program that is gone, an index git can no longer read, records that are
//! damaged. `check` names each one (the window explains it in plain words), `repair` puts it right
//! and `rebuild` starts a damaged history again.
//!
//! Nothing is lost on the way. Before `repair` changes anything, the library exactly as it is in
//! the folder — half-done files included — is kept as a copy in the history (a `backup/…` branch,
//! made without touching the index git may be stuck on), and the answer lists every document that
//! now differs from that copy so the person can compare and bring back any of them. `rebuild`
//! leaves the folder's files alone and sets the damaged history aside instead of deleting it.

use crate::git::{self, Repo};
use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A lock older than this cannot belong to a command the app runs (each one is stopped after
/// `git::LOCAL_TIMEOUT`): it was left behind.
pub const LOCK_STALE: Duration = Duration::from_secs(git::LOCAL_TIMEOUT.as_secs() + 30);

/// The combine of changes that stopped halfway in the history at `g`: `rebase` (a sync receiving
/// changes), `merge` (two devices' changes being combined) or `undo` (an undo or a copied change).
pub fn operation(g: &Path) -> Option<&'static str> {
    if g.join("rebase-merge").is_dir() || g.join("rebase-apply").is_dir() { return Some("rebase"); }
    if g.join("MERGE_HEAD").is_file() { return Some("merge"); }
    if g.join("REVERT_HEAD").is_file() || g.join("CHERRY_PICK_HEAD").is_file() || g.join("sequencer").is_dir() { return Some("undo"); }
    None
}

/// The combine that stopped halfway in the history of the library at `dir`, if any (files only,
/// no program is run: cheap enough to ask before every recording).
pub fn unfinished(dir: &Path) -> Option<&'static str> { operation(&crate::health::git_dir(dir)?) }

/// How long ago the history's lock was taken, when there is one.
fn lock_age(g: &Path, now: SystemTime) -> Option<Duration> {
    let m = std::fs::symlink_metadata(g.join("index.lock")).ok().filter(|m| m.is_file())?;
    Some(now.duration_since(m.modified().ok()?).unwrap_or_default())
}

/// Can git read the index (its list of what each recorded file looks like)?
async fn index_readable(state: &AppState, dir: &Path, g: Option<&Path>) -> bool {
    if g.map_or(false, |g| !g.join("index").exists()) { return true; }
    // A name that matches nothing: the whole index is still read and checked.
    match git::git(state, dir, &["ls-files", "--", ":(top,literal).thoughts-index-check"]).await {
        Ok(_) => true,
        Err(e) => {
            let m = e.to_string().to_lowercase();
            !(m.contains("index") && (m.contains("corrupt") || m.contains("signature") || m.contains("smaller than") || m.contains("unable to read") || m.contains("bad")))
        }
    }
}

/// Is this the failure of a damaged repository (and not a slow computer or a missing git)?
fn damage(e: &anyhow::Error) -> bool { e.downcast_ref::<crate::tr::UserErr>().is_none() }

/// What is wrong with the history of the library at `dir`:
/// `{ok, problems: [{id, stale?, branch?}]}` with `id` one of `locked` (`stale`: left behind, so it
/// can be removed), `rebase`, `merge`, `undo`, `index`, `detached` (not on a branch), `internal` (on
/// one of the app's own branches, `branch`) and `damaged` (records that cannot be read; only looked
/// for when `thorough`, since it reads the whole history). A library without a history is fine.
pub async fn check(state: &AppState, dir: &Path, thorough: bool) -> anyhow::Result<Value> {
    if git::repo_of(dir) == Repo::Missing { return Ok(json!({"ok": true, "problems": []})); }
    let g = crate::health::git_dir(dir);
    // A repository git no longer recognises at all (its HEAD unreadable, say) is damaged.
    if let Err(e) = git::git(state, dir, &["rev-parse", "--git-dir"]).await {
        if damage(&e) { return Ok(json!({"ok": false, "problems": [{"id": "damaged"}]})); }
        return Err(e);
    }
    let mut problems: Vec<Value> = vec![];
    if let Some(age) = g.as_deref().and_then(|g| lock_age(g, SystemTime::now())) {
        problems.push(json!({"id": "locked", "stale": age >= LOCK_STALE}));
    }
    let op = g.as_deref().and_then(operation);
    if let Some(op) = op { problems.push(json!({"id": op})); }
    let index_ok = index_readable(state, dir, g.as_deref()).await;
    if !index_ok { problems.push(json!({"id": "index"})); }
    // While a rebase runs the history is off its branch on purpose: undoing the rebase puts it back.
    if op.is_none() {
        match git::git(state, dir, &["symbolic-ref", "-q", "HEAD"]).await {
            Ok(r) => {
                let b = r.trim().strip_prefix("refs/heads/").unwrap_or(r.trim()).to_string();
                if git::internal_branch(&b) { problems.push(json!({"id": "internal", "branch": b})); }
            }
            Err(e) if damage(&e) => problems.push(json!({"id": "detached"})),
            Err(e) => return Err(e),
        }
    }
    if thorough && index_ok {
        if let Err(e) = git::git(state, dir, &["fsck", "--no-dangling", "--no-reflogs", "--no-progress"]).await {
            if damage(&e) {
                crate::log::warn(format!("history: the check found damaged records ({})", e.to_string().lines().take(3).collect::<Vec<_>>().join(" ")));
                problems.push(json!({"id": "damaged"}));
            }
        }
    }
    Ok(json!({"ok": problems.is_empty(), "problems": problems}))
}

fn ids(v: &Value) -> Vec<String> {
    v["problems"].as_array().map(|a| a.iter().filter_map(|p| p["id"].as_str().map(String::from)).collect()).unwrap_or_default()
}

/// Keep the library exactly as it is in the folder (everything git does not ignore, half-done
/// files included) as a new `backup/…` branch, without using or changing the repository's index.
/// Returns `(the commit, the branch)`.
async fn snapshot(state: &AppState, dir: &Path, g: Option<&Path>, index_ok: bool) -> anyhow::Result<(String, String)> {
    let tmp = std::env::temp_dir().join(format!("thoughts-snapshot-{}", uuid::Uuid::new_v4()));
    crate::paths::create_private_dir(&tmp)?;
    let index = tmp.join("index");
    // Starting from the repository's own index (when it can be read) spares reading every file again.
    if index_ok { if let Some(g) = g { let _ = std::fs::copy(g.join("index"), &index); } }
    let made = async {
        git::git_with_index(state, dir, &index, &["add", "-A"]).await?;
        let tree = git::git_with_index(state, dir, &index, &["write-tree"]).await?.trim().to_string();
        let parent = git::git(state, dir, &["rev-parse", "--verify", "-q", "HEAD^{commit}"]).await.ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let msg = git::subject::edit("the library as it was before the history was repaired");
        let mut args = vec!["commit-tree", tree.as_str(), "-m", msg.as_str()];
        if let Some(p) = &parent { args.extend(["-p", p.as_str()]); }
        let commit = match git::git_ident(state, dir, &args).await {
            Ok(c) => c,
            // The commit it stood on cannot be read: the copy stands on its own.
            Err(_) if parent.is_some() => git::git_ident(state, dir, &["commit-tree", tree.as_str(), "-m", msg.as_str()]).await?,
            Err(e) => return Err(e),
        };
        let commit = commit.trim().to_string();
        let branch = git::make_backup_at(state, dir, &commit).await?;
        anyhow::Ok((commit, branch))
    }.await;
    let _ = std::fs::remove_dir_all(&tmp);
    made
}

/// The documents whose text in the folder now differs from the copy `snapshot` (changed, or gone
/// since), for the person to compare and bring back.
async fn differs(state: &AppState, dir: &Path, snapshot: &str) -> Vec<String> {
    let out = git::git(state, dir, &["diff", "--name-status", "-z", "--no-renames", snapshot, "--"]).await.unwrap_or_default();
    let mut it = out.split('\0');
    let mut list = vec![];
    while let (Some(status), Some(path)) = (it.next(), it.next()) {
        // A document that exists now but not in the copy lost nothing.
        if matches!(status.trim().chars().next(), Some('M' | 'D' | 'T')) && list.len() < 500 { list.push(path.to_string()); }
    }
    list
}

/// The branch to go back to: the one a stopped rebase was on (`hint`), else `main`, `master` or
/// the first of the person's own branches. None when there is none.
async fn branch_to_use(state: &AppState, dir: &Path, hint: Option<&str>) -> Option<String> {
    let out = git::git(state, dir, &["for-each-ref", "--format=%(refname:short)", "refs/heads/"]).await.unwrap_or_default();
    let mine: Vec<&str> = out.lines().map(str::trim).filter(|b| !b.is_empty() && !git::internal_branch(b)).collect();
    [hint.unwrap_or(""), "main", "master"].iter().find(|b| !b.is_empty() && mine.contains(b)).map(|b| b.to_string())
        .or_else(|| mine.first().map(|b| b.to_string()))
}

/// Put the history back on the person's branch. A detached history that only went further than
/// the branch takes the branch along (nothing to lose); otherwise the branch is checked out as
/// it was recorded and the snapshot holds whatever differed.
async fn reattach(state: &AppState, dir: &Path, hint: Option<&str>) -> anyhow::Result<()> {
    let current = git::git(state, dir, &["symbolic-ref", "-q", "--short", "HEAD"]).await.ok().map(|s| s.trim().to_string());
    if current.as_deref().map_or(false, |b| !git::internal_branch(b)) { return Ok(()); }
    let head = git::head(state, dir).await;
    let Some(target) = branch_to_use(state, dir, hint).await else {
        // No branch of the person's own at all: one is made where the history is.
        git::git(state, dir, &["checkout", "-q", "-B", "main"]).await?;
        return Ok(());
    };
    let tip = git::git(state, dir, &["rev-parse", "--verify", "-q", &format!("refs/heads/{target}^{{commit}}")]).await?.trim().to_string();
    let r = format!("refs/heads/{target}");
    if head.as_deref() == Some(tip.as_str()) {
        git::git(state, dir, &["symbolic-ref", "HEAD", &r]).await?;
    } else if let (None, Some(h)) = (&current, &head) {
        if git::git(state, dir, &["merge-base", "--is-ancestor", &tip, h]).await.is_ok() {
            // Detached further along the same line: the branch moves forward to it.
            git::git(state, dir, &["update-ref", &r, h, &tip]).await?;
            git::git(state, dir, &["symbolic-ref", "HEAD", &r]).await?;
        } else {
            git::git(state, dir, &["checkout", "-q", "-f", &target]).await?;
        }
    } else {
        git::git(state, dir, &["checkout", "-q", "-f", &target]).await?;
    }
    Ok(())
}

/// Undo a combine that stopped halfway, trying the gentle way first. The snapshot already holds
/// the folder as it was, so the last resort may set the folder back to the last recorded state.
async fn settle_operation(state: &AppState, dir: &Path, g: &Path, op: &str) -> anyhow::Result<Option<String>> {
    async fn ok(state: &AppState, dir: &Path, args: &[&str]) -> bool { git::git(state, dir, args).await.is_ok() }
    let mut hint = None;
    match op {
        "rebase" => {
            // Which branch the rebase was on, before its notes are gone.
            hint = ["rebase-merge/head-name", "rebase-apply/head-name"].iter()
                .find_map(|f| std::fs::read_to_string(g.join(f)).ok())
                .map(|s| s.trim().trim_start_matches("refs/heads/").to_string()).filter(|s| !s.is_empty() && s != "detached HEAD");
            if !ok(state, dir, &["rebase", "--abort"]).await && !ok(state, dir, &["am", "--abort"]).await {
                let _ = git::git(state, dir, &["rebase", "--quit"]).await;
                for d in ["rebase-merge", "rebase-apply"] {
                    let p = g.join(d);
                    if p.is_dir() { std::fs::remove_dir_all(&p)?; }
                }
            }
        }
        "merge" => {
            if !ok(state, dir, &["merge", "--abort"]).await && !ok(state, dir, &["reset", "-q", "--merge"]).await {
                git::git(state, dir, &["reset", "-q", "--hard", "HEAD"]).await?;
            }
            for f in ["MERGE_HEAD", "MERGE_MSG", "MERGE_MODE", "AUTO_MERGE"] {
                let p = g.join(f);
                if p.is_file() { std::fs::remove_file(&p)?; }
            }
        }
        _ => {
            if !ok(state, dir, &["revert", "--abort"]).await && !ok(state, dir, &["cherry-pick", "--abort"]).await {
                let _ = git::git(state, dir, &["revert", "--quit"]).await;
                let _ = git::git(state, dir, &["cherry-pick", "--quit"]).await;
                let _ = git::git(state, dir, &["reset", "-q", "--hard", "HEAD"]).await;
            }
            for f in ["REVERT_HEAD", "CHERRY_PICK_HEAD"] {
                let p = g.join(f);
                if p.is_file() { std::fs::remove_file(&p)?; }
            }
            let seq = g.join("sequencer");
            if seq.is_dir() { std::fs::remove_dir_all(&seq)?; }
        }
    }
    Ok(hint)
}

/// Put the history of the library at `dir` right (see the module comment). `{ok, repaired: [ids],
/// differs: [paths], snapshot, problems}`: `snapshot` is the copy kept just before (read a document
/// from it like any version), `differs` the documents that now differ from it. With damaged
/// records nothing is changed: the answer says `rebuild` (see `rebuild`). Fails without changing
/// anything when another program is using the history right now (`history.in_use`).
pub async fn repair(state: &AppState, dir: &Path) -> anyhow::Result<Value> {
    let _g = state.git_lock.lock().await;
    let before = check(state, dir, true).await?;
    let found = ids(&before);
    if found.is_empty() { return Ok(json!({"ok": true, "repaired": [], "differs": [], "snapshot": null, "problems": []})); }
    if found.iter().any(|i| i == "damaged") {
        return Ok(json!({"ok": false, "rebuild": true, "repaired": [], "differs": [], "snapshot": null, "problems": before["problems"]}));
    }
    let g = crate::health::git_dir(dir).ok_or_else(|| crate::tr::err("no_history", &[]))?;
    let locked = before["problems"].as_array().and_then(|a| a.iter().find(|p| p["id"] == "locked")).cloned();
    if locked.as_ref().map_or(false, |l| l["stale"] != true) { return Err(crate::tr::err("history.in_use", &[])); }
    crate::trust::require_git(dir)?;
    let index_ok = !found.iter().any(|i| i == "index");
    let (snap, branch) = snapshot(state, dir, Some(&g), index_ok).await?;
    crate::log::info(format!("history repair: found {}; the library as it was is kept as {branch}", found.join(", ")));
    if locked.is_some() {
        let lock = g.join("index.lock");
        if lock.is_file() { std::fs::remove_file(&lock)?; }
    }
    if !index_ok {
        // The index is only git's list of what it last saw: it is made again from the last entry.
        std::fs::remove_file(g.join("index"))?;
        if git::head(state, dir).await.is_some() { git::git(state, dir, &["reset", "-q"]).await?; }
    }
    let mut hint = None;
    if let Some(op) = operation(&g) { hint = settle_operation(state, dir, &g, op).await?; }
    reattach(state, dir, hint.as_deref()).await?;
    let after = check(state, dir, false).await?;
    let differ = differs(state, dir, &snap).await;
    crate::log::info(format!("history repair: done ({} document(s) differ from the copy kept before)", differ.len()));
    Ok(json!({"ok": after["ok"], "repaired": found, "differs": differ, "snapshot": snap, "problems": after["problems"]}))
}

/// Start the history of the library at `dir` again, for records too damaged to repair. The files in
/// the folder are not touched. The damaged history is set aside beside the new one (never deleted).
/// A library with an online location takes the new history from there and records what differs
/// here as the person's changes, so the next sync goes on as usual; when the location cannot be
/// reached nothing is changed and that is said (`repair.offline`). Otherwise the history begins
/// again from the library as it is now.
pub async fn rebuild(state: &AppState, dir: &Path) -> anyhow::Result<Value> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    match git::repo_of(dir) {
        Repo::Missing => Err(crate::tr::err("no_history", &[])),
        Repo::Kept(store) => {
            {
                let _g = state.git_lock.lock().await;
                let aside = aside_of(&store, &stamp);
                std::fs::rename(&store, &aside)?;
                crate::log::info("history rebuild: the damaged history kept by the app was set aside");
            }
            git::start_history(state, dir).await?;
            Ok(json!({"ok": true, "rebuilt": true, "fromLocation": false}))
        }
        Repo::Own => {
            let _g = state.git_lock.lock().await;
            crate::trust::require_git(dir)?;
            let own = dir.join(".git");
            if !std::fs::symlink_metadata(&own).map_or(false, |m| m.is_dir()) { return Err(crate::tr::err("repair.cannot", &[])); }
            // What is needed from the old history: its online locations and its branch.
            let conf = own.join("config").to_string_lossy().to_string();
            let listed = git::git(state, dir, &["config", "-f", &conf, "--get-regexp", r"^remote\..*\.url$"]).await.unwrap_or_default();
            let mut remotes: Vec<(String, String)> = vec![];
            for l in listed.lines() {
                let Some((key, url)) = l.split_once(' ') else { continue };
                if let Some(name) = key.strip_prefix("remote.").and_then(|k| k.strip_suffix(".url")) {
                    if !remotes.iter().any(|(n, _)| n == name) { remotes.push((name.to_string(), url.trim().to_string())); }
                }
            }
            let branch = std::fs::read_to_string(own.join("HEAD")).ok()
                .and_then(|h| h.trim().strip_prefix("ref: refs/heads/").map(String::from))
                .filter(|b| !b.is_empty() && !git::internal_branch(b)).unwrap_or_else(|| "main".into());
            let aside = dir.join(format!(".git-damaged-{stamp}"));
            std::fs::rename(&own, &aside)?;
            let made = start_again(state, dir, &branch, &remotes).await;
            match made {
                Ok(from_location) => {
                    crate::log::info(format!("history rebuild: started again{}; the damaged history was set aside in the library folder", if from_location { " from the online location" } else { "" }));
                    Ok(json!({"ok": true, "rebuilt": true, "fromLocation": from_location}))
                }
                Err(e) => {
                    // Back exactly as it was: the new history goes, the old one returns.
                    if own.is_dir() && aside.is_dir() { std::fs::remove_dir_all(&own)?; }
                    std::fs::rename(&aside, &own)?;
                    Err(e)
                }
            }
        }
    }
}

/// `<store>-damaged-<stamp>`, beside the store.
fn aside_of(store: &Path, stamp: &str) -> PathBuf {
    let name = store.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "history".into());
    store.with_file_name(format!("{name}-damaged-{stamp}"))
}

/// A new repository in `dir` on `branch` with the old `remotes`; from the main location's copy when
/// there is one. Returns whether the history came from there.
async fn start_again(state: &AppState, dir: &Path, branch: &str, remotes: &[(String, String)]) -> anyhow::Result<bool> {
    git::git(state, dir, &["init", "-q", "-b", branch]).await?;
    // The damaged history stays in the folder; the new one never records it.
    let info = dir.join(".git/info");
    std::fs::create_dir_all(&info)?;
    let exclude = info.join("exclude");
    let mut text = std::fs::read_to_string(&exclude).unwrap_or_default();
    text.push_str("\n# Written by Thoughts: a damaged history set aside.\n/.git-damaged-*/\n");
    std::fs::write(&exclude, text)?;
    for (name, url) in remotes { git::git(state, dir, &["remote", "add", "--", name, url]).await?; }
    let chosen = state.settings.read().await.sync.main_location.clone();
    let names: Vec<&str> = remotes.iter().map(|(n, _)| n.as_str()).collect();
    let mut from_location = false;
    if let Some(main) = git::pick_main(&names, chosen.as_deref()) {
        match git::git(state, dir, &["fetch", "--", main, branch]).await {
            Ok(_) => {
                // The history is the location's; the folder stays as it is, and what differs is recorded as the person's own.
                git::git(state, dir, &["reset", "-q", "FETCH_HEAD"]).await?;
                from_location = true;
            }
            Err(e) if e.to_string().contains("couldn't find remote ref") => {}
            Err(e) => {
                crate::log::warn(format!("history rebuild: the online location could not be reached ({e})"));
                return Err(crate::tr::err("repair.offline", &[]));
            }
        }
    }
    let msg = if from_location { git::subject::edit("your changes") } else { git::subject::start() };
    git::commit_all(state, dir, &msg).await?;
    Ok(from_location)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git").current_dir(dir)
            .args(["-c", "user.name=Tester", "-c", "user.email=tester@example.com", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
            .args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }
    /// Plain git that is expected to fail (arranging a stopped rebase or merge).
    fn sh_fails(dir: &Path, args: &[&str]) {
        let o = std::process::Command::new("git").current_dir(dir)
            .args(["-c", "user.name=Tester", "-c", "user.email=tester@example.com", "-c", "commit.gpgsign=false", "-c", "core.editor=true"])
            .args(args).output().unwrap();
        assert!(!o.status.success(), "git {args:?} was expected to stop");
    }
    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn read(dir: &Path, rel: &str) -> String { std::fs::read_to_string(dir.join(rel)).unwrap_or_default() }

    /// A library with its own repository: notes.md recorded twice on main, and a branch `other`
    /// (another device's work) that changed the same line.
    fn library() -> PathBuf {
        let d = dunce::canonicalize(std::env::temp_dir()).unwrap().join(format!("thoughts-repair-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        sh(&d, &["init", "-q", "-b", "main"]);
        for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com")] { sh(&d, &["config", k, v]); }
        write(&d, "notes.md", "first\n");
        write(&d, "keep.md", "untouched\n");
        sh(&d, &["add", "-A"]);
        sh(&d, &["commit", "-q", "-m", "one"]);
        sh(&d, &["checkout", "-q", "-b", "other"]);
        write(&d, "notes.md", "theirs\n");
        sh(&d, &["commit", "-q", "-am", "theirs"]);
        sh(&d, &["checkout", "-q", "main"]);
        write(&d, "notes.md", "mine\n");
        sh(&d, &["commit", "-q", "-am", "mine"]);
        d
    }
    fn problems(v: &Value) -> Vec<String> { ids(v) }

    #[tokio::test]
    async fn a_healthy_history_and_no_history_have_nothing_to_report() {
        let st = AppState::for_test();
        let d = library();
        let v = check(&st, &d, true).await.unwrap();
        assert_eq!(v, json!({"ok": true, "problems": []}));
        let v = repair(&st, &d).await.unwrap();
        assert_eq!((v["ok"].as_bool(), v["snapshot"].is_null()), (Some(true), true), "nothing to repair: nothing is made");
        assert_eq!(sh(&d, &["branch", "--list", "backup/*"]), "");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn a_stopped_merge_is_undone_and_what_was_in_the_folder_is_kept() {
        let st = AppState::for_test();
        let d = library();
        let mine = sh(&d, &["rev-parse", "HEAD"]);
        sh_fails(&d, &["merge", "other"]);
        // The person (or the app) went on editing while the merge was stuck.
        write(&d, "keep.md", "edited while stuck\n");
        assert_eq!(unfinished(&d), Some("merge"));
        // Nothing is recorded into the half-done merge.
        assert_eq!(crate::tr::classify_in(false, &git::commit_all(&st, &d, "edit: x").await.unwrap_err()).code, "history_state");
        let v = check(&st, &d, true).await.unwrap();
        assert_eq!(problems(&v), ["merge"]);
        let r = repair(&st, &d).await.unwrap();
        assert_eq!((r["ok"].as_bool(), r["repaired"].clone()), (Some(true), json!(["merge"])), "{r}");
        assert_eq!(unfinished(&d), None);
        assert_eq!(sh(&d, &["rev-parse", "HEAD"]), mine);
        assert_eq!(read(&d, "notes.md"), "mine\n");
        // The copy holds the folder exactly as it was, half-done file and all, on a backup branch.
        let snap = r["snapshot"].as_str().unwrap();
        assert!(sh(&d, &["cat-file", "-p", &format!("{snap}:notes.md")]).contains("<<<<<<<"));
        assert_eq!(sh(&d, &["cat-file", "-p", &format!("{snap}:keep.md")]), "edited while stuck");
        assert_eq!(sh(&d, &["branch", "--list", "backup/*", "--contains", snap]).lines().count(), 1);
        // The gentle undo kept the edit made meanwhile in place; the half-done document is listed.
        assert_eq!(read(&d, "keep.md"), "edited while stuck\n");
        assert_eq!(r["differs"], json!(["notes.md"]));
        // And it can be brought back from the copy, like any version.
        git::restore_file(&st, &d, "notes.md", snap).await.unwrap();
        assert!(read(&d, "notes.md").contains("<<<<<<<"));
        assert!(check(&st, &d, true).await.unwrap()["ok"] == true);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn a_stopped_rebase_goes_back_to_its_branch() {
        let st = AppState::for_test();
        let d = library();
        let mine = sh(&d, &["rev-parse", "HEAD"]);
        sh_fails(&d, &["rebase", "other"]);
        assert_eq!(unfinished(&d), Some("rebase"));
        assert_eq!(problems(&check(&st, &d, false).await.unwrap()), ["rebase"]);
        let r = repair(&st, &d).await.unwrap();
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(sh(&d, &["symbolic-ref", "--short", "HEAD"]), "main");
        assert_eq!(sh(&d, &["rev-parse", "HEAD"]), mine);
        assert_eq!(read(&d, "notes.md"), "mine\n");
        assert_eq!(sh(&d, &["status", "--porcelain"]), "");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn a_detached_history_returns_to_its_branch_and_keeps_what_was_done_meanwhile() {
        let st = AppState::for_test();
        let d = library();
        let tip = sh(&d, &["rev-parse", "HEAD"]);
        // Detached further along the same line (changes recorded while off the branch): the branch follows.
        sh(&d, &["checkout", "-q", "--detach"]);
        write(&d, "notes.md", "recorded while detached\n");
        git::commit_all(&st, &d, "edit: your changes").await.unwrap();
        let ahead = sh(&d, &["rev-parse", "HEAD"]);
        assert_eq!(problems(&check(&st, &d, false).await.unwrap()), ["detached"]);
        assert_eq!(crate::tr::classify_in(false, &git::current_branch(&st, &d).await.unwrap_err()).code, "history_state");
        let r = repair(&st, &d).await.unwrap();
        assert_eq!(r["ok"], true);
        assert_eq!((sh(&d, &["symbolic-ref", "--short", "HEAD"]), sh(&d, &["rev-parse", "main"])), ("main".into(), ahead.clone()));
        assert_eq!(r["differs"], json!([]), "nothing differs: nothing was lost");
        // Detached at an older point with its own changes: back on the branch, and the changes are listed.
        sh(&d, &["checkout", "-q", "--detach", &format!("{tip}~1")]);
        write(&d, "keep.md", "made on an old point\n");
        let r = repair(&st, &d).await.unwrap();
        assert_eq!(sh(&d, &["symbolic-ref", "--short", "HEAD"]), "main");
        assert_eq!(sh(&d, &["rev-parse", "HEAD"]), ahead);
        assert_eq!(read(&d, "keep.md"), "untouched\n");
        assert!(r["differs"].as_array().unwrap().contains(&json!("keep.md")), "{r}");
        let snap = r["snapshot"].as_str().unwrap();
        assert_eq!(sh(&d, &["cat-file", "-p", &format!("{snap}:keep.md")]), "made on an old point");
        // On one of the app's own branches: back to the person's.
        sh(&d, &["checkout", "-q", "-b", "backup/20260101-000000"]);
        assert_eq!(check(&st, &d, false).await.unwrap()["problems"], json!([{"id": "internal", "branch": "backup/20260101-000000"}]));
        repair(&st, &d).await.unwrap();
        assert_eq!(sh(&d, &["symbolic-ref", "--short", "HEAD"]), "main");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn a_lock_is_removed_only_once_it_was_left_behind() {
        let st = AppState::for_test();
        let d = library();
        let lock = d.join(".git/index.lock");
        std::fs::write(&lock, "").unwrap();
        assert_eq!(check(&st, &d, false).await.unwrap()["problems"], json!([{"id": "locked", "stale": false}]));
        // Fresh: another program may be using the history right now. Nothing is changed.
        let e = repair(&st, &d).await.unwrap_err();
        assert_eq!(crate::tr::classify_in(false, &e).code, "history_state");
        assert!(lock.exists());
        assert_eq!(sh(&d, &["branch", "--list", "backup/*"]), "");
        // Old: left behind by a program that is gone.
        let old = SystemTime::now() - LOCK_STALE - Duration::from_secs(5);
        std::fs::File::options().write(true).open(&lock).unwrap().set_modified(old).unwrap();
        assert_eq!(check(&st, &d, false).await.unwrap()["problems"], json!([{"id": "locked", "stale": true}]));
        let r = repair(&st, &d).await.unwrap();
        assert_eq!(r["ok"], true);
        assert!(!lock.exists());
        write(&d, "notes.md", "can record again\n");
        assert!(git::commit_all(&st, &d, "edit: x").await.unwrap());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn an_unreadable_index_is_made_again_without_touching_the_files() {
        let st = AppState::for_test();
        let d = library();
        write(&d, "notes.md", "not recorded yet\n");
        std::fs::write(d.join(".git/index"), b"DIRC garbage that is not an index").unwrap();
        assert!(problems(&check(&st, &d, true).await.unwrap()).contains(&"index".to_string()));
        let r = repair(&st, &d).await.unwrap();
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(read(&d, "notes.md"), "not recorded yet\n");
        assert_eq!(sh(&d, &["status", "--porcelain"]), "M notes.md");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn damaged_records_start_the_history_again_and_the_old_one_is_set_aside() {
        let st = AppState::for_test();
        let d = library();
        // Damage the record of one document's text.
        let blob = sh(&d, &["rev-parse", "HEAD:keep.md"]);
        let obj = d.join(".git/objects").join(&blob[..2]).join(&blob[2..]);
        let mut perm = std::fs::metadata(&obj).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        std::fs::set_permissions(&obj, perm).unwrap();
        std::fs::write(&obj, b"not a git object").unwrap();
        let v = check(&st, &d, true).await.unwrap();
        assert!(problems(&v).contains(&"damaged".to_string()), "{v}");
        // Repair alone changes nothing: it says the history must start again.
        let r = repair(&st, &d).await.unwrap();
        assert_eq!((r["rebuild"].as_bool(), r["snapshot"].is_null()), (Some(true), true));
        write(&d, "notes.md", "still here\n");
        let r = rebuild(&st, &d).await.unwrap();
        assert_eq!((r["rebuilt"].as_bool(), r["fromLocation"].as_bool()), (Some(true), Some(false)));
        // The files are as they were, the history begins with them, the old one is kept aside.
        assert_eq!((read(&d, "notes.md"), read(&d, "keep.md")), ("still here\n".into(), "untouched\n".into()));
        assert_eq!(check(&st, &d, true).await.unwrap()["ok"], true);
        assert_eq!(sh(&d, &["log", "--format=%s"]), git::subject::start());
        assert_eq!(sh(&d, &["status", "--porcelain"]), "", "the damaged history is not recorded");
        let aside: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(".git-damaged-")).collect();
        assert_eq!(aside.len(), 1);
        assert!(aside[0].path().join("objects").is_dir());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[tokio::test]
    async fn a_damaged_history_kept_by_the_app_starts_again_and_the_old_one_is_set_aside() {
        let st = AppState::for_test();
        let lib = dunce::canonicalize(std::env::temp_dir()).unwrap().join(format!("thoughts-repair-kept-{}", uuid::Uuid::new_v4()));
        write(&lib, "notes.md", "one\n");
        git::start_history(&st, &lib).await.unwrap();
        let Repo::Kept(store) = git::repo_of(&lib) else { panic!("expected a history kept by the app") };
        // Its record of where it stands is unreadable: git no longer recognises it at all.
        std::fs::write(store.join("HEAD"), "not a reference").unwrap();
        write(&lib, "notes.md", "edited since\n");
        assert_eq!(problems(&check(&st, &lib, false).await.unwrap()), ["damaged"]);
        assert_eq!(repair(&st, &lib).await.unwrap()["rebuild"], true);
        let r = rebuild(&st, &lib).await.unwrap();
        assert_eq!((r["rebuilt"].as_bool(), r["fromLocation"].as_bool()), (Some(true), Some(false)));
        // The folder is untouched (nothing of git's in it), the new history starts from it, the old one is beside it.
        assert_eq!(read(&lib, "notes.md"), "edited since\n");
        assert!(!lib.join(".git").exists());
        assert_eq!(check(&st, &lib, true).await.unwrap()["ok"], true);
        assert_eq!(git::git(&st, &lib, &["log", "--format=%s"]).await.unwrap().trim(), git::subject::start());
        let name = store.file_name().unwrap().to_string_lossy().to_string();
        let aside: Vec<PathBuf> = std::fs::read_dir(store.parent().unwrap()).unwrap().flatten().map(|e| e.path())
            .filter(|p| p.file_name().map_or(false, |n| n.to_string_lossy().starts_with(&format!("{name}-damaged-")))).collect();
        assert_eq!(aside.len(), 1);
        assert!(aside[0].join("objects").is_dir());
        std::fs::remove_dir_all(&aside[0]).unwrap();
        std::fs::remove_dir_all(&store).unwrap();
        std::fs::remove_dir_all(&lib).unwrap();
    }

    #[tokio::test]
    async fn a_rebuild_takes_the_history_from_the_online_location_or_changes_nothing() {
        let st = AppState::for_test();
        let d = library();
        let root = d.parent().unwrap().join(format!("thoughts-repair-remote-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        sh(&root, &["init", "-q", "--bare", "-b", "main"]);
        sh(&d, &["remote", "add", "origin", root.to_str().unwrap()]);
        sh(&d, &["push", "-q", "origin", "main"]);
        let pushed = sh(&d, &["rev-parse", "HEAD"]);
        write(&d, "notes.md", "changed after the last sync\n");
        // Unreachable location: nothing changes at all.
        let real = root.with_extension("moved");
        std::fs::rename(&root, &real).unwrap();
        let e = rebuild(&st, &d).await.unwrap_err();
        assert_eq!(crate::tr::classify_in(false, &e).code, "offline");
        assert_eq!(sh(&d, &["rev-parse", "HEAD"]), pushed);
        assert!(std::fs::read_dir(&d).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(".git-damaged-")));
        std::fs::rename(&real, &root).unwrap();
        // Reachable: the history is the location's, and what differs here is one change on top.
        let r = rebuild(&st, &d).await.unwrap();
        assert_eq!(r["fromLocation"], true);
        assert_eq!(sh(&d, &["rev-parse", "HEAD~1"]), pushed);
        assert_eq!(sh(&d, &["log", "-1", "--format=%s"]), "edit: your changes");
        assert_eq!(read(&d, "notes.md"), "changed after the last sync\n");
        assert_eq!(sh(&d, &["remote"]), "origin");
        std::fs::remove_dir_all(&d).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }
}
