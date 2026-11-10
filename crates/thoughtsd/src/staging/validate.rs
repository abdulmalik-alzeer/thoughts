//! Validation of a staged change before the user reviews it. Every problem is a plain sentence in
//! the UI language about one file; while any exist the change cannot be kept.

use crate::applayer::{clean_write_folder, API_VERSION, PERMISSIONS, VIEW_PERMISSIONS};
use crate::packs::{MAX_FILES, MAX_FILE_BYTES, MAX_TOTAL_BYTES};
use serde_json::{json, Map, Value};
use std::path::Path;

const HOME_SECTIONS: &[&str] = &["stats", "ideas", "todos", "chats", "projects", "upcoming"];
/// Pages that must always stay reachable.
const NAV_ALWAYS: &[&str] = &["home", "settings", "help"];

struct Out { arabic: bool, list: Vec<Value> }

impl Out {
    fn add(&mut self, path: &str, en: impl Into<String>, ar: impl Into<String>) {
        if self.list.len() >= 50 { return; }
        self.list.push(json!({"path": path, "message": if self.arabic { ar.into() } else { en.into() }}));
    }
}

fn id_like(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

fn strings<'a>(v: &'a Value) -> Option<Vec<&'a str>> {
    v.as_array()?.iter().map(Value::as_str).collect()
}

/// `{path, message}` for everything wrong with the staged files `files` (`(path, status)`).
pub fn problems(staging: &Path, files: &[(String, &str)], arabic: bool) -> Vec<Value> {
    let mut out = Out { arabic, list: vec![] };
    let live: Vec<&str> = files.iter().filter(|(_, s)| *s != "deleted").map(|(p, _)| p.as_str()).collect();
    if live.len() > MAX_FILES {
        out.add("", format!("The change touches too many files ({}; the limit is {MAX_FILES}).", live.len()),
            format!("يشمل التغيير عددًا كبيرًا من الملفات ({}، والحد الأقصى {MAX_FILES}).", live.len()));
    }
    let mut total = 0usize;
    for path in live {
        let full = staging.join(path);
        let Ok(meta) = std::fs::symlink_metadata(&full) else { continue };
        if !meta.is_file() {
            out.add(path, "This is not an ordinary file.", "هذا ليس ملفًا عاديًا.");
            continue;
        }
        total += meta.len() as usize;
        if meta.len() as usize > MAX_FILE_BYTES {
            out.add(path, "This file is too large (the limit is 512 KB).", "هذا الملف كبير جدًا (الحد الأقصى ٥١٢ كيلوبايت).");
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&full) else {
            out.add(path, "This file is not plain text.", "هذا الملف ليس نصًّا عاديًا.");
            continue;
        };
        if !path.ends_with(".json") { continue; }
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                out.add(path, format!("This file is not valid JSON (line {}, column {}).", e.line(), e.column()),
                    format!("صيغة هذا الملف غير صحيحة (السطر {}، العمود {}).", e.line(), e.column()));
                continue;
            }
        };
        let parts: Vec<&str> = path.split('/').collect();
        match parts.as_slice() {
            ["manifest.json"] => manifest(&mut out, path, &v),
            ["home.json"] => home(&mut out, path, &v),
            ["nav.json"] => nav(&mut out, path, &v),
            ["todos.json"] => todos(&mut out, path, &v),
            ["themes.json"] => themes(&mut out, path, &v),
            ["workflows", _] => workflow(&mut out, path, &v),
            [sub @ ("plugins" | "views"), file] => {
                let stem = file.trim_end_matches(".json");
                let ext = if *sub == "plugins" { "js" } else { "html" };
                if !staging.join(sub).join(format!("{stem}.{ext}")).is_file() {
                    out.add(path, format!("This describes `{sub}/{stem}.{ext}`, but there is no such file."),
                        format!("هذا الملف يصف `{sub}/{stem}.{ext}`، لكن لا يوجد ملف بهذا الاسم."));
                }
                match v.as_object() {
                    Some(o) => declaration(&mut out, path, o, *sub == "views", true),
                    None => not_object(&mut out, path),
                }
            }
            _ => {}
        }
    }
    if total > MAX_TOTAL_BYTES {
        out.add("", "The change is too large in total (the limit is 4 MB).", "حجم التغيير الإجمالي كبير جدًا (الحد الأقصى ٤ ميغابايت).");
    }
    out.list
}

