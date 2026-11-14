//! `pages.json`: details of the app's own pages that a customisation may change — a page's
//! heading and the line under it, named parts it may hide, named labels it may reword, add-on boxes
//! placed at its top or bottom, and an add-on page shown in its place. Only the names listed here
//! exist; the UI (`ui/src/lib/pages.svelte.ts`) knows the same names and ignores anything else, and
//! a page whose replacement is missing or fails shows the app's own page again.
//!
//! The app's questions, warnings, error messages and the Recovery menu are never parts: they
//! cannot be hidden or reworded here.

use super::validate::show_hidden;
use serde_json::{json, Value};
use std::path::Path;

/// One built-in page and what of it may change.
pub struct Page {
    pub id: &'static str,
    /// Parts that `hidden` may name.
    pub parts: &'static [&'static str],
    /// Texts that `labels` may reword.
    pub labels: &'static [&'static str],
    /// `title` / `intro` / `boxes` / `replace` allowed on this page.
    pub title: bool,
    pub intro: bool,
    pub boxes: bool,
    pub replace: bool,
}

pub const PAGES: &[Page] = &[
    // Home's sections and widgets are placed with `home.json`; here are the details around them.
    Page { id: "home", parts: &["date", "greeting", "intro", "emblem", "todoChip", "noteChip", "checkInChip"],
        labels: &["askPlaceholder", "todoChip", "noteChip", "ideasTitle", "ideasIntro", "upcomingTitle", "todosTitle", "chatsTitle", "projectsTitle"],
        title: true, intro: true, boxes: false, replace: false },
    Page { id: "chats", parts: &["newChat", "folderChats", "search", "select", "filters", "systemChats", "keep"],
        labels: &["newChat", "search", "empty"], title: true, intro: true, boxes: true, replace: true },
    Page { id: "ideas", parts: &["capture", "areas", "search", "statusFilter", "sort", "count"],
        labels: &["capturePlaceholder", "capture", "search"], title: true, intro: true, boxes: true, replace: true },
    Page { id: "todos", parts: &["count", "views", "calendarExport", "filters", "grouping", "sorting", "showDone", "moveDone", "quickAdd"],
        labels: &["quickAddPlaceholder", "add", "listPlaceholder"], title: true, intro: true, boxes: true, replace: true },
    // Docs opens files by path (links, `me/`): it is never replaced, and it has no heading of its own.
    Page { id: "docs", parts: &["filter"], labels: &["filter"], title: false, intro: false, boxes: true, replace: false },
    Page { id: "projects", parts: &["intro", "linkForm"], labels: &["link", "newChat"], title: true, intro: true, boxes: true, replace: true },
];

const KEYS: &[&str] = &["title", "intro", "hidden", "labels", "boxes", "replace"];
const PLACES: &[&str] = &["top", "bottom"];
const MAX_BOXES: usize = 6;

fn page(id: &str) -> Option<&'static Page> { PAGES.iter().find(|p| p.id == id) }

