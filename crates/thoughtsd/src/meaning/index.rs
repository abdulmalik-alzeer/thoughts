//! The index of a library: which pieces of which notes there are, and one vector per piece.
//!
//! - It lives outside the library, in `<home>/meaning/index/<libKey>/` (`libKey` = the first 16 hex
//!   of the SHA-256 of the library's real path), a folder every run is closed to.
//! - `vectors.f32` holds N × 384 little-endian numbers, `chunks.json` the pieces as byte offsets
//!   (`[{path, start, end, sha}]`, never their text), `state.json` the files as they were indexed.
//! - The files are those `search.rs` reads text from, without data and logs (json, csv, tsv, log),
//!   walked exactly as word search walks them: protected folders skipped, private folders left
//!   out by where they really are, links not followed.
//! - A piece is 400–1200 characters, cut at headings and blank lines, never inside a fenced code
//!   block; the note's title goes in front of every piece it is turned into a vector with.
//! - Plain brute force: every vector is scored against the question (a few thousand notes make
//!   10–30 k pieces, about 10 M multiply-adds, well under 20 ms).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use super::model::{Embedder, DIMS};

pub const MIN_PIECE: usize = 400;
/// How far below the best note another may score and still be shown (see `Index::top`). Measured
/// with the real model on Arabic and English notes (the `real_model_…` test): a related note
/// scored 0.79–0.88, the best unrelated one up to 0.80 but always at least 0.055 under the best.
pub const MARGIN: f32 = 0.05;
pub const MAX_PIECE: usize = 1200;
/// Pieces turned into vectors at once.
pub const BATCH: usize = 16;
/// The index is written after this many new pieces, so a pass that is cut off goes on from there.
pub const SAVE_EVERY: usize = 500;
/// Text kinds word search reads that are data rather than writing.
const NOT_INDEXED: [&str; 4] = ["json", "csv", "tsv", "log"];

pub fn indexed(p: &Path) -> bool {
    crate::search::is_text(p) && p.extension().and_then(|e| e.to_str()).is_some_and(|e| !NOT_INDEXED.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn lib_key(root: &Path) -> String {
    let real = crate::paths::canonical(root).unwrap_or_else(|_| root.to_path_buf());
    let h = format!("{:x}", Sha256::digest(real.to_string_lossy().as_bytes()));
    h[..16].to_string()
}

pub fn dir_for(home: &Path, root: &Path) -> PathBuf { home.join("meaning").join("index").join(lib_key(root)) }

pub fn short_sha(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes))[..16].to_string() }

// ---------- pieces ----------

/// Where the text after a note's frontmatter starts (0 when there is none).
fn body_start(text: &str) -> usize {
    let first = text.split_inclusive('\n').next().unwrap_or("");
    if first.trim_end() != "---" { return 0; }
    let mut pos = first.len();
    for line in text[pos..].split_inclusive('\n') {
        pos += line.len();
        if matches!(line.trim_end(), "---" | "...") { return pos; }
    }
    0
}

#[derive(Clone, Copy, Debug)]
struct Block { start: usize, end: usize, heading: bool, code: bool }

fn fence_of(t: &str) -> Option<(char, usize)> {
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

fn is_heading(t: &str) -> bool {
    let n = t.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&n) && t[n..].starts_with([' ', '\t'])
}

/// Lines grouped into blocks: a blank line ends one, a heading starts one, a fenced code block is one.
fn blocks(text: &str, from: usize) -> Vec<Block> {
    let mut out: Vec<Block> = vec![];
    let mut cur: Option<Block> = None;
    let mut fence: Option<(char, usize)> = None;
    let mut pos = from;
    for raw in text[from..].split_inclusive('\n') {
        let (start, content_end) = (pos, pos + raw.trim_end_matches(['\n', '\r']).len());
        pos += raw.len();
        let line = &text[start..content_end];
        let t = line.trim_start();
        if let Some((c, n)) = fence {
            if let Some(b) = cur.as_mut() { b.end = content_end; }
            if fence_of(t).is_some_and(|(c2, n2)| c2 == c && n2 >= n) && t.trim_start_matches(c).trim().is_empty() {
                fence = None;
                out.extend(cur.take());
            }
            continue;
        }
        if let Some(f) = fence_of(t) {
            out.extend(cur.take());
            cur = Some(Block { start, end: content_end, heading: false, code: true });
            fence = Some(f);
            continue;
        }
        if line.trim().is_empty() { out.extend(cur.take()); continue; }
        if is_heading(t) {
            out.extend(cur.take());
            cur = Some(Block { start, end: content_end, heading: true, code: false });
            continue;
        }
        match cur.as_mut() {
            Some(b) => b.end = content_end,
            None => cur = Some(Block { start, end: content_end, heading: false, code: false }),
        }
    }
    out.extend(cur);
    out
}

