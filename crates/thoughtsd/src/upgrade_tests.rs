//! Upgrading to 0.1.0 from 0.0.12–0.0.14: what an older copy left on disk is read as before, with
//! the new choices at their defaults, and nothing is reset or moved aside. The fixtures are written
//! in the shapes those versions wrote (no `showCost`, no `sync.checkRemote`, chats with whole tool
//! inputs and an approval card still waiting, "always" approvals), into temp folders only.

use crate::settings::Settings;

fn temp() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("thoughts-upgrade-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A settings.json as 0.0.14 wrote it: two libraries (the other one parked, with its own backup
/// settings), a provider address, a spending limit, quick capture — and none of 0.1.0's new fields.
const SETTINGS_0_0_14: &str = r#"{
  "schemaVersion": 2,
  "onboarded": true,
  "userName": "Sara",
  "language": "ar",
  "theme": "saudi",
  "mode": "dark",
  "libraryPath": "/Users/sara/Thoughts",
  "ai": {
    "primary": "claude",
    "claude": { "binary": "/opt/homebrew/bin/claude", "model": "opus", "effort": "high", "permissionMode": "acceptEdits",
                "miniWrapModel": "haiku", "baseUrl": null, "hasApiToken": false, "suggestNext": true }
  },
  "sync": { "mode": "git-auto", "credentialStore": "keychain", "mainLocation": "origin", "allowCloud": false, "target": "online" },
  "keepHistory": true,
  "modules": { "todos": true, "calendar": false },
  "daemon": { "runAtLogin": true, "keepRunning": true },
  "notifications": true,
  "chatSteps": false,
  "idleMinutes": 20,
  "wrapNagThreshold": 3,
  "chatRetentionDays": 90,
  "autoSummaries": true,
  "projects": [{ "id": "p-1", "name": "Site", "path": "/Users/sara/site" }],
  "lastWrap": "2026-11-10T08:00:00Z",
  "privatePaths": ["me/finances"],
  "strictPrivate": true,
  "usageSignals": true,
  "profession": "consulting",
  "accessibility": { "zoom": 110, "reduceMotion": true, "underlineLinks": false, "strongFocus": false, "textSpacing": false },
  "calendar": { "weekStart": 0, "weekend": [5, 6], "dates": "hijri" },
  "releaseCheck": true,
  "assistantReports": false,
  "spending": { "monthlyLimitUsd": 25.0, "warnPercent": 80, "wholeComputer": false },
  "capture": { "shortcutOn": true, "shortcut": "Alt+Space" },
  "libraries": [
    { "id": "lib-a", "name": "", "path": "/Users/sara/Thoughts" },
    { "id": "lib-b", "name": "Work", "path": "/Users/sara/Work",
      "parked": { "sync": { "mode": "git-manual", "credentialStore": "keychain", "mainLocation": "backup", "allowCloud": false, "target": "drive" },
                  "keepHistory": true, "privatePaths": ["clients"], "profession": null, "modules": {}, "projects": [], "lastWrap": null } }
  ]
}"#;

