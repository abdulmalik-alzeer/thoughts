//! MADO: the review, the gate before the build. Pure functions over the spec, the flow and the plan:
//! what traces to what, what is wrong, the waves and lanes the build will run, and every command
//! the build will run (shown verbatim; approving the review is what allows the daemon to run them).

use crate::mado::{FeatureTrace, GraphDoc, Lane, PlanDoc, Problem, Review, ScreenTrace, Wave};
use std::collections::{HashMap, HashSet};

/// The plan's fingerprint: sha256 of the bytes of `plan.json`, as lowercase hex.
pub fn plan_hash(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn wild(seg: &str) -> bool { seg.contains(['*', '?', '[']) }

/// One path segment pattern (`*`, `?`, `[…]`) against a plain segment, ignoring letter case (macOS
/// and Windows volumes do).
fn seg_matches(pat: &str, name: &str) -> bool {
    let mut re = String::from("(?i)^");
    let mut class = false;
    for c in pat.chars() {
        match c {
            _ if class => { if c == ']' { class = false; } re.push(c); }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            '[' => { class = true; re.push('['); }
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    regex::Regex::new(&re).map_or(true, |r| r.is_match(name))
}

/// Could two `owns` entries (paths or globs, project-relative) name a file in common? Erring on
/// the side of "yes": tasks that might touch the same file run one after another.
pub fn overlaps(a: &str, b: &str) -> bool {
    let (sa, sb): (Vec<&str>, Vec<&str>) = (a.trim_matches('/').split('/').collect(), b.trim_matches('/').split('/').collect());
    let mut i = 0;
    loop {
        match (sa.get(i), sb.get(i)) {
            (Some(&"**"), _) | (_, Some(&"**")) => return true,
            // One ends where the other goes on: the shorter names a folder holding the other (or the same file).
            (None, _) | (_, None) => return true,
            (Some(x), Some(y)) => {
                let same = match (wild(x), wild(y)) {
                    (false, false) => x.eq_ignore_ascii_case(y),
                    (true, false) => seg_matches(x, y),
                    (false, true) => seg_matches(y, x),
                    (true, true) => true,
                };
                if !same { return false; }
            }
        }
        i += 1;
    }
}

fn owns_overlap(a: &[String], b: &[String]) -> Option<String> {
    for x in a { for y in b { if overlaps(x, y) { return Some(if x.len() <= y.len() { x.clone() } else { y.clone() }); } } }
    None
}

/// Each task's dependency level (0 = depends on nothing), `None` for a task in a cycle or after one.
/// Dependencies on tasks that do not exist are left out here (the review names them).
fn levels(plan: &PlanDoc) -> HashMap<String, Option<usize>> {
    let ids: HashSet<&str> = plan.tasks.iter().map(|t| t.id.as_str()).collect();
    let deps: HashMap<&str, Vec<&str>> = plan.tasks.iter().map(|t| (t.id.as_str(), t.depends_on.iter().map(String::as_str).filter(|d| ids.contains(d)).collect())).collect();
    fn level<'a>(id: &'a str, deps: &HashMap<&'a str, Vec<&'a str>>, memo: &mut HashMap<&'a str, Option<usize>>, on: &mut HashSet<&'a str>) -> Option<usize> {
        if let Some(l) = memo.get(id) { return *l; }
        if !on.insert(id) { return None; }
        let mut best = Some(0usize);
        for d in deps.get(id).into_iter().flatten() {
            best = match (best, level(d, deps, memo, on)) { (Some(b), Some(l)) => Some(b.max(l + 1)), _ => None };
        }
        on.remove(id);
        memo.insert(id, best);
        best
    }
    let mut memo = HashMap::new();
    for t in &plan.tasks { let mut on = HashSet::new(); level(&t.id, &deps, &mut memo, &mut on); }
    plan.tasks.iter().map(|t| (t.id.clone(), memo.get(t.id.as_str()).copied().flatten())).collect()
}

/// The waves of the plan: one per dependency level. Inside a wave, tasks whose `owns` overlap share
/// a lane (run one after another, in the plan's order); every other task gets a lane of its own.
/// Tasks in a dependency cycle (and those after them) are in no wave.
pub fn waves(plan: &PlanDoc) -> Vec<Wave> {
    let lv = levels(plan);
    let depth = lv.values().flatten().max().map_or(0, |m| m + 1);
    let mut out = vec![];
    for w in 0..depth {
        let tasks: Vec<&crate::mado::PlanTask> = plan.tasks.iter().filter(|t| lv.get(&t.id).copied().flatten() == Some(w)).collect();
        // Lanes: tasks joined whenever what they own overlaps (directly or through another).
        let mut lane_of: Vec<usize> = (0..tasks.len()).collect();
        fn root(l: &mut Vec<usize>, i: usize) -> usize { let mut r = i; while l[r] != r { r = l[r]; } l[i] = r; r }
        for i in 0..tasks.len() {
            for j in 0..i {
                if owns_overlap(&tasks[i].owns, &tasks[j].owns).is_some() {
                    let (a, b) = (root(&mut lane_of, i), root(&mut lane_of, j));
                    if a != b { lane_of[a.max(b)] = a.min(b); }
                }
            }
        }
        let mut lanes: Vec<(usize, Vec<String>)> = vec![];
        for (i, t) in tasks.iter().enumerate() {
            let r = root(&mut lane_of, i);
            match lanes.iter_mut().find(|(k, _)| *k == r) { Some((_, l)) => l.push(t.id.clone()), None => lanes.push((r, vec![t.id.clone()])) }
        }
        out.push(Wave { lanes: lanes.into_iter().enumerate().map(|(n, (_, tasks))| Lane { id: format!("w{}-l{}", w + 1, n + 1), tasks }).collect() });
    }
    out
}

/// The tasks of the plan that are in a dependency cycle (or between two).
fn in_cycles(plan: &PlanDoc) -> Vec<String> {
    let ids: HashSet<&str> = plan.tasks.iter().map(|t| t.id.as_str()).collect();
    let mut left: HashSet<&str> = ids.clone();
    let deps = |t: &str| -> Vec<&str> { plan.tasks.iter().find(|x| x.id == t).map(|x| x.depends_on.iter().map(String::as_str).filter(|d| ids.contains(d)).collect()).unwrap_or_default() };
    // Take away what depends on nothing left, then what nothing left depends on.
    loop {
        let free: Vec<&str> = left.iter().copied().filter(|t| deps(t).iter().all(|d| !left.contains(d))).collect();
        if free.is_empty() { break; }
        for f in free { left.remove(f); }
    }
    loop {
        let leaves: Vec<&str> = left.iter().copied().filter(|t| !left.iter().any(|o| deps(o).contains(t))).collect();
        if leaves.is_empty() { break; }
        for f in leaves { left.remove(f); }
    }
    plan.tasks.iter().filter(|t| left.contains(t.id.as_str())).map(|t| t.id.clone()).collect()
}

pub(crate) fn problem(ar: bool, kind: &str, key: &str, args: &[(&str, &str)], tasks: Vec<String>) -> Problem {
    Problem { kind: kind.into(), message: crate::tr::text_in(ar, key, args), tasks: (!tasks.is_empty()).then_some(tasks) }
}

/// Does this problem keep the review from being approved? A cycle or a missing dependency cannot be
/// built; a bad path could reach outside the project; a task without a check cannot be called done;
/// a task for a feature the spec does not have traces to nothing; a command with shell or pattern
/// syntax could run more than the user was shown. Untraced features and screens are
/// for the user to judge, and overlapping tasks simply run one after another. A folder that is not a
/// git repository of its own, or not on a branch, cannot be built at all (`mado::repo_problems`);
/// changes not committed yet are only told: the build commits them first.
pub fn blocks(p: &Problem) -> bool { matches!(p.kind.as_str(), "cycle" | "missing-dep" | "bad-path" | "bad-command" | "empty-acceptance" | "unknown-feature" | "no-repo" | "no-branch") }

/// `review_in` in the UI language.
pub fn review(spec: &str, flow: Option<&GraphDoc>, plan: &PlanDoc) -> Review { review_in(crate::tr::arabic(), spec, flow, plan) }

/// The review of `plan` against the spec's features and the flow's screens, its messages in Arabic
/// (`ar`) or English.
pub fn review_in(ar: bool, spec: &str, flow: Option<&GraphDoc>, plan: &PlanDoc) -> Review {
    let feats = crate::mado_validate::features(spec);
    let screens = flow.map(crate::mado::screens_of).unwrap_or_default();
    let ids: HashSet<&str> = plan.tasks.iter().map(|t| t.id.as_str()).collect();
    let features: Vec<FeatureTrace> = feats.iter().map(|(id, name)| FeatureTrace { id: id.clone(), name: name.clone(),
        tasks: plan.tasks.iter().filter(|t| &t.feature == id).map(|t| t.id.clone()).collect() }).collect();
    let screen_traces: Vec<ScreenTrace> = screens.iter().map(|(id, label)| ScreenTrace { id: id.clone(), label: label.clone(),
        tasks: plan.tasks.iter().filter(|t| t.screens.iter().flatten().any(|s| s == id)).map(|t| t.id.clone()).collect() }).collect();
    let mut problems = vec![];
    for c in plan.setup.iter().flatten().filter(|c| !crate::mado_validate::command_ok(c)) {
        problems.push(problem(ar, "bad-command", "mado.p.bad_setup", &[("command", &crate::mado_validate::escaped(c.trim()))], vec![]));
    }
    for c in plan.verify.iter().flatten().filter(|c| !crate::mado_validate::command_ok(c)) {
        problems.push(problem(ar, "bad-command", "mado.p.bad_verify", &[("command", &crate::mado_validate::escaped(c.trim()))], vec![]));
    }
    if let Some(r) = plan.run.as_ref().filter(|r| !crate::mado_validate::command_ok(&r.start)) {
        problems.push(problem(ar, "bad-command", "mado.p.bad_start", &[("command", &crate::mado_validate::escaped(r.start.trim()))], vec![]));
    }
    let cyc = in_cycles(plan);
    if !cyc.is_empty() { problems.push(problem(ar, "cycle", "mado.p.cycle", &[("tasks", &cyc.join(", "))], cyc.clone())); }
    for t in &plan.tasks {
        for d in t.depends_on.iter().filter(|d| !ids.contains(d.as_str())) {
            problems.push(problem(ar, "missing-dep", "mado.p.missing_dep", &[("task", &t.id), ("dep", d)], vec![t.id.clone()]));
        }
        if t.depends_on.iter().any(|d| d == &t.id) && !cyc.contains(&t.id) {
            problems.push(problem(ar, "cycle", "mado.p.cycle", &[("tasks", &t.id)], vec![t.id.clone()]));
        }
        for o in t.owns.iter().filter(|o| !crate::mado_validate::owned_path_ok(o)) {
            problems.push(problem(ar, "bad-path", "mado.p.bad_path", &[("task", &t.id), ("path", o)], vec![t.id.clone()]));
        }
        for c in t.acceptance.iter().filter(|c| !c.trim().is_empty() && !crate::mado_validate::command_ok(c)) {
            problems.push(problem(ar, "bad-command", "mado.p.bad_command", &[("task", &t.id), ("command", &crate::mado_validate::escaped(c.trim()))], vec![t.id.clone()]));
        }
        if t.acceptance.iter().all(|c| c.trim().is_empty()) {
            problems.push(problem(ar, "empty-acceptance", "mado.p.empty_acceptance", &[("task", &t.id)], vec![t.id.clone()]));
        }
        if !feats.is_empty() && !feats.iter().any(|(id, _)| id == &t.feature) {
            problems.push(problem(ar, "unknown-feature", "mado.p.unknown_feature", &[("task", &t.id), ("feature", &t.feature)], vec![t.id.clone()]));
        }
        if flow.is_some() {
            for s in t.screens.iter().flatten().filter(|s| !screens.iter().any(|(id, _)| id == *s)) {
                problems.push(problem(ar, "untraced-screen", "mado.p.unknown_screen", &[("task", &t.id), ("screen", s)], vec![t.id.clone()]));
            }
        }
    }
    for f in features.iter().filter(|f| f.tasks.is_empty()) {
        problems.push(problem(ar, "untraced-feature", "mado.p.untraced_feature", &[("feature", &format!("{} — {}", f.id, f.name))], vec![]));
    }
    for s in screen_traces.iter().filter(|s| s.tasks.is_empty()) {
        problems.push(problem(ar, "untraced-screen", "mado.p.untraced_screen", &[("screen", &s.label)], vec![]));
    }
    // Round 0.4.0, worth a look (none blocks): how the joined product is checked.
    if plan.verify.iter().flatten().all(|c| c.trim().is_empty()) { problems.push(problem(ar, "no-verify", "mado.p.no_verify", &[], vec![])); }
    match &plan.run {
        None => problems.push(problem(ar, "no-run", "mado.p.no_run", &[], vec![])),
        Some(r) => for s in screens.iter().filter(|(id, _)| !r.screens.iter().any(|x| &x.id == id)) {
            problems.push(problem(ar, "unchecked-screen", "mado.p.unchecked_screen", &[("screen", &s.1)], vec![]));
        },
    }
    if plan.tasks.len() > 1 && !plan.tasks.iter().any(|t| t.kind.as_deref() == Some("contracts")) { problems.push(problem(ar, "no-contracts", "mado.p.no_contracts", &[], vec![])); }
    if !plan.tasks.iter().any(|t| t.kind.as_deref() == Some("tests")) { problems.push(problem(ar, "no-tests", "mado.p.no_tests", &[], vec![])); }
    let ws = waves(plan);
    for (n, w) in ws.iter().enumerate() {
        for l in w.lanes.iter().filter(|l| l.tasks.len() > 1) {
            let first = plan.tasks.iter().find(|t| t.id == l.tasks[0]);
            let shared = first.and_then(|a| l.tasks[1..].iter().filter_map(|b| plan.tasks.iter().find(|t| &t.id == b)).find_map(|b| owns_overlap(&a.owns, &b.owns)));
            problems.push(problem(ar, "overlap", "mado.p.overlap", &[("tasks", &l.tasks.join(", ")), ("wave", &(n + 1).to_string()), ("path", shared.as_deref().unwrap_or("…"))], l.tasks.clone()));
        }
    }
    let mut seen = HashSet::new();
    // The setup commands first: they run first, in every fresh working copy; then the tasks' checks,
    // the checks of the joined product (`verify`) and the command that starts it (`run.start`).
    let commands: Vec<String> = plan.all_commands().into_iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty() && seen.insert(c.clone()))
        // A refused command is shown escaped (it blocks approval, so it never reaches an approval record).
        .map(|c| if crate::mado_validate::command_ok(&c) { c } else { crate::mado_validate::escaped(&c) }).collect();
    Review { features, screens: screen_traces, problems, waves: ws, commands, plan_hash: String::new() }
}