/// The largest char boundary of `text` at or before `i`.
fn floor(text: &str, mut i: usize) -> usize {
    while i > 0 && !text.is_char_boundary(i) { i -= 1; }
    i
}

/// A block of writing longer than a piece, cut at line ends, else at a space, else anywhere.
fn split_long(text: &str, b: Block) -> Vec<(usize, usize)> {
    if b.code || b.end - b.start <= MAX_PIECE { return vec![(b.start, b.end)]; }
    let mut out = vec![];
    let mut s = b.start;
    while b.end - s > MAX_PIECE {
        let limit = floor(text, s + MAX_PIECE);
        let window = &text[s..limit];
        let cut = window.rfind('\n').filter(|i| *i >= MIN_PIECE).map(|i| s + i)
            .or_else(|| window.rfind(char::is_whitespace).filter(|i| *i >= MIN_PIECE).map(|i| s + i))
            .unwrap_or(limit);
        let cut = if cut <= s { floor(text, s + MAX_PIECE).max(s + 1) } else { cut };
        out.push((s, cut));
        s = cut;
        // The next piece starts at its first character that is not a space.
        while s < b.end && text[s..].starts_with(char::is_whitespace) { s += text[s..].chars().next().map_or(1, char::len_utf8); }
    }
    if s < b.end { out.push((s, b.end)); }
    out
}

/// A note cut into pieces: byte offsets into `text`.
pub fn pieces(text: &str) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = vec![];
    let mut cur: Option<(usize, usize)> = None;
    for b in blocks(text, body_start(text)) {
        for (i, p) in split_long(text, b).into_iter().enumerate() {
            cur = match cur {
                None => Some(p),
                Some((s, e)) => {
                    let starts_section = b.heading && i == 0 && e - s >= MIN_PIECE;
                    if starts_section || p.1 - s > MAX_PIECE { out.push((s, e)); Some(p) } else { Some((s, p.1)) }
                }
            };
        }
    }
    out.extend(cur);
    out.retain(|(s, e)| !text[*s..*e].trim().is_empty());
    out
}

/// A note's title: its frontmatter title or first heading, else its file name.
pub fn title(rel: &str, text: &str) -> String {
    crate::search::title_of(text).unwrap_or_else(|| {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        name.rsplit_once('.').map_or(name, |(n, _)| n).to_string()
    })
}

// ---------- the index ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Chunk { pub path: String, pub start: usize, pub end: usize, pub sha: String }

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct FileMeta { pub len: u64, pub mtime: i64, pub sha: String }

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct StateFile { model: String, files: BTreeMap<String, FileMeta>, updated: Option<String> }

#[derive(Default)]
pub struct Index {
    pub dir: PathBuf,
    pub model: String,
    pub chunks: Vec<Chunk>,
    pub vecs: Vec<f32>,
    pub files: BTreeMap<String, FileMeta>,
    pub updated: Option<String>,
}

impl Index {
    /// The index in `dir`, or an empty one when it is missing, made with another model, or its parts disagree.
    pub fn open(dir: &Path, model: &str) -> Index {
        let empty = Index { dir: dir.to_path_buf(), model: model.to_string(), ..Default::default() };
        let read = || -> Option<Index> {
            let st: StateFile = serde_json::from_str(&std::fs::read_to_string(dir.join("state.json")).ok()?).ok()?;
            if st.model != model { return None; }
            let chunks: Vec<Chunk> = serde_json::from_str(&std::fs::read_to_string(dir.join("chunks.json")).ok()?).ok()?;
            let raw = std::fs::read(dir.join("vectors.f32")).ok()?;
            if raw.len() != chunks.len() * DIMS * 4 { return None; }
            let vecs = raw.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
            // Every piece belongs to a file the state lists.
            if chunks.iter().any(|c| !st.files.contains_key(&c.path)) { return None; }
            Some(Index { dir: dir.to_path_buf(), model: model.to_string(), chunks, vecs, files: st.files, updated: st.updated })
        };
        read().unwrap_or(empty)
    }

