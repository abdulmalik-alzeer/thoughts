//! Plugins (`bundles.rs`): claims over the shared files, switching off and on, removing, describing,
//! export and import (with "update replaces"), compatibility, format updates, and listing the packs
//! kept before plugins existed. Every layer here is a temp folder.

use super::*;
use crate::applayer::testutil::layer;
use crate::{staging, workflows};

const ID: &str = "p-abcdefghijkl";
/// This app's own version: what a plugin made here asks for at most.
const NOW: &str = env!("CARGO_PKG_VERSION");
const WEEKLY: &str = r#"{"name":"Weekly","trigger":{"type":"schedule","every":"week","at":"16:00","weekday":5},"prompt":"Write my weekly review."}"#;

fn w(dir: &Path, path: &str, text: &str) {
    if let Some(p) = dir.join(path).parent() { std::fs::create_dir_all(p).unwrap(); }
    std::fs::write(dir.join(path), text).unwrap();
}

fn read(dir: &Path, path: &str) -> String { std::fs::read_to_string(dir.join(path)).unwrap_or_default() }

fn key(file: &str, pointer: &str) -> Claim { Claim::Key { file: file.into(), pointer: pointer.into() } }
fn item(file: &str, pointer: &str, k: &str, v: &str) -> Claim { Claim::Item { file: file.into(), pointer: pointer.into(), matches: BTreeMap::from([(k.to_string(), v.to_string())]) } }
fn file(path: &str) -> Claim { Claim::File { path: path.into() } }

/// A plugin file as an export writes it: a feature pack with its plugin lines.
fn plugin_pack(name: &str, id: &str, version: &str, files: Value) -> Value {
    let map: BTreeMap<String, String> = files.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
    json!({"format": "thoughts-pack", "version": 1, "apiVersion": 1, "kind": "feature", "id": id, "name": name, "author": "Sara", "packVersion": version,
        "description": "A board of your items.", "fingerprint": packs::fingerprint("feature", &map), "files": files,
        "plugin": {"id": id, "name": name, "version": version, "minApp": NOW, "layerFormat": 1}})
}

fn kanban_files() -> Value {
    json!({
        "plugins/board.js": "export default () => { /* board */ }\n",
        "plugins/board.json": "{\"apiVersion\":1,\"permissions\":[\"ideas:read\"]}\n",
        "plugins/extra.js": "export default () => { /* extra */ }\n",
        "workflows/weekly.json": WEEKLY,
        "home.json": "{\"chips\":[{\"label\":\"Board\",\"prompt\":\"Open my board\"}],\"order\":[\"widget:board\"]}",
        "nav.json": "{\n  \"labels\": {\n    \"ideas\": {\n      \"en\": \"Clients\"\n    }\n  }\n}\n",
        "custom.css": ".board { gap: 8px }\n",
    })
}

fn kanban(version: &str) -> Value { plugin_pack("Kanban", ID, version, kanban_files()) }

/// A layer the user already made their own, in one entry.
async fn mine(name: &str) -> (Git<'static>, Dirs) {
    let (st, d) = layer(name).await;
    w(&d.layer, "home.json", "{\n  \"chips\": [\n    {\n      \"label\": \"Plan\",\n      \"prompt\": \"Plan my week: \"\n    }\n  ],\n  \"hidden\": [\n    \"stats\"\n  ],\n  \"order\": [\n    \"todos\"\n  ]\n}\n");
    w(&d.layer, "custom.css", "/* mine */\n:root { --radius: 2px }\n");
    w(&d.layer, "plugins/mine.js", "export default () => { /* mine */ }\n");
    w(&d.layer, "plugins/mine.json", "{\"apiVersion\":1,\"permissions\":[\"storage\"]}\n");
    st.commit_all(&d.layer, "my layer").await.unwrap();
    (st, d)
}

/// Stage a pack and keep it, as the review does. → the kept change.
async fn keep_pack(st: Git<'_>, d: &Dirs, pack: &Value) -> Value {
    packs::import(st, d, pack, false).await.unwrap();
    keep(st, d).await
}

async fn keep(st: Git<'_>, d: &Dirs) -> Value {
    let p = staging::pending(st, d, false).await;
    assert_eq!(p["problems"], json!([]), "{p}");
    staging::apply(st, d, false, p["signature"].as_str()).await.unwrap()
}

/// Every customisation file of the layer but the two the app's own switches write.
fn files_of(d: &Dirs) -> BTreeMap<String, String> { packs::collect(&d.layer) }

fn listed_item(d: &Dirs, id: &str) -> Value { list(d, false).into_iter().find(|p| p["id"] == id).unwrap_or(Value::Null) }

async fn count(st: Git<'_>, d: &Dirs) -> usize { applayer::history_in(st, &d.layer).await.len() }

fn done(d: &Dirs) { std::fs::remove_dir_all(&d.home).unwrap(); }

/// Lists compared whatever their order (a merge puts the user's items first).
fn sorted(v: &Value) -> Value {
    match v {
        Value::Array(a) => { let mut a: Vec<Value> = a.iter().map(sorted).collect(); a.sort_by_key(|x| x.to_string()); Value::Array(a) }
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), sorted(v))).collect()),
        other => other.clone(),
    }
}

#[test]
fn what_is_extracted_merges_back_into_the_same_file() {
    let samples = [
        ("home.json", json!({"chips": [{"label": "Plan", "prompt": "Plan my week: "}, {"label": "Board", "prompt": "Open"}], "hidden": ["stats"], "order": ["todos", "widget:board"]})),
        ("nav.json", json!({"order": ["todos", "home"], "hidden": ["graph"], "labels": {"ideas": {"en": "Clients", "ar": "العملاء"}, "docs": {"en": "Files"}},
            "icons": {"ideas": "folder"}, "groups": [{"id": "work", "label": {"en": "Work"}, "items": ["todos", "ideas"]}], "start": "todos"})),
        ("pages.json", json!({"pages": {"home": {"hide": ["stats"], "texts": {"title": {"en": "Start"}}}, "todos": {"replace": "views/board.html"}}})),
        ("todos.json", json!({"groupBy": "due", "sort": "due", "showDone": true})),
        ("themes.json", json!([{"id": "sand", "name": "Sand"}, {"id": "sea", "name": "Sea"}])),
        ("keys.json", json!({"bindings": {"chat.new": ["Mod+KeyJ"], "palette": [], "plugin:hours.open": ["Mod+Alt+KeyH"]}})),
    ];
    for (file, v) in &samples {
        let all = claims_of_partial(file, v);
        assert!(!all.is_empty(), "{file}");
        assert!(all.iter().all(Claim::valid), "{file}: {all:?}");
        // Everything the file holds is claimed, and what the claims hold is the file.
        assert_eq!(&extract(v, &all), v, "{file}");
        let empty = if v.is_array() { json!([]) } else { json!({}) };
        assert_eq!(&packs::merge_json(&empty, &extract(v, &all)), v, "{file}");
        // Any part of it taken out and merged back gives the same members.
        for c in &all {
            let (part, rest) = (extract(v, std::slice::from_ref(c)), subtract(v, std::slice::from_ref(c)));
            assert_ne!(&rest, v, "{file}: {c:?} takes something out");
            assert_ne!(extract(&rest, std::slice::from_ref(c)), part, "{file}: {c:?} is gone after it is taken out");
            assert_eq!(sorted(&packs::merge_json(&rest, &part)), sorted(v), "{file}: {c:?}");
        }
    }
    // The shapes of the claims: members down to two levels, list items by id, label or their text.
    assert_eq!(claims_of_partial("home.json", &samples[0].1), vec![
        item("home.json", "/chips", "label", "Board"), item("home.json", "/chips", "label", "Plan"),
        item("home.json", "/hidden", "value", "stats"), item("home.json", "/order", "value", "todos"), item("home.json", "/order", "value", "widget:board")]);
    let nav = claims_of_partial("nav.json", &samples[1].1);
    for c in [key("nav.json", "/labels/ideas"), key("nav.json", "/labels/docs"), key("nav.json", "/icons/ideas"), key("nav.json", "/start"), item("nav.json", "/groups", "id", "work")] { assert!(nav.contains(&c), "{c:?}"); }
    assert_eq!(claims_of_partial("themes.json", &samples[4].1), vec![item("themes.json", "", "id", "sand"), item("themes.json", "", "id", "sea")]);
    // A file's own bookkeeping is nobody's; a list of things that cannot be told apart is one member.
    assert_eq!(claims_of_partial("keys.json", &json!({"format": 1, "bindings": {"chat.new": ["Mod+KeyJ"]}})), vec![key("keys.json", "/bindings/chat.new")]);
    assert_eq!(claims_of_partial("manifest.json", &json!({"apiVersion": 1, "plugins": {"a.js": {"permissions": []}}})), vec![key("manifest.json", "/plugins/a.js")]);
    assert_eq!(claims_of_partial("home.json", &json!({"rows": [[1, 2], [3]]})), vec![key("home.json", "/rows")]);
    assert_eq!(claims_of_partial("home.json", &json!({"chips": [], "hidden": [], "order": []})), vec![], "the file as the app ships it is nobody's");
    // Pointers are RFC 6901: a slash or a tilde in a name is written out.
    assert_eq!(claims_of_partial("nav.json", &json!({"labels": {"a/b~c": {"en": "X"}}})), vec![key("nav.json", "/labels/a~1b~0c")]);
    assert_eq!(extract(&json!({"labels": {"a/b~c": 1}}), &[key("nav.json", "/labels/a~1b~0c")]), json!({"labels": {"a/b~c": 1}}));
}

