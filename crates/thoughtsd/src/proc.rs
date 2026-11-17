//! The one place child processes are created, so platform quirks live here:
//! on Windows every child gets CREATE_NO_WINDOW, otherwise each git/claude call would flash a console.

use std::ffi::OsStr;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// An async command for `program`.
pub fn command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut c = tokio::process::Command::new(program);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

/// A blocking command for `program`.
pub fn std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Put a child in its own process group (unix), so that it and everything it starts can be ended
/// together with `kill_tree`. On Windows `kill_tree` walks the process tree instead.
pub fn own_group(c: &mut tokio::process::Command) {
    #[cfg(unix)]
    c.process_group(0);
    #[cfg(not(unix))]
    let _ = c;
}

/// End process `pid` and its children: the whole process group on unix (the child must have been
/// started with `own_group`), `taskkill /T` on Windows. Best effort; never blocks for long.
pub fn kill_tree(pid: u32) {
    if pid <= 1 { return; }
    #[cfg(unix)]
    {
        let group = format!("-{pid}");
        let ok = std_command("kill").args(["-KILL", "--", &group]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .status().map(|s| s.success()).unwrap_or(false);
        // Not a group leader (or already gone): fall back to the process itself.
        if !ok { let _ = std_command("kill").args(["-KILL", &pid.to_string()]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status(); }
    }
    #[cfg(windows)]
    {
        let _ = std_command("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    }
}

/// `kill_tree` for async code: the same, without holding a runtime thread while `kill` runs.
pub async fn end_tree(pid: u32) {
    if pid <= 1 { return; }
    let quiet = |mut c: tokio::process::Command| { c.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()); c };
    #[cfg(unix)]
    {
        let mut c = command("kill");
        c.args(["-KILL", "--", &format!("-{pid}")]);
        let ok = quiet(c).status().await.map(|s| s.success()).unwrap_or(false);
        // Not a group leader (or already gone): fall back to the process itself.
        if !ok { let mut c = command("kill"); c.args(["-KILL", &pid.to_string()]); let _ = quiet(c).status().await; }
    }
    #[cfg(windows)]
    {
        let mut c = command("taskkill");
        c.args(["/PID", &pid.to_string(), "/T", "/F"]);
        let _ = quiet(c).status().await;
    }
}

/// Wait for `child` to exit and collect what it printed (its stdout and stderr piped), writing
/// `input` to its stdin first when there is one — but no longer than `limit`. The child stays the
/// caller's throughout: on timeout everything it started is ended while its number still belongs
/// to it (a child is not reaped before it is waited for, so the number can not have gone to
/// another process), then it is killed and waited for, and `TimedOut` is returned.
pub async fn output_within(child: &mut tokio::process::Child, input: Option<&[u8]>, limit: std::time::Duration) -> std::io::Result<std::process::Output> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (stdin, out, err) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let run = async {
        let feed = async {
            if let Some(mut s) = stdin { if let Some(data) = input { s.write_all(data).await?; } }
            Ok::<_, std::io::Error>(())
        };
        let read_out = async { let mut v = vec![]; if let Some(mut o) = out { o.read_to_end(&mut v).await?; } Ok::<_, std::io::Error>(v) };
        let read_err = async { let mut v = vec![]; if let Some(mut e) = err { e.read_to_end(&mut v).await?; } Ok::<_, std::io::Error>(v) };
        let ((), stdout, stderr) = tokio::try_join!(feed, read_out, read_err)?;
        Ok::<_, std::io::Error>(std::process::Output { status: child.wait().await?, stdout, stderr })
    };
    let done = tokio::time::timeout(limit, run).await;
    match done {
        Ok(r) => r,
        Err(_) => {
            if let Some(pid) = child.id() { end_tree(pid).await; }
            let _ = child.kill().await;
            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, TimedOut(limit)))
        }
    }
}

/// Did `e` come from a time limit of this module's (`output_timeout`, `output_within`)?
pub fn timed_out(e: &std::io::Error) -> bool { e.get_ref().map_or(false, |i| i.is::<TimedOut>()) }

