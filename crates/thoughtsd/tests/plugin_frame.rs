//! The page every add-on frame loads (`ui/public/plugin-frame.html`) allows exactly one inline
//! script: the frame runtime, by its sha256 hash. The same hash has to be in the desktop app's own
//! content-security policy, because on Windows the page is handed to the frame as `srcdoc` and then
//! also answers to the policy of the window that embeds it. `scripts/gen-plugin-frame.mjs` writes
//! both from `plugin-frame.js` (it runs with every `vite build`); this test fails when the runtime
//! was edited and the two generated places were not brought along.

use base64::Engine;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn repo(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The `'sha256-…'` sources of one directive of a policy.
fn hashes(policy: &str, directive: &str) -> Vec<String> {
    policy.split(';').map(str::trim).find(|d| d.split_whitespace().next() == Some(directive)).unwrap_or("")
        .split_whitespace().filter(|s| s.starts_with("'sha256-")).map(|s| s.trim_matches('\'').trim_start_matches("sha256-").to_string()).collect()
}

fn attr<'a>(text: &'a str, open: &str) -> &'a str {
    let from = text.find(open).unwrap_or_else(|| panic!("{open} not found")) + open.len();
    &text[from..from + text[from..].find('"').expect("closing quote")]
}

#[test]
fn the_frame_runtime_hash_is_the_same_in_the_frame_page_and_in_the_app_policy() {
    // What the generator hashes: the runtime with LF line endings, no BOM, and `</script` escaped.
    let js = repo("ui/public/plugin-frame.js").trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut inlined = String::with_capacity(js.len());
    let mut rest = js.as_str();
    while let Some(i) = rest.to_ascii_lowercase().find("</script") {
        inlined.push_str(&rest[..i]);
        inlined.push_str("<\\/");
        inlined.push_str(&rest[i + 2..i + 8]);
        rest = &rest[i + 8..];
    }
    inlined.push_str(rest);
    let want = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(inlined.as_bytes()));

    let page = repo("ui/public/plugin-frame.html").replace("\r\n", "\n");
    let page_policy = attr(&page, "http-equiv=\"Content-Security-Policy\" content=\"");
    assert_eq!(hashes(page_policy, "script-src"), [want.clone()], "ui/public/plugin-frame.html is stale: run `node scripts/gen-plugin-frame.mjs`");
    assert!(page.contains(&format!("<script>{inlined}</script>")), "the page does not carry the current runtime: run `node scripts/gen-plugin-frame.mjs`");
    // The frame may reach nothing and may not be sent anywhere; a frame it builds can run only the runtime.
    for must in ["default-src 'none'", "connect-src 'none'", "form-action 'none'", "frame-src 'none'", "base-uri 'none'", "'strict-dynamic'", "webrtc 'block'"] {
        assert!(page_policy.contains(must), "the frame policy lost `{must}`");
    }

    let conf = repo("src-tauri/tauri.conf.json");
    let app_policy = attr(&conf, "\"csp\": \"");
    assert_eq!(hashes(app_policy, "script-src"), [want], "src-tauri/tauri.conf.json is stale: run `node scripts/gen-plugin-frame.mjs`");
    // Frames of the window may only hold the app's own pages and blobs it made (never data: pages).
    let frames = app_policy.split(';').map(str::trim).find(|d| d.starts_with("frame-src")).expect("frame-src");
    assert_eq!(frames, "frame-src 'self' blob:");
}
