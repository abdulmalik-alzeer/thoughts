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
    // Replaced whole, but not forced to the disk (no fsync on every run start and end, on the
    // runtime): the record is for a daemon that dies, and then the system still has the file. A
    // power cut that loses it ends the processes it lists as well.
    let r = serde_json::to_string(runs).map_err(std::io::Error::other)
        .and_then(|t| write_unsynced(file, t.as_bytes()));
    crate::log::ok_or_warn("could not save runs.json", r);
}

/// Write `file` whole and privately: a new file in its folder put in its place.
fn write_unsynced(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let name = file.file_name().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp = file.with_file_name(format!(".{}.{}.tmp", name.to_string_lossy(), uuid::Uuid::new_v4().simple()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let r = o.open(&tmp).and_then(|mut f| f.write_all(bytes)).and_then(|_| std::fs::rename(&tmp, file));
    if r.is_err() { let _ = std::fs::remove_file(&tmp); }
    r
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
    (c.contains("claude") && c.contains("--output-format"))
        // A Codex run: `codex exec --json …` (or the older `--experimental-json`).
        || (c.contains("codex") && c.contains(" exec ") && c.contains("json"))
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
    matches!(image.to_lowercase().as_str(), "claude.exe" | "codex.exe" | "node.exe" | "cmd.exe")
}

#[cfg(windows)]
fn windows_image(pid: u32) -> Option<String> {
    let o = crate::proc::std_command("tasklist").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]).output().ok()?;
    if !o.status.success() { return None; }
    image_from_tasklist(&String::from_utf8_lossy(&o.stdout))
}

/// How far a clock reading may be off (`ps` counts whole seconds; a record is written just after its process starts).
const SLACK_SECS: u64 = 5;

/// How long ago a recorded run was started, by the clock.
fn record_age(r: &RunRecord) -> u64 { (chrono::Utc::now() - r.started).num_seconds().max(0) as u64 }

/// `ps`'s elapsed-time column (`[[dd-]hh:]mm:ss`) in seconds.
pub fn parse_etime(s: &str) -> Option<u64> {
    let s = s.trim();
    let (days, clock) = match s.split_once('-') { Some((d, rest)) => (d.trim().parse::<u64>().ok()?, rest), None => (0, s) };
    let parts: Vec<u64> = clock.split(':').map(|p| p.trim().parse::<u64>().ok()).collect::<Option<_>>()?;
    let (h, m, sec) = match parts[..] { [m, s] => (0, m, s), [h, m, s] => (h, m, s), _ => return None };
    Some(days * 86_400 + h * 3600 + m * 60 + sec)
}

/// Is a live process that has been running for `process_age` seconds the one recorded `record_age`
/// seconds ago? A record is written right after its process starts, so that process is at least
/// as old as the record. A younger one only has the same number: the recorded process ended and
/// its PID was given to something else (after a restart of the computer, for example).
pub fn same_process(process_age: u64, record_age: u64) -> bool { process_age + SLACK_SECS >= record_age }

/// Could a member of a recorded run's process group have been started by that run? Only if it is
/// no older than the run itself.
pub fn started_by_run(process_age: u64, record_age: u64) -> bool { process_age <= record_age + SLACK_SECS }

/// Has the computer been restarted since the record was written? Then nothing recorded is still running.
pub fn restarted_since(uptime: Option<u64>, record_age: u64) -> bool { uptime.map_or(false, |up| record_age > up + SLACK_SECS) }

/// How long a live process has been running, in seconds; None when there is no such process.
#[cfg(unix)]
fn process_age(pid: u32) -> Option<u64> {
    let o = crate::proc::std_command("ps").args(["-p", &pid.to_string(), "-o", "etime="]).output().ok()?;
    if !o.status.success() { return None; }
    parse_etime(&String::from_utf8_lossy(&o.stdout))
}

/// The live members of process group `pgid`.
#[cfg(unix)]
fn group_members(pgid: u32) -> Vec<u32> {
    crate::proc::std_command("pgrep").args(["-g", &pgid.to_string()]).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()).unwrap_or_default()
}

#[cfg(unix)]
fn quiet_kill(args: &[&str]) -> bool {
    crate::proc::std_command("kill").args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}

