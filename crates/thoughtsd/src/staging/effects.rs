//! What a change does, item by item, read from the files themselves and never from what the
//! assistant or a pack says about them: each add-on and page it adds, changes or removes (with the
//! permissions it gains or loses), each Home shortcut and section, each sidebar entry, each todo
//! setting, theme and automatic task. The review leads with this, and the question after an app
//! update uses it to say what a new built-in version changes.
//!
//! Every entry is `{area, op, …}`; the UI words it. Text taken from the files (names, labels,
//! prompts, descriptions) has anything that would not show written out (`validate::show_hidden`).

use super::validate::{css_report, show_hidden};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// `added`, `removed` or `changed`, from whether the file exists before and after.
fn op_of(before: Option<&str>, after: Option<&str>) -> &'static str {
    match (before, after) { (None, Some(_)) => "added", (Some(_), None) => "removed", _ => "changed" }
}

fn line(s: &str) -> String { show_hidden(s.trim(), false) }

fn strs(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).map(line).collect()
}

/// Parsed JSON of a file, `Null` when there is none. `None` when it is there but cannot be read
/// (its problems are listed in the review; nothing is guessed about what it would do).
fn parse(text: Option<&str>) -> Option<Value> {
    match text { None => Some(Value::Null), Some(t) => serde_json::from_str(t).ok() }
}

/// Items present in one list and not the other, in the order of the list they are in.
fn gained(from: &[String], to: &[String]) -> Vec<String> {
    to.iter().filter(|x| !from.contains(x)).cloned().collect()
}

fn hidden_and_order(area: &str, b: &Value, a: &Value, out: &mut Vec<Value>) {
    let (hb, ha) = (strs(&b["hidden"]), strs(&a["hidden"]));
    for what in gained(&hb, &ha) { out.push(json!({"area": area, "op": "hidden", "what": what})); }
    for what in gained(&ha, &hb) { out.push(json!({"area": area, "op": "shown", "what": what})); }
    let (ob, oa) = (strs(&b["order"]), strs(&a["order"]));
    if ob != oa {
        out.push(if oa.is_empty() { json!({"area": area, "op": "orderReset"}) } else { json!({"area": area, "op": "order", "order": oa}) });
    }
}

/// Home shortcuts are matched by their label: a new label is a new shortcut.
fn home(b: &Value, a: &Value) -> Vec<Value> {
    let chips = |v: &Value| -> Vec<(String, String)> {
        v["chips"].as_array().into_iter().flatten()
            .filter_map(|c| Some((c["label"].as_str().filter(|l| !l.trim().is_empty())?, c["prompt"].as_str()?)))
            .map(|(l, p)| (line(l), show_hidden(p, true))).collect()
    };
    let (cb, ca) = (chips(b), chips(a));
    let mut out = vec![];
    for (label, prompt) in &ca {
        match cb.iter().find(|(l, _)| l == label) {
            None => out.push(json!({"area": "home", "op": "chipAdded", "label": label, "prompt": prompt})),
            Some((_, was)) if was != prompt => out.push(json!({"area": "home", "op": "chipChanged", "label": label, "prompt": prompt, "was": was})),
            _ => {}
        }
    }
    for (label, _) in cb.iter().filter(|(l, _)| !ca.iter().any(|(x, _)| x == l)) {
        out.push(json!({"area": "home", "op": "chipRemoved", "label": label}));
    }
    hidden_and_order("home", b, a, &mut out);
    out
}

