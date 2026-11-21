//! MADO end to end, over HTTP on the merged daemon's own router: the planning core (`mado.rs`),
//! the design step (`mado_design.rs`) and the build engine (`mado_build.rs`) meeting for the first
//! time. `run_hook` stands in for the assistant: it writes valid files for each step, applies a
//! change request, and does each build task's work in its worktree.

use crate::claude::RunInfo;
use crate::state::AppState;
use crate::wraps::testutil as t;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One request over a real connection, with the app token; → (status, JSON body).
async fn call(port: u16, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer test-token\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    s.write_all(head.as_bytes()).await.unwrap();
    s.write_all(payload.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    tokio::time::timeout(Duration::from_secs(60), s.read_to_end(&mut out)).await.expect("the answer came").unwrap();
    let text = String::from_utf8_lossy(&out).to_string();
    let status = text.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        // Chunked: size line, data, …
        let mut b = String::new();
        let mut r = rest;
        while let Some((size, more)) = r.split_once("\r\n") {
            let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
            if n == 0 { break; }
            b.push_str(&more[..n.min(more.len())]);
            r = more.get(n + 2..).unwrap_or("");
        }
        b
    } else { rest.to_string() };
    (status, serde_json::from_str(&body).unwrap_or(Value::Null))
}

struct World { st: Arc<AppState>, port: u16, pid: String, dir: PathBuf, root: PathBuf, runs: Arc<Mutex<Vec<(String, String, RunInfo)>>> }

