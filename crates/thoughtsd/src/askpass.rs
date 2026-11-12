//! GIT_ASKPASS mode: git runs this executable with a prompt such as
//! "Username for 'https://github.com': " and reads the answer from stdout.

pub fn main() -> i32 {
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let Some(host) = host_asked(&prompt) else { return 1 };
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

/// The host (with its port) a prompt asks about — only when git asks for an `https://` address.
/// A saved password is never handed out for anything else: an `http://` address (which a library's
/// own settings can name) would send it across the network unencrypted, and no other kind of
/// address the app sets up signs in with one.
pub(crate) fn host_asked(prompt: &str) -> Option<String> {
    let url = prompt.split('\'').nth(1)?;
    if !url.get(..8).map_or(false, |s| s.eq_ignore_ascii_case("https://")) { return None; }
    crate::git::host_of(url)
}

/// The saved entries that can answer for `host` (`host_of` of the address git asks about), best
/// first: the host with its port, then — for a password saved by a version that left the port
/// out — the bare host.
pub(crate) fn keys_for(host: &str) -> Vec<String> {
    let mut keys = vec![host.to_string()];
    if let Some((bare, port)) = host.rsplit_once(':') { if !bare.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) { keys.push(bare.to_string()); } }
    keys
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_password_is_looked_up_under_the_host_and_port_git_asks_for() {
        let host = |prompt: &str| super::host_asked(prompt).unwrap_or_default();
        assert_eq!(super::keys_for(&host("Password for 'https://sara@git.example.com:3000': ")), ["git.example.com:3000", "git.example.com"]);
        assert_eq!(super::keys_for(&host("Username for 'https://github.com': ")), ["github.com"]);
    }

    #[test]
    fn a_password_is_only_given_for_an_encrypted_address() {
        assert_eq!(super::host_asked("Password for 'HTTPS://sara@github.com': ").as_deref(), Some("github.com"));
        for prompt in ["Password for 'http://sara@github.com': ", "Username for 'http://github.com': ", "Password for 'ssh://git@github.com': ",
                       "Password for 'httpsx://github.com': ", "Password: ", "Enter passphrase for key '/Users/sara/.ssh/id_ed25519': "] {
            assert_eq!(super::host_asked(prompt), None, "{prompt}");
        }
    }
}