fn id_like(s: &str) -> bool {
    !s.is_empty() && s.len() <= 120 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

/// `{ "en": …, "ar": … }`: at least one language, each a non-empty single line of at most `max` characters.
fn text_ok(v: &Value, max: usize) -> bool {
    v.as_object().map_or(false, |m| {
        (m.contains_key("en") || m.contains_key("ar"))
            && m.keys().all(|k| k == "en" || k == "ar")
            && m.values().all(|t| t.as_str().map_or(false, |s| !s.trim().is_empty() && s.chars().count() <= max && !s.contains(['\n', '\r'])))
    })
}

fn join(list: &[&str], sep: &str) -> String { list.iter().map(|x| format!("`{x}`")).collect::<Vec<_>>().join(sep) }

/// Every problem with a `pages.json` as `(english, arabic)`. `staging` is the folder it sits in, to
/// check that an add-on page named in `replace` by its file comes with that file.
pub fn problems(v: &Value, staging: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let mut add = |en: String, ar: String| out.push((en, ar));
    let Some(map) = v.as_object() else {
        add("This file must be a JSON object: { \"todos\": { … } }.".into(), "يجب أن يكون محتوى هذا الملف كائن JSON مثل { \"todos\": { … } }.".into());
        return out;
    };
    let ids: Vec<&str> = PAGES.iter().map(|p| p.id).collect();
    for (id, cfg) in map {
        let shown_id = show_hidden(id, false);
        let Some(p) = page(id) else {
            add(format!("`{shown_id}` is not one of the app's pages. Pages: {}. (Add-on pages are changed in the add-on itself.)", join(&ids, ", ")),
                format!("`{shown_id}` ليست من صفحات التطبيق. الصفحات: {}. (صفحات الإضافات تُعدَّل في الإضافة نفسها.)", join(&ids, "، ")));
            continue;
        };
        let Some(o) = cfg.as_object() else {
            add(format!("`{id}` must be an object, such as {{ \"title\": {{ \"en\": \"Tasks\" }} }}."),
                format!("يجب أن تكون قيمة `{id}` كائنًا، مثل {{ \"title\": {{ \"en\": \"Tasks\" }} }}."));
            continue;
        };
        for key in o.keys() {
            if !KEYS.contains(&key.as_str()) {
                let k = show_hidden(key, false);
                add(format!("`{k}` under `{id}` is not something a page can change. Use {}.", join(KEYS, ", ")),
                    format!("`{k}` ضمن `{id}` ليس مما يمكن تغييره في الصفحة. استخدم {}.", join(KEYS, "، ")));
            }
        }
        for (key, allowed, max, what_en, what_ar) in [("title", p.title, 60, "heading", "العنوان"), ("intro", p.intro, 300, "line under the heading", "السطر تحت العنوان")] {
            let Some(t) = o.get(key) else { continue };
            if !allowed {
                add(format!("The `{id}` page has no {what_en} to change: remove `{key}`."), format!("لا توجد في صفحة `{id}` خانة «{what_ar}» لتغييرها: احذف `{key}`."));
            } else if !text_ok(t, max) {
                add(format!("`{id}.{key}` must look like {{ \"en\": \"…\", \"ar\": \"…\" }}: at least one language, one line, {max} characters at most."),
                    format!("يجب أن تكون `{id}.{key}` على هذا الشكل: {{ \"en\": \"…\", \"ar\": \"…\" }}، بلغة واحدة على الأقل، في سطر واحد، وبحد أقصى {max} حرفًا."));
            }
        }
        if let Some(h) = o.get("hidden") {
            match h.as_array().and_then(|a| a.iter().map(Value::as_str).collect::<Option<Vec<_>>>()) {
                None => add(format!("`{id}.hidden` must be a list of part names."), format!("يجب أن تكون `{id}.hidden` قائمة بأسماء الأجزاء.")),
                Some(list) => for part in list {
                    if !p.parts.contains(&part) {
                        let part = show_hidden(part, false);
                        add(format!("`{part}` is not a part of the `{id}` page that can be hidden. Parts: {}.", join(p.parts, ", ")),
                            format!("`{part}` ليس جزءًا يمكن إخفاؤه من صفحة `{id}`. الأجزاء: {}.", join(p.parts, "، ")));
                    }
                },
            }
        }
        if let Some(l) = o.get("labels") {
            match l.as_object() {
                None => add(format!("`{id}.labels` must be an object: {{ \"<name>\": {{ \"en\": text, \"ar\": text }} }}."),
                    format!("يجب أن تكون `{id}.labels` كائنًا: {{ \"<الاسم>\": {{ \"en\": نص، \"ar\": نص }} }}.")),
                Some(m) => for (name, t) in m {
                    if !p.labels.contains(&name.as_str()) {
                        let name = show_hidden(name, false);
                        add(format!("`{name}` is not a text of the `{id}` page that can be renamed. Texts: {}.", join(p.labels, ", ")),
                            format!("`{name}` ليس نصًّا يمكن تغييره في صفحة `{id}`. النصوص: {}.", join(p.labels, "، ")));
                    } else if !text_ok(t, 60) {
                        add(format!("The label `{id}.labels.{name}` must look like {{ \"en\": \"New task\", \"ar\": \"مهمة جديدة\" }}: one line, 60 characters at most."),
                            format!("يجب أن تكون التسمية `{id}.labels.{name}` على هذا الشكل: {{ \"en\": \"New task\", \"ar\": \"مهمة جديدة\" }}، في سطر واحد وبحد أقصى ٦٠ حرفًا."));
                    }
                },
            }
        }
        if let Some(b) = o.get("boxes") {
            if !p.boxes {
                add(format!("Boxes are placed on `{id}` with home.json (`order`), not here: remove `boxes`."),
                    format!("تُرتَّب المربعات في `{id}` من خلال home.json (`order`) لا من هنا: احذف `boxes`."));
            } else {
                match b.as_object() {
                    None => add(format!("`{id}.boxes` must look like {{ \"top\": [\"widget:<id>\"], \"bottom\": [] }}."),
                        format!("يجب أن تكون `{id}.boxes` على هذا الشكل: {{ \"top\": [\"widget:<id>\"], \"bottom\": [] }}.")),
                    Some(m) => for (place, list) in m {
                        if !PLACES.contains(&place.as_str()) {
                            let place = show_hidden(place, false);
                            add(format!("`{place}` in `{id}.boxes` is not a place: use `top` or `bottom`."), format!("`{place}` في `{id}.boxes` ليس موضعًا: استخدم `top` أو `bottom`."));
                            continue;
                        }
                        let ok = list.as_array().map_or(false, |a| a.len() <= MAX_BOXES
                            && a.iter().all(|x| x.as_str().and_then(|s| s.strip_prefix("widget:")).map_or(false, id_like)));
                        if !ok {
                            add(format!("`{id}.boxes.{place}` must be a list of at most {MAX_BOXES} add-on boxes, each `widget:<id>` (the `id` given to `ui.addHomeWidget`)."),
                                format!("يجب أن تكون `{id}.boxes.{place}` قائمة من {MAX_BOXES} مربعات على الأكثر، كل منها `widget:<id>` (المعرّف المعطى لـ `ui.addHomeWidget`)."));
                        }
                    },
                }
            }
        }
        if let Some(r) = o.get("replace") {
            if !p.replace {
                add(format!("The `{id}` page cannot be replaced: remove `replace`. Change its details instead."),
                    format!("لا يمكن استبدال صفحة `{id}`: احذف `replace` وغيّر تفاصيلها بدلًا من ذلك."));
            } else {
                match r.as_str().filter(|s| id_like(s)) {
                    None => add(format!("`{id}.replace` must be the id of an add-on page, such as \"hours.js:board\"."),
                        format!("يجب أن تكون `{id}.replace` معرّف صفحة من إضافة، مثل \"hours.js:board\".")),
                    Some(s) => {
                        // A page named by its add-on's file must come with that file.
                        let file = s.strip_prefix("view:").filter(|f| f.ends_with(".html")).map(|f| format!("views/{f}"))
                            .or_else(|| s.split_once(':').filter(|(f, _)| f.ends_with(".js")).map(|(f, _)| format!("plugins/{f}")));
                        if let Some(file) = file {
                            if !staging.join(&file).is_file() {
                                add(format!("`{id}.replace` names a page of `{file}`, but there is no such file."),
                                    format!("تشير `{id}.replace` إلى صفحة في `{file}`، لكن لا يوجد ملف بهذا الاسم."));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// What a `pages.json` going from `b` to `a` changes, entry by entry (`{area: "pages", op, page, …}`).
pub fn effects(b: &Value, a: &Value) -> Vec<Value> {
    let line = |s: &str| show_hidden(s.trim(), false);
    let text = |v: &Value| -> Option<(String, String)> {
        let o = v.as_object()?;
        let get = |k: &str| o.get(k).and_then(Value::as_str).map(line).unwrap_or_default();
        let (en, ar) = (get("en"), get("ar"));
        (!en.is_empty() || !ar.is_empty()).then_some((en, ar))
    };
    let strs = |v: &Value| -> Vec<String> { v.as_array().into_iter().flatten().filter_map(Value::as_str).map(line).collect() };
    let mut ids: Vec<String> = vec![];
    for m in [b, a] {
        for k in m.as_object().into_iter().flat_map(|o| o.keys()) { if !ids.contains(k) { ids.push(k.clone()); } }
    }
    let mut out = vec![];
    for id in ids {
        let (pb, pa) = (&b[&id], &a[&id]);
        let pg = line(&id);
        for key in ["title", "intro"] {
            match (text(&pb[key]), text(&pa[key])) {
                (x, Some((en, ar))) if x.as_ref() != Some(&(en.clone(), ar.clone())) => out.push(json!({"area": "pages", "op": key, "page": pg, "en": en, "ar": ar})),
                (Some(_), None) => out.push(json!({"area": "pages", "op": format!("{key}Removed"), "page": pg})),
                _ => {}
            }
        }
        let (hb, ha) = (strs(&pb["hidden"]), strs(&pa["hidden"]));
        for what in ha.iter().filter(|x| !hb.contains(x)) { out.push(json!({"area": "pages", "op": "hidden", "page": pg, "what": what})); }
        for what in hb.iter().filter(|x| !ha.contains(x)) { out.push(json!({"area": "pages", "op": "shown", "page": pg, "what": what})); }
        let labels = |v: &Value| -> Vec<(String, (String, String))> {
            v["labels"].as_object().into_iter().flatten().filter_map(|(k, t)| Some((line(k), text(t)?))).collect()
        };
        let (lb, la) = (labels(pb), labels(pa));
        for (what, (en, ar)) in &la {
            if !lb.iter().any(|(w, t)| w == what && t == &(en.clone(), ar.clone())) {
                out.push(json!({"area": "pages", "op": "renamed", "page": pg, "what": what, "en": en, "ar": ar}));
            }
        }
        for (what, _) in lb.iter().filter(|(w, _)| !la.iter().any(|(x, _)| x == w)) {
            out.push(json!({"area": "pages", "op": "renameRemoved", "page": pg, "what": what}));
        }
        for place in PLACES {
            let (xb, xa) = (strs(&pb["boxes"][place]), strs(&pa["boxes"][place]));
            if xb != xa { out.push(json!({"area": "pages", "op": "boxes", "page": pg, "place": place, "boxes": xa})); }
        }
        let (rb, ra) = (pb["replace"].as_str().map(line), pa["replace"].as_str().map(line));
        if rb != ra {
            out.push(match ra { Some(by) => json!({"area": "pages", "op": "replaced", "page": pg, "by": by}), None => json!({"area": "pages", "op": "replaceRemoved", "page": pg}) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probs(text: &str) -> Vec<String> {
        let dir = std::env::temp_dir().join(format!("thoughts-pages-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("plugins")).unwrap();
        std::fs::write(dir.join("plugins/hours.js"), "export default () => {}").unwrap();
        let out = problems(&serde_json::from_str(text).unwrap(), &dir).into_iter().map(|(en, _)| en).collect();
        std::fs::remove_dir_all(dir).unwrap();
        out
    }

    #[test]
    fn a_correct_pages_file_has_no_problems() {
        let p = probs(r#"{
            "home": { "title": { "en": "{greeting}, team" }, "intro": { "en": "Our desk", "ar": "مكتبنا" }, "hidden": ["emblem", "noteChip"],
                      "labels": { "todosTitle": { "en": "Tasks", "ar": "المهام" } } },
            "todos": { "title": { "en": "Tasks", "ar": "المهام" }, "hidden": ["calendarExport", "moveDone"],
                       "labels": { "quickAddPlaceholder": { "en": "What needs doing?" } }, "boxes": { "top": ["widget:hours-month"], "bottom": [] } },
            "chats": { "replace": "hours.js:board" },
            "ideas": { "replace": "board" },
            "docs": { "hidden": ["filter"], "boxes": { "bottom": ["widget:x"] } },
            "projects": { "hidden": ["intro"] }
        }"#);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn wrong_pages_parts_and_shapes_are_named() {
        assert!(probs("[]")[0].contains("JSON object"));
        assert!(probs(r#"{"settings":{}}"#)[0].contains("not one of the app's pages"), "Settings and the rest stay out of reach");
        assert!(probs(r#"{"todos":[]}"#)[0].contains("must be an object"));
        assert!(probs(r#"{"todos":{"colour":"red"}}"#)[0].contains("not something a page can change"));
        assert!(probs(r#"{"todos":{"hidden":["errors"]}}"#)[0].contains("`errors` is not a part of the `todos` page"));
        assert!(probs(r#"{"todos":{"hidden":"filters"}}"#)[0].contains("list of part names"));
        assert!(probs(r#"{"todos":{"labels":{"warning":{"en":"x"}}}}"#)[0].contains("not a text of the `todos` page"));
        assert!(probs(r#"{"todos":{"labels":{"add":{"fr":"Ajouter"}}}}"#)[0].contains("New task"));
        assert!(probs(r#"{"todos":{"labels":{"add":{"en":"two\nlines"}}}}"#)[0].contains("one line"));
        assert!(probs(&format!(r#"{{"todos":{{"title":{{"en":"{}"}}}}}}"#, "x".repeat(61)))[0].contains("60 characters"));
        assert!(probs(r#"{"todos":{"title":{"en":"  "}}}"#)[0].contains("at least one language"));
        assert!(probs(r#"{"docs":{"title":{"en":"Files"}}}"#)[0].contains("has no heading"));
        assert!(probs(r#"{"home":{"boxes":{"top":["widget:a"]}}}"#)[0].contains("home.json"));
        assert!(probs(r#"{"todos":{"boxes":{"middle":["widget:a"]}}}"#)[0].contains("`top` or `bottom`"));
        assert!(probs(r#"{"todos":{"boxes":{"top":["hours"]}}}"#)[0].contains("widget:<id>"));
        assert!(probs(r#"{"todos":{"boxes":{"top":["widget:a","widget:b","widget:c","widget:d","widget:e","widget:f","widget:g"]}}}"#)[0].contains("at most 6"));
    }

    #[test]
    fn home_and_docs_are_never_replaced_and_a_replacement_needs_its_file() {
        assert!(probs(r#"{"home":{"replace":"hours.js:board"}}"#)[0].contains("cannot be replaced"));
        assert!(probs(r#"{"docs":{"replace":"hours.js:board"}}"#)[0].contains("cannot be replaced"));
        assert!(probs(r#"{"todos":{"replace":"gone.js:board"}}"#)[0].contains("plugins/gone.js"));
        assert!(probs(r#"{"todos":{"replace":"view:cal.html"}}"#)[0].contains("views/cal.html"));
        assert!(probs(r#"{"todos":{"replace":"https://example.com"}}"#)[0].contains("id of an add-on page"));
        assert!(probs(r#"{"todos":{"replace":3}}"#)[0].contains("id of an add-on page"));
    }

    #[test]
    fn the_review_packs_and_the_ui_know_the_file() {
        // The staged-change check reads it…
        let dir = std::env::temp_dir().join(format!("thoughts-pages-w-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pages.json"), r#"{"todos":{"hidden":["errors"]}}"#).unwrap();
        let p = super::super::validate::problems(&dir, &[("pages.json".into(), "added")], false);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["path"], "pages.json");
        // …and so does what the review says, packs, and what the app loads.
        assert_eq!(super::super::effects::file_effects("pages.json", None, Some(r#"{"chats":{"hidden":["keep"]}}"#)).unwrap(),
            vec![json!({"area": "pages", "op": "hidden", "page": "chats", "what": "keep"})]);
        assert!(crate::packs::check_path("pages.json").is_ok());
        std::fs::write(dir.join("pages.json"), r#"{"todos":{"hidden":["filters"]}}"#).unwrap();
        assert_eq!(crate::applayer::manifest_of(&dir)["pages"], json!({"todos": {"hidden": ["filters"]}}));
        std::fs::write(dir.join("pages.json"), "[1]").unwrap();
        assert_eq!(crate::applayer::manifest_of(&dir)["pages"], Value::Null, "only an object is handed to the app");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn problems_come_in_both_languages() {
        let dir = std::env::temp_dir();
        let p = problems(&json!({"todos": {"hidden": ["nope"]}}), &dir);
        assert!(p[0].1.contains("ليس جزءًا يمكن إخفاؤه"));
    }

    #[test]
    fn effects_say_what_changes_page_by_page() {
        let b = json!({"todos": {"title": {"en": "Tasks"}, "hidden": ["filters"], "labels": {"add": {"en": "Add"}}, "replace": "a.js:x"}});
        let a = json!({"todos": {"title": {"en": "Tasks", "ar": "المهام"}, "hidden": ["calendarExport"], "labels": {"quickAddPlaceholder": {"en": "What next?"}},
                                 "boxes": {"top": ["widget:hours"]}},
                       "ideas": {"intro": {"en": "My clients\u{202E}"}}});
        assert_eq!(effects(&b, &a), vec![
            json!({"area": "pages", "op": "title", "page": "todos", "en": "Tasks", "ar": "المهام"}),
            json!({"area": "pages", "op": "hidden", "page": "todos", "what": "calendarExport"}),
            json!({"area": "pages", "op": "shown", "page": "todos", "what": "filters"}),
            json!({"area": "pages", "op": "renamed", "page": "todos", "what": "quickAddPlaceholder", "en": "What next?", "ar": ""}),
            json!({"area": "pages", "op": "renameRemoved", "page": "todos", "what": "add"}),
            json!({"area": "pages", "op": "boxes", "page": "todos", "place": "top", "boxes": ["widget:hours"]}),
            json!({"area": "pages", "op": "replaceRemoved", "page": "todos"}),
            json!({"area": "pages", "op": "intro", "page": "ideas", "en": "My clients[U+202E]", "ar": ""}),
        ]);
        // Removing the file gives everything back; nothing that matters changed says nothing.
        assert_eq!(effects(&json!({"chats": {"title": {"en": "Talks"}}}), &Value::Null), vec![json!({"area": "pages", "op": "titleRemoved", "page": "chats"})]);
        assert!(effects(&a, &a).is_empty());
    }
}
