//! Keyboard shortcuts the user sets (round 0.5.0): the layer's `keys.json`,
//! `{ "format": 1, "bindings": { "<id>": ["Mod+Shift+KeyW", …] } }`. A list replaces that command's
//! keys, `[]` takes its key away; the window's registry (`ui/src/lib/keys/registry.ts`) knows the
//! commands and their defaults, this side only checks the file and writes it.
//!
//! * `PUT /api/layer/keys {bindings}` keeps a set the user made in Settings › Keyboard as one History
//!   entry (`applayer::app_change`, Act `keys`); an empty set removes the file ("Reset all").
//! * `problems` checks a `keys.json` in a staged change (`staging::validate::problems`), so a
//!   customisation or a pack cannot take keys the system or Recovery needs.
//! * `native` gives the desktop shell the menu's keys (Settings, Close, Quit, Full Screen).
//!
//! A binding is modifiers in the order Mod, Control, Alt, Shift, Super, then a `KeyboardEvent.code`.
//! `Mod` is ⌘ on a Mac and Ctrl elsewhere. The keys nobody may take are in one file shared with the
//! window, `ui/src/lib/keys/reserved.json`. Everything except reading the file as bindings yields to
//! full access (`full_access::on()`), like every other guard the app adds.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use axum::extract::State;
use axum::routing::put;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiErr;
use crate::applayer::{self, Act, Dirs, Git};
use crate::state::AppState;

pub const FILE: &str = "keys.json";
/// The most commands a set may name, and keys per command.
pub const MAX_IDS: usize = 300;
pub const MAX_PER_ID: usize = 3;

const RESERVED_JSON: &str = include_str!("../../../ui/src/lib/keys/reserved.json");

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Reserved { all: Vec<String>, macos: Vec<String>, windows: Vec<String>, linux: Vec<String>, recovery: Vec<String>, kept_defaults: BTreeMap<String, Vec<String>> }

fn reserved() -> &'static Reserved {
    static R: OnceLock<Reserved> = OnceLock::new();
    R.get_or_init(|| serde_json::from_str(RESERVED_JSON).expect("ui/src/lib/keys/reserved.json is valid"))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Os { Mac, Windows, Linux }

impl Os {
    pub fn here() -> Os { if cfg!(target_os = "macos") { Os::Mac } else if cfg!(windows) { Os::Windows } else { Os::Linux } }
}

const MODS: [&str; 5] = ["Mod", "Control", "Alt", "Shift", "Super"];

/// The `KeyboardEvent.code`s a binding may end in (the window accepts the same, registry.ts `CODE`).
pub fn known_code(c: &str) -> bool {
    const NAMED: &[&str] = &["Enter", "Space", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown", "Escape", "Tab", "Minus", "Equal",
        "BracketLeft", "BracketRight", "Backslash", "Semicolon", "Quote", "Comma", "Period", "Slash", "Backquote", "IntlBackslash",
        "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"];
    const PAD: &[&str] = &["Add", "Subtract", "Multiply", "Divide", "Decimal", "Enter", "Equal"];
    if NAMED.contains(&c) { return true; }
    if let Some(l) = c.strip_prefix("Key") { return l.len() == 1 && l.as_bytes()[0].is_ascii_uppercase(); }
    if let Some(d) = c.strip_prefix("Digit") { return d.len() == 1 && d.as_bytes()[0].is_ascii_digit(); }
    if let Some(p) = c.strip_prefix("Numpad") { return (p.len() == 1 && p.as_bytes()[0].is_ascii_digit()) || PAD.contains(&p); }
    if let Some(n) = c.strip_prefix('F') { return !n.starts_with('0') && n.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)); }
    false
}

/// `(modifiers, code)` of a binding, or None when it is not one (unknown key, a modifier twice or out of order).
pub fn parse(b: &str) -> Option<(Vec<&str>, &str)> {
    if b.len() > 60 { return None; }
    let mut parts: Vec<&str> = b.split('+').collect();
    let code = parts.pop()?;
    if !known_code(code) { return None; }
    let mut last: Option<usize> = None;
    for p in &parts {
        let i = MODS.iter().position(|m| m == p)?;
        if last.is_some_and(|l| i <= l) { return None; }
        last = Some(i);
    }
    Some((parts, code))
}

