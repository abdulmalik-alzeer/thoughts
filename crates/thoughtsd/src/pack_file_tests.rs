//! Plugin and pack files as zips of their real files (`pack_file.rs`): what the archive holds, that
//! it reads back into exactly the pack it was made from, a whole export → add → keep with nothing but
//! the files (no assistant anywhere on the way), legacy `.json` files, and every refusal.

use super::*;
use crate::applayer::{testutil::layer, Dirs, Git};
use crate::{bundles, staging, workflows};
use std::path::Path;
use zip::write::SimpleFileOptions;

const NOW: &str = env!("CARGO_PKG_VERSION");
const ID: &str = "p-abcdefghijkl";

fn w(dir: &Path, path: &str, text: &str) {
    if let Some(p) = dir.join(path).parent() { std::fs::create_dir_all(p).unwrap(); }
    std::fs::write(dir.join(path), text).unwrap();
}

/// A plugin of every kind of part: an add-on, a page, a theme with its look, Home, the sidebar, the
/// Todos page, the app pages, keyboard shortcuts and an automatic task.
fn every_kind() -> Value {
    json!({
        "plugins/calendar.js": "// A calendar of your todos.\nexport default (api) => {\n  api.page({ id: 'calendar', title: 'Calendar' });\n};\n",
        "plugins/calendar.json": "{\n  \"apiVersion\": 1,\n  \"permissions\": [\"todos:read\"],\n  \"description\": \"A calendar of your todos\"\n}\n",
        "views/agenda.html": "<!doctype html>\n<h1>Agenda</h1>\n<p dir=\"auto\">مرحبا</p>\n",
        "views/agenda.json": "{\"apiVersion\":1,\"permissions\":[\"todos:read\"]}\n",
        "themes.json": "[{\"id\":\"dusk\",\"name\":\"Dusk\"}]\n",
        "custom.css": "[data-theme=\"dusk\"] { --bg: #101018; }\n.calendar { gap: 6px }\n",
        "home.json": "{\"order\":[\"widget:calendar\"]}\n",
        "nav.json": "{\"labels\":{\"todos\":{\"en\":\"Tasks\"}}}\n",
        "todos.json": "{\"groupBy\":\"due\"}\n",
        "pages.json": "{\"home\":{\"hidden\":[\"greeting\"]}}\n",
        "keys.json": "{\"format\":1,\"bindings\":{\"chat.new\":[\"Mod+KeyJ\"]}}\n",
        "workflows/agenda-mail.json": "{\"name\":\"Morning agenda\",\"trigger\":{\"type\":\"schedule\",\"every\":\"day\",\"at\":\"08:00\"},\"prompt\":\"Write today's agenda into my library.\"}\n",
    })
}

/// A plugin file's pack value as `bundles::export_in` makes it.
fn plugin_pack(files: Value) -> Value {
    let map: BTreeMap<String, String> = files.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
    json!({"format": "thoughts-pack", "version": 1, "apiVersion": 1, "kind": "feature", "id": ID, "name": "Calendar", "author": "Sara", "packVersion": "1.2.0",
        "description": "A calendar of your todos.", "appVersion": NOW, "fingerprint": packs::fingerprint("feature", &map), "files": files,
        "plugin": {"id": ID, "name": "Calendar", "version": "1.2.0", "minApp": NOW, "layerFormat": 1}})
}

/// The archive's entries, in order, with their bytes.
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..z.len()).map(|i| { let mut f = z.by_index(i).unwrap(); let mut b = vec![]; f.read_to_end(&mut b).unwrap(); (f.name().unwrap().to_string(), b) }).collect()
}