fn not_object(out: &mut Out, path: &str) {
    out.add(path, "This file must be a JSON object: { … }.", "يجب أن يكون محتوى هذا الملف كائن JSON بين قوسين { … }.");
}

fn api_version(out: &mut Out, path: &str, o: &Map<String, Value>, required: bool) {
    match o.get("apiVersion") {
        None if !required => {}
        None => out.add(path, "`apiVersion` is missing; it must be 1.", "الحقل `apiVersion` مفقود، ويجب أن يكون 1."),
        Some(v) => match v.as_u64() {
            Some(n) if n >= 1 && n <= API_VERSION => {}
            Some(n) if n > API_VERSION => out.add(path, format!("This needs a newer version of the app (apiVersion {n}; this app supports {API_VERSION})."),
                format!("يتطلب هذا إصدارًا أحدث من التطبيق (apiVersion {n}، وهذا التطبيق يدعم {API_VERSION}).")),
            _ => out.add(path, "`apiVersion` must be the number 1.", "يجب أن تكون قيمة `apiVersion` الرقم 1."),
        },
    }
}

/// One plugin or view declaration: `{apiVersion?, permissions?, write?, description?}`.
fn declaration(out: &mut Out, path: &str, o: &Map<String, Value>, view: bool, own_file: bool) {
    api_version(out, path, o, own_file);
    let allowed = if view { VIEW_PERMISSIONS } else { PERMISSIONS };
    let mut has_write = false;
    match o.get("permissions") {
        None => {}
        Some(p) => match strings(p) {
            None => out.add(path, "`permissions` must be a list of permission names.", "يجب أن يكون `permissions` قائمة بأسماء الصلاحيات."),
            Some(list) => for name in list {
                has_write |= name == "library:write";
                if allowed.contains(&name) { continue; }
                if view && PERMISSIONS.contains(&name) {
                    out.add(path, format!("A view can only read: `{name}` is not allowed here. Use a plugin instead."),
                        format!("صفحات العرض للقراءة فقط: الصلاحية `{name}` غير مسموحة هنا. استخدم إضافة برمجية بدلًا منها."));
                } else {
                    out.add(path, format!("Unknown permission `{name}`. Allowed: {}.", allowed.join(", ")),
                        format!("صلاحية غير معروفة `{name}`. المسموح: {}.", allowed.join("، ")));
                }
            },
        },
    }
    match o.get("write") {
        None => if has_write {
            out.add(path, "`library:write` needs a `write` list naming the folders it may write into.",
                "الصلاحية `library:write` تحتاج إلى قائمة `write` تحدّد المجلدات المسموح بالكتابة فيها.");
        },
        Some(w) => match strings(w) {
            None => out.add(path, "`write` must be a list of folder names.", "يجب أن يكون `write` قائمة بأسماء مجلدات."),
            Some(list) => {
                if view { out.add(path, "A view cannot write: remove `write`.", "صفحات العرض لا تكتب: احذف `write`."); }
                else if !has_write && !list.is_empty() {
                    out.add(path, "`write` folders only work together with the `library:write` permission.",
                        "قائمة `write` لا تعمل إلا مع الصلاحية `library:write`.");
                }
                if has_write && list.is_empty() {
                    out.add(path, "`write` is empty: name at least one folder.", "قائمة `write` فارغة: حدّد مجلدًا واحدًا على الأقل.");
                }
                if list.len() > 20 { out.add(path, "`write` lists too many folders (at most 20).", "قائمة `write` طويلة جدًا (٢٠ مجلدًا على الأكثر)."); }
                for folder in list {
                    if clean_write_folder(folder).is_err() {
                        out.add(path, format!("`{folder}` is not a folder a plugin may write to. Use a folder inside the library such as `clients/` — no `..`, no absolute paths, never `.claude` or `.git`."),
                            format!("لا يمكن للإضافة الكتابة في `{folder}`. استخدم مجلدًا داخل المكتبة مثل `clients/`، دون `..` ودون مسارات مطلقة، وليس `.claude` أو `.git`."));
                    }
                }
            }
        },
    }
    if o.get("description").map_or(false, |d| !d.is_string()) {
        out.add(path, "`description` must be text.", "يجب أن يكون `description` نصًّا.");
    }
}