/// The binding as `os` reads it: ⌘ is Mod on a Mac (Super folds into it), Ctrl is Mod elsewhere (Control folds into it).
pub fn norm(b: &str, os: Os) -> Option<String> {
    let (mods, code) = parse(b)?;
    let folded: Vec<&str> = mods.iter().map(|m| match (*m, os) { ("Super", Os::Mac) => "Mod", ("Control", Os::Windows | Os::Linux) => "Mod", (m, _) => m }).collect();
    let mut out: Vec<&str> = MODS.iter().copied().filter(|m| folded.contains(m)).collect();
    out.push(code);
    Some(out.join("+"))
}

/// Why `b` may not be a key for `id` on `os`: `Some("system" | "recovery")`, or None. A command's own
/// default is always allowed back (`keptDefaults`).
pub fn refused(id: &str, b: &str, os: Os) -> Option<&'static str> {
    let r = reserved();
    let n = norm(b, os)?;
    let same = |x: &String| norm(x, os).as_deref() == Some(n.as_str());
    if r.kept_defaults.get(id).is_some_and(|k| k.iter().any(same)) { return None; }
    if r.recovery.iter().any(same) { return Some("recovery"); }
    let extra = match os { Os::Mac => &r.macos, Os::Windows => &r.windows, Os::Linux => &r.linux };
    let has_super = parse(b).is_some_and(|(m, _)| m.contains(&"Super"));
    if r.all.iter().chain(extra.iter()).any(|x| if x == "Super+*" { has_super } else { same(x) }) { return Some("system"); }
    None
}

fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty() && b.len() <= 80 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._:-".contains(c))
}

fn hidden(s: &str) -> Option<char> {
    s.chars().find(|c| c.is_control() || matches!(*c as u32, 0x00AD | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0xFEFF | 0xE0000..=0xE007F))
}

fn say(ar: bool, en: String, arabic: String) -> String { if ar { arabic } else { en } }