#[test]
fn taking_claims_out_leaves_everything_else_exactly_as_it_was() {
    let v = json!({"chips": [{"label": "Plan", "prompt": "Plan my week: "}, {"label": "Board", "prompt": "Open"}], "hidden": ["stats"], "order": ["todos", "widget:board"], "mine": {"keep": true, "drop": 1}});
    let claims = [item("home.json", "/chips", "label", "Board"), item("home.json", "/order", "value", "widget:board"), key("home.json", "/mine/drop"), key("home.json", "/absent"), item("home.json", "/nothing", "id", "x")];
    let left = subtract(&v, &claims);
    let expected = json!({"chips": [{"label": "Plan", "prompt": "Plan my week: "}], "hidden": ["stats"], "order": ["todos"], "mine": {"keep": true}});
    assert_eq!(serde_json::to_string_pretty(&left).unwrap(), serde_json::to_string_pretty(&expected).unwrap(), "byte for byte once written again");
    assert_eq!(extract(&v, &claims), json!({"chips": [{"label": "Board", "prompt": "Open"}], "order": ["widget:board"], "mine": {"drop": 1}}));
    // By id, by label and by its own text; a claim never matches by another field.
    let themes = json!([{"id": "sand", "name": "Sand", "label": "x"}, {"id": "sea", "name": "Sand"}]);
    assert_eq!(subtract(&themes, &[item("themes.json", "", "id", "sand")]), json!([{"id": "sea", "name": "Sand"}]));
    assert_eq!(subtract(&themes, &[item("themes.json", "", "label", "x")]), json!([{"id": "sea", "name": "Sand"}]));
    assert_eq!(subtract(&themes, &[item("themes.json", "", "value", "sand")]), themes);
    assert_eq!(subtract(&themes, &[item("themes.json", "", "name", "Sand")]), themes, "only id, label and value are ever matched");
    assert!(!item("themes.json", "", "name", "Sand").valid() && !key("bundles.json", "/off").valid() && !key("addons.json", "/off").valid() && !file("../x.js").valid() && !file("custom.css").valid());
    assert!(!Claim::Css { block: "pack:x */ body{}".into() }.valid() && !Claim::Css { block: "other:x".into() }.valid() && Claim::Css { block: format!("plugin:{ID}") }.valid());
}

#[test]
fn marked_blocks_of_the_style_sheet_are_found_and_cut_out_cleanly() {
    let css = format!("/* mine */\n:root {{ --radius: 2px }}\n\n/* pack:Kanban pabc123 */\n.board {{ gap: 8px }}\n/* end pack:Kanban pabc123 */\n\nbody {{ margin: 0 }}\n/* plugin:{ID} */\n.x {{ }}\n/* end plugin:{ID} */\n/* pack:Open */\n.never {{ }}\n");
    let blocks = css_blocks(&css);
    assert_eq!(blocks.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["pack:Kanban pabc123", &format!("plugin:{ID}")], "a block that is never closed is not one");
    assert_eq!(block_body(&css[blocks[0].1.clone()]), ".board { gap: 8px }");
    let without = css_without(&css, &BTreeSet::from(["pack:Kanban pabc123".to_string()]));
    assert_eq!(without, format!("/* mine */\n:root {{ --radius: 2px }}\n\nbody {{ margin: 0 }}\n/* plugin:{ID} */\n.x {{ }}\n/* end plugin:{ID} */\n/* pack:Open */\n.never {{ }}\n"));
    assert_eq!(css_only(&css, &BTreeSet::from([format!("plugin:{ID}")])), format!("/* plugin:{ID} */\n.x {{ }}\n/* end plugin:{ID} */\n"));
    // What `packs::merge_css` writes is what is found here.
    let merged = packs::merge_css("a{}\n", ".b{}", &packs::css_block_key("Kanban", Some(ID)));
    assert_eq!(css_blocks(&merged)[0].0, format!("pack:{}", packs::marker_name(&packs::css_block_key("Kanban", Some(ID)))));
    assert_eq!(css_without(&merged, &css_blocks(&merged).into_iter().map(|(k, _)| k).collect()), "a{}\n");
    // Your own rules are marked where they stand, and nothing else moves.
    let marked = mark_look("a{}\n\n/* pack:P */\n.p{}\n/* end pack:P */\nb{}\n", ID);
    assert_eq!(marked, format!("/* plugin:{ID} */\na{{}}\n/* end plugin:{ID} */\n/* pack:P */\n.p{{}}\n/* end pack:P */\n/* plugin:{ID} */\nb{{}}\n/* end plugin:{ID} */\n"));
    assert!(!has_look("/* Your overrides. */\n") && !has_look(" \n") && has_look("a{}"));
}

#[test]
fn ids_and_versions_have_one_exact_shape() {
    assert!(valid_id(ID) && valid_id(&new_id()) && valid_id(&id_of("x")));
    assert_eq!(id_of("same"), id_of("same"));
    assert_ne!(id_of("one"), id_of("two"));
    for bad in ["", "p-", "p-ABCDEFGHIJKL", "p-abcdefghijk1", "p-abcdefghijklm", "x-abcdefghijkl", "p-abcdefghij/l", "../../x"] { assert!(!valid_id(bad), "{bad}"); }
    for ok in ["home", "sidebar", "todos", "pages", "keys", "look", "addon:plugins/a.js", "page:views/b.html", "task:workflows/w.json", "theme:sand"] { assert!(valid_loose(ok), "{ok}"); }
    for bad in ["addon:views/b.html", "addon:plugins/../x.js", "page:plugins/a.js", "task:workflows/a/b.json", "task:plugins/a.json", "theme:", "theme:a\u{202e}b", "bundles", "addon:plugins/a.json"] { assert!(!valid_loose(bad), "{bad}"); }
    assert_eq!((semver("0.5.0"), semver("1.2"), semver("1.2.3-beta"), semver("01.2.3"), semver("1..3")), (Some((0, 5, 0)), None, None, Some((1, 2, 3)), None));
    assert!(needs_newer_app("9.0.0") && !needs_newer_app("0.0.1") && !needs_newer_app(env!("CARGO_PKG_VERSION")) && !needs_newer_app(""));
    assert_eq!((clean_version("1.2.0"), clean_version("1.0 beta"), clean_version("v2+build-3"), clean_version(&"1".repeat(21))), ("1.2.0".to_string(), String::new(), "v2+build-3".to_string(), String::new()));
    assert!(older("1.0", "1.2") && older("1.9.0", "1.10") && !older("1.2", "1.2.0") && !older("2", "1.9") && !older("beta", "1.0"));
}

