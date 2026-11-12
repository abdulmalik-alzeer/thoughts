//! GIT_ASKPASS mode: git runs this executable with a prompt such as
//! "Username for 'https://github.com': " and reads the answer from stdout.

pub fn main() -> i32 {
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let host = prompt.split('\'').nth(1).and_then(crate::git::host_of).unwrap_or_default();
    // The same store the daemon writes to: the system keyring, or the private file when a Linux
    // machine has no working keyring.
    let Some(raw) = keys_for(&host).iter().find_map(|k| crate::secrets::get("thoughts-git", k)) else { return 1 };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { return 1 };
    let field = if prompt.to_lowercase().starts_with("username") { "username" } else { "secret" };
    match v[field].as_str() {
        Some(s) => { println!("{s}"); 0 }
        None => 1,
    }
}

/// The saved entries that can answer for `host` (`host_of` of the address git asks about), best
/// first: the host with its port, then — for a password saved by a version that left the port
/// out — the bare host.
fn keys_for(host: &str) -> Vec<String> {
    let mut keys = vec![host.to_string()];
    if let Some((bare, port)) = host.rsplit_once(':') { if !bare.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) { keys.push(bare.to_string()); } }
    keys
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_password_is_looked_up_under_the_host_and_port_git_asks_for() {
        let host = |prompt: &str| prompt.split('\'').nth(1).and_then(crate::git::host_of).unwrap_or_default();
        assert_eq!(super::keys_for(&host("Password for 'https://sara@git.example.com:3000': ")), ["git.example.com:3000", "git.example.com"]);
        assert_eq!(super::keys_for(&host("Username for 'https://github.com': ")), ["github.com"]);
    }
}
