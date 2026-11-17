//! HTTP handlers for Settings → Maintenance: the start-up self-check and its fixes, the
//! newer-version check, moving to another computer, and removing Thoughts from this computer.

use crate::api::ApiErr;
use crate::chats::Chat;
use crate::health::{self, Facts};
use crate::removal::{self, Keyring, SystemKeyring};
use crate::settings::{Project, Settings};
use crate::state::AppState;
use crate::{applayer, git, paths, release, transfer};
use axum::extract::{DefaultBodyLimit, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

type S = State<Arc<AppState>>;
type R = Result<Json<Value>, ApiErr>;

/// What the window sends once the user typed the confirmation word.
pub const REMOVE_CONFIRM: &str = "remove-thoughts";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/health", get(health_get))
        .route("/api/health/fix", post(health_fix))
        .route("/api/release-check", post(release_check))
        .route("/api/move/export", post(move_export))
        .route("/api/move/inspect", post(move_inspect).layer(DefaultBodyLimit::max(transfer::MAX_BODY)))
        .route("/api/move/import", post(move_import).layer(DefaultBodyLimit::max(transfer::MAX_BODY)))
        .route("/api/remove/plan", get(remove_plan))
        .route("/api/remove", post(remove))
}

/// Is anything running that must not be cut off (a chat, a Wrap, a reorganize)?
async fn working(st: &AppState) -> bool { st.library_busy() || !st.runs.lock().await.is_empty() }

// ---------- self-check ----------

async fn issues(st: &AppState) -> Vec<health::Issue> {
    let s = st.settings.read().await.clone();
    let git_installed = if s.history_enabled() { Some(crate::api::git_tool_status().await["installed"] == true) } else { None };
    // The keychain and the login item are shared with the real app: a sandbox copy never looks.
    let key_readable = if s.ai.claude.has_api_token && removal::shares_with_real_app() {
        Some(tokio::task::spawn_blocking(|| SystemKeyring.get("thoughts-ai", "claude").is_some()).await.unwrap_or(true))
    } else { None };
    let s2 = s.clone();
    let login_item = tokio::task::spawn_blocking(move || crate::api::login_item_drift(&s2)).await.ok().flatten();
    health::check(&Facts {
        settings: &s, chats_dir: &paths::chats_dir(), unreadable_at_start: st.chats.unreadable,
        git_installed, key_readable, login_item, now: std::time::SystemTime::now(),
    })
}

async fn health_get(State(st): S) -> R { Ok(Json(json!({"issues": issues(&st).await}))) }

#[derive(Deserialize)]
struct Fix { id: String }

async fn health_fix(State(st): S, Json(f): Json<Fix>) -> R {
    run_fix(&st, &f.id).await?;
    Ok(Json(json!({"ok": true, "issues": issues(&st).await})))
}

async fn run_fix(st: &AppState, id: &str) -> anyhow::Result<()> {
    match id {
        "use-folder-sync" => {
            let mut cur = st.settings.write().await;
            let mut next = cur.clone();
            next.sync.mode = "folder".into();
            next.save()?;
            *cur = next;
            drop(cur);
            crate::log::info("self-check: the backup was switched from an online location to the cloud folder the library is in");
            st.emit(json!({"type": "settings.changed"}));
        }
        "finish-history" | "unlock-history" | "repair-history" => {
            let lib = st.settings.read().await.library().ok_or_else(|| crate::tr::err("library.none", &[]))?;
            // As History's repair: never under an assistant at work in the library, and with the
            // library held for the whole repair, so no wrap, sync or edit changes it halfway.
            if st.run_active_in(&lib) { return Err(crate::tr::err("busy.run", &[])); }
            let _held = st.library_lock.of(&lib).try_lock_owned().map_err(|_| crate::tr::err("busy", &[]))?;
            st.drain_edits().await?;
            // The same repair as History's: a copy of the library as it is now is kept first.
            let r = crate::repair::repair(st, &lib).await?;
            crate::log::info(format!("self-check: history repaired ({})", r["repaired"]));
            st.emit(json!({"type": "library.changed"}));
            st.emit(json!({"type": "history.changed"}));
        }
        "set-aside-chats" => {
            let n = health::set_aside(&health::unreadable_chats(&paths::chats_dir()), &paths::home().join("chats-set-aside"))?;
            crate::log::info(format!("self-check: set {n} unreadable chat file(s) aside"));
        }
        "repair-login-item" => {
            let s = st.settings.read().await.clone();
            tokio::task::spawn_blocking(move || crate::api::repair_login_item(&s)).await??;
        }
        _ => return Err(crate::tr::err("care.unknown_fix", &[])),
    }
    Ok(())
}