fn nav(b: &Value, a: &Value) -> Vec<Value> {
    let mut out = vec![];
    hidden_and_order("nav", b, a, &mut out);
    let labels = |v: &Value| -> Vec<(String, String, String)> {
        v["labels"].as_object().into_iter().flatten().map(|(id, l)| {
            let text = |k: &str| l[k].as_str().map(line).unwrap_or_default();
            (line(id), text("en"), text("ar"))
        }).filter(|(_, en, ar)| !en.is_empty() || !ar.is_empty()).collect()
    };
    let (lb, la) = (labels(b), labels(a));
    for (id, en, ar) in &la {
        if !lb.iter().any(|(i, e, r)| i == id && e == en && r == ar) { out.push(json!({"area": "nav", "op": "renamed", "what": id, "en": en, "ar": ar})); }
    }
    for (id, ..) in lb.iter().filter(|(i, ..)| !la.iter().any(|(x, ..)| x == i)) {
        out.push(json!({"area": "nav", "op": "renameRemoved", "what": id}));
    }
    out
}

fn todos(b: &Value, a: &Value) -> Vec<Value> {
    ["groupBy", "sort", "filter", "showDone", "moveDone"].iter().filter(|k| b[**k] != a[**k]).map(|k| {
        let v = &a[*k];
        if v.is_string() || v.is_boolean() { json!({"area": "todos", "op": "set", "key": k, "value": v}) }
        else { json!({"area": "todos", "op": "unset", "key": k}) }
    }).collect()
}

fn themes(b: &Value, a: &Value) -> Vec<Value> {
    let list = |v: &Value| -> Vec<(String, String, Value)> {
        v.as_array().into_iter().flatten().filter_map(|t| Some((line(t["id"].as_str()?), line(t["name"].as_str().unwrap_or_default()), t.clone()))).collect()
    };
    let (tb, ta) = (list(b), list(a));
    let mut out = vec![];
    for (id, name, all) in &ta {
        match tb.iter().find(|(i, ..)| i == id) {
            None => out.push(json!({"area": "themes", "op": "added", "id": id, "name": name})),
            Some((.., was)) if was != all => out.push(json!({"area": "themes", "op": "changed", "id": id, "name": name})),
            _ => {}
        }
    }
    for (id, name, _) in tb.iter().filter(|(i, ..)| !ta.iter().any(|(x, ..)| x == i)) {
        out.push(json!({"area": "themes", "op": "removed", "id": id, "name": name}));
    }
    out
}

/// One add-on or page declaration (already resolved, see `applayer::resolve_decl`) compared with
/// what it was. `None` when nothing about it differs.
fn item(area: &str, name: &str, before: Option<&Value>, after: Option<&Value>, compare_code: bool) -> Option<Value> {
    let perms = |v: &Value| strs(&v["permissions"]);
    let write = |v: &Value| strs(&v["write"]);
    // The folders it may read in; none listed = the whole library (but private folders).
    let read = |v: &Value| strs(&v["read"]);
    let desc = |v: &Value| v["description"].as_str().map(line).unwrap_or_default();
    match (before, after) {
        (None, Some(y)) => Some(json!({"area": area, "op": "added", "name": line(name), "description": desc(y),
            "permissions": perms(y), "write": write(y), "read": read(y), "unsupported": y["unsupported"] == true})),
        (Some(x), None) => Some(json!({"area": area, "op": "removed", "name": line(name), "description": desc(x)})),
        (Some(x), Some(y)) => {
            let code = compare_code && x["code"] != y["code"];
            let same = perms(x) == perms(y) && write(x) == write(y) && read(x) == read(y) && desc(x) == desc(y) && x["apiVersion"] == y["apiVersion"] && !code;
            if same { return None; }
            Some(json!({"area": area, "op": "changed", "name": line(name), "description": desc(y),
                "permissions": perms(y), "write": write(y), "read": read(y), "unsupported": y["unsupported"] == true,
                // Reading is the other way round: a list that appears narrows it, one that goes widens it.
                "readChanged": read(x) != read(y), "readBefore": read(x),
                "gained": gained(&perms(x), &perms(y)), "lost": gained(&perms(y), &perms(x)),
                "writeGained": gained(&write(x), &write(y)), "writeLost": gained(&write(y), &write(x)),
                "code": code, "descriptionChanged": desc(x) != desc(y)}))
        }
        (None, None) => None,
    }
}

