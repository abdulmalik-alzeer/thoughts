//! Tests for `approvals.rs`: who may ask, the request/answer protocol over the MCP endpoint, the
//! timeout, strict private folders, and that bypass and the app's own runs are untouched.

use super::*;
use crate::chats::{Chat, RunOptions};
use crate::claude::{profile, start_run, RunInfo, RunSpec};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The flag lines of the installed Claude Code (2.1.295) that matter here, as `--help` prints them.
const HELP: &str = "Options:\n\
  --mcp-config <configs...>             Load MCP servers from JSON files or\n                                        strings (space-separated)\n\
  --permission-mode <mode>              Permission mode to use for the session\n\
  --permission-prompts <target>         Who answers permission prompts with\n                                        --print: \"host\" (the SDK host or\n                                        --permission-prompt-tool) or \"none\"\n                                        (choices: \"host\", \"none\", default:\n                                        \"host\")\n\
  --strict-mcp-config                   Only use MCP servers from --mcp-config\n";
/// A Claude Code from before `--permission-prompts` (it cannot be trusted to wait for the tool's server).
const OLD_HELP: &str = "Options:\n  --mcp-config <configs...>  Load MCP servers\n  --permission-mode <mode>  (choices: \"acceptEdits\", \"plan\")\n";

fn ctx(chat: &str, mode: &str) -> RunCtx {
    RunCtx { chat_id: chat.into(), mode: mode.into(), cwd: PathBuf::from("/home/u/Library"), library: Some(PathBuf::from("/home/u/Library")), ..Default::default() }
}

fn req(tool: &str, input: Value, id: &str) -> Request { Request { tool: tool.into(), input, tool_use_id: Some(id.into()) } }

async fn chat(st: &AppState, kind: &str) -> String {
    let c = Chat { kind: kind.into(), ..Default::default() };
    st.chats.insert(c.clone()).await;
    c.id
}

/// The approval cards of a chat, oldest first.
async fn cards(st: &AppState, id: &str) -> Vec<Msg> {
    st.chats.get(id).await.unwrap().messages.into_iter().filter(|m| m.role == "approval").collect()
}