impl World {
    async fn get(&self, path: &str) -> (u16, Value) { call(self.port, "GET", &format!("/api/mado/{}{path}", self.pid), None).await }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) { call(self.port, "POST", &format!("/api/mado/{}{path}", self.pid), Some(body)).await }
    async fn status(&self, step: &str) -> String { self.get("").await.1["state"]["steps"][step]["status"].as_str().unwrap_or_default().to_string() }
    /// Wait until `step` is at `want` (a run ends, its files are checked).
    async fn until(&self, step: &str, want: &str) {
        for _ in 0..1500 {
            if self.status(step).await == want { return; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("{step} never reached {want}: {}", self.get("").await.1);
    }
    async fn run(&self, step: &str) {
        let (s, v) = self.post(&format!("/steps/{step}/run"), json!({})).await;
        assert_eq!(s, 200, "run {step}: {v}");
        self.until(step, "review").await;
    }
    async fn approve(&self, step: &str) {
        let (s, v) = self.post(&format!("/steps/{step}/approve"), json!({})).await;
        assert_eq!(s, 200, "approve {step}: {v}");
    }
    fn done(self) {
        let _ = std::fs::remove_dir_all(&self.root);
        if let Some(d) = crate::mado::record_dir(&self.pid) { let _ = std::fs::remove_dir_all(d); }
    }
}

const SPEC: &str = "# Shop — Spec\n\n## Features\n\n### F1 — Orders\nPeople place orders and see them.\n";

fn flow(extra: bool) -> Value {
    let mut nodes = vec![json!({"id": "sign-in", "label": "Sign in", "kind": "screen"}), json!({"id": "orders", "label": "Orders", "kind": "screen"})];
    let mut edges = vec![json!({"id": "e1", "from": "sign-in", "to": "orders", "label": "signs in"})];
    if extra {
        nodes.push(json!({"id": "pay", "label": "Pay", "kind": "screen"}));
        edges.push(json!({"id": "e2", "from": "orders", "to": "pay", "label": "checkout"}));
    }
    json!({"version": 1, "kind": "flow", "title": "Flow", "nodes": nodes, "edges": edges})
}

fn tokens() -> Value { crate::mado_design::styles::app_tokens()[0].1.json() }

/// What the stand-in assistant writes for a planning step (into `<project>/mado/`).
fn write_step(mado: &Path, step: &str, prompt: &str) {
    let w = |rel: &str, v: String| { let p = mado.join(rel); std::fs::create_dir_all(p.parent().unwrap()).unwrap(); std::fs::write(p, v).unwrap(); };
    match step {
        "brief" => w("brief.md", "# Brief\n\nA small shop.\n".into()),
        "spec" => {
            // The first run writes the spec; a change request (its prompt names the new node) adds the screen.
            let changed = prompt.contains("Pay");
            w("spec.md", if changed { format!("{SPEC}\nPaying is its own screen.\n") } else { SPEC.into() });
            w("flow.graph.json", flow(changed).to_string());
        }
        "tech" => {
            w("tech.md", "# Tech\n\nOne web app.\n".into());
            w("architecture.graph.json", json!({"version": 1, "kind": "architecture", "title": "Parts", "nodes": [{"id": "web", "label": "Web", "kind": "service"}, {"id": "db", "label": "DB", "kind": "database"}], "edges": [{"id": "a1", "from": "web", "to": "db"}]}).to_string());
            w("schema.graph.json", json!({"version": 1, "kind": "erd", "title": "Data", "nodes": [{"id": "order", "label": "Order", "kind": "entity", "props": {"fields": [{"name": "id", "type": "uuid", "key": "pk"}]}}], "edges": []}).to_string());
        }
        "design" => {
            w("design.md", "# Design\n\nCalm.\n".into());
            let styles: Vec<Value> = ["calm", "bold", "warm"].iter().map(|id| json!({"id": id, "name": id, "description": "A style.", "source": "assistant", "tokens": tokens()})).collect();
            w("design/styles.json", json!({"version": 1, "styles": styles}).to_string());
            for id in ["sign-in", "orders", "pay"] {
                w(&format!("design/screens/{id}.html"), format!("<!doctype html><style>body{{background:var(--mado-color-bg);color:var(--mado-color-on-primary)}}</style><h1>{id}</h1>"));
            }
        }
        "plan" => {
            let task = |id: &str, deps: &[&str]| json!({"id": id, "title": format!("Task {id}"), "description": "Make it.", "owns": [format!("src/{id}/**")],
                "dependsOn": deps, "acceptance": [format!("test -f src/{id}/{id}.txt")], "feature": "F1", "screens": ["sign-in", "orders", "pay"]});
            w("plan.json", json!({"version": 1, "setup": ["git --version"], "tasks": [task("a", &[]), task("b", &["a"])]}).to_string());
        }
        _ => {}
    }
}

/// A trusted, linked project that is a git repository with one commit; the daemon listening.
async fn world() -> World {
    let root = std::env::temp_dir().join(format!("thoughts-mado-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("proj")).unwrap();
    let dir = dunce::canonicalize(root.join("proj")).unwrap();
    t::sh(&dir, &["init", "-q", "-b", "main"]);
    for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com"), ("commit.gpgsign", "false")] { t::sh(&dir, &["config", k, v]); }
    t::write(&dir, "README.md", "shop\n");
    t::sh(&dir, &["add", "-A"]);
    t::sh(&dir, &["commit", "-q", "-m", "first"]);
    let st = Arc::new(AppState::for_test());
    let pid = uuid::Uuid::new_v4().to_string();
    st.settings.write().await.projects.push(crate::settings::Project { id: pid.clone(), name: "Shop".into(), path: dir.to_string_lossy().into() });
    crate::trust::trust(&dir).unwrap();

    let runs: Arc<Mutex<Vec<(String, String, RunInfo)>>> = Default::default();
    let (log, st2) = (runs.clone(), Arc::downgrade(&st));
    *st.run_hook.lock().unwrap() = Some(Arc::new(move |info: RunInfo| {
        let (log, st2) = (log.clone(), st2.clone());
        Box::pin(async move {
            let Some(st) = st2.upgrade() else { return false };
            let (kind, meta) = st.chats.with(&info.chat_id, |c| (c.kind.clone(), c.meta.clone())).await.unwrap_or_default();
            let what = match kind.as_str() {
                "mado" => { let step = meta["mado"]["step"].as_str().unwrap_or_default().to_string(); write_step(&info.cwd.join("mado"), &step, &info.prompt); step }
                "mado-build" => {
                    let task = meta["mado"]["taskId"].as_str().unwrap_or_default().to_string();
                    t::write(&info.cwd, &format!("src/{task}/{task}.txt"), "done\n");
                    task
                }
                _ => String::new(),
            };
            log.lock().unwrap().push((kind, what, info));
            true
        })
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = crate::api::router(st.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    World { st, port, pid, dir, root, runs }
}

#[tokio::test]
async fn a_project_goes_from_brief_to_landed_over_http() {
    let w = world().await;
    let before = t::sh(&w.dir, &["rev-parse", "HEAD"]);

    let (s, v) = w.post("/start", json!({})).await;
    assert_eq!((s, v["state"]["steps"]["brief"]["status"].as_str()), (200, Some("ready")), "{v}");

    // brief → spec
    w.run("brief").await;
    w.approve("brief").await;
    w.run("spec").await;

    // A comment on a passage and an edit of the flow, sent as one change request; the new node is
    // named by its temporary key, as the diagram makes it.
    let (s, c) = w.post("/comments", json!({"target": {"kind": "text", "file": "spec.md", "quote": "People place orders", "prefix": "", "suffix": " and see them."}, "body": "Paying needs a screen of its own."})).await;
    assert_eq!(s, 200, "{c}");
    let cid = c["id"].as_str().unwrap().to_string();
    let ops = json!([{"op": "add-node", "label": "Pay", "kind": "screen", "near": "orders"}, {"op": "add-edge", "from": "orders", "to": "new:1", "label": "checkout"}]);
    let (s, v) = w.post("/changes", json!({"step": "spec", "commentIds": [cid], "graphOps": [{"file": "flow.graph.json", "ops": ops}]})).await;
    assert_eq!(s, 200, "{v}");
    w.until("spec", "review").await;
    {
        let runs = w.runs.lock().unwrap();
        let (_, _, info) = runs.iter().rev().find(|(k, s, _)| k == "mado" && s == "spec").unwrap();
        assert!(info.prompt.contains("Paying needs a screen of its own.") && info.prompt.contains("Pay"), "{}", info.prompt);
        assert!(!info.prompt.contains("new:1"), "the temporary key is put in words for the assistant: {}", info.prompt);
        assert_eq!(info.setting_sources.as_deref(), Some(""));
    }
    for _ in 0..200 {
        if w.get("/comments").await.1["comments"][0]["status"] == "resolved" { break; }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(w.get("/comments").await.1["comments"][0]["status"], "resolved");
    let (_, g) = w.get("/graph/flow").await;
    assert!(g["nodes"].as_array().unwrap().iter().any(|n| n["id"] == "pay"), "{g}");
    w.approve("spec").await;

    // tech and design (both open after the spec)
    w.run("tech").await;
    w.approve("tech").await;
    w.run("design").await;
    let (s, v) = w.post("/design/style", json!({"id": "app-calm"})).await;
    assert_eq!((s, v["picked"].as_str()), (200, Some("app-calm")), "{v}");
    assert_eq!(w.get("").await.1["state"]["style"], "app-calm", "the pick lives in the core's state");
    let (_, sc) = w.get("/design/screens").await;
    assert_eq!(sc.as_array().unwrap().len(), 3, "{sc}");
    w.approve("design").await;

    // plan → review
    w.run("plan").await;
    {
        let runs = w.runs.lock().unwrap();
        let (_, _, info) = runs.iter().rev().find(|(k, s, _)| k == "mado" && s == "plan").unwrap();
        let p = crate::claude::rule_path(&w.dir);
        assert!(info.deny.contains(&format!("Edit({p}/README.md)")), "the deny layer is there");
    }
    w.approve("plan").await;
    let (s, r) = w.get("/review").await;
    assert_eq!(s, 200, "{r}");
    assert_eq!(r["commands"][0], "git --version", "setup commands come first: {r}");
    assert!(r["problems"].as_array().unwrap().iter().all(|p| !crate::mado_review::blocks(&serde_json::from_value(p.clone()).unwrap())), "{r}");
    // Without the hash shown, or with another, nothing is approved.
    assert_eq!(w.post("/steps/review/approve", json!({})).await.0, 409);
    assert_eq!(w.post("/steps/review/approve", json!({"planHash": "0".repeat(64)})).await.0, 409);
    // The app token alone is not a yes: the desktop shell's approval for this plan goes with it.
    assert_eq!(w.post("/steps/review/approve", json!({"planHash": r["planHash"]})).await.0, 403);
    let yes = crate::consent::test_mado_approval(&w.pid, r["planHash"].as_str().unwrap());
    let (s, v) = w.post(&format!("/steps/review/approve?approval={yes}"), json!({"planHash": r["planHash"]})).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(w.status("review").await, "approved");
    assert!(crate::mado::approved_plan_in(&w.pid).is_some(), "the approval is recorded outside the project");

    // build: two rounds, on a branch of its own
    let (s, b) = w.get("/build").await;
    assert!(s == 200 && b.get("blocked").map_or(true, Value::is_null), "nothing keeps the build from starting: {b}");
    let (s, v) = w.post("/build/start", json!({"maxParallel": 2})).await;
    assert_eq!(s, 200, "{v}");
    let mut b = Value::Null;
    for _ in 0..3000 {
        b = w.get("/build").await.1;
        if b["status"] == "done" || b["status"] == "waiting" || b["status"] == "failed" { break; }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(b["status"], "done", "{b}");
    assert_eq!(b["waves"].as_array().unwrap().len(), 2, "{b}");
    assert_eq!(t::sh(&w.dir, &["rev-parse", "HEAD"]), before, "the user's branch is untouched during the build");
    {
        let runs = w.runs.lock().unwrap();
        let builds: Vec<&RunInfo> = runs.iter().filter(|(k, _, _)| k == "mado-build").map(|(_, _, i)| i).collect();
        assert_eq!(builds.len(), 2);
        for i in builds { assert_eq!(i.setting_sources.as_deref(), Some(""), "build runs load no user or project settings"); }
    }
    // Build chats are not ordinary chats (the window keeps them out of its lists by kind).
    let (_, chats) = call(w.port, "GET", "/api/chats", None).await;
    assert!(chats.as_array().unwrap().iter().filter(|c| c["kind"] == "mado-build").count() == 2, "{chats}");

    // land
    let (s, v) = w.post("/build/land", json!({})).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(t::read(&w.dir, "src/a/a.txt"), "done\n");
    assert_eq!(t::read(&w.dir, "src/b/b.txt"), "done\n");
    assert_eq!(t::sh(&w.dir, &["status", "--porcelain"]), "?? mado/", "mado/ was never committed");

    // A mock-up fetched is cleaned, whatever the file holds.
    std::fs::write(w.dir.join("mado/design/screens/pay.html"), "<h1 onclick=\"x()\">Pay</h1><script>alert(1)</script><img src=\"https://evil.example/p.gif\"><style>a{background:url(//evil/x)}</style>").unwrap();
    let (s, v) = w.get("/design/screens/pay").await;
    assert_eq!(s, 200, "{v}");
    let html = v["html"].as_str().unwrap();
    assert!(!html.contains("<script") && !html.contains("onclick") && !html.contains("evil"), "{html}");
    w.done();
}

#[tokio::test]
async fn a_forged_state_file_is_not_believed_and_starts_nothing() {
    let w = world().await;
    assert_eq!(w.post("/start", json!({})).await.0, 200);
    // Files and a state.json someone else wrote, saying everything up to the review is approved.
    let mado = w.dir.join("mado");
    for step in ["brief", "spec", "tech", "design", "plan"] { write_step(&mado, step, ""); }
    let plan = std::fs::read(mado.join("plan.json")).unwrap();
    let approved = |s: &str| json!({"status": s, "approvedAt": "2026-11-21T00:00:00Z"});
    std::fs::write(mado.join("state.json"), json!({"version": 1, "steps": {"brief": approved("approved"), "spec": approved("approved"), "tech": approved("approved"),
        "design": approved("approved"), "plan": approved("approved"), "review": approved("approved"), "build": {"status": "ready"}}, "plan_hash": crate::mado::plan_hash(&plan)}).to_string()).unwrap();

    let (_, o) = w.get("").await;
    assert_eq!(o["state"]["steps"]["review"]["status"], "locked", "{o}");
    assert_eq!(o["state"]["steps"]["build"]["status"], "locked", "{o}");
    assert_eq!(o["state"]["steps"]["plan"]["status"], "review", "{o}");
    assert!(o["state"]["plan_hash"].is_null(), "{o}");
    assert!(crate::mado::approved_plan_in(&w.pid).is_none());
    // The build refuses, and no command runs.
    let (_, b) = w.get("/build").await;
    assert!(b["blocked"].as_str().map_or(false, |s| !s.is_empty()), "{b}");
    let (s, v) = w.post("/build/start", json!({})).await;
    assert_eq!(s, 409, "{v}");
    assert!(w.runs.lock().unwrap().is_empty());
    // The approval cannot be had without going through the steps either.
    let (s, _) = w.post("/steps/review/approve", json!({"planHash": crate::mado::plan_hash(&plan)})).await;
    assert_eq!(s, 409);
    w.done();
}