// ---------- newer version ----------

/// "Check for updates" in a copy whose built-in updater is off: `{state: newer|current|unknown, version?, url?}`.
async fn release_check() -> R {
    let (s, told) = release::check_now(&paths::home()).await;
    let v = release::newer_value(&s, env!("CARGO_PKG_VERSION"));
    let state = if !v.is_null() { "newer" } else if told { "current" } else { "unknown" };
    Ok(Json(json!({"state": state, "version": v["version"], "url": v["url"]})))
}

// ---------- moving to another computer ----------

async fn all_chats(st: &AppState) -> Vec<Chat> { st.chats.chats.read().await.values().cloned().collect() }

async fn move_export(State(st): S) -> R {
    let s = st.settings.read().await.clone();
    let chats = all_chats(&st).await;
    let (v, info) = tokio::task::spawn_blocking(move || {
        let (mut v, mut info) = transfer::export(&s, chats, &paths::attachments_dir(), &paths::app_layer_dir());
        transfer::add_addon_data(&mut v, &mut info, &paths::home());
        (v, info)
    }).await?;
    let text = serde_json::to_vec(&v)?;
    let path = tokio::task::spawn_blocking(move || transfer::save_export(&transfer::export_dir(), &text)).await??;
    crate::log::info(format!("move: exported {} chat(s) and {} attachment(s)", info.chats, info.attachments));
    Ok(Json(json!({
        "path": path, "name": path.file_name().map(|n| n.to_string_lossy().to_string()),
        "folder": path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()),
        "info": info,
    })))
}

/// A move file, checked. Errors are the plain "not a move file" / "made by a newer version".
fn read_bundle(v: &Value) -> anyhow::Result<transfer::Bundle> {
    transfer::read(v).map_err(|e| {
        crate::log::warn(format!("move: refused a file ({e})"));
        crate::tr::err(if e.to_string() == "MOVE_BAD::newer" { "move.newer" } else { "move.bad" }, &[])
    })
}

#[derive(Deserialize)]
struct Inspect { bundle: Value }

async fn move_inspect(State(st): S, Json(i): Json<Inspect>) -> R {
    let b = read_bundle(&i.bundle)?;
    let here = st.chats.chats.read().await;
    let existing = b.chats.iter().filter(|c| here.contains_key(&c.id)).count();
    let running = b.chats.iter().filter(|c| here.get(&c.id).map_or(false, |h| h.running)).count();
    drop(here);
    let cur = st.settings.read().await.clone();
    let projects: Vec<Value> = transfer::projects_in(&b).iter().map(|p| {
        let same = cur.projects.iter().find(|q| paths::canonical_lenient(Path::new(&q.path)) == paths::canonical_lenient(Path::new(&p.path)));
        json!({"id": p.id, "name": p.name, "path": p.path, "here": Path::new(&p.path).is_dir(), "linked": same.map(|q| &q.name)})
    }).collect();
    Ok(Json(json!({
        "appVersion": b.app_version, "createdAt": b.created_at,
        "chats": b.chats.len(), "existing": existing, "running": running,
        "attachments": b.attachments.values().map(BTreeMap::len).sum::<usize>(),
        "settings": b.settings.is_some(),
        "customisations": b.customisations.as_ref().map_or(0, |p| custom_changes(p, &paths::app_layer_dir())),
        "projects": projects,
        "addonData": b.addon_data.len(),
        "addonDataExisting": b.addon_data.keys().filter(|n| has_addon_data(&paths::home(), n)).count(),
    })))
}

/// Has add-on `name` saved anything on this computer?
fn has_addon_data(home: &Path, name: &str) -> bool {
    crate::layer_api::storage_read(home, name).map_or(false, |v| v.as_object().map_or(false, |o| !o.is_empty()))
}

