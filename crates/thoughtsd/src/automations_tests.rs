use super::*;
use crate::chats::{Chat, Msg};
use chrono::TimeZone;

fn now() -> DateTime<Utc> { Utc.with_ymd_and_hms(2026, 11, 17, 12, 0, 0).unwrap() }
fn days_ago(d: i64) -> DateTime<Utc> { now() - Duration::days(d) - Duration::minutes(5) }
fn chat(days: i64, title: &str, first: &str) -> ChatFact {
    ChatFact { kind: "chat".into(), title: title.into(), created: days_ago(days), first: first.into(), in_library: true, private: false }
}
fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("thoughts-automate-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}
fn put(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

// ---------- the summary ----------

#[test]
fn requests_are_recognised_in_both_languages_and_only_as_words() {
    assert_eq!(kinds_of("Write an email to Sara about the invoice"), ["email", "invoice"]);
    assert_eq!(kinds_of("اكتب رسالة إلى سارة بخصوص الفاتورة"), ["email", "invoice"]);
    assert_eq!(kinds_of("أرسل لي ايميل بالبريد"), ["email"]);
    assert_eq!(kinds_of("رد على أحمد"), ["email"]);
    assert_eq!(kinds_of("E-MAIL the team the meeting minutes"), ["email", "meeting"]);
    assert_eq!(kinds_of("لخص محضر الاجتماع وترجمه"), ["meeting", "summary", "translate"]);
    assert_eq!(kinds_of("Update my CV and plan the week"), ["plan", "cv"]);
    // Inside another word it does not count.
    assert!(kinds_of("An explanation of the planet's newsletter, worth a billion").is_empty());
    assert!(kinds_of("Mailbox").contains(&"email"), "a word that starts with it does");
    assert_eq!(kinds_of("Pay the bill"), ["invoice"]);
    assert!(kinds_of("").is_empty());
}

#[test]
fn email_addresses_are_counted_once_and_only_real_ones() {
    let found = addresses_in(["Write to sara@example.com and SARA@example.com", "cc <omar.k+work@najd.co.sa>, then (ali@x.io).", "not@an, @nobody, me@home, a@b.c, x@@y.com"]);
    let mut v: Vec<String> = found.into_iter().collect();
    v.sort();
    assert_eq!(v, ["ali@x.io", "omar.k+work@najd.co.sa", "sara@example.com"]);
    assert!(addresses_in(["لا يوجد عنوان هنا"]).is_empty());
    // In the summary's lines they are masked, nothing else is touched.
    assert_eq!(mask_addresses("Write to sara@example.com, cc <Omar@najd.co.sa>. Thanks! me@home a@b"), "Write to [email address], cc <[email address]>. Thanks! me@home a@b");
    assert_eq!(mask_addresses("(ali@x.io)."), "([email address]).");
    assert_eq!(mask_addresses("اكتب إلى sara@example.com  الآن"), "اكتب إلى [email address]  الآن");
}

#[test]
fn chats_that_touch_a_private_folder_are_left_out_whatever_way_they_do() {
    let root = temp("private");
    let private = vec!["me/finances".to_string(), "me".to_string()];
    let base = |text: &str| {
        let mut c = Chat { title: "Plan".into(), kind: "chat".into(), ..Default::default() };
        c.messages.push(Msg::new("user", text));
        c
    };
    // Named in the first message, as a path.
    for text in ["Look at me/finances/taxes.md", "what is in @me", "check ME/notes", &format!("open {}/me/x.md", root.display())] {
        assert!(chat_fact(&base(text), true, &root, &private).private, "{text}");
    }
    // A plain word is not a folder.
    for text in ["write to me about the meeting", "meet me at the mezzanine"] {
        assert!(!chat_fact(&base(text), true, &root, &private).private, "{text}");
    }
    // @-mentioned in a later message.
    let mut c = base("hello");
    let mut later = Msg::new("user", "and this");
    later.tool = Some(json!({"mentions": ["me/finances/2026.md"]}));
    c.messages.push(later);
    assert!(chat_fact(&c, true, &root, &private).private);
    // The first message is one line, cut at 200 characters; titles are cleaned too.
    let mut long = base(&format!("line one\nline\u{202E} two {}", "x".repeat(300)));
    long.title = "Tab\there".into();
    let f = chat_fact(&long, true, &root, &private);
    assert!(f.first.starts_with("line one line two") && f.first.chars().count() == FIRST_CHARS && f.title == "Tab here", "{f:?}");
    // Only ordinary chats keep a first line.
    let mut wrap = base("secret plan");
    wrap.kind = "wrap".into();
    assert_eq!(chat_fact(&wrap, true, &root, &[]).first, "");
    std::fs::remove_dir_all(root).unwrap();
}

fn email_heavy() -> Facts {
    let mut chats = vec![];
    for d in 0..14 { chats.push(chat(d, &format!("Email {d}"), &format!("write an email to client{d}@example.com about the invoice"))); }
    for d in 0..6 { chats.push(chat(d + 1, "رسالة", "اكتب رسالة إلى سارة sara@example.com")); }
    for d in 0..11 { chats.push(chat(d + 2, "Notes", "tidy my notes")); }
    for d in 0..20 { chats.push(chat(40 + d, "Old email", "reply to Omar")); }
    // Never counted: another library, a private folder, older than 90 days, the future.
    chats.push(ChatFact { in_library: false, ..chat(1, "Other library email", "write an email to boss@other.com") });
    chats.push(ChatFact { private: true, ..chat(1, "Private email", "email hr@secret.com about me/finances") });
    chats.push(chat(91, "Ancient email", "email ancient@example.com"));
    chats.push(ChatFact { created: now() + Duration::days(1), ..chat(0, "Future", "email") });
    chats.push(ChatFact { kind: "wrap".into(), ..chat(3, "Wrap", "") });
    chats.push(ChatFact { kind: "wrap".into(), ..chat(4, "Wrap", "") });
    chats.push(ChatFact { kind: "customize".into(), ..chat(5, "Customise", "") });
    chats.push(ChatFact { kind: "wrap".into(), in_library: false, ..chat(4, "Wrap elsewhere", "") });
    Facts { chats, ..Default::default() }
}

#[test]
fn the_summary_counts_only_this_librarys_recent_unprivate_chats() {
    let s = summarise(&email_heavy(), now());
    assert_eq!((s.chats30, s.chats90), (31, 51));
    let email = &s.kinds[0];
    assert_eq!((email.id, email.n30, email.n90), ("email", 20, 40));
    assert_eq!(s.kinds[1].id, "invoice");
    assert_eq!(s.kinds[1].n30, 14);
    // Openings used three times or more, most used first.
    let o: Vec<(&str, usize)> = s.openings.iter().map(|(k, n)| (k.as_str(), *n)).collect();
    assert_eq!(o, [("reply to omar", 20), ("write an email", 14), ("tidy my notes", 11), ("اكتب رسالة إلى", 6)]);
    // 14 client addresses plus sara's; the other library's and the private one's are not there.
    assert_eq!(s.addresses, 15);
    assert_eq!(s.other, [("customisation chats".to_string(), 1), ("wraps".to_string(), 2)]);
    assert_eq!(s.recent.len(), 51);
    assert!(s.recent.iter().all(|(_, t, _)| !t.contains("Other library") && !t.contains("Private") && !t.contains("Ancient") && t != "Future"));
    assert!(s.recent.windows(2).all(|w| w[0].0 >= w[1].0), "newest first");

    let text = render_summary(&s);
    assert!(text.contains("- 20 of 31 chats in the last 30 days were about writing email (40 of 51 in the last 90 days)."), "{text}");
    assert!(text.contains("15 distinct email addresses were written in their messages (the addresses are not shown here)."));
    assert!(text.contains("Other work in the last 90 days: 1 customisation chats, 2 wraps."));
    // No address is ever named, and nothing of the left-out chats is there.
    assert!(text.contains("- 2026-11-17 · Email 0 — write an email to [email address] about the invoice"), "{text}");
    for gone in ["@example.com", "boss@other.com", "hr@secret.com", "me/finances", "Other library", "Private email", "Ancient"] { assert!(!text.contains(gone), "{gone}"); }
    assert!(text.find("What the chats were about").unwrap() < text.find("Recent chats").unwrap());
}

#[test]
fn the_summary_is_the_same_every_time_and_reads_plainly_with_nothing_in_it() {
    let empty = render_summary(&summarise(&Facts::default(), now()));
    assert!(empty.contains("Chats in this library: 0 in the last 30 days, 0 in the last 90 days.") && empty.contains("People list: none yet.")
        && empty.contains("Routines: none.") && empty.contains("Helpers: none.") && empty.contains("Library files: 0."), "{empty}");
    assert!(!empty.contains("What the chats were about") && !empty.contains("Recent chats"));
    let f = email_heavy();
    assert_eq!(render_summary(&summarise(&f, now())), render_summary(&summarise(&f, now())));
    // The order chats come in does not matter.
    let mut shuffled = f.clone();
    shuffled.chats.reverse();
    assert_eq!(render_summary(&summarise(&f, now())), render_summary(&summarise(&shuffled, now())));
}

#[test]
fn everything_else_is_named_and_counted() {
    let mut files = FileFacts::default();
    files.folders.insert("clients".into(), (120, 30));
    files.folders.insert("".into(), (3, 1));
    files.folders.insert("notes".into(), (40, 0));
    files.kinds.insert("md".into(), 150);
    files.kinds.insert("".into(), 2);
    files.kinds.insert("pdf".into(), 11);
    files.total = 163;
    let f = Facts {
        routines: vec![("Meeting notes".into(), "Turn notes into minutes.".into())],
        helpers: vec![("proofreader".into(), "Checks spelling.".into())],
        tasks: vec![("Weekly review".into(), "every Friday at 16:00".into())],
        addons: vec!["Hours".into()], pages: vec!["Calendar".into()], chips: vec!["Plan my week".into()], hidden: vec!["docs".into()],
        people: Some(1), files,
        todos: TodoFacts { open: 12, done: 40, overdue: 3, repeating: 2, groups: vec!["Work".into(), "Home".into()] },
        usage: Some("- Pages opened: todos ×60".into()), arabic: true, ..Default::default()
    };
    let text = render_summary(&summarise(&f, now()));
    for want in ["App language: Arabic.", "Todos: 12 open (3 overdue, 2 repeating), 40 done. Groups in the general list: Work, Home.",
        "People list: 1 person.", "- Meeting notes — Turn notes into minutes.", "- proofreader — Checks spelling.", "- Weekly review, every Friday at 16:00",
        "Add-ons: Hours.", "Pages added to the app: Calendar.", "Home buttons: Plan my week.", "Hidden from the sidebar: docs.",
        "Library files: 163.", "- clients: 120 files, 30 changed in the last 30 days\n- notes: 40 files", "- (top of the library): 3 files",
        "By type: .md 150, .pdf 11, no ending 2.", "How the app was used in the last 30 days:\n- Pages opened: todos ×60"] {
        assert!(text.contains(want), "missing {want:?} in\n{text}");
    }
    // Codex has no helpers: the summary does not mention them.
    let codex = render_summary(&summarise(&Facts { program: Program::Codex, ..f.clone() }, now()));
    assert!(codex.contains("Assistant program: Codex.") && !codex.contains("Helpers"));
    // Usage signals off: nothing about app usage.
    assert!(!render_summary(&summarise(&Facts { usage: None, ..f }, now())).contains("How the app was used"));
}

#[test]
fn a_long_summary_is_cut_from_the_titles_up_never_the_counts() {
    let mut f = email_heavy();
    for d in 0..2000 { f.chats.push(chat(d % 80, &format!("Chat number {d}"), &"a long first line that says a lot ".repeat(6))); }
    for d in 0..200 { f.routines.push((format!("Routine {d}"), "x".repeat(400))); }
    let text = render_summary(&summarise(&f, now()));
    assert!(text.len() <= SUMMARY_MAX, "{}", text.len());
    assert!(text.contains("were about writing email") && text.contains("Todos:") && text.contains("Library files:"));
    assert!(text.contains("Recent chats"), "titles come last and are cut, not dropped wholesale");
    let shown = text.lines().filter(|l| l.contains("Chat number")).count();
    assert!(shown > 0 && shown < MAX_RECENT, "{shown}");
    // Lists are capped, and each description cut short.
    assert_eq!(text.lines().filter(|l| l.starts_with("- Routine ")).count(), MAX_LIST);
    assert!(text.lines().filter(|l| l.starts_with("- Routine ")).all(|l| l.chars().count() < 200));
    assert_eq!(text, render_summary(&summarise(&f, now())));
}

#[test]
fn the_library_is_counted_without_private_folders_hidden_folders_or_links() {
    let lib = temp("walk");
    put(&lib, "README.md", "x");
    put(&lib, "clients/acme/README.md", "x");
    put(&lib, "clients/acme/contract.pdf", "x");
    put(&lib, "notes/a.md", "x");
    put(&lib, "me/finances/salary.md", "secret");
    put(&lib, ".git/config", "x");
    put(&lib, ".claude/skills/x/SKILL.md", "x");
    put(&lib, "node_modules/p/index.js", "x");
    put(&lib, "Makefile", "x");
    #[cfg(unix)]
    {
        let elsewhere = temp("walk-elsewhere");
        put(&elsewhere, "a.md", "x");
        std::os::unix::fs::symlink(&elsewhere, lib.join("linked")).unwrap();
    }
    let f = walk_files(&lib, &["me/finances".to_string()], std::time::SystemTime::now());
    assert_eq!(f.total, 5, "{f:?}");
    assert_eq!(f.folders.get("clients"), Some(&(2, 2)));
    assert_eq!(f.folders.get(""), Some(&(2, 2)));
    assert!(f.folders.get("me").is_none() && f.folders.get("linked").is_none() && !f.cut);
    assert_eq!((f.kinds.get("md"), f.kinds.get("pdf"), f.kinds.get("")), (Some(&3), Some(&1), Some(&1)));
    // Long ago is not "changed in the last 30 days".
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(40 * 86_400);
    assert_eq!(walk_files(&lib, &[], later).folders.get("clients"), Some(&(2, 0)));
    // People are counted by file name only (a private people folder too), the front page left out.
    assert_eq!(count_people(&lib), None);
    put(&lib, "people/README.md", "# People");
    put(&lib, "people/sara.md", "x");
    put(&lib, "people/omar.md", "x");
    assert_eq!(count_people(&lib), Some(2));
    // Todos: counted, private lists left out.
    put(&lib, "TODO.md", "## Work\n- [ ] Late @due(2026-11-01)\n- [ ] Rent @every(month) @due(2026-12-01)\n- [x] Done\n## Home\n- [ ] Call\n");
    put(&lib, "me/finances/todo.md", "- [ ] Secret\n");
    let t = todo_facts(&lib, &["me/finances".to_string()], NaiveDate::from_ymd_opt(2026, 11, 17).unwrap());
    assert_eq!(t, TodoFacts { open: 3, done: 1, overdue: 1, repeating: 1, groups: vec!["Work".into(), "Home".into()] });
    std::fs::remove_dir_all(lib).unwrap();
}

#[test]
fn schedules_are_put_in_words() {
    use crate::workflows::Trigger;
    let s = |every: &str, weekday: Option<u32>, day: Option<Value>| Trigger::Schedule { every: every.into(), at: "16:00".into(), weekday, day, days: None };
    assert_eq!(trigger_words(&s("week", Some(5), None)), "every Friday at 16:00");
    assert_eq!(trigger_words(&s("day", None, None)), "every day at 16:00");
    assert_eq!(trigger_words(&s("weekdays", None, None)), "on working days at 16:00");
    assert_eq!(trigger_words(&s("month", None, Some(json!("last")))), "on the last day of each month at 16:00");
    assert_eq!(trigger_words(&s("month", None, Some(json!(3)))), "each month on day 3 at 16:00");
    assert_eq!(trigger_words(&Trigger::Event { on: "wrap.done".into() }), "after each wrap");
}

// ---------- the question ----------

#[test]
fn the_question_carries_the_summary_what_exists_and_what_was_turned_down() {
    let p = suggest_prompt("SUMMARY TEXT 14 of 31", &["Invoice tracker".into()], &["Routine: Meeting notes".into(), "Built from an earlier suggestion: Email writing".into()], Program::Claude, false);
    assert!(p.contains("<<<SUMMARY\nSUMMARY TEXT 14 of 31\nSUMMARY>>>"));
    assert!(p.contains("- Invoice tracker") && p.contains("- Routine: Meeting notes") && p.contains("- Built from an earlier suggestion: Email writing"));
    assert!(p.contains("\"agent\" (shown as a Helper)") && p.contains("\"skill\" | \"agent\" | \"integration\" | \"tab\""));
    assert!(p.contains("\"email-writing\"") && p.contains("\"email-account\"") && p.contains("plain English words") && p.contains("lines starting with \"!\""));
    assert!(p.contains("You have no tools"));
    let codex = suggest_prompt("S", &[], &[], Program::Codex, true);
    assert!(!codex.contains("\"agent\" (shown as a Helper)") && !codex.contains("\"agent\" |") && codex.contains("Do not suggest helpers"));
    assert!(codex.contains("plain Arabic words") && codex.contains("(none)"));
}

// ---------- reading the answer ----------

fn item(kind: &str, title: &str) -> Value {
    json!({"kind": kind, "title": title, "what": "Drafts emails for you.", "why": "14 of 31 chats were about email.", "build": "A routine you review.", "request": "Steps…", "template": null, "people": false})
}
fn ctx<'a>(skip: &'a HashSet<String>, routines: &'a [String], helpers: &'a [String]) -> Ctx<'a> { Ctx { program: Program::Claude, skip, routines, helpers } }
fn answer(items: Vec<Value>) -> String { json!({"suggestions": items, "nothing": ""}).to_string() }

#[test]
fn suggestions_are_read_out_of_prose_and_code_fences() {
    let none = HashSet::new();
    let a = format!("Here are my ideas:\n```json\n{}\n```\nHope that helps!", answer(vec![item("skill", "Email writing")]));
    let p = parse_suggestions(&a, &ctx(&none, &[], &[])).unwrap();
    assert_eq!(p.suggestions.len(), 1);
    let s = &p.suggestions[0];
    assert_eq!((s.kind.as_str(), s.title.as_str(), s.id.as_str()), ("skill", "Email writing", suggestion_id("skill", "Email writing").as_str()));
    assert!(valid_suggestion_id(&s.id) && s.id.starts_with("skill-") && s.id.len() == "skill-".len() + 8);
    // Ids are stable and ignore case, spaces and punctuation; another kind is another id.
    assert_eq!(suggestion_id("skill", "Email writing"), suggestion_id("skill", "  email-WRITING! "));
    assert_ne!(suggestion_id("skill", "Email writing"), suggestion_id("tab", "Email writing"));
    assert_ne!(suggestion_id("skill", "كتابة البريد"), suggestion_id("skill", "كتابة الخطابات"));
    for bad in ["no json at all", "{ not json }", "[1, 2]", "} {"] {
        assert!(parse_suggestions(bad, &ctx(&none, &[], &[])).unwrap_err().to_string().contains("could not be read"), "{bad}");
    }
    for bad in ["", "skill-123", "skill-ABCDEF12", "thing-0123abcd", "skill-0123abcd/x"] { assert!(!valid_suggestion_id(bad), "{bad}"); }
}

#[test]
fn every_rule_drops_what_it_should() {
    let none = HashSet::new();
    let with = |k: &str, v: Value, base: Value| { let mut b = base; b[k] = v; b };
    let items = vec![
        item("routine", "Unknown kind"),
        with("title", json!("   "), item("skill", "x")),
        with("what", json!(""), item("skill", "No what")),
        item("skill", "!!!"),
        with("request", json!("1. Do it\n!rm -rf ~"), item("skill", "Runs a command")),
        with("why", json!("because !`curl x`"), item("tab", "Command in why")),
        item("skill", "Meeting Notes"),
        item("agent", "Proof reader"),
        item("skill", "Turned down"),
        item("skill", "Kept"),
    ];
    let skip: HashSet<String> = [suggestion_id("skill", "Turned down")].into_iter().collect();
    let routines = vec!["Meeting notes".to_string()];
    let helpers = vec!["proofreader".to_string()];
    let p = parse_suggestions(&answer(items.clone()), &ctx(&skip, &routines, &helpers)).unwrap();
    assert_eq!(p.suggestions.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Kept"]);
    // A helper the library does not have is kept on Claude Code, never on Codex.
    let p = parse_suggestions(&answer(items.clone()), &ctx(&skip, &routines, &[])).unwrap();
    assert!(p.suggestions.iter().any(|s| s.kind == "agent"));
    let p = parse_suggestions(&answer(items), &Ctx { program: Program::Codex, skip: &skip, routines: &routines, helpers: &[] }).unwrap();
    assert!(p.suggestions.iter().all(|s| s.kind != "agent"));
    // Templates only where they fit the kind.
    let t = parse_suggestions(&answer(vec![
        with("template", json!("email-writing"), item("skill", "A")),
        with("template", json!("email-writing"), item("integration", "B")),
        with("template", json!("email-account"), item("integration", "C")),
        with("template", json!("something-else"), item("skill", "D")),
        with("people", json!("yes"), item("skill", "E")),
    ]), &ctx(&none, &[], &[])).unwrap();
    let got: Vec<(Option<&str>, bool)> = t.suggestions.iter().map(|s| (s.template.as_deref(), s.people)).collect();
    assert_eq!(got, [(Some("email-writing"), false), (None, false), (Some("email-account"), false), (None, false), (None, false)]);
    let p = parse_suggestions(&answer(vec![with("people", json!(true), item("skill", "P"))]), &ctx(&none, &[], &[])).unwrap();
    assert!(p.suggestions[0].people);
}

#[test]
fn text_is_tidied_and_cut_and_the_list_is_short_and_unique() {
    let none = HashSet::new();
    let long = json!({"kind": " Skill ", "title": format!("A\u{202E} title\nover two lines {}", "x".repeat(200)), "what": "w".repeat(900), "why": "y\u{0007}es", "build": "b",
        "request": format!("line 1\r\nline\t2\n{}", "r".repeat(5000))});
    let p = parse_suggestions(&answer(vec![long]), &ctx(&none, &[], &[])).unwrap();
    let s = &p.suggestions[0];
    assert_eq!(s.kind, "skill");
    assert!(s.title.starts_with("A title over two lines") && s.title.chars().count() == 80, "{}", s.title);
    assert_eq!((s.what.chars().count(), s.why.as_str()), (500, "y es"));
    assert!(s.request.starts_with("line 1\nline 2\n") && s.request.chars().count() == 4000);
    // Arabic is cut on a character boundary.
    let ar = parse_suggestions(&answer(vec![json!({"kind": "tab", "title": "ص".repeat(100), "what": "و", "request": "ط".repeat(4100)})]), &ctx(&none, &[], &[])).unwrap();
    assert_eq!((ar.suggestions[0].title.chars().count(), ar.suggestions[0].request.chars().count()), (80, 4000));
    // Duplicates collapse to the first; at most five; `nothing` only when nothing is left.
    let mut many = vec![item("skill", "Email writing"), json!({"kind": "skill", "title": "EMAIL writing", "what": "second"})];
    for i in 0..8 { many.push(item("tab", &format!("Page {i}"))); }
    let p = parse_suggestions(&json!({"suggestions": many, "nothing": "ignored"}).to_string(), &ctx(&none, &[], &[])).unwrap();
    assert_eq!(p.suggestions.len(), MAX_SUGGESTIONS);
    assert_eq!(p.suggestions[0].what, "Drafts emails for you.");
    assert_eq!(p.suggestions.iter().filter(|s| s.kind == "skill").count(), 1);
    assert_eq!(p.nothing, "");
    let n = parse_suggestions(&json!({"suggestions": [], "nothing": format!("Not enough yet.\nUse it more. {}", "z".repeat(400))}).to_string(), &ctx(&none, &[], &[])).unwrap();
    assert!(n.suggestions.is_empty() && n.nothing.starts_with("Not enough yet. Use it more.") && n.nothing.chars().count() == 300);
    let n = parse_suggestions("{\"nothing\": \"\"}", &ctx(&none, &[], &[])).unwrap();
    assert_eq!(n, Parsed::default());
}

// ---------- building ----------

#[test]
fn the_email_routine_is_plain_in_both_languages_and_can_be_kept_as_it_is() {
    for arabic in [false, true] {
        let d = email_writing(arabic);
        let kept = crate::routines::check(&d).unwrap_or_else(|e| panic!("{arabic}: {e}"));
        assert_eq!(kept.title, d.title);
        assert!(!crate::routines::runs_commands(&d.steps) && !crate::routines::runs_commands(&d.description));
        for want in ["people/", "people/README.md", "mailto:", "%20", "%0D%0A", "1,800", "name:", "email:", "role:", "organisation:", "relationship:", "language:", "tone:", "updated:", "summary:"] {
            let want = if arabic && want == "1,800" { "1800" } else { want };
            assert!(d.steps.contains(want), "{arabic}: {want}");
        }
        // What it saves reads back the same.
        assert_eq!(crate::routines::parse("email-writing", &crate::routines::render("email-writing", &d)).0, d);
    }
    let (en, ar) = (email_writing(false), email_writing(true));
    assert_eq!(en.title, "Email writing");
    assert_eq!(ar.title, "كتابة رسائل البريد");
    assert!(en.steps.contains("[Open in Mail](mailto:") && en.steps.contains("Never send anything"));
    assert!(ar.steps.contains("[افتح في البريد](mailto:") && ar.steps.contains("لا ترسل شيئاً أبداً"));
    assert!(en.description.contains("write an email to") && ar.description.contains("اكتب رسالة إلى"));
}

#[test]
fn the_email_account_guide_says_first_that_nothing_needs_connecting() {
    for arabic in [false, true] {
        let g = email_account(arabic);
        assert!(!g.title.is_empty() && !g.intro.is_empty() && (4..=10).contains(&g.steps.len()) && !g.chat_prompt.is_empty());
        assert!(g.steps.iter().all(|s| s.chars().count() <= 300));
        for t in std::iter::once(&g.intro).chain(&g.steps).chain([&g.chat_prompt]) { assert!(!crate::routines::runs_commands(t) && !t.contains(".mcp")); }
    }
    let en = email_account(false);
    assert!(en.intro.starts_with("You do not need this") && en.intro.contains("Open in Mail"));
    assert!(en.chat_prompt.contains("Never ask me for my password") && en.chat_prompt.contains("I will do every sign-in myself"));
    let ar = email_account(true);
    assert!(ar.intro.contains("«افتح في البريد»") && ar.chat_prompt.contains("لا تطلب مني كلمة المرور"));
    // As the guide of a suggestion with that template; serialised the way the UI reads it.
    let s = Suggestion { kind: "integration".into(), title: "Mail".into(), template: Some("email-account".into()), ..Default::default() };
    assert_eq!(guide_for(&s, false), en);
    assert!(serde_json::to_value(&en).unwrap()["chatPrompt"].is_string());
}

#[test]
fn a_connection_without_a_template_gets_its_steps_from_the_request() {
    let s = Suggestion { kind: "integration".into(), title: "Your calendar".into(), what: "See meetings in chats.".into(),
        request: format!("1. Open the settings\n- Choose calendar\n\n  * Sign in yourself\n2) Done\n{}", (0..20).map(|i| format!("step {i} {}\n", "x".repeat(400))).collect::<String>()), ..Default::default() };
    let g = guide_for(&s, false);
    assert_eq!(&g.steps[..4], ["Open the settings", "Choose calendar", "Sign in yourself", "Done"]);
    assert_eq!(g.steps.len(), 10);
    assert!(g.steps.iter().all(|l| l.chars().count() <= 300));
    assert_eq!((g.title.as_str(), g.intro.as_str()), ("Your calendar", "See meetings in chats."));
    assert!(g.chat_prompt.contains("\"Your calendar\"") && g.chat_prompt.contains("Never ask me for my password"));
    assert!(guide_for(&s, true).chat_prompt.contains("«Your calendar»"));
    let bare = guide_for(&Suggestion { request: String::new(), ..s }, false);
    assert_eq!(bare.steps, ["See meetings in chats."]);
}

#[test]
fn a_new_page_is_asked_for_with_its_own_words() {
    let s = Suggestion { kind: "tab".into(), title: "People".into(), what: "A page listing the people you write to.".into(), request: "Show people/README.md as cards.".into(), ..Default::default() };
    let p = tab_prompt(&s);
    assert!(p.contains("\"People\"") && p.contains("A page listing the people you write to.") && p.contains("Show people/README.md as cards.") && p.contains("only the permissions it really needs"));
    assert!(tab_prompt(&Suggestion { request: String::new(), ..s }).contains("Details:\nA page listing"));
}

// ---------- state ----------

fn sug(kind: &str, title: &str) -> Suggestion {
    Suggestion { id: suggestion_id(kind, title), kind: kind.into(), title: title.into(), what: "w".into(), ..Default::default() }
}

#[test]
fn dismissing_restoring_and_building_keep_the_list_straight_and_survive_a_restart() {
    let home = temp("state");
    let lib = "/Users/x/Thoughts";
    let (a, b, c) = (sug("skill", "A"), sug("tab", "B"), sug("integration", "C"));
    change(&home, lib, |l| { l.suggestions = vec![a.clone(), b.clone(), c.clone()]; l.generated_at = Some("2026-11-17T10:00:00Z".into()); }).unwrap();
    // Another library's list is its own.
    change(&home, "/elsewhere", |l| l.suggestions = vec![sug("skill", "Z")]).unwrap();

    let (ok, l) = change(&home, lib, |l| dismiss(l, &b.id)).unwrap();
    assert!(ok && l.suggestions == [a.clone(), c.clone()] && l.dismissed[0].title == "B");
    assert!(!change(&home, lib, |l| dismiss(l, &b.id)).unwrap().0, "not in the list any more");
    // Undo puts it back where it was.
    let (ok, l) = change(&home, lib, |l| restore(l, &b.id)).unwrap();
    assert!(ok && l.suggestions == [a.clone(), b.clone(), c.clone()] && l.dismissed.is_empty());
    assert!(!change(&home, lib, |l| restore(l, &b.id)).unwrap().0);
    // Built: gone for good.
    let (ok, l) = change(&home, lib, |l| mark_built(l, &a.id)).unwrap();
    assert!(ok && l.suggestions == [b.clone(), c.clone()] && l.built.len() == 1 && l.built[0].item.is_none());
    assert!(!change(&home, lib, |l| mark_built(l, &a.id)).unwrap().0);

    // Read back from the file as written.
    let raw: Value = serde_json::from_slice(&std::fs::read(store_file(&home)).unwrap()).unwrap();
    assert_eq!(raw["libraries"][lib]["generatedAt"], "2026-11-17T10:00:00Z");
    assert_eq!(raw["libraries"][lib]["built"][0]["title"], "A");
    assert_eq!(raw["libraries"]["/elsewhere"]["suggestions"][0]["title"], "Z");
    assert_eq!(library_state(&home, lib), l);
    assert_eq!(library_state(&home, "/never"), LibState::default());

    // At most 200 of each are kept: the oldest go.
    change(&home, lib, |l| for i in 0..250 { l.suggestions.push(sug("tab", &format!("T{i}"))); let id = l.suggestions.last().unwrap().id.clone(); dismiss(l, &id); }).unwrap();
    let l = library_state(&home, lib);
    assert_eq!(l.dismissed.len(), MAX_MARKS);
    assert_eq!(l.dismissed[0].title, "T50");
    // A damaged file starts afresh rather than failing.
    std::fs::write(store_file(&home), "{broken").unwrap();
    assert_eq!(load(&home), Store::default());
    std::fs::remove_dir_all(home).unwrap();
}

// ---------- the endpoints, with a stand-in for the assistant ----------

const EMAIL_ANSWER: &str = r#"Sure! {"suggestions": [
  {"kind": "skill", "title": "Email writing", "what": "Drafts your emails in each person's language and tone.", "why": "20 of 31 chats in the last 30 days were about writing email.", "build": "You review a ready-made routine and add the people you write to.", "request": "", "template": "email-writing", "people": true},
  {"kind": "integration", "title": "Your email account", "what": "Lets the assistant look things up in your mailbox.", "why": "15 distinct email addresses.", "build": "A guide you follow yourself.", "request": "", "template": "email-account", "people": false},
  {"kind": "agent", "title": "Proofreader", "what": "Checks your drafts before you send them.", "why": "20 of 31 chats.", "build": "A helper you review.", "request": "Check spelling and tone.", "template": null, "people": false}
], "nothing": ""}"#;

