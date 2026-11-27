//! `pages.json`: what a customisation may change on the app's own pages — a page's heading and the
//! line under it, named parts it may hide, named labels it may reword, add-on boxes at its top or
//! bottom, its **sections** (reorder, hide, resize, replace one with an add-on box, insert boxes
//! between them) and an add-on page shown in its place.
//!
//! Every name comes from ONE file shared with the UI, `ui/src/lib/layer/builtin-pages.json` (the
//! "reference"): the UI reads the same lists, and the reference in the layer's README.md is
//! generated from it (`reference_markdown`), so the three cannot drift apart.
//!
//! The app's questions, warnings, error messages, History, the change review, Customise and the
//! Recovery menu are never parts: they cannot be hidden, reworded or replaced here.

use super::validate::show_hidden;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

/// The shared reference, as the UI bundles it.
pub const REFERENCE_JSON: &str = include_str!("../../../../ui/src/lib/layer/builtin-pages.json");

/// One built-in page and what of it may change.
#[derive(Deserialize, Debug, Clone)]
pub struct Page {
    #[serde(skip)]
    pub id: String,
    pub title: bool,
    pub intro: bool,
    pub boxes: bool,
    pub replace: bool,
    pub sections: Vec<String>,
    /// Sections that may be moved but never hidden or replaced (in this, the normal mode).
    #[serde(default)]
    pub locked: Vec<String>,
    pub parts: Vec<String>,
    pub labels: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Nav {
    pub ids: Vec<String>,
    pub never_hidden: Vec<String>,
    /// Entries whose name and icon stay the app's own (the way out of customisations).
    pub fixed_look: Vec<String>,
    pub max_groups: usize,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub version: u32,
    pub sizes: Vec<String>,
    #[serde(deserialize_with = "pages_in_order")]
    pub pages: Vec<Page>,
    pub out_of_reach: Vec<String>,
    pub nav: Nav,
    pub icons: Vec<String>,
    pub renamed: BTreeMap<String, String>,
    pub permissions: Vec<String>,
    pub view_permissions: Vec<String>,
    /// Plugin API method → the permission it needs ("" for none).
    pub api: BTreeMap<String, String>,
}

/// Pages keyed by id, in sidebar order (Settings, which is not in the sidebar, last).
fn pages_in_order<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Page>, D::Error> {
    let map: BTreeMap<String, Page> = BTreeMap::deserialize(d)?;
    let order = ["home", "chats", "ideas", "todos", "docs", "projects", "graph", "inbox", "usage", "settings"];
    let mut v: Vec<Page> = map.into_iter().map(|(id, mut p)| { p.id = id; p }).collect();
    v.sort_by_key(|p| order.iter().position(|x| *x == p.id).unwrap_or(order.len()));
    Ok(v)
}

/// The reference, read once.
pub fn reference() -> &'static Reference {
    static R: OnceLock<Reference> = OnceLock::new();
    R.get_or_init(|| serde_json::from_str(REFERENCE_JSON).expect("ui/src/lib/layer/builtin-pages.json is valid"))
}

pub fn page(id: &str) -> Option<&'static Page> { reference().pages.iter().find(|p| p.id == id) }

const KEYS: &[&str] = &["title", "intro", "hidden", "labels", "boxes", "replace", "sections"];
const SECTION_KEYS: &[&str] = &["order", "hidden", "size", "replace", "insert"];
const PLACES: &[&str] = &["top", "bottom"];
const MAX_BOXES: usize = 6;
const MAX_INSERTS: usize = 12;

fn id_like(s: &str) -> bool {
    !s.is_empty() && s.len() <= 120 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}
fn widget_key(s: &str) -> bool { s.strip_prefix("widget:").is_some_and(id_like) }

/// `{ "en": …, "ar": … }`: at least one language, each a non-empty single line of at most `max` characters.
pub fn text_ok(v: &Value, max: usize) -> bool {
    v.as_object().is_some_and(|m| {
        (m.contains_key("en") || m.contains_key("ar"))
            && m.keys().all(|k| k == "en" || k == "ar")
            && m.values().all(|t| t.as_str().is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= max && !s.contains(['\n', '\r'])))
    })
}

fn join<S: AsRef<str>>(list: &[S], sep: &str) -> String { list.iter().map(|x| format!("`{}`", x.as_ref())).collect::<Vec<_>>().join(sep) }

type Add<'a> = &'a mut dyn FnMut(String, String);

