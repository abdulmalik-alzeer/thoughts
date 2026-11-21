//! STAND-IN for what `mado.rs` (the planning core, built by another worker at the same time)
//! provides. The names and shapes are the contract's (`docs/mado-spec.md`); once both branches are
//! merged, delete this file and point `mado_build.rs` at `crate::mado` instead (one `use` line).

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanTask {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub owns: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub feature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screens: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PlanDoc {
    pub version: u32,
    pub tasks: Vec<PlanTask>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Lane {
    pub id: String,
    pub tasks: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Wave {
    pub lanes: Vec<Lane>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepState {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct MadoState {
    pub version: u32,
    #[serde(default)]
    pub steps: BTreeMap<String, StepState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_hash: Option<String>,
}

/// The literal part of an `owns` pattern, before its first pattern character.
fn literal(p: &str) -> &str {
    let p = p.trim().trim_start_matches("./").trim_end_matches('/');
    match p.find(['*', '?', '[']) { Some(i) => &p[..i], None => p }
}

/// Could two `owns` patterns name the same file? Conservative: when in doubt, yes.
pub fn owns_overlap(a: &str, b: &str) -> bool {
    let (la, lb) = (literal(a), literal(b));
    let glob = |p: &str| p.contains(['*', '?', '[']);
    if !glob(a) && !glob(b) {
        return la == lb || lb.starts_with(&format!("{la}/")) || la.starts_with(&format!("{lb}/"));
    }
    la.starts_with(lb) || lb.starts_with(la)
}

/// One wave per dependency level; inside a wave, tasks whose `owns` overlap share a lane (in plan
/// order), the others get a lane each. Tasks caught in a cycle or depending on unknown tasks come
/// last (start refuses such a plan anyway).
pub fn waves(plan: &PlanDoc) -> Vec<Wave> {
    let ids: HashMap<&str, usize> = plan.tasks.iter().enumerate().map(|(i, t)| (t.id.as_str(), i)).collect();
    let mut level: Vec<Option<usize>> = vec![None; plan.tasks.len()];
    for _ in 0..=plan.tasks.len() {
        let mut changed = false;
        for (i, t) in plan.tasks.iter().enumerate() {
            if level[i].is_some() { continue; }
            let deps: Option<Vec<usize>> = t.depends_on.iter().map(|d| ids.get(d.as_str()).and_then(|j| level[*j])).collect();
            if let Some(d) = deps { level[i] = Some(d.into_iter().map(|l| l + 1).max().unwrap_or(0)); changed = true; }
        }
        if !changed { break; }
    }
    let top = level.iter().flatten().max().map_or(0, |m| m + 1);
    let levels: Vec<usize> = level.iter().map(|l| l.unwrap_or(top)).collect();
    let count = levels.iter().max().map_or(0, |m| m + 1);
    let mut out = vec![];
    for lv in 0..count {
        let members: Vec<usize> = (0..plan.tasks.len()).filter(|i| levels[*i] == lv).collect();
        if members.is_empty() { continue; }
        // Union-find over overlapping `owns`.
        let mut group: Vec<usize> = (0..members.len()).collect();
        fn root(g: &mut [usize], i: usize) -> usize { let mut r = i; while g[r] != r { r = g[r]; } g[i] = r; r }
        for a in 0..members.len() {
            for b in a + 1..members.len() {
                let (ta, tb) = (&plan.tasks[members[a]], &plan.tasks[members[b]]);
                if ta.owns.iter().any(|x| tb.owns.iter().any(|y| owns_overlap(x, y))) {
                    let (ra, rb) = (root(&mut group, a), root(&mut group, b));
                    if ra != rb { group[rb.max(ra)] = rb.min(ra); }
                }
            }
        }
        let mut lanes: Vec<(usize, Vec<String>)> = vec![];
        for k in 0..members.len() {
            let r = root(&mut group, k);
            match lanes.iter_mut().find(|(g, _)| *g == r) {
                Some((_, v)) => v.push(plan.tasks[members[k]].id.clone()),
                None => lanes.push((r, vec![plan.tasks[members[k]].id.clone()])),
            }
        }
        let w = out.len() + 1;
        out.push(Wave { lanes: lanes.into_iter().enumerate().map(|(i, (_, tasks))| Lane { id: format!("w{w}-l{}", i + 1), tasks }).collect() });
    }
    out
}

/// sha256 of `bytes`, as lowercase hex.
pub fn plan_hash(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The folder of the linked project `project_id`, as recorded (its real location). The folder must
/// still be trusted (unless full access is on).
pub async fn project_dir(state: &AppState, project_id: &str) -> anyhow::Result<PathBuf> {
    let (path, full) = {
        let s = state.settings.read().await;
        let p = s.projects.iter().find(|p| p.id == project_id).ok_or_else(|| crate::tr::err("not_found.project", &[]))?;
        (PathBuf::from(&p.path), s.full_access())
    };
    let real = crate::paths::canonical(&path).map_err(|_| crate::tr::err("not_found.folder", &[("path", &path.display().to_string())]))?;
    if !real.is_dir() { return Err(crate::tr::err("not_found.folder", &[("path", &path.display().to_string())])); }
    if !full { crate::trust::require(&real)?; }
    Ok(real)
}

/// Tell the window something about a project's MADO changed (it refetches).
pub async fn tell(state: &AppState, project_id: &str, what: &str) {
    state.emit(json!({"type": "mado", "projectId": project_id, "what": what}));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, owns: &[&str], deps: &[&str]) -> PlanTask {
        PlanTask { id: id.into(), title: id.into(), owns: owns.iter().map(|s| s.to_string()).collect(), depends_on: deps.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn waves_follow_dependencies_and_overlap_shares_a_lane() {
        let plan = PlanDoc { version: 1, tasks: vec![
            t("a", &["src/auth/**"], &[]), t("b", &["src/auth/login.ts"], &[]), t("c", &["src/ui/**"], &[]),
            t("d", &["src/app.ts"], &["a", "c"]),
        ] };
        let w = waves(&plan);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].lanes.iter().map(|l| l.tasks.clone()).collect::<Vec<_>>(), vec![vec!["a".to_string(), "b".into()], vec!["c".into()]]);
        assert_eq!(w[1].lanes[0].tasks, vec!["d".to_string()]);
        assert!(owns_overlap("src", "src/x.ts") && !owns_overlap("src/a.ts", "src/ab.ts") && owns_overlap("src/*.ts", "src/a.ts"));
    }
}
