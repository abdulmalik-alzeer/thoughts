use super::*;
use crate::state::AppState;
use std::sync::Arc;

fn claude_like_session() -> Value {
    json!({
        "sessionId": "s1",
        "modes": {"currentModeId": "default", "availableModes": [
            {"id": "default", "name": "Manual"}, {"id": "acceptEdits", "name": "Accept edits"}, {"id": "plan", "name": "Plan"},
            {"id": "auto", "name": "Auto"}, {"id": "bypassPermissions", "name": "Bypass permissions"}]},
        "configOptions": [
            {"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "default", "options": [
                {"value": "default", "name": "Manual"}, {"value": "acceptEdits", "name": "Accept edits"}, {"value": "plan", "name": "Plan"},
                {"value": "auto", "name": "Auto"}, {"value": "bypassPermissions", "name": "Bypass permissions"}]},
            {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "default", "options": [
                {"group": "main", "name": "Main", "options": [{"value": "default", "name": "Default (recommended)"}, {"value": "opus", "name": "Opus"}]},
                {"value": "haiku", "name": "Haiku"}]},
            {"id": "effort", "name": "Effort", "category": "thought_level", "type": "select", "currentValue": "high", "options": [
                {"value": "low", "name": "Low"}, {"value": "high", "name": "High"}, {"value": "max", "name": "Max"}]},
        ],
    })
}

#[test]
fn the_catalog_is_what_the_agent_reports() {
    let c = catalog(&claude_like_session());
    assert_eq!(c.modes.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["default", "acceptEdits", "plan", "auto", "bypassPermissions"]);
    assert_eq!(c.modes[0].name, "Manual");
    assert_eq!(c.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["default", "opus", "haiku"]);
    assert_eq!(c.efforts.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["low", "high", "max"]);
    assert_eq!((c.mode_option.as_deref(), c.model_option.as_deref(), c.effort_option.as_deref()), (Some("mode"), Some("model"), Some("effort")));
    assert_eq!(c.current_effort.as_deref(), Some("high"));
    // Only the older fields (a Codex-like agent without config options).
    let old = catalog(&json!({"modes": {"currentModeId": "agent", "availableModes": [{"id": "read-only", "name": "Read Only"}, {"id": "agent", "name": "Agent"}]},
        "models": {"currentModelId": "gpt-5", "availableModels": [{"modelId": "gpt-5", "name": "GPT-5"}]}}));
    assert_eq!(old.modes.len(), 2);
    assert_eq!(old.current_mode.as_deref(), Some("agent"));
    assert_eq!(old.models[0].id, "gpt-5");
    assert!(old.mode_option.is_none() && old.efforts.is_empty());
    assert_eq!(catalog(&json!({})), Catalog::default());
}

#[test]
fn options_list_the_agents_choices_under_its_names() {
    let c = catalog(&claude_like_session());
    let mut out = json!({"models": ["sonnet", "opus"], "efforts": ["low"], "permissionModes": ["acceptEdits", "plan", "bypassPermissions"]});
    crate::acp_agent::apply_catalog(crate::assistant::Program::Claude, &c, &mut out);
    assert_eq!(out["models"], json!(["default", "opus", "haiku", "sonnet"]));
    assert_eq!(out["efforts"], json!(["low", "high", "max"]));
    assert_eq!(out["permissionModes"], json!(["default", "acceptEdits", "plan", "auto", "bypassPermissions"]));
    assert_eq!(out["modeNames"]["default"], "Manual");
    assert_eq!(out["modelNames"]["opus"], "Opus");
    // A mode the daemon does not accept is never offered; Codex's own modes never replace the app's.
    let odd = catalog(&json!({"modes": {"availableModes": [{"id": "dontAsk", "name": "Don't ask"}, {"id": "plan", "name": "Plan"}]}}));
    let mut o2 = json!({"models": [], "permissionModes": ["acceptEdits"]});
    crate::acp_agent::apply_catalog(crate::assistant::Program::Claude, &odd, &mut o2);
    assert_eq!(o2["permissionModes"], json!(["plan"]));
    let mut o3 = json!({"models": [], "permissionModes": ["acceptEdits"]});
    crate::acp_agent::apply_catalog(crate::assistant::Program::Codex, &odd, &mut o3);
    assert_eq!(o3["permissionModes"], json!(["acceptEdits"]));
}

