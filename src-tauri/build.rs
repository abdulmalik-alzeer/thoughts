// Which window may call which of the app's own commands.
//
// The rule: every command in `generate_handler!` (src/main.rs) is named once in `COMMANDS` below,
// and granted by name (`allow-<command-with-dashes>`) only to the windows that call it, in
// `capabilities/default.json` (the app window, `main` / `main-*`), `capabilities/window.json` (a
// second app window, `view-*`, see src/windows.rs) or `capabilities/capture.json` (the quick
// capture box). A window, or a frame inside one, that has no grant cannot call the
// command at all; add-on frames get none. Commands that matter also check the caller's window label
// themselves (`is_main_label` in main.rs), as a second lock.
//
// Adding a command: its line in `generate_handler!`, one line here, and its grant. The build stops,
// naming the command, when `generate_handler!` and this list disagree or when a command is granted
// to no window.
const COMMANDS: &[&str] = &[
    "open_library_file",
    "reveal_library_file",
    "open_chat_attachment",
    "reveal_chat_attachment",
    "save_pack",
    "save_file",
    "save_pdf",
    "open_saved_pdf",
    "daemon_info",
    "restart_daemon",
    "approve_action",
    "set_dock_icon",
    "set_attention",
    "update_check",
    "update_install",
    "updates_gh_state",
    "refresh_menu",
    "quit_app",
    "set_safe_mode",
    "set_unsaved",
    "open_window",
    "focus_main",
    "take_deep_link",
    "open_capture",
    "capture_ready",
    "capture_take",
    "capture_show",
    "capture_hide",
    "capture_open_main",
    "capture_shortcut",
    "set_capture_shortcut",
    "pause_capture_shortcut",
    "voice_status",
    "voice_permit",
    "voice_dictate_start",
    "voice_dictate_stop",
    "voice_dictate_cancel",
    "voice_record_start",
    "voice_record_pause",
    "voice_record_stop",
    "voice_record_discard",
    "voice_transcribe",
    "voice_transcribe_cancel",
    "voice_result",
    "voice_result_clear",
];

/// The command names in `generate_handler![…]` of `main_rs`: the last part of each path.
fn handler_commands(main_rs: &str) -> Vec<String> {
    let start = main_rs.find("generate_handler![").expect("src/main.rs has no generate_handler![") + "generate_handler![".len();
    let end = start + main_rs[start..].find(']').expect("generate_handler![ is not closed");
    main_rs[start..end]
        .split(',')
        .map(|entry| entry.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<String>())
        .map(|entry| entry.rsplit("::").next().unwrap_or("").trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// Stop the build, naming each command, when the lists disagree or a command is granted nowhere.
fn check_commands() {
    let main_rs = std::fs::read_to_string("src/main.rs").expect("cannot read src/main.rs");
    let handled = handler_commands(&main_rs);
    let mut problems = Vec::new();
    for name in &handled {
        if !COMMANDS.contains(&name.as_str()) {
            problems.push(format!("`{name}` is in generate_handler! but not in COMMANDS in src-tauri/build.rs"));
        }
    }
    for name in COMMANDS {
        if !handled.iter().any(|h| h == name) {
            problems.push(format!("`{name}` is in COMMANDS in src-tauri/build.rs but not in generate_handler!"));
        }
    }
    let mut grants = String::new();
    for entry in std::fs::read_dir("capabilities").expect("cannot read capabilities/").flatten() {
        if entry.path().extension().is_some_and(|e| e == "json") {
            grants.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
        }
    }
    for name in COMMANDS {
        let grant = format!("\"allow-{}\"", name.replace('_', "-"));
        if !grants.contains(&grant) {
            problems.push(format!("`{name}` is granted to no window: add {grant} to capabilities/default.json, window.json or capture.json"));
        }
    }
    if !problems.is_empty() {
        panic!("\n\nThe app's commands and their grants disagree:\n  - {}\n\n", problems.join("\n  - "));
    }
}

fn main() {
    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=capabilities");
    check_commands();
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)))
        .expect("failed to run tauri-build");
}
