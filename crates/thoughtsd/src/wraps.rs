//! Mini-wraps, the Wrap button, and Reorganize.
//!
//! - A chat gets a cheap mini-wrap when it goes idle or another chat starts.
//!   Engaging the chat again discards it (it is regenerated later).
//! - Wrap consumes every unwrapped chat's mini-wrap: preview (plan mode) first,
//!   then apply on approval, then the app commits (and syncs if auto).
//!   The apply step is all-or-nothing: the library is snapshotted first and put back if the run
//!   fails or is stopped.
//! - Reorganize runs on a branch in a separate working copy (a git worktree), so the library's own
//!   folder never leaves the user's branch; it needs an explicit merge.

use crate::chats::{Chat, Msg, RunOptions};
use crate::claude::{self, RunSpec};
use crate::state::AppState;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

const MINI_WRAP_PROMPT: &str = "You are writing a MINI-WRAP: a compact distillation of one chat from a personal \
ideas library, to be merged into the library later. Output markdown only, at most ~15 lines:\n\
- **Topic:** one line\n- **Ideas touched:** slugs/titles if identifiable, or 'new: <title>'\n\
- **Decided:** …\n- **Learned:** …\n- **Open:** …\n- **Todos:** concrete actions the user wants tracked\n\
Drop empty lines. No preamble.\n\nCHAT TRANSCRIPT:\n\n";

/// Retry state for one chat's automatic mini-wrap.
#[derive(Clone, Debug)]
pub struct Backoff {
    pub failures: u32,
    pub next_at: DateTime<Utc>,
    /// The chat's `updated` when it last failed: a new message resets the backoff.
    pub chat_updated: DateTime<Utc>,
}

/// Wait after the n-th consecutive failure: 1 min, 2, 4, … capped at 1 hour.
pub fn backoff_delay(failures: u32) -> std::time::Duration {
    let mins = 1u64.checked_shl(failures.saturating_sub(1)).unwrap_or(u64::MAX).min(60);
    std::time::Duration::from_secs(mins.max(1) * 60)
}

/// Errors that retrying cannot fix: not signed in, or no network. Automatic mini-wraps stop
/// until some run succeeds again.
pub fn pauses_mini_wraps(err: &str) -> bool {
    let e = err.to_lowercase();
    ["not logged in", "/login", "log in", "authentication", "unauthorized", "401", "403", "invalid api key", "invalid x-api-key",
     "api key", "oauth", "credit balance", "cli not found",
     "enotfound", "econnrefused", "econnreset", "etimedout", "eai_again", "getaddrinfo", "network", "offline", "fetch failed",
     "connection error", "could not resolve", "unable to connect"].iter().any(|k| e.contains(k))
}

fn tr<'a>(ar: bool, en: &'a str, arabic: &'a str) -> &'a str { if ar { arabic } else { en } }
async fn is_ar(state: &AppState) -> bool { state.settings.read().await.language == "ar" }

pub async fn idle_loop(state: Arc<AppState>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let idle = state.settings.read().await.idle_minutes.max(1) as i64;
        let cutoff = Utc::now() - chrono::Duration::minutes(idle);
        for c in state.chats.list().await {
            if c.kind == "chat" && c.dirty && !c.running && c.updated < cutoff {
                mini_wrap_auto(&state, &c).await;
            }
        }
    }
}

/// When a new chat starts, the others are "done for now": mini-wrap them.
pub fn on_new_chat(state: Arc<AppState>, except: String) {
    tokio::spawn(async move {
        for c in state.chats.list().await {
            if c.id != except && c.kind == "chat" && c.dirty && !c.running {
                mini_wrap_auto(&state, &c).await;
            }
        }
    });
}

/// May an automatic mini-wrap for this chat run now? Clears the backoff when the chat has new messages.
fn backoff_allows(map: &mut std::collections::HashMap<String, Backoff>, chat_id: &str, chat_updated: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    match map.get(chat_id) {
        Some(b) if b.chat_updated != chat_updated => { map.remove(chat_id); true }
        Some(b) => now >= b.next_at,
        None => true,
    }
}

fn backoff_record_failure(map: &mut std::collections::HashMap<String, Backoff>, chat_id: &str, chat_updated: DateTime<Utc>, now: DateTime<Utc>) -> std::time::Duration {
    let failures = map.get(chat_id).map_or(0, |b| b.failures) + 1;
    let wait = backoff_delay(failures);
    map.insert(chat_id.to_string(), Backoff { failures, next_at: now + chrono::Duration::from_std(wait).unwrap_or(chrono::Duration::hours(1)), chat_updated });
    wait
}

/// A background mini-wrap: skipped while paused or backing off; failures are logged and spaced out
/// instead of retried every minute.
async fn mini_wrap_auto(state: &AppState, c: &Chat) {
    if state.mini_wraps_blocked.load(Ordering::Relaxed) { return; }
    if !backoff_allows(&mut *state.mini_wrap_backoff.lock().await, &c.id, c.updated, Utc::now()) { return; }
    match mini_wrap(state, &c.id).await {
        Ok(()) => { state.mini_wrap_backoff.lock().await.remove(&c.id); }
        Err(e) => {
            let wait = backoff_record_failure(&mut *state.mini_wrap_backoff.lock().await, &c.id, c.updated, Utc::now());
            let msg = e.to_string();
            if pauses_mini_wraps(&msg) {
                state.mini_wraps_blocked.store(true, Ordering::Relaxed);
                crate::log::warn(format!("mini-wrap for chat {} failed ({msg}); automatic summaries are paused until the assistant works again", c.id));
            } else {
                crate::log::warn(format!("mini-wrap for chat {} failed ({msg}); next try in {} min", c.id, wait.as_secs() / 60));
            }
        }
    }
}

