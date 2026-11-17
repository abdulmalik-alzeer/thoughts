//! Validation of a staged change before the user reviews it. Every problem is a plain sentence in
//! the UI language about one file; while any exist the change cannot be kept.

use crate::applayer::{clean_write_folder, API_VERSION, PERMISSIONS, READ_PERMISSIONS, VIEW_PERMISSIONS};
use crate::packs::{MAX_FILES, MAX_FILE_BYTES, MAX_TOTAL_BYTES};
use serde_json::{json, Map, Value};
use std::path::Path;

const HOME_SECTIONS: &[&str] = &["stats", "ideas", "todos", "chats", "projects", "upcoming", "usage"];
/// Pages that must always stay reachable.
const NAV_ALWAYS: &[&str] = &["home", "settings", "help"];

struct Out { arabic: bool, list: Vec<Value> }

impl Out {
    fn add(&mut self, path: &str, en: impl Into<String>, ar: impl Into<String>) {
        if self.list.len() >= 50 { return; }
        self.list.push(json!({"path": path, "message": if self.arabic { ar.into() } else { en.into() }}));
    }
}

// ---------- characters that do not show ----------

/// Characters that are invisible, or that change the direction text is drawn in: zero-width and
/// bidi controls, tag characters, variation selectors, blank "filler" letters, line and paragraph
/// separators (most of Unicode category Cf and a few look-alikes). The visible Arabic signs that
/// share that category (the number sign, the end-of-ayah mark…) are not in the list.
pub fn is_hidden_char(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x034F | 0x061C | 0x115F | 0x1160 | 0x17B4 | 0x17B5 | 0x180B..=0x180F
        | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F | 0x3164 | 0xFE00..=0xFE0F | 0xFEFF
        | 0xFFA0 | 0xFFF9..=0xFFFB | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0000..=0xE0FFF)
}

/// Is the character at `i` one a reader would not see? Control characters count (line breaks and
/// tabs are fine in `multiline` text). A few invisible characters are part of ordinary writing and
/// pass where they belong: an emoji or text selector right after the character it styles, a joiner
/// between two non-ASCII characters (inside an emoji such as a family or a profession, or inside
/// a Persian word), and a single direction mark as Arabic keyboards type them. Runs of them, and
/// all the others, never pass.
fn offends(chars: &[char], i: usize, multiline: bool) -> bool {
    let c = chars[i];
    if c.is_control() { return !(multiline && matches!(c, '\n' | '\r' | '\t')); }
    if !is_hidden_char(c) { return false; }
    let at = |j: Option<usize>| j.and_then(|j| chars.get(j)).copied();
    let (prev, next) = (at(i.checked_sub(1)), at(Some(i + 1)));
    let shown = |x: Option<char>| x.map_or(false, |x| !is_hidden_char(x) && !x.is_control() && !x.is_whitespace());
    let wide = |x: Option<char>| shown(x) && x.map_or(false, |x| !x.is_ascii());
    match c as u32 {
        0xFE0E | 0xFE0F => !shown(prev),
        0x200D => !((wide(prev) || (prev == Some('\u{FE0F}') && shown(at(i.checked_sub(2))))) && wide(next)),
        0x200C => !(wide(prev) && wide(next)),
        0x200E | 0x200F | 0x061C => prev.map_or(false, is_hidden_char) || next.map_or(false, is_hidden_char),
        _ => true,
    }
}

/// The first character of `s` that a reader would not see (see `offends`).
pub fn first_hidden(s: &str, multiline: bool) -> Option<char> {
    let chars: Vec<char> = s.chars().collect();
    (0..chars.len()).find(|i| offends(&chars, *i, multiline)).map(|i| chars[i])
}

/// `s` with every such character written out (`[U+202E]`), so that what is read is what is there.
/// A long run is shortened to its first few and a count (`[+40]`).
pub fn show_hidden(s: &str, multiline: bool) -> String {
    const SHOWN: usize = 8;
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut run = 0usize;
    for i in 0..chars.len() {
        if offends(&chars, i, multiline) {
            run += 1;
            if run <= SHOWN { out.push_str(&format!("[U+{:04X}]", chars[i] as u32)); }
            continue;
        }
        if run > SHOWN { out.push_str(&format!("[+{}]", run - SHOWN)); }
        run = 0;
        out.push(chars[i]);
    }
    if run > SHOWN { out.push_str(&format!("[+{}]", run - SHOWN)); }
    out
}