/// Everything wrong with a `keys.json`, in the UI language. `full` = full access: only what is not a
/// set of bindings at all is refused then.
pub fn check(v: &Value, ar: bool, full: bool, os: Os) -> Vec<String> {
    let mut out = vec![];
    let Some(o) = v.as_object() else {
        return vec![say(ar, "The keyboard shortcuts file must be a JSON object: { \"format\": 1, \"bindings\": { … } }.".into(),
            "يجب أن يكون ملف اختصارات لوحة المفاتيح كائن JSON: { \"format\": 1, \"bindings\": { … } }.".into())];
    };
    for k in o.keys().filter(|k| *k != "format" && *k != "bindings") {
        out.push(say(ar, format!("`{k}` is not part of the keyboard shortcuts file (only `format` and `bindings`)."),
            format!("`{k}` ليس جزءاً من ملف الاختصارات (يقبل `format` و`bindings` فقط).")));
    }
    if o.get("format").is_some_and(|f| f != &json!(1)) {
        out.push(say(ar, "This keyboard shortcuts file is in a format this version of the app does not know (it reads format 1).".into(),
            "ملف الاختصارات هذا بصيغة لا يعرفها هذا الإصدار من التطبيق (يقرأ الصيغة 1).".into()));
    }
    let Some(b) = o.get("bindings") else { return out };
    let Some(b) = b.as_object() else {
        out.push(say(ar, "`bindings` must be an object: { \"command\": [\"Mod+KeyK\"] }.".into(), "يجب أن يكون `bindings` كائناً: { \"command\": [\"Mod+KeyK\"] }.".into()));
        return out;
    };
    if b.len() > MAX_IDS {
        out.push(say(ar, format!("Too many shortcuts ({}; the limit is {MAX_IDS}).", b.len()), format!("عدد الاختصارات كبير جداً ({}، والحد الأقصى {MAX_IDS}).", b.len())));
    }
    for (id, list) in b {
        if let Some(c) = hidden(id) {
            out.push(say(ar, format!("A command name contains a character that does not show on screen (U+{:04X}).", c as u32),
                format!("اسم أمر يحتوي على حرف لا يظهر على الشاشة (U+{:04X}).", c as u32)));
            continue;
        }
        if !valid_id(id) {
            out.push(say(ar, format!("`{id}` is not a command name (small English letters, digits, dots, dashes, colons; 80 at most)."),
                format!("`{id}` ليس اسم أمر (أحرف إنجليزية صغيرة وأرقام ونقاط وشرطات ونقطتان، ٨٠ حرفاً على الأكثر).")));
            continue;
        }
        let Some(keys) = list.as_array() else {
            out.push(say(ar, format!("The keys of `{id}` must be a list, like [\"Mod+KeyK\"]."), format!("مفاتيح `{id}` يجب أن تكون قائمة، مثل [\"Mod+KeyK\"].")));
            continue;
        };
        if keys.len() > MAX_PER_ID {
            out.push(say(ar, format!("`{id}` has more than {MAX_PER_ID} keys."), format!("لـ`{id}` أكثر من {MAX_PER_ID} مفاتيح.")));
        }
        let mut seen = vec![];
        for k in keys {
            let Some(s) = k.as_str() else {
                out.push(say(ar, format!("A key of `{id}` is not text."), format!("أحد مفاتيح `{id}` ليس نصاً.")));
                continue;
            };
            if let Some(c) = hidden(s) {
                out.push(say(ar, format!("A key of `{id}` contains a character that does not show on screen (U+{:04X}).", c as u32),
                    format!("أحد مفاتيح `{id}` يحتوي على حرف لا يظهر على الشاشة (U+{:04X}).", c as u32)));
                continue;
            }
            if parse(s).is_none() {
                out.push(say(ar, format!("`{s}` (for `{id}`) is not a key: modifiers Mod, Control, Alt, Shift, Super in that order, then a key such as KeyK, Digit1, F5 or Slash."),
                    format!("`{s}` (لـ`{id}`) ليس مفتاحاً: المعدّلات Mod وControl وAlt وShift وSuper بهذا الترتيب، ثم مفتاح مثل KeyK أو Digit1 أو F5 أو Slash.")));
                continue;
            }
            if seen.contains(&s) {
                out.push(say(ar, format!("`{s}` is listed twice for `{id}`."), format!("`{s}` مذكور مرتين لـ`{id}`.")));
            }
            seen.push(s);
            if full { continue; }
            match refused(id, s, os) {
                Some("recovery") => out.push(say(ar, format!("`{s}` (for `{id}`) is kept for the Recovery menu."), format!("`{s}` (لـ`{id}`) محجوز لقائمة الاستعادة."))),
                Some(_) => out.push(say(ar, format!("`{s}` (for `{id}`) is used by the system."), format!("`{s}` (لـ`{id}`) يستعمله النظام."))),
                None => {}
            }
        }
    }
    out
}

/// Everything wrong with the staged `keys.json` in `dir`, as `{path, message}` (`staging::validate::problems`).
/// Whether it is JSON at all is checked there; this checks what it says.
pub fn problems(dir: &Path, arabic: bool) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(dir.join(FILE)) else { return vec![] };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return vec![] };
    check(&v, arabic, crate::full_access::on(), Os::here()).into_iter().map(|m| json!({"path": FILE, "message": m})).collect()
}

/// The History title of a change the app made to the keyboard shortcuts (`applayer` `Act` kind `keys`).
pub fn act_title(ar: bool) -> String {
    (if ar { "تغيير اختصارات لوحة المفاتيح" } else { "Changed the keyboard shortcuts" }).into()
}