pub async fn mini_wrap(state: &AppState, id: &str) -> anyhow::Result<()> {
    let Some(c) = state.chats.get(id).await else { return Ok(()) };
    if !c.messages.iter().any(|m| m.role == "user") { return Ok(()); }
    let model = state.settings.read().await.ai.claude.mini_wrap_model.clone().or(Some("haiku".into()));
    let text = claude::complete(state, &format!("{MINI_WRAP_PROMPT}{}", c.transcript(60_000)), model).await?;
    let started = c.updated;
    let updated = state.chats.update(id, |c| {
        // Only keep it if nobody engaged the chat while we were summarising.
        if c.updated == started {
            c.mini_wrap = Some(text);
            c.mini_wrap_at = Some(Utc::now());
            c.dirty = false;
        }
    }).await;
    if let Some(ch) = updated { state.emit(json!({"type": "chat.updated", "chat": ch.summary()})); }
    Ok(())
}

pub async fn status(state: &AppState) -> Value {
    let chats: Vec<Value> = state.chats.list().await.into_iter().filter(|c| c.is_unwrapped())
        .map(|c| json!({"id": c.id, "title": c.title, "updated": c.updated, "hasMiniWrap": c.mini_wrap.is_some() && !c.dirty}))
        .collect();
    let s = state.settings.read().await;
    json!({"count": chats.len(), "chats": chats, "threshold": s.wrap_nag_threshold, "lastWrap": s.last_wrap,
           "busy": state.library_busy(), "miniWrapsPaused": state.mini_wraps_blocked.load(Ordering::Relaxed)})
}

fn library(state_lib: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    state_lib.ok_or_else(|| anyhow::anyhow!("no library configured"))
}

const BUSY: &str = "BUSY::Your library is being updated right now. Try again when that has finished.";

/// Phase 1: build mini-wraps, then ask for a preview in plan mode.
pub async fn start(state: Arc<AppState>) -> anyhow::Result<String> {
    let lib = library(state.settings.read().await.library())?;
    if state.library_busy() { anyhow::bail!(BUSY); }
    let pending: Vec<Chat> = state.chats.list().await.into_iter().filter(|c| c.is_unwrapped()).collect();
    if pending.is_empty() { anyhow::bail!("nothing to wrap"); }
    let ar = is_ar(&state).await;
    let mut wrap = Chat { title: format!("{} — {}", tr(ar, "Wrap", "تلخيص وحفظ"), chrono::Local::now().format("%Y-%m-%d %H:%M")), kind: "wrap".into(), ..Default::default() };
    // Chats are stamped with this time when the wrap is applied, so anything said after the
    // preview was prepared is picked up by the next wrap.
    wrap.meta = json!({"chatIds": pending.iter().map(|c| &c.id).collect::<Vec<_>>(), "phase": "preview", "previewAt": Utc::now()});
    wrap.messages.push(Msg::new("system", if ar { format!("جارٍ تلخيص المحادثات ({})…", pending.len()) } else { format!("Preparing mini-wraps for {} chat(s)…", pending.len()) }));
    let wrap_id = wrap.id.clone();
    state.chats.insert(wrap).await;
    crate::log::info(format!("wrap {wrap_id}: preview for {} chat(s)", pending.len()));

    let st = state.clone();
    let wid = wrap_id.clone();
    tokio::spawn(async move {
        // Generate missing mini-wraps (in parallel, bounded).
        let need: Vec<_> = pending.iter().filter(|c| c.mini_wrap.is_none() || c.dirty).map(|c| c.id.clone()).collect();
        for chunk in need.chunks(4) {
            for (id, r) in chunk.iter().zip(futures::future::join_all(chunk.iter().map(|id| mini_wrap(&st, id))).await) {
                // Not fatal: the chat's transcript is used instead.
                if let Err(e) = r { crate::log::warn(format!("wrap {wid}: mini-wrap for chat {id} failed: {e}")); }
            }
        }
        let mut body = String::new();
        for c in &pending {
            let c = st.chats.get(&c.id).await.unwrap_or(c.clone());
            let mw = c.mini_wrap.clone().unwrap_or_else(|| c.transcript(8_000));
            body.push_str(&format!("\n---\n#### Chat: {} ({})\n{}\n", c.title, c.updated.format("%Y-%m-%d"), mw));
        }
        let prompt = format!(
            "WRAP — the user pressed the Wrap button. Below are mini-wraps of {} chat(s) since the last wrap.\n\
             Follow this library's wrap ritual (.claude/skills/wrap/SKILL.md and CLAUDE.md conventions) to distill them into the library.\n\n\
             **This is the PREVIEW step — do not edit files.** Output a concise plan:\n\
             1. Ideas touched (and what changes in each README/log/notes)\n2. New ideas to scaffold\n3. Todos to add\n4. me/ updates (if that module exists)\n\
             Keep it under 30 lines.\n{body}",
            pending.len()
        );
        let spec = RunSpec { chat_id: wid.clone(), prompt: prompt.clone(), cwd: lib.clone(), add_dirs: vec![],
            options: RunOptions { permission_mode: Some("plan".into()), ..Default::default() }, resume: None, append_system: None, extra_deny: vec![] };
        claude::push(&st, &wid, Msg::new("user", tr(ar, "Preview the wrap.", "اعرض ما سيُحفظ قبل التنفيذ."))).await;
        if let Err(e) = claude::start_run(st.clone(), spec, None).await {
            crate::log::error(format!("wrap {wid}: preview could not start: {e}"));
            claude::push(&st, &wid, Msg::new("error", e.to_string())).await;
        }
    });
    Ok(wrap_id)
}