/// One problem per file for text a reader would not see, wherever it sits in the file (keys too).
fn no_hidden_text(out: &mut Out, path: &str, v: &Value) {
    fn find(v: &Value, at: &str) -> Option<(String, char)> {
        match v {
            Value::String(s) => first_hidden(s, true).map(|c| (at.to_string(), c)),
            Value::Array(a) => a.iter().enumerate().find_map(|(i, x)| find(x, &format!("{at}[{i}]"))),
            Value::Object(o) => o.iter().find_map(|(k, x)| {
                let here = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
                first_hidden(k, false).map(|c| (show_hidden(&here, false), c)).or_else(|| find(x, &here))
            }),
            _ => None,
        }
    }
    if let Some((at, c)) = find(v, "") {
        let code = format!("U+{:04X}", c as u32);
        let at = show_hidden(&at, false);
        out.add(path, format!("This file contains a character that does not show on screen ({code}), in `{at}`. Remove it, so that what you read is all there is."),
            format!("يحتوي هذا الملف على حرف لا يظهر على الشاشة ({code}) في `{at}`. احذفه ليكون ما تقرؤه هو كل ما في الملف."));
    }
}

/// An add-on or page is known to the app by its file name (its storage is kept under it), so the
/// name must be one every part of the app accepts.
fn plain_name(out: &mut Out, path: &str, file: &str) {
    if crate::layer_api::valid_plugin_name(file) { return; }
    out.add(path, "Add-on and page file names may only use English letters, digits, dots, dashes and underscores, 60 characters at most (for example `hours.js`). Rename this file.",
        "أسماء ملفات الإضافات والصفحات تقبل الأحرف الإنجليزية والأرقام والنقطة والشرطة والشرطة السفلية فقط، وبحد أقصى ٦٠ حرفاً (مثل `hours.js`). غيّر اسم هذا الملف.");
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
            format!("يشمل التغيير عدداً كبيراً من الملفات ({}، والحد الأقصى {MAX_FILES}).", live.len()));
    }
    let mut total = 0usize;
    for path in live {
        let full = staging.join(path);
        let Ok(meta) = std::fs::symlink_metadata(&full) else { continue };
        if !meta.is_file() {
            out.add(path, "This is not an ordinary file.", "هذا ليس ملفاً عادياً.");
            continue;
        }
        total += meta.len() as usize;
        if meta.len() as usize > MAX_FILE_BYTES {
            out.add(path, "This file is too large (the limit is 512 KB).", "هذا الملف كبير جداً (الحد الأقصى ٥١٢ كيلوبايت).");
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&full) else {
            out.add(path, "This file is not plain text.", "هذا الملف ليس نصّاً عادياً.");
            continue;
        };
        let parts: Vec<&str> = path.split('/').collect();
        if let ["plugins" | "views", file] = parts.as_slice() {
            if [".js", ".html", ".json"].iter().any(|ext| file.ends_with(ext)) { plain_name(&mut out, path, file); }
        }
        if !path.ends_with(".json") { continue; }
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                out.add(path, format!("This file is not valid JSON (line {}, column {}).", e.line(), e.column()),
                    format!("صيغة هذا الملف غير صحيحة (السطر {}، العمود {}).", e.line(), e.column()));
                continue;
            }
        };
        // Everything the app reads these files for is shown to the user or handed to the assistant.
        if matches!(parts.as_slice(), [_] | ["plugins" | "views" | "workflows", _]) { no_hidden_text(&mut out, path, &v); }
        match parts.as_slice() {
            ["manifest.json"] => manifest(&mut out, path, &v),
            ["home.json"] => home(&mut out, path, &v),
            ["nav.json"] => nav(&mut out, path, &v),
            ["todos.json"] => todos(&mut out, path, &v),
            ["pages.json"] => for (en, ar) in super::pages::problems(&v, staging) { out.add(path, en, ar) },
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
        out.add("", "The change is too large in total (the limit is 4 MB).", "حجم التغيير الإجمالي كبير جداً (الحد الأقصى ٤ ميغابايت).");
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
                format!("يتطلب هذا إصداراً أحدث من التطبيق (apiVersion {n}، وهذا التطبيق يدعم {API_VERSION}).")),
            _ => out.add(path, "`apiVersion` must be the number 1.", "يجب أن تكون قيمة `apiVersion` الرقم 1."),
        },
    }
}

