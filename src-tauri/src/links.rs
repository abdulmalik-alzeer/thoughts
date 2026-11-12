//! `thoughts://` links (FEAT-38): a calendar entry, a reminder or another app can open a note, a
//! chat, or the quick capture box with some text already in it.
//!
//!   thoughts://note/<path in the library>      e.g. thoughts://note/ideas/shop/README.md
//!   thoughts://note/<folder>/                  the folder's README.md
//!   thoughts://chat/<chat id>
//!   thoughts://capture?text=<text>[&kind=todo|note]
//!
//! Anyone can make such a link (a web page too), so every part of it is checked here, in native
//! code, before anything happens: the path must be a plain path to a file inside the library that
//! the app may show (no `..`, no way out through a link, never the folders that can run programs),
//! a chat id must have the form the app gives chats, and text is cut to a sensible length. Nothing
//! is ever done without the user seeing it: a note or chat opens in the window, a note in a private
//! folder is only opened after the user said yes, and a capture link fills the capture box but never
//! saves by itself. A link that cannot be followed is explained in a native dialog.

use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::MessageDialogKind;

use crate::recovery::{arabic, choose, one_line, tell};

/// The URL scheme registered for the app (also in `plugins.deep-link` of the Tauri config).
pub(crate) const SCHEME: &str = "thoughts";
/// The longest text a capture link may put in the box (longer text is cut).
pub(crate) const MAX_TEXT: usize = 2000;
/// Anything longer is not a link the app made or could use.
const MAX_URL: usize = 16 * 1024;
const MAX_PATH: usize = 1024;

/// What a link asks for, once it passed the checks that need no files.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum Link {
    /// A library-relative path, `/`-separated and decoded. Ends with `/` for a folder.
    Note(String),
    Chat(String),
    Capture { text: String, kind: Option<&'static str> },
}

/// Why a link is not followed.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Bad {
    /// Not a `thoughts:` link at all.
    Foreign,
    /// A `thoughts:` link this version does not know.
    Unknown,
    /// The path is not a plain path inside the library.
    Path,
    /// Not the form a chat's id has.
    Chat,
}

/// Decode `%XX` escapes; `None` for a broken escape or bytes that are not UTF-8.
pub(crate) fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Text for the capture box: line breaks kept (Windows ones made plain), every other control
/// character dropped, trimmed, and at most `MAX_TEXT` characters.
pub(crate) fn clean_text(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let kept: String = text.chars().filter(|c| *c == '\n' || *c == '\t' || !c.is_control()).take(MAX_TEXT).collect();
    kept.trim().to_string()
}

/// The library path in a note link (`url.path()`, still percent-encoded).
fn note_path(encoded: &str) -> Result<String, Bad> {
    let inner = encoded.strip_prefix('/').unwrap_or(encoded);
    let (inner, folder) = match inner.strip_suffix('/') {
        Some(rest) => (rest, true),
        None => (inner, false),
    };
    if inner.is_empty() || inner.len() > MAX_PATH * 3 {
        return Err(Bad::Path);
    }
    let mut parts = Vec::new();
    for seg in inner.split('/') {
        let s = percent_decode(seg).ok_or(Bad::Path)?;
        // A part is a name: never empty, never `.`/`..`, never a separator or a drive (`C:`), never
        // a control character, however it was spelled in the link.
        let plain = !s.trim().is_empty() && s != "." && s != ".." && !s.starts_with('~')
            && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'));
        if !plain {
            return Err(Bad::Path);
        }
        parts.push(s);
    }
    let rel = parts.join("/");
    if rel.chars().count() > MAX_PATH {
        return Err(Bad::Path);
    }
    Ok(if folder { format!("{rel}/") } else { rel })
}

/// Read a link. Only the checks that need no files: `locate` does the rest for notes.
pub(crate) fn parse(raw: &str) -> Result<Link, Bad> {
    let raw = raw.trim();
    if raw.len() > MAX_URL {
        return Err(Bad::Foreign);
    }
    let url = tauri::Url::parse(raw).map_err(|_| Bad::Foreign)?;
    if url.scheme() != SCHEME {
        return Err(Bad::Foreign);
    }
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err(Bad::Unknown);
    }
    match url.host_str().unwrap_or("").to_ascii_lowercase().as_str() {
        "note" => note_path(url.path()).map(Link::Note),
        "chat" => {
            let id = url.path().strip_prefix('/').unwrap_or(url.path());
            let id = id.strip_suffix('/').unwrap_or(id);
            if thoughtsd::chats::valid_id(id) { Ok(Link::Chat(id.to_string())) } else { Err(Bad::Chat) }
        }
        "capture" => {
            if !matches!(url.path(), "" | "/") {
                return Err(Bad::Unknown);
            }
            let (mut text, mut kind) = (None, None);
            for (k, v) in url.query_pairs() {
                match k.as_ref() {
                    "text" if text.is_none() => text = Some(clean_text(&v)),
                    "kind" if kind.is_none() => kind = match v.as_ref() { "todo" => Some("todo"), "note" => Some("note"), _ => None },
                    _ => {}
                }
            }
            Ok(Link::Capture { text: text.unwrap_or_default(), kind })
        }
        _ => Err(Bad::Unknown),
    }
}

