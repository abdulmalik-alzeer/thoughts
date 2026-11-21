//! Tests for MADO's core (`mado.rs` and its helpers).

use super::*;
use crate::claude::{start_run, RunInfo, RunSpec};
use crate::mado_comments as comments;
use crate::mado_review as rv;
use crate::mado_validate as val;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("thoughts-mado-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    crate::paths::canonical(&d).unwrap()
}

fn task(id: &str, owns: &[&str], deps: &[&str], feature: &str) -> PlanTask {
    PlanTask { id: id.into(), title: format!("Task {id}"), description: "Do it.".into(), owns: owns.iter().map(|s| s.to_string()).collect(),
        depends_on: deps.iter().map(|s| s.to_string()).collect(), acceptance: vec![format!("npm test -- {id}")], feature: feature.into(), screens: None }
}

const SPEC: &str = "# Shop — Spec\n\n## Features\n### F1 — Sign in\nText.\n### F2 — Orders\nMore.\n";
const FLOW: &str = r#"{"version":1,"kind":"flow","title":"Flow","nodes":[
  {"id":"sign-in","label":"Sign in","kind":"screen","pos":{"x":10,"y":20}},
  {"id":"orders","label":"Orders","kind":"screen"},
  {"id":"ok","label":"OK?","kind":"decision"}],
  "edges":[{"id":"e1","from":"sign-in","to":"ok","label":"submits"},{"id":"e2","from":"ok","to":"orders"}]}"#;

/// A state with one linked project (its folder canonical), and that folder.
async fn with_project(name: &str) -> (Arc<AppState>, String, PathBuf) {
    let st = Arc::new(AppState::for_test());
    let dir = tmp(name);
    std::fs::write(dir.join("README.md"), "# Shop\n").unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    st.settings.write().await.projects.push(crate::settings::Project { id: id.clone(), name: "Shop".into(), path: dir.to_string_lossy().into() });
    (st, id, dir)
}

// ---------- the state machine ----------

#[test]
fn steps_open_in_order_and_a_second_approval_makes_later_ones_stale() {
    let mut s = MadoState::default();
    let status = |s: &MadoState, k: &str| s.steps[k].status.clone();
    assert_eq!(status(&s, "brief"), "ready");
    for k in ["spec", "tech", "design", "plan", "review", "build"] { assert_eq!(status(&s, k), "locked", "{k}"); }
    assert!(!approvable(&s, "brief"), "nothing drafted yet");
    s.steps.get_mut("brief").unwrap().status = "review".into();
    approve_in(&mut s, "brief", "t");
    assert_eq!(status(&s, "spec"), "ready");
    s.steps.get_mut("spec").unwrap().status = "review".into();
    approve_in(&mut s, "spec", "t");
    assert_eq!((status(&s, "tech"), status(&s, "design"), status(&s, "plan")), ("ready".into(), "ready".into(), "locked".into()));
    for k in ["tech"] { s.steps.get_mut(k).unwrap().status = "review".into(); approve_in(&mut s, k, "t"); }
    assert_eq!(status(&s, "plan"), "locked", "plan needs tech and design");
    s.steps.get_mut("design").unwrap().status = "review".into();
    approve_in(&mut s, "design", "t");
    assert_eq!(status(&s, "plan"), "ready");
    s.steps.get_mut("plan").unwrap().status = "review".into();
    approve_in(&mut s, "plan", "t");
    assert!(approvable(&s, "review"), "the review is approved straight from ready");
    approve_in(&mut s, "review", "t");
    assert_eq!(status(&s, "build"), "ready", "the build is ready once the review is approved");
    // The spec changed and is approved again: everything approved after it goes stale, files kept.
    reopen_in(&mut s, "spec");
    assert_eq!(status(&s, "spec"), "review");
    assert_eq!(status(&s, "tech"), "approved", "later steps stay until the spec is approved again");
    approve_in(&mut s, "spec", "t2");
    for k in ["tech", "design", "plan", "review"] { assert_eq!(status(&s, k), "stale", "{k}"); }
    assert_eq!(status(&s, "brief"), "approved");
    assert_eq!(status(&s, "build"), "locked", "no build while the review is stale");
    assert_eq!(later_steps("tech"), vec!["plan", "review", "build"]);
    // Reopening the review forgets the plan it approved.
    s.plan_hash = Some("x".into());
    reopen_in(&mut s, "review");
    assert!(s.plan_hash.is_none());
}

#[test]
fn a_damaged_or_odd_state_is_put_right() {
    let mut s: MadoState = serde_json::from_value(json!({"version": 7, "steps": {"brief": {"status": "flying"}, "nope": {"status": "ready"}, "review": {"status": "drafting", "chatId": "x"}}})).unwrap();
    normalize(&mut s);
    assert_eq!(s.version, 1);
    assert_eq!(s.steps.len(), 7);
    assert_eq!(s.steps["brief"].status, "ready");
    assert_eq!(s.steps["review"].status, "locked");
    assert!(s.steps["review"].chat_id.is_none());
    // The shape the window reads: camelCase in the steps, plan_hash as written in the contract.
    s.plan_hash = Some("h".into());
    let v = serde_json::to_value(&s).unwrap();
    assert!(v.get("plan_hash").is_some() && v["steps"]["brief"].get("chatId").is_none());
}

// ---------- path safety ----------