/// A section id as the current version knows it (an id renamed in a later version still works).
pub fn current_id(id: &str) -> &str { reference().renamed.get(id).map(String::as_str).unwrap_or(id) }

fn section_problem(p: &Page, key: &str, s: &str, add: Add) -> bool {
    if p.sections.iter().any(|x| x == current_id(s)) { return false; }
    let s = show_hidden(s, false);
    let id = &p.id;
    add(format!("`{s}` in `{id}.sections.{key}` is not a section of the `{id}` page. Sections: {}.", join(&p.sections, ", ")),
        format!("`{s}` في `{id}.sections.{key}` ليس قسماً من صفحة `{id}`. الأقسام: {}.", join(&p.sections, "، ")));
    true
}

fn sections_problems(p: &Page, v: &Value, add: Add) {
    let id = &p.id;
    let Some(o) = v.as_object() else {
        add(format!("`{id}.sections` must be an object such as {{ \"order\": [\"lists\", \"tools\"], \"hidden\": [\"quickAdd\"] }}."),
            format!("يجب أن تكون `{id}.sections` كائناً مثل {{ \"order\": [\"lists\", \"tools\"], \"hidden\": [\"quickAdd\"] }}."));
        return;
    };
    if p.sections.is_empty() {
        add(format!("The `{id}` page has no sections to arrange: remove `sections` (use `boxes`, `hidden` or `replace`)."),
            format!("لا توجد في صفحة `{id}` أقسام لترتيبها: احذف `sections` (استخدم `boxes` أو `hidden` أو `replace`)."));
        return;
    }
    for key in o.keys().filter(|k| !SECTION_KEYS.contains(&k.as_str())) {
        let k = show_hidden(key, false);
        add(format!("`{k}` under `{id}.sections` is not something sections can do. Use {}.", join(SECTION_KEYS, ", ")),
            format!("`{k}` ضمن `{id}.sections` ليس مما يمكن فعله بالأقسام. استخدم {}.", join(SECTION_KEYS, "، ")));
    }
    let list = |k: &str| o.get(k).map(|v| v.as_array().and_then(|a| a.iter().map(Value::as_str).collect::<Option<Vec<_>>>()));
    match list("order") {
        Some(None) => add(format!("`{id}.sections.order` must be a list of section ids and `widget:<id>` boxes."), format!("يجب أن تكون `{id}.sections.order` قائمة بمعرّفات الأقسام ومربعات `widget:<id>`.")),
        Some(Some(l)) => for s in l { if !widget_key(s) { section_problem(p, "order", s, add); } },
        None => {}
    }
    match list("hidden") {
        Some(None) => add(format!("`{id}.sections.hidden` must be a list of section ids."), format!("يجب أن تكون `{id}.sections.hidden` قائمة بمعرّفات الأقسام.")),
        Some(Some(l)) => for s in l {
            if widget_key(s) || section_problem(p, "hidden", s, add) { continue; }
            if !crate::full_access::on() && p.locked.iter().any(|x| x == current_id(s)) {
                add(format!("The `{s}` section of `{id}` always stays: it can be moved, not hidden."), format!("القسم `{s}` في `{id}` يبقى دائماً: يمكن نقله ولا يمكن إخفاؤه."));
            }
        },
        None => {}
    }
    if let Some(sz) = o.get("size") {
        match sz.as_object() {
            None => add(format!("`{id}.sections.size` must be an object: {{ \"<section>\": \"half\" }}."), format!("يجب أن تكون `{id}.sections.size` كائناً: {{ \"<القسم>\": \"half\" }}.")),
            Some(m) => for (s, v) in m {
                if !widget_key(s) { section_problem(p, "size", s, add); }
                if !v.as_str().is_some_and(|x| reference().sizes.iter().any(|y| y == x)) {
                    add(format!("The size of `{id}.sections.size.{s}` must be one of {}.", join(&reference().sizes, ", ")),
                        format!("يجب أن يكون حجم `{id}.sections.size.{s}` واحداً من {}.", join(&reference().sizes, "، ")));
                }
            },
        }
    }
    if let Some(r) = o.get("replace") {
        match r.as_object() {
            None => add(format!("`{id}.sections.replace` must be an object: {{ \"<section>\": \"widget:<id>\" }}."), format!("يجب أن تكون `{id}.sections.replace` كائناً: {{ \"<القسم>\": \"widget:<id>\" }}.")),
            Some(m) => for (s, v) in m {
                if section_problem(p, "replace", s, add) { continue; }
                if !crate::full_access::on() && p.locked.iter().any(|x| x == current_id(s)) {
                    add(format!("The `{s}` section of `{id}` cannot be replaced."), format!("لا يمكن استبدال القسم `{s}` في `{id}`."));
                } else if !v.as_str().is_some_and(widget_key) {
                    add(format!("`{id}.sections.replace.{s}` must be an add-on box, `widget:<id>` (the `id` given to `ui.addHomeWidget`)."),
                        format!("يجب أن تكون `{id}.sections.replace.{s}` مربع إضافة `widget:<id>` (المعرّف المعطى لـ `ui.addHomeWidget`)."));
                }
            },
        }
    }
    if let Some(ins) = o.get("insert") {
        let bad = || (format!("`{id}.sections.insert` must be a list (at most {MAX_INSERTS}) of {{ \"box\": \"widget:<id>\", \"before\": \"<section>\" }} or with \"after\"."),
            format!("يجب أن تكون `{id}.sections.insert` قائمة ({MAX_INSERTS} على الأكثر) من {{ \"box\": \"widget:<id>\", \"before\": \"<القسم>\" }} أو مع \"after\"."));
        match ins.as_array() {
            Some(a) if a.len() <= MAX_INSERTS => for e in a {
                let box_ok = e["box"].as_str().is_some_and(widget_key);
                let keys_ok = e.as_object().is_some_and(|m| m.keys().all(|k| k == "box" || k == "before" || k == "after"));
                let place = match (e["before"].as_str(), e["after"].as_str()) { (Some(s), None) | (None, Some(s)) => Some(s), _ => None };
                match place {
                    Some(s) if box_ok && keys_ok => { section_problem(p, "insert", s, add); }
                    _ => { let (en, ar) = bad(); add(en, ar); }
                }
            },
            _ => { let (en, ar) = bad(); add(en, ar); }
        }
    }
}

