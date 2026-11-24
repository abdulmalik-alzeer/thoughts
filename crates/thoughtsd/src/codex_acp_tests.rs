//! Tests for Codex chats over ACP (`codex_acp.rs`): what travels to the adapter, what stays on
//! `codex exec`, how Codex's updates are shown, and whole turns against a stand-in for codex-acp.

use super::*;
use std::time::Duration;

const SID: &str = "0199aaaa-1111-4222-8333-000000000001";

fn session_answer(mode: &str) -> Value {
    json!({
        "sessionId": SID,
        "configOptions": [
            {"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": mode, "options": [
                {"value": "read-only", "name": "Read-only"}, {"value": "workspace-write", "name": "Workspace access"},
                {"value": "agent", "name": "Auto review"}, {"value": "agent-full-access", "name": "Full access"}]},
            {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "gpt-5", "options": [
                {"value": "gpt-5", "name": "5"}, {"value": "gpt-5-codex", "name": "5 Codex"}]},
            {"id": "reasoning_effort", "name": "Reasoning effort", "category": "thought_level", "type": "select", "currentValue": "medium", "options": [
                {"value": "low", "name": "Low"}, {"value": "medium", "name": "Medium"}, {"value": "high", "name": "High"}]},
            {"id": "collaboration_mode", "name": "Collaboration mode", "category": "collaboration_mode", "type": "select", "currentValue": "default", "options": [
                {"value": "default", "name": "Default"}, {"value": "plan", "name": "Plan"}]}
        ]
    })
}

