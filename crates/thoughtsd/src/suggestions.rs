//! Suggestions: a few plain heuristics over the local usage log that propose customisations
//! ("software that grows with you"). At most two at a time; each can be dismissed forever.
//! Accepting one simply starts a customise run with its ready-made prompt.

use chrono::{DateTime, Duration, Local, NaiveDate};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const MAX_AT_ONCE: usize = 2;

/// Modules that have a page of their own in the sidebar: (module id, view id, English, Arabic).
pub const MODULE_VIEWS: &[(&str, &str, &str, &str)] = &[
    ("todos", "todos", "Todos", "المهام"),
    ("docs", "docs", "Docs", "المستندات"),
    ("builds", "projects", "Projects", "المشاريع"),
];

pub struct Input<'a> {
    pub events: &'a [(DateTime<Local>, String)],
    pub now: DateTime<Local>,
    /// Open todos whose due date has passed.
    pub overdue: usize,
    /// View ids (see `MODULE_VIEWS`) of the modules that are switched on.
    pub enabled_views: &'a [&'a str],
    /// Prompts of the Home chips the user already has.
    pub chips: &'a [String],
    pub has_weekly_workflow: bool,
    pub dismissed: &'a [String],
    pub arabic: bool,
    /// Does the layer already hide this page in `nav.json`? (view ids)
    pub nav_hidden: &'a [String],
}

fn file(home: &Path) -> PathBuf { home.join("suggestions.json") }

pub fn dismissed(home: &Path) -> Vec<String> {
    std::fs::read_to_string(file(home)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["dismissed"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()))
        .unwrap_or_default()
}

pub fn dismiss(home: &Path, id: &str) -> anyhow::Result<()> {
    if id.is_empty() || id.len() > 80 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { anyhow::bail!("unknown suggestion"); }
    let mut d = dismissed(home);
    if !d.iter().any(|x| x == id) { d.push(id.to_string()); }
    crate::paths::write_atomic(&file(home), serde_json::to_string_pretty(&json!({"dismissed": d}))?.as_bytes(), true)?;
    Ok(())
}

/// The first three words of what the user typed on Home — the part that repeats.
pub fn prompt_prefix(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(3).collect();
    words.join(" ").chars().take(40).collect()
}

fn short_hash(s: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(s.as_bytes())).chars().take(8).collect()
}

fn item(id: &str, arabic: bool, en: (&str, &str), ar: (&str, &str), prompt: String) -> Value {
    let (title, body) = if arabic { ar } else { en };
    json!({"id": id, "title": title, "body": body, "prompt": prompt})
}

/// Open todos in the library whose due date is before `today`.
pub fn overdue_todos(library: &Path, today: NaiveDate) -> usize {
    crate::todos::list(library).iter().flat_map(|f| f["items"].as_array().cloned().unwrap_or_default())
        .filter(|i| i["done"] == false)
        .filter(|i| i["due"].as_str().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).map_or(false, |d| d < today))
        .count()
}