/// Why a note link points at nothing the app can open.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Missing {
    /// No such file (moved, renamed, deleted, or in another library).
    Gone,
    /// A folder the app never opens (`.git`, `.claude`…), or a way out of the library.
    Refused,
}

/// The file a note link names, inside `root`: `(library path, in a private folder?)`. A folder
/// stands for its README.md. Every path goes through `library::resolve` (the deny-list, no `..`,
/// no link leading out of the library), and the real file must still be inside the library.
pub(crate) fn locate(root: &Path, rel: &str, private: &[String]) -> Result<(String, bool), Missing> {
    let folder = rel.ends_with('/');
    let rel = rel.trim_end_matches('/');
    let p = thoughtsd::library::resolve(root, rel).map_err(|_| Missing::Refused)?;
    let rel = if folder || p.is_dir() { format!("{rel}/README.md") } else { rel.to_string() };
    let p = thoughtsd::library::resolve(root, &rel).map_err(|_| Missing::Refused)?;
    if !p.is_file() {
        return Err(Missing::Gone);
    }
    let (Ok(real_root), Ok(real)) = (dunce::canonicalize(root), dunce::canonicalize(&p)) else { return Err(Missing::Gone) };
    if !real.starts_with(&real_root) {
        return Err(Missing::Refused);
    }
    Ok((rel.clone(), thoughtsd::paths::is_private(root, &rel, private)))
}

/// The link that will open a library file (the "Copy link" actions use the same shape).
#[cfg(test)]
pub(crate) fn note_link(rel: &str) -> String {
    let enc = |seg: &str| -> String {
        seg.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
    };
    format!("{SCHEME}://note/{}", rel.split('/').map(enc).collect::<Vec<_>>().join("/"))
}

// ---------- words ----------

fn words(key: &str) -> (&'static str, &'static str) {
    match key {
        "open" => ("Open", "فتح"),
        "cancel" => ("Cancel", "إلغاء"),
        "bad.title" => ("This link cannot be opened", "تعذّر فتح هذا الرابط"),
        "bad.unknown" => (
            "Thoughts does not know this kind of link. It may come from a newer version of the app.",
            "لا يعرف Thoughts هذا النوع من الروابط. ربما جاء من إصدار أحدث من التطبيق.",
        ),
        "bad.path" => (
            "The link does not point to a note in your library, so nothing was opened.",
            "الرابط لا يشير إلى ملاحظة في مكتبتك، فلم يُفتح شيء.",
        ),
        "bad.chat" => ("The link does not point to a chat, so nothing was opened.", "الرابط لا يشير إلى محادثة، فلم يُفتح شيء."),
        "gone" => (
            "“{path}” is not in your library. It may have been moved, renamed or deleted, or the link was made on another computer.",
            "«{path}» ليست في مكتبتك. ربما نُقلت أو أُعيدت تسميتها أو حُذفت، أو أُنشئ الرابط على حاسوب آخر.",
        ),
        "refused" => (
            "“{path}” is in a part of the folder Thoughts never opens, so nothing was opened.",
            "«{path}» في جزء من المجلد لا يفتحه Thoughts أبدًا، فلم يُفتح شيء.",
        ),
        "nolib.title" => ("Thoughts is not set up yet", "لم يُجهَّز Thoughts بعد"),
        "nolib.body" => (
            "Finish setting up Thoughts first. After that, links to your notes open here.",
            "أكمل تجهيز Thoughts أولًا، وبعدها تُفتح روابط ملاحظاتك هنا.",
        ),
        "private.title" => ("Open a note from a private folder?", "فتح ملاحظة من مجلد خاص؟"),
        "private.body" => (
            "A link from another app asks to show “{path}”. It is in a folder you marked private.\n\nOpen it only if you made this link yourself or trust where it came from.",
            "يطلب رابط من تطبيق آخر عرض «{path}»، وهي في مجلد جعلته خاصًا.\n\nلا تفتحها إلا إن كنت أنشأت هذا الرابط بنفسك أو تثق بمصدره.",
        ),
        _ => ("", ""),
    }
}

