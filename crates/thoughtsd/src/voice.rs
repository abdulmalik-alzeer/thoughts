//! Voice in (FEAT-55): dictation and meeting recordings.
//!
//! The desktop app records and turns speech into text on this computer (macOS's own recognizer,
//! always in its on-device mode; nothing is sent anywhere). This module holds the parts that are
//! plain logic, so they behave the same everywhere and are tested here:
//! * which recognizer language to ask for (`locale_candidates`),
//! * where a meeting recording is saved (`recording_path`),
//! * how the recognizer's running results become text without losing or repeating words (`Merger`),
//! * how a transcript becomes a dated meeting note in the library's conventions (`transcript_note`).

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Sound files a meeting recording can be: what the recorder writes, and what can be transcribed.
pub const AUDIO_EXT: &[&str] = &["m4a", "wav", "mp3", "aac", "caf", "aiff", "aif"];

/// The longest recording: four hours. At the recorder's speech quality that is about 60 MB, well
/// under what backup services take for one file.
pub const MAX_RECORDING_SECS: f64 = 4.0 * 3600.0;

/// A pause this long (seconds) between two words starts a new piece of the transcript.
const PAUSE: f64 = 2.0;

pub fn is_audio(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    AUDIO_EXT.contains(&ext.as_str())
}

/// The two languages the user can speak in: `ar` or `en` (anything else is English).
pub fn lang(l: &str) -> &'static str { if l.trim().to_lowercase().starts_with("ar") { "ar" } else { "en" } }

/// Recognizer locales to try for a language, best first: the user's own variant from the system's
/// preferred languages ("en-GB", "ar-AE"…), then the variants macOS ships most widely.
pub fn locale_candidates(lang_code: &str, preferred: &[String]) -> Vec<String> {
    let want = lang(lang_code);
    let mut out: Vec<String> = Vec::new();
    for p in preferred {
        let parts: Vec<&str> = p.split(['-', '_']).filter(|s| !s.is_empty()).collect();
        if parts.first().map(|l| l.to_lowercase()) != Some(want.to_string()) { continue; }
        // language-REGION only: a script part ("Latn") or extensions are dropped.
        if let Some(region) = parts.iter().skip(1).find(|r| r.len() == 2 && r.chars().all(|c| c.is_ascii_alphabetic())) {
            let l = format!("{want}-{}", region.to_uppercase());
            if !out.contains(&l) { out.push(l); }
        }
    }
    for d in if want == "ar" { &["ar-SA"][..] } else { &["en-US", "en-GB"][..] } {
        if !out.iter().any(|o| o == d) { out.push(d.to_string()); }
    }
    out
}

/// Today on this computer's clock (the date a recording is filed under).
pub fn today() -> chrono::NaiveDate { chrono::Local::now().date_naive() }

fn clean_title(title: &str, arabic_ui: bool) -> String {
    let t: String = title.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
    if t.is_empty() { (if arabic_ui { "اجتماع" } else { "Meeting" }).to_string() } else { t }
}

/// Where a new meeting recording goes: `<item>/meetings/YYYY-MM-DD-short-topic.m4a`, named the way
/// the library names meeting notes (`library::new_meeting`), so the transcript note made from it
/// can carry the same name with `.md`. A name is only used when neither a note nor a recording has
/// it yet (`-2`, `-3`… otherwise). Nothing is created here; the recorder creates the file.
/// Returns the library-relative path and the title the meeting is filed under.
pub fn recording_path(root: &Path, item_rel: &str, title: &str, date: chrono::NaiveDate, arabic_ui: bool) -> anyhow::Result<(String, String)> {
    let dir = crate::library::item_dir(item_rel)?;
    if !crate::library::resolve(root, &format!("{dir}/README.md"))?.is_file() { return Err(crate::tr::err("not_found.item", &[])); }
    let folder = dir.rsplit('/').next().unwrap_or(&dir);
    let names_ar = crate::library::arabic_names(root, folder);
    let title = clean_title(title, arabic_ui);
    let topic = crate::library::name_from_title(&title, names_ar);
    let topic = if topic.is_empty() { (if names_ar { "اجتماع" } else { "meeting" }).to_string() } else { topic };
    // The meetings folder must be inside the library (not a link out of it), whether or not it exists yet.
    crate::library::resolve(root, &format!("{dir}/meetings"))?;
    let stem = format!("{dir}/meetings/{}-{topic}", date.format("%Y-%m-%d"));
    for n in 1..100 {
        let s = if n == 1 { stem.clone() } else { format!("{stem}-{n}") };
        let taken = std::iter::once("md").chain(AUDIO_EXT.iter().copied()).any(|ext| match crate::library::resolve(root, &format!("{s}.{ext}")) {
            Ok(p) => std::fs::symlink_metadata(p).is_ok(),
            Err(_) => true,
        });
        if !taken { return Ok((format!("{s}.m4a"), title)); }
    }
    Err(crate::tr::err("exists", &[("path", &stem)]))
}