/// Every suggestion that currently applies and has not been dismissed, most useful first (at most two).
pub fn compute(i: &Input) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    let since = |days: i64| i.now - Duration::days(days);
    let recent = |days: i64| i.events.iter().filter(move |(t, _)| *t > since(days) && *t <= i.now);

    // (a) Looks at Todos most days and things are slipping.
    let todo_days: BTreeSet<NaiveDate> = recent(7).filter(|(_, e)| e == "view.todos").map(|(t, _)| t.date_naive()).collect();
    if todo_days.len() >= 5 && i.overdue >= 3 {
        out.push(item("due-on-home", i.arabic,
            ("Show what's due on your Home screen", "You open Todos almost every day and a few items are overdue. A small box on Home can show them the moment you open the app."),
            ("اعرض المهام المستحقة في الصفحة الرئيسية", "تفتح المهام كل يوم تقريباً وهناك مهام فات موعدها. يمكن لمربع صغير في الصفحة الرئيسية أن يعرضها لك فور فتح التطبيق."),
            "Add a Home widget that lists my overdue todos (open todos whose @due date has passed), oldest first, at most 7, each showing its title and due date, with a link to the Todos page. Use a plugin with only the todos:read permission.".into()));
    }

    // (b) Types the same opening on Home again and again. Only what was typed counts: pressing a
    // button the user already has (`home.chip:`) is no reason to offer that button.
    let mut counts: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for (_, e) in recent(14) {
        let Some(text) = e.strip_prefix("home.prompt:") else { continue };
        let prefix = prompt_prefix(text);
        if prefix.chars().count() < 3 { continue; }
        let entry = counts.entry(prefix.to_lowercase()).or_insert((0, prefix));
        entry.0 += 1;
    }
    let mut repeated: Vec<(usize, String, String)> = counts.into_iter().filter(|(_, (n, _))| *n >= 5).map(|(k, (n, shown))| (n, k, shown)).collect();
    repeated.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, key, shown) in repeated {
        if i.chips.iter().any(|c| c.to_lowercase().starts_with(&key)) { continue; }
        let id = format!("chip-{}", short_hash(&key));
        let (title, body) = if i.arabic {
            (format!("أضف زراً بنقرة واحدة لعبارة «{shown}»"), "تبدأ كثيراً من طلباتك في الصفحة الرئيسية بالعبارة نفسها. زرّ جاهز يوفّر عليك كتابتها كل مرة.".to_string())
        } else {
            (format!("Add a one-click button for '{shown}'"), "You often start a request on Home with the same words. A ready button saves you typing them each time.".to_string())
        };
        out.push(json!({"id": id, "title": title, "body": body,
            "prompt": format!("Add a chip to my Home screen (home.json `chips`) labelled \"{shown}\" whose prompt starts with \"{shown} \" so I can finish the sentence. Keep my existing chips.")}));
    }

    // (c) A page that has not been opened in three weeks (only once the log itself goes back that far).
    let log_is_old_enough = i.events.first().map_or(false, |(t, _)| *t <= since(21));
    if log_is_old_enough {
        for (_, view, en, ar) in MODULE_VIEWS {
            if !i.enabled_views.contains(view) { continue; }
            let key = format!("view.{view}");
            if recent(21).any(|(_, e)| *e == key) { continue; }
            if i.nav_hidden.iter().any(|h| h == view) { continue; }
            let id = format!("hide-{view}");
            let (title, body) = if i.arabic {
                (format!("أخفِ «{ar}» من الشريط الجانبي"), format!("لم تفتح «{ar}» منذ ثلاثة أسابيع. إخفاؤها يجعل الشريط الجانبي أبسط، ويمكنك إعادتها متى شئت."))
            } else {
                (format!("Hide {en} from the sidebar"), format!("You have not opened {en} in three weeks. Hiding it keeps the sidebar simpler, and you can bring it back any time."))
            };
            out.push(json!({"id": id, "title": title, "body": body,
                "prompt": format!("I have not used the {en} page in weeks. Hide it from the sidebar by adding \"{view}\" to `hidden` in nav.json (create the file if it is missing and keep whatever is already in it; do not use CSS for this). If Home shows a matching section, add it to `hidden` in home.json too. Do not delete any of my data. In the summary, tell me how to bring it back (History in Settings, or by asking you to show it again).")}));
        }
    }

    // (d) Wraps regularly: a weekly review would fit.
    let wraps = |from: i64, to: i64| i.events.iter().filter(|(t, e)| e == "wrap.done" && *t > since(from) && *t <= since(to)).count();
    if !i.has_weekly_workflow && wraps(7, 0) >= 3 && wraps(14, 7) >= 3 {
        // The working week ends on Thursday where Arabic is the app's language, on Friday elsewhere.
        let (day_en, at) = if i.arabic { ("Thursday", "15:00") } else { ("Friday", "16:00") };
        out.push(item("weekly-review", i.arabic,
            ("Add a weekly review every Friday", "You wrap up your chats regularly. An automatic task can gather what you decided, what is still open and what is due next week, every Friday."),
            ("أضف ملخص الأسبوع مهمةً تلقائية كل خميس", "أنت تحفظ خلاصة محادثاتك بانتظام. يمكن لملخص أسبوعي أن يجمع لك ما قرّرته وما بقي مفتوحاً وما يحين موعده في الأسبوع القادم، تلقائياً."),
            format!("Add a workflow that runs every {day_en} at {at} and writes a short weekly review into my library: what I decided this week, what is still open, and the todos due next week. Put it where my library's conventions say reviews or logs belong.")));
    }

    // (e) Once a month, when the app is really in use: one tune-up proposal based on how it was used.
    let month_events = recent(30).count();
    let log_two_weeks = i.events.first().map_or(false, |(t, _)| *t <= since(14));
    if month_events >= 30 && log_two_weeks {
        let id = format!("tuneup-{}", i.now.format("%Y-%m"));
        let summary = crate::usage::summary(i.events, i.now);
        out.push(item(&id, i.arabic,
            ("Monthly app tune-up", "Let the assistant look at a short summary of how you used the app this month (pages opened, buttons used, changes you kept or discarded — no note contents) and propose one improvement. You review it before anything changes."),
            ("ضبط شهري للتطبيق", "دع المساعد يطّلع على ملخص قصير لطريقة استخدامك للتطبيق هذا الشهر (الصفحات التي فتحتها، والأزرار التي استخدمتها، والتغييرات التي أبقيتها أو تجاهلتها، دون محتوى ملاحظاتك) ليقترح تحسيناً واحداً. ستراجعه قبل أن يتغيّر أي شيء."),
            format!("This is my monthly app tune-up. Below is a summary of how I used the Thoughts app in the last 30 days; it was gathered on my computer.\n\n{summary}\n\n\
                Propose and prepare exactly ONE small customisation that would make the app fit how I actually work: for example a Home button for something I type often, hiding a page I never open (nav.json), a different todo grouping (todos.json), a Home widget, or a workflow. \
                Learn from what I kept and what I discarded or undid: do not repeat a discarded idea. If nothing is clearly worth changing, change nothing and say so in plain words. \
                In the summary, say which part of my usage led you to this change.")));
    }

    out.retain(|s| !i.dismissed.iter().any(|d| s["id"] == d.as_str()));
    out.truncate(MAX_AT_ONCE);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Local> { Local.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).unwrap() }
    fn ago(days: i64, e: &str) -> (DateTime<Local>, String) { (now() - Duration::days(days) - Duration::minutes(5), e.to_string()) }
    fn input<'a>(events: &'a [(DateTime<Local>, String)]) -> Input<'a> {
        Input { events, now: now(), overdue: 0, enabled_views: &[], chips: &[], has_weekly_workflow: false, dismissed: &[], arabic: false, nav_hidden: &[] }
    }
    fn ids(v: &[Value]) -> Vec<String> { v.iter().map(|s| s["id"].as_str().unwrap().to_string()).collect() }

    #[test]
    fn due_on_home_needs_habit_and_overdue() {
        let five: Vec<_> = (0..5).map(|d| ago(d, "view.todos")).collect();
        assert_eq!(ids(&compute(&Input { overdue: 3, ..input(&five) })), ["due-on-home"]);
        assert!(compute(&Input { overdue: 2, ..input(&five) }).is_empty());
        let same_day: Vec<_> = (0..9).map(|_| ago(0, "view.todos")).collect();
        assert!(compute(&Input { overdue: 9, ..input(&same_day) }).is_empty(), "five visits in one day is not five days");
        let d = ["due-on-home".to_string()];
        assert!(compute(&Input { overdue: 3, dismissed: &d, ..input(&five) }).is_empty());
        let ar = compute(&Input { overdue: 3, arabic: true, ..input(&five) });
        assert!(ar[0]["title"].as_str().unwrap().contains("الصفحة الرئيسية") && ar[0]["prompt"].as_str().unwrap().contains("todos:read"));
    }

    #[test]
    fn repeated_home_prompt_becomes_a_chip() {
        let mut ev: Vec<_> = (0..5).map(|d| ago(d, &format!("home.prompt:Draft an invoice for client {d}"))).collect();
        ev.push(ago(1, "home.prompt:Something else entirely"));
        ev.sort_by_key(|e| e.0);
        let s = compute(&input(&ev));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0]["title"], "Add a one-click button for 'Draft an invoice'");
        assert!(s[0]["id"].as_str().unwrap().starts_with("chip-"));
        // Already a chip, or too long ago: nothing.
        let chips = ["draft an invoice for ".to_string()];
        assert!(compute(&Input { chips: &chips, ..input(&ev) }).is_empty());
        let old: Vec<_> = (15..20).map(|d| ago(d, "home.prompt:Draft an invoice now")).collect();
        assert!(compute(&input(&old)).is_empty());
        // Using buttons that already exist, built-in or the user's own, suggests nothing.
        let taps: Vec<_> = (0..6).flat_map(|d| [ago(d, "home.chip:todo"), ago(d, "home.chip:new idea"), ago(d, "home.chip:check-in"), ago(d, "home.chip:Plan my week")]).collect();
        assert!(compute(&input(&taps)).is_empty());
    }

    #[test]
    fn unused_module_is_offered_for_hiding_only_with_enough_history() {
        let ev = vec![ago(30, "view.home"), ago(2, "view.todos")];
        let s = compute(&Input { enabled_views: &["todos", "docs"], ..input(&ev) });
        assert_eq!(ids(&s), ["hide-docs"]);
        assert_eq!(s[0]["title"], "Hide Docs from the sidebar");
        let prompt = s[0]["prompt"].as_str().unwrap();
        assert!(prompt.contains("\"docs\" to `hidden` in nav.json") && !prompt.contains("custom.css"), "uses the supported setting, not a CSS trick");
        let hidden = ["docs".to_string()];
        assert!(compute(&Input { enabled_views: &["todos", "docs"], nav_hidden: &hidden, ..input(&ev) }).is_empty(), "already hidden");
        assert!(compute(&Input { enabled_views: &["todos"], ..input(&ev) }).is_empty(), "docs is switched off");
        let young = vec![ago(3, "view.home")];
        assert!(compute(&Input { enabled_views: &["docs"], ..input(&young) }).is_empty(), "a new install has no history yet");
    }

    #[test]
    fn regular_wraps_suggest_a_weekly_review_and_only_two_show() {
        let mut ev: Vec<_> = [1, 3, 5, 8, 10, 12].iter().map(|d| ago(*d, "wrap.done")).collect();
        ev.sort_by_key(|e| e.0);
        assert_eq!(ids(&compute(&input(&ev))), ["weekly-review"]);
        assert!(compute(&Input { has_weekly_workflow: true, ..input(&ev) }).is_empty());
        let en = compute(&input(&ev));
        assert!(en[0]["title"].as_str().unwrap().contains("Friday") && en[0]["prompt"].as_str().unwrap().contains("every Friday at 16:00"));
        let ar = compute(&Input { arabic: true, ..input(&ev) });
        assert!(ar[0]["title"].as_str().unwrap().contains("خميس") && ar[0]["prompt"].as_str().unwrap().contains("every Thursday at 15:00"));
        let one_week: Vec<_> = [1, 2, 3].iter().map(|d| ago(*d, "wrap.done")).collect();
        assert!(compute(&input(&one_week)).is_empty());
        // Everything at once: capped at two, in priority order.
        let mut all = ev.clone();
        all.push(ago(40, "view.home"));
        all.extend((0..5).map(|d| ago(d, "view.todos")));
        all.extend((0..5).map(|d| ago(d, "home.prompt:New idea: something")));
        all.sort_by_key(|e| e.0);
        let s = compute(&Input { overdue: 4, enabled_views: &["docs"], ..input(&all) });
        assert_eq!(s.len(), MAX_AT_ONCE);
        assert_eq!(s[0]["id"], "due-on-home");
    }

    #[test]
    fn monthly_tune_up_carries_a_local_usage_summary() {
        let mut ev: Vec<_> = (0..20).flat_map(|d| [ago(d, "view.todos"), ago(d, "view.todos"), ago(d, "view.todos"), ago(d, "view.docs")]).collect();
        ev.push(ago(3, "customize.keep:Show overdue todos on Home"));
        ev.push(ago(2, "customize.discard:Make everything pink"));
        ev.sort_by_key(|e| e.0);
        let s = compute(&input(&ev));
        assert_eq!(ids(&s), ["tuneup-2026-11"]);
        let prompt = s[0]["prompt"].as_str().unwrap();
        assert!(prompt.contains("todos ×60, docs ×20") && prompt.contains("\"Make everything pink\"") && prompt.contains("exactly ONE"));
        assert_eq!(compute(&Input { arabic: true, ..input(&ev) })[0]["title"], "ضبط شهري للتطبيق");
        // Dismissed for this month only; quiet installs and brand-new ones are left alone.
        let d = ["tuneup-2026-11".to_string()];
        assert!(compute(&Input { dismissed: &d, ..input(&ev) }).is_empty());
        let next_month = Input { now: now() + Duration::days(21), dismissed: &d, ..input(&ev) };
        assert_eq!(ids(&compute(&next_month)), ["tuneup-2026-12"]);
        assert!(compute(&input(&ev[ev.len() - 10..])).is_empty(), "too little activity");
        let young: Vec<_> = (0..40).map(|_| ago(1, "view.todos")).collect();
        assert!(compute(&input(&young)).is_empty(), "the log is younger than two weeks");
    }

    #[test]
    fn dismissals_persist_and_overdue_reads_the_library() {
        let d = std::env::temp_dir().join(format!("thoughts-sugg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(dismissed(&d).is_empty());
        dismiss(&d, "weekly-review").unwrap();
        dismiss(&d, "weekly-review").unwrap();
        dismiss(&d, "chip-0a1b2c3d").unwrap();
        dismiss(&d, "tuneup-2026-11").unwrap();
        assert!(dismiss(&d, "../x").is_err());
        assert_eq!(dismissed(&d), ["weekly-review", "chip-0a1b2c3d", "tuneup-2026-11"]);
        std::fs::write(d.join("TODO.md"), "- [ ] Late @due(2026-11-01)\n- [x] Done late @due(2026-11-01)\n- [ ] Today @due(2026-11-10)\n- [ ] No date\n- [ ] Also late @due(2025-01-01)\n").unwrap();
        assert_eq!(overdue_todos(&d, NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()), 2);
        std::fs::remove_dir_all(d).unwrap();
    }
}
