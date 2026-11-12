//! Ready-made packs that come with the app (Customise › Ready-made). Each one is a worked example
//! from the customisation guide (`assets/LAYER.md`), in the UI language. Choosing one stages it
//! like any imported pack: the user sees the usual review and nothing is switched on until they
//! keep it. Everything is bundled; nothing is fetched from anywhere.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

const OVERDUE_JS: &str = include_str!("../assets/gallery/overdue.js");
const HOURS_JS: &str = include_str!("../assets/gallery/hours.js");
const BOARD_JS: &str = include_str!("../assets/gallery/status-board.js");
const INTAKE_JS: &str = include_str!("../assets/gallery/client-intake.js");

const SAND_CSS: &str = r#"[data-theme="sand"][data-mode="light"] {
  --bg: #f7f1e5; --panel: #fffaf0; --panel-2: #efe5d2; --sidebar: #ede2cc; --fg: #2b2118; --muted: #6b5c4a;
  --accent: #a4571b; --accent-fg: #ffffff; --accent-2: #2f6f6a; --border: #e0d3bb;
  --danger: #a3261c; --ok: #2f6b3a; --code-bg: #f1e8d6;
}
[data-theme="sand"][data-mode="dark"] {
  --bg: #19140e; --panel: #221b13; --panel-2: #2c2419; --sidebar: #1d1710; --fg: #f3eadb; --muted: #b3a48d;
  --accent: #e39a4f; --accent-fg: #1b1208; --accent-2: #6fc2b9; --border: #3a2f21;
  --danger: #ff8b7a; --ok: #7fd18f; --code-bg: #1e1811;
}
"#;

/// The ids, in the order they are offered.
pub const IDS: &[&str] = &["hours", "board", "intake", "overdue", "weekly-review", "todos-by-date", "sand"];

fn decl(permissions: &[&str], write: &[&str], description: &str) -> String {
    let mut d = json!({"apiVersion": 1, "permissions": permissions, "description": description});
    if !write.is_empty() { d["write"] = json!(write); }
    serde_json::to_string_pretty(&d).unwrap_or_default() + "\n"
}