/// How long a live Windows process has been running, in seconds (UNTESTED there).
#[cfg(windows)]
fn windows_process_age(pid: u32) -> Option<u64> {
    let script = format!("[int]((Get-Date) - (Get-Process -Id {pid}).StartTime).TotalSeconds");
    let o = crate::proc::std_command("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).output().ok()?;
    if !o.status.success() { return None; }
    String::from_utf8_lossy(&o.stdout).trim().parse::<i64>().ok().map(|s| s.max(0) as u64)
}

/// End recorded assistant processes that outlived the previous daemon, then clear the record.
/// Returns how many were ended.
///
/// A PID is only a number, and numbers are handed out again. A recorded run is ended only when
/// the live process with its number (a) looks like an assistant run and (b) has been running at
/// least as long as the record exists — a younger process got the number later and is somebody
/// else's. After a restart of the computer nothing is ended at all.
pub fn reap_stale(file: &Path) -> usize {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let runs = load(file);
    #[allow(unused_mut)]
    let mut ended = 0;
    #[cfg(unix)]
    {
        // The first process of the system has been running since the computer started.
        let uptime = if runs.is_empty() { None } else { process_age(1) };
        for r in &runs {
            if r.pid <= 1 || r.pid == std::process::id() { continue; }
            let age = record_age(r);
            if restarted_since(uptime, age) { continue; }
            let group = format!("-{}", r.pid);
            match command_line(r.pid) {
                Some(cmd) => {
                    if !looks_like_assistant_run(&cmd) { continue; }
                    if !process_age(r.pid).map_or(false, |p| same_process(p, age)) {
                        crate::log::info(format!("process {} is not the run recorded for chat {} (it started later); left alone", r.pid, r.chat_id));
                        continue;
                    }
                    // Runs are started as the leader of their own process group: end the whole group, so the
                    // assistant's own children go too; a run recorded by an older version is ended on its own.
                    let ok = quiet_kill(&["-TERM", "--", &group]) || quiet_kill(&["-TERM", &r.pid.to_string()]);
                    crate::log::warn(format!("ended assistant process {} left over from chat {} ({})", r.pid, r.chat_id, if ok { "ok" } else { "failed" }));
                    if ok { ended += 1; }
                }
                // The assistant itself is gone, but what it started (shells, tools) may live on in its
                // group. A group keeps its number for as long as it has members, so these are that
                // run's — unless every one of them is older than the run, which its children cannot be.
                None => {
                    let members = group_members(r.pid);
                    if members.is_empty() || !members.iter().all(|m| process_age(*m).map_or(true, |p| started_by_run(p, age))) { continue; }
                    let ok = quiet_kill(&["-TERM", "--", &group]);
                    crate::log::warn(format!("ended {} process(es) the assistant had started for chat {} ({})", members.len(), r.chat_id, if ok { "ok" } else { "failed" }));
                    if ok { ended += 1; }
                }
            }
        }
    }
    // Windows (UNTESTED there): the recorded PID must still belong to a program an assistant run
    // could be, and that process must be at least as old as the record; then the whole process
    // tree is ended. When its age cannot be read it is left alone.
    #[cfg(windows)]
    for r in &runs {
        if r.pid <= 4 || r.pid == std::process::id() { continue; }
        let Some(image) = windows_image(r.pid) else { continue };
        if !windows_image_is_assistant(&image) { continue; }
        if !windows_process_age(r.pid).map_or(false, |p| same_process(p, record_age(r))) { continue; }
        let ok = crate::proc::std_command("taskkill").args(["/PID", &r.pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
        crate::log::warn(format!("ended assistant process {} ({image}) left over from chat {} ({})", r.pid, r.chat_id, if ok { "ok" } else { "failed" }));
        if ok { ended += 1; }
    }
    if !runs.is_empty() || file.exists() { let _ = std::fs::remove_file(file); }
    ended
}

/// The daemon is stopping: end every assistant process it still has running, each with
/// everything it started (shells, tools, helper servers), and clear the record. These are this
/// daemon's own live children, so their numbers are certain. Returns how many there were.
pub fn end_all(file: &Path) -> usize {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let runs = load(file);
    for r in &runs {
        if r.pid <= 1 || r.pid == std::process::id() { continue; }
        crate::proc::kill_tree(r.pid);
    }
    if !runs.is_empty() || file.exists() { let _ = std::fs::remove_file(file); }
    runs.len()
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
        assert!(looks_like_assistant_run("/opt/homebrew/bin/codex exec --json --skip-git-repo-check --sandbox read-only -"));
        assert!(looks_like_assistant_run("node /usr/local/lib/node_modules/@openai/codex/bin/codex.js exec --experimental-json -"));
        for other in ["/usr/bin/vim notes.md", "claude", "/Applications/Claude.app/Contents/MacOS/Claude", "sleep 300", "",
                      "codex", "/opt/homebrew/bin/codex login", "vim codex-notes.json"] {
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

    #[test]
    fn a_process_younger_than_its_record_is_somebody_elses() {
        assert_eq!(parse_etime("00:05"), Some(5));
        assert_eq!(parse_etime("  12:34\n"), Some(754));
        assert_eq!(parse_etime("01:02:03"), Some(3723));
        assert_eq!(parse_etime("2-03:04:05"), Some(2 * 86_400 + 3 * 3600 + 4 * 60 + 5));
        for bad in ["", "abc", "1:2:3:4", "5", "1-xx:00"] { assert_eq!(parse_etime(bad), None, "{bad}"); }
        // Recorded an hour ago, running for an hour (or longer: the clock was set back): the same process.
        assert!(same_process(3600, 3600) && same_process(3598, 3600) && same_process(9000, 3600));
        // Recorded an hour ago, running for two minutes: the number was handed to something new.
        assert!(!same_process(120, 3600) && !same_process(3590, 3600));
        // What a run started is never older than the run.
        assert!(started_by_run(10, 3600) && started_by_run(3600, 3600) && !started_by_run(7200, 3600));
        // The computer has been up for ten minutes; a record from an hour ago is from before the restart.
        assert!(restarted_since(Some(600), 3600) && !restarted_since(Some(86_400), 3600) && !restarted_since(None, 3600));
    }

    /// Is this process (still) there?
    #[cfg(unix)]
    fn alive(pid: u32) -> bool { quiet_kill(&["-0", &pid.to_string()]) }

    #[cfg(unix)]
    fn wait_gone(pid: u32) -> bool {
        for _ in 0..100 { if !alive(pid) { return true; } std::thread::sleep(std::time::Duration::from_millis(30)); }
        false
    }

    /// A stand-in whose command line reads like an assistant run, leading its own process group.
    #[cfg(unix)]
    fn fake_run(script: &str) -> std::process::Child {
        use std::os::unix::process::CommandExt;
        std::process::Command::new("sh").args(["-c", script, "claude", "--output-format", "stream-json"]).process_group(0).spawn().unwrap()
    }

    #[cfg(unix)]
    fn write_record(f: &Path, pid: u32, started: chrono::DateTime<chrono::Utc>) {
        std::fs::write(f, serde_json::to_string(&vec![RunRecord { pid, chat_id: "c".into(), started }]).unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_reused_pid_is_spared_and_a_real_leftover_is_ended_with_what_it_started() {
        let d = std::env::temp_dir().join(format!("thoughts-reap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("runs.json");
        // Looks exactly like an assistant run, but started long after the record was written: PID reuse.
        let mut newer = fake_run("sleep 30");
        write_record(&f, newer.id(), chrono::Utc::now() - chrono::Duration::hours(2));
        assert_eq!(reap_stale(&f), 0);
        assert!(!f.exists() && newer.try_wait().unwrap().is_none(), "a process that only shares the number must not be ended");
        newer.kill().unwrap();
        let _ = newer.wait();
        // The recorded process itself, with a child of its own: both go.
        let mut run = fake_run("sleep 31 & wait");
        std::thread::sleep(std::time::Duration::from_millis(200));
        let kids = group_members(run.id());
        assert!(kids.len() >= 2, "{kids:?}");
        record_in(&f, run.id(), "c");
        assert_eq!(reap_stale(&f), 1);
        let _ = run.wait();
        for k in kids { assert!(wait_gone(k), "process {k} of the run's group is still there"); }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn what_a_dead_run_started_is_ended_too() {
        let d = std::env::temp_dir().join(format!("thoughts-reap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("runs.json");
        // The assistant is gone (it exits at once), the shell command it started lives on in its group.
        let mut run = fake_run("sleep 32 & exit 0");
        let pid = run.id();
        record_in(&f, pid, "c");
        let _ = run.wait();
        let left = group_members(pid);
        assert_eq!(left.len(), 1, "{left:?}");
        assert!(command_line(pid).is_none());
        assert_eq!(reap_stale(&f), 1);
        assert!(wait_gone(left[0]), "the orphaned child is still running");
        // The same when the daemon stops normally: every recorded run goes, children included.
        let mut run = fake_run("sleep 33 & wait");
        std::thread::sleep(std::time::Duration::from_millis(200));
        let all = group_members(run.id());
        record_in(&f, run.id(), "c");
        assert_eq!(end_all(&f), 1);
        assert!(!f.exists());
        let _ = run.wait();
        for p in all { assert!(wait_gone(p), "process {p} outlived the daemon"); }
        std::fs::remove_dir_all(d).unwrap();
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
