//! Mini-wraps, the Wrap button, and Reorganize.
//!
//! - A chat gets a cheap mini-wrap when it goes idle or another chat starts.
//!   Engaging the chat again discards it (it is regenerated later).
//! - Wrap consumes every unwrapped chat's mini-wrap: preview (plan mode) first,
//!   then apply on approval, then the app commits (and syncs if auto).
//! - Reorganize runs on a branch and needs an explicit merge.

use crate::chats::{Chat, Msg, RunOptions};
use crate::claude::{self, RunSpec};
use crate::state::AppState;
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::Arc;

const MINI_WRAP_PROMPT: &str = "You are writing a MINI-WRAP: a compact distillation of one chat from a personal \
ideas library, to be merged into the library later. Output markdown only, at most ~15 lines:\n\
- **Topic:** one line\n- **Ideas touched:** slugs/titles if identifiable, or 'new: <title>'\n\
- **Decided:** …\n- **Learned:** …\n- **Open:** …\n- **Todos:** concrete actions the user wants tracked\n\
Drop empty lines. No preamble.\n\nCHAT TRANSCRIPT:\n\n";

pub async fn idle_loop(state: Arc<AppState>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let idle = state.settings.read().await.idle_minutes.max(1) as i64;
        let cutoff = Utc::now() - chrono::Duration::minutes(idle);
        for c in state.chats.list().await {
            if c.kind == "chat" && c.dirty && !c.running && c.updated < cutoff {
                let _ = mini_wrap(&state, &c.id).await;
            }
        }
    }
}

/// When a new chat starts, the others are "done for now": mini-wrap them.
pub fn on_new_chat(state: Arc<AppState>, except: String) {
    tokio::spawn(async move {
        for c in state.chats.list().await {
            if c.id != except && c.kind == "chat" && c.dirty && !c.running {
                let _ = mini_wrap(&state, &c.id).await;
            }
        }
    });
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
    json!({"count": chats.len(), "chats": chats, "threshold": s.wrap_nag_threshold, "lastWrap": s.last_wrap})
}

fn library(state_lib: Option<std::path::PathBuf>) -> anyhow::Result<std::path::PathBuf> {
    state_lib.ok_or_else(|| anyhow::anyhow!("no library configured"))
}

/// Phase 1: build mini-wraps, then ask for a preview in plan mode.
pub async fn start(state: Arc<AppState>) -> anyhow::Result<String> {
    let lib = library(state.settings.read().await.library())?;
    let pending: Vec<Chat> = state.chats.list().await.into_iter().filter(|c| c.is_unwrapped()).collect();
    if pending.is_empty() { anyhow::bail!("nothing to wrap"); }
    let mut wrap = Chat { title: format!("Wrap — {}", chrono::Local::now().format("%Y-%m-%d %H:%M")), kind: "wrap".into(), ..Default::default() };
    wrap.meta = json!({"chatIds": pending.iter().map(|c| &c.id).collect::<Vec<_>>(), "phase": "preview"});
    wrap.messages.push(Msg::new("system", format!("Preparing mini-wraps for {} chat(s)…", pending.len())));
    let wrap_id = wrap.id.clone();
    state.chats.insert(wrap).await;

    let st = state.clone();
    let wid = wrap_id.clone();
    tokio::spawn(async move {
        // Generate missing mini-wraps (in parallel, bounded).
        let need: Vec<_> = pending.iter().filter(|c| c.mini_wrap.is_none() || c.dirty).map(|c| c.id.clone()).collect();
        for chunk in need.chunks(4) {
            futures::future::join_all(chunk.iter().map(|id| mini_wrap(&st, id))).await;
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
            options: RunOptions { permission_mode: Some("plan".into()), ..Default::default() }, resume: None, append_system: None };
        claude::push(&st, &wid, Msg::new("user", "Preview the wrap.")).await;
        if let Err(e) = claude::start_run(st.clone(), spec, None).await {
            claude::push(&st, &wid, Msg::new("error", e.to_string())).await;
        }
    });
    Ok(wrap_id)
}