async fn setup() -> (Arc<AppState>, PathBuf, Arc<Mutex<Vec<String>>>) {
    let (st, lib) = crate::wraps::testutil::library().await;
    st.settings.write().await.private_paths = vec!["me/finances".into()];
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let s2 = seen.clone();
    crate::wraps::testutil::summariser(&st, move |prompt, _| {
        s2.lock().unwrap().push(prompt.clone());
        if prompt.contains("BROKEN") { Ok("I could not decide.".into()) } else { Ok(EMAIL_ANSWER.into()) }
    });
    for i in 0..6 {
        let mut c = Chat { title: format!("Email {i}"), kind: "chat".into(), library: Some(lib.to_string_lossy().to_string()), ..Default::default() };
        c.messages.push(Msg::new("user", &format!("write an email to person{i}@example.com about the report")));
        st.chats.insert(c).await;
    }
    let mut secret = Chat { title: "Salary".into(), kind: "chat".into(), library: Some(lib.to_string_lossy().to_string()), ..Default::default() };
    secret.messages.push(Msg::new("user", "summarise me/finances/salary.md and email hr@secret.com"));
    st.chats.insert(secret).await;
    let mut other = Chat { title: "Elsewhere".into(), kind: "chat".into(), library: Some("/some/other/library".into()), ..Default::default() };
    other.messages.push(Msg::new("user", "email boss@other.com"));
    st.chats.insert(other).await;
    crate::wraps::testutil::write(&lib, "me/finances/salary.md", "100");
    (st, lib, seen)
}