// ---------- from the recognizer's results to text ----------

/// One word the recognizer heard, with its time in the recording (seconds). Partial results often
/// carry no times yet (all zero).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Word { pub text: String, pub start: f64, pub dur: f64 }

/// A stretch of speech between two pauses.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Piece { pub start: f64, pub end: f64, pub text: String }

fn timed(words: &[Word]) -> bool { words.iter().any(|w| w.start > 0.0 || w.dur > 0.0) }

/// Split a result's text where the speaker paused for `PAUSE` seconds or more. The text is cut in
/// the recognizer's own formatted text (punctuation, capitals and spacing kept), at the start of the
/// first word after the pause; each word is looked for after the one before it. Without word times
/// the result is one piece, placed at `base` (the end of what came before).
pub fn split_at_pauses(text: &str, words: &[Word], base: f64) -> Vec<Piece> {
    let whole = text.trim();
    if whole.is_empty() { return vec![]; }
    if !timed(words) { return vec![Piece { start: base, end: base, text: whole.to_string() }]; }
    // (byte offset of the word in `text`, word)
    let mut found: Vec<(usize, &Word)> = Vec::new();
    let mut cursor = 0;
    for w in words {
        let needle = w.text.trim();
        if needle.is_empty() { continue; }
        if let Some(i) = text[cursor..].find(needle) {
            found.push((cursor + i, w));
            cursor += i + needle.len();
        }
    }
    if found.is_empty() {
        let start = words.iter().map(|w| w.start).fold(f64::MAX, f64::min);
        let end = words.iter().map(|w| w.start + w.dur).fold(0.0, f64::max);
        return vec![Piece { start, end, text: whole.to_string() }];
    }
    let mut out = Vec::new();
    let (mut from, mut start, mut end) = (0usize, found[0].1.start, found[0].1.start + found[0].1.dur);
    for pair in found.windows(2) {
        let (prev, (at, w)) = (pair[0].1, pair[1]);
        if w.start - (prev.start + prev.dur) >= PAUSE {
            let t = text[from..at].trim();
            if !t.is_empty() { out.push(Piece { start, end, text: t.to_string() }); }
            from = at;
            start = w.start;
        }
        end = end.max(w.start + w.dur);
    }
    let t = text[from..].trim();
    if !t.is_empty() { out.push(Piece { start, end, text: t.to_string() }); }
    out
}

/// Letters and digits only, lower-cased: two recognitions of the same words compare equal even if
/// punctuation or capitals were revised.
fn norm(s: &str) -> String { s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect() }

/// Collects the recognizer's results into one text. The recognizer reports as it goes: partial
/// results for what is being said now (revised again and again), and a finished result when an
/// utterance (or the whole recording) ends. Depending on the macOS version a finished result holds
/// either just the last utterance or everything heard so far; both are handled, so no words are
/// lost or repeated.
#[derive(Clone, Debug, Default)]
pub struct Merger { done: Vec<Piece>, live: Vec<Piece>, live_text: String }

impl Merger {
    pub fn new() -> Self { Self::default() }

    fn done_text(&self) -> String { self.done.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join(" ") }

    /// Does this result repeat what is already done (the recognizer reporting everything so far)?
    fn repeats_done(&self, text: &str) -> bool {
        let done = norm(&self.done_text());
        if done.is_empty() { return false; }
        let now = norm(text);
        let n = done.chars().count().min(40);
        let head: String = done.chars().take(n).collect();
        now.starts_with(&head)
    }

    /// Where the speech done so far ends (seconds; 0 when the recognizer gave no times).
    pub fn heard_until(&self) -> f64 { self.done.iter().chain(self.live.iter()).map(|p| p.end).fold(0.0, f64::max) }

