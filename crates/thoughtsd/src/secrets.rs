//! Secrets (the provider token, git passwords) live in the OS keyring. On Linux machines without a
//! working keyring service they fall back to a 0600 file under `~/.thoughts`, logged once.
//! macOS and Windows behave exactly as the keyring does.

use std::collections::BTreeMap;
use std::path::Path;

fn file_load(file: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn key(service: &str, account: &str) -> String { format!("{service}/{account}") }

pub fn file_set(file: &Path, service: &str, account: &str, value: &str) -> anyhow::Result<()> {
    let mut m = file_load(file);
    m.insert(key(service, account), value.to_string());
    crate::paths::write_atomic(file, serde_json::to_string_pretty(&m)?.as_bytes(), true)?;
    Ok(())
}

pub fn file_get(file: &Path, service: &str, account: &str) -> Option<String> {
    file_load(file).remove(&key(service, account))
}

pub fn file_delete(file: &Path, service: &str, account: &str) -> anyhow::Result<()> {
    let mut m = file_load(file);
    if m.remove(&key(service, account)).is_some() {
        crate::paths::write_atomic(file, serde_json::to_string_pretty(&m)?.as_bytes(), true)?;
    }
    Ok(())
}

#[cfg(all(target_os = "linux", not(test)))]
fn note_fallback(e: &dyn std::fmt::Display) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| crate::log::warn(format!("system keyring unavailable ({e}); storing secrets in a private file under ~/.thoughts instead")));
}

/// Under test the system keyring is never reached: the real one is shared with the user's
/// installed app. Every test in the process shares this one store in memory instead (tests use
/// names of their own).
#[cfg(test)]
pub(crate) static TEST_STORE: std::sync::Mutex<BTreeMap<String, String>> = std::sync::Mutex::new(BTreeMap::new());

#[cfg(test)]
pub fn set(service: &str, account: &str, value: &str) -> anyhow::Result<()> {
    TEST_STORE.lock().unwrap_or_else(|e| e.into_inner()).insert(key(service, account), value.to_string());
    Ok(())
}

#[cfg(test)]
pub fn get(service: &str, account: &str) -> Option<String> {
    TEST_STORE.lock().unwrap_or_else(|e| e.into_inner()).get(&key(service, account)).cloned()
}

#[cfg(test)]
pub fn delete(service: &str, account: &str) -> anyhow::Result<()> {
    TEST_STORE.lock().unwrap_or_else(|e| e.into_inner()).remove(&key(service, account));
    Ok(())
}

#[cfg(not(test))]
pub fn set(service: &str, account: &str, value: &str) -> anyhow::Result<()> {
    let r = keyring::Entry::new(service, account).and_then(|e| e.set_password(value));
    #[cfg(target_os = "linux")]
    if let Err(e) = &r {
        note_fallback(e);
        return file_set(&crate::paths::secrets_file(), service, account, value);
    }
    Ok(r?)
}

#[cfg(not(test))]
pub fn get(service: &str, account: &str) -> Option<String> {
    let r = keyring::Entry::new(service, account).and_then(|e| e.get_password());
    #[cfg(target_os = "linux")]
    if r.is_err() { return file_get(&crate::paths::secrets_file(), service, account); }
    r.ok()
}

/// Missing is fine; anything else is an error worth surfacing.
#[cfg(not(test))]
pub fn delete(service: &str, account: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    file_delete(&crate::paths::secrets_file(), service, account)?;
    match keyring::Entry::new(service, account).and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        #[cfg(target_os = "linux")]
        Err(e) => { note_fallback(&e); Ok(()) }
        #[cfg(not(target_os = "linux"))]
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_round_trips_privately() {
        let d = std::env::temp_dir().join(format!("thoughts-secrets-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("secrets.json");
        assert_eq!(file_get(&f, "thoughts-ai", "claude"), None);
        file_set(&f, "thoughts-ai", "claude", "one").unwrap();
        file_set(&f, "thoughts-git", "github.com", "two").unwrap();
        assert_eq!(file_get(&f, "thoughts-ai", "claude").as_deref(), Some("one"));
        file_delete(&f, "thoughts-ai", "claude").unwrap();
        file_delete(&f, "thoughts-ai", "missing").unwrap();
        assert_eq!(file_get(&f, "thoughts-ai", "claude"), None);
        assert_eq!(file_get(&f, "thoughts-git", "github.com").as_deref(), Some("two"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(d).unwrap();
    }
}