/// How many of a pack's files differ from the customisation layer here (a file identical to the
/// one already in use needs no review).
fn custom_changes(pack: &Value, layer: &Path) -> usize {
    let mine = crate::packs::collect(layer);
    pack["files"].as_object().map_or(0, |f| f.iter().filter(|(p, v)| mine.get(p.as_str()).map(String::as_str) != v.as_str()).count())
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Choices {
    settings: bool,
    chats: bool,
    customisations: bool,
    /// Replace chats that are already on this computer (otherwise they are left as they are).
    replace: bool,
    /// Linked folder id in the file -> the folder on this computer ("" or missing: leave it out).
    projects: BTreeMap<String, String>,
    /// Bring what add-ons saved on the other computer.
    addon_data: bool,
    /// …also for add-ons that have saved data here (what is here is kept as an earlier copy).
    addon_data_replace: bool,
}

#[derive(Deserialize)]
struct Import { bundle: Value, choices: Choices }

/// Is chat id `id`, new here, already used in any letter case — by a chat, its saved file or an
/// attachments folder (a disk that ignores case finds `ABC` under `abc`)?
async fn id_taken(st: &AppState, attachments_dir: &Path, id: &str) -> bool {
    st.chats.list().await.iter().any(|m| m.id.eq_ignore_ascii_case(id))
        || std::fs::symlink_metadata(attachments_dir.join(id)).is_ok()
        || std::fs::symlink_metadata(paths::chats_dir().join(format!("{id}.json"))).is_ok()
}

async fn move_import(State(st): S, Json(i): Json<Import>) -> R {
    let b = read_bundle(&i.bundle)?;
    Ok(Json(import(&st, b, &i.choices, &paths::home()).await?))
}

/// Import a checked file. Linked folders are checked first (a folder must exist and be trusted),
/// so a refusal there leaves everything as it was.
async fn import(st: &AppState, b: transfer::Bundle, ch: &Choices, home: &Path) -> anyhow::Result<Value> {
    let attachments_dir = home.join("attachments");
    let cur = st.settings.read().await.clone();
    // 1. Linked folders: the old id -> a folder on this computer (an existing link to it, or a new one).
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    let mut new_projects: Vec<Project> = vec![];
    if ch.chats {
        for p in transfer::projects_in(&b) {
            let Some(raw) = ch.projects.get(&p.id).map(|s| s.trim()).filter(|s| !s.is_empty()) else { continue };
            let dir = paths::expand_tilde(raw)?;
            if !dir.is_absolute() || !dir.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", raw)])); }
            let dir = paths::canonical(&dir)?;
            crate::trust::require(&dir)?;
            let path = dir.to_string_lossy().to_string();
            if let Some(q) = cur.projects.iter().chain(&new_projects).find(|q| paths::canonical_lenient(Path::new(&q.path)) == dir) {
                map.insert(p.id.clone(), q.id.clone());
                continue;
            }
            let taken = cur.projects.iter().chain(&new_projects).any(|q| q.id == p.id);
            let id = if taken { uuid::Uuid::new_v4().to_string() } else { p.id.clone() };
            map.insert(p.id.clone(), id.clone());
            new_projects.push(Project { id, name: p.name.chars().filter(|c| !c.is_control()).take(120).collect(), path });
        }
    }
    // 2. Settings (and the new linked folders) in one save.
    let mut settings_done = false;
    if (ch.settings && b.settings.is_some()) || !new_projects.is_empty() {
        let mut guard = st.settings.write().await;
        let old = guard.clone();
        let mut next = match (&b.settings, ch.settings) {
            (Some(from), true) => {
                let mut n = transfer::merged_settings(&old, from);
                n.sanitize_update(&old)?;
                settings_done = true;
                n
            }
            _ => old.clone(),
        };
        next.projects.extend(new_projects.iter().cloned());
        next.save()?;
        crate::tr::set_language(&next.language);
        crate::claude::set_reports(next.assistant_reports);
        *guard = next;
        drop(guard);
        st.emit(json!({"type": "settings.changed"}));
    }
    // 3. Chats.
    let (mut added, mut replaced, mut skipped, mut files) = (0usize, 0usize, 0usize, 0usize);
    let here = crate::wraps::Scope::now(&st).await.key();
    if ch.chats {
        for c in b.chats {
            let mut c = c;
            // Its attachments in the file, whatever id it ends up with here.
            let in_file = c.id.clone();
            let existing = st.chats.meta(&c.id).await;
            if existing.as_ref().map_or(false, |h| !ch.replace || h.running) { skipped += 1; continue; }
            // A new chat whose id is another chat's here in other letter case, or whose files would
            // land where something already is (on a disk that ignores case, the same place): it gets
            // an id of its own, so nothing of another chat is ever replaced.
            if existing.is_none() && id_taken(st, &attachments_dir, &c.id).await { c.id = uuid::Uuid::new_v4().to_string(); }
            // What comes in is written first, beside the chat already here: a write that fails (a
            // full disk) leaves that chat and its files exactly as they were.
            let empty = BTreeMap::new();
            let staged = transfer::stage_attachments(&attachments_dir, &c.id, b.attachments.get(&in_file).unwrap_or(&empty))?;
            if existing.is_some() {
                st.chats.remove(&c.id).await;
                replaced += 1;
            } else {
                added += 1;
            }
            let present: BTreeSet<String> = transfer::place_attachments(&attachments_dir, &c.id, staged, existing.is_some())?;
            files += present.len();
            let project = c.project_id.as_ref().and_then(|p| map.get(p).cloned());
            // A moved chat belongs to the library open here (the file holds one library's chats).
            let mut c = transfer::localised(c, project, &attachments_dir, &present);
            c.library = here.clone();
            st.chats.insert(c).await;
        }
        st.emit(json!({"type": "chat.updated"}));
    }
    // 4. Customisations: staged for the usual review, never switched on here.
    let mut custom = "none";
    let mut custom_error = None;
    if ch.customisations {
        if let Some(pack) = b.customisations.as_ref().filter(|p| custom_changes(p, &applayer::Dirs::at(home).layer) > 0) {
            let _g = crate::layer_api::LAYER_LOCK.lock().await;
            let d = applayer::Dirs::at(home);
            let ar = st.settings.read().await.language == "ar";
            let staged = async { applayer::ensure_in(applayer::Git(Some(st)), &d).await?; crate::packs::import(applayer::Git(Some(st)), &d, pack, ar).await };
            match staged.await {
                Ok(()) => { custom = "staged"; st.emit(json!({"type": "layer.pending"})); }
                Err(e) => { custom = "failed"; custom_error = Some(crate::tr::classify(&e).message); }
            }
        }
    }
    // 5. What add-ons saved: added where there is none here; replacing only when asked, and then
    //    what is here is kept as an earlier copy the add-on's data can be put back to.
    let (mut data_added, mut data_replaced, mut data_skipped) = (0usize, 0usize, 0usize);
    if ch.addon_data {
        for (name, data) in &b.addon_data {
            let here = has_addon_data(home, name);
            if here && !ch.addon_data_replace { data_skipped += 1; continue; }
            crate::layer_api::storage_replace(home, name, data)?;
            if here { data_replaced += 1 } else { data_added += 1 }
        }
    }
    crate::log::info(format!("move: imported {added} new and {replaced} replaced chat(s), skipped {skipped}; settings {settings_done}; customisations {custom}; add-on data {data_added} new, {data_replaced} replaced, {data_skipped} kept"));
    Ok(json!({"chatsAdded": added, "chatsReplaced": replaced, "chatsSkipped": skipped, "attachments": files,
              "settings": settings_done, "projects": new_projects.len(), "customisations": custom, "customisationsError": custom_error,
              "addonData": {"added": data_added, "replaced": data_replaced, "skipped": data_skipped}}))
}

// ---------- removing Thoughts from this computer ----------

async fn user_folders(s: &Settings) -> (Option<PathBuf>, Vec<PathBuf>) {
    // Every other library in the switcher, and their linked folders, are left alone just the same.
    (s.library().filter(|_| s.library_path.as_deref().map_or(false, |p| !p.trim().is_empty())), s.projects.iter().map(|p| PathBuf::from(&p.path)).chain(s.other_folders()).collect())
}

/// What "Remove Thoughts from this computer" would do, for the confirmation screen.
async fn remove_plan(State(st): S) -> R {
    let s = st.settings.read().await.clone();
    let home = paths::home();
    let (library, linked) = user_folders(&s).await;
    let p = removal::plan(&home, library.as_deref(), &linked);
    Ok(Json(json!({
        "dataFolder": home, "blocked": p.blocked, "unknown": p.left.len(),
        "library": library, "folders": s.projects.iter().map(|p| json!({"name": p.name, "path": p.path}))
            .chain(s.libraries.iter().filter(|l| !s.is_open_library(Path::new(&l.path))).map(|l| json!({"name": l.display_name(), "path": l.path}))).collect::<Vec<_>>(),
        "shared": removal::shares_with_real_app(), "busy": working(&st).await,
        "hasKey": s.ai.claude.has_api_token, "loginItem": s.daemon.run_at_login,
        "keptHistory": std::fs::read_dir(home.join("history")).map_or(false, |mut d| d.next().is_some()),
        "confirm": REMOVE_CONFIRM,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Remove { delete_data: bool, confirm: String }

/// What the removal may reach besides the data folder. `None`: leave it alone (a copy with its own
/// data folder shares the keychain and the login item with the real app).
pub struct Reach<'a> {
    pub keyring: Option<&'a dyn Keyring>,
    pub login_item: Option<&'a (dyn Fn() -> anyhow::Result<()> + Sync)>,
}

/// Remove the login item, the saved passwords and key, and (with `delete_data`) the app's own
/// files in `home`. Nothing at all is done when the data folder cannot be removed safely.
pub async fn perform(st: &AppState, home: &Path, delete_data: bool, reach: Reach<'_>) -> anyhow::Result<(removal::Outcome, Option<removal::Plan>)> {
    if working(st).await { return Err(crate::tr::err("care.running", &[])); }
    let s = st.settings.read().await.clone();
    let (library, linked) = user_folders(&s).await;
    let plan = if delete_data {
        let p = removal::plan(home, library.as_deref(), &linked);
        if p.blocked.is_some() { return Err(crate::tr::err("remove.blocked", &[])); }
        Some(p)
    } else { None };
    let mut out = removal::Outcome::default();
    out.login_item = match reach.login_item {
        Some(f) => match f() { Ok(()) => "removed", Err(e) => { out.errors.push(format!("{e:#}")); "failed" } },
        None => "skipped",
    };
    out.passwords = match reach.keyring {
        Some(k) => {
            let remotes = match &library {
                Some(l) if git::is_repo(st, l).await => git::remotes(st, l).await.iter().filter_map(|r| r["url"].as_str().map(String::from)).collect(),
                _ => vec![],
            };
            let errors = removal::remove_secrets(k, &removal::credential_hosts(&remotes, &home.join("git-credentials")));
            let failed = !errors.is_empty();
            out.errors.extend(errors);
            if failed { "failed" } else { "removed" }
        }
        None => "skipped",
    };
    match &plan {
        Some(p) => {
            let done = removal::remove_data(p)?;
            out = removal::Outcome { passwords: out.passwords, login_item: out.login_item, errors: out.errors, ..done };
        }
        None => {
            // The data stays: it must not say a key or a login item exists any more.
            out.data_kept = true;
            let mut next = s.clone();
            if out.passwords == "removed" { next.ai.claude.has_api_token = false; }
            if out.login_item == "removed" { next.daemon.run_at_login = false; }
            if !crate::settings::read_only() {
                crate::log::ok_or_warn("could not save the settings after removing the saved key", next.save_to(&home.join("settings.json")));
                *st.settings.write().await = next;
            }
        }
    }
    Ok((out, plan))
}

async fn remove(State(st): S, Json(r): Json<Remove>) -> R {
    if r.confirm != REMOVE_CONFIRM { return Err(crate::tr::err("remove.unconfirmed", &[]).into()); }
    let shared = removal::shares_with_real_app();
    let login = || crate::api::remove_login_item();
    let reach = Reach { keyring: shared.then_some(&SystemKeyring as &dyn Keyring), login_item: shared.then_some(&login as &(dyn Fn() -> anyhow::Result<()> + Sync)) };
    crate::log::info(format!("removing Thoughts from this computer (data folder {})", if r.delete_data { "too" } else { "kept" }));
    let (out, plan) = perform(&st, &paths::home(), r.delete_data, reach).await?;
    // The service stops once the answer is on its way, without saving anything on the way out.
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        match plan { Some(p) => removal::sweep(&p), None => paths::clear_daemon_info() }
        std::process::exit(0);
    });
    Ok(Json(serde_json::to_value(out)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::removal::tests::FakeKeyring;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-care-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn removal_takes_the_apps_data_passwords_and_login_item_but_not_the_library() {
        let root = tmp();
        let home = root.join("data");
        for d in ["chats", "attachments/x", "app-layer"] { std::fs::create_dir_all(home.join(d)).unwrap(); }
        std::fs::write(home.join("settings.json"), "{}").unwrap();
        std::fs::write(home.join("git-credentials"), "https://me:pw@codeberg.org\n").unwrap();
        let library = root.join("Notes");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(library.join("idea.md"), "keep").unwrap();
        let st = AppState::for_test();
        st.settings.write().await.library_path = Some(library.to_string_lossy().into());
        let k = FakeKeyring::with(&[("thoughts-ai", "claude", "sk"), ("thoughts-git", "codeberg.org", "{}"), ("someone-else", "x", "keep")]);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let login = || { calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst); Ok(()) };
        let (out, plan) = perform(&st, &home, true, Reach { keyring: Some(&k), login_item: Some(&login) }).await.unwrap();
        assert_eq!((out.passwords, out.login_item), ("removed", "removed"));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(k.left(), [("someone-else".to_string(), "x".to_string())]);
        assert!(out.folder_removed && !home.exists() && plan.is_some());
        assert_eq!(std::fs::read_to_string(library.join("idea.md")).unwrap(), "keep");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn keeping_the_data_still_forgets_the_key_and_a_sandbox_reaches_nothing_shared() {
        let root = tmp();
        let home = root.join("data");
        std::fs::create_dir_all(home.join("chats")).unwrap();
        let st = AppState::for_test();
        { let mut s = st.settings.write().await; s.ai.claude.has_api_token = true; s.daemon.run_at_login = true; }
        let k = FakeKeyring::with(&[("thoughts-ai", "claude", "sk")]);
        let login = || Ok(());
        let (out, plan) = perform(&st, &home, false, Reach { keyring: Some(&k), login_item: Some(&login) }).await.unwrap();
        assert!(out.data_kept && plan.is_none() && home.join("chats").is_dir());
        assert!(k.left().is_empty());
        let saved: Settings = serde_json::from_str(&std::fs::read_to_string(home.join("settings.json")).unwrap()).unwrap();
        assert!(!saved.ai.claude.has_api_token && !saved.daemon.run_at_login);
        // No keychain and no login item to reach (a copy with its own data folder): both skipped.
        let (out, _) = perform(&st, &home, true, Reach { keyring: None, login_item: None }).await.unwrap();
        assert_eq!((out.passwords, out.login_item), ("skipped", "skipped"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn removal_waits_for_running_work_and_refuses_an_unsafe_folder() {
        let root = tmp();
        let home = root.join("data");
        std::fs::create_dir_all(home.join("chats")).unwrap();
        let st = AppState::for_test();
        // The library inside the data folder: nothing is done at all, not even the keychain.
        st.settings.write().await.library_path = Some(home.join("chats").to_string_lossy().into());
        let k = FakeKeyring::with(&[("thoughts-ai", "claude", "sk")]);
        let e = perform(&st, &home, true, Reach { keyring: Some(&k), login_item: None }).await.unwrap_err();
        assert!(e.to_string().starts_with("REMOVE_BLOCKED::"), "{e}");
        assert_eq!(k.left().len(), 1);
        // A chat still running.
        let (tx, _rx) = tokio::sync::oneshot::channel();
        st.runs.lock().await.insert("c".into(), tx);
        assert!(perform(&st, &home, false, Reach { keyring: Some(&k), login_item: None }).await.is_err());
        assert_eq!(k.left().len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn bundle_with(chats: Value, settings: Value, attachments: Value) -> Value {
        json!({"format": transfer::FORMAT, "version": 1, "settings": settings, "chats": chats, "attachments": attachments})
    }

    #[tokio::test]
    async fn the_self_check_repairs_history_only_when_nothing_else_is_at_work_in_the_library() {
        use crate::wraps::testutil as t;
        let (st, lib) = t::library().await;
        // A history left off its branch: what the repair would put right.
        t::sh(&lib, &["checkout", "-q", "--detach"]);
        let detached = || t::sh(&lib, &["rev-parse", "--abbrev-ref", "HEAD"]) == "HEAD";
        for fix in ["finish-history", "unlock-history", "repair-history"] {
            // The assistant is writing in the library: refused, nothing touched.
            st.run_dirs.lock().unwrap().insert("c-run".into(), lib.clone());
            assert!(crate::tr::classify_in(false, &run_fix(&st, fix).await.unwrap_err()).message.ends_with(&crate::tr::text_in(false, "busy.run", &[])), "{fix}");
            st.run_dirs.lock().unwrap().clear();
            // A wrap, a sync or another repair holds the library: refused, nothing touched.
            let held = st.library_lock.of(&lib).try_lock_owned().unwrap();
            assert!(crate::tr::classify_in(false, &run_fix(&st, fix).await.unwrap_err()).message.ends_with(&crate::tr::text_in(false, "busy", &[])), "{fix}");
            assert!(detached());
            drop(held);
        }
        // Free: repaired, and the library is held while it is (nothing else gets in meanwhile).
        run_fix(&st, "repair-history").await.unwrap();
        assert!(!detached());
        assert!(!st.library_busy());
        std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn replacing_a_chat_that_cannot_be_written_leaves_the_one_here_as_it_was() {
        use std::os::unix::fs::PermissionsExt;
        let root = tmp();
        let home = root.join("data");
        let att = home.join("attachments");
        std::fs::create_dir_all(att.join("c-9")).unwrap();
        std::fs::write(att.join("c-9/old.txt"), "mine").unwrap();
        let st = AppState::for_test();
        st.chats.insert(Chat { id: "c-9".into(), title: "Mine here".into(), ..Default::default() }).await;
        let msg = json!({"id": "m", "role": "user", "text": "hi", "ts": "2026-01-01T00:00:00Z", "attachments": ["new.txt"]});
        let bundle = bundle_with(json!([{"id": "c-9", "title": "Theirs", "messages": [msg]}]), Value::Null, json!({"c-9": {"new.txt": "aGk="}}));
        let replace = Choices { chats: true, replace: true, ..Default::default() };
        // Nothing can be written where attachments go (a full disk, say): the import fails…
        std::fs::set_permissions(&att, std::fs::Permissions::from_mode(0o500)).unwrap();
        assert!(import(&st, transfer::read(&bundle).unwrap(), &replace, &home).await.is_err());
        std::fs::set_permissions(&att, std::fs::Permissions::from_mode(0o700)).unwrap();
        // …and the chat here, with its files, is exactly as it was.
        assert_eq!(st.chats.get("c-9").await.unwrap().title, "Mine here");
        assert_eq!(std::fs::read_to_string(att.join("c-9/old.txt")).unwrap(), "mine");
        // Once it can be written, the chat is replaced whole, its files too, and nothing is left beside them.
        let r = import(&st, transfer::read(&bundle).unwrap(), &replace, &home).await.unwrap();
        assert_eq!(r["chatsReplaced"], 1);
        assert_eq!(st.chats.get("c-9").await.unwrap().title, "Theirs");
        let names: Vec<String> = std::fs::read_dir(&att).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(names, ["c-9"]);
        assert!(!att.join("c-9/old.txt").exists());
        assert_eq!(std::fs::read(att.join("c-9/new.txt")).unwrap(), b"hi");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn an_id_in_other_letter_case_never_reaches_another_chats_files() {
        let root = tmp();
        let home = root.join("data");
        let att = home.join("attachments");
        let st = AppState::for_test();
        // Here: a chat whose id has capitals (an older import), with its files…
        std::fs::create_dir_all(att.join("Abc-3")).unwrap();
        std::fs::write(att.join("Abc-3/mine.txt"), "mine").unwrap();
        st.chats.insert(Chat { id: "Abc-3".into(), title: "Mine here".into(), ..Default::default() }).await;
        // …and files found under a name that, on a disk ignoring case, is the one a new chat would use.
        std::fs::create_dir_all(att.join("abc-4")).unwrap();
        std::fs::write(att.join("abc-4/other.txt"), "another chat's").unwrap();
        let msg = |a: &str| json!({"id": "m", "role": "user", "text": "hi", "ts": "2026-01-01T00:00:00Z", "attachments": [a]});
        let chats = json!([{"id": "ABC-3", "title": "From there", "messages": [msg("a.txt")]}, {"id": "ABC-4", "title": "Also from there", "messages": [msg("b.txt")]}]);
        let bundle = bundle_with(chats, Value::Null, json!({"ABC-3": {"a.txt": "aGk="}, "ABC-4": {"b.txt": "aGk="}}));
        // Even when told to replace, neither takes the place of what is here: each gets an id of its own.
        let r = import(&st, transfer::read(&bundle).unwrap(), &Choices { chats: true, replace: true, ..Default::default() }, &home).await.unwrap();
        assert_eq!((r["chatsAdded"].as_u64(), r["chatsReplaced"].as_u64()), (Some(2), Some(0)), "{r}");
        assert_eq!(st.chats.get("Abc-3").await.unwrap().title, "Mine here");
        assert_eq!(std::fs::read_to_string(att.join("Abc-3/mine.txt")).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(att.join("abc-4/other.txt")).unwrap(), "another chat's");
        for title in ["From there", "Also from there"] {
            let c = st.chats.list().await.into_iter().find(|m| m.title == title).unwrap();
            assert!(!["abc-3", "abc-4"].contains(&c.id.to_lowercase().as_str()), "{title}: {}", c.id);
            let full = st.chats.get(&c.id).await.unwrap();
            let file = PathBuf::from(&full.messages[0].attachments[0]);
            assert_eq!((file.parent().unwrap(), std::fs::read(&file).unwrap()), (att.join(&c.id).as_path(), b"hi".to_vec()));
        }
        // One file naming the same chat twice in other letter cases is refused whole.
        let twice = bundle_with(json!([{"id": "abc-5", "title": "a"}, {"id": "ABC-5", "title": "b"}]), Value::Null, Value::Null);
        assert!(transfer::read(&twice).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn an_import_adds_chats_maps_folders_and_never_overwrites_unasked() {
        let root = tmp();
        let home = root.join("data");
        std::fs::create_dir_all(&home).unwrap();
        let folder = root.join("Client");
        std::fs::create_dir_all(&folder).unwrap();
        let st = AppState::for_test();
        let existing = Chat { id: "c-2".into(), title: "Mine here".into(), ..Default::default() };
        st.chats.insert(existing).await;
        let msg = json!({"id": "m", "role": "user", "text": "hi", "ts": "2026-01-01T00:00:00Z", "attachments": ["a.txt"]});
        let chats = json!([
            {"id": "c-1", "title": "From there", "projectId": "p-old", "sessionId": "s", "messages": [msg]},
            {"id": "c-2", "title": "Theirs"},
            {"id": "c-3", "title": "Unknown folder", "projectId": "p-gone", "library": "/there/Thoughts"},
        ]);
        let here = root.join("Thoughts");
        std::fs::create_dir_all(&here).unwrap();
        st.settings.write().await.library_path = Some(here.to_string_lossy().to_string());
        let settings = json!({"projects": [{"id": "p-old", "name": "Client", "path": "/there/Client"}, {"id": "p-gone", "name": "Gone", "path": "/there/Gone"}]});
        let b = transfer::read(&bundle_with(chats, settings, json!({"c-1": {"a.txt": "aGk="}}))).unwrap();
        let mut ch = Choices { chats: true, ..Default::default() };
        ch.projects.insert("p-old".into(), folder.to_string_lossy().into());
        // The linked folder is a plain one (nothing in it can run): trusted without asking.
        let r = import(&st, b, &ch, &home).await.unwrap();
        assert_eq!((r["chatsAdded"].as_u64(), r["chatsSkipped"].as_u64(), r["projects"].as_u64()), (Some(2), Some(1), Some(1)));
        assert_eq!(st.chats.get("c-2").await.unwrap().title, "Mine here", "not replaced without asking");
        let c1 = st.chats.get("c-1").await.unwrap();
        assert!(c1.session_id.is_none());
        let proj = st.settings.read().await.projects.iter().find(|p| Some(&p.id) == c1.project_id.as_ref()).cloned().unwrap();
        assert_eq!(PathBuf::from(&proj.path), paths::canonical(&folder).unwrap());
        // Named the way this computer names files (backslashes on Windows), like every other attachment.
        let attached = home.join("attachments").join("c-1").join("a.txt");
        assert_eq!(c1.messages[0].attachments, [attached.to_string_lossy().to_string()]);
        assert_eq!(std::fs::read(&attached).unwrap(), b"hi");
        assert_eq!(st.chats.get("c-3").await.unwrap().project_id, None, "a folder left out: the chat belongs to the library");
        // The other computer's library folder is not this one's: the chat is listed in the library open here.
        let c3 = st.chats.get("c-3").await.unwrap();
        assert_eq!(c3.library.as_deref(), Some(here.to_string_lossy().as_ref()));
        assert!(crate::wraps::Scope::now(&st).await.holds(c3.library.as_deref()));
        // Asked to replace: it is.
        let b = transfer::read(&bundle_with(json!([{"id": "c-2", "title": "Theirs"}]), Value::Null, Value::Null)).unwrap();
        let r = import(&st, b, &Choices { chats: true, replace: true, ..Default::default() }, &home).await.unwrap();
        assert_eq!(r["chatsReplaced"], 1);
        assert_eq!(st.chats.get("c-2").await.unwrap().title, "Theirs");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn add_on_data_comes_along_and_replaces_only_when_asked() {
        let root = tmp();
        let home = root.join("data");
        std::fs::create_dir_all(&home).unwrap();
        let st = AppState::for_test();
        crate::layer_api::storage_write(&home, "hours.js", br#"{"entries":["here"]}"#, false).unwrap();
        let mut file = bundle_with(Value::Null, Value::Null, Value::Null);
        file["addonData"] = json!({"hours.js": {"entries": ["there"]}, "board.js": {"cols": 4}});
        let ch = |replace: bool| Choices { addon_data: true, addon_data_replace: replace, ..Default::default() };
        let r = import(&st, transfer::read(&file).unwrap(), &Choices::default(), &home).await.unwrap();
        assert_eq!(r["addonData"]["added"], 0, "not chosen: nothing");
        let r = import(&st, transfer::read(&file).unwrap(), &ch(false), &home).await.unwrap();
        assert_eq!((r["addonData"]["added"].as_u64(), r["addonData"]["skipped"].as_u64()), (Some(1), Some(1)));
        assert_eq!(crate::layer_api::storage_read(&home, "hours.js").unwrap()["entries"], json!(["here"]), "not replaced unasked");
        assert_eq!(crate::layer_api::storage_read(&home, "board.js").unwrap()["cols"], 4);
        let r = import(&st, transfer::read(&file).unwrap(), &ch(true), &home).await.unwrap();
        assert_eq!(r["addonData"]["replaced"], 2);
        assert_eq!(crate::layer_api::storage_read(&home, "hours.js").unwrap()["entries"], json!(["there"]));
        // What was here can be put back.
        let info = crate::layer_api::data_info(&home, "hours.js").unwrap();
        let first = info["versions"].as_array().unwrap().iter().find(|v| v["bytes"] == br#"{"entries":["here"]}"#.len()).unwrap()["id"].as_str().unwrap().to_string();
        crate::layer_api::storage_restore(&home, "hours.js", &first).unwrap();
        assert_eq!(crate::layer_api::storage_read(&home, "hours.js").unwrap()["entries"], json!(["here"]));
        std::fs::remove_dir_all(root).unwrap();
    }
}