/// One ready-made pack in the UI language: `(name, what it does, its files)`.
fn build(id: &str, ar: bool) -> Option<(&'static str, &'static str, BTreeMap<String, String>)> {
    let mut f = BTreeMap::new();
    let mut put = |path: &str, text: String| { f.insert(path.to_string(), text); };
    let (name, about) = match id {
        "hours" => {
            put("plugins/hours.js", HOURS_JS.into());
            put("plugins/hours.json", decl(&["storage"], &[], if ar { "تسجّل ساعات العمل المحتسبة وتعرض مجموع هذا الشهر في الصفحة الرئيسية" } else { "Tracks billable hours and shows this month's total on Home" }));
            put("home.json", "{\n  \"order\": [\"widget:hours-month\"]\n}\n".into());
            if ar { ("ساعات العمل", "صفحة باسم «الساعات» تسجّل فيها التاريخ والعميل وعدد ساعات العمل، وتعرض إدخالاتك. وتُظهر الصفحة الرئيسية مجموع ساعات هذا الشهر. تُحفظ الإدخالات على هذا الجهاز ضمن بيانات الإضافة، لا في ملاحظاتك.") }
            else { ("Billable hours", "A page called Hours where you add the date, client and hours worked, with a list of your entries. Home shows this month's total. The entries are kept on this computer as the add-on's saved data, not in your notes.") }
        }
        "board" => {
            put("plugins/status-board.js", BOARD_JS.into());
            put("plugins/status-board.json", decl(&["ideas:read", "ideas:write"], &[], if ar { "لوحة تعرض عناصرك حسب حالتها؛ اسحب البطاقة لتغيير حالتها" } else { "A board of your items by status; drag a card to change its status" }));
            if ar { ("لوحة الحالات", "صفحة باسم «اللوحة» تعرض عناصرك في أربعة أعمدة: جديد، وقيد العمل، ومعلّق، ومنجز. اسحب البطاقة إلى عمود آخر، أو اختر من قائمتها، لتغيير حالتها. لا يتغيّر شيء آخر في العنصر.") }
            else { ("Status board", "A page called Board that shows your items in four columns: New, In progress, On hold and Done. Drag a card to another column, or use its menu, to change its status. Nothing else about the item changes.") }
        }
        "intake" => {
            put("plugins/client-intake.js", INTAKE_JS.into());
            put("plugins/client-intake.json", decl(&["library:read", "library:write"], &["clients/"], if ar { "نموذج يحفظ كل عميل جديد في ملاحظة داخل مجلد العملاء" } else { "A form that saves each new client as a note in the clients folder" }));
            if ar { ("نموذج عميل جديد", "صفحة باسم «عميل جديد» فيها نموذج: اسم العميل، ووسيلة التواصل، وما يحتاج إليه. عند الحفظ تُنشأ ملاحظة لهذا العميل في مجلد clients في مكتبتك، دون أن تُستبدل ملاحظة موجودة. لا تكتب هذه الإضافة إلا داخل ذلك المجلد.") }
            else { ("New client form", "A page called New client with a form: the client's name, how to reach them and what they need. Saving it creates a note for that client in the clients folder of your library, never overwriting one. This add-on writes only inside that folder.") }
        }
        "overdue" => {
            put("plugins/overdue.js", OVERDUE_JS.into());
            put("plugins/overdue.json", decl(&["todos:read"], &[], if ar { "تعرض المهام المتأخرة في الصفحة الرئيسية" } else { "Lists overdue todos on Home" }));
            if ar { ("المهام المتأخرة", "مربع في الصفحة الرئيسية يعرض حتى سبع مهام فات موعدها، الأقدم أولاً. انقر مهمة لتنتقل إلى صفحة المهام.") }
            else { ("Overdue todos", "A box on Home that lists up to seven todos whose date has passed, oldest first. Click one to go to Todos.") }
        }
        "weekly-review" => {
            let wf = if ar {
                json!({"name": "المراجعة الأسبوعية", "trigger": {"type": "schedule", "every": "week", "at": "16:00", "weekday": 4},
                    "prompt": "اكتب مراجعتي الأسبوعية باللغة العربية. اقرأ الملاحظات والسجلات التي تغيّرت في الأيام السبعة الماضية وقوائم مهامي. ثم أضف إدخالاً واحداً مؤرّخاً، في 15 سطراً على الأكثر، في المكان الذي تحفظ فيه هذه المكتبة مراجعاتها أو سجلاتها (اتبع CLAUDE.md): ما قررته، وما زال مفتوحاً، والمهام المستحقة في الأيام السبعة القادمة. لا تغيّر أي شيء آخر."})
            } else {
                json!({"name": "Weekly review", "trigger": {"type": "schedule", "every": "week", "at": "16:00", "weekday": 5},
                    "prompt": "Write my weekly review. Read the notes and logs changed in the last 7 days and my todo lists. Then add one dated entry, at most 15 lines, where this library keeps reviews or logs (follow CLAUDE.md): what I decided, what is still open, and the todos due in the next 7 days. Do not change anything else."})
            };
            put("workflows/weekly-review.json", serde_json::to_string_pretty(&wf).unwrap_or_default() + "\n");
            if ar { ("المراجعة الأسبوعية", "مهمة تلقائية: كل خميس الساعة الرابعة عصراً يكتب المساعد مراجعة قصيرة لأسبوعك في مكتبتك: ما قررته، وما زال مفتوحاً، وما يستحق الأسبوع القادم. لن تعمل إلا بعد أن تسمح لها من «المهام التلقائية»، وتظهر كل مرة تعمل فيها ضمن محادثاتك.") }
            else { ("Weekly review", "An automatic task: every Friday at 4 pm the assistant writes a short review of your week into your library: what you decided, what is still open and what is due next week. It runs only after you allow it under Automatic tasks, and each run appears in your chats.") }
        }
        "todos-by-date" => {
            put("todos.json", "{\n  \"groupBy\": \"due\",\n  \"sort\": \"due\"\n}\n".into());
            if ar { ("المهام حسب التاريخ", "تجمع صفحة المهام مهامك حسب موعدها بدلاً من قائمة لكل ملف: المتأخرة أولاً، ثم القادمة، ثم التي بلا موعد. لا تتغيّر مهامك نفسها.") }
            else { ("Todos by date", "The Todos page groups your todos by when they are due instead of one list per file: overdue first, then what is coming up, then those with no date. Your todos themselves do not change.") }
        }
        "sand" => {
            put("themes.json", serde_json::to_string_pretty(&json!([{"id": "sand", "name": if ar { "رملي" } else { "Sand" }}])).unwrap_or_default() + "\n");
            put("custom.css", SAND_CSS.into());
            if ar { ("سمة رملية", "سمة دافئة بلون الرمل، بنسخة نهارية وأخرى ليلية. لتجربتها افتح اختيار السمة واختر «رملي»؛ تبقى سمتك الحالية كما هي حتى تغيّرها.") }
            else { ("Sand theme", "A warm sand-coloured theme, in a day and a night version. To try it, open the theme picker and choose Sand; your current theme stays until you switch.") }
        }
        _ => return None,
    };
    Some((name, about, f))
}