    pub fn save(&self) -> std::io::Result<()> {
        crate::paths::create_private_dir(&self.dir)?;
        let mut raw = Vec::with_capacity(self.vecs.len() * 4);
        for v in &self.vecs { raw.extend_from_slice(&v.to_le_bytes()); }
        let st = StateFile { model: self.model.clone(), files: self.files.clone(), updated: self.updated.clone() };
        crate::paths::write_atomic(&self.dir.join("vectors.f32"), &raw, true)?;
        crate::paths::write_atomic(&self.dir.join("chunks.json"), serde_json::to_string(&self.chunks)?.as_bytes(), true)?;
        crate::paths::write_atomic(&self.dir.join("state.json"), serde_json::to_string(&st)?.as_bytes(), true)
    }

    /// Take out every file (and its pieces) for which `gone` holds. How many files went.
    pub fn drop_where(&mut self, gone: impl Fn(&str) -> bool) -> usize {
        let before = self.files.len();
        self.files.retain(|p, _| !gone(p));
        let keep: Vec<bool> = self.chunks.iter().map(|c| !gone(&c.path)).collect();
        if keep.iter().any(|k| !k) {
            let mut vecs = Vec::with_capacity(self.vecs.len());
            for (i, k) in keep.iter().enumerate() { if *k { vecs.extend_from_slice(&self.vecs[i * DIMS..(i + 1) * DIMS]); } }
            self.vecs = vecs;
            let mut it = keep.iter();
            self.chunks.retain(|_| *it.next().unwrap_or(&true));
        }
        before - self.files.len()
    }

    /// Put in a file's pieces, in place of what it had.
    pub fn put(&mut self, path: &str, meta: FileMeta, chunks: Vec<Chunk>, vecs: Vec<Vec<f32>>) {
        self.drop_where(|p| p == path);
        for (c, v) in chunks.into_iter().zip(vecs) {
            if v.len() != DIMS { continue; }
            self.chunks.push(c);
            self.vecs.extend_from_slice(&v);
        }
        self.files.insert(path.to_string(), meta);
    }

    /// The best piece of each file for `q`, best first: (piece number, score), score ≥ `threshold`.
    pub fn top(&self, q: &[f32], limit: usize, threshold: f32) -> Vec<(usize, f32)> {
        if q.len() != DIMS { return vec![]; }
        let mut best: HashMap<&str, (usize, f32)> = HashMap::new();
        for (i, c) in self.chunks.iter().enumerate() {
            let v = &self.vecs[i * DIMS..(i + 1) * DIMS];
            let s: f32 = v.iter().zip(q).map(|(a, b)| a * b).sum();
            let e = best.entry(c.path.as_str()).or_insert((i, f32::MIN));
            if s > e.1 { *e = (i, s); }
        }
        // Above the threshold, and not far below the best: a note far behind the best one is
        // about something else, even when its score alone would pass (`MARGIN`).
        let top = best.values().map(|(_, s)| *s).fold(f32::MIN, f32::max);
        let floor = if threshold > 0.0 { threshold.max(top - MARGIN) } else { threshold };
        let mut out: Vec<(usize, f32)> = best.into_values().filter(|(_, s)| *s >= floor).collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(self.chunks[a.0].path.cmp(&self.chunks[b.0].path)));
        out.truncate(limit);
        out
    }
}

// ---------- walking the library ----------

pub struct Seen { pub rel: String, pub path: PathBuf, pub len: u64, pub mtime: i64 }

fn millis(t: Option<std::time::SystemTime>) -> i64 {
    t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64)
}

/// The files to index, walked exactly as `search::notes` walks the library.
pub fn walk(root: &Path, private: &[String]) -> Vec<Seen> {
    let hidden = crate::library::Hidden::new(root, private);
    let mut out = vec![];
    for e in WalkDir::new(root).into_iter().filter_entry(|e| !crate::library::skip(e) && !hidden.folder(e)).flatten() {
        if !e.file_type().is_file() || !indexed(e.path()) { continue; }
        let Some(rel) = e.path().strip_prefix(root).ok().map(|p| p.to_string_lossy().replace('\\', "/")) else { continue };
        if crate::paths::is_under(&rel, private) { continue; }
        let Ok(meta) = e.metadata() else { continue };
        if meta.len() > crate::search::TEXT_MAX_BYTES { continue; }
        out.push(Seen { rel, path: e.path().to_path_buf(), len: meta.len(), mtime: millis(meta.modified().ok()) });
    }
    out
}

/// How a pass went.
#[derive(Debug, Default, PartialEq)]
pub struct Pass { pub files: usize, pub chunks: usize, pub dropped: usize, pub cancelled: bool }