/// The command did not finish within its time limit and was ended.
#[derive(Debug)]
pub struct TimedOut(pub std::time::Duration);
impl std::fmt::Display for TimedOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "timed out after {} seconds", self.0.as_secs()) }
}
impl std::error::Error for TimedOut {}

/// Run `c` to completion, but never longer than `limit`: on timeout the child and everything it
/// started are killed and `TimedOut` is returned (as an `io::Error` of kind `TimedOut`).
pub async fn output_timeout(c: &mut tokio::process::Command, limit: std::time::Duration) -> std::io::Result<std::process::Output> {
    own_group(c);
    c.kill_on_drop(true).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = c.spawn()?;
    output_within(&mut child, None, limit).await
}

/// Blocking twin of `output_timeout`, for the few places that run outside the async runtime.
pub fn std_output_timeout(c: &mut std::process::Command, limit: std::time::Duration) -> std::io::Result<std::process::Output> {
    #[cfg(unix)]
    { use std::os::unix::process::CommandExt; c.process_group(0); }
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let child = c.spawn()?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || { let _ = tx.send(child.wait_with_output()); });
    match rx.recv_timeout(limit) {
        Ok(r) => r,
        Err(_) => {
            kill_tree(pid);
            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, TimedOut(limit)))
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_stalled_command_is_ended_with_its_children() {
        let d = std::env::temp_dir().join(format!("thoughts-proc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let marker = d.join("child-survived");
        // A shell that starts a grandchild which would leave a marker behind if it outlived the timeout.
        let script = format!("(sleep 2; touch '{}') & sleep 30", marker.display());
        let started = std::time::Instant::now();
        let mut c = command("sh");
        c.args(["-c", &script]).stdin(std::process::Stdio::null());
        let err = output_timeout(&mut c, std::time::Duration::from_millis(300)).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
        assert!(!marker.exists(), "the stalled command's child kept running");
        // A command that finishes in time is returned as usual.
        let mut c = command("sh");
        c.args(["-c", "printf ok"]);
        assert_eq!(output_timeout(&mut c, std::time::Duration::from_secs(10)).await.unwrap().stdout, b"ok");
        let mut c = std_command("sh");
        c.args(["-c", "sleep 30"]);
        assert_eq!(std_output_timeout(&mut c, std::time::Duration::from_millis(200)).unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        let mut c = std_command("sh");
        c.args(["-c", "printf ok"]);
        assert_eq!(std_output_timeout(&mut c, std::time::Duration::from_secs(10)).unwrap().stdout, b"ok");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[tokio::test]
    async fn a_child_kept_by_the_caller_is_fed_read_and_on_timeout_ended_before_it_is_let_go() {
        let piped = |c: &mut tokio::process::Command| { own_group(c); c.kill_on_drop(true).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()); };
        // What is given goes to its stdin; what it prints on both streams comes back.
        let mut c = command("sh");
        c.args(["-c", "cat; printf err >&2"]);
        piped(&mut c);
        let mut child = c.spawn().unwrap();
        let out = output_within(&mut child, Some(b"hello"), std::time::Duration::from_secs(10)).await.unwrap();
        assert_eq!((out.stdout.as_slice(), out.stderr.as_slice(), out.status.success()), (&b"hello"[..], &b"err"[..], true));
        // Stalled: ended, and by the time it is handed back it has been waited for (its number
        // was still its own when its group was ended).
        let mut c = command("sh");
        c.args(["-c", "sleep 30"]);
        piped(&mut c);
        let mut child = c.spawn().unwrap();
        let err = output_within(&mut child, Some(b""), std::time::Duration::from_millis(200)).await.unwrap_err();
        assert!(timed_out(&err) && err.kind() == std::io::ErrorKind::TimedOut);
        assert!(child.try_wait().unwrap().is_some() && child.id().is_none());
        assert!(!timed_out(&std::io::Error::new(std::io::ErrorKind::TimedOut, "other")));
    }
}
