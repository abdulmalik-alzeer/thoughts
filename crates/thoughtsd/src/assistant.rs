//! Which assistant program answers a run: Claude Code (`claude.rs`) or Codex (`codex.rs`).
//!
//! Settings › Assistant names one of them for everything the app starts itself (Wrap, Reorganize,
//! set-up, adding a feature, automatic tasks, summaries) and for new chats; each chat then keeps
//! its own, and can be moved to the other one (`handoff.rs`). Everything else about a run — trust,
//! the permission mode, bypass only for ordinary chats, the standing files — is decided once, in
//! `claude::start_run`, whichever program then does the work.

use crate::settings::Settings;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program { Claude, Codex }

impl Program {
    pub fn name(self) -> &'static str { match self { Program::Claude => "claude", Program::Codex => "codex" } }
    /// The name as people know it (the same in both languages).
    pub fn label(self) -> &'static str { match self { Program::Claude => "Claude Code", Program::Codex => "Codex" } }
    pub fn from_name(n: &str) -> Option<Self> {
        match n { "claude" => Some(Program::Claude), "codex" => Some(Program::Codex), _ => None }
    }
}

/// The program chosen in Settings.
pub fn primary(s: &Settings) -> Program { Program::from_name(&s.ai.primary).unwrap_or(Program::Claude) }

/// The program a chat uses (a chat from before Codex existed, or with a name this version does not
/// know, uses Claude Code).
pub fn of_chat(provider: &str) -> Program { Program::from_name(provider).unwrap_or(Program::Claude) }

/// The program a run uses.
/// - An ordinary or customise chat: its own choice.
/// - Anything the app starts itself: the one chosen in Settings when it starts afresh — recorded on
///   its chat — and, when it continues a session (a Wrap's saving step continues its preview), the
///   one that made that session.
/// - Customising the app: always Claude Code (`CUSTOMIZE`), whatever the chat or Settings say.
pub async fn for_run(state: &AppState, chat_id: &str, kind: &str, resumes: bool) -> Program {
    let own = state.chats.with(chat_id, |c| of_chat(&c.provider)).await.unwrap_or(Program::Claude);
    if kind == "customize" {
        // A customise chat made on Codex by an earlier build is put right, not run there.
        if own != CUSTOMIZE { state.chats.update(chat_id, |c| c.provider = CUSTOMIZE.name().into()).await; }
        return CUSTOMIZE;
    }
    // MADO's runs are Claude Code's: its rules keep a planning step to `mado/` and a build task to its
    // approved commands, which Codex's sandbox cannot hold (`mado.rs`). Full access lifts that.
    if (kind == "mado" || kind == "mado-build") && !state.settings.read().await.full_access() {
        if own != Program::Claude { state.chats.update(chat_id, |c| c.provider = Program::Claude.name().into()).await; }
        return Program::Claude;
    }
    if kind == "chat" || resumes { return own; }
    let chosen = primary(&*state.settings.read().await);
    if chosen != own { state.chats.update(chat_id, |c| c.provider = chosen.name().into()).await; }
    chosen
}

/// The program that customises the app, whichever one is chosen in Settings. On Codex a customise run
/// could read the app's own data (settings, other chats and their files) — its sandbox closes writes,
/// not reads — which Claude Code's deny rules keep closed (`paths::secret_paths_in`).
pub const CUSTOMIZE: Program = Program::Claude;

/// Ask before a message is sent whether `program` is signed in, so a missing sign-in is said
/// plainly up front instead of as a failed answer.
pub async fn require_signed_in(state: &AppState, program: Program) -> anyhow::Result<()> {
    match program {
        Program::Claude => crate::claude::require_signed_in(state).await,
        Program::Codex => crate::codex::require_signed_in(state).await,
    }
}

/// `require_signed_in` before a customise message: Claude Code must be there and signed in. With
/// Codex chosen in Settings, the person is told that customising needs Claude Code (with the way to
/// sign in, when that is what is missing), not just that Claude Code is missing.
pub async fn require_for_customize(state: &AppState) -> anyhow::Result<()> {
    let r = require_signed_in(state, CUSTOMIZE).await;
    if primary(&*state.settings.read().await) == CUSTOMIZE { return r; }
    r.map_err(|e| match e.downcast_ref::<crate::tr::UserErr>().map(|u| u.code) {
        Some("assistant_missing") => crate::tr::err("customize.needs_claude", &[]),
        Some("signed_out") => crate::tr::err("customize.claude_signed_out", &[]),
        _ => e,
    })
}