/// The pack itself, as an exported pack would carry it.
fn as_pack(name: &str, about: &str, files: &BTreeMap<String, String>) -> Value {
    json!({
        "format": "thoughts-pack", "version": 1, "apiVersion": crate::applayer::API_VERSION, "kind": "feature",
        "name": name, "author": "Thoughts", "packVersion": env!("CARGO_PKG_VERSION"), "description": about,
        "appVersion": env!("CARGO_PKG_VERSION"), "fingerprint": crate::packs::fingerprint("feature", files), "files": files,
    })
}

/// The ready-made pack `id` in the UI language, with the app's own description of the change it
/// stages (its first line is the change's title).
pub fn pack(id: &str, ar: bool) -> Option<(Value, String)> {
    let (name, about, files) = build(id, ar)?;
    let summary = if ar {
        format!("إضافة «{name}» من الحزم الجاهزة\n\n{about}\n\nهذه الحزمة مرفقة بالتطبيق. جرّبها أولاً، ثم اعتمدها إن أعجبتك؛ ويمكنك لاحقاً إيقافها أو إزالتها أو التراجع عنها.")
    } else {
        format!("Add \u{201c}{name}\u{201d} from the ready-made packs\n\n{about}\n\nIt comes with Thoughts. Try it first, then keep it if you like it; you can switch it off, remove it or undo it later.")
    };
    Some((as_pack(name, about, &files), summary))
}

