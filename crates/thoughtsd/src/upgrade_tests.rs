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