/// A zip made by hand: `(name, bytes, options)`.
fn zip_of(parts: &[(&str, &[u8], SimpleFileOptions)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, opt) in parts {
        if name.ends_with('/') { z.add_directory(*name, *opt).unwrap(); continue; }
        z.start_file(*name, *opt).unwrap_or_else(|e| panic!("{name}: {e}"));
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn opt() -> SimpleFileOptions { SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated) }

const MANIFEST_OK: &[u8] = br#"{"format":"thoughts-plugin","formatVersion":1,"kind":"feature","name":"X","version":"1.0.0"}"#;

fn refusal(bytes: &[u8]) -> String { read(bytes).unwrap_err().to_string() }

#[test]
fn a_plugin_file_is_a_zip_of_its_real_files() {
    let pack = plugin_pack(every_kind());
    let bytes = write(&pack).unwrap();
    assert!(is_archive(&bytes) && extension(&pack) == "thoughts-plugin");
    let got = entries(&bytes);
    // plugin.json first, then every file at its own path, as the code it is (not an escaped string).
    let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
    let kinds = every_kind();
    let mut want: Vec<&str> = kinds.as_object().unwrap().keys().map(|k| k.as_str()).collect::<Vec<_>>();
    want.sort();
    assert_eq!(names[0], "plugin.json");
    assert_eq!(&names[1..], want.as_slice());
    for (name, body) in &got[1..] { assert_eq!(std::str::from_utf8(body).unwrap(), every_kind()[name].as_str().unwrap(), "{name}"); }
    let m: Value = serde_json::from_slice(&got[0].1).unwrap();
    let files = packs::check_pack(&pack).unwrap();
    assert_eq!(m, json!({"format": "thoughts-plugin", "formatVersion": 1, "kind": "feature", "id": ID, "name": "Calendar", "version": "1.2.0", "author": "Sara",
        "description": "A calendar of your todos.", "minApp": NOW, "layerFormat": 1, "apiVersion": 1, "appVersion": NOW, "fingerprint": packs::fingerprint("feature", &files)}));
    // The same plugin always gives the same bytes; it reads back into the same pack.
    assert_eq!(write(&pack).unwrap(), bytes);
    let back = read(&bytes).unwrap();
    assert_eq!(packs::check_pack(&back).unwrap(), files);
    assert_eq!(write(&back).unwrap(), bytes, "and that pack writes the same file again");
    let tmp = std::env::temp_dir().join(format!("pf-inspect-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    assert_eq!(bundles::inspect_plugin(&back, &tmp).unwrap(), bundles::inspect_plugin(&pack, &tmp).unwrap(), "the same plugin lines");
    std::fs::remove_dir_all(&tmp).unwrap();
    // What an export route answers.
    let a = answer(&pack).unwrap();
    assert_eq!((&a["extension"], &a["kind"], &a["fingerprint"]), (&json!("thoughts-plugin"), &json!("feature"), &json!(packs::fingerprint("feature", &files))));
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(a["file"].as_str().unwrap()).unwrap(), bytes);
}

#[test]
fn packs_and_legacy_files_open_too() {
    // A feature pack of the Share tab, with the window's id: a `.thoughts-pack`, and its id stays.
    let mut feature = plugin_pack(json!({"custom.css": "a{}\n"}));
    feature.as_object_mut().unwrap().remove("plugin");
    feature["id"] = json!("k3j9x0ab12cd");
    assert_eq!(extension(&feature), "thoughts-pack");
    let back = read(&write(&feature).unwrap()).unwrap();
    assert_eq!((&back["id"], &back["kind"], &back["packVersion"]), (&json!("k3j9x0ab12cd"), &json!("feature"), &json!("1.2.0")));
    // A pack of everything keeps its list of plugins and which are off.
    let list = json!([{"id": ID, "name": "Calendar", "version": "1.0.0", "on": false, "claims": [{"file": {"path": "plugins/calendar.js"}}]}, {"id": "addon:plugins/x.js", "on": false}]);
    let full = json!({"format": "thoughts-pack", "version": 1, "apiVersion": 1, "kind": "full", "name": "Everything", "files": every_kind(), "plugins": list});
    let back = read(&write(&full).unwrap()).unwrap();
    assert_eq!((&back["kind"], &back["plugins"]), (&json!("full"), &list));
    assert!(back["plugin"].is_null(), "a pack of everything is not one plugin");
    assert_eq!(packs::check_pack(&back).unwrap(), packs::check_pack(&full).unwrap());
    // A legacy `.json` file is read as it is, also with a byte-order mark, also sent as the file.
    let legacy = plugin_pack(every_kind());
    let text = serde_json::to_vec_pretty(&legacy).unwrap();
    assert_eq!(open(&text).unwrap(), legacy);
    assert_eq!(open(&[b"\xef\xbb\xbf".as_slice(), &text].concat()).unwrap(), legacy);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&text);
    assert_eq!(received(None, Some(&b64)).unwrap(), legacy);
    assert_eq!(received(Some(legacy.clone()), None).unwrap(), legacy, "a window before 0.7.0 sends the pack itself");
    assert!(received(None, None).is_err() && received(None, Some("@@@")).is_err());
    assert!(open(b"hello").unwrap_err().to_string().contains("not a Thoughts customisation pack"));
    assert!(received(None, Some(&"A".repeat(MAX_ARCHIVE_BYTES * 2))).unwrap_err().to_string().contains("5 MB"));
}

#[test]
fn a_folder_zipped_by_hand_opens_and_what_the_computer_adds_is_left_out() {
    // Unzipped, edited, zipped again with the computer's own Compress: one folder around it all,
    // with `__MACOSX/` and `.DS_Store` beside the files.
    let bytes = zip_of(&[("Calendar/", b"", opt()), ("Calendar/plugin.json", MANIFEST_OK, opt()), ("Calendar/plugins/", b"", opt()),
        ("Calendar/plugins/calendar.js", b"export default () => {}\n", opt()), ("Calendar/.DS_Store", b"\0\0", opt()),
        ("__MACOSX/Calendar/plugins/._calendar.js", b"\0\x05\x16\x07", opt())]);
    let pack = read(&bytes).unwrap();
    assert_eq!(packs::check_pack(&pack).unwrap().keys().collect::<Vec<_>>(), ["plugins/calendar.js"]);
    // The manifest's claims grant nothing: its fingerprint is only compared (here it has none).
    let tmp = std::env::temp_dir().join(format!("pf-hand-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let look = packs::inspect(&pack, &tmp, &[]).unwrap();
    assert!(look["matches"].is_null());
    let mut lying = MANIFEST_OK.to_vec();
    lying.truncate(lying.len() - 1);
    lying.extend_from_slice(br#","fingerprint":"0000000000000000000000000000000000000000000000000000000000000000"}"#);
    let edited = read(&zip_of(&[("plugin.json", &lying, opt()), ("plugins/calendar.js", b"export default () => { steal() }\n", opt())])).unwrap();
    assert_eq!(packs::inspect(&edited, &tmp, &[]).unwrap()["matches"], false, "changed after it was made: the review says so");
    std::fs::remove_dir_all(&tmp).unwrap();
    // Two folders, or none with plugin.json in it, is not a plugin file.
    assert!(refusal(&zip_of(&[("a/plugin.json", MANIFEST_OK, opt()), ("b/plugins/x.js", b"", opt())])).contains("plugin.json"));
    assert!(refusal(&zip_of(&[("plugins/x.js", b"", opt())])).contains("plugin.json"));
}

#[test]
fn every_archive_trick_is_refused() {
    let m = ("plugin.json", MANIFEST_OK, opt());
    let file = |name: &'static str| (name, b"x".as_slice(), opt());
    // Outside the layer's places, climbing out, absolute, with a backslash or something invisible.
    for bad in ["README.md", "../plugins/x.js", "plugins/../../x.js", "/plugins/x.js", "plugins\\x.js", "C:/x.js", "plugins/a\u{202e}sj.txt", ".git/config", "other/x.js", "plugins/.hidden.js"] {
        let e = refusal(&zip_of(&[m, file(bad)]));
        assert!(e.contains("does not accept"), "{bad}: {e}");
    }
    assert!(refusal(&zip_of(&[m, ("other/", b"", opt()), file("plugins/a.js")])).contains("does not accept"), "a folder outside the layer's places");
    // The same name twice, also in other capitals (and plugin.json too).
    for parts in [vec![m, file("plugins/a.js"), file("plugins/A.js")], vec![m, ("PLUGIN.JSON", MANIFEST_OK, opt()), file("plugins/a.js")]] {
        let e = refusal(&zip_of(&parts));
        assert!(e.contains("more than once"), "{e}");
    }
    // (No zip writer makes the very same name twice: one is renamed in the bytes.)
    let mut twice = zip_of(&[m, file("plugins/a.js"), file("plugins/b.js")]);
    let at: Vec<usize> = twice.windows(12).enumerate().filter(|(_, w)| *w == b"plugins/b.js").map(|(i, _)| i).collect();
    assert_eq!(at.len(), 2, "its local header and its directory record");
    for i in at { twice[i + 8] = b'a'; }
    assert!(refusal(&twice).contains("could not be opened"), "the reader would keep only one of them");
    // Links and special entries.
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("plugin.json", opt()).unwrap();
    z.write_all(MANIFEST_OK).unwrap();
    z.add_symlink("plugins/a.js", "/etc/passwd", opt()).unwrap();
    let e = refusal(&z.finish().unwrap().into_inner());
    assert!(e.contains("special entry"), "{e}");
    let fifo = SimpleFileOptions::default().system(zip::System::Unix).external_attributes(0o010644 << 16);
    assert!(refusal(&zip_of(&[m, ("plugins/a.js", b"x", fifo)])).contains("special entry"));
    // A zip bomb: a little in the archive, far more than a file may be once unpacked.
    let zeros = vec![b' '; MAX_FILE_BYTES + 1];
    let bomb = zip_of(&[m, ("plugins/a.js", &zeros, opt())]);
    assert!(bomb.len() < 10_000);
    assert!(refusal(&bomb).contains("too large (at most 512 KB"));
    // Many files that are each allowed, together more than a pack may hold.
    let big = vec![b' '; MAX_FILE_BYTES];
    let names: Vec<String> = (0..9).map(|i| format!("plugins/p{i}.js")).collect();
    let mut parts = vec![m];
    for n in &names { parts.push((n.as_str(), big.as_slice(), opt())); }
    assert!(refusal(&zip_of(&parts)).contains("4 MB in all"));
    // More entries than a pack may have files.
    let names: Vec<String> = (0..=MAX_ENTRIES).map(|i| format!("plugins/p{i}.js")).collect();
    let mut parts = vec![m];
    for n in &names { parts.push((n.as_str(), b"".as_slice(), opt())); }
    assert!(refusal(&zip_of(&parts)).contains("too many files"));
    let names: Vec<String> = (0..=MAX_FILES).map(|i| format!("plugins/p{i}.js")).collect();
    let mut parts = vec![m];
    for n in &names { parts.push((n.as_str(), b"".as_slice(), opt())); }
    assert!(refusal(&zip_of(&parts)).contains("too many files"));
    // Not text; a damaged archive; too large as it arrives; nothing in it but plugin.json.
    assert!(refusal(&zip_of(&[m, ("plugins/a.js", b"\xff\xfe\x00", opt())])).contains("is not text"));
    let mut cut = zip_of(&[m, file("plugins/a.js")]);
    cut.truncate(cut.len() - 30);
    assert!(refusal(&cut).contains("could not be opened"));
    assert!(refusal(&vec![b'P'; MAX_ARCHIVE_BYTES + 1]).contains("5 MB"));
    assert!(refusal(&zip_of(&[m])).contains("nothing in it"));
    // What plugin.json says: not one, a newer file format, a newer layer, a full pack for a newer app.
    let with = |manifest: &str| refusal(&zip_of(&[("plugin.json", manifest.as_bytes(), opt()), file("plugins/a.js")]));
    assert!(with("[1]").contains("not a Thoughts customisation pack"));
    assert!(with("{broken").contains("not a Thoughts customisation pack"));
    assert!(with(r#"{"format":"zip","formatVersion":1,"kind":"feature"}"#).contains("not a Thoughts customisation pack"));
    assert!(with(r#"{"format":"thoughts-plugin","kind":"feature"}"#).contains("not a Thoughts customisation pack"));
    assert!(with(r#"{"format":"thoughts-plugin","formatVersion":2,"kind":"feature"}"#).contains("newer version of Thoughts"));
    assert!(with(r#"{"format":"thoughts-plugin","formatVersion":1,"kind":"theme"}"#).contains("newer version of Thoughts"));
    assert!(with(r#"{"format":"thoughts-plugin","formatVersion":1,"kind":"feature","apiVersion":9}"#).contains("newer version of Thoughts"));
    assert!(with(r#"{"format":"thoughts-plugin","formatVersion":1,"kind":"feature","layerFormat":"1"}"#).contains("newer version of Thoughts"));
    assert!(with(r#"{"format":"thoughts-pack","formatVersion":1,"kind":"full","layerFormat":9}"#).contains("newer version of Thoughts"));
    assert!(with(r#"{"format":"thoughts-pack","formatVersion":1,"kind":"full","minApp":"99.0.0"}"#).contains("99.0.0"));
    // A plugin for a newer app is read, and the review says it cannot be added (`bundles::check_compatible`).
    let newer = read(&zip_of(&[("plugin.json", br#"{"format":"thoughts-plugin","formatVersion":1,"kind":"feature","name":"X","minApp":"99.0.0"}"#.as_slice(), opt()), file("plugins/a.js")])).unwrap();
    assert!(bundles::check_compatible(&newer).unwrap_err().to_string().contains("99.0.0"));
    // Every refusal of an archive has its Arabic words too.
    for key in ["pack.archive_damaged", "pack.archive_too_big", "pack.archive_no_manifest", "pack.archive_odd_entry", "pack.archive_duplicate", "pack.archive_locked", "pack.archive_method", "pack.archive_bad_name"] {
        let (en, ar) = (crate::tr::text_in(false, key, &[("path", "plugins/a.js")]), crate::tr::text_in(true, key, &[("path", "plugins/a.js")]));
        assert!(en.contains("Nothing was changed") && ar.contains("لم يتغيّر شيء"), "{key}");
    }
}

async fn keep(st: Git<'_>, d: &Dirs) -> Value {
    let p = staging::pending(st, d, false).await;
    assert_eq!(p["problems"], json!([]), "{p}");
    staging::apply(st, d, false, p["signature"].as_str()).await.unwrap()
}

/// The owner's question: a plugin file is the plugin's code, and adding it needs no assistant. A
/// plugin with every kind of part goes from one computer to a fresh one as a `.thoughts-plugin`
/// file: inspected, staged, kept, listed and working there, with the same files byte for byte and
/// the same fingerprint. Every step is called here without the app's state (`Git(None)`, no
/// `AppState`): nothing on the way can start a chat or an assistant run, because nothing on the way
/// has what starting one takes.
#[tokio::test]
async fn a_plugin_file_goes_to_a_fresh_computer_and_works_with_no_assistant() {
    // On the first computer: the plugin is there (added once and kept), and is exported.
    let (st, a) = layer("pf-from").await;
    packs::import(st, &a, &plugin_pack(every_kind()), false).await.unwrap();
    keep(st, &a).await;
    let first = bundles::export_in(&a, ID).unwrap();
    let file = write(&first).unwrap();
    let sent = packs::check_pack(&first).unwrap();
    for (path, text) in &sent { if path.contains('/') { assert_eq!(text, &std::fs::read_to_string(a.layer.join(path)).unwrap(), "{path} is the file itself"); } }

    // On a fresh computer: the file is chosen, checked, staged, kept.
    let (st2, b) = layer("pf-to").await;
    let pack = received(None, Some(&base64::engine::general_purpose::STANDARD.encode(&file))).unwrap();
    let look = packs::inspect(&pack, &b.layer, &[]).unwrap();
    assert_eq!((&look["matches"], &look["fingerprint"], &look["kind"]), (&json!(true), &first["fingerprint"], &json!("feature")));
    assert_eq!((&look["plugin"]["compatible"], &look["plugin"]["replaces"], &look["plugin"]["id"]), (&json!(true), &Value::Null, &json!(ID)));
    packs::import_as(st2, &b, &pack, false, None, false).await.unwrap();
    assert!(!b.layer.join("plugins/calendar.js").exists(), "staged only until it is kept");
    let p = staging::pending(st2, &b, false).await;
    assert_eq!((&p["origin"], &p["plugin"]["target"], &p["pack"]["fingerprint"]), (&json!("import"), &json!("new"), &first["fingerprint"]));
    keep(st2, &b).await;

    // Kept and working: the add-on and the page with their permissions, the theme, every setting,
    // the automatic task (waiting for its own approval), all one plugin that is on.
    let m = crate::applayer::manifest_of(&b.layer);
    assert_eq!(m["plugins"][0]["name"], "calendar.js");
    assert_eq!(m["plugins"][0]["permissions"], json!(["todos:read"]));
    assert!(m["views"].as_array().unwrap().iter().any(|v| v["name"] == "agenda.html"), "{}", m["views"]);
    assert!(m["themes"].as_array().unwrap().iter().any(|t| t["id"] == "dusk"));
    assert_eq!(m["todos"]["groupBy"], "due");
    assert!(m["workflows"].as_array().unwrap().iter().any(|w| w["file"] == "agenda-mail.json" || w["name"] == "Morning agenda"), "{}", m["workflows"]);
    assert!(workflows::approved_workflows(&b.layer, &b.home).is_empty(), "an imported task never runs before the user allows it");
    let item = bundles::list(&b, false).into_iter().find(|x| x["id"] == ID).expect("listed as a plugin");
    assert_eq!((&item["on"], &item["state"], &item["name"], &item["version"]), (&json!(true), &json!("ok"), &json!("Calendar"), &json!("1.2.0")));
    for (path, text) in &sent { if path.contains('/') { assert_eq!(text, &std::fs::read_to_string(b.layer.join(path)).unwrap(), "{path} arrived byte for byte"); } }

    // Exported again there: the same files, the same fingerprint, the same file.
    let again = bundles::export_in(&b, ID).unwrap();
    assert_eq!(packs::check_pack(&again).unwrap(), sent);
    assert_eq!(again["fingerprint"], first["fingerprint"]);
    assert_eq!(write(&again).unwrap(), file, "byte for byte the same plugin file");

    // An update of it (a newer version, from a file) is also only files: it replaces the old one.
    let mut newer_files = every_kind();
    newer_files.as_object_mut().unwrap().remove("views/agenda.html");
    newer_files.as_object_mut().unwrap().remove("views/agenda.json");
    let mut newer = plugin_pack(newer_files);
    newer["packVersion"] = json!("1.3.0");
    newer["plugin"]["version"] = json!("1.3.0");
    let newer = read(&write(&newer).unwrap()).unwrap();
    assert_eq!(packs::inspect(&newer, &b.layer, &[]).unwrap()["plugin"]["replaces"]["id"], ID);
    packs::import_as(st2, &b, &newer, false, None, false).await.unwrap();
    keep(st2, &b).await;
    assert!(!b.layer.join("views/agenda.html").exists(), "what the new version dropped is gone");
    assert_eq!(bundles::load(&b.layer).bundles[ID].version, "1.3.0");
    std::fs::remove_dir_all(&a.home).unwrap();
    std::fs::remove_dir_all(&b.home).unwrap();
}

/// Export all, as a `.thoughts-pack`, onto a fresh computer: the same plugins, what was off still off.
#[tokio::test]
async fn export_all_as_a_pack_file_moves_every_plugin() {
    let (st, a) = layer("pf-all-from").await;
    packs::import(st, &a, &plugin_pack(every_kind()), false).await.unwrap();
    keep(st, &a).await;
    w(&a.layer, "plugins/loose.js", "export default () => {}\n");
    st.commit_all(&a.layer, "a loose add-on").await.unwrap();
    bundles::switch_in(st, &a, ID, false).await.unwrap();
    let all = bundles::export_all(&a, "Everything").unwrap();
    let file = write(&all).unwrap();
    assert_eq!(extension(&all), "thoughts-pack");
    let (st2, b) = layer("pf-all-to").await;
    let pack = open(&file).unwrap();
    assert_eq!(pack["kind"], "full");
    packs::import(st2, &b, &pack, false).await.unwrap();
    keep(st2, &b).await;
    let f = bundles::load(&b.layer);
    assert!(f.bundles.contains_key(ID) && f.off.contains(ID), "the plugin is there, and still off");
    assert!(b.layer.join("plugins/loose.js").is_file() && b.layer.join("plugins/calendar.js").is_file());
    std::fs::remove_dir_all(&a.home).unwrap();
    std::fs::remove_dir_all(&b.home).unwrap();
}
