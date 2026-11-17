//! `docs/GLOSSARY.md`: Arabic copy puts tanween fath on the alif («تلقائياً», not «تلقائيًا»), and
//! with a shadda the tanween still comes last («نصّاً», not «نصًّا»). These tests read the app's own
//! source, so a new string in the other spelling fails here: the screen strings (everything under
//! `ui/src`, where the dictionaries in `ui/src/lib/i18n` live) and the daemon's own messages.

use std::path::{Path, PathBuf};

const FATHATAN: char = '\u{064B}';
const SHADDA: char = '\u{0651}';
const ALIF: char = '\u{0627}';

/// Places that hold the other spelling on purpose: what people type is recognised in both forms.
const ALLOWED: &[(&str, &str)] = &[("crates/thoughtsd/src/todos.rs", "\"غدًا\"")];

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }

fn files(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())).flatten() {
        let p = e.path();
        if p.is_dir() { files(&p, exts, out); }
        else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.contains(&x)) { out.push(p); }
    }
}

fn is_arabic_letter(c: char) -> bool { ('\u{0621}'..='\u{064A}').contains(&c) }

/// Every word (`file:line: word`) that carries tanween fath before the alif instead of on it.
/// A bare `ًا` with no letter before it is a pattern in a check like this one, not copy.
fn misspelt(dir: &Path, exts: &[&str]) -> Vec<String> {
    let mut list = Vec::new();
    files(dir, exts, &mut list);
    list.sort();
    let mut bad = Vec::new();
    for path in list {
        let rel = path.strip_prefix(root()).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel}: {e}"));
        for (n, line) in text.lines().enumerate() {
            let allowed: Vec<&str> = ALLOWED.iter().filter(|(f, _)| rel.ends_with(f)).map(|(_, w)| *w).collect();
            let mut cleaned = line.to_string();
            for w in &allowed { cleaned = cleaned.replace(w, ""); }
            let cs: Vec<char> = cleaned.chars().collect();
            for i in 0..cs.len() {
                if cs[i] != ALIF || i < 2 { continue; }
                let (a, b) = (cs[i - 2], cs[i - 1]);
                let wrong = (b == FATHATAN && is_arabic_letter(a))
                    || (b == SHADDA && a == FATHATAN && i >= 3 && is_arabic_letter(cs[i - 3]))
                    || (b == FATHATAN && a == SHADDA && i >= 3 && is_arabic_letter(cs[i - 3]));
                if wrong {
                    let start = cs[..i].iter().rposition(|c| !(is_arabic_letter(*c) || ('\u{064B}'..='\u{0652}').contains(c))).map_or(0, |p| p + 1);
                    let end = cs[i..].iter().position(|c| !is_arabic_letter(*c)).map_or(cs.len(), |p| i + p);
                    bad.push(format!("{rel}:{}: {}", n + 1, cs[start..end].iter().collect::<String>()));
                }
            }
        }
    }
    bad
}

#[test]
fn the_screen_strings_put_tanween_on_the_alif() {
    let bad = misspelt(&root().join("ui/src"), &["ts", "svelte", "js"]);
    assert!(bad.is_empty(), "write اً (and ّاً), not ًا:\n{}", bad.join("\n"));
}

#[test]
fn the_daemon_messages_put_tanween_on_the_alif() {
    let bad = misspelt(&root().join("crates/thoughtsd/src"), &["rs"]);
    assert!(bad.is_empty(), "write اً (and ّاً), not ًا:\n{}", bad.join("\n"));
}

#[test]
fn the_check_finds_both_wrong_forms_and_passes_the_right_ones() {
    let dir = std::env::temp_dir().join(format!("thoughts-arabic-spelling-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.ts"), "'x': 'تلقائيًا نصًّا تلقائياً نصّاً',\nassert(!s.includes('ًا'));\n").unwrap();
    let bad = misspelt(&dir, &["ts"]);
    let _ = std::fs::remove_file(dir.join("a.ts"));
    let _ = std::fs::remove_dir(&dir);
    assert_eq!(bad.len(), 2, "{bad:?}");
    assert!(bad[0].ends_with("تلقائيًا") && bad[1].ends_with("نصًّا"), "{bad:?}");
}