#[test]
fn a_settings_file_from_0_0_14_loads_whole_with_the_new_choices_at_their_defaults() {
    let d = temp();
    let file = d.join("settings.json");
    std::fs::write(&file, SETTINGS_0_0_14).unwrap();
    let l = Settings::load_full(&file);
    assert!(!l.reset && !l.restored && !l.newer, "read as it is: not reset, not restored from a backup, not newer");
    let s = l.settings;
    assert_eq!((s.language.as_str(), s.user_name.as_str(), s.theme.as_str()), ("ar", "Sara", "saudi"));
    assert_eq!(s.ai.claude.model.as_deref(), Some("opus"));
    assert_eq!(s.ai.claude.permission_mode.as_deref(), Some("acceptEdits"));
    assert_eq!(s.sync.mode, "git-auto");
    assert_eq!(s.spending.monthly_limit_usd, Some(25.0));
    assert_eq!(s.private_paths, ["me/finances"]);
    assert_eq!(s.libraries.len(), 2);
    let parked = s.libraries[1].parked.as_ref().expect("the other library keeps its own settings");
    assert_eq!((parked.sync.target.as_str(), parked.private_paths.clone()), ("drive", vec!["clients".to_string()]));
    // 0.1.0's new choices: the cost line keeps following the window's old choice until it is saved,
    // and the background check for changes is off, here and in the parked library.
    assert_eq!(s.show_cost, None);
    assert!(!s.sync.check_remote && !parked.sync.check_remote);
    // Nothing on disk was moved aside or rewritten by loading it.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), SETTINGS_0_0_14);
    let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert_eq!(left, ["settings.json"]);
    // Saved again by 0.1.0 and read back: the same settings, still nothing new written for the cost line.
    s.save_to(&file).unwrap();
    let again = Settings::load_full(&file).settings;
    assert_eq!(serde_json::to_value(&again).unwrap(), serde_json::to_value(&s).unwrap());
    assert!(!std::fs::read_to_string(&file).unwrap().contains("showCost"));
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn a_move_file_from_0_0_14_is_read_and_its_settings_merge() {
    let settings: serde_json::Value = serde_json::from_str(SETTINGS_0_0_14).unwrap();
    let file = serde_json::json!({
        "format": crate::transfer::FORMAT, "version": 1, "appVersion": "0.0.14", "createdAt": "2026-11-12T10:00:00Z", "platform": "macos",
        "settings": settings, "chats": [], "attachments": {}, "customisations": null,
    });
    let b = crate::transfer::read(&file).expect("a move file 0.0.14 made is still read");
    let from = b.settings.unwrap();
    assert_eq!(from.show_cost, None);
    let mut here = Settings::default();
    here.show_cost = Some(true);
    let n = crate::transfer::merged_settings(&here, &from);
    assert_eq!(n.show_cost, Some(true), "a file without the cost choice leaves this computer's");
    assert_eq!(n.language, "ar");
}

#[test]
fn chats_and_always_approvals_from_0_0_14_load() {
    let d = temp();
    let chats = d.join("chats");
    std::fs::create_dir_all(&chats).unwrap();
    // A chat as 0.0.14 saved it while a run was going: a whole file in a step, an approval card
    // still waiting, the run marked running.
    let big = "x".repeat(50_000);
    let chat = serde_json::json!({
        "id": "c-old", "title": "Old chat", "kind": "chat", "running": true, "createdAt": "2026-11-01T09:00:00Z",
        "options": { "model": "opus", "permissionMode": "acceptEdits" },
        "messages": [
            {"id": "m1", "role": "user", "text": "write it", "ts": "2026-11-01T09:00:00Z"},
            {"id": "m2", "role": "tool", "text": "Write notes/a.md", "ts": "2026-11-01T09:00:01Z",
             "tool": {"name": "Write", "id": "toolu_1", "input": {"file_path": "/lib/notes/a.md", "content": big}}},
            {"id": "m3", "role": "approval", "text": "asked", "ts": "2026-11-01T09:00:02Z",
             "tool": {"code": "approval", "status": "waiting", "card": {"tool": "Bash"}, "expiresAt": "2026-11-01T09:05:02Z"}}
        ]
    });
    std::fs::write(chats.join("c-old.json"), chat.to_string()).unwrap();
    let store = crate::chats::ChatStore::load_from(&chats);
    let c = store.chats.try_read().unwrap()["c-old"].clone();
    assert_eq!(c.title, "Old chat");
    assert_eq!(c.messages.len(), 3);
    assert!(!c.running && c.interrupted, "a run cut off by the update is shown as interrupted");
    assert_eq!(c.messages[2].tool.as_ref().unwrap()["status"], "ended");
    assert_eq!(c.messages[1].tool.as_ref().unwrap()["input"]["file_path"], "/lib/notes/a.md");
    assert_eq!(c.options.model.as_deref(), Some("opus"));

    // "Always for this chat" as 0.0.14 kept it.
    let file = d.join("chat-approvals.json");
    std::fs::write(&file, serde_json::json!({"c-old": [
        {"id": "a1", "kind": "command", "tool": "Bash", "key": "npm test", "what": "npm test", "target": "", "at": "2026-11-01T09:00:00Z"}
    ]}).to_string()).unwrap();
    let a = crate::approvals::Approvals::at(file);
    let always = a.always("c-old");
    assert_eq!(always.len(), 1);
    assert_eq!((always[0].kind.as_str(), always[0].key.as_str()), ("command", "npm test"));
    std::fs::remove_dir_all(d).unwrap();
}

// ---------- 0.4.1 → 0.5.0: customisations become plugins, and nothing is lost ----------

mod plugins_from_0_4_1 {
    use crate::applayer::{self, testutil, Dirs, Git, PackMark};
    use crate::bundles::{self, Claim};
    use crate::{packs, workflows};
    use serde_json::{json, Value};
    use std::collections::{BTreeMap, BTreeSet};

    fn w(d: &Dirs, path: &str, text: &str) {
        if let Some(p) = d.layer.join(path).parent() { std::fs::create_dir_all(p).unwrap(); }
        std::fs::write(d.layer.join(path), text).unwrap();
    }

    /// One entry as 0.4.1 kept a pack: the files as its merge wrote them, the pack's own summary as
    /// the entry's text, and the pack's identity in the marks beside the history.
    async fn kept_pack(st: Git<'_>, d: &Dirs, summary: &str, mark: PackMark) -> String {
        st.commit_all(&d.layer, summary).await.unwrap();
        let hash = st.rev(&d.layer, "HEAD").await.unwrap();
        applayer::mark_pack(st, &d.layer, &hash, mark).await;
        hash
    }

    const WEEKLY: &str = r#"{"name":"Weekly review","trigger":{"type":"schedule","every":"week","at":"16:00","weekday":5},"prompt":"Write my weekly review."}"#;
    const KANBAN_PRINT: &str = "3f9a12c4aa00bb11cc22dd33ee44ff5566778899aabbccddeeff001122334455";
    const SAND_PRINT: &str = "0000111122223333444455556666777788889999aaaabbbbccccddddeeeeffff";

    /// A layer as a 0.4.1 user left it: their own changes, two packs kept (one shared by someone, one
    /// ready-made), a third pack kept and then undone, an add-on switched off, a task allowed to run,
    /// the sidebar edited by hand. No `bundles.json`: 0.4.1 never heard of it.
    /// → `(the Kanban entry, the Sand entry)`.
    async fn layer_0_4_1(name: &str) -> (Git<'static>, Dirs, String, String) {
        let (st, d) = testutil::layer(name).await;
        // Their own first change.
        w(&d, "custom.css", "/* Your overrides. */\n:root { --radius: 2px }\n");
        w(&d, "home.json", "{\n  \"chips\": [\n    {\n      \"label\": \"Plan\",\n      \"prompt\": \"Plan my week: \"\n    }\n  ],\n  \"hidden\": [\n    \"stats\"\n  ],\n  \"order\": [\n    \"todos\"\n  ]\n}\n");
        w(&d, "plugins/mine.js", "export default () => { /* mine */ }\n");
        w(&d, "manifest.json", "{\n  \"apiVersion\": 1,\n  \"plugins\": {\n    \"mine.js\": {\n      \"permissions\": [\n        \"storage\"\n      ]\n    }\n  }\n}\n");
        w(&d, "workflows/weekly.json", WEEKLY);
        st.commit_all(&d.layer, "Softer corners and my own add-on\n\nSofter corners and my own add-on").await.unwrap();
        // "Kanban", a feature pack someone sent (exported by 0.4.1 with its 12-character id): merged as 0.4.1 merges.
        w(&d, "plugins/board.js", "export default () => { /* board */ }\n");
        w(&d, "plugins/board.json", "{\n  \"apiVersion\": 1,\n  \"permissions\": [\n    \"ideas:read\",\n    \"ideas:write\"\n  ]\n}\n");
        w(&d, "home.json", "{\n  \"chips\": [\n    {\n      \"label\": \"Plan\",\n      \"prompt\": \"Plan my week: \"\n    },\n    {\n      \"label\": \"Board\",\n      \"prompt\": \"Open my board\"\n    }\n  ],\n  \"hidden\": [\n    \"stats\"\n  ],\n  \"order\": [\n    \"todos\",\n    \"widget:board\"\n  ]\n}\n");
        w(&d, "custom.css", "/* Your overrides. */\n:root { --radius: 2px }\n\n/* pack:Kanban k3j9x0ab12cd */\n.board { gap: 8px }\n/* end pack:Kanban k3j9x0ab12cd */\n");
        let kanban = kept_pack(st, &d, "Add the pack \"Kanban\" to your customisations\n\nAdd the pack \"Kanban\" to your customisations\n\nThe pack brings 4 files and is merged into what you already have.",
            PackMark { name: "Kanban".into(), author: "Sara".into(), version: "1.2".into(), fingerprint: KANBAN_PRINT.into() }).await;
        // "Sand theme", ready-made (0.4.1's own words; no id, so its block is keyed by its name alone).
        w(&d, "themes.json", "[\n  {\n    \"id\": \"sand\",\n    \"name\": \"Sand\"\n  }\n]\n");
        let css = std::fs::read_to_string(d.layer.join("custom.css")).unwrap();
        w(&d, "custom.css", &format!("{css}\n/* pack:Sand theme */\n[data-theme=\"sand\"] {{ --bg: #f7f1e5 }}\n/* end pack:Sand theme */\n"));
        let sand = kept_pack(st, &d, "Add \u{201c}Sand theme\u{201d} from the ready-made packs\n\nAdd \u{201c}Sand theme\u{201d} from the ready-made packs\n\nA warm sand-coloured theme.",
            PackMark { name: "Sand theme".into(), author: "Thoughts".into(), version: "0.4.1".into(), fingerprint: SAND_PRINT.into() }).await;
        // A pack they kept and then undid: no longer part of the app.
        w(&d, "todos.json", "{\n  \"groupBy\": \"due\"\n}\n");
        let undone = kept_pack(st, &d, "Add \u{201c}Todos by date\u{201d} from the ready-made packs", PackMark { name: "Todos by date".into(), author: "Thoughts".into(), version: "0.4.1".into(), fingerprint: "ab".repeat(32) }).await;
        applayer::revert_in(st, &d.layer, &undone).await.unwrap();
        // The rest of what 0.4.1 could do: an add-on switched off, a task allowed, the sidebar by hand.
        applayer::switch_in(st, &d.layer, "plugins/mine.js", false).await.unwrap();
        let weekly = workflows::load_files(&d.layer).remove(0);
        workflows::approve(&d.layer, &d.home, &weekly.file, &weekly.hash).unwrap();
        applayer::nav_in(st, &d.layer, &json!({"labels": {"ideas": {"en": "Clients"}}, "hidden": ["graph"]}), false).await.unwrap();
        assert!(!d.layer.join(bundles::FILE).exists());
        (st, d, kanban, sand)
    }

    /// Every file of the layer, the app's own records included (all but the new one).
    fn everything(d: &Dirs) -> BTreeMap<String, String> {
        let mut all = packs::collect(&d.layer);
        for f in ["addons.json", "README.md", "CLAUDE.md"] { all.insert(f.into(), std::fs::read_to_string(d.layer.join(f)).unwrap_or_default()); }
        all
    }

    fn ids(d: &Dirs) -> Vec<(String, String, bool)> {
        bundles::list(d, false).iter().map(|p| (p["id"].as_str().unwrap().to_string(), p["name"].as_str().unwrap().to_string(), p["on"].as_bool().unwrap())).collect()
    }

    #[tokio::test]
    async fn a_0_4_1_layer_starts_under_0_5_0_with_everything_listed_and_nothing_lost() {
        let (st, d, kanban, sand) = layer_0_4_1("upgrade-plugins").await;
        let (files, loads, history) = (everything(&d), applayer::manifest_of(&d.layer), applayer::history_in(st, &d.layer).await);
        let runs = |d: &Dirs| workflows::approved_workflows(&d.layer, &d.home).into_iter().map(|l| l.file).collect::<Vec<_>>();
        assert_eq!(runs(&d), ["weekly.json"]);

        // The first start of 0.5.0.
        applayer::ensure_in(st, &d).await.unwrap();

        // Nothing the user had changed: every file is byte for byte what it was, the app loads the
        // very same customisations, the same task runs, and the history is theirs plus one entry of the app's own.
        assert_eq!(everything(&d), files, "every layer file is byte-identical");
        assert_eq!(applayer::manifest_of(&d.layer), loads, "what the app loads is exactly what it loaded before");
        assert_eq!(runs(&d), ["weekly.json"]);
        assert_eq!(applayer::switched_off(&d.layer), BTreeSet::from(["plugins/mine.js".to_string()]), "what was off is still off");
        let after = applayer::history_in(st, &d.layer).await;
        assert_eq!(after.len(), history.len() + 1, "one entry more");
        assert_eq!(&after[1..], &history[..], "every earlier entry is as it was");
        assert_eq!((after[0]["summary"].as_str(), after[0]["kind"].as_str(), after[0]["entry"].as_str(), after[0]["undoable"].as_bool(), &after[0]["files"]),
            (Some("Customisations listed as plugins"), Some("system"), Some("plugins-organise"), Some(false), &json!([bundles::FILE])));
        assert_eq!(applayer::history_page_in(st, &d.layer, 0, 1, true).await[0]["summary"], "إدراج تخصيصاتك كملحقات");

        // The two kept packs are named plugins, each owning exactly what its pack brought and is still there.
        let f = bundles::load(&d.layer);
        assert_eq!(f.bundles.len(), 2, "{:?}", f.bundles.keys());
        assert_eq!(f.migrated, env!("CARGO_PKG_VERSION"));
        let by_name = |n: &str| f.bundles.values().find(|b| b.name == n).unwrap().clone();
        let (k, s) = (by_name("Kanban"), by_name("Sand theme"));
        let file = |p: &str| Claim::File { path: p.into() };
        let item = |f: &str, ptr: &str, key: &str, v: &str| Claim::Item { file: f.into(), pointer: ptr.into(), matches: BTreeMap::from([(key.to_string(), v.to_string())]) };
        let css = |b: &str| Claim::Css { block: b.into() };
        assert_eq!((k.origin.as_str(), k.version.as_str(), k.author.as_str(), k.fingerprint.as_str(), k.min_app.as_str(), k.layer_format), ("migrated", "1.2", "Sara", KANBAN_PRINT, "", bundles::FORMAT));
        assert_eq!(k.claims.iter().cloned().collect::<BTreeSet<_>>(), BTreeSet::from([file("plugins/board.js"), file("plugins/board.json"), css("pack:Kanban k3j9x0ab12cd"),
            item("home.json", "/chips", "label", "Board"), item("home.json", "/order", "value", "widget:board")]));
        assert_eq!((s.origin.as_str(), s.author.as_str(), s.fingerprint.as_str()), ("ready", "Thoughts", SAND_PRINT));
        assert_eq!(s.claims.iter().cloned().collect::<BTreeSet<_>>(), BTreeSet::from([css("pack:Sand theme"), item("themes.json", "", "id", "sand")]));
        // Ids come from the pack and its entry: the same on every run.
        assert_eq!((k.id.as_str(), s.id.as_str()), (bundles::id_of(&format!("{KANBAN_PRINT}{kanban}")).as_str(), bundles::id_of(&format!("{SAND_PRINT}{sand}")).as_str()));

        // The list is complete: the two plugins, and everything else on its own, each thing once.
        let list = ids(&d);
        assert_eq!(list.iter().map(|(id, name, on)| (id.as_str(), name.as_str(), *on)).collect::<Vec<_>>(), [
            (k.id.as_str(), "Kanban", true), (s.id.as_str(), "Sand theme", true),
            ("addon:plugins/mine.js", "mine", false), ("task:workflows/weekly.json", "Weekly review", true),
            ("home", "Home arrangement", true), ("sidebar", "Sidebar", true), ("look", "Your look", true)]);
        let listed = bundles::list(&d, false);
        assert_eq!(listed[0]["permissions"], json!(["ideas:read", "ideas:write"]));
        assert_eq!(listed[2]["permissions"], json!(["storage"]), "a permission declared in manifest.json, as 0.4.1 allowed");
        assert_eq!(listed[3]["tasks"], json!([{"file": "weekly.json", "name": "Weekly review", "approved": true}]));

        // Starting again changes nothing (also after the app wrote the record for another reason).
        applayer::ensure_in(st, &d).await.unwrap();
        assert_eq!(applayer::history_in(st, &d.layer).await.len(), after.len());
        assert_eq!(bundles::load(&d.layer), f);

        // The new abilities work on the old customisations: off and on restores exactly, and each exports.
        bundles::switch_in(st, &d, &k.id, false).await.unwrap();
        let off = applayer::manifest_of(&d.layer);
        assert!(off["plugins"].as_array().unwrap().is_empty() && !off["css"].as_str().unwrap().contains(".board") && off["css"].as_str().unwrap().contains("--radius: 2px") && off["css"].as_str().unwrap().contains("data-theme"));
        assert_eq!(off["home"]["chips"], json!([{"label": "Plan", "prompt": "Plan my week: "}]));
        bundles::switch_in(st, &d, &k.id, true).await.unwrap();
        assert_eq!(applayer::manifest_of(&d.layer), loads);
        assert_eq!(everything(&d), files);
        let pack = bundles::export_in(&d, &k.id).unwrap();
        assert_eq!(packs::check_pack(&pack).unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["custom.css", "home.json", "plugins/board.js", "plugins/board.json"]);
        assert_eq!(packs::check_pack(&pack).unwrap()["custom.css"], ".board { gap: 8px }\n");

        // Every earlier entry can still be undone on its own: here the ready-made pack, kept under 0.4.1.
        applayer::revert_in(st, &d.layer, &sand).await.unwrap();
        let m = applayer::manifest_of(&d.layer);
        assert_eq!(m["themes"], json!([]));
        assert!(!m["css"].as_str().unwrap().contains("data-theme") && m["css"].as_str().unwrap().contains(".board"));
        assert_eq!(bundles::load(&d.layer).bundles.keys().collect::<Vec<_>>(), [&k.id], "its plugin went with it, in the same undo");
        let undo = applayer::history_in(st, &d.layer).await[0]["hash"].as_str().unwrap().to_string();
        applayer::revert_in(st, &d.layer, &undo).await.unwrap();
        assert_eq!(bundles::load(&d.layer).bundles[&s.id].claims.len(), 2, "and came back with it");
        assert_eq!(applayer::manifest_of(&d.layer), loads);
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    /// Rollback to a point before the listing takes the record away with everything else; the next
    /// start lists the packs again, as the same plugins.
    #[tokio::test]
    async fn after_a_rollback_to_before_it_the_listing_is_done_again_with_the_same_ids() {
        let (st, d, _, _) = layer_0_4_1("upgrade-rollback").await;
        let before = applayer::history_in(st, &d.layer).await[0]["hash"].as_str().unwrap().to_string();
        applayer::ensure_in(st, &d).await.unwrap();
        let first = bundles::load(&d.layer);
        let files = everything(&d);
        // Something done since, then back to before 0.5.0 ever started.
        let kanban = first.bundles.values().find(|b| b.name == "Kanban").unwrap().id.clone();
        bundles::switch_in(st, &d, &kanban, false).await.unwrap();
        applayer::rollback_in(st, &d.layer, &before).await.unwrap();
        assert!(!d.layer.join(bundles::FILE).exists(), "the record went back with the rest");
        assert_eq!(everything(&d), files);
        assert_eq!(ids(&d).iter().filter(|(id, ..)| id.starts_with("p-")).count(), 0, "until the next start, everything is on its own");
        let entries = applayer::history_in(st, &d.layer).await.len();
        applayer::ensure_in(st, &d).await.unwrap();
        let again = bundles::load(&d.layer);
        assert_eq!(applayer::history_in(st, &d.layer).await.len(), entries + 1);
        assert_eq!(again.bundles.keys().collect::<Vec<_>>(), first.bundles.keys().collect::<Vec<_>>(), "the same ids");
        for (id, b) in &first.bundles { assert_eq!((&again.bundles[id].claims, &again.bundles[id].name, &again.bundles[id].origin), (&b.claims, &b.name, &b.origin)); }
        assert!(again.off.is_empty());
        assert_eq!(everything(&d), files);
        // Rollback itself still goes anywhere, the starting point included.
        let start = applayer::history_in(st, &d.layer).await.last().unwrap()["hash"].as_str().unwrap().to_string();
        applayer::rollback_in(st, &d.layer, &start).await.unwrap();
        assert!(!d.layer.join("plugins/board.js").exists() && !d.layer.join(bundles::FILE).exists());
        applayer::ensure_in(st, &d).await.unwrap();
        assert!(!d.layer.join(bundles::FILE).exists(), "nothing of the packs is left, so there is nothing to list");
        assert_eq!(ids(&d), Vec::<(String, String, bool)>::new());
        std::fs::remove_dir_all(&d.home).unwrap();
    }

    /// A 0.4.1 layer with no pack ever kept gets no record and no entry at all.
    #[tokio::test]
    async fn a_layer_with_no_kept_pack_is_left_exactly_as_it_was() {
        let (st, d) = testutil::layer("upgrade-plain").await;
        w(&d, "custom.css", "/* Your overrides. */\nbody { margin: 0 }\n");
        w(&d, "plugins/mine.js", "export default () => {}\n");
        st.commit_all(&d.layer, "Mine").await.unwrap();
        let (files, n) = (everything(&d), applayer::history_in(st, &d.layer).await.len());
        applayer::ensure_in(st, &d).await.unwrap();
        assert_eq!((everything(&d), applayer::history_in(st, &d.layer).await.len(), d.layer.join(bundles::FILE).exists()), (files, n, false));
        assert_eq!(ids(&d).iter().map(|(id, ..)| id.as_str()).collect::<Vec<_>>(), ["addon:plugins/mine.js", "look"]);
        // A pack as 0.4.1 exported it still imports, and is a plugin from then on.
        let old: Value = json!({"format": "thoughts-pack", "version": 1, "apiVersion": 1, "kind": "feature", "id": "k3j9x0ab12cd", "name": "Kanban", "packVersion": "1.2",
            "files": {"plugins/board.js": "export default () => {}\n", "custom.css": ".board { gap: 8px }\n"}});
        packs::import(st, &d, &old, false).await.unwrap();
        let p = crate::staging::pending(st, &d, false).await;
        assert_eq!((&p["plugin"]["target"], &p["plugin"]["name"], &p["problems"]), (&json!("new"), &json!("Kanban"), &json!([])));
        crate::staging::apply(st, &d, false, p["signature"].as_str()).await.unwrap();
        let f = bundles::load(&d.layer);
        let b = f.bundles.values().next().unwrap();
        assert!(bundles::valid_id(&b.id), "its 0.4.1 id is not a plugin id: a new one is made ({})", b.id);
        assert_eq!(b.claims, vec![Claim::Css { block: "pack:Kanban k3j9x0ab12cd".into() }, Claim::File { path: "plugins/board.js".into() }], "its look keeps the block 0.4.1 would have written");
        // The same pack again is that plugin (by name), not a second one.
        let mut newer = old.clone();
        newer["files"]["plugins/board.js"] = json!("export default () => { /* 1.3 */ }\n");
        newer["packVersion"] = json!("1.3");
        assert_eq!(packs::inspect(&newer, &d.layer, &[]).unwrap()["plugin"]["replaces"]["id"], b.id.as_str());
        std::fs::remove_dir_all(&d.home).unwrap();
    }
}