#[test]
fn versions_are_read_from_the_program_and_from_the_adapters_own_dependency() {
    assert_eq!(parse_version("codex-cli 0.27.0"), Some((0, 27, 0)));
    assert_eq!(parse_version("^0.171.0"), Some((0, 171, 0)));
    assert_eq!(parse_version("codex"), None);
    let d = std::env::temp_dir().join(format!("thoughts-codex-acp-pkg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(d.join("dist")).unwrap();
    let entry = d.join("dist/index.js");
    assert_eq!(required_codex(&entry), None, "no package.json: nothing is guessed");
    std::fs::write(d.join("package.json"), r#"{"dependencies":{"@openai/codex":"^0.171.0"}}"#).unwrap();
    assert_eq!(required_codex(&entry), Some((0, 171, 0)));
    assert!((0, 27, 0) < (0, 171, 0) && (0, 171, 2) >= (0, 171, 0) && (1, 0, 0) > (0, 171, 0));
    std::fs::remove_dir_all(d).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn an_older_codex_or_one_without_app_server_is_never_driven_by_the_adapter() {
    use std::os::unix::fs::PermissionsExt;
    let d = std::env::temp_dir().join(format!("thoughts-codex-acp-fit-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(d.join("pkg/dist")).unwrap();
    std::fs::write(d.join("pkg/package.json"), r#"{"dependencies":{"@openai/codex":"^0.171.0"}}"#).unwrap();
    let entry = d.join("pkg/dist/index.js");
    // A stand-in that says its version and lists its sub-commands, and records anything else it is asked.
    let codex = |version: &str, commands: &str| {
        let bin = d.join(format!("codex-{version}"));
        std::fs::write(&bin, format!("#!/bin/sh\ncase \"$1\" in --version) echo 'codex-cli {version}';; --help) printf 'Commands:\\n{commands}\\n\\nOptions:\\n';; *) echo \"$@\" >> '{}';; esac\n",
            d.join("other.txt").display())).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    };
    let node = crate::claude::find_binary(&None, "node").is_some();
    for (version, commands, want) in [("0.27.0", "  exec  Run\\n  proto  Protocol", Err(if node { Stay::TooOld } else { Stay::NoNode })),
                                      ("0.171.0", "  exec  Run\\n  proto  Protocol", Err(if node { Stay::NoAppServer } else { Stay::NoNode })),
                                      ("0.172.1", "  exec  Run\\n  app-server  Serve", if node { Ok(()) } else { Err(Stay::NoNode) })] {
        let st = AppState::for_test();
        st.settings.write().await.ai.codex.binary = Some(codex(version, commands).to_string_lossy().into());
        assert_eq!(codex_fits(&st, &entry).await, want, "{version}");
    }
    assert!(!d.join("other.txt").exists(), "nothing but its version and help was asked of it");
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn what_cannot_travel_keeps_the_command_line() {
    assert_eq!(stays("chat", false, false, false, false), None);
    for kind in ["wrap", "workflow", "setup", "module", "customize", "mado", "mado-build"] { assert_eq!(stays(kind, false, false, false, false), Some(Stay::NotChat), "{kind}"); }
    assert_eq!(stays("chat", true, false, false, false), Some(Stay::Classic));
    // Bypass on macOS: `codex exec` runs inside the app's own sandbox.
    assert_eq!(stays("chat", false, true, false, false), Some(Stay::OuterSandbox));
    assert_eq!(stays("chat", false, false, false, true), Some(Stay::OwnSettings));
    assert_eq!(stays("chat", false, false, true, true), None, "full access: the folder's own settings yield too");
    // The sandboxes as the adapter's modes, and how much each does.
    assert_eq!(mode_for("read-only", false), "read-only");
    assert_eq!(mode_for("workspace-write", false), "workspace-write");
    assert_eq!(mode_for("danger-full-access", false), "agent-full-access");
    assert_eq!(mode_for("workspace-write", true), "agent-full-access");
    for (checked, now, ok) in [("workspace-write", "read-only", true), ("workspace-write", "workspace-write", true), ("workspace-write", "agent", false),
                               ("workspace-write", "agent-full-access", false), ("read-only", "workspace-write", false), ("agent-full-access", "workspace-write", true),
                               ("workspace-write", "permission-profile:x", false)] {
        assert_eq!(crate::acp::mode_within(checked, now), ok, "{checked} → {now}");
    }
}

#[test]
fn a_folder_with_codex_settings_of_its_own_is_found_above_it_too() {
    let d = std::env::temp_dir().join(format!("thoughts-codex-acp-own-{}", uuid::Uuid::new_v4()));
    let (work, home) = (d.join("proj/sub"), d.join("codex-home"));
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("config.toml"), "").unwrap();
    assert!(!own_settings(&[&work], &home));
    std::fs::create_dir_all(d.join("proj/.codex")).unwrap();
    assert!(!own_settings(&[&work], &home), "an empty .codex folder sets nothing");
    std::fs::write(d.join("proj/.codex/config.toml"), "[mcp_servers.x]").unwrap();
    assert!(own_settings(&[&work], &home));
    // Codex's own folder is the user's settings, loaded either way.
    let lone = d.join("lone");
    std::fs::create_dir_all(lone.join(".codex")).unwrap();
    std::fs::write(lone.join(".codex/config.toml"), "").unwrap();
    assert!(!own_settings(&[&lone.join("x")], &lone.join(".codex")));
    std::fs::remove_dir_all(d).unwrap();
}

/// The command line of a chat's `codex exec` and the adapter's session config made from it.
fn config_for(kind: &str, profile: &crate::claude::Profile, svc: Option<&crate::providers::CodexService>) -> (Vec<OsString>, Value) {
    let help = "Options:\n      --json  JSON\n  -s, --sandbox <MODE>  [possible values: read-only, workspace-write, danger-full-access]\n      --skip-git-repo-check  x\n";
    let sw = crate::codex::switches(kind, "acceptEdits", profile);
    let provider = svc.map(|s| crate::providers::codex_args(s, &["MY_TOKEN".into()]));
    let writable = vec![PathBuf::from("/lib")];
    let resume = crate::codex::Resume::File(PathBuf::from("/x.jsonl"));
    let args = crate::codex::exec_args(&crate::codex::Plan { help, sandbox: sw.sandbox, model: Some("gpt-5"), effort: Some("high"), web: sw.web,
        own_setup: sw.own_setup, mcp: sw.mcp, network: sw.network, images: &[], writable: &writable, resume: Some(&resume), provider: provider.as_deref() });
    let c = session_config(&args, svc);
    (args, c)
}

#[test]
fn every_setting_codex_exec_gets_travels_as_the_session_config() {
    let chat = crate::claude::profile("chat", "acceptEdits", false);
    let (args, c) = config_for("chat", &chat, None);
    let n = args.iter().filter(|a| *a == "-c").count();
    // Everything but how it resumes and the writable folders (which ACP carries in their own way).
    assert_eq!(c.as_object().unwrap().len(), n - 2, "{c}");
    assert_eq!((c["model"].as_str(), c["model_reasoning_effort"].as_str()), (Some("gpt-5"), Some("high")));
    assert_eq!(c["history.persistence"], "none");
    assert_eq!(c["sandbox_workspace_write.network_access"], false);
    assert!(c.get("experimental_resume").is_none() && c.get("sandbox_workspace_write.writable_roots").is_none());
    if !crate::claude::sends_reports() { assert_eq!(c["analytics.enabled"], false); }
    // An ordinary chat keeps the user's MCP servers and notifier, as on the command line.
    assert!(c.get("mcp_servers").is_none() && c.get("notify").is_none());
    // A chat with web lookups: no connected tools, Codex's web search, and it reads only.
    let web = crate::claude::profile("chat", "acceptEdits", true);
    let (_, c) = config_for("chat", &web, None);
    assert_eq!((c["mcp_servers"].clone(), c["tools.web_search"].clone()), (json!({}), json!(true)));
    assert_eq!(crate::codex::sandbox_of("chat", "acceptEdits", &web), "read-only");
    // Another provider: as a table the adapter starts Codex with, the key only named, excludes as `codex exec` has them.
    let svc = crate::providers::CodexService { base_url: "https://llm.example/v1".into(), key: "sk-secret-123".into() };
    let (_, c) = config_for("chat", &chat, Some(&svc));
    assert!(!c.to_string().contains("sk-secret-123"), "the key never travels in the config");
    assert_eq!(c["model_provider"], c["model_providers"].as_object().unwrap().keys().next().unwrap().as_str());
    let p = c["model_providers"].as_object().unwrap().values().next().unwrap();
    assert_eq!((p["base_url"].as_str(), p["env_key"].as_str()), (Some("https://llm.example/v1"), Some(crate::providers::CODEX_KEY_ENV)));
    assert!(!c.as_object().unwrap().keys().any(|k| k.starts_with("model_providers.")));
    let ex = c["shell_environment_policy.exclude"].as_array().unwrap();
    assert!(ex.contains(&json!("MY_TOKEN")) && ex.contains(&json!(crate::providers::CODEX_KEY_ENV)));
    assert_eq!(c["shell_environment_policy.ignore_default_excludes"], false);
}

#[test]
fn the_session_opens_in_the_chats_folders_and_resumes_its_own() {
    let (m, p) = open_request(Path::new("/work"), &[PathBuf::from("/lib")], None);
    assert_eq!(m, "session/new");
    assert_eq!((p["cwd"].as_str(), p["additionalDirectories"].clone(), p["mcpServers"].clone()), (Some("/work"), json!(["/lib"]), json!([])));
    assert!(p.get("_meta").is_none(), "nothing of Claude Code's options goes to Codex");
    let (m, p) = open_request(Path::new("/work"), &[], Some(SID));
    assert_eq!((m, p["sessionId"].as_str()), ("session/resume", Some(SID)));
}

#[test]
fn permission_requests_are_refused_from_the_list_of_plain_noes() {
    let cmd = json!([{"optionId": "allow_once", "kind": "allow_once"}, {"optionId": "allow_for_session", "kind": "allow_always"},
        {"optionId": "accept_execpolicy_amendment", "kind": "allow_always"}, {"optionId": "decline", "kind": "reject_once"}, {"optionId": "cancel", "kind": "reject_once"}]);
    assert_eq!(refusal(&cmd).as_deref(), Some("decline"));
    let file = json!([{"optionId": "allow_once", "kind": "allow_once"}, {"optionId": "cancel", "kind": "reject_once"}]);
    assert_eq!(refusal(&file).as_deref(), Some("cancel"));
    let perms = json!([{"optionId": "allow_permissions_turn", "kind": "allow_once"}, {"optionId": "reject_permissions", "kind": "reject_once"}]);
    assert_eq!(refusal(&perms).as_deref(), Some("reject_permissions"));
    // A "no" by a name it does not know, or one dressed as a yes, is never picked: "cancelled" instead.
    assert_eq!(refusal(&json!([{"optionId": "allow_once", "kind": "allow_once"}, {"optionId": "nope", "kind": "reject_once"}])), None);
    assert_eq!(refusal(&json!([{"optionId": "decline", "kind": "allow_once"}])), None);
    assert_eq!(crate::acp::permission_answer(None), json!({"outcome": {"outcome": "cancelled"}}));
}

fn evs(steps: Vec<Step>) -> Vec<Ev> { steps.into_iter().filter_map(|s| match s { Step::Ev(e) => Some(e), _ => None }).collect() }

#[test]
fn codex_updates_become_the_events_its_command_line_runs_report() {
    let mut v = View::new(Some("gpt-5".into()));
    let streamed = v.steps(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Looking."}}));
    assert!(matches!(&streamed[..], [Step::Stream(_)]), "text streams in");
    let call = v.steps(&json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "kind": "execute", "title": "ls", "status": "inProgress",
        "rawInput": {"command": ["bash", "-lc", "ls -la"], "cwd": "/w"}}));
    assert_eq!(evs(call), [Ev::Message("Looking.".into()), Ev::Command { id: "c1".into(), command: "ls -la".into() }]);
    let done = v.steps(&json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed", "rawOutput": {"exit_code": 0},
        "content": [{"type": "content", "content": {"type": "text", "text": "TODO.md"}}]}));
    assert_eq!(evs(done), [Ev::CommandDone { id: "c1".into(), command: None, output: "TODO.md".into(), exit: Some(0) }]);
    assert!(evs(v.steps(&json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed"}))).is_empty(), "a result is shown once");
    let edit = v.steps(&json!({"sessionUpdate": "tool_call", "toolCallId": "e1", "kind": "edit", "title": "Editing files", "status": "completed",
        "content": [{"type": "diff", "path": "/w/a.md", "oldText": "", "newText": "x"}]}));
    assert_eq!(evs(edit), [Ev::Change { id: "e1".into(), files: vec!["/w/a.md".into()] }, Ev::ChangeDone { id: "e1".into(), ok: true, output: String::new() }]);
    let web = v.steps(&json!({"sessionUpdate": "tool_call", "toolCallId": "w1", "title": "Web search: candles", "rawInput": {"type": "webSearch", "query": "candles"}}));
    assert_eq!(evs(web), [Ev::Search { id: "w1".into(), query: "candles".into() }]);
    let plan = v.steps(&json!({"sessionUpdate": "plan", "entries": [{"content": "Read", "status": "completed"}, {"content": "Write", "status": "pending"}]}));
    assert_eq!(evs(plan), [Ev::Plan("☑ Read\n☐ Write".into())]);
    // A failed command that says nothing of its exit may have been refused by the sandbox.
    v.steps(&json!({"sessionUpdate": "tool_call", "toolCallId": "c2", "kind": "execute", "title": "rm x", "rawInput": {"command": "rm x"}}));
    let failed = v.steps(&json!({"sessionUpdate": "tool_call_update", "toolCallId": "c2", "status": "failed"}));
    assert_eq!(evs(failed), [Ev::CommandDone { id: "c2".into(), command: None, output: String::new(), exit: None }]);
    assert_eq!(evs(v.steps(&json!({"sessionUpdate": "usage_update", "used": 900, "size": 272000}))), [Ev::Context(272000)]);
    v.steps(&json!({"sessionUpdate": "config_option_update", "configOptions": [{"id": "mode", "category": "mode", "currentValue": "agent"}]}));
    assert_eq!(v.mode.as_deref(), Some("agent"));
    v.steps(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Done."}}));
    let end = v.end_steps(Ok(&json!({"stopReason": "end_turn", "usage": {"totalTokens": 1500, "inputTokens": 1000, "cachedReadTokens": 200, "outputTokens": 300}})));
    assert_eq!(evs(end), [Ev::Message("Done.".into()), Ev::Usage(1500), Ev::Done(true)]);
    assert_eq!(evs(View::default().end_steps(Err("boom".into()))), [Ev::Error("boom".into()), Ev::Done(false)]);
    assert_eq!(evs(View::default().end_steps(Ok(&json!({"stopReason": "refusal"})))).last(), Some(&Ev::Done(false)));
    // What it used, counted as Codex's own runs count it (input with the cached part, which is told apart).
    let t = meter_of(&json!({"totalTokens": 1500, "inputTokens": 1000, "cachedReadTokens": 200, "outputTokens": 300})).tokens();
    assert_eq!((t.input, t.cache_read, t.output), (1000, 200, 300));
    assert!(meter_of(&Value::Null).tokens().is_empty());
}

// ---------- whole turns against a stand-in for codex-acp ----------

/// A stand-in codex-acp: logs every line it is sent, opens a session in `start_mode`, reports
/// `after_set` as its mode once a mode is set, and in its turn writes, runs a command, asks to leave
/// its sandbox for a second one (which then fails), sends `extra` (an update, as JSON), and ends.
#[cfg(unix)]
fn stand_in(dir: &Path, start_mode: &str, after_set: &str, extra: &str, init: &str) -> PathBuf {
    let script = dir.join("agent.sh");
    let log = dir.join("log.txt");
    let session = session_answer(start_mode).to_string();
    let up = |u: Value| json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": SID, "update": u}}).to_string();
    let turn = [
        up(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Looking."}})),
        up(json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "kind": "execute", "title": "ls", "status": "inProgress", "rawInput": {"command": "ls"}})),
        up(json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed", "rawOutput": {"exit_code": 0}, "content": [{"type": "content", "content": {"type": "text", "text": "TODO.md"}}]})),
        up(json!({"sessionUpdate": "tool_call", "toolCallId": "c2", "kind": "execute", "title": "curl x", "status": "pending", "rawInput": {"command": "curl https://x.example"}})),
    ].join("\n");
    let ask = json!({"jsonrpc": "2.0", "id": "perm-1", "method": "session/request_permission", "params": {"sessionId": SID,
        "toolCall": {"toolCallId": "c2", "kind": "execute", "title": "curl x", "rawInput": {"command": "curl https://x.example"}},
        "options": [{"optionId": "allow_once", "kind": "allow_once", "name": "Yes"}, {"optionId": "allow_for_session", "kind": "allow_always", "name": "Yes, always"},
                    {"optionId": "decline", "kind": "reject_once", "name": "No"}, {"optionId": "cancel", "kind": "reject_once", "name": "No, and tell"}]}}).to_string();
    let after = [
        up(json!({"sessionUpdate": "tool_call_update", "toolCallId": "c2", "status": "failed", "rawOutput": {"exit_code": 1}, "content": [{"type": "content", "content": {"type": "text", "text": "curl: Operation not permitted"}}]})),
        up(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Your week is planned."}})),
    ].join("\n");
    let set = up(json!({"sessionUpdate": "config_option_update", "configOptions": [{"id": "mode", "category": "mode", "currentValue": after_set}]}));
    let extra = if extra.is_empty() { String::new() } else { up(serde_json::from_str(extra).unwrap()) };
    let text = format!(r#"#!/bin/sh
while IFS= read -r line; do
  printf '%s\n' "$line" >> '{log}'
  id=$(printf '%s' "$line" | sed -n 's/^{{"id":\([0-9]*\),.*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":%s}}\n' "$id" '{init}' ;;
    *'"method":"session/new"'*|*'"method":"session/resume"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":%s}}\n' "$id" '{session}' ;;
    *'"method":"session/set_config_option"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{}}}}\n' "$id"
      case "$line" in *'"configId":"mode"'*) printf '%s\n' '{set}' ;; esac ;;
    *'"method":"session/prompt"'*)
      printf '%s\n' '{turn}'
      [ -n '{extra}' ] && printf '%s\n' '{extra}'
      printf '%s\n' '{ask}'
      IFS= read -r answer
      printf '%s\n' "$answer" >> '{log}'
      printf '%s\n' '{after}'
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn","usage":{{"totalTokens":1500,"inputTokens":1000,"cachedReadTokens":200,"outputTokens":300}}}}}}\n' "$id" ;;
  esac
done
"#, log = log.display());
    std::fs::write(&script, text).unwrap();
    script
}

#[cfg(unix)]
const TAKES_PICTURES: &str = r#"{"protocolVersion":1,"agentCapabilities":{"promptCapabilities":{"image":true}}}"#;

#[cfg(unix)]
fn sh(script: &Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new("/bin/sh");
    c.arg(script).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    crate::proc::own_group(&mut c);
    c
}

#[cfg(unix)]
fn after() -> After {
    After { kind: "chat".into(), record: None, touches_library: false, on_done: None, resumed: false, mark: "m".into(), chosen_model: Some("gpt-5-codex".into()),
        stopped: "Stopped.", snapshots: vec![], files: Default::default() }
}

#[cfg(unix)]
struct Turn { st: Arc<AppState>, id: String, dir: PathBuf, spec: crate::claude::RunSpec, profile: crate::claude::Profile }

#[cfg(unix)]
async fn turn_setup() -> Turn {
    let dir = std::env::temp_dir().join(format!("thoughts-codex-acp-run-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let st = Arc::new(AppState::for_test());
    let chat = crate::chats::Chat { provider: "codex".into(), ..Default::default() };
    let id = chat.id.clone();
    st.chats.insert(chat).await;
    let options = crate::chats::RunOptions { model: Some("gpt-5-codex".into()), ..Default::default() };
    let spec = crate::claude::RunSpec { chat_id: id.clone(), prompt: "Plan my week".into(), cwd: dir.join("work"), add_dirs: vec![], options,
        resume: None, append_system: None, extra_deny: vec![] };
    Turn { st, id, dir, spec, profile: crate::claude::profile("chat", "acceptEdits", false) }
}

#[cfg(unix)]
fn plain<'a>(t: &'a Turn, images: &'a [PathBuf]) -> Try<'a> {
    Try { spec: &t.spec, kind: "chat", profile: &t.profile, sandbox: "workspace-write", unrestricted: false, outer: false, full: false,
        writable: &[], effort: None, args: &[], service: None, images, prompt: "x" }
}

#[cfg(unix)]
async fn wait_done(st: &AppState, id: &str) {
    for _ in 0..1000 {
        if !st.chats.get(id).await.unwrap().running && !st.runs.lock().await.contains_key(id) { return; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the turn did not end");
}

#[cfg(unix)]
fn sent(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("log.txt")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[cfg(unix)]
#[tokio::test]
async fn a_codex_chat_turn_runs_over_acp_with_its_guards_and_shows_like_a_command_line_run() {
    let t = turn_setup().await;
    let picture = t.dir.join("1.png");
    std::fs::write(&picture, b"\x89PNG").unwrap();
    let script = stand_in(&t.dir, "agent", "workspace-write", "", TAKES_PICTURES);
    let args: Vec<OsString> = ["exec", "--sandbox", "workspace-write", "-c", "history.persistence=\"none\"", "-c", "notify=[]", "-"].iter().map(Into::into).collect();
    let writable = vec![t.dir.join("lib")];
    let tr = Try { spec: &t.spec, kind: "chat", profile: &t.profile, sandbox: "workspace-write", unrestricted: false, outer: false, full: false,
        writable: &writable, effort: Some("high"), args: &args, service: None, images: std::slice::from_ref(&picture),
        prompt: "<thoughts_app_instructions>notes</thoughts_app_instructions>\n\nPlan my week" };
    assert!(go(&t.st, tr, after(), sh(&script)).await.unwrap().is_none(), "it ran over ACP");
    wait_done(&t.st, &t.id).await;
    let lines = sent(&t.dir);
    let new = lines.iter().find(|l| l["method"] == "session/new").unwrap();
    assert_eq!(new["params"]["additionalDirectories"], json!(writable));
    // It opened in the adapter's default ("agent": a model approves) and was put in the chat's sandbox.
    let set: Vec<(&str, &str)> = lines.iter().filter(|l| l["method"] == "session/set_config_option")
        .map(|l| (l["params"]["configId"].as_str().unwrap(), l["params"]["value"].as_str().unwrap())).collect();
    assert_eq!(set, [("mode", "workspace-write"), ("model", "gpt-5-codex"), ("reasoning_effort", "high")]);
    // The notes travel in front of the prompt, as on stdin for `codex exec`; the picture as a picture.
    let prompt = &lines.iter().find(|l| l["method"] == "session/prompt").unwrap()["params"]["prompt"];
    let text = prompt[0]["text"].as_str().unwrap();
    assert!(text.starts_with("<thoughts_app_instructions>") && text.ends_with("Plan my week"), "{text}");
    assert_eq!((prompt[1]["type"].as_str(), prompt[1]["mimeType"].as_str()), (Some("image"), Some("image/png")));
    // Asked to leave its sandbox: refused with its plain no, never a yes.
    let answer = lines.iter().find(|l| l["id"] == "perm-1").unwrap();
    assert_eq!(answer["result"]["outcome"]["optionId"], "decline");
    let c = t.st.chats.get(&t.id).await.unwrap();
    let said: Vec<(&str, &str)> = c.messages.iter().map(|m| (m.role.as_str(), m.text.as_str())).collect();
    assert!(said.contains(&("assistant", "Looking.")) && said.contains(&("assistant", "Your week is planned.")), "{said:?}");
    assert!(said.contains(&("tool", "Command: ls")) && said.contains(&("tool_result", "TODO.md")), "{said:?}");
    let denied = c.messages.iter().find(|m| m.role == "denied").expect("what was refused is listed under the answer");
    let list = denied.tool.as_ref().unwrap()["denials"].as_array().unwrap().clone();
    assert!(list.iter().all(|d| d["why"] == "codexSandbox" && d["what"] == "command"), "{list:?}");
    assert!(!c.messages.iter().any(|m| m.role == "error"), "{said:?}");
    assert_eq!((c.session_id.as_deref(), c.usage_tokens, c.provider.as_str(), c.running), (Some(SID), 1500, "codex", false));
    std::fs::remove_dir_all(&t.dir).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_switch_mid_turn_to_a_mode_that_does_more_ends_the_turn() {
    let t = turn_setup().await;
    let script = stand_in(&t.dir, "workspace-write", "workspace-write", r#"{"sessionUpdate":"current_mode_update","currentModeId":"agent-full-access"}"#, TAKES_PICTURES);
    assert!(go(&t.st, plain(&t, &[]), after(), sh(&script)).await.unwrap().is_none());
    wait_done(&t.st, &t.id).await;
    let c = t.st.chats.get(&t.id).await.unwrap();
    assert_eq!(c.messages.last().map(|m| m.text.clone()), Some(crate::tr::text("run.lost", &[])));
    assert!(!sent(&t.dir).iter().any(|l| l["id"] == "perm-1"), "nothing more was answered after it");
    std::fs::remove_dir_all(&t.dir).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn an_adapter_that_cannot_open_the_chat_sends_it_back_to_the_command_line_before_any_turn() {
    let t = turn_setup().await;
    // It does not offer the mode the chat needs.
    let script = stand_in(&t.dir, "agent", "agent", "", TAKES_PICTURES);
    let offered = session_answer("agent").to_string();
    std::fs::write(&script, std::fs::read_to_string(&script).unwrap().replace(&offered, &offered.replace("\"workspace-write\"", "\"something-else\""))).unwrap();
    assert!(go(&t.st, plain(&t, &[]), after(), sh(&script)).await.unwrap().is_some(), "back to codex exec");
    assert!(!sent(&t.dir).iter().any(|l| l["method"] == "session/prompt"), "no turn was sent");
    // Pictures it does not take, or no adapter at all: the same.
    let picture = t.dir.join("1.png");
    std::fs::write(&picture, b"x").unwrap();
    let script = stand_in(&t.dir, "workspace-write", "workspace-write", "", r#"{"protocolVersion":1,"agentCapabilities":{}}"#);
    assert!(go(&t.st, plain(&t, std::slice::from_ref(&picture)), after(), sh(&script)).await.unwrap().is_some());
    assert!(go(&t.st, plain(&t, &[]), after(), sh(&t.dir.join("missing.sh"))).await.unwrap().is_some());
    let c = t.st.chats.get(&t.id).await.unwrap();
    assert!(!c.running && c.messages.is_empty() && !t.st.runs.lock().await.contains_key(&t.id), "the chat was not touched");
    std::fs::remove_dir_all(&t.dir).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn stop_cancels_the_turn() {
    let t = turn_setup().await;
    // A turn that never gets past its permission request by itself: Stop must end it.
    let script = stand_in(&t.dir, "workspace-write", "workspace-write", "", TAKES_PICTURES);
    let text = std::fs::read_to_string(&script).unwrap().replace("      IFS= read -r answer\n", "      IFS= read -r answer\n      sleep 30\n");
    std::fs::write(&script, text).unwrap();
    assert!(go(&t.st, plain(&t, &[]), after(), sh(&script)).await.unwrap().is_none());
    for _ in 0..300 { if t.st.chats.get(&t.id).await.unwrap().messages.iter().any(|m| m.role == "tool") { break; } tokio::time::sleep(Duration::from_millis(10)).await; }
    let stop = t.st.runs.lock().await.remove(&t.id).expect("the turn holds its Stop switch");
    let _ = stop.send(());
    wait_done(&t.st, &t.id).await;
    let c = t.st.chats.get(&t.id).await.unwrap();
    assert_eq!(c.messages.last().map(|m| m.text.as_str()), Some("Stopped."));
    std::fs::remove_dir_all(&t.dir).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn the_adapter_starts_with_the_runs_settings_and_in_its_mode() {
    let t = turn_setup().await;
    let args: Vec<OsString> = ["exec", "-c", "notify=[]", "-c", "mcp_servers={}", "-"].iter().map(Into::into).collect();
    let svc = crate::providers::CodexService { base_url: "https://llm.example/v1".into(), key: "sk-secret-456".into() };
    let tr = Try { args: &args, service: Some(&svc), sandbox: "read-only", ..plain(&t, &[]) };
    let env = adapter_env(&tr, mode_for(tr.sandbox, false));
    let config: Value = serde_json::from_str(&env.iter().find(|(k, _)| *k == "CODEX_CONFIG").unwrap().1).unwrap();
    assert_eq!((config["notify"].clone(), config["mcp_servers"].clone()), (json!([]), json!({})));
    assert!(env.iter().all(|(_, v)| !v.contains("sk-secret-456")), "the key is not among them");
    assert_eq!(env.iter().find(|(k, _)| *k == "INITIAL_AGENT_MODE").map(|(_, v)| v.as_str()), Some("read-only"));
    std::fs::remove_dir_all(&t.dir).unwrap();
}