fn tx(ar: bool, key: &str) -> &'static str {
    let (en, a) = words(key);
    if ar { a } else { en }
}

fn fill(ar: bool, key: &str, path: &str) -> String {
    tx(ar, key).replace("{path}", &one_line(path, 120))
}

// ---------- opening a link ----------

/// What the window should open, waiting until its page asks (`take_deep_link`): `{kind: "note",
/// path}`, `{kind: "chat", id}` or `{kind: "view", view}`. A page that is still starting picks it up
/// once it is ready.
static PENDING: Mutex<Option<Value>> = Mutex::new(None);
/// The link handled last and when: the same link can arrive twice at start-up (as the launch
/// link and as an event).
static LAST: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// Bring the window forward and have its page open `what`.
pub(crate) fn route(app: &AppHandle, what: Value) {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(what);
    crate::show_main(app);
    if let Some(w) = crate::main_window(app) {
        let _ = app.emit_to(w.label(), "deep-link", ());
    }
}

/// The page's side of `route`: what to open, once (null when nothing is waiting).
#[tauri::command]
pub(crate) fn take_deep_link(window: tauri::Window) -> Option<Value> {
    if !crate::is_main_label(window.label()) {
        return None;
    }
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Whether a second launch's arguments carry a link (it is handled as a link, not as "show the window").
pub(crate) fn in_args(args: &[String]) -> bool {
    args.iter().skip(1).any(|a| a.trim().to_ascii_lowercase().starts_with(&format!("{SCHEME}:")))
}

/// Links that arrived (from the system, a second launch or the launch itself). One at a time, each
/// on its own thread: answering may need a dialog.
pub(crate) fn handle(app: &AppHandle, urls: Vec<tauri::Url>) {
    // A link opens one thing; a batch of them (a page firing links) opens only the first.
    let Some(url) = urls.into_iter().next() else { return };
    let raw = url.to_string();
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_ref().is_some_and(|(u, at)| *u == raw && at.elapsed() < Duration::from_secs(2)) {
            return;
        }
        *last = Some((raw.clone(), Instant::now()));
    }
    let app = app.clone();
    std::thread::spawn(move || follow(&app, &raw));
}