/// Bring the index up to date with the library: drop what is gone or now private, turn into
/// vectors only the files whose content changed. `progress(done, total)` counts pieces.
pub fn run_pass(ix: &RwLock<Index>, root: &Path, private: &[String], emb: &dyn Embedder, cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize)) -> anyhow::Result<Pass> {
    let seen = walk(root, private);
    let names: HashSet<&str> = seen.iter().map(|s| s.rel.as_str()).collect();
    let mut pass = Pass::default();
    {
        let mut w = ix.write().unwrap_or_else(|e| e.into_inner());
        pass.dropped = w.drop_where(|p| !names.contains(p) || crate::paths::is_under(p, private));
    }
    // What changed: by size and time first, then by content.
    let mut todo: Vec<(String, FileMeta, String, Vec<(usize, usize)>)> = vec![];
    for s in &seen {
        let known = ix.read().unwrap_or_else(|e| e.into_inner()).files.get(&s.rel).cloned();
        if known.as_ref().is_some_and(|k| k.len == s.len && k.mtime == s.mtime) { continue; }
        let Ok(text) = std::fs::read_to_string(&s.path) else { continue };
        let meta = FileMeta { len: s.len, mtime: s.mtime, sha: short_sha(text.as_bytes()) };
        if known.as_ref().is_some_and(|k| k.sha == meta.sha) {
            ix.write().unwrap_or_else(|e| e.into_inner()).files.insert(s.rel.clone(), meta);
            continue;
        }
        let p = pieces(&text);
        todo.push((s.rel.clone(), meta, text, p));
    }
    let total: usize = todo.iter().map(|t| t.3.len()).sum();
    let (mut done, mut unsaved) = (0usize, 0usize);
    progress(0, total);
    for (rel, meta, text, ps) in todo {
        let name = title(&rel, &text);
        let mut vecs: Vec<Vec<f32>> = Vec::with_capacity(ps.len());
        for batch in ps.chunks(BATCH) {
            if cancel.load(Ordering::Relaxed) {
                pass.cancelled = true;
                ix.read().unwrap_or_else(|e| e.into_inner()).save()?;
                return Ok(pass);
            }
            let texts: Vec<String> = batch.iter().map(|(s, e)| super::model::passage_text(&name, &text[*s..*e])).collect();
            vecs.extend(emb.embed(&texts)?);
            done += batch.len();
            progress(done, total);
        }
        let chunks: Vec<Chunk> = ps.iter().map(|(s, e)| Chunk { path: rel.clone(), start: *s, end: *e, sha: short_sha(&text.as_bytes()[*s..*e]) }).collect();
        unsaved += chunks.len();
        pass.files += 1;
        pass.chunks += chunks.len();
        let mut w = ix.write().unwrap_or_else(|e| e.into_inner());
        w.put(&rel, meta, chunks, vecs);
        if unsaved >= SAVE_EVERY { w.save()?; unsaved = 0; }
    }
    let mut w = ix.write().unwrap_or_else(|e| e.into_inner());
    if pass.files > 0 || pass.dropped > 0 || w.updated.is_none() { w.updated = Some(chrono::Utc::now().to_rfc3339()); }
    w.save()?;
    Ok(pass)
}

/// The words shown under a result: the piece's text as it is now in the file, at most `chars`
/// characters, and its line. `None` when the file changed there since it was indexed.
pub fn snippet(root: &Path, c: &Chunk, chars: usize) -> Option<(String, usize, String)> {
    let path = crate::library::resolve(root, &c.path).ok()?;
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > crate::search::TEXT_MAX_BYTES { return None; }
    let text = std::fs::read_to_string(&path).ok()?;
    let ttl = title(&c.path, &text);
    let piece = text.as_bytes().get(c.start..c.end)?;
    if short_sha(piece) != c.sha || !text.is_char_boundary(c.start) || !text.is_char_boundary(c.end) { return None; }
    // Headings are left out of the words shown (the title is shown already), unless that is all there is.
    let piece = &text[c.start..c.end];
    let body: Vec<&str> = piece.lines().filter(|l| !is_heading(l.trim_start())).collect();
    let shown_from = if body.iter().any(|l| !l.trim().is_empty()) { body.join(" ") } else { piece.to_string() };
    let words = shown_from.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut shown: String = words.chars().take(chars).collect();
    if words.chars().count() > chars { shown.push('…'); }
    let line = text[..c.start].matches('\n').count() + 1;
    Some((shown, line, ttl))
}