/// Phase 2: apply the approved preview, then commit + optional sync.
pub async fn apply(state: Arc<AppState>, wrap_id: &str, note: &str) -> anyhow::Result<()> {
    let lib = library(state.settings.read().await.library())?;
    let w = state.chats.get(wrap_id).await.ok_or_else(|| anyhow::anyhow!("no such wrap"))?;
    let ids: Vec<String> = w.meta["chatIds"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    let text = format!("Approved{}. Apply the wrap now: edit the files. Do not run git — the app commits and syncs afterwards. \
        End with a 3-line summary.", if note.trim().is_empty() { String::new() } else { format!(" with these changes: {note}") });
    claude::push(&state, wrap_id, Msg::new("user", &text)).await;
    state.chats.update(wrap_id, |c| c.meta["phase"] = json!("applying")).await;
    let spec = RunSpec { chat_id: wrap_id.into(), prompt: text, cwd: lib.clone(), add_dirs: vec![],
        options: RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() },
        resume: w.session_id.clone(), append_system: None };
    let wid = wrap_id.to_string();
    claude::start_run(state.clone(), spec, Some(Box::new(move |st: Arc<AppState>, ok: bool| Box::pin(async move {
        if !ok { return; }
        let now = Utc::now();
        for id in &ids { st.chats.update(id, |c| c.wrapped_at = Some(now)).await; }
        { let mut s = st.settings.write().await; s.last_wrap = Some(now); let _ = s.save(); }
        st.chats.update(&wid, |c| c.meta["phase"] = json!("done")).await;
        let git_mode = st.settings.read().await.sync.mode.clone();
        if git_mode.starts_with("git") {
            let msg = format!("wrap: {} — {} chat(s)", chrono::Local::now().format("%Y-%m-%d"), ids.len());
            let r = crate::git::commit_all(&st, &lib, &msg).await;
            let note = match (&r, git_mode.as_str()) {
                (Err(e), _) => format!("Commit failed: {e}"),
                (Ok(_), "git-auto") => match crate::git::sync(&st, &lib).await {
                    Ok(v) => format!("Committed and synced ({}).", v["log"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()),
                    Err(e) => format!("Committed; sync failed: {e}"),
                },
                _ => "Committed. Press Sync to push.".into(),
            };
            claude::push(&st, &wid, Msg::new("system", note)).await;
        }
        st.emit(json!({"type": "wrap.done"}));
        st.notify("Wrap complete", &format!("{} chat(s) saved into your library", ids.len()));
    }))) ).await
}

pub async fn reorganize_start(state: Arc<AppState>, instructions: &str) -> anyhow::Result<String> {
    let lib = library(state.settings.read().await.library())?;
    if !crate::git::is_repo(&state, &lib).await { anyhow::bail!("Reorganize needs the library to be a git repo (it works on a branch)"); }
    crate::git::commit_all(&state, &lib, "snapshot before reorganize").await?;
    let base = crate::git::git(&state, &lib, &["rev-parse", "--abbrev-ref", "HEAD"]).await?.trim().to_string();
    let branch = format!("reorganize/{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    crate::git::git(&state, &lib, &["checkout", "-q", "-b", &branch]).await?;
    let mut c = Chat { title: "Reorganize library".into(), kind: "reorganize".into(), ..Default::default() };
    c.meta = json!({"base": base, "branch": branch, "phase": "running"});
    let id = c.id.clone();
    state.chats.insert(c).await;
    let prompt = format!(
        "REORGANIZE the whole library so it is easiest to manage at its current size. You are on a throwaway git branch; \
         the user reviews the diff before anything is kept.\n\
         You may restructure areas, merge/split notes, rotate long logs, fix links, regenerate INDEX.md, and update CLAUDE.md \
         conventions to match. Preserve every piece of information. Do not run git.\n\
         User instructions: {}\nEnd with a summary of the new structure and why.",
        if instructions.trim().is_empty() { "(none — use your judgment)" } else { instructions }
    );
    claude::push(&state, &id, Msg::new("user", &prompt)).await;
    let spec = RunSpec { chat_id: id.clone(), prompt, cwd: lib.clone(), add_dirs: vec![],
        options: RunOptions { permission_mode: Some("acceptEdits".into()), ..Default::default() }, resume: None, append_system: None };
    let cid = id.clone();
    claude::start_run(state.clone(), spec, Some(Box::new(move |st: Arc<AppState>, _ok| Box::pin(async move {
        let _ = crate::git::commit_all(&st, &lib, "reorganize: proposed structure").await;
        st.chats.update(&cid, |c| c.meta["phase"] = json!("review")).await;
        st.emit(json!({"type": "reorganize.ready", "chatId": cid}));
    })))).await?;
    Ok(id)
}

pub async fn reorganize_diff(state: &AppState, id: &str) -> anyhow::Result<Value> {
    let lib = library(state.settings.read().await.library())?;
    let c = state.chats.get(id).await.ok_or_else(|| anyhow::anyhow!("no such chat"))?;
    let (base, branch) = (c.meta["base"].as_str().unwrap_or("main"), c.meta["branch"].as_str().unwrap_or_default());
    let range = format!("{base}...{branch}");
    let stat = crate::git::git(state, &lib, &["diff", "--stat", "-M", &range]).await?;
    let mut diff = crate::git::git(state, &lib, &["diff", "-M", &range]).await?;
    if diff.len() > 400_000 { diff.truncate(400_000); diff.push_str("\n…(diff truncated)"); }
    Ok(json!({"stat": stat, "diff": diff, "phase": c.meta["phase"]}))
}

pub async fn reorganize_finish(state: &AppState, id: &str, keep: bool) -> anyhow::Result<()> {
    let lib = library(state.settings.read().await.library())?;
    let c = state.chats.get(id).await.ok_or_else(|| anyhow::anyhow!("no such chat"))?;
    let (base, branch) = (c.meta["base"].as_str().unwrap_or("main").to_string(), c.meta["branch"].as_str().unwrap_or_default().to_string());
    crate::git::commit_all(state, &lib, "reorganize: final touches").await?;
    crate::git::git(state, &lib, &["checkout", "-q", &base]).await?;
    if keep {
        if crate::git::git(state, &lib, &["merge", "--ff-only", &branch]).await.is_err() {
            crate::git::git(state, &lib, &["merge", "--no-edit", &branch]).await?;
        }
    }
    crate::git::git(state, &lib, &["branch", "-D", &branch]).await?;
    state.chats.update(id, |c| c.meta["phase"] = json!(if keep { "merged" } else { "discarded" })).await;
    Ok(())
}