async fn changed(rx: &mut tokio::sync::broadcast::Receiver<Value>) {
    tokio::time::timeout(std::time::Duration::from_secs(20), async { loop { if rx.recv().await.unwrap()["type"] == "automations.changed" { return; } } })
        .await.expect("an automations.changed event");
}

/// The message an error answer carries.
async fn err_text(e: crate::api::ApiErr) -> String {
    use axum::response::IntoResponse;
    let bytes = axum::body::to_bytes(e.into_response().into_body(), 1 << 20).await.unwrap();
    serde_json::from_slice::<Value>(&bytes).unwrap()["error"].as_str().unwrap_or_default().to_string()
}

fn cleanup(lib: &Path) { let _ = std::fs::remove_dir_all(lib.parent().unwrap()); }

#[tokio::test]
async fn pressing_suggest_asks_once_with_the_preview_and_lists_the_email_routine_first() {
    let (st, lib, seen) = setup().await;
    let before = api_state(State(st.clone())).await.unwrap().0;
    assert_eq!((before["running"].clone(), before["canSuggest"].clone(), before["blocked"].clone(), before["program"].clone()), (json!(false), json!(true), Value::Null, json!("claude")));
    assert_eq!(before["suggestions"], json!([]));
    let preview = api_preview(State(st.clone())).await.unwrap().0["text"].as_str().unwrap().to_string();
    assert!(preview.contains("6 of 6 chats in the last 30 days were about writing email") && preview.contains("6 distinct email addresses"), "{preview}");
    for gone in ["Salary", "hr@secret.com", "me/finances", "Elsewhere", "boss@other.com", "person0@example.com"] { assert!(!preview.contains(gone), "{gone} in\n{preview}"); }

    let mut rx = st.events.subscribe();
    assert_eq!(api_suggest(State(st.clone())).await.unwrap().0, json!({"running": true}));
    changed(&mut rx).await;
    let prompts = seen.lock().unwrap().clone();
    assert_eq!(prompts.len(), 1, "one question");
    assert!(prompts[0].contains(&preview), "exactly what the preview showed is sent");
    let s = api_state(State(st.clone())).await.unwrap().0;
    assert_eq!((s["running"].clone(), s["error"].clone(), s["canSuggest"].clone()), (json!(false), Value::Null, json!(true)));
    assert!(s["generatedAt"].is_string());
    let list = s["suggestions"].as_array().unwrap();
    assert_eq!(list.iter().map(|x| x["kind"].as_str().unwrap()).collect::<Vec<_>>(), ["skill", "integration", "agent"]);
    assert_eq!((list[0]["title"].clone(), list[0]["template"].clone(), list[0]["people"].clone()), (json!("Email writing"), json!("email-writing"), json!(true)));

    // Build the routine: the app's own template, nothing asked, not yet marked built.
    let id = list[0]["id"].as_str().unwrap().to_string();
    let b = api_build(State(st.clone()), UrlPath(id.clone())).await.unwrap().0;
    assert_eq!((b["kind"].clone(), b["id"].clone(), b["people"].clone(), b["template"].clone()), (json!("skill"), json!(id), json!(true), json!("email-writing")));
    assert_eq!(b["draft"]["title"], "Email writing");
    assert!(b["draft"]["steps"].as_str().unwrap().contains("mailto:"));
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(api_state(State(st.clone())).await.unwrap().0["suggestions"].as_array().unwrap().len(), 3);
    // Kept in the editor: the UI says so and it leaves the list.
    let after = api_built(State(st.clone()), UrlPath(id.clone())).await.unwrap().0;
    assert_eq!(after["suggestions"].as_array().unwrap().len(), 2);
    assert!(api_build(State(st.clone()), UrlPath(id.clone())).await.is_err(), "built is gone");

    // The connection guide; the helper.
    let gid = after["suggestions"][0]["id"].as_str().unwrap().to_string();
    let g = api_build(State(st.clone()), UrlPath(gid)).await.unwrap().0;
    assert_eq!(g["kind"], "integration");
    assert!(g["guide"]["intro"].as_str().unwrap().contains("Open in Mail") && g["guide"]["chatPrompt"].as_str().unwrap().contains("password"));
    let hid = after["suggestions"][1]["id"].as_str().unwrap().to_string();
    let h = api_build(State(st.clone()), UrlPath(hid.clone())).await.unwrap().0;
    assert_eq!((h["kind"].clone(), h["request"].clone()), (json!("agent"), json!("Check spelling and tone.")));

    // Not for me, then Undo.
    let d = api_dismiss(State(st.clone()), UrlPath(hid.clone())).await.unwrap().0;
    assert_eq!(d["suggestions"].as_array().unwrap().len(), 1);
    let r = api_restore(State(st.clone()), UrlPath(hid.clone())).await.unwrap().0;
    assert_eq!(r["suggestions"][1]["id"], json!(hid));
    let _ = api_dismiss(State(st.clone()), UrlPath(hid.clone())).await.unwrap();
    assert!(api_build(State(st.clone()), UrlPath("nonsense".into())).await.is_err());
    assert!(api_dismiss(State(st.clone()), UrlPath("../x".into())).await.is_err());

    // Suggest again: what was built or turned down is named in the question and never comes back.
    seen.lock().unwrap().clear();
    let mut rx = st.events.subscribe();
    let _ = api_suggest(State(st.clone())).await.unwrap();
    changed(&mut rx).await;
    let p = seen.lock().unwrap()[0].clone();
    assert!(p.contains("- Proofreader") && p.contains("Built from an earlier suggestion: Email writing"), "{p}");
    let s = api_state(State(st.clone())).await.unwrap().0;
    assert_eq!(s["suggestions"].as_array().unwrap().iter().map(|x| x["kind"].as_str().unwrap()).collect::<Vec<_>>(), ["integration"]);
    cleanup(&lib);
}

