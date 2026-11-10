//! Update packs. Because every install is customised, official updates ship as
//! *packs*: a manifest of items, each with files and a prompt. The user picks
//! items and their AI merges them into their own app layer / library.

use include_dir::{include_dir, Dir};
use serde_json::{json, Value};

static UPDATES: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../updates");

pub fn list() -> Vec<Value> {
    let mut v: Vec<Value> = UPDATES.dirs().filter_map(|d| {
        let m = d.get_file(d.path().join("manifest.json"))?;
        let mut man: Value = serde_json::from_slice(m.contents()).ok()?;
        man["id"] = json!(d.path().to_string_lossy());
        Some(man)
    }).collect();
    v.sort_by(|a, b| b["id"].as_str().cmp(&a["id"].as_str()));
    v
}

/// Extract the chosen items' files to a staging dir and build the AI prompt.
pub fn prepare(pack: &str, items: &[String]) -> anyhow::Result<(std::path::PathBuf, String)> {
    let d = UPDATES.get_dir(pack).ok_or_else(|| anyhow::anyhow!("unknown pack"))?;
    let man: Value = serde_json::from_slice(d.get_file(d.path().join("manifest.json")).unwrap().contents())?;
    let stage = crate::paths::home().join("updates").join(pack);
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage)?;
    d.extract(stage.parent().unwrap())?;
    let mut prompt = format!(
        "Apply the Thoughts update pack **{}** to this install. The pack's files are staged in `{}`.\n\
         The user's app layer has been customised — merge, never blindly overwrite; keep their changes.\n\n\
         Apply ONLY these items:\n",
        man["title"].as_str().unwrap_or(pack), stage.display()
    );
    for it in man["items"].as_array().into_iter().flatten() {
        let id = it["id"].as_str().unwrap_or_default();
        if items.iter().any(|x| x == id) {
            prompt.push_str(&format!("\n### {}\n{}\n", it["title"].as_str().unwrap_or(id), it["prompt"].as_str().unwrap_or("")));
        }
    }
    prompt.push_str("\nFinish with a short list of what changed.");
    Ok((stage, prompt))
}