fn manifest(out: &mut Out, path: &str, v: &Value) {
    let Some(o) = v.as_object() else { return not_object(out, path) };
    api_version(out, path, o, true);
    for (key, ext, view) in [("plugins", ".js", false), ("views", ".html", true)] {
        let Some(section) = o.get(key) else { continue };
        let Some(map) = section.as_object() else {
            out.add(path, format!("`{key}` must be an object keyed by file name."), format!("يجب أن يكون `{key}` كائنًا مفاتيحه أسماء الملفات."));
            continue;
        };
        for (name, decl) in map {
            if !name.ends_with(ext) || name.contains('/') {
                out.add(path, format!("`{name}` under `{key}` must be a file name ending in `{ext}`."), format!("يجب أن يكون `{name}` ضمن `{key}` اسم ملف ينتهي بـ `{ext}`."));
            }
            match decl.as_object() {
                Some(d) => declaration(out, path, d, view, false),
                None => out.add(path, format!("The entry for `{name}` must be an object."), format!("يجب أن يكون إدخال `{name}` كائنًا.")),
            }
        }
    }
}

fn home(out: &mut Out, path: &str, v: &Value) {
    let Some(o) = v.as_object() else { return not_object(out, path) };
    if let Some(chips) = o.get("chips") {
        let ok = chips.as_array().map_or(false, |a| a.iter().all(|c| c["label"].as_str().map_or(false, |l| !l.trim().is_empty()) && c["prompt"].is_string()));
        if !ok { out.add(path, "`chips` must be a list of { \"label\": text, \"prompt\": text }.", "يجب أن يكون `chips` قائمة من { \"label\": نص، \"prompt\": نص }."); }
    }
    if let Some(hidden) = o.get("hidden") {
        match strings(hidden) {
            None => out.add(path, "`hidden` must be a list of section names.", "يجب أن يكون `hidden` قائمة بأسماء الأقسام."),
            Some(list) => for s in list {
                if !HOME_SECTIONS.contains(&s) && s != "suggestions" && !s.strip_prefix("widget:").map_or(false, id_like) {
                    out.add(path, format!("`{s}` in `hidden` is not a Home section. Sections: {}.", HOME_SECTIONS.join(", ")),
                        format!("`{s}` في `hidden` ليس قسمًا من أقسام الصفحة الرئيسية. الأقسام: {}.", HOME_SECTIONS.join("، ")));
                }
            },
        }
    }
    if let Some(order) = o.get("order") {
        match strings(order) {
            None => out.add(path, "`order` must be a list of section names.", "يجب أن يكون `order` قائمة بأسماء الأقسام."),
            Some(list) => for s in list {
                if !HOME_SECTIONS.contains(&s) && s != "suggestions" && !s.strip_prefix("widget:").map_or(false, id_like) {
                    out.add(path, format!("`{s}` in `order` is not something Home can place. Use a section ({}), `suggestions`, or `widget:<id>`.", HOME_SECTIONS.join(", ")),
                        format!("`{s}` في `order` ليس عنصرًا يمكن ترتيبه في الصفحة الرئيسية. استخدم قسمًا ({})، أو `suggestions`، أو `widget:<id>`.", HOME_SECTIONS.join("، ")));
                }
            },
        }
    }
}

