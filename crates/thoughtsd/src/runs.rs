//! A record of the assistant processes this daemon started (`~/.thoughts/runs.json`). If the daemon
//! dies without cleaning up, the next one ends whatever is still running from that list.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord { pub pid: u32, pub chat_id: String, pub started: chrono::DateTime<chrono::Utc> }

static LOCK: Mutex<()> = Mutex::new(());

pub fn load(file: &Path) -> Vec<RunRecord> {
    std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(file: &Path, runs: &[RunRecord]) {
    let r = serde_json::to_string(runs).map_err(std::io::Error::other)
        .and_then(|t| crate::paths::write_atomic(file, t.as_bytes(), true));
    crate::log::ok_or_warn("could not save runs.json", r);
}

pub fn record_in(file: &Path, pid: u32, chat_id: &str) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut runs = load(file);
    runs.retain(|r| r.pid != pid);
    runs.push(RunRecord { pid, chat_id: chat_id.into(), started: chrono::Utc::now() });
    save(file, &runs);
}

pub fn forget_in(file: &Path, pid: u32) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut runs = load(file);
    let before = runs.len();
    runs.retain(|r| r.pid != pid);
    if runs.len() != before { save(file, &runs); }
}

pub fn record(pid: Option<u32>, chat_id: &str) { if let Some(pid) = pid { record_in(&crate::paths::runs_file(), pid, chat_id) } }
pub fn forget(pid: Option<u32>) { if let Some(pid) = pid { forget_in(&crate::paths::runs_file(), pid) } }

/// Is this command line one of the assistant runs the daemon starts? (PIDs get reused: never end a
/// process on its number alone.)
pub fn looks_like_assistant_run(cmdline: &str) -> bool {
    let c = cmdline.to_lowercase();
    c.contains("claude") && c.contains("--output-format")
}

/// The command line of a live process, or None when there is no such process.
#[cfg(unix)]
fn command_line(pid: u32) -> Option<String> {
    let o = crate::proc::std_command("ps").args(["-p", &pid.to_string(), "-o", "command="]).output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (o.status.success() && !s.is_empty()).then_some(s)
}

/// The program name of a live process on Windows (`claude.exe`, `node.exe`…), from
/// `tasklist /FI "PID eq N" /FO CSV /NH`, or None when there is no such process.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn image_from_tasklist(csv: &str) -> Option<String> {
    let line = csv.lines().map(str::trim).find(|l| l.starts_with('"'))?;
    let name = line.trim_start_matches('"').split('"').next()?.trim().to_lowercase();
    (!name.is_empty()).then_some(name)
}

/// Could a Windows process with this program name be one of the assistant runs the daemon starts?
/// (The native program, or the npm install: `cmd.exe` running `claude.cmd`, which runs `node.exe`.)
#[cfg_attr(not(windows), allow(dead_code))]
pub fn windows_image_is_assistant(image: &str) -> bool {
    matches!(image.to_lowercase().as_str(), "claude.exe" | "node.exe" | "cmd.exe")
}

#[cfg(windows)]
fn windows_image(pid: u32) -> Option<String> {
    let o = crate::proc::std_command("tasklist").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]).output().ok()?;
    if !o.status.success() { return None; }
    image_from_tasklist(&String::from_utf8_lossy(&o.stdout))
}