/// One plugin or view declaration: `{apiVersion?, permissions?, write?, description?}`.
fn declaration(out: &mut Out, path: &str, o: &Map<String, Value>, view: bool, own_file: bool) {
    api_version(out, path, o, own_file);
    let allowed = if view { VIEW_PERMISSIONS } else { PERMISSIONS };
    // `library:write` needs `write` folders; `ideas:write` may name them too (it only reaches the
    // items inside them, so without any it changes nothing).
    let (mut has_write, mut may_write) = (false, false);
    match o.get("permissions") {
        None => {}
        Some(p) => match strings(p) {
            None => out.add(path, "`permissions` must be a list of permission names.", "يجب أن يكون `permissions` قائمة بأسماء الصلاحيات."),
            Some(list) => for name in list {
                has_write |= name == "library:write";
                may_write |= name == "library:write" || name == "ideas:write";
                if allowed.contains(&name) { continue; }
                if view && PERMISSIONS.contains(&name) {
                    out.add(path, format!("A view can only read: `{name}` is not allowed here. Use a plugin instead."),
                        format!("صفحات العرض للقراءة فقط: الصلاحية `{name}` غير مسموحة هنا. استخدم إضافة برمجية بدلاً منها."));
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
                else if !may_write && !list.is_empty() {
                    out.add(path, "`write` folders only work together with the `library:write` or `ideas:write` permission.",
                        "قائمة `write` لا تعمل إلا مع الصلاحية `library:write` أو `ideas:write`.");
                }
                if has_write && list.is_empty() {
                    out.add(path, "`write` is empty: name at least one folder.", "قائمة `write` فارغة: حدّد مجلداً واحداً على الأقل.");
                }
                if list.len() > 20 { out.add(path, "`write` lists too many folders (at most 20).", "قائمة `write` طويلة جداً (٢٠ مجلداً على الأكثر)."); }
                for folder in list {
                    if clean_write_folder(folder).is_err() {
                        out.add(path, format!("`{folder}` is not a folder a plugin may write to. Use a folder inside the library such as `clients/` — no `..`, no absolute paths, never `.claude` or `.git`."),
                            format!("لا يمكن للإضافة الكتابة في `{folder}`. استخدم مجلداً داخل المكتبة مثل `clients/`، دون `..` ودون مسارات مطلقة، وليس `.claude` أو `.git`."));
                    }
                }
            }
        },
    }
    // `read` narrows what it reads (files, todos, ideas) to these folders. Left out, it reads the
    // whole library except private folders.
    if let Some(r) = o.get("read") {
        let reads = strings(o.get("permissions").unwrap_or(&Value::Null)).map_or(false, |l| l.iter().any(|p| READ_PERMISSIONS.contains(p)));
        match strings(r) {
            None => out.add(path, "`read` must be a list of folder names.", "يجب أن يكون `read` قائمة بأسماء مجلدات."),
            Some(list) => {
                if !reads {
                    out.add(path, "`read` folders only work together with `library:read`, `todos:read` or `ideas:read`.",
                        "قائمة `read` لا تعمل إلا مع الصلاحية `library:read` أو `todos:read` أو `ideas:read`.");
                }
                if list.is_empty() {
                    out.add(path, "`read` is empty: name at least one folder, or leave `read` out to read the whole library.",
                        "قائمة `read` فارغة: حدّد مجلداً واحداً على الأقل، أو احذف `read` لتقرأ المكتبة كلها.");
                }
                if list.len() > 20 { out.add(path, "`read` lists too many folders (at most 20).", "قائمة `read` طويلة جداً (٢٠ مجلداً على الأكثر)."); }
                for folder in list {
                    if clean_write_folder(folder).is_err() {
                        out.add(path, format!("`{folder}` is not a folder an add-on may read. Use a folder inside the library such as `clients/` — no `..`, no absolute paths, never `.claude` or `.git`."),
                            format!("لا يمكن للإضافة القراءة من `{folder}`. استخدم مجلداً داخل المكتبة مثل `clients/`، دون `..` ودون مسارات مطلقة، وليس `.claude` أو `.git`."));
                    }
                }
            }
        }
    }
    if o.get("description").map_or(false, |d| !d.is_string()) {
        out.add(path, "`description` must be text.", "يجب أن يكون `description` نصّاً.");
    }
}

fn manifest(out: &mut Out, path: &str, v: &Value) {
    let Some(o) = v.as_object() else { return not_object(out, path) };
    api_version(out, path, o, true);
    for (key, ext, view) in [("plugins", ".js", false), ("views", ".html", true)] {
        let Some(section) = o.get(key) else { continue };
        let Some(map) = section.as_object() else {
            out.add(path, format!("`{key}` must be an object keyed by file name."), format!("يجب أن يكون `{key}` كائناً مفاتيحه أسماء الملفات."));
            continue;
        };
        for (name, decl) in map {
            if !name.ends_with(ext) || !crate::layer_api::valid_plugin_name(name) {
                let name = show_hidden(name, false);
                out.add(path, format!("`{name}` under `{key}` must be a plain file name ending in `{ext}` (English letters, digits, dots, dashes, underscores)."),
                    format!("يجب أن يكون `{name}` ضمن `{key}` اسم ملف بسيطاً ينتهي بـ `{ext}` (أحرف إنجليزية وأرقام ونقطة وشرطة وشرطة سفلية)."));
            }
            match decl.as_object() {
                Some(d) => declaration(out, path, d, view, false),
                None => out.add(path, format!("The entry for `{name}` must be an object."), format!("يجب أن يكون إدخال `{name}` كائناً.")),
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
                        format!("`{s}` في `hidden` ليس قسماً من أقسام الصفحة الرئيسية. الأقسام: {}.", HOME_SECTIONS.join("، ")));
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
                        format!("`{s}` في `order` ليس عنصراً يمكن ترتيبه في الصفحة الرئيسية. استخدم قسماً ({})، أو `suggestions`، أو `widget:<id>`.", HOME_SECTIONS.join("، ")));
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
            None => out.add(path, "`labels` must be an object: { \"<page id>\": { \"en\": text, \"ar\": text } }.", "يجب أن يكون `labels` كائناً: { \"<معرّف الصفحة>\": { \"en\": نص، \"ar\": نص } }."),
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
        out.add(path, "`name` must be a short line of text.", "يجب أن يكون `name` نصّاً قصيراً في سطر واحد.");
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
            ("`at` must be a 24-hour time such as \"09:00\".".into(), "يجب أن يكون `at` وقتاً بنظام ٢٤ ساعة مثل \"09:00\".".into()),
        (Some("schedule"), Some("month")) => ("A monthly schedule needs `day`: a number from 1 to 28, or \"last\".".into(),
            "الجدول الشهري يحتاج إلى `day`: رقم من 1 إلى 28، أو \"last\".".into()),
        (Some("schedule"), Some("week")) => ("`weekday` must be a number from 0 (Sunday) to 6 (Saturday).".into(),
            "يجب أن يكون `weekday` رقماً من 0 (الأحد) إلى 6 (السبت).".into()),
        (Some("schedule"), Some("weekdays")) => ("`days` must be a list of weekdays, each from 0 (Sunday) to 6 (Saturday).".into(),
            "يجب أن يكون `days` قائمة بأيام الأسبوع، كلٌّ منها من 0 (الأحد) إلى 6 (السبت).".into()),
        (Some("schedule"), _) => ("`every` must be \"day\", \"weekdays\", \"week\" or \"month\".".into(),
            "يجب أن تكون قيمة `every` واحدة من: \"day\" أو \"weekdays\" أو \"week\" أو \"month\".".into()),
        _ => ("`trigger` must have \"type\": \"schedule\" or \"event\".".into(), "يجب أن يحتوي `trigger` على \"type\": \"schedule\" أو \"event\".".into()),
    };
    out.add(path, en, ar);
}

// ---------- what a style change does ----------

/// One declaration of a style sheet, with everything it sits inside (selectors, @-rules).
struct Decl { ctx: String, prop: String, value: String }

/// CSS escapes resolved, lower case, single spaces: `C\6f ntent` reads `content`.
fn css_norm(raw: &str) -> String {
    let mut out = String::new();
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' { out.push(c); continue; }
        let mut hex = String::new();
        while hex.len() < 6 {
            match it.peek() { Some(h) if h.is_ascii_hexdigit() => { hex.push(*h); it.next(); } _ => break }
        }
        if hex.is_empty() {
            if let Some(n) = it.next() { if n != '\n' { out.push(n); } }
            continue;
        }
        if it.peek().map_or(false, |w| w.is_whitespace()) { it.next(); }
        out.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).filter(|c| *c != '\0').unwrap_or('\u{FFFD}'));
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn css_flush(buf: &mut String, stack: &[String], out: &mut Vec<Decl>) {
    let raw = std::mem::take(buf);
    if raw.trim().is_empty() { return; }
    let (prop, value) = match raw.split_once(':') {
        Some((p, v)) if !raw.trim_start().starts_with('@') => (css_norm(p), css_norm(v)),
        _ => (css_norm(&raw), String::new()),
    };
    out.push(Decl { ctx: stack.join(" { "), prop, value });
}