#[test]
fn names_inside_mado_are_plain_and_never_lead_outside() {
    for bad in ["", "..", "../x", "a/../b", "/etc/passwd", ".hidden", "a//b", "a\\b", "x y", "design/.x"] { assert!(!clean_rel(bad), "{bad:?}"); }
    for good in ["state.json", "design/screens/home.html", "spec.md"] { assert!(clean_rel(good), "{good}"); }
    let root = tmp("paths");
    let mado = root.join("mado");
    std::fs::create_dir_all(mado.join("design")).unwrap();
    assert!(resolve_in(&mado, "../secret", false).is_err());
    let outside = tmp("outside");
    std::fs::write(outside.join("x.md"), "secret").unwrap();
    #[cfg(unix)]
    {
        // A link inside mado/ to somewhere else: never read, never written through.
        std::os::unix::fs::symlink(outside.join("x.md"), mado.join("spec.md")).unwrap();
        assert!(read_in(&mado, "spec.md", false, DOC_MAX).is_err());
        assert!(write_in(&mado, "spec.md", b"new", false).is_err());
        assert_eq!(std::fs::read_to_string(outside.join("x.md")).unwrap(), "secret");
        std::os::unix::fs::symlink(&outside, mado.join("design").join("screens")).unwrap();
        assert!(write_in(&mado, "design/screens/home.html", b"x", false).is_err());
        assert!(!outside.join("home.html").exists());
        // With full access the link is followed, like every other guard yields.
        assert_eq!(read_in(&mado, "spec.md", true, DOC_MAX).unwrap().unwrap(), b"secret");
        // mado/ itself a link: refused (and allowed with full access).
        let other = tmp("linked");
        std::os::unix::fs::symlink(&outside, other.join("mado")).unwrap();
        assert!(mado_dir_of(&other, false).is_err());
        assert!(mado_dir_of(&other, true).is_ok());
        std::fs::remove_dir_all(other).unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[tokio::test]
async fn only_a_linked_project_is_reached() {
    let (st, pid, dir) = with_project("linked").await;
    let e = overview(State(st.clone()), UrlPath(uuid::Uuid::new_v4().to_string())).await.unwrap_err();
    assert_eq!(crate::api::shown_status(&e), 404);
    let e = overview(State(st.clone()), UrlPath("../..".into())).await.unwrap_err();
    assert_eq!(crate::api::shown_status(&e), 404);
    let v = overview(State(st.clone()), UrlPath(pid.clone())).await.unwrap().0;
    assert_eq!(v["exists"], false);
    let _ = start(State(st.clone()), UrlPath(pid.clone())).await.unwrap();
    let v = overview(State(st.clone()), UrlPath(pid.clone())).await.unwrap().0;
    assert_eq!((v["exists"].as_bool(), v["state"]["steps"]["brief"]["status"].as_str()), (Some(true), Some("ready")));
    assert!(dir.join("mado/state.json").is_file());
    // Unknown names are refused, not looked up as paths.
    assert!(doc_get(State(st.clone()), UrlPath((pid.clone(), "../../etc/passwd".into()))).await.is_err());
    assert!(graph_get(State(st.clone()), UrlPath((pid.clone(), "state".into()))).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn starting_needs_a_trusted_folder_unless_full_access_is_on() {
    let (st, pid, dir) = with_project("untrusted").await;
    std::fs::write(dir.join(".mcp.json"), r#"{"mcpServers":{"x":{"command":"evil"}}}"#).unwrap();
    assert!(start(State(st.clone()), UrlPath(pid.clone())).await.is_err());
    assert!(!dir.join("mado").exists());
    st.settings.write().await.full_access.on = true;
    assert!(start(State(st.clone()), UrlPath(pid.clone())).await.is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}

// ---------- validation ----------

#[test]
fn diagrams_and_plans_are_checked_before_anything_uses_them() {
    assert!(val::graph(FLOW.as_bytes(), "flow").is_ok());
    let wrong_kind = val::graph(FLOW.as_bytes(), "erd").unwrap_err();
    assert!(wrong_kind.iter().any(|p| p.contains("\"kind\" must be \"erd\"")));
    let bad = r#"{"version":1,"kind":"flow","title":"F","nodes":[{"id":"Home","label":"","kind":"screen"},{"id":"a","label":"A","kind":"screen"},{"id":"a","label":"B","kind":"screen","pos":{"x":1e9,"y":0}}],
        "edges":[{"id":"e","from":"a","to":"nowhere"}]}"#;
    let p = val::graph(bad.as_bytes(), "flow").unwrap_err();
    for want in ["nodes[0].id \"Home\"", "needs a non-empty \"label\"", "\"a\" is used twice", "not a pair of finite numbers", "\"nowhere\", which is not a node"] {
        assert!(p.iter().any(|x| x.contains(want)), "{want}: {p:?}");
    }
    let too_many = json!({"version":1,"kind":"architecture","title":"A","nodes": (0..401).map(|i| json!({"id": format!("n{i}"), "label": "N", "kind": "service"})).collect::<Vec<_>>(), "edges": []});
    assert!(val::graph(too_many.to_string().as_bytes(), "architecture").unwrap_err()[0].contains("at most 400"));
    let erd = r#"{"version":1,"kind":"erd","title":"D","nodes":[{"id":"user","label":"User","kind":"entity","props":{"fields":[{"name":"id","type":"uuid","key":"pk"}]}},{"id":"order","label":"Order","kind":"entity"}],"edges":[{"id":"r","from":"user","to":"order"}]}"#;
    let p = val::graph(erd.as_bytes(), "erd").unwrap_err();
    assert!(p.iter().any(|x| x.contains("entity \"order\" needs props.fields")) && p.iter().any(|x| x.contains("\"1-1\", \"1-n\" or \"n-n\"")), "{p:?}");
    assert!(val::graph(b"{not json", "flow").unwrap_err()[0].contains("not valid GraphDoc JSON"));
    // Plans: shape, ids, feature ids.
    let good = PlanDoc { version: 1, tasks: vec![task("t-a", &["src/a/**"], &[], "F1")] };
    assert!(val::plan(&serde_json::to_vec(&good).unwrap()).is_ok());
    let mut bad = good.clone();
    bad.tasks.push(PlanTask { feature: "feature-1".into(), acceptance: vec!["a\nb".into()], ..task("T B", &[], &[], "F1") });
    let p = val::plan(&serde_json::to_vec(&bad).unwrap()).unwrap_err();
    assert!(p.iter().any(|x| x.contains("tasks[1].id")) && p.iter().any(|x| x.contains("feature id like F1")) && p.iter().any(|x| x.contains("one non-empty line")), "{p:?}");
    assert!(val::id_ok("a-1") && !val::id_ok("-a") && !val::id_ok(&"a".repeat(65)) && val::feature_ok("F12") && !val::feature_ok("F1234"));
    // Spec headings.
    assert_eq!(val::features(SPEC), vec![("F1".into(), "Sign in".into()), ("F2".into(), "Orders".into())]);
    assert!(val::file("spec.md", b"# Nothing here").iter().any(|p| p.contains("no feature headings")));
    assert!(val::file("design/screens/x.html", b"<div style=\"color:red\"><script>x()</script></div>").iter().any(|p| p.contains("<script>")));
    assert!(val::owned_path_ok("src/a/**") && !val::owned_path_ok("../x") && !val::owned_path_ok("/etc") && !val::owned_path_ok("mado/plan.json") && !val::owned_path_ok(".git/config"));
}

// ---------- the review ----------

#[test]
fn owned_paths_overlap_when_they_could_share_a_file() {
    assert!(rv::overlaps("src/auth/**", "src/auth/login.ts"));
    assert!(rv::overlaps("src/auth", "src/auth/login.ts"), "a folder holds its files");
    assert!(rv::overlaps("src/*.ts", "src/app.ts"));
    assert!(rv::overlaps("SRC/App.ts", "src/app.ts"), "letter case does not tell files apart on macOS");
    assert!(rv::overlaps("**/*.css", "web/x.css"));
    assert!(!rv::overlaps("src/auth/**", "src/orders/**"));
    assert!(!rv::overlaps("src/*.ts", "src/app.css"));
    assert!(!rv::overlaps("package.json", "src/package.json"));
}

#[test]
fn waves_follow_dependencies_and_overlapping_tasks_share_a_lane() {
    let plan = PlanDoc { version: 1, tasks: vec![
        task("setup", &["package.json"], &[], "F1"),
        task("auth", &["src/auth/**"], &["setup"], "F1"),
        task("auth-ui", &["src/auth/ui.ts"], &["setup"], "F1"),
        task("orders", &["src/orders/**"], &["setup"], "F2"),
        task("report", &["src/report/**"], &["auth", "orders"], "F2"),
    ] };
    let w = waves(&plan);
    assert_eq!(w.len(), 3);
    assert_eq!(w[0].lanes, vec![Lane { id: "w1-l1".into(), tasks: vec!["setup".into()] }]);
    assert_eq!(w[1].lanes.iter().map(|l| l.tasks.clone()).collect::<Vec<_>>(), vec![vec!["auth".to_string(), "auth-ui".into()], vec!["orders".into()]]);
    assert_eq!(w[2].lanes[0].tasks, vec!["report"]);
    let flow: GraphDoc = serde_json::from_str(FLOW).unwrap();
    let r = review(SPEC, Some(&flow), &plan);
    assert_eq!(r.features[0].tasks, vec!["setup", "auth", "auth-ui"]);
    let kinds: Vec<&str> = r.problems.iter().map(|p| p.kind.as_str()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "untraced-screen").count(), 2, "{kinds:?}");
    let overlap = r.problems.iter().find(|p| p.kind == "overlap").unwrap();
    assert_eq!(overlap.tasks.as_deref(), Some(&["auth".to_string(), "auth-ui".into()][..]));
    assert!(!r.problems.iter().any(rv::blocks), "{:?}", r.problems);
    assert_eq!(r.commands.len(), 5);
    // The derived task diagram.
    let g = tasks_graph(&plan);
    assert_eq!(g.nodes.len(), 5);
    assert_eq!(g.edges.len(), 5);
    let report = g.nodes.iter().find(|n| n.id == "report").unwrap().props.clone().unwrap();
    assert_eq!((report["wave"].as_u64(), report["feature"].as_str()), (Some(3), Some("F2")));
    assert!(val::graph(&serde_json::to_vec(&g).unwrap(), "tasks").is_ok());
}

#[test]
fn the_review_names_what_would_keep_the_plan_from_being_built() {
    let mut plan = PlanDoc { version: 1, tasks: vec![
        task("a", &["src/a/**"], &["b"], "F1"),
        task("b", &["src/b/**"], &["a"], "F1"),
        task("c", &["../outside"], &["ghost"], "F9"),
    ] };
    plan.tasks[2].acceptance.clear();
    plan.tasks[2].acceptance.push("  ".into());
    let r = rv::review_in(false, SPEC, None, &plan);
    let find = |k: &str| r.problems.iter().filter(|p| p.kind == k).collect::<Vec<_>>();
    assert_eq!(find("cycle")[0].tasks.as_deref(), Some(&["a".to_string(), "b".into()][..]));
    assert!(find("missing-dep")[0].message.contains("ghost"));
    assert!(find("bad-path")[0].message.contains("../outside"));
    assert_eq!(find("empty-acceptance").len(), 1);
    assert!(find("unknown-feature")[0].message.contains("F9"));
    assert!(find("untraced-feature")[0].message.contains("F2 — Orders"));
    assert!(r.problems.iter().filter(|p| rv::blocks(p)).count() >= 5);
    // Tasks in a cycle are in no wave.
    assert!(r.waves.iter().all(|w| w.lanes.iter().all(|l| !l.tasks.contains(&"a".to_string()))));
    assert_eq!(rv::plan_hash(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    // In Arabic for the window.
    assert!(rv::review_in(true, SPEC, None, &plan).problems[0].message.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)));
}

// ---------- comments ----------

#[test]
fn a_quoted_passage_is_found_again_after_the_text_moved() {
    let before = "# Spec\n## F1\nUsers sign in with email.\n## F2\nUsers sign in with email.\n";
    let a = comments::anchor(before, "sign in with email", "## F2\nUsers ", ".\n", Some("F2")).unwrap();
    assert_eq!(&before[a.start..a.start + a.quote.len()], "sign in with email");
    assert!(a.start > before.find("## F2").unwrap(), "the second one: its context and heading match");
    // Text added above, spacing changed: still found, context updated.
    let after = "# Spec\nNew intro.\n## F1\nUsers sign in with email.\n## F2\nUsers sign  in with\nemail.\n";
    let b = comments::anchor(after, "sign in with email", "## F2\nUsers ", ".\n", Some("F2")).unwrap();
    assert_eq!(b.quote, "sign  in with\nemail");
    assert_eq!(b.heading.as_deref(), Some("F2"));
    assert!(b.prefix.ends_with("Users ") && b.prefix.chars().count() <= comments::CONTEXT);
    assert!(comments::anchor(after, "sign in with a passkey", "", "", None).is_none());
}

#[tokio::test]
async fn comments_follow_their_targets_and_go_orphaned_when_they_are_gone() {
    let (st, pid, dir) = with_project("comments").await;
    let _ = start(State(st.clone()), UrlPath(pid.clone())).await.unwrap();
    let mado = dir.join("mado");
    std::fs::write(mado.join("spec.md"), SPEC).unwrap();
    std::fs::write(mado.join("flow.graph.json"), FLOW).unwrap();
    let text = comments::add(&st, &pid, CommentTarget::Text { file: "spec".into(), quote: "More.".into(), prefix: "### F2 — Orders\n".into(), suffix: "\n".into(), heading: None }, "Say more".into()).await.unwrap();
    assert_eq!(text.status, "open");
    if let CommentTarget::Text { file, .. } = &text.target { assert_eq!(file, "spec.md", "stored by its file"); }
    let node = comments::add(&st, &pid, CommentTarget::Node { file: "flow".into(), node_id: "orders".into() }, "Split it".into()).await.unwrap();
    let ghost = comments::add(&st, &pid, CommentTarget::Task { task_id: "t-none".into() }, "?".into()).await.unwrap();
    assert_eq!(ghost.status, "orphaned");
    assert!(comments::add(&st, &pid, CommentTarget::Screen { screen_id: "orders".into(), x: 1.5, y: 0.0 }, "x".into()).await.is_err());
    assert!(comments::add(&st, &pid, CommentTarget::Node { file: "../state.json".into(), node_id: "a".into() }, "x".into()).await.is_err());
    // A run sent both and ended well; it removed the node and moved the passage.
    comments::mark_sent(&st, &pid, &[text.id.clone(), node.id.clone()], "chat-1").await.unwrap();
    std::fs::write(mado.join("spec.md"), "# Shop — Spec\n\nIntro.\n\n### F1 — Sign in\nText.\n### F2 — Orders\nMore.\n").unwrap();
    std::fs::write(mado.join("flow.graph.json"), FLOW.replace(r#"{"id":"orders","label":"Orders","kind":"screen"},"#, "").replace(r#",{"id":"e2","from":"ok","to":"orders"}"#, "")).unwrap();
    comments::after_run(&st, &pid, "chat-1", true).await.unwrap();
    let all = comments::load(&mado, false).unwrap().comments;
    let get = |id: &str| all.iter().find(|c| c.id == id).unwrap().clone();
    assert_eq!(get(&text.id).status, "resolved");
    assert_eq!(get(&node.id).status, "resolved", "resolved first: it was applied");
    // An open comment on a removed passage is orphaned by the next look.
    let open = comments::add(&st, &pid, CommentTarget::Text { file: "spec.md".into(), quote: "Intro.".into(), prefix: "".into(), suffix: "".into(), heading: None }, "Drop".into()).await.unwrap();
    std::fs::write(mado.join("spec.md"), SPEC).unwrap();
    comments::reanchor_project(&st, &pid).await.unwrap();
    assert_eq!(comments::load(&mado, false).unwrap().comments.iter().find(|c| c.id == open.id).unwrap().status, "orphaned");
    // A run that failed puts what it was sent back to open.
    let again = comments::add(&st, &pid, CommentTarget::Node { file: "flow.graph.json".into(), node_id: "sign-in".into() }, "Bigger".into()).await.unwrap();
    comments::mark_sent(&st, &pid, &[again.id.clone()], "chat-2").await.unwrap();
    comments::after_run(&st, &pid, "chat-2", false).await.unwrap();
    let c = comments::load(&mado, false).unwrap().comments.into_iter().find(|c| c.id == again.id).unwrap();
    assert_eq!((c.status.as_str(), c.run_id.is_none()), ("open", true));
    // The user marks and edits; nothing else is allowed.
    assert!(comments::change(&st, &pid, &again.id, None, Some("sent".into())).await.is_err());
    assert_eq!(comments::change(&st, &pid, &again.id, Some("Much bigger".into()), Some("resolved".into())).await.unwrap().status, "resolved");
    comments::remove(&st, &pid, &again.id).await.unwrap();
    assert!(comments::remove(&st, &pid, &again.id).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_change_request_says_exactly_what_to_change_and_where() {
    let root = tmp("prompt");
    let mado = root.join("mado");
    std::fs::create_dir_all(&mado).unwrap();
    std::fs::write(mado.join("flow.graph.json"), FLOW).unwrap();
    let c = Comment { id: "c1".into(), target: CommentTarget::Text { file: "spec.md".into(), quote: "More.".into(), prefix: "Orders\n".into(), suffix: "\n".into(), heading: Some("F2 — Orders".into()) },
        body: "List the order states.".into(), created_at: "t".into(), status: "open".into(), run_id: None };
    let n = Comment { id: "c2".into(), target: CommentTarget::Node { file: "flow.graph.json".into(), node_id: "orders".into() }, body: "Add filters".into(), created_at: "t".into(), status: "open".into(), run_id: None };
    let pin = Comment { id: "c3".into(), target: CommentTarget::Screen { screen_id: "sign-in".into(), x: 0.25, y: 0.5 }, body: "Bigger button".into(), created_at: "t".into(), status: "open".into(), run_id: None };
    let ops = vec![OpsOnFile { file: "flow.graph.json".into(), ops: vec![
        GraphOp::Rename { id: "orders".into(), label: "My orders".into() },
        GraphOp::AddNode { label: "Order detail".into(), kind: "screen".into(), near: Some("orders".into()) },
        GraphOp::RemoveEdge { id: "e1".into() },
    ] }];
    let p = comments::change_prompt(&mado, false, &[c, n, pin], &ops, Some("Keep it short."));
    for want in ["AND to every passage", "Keep every id", "Keep the \"pos\" of every node", "Never write state.json",
        "under the heading \"F2 — Orders\"", "   > More.", "List the order states.",
        "node `orders` (\"Orders\", kind screen) of `flow.graph.json`", "25% from the left and 50% from the top",
        "rename node `orders` (\"Orders\", kind screen) to \"My orders\"", "add a node \"Order detail\" of kind screen, next to `orders`",
        "remove edge `e1` (from `sign-in`", "## The user's note\n\nKeep it short."] {
        assert!(p.contains(want), "{want}\n---\n{p}");
    }
    std::fs::remove_dir_all(root).unwrap();
}

// ---------- the runs ----------

fn respec(s: &RunSpec) -> RunSpec {
    RunSpec { chat_id: s.chat_id.clone(), prompt: s.prompt.clone(), cwd: s.cwd.clone(), add_dirs: s.add_dirs.clone(), options: s.options.clone(),
        resume: s.resume.clone(), append_system: s.append_system.clone(), extra_deny: s.extra_deny.clone() }
}

type Seen = Arc<std::sync::Mutex<Vec<RunInfo>>>;

/// Runs stand in for the assistant: each records what it was started with, then `work` runs in the project.
fn hook(st: &AppState, work: impl Fn(&RunInfo) + Send + Sync + 'static) -> Seen {
    let seen: Seen = Default::default();
    let log = seen.clone();
    let work = Arc::new(work);
    *st.run_hook.lock().unwrap() = Some(Arc::new(move |info: RunInfo| {
        work(&info);
        log.lock().unwrap().push(info);
        Box::pin(async { true })
    }));
    seen
}

async fn idle(st: &AppState, chat: &str, seen: &Seen, n: usize) {
    for _ in 0..600 {
        let running = st.chats.with(chat, |c| c.running).await == Some(true) || st.runs.lock().await.contains_key(chat);
        if seen.lock().unwrap().len() >= n && !running { tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            if st.chats.with(chat, |c| c.running).await != Some(true) { return; } }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the run did not end");
}

fn status_of(dir: &Path, step: &str) -> String { load_state(&dir.join("mado"), false).unwrap().unwrap().steps[step].status.clone() }

#[tokio::test]
async fn a_step_run_may_write_only_mado_and_has_no_commands_tools_or_bypass() {
    let (st, pid, dir) = with_project("profile").await;
    let seen = hook(&st, |_| {});
    let _ = start(State(st.clone()), UrlPath(pid.clone())).await.unwrap();
    let v = step_run(State(st.clone()), UrlPath((pid.clone(), "brief".into())), None).await.unwrap().0;
    let chat = v["chatId"].as_str().unwrap().to_string();
    idle(&st, &chat, &seen, 1).await;
    let info = seen.lock().unwrap()[0].clone();
    assert_eq!(info.profile.name, "mado");
    assert!(!info.profile.shell && !info.profile.mcp && !info.profile.own_setup && !info.profile.web);
    assert_eq!(info.permission_mode.as_deref(), Some("acceptEdits"));
    assert_eq!(info.cwd, dir);
    let p = crate::claude::rule_path(&dir);
    for rule in ["Bash".to_string(), "mcp__*".into(), "WebFetch".into(), format!("Edit({p}/README.md)"), format!("Edit({p}/src/**)"), format!("Edit({p}/*.*)"),
                 format!("Edit({p}/?????*)"), format!("Edit({p}/mado/state.json)"), format!("Edit({p}/mado/comments.json)"), format!("Edit({p}/mado/build.json)")] {
        assert!(info.deny.contains(&rule), "{rule} missing from {:?}", info.deny);
    }
    // Nothing that would close mado/ itself.
    assert!(!info.deny.iter().any(|d| d == &format!("Edit({p}/mado)") || d == &format!("Edit({p}/mado/**)") || d == &format!("Edit({p}/**)")));
    assert!(!info.deny.iter().any(|d| d.starts_with(&format!("Edit({p}/")) && d.contains("[m") && d.ends_with("???)")), "no four-letter pattern matches mado");
    assert!(info.system.contains("This step: the BRIEF") && info.system.contains("You write only inside `mado/`"), "{}", info.system);
    assert!(info.system.contains("interface language is English"), "the language note is there");
    assert_eq!(st.chats.get(&chat).await.unwrap().kind, "mado");
    // Nothing written: the interview goes on, the step stays drafting.
    assert_eq!(status_of(&dir, "brief"), "drafting");
    // Bypass is refused for a step run (it is not an ordinary chat).
    st.chats.update(&chat, |c| c.options.permission_mode = Some(crate::settings::BYPASS.into())).await;
    let spec = RunSpec { chat_id: chat.clone(), prompt: "go".into(), cwd: dir.clone(), add_dirs: vec![], options: st.chats.get(&chat).await.unwrap().options,
        resume: None, append_system: None, extra_deny: vec![] };
    assert!(start_run(st.clone(), respec(&spec), None).await.is_err());
    // A step run somewhere else than its project is refused too.
    let elsewhere = tmp("elsewhere");
    let mut away = respec(&spec);
    away.cwd = elsewhere.clone();
    away.options.permission_mode = None;
    assert!(start_run(st.clone(), away, None).await.is_err());
    // With full access everything yields: bypass, no rules at all.
    st.settings.write().await.full_access.on = true;
    start_run(st.clone(), spec, None).await.unwrap();
    idle(&st, &chat, &seen, 2).await;
    let full = seen.lock().unwrap()[1].clone();
    assert_eq!(full.permission_mode.as_deref(), Some(crate::settings::BYPASS));
    assert!(full.deny.is_empty() && full.full_access && full.system.contains("This step: the BRIEF"));
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(elsewhere).unwrap();
}

#[tokio::test]
async fn invalid_output_gets_one_repair_turn_then_the_user_is_told() {
    let (st, pid, dir) = with_project("repair").await;
    let mado = dir.join("mado");
    let m = mado.clone();
    // Every run writes a broken flow next to a good spec.
    let seen = hook(&st, move |_| {
        std::fs::write(m.join("spec.md"), SPEC).unwrap();
        std::fs::write(m.join("flow.graph.json"), FLOW.replace("\"to\":\"orders\"", "\"to\":\"nowhere\"")).unwrap();
    });
    let _ = start(State(st.clone()), UrlPath(pid.clone())).await.unwrap();
    with_state(&st, &pid, |s| { s.steps.get_mut("brief").unwrap().status = "approved".into(); Ok(()) }).await.unwrap();
    let chat = step_run(State(st.clone()), UrlPath((pid.clone(), "spec".into())), Some(Json(RunBody { note: Some("Keep it small".into()) }))).await.unwrap().0["chatId"].as_str().unwrap().to_string();
    idle(&st, &chat, &seen, 2).await;
    for _ in 0..200 { if status_of(&dir, "spec") != "drafting" { break; } tokio::time::sleep(std::time::Duration::from_millis(10)).await; }
    let runs = seen.lock().unwrap().clone();
    assert_eq!(runs.len(), 2, "one repair turn, not more");
    assert!(runs[0].prompt.contains("Keep it small"));
    assert!(runs[1].prompt.contains("flow.graph.json: edge \"e2\": \"to\" names \"nowhere\""), "{}", runs[1].prompt);
    let c = st.chats.get(&chat).await.unwrap();
    assert!(c.messages.iter().any(|m| m.role == "error" && m.text.contains("nowhere")), "the user is told");
    assert_eq!(status_of(&dir, "spec"), "review");
    // It cannot be approved while it is invalid, and the diagram is never handed out.
    assert!(step_approve(State(st.clone()), UrlPath((pid.clone(), "spec".into())), None).await.is_err());
    assert!(graph_get(State(st.clone()), UrlPath((pid.clone(), "flow".into()))).await.is_err());
    // Fixed by hand: approvable.
    std::fs::write(mado.join("flow.graph.json"), FLOW).unwrap();
    let _ = step_approve(State(st.clone()), UrlPath((pid.clone(), "spec".into())), None).await.unwrap();
    assert_eq!((status_of(&dir, "spec"), status_of(&dir, "tech")), ("approved".into(), "ready".into()));
    // Positions only: the window's save keeps everything else.
    let mut pos = HashMap::new();
    pos.insert("orders".to_string(), Pos { x: 5.0, y: 6.0 });
    pos.insert("unknown".to_string(), Pos { x: 1.0, y: 1.0 });
    let _ = graph_positions(State(st.clone()), UrlPath((pid.clone(), "flow".into())), Json(Positions { positions: pos })).await.unwrap();
    let g = read_graph(&mado, "flow.graph.json", "flow", false).unwrap().unwrap();
    assert_eq!(g.nodes.iter().find(|n| n.id == "orders").unwrap().pos, Some(Pos { x: 5.0, y: 6.0 }));
    assert_eq!(g.nodes.iter().find(|n| n.id == "sign-in").unwrap().pos, Some(Pos { x: 10.0, y: 20.0 }));
    assert_eq!(g.edges.len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn approving_the_review_is_bound_to_the_plan_the_user_was_shown() {
    let (st, pid, dir) = with_project("review").await;
    let mado = dir.join("mado");
    let _ = start(State(st.clone()), UrlPath(pid.clone())).await.unwrap();
    std::fs::write(mado.join("spec.md"), SPEC).unwrap();
    std::fs::write(mado.join("flow.graph.json"), FLOW).unwrap();
    let mut plan = PlanDoc { version: 1, tasks: vec![task("t-a", &["src/a/**"], &[], "F1"), task("t-b", &["src/b/**"], &["t-a"], "F2")] };
    plan.tasks[0].screens = Some(vec!["sign-in".into(), "orders".into()]);
    std::fs::write(mado.join("plan.json"), serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
    with_state(&st, &pid, |s| { for k in ["brief", "spec", "tech", "design", "plan"] { s.steps.get_mut(k).unwrap().status = "approved".into(); } Ok(()) }).await.unwrap();
    let r = review_get(State(st.clone()), UrlPath(pid.clone())).await.unwrap().0;
    let shown = r["planHash"].as_str().unwrap().to_string();
    assert_eq!(shown, plan_hash(&std::fs::read(mado.join("plan.json")).unwrap()));
    assert_eq!(r["commands"].as_array().unwrap().len(), 2);
    let approve = |h: Option<String>| step_approve(State(st.clone()), UrlPath((pid.clone(), "review".into())), Some(Json(ApproveBody { plan_hash: h })));
    // Without the hash: refused.
    let e = approve(None).await.unwrap_err();
    assert_eq!(crate::api::shown_status(&e), 409);
    // The plan changed after the review was shown: refused, nothing approved.
    plan.tasks[1].acceptance = vec!["rm -rf ~".into()];
    std::fs::write(mado.join("plan.json"), serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
    let e = approve(Some(shown.clone())).await.unwrap_err();
    assert_eq!(crate::api::shown_status(&e), 409);
    assert_eq!(status_of(&dir, "review"), "ready");
    assert!(load_state(&mado, false).unwrap().unwrap().plan_hash.is_none());
    // Shown again, approved with that hash, which is kept.
    let now = review_get(State(st.clone()), UrlPath(pid.clone())).await.unwrap().0["planHash"].as_str().unwrap().to_string();
    let _ = approve(Some(now.clone())).await.unwrap();
    let s = load_state(&mado, false).unwrap().unwrap();
    assert_eq!((s.steps["review"].status.as_str(), s.plan_hash.as_deref()), ("approved", Some(now.as_str())));
    // A plan changed after the approval makes the review stale (the build is not allowed then).
    std::fs::write(mado.join("plan.json"), serde_json::to_vec(&plan).unwrap()).unwrap();
    let v = overview(State(st.clone()), UrlPath(pid.clone())).await.unwrap().0;
    assert_eq!(v["state"]["steps"]["review"]["status"], "stale");
    // The task diagram is read-only.
    assert!(graph_positions(State(st.clone()), UrlPath((pid.clone(), "tasks".into())), Json(Positions { positions: HashMap::new() })).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_build_task_may_run_only_its_approved_commands_and_not_write_the_project() {
    let (st, pid, dir) = with_project("build").await;
    let mado = dir.join("mado");
    let _ = start(State(st.clone()), UrlPath(pid.clone())).await.unwrap();
    let plan = PlanDoc { version: 1, tasks: vec![PlanTask { acceptance: vec!["npm ci && npm test".into()], ..task("t-a", &["src/**"], &[], "F1") }] };
    let bytes = serde_json::to_vec(&plan).unwrap();
    std::fs::write(mado.join("plan.json"), &bytes).unwrap();
    with_state(&st, &pid, |s| { s.plan_hash = Some(plan_hash(&bytes)); Ok(()) }).await.unwrap();
    let seen = hook(&st, |_| {});
    let lane = tmp("lane");
    let chat = crate::chats::Chat { kind: "mado-build".into(), meta: json!({"mado": {"projectId": pid, "taskId": "t-a"}}), ..Default::default() };
    st.chats.insert(chat.clone()).await;
    let spec = RunSpec { chat_id: chat.id.clone(), prompt: "build".into(), cwd: lane.clone(), add_dirs: vec![], options: Default::default(), resume: None, append_system: None, extra_deny: vec![] };
    start_run(st.clone(), respec(&spec), None).await.unwrap();
    idle(&st, &chat.id, &seen, 1).await;
    let info = seen.lock().unwrap()[0].clone();
    assert_eq!(info.profile.name, "build");
    assert!(info.profile.shell && !info.profile.mcp && !info.profile.web);
    assert!(!info.deny.contains(&"Bash".to_string()) && info.deny.contains(&"mcp__*".to_string()));
    for a in ["Bash(npm ci && npm test)", "Bash(npm ci)", "Bash(npm test)"] { assert!(info.allow.contains(&a.to_string()), "{a}: {:?}", info.allow); }
    assert!(info.deny.contains(&format!("Edit({}/**)", crate::claude::rule_path(&dir))));
    assert_ne!(info.permission_mode.as_deref(), Some(crate::settings::BYPASS));
    // The plan changed since the review: none of its commands run without asking.
    std::fs::write(mado.join("plan.json"), serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
    start_run(st.clone(), respec(&spec), None).await.unwrap();
    idle(&st, &chat.id, &seen, 2).await;
    assert!(!seen.lock().unwrap()[1].allow.iter().any(|a| a.starts_with("Bash(")));
    // Bypass refused; full access yields.
    let mut by = respec(&spec);
    by.options.permission_mode = Some(crate::settings::BYPASS.into());
    assert!(start_run(st.clone(), by, None).await.is_err());
    st.settings.write().await.full_access.on = true;
    start_run(st.clone(), spec, None).await.unwrap();
    idle(&st, &chat.id, &seen, 3).await;
    let full = seen.lock().unwrap()[2].clone();
    assert!(full.deny.is_empty() && full.allow.is_empty() && full.permission_mode.as_deref() == Some(crate::settings::BYPASS));
    assert_eq!(crate::claude::run_limits("mado").total, None);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(lane).unwrap();
}

#[test]
fn every_step_prompt_names_its_files_and_shapes() {
    let ctx = crate::mado_prompts::Context { files: vec!["brief.md".into()], screens: vec![("home".into(), "Home".into())], features: vec![("F1".into(), "Sign in".into())] };
    for (step, wants) in [
        ("brief", vec!["`mado/brief.md`", "Batch 1", "(assumed)"]),
        ("spec", vec!["### F<n> — <Name>", "\"kind\": \"flow\"", "CRUD completeness", "Forbidden:", "empty, loading, error and forbidden"]),
        ("tech", vec!["`mado/schema.graph.json`", "\"kind\": \"erd\"", "\"1-1\", \"1-n\" or \"n-n\"", "How the work is checked"]),
        ("design", vec!["`mado/design/styles.json`", "three or four", "--mado-color-on-primary", "No scripts", "`home` — Home", "\"Transitional\""]),
        ("plan", vec!["\"dependsOn\"", "\"acceptance\"", "Traceability", "F1 — Sign in"]),
    ] {
        let s = crate::mado_prompts::system(step, &ctx);
        assert!(s.contains("Never write `mado/state.json`") && s.contains("`mado/brief.md`"), "{step}");
        for w in wants { assert!(s.contains(w), "{step}: {w}"); }
        assert!(!s.contains("{screens}") && !s.contains("{{"), "{step}: a placeholder was left");
    }
    let r = crate::mado_prompts::repair(&["plan.json: x".into()]);
    assert!(r.contains("- plan.json: x") && r.contains("keep every id"));
}

#[tokio::test]
async fn a_change_request_runs_in_the_step_chat_and_resolves_what_it_was_sent() {
    let (st, pid, dir) = with_project("changes").await;
    let mado = dir.join("mado");
    let m = mado.clone();
    let seen = hook(&st, move |info| {
        if info.prompt.contains("Rename the screen") { std::fs::write(m.join("flow.graph.json"), FLOW.replace("\"Orders\"", "\"My orders\"")).unwrap(); }
    });
    let _ = start(State(st.clone()), UrlPath(pid.clone())).await.unwrap();
    std::fs::write(mado.join("spec.md"), SPEC).unwrap();
    std::fs::write(mado.join("flow.graph.json"), FLOW).unwrap();
    with_state(&st, &pid, |s| { s.steps.get_mut("brief").unwrap().status = "approved".into(); s.steps.get_mut("spec").unwrap().status = "review".into(); Ok(()) }).await.unwrap();
    let c = comments::add(&st, &pid, CommentTarget::Node { file: "flow.graph.json".into(), node_id: "orders".into() }, "Rename the screen".into()).await.unwrap();
    // Nothing to send is refused; an unknown comment too.
    let none = ChangesBody { step: "spec".into(), comment_ids: vec![], graph_ops: vec![], note: None };
    assert!(changes(State(st.clone()), UrlPath(pid.clone()), Json(none)).await.is_err());
    let ghost = ChangesBody { step: "spec".into(), comment_ids: vec!["nope".into()], graph_ops: vec![], note: None };
    assert!(changes(State(st.clone()), UrlPath(pid.clone()), Json(ghost)).await.is_err());
    let body = ChangesBody { step: "spec".into(), comment_ids: vec![c.id.clone()], graph_ops: vec![OpsOnFile { file: "flow.graph.json".into(), ops: vec![GraphOp::RemoveEdge { id: "e2".into() }] }], note: None };
    let chat = changes(State(st.clone()), UrlPath(pid.clone()), Json(body)).await.unwrap().0["chatId"].as_str().unwrap().to_string();
    idle(&st, &chat, &seen, 1).await;
    for _ in 0..200 { if comments::load(&mado, false).unwrap().comments[0].status != "sent" { break; } tokio::time::sleep(std::time::Duration::from_millis(10)).await; }
    let prompt = seen.lock().unwrap()[0].prompt.clone();
    assert!(prompt.contains("Rename the screen") && prompt.contains("remove edge `e2`"), "{prompt}");
    assert_eq!(comments::load(&mado, false).unwrap().comments[0].status, "resolved");
    assert_eq!(status_of(&dir, "spec"), "review");
    assert_eq!(load_state(&mado, false).unwrap().unwrap().steps["spec"].chat_id.as_deref(), Some(chat.as_str()), "the step's one chat");
    std::fs::remove_dir_all(dir).unwrap();
}