/// Keep `bindings` as the user's set: one History entry (`Err` carries the problems, one per line).
/// An empty set removes the file. Returns the entry ("" when nothing changed).
pub async fn keys_in(g: Git<'_>, dir: &Path, bindings: &Value, arabic: bool) -> anyhow::Result<String> {
    let bindings = if bindings.is_null() { json!({}) } else { bindings.clone() };
    let empty = bindings.as_object().is_some_and(|o| o.is_empty());
    let file = json!({"format": 1, "bindings": bindings});
    let problems = check(&file, arabic, crate::full_access::on(), Os::here());
    if !problems.is_empty() { return Err(crate::tr::err("keys.invalid", &[("problems", &problems.join("\n"))])); }
    let act = Act { kind: "keys".into(), item: FILE.into(), ..Default::default() };
    applayer::app_change(g, dir, "Change keyboard shortcuts", act, |dir| {
        let path = dir.join(FILE);
        if empty {
            if !path.exists() { return Ok(vec![]); }
            std::fs::remove_file(&path)?;
        } else {
            let text = serde_json::to_string_pretty(&file)? + "\n";
            crate::paths::write_atomic(&path, text.as_bytes(), false)?;
        }
        Ok(vec![FILE.to_string()])
    }).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct KeysBody { bindings: Value }

/// `PUT /api/layer/keys {bindings}` → `{ok, hash, entry}` (the History entry; empty when nothing changed).
async fn put_keys(State(st): State<Arc<AppState>>, Json(b): Json<KeysBody>) -> Result<Json<Value>, ApiErr> {
    let _g = crate::layer_api::LAYER_LOCK.lock().await;
    let ar = st.settings.read().await.language == "ar";
    let hash = keys_in(Git(Some(&st)), &Dirs::real().layer, &b.bindings, ar).await?;
    crate::log::info("layer: the keyboard shortcuts were changed in Settings");
    if !hash.is_empty() { st.emit(json!({"type": "layer.changed"})); }
    Ok(Json(json!({"ok": true, "hash": hash, "entry": hash})))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/layer/keys", put(put_keys))
}

// ---------- the native menu ----------

/// The keys the shell's menu items carry, as Tauri accelerators (`None` = the item has no key).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeKeys {
    pub settings: Option<String>,
    pub close_window: Option<String>,
    pub quit: Option<String>,
    pub fullscreen: Option<String>,
    /// The user changed Close (the shell then uses its own item instead of the system's, whose ⌘W is fixed).
    pub close_changed: bool,
}

/// A binding as a Tauri accelerator: `Mod` → `CmdOrCtrl`, `Control` → `Ctrl`; the key keeps its code name.
pub fn accelerator(b: &str) -> Option<String> {
    let (mods, code) = parse(b)?;
    let mut out: Vec<&str> = mods.iter().map(|m| match *m { "Mod" => "CmdOrCtrl", "Control" => "Ctrl", m => m }).collect();
    out.push(code);
    Some(out.join("+"))
}

/// The app's own menu keys on `os` (exactly what the shell had before 0.5.0).
pub fn native_defaults(os: Os) -> NativeKeys {
    NativeKeys {
        settings: Some("CmdOrCtrl+Comma".into()),
        close_window: Some("CmdOrCtrl+W".into()),
        // Windows closes with Alt+F4; Ctrl+Q is not a convention there.
        quit: (os != Os::Windows).then(|| "CmdOrCtrl+Q".into()),
        fullscreen: Some(if os == Os::Mac { "Ctrl+Super+F" } else { "F11" }.into()),
        close_changed: false,
    }
}

/// The menu's keys from the layer at `layer`: the user's (the first key of each list; `[]` = none) or
/// the defaults; always the defaults with customisations off (`safe`). A list naming a key that is not
/// allowed is ignored (the defaults stay), unless full access is on.
pub fn native(layer: &Path, safe: bool) -> NativeKeys { native_on(layer, safe, Os::here(), crate::full_access::on()) }