/// A forgiving reading of a style sheet into its declarations. `None` when it cannot be read with
/// confidence (an unfinished comment or string, braces that do not pair up).
fn css_decls(text: &str) -> Option<Vec<Decl>> {
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut stack, mut buf) = (vec![], Vec::<String>::new(), String::new());
    let (mut paren, mut i) = (0usize, 0usize);
    while i < chars.len() {
        let c = chars[i];
        match c {
            '/' if chars.get(i + 1) == Some(&'*') => {
                let end = (i + 2..chars.len().saturating_sub(1)).find(|j| chars[*j] == '*' && chars[*j + 1] == '/')?;
                buf.push(' ');
                i = end + 2;
                continue;
            }
            '"' | '\'' => {
                buf.push(c);
                i += 1;
                loop {
                    let d = *chars.get(i)?;
                    if d == '\n' { return None; }
                    buf.push(d);
                    i += 1;
                    if d == '\\' { if let Some(e) = chars.get(i) { buf.push(*e); i += 1; } continue; }
                    if d == c { break; }
                }
                continue;
            }
            '\\' => { buf.push(c); if let Some(e) = chars.get(i + 1) { buf.push(*e); i += 1; } }
            '(' => { paren += 1; buf.push(c); }
            ')' => { paren = paren.saturating_sub(1); buf.push(c); }
            '{' if paren == 0 => { stack.push(css_norm(&buf)); buf.clear(); }
            '}' if paren == 0 => { css_flush(&mut buf, &stack, &mut out); stack.pop()?; }
            ';' if paren == 0 => css_flush(&mut buf, &stack, &mut out),
            _ => buf.push(c),
        }
        i += 1;
    }
    if !stack.is_empty() || paren != 0 { return None; }
    css_flush(&mut buf, &stack, &mut out);
    Some(out)
}