async fn set_phase(state: &AppState, id: &str, phase: &str) {
    if let Some(ch) = state.chats.update(id, |c| c.meta["phase"] = json!(phase)).await {
        state.emit(json!({"type": "chat.updated", "chat": ch.summary()}));
    }
}

/// Put the library back as it was before a wrap's apply step. Returns whether it was restored.
async fn undo_partial(state: &AppState, lib: &Path, wid: &str, snapshot: Option<&str>, ar: bool) -> bool {
    let Some(snap) = snapshot else {
        crate::log::error(format!("wrap {wid}: failed and the library has no history to restore from"));
        claude::push(state, wid, Msg::new("system", tr(ar,
            "The wrap did not finish. This library keeps no history, so any changes it had already made could not be undone — please check your recent files.",
            "لم يكتمل التلخيص والحفظ. هذه المكتبة لا تحتفظ بسجلّ للتغييرات، لذا تعذّر التراجع عمّا كُتب قبل التوقف. راجع ملفاتك الأخيرة من فضلك."))).await;
        return false;
    };
    let res = { let _g = state.git_lock.lock().await; crate::git::rollback_to(state, lib, snap).await };
    match res {
        Ok(created) => {
            crate::log::warn(format!("wrap {wid}: failed; library restored to {snap} ({} new file(s) removed)", created.len()));
            claude::push(state, wid, Msg::new("system", tr(ar,
                "The wrap did not finish, so every change it had started was undone. Your library is exactly as it was, and these chats are still waiting to be wrapped.",
                "لم يكتمل التلخيص والحفظ، فأُلغيت كل التغييرات التي بدأها. مكتبتك كما كانت تمامًا، وهذه المحادثات ما زالت بانتظار التلخيص."))).await;
            true
        }
        Err(e) => {
            crate::log::error(format!("wrap {wid}: failed and the library could not be restored to {snap}: {e}"));
            claude::push(state, wid, Msg::new("error", if ar { format!("لم يكتمل التلخيص والحفظ، وتعذّر التراجع عن التغييرات تلقائيًا: {e}") }
                else { format!("The wrap did not finish, and its partial changes could not be undone automatically: {e}") })).await;
            false
        }
    }
}

