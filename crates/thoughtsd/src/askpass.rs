//! GIT_ASKPASS mode: git runs this executable with a prompt such as
//! "Username for 'https://github.com': " and reads the answer from stdout.

pub fn main() -> i32 {
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let host = prompt.split('\'').nth(1).and_then(crate::git::host_of).unwrap_or_default();
    // The same store the daemon writes to: the system keyring, or the private file when a Linux
    // machine has no working keyring.
    let Some(raw) = crate::secrets::get("thoughts-git", &host) else { return 1 };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { return 1 };
    let field = if prompt.to_lowercase().starts_with("username") { "username" } else { "secret" };
    match v[field].as_str() {
        Some(s) => { println!("{s}"); 0 }
        None => 1,
    }
}