/// Declarations that only set colours, shadows and corner shapes (and the theme's own tokens).
fn css_look_only(d: &Decl) -> bool {
    const PROPS: &[&str] = &["color", "background", "background-color", "border-color", "outline-color", "accent-color", "caret-color", "fill", "stroke",
        "text-decoration-color", "column-rule-color", "scrollbar-color", "box-shadow", "text-shadow", "border-radius"];
    let p = d.prop.as_str();
    !d.value.contains("url(") && (p.starts_with("--") || PROPS.contains(&p) || (p.starts_with("border-") && (p.ends_with("-color") || p.ends_with("-radius"))))
}

/// The ways a style rule can mislead rather than decorate (see `css_report`).
fn css_flags(d: &Decl) -> Vec<&'static str> {
    let mut flags = vec![];
    let (p, v) = (d.prop.as_str(), d.value.split('!').next().unwrap_or("").trim());
    let number = |v: &str| v.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%').trim().parse::<f64>().ok();
    let zero = |v: &str| number(v) == Some(0.0);
    if p == "content" && !matches!(v, "" | "none" | "normal" | "\"\"" | "''") { flags.push("text"); }
    if p == "position" && matches!(v, "fixed" | "sticky" | "absolute") { flags.push("overlay"); }
    if ["dlg", "modal", "dialog", "toast", "no-print"].iter().any(|n| d.ctx.contains(n)) { flags.push("dialogs"); }
    let hides = match p {
        "display" => v == "none",
        "visibility" => v == "hidden" || v == "collapse",
        "content-visibility" => v == "hidden",
        "opacity" | "font-size" | "width" | "height" | "max-width" | "max-height" | "line-height" | "scale" | "zoom" => zero(v),
        "color" | "-webkit-text-fill-color" => v.starts_with("transparent"),
        "text-indent" => number(v).map_or(false, |n| n.abs() >= 500.0),
        "clip" | "clip-path" | "mask" | "mask-image" | "-webkit-mask" | "-webkit-mask-image" => v != "none",
        "transform" => v.replace(' ', "").contains("scale(0)") || v.replace(' ', "").contains("scale(0,"),
        _ => false,
    };
    if hides { flags.push("hides"); }
    let loads = |t: &str| t.split("url(").skip(1).any(|rest| !rest.trim_start_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace()).starts_with('#'));
    if p.starts_with("@import") || loads(&d.prop) || loads(&d.value) { flags.push("remote"); }
    flags
}