/// Phase 2: apply the approved preview, then commit + optional sync. All-or-nothing: the library
/// is snapshotted first and restored if the run fails or is stopped.
pub async fn apply(state: Arc<AppState>, wrap_id: &str, note: &str) -> anyhow::Result<()> {
    let lib = library(state.settings.read().await.library())?;
    let w = state.chats.get(wrap_id).await.ok_or_else(|| anyhow::anyhow!("no such wrap"))?;
    if w.kind != "wrap" { anyhow::bail!("no such wrap"); }
    if w.running { anyhow::bail!("the preview is still being prepared"); }
    match w.meta["phase"].as_str() {
        Some("applying") => anyhow::bail!(BUSY),
        Some("done") => anyhow::bail!("this wrap has already been saved"),
        _ => {}
    }
    // Held until the commit is made: nothing else syncs, wraps or shuts down halfway through.
    let guard = state.library_lock.clone().try_lock_owned().map_err(|_| anyhow::anyhow!(BUSY))?;
    let ar = is_ar(&state).await;
    let ids: Vec<String> = w.meta["chatIds"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    let preview_at = w.meta["previewAt"].as_str().and_then(|t| t.parse::<DateTime<Utc>>().ok()).unwrap_or(w.created);
    let git_on = state.settings.read().await.git_enabled() && crate::git::is_repo(&state, &lib).await;
    let snapshot = if git_on {
        let _g = state.git_lock.lock().await;
        crate::git::commit_all(&state, &lib, "snapshot before wrap").await?;
        crate::git::head(&state, &lib).await
    } else { None };
    crate::log::info(format!("wrap {wrap_id}: applying {} chat(s), snapshot {}", ids.len(), snapshot.as_deref().unwrap_or("none")));
    let text = format!("Approved{}. Apply the wrap now: edit the files. Do not run git — the app commits and syncs afterwards. \
        End with a 3-line summary.", if note.trim().is_empty() { String::new() } else { format!(" with these changes: {note}") });
    claude::push(&state, wrap_id, Msg::new("user", &text)).await;
    let snap_meta = snapshot.clone();
    state.chats.update(wrap_id, |c| { c.meta["phase"] = json!("applying"); c.meta["snapshot"] = json!(snap_meta); }).await;
    let spec = RunSpec { chat_id: wrap_id.into(), prompt: text, cwd: lib.clone(), add_dirs: vec![],
        options: RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() },
        resume: w.session_id.clone(), append_system: None, extra_deny: vec![] };
    let wid = wrap_id.to_string();
    let started = claude::start_run(state.clone(), spec, Some(Box::new(move |st: Arc<AppState>, ok: bool| Box::pin(async move {
        let _guard = guard;
        if !ok {
            let rolled_back = undo_partial(&st, &lib, &wid, snapshot.as_deref(), ar).await;
            st.chats.update(&wid, |c| { c.meta["phase"] = json!("failed"); c.meta["rolledBack"] = json!(rolled_back); }).await;
            st.emit(json!({"type": "wrap.failed", "chatId": wid, "rolledBack": rolled_back}));
            st.emit(json!({"type": "library.changed"}));
            st.notify(tr(ar, "Wrap did not finish", "لم يكتمل التلخيص والحفظ"),
                if rolled_back { tr(ar, "Nothing was changed in your library. You can try again.", "لم يتغيّر شيء في مكتبتك. يمكنك المحاولة مرة أخرى.") }
                else { tr(ar, "Please check your library — some changes may be incomplete.", "راجع مكتبتك من فضلك، فقد تكون بعض التغييرات غير مكتملة.") });
            return;
        }
        // Stamp with the preview time, not now: messages sent since then belong to the next wrap.
        for id in &ids { st.chats.update(id, |c| c.wrapped_at = Some(preview_at)).await; }
        {
            let mut s = st.settings.write().await;
            s.last_wrap = Some(Utc::now());
            crate::log::ok_or_warn("wrap: could not save settings", s.save());
        }
        st.chats.update(&wid, |c| c.meta["phase"] = json!("done")).await;
        let git_mode = st.settings.read().await.sync.mode.clone();
        if git_on {
            let msg = format!("wrap: {} — {} chat(s)", chrono::Local::now().format("%Y-%m-%d"), ids.len());
            let r = { let _g = st.git_lock.lock().await; crate::git::commit_all(&st, &lib, &msg).await };
            let note = match (&r, git_mode.as_str()) {
                (Err(e), _) => { crate::log::error(format!("wrap {wid}: commit failed: {e}")); format!("Commit failed: {e}") }
                (Ok(_), "git-auto") => match crate::git::sync(&st, &lib).await {
                    Ok(v) => format!("Committed and synced ({}).", v["log"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()),
                    Err(e) if e.to_string().starts_with("CONFLICT::") => "Committed. Sync found changes from another device that clash with yours — press Sync to choose which to keep.".into(),
                    Err(e) => format!("Committed; sync failed: {e}"),
                },
                _ => "Committed. Press Sync to push.".into(),
            };
            claude::push(&st, &wid, Msg::new("system", note)).await;
        }
        crate::log::info(format!("wrap {wid}: done ({} chat(s))", ids.len()));
        st.emit(json!({"type": "wrap.done", "chatId": wid}));
        st.emit(json!({"type": "library.changed"}));
        if ar { st.notify("اكتمل التلخيص والحفظ", &format!("حُفظت خلاصة المحادثات ({}) في مكتبتك", ids.len())); }
        else { st.notify("Wrap complete", &format!("{} chat(s) saved into your library", ids.len())); }
    })))).await;
    if let Err(e) = &started {
        // Nothing ran (and the lock went with the dropped callback): back to the preview.
        crate::log::error(format!("wrap {wrap_id}: apply could not start: {e}"));
        set_phase(&state, wrap_id, "preview").await;
    }
    started
}

/// Undo the partial changes of a wrap that was cut off by a restart (phase `interrupted`).
/// Refuses when the library's history has moved on since, rather than discard later work.
pub async fn rollback(state: &AppState, wrap_id: &str) -> anyhow::Result<()> {
    let lib = library(state.settings.read().await.library())?;
    let w = state.chats.get(wrap_id).await.ok_or_else(|| anyhow::anyhow!("no such wrap"))?;
    if w.kind != "wrap" || w.meta["phase"] != "interrupted" { anyhow::bail!("this wrap has nothing to undo"); }
    let snap = w.meta["snapshot"].as_str().ok_or_else(|| anyhow::anyhow!("this library keeps no history, so the wrap cannot be undone automatically"))?.to_string();
    let _guard = state.library_lock.clone().try_lock_owned().map_err(|_| anyhow::anyhow!(BUSY))?;
    {
        let _g = state.git_lock.lock().await;
        if crate::git::head(state, &lib).await.as_deref() != Some(snap.as_str()) {
            anyhow::bail!("your library has been saved or synced since this wrap was cut off, so it cannot be undone automatically");
        }
        let created = crate::git::rollback_to(state, &lib, &snap).await?;
        crate::log::warn(format!("wrap {wrap_id}: interrupted wrap undone, library restored to {snap} ({} new file(s) removed)", created.len()));
    }
    state.chats.update(wrap_id, |c| { c.meta["phase"] = json!("failed"); c.meta["rolledBack"] = json!(true); c.meta["canRollback"] = json!(false); }).await;
    state.emit(json!({"type": "wrap.failed", "chatId": wrap_id, "rolledBack": true}));
    state.emit(json!({"type": "library.changed"}));
    Ok(())
}

/// Remove a reorganize working copy and (when given) its branch. The library's own folder is not touched.
async fn remove_worktree(state: &AppState, lib: &Path, wt: &Path, branch: &str) {
    if wt.exists() {
        if let Err(e) = crate::git::git(state, lib, &["worktree", "remove", "--force", &wt.to_string_lossy()]).await {
            crate::log::warn(format!("reorganize: git could not remove the working copy ({e}); removing the folder"));
        }
        if wt.exists() { crate::log::ok_or_warn("reorganize: could not remove the working copy", std::fs::remove_dir_all(wt)); }
    }
    let _ = crate::git::git(state, lib, &["worktree", "prune"]).await;
    if branch.starts_with("reorganize/") {
        let _ = crate::git::git(state, lib, &["branch", "-q", "-D", "--", branch]).await;
    }
}

/// Snapshot the library and open a new `reorganize/<timestamp>` branch in a separate working copy
/// at `wt`. The library's folder stays on its branch. Returns (base branch, new branch).
pub async fn reorganize_prepare(state: &AppState, lib: &Path, wt: &Path) -> anyhow::Result<(String, String)> {
    let _g = state.git_lock.lock().await;
    crate::git::commit_all(state, lib, "snapshot before reorganize").await?;
    if crate::git::head(state, lib).await.is_none() { anyhow::bail!("there is nothing in this library to reorganize yet"); }
    let base = crate::git::current_branch(state, lib).await?;
    if crate::git::internal_branch(&base) { anyhow::bail!("the library is on \"{base}\", which the app keeps for its own use"); }
    remove_worktree(state, lib, wt, "").await;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut branch = format!("reorganize/{stamp}");
    let mut n = 1;
    while crate::git::git(state, lib, &["rev-parse", "--verify", "-q", &format!("refs/heads/{branch}")]).await.is_ok() {
        n += 1;
        branch = format!("reorganize/{stamp}-{n}");
    }
    if let Some(parent) = wt.parent() { std::fs::create_dir_all(parent)?; }
    crate::git::git(state, lib, &["worktree", "add", "-q", "-b", &branch, &wt.to_string_lossy(), "HEAD"]).await?;
    Ok((base, branch))
}

/// Keep (merge) or discard a reorganize branch. Work done in the library meanwhile is committed
/// first and never lost; a merge that conflicts is undone and reported as
/// `REORG_CONFLICT::<file;file;…>`, leaving the proposal in place.
pub async fn reorganize_conclude(state: &AppState, lib: &Path, wt: &Path, branch: &str, keep: bool) -> anyhow::Result<()> {
    if !branch.starts_with("reorganize/") { anyhow::bail!("not a reorganize branch"); }
    let _g = state.git_lock.lock().await;
    if keep {
        if wt.exists() { crate::git::commit_all(state, wt, "reorganize: final touches").await?; }
        crate::git::commit_all(state, lib, "snapshot before applying reorganize").await?;
        if crate::git::git(state, lib, &["merge", "-q", "--ff-only", branch]).await.is_err() {
            if let Err(e) = crate::git::git_ident(state, lib, &["merge", "-q", "--no-edit", branch]).await {
                let files = crate::git::unmerged(state, lib).await;
                let _ = crate::git::git(state, lib, &["merge", "--abort"]).await;
                crate::log::warn(format!("reorganize: merge of {branch} conflicts in {} file(s): {e}", files.len()));
                if files.is_empty() { anyhow::bail!("could not apply the new structure: {e}"); }
                anyhow::bail!("REORG_CONFLICT::{}", files.join(";"));
            }
        }
    }
    remove_worktree(state, lib, wt, branch).await;
    Ok(())
}

pub async fn reorganize_start(state: Arc<AppState>, instructions: &str) -> anyhow::Result<String> {
    let lib = library(state.settings.read().await.library())?;
    if !crate::git::is_repo(&state, &lib).await { anyhow::bail!("Reorganize needs the library to be a git repo (it works on a branch)"); }
    if state.chats.list().await.iter().any(|c| c.kind == "reorganize" && matches!(c.meta["phase"].as_str(), Some("running") | Some("review"))) {
        anyhow::bail!("A reorganize is already in progress or waiting for your review. Keep or discard it first.");
    }
    crate::trust::require(&lib)?;
    let wt = crate::paths::reorganize_worktree();
    let (base, branch) = reorganize_prepare(&state, &lib, &wt).await?;
    // The working copy is the same content the user already trusted in the library.
    if !crate::trust::findings(&wt).is_empty() {
        if let Err(e) = crate::trust::trust(&wt) {
            remove_worktree(&state, &lib, &wt, &branch).await;
            return Err(e);
        }
    }
    let ar = is_ar(&state).await;
    let mut c = Chat { title: tr(ar, "Reorganize library", "إعادة تنظيم المكتبة").into(), kind: "reorganize".into(), ..Default::default() };
    c.meta = json!({"base": base, "branch": branch, "worktree": wt, "phase": "running"});
    let id = c.id.clone();
    state.chats.insert(c).await;
    crate::log::info(format!("reorganize {id}: started on {branch}"));
    let prompt = format!(
        "REORGANIZE the whole library so it is easiest to manage at its current size. You are working in a separate copy of the \
         library on a throwaway git branch; the user reviews the diff before anything is kept. Work only inside this folder.\n\
         You may restructure areas, merge/split notes, rotate long logs, fix links, regenerate INDEX.md, and update CLAUDE.md \
         conventions to match. Preserve every piece of information. Do not run git.\n\
         User instructions: {}\nEnd with a summary of the new structure and why.",
        if instructions.trim().is_empty() { "(none — use your judgment)" } else { instructions }
    );
    claude::push(&state, &id, Msg::new("user", &prompt)).await;
    let spec = RunSpec { chat_id: id.clone(), prompt, cwd: wt.clone(), add_dirs: vec![],
        options: RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() }, resume: None, append_system: None, extra_deny: vec![] };
    let cid = id.clone();
    let (lib2, wt2, branch2) = (lib.clone(), wt.clone(), branch.clone());
    let started = claude::start_run(state.clone(), spec, Some(Box::new(move |st: Arc<AppState>, ok| Box::pin(async move {
        let r = { let _g = st.git_lock.lock().await; crate::git::commit_all(&st, &wt, "reorganize: proposed structure").await };
        if let Err(e) = &r { crate::log::error(format!("reorganize {cid}: could not record the proposal: {e}")); }
        crate::log::info(format!("reorganize {cid}: ready for review (run ok: {ok})"));
        st.chats.update(&cid, |c| { c.meta["phase"] = json!("review"); c.meta["ok"] = json!(ok); }).await;
        st.emit(json!({"type": "reorganize.ready", "chatId": cid, "ok": ok}));
    })))).await;
    if let Err(e) = started {
        crate::log::error(format!("reorganize {id}: could not start: {e}"));
        remove_worktree(&state, &lib2, &wt2, &branch2).await;
        set_phase(&state, &id, "failed").await;
        return Err(e);
    }
    Ok(id)
}

