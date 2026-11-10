//! Optional library modules. Anything skipped in onboarding can be added later:
//! the AI integrates it into the library as it exists *then*.

use serde_json::{json, Value};

pub struct Module {
    pub id: &'static str,
    pub name: &'static str,
    pub name_ar: &'static str,
    pub description: &'static str,
    pub description_ar: &'static str,
    pub default_on: bool,
    /// Instructions given to the AI when the module is added after setup.
    pub add_prompt: &'static str,
}

pub const MODULES: &[Module] = &[
    Module { id: "ideas", name: "Ideas & projects", name_ar: "الأفكار والمشاريع", default_on: true,
        description: "Capture ideas and develop them over many sessions. The backbone — always on.",
        description_ar: "دوّن أفكارك وطوّرها على مدى جلسات متعددة. هذه الميزة هي الأساس، وتبقى مفعّلة دائماً.",
        add_prompt: "" },
    Module { id: "todos", name: "Todos & tasks", name_ar: "المهام", default_on: true,
        description: "One general task list plus a list for each idea, with due dates. Tick, add and edit tasks right in the app.",
        description_ar: "قائمة مهام عامة وقائمة خاصة بكل فكرة، مع مواعيد الاستحقاق. أنجز المهام وأضفها وعدّلها مباشرة من التطبيق.",
        add_prompt: "Add a todos module: create tasks/TODO.md with ## Now / ## Next / ## Later / ## Done headings (if missing), add a Todos paragraph to CLAUDE.md Conventions (top-level GFM task lines; a task may end with ` @due(YYYY-MM-DD)` — add it whenever the user gives or implies a date, working out the actual date; other people's tasks start with their name; per-item todo.md next to the item's README, created lazily; new items into the top group; change lines in place, never rewrite the file) and a todo trigger row in the rituals table." },
    Module { id: "docs", name: "Docs / wiki", name_ar: "المستندات", default_on: true,
        description: "Reference pages you keep and come back to — procedures, templates, how-tos — linked from your ideas and work.",
        description_ar: "صفحات مرجعية تحتفظ بها وتعود إليها، كالإجراءات والنماذج والشروحات، مع روابط إليها من أفكارك وأعمالك.",
        add_prompt: "Add a docs (wiki) module: create docs/README.md as the wiki front door with a file map, and add a Docs convention to CLAUDE.md (one topic per file, light frontmatter title/updated/summary, rewrite rather than append)." },
    Module { id: "me", name: "Personal profile", name_ar: "ملفي", default_on: false,
        description: "What the assistant should know about you, so it never asks twice. It grows whenever you say \"interview me\".",
        description_ar: "ما ينبغي أن يعرفه المساعد عنك حتى لا يكرر أسئلته. يكتمل شيئاً فشيئاً كلما قلت «حاورني».",
        add_prompt: "Add the personal-profile module: create me/README.md (frontmatter title/updated/summary, an empty ## Files map) and me/interview-map.md listing the standard domains (background & education, work history, skills, finances, health & habits, goals, people & relationships) each `untouched`. The app has installed the interview-me routine at .claude/skills/interview-me/SKILL.md (tree interview: root question per domain, branching follow-ups, answers recorded into me/ topic files) — read it, do not edit it. Add its row to the CLAUDE.md rituals table, plus a Reading-protocol rule to consult me/ when an idea touches the user's situation." },
    Module { id: "checkin", name: "Check-ins (daily/weekly)", name_ar: "المراجعات الدورية", default_on: false,
        description: "A short daily or weekly review with the assistant: what happened, what you learned, what comes next. The gist is kept in your personal journal.",
        description_ar: "مراجعة قصيرة مع المساعد كل يوم أو كل أسبوع: ماذا جرى، وماذا تعلّمت، وما التالي. تُحفظ خلاصتها في سجلّك الشخصي.",
        add_prompt: "Add the check-in module: create me/log.md (heading '# Log — <name>') if missing, and add the check-in row to the CLAUDE.md rituals table. The app has installed the routine itself at .claude/skills/check-in/SKILL.md (daily/weekly debrief: what happened, learned, progress vs goals, distilled newest-first into me/log.md) — read it, do not edit it." },
    Module { id: "learning", name: "Learning program", name_ar: "برنامج التعلم", default_on: false,
        description: "Study any subject step by step: the assistant tracks your level from beginner to expert, quizzes you at the right difficulty and plans each week.",
        description_ar: "ادرس أي موضوع خطوة بخطوة: يتابع المساعد مستواك من المبتدئ إلى المتمكّن، ويختبرك بما يناسبك، ويضع لك خطة كل أسبوع.",
        add_prompt: "Add a learning module: create learning/learning-program/ as an item (README with `kind: project` in its note details and a method summary, notes/method.md describing levels L0-L5, quiz-history and knowledge-graph per topic, weekly plans in notes/plans/), describe the quiz and plan-week routines step by step in learning/learning-program/notes/routines.md (the .claude folder is managed by the app and you cannot write there), add their rows to the CLAUDE.md rituals table pointing at that file, and add a Learning paragraph to CLAUDE.md Conventions. Add its INDEX.md line." },
    Module { id: "builds", name: "Projects", name_ar: "المشاريع", default_on: true,
        description: "Connect any folder on your computer — a client's files, a report, a website — and chat with the assistant about what is in it.",
        description_ar: "اربط أي مجلد على جهازك، كملفات عميل أو تقرير أو موقع، وتحدّث مع المساعد عن محتواه.",
        add_prompt: "" },
];

