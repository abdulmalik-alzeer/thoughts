//! A check on every shell command a Claude Code run is about to run, in every permission mode
//! (bypass included): Claude Code runs this same executable as a `PreToolUse` hook, and a command
//! that names the app's own data folder or the attachment copies is refused (exit code 2).
//!
//! It reads the command text, so it is a second lock, not a wall: a command can still reach a
//! folder without naming it (a pattern like `~/.th*`, a variable, a script). Permission deny rules
//! work the same way for the shell. What it does stop is the plain ways in, in any letter case.

use std::path::Path;

/// The first argument that starts this mode: `<exe> --thoughts-shell-guard <home>`.
pub const FLAG: &str = "--thoughts-shell-guard";

/// The hook entry for a run's `--settings` document.
pub fn hooks(exe: &Path, home: &Path) -> serde_json::Value {
    let command = format!("{} {FLAG} {}", quote(&exe.to_string_lossy()), quote(&home.to_string_lossy()));
    serde_json::json!({"PreToolUse": [{"matcher": "Bash|PowerShell", "hooks": [{"type": "command", "command": command}]}]})
}

/// One argument for the shell that runs the hook (`sh` everywhere Claude Code runs hooks).
fn quote(s: &str) -> String { format!("'{}'", s.replace('\'', r"'\''")) }

/// Add the hook to a run's `--settings` document.
pub fn with_hooks(doc: Option<String>, exe: &Path, home: &Path) -> Option<String> {
    let mut v = doc.as_deref().and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()).unwrap_or_else(|| serde_json::json!({"permissions": {}}));
    v["hooks"] = hooks(exe, home);
    Some(v.to_string())
}

/// Does `command`, run from `cwd`, name the app's data folder `home` (outside the run's own working
/// folder, when that is inside it), or any attachment copies? Letter case does not count.
pub fn blocked(command: &str, cwd: &str, home: &Path) -> bool {
    let norm = |s: &str| s.replace('\\', "/").to_lowercase();
    let mut cmd = norm(command);
    let home_s = norm(&home.to_string_lossy());
    let home_s = home_s.trim_end_matches('/');
    let cwd_s = norm(cwd);
    let cwd_s = cwd_s.trim_end_matches('/');
    // A run that works inside the app's folder (Reorganize's copy, a customisation's staging copy) may name its own folder.
    if !home_s.is_empty() && cwd_s.len() > home_s.len() && cwd_s.starts_with(&format!("{home_s}/")) {
        if cmd.contains(&format!("{cwd_s}/..")) { return true; }
        cmd = cmd.replace(cwd_s, "");
    }
    if cmd.contains(crate::run_files::DIR) { return true; }
    if !home_s.is_empty() && cmd.contains(home_s) { return true; }
    // The folder by its name alone (`~/.thoughts`, `$HOME/.thoughts`), when it is a dot folder.
    match home.file_name().map(|n| n.to_string_lossy().to_lowercase()) {
        Some(name) if name.starts_with('.') && name.len() > 1 => cmd.contains(&format!("/{name}")) || cmd.split(|c: char| c.is_whitespace() || "'\"=;&|()<>".contains(c)).any(|w| w == name || w.starts_with(&format!("{name}/"))),
        _ => false,
    }
}

/// The hook itself: reads what Claude Code sends on stdin and answers with an exit code (0 lets the
/// command run, 2 refuses it and tells the assistant why). What cannot be read is refused.
pub fn main() -> i32 {
    let home = std::env::args().nth(2).unwrap_or_default();
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    let v: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
    let (Some(cmd), false) = (v["tool_input"]["command"].as_str(), home.is_empty()) else {
        eprintln!("The Thoughts app could not check this command, so it was not run.");
        return 2;
    };
    if blocked(cmd, v["cwd"].as_str().unwrap_or_default(), Path::new(&home)) {
        eprintln!("Refused by the Thoughts app: commands may not reach the app's own data folder or attached files. Open attached files with the Read tool instead.");
        return 2;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_naming_the_apps_folder_are_refused_in_any_letter_case() {
        let home = Path::new("/Users/u/.thoughts");
        for cmd in ["cat /Users/u/.thoughts/attachments/c-1/x.png", "cat /USERS/U/.THOUGHTS/ATTACHMENTS/c/x", "ls ~/.thoughts", "ls ~/.Thoughts/chats",
                    "cd $HOME/.thoughts && cat secrets.json", "cd .thoughts", "cat x/Chat-Files/1/2/a", "grep -r key /users/u/.thoughts"] {
            assert!(blocked(cmd, "/Users/u/Library", home), "{cmd}");
        }
        for cmd in ["ls", "cat notes/attachments.md", "grep -r thoughts .", "git status", "cat /Users/u/Library/me/thoughts.md"] {
            assert!(!blocked(cmd, "/Users/u/Library", home), "{cmd}");
        }
        // A run working inside the app's folder may name that folder, and nothing else of it.
        let wt = "/Users/u/.thoughts/reorganize-worktree";
        assert!(!blocked("ls /Users/u/.thoughts/reorganize-worktree/notes", wt, home));
        assert!(blocked("cat /Users/u/.thoughts/reorganize-worktree/../secrets.json", wt, home));
        assert!(blocked("cat /Users/u/.thoughts/secrets.json", wt, home));
        // The hook entry: this executable, in this mode, for both shells.
        let h = hooks(Path::new("/Apps/Thoughts's.app/x"), home);
        assert_eq!(h["PreToolUse"][0]["matcher"], "Bash|PowerShell");
        assert_eq!(h["PreToolUse"][0]["hooks"][0]["command"], format!("'/Apps/Thoughts'\\''s.app/x' {FLAG} '/Users/u/.thoughts'"));
        let doc = with_hooks(Some(r#"{"permissions":{"deny":["Bash(x)"]}}"#.into()), Path::new("/a"), home).unwrap();
        let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
        assert_eq!((v["permissions"]["deny"][0].as_str(), v["hooks"]["PreToolUse"].is_array()), (Some("Bash(x)"), true));
    }
}