/// A plugin from a file: listed as one named thing, switched off and on as a whole (one History
/// entry each time, undoable), with nothing of the user's own touched.
#[tokio::test]
async fn a_plugin_is_switched_off_and_on_as_one_thing() {
    let (st, d) = mine("switch").await;
    let kept = keep_pack(st, &d, &kanban("1.0.0")).await;
    assert_eq!(kept["plugin"], json!({"id": ID, "name": "Kanban"}));
    let weekly = workflows::load_files(&d.layer).remove(0);
    workflows::approve(&d.layer, &d.home, &weekly.file, &weekly.hash).unwrap();
    // One named plugin that groups its things; the user's own add-on and Home stay loose.
    let all = list(&d, false);
    let ids: Vec<&str> = all.iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [ID, "addon:plugins/mine.js", "home", "look"], "{all:?}");
    let p = &all[0];
    assert_eq!((&p["name"], &p["version"], &p["author"], &p["origin"], &p["state"], &p["on"], &p["named"]), (&json!("Kanban"), &json!("1.0.0"), &json!("Sara"), &json!("pack"), &json!("ok"), &json!(true), &json!(true)));
    assert_eq!(p["permissions"], json!(["ideas:read"]));
    assert_eq!(p["permissionWords"], json!(["read your ideas"]));
    assert_eq!(p["tasks"], json!([{"file": "weekly.json", "name": "Weekly", "approved": true}]));
    let kinds: BTreeSet<&str> = p["contains"].as_array().unwrap().iter().map(|c| c["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, BTreeSet::from(["addon", "task", "home", "sidebar", "css"]));
    assert_eq!(listed_item(&d, "home")["contains"].as_array().unwrap().len(), 3, "the user's chip, hidden box and order entry");

    let before = applayer::manifest_of(&d.layer);
    let (files, entries) = (files_of(&d), count(st, &d).await);
    let hash = switch_in(st, &d, ID, false).await.unwrap();
    assert!(!hash.is_empty());
    assert_eq!(switch_in(st, &d, ID, false).await.unwrap(), "", "already off: nothing recorded");
    assert_eq!(count(st, &d).await, entries + 1, "one entry");
    let h = applayer::history_in(st, &d.layer).await;
    assert_eq!((h[0]["summary"].as_str(), h[0]["entry"].as_str(), h[0]["undoable"].as_bool()), (Some("Switched off the plugin \u{201c}Kanban\u{201d}"), Some("plugin-off"), Some(true)));
    assert_eq!(applayer::history_page_in(st, &d.layer, 0, 1, true).await[0]["summary"], "إيقاف الملحق «Kanban»");
    assert_eq!(files_of(&d), files, "nothing in the layer changed but the two switch files");
    // Off: nothing of it is handed to the UI, and its automatic task does not run.
    let off = applayer::manifest_of(&d.layer);
    assert_eq!(off["plugins"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect::<Vec<_>>(), ["mine.js"]);
    assert_eq!(off["home"], json!({"chips": [{"label": "Plan", "prompt": "Plan my week: "}], "hidden": ["stats"], "order": ["todos"]}));
    assert_eq!((&off["nav"], &off["workflows"]), (&Value::Null, &json!([])));
    assert_eq!(off["css"], "/* mine */\n:root { --radius: 2px }\n");
    assert!(workflows::approved_workflows(&d.layer, &d.home).is_empty());
    assert_eq!((&listed_item(&d, ID)["state"], &listed_item(&d, ID)["on"]), (&json!("off"), &json!(false)));
    // A change being tried keeps it off too (the list is the kept layer's).
    assert_eq!(applayer::manifest_preview(&d.layer, &d.layer, false)["plugins"].as_array().unwrap().len(), 1);

    switch_in(st, &d, ID, true).await.unwrap();
    assert_eq!(applayer::manifest_of(&d.layer), before, "back on: exactly as before");
    assert_eq!(workflows::approved_workflows(&d.layer, &d.home).len(), 1);
    assert_eq!(applayer::history_in(st, &d.layer).await[0]["summary"], "Switched on the plugin \u{201c}Kanban\u{201d}");
    // Undoing the switch-off entry itself works whatever came after, and undoing that undo too.
    switch_in(st, &d, ID, false).await.unwrap();
    switch_in(st, &d, "home", false).await.unwrap();
    let off_entry = applayer::history_in(st, &d.layer).await[1]["hash"].as_str().unwrap().to_string();
    applayer::revert_in(st, &d.layer, &off_entry).await.unwrap();
    let f = load(&d.layer);
    assert_eq!(f.off, BTreeSet::from(["home".to_string()]), "only that switch was undone");
    assert!(applayer::switched_off(&d.layer).is_empty());
    assert_eq!(applayer::manifest_of(&d.layer)["plugins"].as_array().unwrap().len(), 3);
    assert!(switch_in(st, &d, "p-zzzzzzzzzzzz", false).await.is_err() && switch_in(st, &d, "../x", false).await.is_err() && switch_in(st, &d, "theme:none", false).await.is_err());
    done(&d);
}

#[tokio::test]
async fn loose_plugins_are_switched_without_any_record_of_their_own() {
    let (st, d) = mine("loose").await;
    w(&d.layer, "themes.json", "[\n  {\n    \"id\": \"sand\",\n    \"name\": \"Sand\"\n  },\n  {\n    \"id\": \"sea\",\n    \"name\": \"Sea\"\n  }\n]\n");
    w(&d.layer, "workflows/weekly.json", WEEKLY);
    w(&d.layer, "todos.json", "{\"groupBy\":\"due\"}\n");
    w(&d.layer, "keys.json", "{\"format\":1,\"bindings\":{\"chat.new\":[\"Mod+KeyJ\"]}}\n");
    w(&d.layer, "views/cal.html", "<p>cal</p>");
    st.commit_all(&d.layer, "more of mine").await.unwrap();
    let weekly = workflows::load_files(&d.layer).remove(0);
    workflows::approve(&d.layer, &d.home, &weekly.file, &weekly.hash).unwrap();
    let ids: Vec<String> = list(&d, false).iter().map(|p| p["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids, ["addon:plugins/mine.js", "page:views/cal.html", "task:workflows/weekly.json", "theme:sand", "theme:sea", "home", "todos", "keys", "look"]);
    assert_eq!(list(&d, true).iter().map(|p| p["name"].as_str().unwrap().to_string()).filter(|n| n.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c))).count(), 4, "the app's own names are in the UI language");
    assert!(!d.layer.join(FILE).exists(), "no record is needed to list them");
    let before = applayer::manifest_of(&d.layer);
    let files = files_of(&d);

    // A theme on its own.
    let entries = count(st, &d).await;
    switch_in(st, &d, "theme:sand", false).await.unwrap();
    assert_eq!(applayer::manifest_lang(&d.layer, false)["themes"], json!([{"id": "sea", "name": "Sea"}]));
    assert_eq!((count(st, &d).await, listed_item(&d, "theme:sand")["on"].clone()), (entries + 1, json!(false)));
    assert_eq!(applayer::history_in(st, &d.layer).await[0]["summary"], "Switched off the plugin \u{201c}Sand\u{201d}");
    // Home, the Todos page, the keyboard shortcuts, your look, an automatic task.
    for id in ["home", "todos", "keys", "look", "task:workflows/weekly.json"] { assert!(!switch_in(st, &d, id, false).await.unwrap().is_empty(), "{id}"); }
    let m = applayer::manifest_of(&d.layer);
    assert_eq!(m["home"], json!({"chips": [], "hidden": [], "order": []}));
    assert_eq!((&m["todos"], &m["css"], &m["workflows"]), (&Value::Null, &json!(""), &json!([])));
    assert_eq!(m["keys"], json!({"format": 1}), "the file's own bookkeeping is not a customisation");
    assert!(workflows::approved_workflows(&d.layer, &d.home).is_empty());
    // An add-on or a page is switched in the list the app already kept (0.4.x reads it the same way).
    switch_in(st, &d, "page:views/cal.html", false).await.unwrap();
    assert_eq!(applayer::switched_off(&d.layer), BTreeSet::from(["views/cal.html".to_string()]));
    assert_eq!(applayer::history_in(st, &d.layer).await[0]["summary"], "Switched off the page \u{201c}cal\u{201d}");
    assert_eq!(files_of(&d), files, "no customisation file was changed by any switch");
    for id in ["home", "todos", "keys", "look", "task:workflows/weekly.json", "theme:sand", "page:views/cal.html"] { switch_in(st, &d, id, true).await.unwrap(); }
    assert_eq!(applayer::manifest_of(&d.layer), before, "everything back on: exactly as before");
    assert_eq!(workflows::approved_workflows(&d.layer, &d.home).len(), 1);

    // Removing a loose one takes only what is its own.
    let entries = count(st, &d).await;
    let hash = remove_in(st, &d, "theme:sand").await.unwrap();
    assert_eq!(serde_json::from_str::<Value>(&read(&d.layer, "themes.json")).unwrap(), json!([{"id": "sea", "name": "Sea"}]));
    remove_in(st, &d, "todos").await.unwrap();
    assert!(!d.layer.join("todos.json").exists(), "a file with nothing left in it goes");
    remove_in(st, &d, "home").await.unwrap();
    assert_eq!(serde_json::from_str::<Value>(&read(&d.layer, "home.json")).unwrap(), json!({"chips": [], "hidden": [], "order": []}), "a file the app ships stays");
    remove_in(st, &d, "task:workflows/weekly.json").await.unwrap();
    remove_in(st, &d, "look").await.unwrap();
    assert_eq!(read(&d.layer, "custom.css"), "");
    remove_in(st, &d, "addon:plugins/mine.js").await.unwrap();
    assert!(!d.layer.join("plugins/mine.js").exists() && !d.layer.join("plugins/mine.json").exists() && !d.layer.join("workflows/weekly.json").exists());
    assert_eq!(count(st, &d).await, entries + 6);
    applayer::revert_in(st, &d.layer, &hash).await.unwrap();
    assert_eq!(applayer::manifest_of(&d.layer)["themes"].as_array().unwrap().len(), 2, "undone on its own");
    assert!(remove_in(st, &d, "todos").await.is_err(), "nothing of it is left to remove");
    done(&d);
}

#[tokio::test]
async fn removing_a_plugin_takes_its_things_and_keeps_what_it_saved() {
    let (st, d) = mine("remove").await;
    keep_pack(st, &d, &kanban("1.0.0")).await;
    crate::layer_api::storage_write(&d.home, "board.js", br#"{"cards":3}"#, false).unwrap();
    switch_in(st, &d, ID, false).await.unwrap();
    let (mine_files, entries) = (["custom.css", "home.json", "plugins/mine.js", "plugins/mine.json"], count(st, &d).await);
    let hash = remove_in(st, &d, ID).await.unwrap();
    assert_eq!(count(st, &d).await, entries + 1);
    assert_eq!(applayer::history_in(st, &d.layer).await[0]["summary"], "Removed the plugin \u{201c}Kanban\u{201d}");
    let left = files_of(&d);
    assert_eq!(left.keys().filter(|k| !["manifest.json", "themes.json"].contains(&k.as_str())).map(String::as_str).collect::<Vec<_>>(), mine_files, "{:?}", left.keys());
    assert_eq!(left["custom.css"], "/* mine */\n:root { --radius: 2px }\n", "its block is gone, the user's lines are untouched");
    assert_eq!(serde_json::from_str::<Value>(&left["home.json"]).unwrap(), json!({"chips": [{"label": "Plan", "prompt": "Plan my week: "}], "hidden": ["stats"], "order": ["todos"]}));
    assert!(load(&d.layer).bundles.is_empty() && load(&d.layer).off.is_empty() && applayer::switched_off(&d.layer).is_empty());
    assert_eq!(crate::layer_api::storage_read(&d.home, "board.js").unwrap(), json!({"cards": 3}), "what its add-on saved is kept");
    assert!(list(&d, false).iter().all(|p| p["id"] != ID));
    // Undo brings every file and the record back, switched off as it was.
    applayer::revert_in(st, &d.layer, &hash).await.unwrap();
    let f = load(&d.layer);
    assert!(d.layer.join("plugins/board.js").exists() && d.layer.join("workflows/weekly.json").exists() && read(&d.layer, "custom.css").contains(".board { gap: 8px }"));
    assert_eq!((f.bundles[ID].name.as_str(), f.off.contains(ID), f.bundles[ID].claims.len()), ("Kanban", true, 8));
    assert_eq!(applayer::switched_off(&d.layer), BTreeSet::from(["plugins/board.js".to_string(), "plugins/extra.js".to_string()]));
    done(&d);
}

#[tokio::test]
async fn naming_a_loose_item_makes_it_a_plugin_that_can_be_exported() {
    let (st, d) = mine("describe").await;
    let entries = count(st, &d).await;
    let who = |id: &str, name: &str| Describe { id: id.into(), name: name.into(), version: String::new(), description: "Mine.".into(), author: "Sara\u{202e}".into() };
    assert!(describe_in(st, &d, &who("addon:plugins/mine.js", "")).await.unwrap_err().to_string().contains("name"));
    assert!(describe_in(st, &d, &who("addon:plugins/none.js", "X")).await.is_err());
    let (hash, id) = describe_in(st, &d, &who("addon:plugins/mine.js", "My \u{201c}tool\u{201d}")).await.unwrap();
    assert!(valid_id(&id) && !hash.is_empty());
    let b = load(&d.layer).bundles[&id].clone();
    assert_eq!((b.name.as_str(), b.version.as_str(), b.origin.as_str(), b.author.as_str()), ("My tool", "1.0.0", "made", "Sara[U+202E]"));
    assert_eq!(b.claims, vec![file("plugins/mine.js"), file("plugins/mine.json")]);
    assert_eq!(applayer::history_in(st, &d.layer).await[0]["summary"], "Described the plugin \u{201c}My tool\u{201d}");
    assert!(list(&d, false).iter().all(|p| p["id"] != "addon:plugins/mine.js"), "it is no longer listed loose");
    // Described again: the same plugin, renamed and given a version; its id stays.
    let (_, again) = describe_in(st, &d, &Describe { id: id.clone(), name: "Tool".into(), version: "1.1.0".into(), ..Default::default() }).await.unwrap();
    assert_eq!((again.as_str(), load(&d.layer).bundles[&id].version.as_str()), (id.as_str(), "1.1.0"));
    // Home and your look: what is unclaimed becomes the plugin's; the look is marked where it stands.
    let (_, home) = describe_in(st, &d, &who("home", "My Home")).await.unwrap();
    let (_, look) = describe_in(st, &d, &who("look", "My look")).await.unwrap();
    assert_eq!(load(&d.layer).bundles[&home].claims.len(), 3);
    assert_eq!(read(&d.layer, "custom.css"), format!("/* plugin:{look} */\n/* mine */\n:root {{ --radius: 2px }}\n/* end plugin:{look} */\n"));
    assert_eq!(count(st, &d).await, entries + 4);
    assert!(list(&d, false).iter().all(|p| p["named"] == true), "{:?}", list(&d, false));
    // Each exports as a plugin file 0.4.x reads as a feature pack, with a fingerprint that is right.
    let pack = export_in(&d, &id).unwrap();
    let files = packs::check_pack(&pack).unwrap();
    assert_eq!(files.keys().map(String::as_str).collect::<Vec<_>>(), ["plugins/mine.js", "plugins/mine.json"]);
    assert_eq!((&pack["kind"], &pack["id"], &pack["name"], &pack["packVersion"], pack["fingerprint"].as_str()), (&json!("feature"), &json!(id), &json!("Tool"), &json!("1.1.0"), Some(packs::fingerprint("feature", &files).as_str())));
    assert_eq!(pack["plugin"], json!({"id": id, "name": "Tool", "version": "1.1.0", "minApp": first_app(), "layerFormat": 1}));
    assert_eq!(packs::check_pack(&export_in(&d, &look).unwrap()).unwrap()["custom.css"], "/* mine */\n:root { --radius: 2px }\n");
    assert_eq!(serde_json::from_str::<Value>(&packs::check_pack(&export_in(&d, &home).unwrap()).unwrap()["home.json"]).unwrap(), json!({"chips": [{"label": "Plan", "prompt": "Plan my week: "}], "hidden": ["stats"], "order": ["todos"]}));
    assert!(export_in(&d, "home").is_err() && export_in(&d, "p-zzzzzzzzzzzz").is_err());
    done(&d);
}

/// One computer to the next: the exported file imports through the usual review and gives the same
/// plugin (same id, same claims), and exporting it again there gives the same fingerprint.
#[tokio::test]
async fn an_exported_plugin_imports_as_the_same_plugin_elsewhere() {
    let (st, a) = mine("export-a").await;
    keep_pack(st, &a, &kanban("1.2.0")).await;
    let first = export_in(&a, ID).unwrap();
    assert_eq!(first["plugin"]["version"], "1.2.0");
    assert_eq!(packs::check_pack(&first).unwrap().keys().map(String::as_str).collect::<Vec<_>>(),
        ["custom.css", "home.json", "nav.json", "plugins/board.js", "plugins/board.json", "plugins/extra.js", "workflows/weekly.json"]);

    let (st2, b) = layer("export-b").await;
    let look = packs::inspect(&first, &b.layer, &[]).unwrap();
    assert_eq!(look["plugin"], json!({"id": ID, "name": "Kanban", "version": "1.2.0", "minApp": NOW, "layerFormat": 1, "compatible": true, "why": null, "replaces": null, "olderThanYours": false, "migrates": false}));
    assert_eq!(look["matches"], true);
    packs::import(st2, &b, &first, false).await.unwrap();
    let p = staging::pending(st2, &b, false).await;
    assert_eq!((&p["origin"], &p["plugin"]["target"], &p["plugin"]["id"], &p["plugin"]["name"]), (&json!("import"), &json!("new"), &json!(ID), &json!("Kanban")));
    assert!(!b.layer.join("plugins/board.js").exists() && !b.layer.join(FILE).exists(), "staged only: nothing is in the app yet");
    assert!(p["workflows"].as_array().unwrap().len() == 1, "its automatic task is shown for its own approval");
    keep(st2, &b).await;
    assert!(workflows::approved_workflows(&b.layer, &b.home).is_empty(), "an imported task never runs before the user allows it");
    let (fa, fb) = (load(&a.layer), load(&b.layer));
    assert_eq!((fb.bundles[ID].name.as_str(), fb.bundles[ID].version.as_str(), fb.bundles[ID].author.as_str(), fb.bundles[ID].origin.as_str()), ("Kanban", "1.2.0", "Sara", "pack"));
    assert_eq!(fb.bundles[ID].fingerprint, first["fingerprint"].as_str().unwrap());
    assert_eq!(fa.bundles[ID].claims, fb.bundles[ID].claims, "the same claims on both computers");
    let again = export_in(&b, ID).unwrap();
    assert_eq!(again["fingerprint"], first["fingerprint"], "the same content, so the same fingerprint");
    assert_eq!(listed_item(&b, ID)["contains"], listed_item(&a, ID)["contains"]);
    let _ = st;
    done(&a);
    done(&b);
}

#[tokio::test]
async fn a_newer_version_replaces_the_older_one_and_what_it_dropped_goes() {
    let (st, d) = mine("update").await;
    keep_pack(st, &d, &kanban("1.0.0")).await;
    // 1.1 no longer has the extra add-on nor the sidebar label, and its Home shortcut is renamed.
    let mut files = kanban_files();
    let o = files.as_object_mut().unwrap();
    o.remove("plugins/extra.js");
    o.remove("nav.json");
    o.insert("home.json".into(), json!("{\"chips\":[{\"label\":\"Kanban\",\"prompt\":\"Open my board\"}],\"order\":[\"widget:board\"]}"));
    let newer = plugin_pack("Kanban", ID, "1.1.0", files);
    let look = packs::inspect(&newer, &d.layer, &[]).unwrap();
    assert_eq!((&look["plugin"]["replaces"], &look["plugin"]["olderThanYours"]), (&json!({"id": ID, "version": "1.0.0", "name": "Kanban"}), &json!(false)));
    assert_eq!(packs::inspect(&plugin_pack("Kanban", ID, "0.9", kanban_files()), &d.layer, &[]).unwrap()["plugin"]["olderThanYours"], true);
    packs::import(st, &d, &newer, false).await.unwrap();
    assert!(!d.staging.join("plugins/extra.js").exists() && !d.staging.join("nav.json").exists(), "what 1.1 dropped is taken out in the staged copy");
    let p = staging::pending(st, &d, false).await;
    let status = |path: &str| p["files"].as_array().unwrap().iter().find(|f| f["path"] == path).map(|f| f["status"].as_str().unwrap().to_string());
    assert_eq!((status("plugins/extra.js").as_deref(), status("nav.json").as_deref(), status("home.json").as_deref(), status("plugins/board.js"), status("plugins/mine.js")), (Some("deleted"), Some("deleted"), Some("modified"), None, None));
    assert_eq!(p["plugin"], json!({"target": "existing", "id": ID, "name": "Kanban", "version": "1.1.0", "suggested": true, "replaces": {"id": ID, "version": "1.0.0"}, "takes": [], "migrated": false}));
    assert!(d.layer.join("plugins/extra.js").exists(), "the app itself is untouched until Keep");
    keep(st, &d).await;
    let b = load(&d.layer).bundles[ID].clone();
    assert_eq!((b.version.as_str(), b.claims.contains(&file("plugins/extra.js")), b.claims.contains(&item("home.json", "/chips", "label", "Kanban"))), ("1.1.0", false, true));
    assert_eq!(serde_json::from_str::<Value>(&read(&d.layer, "home.json")).unwrap()["chips"], json!([{"label": "Plan", "prompt": "Plan my week: "}, {"label": "Kanban", "prompt": "Open my board"}]), "the user's own chip stays, the old one of the plugin goes");
    assert_eq!(load(&d.layer).bundles.len(), 1);
    // Undoing the update brings 1.0 back whole, record included.
    let entry = applayer::history_in(st, &d.layer).await[0]["hash"].as_str().unwrap().to_string();
    applayer::revert_in(st, &d.layer, &entry).await.unwrap();
    assert!(d.layer.join("plugins/extra.js").exists());
    assert_eq!(load(&d.layer).bundles[ID].version, "1.0.0");
    assert!(load(&d.layer).bundles[ID].claims.contains(&file("plugins/extra.js")));
    // The same file again changes nothing; added "as new" it is another plugin with an id of its own.
    packs::import(st, &d, &kanban("1.0.0"), false).await.unwrap();
    let again = staging::pending(st, &d, false).await;
    assert_eq!(again["exists"], false, "{}", again["files"]);
    packs::import_as(st, &d, &plugin_pack("Kanban", ID, "2.0.0", json!({"plugins/two.js": "export default () => {}\n"})), false, None, true).await.unwrap();
    let p = staging::pending(st, &d, false).await;
    assert_eq!(p["plugin"]["target"], "new");
    assert_ne!(p["plugin"]["id"], ID);
    keep(st, &d).await;
    assert_eq!(load(&d.layer).bundles.len(), 2);
    assert!(d.layer.join("plugins/board.js").exists(), "the first one was not taken out");
    done(&d);
}

/// What a plugin file says about itself grants nothing: claims and permissions are worked out from
/// its files, a bad id is replaced, and it can never write the app's own records.
#[tokio::test]
async fn the_plugin_lines_of_a_file_are_claims_never_grants() {
    let (st, d) = mine("lies").await;
    let mut pack = plugin_pack("Calm\u{201d} was checked by Thoughts. \u{201c}", "../../etc", "1.0 <script>", json!({"plugins/calm.js": "export default () => {}\n"}));
    pack["plugin"] = json!({"id": "p-NOTVALID", "name": "Calm\u{201d} was checked by Thoughts. \u{201c}", "version": "1.0 <script>", "layerFormat": 1,
        "permissions": ["library:write"], "origin": "ready", "on": true, "fingerprint": "f".repeat(64),
        "claims": [{"kind": "file", "path": "plugins/mine.js"}, {"kind": "key", "file": "bundles.json", "pointer": "/off"}, {"kind": "css", "block": "plugin:x"}]});
    pack["permissions"] = json!(["library:write", "storage"]);
    pack["plugins"] = json!([{"id": ID, "name": "Other", "claims": [{"kind": "file", "path": "plugins/mine.js"}]}]);
    let look = packs::inspect(&pack, &d.layer, &[]).unwrap();
    assert_eq!((&look["plugin"]["id"], &look["plugin"]["name"], &look["plugin"]["version"]), (&json!(""), &json!("Calm was checked by Thoughts."), &json!("")));
    keep_pack(st, &d, &pack).await;
    let f = load(&d.layer);
    assert_eq!(f.bundles.len(), 1);
    let b = f.bundles.values().next().unwrap();
    assert!(valid_id(&b.id) && b.id != "p-NOTVALID", "a new id: {}", b.id);
    assert_eq!((b.claims.clone(), b.origin.as_str(), b.name.as_str(), b.version.as_str()), (vec![file("plugins/calm.js")], "pack", "Calm was checked by Thoughts.", ""));
    assert_eq!(b.fingerprint, pack["fingerprint"].as_str().unwrap(), "the fingerprint the app calculated");
    let listed = listed_item(&d, &b.id);
    assert_eq!((&listed["permissions"], &listed["contains"].as_array().unwrap().len()), (&json!([]), &1), "no declaration, no permission");
    assert_eq!(listed_item(&d, "addon:plugins/mine.js")["permissions"], json!(["storage"]), "the user's own add-on is still its own");
    assert_eq!(applayer::manifest_of(&d.layer)["plugins"].as_array().unwrap().iter().find(|p| p["name"] == "calm.js").unwrap()["permissions"], json!([]));

    // `bundles.json` and `addons.json` are not customisation files: a pack that carries one is refused whole…
    for hostile in [FILE, "addons.json"] {
        let e = packs::check_pack(&json!({"format": "thoughts-pack", "version": 1, "kind": "feature", "files": {hostile: "{}", "custom.css": "a{}"}})).unwrap_err().to_string();
        assert!(e.contains(hostile), "{e}");
    }
    // …and one written into a change under review is never kept.
    staging::recreate(st, &d, "customize", None).await.unwrap();
    w(&d.staging, "custom.css", "body { margin: 0 }\n");
    w(&d.staging, FILE, &format!("{{\"format\":1,\"bundles\":{{}},\"off\":[\"{}\",\"look\"]}}", b.id));
    w(&d.staging, "addons.json", "{\"off\":[\"plugins/mine.js\"]}");
    w(&d.staging, "SUMMARY.md", "Tidy margins");
    let p = staging::pending(st, &d, false).await;
    assert_eq!(p["ignored"], json!(["addons.json", FILE]));
    assert_eq!(p["files"].as_array().unwrap().len(), 1);
    assert_eq!(p["plugin"]["target"], "none", "one thing changed: no plugin is suggested");
    staging::apply(st, &d, false, p["signature"].as_str()).await.unwrap();
    assert_eq!(load(&d.layer), f, "the record is as the app wrote it");
    assert!(applayer::switched_off(&d.layer).is_empty());
    // A record someone did put there by hand is read with everything out of shape dropped.
    w(&d.layer, FILE, &json!({"format": 1, "off": ["look", "../x", "bundles"], "bundles": {
        "p-aaaaaaaaaaaa": {"name": "A\u{202e}", "origin": "system", "minApp": "soon", "fingerprint": "abc", "claims": [{"kind": "file", "path": "../../x"}, {"kind": "file", "path": "plugins/mine.js"}, {"kind": "key", "file": "addons.json", "pointer": "/off"}]},
        "nope": {"name": "B"}}}).to_string());
    let f = load(&d.layer);
    assert_eq!(f.off, BTreeSet::from(["look".to_string()]));
    assert_eq!(f.bundles.keys().collect::<Vec<_>>(), ["p-aaaaaaaaaaaa"]);
    let a = &f.bundles["p-aaaaaaaaaaaa"];
    assert_eq!((a.name.as_str(), a.origin.as_str(), a.min_app.as_str(), a.fingerprint.as_str(), a.claims.clone()), ("A[U+202E]", "made", "", "", vec![file("plugins/mine.js")]));
    done(&d);
}

#[tokio::test]
async fn a_plugin_made_for_a_newer_thoughts_is_refused_and_never_loaded() {
    let (st, d) = mine("minapp").await;
    let mut pack = kanban("1.0.0");
    pack["plugin"]["minApp"] = json!("9.0.0");
    let look = packs::inspect(&pack, &d.layer, &[]).unwrap();
    assert_eq!((&look["plugin"]["compatible"], &look["plugin"]["why"], &look["plugin"]["minApp"]), (&json!(false), &json!("newerApp"), &json!("9.0.0")));
    let e = packs::import(st, &d, &pack, false).await.unwrap_err().to_string();
    assert!(e.contains("needs Thoughts 9.0.0 or newer"), "{e}");
    assert!(!d.staging.exists(), "nothing was staged");
    assert!(packs::import(st, &d, &pack, true).await.unwrap_err().to_string().contains("9.0.0"));
    // A version that is not MAJOR.MINOR.PATCH is no version: refused, at the look already.
    for bad in [json!("banana"), json!("1.0"), json!(9), json!("9.0.0 or so")] {
        pack["plugin"]["minApp"] = bad.clone();
        assert!(packs::inspect(&pack, &d.layer, &[]).is_err() && packs::import(st, &d, &pack, false).await.is_err(), "{bad}");
    }
    pack["plugin"]["minApp"] = json!(NOW);
    pack["plugin"]["layerFormat"] = json!(FORMAT + 1);
    assert_eq!(packs::inspect(&pack, &d.layer, &[]).unwrap()["plugin"]["why"], "newerFormat");
    assert!(packs::import(st, &d, &pack, false).await.is_err());

    // An installed copy (a layer that came from a newer computer, or after going back a version):
    // listed with the reason, not loaded, not switched on, its task not run.
    keep_pack(st, &d, &kanban("1.0.0")).await;
    let weekly = workflows::load_files(&d.layer).remove(0);
    workflows::approve(&d.layer, &d.home, &weekly.file, &weekly.hash).unwrap();
    let mut f = load(&d.layer);
    f.bundles.get_mut(ID).unwrap().min_app = "9.0.0".into();
    save(&d.layer, &f).unwrap();
    st.commit_all(&d.layer, "from a newer computer").await.unwrap();
    let p = listed_item(&d, ID);
    assert_eq!((&p["state"], &p["on"], &p["canSwitch"], &p["canExport"], &p["canRemove"]), (&json!("needsNewerApp"), &json!(false), &json!(false), &json!(false), &json!(true)));
    assert!(p["message"].as_str().unwrap().contains("9.0.0"));
    assert!(list(&d, true)[0]["message"].as_str().unwrap().contains("أو أحدث"));
    let m = applayer::manifest_of(&d.layer);
    assert_eq!(m["plugins"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect::<Vec<_>>(), ["mine.js"]);
    assert_eq!((&m["home"]["chips"], &m["nav"], &m["workflows"], m["css"].as_str().unwrap().contains(".board")), (&json!([{"label": "Plan", "prompt": "Plan my week: "}]), &Value::Null, &json!([]), false));
    assert!(workflows::approved_workflows(&d.layer, &d.home).is_empty());
    assert!(switch_in(st, &d, ID, false).await.is_ok() && switch_in(st, &d, ID, true).await.unwrap_err().to_string().contains("9.0.0"));
    assert!(export_in(&d, ID).is_err());
    assert!(!remove_in(st, &d, ID).await.unwrap().is_empty(), "it can still be removed");
    done(&d);
}

fn rename_chip(files: &mut BTreeMap<String, String>) -> anyhow::Result<()> {
    if let Some(t) = files.get_mut("home.json") { *t = t.replace("\"Board\"", "\"Kanban board\""); }
    Ok(())
}
fn refuse(_: &mut BTreeMap<String, String>) -> anyhow::Result<()> { anyhow::bail!("this step cannot do it") }
fn escape(files: &mut BTreeMap<String, String>) -> anyhow::Result<()> { files.insert("../outside.txt".into(), "x".into()); Ok(()) }

/// The hook for a future layer format: a step runs on a file made for the older format before it is
/// staged, and on the live layer at start as one entry of the app's own.
#[tokio::test]
async fn a_format_update_runs_on_import_and_on_the_live_layer() {
    let set = |steps: Vec<(u64, Migration)>| TEST_MIGRATIONS.with(|t| *t.borrow_mut() = steps);
    let (st, d) = mine("format").await;
    let mut old = kanban("1.0.0");
    old["plugin"]["layerFormat"] = json!(0);
    assert_eq!(packs::inspect(&old, &d.layer, &[]).unwrap()["plugin"]["migrates"], true);
    // (No step known: a file that only says "older" is taken as it is.)
    packs::import(st, &d, &old, false).await.unwrap();
    assert_eq!(staging::pending(st, &d, false).await["plugin"]["migrated"], false);
    set(vec![(0, rename_chip)]);
    packs::import(st, &d, &old, false).await.unwrap();
    let p = staging::pending(st, &d, false).await;
    assert_eq!(p["plugin"]["migrated"], true, "the review says it was updated from an older format");
    assert!(read(&d.staging, "home.json").contains("Kanban board") && !read(&d.staging, "home.json").contains("\"Board\""));
    assert_eq!(p["pack"]["fingerprint"], old["fingerprint"], "the fingerprint is of the file as it came");
    keep(st, &d).await;
    assert!(load(&d.layer).bundles[ID].claims.contains(&item("home.json", "/chips", "label", "Kanban board")));
    // A step that fails, or writes outside the customisation files, stages nothing.
    for bad in [refuse as Migration, escape as Migration] {
        set(vec![(0, bad)]);
        assert!(packs::import_as(st, &d, &old, false, None, true).await.is_err());
        assert!(!d.staging.exists());
    }

    // The live layer, as a 0.5.0 start finds one whose record says format 0.
    let (st2, live) = mine("format-live").await;
    set(vec![]);
    keep_pack(st2, &live, &kanban("1.0.0")).await;
    let as_old = |d: &Dirs| {
        let mut v: Value = serde_json::from_str(&read(&d.layer, FILE)).unwrap();
        v["format"] = json!(0);
        v["bundles"][ID]["layerFormat"] = json!(0);
        w(&d.layer, FILE, &(serde_json::to_string_pretty(&v).unwrap() + "\n"));
    };
    set(vec![(0, rename_chip)]);
    as_old(&live);
    st2.commit_all(&live.layer, "as an older version wrote it").await.unwrap();
    let entries = count(st2, &live).await;
    applayer::ensure_in(st2, &live).await.unwrap();
    let h = applayer::history_in(st2, &live.layer).await;
    assert_eq!(h.len(), entries + 1, "one entry");
    assert_eq!((h[0]["summary"].as_str().unwrap(), h[0]["kind"].as_str(), h[0]["entry"].as_str(), h[0]["undoable"].as_bool()),
        (format!("Updated your customisations for Thoughts {}", env!("CARGO_PKG_VERSION")).as_str(), Some("system"), Some("plugins-format"), Some(false)));
    assert!(applayer::history_page_in(st2, &live.layer, 0, 1, true).await[0]["summary"].as_str().unwrap().starts_with("تحديث تخصيصاتك"));
    assert_eq!(h[0]["files"], json!([FILE, "home.json"]));
    assert!(read(&live.layer, "home.json").contains("Kanban board"));
    let f = load(&live.layer);
    assert_eq!((serde_json::from_str::<Value>(&read(&live.layer, FILE)).unwrap()["format"].clone(), f.bundles[ID].layer_format), (json!(FORMAT), FORMAT));
    applayer::ensure_in(st2, &live).await.unwrap();
    assert_eq!(count(st2, &live).await, entries + 1, "done once");
    // Rollback goes back from it like from any other point.
    let back = h[1]["hash"].as_str().unwrap().to_string();
    set(vec![]);
    applayer::rollback_in(st2, &live.layer, &back).await.unwrap();
    assert!(!read(&live.layer, "home.json").contains("Kanban board"));
    // A step that fails changes no file: the plugins made for the older format are switched off, with the reason.
    set(vec![(0, refuse)]);
    let before = files_of(&live);
    applayer::ensure_in(st2, &live).await.unwrap();
    assert_eq!(files_of(&live), before);
    let p = listed_item(&live, ID);
    assert_eq!((&p["state"], p["message"].as_str().unwrap()), (&json!("off"), "This was made for an older version of Thoughts and could not be updated for this one."));
    set(vec![]);
    let _ = st;
    done(&d);
    done(&live);
}

/// A change made with the assistant that touches two or more things is kept as one plugin by
/// default; the user can untick that, name it, or the chat can be one that updates a plugin.
#[tokio::test]
async fn a_change_from_a_chat_is_kept_as_one_plugin() {
    let (st, d) = mine("review").await;
    let stage = |d: &Dirs| {
        w(&d.staging, "plugins/hours.js", "export default () => { /* hours */ }\n");
        w(&d.staging, "plugins/hours.json", "{\"apiVersion\":1,\"permissions\":[\"storage\"]}\n");
        w(&d.staging, "home.json", "{\n  \"chips\": [\n    {\n      \"label\": \"Plan\",\n      \"prompt\": \"Plan my week: \"\n    },\n    {\n      \"label\": \"Hours\",\n      \"prompt\": \"Log hours\"\n    }\n  ],\n  \"hidden\": [\n    \"stats\"\n  ],\n  \"order\": [\n    \"todos\",\n    \"widget:hours\"\n  ]\n}\n");
        w(&d.staging, "SUMMARY.md", "Track \u{201c}billable\u{201d} hours\n\nA page for hours and a shortcut on Home.");
    };
    staging::recreate(st, &d, "customize", Some("chat-1")).await.unwrap();
    stage(&d);
    let p = staging::pending(st, &d, false).await;
    assert_eq!(p["plugin"], json!({"target": "new", "name": "Track billable hours", "suggested": true, "takes": []}), "ticked by default, named after the summary's first line");
    // Unticked: kept as it is, nothing registered.
    staging::set_bundle(&d, "none", "").unwrap();
    assert_eq!(staging::pending(st, &d, false).await["plugin"]["target"], "none");
    assert!(staging::set_bundle(&d, "p-zzzzzzzzzzzz", "").is_err() && staging::set_bundle(&d, "all", "").is_err() && staging::set_bundle(&d, "../x", "").is_err());
    // Ticked again with a name of the user's.
    staging::set_bundle(&d, "new", "Hours \u{201c}tracker\u{201d}\u{202e}").unwrap();
    let p = staging::pending(st, &d, false).await;
    assert_eq!((&p["plugin"]["target"], &p["plugin"]["name"]), (&json!("new"), &json!("Hours tracker[U+202E]")));
    let entries = count(st, &d).await;
    let kept = staging::apply(st, &d, false, p["signature"].as_str()).await.unwrap();
    assert_eq!(count(st, &d).await, entries + 1, "the plugin is registered in the change's own entry");
    let id = kept["plugin"]["id"].as_str().unwrap().to_string();
    let b = load(&d.layer).bundles[&id].clone();
    assert_eq!((b.name.as_str(), b.origin.as_str(), b.version.as_str()), ("Hours tracker[U+202E]", "made", "1.0.0"));
    assert_eq!(b.claims, vec![file("plugins/hours.js"), file("plugins/hours.json"), item("home.json", "/chips", "label", "Hours"), item("home.json", "/order", "value", "widget:hours")], "only what the change added");
    assert_eq!(applayer::history_in(st, &d.layer).await[0]["files"], json!([FILE, "home.json", "plugins/hours.js", "plugins/hours.json"]));
    // Undoing the change undoes the registration with it.
    let entry = kept["hash"].as_str().unwrap().to_string();
    applayer::revert_in(st, &d.layer, &entry).await.unwrap();
    assert!(!d.layer.join(FILE).exists() && !d.layer.join("plugins/hours.js").exists());
    let undo = applayer::history_in(st, &d.layer).await[0]["hash"].as_str().unwrap().to_string();
    applayer::revert_in(st, &d.layer, &undo).await.unwrap();
    assert_eq!(load(&d.layer).bundles[&id].claims.len(), 4, "and bringing it back brings the plugin back");

    // "Change with the assistant": a chat bound to the plugin updates it, and is told its id.
    bind_chat(&d, "chat-2", &id).unwrap();
    staging::recreate(st, &d, "customize", Some("chat-2")).await.unwrap();
    assert_eq!(staging::load_meta(&d).unwrap().bundle_target.as_deref(), Some(id.as_str()));
    let note = prompt_note(&d);
    assert!(note.contains(&format!("/* plugin:{id} */")) && note.contains("Never edit bundles.json") && note.contains("<plugin-name>Hours tracker[U+202E]</plugin-name>"), "{note}");
    assert!(staging::system_prompt(&d).contains(&id) && staging::system_prompt_full(&d).contains(&id));
    w(&d.staging, "custom.css", &format!("/* mine */\n:root {{ --radius: 2px }}\n\n/* plugin:{id} */\n.hours {{ gap: 4px }}\n/* end plugin:{id} */\n"));
    w(&d.staging, "SUMMARY.md", "Tighter rows");
    let p = staging::pending(st, &d, false).await;
    assert_eq!(p["plugin"], json!({"target": "existing", "id": id, "name": "Hours tracker[U+202E]", "suggested": false, "takes": []}));
    let kept = staging::apply(st, &d, false, p["signature"].as_str()).await.unwrap();
    assert_eq!(kept["plugin"]["id"], id.as_str());
    let b = load(&d.layer).bundles[&id].clone();
    assert_eq!(b.claims.len(), 5);
    assert!(b.claims.contains(&Claim::Css { block: format!("plugin:{id}") }));
    assert_eq!(listed_item(&d, "look")["id"], "look", "the user's own lines are still their look");
    // A chat with no plugin of its own, changing one thing: nothing is suggested, nothing registered.
    assert_eq!(chat_target(&d, "chat-1"), None);
    staging::recreate(st, &d, "customize", Some("chat-1")).await.unwrap();
    w(&d.staging, "todos.json", "{\"groupBy\":\"due\"}\n");
    let p = staging::pending(st, &d, false).await;
    assert_eq!((&p["plugin"]["target"], &p["plugin"]["suggested"]), (&json!("none"), &json!(false)));
    assert!(staging::apply(st, &d, false, p["signature"].as_str()).await.unwrap()["plugin"].is_null());
    assert_eq!(load(&d.layer).bundles.len(), 1);
    // A later change that deletes one of the plugin's files drops that claim in the same entry.
    staging::recreate(st, &d, "customize", Some("chat-1")).await.unwrap();
    std::fs::remove_file(d.staging.join("plugins/hours.json")).unwrap();
    let p = staging::pending(st, &d, false).await;
    staging::apply(st, &d, false, p["signature"].as_str()).await.unwrap();
    assert!(!load(&d.layer).bundles[&id].claims.contains(&file("plugins/hours.json")));
    done(&d);
}

/// "Export all" on one computer, one review on the other: every plugin is there again, with the
/// same claims and what was off still off. Nothing is ever switched on by a file.
#[tokio::test]
async fn everything_exports_as_one_file_and_imports_through_one_review() {
    let (st, a) = mine("all-a").await;
    keep_pack(st, &a, &kanban("1.0.0")).await;
    w(&a.layer, "themes.json", "[{\"id\":\"sand\",\"name\":\"Sand\"}]\n");
    st.commit_all(&a.layer, "a theme").await.unwrap();
    crate::layer_api::storage_write(&a.home, "mine.js", br#"{"secret":"zebracode"}"#, false).unwrap();
    switch_in(st, &a, ID, false).await.unwrap();
    switch_in(st, &a, "theme:sand", false).await.unwrap();
    switch_in(st, &a, "addon:plugins/mine.js", false).await.unwrap();
    let pack = export_all(&a, "Everything").unwrap();
    assert_eq!((&pack["kind"], &pack["name"]), (&json!("full"), &json!("Everything")));
    let files = packs::check_pack(&pack).unwrap();
    assert!(!files.contains_key(FILE) && !files.contains_key("addons.json") && !pack.to_string().contains("zebracode"), "the app's own records and what add-ons saved are not in it");
    assert_eq!(pack["plugins"].as_array().unwrap().iter().map(|p| (p["id"].as_str().unwrap(), p["on"].as_bool().unwrap())).collect::<Vec<_>>(),
        [(ID, false), ("theme:sand", false), ("addon:plugins/board.js", false), ("addon:plugins/extra.js", false), ("addon:plugins/mine.js", false)]);

    let (st2, b) = layer("all-b").await;
    let look = packs::inspect(&pack, &b.layer, &[]).unwrap();
    assert_eq!(look["plugin"], json!({"all": [{"id": ID, "name": "Kanban", "version": "1.0.0", "on": false}]}));
    packs::import(st2, &b, &pack, false).await.unwrap();
    let p = staging::pending(st2, &b, false).await;
    assert_eq!(p["plugin"], json!({"target": "all", "name": "", "suggested": false, "takes": [], "count": 1}));
    let entries = count(st2, &b).await;
    keep(st2, &b).await;
    assert_eq!(count(st2, &b).await, entries + 1, "one review, one entry");
    let (fa, fb) = (load(&a.layer), load(&b.layer));
    assert_eq!(fb.bundles[ID].claims, fa.bundles[ID].claims);
    assert_eq!((fb.bundles[ID].name.as_str(), fb.bundles[ID].origin.as_str()), ("Kanban", "pack"));
    assert_eq!(fb.off, fa.off);
    assert_eq!(applayer::switched_off(&b.layer), applayer::switched_off(&a.layer), "an add-on that was off stays off");
    assert!(applayer::switched_off(&b.layer).contains("plugins/mine.js"));
    let ids = |d: &Dirs| list(d, false).iter().map(|p| (p["id"].as_str().unwrap().to_string(), p["on"].as_bool().unwrap())).collect::<Vec<_>>();
    assert_eq!(ids(&b), ids(&a), "the same list, the same switches");
    assert_eq!(applayer::manifest_of(&b.layer)["plugins"], json!([]), "nothing that was off runs here");
    // A list that lies: claims on things that are not in the file are dropped, an id out of shape is skipped, and "on" switches nothing on.
    let (st3, c) = mine("all-c").await;
    switch_in(st3, &c, "addon:plugins/mine.js", false).await.unwrap();
    let mut lying = pack.clone();
    lying["plugins"] = json!([{"id": "p-bbbbbbbbbbbb", "name": "Ghost", "on": true, "claims": [{"kind": "file", "path": "plugins/none.js"}, {"kind": "key", "file": "bundles.json", "pointer": "/off"}]},
        {"id": "../x", "name": "Bad", "claims": [{"kind": "file", "path": "plugins/mine.js"}]}, {"id": "addon:plugins/mine.js", "on": true}, {"id": "look", "on": true}]);
    switch_in(st3, &c, "look", false).await.unwrap();
    packs::import(st3, &c, &lying, false).await.unwrap();
    keep(st3, &c).await;
    assert!(load(&c.layer).bundles.is_empty(), "{:?}", load(&c.layer).bundles);
    assert_eq!((load(&c.layer).off, applayer::switched_off(&c.layer)), (BTreeSet::from(["look".to_string()]), BTreeSet::from(["plugins/mine.js".to_string()])), "what the user switched off here stays off");
    let _ = st;
    done(&a);
    done(&b);
    done(&c);
}

#[test]
fn undoing_an_entry_takes_back_only_what_it_did_to_the_record() {
    let b = |id: &str, name: &str, claims: Vec<Claim>| Bundle { id: id.into(), name: name.into(), claims, ..Default::default() };
    let file_of = |list: Vec<Bundle>, off: &[&str]| BundlesFile { format: 1, bundles: list.into_iter().map(|x| (x.id.clone(), x)).collect(), off: off.iter().map(|s| s.to_string()).collect(), migrated: "0.5.0".into(), unreadable: false };
    let (a, a2, c) = (b("p-aaaaaaaaaaaa", "A", vec![file("plugins/a.js")]), b("p-aaaaaaaaaaaa", "A2", vec![file("plugins/a.js"), file("plugins/a2.js")]), b("p-cccccccccccc", "C", vec![file("plugins/c.js")]));
    // The entry added C and switched "home" off; since then A was renamed and "look" switched off.
    let (before, after, now) = (file_of(vec![a.clone()], &[]), file_of(vec![a.clone(), c.clone()], &["home"]), file_of(vec![a2.clone(), c.clone()], &["home", "look"]));
    assert_eq!(merge_undo(&now, &after, &before), file_of(vec![a2.clone()], &["look"]));
    // The entry changed A (to A2); since then C came. Undoing it gives A back as it was and leaves C.
    assert_eq!(merge_undo(&file_of(vec![a2.clone(), c.clone()], &[]), &file_of(vec![a2.clone()], &[]), &file_of(vec![a.clone()], &[])), file_of(vec![a.clone(), c.clone()], &[]));
    // The entry removed C while it was off; undoing it brings C back, off, and C takes its claim back.
    let thief = b("p-dddddddddddd", "D", vec![file("plugins/c.js"), file("plugins/d.js")]);
    let merged = merge_undo(&file_of(vec![a.clone(), thief], &[]), &file_of(vec![a.clone()], &[]), &file_of(vec![a.clone(), c.clone()], &["p-cccccccccccc"]));
    assert_eq!(merged, file_of(vec![a, c, b("p-dddddddddddd", "D", vec![file("plugins/d.js")])], &["p-cccccccccccc"]));
}

/// Over a real connection: plugins are managed by the app's own window only.
#[tokio::test]
async fn access_keys_and_add_ons_never_reach_the_plugin_addresses() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn call(port: u16, method: &str, path: &str, auth: &str, body: &str) -> (u16, String) {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {auth}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        (out.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0), out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default())
    }
    let (st, _lib) = crate::wraps::testutil::library().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = crate::api::router(st.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut keys = vec![];
    for (name, scopes) in [("reader", json!(["read"])), ("adder", json!(["capture"])), ("both", json!(["read", "capture"]))] {
        let (code, body) = call(port, "POST", "/api/access-keys", "test-token", &json!({"name": name, "scopes": scopes}).to_string()).await;
        assert_eq!(code, 200, "{body}");
        keys.push(serde_json::from_str::<Value>(&body).unwrap()["secret"].as_str().unwrap().to_string());
    }
    let addresses = [("GET", "/api/plugins", ""), ("POST", "/api/plugins/switch", r#"{"id":"home","on":false}"#), ("POST", "/api/plugins/remove", r#"{"id":"home"}"#),
        ("POST", "/api/plugins/describe", r#"{"id":"home","name":"x"}"#), ("GET", "/api/plugins/export?all=1", ""), ("GET", "/api/plugins/export?id=p-abcdefghijkl", ""),
        ("POST", "/api/plugins/customise", r#"{"id":"p-abcdefghijkl"}"#), ("POST", "/api/layer/pending/plugin", r#"{"target":"new"}"#)];
    for (m, p, b) in addresses {
        for key in &keys {
            let (code, body) = call(port, m, p, key, b).await;
            assert!(code == 403 && body.contains("\"code\":\"forbidden\""), "a key: {m} {p}: {code} {body}");
        }
        if !p.starts_with("/api/plugins") { continue; }
        // A request made on an add-on's behalf (`scope=plugin`), even from the app's own window.
        let scoped = format!("{p}{}scope=plugin", if p.contains('?') { "&" } else { "?" });
        let (code, body) = call(port, m, &scoped, "test-token", b).await;
        assert!(code == 403 && body.contains("\"code\":\"forbidden\""), "an add-on: {m} {scoped}: {code} {body}");
        assert_eq!(call(port, m, p, "wrong-token", b).await.0, 401, "{m} {p}");
    }
    // The app's own window does reach the list (an empty app here).
    let (code, body) = call(port, "GET", "/api/plugins", "test-token", "").await;
    assert_eq!(code, 200, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!((&v["format"], v["appVersion"].as_str(), v["plugins"].is_array()), (&json!(FORMAT), Some(env!("CARGO_PKG_VERSION")), true));
    server.abort();
}

/// A record that is there but cannot be read is never taken for "no plugins": that would switch
/// everything that was off back on. Nothing of the layer loads, no task runs, nothing is written on
/// top of it, the user is told, and undoing the entry that broke it is the way out.
#[tokio::test]
async fn a_record_that_cannot_be_read_fails_closed() {
    let (st, d) = mine("unreadable").await;
    keep_pack(st, &d, &kanban("1.0.0")).await;
    let weekly = workflows::load_files(&d.layer).remove(0);
    workflows::approve(&d.layer, &d.home, &weekly.file, &weekly.hash).unwrap();
    switch_in(st, &d, ID, false).await.unwrap();
    let good = read(&d.layer, FILE);
    assert!(!load(&d.layer).unreadable && !load(&Path::new("/nowhere/at/all")).unreadable, "a file that is not there is simply no plugins yet");
    let mut over = serde_json::from_str::<Value>(&good).unwrap();
    over["off"] = json!((0..=MAX_OFF).map(|i| format!("theme:t{i}")).collect::<Vec<_>>());
    let unknown = good.replace("\"kind\": \"file\"", "\"kind\": \"everything\"");
    assert_ne!(unknown, good);
    for (why, broken) in [("too large", format!("{{\"pad\":\"{}\"}}", "x".repeat(MAX_BYTES))), ("not JSON", "{\"format\": 1, \"bundles\": {".to_string()),
        ("cut short", good[..good.len() / 2].to_string()), ("another shape", "[]".to_string()), ("an object that is not the record", "{\"off\": []}".to_string()), ("over the limits", over.to_string()), ("a claim of a kind this version does not know", unknown)] {
        w(&d.layer, FILE, &broken);
        st.commit_all(&d.layer, "the record was damaged").await.unwrap();
        assert!(load(&d.layer).unreadable, "{why}");
        // The plugin that was off does not come back on: nothing of the layer is handed to the UI…
        let m = applayer::manifest_of(&d.layer);
        assert_eq!((&m["plugins"], &m["views"], &m["workflows"], &m["themes"], &m["css"], &m["home"], &m["nav"], &m["pluginsUnreadable"]),
            (&json!([]), &json!([]), &json!([]), &json!([]), &json!(""), &Value::Null, &Value::Null, &json!(true)), "{why}");
        assert_eq!(applayer::manifest_preview(&d.layer, &d.layer, false)["plugins"], json!([]), "{why}: nor while a change is tried");
        // …and no automatic task runs, by schedule or by hand.
        assert!(workflows::approved_workflows(&d.layer, &d.home).is_empty(), "{why}");
        assert!(inactive_workflows(&d.layer).contains("weekly.json"));
        // Nothing is written on top of it: switch, remove, describe, export, import and Keep are refused, in plain words.
        let said = |e: anyhow::Error| { let e = e.to_string(); assert!(e.contains("is damaged") && e.contains("History"), "{why}: {e}"); };
        said(switch_in(st, &d, ID, true).await.unwrap_err());
        said(switch_in(st, &d, "home", false).await.unwrap_err());
        said(switch_in(st, &d, "addon:plugins/mine.js", false).await.unwrap_err());
        said(remove_in(st, &d, ID).await.unwrap_err());
        said(remove_in(st, &d, "look").await.unwrap_err());
        said(describe_in(st, &d, &Describe { id: "home".into(), name: "X".into(), ..Default::default() }).await.unwrap_err());
        said(export_in(&d, ID).unwrap_err());
        said(export_all(&d, "x").unwrap_err());
        said(packs::import(st, &d, &kanban("1.1.0"), false).await.unwrap_err());
        assert!(!d.staging.exists(), "{why}: nothing was staged");
        staging::recreate(st, &d, "customize", None).await.unwrap();
        w(&d.staging, "custom.css", "body { margin: 0 }\n");
        let p = staging::pending(st, &d, false).await;
        said(staging::apply(st, &d, false, p["signature"].as_str()).await.unwrap_err());
        assert!(!read(&d.layer, "custom.css").contains("margin: 0"), "{why}: the change was not kept");
        staging::discard(st, &d).await.unwrap();
        assert_eq!(read(&d.layer, FILE), broken, "{why}: the file was not written over");
        assert!(list(&d, false).is_empty());
        assert!(crate::tr::user_err_in(true, "plugin.unreadable", &[]).message.contains("السجل"));
        // The way out: undo the entry that damaged it (or go back to before it), and all is as it was.
        let entry = applayer::history_in(st, &d.layer).await[0]["hash"].as_str().unwrap().to_string();
        applayer::revert_in(st, &d.layer, &entry).await.unwrap();
        let f = load(&d.layer);
        assert!(!f.unreadable && f.off.contains(ID) && f.bundles.contains_key(ID), "{why}: {f:?}");
        assert_eq!(applayer::manifest_of(&d.layer)["plugins"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect::<Vec<_>>(), ["mine.js"], "{why}: still off");
    }
    done(&d);
}

/// The app never writes a record it could not read back: past the limits on plugins, claims or
/// size the change is refused whole, and what was there stays.
#[tokio::test]
async fn the_record_is_never_written_past_what_is_read_back() {
    let big = |n: usize, claims: usize| BundlesFile { format: 1, migrated: "x".into(), bundles: (0..n).map(|i| {
        let id = id_of(&i.to_string());
        (id.clone(), Bundle { id, name: format!("P{i}"), origin: "made".into(), layer_format: 1, claims: (0..claims).map(|c| file(&format!("plugins/p{i}-{c}.js"))).collect(), ..Default::default() })
    }).collect(), ..Default::default() };
    assert!(text_of(&big(MAX_BUNDLES, 1)).is_ok());
    for too_much in [big(MAX_BUNDLES + 1, 1), big(1, MAX_CLAIMS + 1), big(MAX_BUNDLES, 60)] {
        assert!(text_of(&too_much).unwrap_err().to_string().contains("too many plugins"));
    }
    let mut off = big(1, 1);
    off.off = (0..=MAX_OFF).map(|i| format!("theme:t{i}")).collect();
    assert!(text_of(&off).is_err() && text_of(&BundlesFile { unreadable: true, ..Default::default() }).is_err());
    // What is written is read back as it was.
    let (st, d) = mine("limits").await;
    let full = big(MAX_BUNDLES, 1);
    save(&d.layer, &full).unwrap();
    assert_eq!(load(&d.layer), full);
    assert!(read(&d.layer, FILE).len() <= MAX_BYTES);
    st.commit_all(&d.layer, "as many plugins as there can be").await.unwrap();
    let before = (read(&d.layer, FILE), files_of(&d), count(st, &d).await);
    // One more is refused wherever it would come from, and nothing is half-done.
    assert!(describe_in(st, &d, &Describe { id: "home".into(), name: "One more".into(), ..Default::default() }).await.unwrap_err().to_string().contains("too many plugins"));
    packs::import(st, &d, &kanban("1.0.0"), false).await.unwrap();
    let p = staging::pending(st, &d, false).await;
    assert!(staging::apply(st, &d, false, p["signature"].as_str()).await.unwrap_err().to_string().contains("too many plugins"));
    staging::discard(st, &d).await.unwrap();
    let many: Vec<String> = (0..=MAX_OFF).map(|i| format!("theme:t{i}")).collect();
    w(&d.layer, "themes.json", &json!(many.iter().map(|t| json!({"id": &t[6..], "name": "T"})).collect::<Vec<_>>()).to_string());
    st.commit_all(&d.layer, "a great many themes").await.unwrap();
    let mut f = load(&d.layer);
    f.off = many.iter().take(MAX_OFF).cloned().collect();
    save(&d.layer, &f).unwrap();
    st.commit_all(&d.layer, "all but one off").await.unwrap();
    let entries = count(st, &d).await;
    assert!(switch_in(st, &d, &many[MAX_OFF], false).await.unwrap_err().to_string().contains("too many plugins"));
    assert_eq!((count(st, &d).await, load(&d.layer).off.len(), load(&d.layer).unreadable), (entries, MAX_OFF, false), "refused whole: the record is as it was");
    assert_eq!(before.1.get("home.json"), files_of(&d).get("home.json"));
    assert!(before.2 > 0 && before.0.len() <= MAX_BYTES);
    done(&d);
}