pub fn native_on(layer: &Path, safe: bool, os: Os, full: bool) -> NativeKeys {
    let mut k = native_defaults(os);
    if safe { return k; }
    let Some(b) = std::fs::read_to_string(layer.join(FILE)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.get("bindings").cloned()) else { return k };
    let pick = |id: &str, slot: &mut Option<String>| -> bool {
        let Some(list) = b.get(id).and_then(|l| l.as_array()) else { return false };
        let keys: Vec<&str> = list.iter().filter_map(|x| x.as_str()).collect();
        if keys.len() != list.len() || keys.iter().any(|s| parse(s).is_none() || (!full && refused(id, s, os).is_some())) { return false; }
        *slot = keys.first().and_then(|s| accelerator(s));
        true
    };
    pick("settings", &mut k.settings);
    k.close_changed = pick("window.close", &mut k.close_window);
    pick("app.quit", &mut k.quit);
    pick("window.fullscreen", &mut k.fullscreen);
    k
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applayer::{history_in, revert_in, testutil};

    #[test]
    fn bindings_parse_only_in_the_one_written_order() {
        assert!(parse("Mod+Shift+KeyW").is_some() && parse("Slash").is_some() && parse("F24").is_some() && parse("Mod+Alt+Numpad3").is_some());
        for bad in ["Shift+Mod+KeyW", "Mod+Mod+KeyK", "Mod+KeyLL", "F25", "F0", "F01", "Hyper+KeyK", "", "Mod+", "Mod+keyk", "Mod+Digit10"] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        assert_eq!(norm("Super+KeyK", Os::Mac).unwrap(), "Mod+KeyK");
        assert_eq!(norm("Control+KeyY", Os::Windows).unwrap(), "Mod+KeyY");
        assert_eq!(norm("Mod+Control+KeyF", Os::Mac).unwrap(), "Mod+Control+KeyF");
    }

    #[test]
    fn the_system_and_recovery_keep_their_keys() {
        assert_eq!(refused("chat.new", "Mod+KeyC", Os::Linux), Some("system"));
        assert_eq!(refused("chat.new", "Escape", Os::Mac), Some("system"));
        assert_eq!(refused("chat.new", "Mod+KeyR", Os::Windows), Some("recovery"));
        assert_eq!(refused("chat.new", "Mod+KeyH", Os::Mac), Some("system"));
        assert_eq!(refused("chat.new", "Mod+KeyH", Os::Windows), None);
        assert_eq!(refused("chat.new", "Super+KeyJ", Os::Windows), Some("system"));
        assert_eq!(refused("chat.new", "Alt+F4", Os::Windows), Some("system"));
        assert_eq!(refused("chat.new", "Mod+KeyJ", Os::Mac), None);
        // A command's own default may come back.
        assert_eq!(refused("mado-graph.undo", "Mod+KeyZ", Os::Mac), None);
        assert_eq!(refused("mado-graph.redo", "Control+KeyY", Os::Mac), None);
    }

    #[test]
    fn a_file_is_checked_whole_and_in_the_users_language() {
        let ok = json!({"format": 1, "bindings": {"chat.new": ["Mod+KeyJ"], "palette": [], "plugin:hours.open": ["Mod+Alt+KeyH"]}});
        assert!(check(&ok, false, false, Os::Windows).is_empty(), "{:?}", check(&ok, false, false, Os::Windows));
        let p = check(&json!({"format": 2, "colour": 1, "bindings": {"Bad Id": ["Mod+KeyJ"], "a": ["Mod+KeyJ", "Mod+KeyJ"], "b": ["x"], "c": ["Mod+KeyC"],
            "d": ["Mod+KeyA", "Mod+KeyB", "Mod+KeyD", "Mod+KeyE"], "e": "Mod+KeyE", "f": ["Mod+KeyR"]}}), false, false, Os::Linux);
        let all = p.join("\n");
        for want in ["`colour`", "format", "`Bad Id`", "listed twice", "`x`", "used by the system", "more than 3", "must be a list", "Recovery"] {
            assert!(all.contains(want), "{want} in {all}");
        }
        let ar = check(&json!({"bindings": {"c": ["Mod+KeyC"]}}), true, false, Os::Linux).join("");
        assert!(ar.contains("يستعمله النظام"), "{ar}");
        let invisible = check(&json!({"bindings": {"a\u{200b}b": ["Mod+KeyJ"], "c": ["Mod+Key\u{202e}J"]}}), false, false, Os::Linux).join("\n");
        assert_eq!(invisible.matches("does not show on screen").count(), 2, "{invisible}");
        let many: serde_json::Map<String, Value> = (0..=MAX_IDS).map(|i| (format!("plugin:x{i}"), json!([]))).collect();
        assert!(check(&json!({"bindings": many}), false, false, Os::Linux).join("").contains("Too many"));
        assert!(!check(&json!([]), false, false, Os::Linux).is_empty());
    }

    #[test]
    fn full_access_yields_except_for_what_is_not_a_key() {
        let f = json!({"bindings": {"chat.new": ["Mod+KeyC", "Mod+KeyR"]}});
        assert!(check(&f, false, true, Os::Linux).is_empty());
        assert!(!check(&json!({"bindings": {"chat.new": ["Mod+Nope"]}}), false, true, Os::Linux).is_empty());
    }

    #[test]
    fn the_shell_menu_gets_tauri_accelerators() {
        let home = std::env::temp_dir().join(format!("thoughts-keys-native-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(native_on(&home, false, Os::Mac, false), native_defaults(Os::Mac));
        assert_eq!(native_defaults(Os::Windows).quit, None);
        assert_eq!(accelerator("Mod+Comma").unwrap(), "CmdOrCtrl+Comma");
        assert_eq!(accelerator("Mod+Control+Alt+Shift+Super+KeyK").unwrap(), "CmdOrCtrl+Ctrl+Alt+Shift+Super+KeyK");
        std::fs::write(home.join(FILE), json!({"format": 1, "bindings": {"settings": ["Mod+Semicolon", "F5"], "app.quit": [], "window.close": ["Mod+Shift+KeyW"],
            "window.fullscreen": ["Mod+KeyC"]}}).to_string()).unwrap();
        let k = native_on(&home, false, Os::Linux, false);
        assert_eq!(k.settings.as_deref(), Some("CmdOrCtrl+Semicolon"));
        assert_eq!(k.quit, None, "[] takes the key away");
        assert_eq!(k.close_window.as_deref(), Some("CmdOrCtrl+Shift+KeyW"));
        assert!(k.close_changed);
        assert_eq!(k.fullscreen.as_deref(), Some("F11"), "a reserved key is ignored: the default stays");
        assert_eq!(native_on(&home, false, Os::Linux, true).fullscreen.as_deref(), Some("CmdOrCtrl+KeyC"), "full access lets it through");
        assert_eq!(native_on(&home, true, Os::Linux, false), native_defaults(Os::Linux), "customisations off: the app's own keys");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[tokio::test]
    async fn a_set_saved_in_settings_is_one_undoable_entry() {
        let (st, d) = testutil::layer("keys").await;
        let b = json!({"chat.new": ["Mod+KeyJ"]});
        let hash = keys_in(st, &d.layer, &b, false).await.unwrap();
        assert!(!hash.is_empty());
        let m = applayer::manifest_of(&d.layer);
        assert_eq!(m["keys"]["bindings"], b);
        assert_eq!(m["keys"]["format"], 1);
        let h = history_in(st, &d.layer).await;
        assert_eq!(h[0]["summary"], act_title(false));
        assert_eq!(h[0]["files"], json!([FILE]));
        assert_eq!(keys_in(st, &d.layer, &b, false).await.unwrap(), "", "the same set again changes nothing");
        let e = keys_in(st, &d.layer, &json!({"chat.new": ["Mod+KeyC"]}), true).await.unwrap_err().to_string();
        assert!(e.contains("يستعمله النظام"), "{e}");
        assert_eq!(applayer::manifest_of(&d.layer)["keys"]["bindings"], b, "a refused set changes nothing");
        revert_in(st, &d.layer, &hash).await.unwrap();
        assert!(!d.layer.join(FILE).exists(), "undo brings the usual keys back");
        keys_in(st, &d.layer, &b, false).await.unwrap();
        keys_in(st, &d.layer, &json!({}), false).await.unwrap();
        assert!(!d.layer.join(FILE).exists(), "Reset all removes the file");
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    #[test]
    fn a_staged_keys_file_with_a_reserved_key_is_a_problem_unless_full_access() {
        let dir = std::env::temp_dir().join(format!("thoughts-keys-staged-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), json!({"format": 1, "bindings": {"chat.new": ["Mod+KeyR"]}}).to_string()).unwrap();
        let files = vec![(FILE.to_string(), "added")];
        let p = crate::staging::validate::problems(&dir, &files, false);
        assert!(p.iter().any(|x| x["path"] == FILE && x["message"].as_str().unwrap().contains("Recovery")), "{p:?}");
        crate::full_access::set_on(true);
        let p = crate::staging::validate::problems(&dir, &files, false);
        crate::full_access::set_on(false);
        assert!(!p.iter().any(|x| x["path"] == FILE), "{p:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