pub async fn reorganize_diff(state: &AppState, id: &str) -> anyhow::Result<Value> {
    let lib = library(state.settings.read().await.library())?;
    let c = state.chats.get(id).await.ok_or_else(|| anyhow::anyhow!("no such chat"))?;
    let (base, branch) = (c.meta["base"].as_str().unwrap_or("main"), c.meta["branch"].as_str().unwrap_or_default());
    if !branch.starts_with("reorganize/") || base.starts_with('-') { anyhow::bail!("no such chat"); }
    let range = format!("{base}...{branch}");
    let stat = crate::git::git(state, &lib, &["diff", "--stat", "-M", &range, "--"]).await?;
    let mut diff = crate::git::git(state, &lib, &["diff", "-M", &range, "--"]).await?;
    if diff.len() > 400_000 {
        let mut cut = 400_000;
        while !diff.is_char_boundary(cut) { cut -= 1; }
        diff.truncate(cut);
        diff.push_str("\n…(diff truncated)");
    }
    Ok(json!({"stat": stat, "diff": diff, "phase": c.meta["phase"]}))
}

pub async fn reorganize_finish(state: &AppState, id: &str, keep: bool) -> anyhow::Result<()> {
    let lib = library(state.settings.read().await.library())?;
    let c = state.chats.get(id).await.ok_or_else(|| anyhow::anyhow!("no such chat"))?;
    if c.kind != "reorganize" { anyhow::bail!("no such chat"); }
    if c.running { anyhow::bail!("the reorganize is still running — stop it or wait for it to finish"); }
    match c.meta["phase"].as_str() {
        Some("merged") | Some("discarded") => anyhow::bail!("this reorganize has already been closed"),
        Some("interrupted") | Some("failed") if keep => anyhow::bail!("this reorganize did not finish, so there is nothing to apply"),
        _ => {}
    }
    let branch = c.meta["branch"].as_str().unwrap_or_default().to_string();
    let _guard = state.library_lock.clone().try_lock_owned().map_err(|_| anyhow::anyhow!(BUSY))?;
    match c.meta["worktree"].as_str() {
        Some(wt) => reorganize_conclude(state, &lib, Path::new(wt), &branch, keep).await?,
        // A reorganize started by an older version, which switched the library itself to the branch.
        None => {
            let base = c.meta["base"].as_str().unwrap_or("main").to_string();
            if !branch.starts_with("reorganize/") || base.starts_with('-') { anyhow::bail!("no such chat"); }
            let _g = state.git_lock.lock().await;
            crate::git::commit_all(state, &lib, "reorganize: final touches").await?;
            crate::git::git(state, &lib, &["checkout", "-q", &base, "--"]).await?;
            if keep && crate::git::git(state, &lib, &["merge", "-q", "--ff-only", &branch]).await.is_err() {
                crate::git::git_ident(state, &lib, &["merge", "-q", "--no-edit", &branch]).await?;
            }
            crate::git::git(state, &lib, &["branch", "-q", "-D", "--", &branch]).await?;
        }
    }
    crate::log::info(format!("reorganize {id}: {}", if keep { "merged" } else { "discarded" }));
    set_phase(state, id, if keep { "merged" } else { "discarded" }).await;
    Ok(())
}

