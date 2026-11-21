//! The design routes: only linked projects, only inside `<project>/mado/`, nothing a run wrote is
//! handed on unchecked, and `state.json` is read but never written.

use super::*;
use axum::response::IntoResponse;
use std::path::PathBuf;

struct Fixture { st: Arc<AppState>, id: String, project: PathBuf, mado: PathBuf }

async fn fixture() -> Fixture {
    let st = Arc::new(AppState::for_test());
    let root = std::env::temp_dir().join(format!("thoughts-mado-design-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(root.join("mado")).unwrap();
    let project = crate::paths::canonical(&root).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    st.settings.write().await.projects.push(crate::settings::Project { id: id.clone(), name: "Shop".into(), path: project.to_string_lossy().into() });
    Fixture { st, id, mado: project.join("mado"), project }
}

impl Fixture {
    fn put(&self, rel: &str, text: &str) {
        let p = self.mado.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn read(&self, rel: &str) -> Option<String> { std::fs::read_to_string(self.mado.join(rel)).ok() }
    fn state(&self, design: &str) { self.put("state.json", &json!({"version": 1, "steps": {"design": {"status": design}}}).to_string()); }
    fn s(&self) -> S { State(self.st.clone()) }
    fn p(&self) -> UrlPath<String> { UrlPath(self.id.clone()) }
}

/// (HTTP status, body) of an answer, as the window would get it.
async fn answer(r: R) -> (u16, Value) {
    let resp = match r { Ok(j) => j.into_response(), Err(e) => e.into_response() };
    let status = resp.status().as_u16();
    let bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn good_tokens() -> Value { styles::app_tokens()[0].1.json() }

#[tokio::test]
async fn only_a_linked_project_with_a_plan_is_answered() {
    let f = fixture().await;
    let (status, body) = answer(list_styles(f.s(), UrlPath("no-such-project".into())).await).await;
    assert_eq!((status, body["code"].as_str()), (404, Some("not_found")));
    // A path is not an id, however it is written.
    for id in ["../..", f.project.to_str().unwrap(), ""] {
        assert_eq!(answer(get_tokens(f.s(), UrlPath(id.into())).await).await.0, 404, "{id}");
        assert_eq!(answer(screen(f.s(), UrlPath((id.into(), "home".into()))).await).await.0, 404, "{id}");
    }
    // A linked project whose plan was never started.
    std::fs::remove_dir(&f.mado).unwrap();
    for r in [list_styles(f.s(), f.p()).await, get_tokens(f.s(), f.p()).await, screens(f.s(), f.p()).await,
        put_tokens(f.s(), f.p(), Json(good_tokens())).await, pick(f.s(), f.p(), Json(json!({"id": "app-calm"}))).await] {
        assert_eq!(answer(r).await.0, 404);
    }
    assert!(!f.mado.exists(), "nothing made a plan folder on its own");
}

#[tokio::test]
async fn styles_are_the_apps_first_then_the_assistants_valid_ones() {
    let f = fixture().await;
    let (status, body) = answer(list_styles(f.s(), f.p()).await).await;
    assert_eq!(status, 200);
    let list = body["styles"].as_array().unwrap();
    assert_eq!(list.len(), 8);
    assert!(list.iter().all(|s| s["source"] == "app" && s["id"].as_str().unwrap().starts_with("app-") && !s["name"].as_str().unwrap().is_empty()));
    assert_eq!((body["dropped"].as_u64(), body.get("picked")), (Some(0), None));

    let mut bad = good_tokens();
    bad["radius"] = json!(900);
    f.put("design/styles.json", &json!({"version": 1, "styles": [
        {"id": "harbour", "name": "Harbour", "description": "Sea colours.", "tokens": good_tokens(), "source": "assistant"},
        {"id": "broken", "name": "Broken", "description": "", "tokens": bad, "source": "assistant"},
    ]}).to_string());
    let (_, body) = answer(list_styles(f.s(), f.p()).await).await;
    let list = body["styles"].as_array().unwrap();
    assert_eq!(list.len(), 9);
    assert_eq!((list[8]["id"].as_str(), list[8]["source"].as_str()), (Some("harbour"), Some("assistant")));
    assert_eq!(body["dropped"], 1);

    // A file that is not the shape at all, or far too large: the app's examples still come.
    f.put("design/styles.json", "<html>");
    let (_, body) = answer(list_styles(f.s(), f.p()).await).await;
    assert_eq!((body["styles"].as_array().unwrap().len(), body["dropped"].as_u64()), (8, Some(1)));
    f.put("design/styles.json", &" ".repeat(MAX_STYLES as usize + 1));
    let (status, body) = answer(list_styles(f.s(), f.p()).await).await;
    assert_eq!((status, body["styles"].as_array().unwrap().len(), body["dropped"].as_u64()), (200, 8, Some(1)));
}

#[tokio::test]
async fn picking_a_style_writes_its_settings_and_tells_the_window() {
    let f = fixture().await;
    let mut events = f.st.events.subscribe();
    f.put("design/styles.json", &json!({"version": 1, "styles": [{"id": "harbour", "name": "Harbour", "description": "", "tokens": styles::app_tokens()[3].1.json()}]}).to_string());

    for bad in [json!({"id": "../x"}), json!({"id": 3}), json!({}), json!({"id": "Harbour"})] {
        assert_eq!(answer(pick(f.s(), f.p(), Json(bad.clone())).await).await.0, 422, "{bad}");
    }
    assert_eq!(answer(pick(f.s(), f.p(), Json(json!({"id": "gone"}))).await).await.0, 404);
    assert!(f.read("design/tokens.json").is_none() && events.try_recv().is_err());

    let (status, body) = answer(pick(f.s(), f.p(), Json(json!({"id": "harbour"}))).await).await;
    assert_eq!((status, body["picked"].as_str()), (200, Some("harbour")));
    let saved: Value = serde_json::from_str(&f.read("design/tokens.json").unwrap()).unwrap();
    assert_eq!(saved, styles::app_tokens()[3].1.json());
    assert_eq!(events.try_recv().unwrap(), json!({"type": "mado", "projectId": f.id, "what": "design"}));
    assert_eq!(answer(list_styles(f.s(), f.p()).await).await.1["picked"], "harbour");
    assert_eq!(answer(get_tokens(f.s(), f.p()).await).await.1, saved);

    // One of the app's own.
    assert_eq!(answer(pick(f.s(), f.p(), Json(json!({"id": "app-luxury"}))).await).await.0, 200);
    assert_eq!(answer(list_styles(f.s(), f.p()).await).await.1["picked"], "app-luxury");
    // The core's record wins when it keeps one.
    f.put("state.json", &json!({"version": 1, "steps": {}, "style": "harbour"}).to_string());
    assert_eq!(answer(list_styles(f.s(), f.p()).await).await.1["picked"], "harbour");
    f.put("state.json", &json!({"version": 1, "steps": {}, "style": "../../etc"}).to_string());
    assert_eq!(answer(list_styles(f.s(), f.p()).await).await.1.get("picked"), None);
}

#[tokio::test]
async fn settings_are_validated_saved_whole_and_closed_while_approved() {
    let f = fixture().await;
    assert_eq!(answer(get_tokens(f.s(), f.p()).await).await, (200, Value::Null));

    let mut mine = good_tokens();
    mine["color"]["primary"] = json!("#AA3366");
    mine["radius"] = json!(12.5);
    let (status, body) = answer(put_tokens(f.s(), f.p(), Json(mine.clone())).await).await;
    assert_eq!((status, body["color"]["primary"].as_str(), body["radius"].as_f64()), (200, Some("#aa3366"), Some(12.5)));
    let saved = f.read("design/tokens.json").unwrap();
    assert_eq!(serde_json::from_str::<Value>(&saved).unwrap(), body);
    // No half-written or temporary file is left beside it.
    let names: Vec<String> = std::fs::read_dir(f.mado.join("design")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["tokens.json"]);

    // Anything invalid is refused and what was saved stays.
    let mut bad = mine.clone();
    bad["color"]["bg"] = json!("url(//evil)");
    let mut extra = mine.clone();
    extra["script"] = json!("alert(1)");
    for body in [bad, extra, json!(null), json!([]), json!("x")] {
        let (status, err) = answer(put_tokens(f.s(), f.p(), Json(body)).await).await;
        assert_eq!((status, err["code"].as_str()), (422, Some("validation")));
    }
    assert_eq!(f.read("design/tokens.json").unwrap(), saved);

    // Approved: closed, for a new pick as well. Reopened (any other status): open again.
    f.state("approved");
    mine["spacing"] = json!(12);
    assert_eq!(answer(put_tokens(f.s(), f.p(), Json(mine.clone())).await).await.0, 409);
    assert_eq!(answer(pick(f.s(), f.p(), Json(json!({"id": "app-warm"}))).await).await.0, 409);
    assert_eq!(f.read("design/tokens.json").unwrap(), saved);
    assert_eq!(answer(get_tokens(f.s(), f.p()).await).await.0, 200, "reading stays open");
    for status in ["review", "stale", "ready", "drafting"] {
        f.state(status);
        assert_eq!(answer(put_tokens(f.s(), f.p(), Json(mine.clone())).await).await.0, 200, "{status}");
    }
    // The state file is exactly what the core wrote.
    assert_eq!(f.read("state.json").unwrap(), json!({"version": 1, "steps": {"design": {"status": "drafting"}}}).to_string());

    // What a run left in the file is not handed on unchecked.
    f.put("design/tokens.json", "{\"color\": \"</style><script>alert(1)</script>\"}");
    let (status, err) = answer(get_tokens(f.s(), f.p()).await).await;
    assert_eq!((status, err["code"].as_str()), (422, Some("validation")));
    f.put("design/tokens.json", &"x".repeat(MAX_TOKENS as usize + 1));
    assert_eq!(answer(get_tokens(f.s(), f.p()).await).await.0, 413);
}

#[tokio::test]
async fn screens_come_from_the_flow_and_mock_ups_come_cleaned() {
    let f = fixture().await;
    assert_eq!(answer(screens(f.s(), f.p()).await).await, (200, json!([])));
    f.put("flow.graph.json", &json!({"version": 1, "kind": "flow", "title": "Flow", "edges": [], "nodes": [
        {"id": "home", "label": "Home", "kind": "screen"},
        {"id": "pay", "label": "Pay\u{0007}ment", "kind": "screen"},
        {"id": "is-member", "label": "Member?", "kind": "decision"},
        {"id": "../../secret", "label": "Path", "kind": "screen"},
        {"id": "Home", "label": "Upper", "kind": "screen"},
        {"id": "home", "label": "Twice", "kind": "screen"},
        {"id": "empty", "kind": "screen"},
    ]}).to_string());
    f.put("design/screens/home.html", "<!doctype html><title>t</title><style>h1{color:var(--mado-color-primary);background:url(//evil/x)}</style>\
        <h1 onclick=\"alert(1)\">Home</h1><script>alert(1)</script><img src=\"https://evil.example/p.gif\"><a href=\"javascript:alert(1)\">go</a>");
    let (status, list) = answer(screens(f.s(), f.p()).await).await;
    assert_eq!(status, 200);
    assert_eq!(list, json!([{"id": "home", "label": "Home", "hasMock": true}, {"id": "pay", "label": "Payment", "hasMock": false}, {"id": "empty", "label": "empty", "hasMock": false}]));

    let (status, body) = answer(screen(f.s(), UrlPath((f.id.clone(), "home".into()))).await).await;
    assert_eq!(status, 200);
    assert_eq!(body["html"], "<style>h1{color:var(--mado-color-primary);}</style><body><h1>Home</h1><img><a>go</a></body>");

    assert_eq!(answer(screen(f.s(), UrlPath((f.id.clone(), "pay".into()))).await).await.0, 404);
    for id in ["../../secret", "..", "home.html", "Home", "home/../home", "", "a b", &"a".repeat(65)] {
        let (status, err) = answer(screen(f.s(), UrlPath((f.id.clone(), id.to_string()))).await).await;
        assert_eq!((status, err["code"].as_str()), (422, Some("validation")), "{id}");
    }
    f.put("design/screens/pay.html", &"x".repeat(MAX_MOCK as usize + 1));
    assert_eq!(answer(screen(f.s(), UrlPath((f.id.clone(), "pay".into()))).await).await.0, 413);
    f.put("design/screens/pay.html", &"x".repeat(MAX_MOCK as usize));
    assert_eq!(answer(screen(f.s(), UrlPath((f.id.clone(), "pay".into()))).await).await.0, 200);
    // A page that passes a limit while being read is refused whole, in plain words.
    f.put("design/screens/pay.html", &"<div>".repeat(50_000));
    let (status, err) = answer(screen(f.s(), UrlPath((f.id.clone(), "pay".into()))).await).await;
    assert_eq!((status, err["code"].as_str(), err["detail"].as_str()), (422, Some("validation"), Some("TooDeep")));
    // Not text at all.
    std::fs::write(f.mado.join("design/screens/empty.html"), [0xffu8, 0xfe, 0x00, b'<', b's', b'c', b'r', b'i', b'p', b't', b'>']).unwrap();
    let (status, body) = answer(screen(f.s(), UrlPath((f.id.clone(), "empty".into()))).await).await;
    assert!(status == 200 && !body["html"].as_str().unwrap().contains("<script"), "{body}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_leading_out_of_the_plan_folder_is_refused() {
    let f = fixture().await;
    let outside = crate::paths::canonical(&std::env::temp_dir()).unwrap().join(format!("thoughts-mado-outside-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(outside.join("screens")).unwrap();
    std::fs::write(outside.join("secret.html"), "<p>secret</p>").unwrap();
    std::fs::write(outside.join("screens/away.html"), "<p>secret</p>").unwrap();
    std::fs::write(outside.join("tokens.json"), good_tokens().to_string()).unwrap();
    f.put("flow.graph.json", &json!({"nodes": [{"id": "home", "label": "Home", "kind": "screen"}, {"id": "away", "label": "Away", "kind": "screen"}]}).to_string());

    // A mock-up that is a link to a file outside.
    std::fs::create_dir_all(f.mado.join("design/screens")).unwrap();
    std::os::unix::fs::symlink(outside.join("secret.html"), f.mado.join("design/screens/home.html")).unwrap();
    let (status, err) = answer(screen(f.s(), UrlPath((f.id.clone(), "home".into()))).await).await;
    assert_eq!((status, err["code"].as_str()), (403, Some("forbidden")));
    assert_eq!(answer(screens(f.s(), f.p()).await).await.1[0]["hasMock"], false);
    // Full access is the owner's exception: the guard yields, the cleaning does not.
    crate::full_access::set_on(true);
    let (status, body) = answer(screen(f.s(), UrlPath((f.id.clone(), "home".into()))).await).await;
    crate::full_access::set_on(false);
    assert_eq!((status, body["html"].as_str()), (200, Some("<body><p>secret</p></body>")));

    // A folder on the way that is a link out: nothing is read through it, nothing is written through it.
    std::fs::remove_dir_all(f.mado.join("design")).unwrap();
    std::os::unix::fs::symlink(&outside, f.mado.join("design")).unwrap();
    assert_eq!(answer(screen(f.s(), UrlPath((f.id.clone(), "away".into()))).await).await.0, 403);
    assert_eq!(answer(get_tokens(f.s(), f.p()).await).await.0, 403);
    let before = std::fs::read_to_string(outside.join("tokens.json")).unwrap();
    let mut mine = good_tokens();
    mine["radius"] = json!(3);
    assert_eq!(answer(put_tokens(f.s(), f.p(), Json(mine)).await).await.0, 403);
    assert_eq!(answer(pick(f.s(), f.p(), Json(json!({"id": "app-warm"}))).await).await.0, 403);
    assert_eq!(std::fs::read_to_string(outside.join("tokens.json")).unwrap(), before);
    assert!(!outside.join("picked.json").exists());
    let (status, body) = answer(list_styles(f.s(), f.p()).await).await;
    assert_eq!((status, body["styles"].as_array().unwrap().len()), (200, 8), "a styles file behind a link is not read");

    // The settings file itself a link out.
    std::fs::remove_file(f.mado.join("design")).unwrap();
    std::fs::create_dir_all(f.mado.join("design")).unwrap();
    std::os::unix::fs::symlink(outside.join("tokens.json"), f.mado.join("design/tokens.json")).unwrap();
    assert_eq!(answer(put_tokens(f.s(), f.p(), Json(good_tokens())).await).await.0, 403);
    assert_eq!(std::fs::read_to_string(outside.join("tokens.json")).unwrap(), before);

    // The plan folder itself a link out of the project.
    std::fs::remove_dir_all(&f.mado).unwrap();
    std::os::unix::fs::symlink(&outside, &f.mado).unwrap();
    for r in [list_styles(f.s(), f.p()).await, get_tokens(f.s(), f.p()).await, screens(f.s(), f.p()).await] {
        assert_eq!(answer(r).await.0, 403);
    }
    std::fs::remove_file(&f.mado).unwrap();
    std::fs::remove_dir_all(&outside).unwrap();
}

#[tokio::test]
async fn the_routes_are_behind_the_app_token_like_every_route() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let f = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = crate::api::router(f.st.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let get = |auth: &'static str, path: String| async move {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Connection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out).await;
        let out = String::from_utf8_lossy(&out).to_string();
        (out.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0), out)
    };
    let (status, body) = get("Authorization: Bearer test-token\r\n", format!("/api/mado/{}/design/styles", f.id)).await;
    assert!(status == 200 && body.contains("app-calm"), "{body}");
    assert_eq!(get("", format!("/api/mado/{}/design/styles", f.id)).await.0, 401);
    assert_eq!(get("Authorization: Bearer wrong\r\n", format!("/api/mado/{}/design/screens/home", f.id)).await.0, 401);
    // An encoded path in the id is still only an id.
    assert_eq!(get("Authorization: Bearer test-token\r\n", format!("/api/mado/{}/design/screens/..%2F..%2Fstate", f.id)).await.0, 422);
}