/// What the staged `custom.css` adds to the one in use, in terms a reviewer can act on:
/// `{kind, flags}`. `kind` is `removes` (nothing new), `colours` (new rules only set colours,
/// shadows, corner shapes and theme tokens), `layout` (anything else) or `unreadable`. `flags` names
/// what deserves a careful look: `text` (prints its own words), `overlay` (places things on top of
/// the app), `dialogs` (restyles the app's own questions and warnings), `hides` (makes parts
/// invisible), `remote` (tries to load a file). Rules the user already has are not counted again.
pub fn css_report(current: &str, staged: &str) -> Value {
    let Some(new) = css_decls(staged) else { return json!({"kind": "unreadable", "flags": []}) };
    let mut have: std::collections::HashMap<(String, String, String), usize> = std::collections::HashMap::new();
    for d in css_decls(current).unwrap_or_default() { *have.entry((d.ctx, d.prop, d.value)).or_default() += 1; }
    let added: Vec<Decl> = new.into_iter().filter(|d| {
        match have.get_mut(&(d.ctx.clone(), d.prop.clone(), d.value.clone())) {
            Some(n) if *n > 0 => { *n -= 1; false }
            _ => true,
        }
    }).collect();
    let mut flags: Vec<&str> = vec![];
    for f in added.iter().flat_map(css_flags) { if !flags.contains(&f) { flags.push(f); } }
    let kind = if added.is_empty() { "removes" } else if flags.is_empty() && added.iter().all(css_look_only) { "colours" } else { "layout" };
    json!({"kind": kind, "flags": flags})
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
        // `ideas:write` may name folders (its status changes stay inside them) and need not.
        assert!(with_js(r#"{"apiVersion":1,"permissions":["ideas:write"],"write":["clients/"]}"#).is_empty());
        assert!(with_js(r#"{"apiVersion":1,"permissions":["ideas:write"]}"#).is_empty());
        assert!(with_js(r#"{"apiVersion":1,"permissions":["ideas:read"],"write":["clients/"]}"#)[0].contains("only work together"));
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
        // `read` (FEAT-44): folders, with a reading permission, never empty; pages may list them too.
        assert!(with_js(r#"{"apiVersion":1,"permissions":["library:read","todos:read"],"read":["clients/","work/clients"]}"#).is_empty());
        assert!(with_js(r#"{"apiVersion":1,"permissions":["storage"],"read":["clients"]}"#)[0].contains("only work together"));
        assert!(with_js(r#"{"apiVersion":1,"permissions":["library:read"],"read":[]}"#)[0].contains("is empty"));
        assert!(with_js(r#"{"apiVersion":1,"permissions":["library:read"],"read":"clients"}"#)[0].contains("list of folder names"));
        for bad in ["../x", "/etc", ".git", "notes/.claude"] {
            assert!(with_js(&json!({"apiVersion": 1, "permissions": ["ideas:read"], "read": [bad]}).to_string())[0].contains("may read"), "{bad}");
        }
        assert!(check(&[("views/v.html", ""), ("views/v.json", r#"{"apiVersion":1,"permissions":["todos:read"],"read":["clients"]}"#)], false).is_empty());
        // The new abilities are permissions like any other.
        assert!(with_js(r#"{"apiVersion":1,"permissions":["files:save","notify","assistant:ask"]}"#).is_empty());
        assert!(check(&[("views/v.html", ""), ("views/v.json", r#"{"apiVersion":1,"permissions":["notify"]}"#)], false)[0].1.contains("A view can only read"));
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

    #[test]
    fn invisible_and_direction_changing_characters_are_caught_and_shown() {
        // Refused wherever they hide: a tag-character payload after a prompt, an override in a name.
        let smuggled = format!("Summarise my week{}", "\u{E0049}\u{E0047}\u{E004E}");
        assert_eq!(first_hidden(&smuggled, true), Some('\u{E0049}'));
        assert_eq!(show_hidden(&smuggled, true), "Summarise my week[U+E0049][U+E0047][U+E004E]");
        for bad in ["a\u{200B}b", "a\u{202E}b", "\u{2066}x\u{2069}", "x\u{FEFF}", "a\u{00AD}b", "line\u{2028}two", "a\u{1F}b", "ab\u{200D}", "a\u{200E}\u{200F}b", "\u{FE0F}x"] {
            assert!(first_hidden(bad, true).is_some(), "{bad:?}");
        }
        // Ordinary writing passes: Arabic, emoji sequences, a Persian joiner, one direction mark, line breaks.
        for ok in ["تغيير الألوان", "👩\u{200D}💻 Code", "❤\u{FE0F}", "🏳\u{FE0F}\u{200D}🌈", "می\u{200C}خواهم", "سعر (USD)\u{200F}.", "one\ntwo\tthree"] {
            assert_eq!(first_hidden(ok, true), None, "{ok:?}");
            assert_eq!(show_hidden(ok, true), ok);
        }
        assert_eq!(first_hidden("one\ntwo", false), Some('\n'), "a one-line field has no line breaks");
        assert_eq!(show_hidden(&"\u{200B}".repeat(50), false), format!("{}[+42]", "[U+200B]".repeat(8)));
        // In the files the app reads: one problem per file, saying where.
        let wf = format!(r#"{{"name":"Weekly","trigger":{{"type":"event","on":"wrap.done"}},"prompt":"{smuggled}"}}"#);
        let p = one("workflows/w.json", &wf);
        assert!(p.len() == 1 && p[0].contains("U+E0049") && p[0].contains("`prompt`"), "{p:?}");
        let p = one("home.json", "{\"chips\":[{\"label\":\"Plan\u{202E}\",\"prompt\":\"x\"}]}");
        assert!(p.len() == 1 && p[0].contains("U+202E") && p[0].contains("chips[0].label"), "{p:?}");
        assert!(one("nav.json", "{\"labels\":{\"ideas\":{\"en\":\"Cli\u{200B}ents\"}}}")[0].contains("U+200B"));
        assert!(check(&[("plugins/p.js", ""), ("plugins/p.json", "{\"apiVersion\":1,\"description\":\"Safe\u{2067}\"}")], true)[0].1.contains("لا يظهر"));
    }

    #[test]
    fn add_on_file_names_must_be_ones_storage_accepts() {
        let names = |files: &[(&str, &str)]| check(files, false).into_iter().filter(|(_, m)| m.contains("file names")).map(|(p, _)| p).collect::<Vec<_>>();
        assert!(names(&[("plugins/hours.js", ""), ("plugins/hours.json", r#"{"apiVersion":1}"#), ("views/cal-2.html", ""), ("plugins/lib/ساعات.js", "")]).is_empty());
        assert_eq!(names(&[("plugins/ساعات.js", ""), ("plugins/client hours.js", ""), ("views/تقويم.html", "")]), ["plugins/ساعات.js", "plugins/client hours.js", "views/تقويم.html"]);
        let long = format!("plugins/{}.js", "x".repeat(60));
        assert_eq!(names(&[(long.as_str(), "")]), [long.as_str()]);
        assert!(check(&[("plugins/ساعات.js", "")], true)[0].1.contains("hours.js"), "Arabic wording, same example");
        assert!(one("manifest.json", r#"{"apiVersion":1,"plugins":{"ساعات.js":{}}}"#)[0].contains("plain file name"));
    }

    #[test]
    fn style_changes_are_described_by_what_they_do() {
        let base = "/* Your overrides. */\n:root { --radius: 2px }\n";
        let r = |staged: &str| css_report(base, staged);
        assert_eq!(r(base), json!({"kind": "removes", "flags": []}));
        assert_eq!(r("/* gone */"), json!({"kind": "removes", "flags": []}));
        let theme = format!("{base}[data-theme=\"sand\"][data-mode=\"light\"] {{ --accent: #0a5c8a; --bg: #fff; }}\n.card {{ border-radius: 12px; box-shadow: 0 1px 2px rgba(0,0,0,.2); color: #222 }}\n");
        assert_eq!(r(&theme), json!({"kind": "colours", "flags": []}));
        assert_eq!(r(&format!("{base}.sidebar {{ width: 300px }}")), json!({"kind": "layout", "flags": []}));
        // The tricks from the audit: hide a dialog's words and print others, cover the window, restyle prompts.
        let fake = format!("{base}#dlg-body {{ display: none }}\n.dlg::before {{ content: 'Make this chat faster?' }}\n");
        assert_eq!(r(&fake), json!({"kind": "layout", "flags": ["dialogs", "hides", "text"]}));
        let cover = "body::after{content:'FAKE';position:fixed;top:0;left:0;right:0;height:90px;z-index:99999;background:#fff;pointer-events:none}";
        assert_eq!(r(cover)["flags"], json!(["text", "overlay"]));
        assert_eq!(r(".no-print{-webkit-text-fill-color:transparent!important}")["flags"], json!(["dialogs", "hides"]));
        // Escapes and comments do not hide a property; a comment in the middle of a word is not one.
        assert_eq!(r("p::after { c\\6f ntent: \"x\" }")["flags"], json!(["text"]));
        assert_eq!(r("p::after { CONTENT /* x */ : \"x\" }")["flags"], json!(["text"]));
        assert_eq!(r("p::after { con/**/tent: \"x\" }")["flags"], json!([]));
        assert_eq!(r(".x::before { content: \"\" } .y { content: none }")["flags"], json!([]));
        assert_eq!(r("@media (min-width: 600px) { [role=\"alertdialog\"] p { opacity: 0.0 } }")["flags"], json!(["dialogs", "hides"]));
        assert_eq!(r("@import url(https://example.org/x.css);")["flags"], json!(["remote"]));
        assert_eq!(r(":root { --pattern: url(\"data:image/png;base64,AAAA\") }"), json!({"kind": "layout", "flags": ["remote"]}));
        assert_eq!(r(".a { fill: url(#grad) }")["flags"], json!([]));
        // Only what is new counts: a rule the user already has is not reported again.
        let mine = ".toast { position: fixed }\n";
        assert_eq!(css_report(mine, &format!("{mine}:root {{ --fg: #111 }}")), json!({"kind": "colours", "flags": []}));
        // What cannot be read is said to be so.
        for broken in [".a { color: red", "/* never closed", ".a { content: \"x }", ".a } .b {"] {
            assert_eq!(r(broken)["kind"], "unreadable", "{broken}");
        }
    }
}
