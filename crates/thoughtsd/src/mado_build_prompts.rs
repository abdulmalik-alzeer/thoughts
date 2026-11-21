//! The instructions of the build's runs added in round 0.4.0 (the `verify` gate's fix, the review,
//! the review's fix) and the parts added to a task's brief. The app's own words; everything from the
//! project or a run (a command's output, the conventions, comments, screen results, findings) goes in
//! only as a data block (`mado_prompts::block`, a fresh random code each) with the data note.
//! (Kept apart from `mado_prompts`, the planning steps' instructions.)

use super::finish::{Finding, Severity};
use crate::mado_prompts::{block, DATA_NOTE};

/// The project's conventions, for a task's brief or a fix run (text of `mado/conventions.md`).
pub fn conventions(text: &str) -> String {
    format!("\nThe project's conventions (mado/conventions.md): the one way this project builds a screen, handles an error, names things, \
        talks to its API, and the shared helpers that exist and how they must be called. Follow them exactly; do not invent a second way:\n{}\n", block(text))
}

/// Added to the brief of a `contracts` or `tests` task.
pub const LOCKING_TASK: &str = "\nWhat this task writes is the agreement the other tasks build on (the example answers of every endpoint or \
    module boundary, or the acceptance tests of a feature from mado/spec.md). Once it is done, no later task may change these files: \
    write them completely and exactly now.\n";

/// The first fix run of a failed `verify` in the build's copy.
pub fn gate_fix(output: &str, conventions_text: Option<&str>) -> String {
    let mut out = format!("This folder holds the build so far: the work of every task up to now, put together. The plan's checks of the whole \
        product (its test suite, type check and linter) now fail here. What they printed:\n{}\n\n\
        Fix the code so that these checks pass, keeping the work of every task. Change only what the failure needs. \
        Files written by a contracts or tests task are agreed and locked: make the code fit them, never the other way round \
        (changes to them are undone). You cannot run commands: the app runs the checks again when you finish. \
        Commit nothing: the app commits the result.\n", block(output));
    if let Some(c) = conventions_text { out.push_str(&conventions(c)); }
    out.push_str(&format!("\n{DATA_NOTE}\n"));
    out
}

/// A further try of the same fix, in the same chat.
pub fn gate_retry(output: &str, undone: &[String]) -> String {
    let mut out = format!("The app ran the checks of the whole product again after your changes, and they still fail. What they printed:\n{}\n", block(output));
    if !undone.is_empty() {
        out.push_str(&format!("Your changes to these locked files were undone: {}\n", undone.iter().take(10).map(|f| crate::mado_prompts::inline(f)).collect::<Vec<_>>().join(", ")));
    }
    out.push_str(&format!("Fix it in the files. You still cannot run commands: the app runs the checks again when you finish.\n\n{DATA_NOTE}\n"));
    out
}

pub const REVIEW_SYSTEM: &str = "This run reviews a build the Thoughts app made by itself from a plan the user approved. Nobody is watching it: \
do not ask questions. Only read: you have no shell and you may not change any file. End with your findings in the exact form the brief asks for.";

/// The review: a fresh run that reads the build's copy against the spec, the conventions, the
/// comments and what the screens showed. `earlier`: what the review before this one found (after fixes).
pub fn review(round: u32, has_conventions: bool, comments: &str, screens: &str, earlier: &[Finding]) -> String {
    let mut out = format!("Review {round} of this build. This folder holds the whole product as the app built it, every task's work put together. \
        Each task passed its own checks; what goes wrong now is usually where one task's work meets another's: a screen and the API it calls \
        (paths, field names, the shape of data and of errors), shared helpers called in different ways, a form that sends what the server does not read, \
        a list that shows nothing because the names differ.\n\n\
        Read mado/spec.md first: for every feature, go through its pass/fail lists and decide from the code whether each item passes. \
        Then read mado/tech.md, mado/design.md and mado/plan.json{}, and the code itself.\n", if has_conventions { ", mado/conventions.md (the one way this project does things: report every place that does it another way)" } else { "" });
    if !screens.trim().is_empty() {
        out.push_str(&format!("\nThe app started the product and looked at its screens. For each: its address, the status code the app got, and (when a window could look) \
            whether it worked, the errors it raised, the text it showed and the value of every field:\n{}\n", block(screens)));
    } else {
        out.push_str("\nThe app could not look at the screens this time: judge them from the code.\n");
    }
    if !comments.trim().is_empty() {
        out.push_str(&format!("\nThe user's comments on the plan (open ones still stand; resolved ones were settled in the plan and must hold in the product):\n{}\n", block(comments)));
    }
    if !earlier.is_empty() {
        let list: Vec<String> = earlier.iter().map(|f| format!("[{}] {}{}", match f.severity { Severity::Must => "must", Severity::Should => "should" },
            f.at.as_deref().map(|w| format!("{w}: ")).unwrap_or_default(), f.text)).collect();
        out.push_str(&format!("\nThe review before this one found these, and a fix was made since. Check each again, and look for what the fix may have broken:\n{}\n", block(&list.join("\n"))));
    }
    out.push_str("\nReport only real faults you can point to in the code (a file, and a line when you can). `must`: the product is broken there, or a pass/fail \
        item of the spec fails; `should`: worth fixing, but nothing is broken. At most 30 findings, the most important first.\n\
        End your final message with exactly one fenced block in this form, and nothing after it (an empty list when nothing is wrong):\n\
        ```json\n{\"findings\": [{\"severity\": \"must\", \"where\": \"src/screens/customers.ts:42\", \"text\": \"What is wrong, and what it should be.\"}]}\n```\n");
    out.push_str(&format!("\n{DATA_NOTE}\n"));
    out
}

/// The fix of a review's `must` findings, in the build's copy.
pub fn review_fix(musts: &[Finding], conventions_text: Option<&str>) -> String {
    let list: Vec<String> = musts.iter().map(|f| format!("- {}{}", f.at.as_deref().map(|w| format!("{w}: ")).unwrap_or_default(), f.text)).collect();
    let mut out = format!("This folder holds the whole product the app built. A review found these faults that must be fixed:\n{}\n\n\
        Fix each of them in the code, keeping everything else as it is. Files written by a contracts or tests task are agreed and locked: \
        make the code fit them (changes to them are undone). You cannot run commands: the app runs the product's checks afterwards and reviews it again. \
        Commit nothing: the app commits the result.\n", block(&list.join("\n")));
    if let Some(c) = conventions_text { out.push_str(&conventions(c)); }
    out.push_str(&format!("\n{DATA_NOTE}\n"));
    out
}