#[tokio::test]
async fn one_run_at_a_time_never_past_a_limit_and_a_bad_answer_is_an_error_to_show() {
    let (st, lib, _) = setup().await;
    // An answer that never comes: a second press is refused while the first one waits.
    *st.complete_hook.lock().unwrap() = Some(Arc::new(|_p: String, _m| Box::pin(futures::future::pending())));
    let _ = api_suggest(State(st.clone())).await.unwrap();
    let s = api_state(State(st.clone())).await.unwrap().0;
    assert_eq!((s["running"].clone(), s["canSuggest"].clone()), (json!(true), json!(false)));
    assert!(api_suggest(State(st.clone())).await.map(|_| ()).map_err(err_text).unwrap_err().await.contains("already being made"));

    // A fresh library: an answer that cannot be read becomes the error shown, and the run ends.
    let (st, lib2, _) = setup().await;
    crate::wraps::testutil::summariser(&st, |_, _| Ok("Sorry, I have no JSON for you.".into()));
    let mut rx = st.events.subscribe();
    let _ = api_suggest(State(st.clone())).await.unwrap();
    changed(&mut rx).await;
    let s = api_state(State(st.clone())).await.unwrap().0;
    assert_eq!(s["running"], false);
    assert!(s["error"].as_str().unwrap().contains("could not be read"));
    assert_eq!(s["suggestions"], json!([]));
    // An assistant that fails: its message is the error.
    crate::wraps::testutil::summariser(&st, |_, _| Err(crate::tr::err("plan.limit_unknown", &[])));
    let mut rx = st.events.subscribe();
    let _ = api_suggest(State(st.clone())).await.unwrap();
    changed(&mut rx).await;
    assert!(api_state(State(st.clone())).await.unwrap().0["error"].as_str().unwrap().contains("usage limit"));
    // Nothing worth suggesting yet.
    crate::wraps::testutil::summariser(&st, |_, _| Ok("{\"suggestions\": [], \"nothing\": \"You have only just started.\"}".into()));
    let mut rx = st.events.subscribe();
    let _ = api_suggest(State(st.clone())).await.unwrap();
    changed(&mut rx).await;
    let s = api_state(State(st.clone())).await.unwrap().0;
    assert_eq!((s["nothing"].clone(), s["error"].clone(), s["suggestions"].clone()), (json!("You have only just started."), Value::Null, json!([])));

    // The month's spending limit reached: refused before anything is asked, and said up front.
    st.settings.write().await.spending.monthly_limit_usd = Some(1.0);
    st.usage.lock().await.insert(chrono::Local::now().format("%Y-%m").to_string(), 5.0);
    assert_eq!(api_state(State(st.clone())).await.unwrap().0["blocked"], "spend");
    assert!(api_suggest(State(st.clone())).await.map(|_| ()).map_err(err_text).unwrap_err().await.contains("spending limit"));
    // No library.
    st.settings.write().await.library_path = None;
    let s = api_state(State(st.clone())).await.unwrap().0;
    assert_eq!((s["blocked"].clone(), s["canSuggest"].clone()), (json!("noLibrary"), json!(false)));
    assert!(api_suggest(State(st.clone())).await.is_err());
    cleanup(&lib);
    cleanup(&lib2);
}

