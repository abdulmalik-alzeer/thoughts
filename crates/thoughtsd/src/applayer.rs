//! The app layer: a git repo (`~/.thoughts/app-layer`) holding everything that
//! makes this install *yours* — CSS, JS plugins, custom views. The AI edits it
//! freely; every change is a commit; Rollback restores any commit.

use crate::state::AppState;
use serde_json::{json, Value};

const README: &str = r#"# Thoughts app layer

This folder customises the Thoughts app itself. The AI may change anything here.
Every change is committed; the Rollback button (always in the title bar and in
the Recovery menu) restores any previous commit.

- `custom.css` — loaded after the built-in theme. Override CSS variables such as
  `--bg`, `--fg`, `--accent`, `--panel`, `--border`, `--muted` per
  `[data-theme="saudi|contrast|tech"][data-mode="light|dark"]`, or add new themes
  with `[data-theme="mytheme"]` and list them in `themes.json`
  (`[{"id":"mytheme","name":"My theme"}]`).
- `plugins/*.js` — ES modules: `export default function (thoughts) { … }`.
  `thoughts` exposes `api(path, opts)`, `addView({id, title, icon, render(el)})`,
  `addCommand({id, title, run})`, `on(event, fn)`, `toast(text)`.
- `views/*.html` — static pages shown in the sidebar (rendered in a sandboxed frame).

Safe mode (Recovery menu) starts the app with this layer disabled.
"#;

pub async fn ensure(state: &AppState) -> anyhow::Result<()> {
    let dir = crate::paths::app_layer_dir();
    if dir.join(".git").exists() { return Ok(()); }
    std::fs::create_dir_all(dir.join("plugins"))?;
    std::fs::create_dir_all(dir.join("views"))?;
    std::fs::write(dir.join("README.md"), README)?;
    std::fs::write(dir.join("CLAUDE.md"), "Read README.md: it is the contract for customising the Thoughts app. Keep changes small and self-contained.\n")?;
    std::fs::write(dir.join("custom.css"), "/* Your overrides. */\n")?;
    std::fs::write(dir.join("themes.json"), "[]\n")?;
    std::fs::write(dir.join("plugins/.gitkeep"), "")?;
    std::fs::write(dir.join("views/.gitkeep"), "")?;
    crate::git::init(state, &dir).await?;
    crate::git::commit_all(state, &dir, "app layer: initial").await?;
    Ok(())
}

pub async fn manifest() -> Value {
    let dir = crate::paths::app_layer_dir();
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap_or_default();
    let list = |sub: &str, ext: &str| -> Vec<Value> {
        let mut v: Vec<Value> = std::fs::read_dir(dir.join(sub)).into_iter().flatten().flatten()
            .filter(|e| e.path().extension().map_or(false, |x| x == ext))
            .map(|e| json!({"name": e.file_name().to_string_lossy(), "code": std::fs::read_to_string(e.path()).unwrap_or_default()}))
            .collect();
        v.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        v
    };
    json!({
        "css": read("custom.css"),
        "themes": serde_json::from_str::<Value>(&read("themes.json")).unwrap_or(json!([])),
        "plugins": list("plugins", "js"),
        "views": list("views", "html"),
    })
}

pub async fn history(state: &AppState) -> Vec<Value> {
    crate::git::log(state, &crate::paths::app_layer_dir(), 100).await
}

/// Restore the layer to `hash` as a *new* commit, so rollback itself is undoable.
pub async fn rollback(state: &AppState, hash: &str) -> anyhow::Result<()> {
    if !hash.chars().all(|c| c.is_ascii_hexdigit()) && hash != "HEAD~1" { anyhow::bail!("bad commit"); }
    let dir = crate::paths::app_layer_dir();
    crate::git::commit_all(state, &dir, "app layer: snapshot before rollback").await?;
    let short = crate::git::git(state, &dir, &["rev-parse", "--short", hash]).await?.trim().to_string();
    crate::git::git(state, &dir, &["read-tree", "-u", "--reset", hash]).await?;
    crate::git::commit_all(state, &dir, &format!("rollback to {short}")).await?;
    state.emit(json!({"type": "layer.changed"}));
    Ok(())
}