/// End recorded assistant processes that outlived the previous daemon, then clear the record.
/// Returns how many were ended.
pub fn reap_stale(file: &Path) -> usize {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let runs = load(file);
    #[allow(unused_mut)]
    let mut ended = 0;
    #[cfg(unix)]
    for r in &runs {
        if r.pid <= 1 || r.pid == std::process::id() { continue; }
        let Some(cmd) = command_line(r.pid) else { continue };
        if !looks_like_assistant_run(&cmd) { continue; }
        // Runs are started as the leader of their own process group: end the whole group, so the
        // assistant's own children go too; a run recorded by an older version is ended on its own.
        let quiet = |args: &[&str]| crate::proc::std_command("kill").args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .status().map(|s| s.success()).unwrap_or(false);
        let ok = quiet(&["-TERM", "--", &format!("-{}", r.pid)]) || quiet(&["-TERM", &r.pid.to_string()]);
        crate::log::warn(format!("ended assistant process {} left over from chat {} ({})", r.pid, r.chat_id, if ok { "ok" } else { "failed" }));
        if ok { ended += 1; }
    }
    // Windows (UNTESTED there): the recorded PID must still belong to a program an assistant run
    // could be, and must have been started after it was recorded… which `tasklist` cannot tell, so
    // only the program name is checked; then the whole process tree is ended.
    #[cfg(windows)]
    for r in &runs {
        if r.pid <= 4 || r.pid == std::process::id() { continue; }
        let Some(image) = windows_image(r.pid) else { continue };
        if !windows_image_is_assistant(&image) { continue; }
        let ok = crate::proc::std_command("taskkill").args(["/PID", &r.pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
        crate::log::warn(format!("ended assistant process {} ({image}) left over from chat {} ({})", r.pid, r.chat_id, if ok { "ok" } else { "failed" }));
        if ok { ended += 1; }
    }
    if !runs.is_empty() || file.exists() { let _ = std::fs::remove_file(file); }
    ended
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_forgets() {
        let d = std::env::temp_dir().join(format!("thoughts-runs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("runs.json");
        record_in(&f, 111, "a");
        record_in(&f, 222, "b");
        record_in(&f, 111, "c");
        assert_eq!(load(&f).iter().map(|r| (r.pid, r.chat_id.as_str())).collect::<Vec<_>>(), [(222, "b"), (111, "c")]);
        forget_in(&f, 222);
        assert_eq!(load(&f).len(), 1);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn only_assistant_processes_are_matched() {
        assert!(looks_like_assistant_run("/Users/a/.local/bin/claude -p --output-format stream-json --verbose"));
        assert!(looks_like_assistant_run("node /opt/homebrew/lib/node_modules/@anthropic-ai/claude-code/cli.js -p --output-format json"));
        for other in ["/usr/bin/vim notes.md", "claude", "/Applications/Claude.app/Contents/MacOS/Claude", "sleep 300", ""] {
            assert!(!looks_like_assistant_run(other), "{other}");
        }
    }

    #[test]
    fn windows_process_names_are_read_from_tasklist() {
        assert_eq!(image_from_tasklist("\"claude.exe\",\"4242\",\"Console\",\"1\",\"98,112 K\"\r\n").as_deref(), Some("claude.exe"));
        assert_eq!(image_from_tasklist("\r\n\"Node.EXE\",\"7\",\"Console\",\"1\",\"1 K\"").as_deref(), Some("node.exe"));
        assert_eq!(image_from_tasklist("INFO: No tasks are running which match the specified criteria.\r\n"), None);
        assert_eq!(image_from_tasklist(""), None);
        for yes in ["claude.exe", "node.exe", "CMD.EXE"] { assert!(windows_image_is_assistant(yes), "{yes}"); }
        for no in ["explorer.exe", "winword.exe", "git.exe", ""] { assert!(!windows_image_is_assistant(no), "{no}"); }
    }

    #[cfg(unix)]
    #[test]
    fn reaping_spares_unrelated_processes_and_clears_the_file() {
        let d = std::env::temp_dir().join(format!("thoughts-reap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("runs.json");
        // A live process that is not an assistant run, and a PID that does not exist.
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        record_in(&f, child.id(), "a");
        record_in(&f, 4_000_000, "b");
        assert_eq!(reap_stale(&f), 0);
        assert!(!f.exists());
        assert!(child.try_wait().unwrap().is_none(), "an unrelated process must not be ended");
        child.kill().unwrap();
        let _ = child.wait();
        std::fs::remove_dir_all(d).unwrap();
    }
}