/// Wait until chat `id` has `n` cards, and return the last.
async fn nth_card(st: &AppState, id: &str, n: usize) -> Msg {
    for _ in 0..400 {
        let c = cards(st, id).await;
        if c.len() >= n { return c[n - 1].clone(); }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("no card {n} in chat {id}");
}

fn status(m: &Msg) -> String { m.tool.as_ref().unwrap()["status"].as_str().unwrap_or("").to_string() }

/// Wait until card `msg_id` of chat `id` says `want`.
async fn settled(st: &AppState, id: &str, msg_id: &str, want: &str) {
    for _ in 0..400 {
        if cards(st, id).await.iter().any(|m| m.id == msg_id && status(m) == want) { return; }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("card {msg_id} never said {want}: {:?}", cards(st, id).await.iter().map(status).collect::<Vec<_>>());
}

#[test]
fn only_ordinary_chats_ask_and_only_with_a_program_that_can() {
    let w = |kind: &str, mode: &str, web: bool| wanted(kind, mode, &profile(kind, mode, web));
    assert!(w("chat", "acceptEdits", false) && w("chat", "plan", false));
    // Bypass: nothing asks there. Web lookups: a page could make the assistant ask for anything.
    assert!(!w("chat", crate::settings::BYPASS, false) && !w("chat", "acceptEdits", true));
    // Nothing the app starts by itself asks — nobody is watching it. A customise chat keeps its fixed rules.
    for kind in ["wrap", "workflow", "module", "setup", "reorganize", "customize", ""] { assert!(!w(kind, "acceptEdits", false), "{kind}"); }
    // The program must have `--permission-prompts host` (which names the tool) and `--mcp-config`.
    assert!(supported(HELP));
    assert!(!supported(OLD_HELP), "an older program keeps refusing what would ask");
    assert!(!supported(&HELP.replace("--mcp-config <configs...>", "--other <x>")));
}

#[test]
fn a_run_that_asks_is_started_with_the_prompt_tool_in_place_of_none() {
    let base: Vec<String> = ["--permission-prompts", "none", "--strict-mcp-config"].iter().map(|s| s.to_string()).collect();
    let a: Vec<String> = flags(base, HELP, Path::new("/t/mcp.json")).iter().map(|x| x.to_string_lossy().to_string()).collect();
    assert_eq!(a, ["--strict-mcp-config", "--permission-prompts", "host", "--mcp-config", "/t/mcp.json", "--permission-prompt-tool", "mcp__thoughtsapp__approve"]);
    // `--mcp-config` takes a list: what follows it is always another flag.
    let at = a.iter().position(|x| x == "--mcp-config").unwrap();
    assert!(a[at + 2].starts_with("--"));
    assert_eq!(prompt_tool(), "mcp__thoughtsapp__approve");
    let c: Value = serde_json::from_str(&config_json(4321, "tok")).unwrap();
    let s = &c["mcpServers"]["thoughtsapp"];
    assert_eq!((s["type"].as_str(), s["url"].as_str(), s["headers"]["Authorization"].as_str()), (Some("http"), Some("http://127.0.0.1:4321/mcp/approvals"), Some("Bearer tok")));
    // Claude Code gives an HTTP server 60 seconds a request unless told otherwise: longer than the wait.
    assert!(s["timeout"].as_u64().unwrap() > WAIT.as_millis() as u64);
}

#[test]
fn a_request_is_said_in_plain_words_with_why_the_mode_did_not_let_it_through() {
    let c = ctx("c", "acceptEdits");
    let k = card(&req("Bash", json!({"command": "npm install", "description": "Install"}), "t"), &c);
    assert_eq!((k.what.as_str(), k.target.as_str(), k.why.as_str(), k.always.as_deref()), ("command", "npm install", "command", Some("command")));
    assert_eq!(card(&req("Bash", json!({"command": "rm x"}), "t"), &ctx("c", "plan")).why, "planCommand");
    let k = card(&req("Write", json!({"file_path": "/home/u/Elsewhere/a.md", "content": "x".repeat(9000)}), "t"), &c);
    assert_eq!((k.what.as_str(), k.target.as_str(), k.why.as_str(), k.always.as_deref()), ("change", "/home/u/Elsewhere/a.md", "outside", Some("file")));
    assert!(k.detail["content"].as_str().unwrap().chars().count() <= DETAIL_CHARS + 1, "long values are cut for the card");
    let k = card(&req("Edit", json!({"file_path": "/home/u/Library/ideas/x/README.md"}), "t"), &ctx("c", "plan"));
    assert_eq!((k.target.as_str(), k.why.as_str()), ("ideas/x/README.md", "plan"));
    let k = card(&req("mcp__mail__send_email", json!({"to": "a@b.c"}), "t"), &c);
    assert_eq!((k.what.as_str(), k.target.as_str(), k.why.as_str(), k.always.as_deref()), ("connected", "send_email (mail)", "connected", Some("tool")));
    // Strict private folders: a command carries the warning and the folders, and is offered once only.
    let strict = RunCtx { strict: true, private: vec!["me/finances".into()], ..c.clone() };
    let k = card(&req("Bash", json!({"command": "grep -r invoice ."}), "t"), &strict);
    assert_eq!((k.why.as_str(), k.strict, k.private.clone(), k.always.clone()), ("strict", true, vec!["me/finances".to_string()], None));
    // …while a file change is worded as ever (the deny rules keep the private folders closed to it).
    assert!(!card(&req("Edit", json!({"file_path": "/home/u/Elsewhere/a.md"}), "t"), &strict).strict);
}

#[test]
fn characters_that_do_not_show_are_written_out_and_flagged_and_nothing_is_cut() {
    let c = ctx("c", "acceptEdits");
    // Shown as `ls # cleanup.txt`, runs `rm -rf ~ ; …`: a right-to-left override, zero-width spaces, a tag character.
    let sneaky = "ls \u{202E}txt.punaelc # \u{202C}\u{200B}\u{200B}; rm -rf ~\u{E0041}";
    let k = card(&req("Bash", json!({"command": sneaky}), "t"), &c);
    assert!(k.hidden);
    assert_eq!(k.target, "ls [U+202E]txt.punaelc # [U+202C][U+200B][U+200B]; rm -rf ~[U+E0041]");
    assert!(!k.target.chars().any(hidden_char) && !k.detail.to_string().chars().any(hidden_char), "nothing invisible reaches the card");
    assert_eq!(k.detail["command"], k.target);
    assert_eq!(k.always, None, "only what can be seen in full can be allowed for good");
    // Every direction mark counts here, also one alone (a command is not Arabic prose), and so do control characters.
    for x in ["a\u{200F}b", "a\u{200E}b", "a\u{061C}b", "a\u{2066}b\u{2069}", "a\u{FEFF}", "a\u{1B}[2Kb", "a\u{FE0F}"] {
        assert!(card(&req("Bash", json!({"command": x}), "t"), &c).hidden, "{x:?}");
    }
    // In a file's name, and in the tool's own name, too.
    assert!(card(&req("Write", json!({"file_path": "/home/u/Elsewhere/re\u{202E}fdp.exe"}), "t"), &c).hidden);
    assert!(card(&req("mcp__mail\u{200B}__send", json!({}), "t"), &c).hidden);
    // Ordinary text, Arabic included, line breaks and tabs: nothing to say.
    let plain = card(&req("Bash", json!({"command": "mv \"ملاحظات قديمة.md\" archive/\n\tls"}), "t"), &c);
    assert!(!plain.hidden && plain.target.contains("ملاحظات") && plain.always.is_some());
    // A run of them is shortened to a count; nothing else is.
    assert_eq!(reveal(&format!("a{}b", "\u{200B}".repeat(20))), format!("a{}[+12]b", "[U+200B]".repeat(8)));
    // The whole command is on the card however long it is.
    let long = format!("echo {}", "word ".repeat(2000));
    assert_eq!(card(&req("Bash", json!({"command": long.clone()}), "t"), &c).target, long);
}

#[test]
fn some_requests_are_answered_without_asking() {
    let c = ctx("c", "acceptEdits");
    let deny = |r: &Request| matches!(decide_now(r, &c, false, &[]), Some(Verdict::Deny(_)));
    // The app's own tool, questions and plans the app does not show this way, and the web (off in this chat).
    for t in ["mcp__thoughtsapp__approve", "AskUserQuestion", "ExitPlanMode", "WebFetch", "WebSearch", ""] { assert!(deny(&req(t, json!({}), "t")), "{t}"); }
    // Asked about before: once only.
    assert!(matches!(decide_now(&req("Bash", json!({"command": "ls"}), "t"), &c, true, &[]), Some(Verdict::Deny(_))));
    // Everything else is the user's to answer…
    assert_eq!(decide_now(&req("Bash", json!({"command": "npm test"}), "t"), &c, false, &[]), None);
    // …unless they allowed exactly this for the chat.
    let always = |kind: &str, tool: &str, key: &str| Always { id: "a".into(), kind: kind.into(), tool: tool.into(), key: key.into(), what: String::new(), target: String::new(), at: Utc::now() };
    let rules = [always("command", "Bash", "npm test"), always("file", "Edit", "/home/u/Elsewhere/a.md"), always("tool", "mcp__mail__send", "mcp__mail__send")];
    assert_eq!(decide_now(&req("Bash", json!({"command": "npm test"}), "t"), &c, false, &rules), Some(Verdict::Allow));
    assert_eq!(decide_now(&req("Bash", json!({"command": "npm test && rm -rf ~"}), "t"), &c, false, &rules), None, "exactly that command, nothing more");
    assert_eq!(decide_now(&req("PowerShell", json!({"command": "npm test"}), "t"), &c, false, &rules), None, "Bash is not PowerShell");
    assert_eq!(decide_now(&req("Write", json!({"file_path": "/home/u/Elsewhere/a.md"}), "t"), &c, false, &rules), Some(Verdict::Allow), "changes to that file, by any edit tool");
    assert_eq!(decide_now(&req("mcp__mail__send", json!({"to": "x"}), "t"), &c, false, &rules), Some(Verdict::Allow));
    // Under strict private folders a command is looked at every time, whatever was allowed before.
    assert_eq!(decide_now(&req("Bash", json!({"command": "npm test"}), "t"), &RunCtx { strict: true, ..c.clone() }, false, &rules), None);
}

#[cfg(unix)]
#[test]
fn a_file_in_a_private_folder_is_refused_without_asking_by_its_real_path() {
    let base = std::env::temp_dir().join(format!("thoughts-appr-priv-{}", uuid::Uuid::new_v4()));
    let lib = base.join("Library");
    std::fs::create_dir_all(lib.join("me/finances")).unwrap();
    std::fs::write(lib.join("me/finances/notes.md"), "x").unwrap();
    std::fs::create_dir_all(base.join("elsewhere")).unwrap();
    // A link outside the library that leads into the private folder.
    std::os::unix::fs::symlink(lib.join("me/finances"), base.join("elsewhere/link")).unwrap();
    let c = RunCtx { chat_id: "c".into(), mode: "acceptEdits".into(), cwd: base.join("project"), library: Some(lib.clone()), private: vec!["me/finances".into()], ..Default::default() };
    let refused = |path: &Path, tool: &str| matches!(decide_now(&req(tool, json!({"file_path": path.to_string_lossy(), "path": path.to_string_lossy()}), "t"), &c, false, &[]), Some(Verdict::Deny(m)) if m.contains("private"));
    assert!(refused(&base.join("elsewhere/link/notes.md"), "Read"), "through a link");
    assert!(refused(&base.join("elsewhere/link"), "Grep"));
    assert!(refused(&lib.join("Me/Finances/notes.md"), "Edit"), "in another letter case");
    assert!(!refused(&lib.join("me/other.md"), "Edit"));
    std::fs::remove_dir_all(base).unwrap();
}

#[tokio::test]
async fn the_user_answers_once_always_or_no_and_the_run_gets_that() {
    let st = Arc::new(AppState::for_test());
    let id = chat(&st, "chat").await;
    let ticket = open(&st, ctx(&id, "acceptEdits"));
    let token = ticket.token().to_string();
    assert!(st.approvals.asking(&id));
    let asked = |input: Value, tid: &str| { let (st, token, r) = (st.clone(), token.clone(), req("Bash", input, tid)); tokio::spawn(async move { ask(&st, &token, r, WAIT).await }) };

    // Allow once.
    let waiting = asked(json!({"command": "npm install"}), "t1");
    let c1 = nth_card(&st, &id, 1).await;
    assert_eq!(status(&c1), "waiting");
    assert_eq!(c1.tool.as_ref().unwrap()["card"]["target"], "npm install");
    assert!(c1.tool.as_ref().unwrap()["expiresAt"].is_string());
    assert_eq!(answer(&st, &id, &c1.id, Answer::Once).await.unwrap(), "once");
    assert_eq!(waiting.await.unwrap(), Verdict::Allow);
    assert_eq!(status(&nth_card(&st, &id, 1).await), "once");
    // An answer counts once: the same card cannot be answered again, nor from another chat.
    assert_eq!(crate::tr::classify_in(false, &answer(&st, &id, &c1.id, Answer::Deny).await.unwrap_err()).code, "approval_gone");

    // Deny.
    let waiting = asked(json!({"command": "rm -rf build"}), "t2");
    let c2 = nth_card(&st, &id, 2).await;
    let other = chat(&st, "chat").await;
    assert!(answer(&st, &other, &c2.id, Answer::Once).await.is_err(), "a card is answered in its own chat only");
    answer(&st, &id, &c2.id, Answer::Deny).await.unwrap();
    assert!(matches!(waiting.await.unwrap(), Verdict::Deny(m) if m.contains("refused")));
    assert_eq!(status(&nth_card(&st, &id, 2).await), "denied");
    // What the user refused on a card is not listed again under the answer.
    assert!(st.approvals.carded(&id).contains("t2"));

    // Always for this chat: allowed now, and the same command later without a card.
    let waiting = asked(json!({"command": "npm test"}), "t3");
    let c3 = nth_card(&st, &id, 3).await;
    answer(&st, &id, &c3.id, Answer::Always).await.unwrap();
    assert_eq!(waiting.await.unwrap(), Verdict::Allow);
    assert_eq!(st.approvals.always(&id).iter().map(|a| a.key.as_str()).collect::<Vec<_>>(), ["npm test"]);
    assert_eq!(asked(json!({"command": "npm test"}), "t4").await.unwrap(), Verdict::Allow);
    assert_eq!(cards(&st, &id).await.len(), 3, "no card for what is always allowed");
    // A request with characters that do not show is never allowed for good, whatever is pressed.
    let waiting = asked(json!({"command": "npm test\u{202E}"}), "t3b");
    let c3b = nth_card(&st, &id, 4).await;
    assert_eq!(c3b.tool.as_ref().unwrap()["card"]["hidden"], true);
    assert_eq!(answer(&st, &id, &c3b.id, Answer::Always).await.unwrap(), "once");
    assert_eq!(waiting.await.unwrap(), Verdict::Allow);
    assert_eq!(st.approvals.always(&id).len(), 1);
    // A tool use is asked about once: the same id again is refused, without a card.
    assert!(matches!(asked(json!({"command": "npm install"}), "t1").await.unwrap(), Verdict::Deny(_)));
    // It is remembered across restarts of the service, and can be taken back.
    let again = Approvals::at(st.approvals.file.clone());
    let rule = again.always(&id)[0].clone();
    assert_eq!(rule.kind, "command");
    assert!(again.forget(&id, &rule.id).is_empty());
    assert!(Approvals::at(st.approvals.file.clone()).always(&id).is_empty());
    // Deleting the chat forgets what was allowed in it.
    st.approvals.forget_chat(&id);
    assert!(st.approvals.always(&id).is_empty());

    // The run ends while a request waits: refused, and the card says so.
    let waiting = asked(json!({"command": "make"}), "t5");
    let c5 = nth_card(&st, &id, 5).await;
    drop(ticket);
    assert!(matches!(waiting.await.unwrap(), Verdict::Deny(_)));
    settled(&st, &id, &c5.id, "ended").await;
    assert!(!st.approvals.asking(&id));
    // A run that is over asks nothing at all.
    assert!(matches!(ask(&st, &token, req("Bash", json!({"command": "ls"}), "t6"), WAIT).await, Verdict::Deny(_)));
}

#[tokio::test]
async fn an_unanswered_request_is_refused_when_its_time_runs_out() {
    let st = Arc::new(AppState::for_test());
    let id = chat(&st, "chat").await;
    let ticket = open(&st, ctx(&id, "acceptEdits"));
    let v = ask(&st, ticket.token(), req("Bash", json!({"command": "npm publish"}), "t1"), Duration::from_millis(60)).await;
    assert!(matches!(&v, Verdict::Deny(m) if m.contains("Nobody answered")), "{v:?}");
    let c = nth_card(&st, &id, 1).await;
    assert_eq!(status(&c), "timeout");
    // Too late to answer now.
    assert!(answer(&st, &id, &c.id, Answer::Once).await.is_err());
    // A program that gives up on its call (stopped, gone) leaves a card that says so.
    let (st2, token) = (st.clone(), ticket.token().to_string());
    let call = tokio::spawn(async move { ask(&st2, &token, req("Bash", json!({"command": "make"}), "t2"), WAIT).await });
    let c2 = nth_card(&st, &id, 2).await;
    call.abort();
    settled(&st, &id, &c2.id, "ended").await;
}

// ---------- over HTTP, as Claude Code calls it ----------

async fn serve(st: &Arc<AppState>) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = mcp_routes().with_state(st.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

/// Send one request; the stream is returned so a slow answer can be read later.
async fn send(port: u16, method: &str, headers: &[(&str, &str)], body: &str) -> tokio::net::TcpStream {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let extra: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
    let head = format!("{method} {MCP_PATH} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    s.write_all(head.as_bytes()).await.unwrap();
    s.write_all(body.as_bytes()).await.unwrap();
    s
}

async fn read_all(mut s: tokio::net::TcpStream) -> (u16, String) {
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(10), s.read_to_end(&mut out)).await;
    let out = String::from_utf8_lossy(&out).to_string();
    (out.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0), out)
}

/// The JSON-RPC message in a response: the plain body, or the `data:` line of an event stream.
fn rpc(raw: &str) -> Value {
    let body = raw.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    if let Some(line) = body.lines().find_map(|l| l.strip_prefix("data: ").or_else(|| l.strip_prefix("data:"))) { return serde_json::from_str(line.trim()).unwrap(); }
    serde_json::from_str(body.trim()).unwrap_or(Value::Null)
}

#[tokio::test]
async fn the_endpoint_speaks_mcp_to_the_run_that_holds_its_token_and_nobody_else() {
    let st = Arc::new(AppState::for_test());
    let id = chat(&st, "chat").await;
    let ticket = open(&st, ctx(&id, "acceptEdits"));
    let port = serve(&st).await;
    let auth = format!("Bearer {}", ticket.token());
    let both = [("Authorization", auth.as_str()), ("Accept", "application/json, text/event-stream")];
    let call = |method: &str, n: u32, params: Value| json!({"jsonrpc": "2.0", "id": n, "method": method, "params": params}).to_string();

    // No token, a wrong one, or a web page: nothing.
    assert_eq!(read_all(send(port, "POST", &[], &call("ping", 1, json!({}))).await).await.0, 401);
    assert_eq!(read_all(send(port, "POST", &[("Authorization", "Bearer nope")], &call("ping", 1, json!({}))).await).await.0, 401);
    assert_eq!(read_all(send(port, "POST", &[both[0], ("Origin", "https://example.com")], &call("ping", 1, json!({}))).await).await.0, 403);
    // The handshake.
    let (code, raw) = read_all(send(port, "POST", &both, &call("initialize", 1, json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "claude-code"}}))).await).await;
    assert_eq!(code, 200);
    let init = rpc(&raw);
    assert_eq!((init["id"].as_u64(), init["result"]["protocolVersion"].as_str()), (Some(1), Some("2025-06-18")));
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert_eq!(read_all(send(port, "POST", &both, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string()).await).await.0, 202);
    // No stream of its own to open, no session to end.
    assert_eq!(read_all(send(port, "GET", &both, "").await).await.0, 405);
    let tools = rpc(&read_all(send(port, "POST", &both, &call("tools/list", 2, json!({}))).await).await.1);
    assert_eq!(tools["result"]["tools"][0]["name"], "approve");
    assert!(tools["result"]["tools"][0].get("outputSchema").is_none(), "no structured output: Claude Code wants one text block");
    assert_eq!(rpc(&read_all(send(port, "POST", &both, &call("resources/list", 3, json!({}))).await).await.1)["error"]["code"], -32601);

    // A permission request: the answer streams back once the user has given it.
    let s = send(port, "POST", &both, &call("tools/call", 4, json!({"name": "approve", "arguments": {"tool_name": "Bash", "input": {"command": "npm install"}, "tool_use_id": "toolu_1"}}))).await;
    let card = nth_card(&st, &id, 1).await;
    answer(&st, &id, &card.id, Answer::Once).await.unwrap();
    let (code, raw) = read_all(s).await;
    assert_eq!(code, 200);
    assert!(raw.to_lowercase().contains("content-type: text/event-stream"), "{raw}");
    let out = rpc(&raw);
    assert_eq!(out["id"], 4);
    let blocks = out["result"]["content"].as_array().unwrap();
    assert_eq!((blocks.len(), blocks[0]["type"].as_str()), (1, Some("text")));
    assert_eq!(serde_json::from_str::<Value>(blocks[0]["text"].as_str().unwrap()).unwrap(), json!({"behavior": "allow"}));
    assert!(out["result"].get("structuredContent").is_none());

    // Refused, over a client that wants plain JSON.
    let s = send(port, "POST", &[both[0], ("Accept", "application/json")], &call("tools/call", 5, json!({"name": "approve", "arguments": {"tool_name": "Write", "input": {"file_path": "/etc/hosts", "content": "x"}, "tool_use_id": "toolu_2"}}))).await;
    let card = nth_card(&st, &id, 2).await;
    answer(&st, &id, &card.id, Answer::Deny).await.unwrap();
    let out = rpc(&read_all(s).await.1);
    let said: Value = serde_json::from_str(out["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(said["behavior"], "deny");
    assert!(said["message"].as_str().unwrap().contains("refused"));

    // Once the run is over its token opens nothing.
    drop(ticket);
    assert_eq!(read_all(send(port, "POST", &[("Authorization", auth.as_str())], &call("ping", 6, json!({}))).await).await.0, 401);
}

// ---------- what runs are started with ----------

/// Start a run of `kind` with the stand-in program and return what it was given.
async fn run_info(st: &Arc<AppState>, seen: &Arc<std::sync::Mutex<Vec<RunInfo>>>, cwd: &Path, kind: &str, mode: &str, web: bool) -> RunInfo {
    let id = chat(st, kind).await;
    let n = seen.lock().unwrap().len();
    start_run(st.clone(), RunSpec { chat_id: id, prompt: "hi".into(), cwd: cwd.to_path_buf(), add_dirs: vec![],
        options: RunOptions { permission_mode: Some(mode.into()), web, ..Default::default() }, resume: None, append_system: None, extra_deny: vec![] }, None).await.unwrap();
    for _ in 0..400 { if seen.lock().unwrap().len() > n { break; } tokio::time::sleep(Duration::from_millis(5)).await; }
    seen.lock().unwrap().last().cloned().unwrap()
}

#[tokio::test]
async fn only_ordinary_chats_get_the_prompt_tool_strict_mode_closes_commands_and_bypass_is_untouched() {
    let st = Arc::new(AppState::for_test());
    let seen = Arc::new(std::sync::Mutex::new(Vec::<RunInfo>::new()));
    let log = seen.clone();
    *st.run_hook.lock().unwrap() = Some(Arc::new(move |info: RunInfo| { log.lock().unwrap().push(info); Box::pin(async { true }) }));
    let base = std::env::temp_dir().join(format!("thoughts-appr-runs-{}", uuid::Uuid::new_v4()));
    let lib = base.join("Library");
    std::fs::create_dir_all(lib.join("clients/acme")).unwrap();
    { let mut s = st.settings.write().await; s.library_path = Some(lib.to_string_lossy().into()); }
    let has = |v: &[String], r: &str| v.iter().any(|x| x == r);
    let bypass = crate::settings::BYPASS;

    // Without strict mode: an ordinary chat asks, and nothing about its rules changes.
    let chat = run_info(&st, &seen, &lib, "chat", "acceptEdits", false).await;
    assert!(chat.prompt_token.is_some() && chat.ask.is_empty() && !has(&chat.deny, "Bash"));
    assert!(chat.system.contains("Permission requests") && chat.system.contains("never call it yourself"));
    let plan = run_info(&st, &seen, &lib, "chat", "plan", false).await;
    assert!(plan.prompt_token.is_some());
    // The app's own runs, a customise chat, a web chat and a bypass chat never ask.
    for (kind, mode, web) in [("wrap", "acceptEdits", false), ("workflow", "acceptEdits", false), ("module", "acceptEdits", false), ("setup", "acceptEdits", false),
                              ("reorganize", "acceptEdits", false), ("customize", "acceptEdits", false), ("chat", "acceptEdits", true), ("chat", bypass, false)] {
        let r = run_info(&st, &seen, &lib, kind, mode, web).await;
        assert!(r.prompt_token.is_none() && !r.system.contains("Permission requests"), "{kind} {mode} web={web}");
    }
    let bypass_before = run_info(&st, &seen, &lib, "chat", bypass, false).await;

    // Strict private folders on, but no private folder yet: nothing changes.
    st.settings.write().await.strict_private = Some(true);
    let r = run_info(&st, &seen, &lib, "chat", "acceptEdits", false).await;
    assert!(r.ask.is_empty() && !has(&r.deny, "Bash") && !r.profile.strict);

    // With a private folder: every command of an ordinary chat is put to the user…
    st.settings.write().await.private_paths = vec!["clients/acme".into()];
    let r = run_info(&st, &seen, &lib, "chat", "acceptEdits", false).await;
    assert_eq!(r.ask, ["Bash", "PowerShell"]);
    assert!(!has(&r.deny, "Bash") && r.prompt_token.is_some() && r.profile.strict && r.profile.shell);
    assert!(r.system.contains("Strict private folders is on: every command needs the user's approval"));
    // …and closed wherever nobody can be asked: a customise chat, a web chat.
    for (kind, web) in [("customize", false), ("chat", true)] {
        let r = run_info(&st, &seen, &lib, kind, "acceptEdits", web).await;
        assert!(has(&r.deny, "Bash") && has(&r.deny, "PowerShell") && r.ask.is_empty() && !r.profile.shell, "{kind} web={web}");
    }
    let r = run_info(&st, &seen, &lib, "customize", "acceptEdits", false).await;
    assert!(r.system.contains("commands are switched off in this chat"));
    // The app's own runs are as they were (Wrap and automatic tasks have no commands anyway; set-up
    // and Reorganize keep theirs).
    for (kind, shell) in [("wrap", false), ("workflow", false), ("module", false), ("setup", true), ("reorganize", true)] {
        let r = run_info(&st, &seen, &lib, kind, "acceptEdits", false).await;
        assert_eq!((has(&r.deny, "Bash"), r.ask.is_empty(), r.profile.strict), (!shell, true, false), "{kind}");
    }
    // Bypass: exactly as before strict mode and the approvals existed — no ask rule, no prompt tool, the same rules.
    let r = run_info(&st, &seen, &lib, "chat", bypass, false).await;
    assert!(r.prompt_token.is_none() && r.ask.is_empty() && !r.profile.strict && !has(&r.deny, "Bash"));
    // (Other tests add and remove files in the test home they share meanwhile: rules naming it are left out too.)
    let home = crate::paths::home().file_name().unwrap().to_string_lossy().to_string();
    let rules_of = |i: &RunInfo| i.deny.iter().filter(|d| !d.contains("clients/") && !d.contains("[Cc][Ll][Ii]") && !d.contains(&home)).cloned().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(rules_of(&r), rules_of(&bypass_before), "only the private folder's own rules were added");
    assert_eq!((r.allow.clone(), r.profile), (bypass_before.allow.clone(), bypass_before.profile));
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn strict_applies_to_chats_with_private_folders_outside_bypass() {
    let p = vec!["me/finances".to_string()];
    assert!(strict_applies(true, &p, "chat", "acceptEdits") && strict_applies(true, &p, "customize", "plan"));
    assert!(!strict_applies(false, &p, "chat", "acceptEdits"), "off unless switched on");
    assert!(!strict_applies(true, &[], "chat", "acceptEdits") && !strict_applies(true, &[" ".into()], "chat", "acceptEdits"), "only while there are private folders");
    assert!(!strict_applies(true, &p, "chat", crate::settings::BYPASS));
    for kind in ["wrap", "workflow", "module", "setup", "reorganize"] { assert!(!strict_applies(true, &p, kind, "acceptEdits"), "{kind}"); }
    // A refused command says why.
    let (lib, home) = (Path::new("/home/u/Library"), Path::new("/home/u/.thoughts"));
    let cx = crate::claude::DenialContext { profile: profile("customize", "acceptEdits", false).strict(false), mode: "acceptEdits", cwd: lib, library: Some(lib), private: &p, home };
    let out = crate::claude::denials(&json!([{"tool_name": "Bash", "tool_use_id": "t", "tool_input": {"command": "ls me"}}]), &cx);
    assert_eq!(out[0]["why"], "strict");
}

#[tokio::test]
async fn a_refusal_the_user_gave_on_a_card_is_not_listed_again_under_the_answer() {
    let st = Arc::new(AppState::for_test());
    let id = chat(&st, "chat").await;
    let ticket = open(&st, ctx(&id, "acceptEdits"));
    let (st2, token) = (st.clone(), ticket.token().to_string());
    let call = tokio::spawn(async move { ask(&st2, &token, req("Bash", json!({"command": "git push"}), "carded"), WAIT).await });
    let c = nth_card(&st, &id, 1).await;
    answer(&st, &id, &c.id, Answer::Deny).await.unwrap();
    call.await.unwrap();
    let refused = json!([
        {"tool_name": "Bash", "tool_use_id": "carded", "tool_input": {"command": "git push"}},
        {"tool_name": "WebFetch", "tool_use_id": "web", "tool_input": {"url": "https://example.com"}},
    ]);
    crate::claude::handle_event(&st, &id, &json!({"type": "result", "subtype": "success", "is_error": false, "permission_denials": refused})).await;
    let msgs = st.chats.get(&id).await.unwrap().messages;
    let listed = msgs.iter().find(|m| m.role == "denied").unwrap();
    let items = listed.tool.as_ref().unwrap()["denials"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["what"], "page");
}

#[test]
fn a_save_that_does_not_name_strict_mode_keeps_it() {
    let mut old = crate::settings::Settings::default();
    old.strict_private = Some(true);
    let mut new: crate::settings::Settings = serde_json::from_value(json!({"language": "en"})).unwrap();
    assert_eq!(new.strict_private, None);
    new.sanitize_update(&old).unwrap();
    assert!(new.strict_private());
    let mut off: crate::settings::Settings = serde_json::from_value(json!({"strictPrivate": false})).unwrap();
    off.sanitize_update(&old).unwrap();
    assert!(!off.strict_private(), "switching it off is the user's to do");
    assert!(!crate::settings::Settings::default().strict_private(), "off unless switched on");
}

// ---------- with the real Claude Code, against a stand-in for the Anthropic API ----------

/// A stand-in for the Messages API: the first answer asks to run `command` (a tool use), every later
/// one is plain text. Each request body is kept, so a test can read what the program sent back.
async fn fake_api(command: String) -> (u16, Arc<std::sync::Mutex<Vec<Value>>>) {
    use axum::response::sse::{Event as E, Sse as S};
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let log = seen.clone();
    let answer = move |axum::Json(body): axum::Json<Value>| {
        let (log, command) = (log.clone(), command.clone());
        async move {
            log.lock().unwrap().push(body.clone());
            let msgs = body["messages"].to_string();
            let has_bash = body["tools"].as_array().map_or(false, |t| t.iter().any(|x| x["name"] == "Bash"));
            let tool_turn = has_bash && !msgs.contains("tool_result");
            let model = body["model"].as_str().unwrap_or("mock").to_string();
            let start = json!({"type": "message_start", "message": {"id": "msg_1", "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 1}}});
            let (block, delta, stop) = if tool_turn {
                (json!({"type": "tool_use", "id": "toolu_e2e_1", "name": "Bash", "input": {}}),
                 json!({"type": "input_json_delta", "partial_json": json!({"command": command, "description": "Make a file"}).to_string()}), "tool_use")
            } else {
                (json!({"type": "text", "text": ""}), json!({"type": "text_delta", "text": "Done."}), "end_turn")
            };
            if body["stream"] != true {
                let content = if tool_turn { json!([{"type": "tool_use", "id": "toolu_e2e_1", "name": "Bash", "input": {"command": command}}]) } else { json!([{"type": "text", "text": "Done."}]) };
                return axum::Json(json!({"id": "msg_1", "type": "message", "role": "assistant", "model": model, "content": content, "stop_reason": stop, "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 5}})).into_response();
            }
            let events = vec![
                ("message_start", start),
                ("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": block})),
                ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": delta})),
                ("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
                ("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": null}, "usage": {"output_tokens": 5}})),
                ("message_stop", json!({"type": "message_stop"})),
            ];
            S::new(futures::stream::iter(events.into_iter().map(|(n, d)| Ok::<_, std::convert::Infallible>(E::default().event(n).data(d.to_string()))))).into_response()
        }
    };
    let app = Router::new()
        .route("/v1/messages", post(answer))
        .route("/v1/messages/count_tokens", post(|| async { axum::Json(json!({"input_tokens": 10})) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (port, seen)
}

/// The real program asks the daemon over MCP, waits for the user, and does what they said. Nothing
/// reaches Anthropic: a dummy key, a stand-in API on this computer, a throwaway home, `--bare` (no
/// keychain, no CLAUDE.md, no hooks). Run by hand: `cq test -p thoughtsd real_claude_code -- --ignored`.
/// `THOUGHTS_E2E_WAIT_SECS` delays the answer (past Claude Code's own 60-second and 2-minute marks).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn real_claude_code_asks_through_the_endpoint_and_obeys_the_answer() {
    let Some(bin) = crate::claude::find_binary(&None, "claude") else { eprintln!("no claude installed: skipped"); return };
    let help = String::from_utf8_lossy(&tokio::process::Command::new(&bin).arg("--help").output().await.unwrap().stdout).to_string();
    assert!(supported(&help), "this Claude Code cannot ask through a tool");
    let delay = std::env::var("THOUGHTS_E2E_WAIT_SECS").ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let st = Arc::new(AppState::for_test());
    let daemon = serve(&st).await;
    let base = std::env::temp_dir().join(format!("thoughts-e2e-{}", uuid::Uuid::new_v4()));
    let (home, work) = (base.join("home"), base.join("work"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    for (answer_with, made) in [(Answer::Once, true), (Answer::Deny, false)] {
        let target = work.join(format!("made-{made}.txt"));
        // Not a command accept-edits allows on its own (`touch`, `mkdir`… are), so it has to ask.
        let (api, seen) = fake_api(format!("dd if=/dev/null of={}", target.display())).await;
        let id = chat(&st, "chat").await;
        let ticket = open(&st, RunCtx { chat_id: id.clone(), mode: "acceptEdits".into(), cwd: work.clone(), ..Default::default() });
        let config = base.join(format!("mcp-{made}.json"));
        std::fs::write(&config, config_json(daemon, ticket.token())).unwrap();
        let mut c = tokio::process::Command::new(&bin);
        c.env_clear().env("PATH", std::env::var("PATH").unwrap_or_default()).env("HOME", &home).env("CLAUDE_CONFIG_DIR", home.join(".claude"))
            .env("ANTHROPIC_API_KEY", "sk-ant-dummy-not-a-key").env("ANTHROPIC_BASE_URL", format!("http://127.0.0.1:{api}"))
            .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1").env("DISABLE_AUTOUPDATER", "1").env("DISABLE_TELEMETRY", "1").env("DISABLE_ERROR_REPORTING", "1")
            .current_dir(&work)
            .args(["-p", "--bare", "--output-format", "stream-json", "--verbose", "--permission-mode", "acceptEdits"])
            .args(flags(crate::claude::profile_flags(&profile("chat", "acceptEdits", false), &help), &help, &config))
            .arg("Make the file.")
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
        let run = tokio::spawn(c.spawn().unwrap().wait_with_output());
        // The card appears in the chat: the program asked.
        let card = {
            let mut found = None;
            for _ in 0..1200 {
                if let Some(m) = cards(&st, &id).await.pop() { found = Some(m); break; }
                if run.is_finished() { break; }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            match found {
                Some(m) => m,
                None => {
                    let out = run.await.unwrap().unwrap();
                    panic!("the program never asked ({} API requests).\nstdout: {}\nstderr: {}", seen.lock().unwrap().len(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                }
            }
        };
        let k = &card.tool.as_ref().unwrap()["card"];
        assert_eq!((k["tool"].as_str(), k["what"].as_str(), k["why"].as_str()), (Some("Bash"), Some("command"), Some("command")), "{k}");
        assert_eq!(k["toolUseId"], "toolu_e2e_1");
        if delay > 0 { tokio::time::sleep(Duration::from_secs(delay)).await; }
        answer(&st, &id, &card.id, answer_with).await.unwrap();
        let out = tokio::time::timeout(Duration::from_secs(90), run).await.expect("the program did not finish").unwrap().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("\"type\":\"result\""), "no result; stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(target.exists(), made, "answered {answer_with:?}; stdout: {stdout}");
        let sent_back = seen.lock().unwrap().iter().map(|b| b["messages"].to_string()).collect::<Vec<_>>().join("\n");
        if !made { assert!(sent_back.contains("refused"), "the model is told the user refused: {sent_back}"); }
        drop(ticket);
    }
    std::fs::remove_dir_all(base).unwrap();
}
