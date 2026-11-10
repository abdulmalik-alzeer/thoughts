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
        description_ar: "دوّن أفكارك وطوّرها على مدى جلسات متعددة. هذه الوحدة هي الأساس، وتبقى مفعّلة دائمًا.",
        add_prompt: "" },
    Module { id: "todos", name: "Todos & tasks", name_ar: "المهام", default_on: true,
        description: "A root TODO.md plus a todo.md per idea, editable from the app.",
        description_ar: "قائمة مهام عامة، وقائمة خاصة بكل فكرة، تعدّلها مباشرة من التطبيق.",
        add_prompt: "Add a todos module: create tasks/TODO.md with ## Now / ## Next / ## Later / ## Done headings (if missing), add a Todos paragraph to CLAUDE.md Conventions (top-level GFM task lines; a task may end with ` @due(YYYY-MM-DD)` — add it whenever the user gives or implies a date, working out the actual date; other people's tasks start with their name; per-item todo.md next to the item's README, created lazily; new items into the top group; change lines in place, never rewrite the file) and a todo trigger row in the rituals table." },
    Module { id: "docs", name: "Docs / wiki", name_ar: "المستندات", default_on: true,
        description: "Long-lived reference docs in docs/, linked from ideas and work items.",
        description_ar: "مستندات مرجعية تبقى معك طويلًا في مجلد docs/، مع روابط إليها من أفكارك.",
        add_prompt: "Add a docs (wiki) module: create docs/README.md as the wiki front door with a file map, and add a Docs convention to CLAUDE.md (one topic per file, light frontmatter title/updated/summary, rewrite rather than append)." },
    Module { id: "me", name: "Personal profile (me/)", name_ar: "الملف الشخصي", default_on: false,
        description: "General facts about you so the AI never re-asks. Grown by \"interview me\".",
        description_ar: "معلومات عامة عنك حتى لا يكرر المساعد أسئلته. تكتمل شيئًا فشيئًا عبر «حاورني».",
        add_prompt: "Add the personal-profile module: create me/README.md (frontmatter title/updated/summary, an empty ## Files map) and me/interview-map.md listing the standard domains (background & education, work history, skills, finances, health & habits, goals, people & relationships) each `untouched`. Add an interview-me skill at .claude/skills/interview-me/SKILL.md if missing (tree interview: root question per domain, branching follow-ups, record answers into me/ topic files) and its row in the CLAUDE.md rituals table, plus a Reading-protocol rule to consult me/ when an idea touches the user's situation." },
    Module { id: "checkin", name: "Check-ins (daily/weekly)", name_ar: "المراجعات الدورية", default_on: false,
        description: "Recurring debriefs distilled into me/log.md.",
        description_ar: "جلسات مراجعة يومية أو أسبوعية تُحفظ خلاصتها في سجلّك الشخصي.",
        add_prompt: "Add the check-in module: create me/log.md (heading '# Log — <name>') if missing, add .claude/skills/check-in/SKILL.md if missing (daily/weekly debrief: what happened, learned, progress vs goals, distilled newest-first into me/log.md), and its CLAUDE.md rituals row." },
    Module { id: "learning", name: "Learning program", name_ar: "برنامج التعلم", default_on: false,
        description: "Study topics with levels L0–L5, adaptive quizzes and weekly plans.",
        description_ar: "موضوعات تدرسها بمستويات من L0 إلى L5، مع اختبارات تتكيّف مع مستواك وخطط أسبوعية.",
        add_prompt: "Add a learning module: create learning/learning-program/ as an item (README with `kind: project` in its note details and a method summary, notes/method.md describing levels L0-L5, quiz-history and knowledge-graph per topic, weekly plans in notes/plans/), add quiz and plan-week skills under .claude/skills/, rituals rows, and a Learning paragraph in CLAUDE.md Conventions. Add its INDEX.md line." },
    Module { id: "builds", name: "Projects", name_ar: "المشاريع", default_on: true,
        description: "Link any folder (client files, a report, a code repo) as a project; chats run inside it.",
        description_ar: "اربط أي مجلد (ملفات عميل، تقرير، مشروع برمجي) كمشروع، وستعمل المحادثات داخله.",
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

pub fn add_prompt(m: &Module) -> String {
    format!(
        "The user just enabled the **{}** module in the Thoughts app. Integrate it into this library \
         *as it exists now* — read CLAUDE.md and INDEX.md first and fit the new pieces to the existing \
         structure and conventions rather than pasting a template. Follow the conventions already in CLAUDE.md: \
         items can be ideas or work items (a README's note details carry `kind:` — idea, client, matter, project, or the \
         user's own word), todos are task lines that may end with ` @due(YYYY-MM-DD)`, and everything is written in the \
         user's language while the file names and keys the app reads stay as they are.\n\n{}\n\n\
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
        assert!(docs["descriptionAr"].as_str().unwrap().contains("docs/"));
    }
}