/// Every ready-made pack for the gallery: `{id, name, about, author, version, fingerprint, files,
/// permissions, write, workflow, theme, added}`. `added`: it is already part of the app (in the
/// layer at `layer`) exactly as it comes.
pub fn list(ar: bool, layer: &Path) -> Vec<Value> {
    IDS.iter().filter_map(|id| {
        let (name, about, files) = build(id, ar)?;
        let pack = as_pack(name, about, &files);
        let mut permissions: Vec<String> = vec![];
        let mut write: Vec<String> = vec![];
        for (path, text) in &files {
            if !(path.starts_with("plugins/") && path.ends_with(".json")) { continue; }
            let d: Value = serde_json::from_str(text).unwrap_or(Value::Null);
            for p in d["permissions"].as_array().into_iter().flatten().filter_map(Value::as_str) { if !permissions.iter().any(|x| x == p) { permissions.push(p.into()); } }
            for w in d["write"].as_array().into_iter().flatten().filter_map(Value::as_str) { if !write.iter().any(|x| x == w) { write.push(w.into()); } }
        }
        Some(json!({
            "id": id, "name": name, "about": about, "author": "Thoughts", "version": env!("CARGO_PKG_VERSION"),
            "fingerprint": pack["fingerprint"], "code": crate::packs::fingerprint_code(pack["fingerprint"].as_str().unwrap_or_default()),
            "files": files.keys().collect::<Vec<_>>(),
            "permissions": permissions, "write": write,
            "workflow": files.keys().any(|p| p.starts_with("workflows/")), "theme": files.contains_key("themes.json"),
            "added": crate::packs::already_in(&pack, layer),
        }))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every ready-made pack, in both languages, is a valid pack that passes the review's checks,
    /// can be kept, and then counts as added; and it really does what its card says.
    #[tokio::test]
    async fn every_ready_made_pack_passes_the_review_and_can_be_kept() {
        for ar in [false, true] {
            for id in IDS {
                let (st, d) = crate::applayer::testutil::layer(&format!("gallery-{id}")).await;
                let (pack, summary) = pack(id, ar).unwrap();
                crate::packs::check_pack(&pack).unwrap();
                assert!(!list(ar, &d.layer).iter().find(|x| x["id"] == *id).unwrap()["added"].as_bool().unwrap(), "{id}");
                crate::packs::import_with(st, &d, &pack, ar, Some(summary.clone())).await.unwrap();
                let p = crate::staging::pending(st, &d, ar).await;
                assert_eq!((&p["exists"], &p["origin"], &p["problems"]), (&json!(true), &json!("import"), &json!([])), "{id} ({ar}): {p}");
                assert_eq!(p["summary"].as_str().unwrap(), summary.trim(), "the app's own words, not a quotation");
                assert_eq!(p["pack"]["description"], "", "no maker's quotation for the app's own packs");
                assert_eq!(p["pack"]["author"], "Thoughts");
                assert_eq!(p["pack"]["fingerprint"], pack["fingerprint"]);
                crate::staging::apply(st, &d, ar, p["signature"].as_str()).await.unwrap();
                assert!(list(ar, &d.layer).iter().find(|x| x["id"] == *id).unwrap()["added"].as_bool().unwrap(), "{id} counts as added once kept");
                let m = crate::applayer::manifest_of(&d.layer);
                for (path, text) in pack["files"].as_object().unwrap() {
                    if path.contains('/') { assert_eq!(std::fs::read_to_string(d.layer.join(path)).unwrap(), text.as_str().unwrap(), "{path}"); }
                }
                // Each plugin runs with exactly the permissions its card lists.
                let card = list(ar, &d.layer).into_iter().find(|x| x["id"] == *id).unwrap();
                let granted: Vec<Value> = m["plugins"].as_array().unwrap().iter().flat_map(|p| p["permissions"].as_array().unwrap().clone()).collect();
                assert_eq!(json!(granted), card["permissions"], "{id}");
                if *id == "hours" { assert!(m["home"]["order"].as_array().unwrap().contains(&json!("widget:hours-month"))); }
                if *id == "todos-by-date" { assert_eq!(m["todos"]["groupBy"], "due"); }
                if *id == "sand" { assert_eq!(m["themes"][0]["id"], "sand"); assert!(m["css"].as_str().unwrap().contains("/* pack:")); }
                if *id == "weekly-review" { assert_eq!(crate::workflows::load(&d.layer).len(), 1); }
                std::fs::remove_dir_all(&d.home).unwrap();
            }
        }
        assert!(pack("nope", false).is_none());
    }

    #[test]
    fn ready_made_plugins_speak_both_languages_and_use_no_network() {
        for js in [OVERDUE_JS, HOURS_JS, BOARD_JS, INTAKE_JS] {
            assert!(js.contains("document.documentElement.lang === 'ar'"));
            for banned in ["fetch(", "XMLHttpRequest", "innerHTML", "http://", "https://", "import("] { assert!(!js.contains(banned), "{banned}"); }
        }
    }
}