/// Every problem with a `pages.json` as `(english, arabic)`. `staging` is the folder it sits in, to
/// check that an add-on page named in `replace` by its file comes with that file.
pub fn problems(v: &Value, staging: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let mut push = |en: String, ar: String| out.push((en, ar));
    let add: Add = &mut push;
    let Some(map) = v.as_object() else {
        add("This file must be a JSON object: { \"todos\": { … } }.".into(), "يجب أن يكون محتوى هذا الملف كائن JSON مثل { \"todos\": { … } }.".into());
        return out;
    };
    let r = reference();
    let ids: Vec<&str> = r.pages.iter().map(|p| p.id.as_str()).collect();
    for (id, cfg) in map {
        let shown_id = show_hidden(id, false);
        let Some(p) = page(id) else {
            // Full access (`full_access.rs`): the pages kept out of reach may be replaced too.
            if r.out_of_reach.iter().any(|x| x == id) && crate::full_access::on() { continue; }
            if r.out_of_reach.iter().any(|x| x == id) {
                add(format!("`{shown_id}` is where customisations are reviewed, undone or switched off: it always stays as the app made it."),
                    format!("`{shown_id}` هي المكان الذي تُراجَع فيه التخصيصات أو يُتراجع عنها أو تُوقَف: تبقى دائماً كما صنعها التطبيق."));
            } else {
                add(format!("`{shown_id}` is not one of the app's pages. Pages: {}. (Add-on pages are changed in the add-on itself.)", join(&ids, ", ")),
                    format!("`{shown_id}` ليست من صفحات التطبيق. الصفحات: {}. (صفحات الإضافات تُعدَّل في الإضافة نفسها.)", join(&ids, "، ")));
            }
            continue;
        };
        let Some(o) = cfg.as_object() else {
            add(format!("`{id}` must be an object, such as {{ \"title\": {{ \"en\": \"Tasks\" }} }}."),
                format!("يجب أن تكون قيمة `{id}` كائناً، مثل {{ \"title\": {{ \"en\": \"Tasks\" }} }}."));
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
                    format!("يجب أن تكون `{id}.{key}` على هذا الشكل: {{ \"en\": \"…\", \"ar\": \"…\" }}، بلغة واحدة على الأقل، في سطر واحد، وبحد أقصى {max} حرفاً."));
            }
        }
        if let Some(h) = o.get("hidden") {
            match h.as_array().and_then(|a| a.iter().map(Value::as_str).collect::<Option<Vec<_>>>()) {
                None => add(format!("`{id}.hidden` must be a list of part names."), format!("يجب أن تكون `{id}.hidden` قائمة بأسماء الأجزاء.")),
                Some(list) => for part in list {
                    if !p.parts.iter().any(|x| x == part) {
                        let part = show_hidden(part, false);
                        let hint = if p.sections.iter().any(|x| x == &part) { format!(" (`{part}` is a section: hide it with `sections.hidden`)") } else { String::new() };
                        add(format!("`{part}` is not a part of the `{id}` page that can be hidden{hint}. Parts: {}.", join(&p.parts, ", ")),
                            format!("`{part}` ليس جزءاً يمكن إخفاؤه من صفحة `{id}`. الأجزاء: {}.", join(&p.parts, "، ")));
                    }
                },
            }
        }
        if let Some(l) = o.get("labels") {
            match l.as_object() {
                None => add(format!("`{id}.labels` must be an object: {{ \"<name>\": {{ \"en\": text, \"ar\": text }} }}."),
                    format!("يجب أن تكون `{id}.labels` كائناً: {{ \"<الاسم>\": {{ \"en\": نص، \"ar\": نص }} }}.")),
                Some(m) => for (name, t) in m {
                    if !p.labels.iter().any(|x| x == name) {
                        let name = show_hidden(name, false);
                        add(format!("`{name}` is not a text of the `{id}` page that can be renamed. Texts: {}.", join(&p.labels, ", ")),
                            format!("`{name}` ليس نصّاً يمكن تغييره في صفحة `{id}`. النصوص: {}.", join(&p.labels, "، ")));
                    } else if !text_ok(t, 60) {
                        add(format!("The label `{id}.labels.{name}` must look like {{ \"en\": \"New task\", \"ar\": \"مهمة جديدة\" }}: one line, 60 characters at most."),
                            format!("يجب أن تكون التسمية `{id}.labels.{name}` على هذا الشكل: {{ \"en\": \"New task\", \"ar\": \"مهمة جديدة\" }}، في سطر واحد وبحد أقصى ٦٠ حرفاً."));
                    }
                },
            }
        }
        if let Some(b) = o.get("boxes") {
            if !p.boxes {
                add(format!("Boxes are placed on `{id}` with `sections` (`order` or `insert`), not here: remove `boxes`."),
                    format!("تُوضع المربعات في `{id}` من خلال `sections` (`order` أو `insert`) لا من هنا: احذف `boxes`."));
            } else {
                match b.as_object() {
                    None => add(format!("`{id}.boxes` must look like {{ \"top\": [\"widget:<id>\"], \"bottom\": [] }}."),
                        format!("يجب أن تكون `{id}.boxes` على هذا الشكل: {{ \"top\": [\"widget:<id>\"], \"bottom\": [] }}.")),
                    Some(m) => for (place, list) in m {
                        if !PLACES.contains(&place.as_str()) {
                            let place = show_hidden(place, false);
                            add(format!("`{place}` in `{id}.boxes` is not a place: use `top` or `bottom`."), format!("`{place}` في `{id}.boxes` ليس موضعاً: استخدم `top` أو `bottom`."));
                            continue;
                        }
                        let ok = list.as_array().is_some_and(|a| a.len() <= MAX_BOXES && a.iter().all(|x| x.as_str().is_some_and(widget_key)));
                        if !ok {
                            add(format!("`{id}.boxes.{place}` must be a list of at most {MAX_BOXES} add-on boxes, each `widget:<id>` (the `id` given to `ui.addHomeWidget`)."),
                                format!("يجب أن تكون `{id}.boxes.{place}` قائمة من {MAX_BOXES} مربعات على الأكثر، كل منها `widget:<id>` (المعرّف المعطى لـ `ui.addHomeWidget`)."));
                        }
                    },
                }
            }
        }
        if let Some(s) = o.get("sections") { sections_problems(p, s, add); }
        if let Some(rp) = o.get("replace") {
            if !p.replace && !crate::full_access::on() {
                add(format!("The `{id}` page cannot be replaced as a whole: remove `replace`. Arrange its `sections` or change its details instead."),
                    format!("لا يمكن استبدال صفحة `{id}` كاملة: احذف `replace` ورتّب أقسامها `sections` أو غيّر تفاصيلها بدلاً من ذلك."));
            } else {
                match rp.as_str().filter(|s| id_like(s)) {
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
        // Sections: order, hidden, sizes, single-section replacements, inserted boxes.
        let (sb, sa) = (&pb["sections"], &pa["sections"]);
        let (ob, oa) = (strs(&sb["order"]), strs(&sa["order"]));
        if ob != oa { out.push(if oa.is_empty() { json!({"area": "pages", "op": "sectionOrderReset", "page": pg}) } else { json!({"area": "pages", "op": "sectionOrder", "page": pg, "order": oa}) }); }
        let (hb, ha) = (strs(&sb["hidden"]), strs(&sa["hidden"]));
        for what in ha.iter().filter(|x| !hb.contains(x)) { out.push(json!({"area": "pages", "op": "sectionHidden", "page": pg, "what": what})); }
        for what in hb.iter().filter(|x| !ha.contains(x)) { out.push(json!({"area": "pages", "op": "sectionShown", "page": pg, "what": what})); }
        let map = |v: &Value| -> BTreeMap<String, String> {
            v.as_object().into_iter().flatten().filter_map(|(k, x)| Some((line(k), line(x.as_str()?)))).collect()
        };
        let (zb, za) = (map(&sb["size"]), map(&sa["size"]));
        for (what, size) in &za { if zb.get(what) != Some(size) { out.push(json!({"area": "pages", "op": "sectionSize", "page": pg, "what": what, "size": size})); } }
        for what in zb.keys().filter(|k| !za.contains_key(*k)) { out.push(json!({"area": "pages", "op": "sectionSize", "page": pg, "what": what, "size": "full"})); }
        let (rb, ra) = (map(&sb["replace"]), map(&sa["replace"]));
        for (what, by) in &ra { if rb.get(what) != Some(by) { out.push(json!({"area": "pages", "op": "sectionReplaced", "page": pg, "what": what, "by": by})); } }
        for what in rb.keys().filter(|k| !ra.contains_key(*k)) { out.push(json!({"area": "pages", "op": "sectionRestored", "page": pg, "what": what})); }
        let inserts = |v: &Value| -> Vec<(String, String, String)> {
            v["insert"].as_array().into_iter().flatten().filter_map(|e| {
                let bx = line(e["box"].as_str()?);
                match (e["before"].as_str(), e["after"].as_str()) {
                    (Some(s), _) => Some((bx, "before".into(), line(s))),
                    (None, Some(s)) => Some((bx, "after".into(), line(s))),
                    _ => None,
                }
            }).collect()
        };
        let (ib, ia) = (inserts(sb), inserts(sa));
        for (bx, place, what) in ia.iter().filter(|x| !ib.contains(x)) { out.push(json!({"area": "pages", "op": "sectionInsert", "page": pg, "box": bx, "place": place, "what": what})); }
        for (bx, ..) in ib.iter().filter(|x| !ia.iter().any(|y| y.0 == x.0)) { out.push(json!({"area": "pages", "op": "sectionInsertRemoved", "page": pg, "box": bx})); }
        let (rb, ra) = (pb["replace"].as_str().map(line), pa["replace"].as_str().map(line));
        if rb != ra {
            out.push(match ra { Some(by) => json!({"area": "pages", "op": "replaced", "page": pg, "by": by}), None => json!({"area": "pages", "op": "replaceRemoved", "page": pg}) });
        }
    }
    out
}

/// The "Built-in pages reference" of the layer's README.md, generated from the reference so it
/// always says exactly what this version accepts.
pub fn reference_markdown() -> String {
    let r = reference();
    let yes = |b: bool| if b { "yes" } else { "—" };
    let list = |v: &[String]| if v.is_empty() { "—".to_string() } else { join(v, ", ") };
    let mut s = format!("Reference version **{}**. Everything below is checked against this list before a change can be kept.\n\n", r.version);
    s.push_str("| Page | Sections (`sections`) | Parts (`hidden`) | Texts (`labels`) | `title` | `intro` | `boxes` | `replace` |\n|---|---|---|---|---|---|---|---|\n");
    for p in &r.pages {
        let mut secs = list(&p.sections);
        if !p.locked.is_empty() { secs.push_str(&format!(" (always shown: {})", join(&p.locked, ", "))); }
        s.push_str(&format!("| `{}` | {} | {} | {} | {} | {} | {} | {} |\n", p.id, secs, list(&p.parts), list(&p.labels), yes(p.title), yes(p.intro), yes(p.boxes), yes(p.replace)));
    }
    s.push_str(&format!("\nSection sizes: {}.\n\n", join(&r.sizes, ", ")));
    s.push_str(&format!("Never changed by a customisation: {}.\n\n", join(&r.out_of_reach, ", ")));
    s.push_str(&format!("Sidebar ids (`nav.json`): {}, plus any add-on page. Never hidden: {}. Name and icon fixed: {}. At most {} groups.\n\n",
        join(&r.nav.ids, ", "), join(&r.nav.never_hidden, ", "), join(&r.nav.fixed_look, ", "), r.nav.max_groups));
    s.push_str(&format!("Sidebar icons (`nav.json` `icons`): {}.\n\n", join(&r.icons, ", ")));
    if !r.renamed.is_empty() {
        let pairs: Vec<String> = r.renamed.iter().map(|(a, b)| format!("`{a}` → `{b}`")).collect();
        s.push_str(&format!("Renamed ids (the old name still works): {}.\n\n", pairs.join(", ")));
    }
    s.push_str("Plugin API calls and the permission each needs:\n\n| Call | Permission |\n|---|---|\n");
    for (m, perm) in &r.api {
        s.push_str(&format!("| `thoughts.{m}` | {} |\n", if perm.is_empty() { "none".to_string() } else { format!("`{perm}`") }));
    }
    s
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
                      "labels": { "todosTitle": { "en": "Tasks", "ar": "المهام" } },
                      "sections": { "order": ["todos", "hero", "widget:hours"], "hidden": ["stats"], "size": { "todos": "half", "widget:hours": "half" } } },
            "todos": { "title": { "en": "Tasks", "ar": "المهام" }, "hidden": ["calendarExport", "moveDone"],
                       "labels": { "quickAddPlaceholder": { "en": "What needs doing?" } }, "boxes": { "top": ["widget:hours-month"], "bottom": [] },
                       "sections": { "order": ["lists", "tools"], "replace": { "quickAdd": "widget:my-add" }, "insert": [{ "box": "widget:x", "before": "lists" }] } },
            "chats": { "replace": "hours.js:board" },
            "ideas": { "replace": "board" },
            "docs": { "hidden": ["filter"], "boxes": { "bottom": ["widget:x"] }, "replace": "files" },
            "projects": { "hidden": ["intro"] },
            "graph": { "title": { "en": "Map" } }, "inbox": { "replace": "hours.js:inbox" }, "usage": { "sections": { "order": ["cost", "plans"] } },
            "settings": { "sections": { "order": ["modules", "general"], "hidden": ["git"] } }
        }"#);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn wrong_pages_parts_and_shapes_are_named() {
        assert!(probs("[]")[0].contains("JSON object"));
        assert!(probs(r#"{"history":{}}"#)[0].contains("always stays as the app made it"), "History stays out of reach");
        assert!(probs(r#"{"customise":{}}"#)[0].contains("always stays"));
        assert!(probs(r#"{"nowhere":{}}"#)[0].contains("not one of the app's pages"));
        assert!(probs(r#"{"todos":[]}"#)[0].contains("must be an object"));
        assert!(probs(r#"{"todos":{"colour":"red"}}"#)[0].contains("not something a page can change"));
        assert!(probs(r#"{"todos":{"hidden":["errors"]}}"#)[0].contains("`errors` is not a part of the `todos` page"));
        assert!(probs(r#"{"todos":{"hidden":["lists"]}}"#)[0].contains("sections.hidden"), "a section named as a part says where it goes");
        assert!(probs(r#"{"todos":{"hidden":"filters"}}"#)[0].contains("list of part names"));
        assert!(probs(r#"{"todos":{"labels":{"warning":{"en":"x"}}}}"#)[0].contains("not a text of the `todos` page"));
        assert!(probs(r#"{"todos":{"labels":{"add":{"fr":"Ajouter"}}}}"#)[0].contains("New task"));
        assert!(probs(r#"{"todos":{"labels":{"add":{"en":"two\nlines"}}}}"#)[0].contains("one line"));
        assert!(probs(&format!(r#"{{"todos":{{"title":{{"en":"{}"}}}}}}"#, "x".repeat(61)))[0].contains("60 characters"));
        assert!(probs(r#"{"todos":{"title":{"en":"  "}}}"#)[0].contains("at least one language"));
        assert!(probs(r#"{"docs":{"title":{"en":"Files"}}}"#)[0].contains("has no heading"));
        assert!(probs(r#"{"home":{"boxes":{"top":["widget:a"]}}}"#)[0].contains("with `sections`"));
        assert!(probs(r#"{"todos":{"boxes":{"middle":["widget:a"]}}}"#)[0].contains("`top` or `bottom`"));
        assert!(probs(r#"{"todos":{"boxes":{"top":["hours"]}}}"#)[0].contains("widget:<id>"));
        assert!(probs(r#"{"todos":{"boxes":{"top":["widget:a","widget:b","widget:c","widget:d","widget:e","widget:f","widget:g"]}}}"#)[0].contains("at most 6"));
    }

    #[test]
    fn sections_are_checked_against_the_reference() {
        assert!(probs(r#"{"todos":{"sections":{"order":["nope"]}}}"#)[0].contains("not a section of the `todos` page"));
        assert!(probs(r#"{"todos":{"sections":{"spin":true}}}"#)[0].contains("not something sections can do"));
        assert!(probs(r#"{"todos":{"sections":{"size":{"lists":"huge"}}}}"#)[0].contains("must be one of"));
        assert!(probs(r#"{"todos":{"sections":{"replace":{"lists":"hours"}}}}"#)[0].contains("add-on box"));
        assert!(probs(r#"{"todos":{"sections":{"insert":[{"box":"widget:a"}]}}}"#)[0].contains("insert"));
        assert!(probs(r#"{"todos":{"sections":{"insert":[{"box":"widget:a","before":"lists","after":"tools"}]}}}"#)[0].contains("insert"));
        assert!(probs(r#"{"docs":{"sections":{"order":[]}}}"#)[0].contains("no sections"));
        // Settings: its safety-relevant categories can be moved, never hidden or replaced.
        assert!(probs(r#"{"settings":{"sections":{"hidden":["appLayer"]}}}"#)[0].contains("can be moved, not hidden"));
        assert!(probs(r#"{"settings":{"sections":{"replace":{"privacy":"widget:x"}}}}"#)[0].contains("cannot be replaced"));
        assert!(probs(r#"{"settings":{"replace":"x.js:y"}}"#)[0].contains("cannot be replaced as a whole"));
    }

    #[test]
    fn every_page_but_settings_can_be_replaced_and_a_replacement_needs_its_file() {
        for p in &reference().pages { assert_eq!(p.replace, p.id != "settings", "{}", p.id); }
        assert!(probs(r#"{"home":{"replace":"hours.js:board"}}"#).is_empty());
        assert!(probs(r#"{"todos":{"replace":"gone.js:board"}}"#)[0].contains("plugins/gone.js"));
        assert!(probs(r#"{"todos":{"replace":"view:cal.html"}}"#)[0].contains("views/cal.html"));
        assert!(probs(r#"{"todos":{"replace":"https://example.com"}}"#)[0].contains("id of an add-on page"));
        assert!(probs(r#"{"todos":{"replace":3}}"#)[0].contains("id of an add-on page"));
    }

    /// Section ids are a promise to whoever customised a page: one may only go away by being
    /// renamed (`renamed` in the reference), so a kept customisation keeps working after an update.
    #[test]
    fn section_ids_survive_releases() {
        let v2: &[(&str, &[&str])] = &[
            ("home", &["hero", "suggestions", "stats", "usage", "ideas", "upcoming", "todos", "chats", "projects", "automate"]),
            ("chats", &["actions", "tools", "list", "keep"]), ("ideas", &["capture", "tools", "list"]),
            ("todos", &["tools", "quickAdd", "lists", "calendar"]), ("projects", &["linkForm", "list"]),
            ("inbox", &["list"]), ("graph", &["graph"]), ("usage", &["plans", "cost"]),
            ("settings", &["general", "accessibility", "ai", "usage", "git", "privacy", "modules", "daemon", "appLayer", "maintenance"]),
        ];
        for (pg, ids) in v2 {
            let p = page(pg).unwrap_or_else(|| panic!("page {pg} disappeared"));
            for id in *ids { assert!(p.sections.iter().any(|x| x == current_id(id)), "{pg}.{id} was removed without an entry in `renamed`"); }
        }
        assert!(reference().version >= 2);
    }

    #[test]
    fn the_review_packs_and_the_ui_know_the_file() {
        let dir = std::env::temp_dir().join(format!("thoughts-pages-w-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pages.json"), r#"{"todos":{"hidden":["errors"]}}"#).unwrap();
        let p = super::super::validate::problems(&dir, &[("pages.json".into(), "added")], false);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["path"], "pages.json");
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
    fn the_customising_assistant_is_taught_every_name() {
        let contract = crate::applayer::contract();
        assert!(contract.contains("### pages.json") && contract.contains("| `pages.json` |"));
        assert!(!contract.contains("{{BUILTIN_PAGES}}"), "the reference is filled in");
        let r = reference();
        for p in &r.pages {
            assert!(contract.contains(&format!("| `{}` |", p.id)), "{}", p.id);
            for name in p.parts.iter().chain(&p.labels).chain(&p.sections) { assert!(contract.contains(&format!("`{name}`")), "{}.{name}", p.id); }
        }
        for icon in &r.icons { assert!(contract.contains(&format!("`{icon}`")), "{icon}"); }
        for m in r.api.keys() { assert!(contract.contains(&format!("`thoughts.{m}`")), "{m}"); }
        let d = crate::applayer::Dirs::at(&std::env::temp_dir().join("thoughts-pages-prompt"));
        let prompt = super::super::system_prompt(&d);
        assert!(prompt.contains("pages.json") && prompt.contains("nav.json") && prompt.contains("Built-in pages reference"));
    }

    #[test]
    fn the_reference_agrees_with_the_service() {
        let r = reference();
        assert_eq!(r.permissions.iter().map(String::as_str).collect::<Vec<_>>(), crate::applayer::PERMISSIONS);
        assert_eq!(r.view_permissions.iter().map(String::as_str).collect::<Vec<_>>(), crate::applayer::VIEW_PERMISSIONS);
        for perm in r.api.values().filter(|p| !p.is_empty()) { assert!(r.permissions.contains(perm), "{perm}"); }
    }

    #[test]
    fn problems_come_in_both_languages() {
        let dir = std::env::temp_dir();
        let p = problems(&json!({"todos": {"hidden": ["nope"]}}), &dir);
        assert!(p[0].1.contains("ليس جزءاً يمكن إخفاؤه"));
    }

    #[test]
    fn effects_say_what_changes_page_by_page() {
        let b = json!({"todos": {"title": {"en": "Tasks"}, "hidden": ["filters"], "labels": {"add": {"en": "Add"}}, "replace": "a.js:x",
                                 "sections": {"size": {"tools": "half"}, "replace": {"quickAdd": "widget:q"}}}});
        let a = json!({"todos": {"title": {"en": "Tasks", "ar": "المهام"}, "hidden": ["calendarExport"], "labels": {"quickAddPlaceholder": {"en": "What next?"}},
                                 "boxes": {"top": ["widget:hours"]},
                                 "sections": {"order": ["lists", "tools"], "hidden": ["quickAdd"], "size": {"lists": "wide"}, "insert": [{"box": "widget:x", "after": "tools"}]}},
                       "ideas": {"intro": {"en": "My clients\u{202E}"}}});
        assert_eq!(effects(&b, &a), vec![
            json!({"area": "pages", "op": "title", "page": "todos", "en": "Tasks", "ar": "المهام"}),
            json!({"area": "pages", "op": "hidden", "page": "todos", "what": "calendarExport"}),
            json!({"area": "pages", "op": "shown", "page": "todos", "what": "filters"}),
            json!({"area": "pages", "op": "renamed", "page": "todos", "what": "quickAddPlaceholder", "en": "What next?", "ar": ""}),
            json!({"area": "pages", "op": "renameRemoved", "page": "todos", "what": "add"}),
            json!({"area": "pages", "op": "boxes", "page": "todos", "place": "top", "boxes": ["widget:hours"]}),
            json!({"area": "pages", "op": "sectionOrder", "page": "todos", "order": ["lists", "tools"]}),
            json!({"area": "pages", "op": "sectionHidden", "page": "todos", "what": "quickAdd"}),
            json!({"area": "pages", "op": "sectionSize", "page": "todos", "what": "lists", "size": "wide"}),
            json!({"area": "pages", "op": "sectionSize", "page": "todos", "what": "tools", "size": "full"}),
            json!({"area": "pages", "op": "sectionRestored", "page": "todos", "what": "quickAdd"}),
            json!({"area": "pages", "op": "sectionInsert", "page": "todos", "box": "widget:x", "place": "after", "what": "tools"}),
            json!({"area": "pages", "op": "replaceRemoved", "page": "todos"}),
            json!({"area": "pages", "op": "intro", "page": "ideas", "en": "My clients[U+202E]", "ar": ""}),
        ]);
        assert_eq!(effects(&json!({"chats": {"title": {"en": "Talks"}}}), &Value::Null), vec![json!({"area": "pages", "op": "titleRemoved", "page": "chats"})]);
        assert!(effects(&a, &a).is_empty());
    }
}