    /// Take one result: its formatted text, its words, and whether it is finished.
    pub fn push(&mut self, text: &str, words: &[Word], finished: bool) {
        let base = self.done.last().map(|p| p.end).unwrap_or(0.0);
        let pieces = split_at_pauses(text, words, base);
        if !finished {
            self.live = pieces;
            self.live_text = text.trim().to_string();
            return;
        }
        self.live.clear();
        self.live_text.clear();
        if pieces.is_empty() { return; }
        let first = pieces[0].start;
        let now = norm(text);
        // A piece is being said again when this result starts with that piece's words.
        let again = |p: &Piece| { let head: String = norm(&p.text).chars().take(20).collect(); !head.is_empty() && now.starts_with(&head) };
        // It covers time already done, and starts with the same words: from there on it replaces
        // what was done, what came before is kept. (The same times with other words are a new
        // utterance whose times count from its own start: it is added.)
        let overlap = if timed(words) { self.done.iter().position(|p| (p.start > 0.0 || p.end > 0.0) && p.end > first + 0.25) } else { None };
        match overlap {
            Some(i) if again(&self.done[i]) => self.done.truncate(i),
            _ if self.repeats_done(text) => self.done.clear(),
            _ => {}
        }
        self.done.extend(pieces);
    }

    /// Everything heard so far, the part still being revised included.
    pub fn pieces(&self) -> Vec<Piece> {
        if self.live.is_empty() { return self.done.clone(); }
        if self.repeats_done(&self.live_text) { return self.live.clone(); }
        self.done.iter().chain(self.live.iter()).cloned().collect()
    }

