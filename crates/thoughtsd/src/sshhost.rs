//! An SSH backup address (`ssh://…`, `git@host:path`) to a server this computer has never
//! connected to. git runs without a terminal, so ssh cannot ask "are you sure you want to
//! continue connecting?" and the sync fails. Here the app asks the server for its keys, shows the
//! user their fingerprints to compare with the ones the server's owner publishes, and — only when
//! the user says so — records the one they saw in the user's own list of known servers, exactly as
//! answering "yes" to ssh would.

use base64::Engine;
use sha2::Digest;
use std::path::PathBuf;

/// The server and port an SSH address connects to: `ssh://[user@]host[:port]/path` or
/// `[user@]host:path`. `None` for anything else.
pub fn target(url: &str) -> Option<(String, u16)> {
    let (host, port) = if let Some(rest) = url.strip_prefix("ssh://") {
        let authority = rest.split('/').next()?;
        let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().ok()?),
            None => (hostport, 22),
        }
    } else {
        if url.contains("://") { return None; }
        let (before, _) = url.split_once(':')?;
        if before.contains('/') { return None; }
        (before.rsplit_once('@').map_or(before, |(_, h)| h), 22)
    };
    let ok = !host.is_empty() && host.len() <= 253 && !host.starts_with(['-', '.'])
        && host.bytes().all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b));
    ok.then(|| (host.to_string(), port))
}

/// The name a server is listed under in known_hosts: `host`, or `[host]:port` off the usual port.
fn listed_as(host: &str, port: u16) -> String { if port == 22 { host.to_string() } else { format!("[{host}]:{port}") } }

/// One key a server presented: its type, its key text, and its fingerprint as ssh shows it (`SHA256:…`).
#[derive(Debug, Clone, PartialEq)]
pub struct HostKey { pub kind: String, pub key: String, pub fingerprint: String }

/// The fingerprint ssh shows for a key given in base64: `SHA256:` and the unpadded base64 of its digest.
pub fn fingerprint(key_b64: &str) -> Option<String> {
    let blob = base64::engine::general_purpose::STANDARD.decode(key_b64.trim()).ok()?;
    Some(format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(sha2::Sha256::digest(&blob))))
}

/// The keys in the answer of `ssh-keyscan` (lines `host type key`; comments and anything else skipped).
pub fn parse_scan(out: &str) -> Vec<HostKey> {
    let mut keys: Vec<HostKey> = out.lines().filter(|l| !l.trim_start().starts_with('#')).filter_map(|l| {
        let mut f = l.split_whitespace();
        let (_host, kind, key) = (f.next()?, f.next()?, f.next()?);
        let known = kind.starts_with("ssh-") || kind.starts_with("ecdsa-") || kind.starts_with("sk-");
        if !known { return None; }
        Some(HostKey { kind: kind.to_string(), key: key.to_string(), fingerprint: fingerprint(key)? })
    }).collect();
    keys.dedup_by(|a, b| a.key == b.key);
    keys
}

/// Ask the server for its keys. Fails plainly when the tool for it is missing or the server does
/// not answer.
pub async fn scan(host: &str, port: u16) -> anyhow::Result<Vec<HostKey>> {
    let Some(bin) = crate::claude::find_binary(&None, "ssh-keyscan") else { return Err(crate::tr::err("hostkey.no_tool", &[])) };
    let mut c = crate::proc::command(bin);
    c.args(["-T", "10", "-p", &port.to_string(), "--", host]).env("PATH", crate::claude::shell_path()).stdin(std::process::Stdio::null());
    let out = crate::proc::output_timeout(&mut c, std::time::Duration::from_secs(30)).await.map_err(|_| crate::tr::err("hostkey.unreachable", &[]))?;
    let keys = parse_scan(&String::from_utf8_lossy(&out.stdout));
    if keys.is_empty() { return Err(crate::tr::err("hostkey.unreachable", &[])); }
    Ok(keys)
}

/// The user's list of servers ssh knows. Under test, a file in the test's own folder.
pub fn known_hosts_file() -> PathBuf {
    if cfg!(test) { return crate::paths::home().join("ssh").join("known_hosts"); }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".ssh").join("known_hosts")
}

/// Record `key` for the server in `file`, the way ssh does when told "yes". Nothing changes when
/// that very key is already listed.
pub fn trust_in(file: &std::path::Path, host: &str, port: u16, key: &HostKey) -> anyhow::Result<()> {
    let line = format!("{} {} {}", listed_as(host, port), key.kind, key.key);
    let existing = std::fs::read_to_string(file).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == line) { return Ok(()); }
    if let Some(dir) = file.parent() {
        if !dir.exists() {
            std::fs::create_dir_all(dir)?;
            #[cfg(unix)]
            { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?; }
        }
    }
    // Appended in place, keeping the file's owner and permissions as ssh left them.
    let mut chunk = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') { chunk.push('\n'); }
    chunk.push_str(&line);
    chunk.push('\n');
    use std::io::Write;
    std::fs::OpenOptions::new().create(true).append(true).open(file)?.write_all(chunk.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_of_an_ssh_address() {
        assert_eq!(target("git@github.com:sara/notes.git"), Some(("github.com".into(), 22)));
        assert_eq!(target("ssh://git@git.example.com:2222/sara/notes.git"), Some(("git.example.com".into(), 2222)));
        assert_eq!(target("ssh://example.com/notes.git"), Some(("example.com".into(), 22)));
        for bad in ["https://github.com/a/b.git", "/Volumes/Backup/notes.git", "git@-oProxyCommand=x:y", "ssh://host:99999/x", "a/b:c", "git@:x"] {
            assert_eq!(target(bad), None, "{bad}");
        }
        assert_eq!(listed_as("github.com", 22), "github.com");
        assert_eq!(listed_as("git.example.com", 2222), "[git.example.com]:2222");
    }

    #[test]
    fn fingerprints_are_the_ones_ssh_shows() {
        // GitHub's published ed25519 key and the fingerprint GitHub publishes for it.
        let key = "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
        assert_eq!(fingerprint(key).as_deref(), Some("SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU"));
        let scan = format!("# github.com:22 SSH-2.0-babeld\ngithub.com ssh-ed25519 {key}\ngithub.com ssh-ed25519 {key}\nnot a key line\n");
        let keys = parse_scan(&scan);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].kind, "ssh-ed25519");
        assert_eq!(fingerprint("not base64!"), None);
    }

    #[test]
    fn trusting_a_server_adds_its_key_once_and_keeps_the_rest() {
        let d = std::env::temp_dir().join(format!("thoughts-ssh-{}", uuid::Uuid::new_v4()));
        let f = d.join(".ssh").join("known_hosts");
        let key = HostKey { kind: "ssh-ed25519".into(), key: "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl".into(), fingerprint: String::new() };
        trust_in(&f, "git.example.com", 2222, &key).unwrap();
        trust_in(&f, "git.example.com", 2222, &key).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), format!("[git.example.com]:2222 ssh-ed25519 {}\n", key.key));
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(f.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700); }
        std::fs::write(&f, "other.example.com ssh-rsa AAAA").unwrap();
        trust_in(&f, "github.com", 22, &key).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), format!("other.example.com ssh-rsa AAAA\ngithub.com ssh-ed25519 {}\n", key.key));
        std::fs::remove_dir_all(d).unwrap();
    }
}