pub fn list() -> Vec<Value> {
    MODULES.iter().map(|m| json!({
        "id": m.id, "name": m.name, "nameAr": m.name_ar,
        "description": m.description, "descriptionAr": m.description_ar, "defaultOn": m.default_on,
        "addLater": !m.add_prompt.is_empty(),
    })).collect()
}

pub fn get(id: &str) -> Option<&'static Module> {
    MODULES.iter().find(|m| m.id == id)
}

/// Routines from the starter kit that belong to a feature. The assistant may not write under
/// `.claude/` (nothing there may change without the user seeing it), so the app installs these
/// itself, byte for byte as shipped, when the feature is switched on.
pub fn skills(id: &str) -> &'static [&'static str] {
    match id {
        "me" => &["interview-me"],
        "checkin" => &["check-in"],
        _ => &[],
    }
}

/// Copy a feature's routines into the library where they are missing. Existing files are never replaced.
pub fn install_skills(library: &std::path::Path, id: &str) -> std::io::Result<Vec<String>> {
    let mut installed = vec![];
    for skill in skills(id) {
        let Some(dir) = crate::library::STARTER.get_dir(format!(".claude/skills/{skill}")) else { continue };
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            stack.extend(d.dirs());
            for f in d.files() {
                let dest = library.join(f.path());
                if std::fs::symlink_metadata(&dest).is_ok() { continue; }
                if let Some(parent) = dest.parent() { std::fs::create_dir_all(parent)?; }
                std::fs::write(&dest, f.contents())?;
                installed.push(f.path().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(installed)
}

pub fn add_prompt(m: &Module) -> String {
    format!(
        "The user just enabled the **{}** module in the Thoughts app. Integrate it into this library \
         *as it exists now* — read CLAUDE.md and INDEX.md first and fit the new pieces to the existing \
         structure and conventions rather than pasting a template. Follow the conventions already in CLAUDE.md: \
         items can be ideas or work items (a README's note details carry `kind:` — idea, client, matter, project, or the \
         user's own word), todos are task lines that may end with ` @due(YYYY-MM-DD)`, and everything is written in the \
         user's language while the file names and keys the app reads stay as they are. The `.claude` folder is managed by \
         the app: you can read it but not change it.\n\n{}\n\n\
         When done, summarise in 3 lines what you added and how the user triggers it.",
        m.name, m.add_prompt
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_module_has_arabic_copy() {
        for m in MODULES {
            assert!(!m.name_ar.is_empty() && !m.description_ar.is_empty(), "{}", m.id);
        }
        let docs = list().into_iter().find(|m| m["id"] == "docs").unwrap();
        assert_eq!(docs["nameAr"], "المستندات");
    }

    #[test]
    fn descriptions_speak_to_people_not_developers() {
        for m in MODULES {
            for text in [m.name, m.name_ar, m.description, m.description_ar] {
                for jargon in [".md", "me/", "docs/", "L0", "L5", "code repo", "TODO", "README", "git", "وحدة", "\u{064B}\u{0627}"] {
                    assert!(!text.contains(jargon), "{}: {text:?} contains {jargon:?}", m.id);
                }
            }
        }
    }

    #[test]
    fn a_features_routines_are_installed_exactly_as_shipped() {
        let d = std::env::temp_dir().join(format!("thoughts-modules-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join(".claude/skills/check-in")).unwrap();
        std::fs::write(d.join(".claude/skills/check-in/SKILL.md"), "mine").unwrap();
        // Nothing the user already has is replaced.
        assert!(install_skills(&d, "checkin").unwrap().iter().all(|p| p != ".claude/skills/check-in/SKILL.md"));
        assert_eq!(std::fs::read_to_string(d.join(".claude/skills/check-in/SKILL.md")).unwrap(), "mine");
        let installed = install_skills(&d, "me").unwrap();
        assert!(installed.contains(&".claude/skills/interview-me/SKILL.md".to_string()), "{installed:?}");
        let shipped = crate::library::STARTER.get_file(".claude/skills/interview-me/SKILL.md").unwrap().contents();
        assert_eq!(std::fs::read(d.join(".claude/skills/interview-me/SKILL.md")).unwrap(), shipped);
        assert!(install_skills(&d, "me").unwrap().is_empty());
        assert!(install_skills(&d, "todos").unwrap().is_empty());
        // The prompts no longer ask the assistant to write where it is not allowed to.
        for m in MODULES { assert!(!m.add_prompt.contains("skills under .claude"), "{}", m.id); }
        std::fs::remove_dir_all(d).unwrap();
    }
}