/// Startup: nothing may stay "applying"/"running" from a previous daemon. Interrupted wraps keep
/// their snapshot so the user can undo them; interrupted reorganizes lose their working copy
/// (the library itself was never touched), and a working copy nobody is reviewing is removed.
pub async fn recover(state: &AppState) {
    let lib = state.settings.read().await.library();
    let ar = is_ar(state).await;
    let mut reviewing = false;
    for c in state.chats.list().await {
        match (c.kind.as_str(), c.meta["phase"].as_str()) {
            ("wrap", Some("applying")) => {
                let can = c.meta["snapshot"].is_string();
                crate::log::warn(format!("wrap {}: was being applied when the app stopped; marked interrupted", c.id));
                state.chats.update(&c.id, |c| { c.meta["phase"] = json!("interrupted"); c.meta["canRollback"] = json!(can); }).await;
                claude::push(state, &c.id, Msg::new("system", tr(ar,
                    "The app stopped while this wrap was being saved, so your library may hold only part of it. You can undo the partial changes and wrap again.",
                    "توقف التطبيق أثناء حفظ هذا التلخيص، وقد تحتوي مكتبتك على جزء منه فقط. يمكنك التراجع عن التغييرات الناقصة ثم إعادة التلخيص."))).await;
            }
            ("reorganize", Some("running")) => {
                crate::log::warn(format!("reorganize {}: was running when the app stopped; marked interrupted", c.id));
                if let (Some(lib), Some(wt)) = (&lib, c.meta["worktree"].as_str()) {
                    remove_worktree(state, lib, Path::new(wt), c.meta["branch"].as_str().unwrap_or_default()).await;
                }
                state.chats.update(&c.id, |c| c.meta["phase"] = json!("interrupted")).await;
                claude::push(state, &c.id, Msg::new("system", tr(ar,
                    "The app stopped before this reorganize finished. Nothing in your library was changed — start it again when you are ready.",
                    "توقف التطبيق قبل اكتمال إعادة التنظيم. لم يتغيّر شيء في مكتبتك، ويمكنك البدء من جديد متى شئت."))).await;
            }
            ("reorganize", Some("review")) if c.meta["worktree"].is_string() => reviewing = true,
            _ => {}
        }
    }
    let wt = crate::paths::reorganize_worktree();
    if !reviewing && wt.exists() {
        crate::log::warn("reorganize: removing a working copy left behind by an earlier run");
        match &lib {
            Some(lib) => remove_worktree(state, lib, &wt, "").await,
            None => { crate::log::ok_or_warn("reorganize: could not remove the working copy", std::fs::remove_dir_all(&wt)); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_from_a_minute_to_an_hour() {
        let mins: Vec<u64> = (1..=9).map(|n| backoff_delay(n).as_secs() / 60).collect();
        assert_eq!(mins, [1, 2, 4, 8, 16, 32, 60, 60, 60]);
        assert_eq!(backoff_delay(0).as_secs(), 60);
        assert_eq!(backoff_delay(u32::MAX).as_secs(), 3600);
    }

    #[test]
    fn backoff_waits_then_resets_on_a_new_message() {
        let mut map = std::collections::HashMap::new();
        let t0 = Utc::now();
        let updated = t0 - chrono::Duration::minutes(30);
        assert!(backoff_allows(&mut map, "a", updated, t0));
        assert_eq!(backoff_record_failure(&mut map, "a", updated, t0).as_secs(), 60);
        assert!(!backoff_allows(&mut map, "a", updated, t0 + chrono::Duration::seconds(59)));
        assert!(backoff_allows(&mut map, "a", updated, t0 + chrono::Duration::seconds(60)));
        assert_eq!(backoff_record_failure(&mut map, "a", updated, t0).as_secs(), 120);
        assert_eq!(backoff_record_failure(&mut map, "a", updated, t0).as_secs(), 240);
        assert!(!backoff_allows(&mut map, "a", updated, t0 + chrono::Duration::seconds(200)));
        // Other chats are unaffected; a new message in this one starts over.
        assert!(backoff_allows(&mut map, "b", updated, t0));
        assert!(backoff_allows(&mut map, "a", t0, t0 + chrono::Duration::seconds(1)));
        assert!(map.is_empty());
        assert_eq!(backoff_record_failure(&mut map, "a", t0, t0).as_secs(), 60);
    }

    #[test]
    fn sign_in_and_network_errors_pause_mini_wraps() {
        for e in ["Invalid API key · Please run /login", "API Error: 401 authentication_error", "getaddrinfo ENOTFOUND api.anthropic.com",
                  "Connection error.", "Claude Code CLI not found", "fetch failed"] {
            assert!(pauses_mini_wraps(e), "{e}");
        }
        for e in ["the assistant timed out (no answer within 120 seconds)", "API Error: 529 overloaded_error", "claude: unexpected output"] {
            assert!(!pauses_mini_wraps(e), "{e}");
        }
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git").current_dir(dir)
            .args(["-c", "user.name=Tester", "-c", "user.email=tester@example.com", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }
    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn read(dir: &Path, rel: &str) -> String { std::fs::read_to_string(dir.join(rel)).unwrap() }

    /// A library with one commit, and a reorganize proposal (README rewritten, a file moved) made
    /// in its separate working copy.
    async fn proposal(st: &AppState) -> (PathBuf, PathBuf, PathBuf, String) {
        let root = std::env::temp_dir().join(format!("thoughts-reorg-{}", uuid::Uuid::new_v4()));
        let (lib, wt) = (root.join("library"), root.join("worktree"));
        std::fs::create_dir_all(&lib).unwrap();
        sh(&lib, &["init", "-q", "-b", "main"]);
        for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com")] { sh(&lib, &["config", k, v]); }
        write(&lib, "README.md", "old structure\n");
        write(&lib, "ideas/a.md", "idea a\n");
        sh(&lib, &["add", "-A"]);
        sh(&lib, &["commit", "-q", "-m", "first"]);
        // Uncommitted work at the moment reorganize starts is snapshotted, not lost.
        write(&lib, "draft.md", "unsaved draft\n");
        let (base, branch) = reorganize_prepare(st, &lib, &wt).await.unwrap();
        assert_eq!(base, "main");
        assert!(branch.starts_with("reorganize/"));
        // The library's own folder never leaves its branch.
        assert_eq!(sh(&lib, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
        assert_eq!(sh(&wt, &["rev-parse", "--abbrev-ref", "HEAD"]), branch);
        assert_eq!(read(&wt, "draft.md"), "unsaved draft\n");
        write(&wt, "README.md", "new structure\n");
        std::fs::create_dir_all(wt.join("areas/apps")).unwrap();
        std::fs::rename(wt.join("ideas/a.md"), wt.join("areas/apps/a.md")).unwrap();
        crate::git::commit_all(st, &wt, "reorganize: proposed structure").await.unwrap();
        assert_eq!(read(&lib, "README.md"), "old structure\n");
        (root, lib, wt, branch)
    }

    #[tokio::test]
    async fn reorganize_apply_merges_and_keeps_work_done_meanwhile() {
        let st = AppState::load().unwrap();
        let (root, lib, wt, branch) = proposal(&st).await;
        write(&lib, "notes/today.md", "written while reviewing\n");
        reorganize_conclude(&st, &lib, &wt, &branch, true).await.unwrap();
        assert_eq!(sh(&lib, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
        assert_eq!(read(&lib, "README.md"), "new structure\n");
        assert_eq!(read(&lib, "areas/apps/a.md"), "idea a\n");
        assert!(!lib.join("ideas/a.md").exists());
        assert_eq!(read(&lib, "notes/today.md"), "written while reviewing\n");
        assert!(!wt.exists());
        assert_eq!(sh(&lib, &["branch", "--list", "reorganize/*"]), "");
        assert_eq!(sh(&lib, &["status", "--porcelain"]), "");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn reorganize_discard_never_deletes_work_done_meanwhile() {
        let st = AppState::load().unwrap();
        let (root, lib, wt, branch) = proposal(&st).await;
        write(&lib, "notes/today.md", "written while reviewing\n");
        write(&lib, "README.md", "old structure, edited\n");
        reorganize_conclude(&st, &lib, &wt, &branch, false).await.unwrap();
        assert_eq!(sh(&lib, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
        assert_eq!(read(&lib, "README.md"), "old structure, edited\n");
        assert_eq!(read(&lib, "notes/today.md"), "written while reviewing\n");
        assert_eq!(read(&lib, "ideas/a.md"), "idea a\n");
        assert!(!wt.exists());
        assert_eq!(sh(&lib, &["branch", "--list", "reorganize/*"]), "");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn reorganize_conflict_leaves_the_library_untouched() {
        let st = AppState::load().unwrap();
        let (root, lib, wt, branch) = proposal(&st).await;
        write(&lib, "README.md", "edited by the user meanwhile\n");
        let err = reorganize_conclude(&st, &lib, &wt, &branch, true).await.unwrap_err().to_string();
        assert_eq!(err, "REORG_CONFLICT::README.md");
        assert_eq!(sh(&lib, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
        assert_eq!(read(&lib, "README.md"), "edited by the user meanwhile\n");
        assert_eq!(read(&lib, "ideas/a.md"), "idea a\n");
        assert_eq!(sh(&lib, &["status", "--porcelain"]), "");
        // The proposal is still there to retry or discard.
        assert!(wt.exists());
        reorganize_conclude(&st, &lib, &wt, &branch, false).await.unwrap();
        assert!(!wt.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_stale_working_copy_is_replaced() {
        let st = AppState::load().unwrap();
        let (root, lib, wt, first) = proposal(&st).await;
        // A second start (after a crash) finds the old working copy in the way.
        let (_, second) = reorganize_prepare(&st, &lib, &wt).await.unwrap();
        assert_ne!(first, second);
        assert_eq!(sh(&wt, &["rev-parse", "--abbrev-ref", "HEAD"]), second);
        assert_eq!(read(&wt, "README.md"), "old structure\n");
        assert_eq!(sh(&lib, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
        std::fs::remove_dir_all(root).unwrap();
    }
}