fn follow(app: &AppHandle, raw: &str) {
    let ar = arabic();
    let refuse = |body: String| {
        crate::show_main(app);
        tell(app, MessageDialogKind::Warning, tx(ar, "bad.title"), &body);
    };
    match parse(raw) {
        Ok(Link::Capture { text, kind }) => crate::capture::open(app, Some(crate::capture::Prefill { text, kind })),
        Ok(Link::Chat(id)) => route(app, json!({"kind": "chat", "id": id})),
        Ok(Link::Note(rel)) => {
            let settings = thoughtsd::settings::Settings::load();
            let Some(root) = settings.library().filter(|_| settings.onboarded) else {
                crate::show_main(app);
                tell(app, MessageDialogKind::Info, tx(ar, "nolib.title"), tx(ar, "nolib.body"));
                return;
            };
            match locate(&root, &rel, &settings.private_paths) {
                Ok((path, private)) => {
                    if private {
                        crate::show_main(app);
                        let asked = choose(app, MessageDialogKind::Warning, tx(ar, "private.title"), &fill(ar, "private.body", &path), &[tx(ar, "open"), tx(ar, "cancel")]);
                        if asked != 0 {
                            return;
                        }
                    }
                    route(app, json!({"kind": "note", "path": path}));
                }
                Err(Missing::Gone) => refuse(fill(ar, "gone", rel.trim_end_matches('/'))),
                Err(Missing::Refused) => refuse(fill(ar, "refused", rel.trim_end_matches('/'))),
            }
        }
        // Not ours: the system only hands the app its own links, so there is nothing to say.
        Err(Bad::Foreign) => {}
        Err(Bad::Unknown) => refuse(tx(ar, "bad.unknown").to_string()),
        Err(Bad::Path) => refuse(tx(ar, "bad.path").to_string()),
        Err(Bad::Chat) => refuse(tx(ar, "bad.chat").to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_library() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-links-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        for dir in ["ideas/shop/notes", "me/finances", ".git", ".claude", "مجلد عربي"] {
            std::fs::create_dir_all(d.join(dir)).unwrap();
        }
        for f in ["ideas/shop/README.md", "ideas/shop/notes/plan.md", "me/finances/budget.md", ".git/config", ".claude/settings.json", "مجلد عربي/ملاحظة.md", "INDEX.md"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        d
    }

    #[test]
    fn note_links_are_plain_paths_inside_the_library() {
        assert_eq!(parse("thoughts://note/ideas/shop/README.md"), Ok(Link::Note("ideas/shop/README.md".into())));
        assert_eq!(parse("thoughts://note/ideas/shop/"), Ok(Link::Note("ideas/shop/".into())));
        assert_eq!(parse("  THOUGHTS://Note/INDEX.md  "), Ok(Link::Note("INDEX.md".into())));
        // Arabic and spaces, in the encoded form a browser or calendar sends them.
        let arabic = note_link("مجلد عربي/ملاحظة.md");
        assert_eq!(parse(&arabic), Ok(Link::Note("مجلد عربي/ملاحظة.md".into())));
        assert_eq!(parse("thoughts://note/my%20notes/a%2Bb.md"), Ok(Link::Note("my notes/a+b.md".into())));
        // Dot segments are folded away by the URL reader itself, so they can never climb out…
        assert_eq!(parse("thoughts://note/ideas/../../../etc/passwd"), Ok(Link::Note("etc/passwd".into())));
        assert_eq!(parse("thoughts://note/%2e%2e/%2E%2E/etc/passwd"), Ok(Link::Note("etc/passwd".into())));
        // …and anything that still smells of a way out, or is not a name, is refused.
        for bad in [
            "thoughts://note/", "thoughts://note", "thoughts://note//etc/passwd", "thoughts://note/a//b.md",
            "thoughts://note/a%2F..%2F..%2Fetc%2Fpasswd", "thoughts://note/..%5C..%5Cwindows%5Cwin.ini", "thoughts://note/C:%5Cboot.ini",
            "thoughts://note/C:/Windows/win.ini", "thoughts://note/a%00b.md", "thoughts://note/%7E/notes.md", "thoughts://note/a%ZZ.md",
            "thoughts://note/%FF%FE.md", "thoughts://note/%20/x.md",
        ] {
            assert_eq!(parse(bad), Err(Bad::Path), "{bad}");
        }
        assert_eq!(parse(&format!("thoughts://note/{}", "a/".repeat(600))), Err(Bad::Path));
    }

    #[test]
    fn chat_and_capture_links_are_checked_and_never_more_than_asked() {
        let id = "0b0f6c2e-1d2a-4f8e-9c1e-5c9f2d7e8a10";
        assert_eq!(parse(&format!("thoughts://chat/{id}")), Ok(Link::Chat(id.into())));
        assert_eq!(parse(&format!("thoughts://chat/{id}/")), Ok(Link::Chat(id.into())));
        // `..` is folded away by the URL reader: what is left is just an id no chat has.
        assert_eq!(parse("thoughts://chat/../settings"), Ok(Link::Chat("settings".into())));
        for bad in ["thoughts://chat/", "thoughts://chat/a%2Fb", "thoughts://chat/a/b", "thoughts://chat/a b", "thoughts://chat/دردشة"] {
            assert_eq!(parse(bad), Err(Bad::Chat), "{bad}");
        }
        assert_eq!(parse(&format!("thoughts://chat/{}", "a".repeat(65))), Err(Bad::Chat));

        let cap = |u: &str| parse(u).unwrap();
        assert_eq!(cap("thoughts://capture?text=Call%20the%20bank%20tomorrow"), Link::Capture { text: "Call the bank tomorrow".into(), kind: None });
        assert_eq!(cap("thoughts://capture/?text=a+b&kind=note"), Link::Capture { text: "a b".into(), kind: Some("note") });
        assert_eq!(cap("thoughts://capture?kind=todo&text=%D8%BA%D8%AF%D8%A7%D9%8B"), Link::Capture { text: "غداً".into(), kind: Some("todo") });
        // Only the first text counts, an unknown kind is no kind, and nothing else is read.
        assert_eq!(cap("thoughts://capture?text=one&text=two&kind=run&save=1"), Link::Capture { text: "one".into(), kind: None });
        assert_eq!(cap("thoughts://capture"), Link::Capture { text: String::new(), kind: None });
        // Control characters go, line breaks stay, and the text is cut to a sensible length.
        assert_eq!(cap("thoughts://capture?text=a%0D%0Ab%1B%5B31m%00c"), Link::Capture { text: "a\nb[31mc".into(), kind: None });
        let Link::Capture { text, .. } = cap(&format!("thoughts://capture?text={}", "x".repeat(5000))) else { panic!() };
        assert_eq!(text.chars().count(), MAX_TEXT);
        assert_eq!(parse("thoughts://capture/save?text=x"), Err(Bad::Unknown));
    }

    #[test]
    fn other_links_are_not_followed() {
        for foreign in ["https://example.com/note/x.md", "file:///etc/passwd", "javascript:alert(1)", "not a link", "thought://note/x.md", ""] {
            assert_eq!(parse(foreign), Err(Bad::Foreign), "{foreign}");
        }
        assert_eq!(parse(&format!("thoughts://capture?text={}", "x".repeat(MAX_URL))), Err(Bad::Foreign));
        for unknown in ["thoughts://settings", "thoughts://run/claude", "thoughts:note/x.md", "thoughts:///note/x.md", "thoughts://user:pw@note/x.md", "thoughts://note:99/x.md"] {
            assert_eq!(parse(unknown), Err(Bad::Unknown), "{unknown}");
        }
    }

    #[test]
    fn a_note_link_opens_only_a_real_file_the_app_may_show() {
        let root = temp_library();
        let private = vec!["me/finances".to_string()];
        assert_eq!(locate(&root, "ideas/shop/notes/plan.md", &private), Ok(("ideas/shop/notes/plan.md".into(), false)));
        // A folder opens its front page, with or without the slash.
        assert_eq!(locate(&root, "ideas/shop/", &private), Ok(("ideas/shop/README.md".into(), false)));
        assert_eq!(locate(&root, "ideas/shop", &private), Ok(("ideas/shop/README.md".into(), false)));
        assert_eq!(locate(&root, "مجلد عربي/ملاحظة.md", &private), Ok(("مجلد عربي/ملاحظة.md".into(), false)));
        // Private folders are found (and then asked about), whatever the spelling.
        assert_eq!(locate(&root, "me/finances/budget.md", &private), Ok(("me/finances/budget.md".into(), true)));
        assert_eq!(locate(&root, "Me/Finances/budget.md", &private).map(|(_, p)| p), Ok(true));
        // Missing files and folders without a front page.
        assert_eq!(locate(&root, "ideas/gone.md", &private), Err(Missing::Gone));
        assert_eq!(locate(&root, "me/finances/", &private), Err(Missing::Gone));
        // The folders that can make the computer run programs are never opened.
        assert_eq!(locate(&root, ".git/config", &private), Err(Missing::Refused));
        assert_eq!(locate(&root, ".claude/settings.json", &private), Err(Missing::Refused));
        // A link inside the library that leads out of it.
        #[cfg(unix)]
        {
            let outside = root.with_extension("outside");
            std::fs::create_dir_all(&outside).unwrap();
            std::fs::write(outside.join("secret.md"), "x").unwrap();
            std::os::unix::fs::symlink(&outside, root.join("ideas/away")).unwrap();
            assert_eq!(locate(&root, "ideas/away/secret.md", &private), Err(Missing::Refused));
            std::os::unix::fs::symlink(root.join(".git"), root.join("ideas/hidden")).unwrap();
            assert_eq!(locate(&root, "ideas/hidden/config", &private), Err(Missing::Refused));
            let _ = std::fs::remove_dir_all(&outside);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_launch_with_a_link_is_a_link() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(in_args(&args(&["/Applications/Thoughts.app/Contents/MacOS/Thoughts", "thoughts://note/INDEX.md"])));
        assert!(in_args(&args(&["C:\\Thoughts\\thoughts.exe", "THOUGHTS://capture?text=x"])));
        assert!(!in_args(&args(&["thoughts://note/INDEX.md"])), "the program itself is not an argument");
        assert!(!in_args(&args(&["thoughts", "--daemon"])));
        assert!(!in_args(&args(&["thoughts"])));
    }

    #[test]
    fn every_dialog_sentence_exists_in_both_languages() {
        for k in ["open", "cancel", "bad.title", "bad.unknown", "bad.path", "bad.chat", "gone", "refused", "nolib.title", "nolib.body", "private.title", "private.body"] {
            let (en, ar) = words(k);
            assert!(!en.is_empty() && !ar.is_empty(), "{k}");
        }
        assert!(fill(false, "gone", "a\nb").contains("“a b”"));
    }
}