/// The model a one-shot answer (a summary, a routine draft) of the chosen program asks for.
/// `background`: a summary the app makes by itself, which takes the cheapest suitable model — for
/// Claude Code the small model (`claude::background_model`), for Codex the one chosen for
/// summaries, else its own default (run at its lowest effort, see `codex::complete`).
pub fn one_shot_model(s: &Settings, background: bool) -> Option<String> { one_shot_model_for(s, primary(s), background) }

/// `one_shot_model` for a program named outright (a chat moved to it, see `handoff.rs`).
pub fn one_shot_model_for(s: &Settings, program: Program, background: bool) -> Option<String> {
    let pick = |m: &Option<String>| m.clone().filter(|m| !m.trim().is_empty());
    match (program, background) {
        (Program::Claude, true) => crate::claude::background_model(&s.ai.claude),
        (Program::Claude, false) => s.ai.claude.model.clone(),
        (Program::Codex, true) => pick(&s.ai.codex.mini_wrap_model).or_else(|| pick(&s.ai.codex.model)),
        (Program::Codex, false) => pick(&s.ai.codex.model),
    }
}

/// A one-shot answer from `program`, whichever is chosen in Settings. `cheap`: a summary the app
/// makes by itself, as cheaply as the program allows.
pub async fn complete_by(state: &AppState, program: Program, prompt: &str, model: Option<String>, cheap: bool) -> anyhow::Result<String> {
    match program {
        Program::Claude => crate::claude::complete_claude(state, prompt, model).await,
        Program::Codex => crate::codex::complete(state, prompt, model, cheap).await,
    }
}

/// The model and effort a new chat of `program` starts with.
pub fn chat_defaults(s: &Settings, program: Program) -> (Option<String>, Option<String>) {
    match program {
        Program::Claude => (s.ai.claude.model.clone(), s.ai.claude.effort.clone()),
        Program::Codex => (s.ai.codex.model.clone(), s.ai.codex.effort.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chats::Chat;

    #[tokio::test]
    async fn chats_keep_their_own_program_and_app_runs_take_the_one_in_settings() {
        let state = AppState::for_test();
        state.settings.write().await.ai.primary = "codex".into();
        let mut ids = std::collections::BTreeMap::new();
        for kind in ["chat", "customize", "wrap", "workflow", "setup", "reorganize", "module"] {
            let c = Chat { kind: kind.into(), ..Default::default() };
            ids.insert(kind, c.id.clone());
            state.chats.insert(c).await;
        }
        // A chat was made for Claude Code and stays with it until the user moves it.
        assert_eq!(for_run(&state, &ids["chat"], "chat", false).await, Program::Claude);
        assert_eq!(for_run(&state, &ids["customize"], "customize", false).await, Program::Claude);
        // What the app starts afresh goes to the program chosen in Settings, which the chat records…
        for kind in ["wrap", "workflow", "setup", "reorganize", "module"] {
            assert_eq!(for_run(&state, &ids[kind], kind, false).await, Program::Codex, "{kind}");
            assert_eq!(state.chats.get(&ids[kind]).await.unwrap().provider, "codex", "{kind}");
        }
        // …and a step that continues its session stays with the program that made it, even after
        // Settings changed in between (a Wrap previewed by Codex is saved by Codex).
        state.settings.write().await.ai.primary = "claude".into();
        assert_eq!(for_run(&state, &ids["wrap"], "wrap", true).await, Program::Codex);
        assert_eq!(for_run(&state, &ids["workflow"], "workflow", false).await, Program::Claude);
        // A name this version does not know is Claude Code.
        assert_eq!(of_chat("gpt"), Program::Claude);
    }

    #[test]
    fn summaries_take_the_cheapest_model_of_the_program_in_settings() {
        let mut s = Settings::default();
        assert_eq!(one_shot_model(&s, true).as_deref(), Some("haiku"));
        s.ai.claude.model = Some("opus".into());
        assert_eq!(one_shot_model(&s, false).as_deref(), Some("opus"));
        s.ai.primary = "codex".into();
        // No Claude name is ever sent to Codex: its own default (at its lowest effort) unless one is chosen.
        assert_eq!(one_shot_model(&s, true), None);
        assert_eq!(one_shot_model(&s, false), None);
        s.ai.codex.model = Some("gpt-5".into());
        assert_eq!(one_shot_model(&s, true).as_deref(), Some("gpt-5"));
        s.ai.codex.mini_wrap_model = Some("gpt-5-mini".into());
        assert_eq!(one_shot_model(&s, true).as_deref(), Some("gpt-5-mini"));
        assert_eq!(one_shot_model(&s, false).as_deref(), Some("gpt-5"));
        assert_eq!(chat_defaults(&s, Program::Codex), (Some("gpt-5".into()), None));
    }
}