#[test]
fn a_permission_answer_never_switches_permission_checks_off() {
    // Leaving plan mode, as the pinned adapter offers it (`permissions/options/tools.js`).
    let exit_plan = json!([
        {"optionId": "exit-plan-clear-auto", "name": "Yes, clear context and use auto mode", "kind": "allow_always"},
        {"optionId": "exit-plan-auto", "name": "Yes, and use auto mode", "kind": "allow_always"},
        {"optionId": "exit-plan-bypass", "name": "Yes, and bypass permissions", "kind": "allow_always"},
        {"optionId": "exit-plan-default", "name": "Yes, manually approve edits", "kind": "allow_once"},
        {"optionId": "reject", "name": "No, keep planning", "kind": "reject_once"},
    ]);
    assert_eq!(pick_option(&exit_plan, true, "acceptEdits", false).as_deref(), Some("exit-plan-default"));
    assert_eq!(pick_option(&exit_plan, false, "acceptEdits", false).as_deref(), Some("reject"));
    // Only standing choices that switch to auto or bypass: never picked by a chat checked for less.
    let elevated = json!([{"optionId": "exit-plan-auto", "kind": "allow_always"}, {"optionId": "exit-plan-clear-auto", "kind": "allow_always"},
        {"optionId": "exit-plan-bypass", "kind": "allow_always"}, {"optionId": "reject", "kind": "reject_once"}]);
    for checked in ["plan", "default", "acceptEdits"] { assert_eq!(pick_option(&elevated, true, checked, false), None, "{checked}"); }
    assert_eq!(pick_option(&elevated, true, "auto", false).as_deref(), Some("exit-plan-auto"));
    assert_eq!(pick_option(&elevated, true, crate::settings::BYPASS, false).as_deref(), Some("exit-plan-auto"));
    // A chat checked for plan mode never leaves it through an answer.
    assert_eq!(pick_option(&exit_plan, true, "plan", false), None);
    let mode_ids = json!([{"optionId": "bypassPermissions", "kind": "allow_always"}, {"optionId": "acceptEdits", "kind": "allow_always"}, {"optionId": "no", "kind": "reject_once"}]);
    assert_eq!(pick_option(&mode_ids, true, "default", false), None);
    assert_eq!(pick_option(&mode_ids, true, "acceptEdits", false).as_deref(), Some("acceptEdits"));
    // Allowlist: an option the app does not know is never picked, whatever its kind says.
    let unknown = json!([{"optionId": "yolo-mode", "kind": "allow_once"}, {"optionId": "skip-all", "kind": "allow_always"}, {"optionId": "nope-forever", "kind": "reject_once"}]);
    assert_eq!(pick_option(&unknown, true, crate::settings::BYPASS, false), None);
    assert_eq!(pick_option(&unknown, false, "default", false), None);
    // Full access: this guard yields like every other.
    assert_eq!(pick_option(&unknown, true, "default", true).as_deref(), Some("yolo-mode"));
    assert_eq!(pick_option(&elevated, true, "plan", true).as_deref(), Some("exit-plan-auto"));
    let tool = json!([{"optionId": "allow-once", "kind": "allow_once"}, {"optionId": "allow-with-updates", "kind": "allow_always"}, {"optionId": "reject", "kind": "reject_once"}]);
    assert_eq!(pick_option(&tool, true, "default", false).as_deref(), Some("allow-once"));
    assert_eq!(permission_answer(None), json!({"outcome": {"outcome": "cancelled"}}));
    assert_eq!(permission_answer(Some("allow".into())), json!({"outcome": {"outcome": "selected", "optionId": "allow"}}));
}