/// The declarations listed in a `manifest.json` (`plugins` / `views`), compared on their own (an
/// app update ships this file without any add-on code beside it).
fn manifest(b: &Value, a: &Value) -> Vec<Value> {
    let mut out = vec![];
    let empty = Map::new();
    for (sub, area) in [("plugins", "addon"), ("views", "page")] {
        let (mb, ma) = (b[sub].as_object().unwrap_or(&empty), a[sub].as_object().unwrap_or(&empty));
        let names: BTreeSet<&String> = mb.keys().chain(ma.keys()).collect();
        for name in names {
            let decl = |v: Option<&Value>| v.map(|d| json!({"permissions": d["permissions"], "write": d["write"], "read": d["read"], "description": d["description"], "apiVersion": d["apiVersion"]}));
            if let Some(e) = item(area, name, decl(mb.get(name)).as_ref(), decl(ma.get(name)).as_ref(), false) { out.push(e); }
        }
    }
    out
}

/// What one file going from `before` to `after` does (`None` = no such file), for the files that
/// hold settings. `None` when the file is not one of those, or cannot be read.
pub fn file_effects(path: &str, before: Option<&str>, after: Option<&str>) -> Option<Vec<Value>> {
    let op = op_of(before, after);
    if path == "custom.css" {
        return Some(vec![json!({"area": "style", "op": op, "report": css_report(before.unwrap_or(""), after.unwrap_or(""))})]);
    }
    if path == "CLAUDE.md" { return Some(vec![json!({"area": "guide", "op": op})]); }
    let parts: Vec<&str> = path.split('/').collect();
    if let ["workflows", file] = parts.as_slice() {
        if !file.ends_with(".json") { return None; }
        let stem = file.trim_end_matches(".json");
        let name = |t: Option<&str>| t.and_then(|t| serde_json::from_str::<Value>(t).ok())
            .and_then(|v| v["name"].as_str().filter(|n| !n.trim().is_empty()).map(line));
        return Some(vec![json!({"area": "task", "op": op, "name": name(after).or_else(|| name(before)).unwrap_or_else(|| line(stem)), "path": path})]);
    }
    let (b, a) = (parse(before)?, parse(after)?);
    let fx = match path {
        "home.json" => home(&b, &a),
        "nav.json" => nav(&b, &a),
        "todos.json" => todos(&b, &a),
        "themes.json" => themes(&b, &a),
        "manifest.json" => manifest(&b, &a),
        _ => return None,
    };
    Some(fx)
}

fn text_of(p: &Path) -> Option<String> {
    if !std::fs::symlink_metadata(p).map_or(false, |m| m.is_file() && m.len() as usize <= crate::packs::MAX_FILE_BYTES) { return None; }
    std::fs::read_to_string(p).ok()
}