#[tokio::test]
async fn codex_gets_no_helpers_and_a_signed_out_assistant_is_said_up_front() {
    let (st, lib, seen) = setup().await;
    st.settings.write().await.ai.primary = "codex".into();
    let mut rx = st.events.subscribe();
    let _ = api_suggest(State(st.clone())).await.unwrap();
    changed(&mut rx).await;
    assert!(seen.lock().unwrap()[0].contains("Do not suggest helpers"));
    let s = api_state(State(st.clone())).await.unwrap().0;
    assert_eq!(s["program"], "codex");
    assert!(s["suggestions"].as_array().unwrap().iter().all(|x| x["kind"] != "agent"));
    st.codex.auth_failed.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(api_state(State(st.clone())).await.unwrap().0["blocked"], "signedOut");
    cleanup(&lib);
}

#[tokio::test]
async fn helpers_in_the_library_are_named_in_the_summary_and_not_suggested_again() {
    let (st, lib) = crate::wraps::testutil::library().await;
    crate::agents::save(&lib, None, None, &crate::agents::Draft {
        title: "Proof reader".into(), description: "Checks drafts for mistakes.".into(), instructions: "You read the draft.".into(),
    }).unwrap();
    let facts = gather(&st, &lib).await;
    assert_eq!(facts.helpers, [("Proof reader".to_string(), "Checks drafts for mistakes.".to_string())]);
    assert!(render_summary(&summarise(&facts, now())).contains("Proof reader — Checks drafts for mistakes."));
    let helpers: Vec<String> = facts.helpers.iter().map(|(t, _)| t.clone()).collect();
    let none = HashSet::new();
    let p = parse_suggestions(&answer(vec![item("agent", "Proof-reader"), item("agent", "Translator")]), &ctx(&none, &[], &helpers)).unwrap();
    assert_eq!(p.suggestions.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Translator"]);
    cleanup(&lib);
}