#[test]
fn modes_are_ranked_and_unknown_ones_are_never_within() {
    assert!(mode_within("acceptEdits", "plan") && mode_within("acceptEdits", "default") && mode_within("acceptEdits", "acceptEdits"));
    assert!(!mode_within("acceptEdits", "auto") && !mode_within("auto", crate::settings::BYPASS) && !mode_within("plan", "default"));
    assert!(!mode_within("acceptEdits", "dontAsk") && !mode_within("dontAsk", "plan") && !mode_within("default", ""));
    let mut tr = Translator::default();
    tr.update(&json!({"sessionUpdate": "config_option_update", "configOptions": [{"id": "model", "category": "model", "currentValue": "opus"},
        {"id": "mode", "category": "mode", "currentValue": "auto"}]}));
    assert_eq!(tr.mode.as_deref(), Some("auto"));
    tr.update(&json!({"sessionUpdate": "current_mode_update", "currentModeId": "plan"}));
    assert_eq!(tr.mode.as_deref(), Some("plan"));
}

#[test]
fn updates_become_the_stream_the_chat_already_shows() {
    let mut t = Translator::default();
    let d = t.update(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Hel"}}));
    assert_eq!(d[0]["event"]["delta"]["text"], "Hel");
    t.update(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "lo"}}));
    let think = t.update(&json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "hmm"}}));
    assert_eq!(think[0]["event"]["delta"]["thinking"], "hmm");
    let call = t.update(&json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Read a.md", "status": "pending",
        "rawInput": {"file_path": "a.md"}, "_meta": {"claudeCode": {"toolName": "Read"}}}));
    // The text so far is one message, then the tool call.
    assert_eq!(call[0]["message"]["content"][0]["text"], "Hello");
    assert_eq!(call[1]["message"]["content"][0]["name"], "Read");
    assert_eq!(call[1]["message"]["content"][0]["input"]["file_path"], "a.md");
    assert!(t.update(&json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "in_progress"})).is_empty());
    let done = t.update(&json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed",
        "content": [{"type": "content", "content": {"type": "text", "text": "# A"}}]}));
    assert_eq!(done[0]["message"]["content"][0]["tool_use_id"], "t1");
    assert_eq!(done[0]["message"]["content"][0]["content"], "# A");
    // Reported once.
    assert!(t.update(&json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"})).is_empty());
    t.update(&json!({"sessionUpdate": "usage_update", "used": 10, "size": 100, "cost": {"amount": 0.25, "currency": "USD"}}));
    t.update(&json!({"sessionUpdate": "current_mode_update", "currentModeId": "acceptEdits"}));
    assert_eq!(t.mode.as_deref(), Some("acceptEdits"));
    t.update(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Done."}}));
    let end = t.end(Ok("end_turn"), "s1");
    assert_eq!(end[0]["message"]["content"][0]["text"], "Done.");
    assert_eq!(end[1]["type"], "result");
    assert_eq!(end[1]["is_error"], false);
    assert_eq!(end[1]["session_id"], "s1");
    assert_eq!(end[1]["total_cost_usd"], 0.25);
    let failed = Translator::default().end(Err("overloaded".into()), "s1");
    assert_eq!(failed[0]["is_error"], true);
    assert_eq!(failed[0]["result"], "overloaded");
}

/// A fake ACP agent as a shell script over stdio: it answers by request id and plays one turn
/// (text, a tool call, a permission request, the end). Every line it reads is kept in `log.txt`.
fn fake_agent(dir: &std::path::Path, mode_after_set: &str) -> std::path::PathBuf { fake_agent_with(dir, mode_after_set, "") }

/// The fake agent, sending `turn_update` (a `session/update` update, as JSON) first in its turn.
fn fake_agent_with(dir: &std::path::Path, mode_after_set: &str, turn_update: &str) -> std::path::PathBuf {
    let script = dir.join("agent.sh");
    let log = dir.join("log.txt");
    let session = claude_like_session().to_string();
    let text = format!(r#"#!/bin/sh
while IFS= read -r line; do
  printf '%s\n' "$line" >> '{log}'
  id=$(printf '%s' "$line" | sed -n 's/^{{"id":\([0-9]*\),.*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":1,"agentCapabilities":{{}}}}}}\n' "$id" ;;
    *'"method":"session/new"'*|*'"method":"session/resume"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":%s}}\n' "$id" '{session}' ;;
    *'"method":"session/set_config_option"'*|*'"method":"session/set_mode"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{}}}}\n' "$id"
      printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s1","update":{{"sessionUpdate":"current_mode_update","currentModeId":"{mode}"}}}}}}\n' ;;
    *'"method":"session/prompt"'*)
      [ -n '{turn}' ] && printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s1","update":%s}}}}\n' '{turn}'
      printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Hi there"}}}}}}}}\n'
      printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s1","update":{{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Bash","status":"pending","rawInput":{{"command":"ls"}},"_meta":{{"claudeCode":{{"toolName":"Bash"}}}}}}}}}}\n'
      printf '{{"jsonrpc":"2.0","id":"perm-1","method":"session/request_permission","params":{{"sessionId":"s1","toolCall":{{"toolCallId":"t1","rawInput":{{"command":"ls"}}}},"options":[{{"optionId":"allow","kind":"allow_once"}},{{"optionId":"reject","kind":"reject_once"}}]}}}}\n'
      IFS= read -r answer
      printf '%s\n' "$answer" >> '{log}'
      printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s1","update":{{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"failed","content":[{{"type":"content","content":{{"type":"text","text":"refused"}}}}]}}}}}}\n'
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$id" ;;
  esac