/// What the layer in `after` does that the one in `before` does not, for the changed `files`
/// (`(path, status)`): `{effects, installed}`. `installed` lists the add-ons and pages that stay
/// as they are, with what they may do, for an "Already installed" list.
pub fn describe(before: &Path, after: &Path, files: &[(String, &str)]) -> Value {
    let (mb, ma) = (crate::applayer::manifest_of(before), crate::applayer::manifest_of(after));
    let (mut effects, mut installed, mut covered) = (vec![], vec![], BTreeSet::new());
    for (sub, area, ext) in [("plugins", "addon", "js"), ("views", "page", "html")] {
        let list = |m: &Value| -> Vec<Value> { m[sub].as_array().cloned().unwrap_or_default() };
        let (lb, la) = (list(&mb), list(&ma));
        let named = |l: &[Value], n: &str| l.iter().find(|x| x["name"] == n).cloned();
        let names: BTreeSet<String> = lb.iter().chain(la.iter()).filter_map(|x| x["name"].as_str().map(String::from)).collect();
        for name in names {
            let (x, y) = (named(&lb, &name), named(&la, &name));
            match item(area, &name, x.as_ref(), y.as_ref(), true) {
                Some(e) => {
                    let stem = name.strip_suffix(&format!(".{ext}")).unwrap_or(&name);
                    covered.insert(format!("{sub}/{name}"));
                    covered.insert(format!("{sub}/{stem}.json"));
                    covered.insert("manifest.json".to_string());
                    effects.push(e);
                }
                None => if let Some(y) = y {
                    installed.push(json!({"area": area, "name": line(&name), "description": y["description"].as_str().map(line).unwrap_or_default(),
                        "permissions": strs(&y["permissions"]), "write": strs(&y["write"]), "read": strs(&y["read"])}));
                },
            }
        }
    }
    for (path, status) in files {
        if covered.contains(path) { continue; }
        let (b, a) = (text_of(&before.join(path)), if *status == "deleted" { None } else { text_of(&after.join(path)) });
        // (What `manifest.json` grants is already in the add-on comparison above, which also knows
        // when an add-on's own declaration overrides it; on its own it is just a changed file.)
        let found = if path == "manifest.json" { None } else { file_effects(path, b.as_deref(), a.as_deref()).filter(|f| !f.is_empty()) };
        match found {
            Some(f) => effects.extend(f),
            None => effects.push(json!({"area": "file", "path": path,
                "op": match *status { "added" => "added", "deleted" => "removed", _ => "changed" }})),
        }
    }
    json!({"effects": effects, "installed": installed})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(path: &str, before: Option<&str>, after: Option<&str>) -> Vec<Value> { file_effects(path, before, after).unwrap() }

    #[test]
    fn settings_files_are_described_by_what_they_change() {
        // Home: a new shortcut with its prompt, a reworded one, one taken away, a section hidden.
        let before = r#"{"chips":[{"label":"Plan","prompt":"Plan my day"},{"label":"Old","prompt":"x"}],"hidden":["projects"],"order":[]}"#;
        let after = r#"{"chips":[{"label":"Plan","prompt":"Plan my week"},{"label":"Plan my week","prompt":"Plan my week: list what slipped"}],"hidden":["stats"],"order":["todos","widget:hours"]}"#;
        assert_eq!(fx("home.json", Some(before), Some(after)), vec![
            json!({"area": "home", "op": "chipChanged", "label": "Plan", "prompt": "Plan my week", "was": "Plan my day"}),
            json!({"area": "home", "op": "chipAdded", "label": "Plan my week", "prompt": "Plan my week: list what slipped"}),
            json!({"area": "home", "op": "chipRemoved", "label": "Old"}),
            json!({"area": "home", "op": "hidden", "what": "stats"}),
            json!({"area": "home", "op": "shown", "what": "projects"}),
            json!({"area": "home", "op": "order", "order": ["todos", "widget:hours"]}),
        ]);
        // The sidebar: hides, renames (in both languages), takes a name back.
        let nav = fx("nav.json", Some(r#"{"labels":{"todos":{"en":"Tasks"}}}"#), Some(r#"{"hidden":["projects"],"labels":{"ideas":{"en":"Clients","ar":"العملاء"}}}"#));
        assert_eq!(nav, vec![
            json!({"area": "nav", "op": "hidden", "what": "projects"}),
            json!({"area": "nav", "op": "renamed", "what": "ideas", "en": "Clients", "ar": "العملاء"}),
            json!({"area": "nav", "op": "renameRemoved", "what": "todos"}),
        ]);
        // A new file starts from nothing; a removed one gives everything back.
        assert_eq!(fx("todos.json", None, Some(r#"{"groupBy":"due","showDone":false}"#)), vec![
            json!({"area": "todos", "op": "set", "key": "groupBy", "value": "due"}), json!({"area": "todos", "op": "set", "key": "showDone", "value": false})]);
        assert_eq!(fx("todos.json", Some(r#"{"sort":"due"}"#), None), vec![json!({"area": "todos", "op": "unset", "key": "sort"})]);
        assert_eq!(fx("themes.json", Some(r#"[{"id":"sand","name":"Sand"}]"#), Some(r#"[{"id":"sand","name":"Sand"},{"id":"sea","name":"Sea"}]"#)),
            vec![json!({"area": "themes", "op": "added", "id": "sea", "name": "Sea"})]);
        // Nothing that matters changed: nothing is said.
        assert!(fx("home.json", Some(r#"{"chips":[]}"#), Some("{\n  \"chips\": []\n}\n")).is_empty());
        // What cannot be read is not guessed at.
        assert!(file_effects("home.json", Some("{}"), Some("{broken")).is_none());
        assert!(file_effects("plugins/lib/data.json", None, Some("[]")).is_none());
        // Words that would not show are written out, here and in the prompt.
        let sneaky = fx("home.json", None, Some("{\"chips\":[{\"label\":\"Plan\u{202E}\",\"prompt\":\"go\u{200B}\"}]}"));
        assert_eq!((&sneaky[0]["label"], &sneaky[0]["prompt"]), (&json!("Plan[U+202E]"), &json!("go[U+200B]")));
        // Styles, tasks and the guide say what they are; the style report is the review's own.
        assert_eq!(fx("custom.css", Some(""), Some(":root { --accent: red }"))[0], json!({"area": "style", "op": "changed", "report": {"kind": "colours", "flags": []}}));
        assert_eq!(fx("workflows/w.json", None, Some(r#"{"name":"Weekly review"}"#))[0], json!({"area": "task", "op": "added", "name": "Weekly review", "path": "workflows/w.json"}));
        assert_eq!(fx("workflows/w.json", Some("{}"), None)[0]["name"], "w");
    }

    #[test]
    fn a_manifest_alone_says_which_permissions_change() {
        let b = r#"{"apiVersion":1,"plugins":{"a.js":{"permissions":["todos:read"]},"gone.js":{}}}"#;
        let a = r#"{"apiVersion":1,"plugins":{"a.js":{"permissions":["todos:read","library:write"],"write":["clients"]},"new.js":{"permissions":["storage"]}}}"#;
        let f = fx("manifest.json", Some(b), Some(a));
        assert_eq!(f.len(), 3, "{f:?}");
        assert_eq!((&f[0]["name"], &f[0]["op"], &f[0]["gained"], &f[0]["writeGained"]), (&json!("a.js"), &json!("changed"), &json!(["library:write"]), &json!(["clients"])));
        assert_eq!((&f[1]["name"], &f[1]["op"]), (&json!("gone.js"), &json!("removed")));
        assert_eq!((&f[2]["name"], &f[2]["op"], &f[2]["permissions"]), (&json!("new.js"), &json!("added"), &json!(["storage"])));
    }

    #[test]
    fn add_ons_say_new_changed_or_already_installed() {
        let root = std::env::temp_dir().join(format!("thoughts-effects-{}", uuid::Uuid::new_v4()));
        let (b, a) = (root.join("live"), root.join("staged"));
        let w = |dir: &Path, p: &str, t: &str| { let f = dir.join(p); std::fs::create_dir_all(f.parent().unwrap()).unwrap(); std::fs::write(f, t).unwrap(); };
        for dir in [&b, &a] {
            w(dir, "plugins/calm.js", "export default () => {}\n");
            w(dir, "plugins/calm.json", r#"{"apiVersion":1,"permissions":["todos:read"],"description":"Quiet list"}"#);
            w(dir, "views/board.html", "<p>board</p>");
        }
        // Live: an add-on that may only read todos. Staged: it may now also write into clients/, a
        // second one is new, the page's code changed and a colour was added.
        w(&b, "plugins/hours.js", "export default () => {}\n");
        w(&b, "plugins/hours.json", r#"{"apiVersion":1,"permissions":["todos:read"]}"#);
        w(&a, "plugins/hours.js", "export default () => {}\n");
        w(&a, "plugins/hours.json", r#"{"apiVersion":1,"permissions":["todos:read","library:write"],"write":["clients/"]}"#);
        w(&a, "plugins/intake.js", "export default () => {}\n");
        w(&a, "views/board.html", "<p>board v2</p>");
        w(&b, "custom.css", "");
        w(&a, "custom.css", ":root { --accent: #0a5c8a }");
        w(&a, "plugins/lib/data.json", "[]");
        let files: Vec<(String, &str)> = vec![("custom.css".into(), "modified"), ("plugins/hours.json".into(), "modified"),
            ("plugins/intake.js".into(), "added"), ("plugins/lib/data.json".into(), "added"), ("views/board.html".into(), "modified")];
        let r = describe(&b, &a, &files);
        let e = r["effects"].as_array().unwrap();
        let hours = e.iter().find(|x| x["name"] == "hours.js").unwrap();
        assert_eq!((&hours["op"], &hours["gained"], &hours["writeGained"], &hours["code"]), (&json!("changed"), &json!(["library:write"]), &json!(["clients"]), &json!(false)));
        assert_eq!(hours["permissions"], json!(["todos:read", "library:write"]), "and everything it may do from now on");
        let intake = e.iter().find(|x| x["name"] == "intake.js").unwrap();
        assert_eq!((&intake["op"], &intake["permissions"]), (&json!("added"), &json!([])));
        let board = e.iter().find(|x| x["name"] == "board.html").unwrap();
        assert_eq!((&board["area"], &board["op"], &board["code"], &board["gained"]), (&json!("page"), &json!("changed"), &json!(true), &json!([])));
        assert!(e.iter().any(|x| x["area"] == "style" && x["report"]["kind"] == "colours"));
        assert!(e.iter().any(|x| x["area"] == "file" && x["path"] == "plugins/lib/data.json" && x["op"] == "added"), "nothing goes unmentioned");
        assert_eq!(e.len(), 5, "{e:?}");
        // The add-on nobody touched is listed apart, with what it may do.
        assert_eq!(r["installed"], json!([{"area": "addon", "name": "calm.js", "description": "Quiet list", "permissions": ["todos:read"], "write": [], "read": []}]));
        // A colours-only change in a layer with add-ons mentions no add-on at all.
        let only = describe(&b, &b, &[("custom.css".into(), "modified")]);
        assert!(only["effects"].as_array().unwrap().iter().all(|x| x["area"] != "addon" && x["area"] != "page"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_folders_an_add_on_reads_are_part_of_what_it_may_do() {
        let b = r#"{"apiVersion":1,"plugins":{"a.js":{"permissions":["library:read"]},"b.js":{"permissions":["library:read"],"read":["clients"]}}}"#;
        let a = r#"{"apiVersion":1,"plugins":{"a.js":{"permissions":["library:read"],"read":["clients"]},"b.js":{"permissions":["library:read"]},"c.js":{"permissions":["ideas:read"],"read":["work/clients"]}}}"#;
        let f = fx("manifest.json", Some(b), Some(a));
        let by = |n: &str| f.iter().find(|x| x["name"] == n).unwrap().clone();
        // Narrowed: now only clients/. Widened: from clients/ to the whole library. New: with its folders.
        assert_eq!((by("a.js")["op"].clone(), by("a.js")["readChanged"].clone(), by("a.js")["read"].clone(), by("a.js")["readBefore"].clone()), (json!("changed"), json!(true), json!(["clients"]), json!([])));
        assert_eq!((by("b.js")["read"].clone(), by("b.js")["readBefore"].clone(), by("b.js")["gained"].clone()), (json!([]), json!(["clients"]), json!([])));
        assert_eq!((by("c.js")["op"].clone(), by("c.js")["read"].clone()), (json!("added"), json!(["work/clients"])));
        // The same list is no change.
        assert!(fx("manifest.json", Some(a), Some(a)).is_empty());
    }
}