fn nav(out: &mut Out, path: &str, v: &Value) {
    let Some(o) = v.as_object() else { return not_object(out, path) };
    for key in ["hidden", "order"] {
        let Some(list) = o.get(key) else { continue };
        match strings(list) {
            None => out.add(path, format!("`{key}` must be a list of page ids."), format!("يجب أن يكون `{key}` قائمة بمعرّفات الصفحات.")),
            Some(ids) => for id in ids {
                if !id_like(id) {
                    out.add(path, format!("`{id}` in `{key}` is not a page id."), format!("`{id}` في `{key}` ليس معرّف صفحة."));
                } else if key == "hidden" && NAV_ALWAYS.contains(&id.to_lowercase().as_str()) {
                    out.add(path, format!("`{id}` can never be hidden from the sidebar."), format!("لا يمكن إخفاء `{id}` من الشريط الجانبي."));
                }
            },
        }
    }
    if let Some(labels) = o.get("labels") {
        match labels.as_object() {
            None => out.add(path, "`labels` must be an object: { \"<page id>\": { \"en\": text, \"ar\": text } }.", "يجب أن يكون `labels` كائنًا: { \"<معرّف الصفحة>\": { \"en\": نص، \"ar\": نص } }."),
            Some(map) => for (id, l) in map {
                let texts_ok = l.as_object().map_or(false, |m| {
                    (m.contains_key("en") || m.contains_key("ar"))
                        && ["en", "ar"].iter().all(|k| m.get(*k).map_or(true, |t| t.as_str().map_or(false, |s| !s.trim().is_empty() && s.chars().count() <= 40)))
                });
                if !id_like(id) || !texts_ok {
                    out.add(path, format!("The label for `{id}` must look like {{ \"en\": \"Clients\", \"ar\": \"العملاء\" }} (short text, at least one language)."),
                        format!("يجب أن تكون تسمية `{id}` على هذا الشكل: {{ \"en\": \"Clients\", \"ar\": \"العملاء\" }} (نص قصير، بلغة واحدة على الأقل)."));
                }
            },
        }
    }
}

fn todos(out: &mut Out, path: &str, v: &Value) {
    let Some(o) = v.as_object() else { return not_object(out, path) };
    for (key, allowed) in [("groupBy", &["list", "due", "tag", "none"][..]), ("sort", &["list", "due"][..]), ("filter", &["all", "soon", "overdue"][..])] {
        if let Some(val) = o.get(key) {
            if !val.as_str().map_or(false, |s| allowed.contains(&s)) {
                let options = allowed.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>();
                out.add(path, format!("`{key}` must be one of {}.", options.join(", ")), format!("يجب أن تكون قيمة `{key}` واحدة من: {}.", options.join("، ")));
            }
        }
    }
    if o.get("showDone").map_or(false, |b| !b.is_boolean()) {
        out.add(path, "`showDone` must be true or false.", "يجب أن تكون قيمة `showDone` إما true أو false.");
    }
}