done
"#, log = log.display(), session = session, mode = mode_after_set, turn = turn_update);
    std::fs::write(&script, text).unwrap();
    script
}

fn spec(dir: &std::path::Path, mode: &str) -> crate::acp_agent::ChatSpec {
    crate::acp_agent::ChatSpec { cwd: dir.to_path_buf(), add_dirs: vec![], resume: None, fork: false, model: Some("opus".into()), effort: Some("max".into()),
        mode: mode.into(), rules: Some(dir.join("rules.json")), system: "app notes".into(), budget: Some(2.0), prompt: "hello".into(),
        setting_sources: None, strict_mcp: false }
}

fn sh(script: &std::path::Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new("/bin/sh");
    c.arg(script).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    c
}

#[tokio::test]
async fn a_chat_turn_runs_over_acp_with_a_fake_agent() {
    let dir = std::env::temp_dir().join(format!("thoughts-acp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = fake_agent(&dir, "acceptEdits");
    let (child, conn, rx) = crate::acp_agent::connect(sh(&script)).await.unwrap();
    let s = spec(&dir, "acceptEdits");
    let (session, cat) = crate::acp_agent::open_chat(&conn, &s).await.unwrap();
    assert_eq!(session, "s1");
    assert_eq!(cat.models.len(), 3);
    let st = Arc::new(AppState::for_test());
    let chat = crate::chats::Chat::default();
    st.chats.insert(chat.clone()).await;
    let (_kill_tx, mut kill_rx) = tokio::sync::oneshot::channel();
    // Nobody can be asked: the permission request is refused.
    let f = crate::acp_agent::drive(&st, &chat.id, child, conn, rx, session, s.prompt.clone(), asker("acceptEdits"), &mut kill_rx).await;
    assert_eq!(f.ended, crate::claude::Ended::Result(true));
    let saved = st.chats.get(&chat.id).await.unwrap();
    assert_eq!(saved.session_id.as_deref(), Some("s1"));
    let roles: Vec<&str> = saved.messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, ["assistant", "tool", "tool_result"]);
    assert_eq!(saved.messages[0].text, "Hi there");
    let log = std::fs::read_to_string(dir.join("log.txt")).unwrap();
    let lines: Vec<Value> = log.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let new = lines.iter().find(|l| l["method"] == "session/new").unwrap();
    let o = &new["params"]["_meta"]["claudeCode"]["options"];
    // The rules file, the notes on top of Claude Code's prompt, the limit, and no bypass.
    assert_eq!(o["settings"], json!(dir.join("rules.json")));
    assert_eq!(o["systemPrompt"]["append"], "app notes");
    assert_eq!(o["allowDangerouslySkipPermissions"], false);
    assert_eq!(o["maxBudgetUsd"], 2.0);
    let set: Vec<(&str, &str)> = lines.iter().filter(|l| l["method"] == "session/set_config_option")
        .map(|l| (l["params"]["configId"].as_str().unwrap(), l["params"]["value"].as_str().unwrap())).collect();
    assert_eq!(set, [("mode", "acceptEdits"), ("model", "opus"), ("effort", "max")]);
    let answer = lines.iter().find(|l| l["id"] == "perm-1").unwrap();
    assert_eq!(answer["result"]["outcome"]["optionId"], "reject");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_mode_the_agent_does_not_offer_never_starts_and_a_switch_to_bypass_stops_the_turn() {
    let dir = std::env::temp_dir().join(format!("thoughts-acp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = fake_agent(&dir, "bypassPermissions");
    let (mut child, conn, _rx) = crate::acp_agent::connect(sh(&script)).await.unwrap();
    assert!(crate::acp_agent::open_chat(&conn, &spec(&dir, "dontAsk")).await.is_err());
    crate::acp_agent::end(&mut child).await;
    // The agent reports bypass after the mode was set: the turn is ended, never carried on.
    let (child, conn, rx) = crate::acp_agent::connect(sh(&script)).await.unwrap();
    let (session, _) = crate::acp_agent::open_chat(&conn, &spec(&dir, "plan")).await.unwrap();
    let st = Arc::new(AppState::for_test());
    let chat = crate::chats::Chat::default();
    st.chats.insert(chat.clone()).await;
    let (_k, mut kill_rx) = tokio::sync::oneshot::channel();
    let f = crate::acp_agent::drive(&st, &chat.id, child, conn, rx, session, "x".into(), asker("acceptEdits"), &mut kill_rx).await;
    assert!(matches!(f.ended, crate::claude::Ended::Lost(_)), "{:?}", f.ended);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn resume_and_branch_open_the_chats_own_session() {
    let dir = std::env::temp_dir();
    let mut s = spec(&dir, "plan");
    assert_eq!(crate::acp_agent::open_request(&s).0, "session/new");
    s.resume = Some("abc".into());
    let (m, p) = crate::acp_agent::open_request(&s);
    assert_eq!((m, p["sessionId"].as_str()), ("session/resume", Some("abc")));
    s.fork = true;
    assert_eq!(crate::acp_agent::open_request(&s).0, "session/fork");
    s.mode = crate::settings::BYPASS.into();
    assert_eq!(crate::acp_agent::claude_options(&s)["allowDangerouslySkipPermissions"], true);
}

#[test]
fn adapters_are_pinned_and_installed_without_scripts() {
    use crate::acp_install::*;
    for p in [crate::assistant::Program::Claude, crate::assistant::Program::Codex] {
        let pin = pin(p);
        assert!(pin.version.split('.').all(|n| n.parse::<u32>().is_ok()), "an exact version: {}", pin.version);
        let a: Vec<String> = install_args(pin, std::path::Path::new("/x")).iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert!(a.contains(&"--ignore-scripts".to_string()) && a.contains(&"--omit=optional".to_string()));
        assert_eq!(a.last().unwrap(), &format!("{}@{}", pin.package, pin.version));
        assert!(dir(p).starts_with(crate::paths::home()));
    }
    let d = std::env::temp_dir().join(format!("thoughts-acp-pin-{}", uuid::Uuid::new_v4()));
    let pkg = d.join("node_modules").join(CLAUDE.package);
    std::fs::create_dir_all(pkg.join("dist")).unwrap();
    std::fs::write(pkg.join("dist/index.js"), "").unwrap();
    std::fs::write(pkg.join("package.json"), r#"{"version":"0.0.1"}"#).unwrap();
    assert!(entry_in(&d, &CLAUDE).is_none(), "another version is not the pinned one");
    std::fs::write(pkg.join("package.json"), format!(r#"{{"version":"{}"}}"#, CLAUDE.version)).unwrap();
    assert_eq!(entry_in(&d, &CLAUDE), Some(pkg.join("dist/index.js")));
    std::fs::remove_dir_all(&d).unwrap();
}

fn asker(mode: &str) -> crate::acp_agent::Asker { crate::acp_agent::Asker { token: None, mode: mode.into(), full: false } }

/// One turn of a chat checked for `checked`, with the agent sending `turn_update` first.
async fn turn_with(checked: &str, turn_update: &str) -> crate::claude::Ended {
    let dir = std::env::temp_dir().join(format!("thoughts-acp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = fake_agent_with(&dir, checked, turn_update);
    let (child, conn, rx) = crate::acp_agent::connect(sh(&script)).await.unwrap();
    let (session, _) = crate::acp_agent::open_chat(&conn, &spec(&dir, checked)).await.unwrap();
    let st = Arc::new(AppState::for_test());
    let chat = crate::chats::Chat::default();
    st.chats.insert(chat.clone()).await;
    let (_k, mut kill_rx) = tokio::sync::oneshot::channel();
    let f = crate::acp_agent::drive(&st, &chat.id, child, conn, rx, session, "x".into(), asker(checked), &mut kill_rx).await;
    std::fs::remove_dir_all(&dir).unwrap();
    f.ended
}

#[tokio::test]
async fn a_switch_mid_turn_to_any_mode_that_does_more_ends_the_turn() {
    let mode_update = |m: &str| json!({"sessionUpdate": "current_mode_update", "currentModeId": m}).to_string();
    let config_update = |m: &str| json!({"sessionUpdate": "config_option_update", "configOptions": [{"id": "mode", "category": "mode", "currentValue": m}]}).to_string();
    // Auto (not only bypass), by either kind of update, ends a chat checked for accept-edits.
    for u in [mode_update("auto"), config_update("auto"), config_update(crate::settings::BYPASS), mode_update("dontAsk")] {
        assert!(matches!(turn_with("acceptEdits", &u).await, crate::claude::Ended::Lost(_)), "{u}");
    }
    assert!(matches!(turn_with("plan", &config_update("default")).await, crate::claude::Ended::Lost(_)));
    // The same mode or a stricter one goes on.
    assert_eq!(turn_with("acceptEdits", &mode_update("plan")).await, crate::claude::Ended::Result(true));
    assert_eq!(turn_with("auto", &config_update("acceptEdits")).await, crate::claude::Ended::Result(true));
}

#[tokio::test]
async fn the_profile_guards_travel_to_the_adapter() {
    let dir = std::env::temp_dir().join(format!("thoughts-acp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = fake_agent(&dir, "acceptEdits");
    let (mut child, conn, _rx) = crate::acp_agent::connect(sh(&script)).await.unwrap();
    // A chat with web lookups: no connected tools (`--strict-mcp-config` on the command line).
    let web = crate::claude::profile("chat", "acceptEdits", true);
    assert!(!web.mcp);
    let mut s = spec(&dir, "acceptEdits");
    s.setting_sources = web.setting_sources().map(String::from);
    s.strict_mcp = !web.mcp;
    crate::acp_agent::open_chat(&conn, &s).await.unwrap();
    crate::acp_agent::end(&mut child).await;
    let log = std::fs::read_to_string(dir.join("log.txt")).unwrap();
    let new: Value = log.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).find(|l| l["method"] == "session/new").unwrap();
    let o = &new["params"]["_meta"]["claudeCode"]["options"];
    assert_eq!(o["strictMcpConfig"], true);
    assert_eq!(o["mcpServers"], json!({}));
    assert_eq!(o["disallowedTools"], json!(["mcp__*"]));
    assert!(o.get("settingSources").is_none(), "an ordinary chat keeps the user's own set-up");
    std::fs::remove_dir_all(&dir).unwrap();
    // Setting sources as the command line names them: some, or none at all.
    s.setting_sources = Some("project,local".into());
    assert_eq!(crate::acp_agent::claude_options(&s)["settingSources"], json!(["project", "local"]));
    s.setting_sources = Some(String::new());
    assert_eq!(crate::acp_agent::claude_options(&s)["settingSources"], json!([]));
    s.strict_mcp = false;
    assert!(crate::acp_agent::claude_options(&s).get("strictMcpConfig").is_none());
    // `--restricted` has no equivalent over ACP: such a profile is never run there.
    for kind in ["chat", "customize"] { assert!(crate::acp_agent::carries(&crate::claude::profile(kind, "acceptEdits", false)), "{kind}"); }
    assert!(crate::acp_agent::carries(&crate::full_access::profile()));
    for kind in ["workflow", "module", "mado", "mado-build", "wrap"] { assert!(!crate::acp_agent::carries(&crate::claude::profile(kind, "acceptEdits", false)), "{kind}"); }
}