    /// The whole text, as one run of sentences (for dictation).
    pub fn text(&self) -> String {
        self.pieces().iter().map(|p| p.text.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ")
    }
}

// ---------- the transcript as a meeting note ----------

/// A paragraph of a transcript and where it starts in the recording (seconds).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Para { pub at: f64, pub text: String }

/// Paragraphs from the pieces: a new one after a long pause, or when one has grown long; short
/// bits are kept together. Without times (some recognizers give none) each piece of any length is
/// its own paragraph.
pub fn paragraphs(pieces: &[Piece]) -> Vec<Para> {
    let mut out: Vec<Para> = Vec::new();
    let mut prev_end: Option<f64> = None;
    for p in pieces {
        let text = p.text.trim();
        if text.is_empty() { continue; }
        let timed = p.start > 0.0 || p.end > 0.0;
        let gap = prev_end.map(|e| p.start - e).unwrap_or(0.0);
        let len = out.last().map(|q| q.text.chars().count()).unwrap_or(0);
        let fresh = out.is_empty() || len >= 900 || (timed && (gap >= 6.0 || (gap >= PAUSE && len >= 200))) || (!timed && len >= 80);
        if fresh { out.push(Para { at: p.start.max(0.0), text: text.to_string() }); } else if let Some(q) = out.last_mut() {
            q.text.push(' ');
            q.text.push_str(text);
        }
        prev_end = Some(p.end);
    }
    out
}

/// `05:12` under an hour, `1:05:12` from an hour on.
pub fn clock(secs: f64) -> String {
    let s = if secs.is_finite() && secs > 0.0 { secs.round() as u64 } else { 0 };
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// The transcript's paragraphs, each led by its time in the recording.
pub fn transcript_markdown(paras: &[Para]) -> String {
    paras.iter().filter(|p| !p.text.trim().is_empty())
        .map(|p| format!("**{}** {}", clock(p.at), p.text.split_whitespace().collect::<Vec<_>>().join(" ")))
        .collect::<Vec<_>>().join("\n\n")
}

/// The most a transcript sent to the service may hold.
pub const MAX_TRANSCRIPT_BYTES: usize = 4 << 20;

/// File a transcript as the item's meeting note, beside its recording: `<stem>.md` for
/// `<stem>.m4a` (`<stem>-2.md`… if that is taken; never replaces a file). The note has the meeting
/// note's details and sections (headings in the app's language) for the meeting-notes routine to
/// fill in, then the transcript at the end with a link to the recording. `complete: false` says
/// that part of the recording could not be turned into text. Returns the new note.
#[allow(clippy::too_many_arguments)]
pub fn transcript_note(root: &Path, item_rel: &str, recording_rel: &str, title: &str, date: chrono::NaiveDate, today: chrono::NaiveDate,
    paras: &[Para], seconds: f64, complete: bool, arabic: bool) -> anyhow::Result<String> {
    let dir = crate::library::item_dir(item_rel)?;
    if !crate::library::resolve(root, &format!("{dir}/README.md"))?.is_file() { return Err(crate::tr::err("not_found.item", &[])); }
    let rec = recording_rel.trim().replace('\\', "/");
    let inside = rec.strip_prefix(&format!("{dir}/meetings/")).filter(|r| !r.is_empty() && !r.split('/').any(|p| p.is_empty() || p == "." || p == ".."));
    let Some(inside) = inside else { return Err(crate::tr::err("voice.recording", &[])) };
    if !is_audio(&rec) || !crate::library::resolve(root, &rec)?.is_file() { return Err(crate::tr::err("voice.recording", &[])); }
    let size: usize = paras.iter().map(|p| p.text.len() + 16).sum();
    if size > MAX_TRANSCRIPT_BYTES || paras.len() > 100_000 { return Err(crate::tr::err("voice.too_long", &[])); }

    let title = clean_title(title, arabic);
    let quoted = title.replace('\\', "\\\\").replace('"', "\\\"");
    let file = inside.rsplit('/').next().unwrap_or(inside);
    let (discussed, decisions, actions, heading) = if arabic { ("ما نوقش", "القرارات", "بنود العمل", "التفريغ النصي") } else { ("Discussed", "Decisions", "Action items", "Transcript") };
    let (rec_word, here) = if arabic { ("التسجيل", "حُوِّل إلى نص على هذا الجهاز") } else { ("Recording", "turned into text on this computer") };
    let length = if seconds.is_finite() && seconds > 0.0 { format!(" · {}", clock(seconds)) } else { String::new() };
    let body = transcript_markdown(paras);
    let body = if body.is_empty() {
        (if arabic { "(لم يُتعرَّف على أي كلام في التسجيل.)" } else { "(No speech was recognised in the recording.)" }).to_string()
    } else { body };
    let partial = if complete { "" } else if arabic { "\n\n(توقف التفريغ قبل نهاية التسجيل: تعذّر تحويل جزء منه إلى نص.)" } else { "\n\n(The transcript stops early: part of the recording could not be turned into text.)" };
    let content = format!(
        "---\ntitle: \"{quoted}\"\ndate: {d}\nattendees: []\nupdated: {u}\nsummary: \n---\n\n# {title}\n\n## {discussed}\n\n\n## {decisions}\n\n\n## {actions}\n\n\n## {heading}\n\n{rec_word}: [{file}]({file}){length} · {here}\n\n{body}{partial}\n",
        d = date.format("%Y-%m-%d"), u = today.format("%Y-%m-%d"));
    let stem = rec.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(rec.clone());
    for n in 1..100 {
        let path = if n == 1 { format!("{stem}.md") } else { format!("{stem}-{n}.md") };
        match crate::library::create(root, &path, &content) {
            Ok(_) => return Ok(path),
            Err(e) if crate::tr::classify(&e).code == "exists" => continue,
            Err(e) => return Err(e),
        }
    }
    Err(crate::tr::err("exists", &[("path", &stem)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("thoughts-voice-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("clients/acme")).unwrap();
        std::fs::write(d.join("clients/acme/README.md"), "---\ntitle: Acme\n---\n").unwrap();
        d
    }
    fn w(text: &str, start: f64, dur: f64) -> Word { Word { text: text.into(), start, dur } }
    fn day(s: &str) -> chrono::NaiveDate { chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap() }

    #[test]
    fn the_language_picked_becomes_recognizer_locales_with_the_users_own_variant_first() {
        let pref = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(locale_candidates("en", &pref(&["ar-SA", "en-GB"])), ["en-GB", "en-US"]);
        assert_eq!(locale_candidates("en", &[]), ["en-US", "en-GB"]);
        assert_eq!(locale_candidates("ar", &pref(&["en-US", "ar-AE", "ar_SA"])), ["ar-AE", "ar-SA"]);
        assert_eq!(locale_candidates("ar", &pref(&["ar"])), ["ar-SA"]);
        assert_eq!(locale_candidates("ar-SA", &pref(&["zh-Hans-CN", "en-Latn-SA"])), ["ar-SA"]);
        assert_eq!(locale_candidates("en", &pref(&["en-Latn-SA"])), ["en-SA", "en-US", "en-GB"]);
        // Anything else is English; the app language is the default.
        assert_eq!(lang("fr"), "en");
        assert_eq!(lang("AR"), "ar");
        assert!(is_audio("a/b/x.M4A") && is_audio("x.wav") && !is_audio("x.md") && !is_audio("m4a"));
    }

    #[test]
    fn a_recording_is_named_like_a_meeting_note_and_never_takes_a_name_in_use() {
        let d = lib();
        let (p, title) = recording_path(&d, "clients/acme", "  Weekly   sync ", day("2026-11-12"), false).unwrap();
        assert_eq!((p.as_str(), title.as_str()), ("clients/acme/meetings/2026-11-12-weekly-sync.m4a", "Weekly sync"));
        // A note of that name (made by hand) or a recording (any sound file) takes the name.
        std::fs::create_dir_all(d.join("clients/acme/meetings")).unwrap();
        std::fs::write(d.join("clients/acme/meetings/2026-11-12-weekly-sync.md"), "x").unwrap();
        assert_eq!(recording_path(&d, "clients/acme/README.md", "Weekly sync", day("2026-11-12"), false).unwrap().0, "clients/acme/meetings/2026-11-12-weekly-sync-2.m4a");
        std::fs::write(d.join("clients/acme/meetings/2026-11-12-weekly-sync-2.wav"), "x").unwrap();
        assert_eq!(recording_path(&d, "clients/acme", "Weekly sync", day("2026-11-12"), false).unwrap().0, "clients/acme/meetings/2026-11-12-weekly-sync-3.m4a");
        // No title: "Meeting" in the app's language; the name follows the library's naming.
        assert_eq!(recording_path(&d, "clients/acme", "", day("2026-11-13"), true).unwrap(), ("clients/acme/meetings/2026-11-13-meeting.m4a".into(), "اجتماع".into()));
        std::fs::write(d.join("CLAUDE.md"), "## About this library\n- Names: Arabic\n").unwrap();
        assert_eq!(recording_path(&d, "clients/acme", "مراجعة الحسابات", day("2026-11-13"), true).unwrap().0, "clients/acme/meetings/2026-11-13-مراجعة-الحسابات.m4a");
        assert_eq!(recording_path(&d, "clients/acme", "", day("2026-11-13"), true).unwrap().0, "clients/acme/meetings/2026-11-13-اجتماع.m4a");
        // Only items; never outside the library.
        for bad in ["clients/nobody", "clients", "me/plans", "../x/y", "clients/../.."] {
            assert!(recording_path(&d, bad, "x", day("2026-11-13"), false).is_err(), "{bad}");
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_recording_is_never_saved_through_a_link_that_leads_out_of_the_library() {
        let d = lib();
        let outside = std::env::temp_dir().join(format!("thoughts-voice-out-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, d.join("clients/acme/meetings")).unwrap();
        assert!(recording_path(&d, "clients/acme", "Call", day("2026-11-12"), false).is_err());
        // Nor is a transcript filed through it.
        std::fs::write(outside.join("2026-11-12-call.m4a"), "x").unwrap();
        assert!(transcript_note(&d, "clients/acme", "clients/acme/meetings/2026-11-12-call.m4a", "Call", day("2026-11-12"), day("2026-11-12"), &[], 1.0, true, false).is_err());
        assert!(!outside.join("2026-11-12-call.md").exists());
        std::fs::remove_dir_all(&d).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn a_result_is_cut_where_the_speaker_paused_keeping_the_recognizers_punctuation() {
        let words = [w("Hello", 0.5, 0.4), w("everyone.", 1.0, 0.5), w("Next,", 5.0, 0.4), w("the", 5.5, 0.2), w("budget.", 5.75, 0.5)];
        let p = split_at_pauses("Hello everyone. Next, the budget.", &words, 0.0);
        assert_eq!(p, vec![Piece { start: 0.5, end: 1.5, text: "Hello everyone.".into() }, Piece { start: 5.0, end: 6.25, text: "Next, the budget.".into() }]);
        // No times yet (a partial result): one piece where the last one ended.
        assert_eq!(split_at_pauses(" so the plan ", &[w("so", 0.0, 0.0)], 7.5), vec![Piece { start: 7.5, end: 7.5, text: "so the plan".into() }]);
        assert!(split_at_pauses("  ", &words, 0.0).is_empty());
        // Arabic, with a word the text spells differently: the cut still lands between words.
        let ar = [w("مرحبا", 0.2, 0.5), w("بكم", 0.8, 0.3), w("ننتقل", 4.0, 0.4), w("الميزانيه", 4.5, 0.6)];
        let p = split_at_pauses("مرحبا بكم. ننتقل إلى الميزانية.", &ar, 0.0);
        assert_eq!(p.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), ["مرحبا بكم.", "ننتقل إلى الميزانية."]);
    }

    #[test]
    fn utterances_reported_one_by_one_are_joined_without_losing_or_repeating_words() {
        // Newer macOS: each finished utterance holds only itself; partial results in between.
        let mut m = Merger::new();
        m.push("Hello", &[w("Hello", 0.0, 0.0)], false);
        assert_eq!(m.text(), "Hello");
        m.push("Hello everyone.", &[w("Hello", 0.5, 0.4), w("everyone.", 1.0, 0.5)], true);
        m.push("Next", &[w("Next", 0.0, 0.0)], false);
        assert_eq!(m.text(), "Hello everyone. Next");
        m.push("Next, the budget.", &[w("Next,", 5.0, 0.4), w("the", 5.5, 0.2), w("budget.", 5.75, 0.5)], true);
        assert_eq!(m.text(), "Hello everyone. Next, the budget.");
        assert_eq!(m.heard_until(), 6.25);
        assert_eq!(m.pieces().len(), 2);
    }

    #[test]
    fn results_that_repeat_everything_so_far_replace_it() {
        // Older macOS: every result holds the whole recording so far, revised.
        let mut m = Merger::new();
        m.push("Hello every one", &[], false);
        m.push("Hello everyone.", &[w("Hello", 0.5, 0.4), w("everyone.", 1.0, 0.5)], true);
        m.push("hello everyone next the", &[], false);
        assert_eq!(m.text(), "hello everyone next the", "a partial that repeats the start is not shown twice");
        m.push("Hello everyone. Next, the budget.", &[w("Hello", 0.5, 0.4), w("everyone.", 1.0, 0.5), w("Next,", 5.0, 0.4), w("the", 5.5, 0.2), w("budget.", 5.75, 0.5)], true);
        assert_eq!(m.text(), "Hello everyone. Next, the budget.");
        // The same without any times: recognised by its words.
        let mut m = Merger::new();
        m.push("Good morning.", &[], true);
        m.push("Good morning. Let's start.", &[], true);
        assert_eq!(m.text(), "Good morning. Let's start.");
        m.push("Any questions?", &[], true);
        assert_eq!(m.text(), "Good morning. Let's start. Any questions?");
        // A finished result that covers time already done replaces that part only.
        let mut m = Merger::new();
        m.push("One.", &[w("One.", 1.0, 0.5)], true);
        m.push("Two.", &[w("Two.", 5.0, 0.5)], true);
        m.push("Two, three.", &[w("Two,", 5.0, 0.5), w("three.", 5.6, 0.5)], true);
        assert_eq!(m.text(), "One. Two, three.");
        // An empty finished result (nothing said) changes nothing.
        m.push("", &[], true);
        assert_eq!(m.text(), "One. Two, three.");
        // An utterance whose times count from its own start is new speech, not a revision.
        let mut m = Merger::new();
        m.push("Hello everyone.", &[w("Hello", 0.5, 0.4), w("everyone.", 1.0, 0.5)], true);
        m.push("Next, the budget.", &[w("Next,", 0.3, 0.4), w("the", 0.8, 0.2), w("budget.", 1.1, 0.6)], true);
        assert_eq!(m.text(), "Hello everyone. Next, the budget.");
    }

    #[test]
    fn a_transcript_becomes_paragraphs_led_by_their_time() {
        let p = |start: f64, end: f64, text: &str| Piece { start, end, text: text.into() };
        let long = "word ".repeat(50);
        let paras = paragraphs(&[p(0.5, 2.0, "Hello everyone."), p(3.0, 4.0, "Short."), p(11.0, 20.0, long.trim()), p(23.0, 24.0, "After a pause."), p(3725.0, 3730.0, "An hour in.")]);
        assert_eq!(paras.iter().map(|x| x.at).collect::<Vec<_>>(), [0.5, 11.0, 23.0, 3725.0]);
        assert_eq!(paras[0].text, "Hello everyone. Short.");
        let md = transcript_markdown(&paras);
        assert!(md.starts_with("**00:01** Hello everyone. Short.\n\n**00:11** word word"), "{md}");
        assert!(md.ends_with("\n\n**1:02:05** An hour in."), "{md}");
        assert_eq!((clock(0.0), clock(59.4), clock(-3.0), clock(f64::NAN), clock(3600.0)), ("00:00".into(), "00:59".into(), "00:00".into(), "00:00".into(), "1:00:00".into()));
        // Without times, each sizeable piece is its own paragraph.
        let paras = paragraphs(&[p(0.0, 0.0, &"a ".repeat(50)), p(0.0, 0.0, "b."), p(0.0, 0.0, "c.")]);
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[1].text, "b. c.");
    }

    #[test]
    fn a_transcript_is_filed_as_the_meeting_note_beside_its_recording() {
        let d = lib();
        std::fs::create_dir_all(d.join("clients/acme/meetings")).unwrap();
        std::fs::write(d.join("clients/acme/meetings/2026-11-12-weekly-sync.m4a"), "audio").unwrap();
        let paras = [Para { at: 0.0, text: "Hello everyone.".into() }, Para { at: 75.0, text: "The budget is \"agreed\".".into() }];
        let p = transcript_note(&d, "clients/acme", "clients/acme/meetings/2026-11-12-weekly-sync.m4a", "Weekly \"sync\"", day("2026-11-12"), day("2026-11-13"), &paras, 2531.0, true, false).unwrap();
        assert_eq!(p, "clients/acme/meetings/2026-11-12-weekly-sync.md");
        let text = std::fs::read_to_string(d.join(&p)).unwrap();
        assert!(text.starts_with("---\ntitle: \"Weekly \\\"sync\\\"\"\ndate: 2026-11-12\nattendees: []\nupdated: 2026-11-13\nsummary: \n---\n\n# Weekly \"sync\"\n\n## Discussed\n\n\n## Decisions\n\n\n## Action items\n\n\n## Transcript\n\n"), "{text}");
        assert!(text.contains("Recording: [2026-11-12-weekly-sync.m4a](2026-11-12-weekly-sync.m4a) · 42:11 · turned into text on this computer\n\n**00:00** Hello everyone.\n\n**01:15** The budget is \"agreed\".\n"), "{text}");
        // The item's page lists it with its details, like any meeting note.
        let v = crate::library::item(&d, "clients/acme", &[]).unwrap();
        assert!(v["meetings"].as_array().unwrap().iter().any(|m| m["path"] == p && m["title"] == "Weekly \"sync\"" && m["date"] == "2026-11-12"));
        // Again: never replaces the note.
        let p2 = transcript_note(&d, "clients/acme", "clients/acme/meetings/2026-11-12-weekly-sync.m4a", "", day("2026-11-12"), day("2026-11-12"), &[], 0.0, false, true).unwrap();
        assert_eq!(p2, "clients/acme/meetings/2026-11-12-weekly-sync-2.md");
        let ar = std::fs::read_to_string(d.join(&p2)).unwrap();
        assert!(ar.contains("title: \"اجتماع\"") && ar.contains("## ما نوقش") && ar.contains("## التفريغ النصي\n\nالتسجيل: [2026-11-12-weekly-sync.m4a](2026-11-12-weekly-sync.m4a) · حُوِّل إلى نص على هذا الجهاز\n\n(لم يُتعرَّف على أي كلام في التسجيل.)\n\n(توقف التفريغ"), "{ar}");
        assert_eq!(std::fs::read_to_string(d.join(&p)).unwrap(), text);
        // Only a sound file in this item's meetings folder.
        for bad in ["clients/acme/meetings/none.m4a", "clients/acme/README.md", "clients/acme/notes/x.m4a", "clients/other/meetings/2026-11-12-weekly-sync.m4a",
            "clients/acme/meetings/../meetings/2026-11-12-weekly-sync.m4a", "clients/acme/meetings/", "/etc/x.m4a"] {
            assert!(transcript_note(&d, "clients/acme", bad, "x", day("2026-11-12"), day("2026-11-12"), &[], 0.0, true, false).is_err(), "{bad}");
        }
        assert!(transcript_note(&d, "clients/nobody", "clients/nobody/meetings/a.m4a", "x", day("2026-11-12"), day("2026-11-12"), &[], 0.0, true, false).is_err());
        let huge = [Para { at: 0.0, text: "x".repeat(MAX_TRANSCRIPT_BYTES) }];
        assert!(transcript_note(&d, "clients/acme", "clients/acme/meetings/2026-11-12-weekly-sync.m4a", "x", day("2026-11-12"), day("2026-11-12"), &huge, 0.0, true, false).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