fn themes(out: &mut Out, path: &str, v: &Value) {
    let ok = v.as_array().map_or(false, |a| a.iter().all(|t| {
        t["id"].as_str().map_or(false, |id| !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
            && t["name"].as_str().map_or(false, |n| !n.trim().is_empty())
    }));
    if !ok {
        out.add(path, "This must be a list of themes like [{ \"id\": \"sand\", \"name\": \"Sand\" }]; `id` uses lowercase letters, digits and dashes.",
            "يجب أن يكون هذا الملف قائمة سمات مثل [{ \"id\": \"sand\", \"name\": \"Sand\" }]، ويتكوّن `id` من أحرف لاتينية صغيرة وأرقام وشرطات.");
    }
}

fn workflow(out: &mut Out, path: &str, v: &Value) {
    let Some(o) = v.as_object() else { return not_object(out, path) };
    if o.get("name").map_or(false, |n| !n.as_str().map_or(false, |s| s.chars().count() <= 80 && !s.chars().any(char::is_control))) {
        out.add(path, "`name` must be a short line of text.", "يجب أن يكون `name` نصًّا قصيرًا في سطر واحد.");
    }
    if !o.get("prompt").and_then(Value::as_str).map_or(false, |p| !p.trim().is_empty() && p.len() <= 20_000) {
        out.add(path, "`prompt` is missing: say what the assistant should do when this runs.", "الحقل `prompt` مفقود: اكتب ما على المساعد فعله عند التشغيل.");
    }
    let t = &o.get("trigger").cloned().unwrap_or(Value::Null);
    let probe = json!({"name": "x", "prompt": "x", "trigger": t});
    let understood = serde_json::from_value::<crate::workflows::Workflow>(probe).map_or(false, |w| crate::workflows::valid(&w));
    if understood { return; }
    let (en, ar): (String, String) = match (t["type"].as_str(), t["every"].as_str()) {
        (Some("event"), _) => ("An event trigger must be { \"type\": \"event\", \"on\": \"wrap.done\" } or \"app.start\".".into(),
            "مشغّل الحدث يجب أن يكون { \"type\": \"event\", \"on\": \"wrap.done\" } أو \"app.start\".".into()),
        (Some("schedule"), _) if !t["at"].as_str().map_or(false, |a| a.len() == 5 && chrono::NaiveTime::parse_from_str(a, "%H:%M").is_ok()) =>
            ("`at` must be a 24-hour time such as \"09:00\".".into(), "يجب أن يكون `at` وقتًا بنظام ٢٤ ساعة مثل \"09:00\".".into()),
        (Some("schedule"), Some("month")) => ("A monthly schedule needs `day`: a number from 1 to 28, or \"last\".".into(),
            "الجدول الشهري يحتاج إلى `day`: رقم من 1 إلى 28، أو \"last\".".into()),
        (Some("schedule"), Some("week")) => ("`weekday` must be a number from 0 (Sunday) to 6 (Saturday).".into(),
            "يجب أن يكون `weekday` رقمًا من 0 (الأحد) إلى 6 (السبت).".into()),
        (Some("schedule"), Some("weekdays")) => ("`days` must be a list of weekdays, each from 0 (Sunday) to 6 (Saturday).".into(),
            "يجب أن يكون `days` قائمة بأيام الأسبوع، كلٌّ منها من 0 (الأحد) إلى 6 (السبت).".into()),
        (Some("schedule"), _) => ("`every` must be \"day\", \"weekdays\", \"week\" or \"month\".".into(),
            "يجب أن تكون قيمة `every` واحدة من: \"day\" أو \"weekdays\" أو \"week\" أو \"month\".".into()),
        _ => ("`trigger` must have \"type\": \"schedule\" or \"event\".".into(), "يجب أن يحتوي `trigger` على \"type\": \"schedule\" أو \"event\".".into()),
    };
    out.add(path, en, ar);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(files: &[(&str, &str)], arabic: bool) -> Vec<(String, String)> {
        let d = std::env::temp_dir().join(format!("thoughts-validate-{}", uuid::Uuid::new_v4()));
        let mut list = vec![];
        for (p, text) in files {
            let full = d.join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, text).unwrap();
            list.push((p.to_string(), "added"));
        }
        let out = problems(&d, &list, arabic).into_iter().map(|p| (p["path"].as_str().unwrap().to_string(), p["message"].as_str().unwrap().to_string())).collect();
        std::fs::remove_dir_all(d).unwrap();
        out
    }
    fn one(path: &str, text: &str) -> Vec<String> { check(&[(path, text)], false).into_iter().map(|(_, m)| m).collect() }

    #[test]
    fn a_correct_change_has_no_problems() {
        let p = check(&[
            ("custom.css", ":root { --radius: 4px }"),
            ("manifest.json", r#"{"apiVersion":1,"plugins":{"a.js":{"permissions":["todos:read"]}},"views":{"c.html":{"permissions":["todos:read"]}}}"#),
            ("plugins/intake.js", "export default () => {}"),
            ("plugins/intake.json", r#"{"apiVersion":1,"permissions":["library:write","storage"],"write":["clients/","notes/intake"],"description":"Client intake form"}"#),
            ("views/cal.html", "<p></p>"),
            ("views/cal.json", r#"{"apiVersion":1,"permissions":["todos:read"]}"#),
            ("home.json", r#"{"chips":[{"label":"Plan","prompt":"Plan my week: "}],"hidden":["stats"],"order":["widget:hours-month","suggestions","todos"]}"#),
            ("nav.json", r#"{"hidden":["projects"],"order":["home","todos","client-intake"],"labels":{"ideas":{"en":"Clients","ar":"العملاء"}}}"#),
            ("todos.json", r#"{"groupBy":"due","sort":"due","filter":"all","showDone":false}"#),
            ("themes.json", r#"[{"id":"sand","name":"Sand"}]"#),
            ("workflows/month-end.json", r#"{"name":"Month end","trigger":{"type":"schedule","every":"month","at":"09:00","day":"last"},"prompt":"Summarise the month."}"#),
            ("workflows/standup.json", r#"{"trigger":{"type":"schedule","every":"weekdays","at":"08:30"},"prompt":"List today's todos."}"#),
        ], false);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn json_syntax_and_shapes() {
        assert!(one("home.json", "{\n  \"chips\": [,]\n}")[0].contains("not valid JSON (line 2"));
        assert!(one("home.json", "[]")[0].contains("JSON object"));
        assert_eq!(one("home.json", r#"{"chips":[{"label":"x"}],"hidden":["sidebar"],"order":["widget:","todos"]}"#).len(), 3);
        assert!(one("nav.json", r#"{"hidden":["home"]}"#)[0].contains("can never be hidden"));
        assert_eq!(one("nav.json", r#"{"hidden":["Settings","help","docs"],"order":"todos","labels":{"ideas":{"fr":"Clients"},"docs":{"en":""}}}"#).len(), 5);
        assert_eq!(one("todos.json", r#"{"groupBy":"project","sort":"az","filter":"mine","showDone":"yes"}"#).len(), 4);
        assert!(one("todos.json", r#"{"groupBy":"project"}"#)[0].contains("\"list\", \"due\", \"tag\", \"none\""));
        assert_eq!(one("themes.json", r#"[{"id":"Sand Theme","name":"Sand"}]"#).len(), 1);
        assert_eq!(one("themes.json", "{}").len(), 1);
        // Anything else that is JSON under plugins/ deeper down is the plugin's own business.
        assert!(check(&[("plugins/lib/data.json", "[1,2]"), ("plugins/a.js", "")], false).is_empty());
    }

    #[test]
    fn manifests_permissions_write_folders_and_api_version() {
        let with_js = |json: &str| check(&[("plugins/p.js", "export default () => {}"), ("plugins/p.json", json)], false).into_iter().map(|(_, m)| m).collect::<Vec<_>>();
        assert!(with_js(r#"{"apiVersion":1,"permissions":["todos:write"]}"#).is_empty());
        assert!(with_js(r#"{"permissions":[]}"#)[0].contains("`apiVersion` is missing"));
        assert!(with_js(r#"{"apiVersion":2,"permissions":[]}"#)[0].contains("newer version of the app"));
        assert!(with_js(r#"{"apiVersion":"1"}"#)[0].contains("must be the number 1"));
        assert!(with_js(r#"{"apiVersion":1,"permissions":["network"]}"#)[0].contains("Unknown permission `network`"));
        assert!(with_js(r#"{"apiVersion":1,"permissions":"todos:read"}"#)[0].contains("must be a list"));
        assert!(with_js(r#"{"apiVersion":1,"permissions":["library:write"]}"#)[0].contains("needs a `write` list"));
        assert!(with_js(r#"{"apiVersion":1,"permissions":["library:write"],"write":[]}"#)[0].contains("is empty"));
        assert!(with_js(r#"{"apiVersion":1,"permissions":["library:read"],"write":["clients"]}"#)[0].contains("only work together"));
        for bad in ["../outside", "/etc", "~/x", ".git", "notes/.claude/skills", "a/../b", "", ".", "C:\\x"] {
            let p = with_js(&json!({"apiVersion": 1, "permissions": ["library:write"], "write": [bad]}).to_string());
            assert!(p.iter().any(|m| m.contains("not a folder a plugin may write to")), "{bad}: {p:?}");
        }
        assert!(one("plugins/ghost.json", r#"{"apiVersion":1}"#)[0].contains("there is no such file"));
        assert!(one("plugins/p.json", "[]").iter().any(|m| m.contains("JSON object")));
        // Views only read.
        let view = check(&[("views/v.html", ""), ("views/v.json", r#"{"apiVersion":1,"permissions":["todos:write","storage"],"write":["x"]}"#)], false);
        assert_eq!(view.len(), 3, "{view:?}");
        assert!(view[0].1.contains("A view can only read"));
        // The shared manifest: same rules per entry, version required.
        assert!(one("manifest.json", r#"{"plugins":{}}"#)[0].contains("`apiVersion` is missing"));
        assert!(one("manifest.json", r#"{"apiVersion":3}"#)[0].contains("newer version"));
        let m = one("manifest.json", r#"{"apiVersion":1,"plugins":{"a":{"permissions":["x"]},"b.js":5},"views":[]}"#);
        assert_eq!(m.len(), 4, "{m:?}");
    }

    #[test]
    fn workflows_sizes_and_arabic() {
        let w = |trigger: &str| one("workflows/w.json", &format!(r#"{{"name":"W","trigger":{trigger},"prompt":"Do it."}}"#));
        assert!(w(r#"{"type":"schedule","every":"month","at":"09:00"}"#)[0].contains("1 to 28"));
        assert!(w(r#"{"type":"schedule","every":"month","at":"09:00","day":31}"#)[0].contains("1 to 28"));
        assert!(w(r#"{"type":"schedule","every":"day","at":"9am"}"#)[0].contains("24-hour"));
        assert!(w(r#"{"type":"schedule","every":"week","at":"09:00","weekday":9}"#)[0].contains("0 (Sunday)"));
        assert!(w(r#"{"type":"schedule","every":"hour","at":"09:00"}"#)[0].contains("`every` must be"));
        assert!(w(r#"{"type":"event","on":"chat.message"}"#)[0].contains("event trigger"));
        assert!(w(r#"{"type":"cron"}"#)[0].contains("\"schedule\" or \"event\""));
        assert!(w(r#"{"type":"event","on":"wrap.done"}"#).is_empty());
        assert!(one("workflows/w.json", r#"{"trigger":{"type":"event","on":"app.start"}}"#)[0].contains("`prompt` is missing"));
        assert!(one("custom.css", &"x".repeat(MAX_FILE_BYTES + 1))[0].contains("too large"));
        let many: Vec<(String, String)> = (0..=MAX_FILES).map(|i| (format!("plugins/p{i}.js"), String::new())).collect();
        let refs: Vec<(&str, &str)> = many.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        assert!(check(&refs, false)[0].1.contains("too many files"));
        // Deleted files are not checked; messages come in the UI language.
        let d = std::env::temp_dir().join(format!("thoughts-validate-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(problems(&d, &[("home.json".into(), "deleted")], false).is_empty());
        std::fs::remove_dir_all(d).unwrap();
        let ar = check(&[("todos.json", r#"{"showDone":1}"#), ("nav.json", "{")], true);
        assert!(ar.iter().any(|(p, m)| p == "todos.json" && m.contains("يجب أن تكون")) && ar.iter().any(|(p, m)| p == "nav.json" && m.contains("السطر")));
    }
}
