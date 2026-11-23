//! Search by meaning, with a tiny stand-in for the model (no model is ever downloaded here).

use super::index::{self, Chunk, FileMeta, Index};
use super::model::{self, Embedder, DIMS};
use super::*;
use std::io::{Read as _, Write as _};
use std::sync::atomic::AtomicUsize;

/// The address the download is pointed at in a test.
pub(super) fn base() -> &'static Mutex<Option<String>> {
    static B: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    B.get_or_init(Default::default)
}

/// The tests share the module's state (the work, the loaded model, the index): one at a time.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static S: Mutex<()> = Mutex::new(());
    lock(&S)
}

/// A stand-in model: a text about a topic (by any of its words, in either language) points one way,
/// anything else another way of its own. Counts the texts it was given.
struct Stub { calls: AtomicUsize, slow: Option<Duration> }
const TOPICS: [&[&str]; 3] = [&["fruit", "apple", "تفاح", "فاكهة"], &["car", "engine", "سيارة", "محرك"], &["tax", "salary", "ضريبة", "راتب"]];
impl Stub { fn new() -> Arc<Stub> { Arc::new(Stub { calls: AtomicUsize::new(0), slow: None }) } }
impl Embedder for Stub {
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if let Some(d) = self.slow { std::thread::sleep(d); }
        self.calls.fetch_add(texts.len(), Ordering::Relaxed);
        Ok(texts.iter().map(|t| {
            let low = t.to_lowercase();
            let mut v = vec![0f32; DIMS];
            match TOPICS.iter().position(|words| words.iter().any(|w| low.contains(w))) {
                Some(i) => v[i] = 1.0,
                None => v[10 + (index::short_sha(low.as_bytes()).as_bytes()[0] as usize % 300)] = 1.0,
            }
            v
        }).collect())
    }
}

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("thoughts-meaning-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&d).unwrap();
    crate::paths::canonical(&d).unwrap()
}
fn put(d: &Path, rel: &str, text: &str) {
    let p = d.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn fresh_index(dir: &Path) -> RwLock<Index> { RwLock::new(Index::open(dir, &model::tag())) }
fn pass(ix: &RwLock<Index>, root: &Path, private: &[String], emb: &dyn Embedder) -> index::Pass {
    index::run_pass(ix, root, private, emb, &AtomicBool::new(false), &mut |_, _| {}).unwrap()
}
fn paths_of(ix: &RwLock<Index>) -> Vec<String> { ix.read().unwrap().files.keys().cloned().collect() }

/// Make the module's state as a fresh daemon has it, with the stand-in loaded.
fn reset(emb: Option<Arc<dyn Embedder>>) {
    *lock(engine_slot()) = emb;
    *lock(index_slot()) = None;
    lock(offers()).clear();
    *lock(size_cache()) = None;
    let mut w = lock(work());
    w.phase = Phase::Idle;
    w.error = None;
}

async fn state_with_library(root: &Path) -> Arc<AppState> {
    let st = Arc::new(AppState::for_test());
    st.settings.write().await.library_path = Some(root.to_string_lossy().into());
    st
}

// ---------- pieces ----------

#[test]
fn pieces_follow_headings_keep_code_whole_and_round_trip() {
    let para = |w: &str, n: usize| (0..n).map(|_| w).collect::<Vec<_>>().join(" ");
    let code = format!("```rust\n{}\n\n{}\n```", para("let x = 1;", 60), para("y();", 60));
    let text = format!("---\ntitle: T\ntags: [a]\n---\n# One\n\n{}\n\n## Two\n\n{}\n\n{}\n\n{}\n", para("alpha", 90), para("beta", 300), code, para("gamma", 10));
    let ps = index::pieces(&text);
    assert!(ps.len() >= 4, "{ps:?}");
    for (s, e) in &ps {
        assert!(text.is_char_boundary(*s) && text.is_char_boundary(*e) && s < e);
        let piece = &text[*s..*e];
        assert!(!piece.contains("title: T"), "frontmatter is skipped");
        // A fenced block is never cut: a piece holds both of its fences or neither.
        assert_eq!(piece.matches("```").count() % 2, 0, "{piece}");
        if !piece.contains("```") { assert!(piece.len() <= index::MAX_PIECE, "{}", piece.len()); }
    }
    // A heading starts a piece once the piece before it is long enough.
    assert!(ps.iter().any(|(s, _)| text[*s..].starts_with("## Two")));
    // In reading order, without overlaps.
    assert!(ps.windows(2).all(|w| w[0].1 <= w[1].0));
    // A short note is one piece; an empty one is none.
    assert_eq!(index::pieces("# Hi\n\nshort").len(), 1);
    assert!(index::pieces("---\ntitle: x\n---\n\n").is_empty());
}

#[test]
fn arabic_text_is_cut_on_character_boundaries() {
    let words = "الضريبة على الراتب الشهري تُحسب في نهاية السنة ".repeat(80);
    let text = format!("# ملاحظات\n\n{words}");
    let ps = index::pieces(&text);
    assert!(ps.len() >= 2);
    for (s, e) in ps {
        assert!(text.is_char_boundary(s) && text.is_char_boundary(e));
        assert!(e - s <= index::MAX_PIECE);
    }
    assert_eq!(index::title("a/b.md", &text), "ملاحظات");
    assert_eq!(index::title("a/نص.md", "no heading"), "نص");
}

// ---------- the index ----------

#[test]
fn private_folders_and_links_into_them_are_never_read() {
    let _g = serial();
    let (root, home) = (tmp(), tmp());
    put(&root, "ideas/apple.md", "# Apples\nfruit");
    put(&root, "Me/Finances/tax.md", "# Tax\nsalary");
    put(&root, "data/x.json", "{\"apple\": 1}");
    put(&root, ".git/notes.md", "apple");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("Me/Finances/tax.md"), root.join("ideas/link.md")).unwrap();
        std::os::unix::fs::symlink(root.join("Me/Finances"), root.join("ideas/money")).unwrap();
    }
    let emb = Stub::new();
    let ix = fresh_index(&home.join("ix"));
    pass(&ix, &root, &["me/finances".into()], emb.as_ref());
    assert_eq!(paths_of(&ix), ["ideas/apple.md"], "private, data, protected and linked files are left out");
    for c in &ix.read().unwrap().chunks { assert_eq!(c.path, "ideas/apple.md"); }
    // The index holds offsets and hashes, never the text.
    ix.read().unwrap().save().unwrap();
    let saved = std::fs::read_to_string(home.join("ix/chunks.json")).unwrap() + &std::fs::read_to_string(home.join("ix/state.json")).unwrap();
    assert!(!saved.contains("fruit") && !saved.contains("Apples"));
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_pass_embeds_only_what_changed_and_drops_what_went() {
    let _g = serial();
    let (root, home) = (tmp(), tmp());
    put(&root, "a.md", "# A\napple");
    put(&root, "b.md", "# B\ncar");
    put(&root, "c.md", "# C\ntax");
    let emb = Stub::new();
    let ix = fresh_index(&home.join("ix"));
    let p = pass(&ix, &root, &[], emb.as_ref());
    assert_eq!((p.files, p.dropped), (3, 0));
    assert_eq!(emb.calls.load(Ordering::Relaxed), 3);
    // Nothing changed: nothing is embedded again.
    assert_eq!(pass(&ix, &root, &[], emb.as_ref()), index::Pass::default());
    assert_eq!(emb.calls.load(Ordering::Relaxed), 3);
    // One changed, one went, one only touched (same content): one piece is embedded.
    std::thread::sleep(Duration::from_millis(20));
    put(&root, "a.md", "# A\napple, and a longer text now");
    std::fs::remove_file(root.join("b.md")).unwrap();
    put(&root, "c.md", "# C\ntax");
    let p = pass(&ix, &root, &[], emb.as_ref());
    assert_eq!((p.files, p.dropped), (1, 1));
    assert_eq!(emb.calls.load(Ordering::Relaxed), 4);
    assert_eq!(paths_of(&ix), ["a.md", "c.md"]);
    // Read back from disk, it is the same index.
    let again = Index::open(&home.join("ix"), &model::tag());
    assert_eq!(again.chunks, ix.read().unwrap().chunks);
    assert_eq!(again.vecs.len(), again.chunks.len() * DIMS);
    // An index made with another model is not used.
    assert!(Index::open(&home.join("ix"), "other@1").chunks.is_empty());
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_cancelled_pass_keeps_what_it_did() {
    let _g = serial();
    let (root, home) = (tmp(), tmp());
    put(&root, "a.md", "apple");
    let ix = fresh_index(&home.join("ix"));
    let p = index::run_pass(&ix, &root, &[], Stub::new().as_ref(), &AtomicBool::new(true), &mut |_, _| {}).unwrap();
    assert!(p.cancelled);
    assert!(ix.read().unwrap().updated.is_none(), "not counted as gone through");
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn the_best_piece_of_each_file_ranks_and_the_threshold_holds() {
    let mut ix = Index::default();
    let unit = |i: usize, w: f32| { let mut v = vec![0f32; DIMS]; v[0] = w; v[i] = (1.0 - w * w).sqrt(); v };
    let c = |p: &str| Chunk { path: p.into(), start: 0, end: 1, sha: String::new() };
    ix.put("a.md", FileMeta::default(), vec![c("a.md"), c("a.md")], vec![unit(1, 0.5), unit(2, 0.95)]);
    ix.put("b.md", FileMeta::default(), vec![c("b.md")], vec![unit(3, 0.9)]);
    ix.put("c.md", FileMeta::default(), vec![c("c.md")], vec![unit(4, 0.2)]);
    let mut q = vec![0f32; DIMS];
    q[0] = 1.0;
    let top = ix.top(&q, 10, 0.8);
    assert_eq!(top.iter().map(|(i, _)| ix.chunks[*i].path.as_str()).collect::<Vec<_>>(), ["a.md", "b.md"]);
    assert_eq!(top[0].0, 1, "the better piece of a.md");
    assert!((top[0].1 - 0.95).abs() < 1e-5);
    assert_eq!(ix.top(&q, 1, 0.0).len(), 1);
    // Taking a file out takes its vectors with it.
    ix.drop_where(|p| p == "a.md");
    assert_eq!((ix.chunks.len(), ix.vecs.len()), (2, 2 * DIMS));
    assert_eq!(ix.chunks[ix.top(&q, 1, 0.0)[0].0].path, "b.md");
}

#[test]
fn a_snippet_is_read_only_while_the_piece_is_unchanged() {
    let root = tmp();
    put(&root, "n.md", "# Note\n\nfirst line\nthe fruit we like is apple\n");
    let text = std::fs::read_to_string(root.join("n.md")).unwrap();
    let start = text.find("the fruit").unwrap();
    let end = start + "the fruit we like is apple".len();
    let c = Chunk { path: "n.md".into(), start, end, sha: index::short_sha(&text.as_bytes()[start..end]) };
    let (s, line, title) = index::snippet(&root, &c, 9).unwrap();
    assert_eq!((s.as_str(), line, title.as_str()), ("the fruit…", 4, "Note"));
    put(&root, "n.md", "# Note\n\nfirst line\nthe fruit we like is pears\n");
    assert!(index::snippet(&root, &c, 180).is_none());
    std::fs::remove_dir_all(root).unwrap();
}

// ---------- download ----------

/// A small file server for one test: Range answers, an optional redirect, and a count of requests.
struct Served { port: u16, hits: Arc<AtomicUsize>, ranges: Arc<Mutex<Vec<String>>> }

fn serve(files: Vec<(&'static str, Vec<u8>)>, redirect_to: Option<String>, cut_after: Option<usize>) -> Served {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (hits, ranges) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(vec![])));
    let (h, r) = (hits.clone(), ranges.clone());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { break };
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
            let range = req.lines().find_map(|l| l.strip_prefix("Range: bytes=").or_else(|| l.strip_prefix("range: bytes="))).map(|x| x.trim_end_matches('-').to_string());
            let first = h.fetch_add(1, Ordering::Relaxed) == 0;
            lock(&r).push(range.clone().unwrap_or_default());
            if let Some(to) = &redirect_to {
                let _ = write!(s, "HTTP/1.1 302 Found\r\nLocation: {to}{path}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            }
            let Some((_, body)) = files.iter().find(|(n, _)| path.ends_with(&format!("/{n}"))) else {
                let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            };
            let from: usize = range.and_then(|x| x.parse().ok()).unwrap_or(0);
            let part = &body[from.min(body.len())..];
            let (code, extra) = if from > 0 { ("206 Partial Content", format!("Content-Range: bytes {from}-{}/{}\r\n", body.len() - 1, body.len())) } else { ("200 OK", String::new()) };
            let _ = write!(s, "HTTP/1.1 {code}\r\n{extra}ETag: \"v1\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", part.len());
            // The first answer may be cut off half-way, as a dropped connection would.
            let send = if first { cut_after.map_or(part.len(), |c| c.min(part.len())) } else { part.len() };
            let _ = s.write_all(&part[..send]);
        }
    });
    Served { port, hits, ranges }
}

fn file_for(name: &'static str, body: &[u8]) -> model::File {
    model::File { name, bytes: body.len() as u64, sha256: Box::leak(format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(body)).into_boxed_str()) }
}

#[test]
fn a_cut_off_download_goes_on_from_where_it_stopped() {
    let home = tmp();
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let srv = serve(vec![("w.bin", body.clone())], None, Some(100_000));
    let f = file_for("w.bin", &body);
    let base = format!("http://127.0.0.1:{}/x", srv.port);
    let cancel = AtomicBool::new(false);
    let first = model::download_files(&home, &base, std::slice::from_ref(&f), &cancel, &mut |_, _| {});
    assert!(first.is_err(), "the connection ended early");
    let part = model::dir(&home).join("w.bin.part");
    assert_eq!(std::fs::metadata(&part).unwrap().len(), 100_000);
    let mut seen = vec![];
    model::download_files(&home, &base, std::slice::from_ref(&f), &cancel, &mut |g, t| seen.push((g, t))).unwrap();
    assert_eq!(std::fs::read(model::dir(&home).join("w.bin")).unwrap(), body);
    assert!(!part.exists());
    assert_eq!(lock(&srv.ranges).as_slice(), ["", "100000"], "asked for the rest only");
    assert_eq!(seen.last(), Some(&(300_000, 300_000)));
    // Checked once: not fetched again.
    model::download_files(&home, &base, std::slice::from_ref(&f), &cancel, &mut |_, _| {}).unwrap();
    assert_eq!(srv.hits.load(Ordering::Relaxed), 2);
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_file_with_the_wrong_hash_is_deleted() {
    let home = tmp();
    let srv = serve(vec![("w.bin", b"not the model".to_vec())], None, None);
    let mut f = file_for("w.bin", b"not the model");
    f.sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
    let e = model::download_files(&home, &format!("http://127.0.0.1:{}", srv.port), std::slice::from_ref(&f), &AtomicBool::new(false), &mut |_, _| {}).unwrap_err();
    assert!(e.to_string().contains(&crate::tr::err("meaning.bad_download", &[]).to_string()), "{e}");
    for n in ["w.bin", "w.bin.part", "w.bin.part.json"] { assert!(!model::dir(&home).join(n).exists(), "{n}"); }
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_redirect_to_another_host_is_refused() {
    let home = tmp();
    let srv = serve(vec![], Some("https://evil.example.com".into()), None);
    let f = file_for("w.bin", b"x");
    let e = model::download_files(&home, &format!("http://127.0.0.1:{}", srv.port), std::slice::from_ref(&f), &AtomicBool::new(false), &mut |_, _| {}).unwrap_err();
    assert!(e.to_string().contains(&crate::tr::err("meaning.bad_host", &[]).to_string()), "{e}");
    // Only the preset host and the Hub's CDN, over https.
    for ok in ["https://huggingface.co/x", "https://HuggingFace.co/x", "https://huggingface.co:443/x", "https://us.aws.cdn.hf.co/x?y=1", "https://cas-bridge.xethub.hf.co/a"] {
        assert!(model::allowed(ok), "{ok}");
    }
    for bad in ["http://huggingface.co/x", "https://huggingface.co.evil.com/x", "https://evil.com/huggingface.co", "https://user@huggingface.co/x",
        "https://user:pw@huggingface.co/x", "https://evil.com@huggingface.co/x", "https://huggingface.co@evil.com/x",
        "https://huggingface.co:444/x", "https://hf.co.evil/x", "ftp://huggingface.co/x", "https://evilhuggingface.co/x", "https://evilhf.co/x",
        "https://hf.co/x", "https://.hf.co/x", "https://a..hf.co/x", "https://huggingface.co./x", "https://us.cdn.hf.co./x",
        "https://huggingface.co\\@evil.com/x", "https://evil.com\\.huggingface.co/x", "https://huggingface.co/x\ny", "https://hugging face.co/x",
        "https://evil.com%2f.huggingface.co/x", "https://evil.com%2F@huggingface.co/x", "https://evil%2ecom/x",
        "https://13.33.0.1/x", "https://[::1]/x", "https://0x7f000001/x", "http://127.0.0.1.evil.com/x", "huggingface.co/x", ""] {
        assert!(!model::allowed(bad), "{bad}");
    }
    // What passes is what is asked for: the parsed address names an allowed host.
    let u = model::checked_url("https://us.aws.cdn.hf.co/a/../b?x=1").unwrap();
    assert_eq!((u.host_str(), u.path()), (Some("us.aws.cdn.hf.co"), "/b"));
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn the_preset_points_at_the_pinned_commit() {
    assert!(model::base_url().starts_with("https://huggingface.co/intfloat/multilingual-e5-small/resolve/614241f6"));
    assert_eq!(model::total_bytes(), 655 + 17_082_730 + 470_641_600);
    assert!(model::FILES.iter().all(|f| f.sha256.len() == 64));
    assert!(!model::installed(&tmp()));
}

// ---------- the routes ----------

fn q(pairs: &[(&str, &str)]) -> Q { Query(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()) }
fn status(e: crate::api::ApiErr) -> u16 { crate::api::shown_status(&e) }

#[tokio::test]
async fn off_by_default_with_nothing_on_disk_and_nothing_running() {
    let _g = serial();
    reset(None);
    let root = tmp();
    let st = state_with_library(&root).await;
    let v = info(State(st.clone()), q(&[])).await.unwrap().0;
    assert_eq!(v["state"], "off");
    assert_eq!(v["model"]["license"], "MIT");
    assert_eq!(v["model"]["bytes"], model::total_bytes());
    assert!(v.get("download").is_none() && v.get("index").is_none());
    assert!(!lock(work()).running, "nothing started");
    assert!(!crate::paths::home().join("meaning").exists(), "nothing written");
    assert_eq!(search(State(st.clone()), q(&[("q", "apple")])).await.map(|_| ()).map_err(status), Err(409));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn the_routes_refuse_add_ons() {
    let _g = serial();
    let st = Arc::new(AppState::for_test());
    let p = || q(&[("scope", "plugin")]);
    assert_eq!(info(State(st.clone()), p()).await.map(|_| ()).map_err(status), Err(403));
    assert_eq!(search(State(st.clone()), q(&[("scope", "plugin"), ("q", "x")])).await.map(|_| ()).map_err(status), Err(403));
    assert_eq!(install(State(st.clone()), p()).await.map(|_| ()).map_err(status), Err(403));
    assert_eq!(remove(State(st.clone()), p()).await.map(|_| ()).map_err(status), Err(403));
    assert_eq!(offer(State(st.clone()), p(), Json(OfferReq { op: "never".into() })).await.map(|_| ()).map_err(status), Err(403));
    assert!(!st.settings.read().await.search.meaning.on && !st.settings.read().await.search.meaning.never);
    // No access key reaches them either.
    let caller = crate::tokens::Caller { id: "k".into(), scopes: vec![crate::tokens::Scope::Read, crate::tokens::Scope::Capture] };
    for path in ["/api/meaning", "/api/meaning/search?q=x", "/api/meaning/install"] {
        for m in [axum::http::Method::GET, axum::http::Method::POST] {
            assert!(crate::tokens::admit(&caller, &m, &path.parse().unwrap()).is_none(), "{m} {path}");
        }
    }
}

#[tokio::test]
async fn ready_search_finds_by_meaning_and_leaves_private_folders_out() {
    let _g = serial();
    reset(Some(Stub::new()));
    let root = tmp();
    put(&root, "ideas/orchard.md", "# Orchard\n\nWe grow apple trees by the river.\n");
    put(&root, "ideas/garage.md", "# Garage\n\nThe engine needs oil.\n");
    put(&root, "me/money/tax.md", "# Tax\n\nsalary and tax\n");
    let st = state_with_library(&root).await;
    st.settings.write().await.private_paths = vec!["me/money".into()];
    let home = crate::paths::home();
    let ix = index_for(&home, &root);
    pass(&ix, &root, &[], lock(engine_slot()).clone().unwrap().as_ref());
    st.settings.write().await.search.meaning.on = true;
    // An Arabic question finds the English note about the same thing.
    let r = search(State(st.clone()), q(&[("q", "تفاح")])).await.unwrap().0;
    let res = r["results"].as_array().unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0]["path"], "ideas/orchard.md");
    assert_eq!(res[0]["title"], "Orchard");
    assert_eq!(res[0]["line"], 1);
    assert!(res[0]["snippet"].as_str().unwrap().contains("apple trees"));
    // The pass above was made as if nothing were private: the answer still leaves it out…
    assert!(search(State(st.clone()), q(&[("q", "salary")])).await.unwrap().0["results"].as_array().unwrap().is_empty());
    // …and a change to the private folders takes it out of the index at once.
    assert!(ix.read().unwrap().files.contains_key("me/money/tax.md"));
    private_changed(&st).await;
    assert!(!ix.read().unwrap().files.contains_key("me/money/tax.md"));
    assert!(!Index::open(&index::dir_for(&home, &root), &model::tag()).files.contains_key("me/money/tax.md"), "saved so");
    // The limit holds, at most 30.
    assert_eq!(search(State(st.clone()), q(&[("q", "engine"), ("limit", "500")])).await.unwrap().0["results"].as_array().unwrap().len(), 1);
    // Removing deletes the model and every index, and switches it off.
    std::fs::create_dir_all(model::dir(&home)).unwrap();
    std::fs::write(model::dir(&home).join("model.safetensors"), vec![0u8; 1000]).unwrap();
    let v = remove(State(st.clone()), q(&[])).await.unwrap().0;
    assert!(v["freed"].as_u64().unwrap() >= 1000);
    assert!(!home.join("meaning").exists());
    assert!(!st.settings.read().await.search.meaning.on);
    assert!(lock(engine_slot()).is_none() && lock(index_slot()).is_none());
    assert_eq!(search(State(st.clone()), q(&[("q", "apple")])).await.map(|_| ()).map_err(status), Err(409));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn settings_saves_never_switch_it_on_or_off() {
    let mut old = crate::settings::Settings::default();
    old.search.meaning.on = true;
    old.search.meaning.never = true;
    let mut new = crate::settings::Settings::default();
    new.sanitize_update(&old).unwrap();
    assert_eq!(new.search.meaning, old.search.meaning);
    // Settings from before 0.5.0 read as off.
    let s: crate::settings::Settings = serde_json::from_value(json!({"search": {"fileThreshold": 1500}})).unwrap();
    assert_eq!(s.search.meaning, crate::settings::MeaningSettings::default());
}

// ---------- the assistant's suggestion ----------

async fn chat_of(st: &AppState, kind: &str) -> String {
    let c = crate::chats::Chat { kind: kind.into(), ..Default::default() };
    let id = c.id.clone();
    st.chats.insert(c).await;
    id
}

#[tokio::test]
async fn the_marker_becomes_one_offer_per_chat_in_ordinary_chats_only() {
    let _g = serial();
    reset(None);
    let st = Arc::new(AppState::for_test());
    let mut rx = st.events.subscribe();
    let chat = chat_of(&st, "chat").await;
    let wrap = chat_of(&st, "wrap").await;
    let text = format!("I found nothing about that.\n{MARKER}\n");
    crate::claude::push(&st, &chat, Msg::new("assistant", text.clone())).await;
    let shown = st.chats.get(&chat).await.unwrap().messages.last().unwrap().text.clone();
    assert_eq!(shown, "I found nothing about that.");
    let mut kinds = vec![];
    while let Ok(e) = rx.try_recv() { kinds.push(e["type"].as_str().unwrap_or("").to_string()); }
    assert_eq!(kinds, ["chat.message", "meaning.suggest"], "the offer comes after the answer it belongs to");
    assert_eq!(lock(offers()).get(&chat).cloned(), st.chats.get(&chat).await.unwrap().messages.last().map(|m| m.id.clone()));
    // Once per chat.
    crate::claude::push(&st, &chat, Msg::new("assistant", text.clone())).await;
    assert!(!rx.try_recv().into_iter().any(|e| e["type"] == "meaning.suggest"));
    assert_eq!(st.settings.read().await.search.meaning.suggested_chats, [chat.clone()]);
    // Other kinds of run keep the line as it is, and offer nothing.
    crate::claude::push(&st, &wrap, Msg::new("assistant", text.clone())).await;
    assert_eq!(st.chats.get(&wrap).await.unwrap().messages.last().unwrap().text, text);
    // The user's own message is never changed.
    let other = chat_of(&st, "chat").await;
    crate::claude::push(&st, &other, Msg::new("user", text.clone())).await;
    assert_eq!(st.chats.get(&other).await.unwrap().messages.last().unwrap().text, text);
    // "Not now": no offer for 30 days, and the open ones go.
    offer(State(st.clone()), q(&[]), Json(OfferReq { op: "later".into() })).await.unwrap();
    assert!(lock(offers()).is_empty());
    let third = chat_of(&st, "chat").await;
    let mut rx = st.events.subscribe();
    crate::claude::push(&st, &third, Msg::new("assistant", text.clone())).await;
    assert_eq!(st.chats.get(&third).await.unwrap().messages.last().unwrap().text, "I found nothing about that.", "still taken out");
    assert!(!std::iter::from_fn(|| rx.try_recv().ok()).any(|e| e["type"] == "meaning.suggest"));
    let snooze = st.settings.read().await.search.meaning.snooze_until.clone().unwrap();
    let days = (chrono::DateTime::parse_from_rfc3339(&snooze).unwrap().with_timezone(&chrono::Utc) - chrono::Utc::now()).num_days();
    assert!((29..=30).contains(&days));
    // "Don't ask again".
    st.settings.write().await.search.meaning.snooze_until = None;
    offer(State(st.clone()), q(&[]), Json(OfferReq { op: "never".into() })).await.unwrap();
    assert!(st.settings.read().await.search.meaning.never);
    let fourth = chat_of(&st, "chat").await;
    crate::claude::push(&st, &fourth, Msg::new("assistant", text)).await;
    assert!(lock(offers()).is_empty());
    assert!(offer(State(st.clone()), q(&[]), Json(OfferReq { op: "install".into() })).await.is_err(), "the offer never installs");
    assert!(!st.settings.read().await.search.meaning.on);
}

#[tokio::test]
async fn chats_are_told_how_to_ask_until_it_is_declined_and_nothing_else_is() {
    let _g = serial();
    reset(None);
    let root = tmp();
    let st = state_with_library(&root).await;
    assert_eq!(run_note(&st, "chat", "hi", Some(&root)).await.as_deref(), Some(MARKER_NOTE));
    for kind in ["wrap", "workflow", "mado", "customize"] { assert!(run_note(&st, kind, "hi", Some(&root)).await.is_none(), "{kind}"); }
    assert!(run_note(&st, "chat", "hi", None).await.is_none(), "no library, nothing to search");
    st.settings.write().await.search.meaning.never = true;
    assert!(run_note(&st, "chat", "hi", Some(&root)).await.is_none(), "declined: the run is told nothing more");
    st.settings.write().await.search.meaning.never = false;
    st.settings.write().await.search.meaning.snooze_until = Some((chrono::Utc::now() + chrono::Duration::days(3)).to_rfc3339());
    assert!(run_note(&st, "chat", "hi", Some(&root)).await.is_none());
    st.settings.write().await.search.meaning.snooze_until = Some((chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339());
    assert!(run_note(&st, "chat", "hi", Some(&root)).await.is_some(), "a snooze that is over");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn when_ready_a_chat_is_told_paths_only_within_the_time_budget() {
    let _g = serial();
    let root = tmp();
    put(&root, "ideas/orchard.md", "# Orchard\n\nWe grow apple trees. A secret sentence.\n");
    put(&root, "ideas/garage.md", "# Garage\n\nThe engine needs oil.\n");
    let st = state_with_library(&root).await;
    st.settings.write().await.search.meaning.on = true;
    reset(Some(Stub::new()));
    // On but not ready yet: nothing.
    assert!(run_note(&st, "chat", "the fruit", Some(&root)).await.is_none());
    let ix = index_for(&crate::paths::home(), &root);
    pass(&ix, &root, &[], Stub::new().as_ref());
    let note = run_note(&st, "chat", "what did I write about fruit?", Some(&root)).await.unwrap();
    assert!(note.contains("`ideas/orchard.md`") && !note.contains("garage"), "{note}");
    assert!(!note.contains("secret") && !note.contains("Orchard"), "paths only");
    assert!(run_note(&st, "wrap", "fruit", Some(&root)).await.is_none());
    // A model too slow for the budget: the chat starts without the note.
    *lock(engine_slot()) = Some(Arc::new(Stub { calls: AtomicUsize::new(0), slow: Some(NOTE_BUDGET * 3) }));
    let t = Instant::now();
    assert!(run_note(&st, "chat", "fruit", Some(&root)).await.is_none());
    assert!(t.elapsed() < NOTE_BUDGET * 2);
    reset(None);
    let _ = std::fs::remove_dir_all(index::dir_for(&crate::paths::home(), &root));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_large_library_is_suggested_until_declined() {
    let _g = serial();
    reset(None);
    let root = tmp();
    for i in 0..5 { put(&root, &format!("n{i}.md"), "x"); }
    put(&root, "private/p.md", "x");
    let st = state_with_library(&root).await;
    { let mut s = st.settings.write().await; s.search.file_threshold = 5; s.private_paths = vec!["private".into()]; }
    assert_eq!(info(State(st.clone()), q(&[])).await.unwrap().0["suggest"], json!({"why": "size", "files": 5}));
    *lock(size_cache()) = None;
    st.settings.write().await.search.file_threshold = 6;
    assert_eq!(info(State(st.clone()), q(&[])).await.unwrap().0["suggest"], Value::Null, "private files are not counted");
    st.settings.write().await.search.file_threshold = 5;
    st.settings.write().await.search.meaning.never = true;
    let v = info(State(st.clone()), q(&[])).await.unwrap().0;
    assert_eq!((v["suggest"].clone(), v["quiet"].clone()), (Value::Null, json!(true)));
    std::fs::remove_dir_all(root).unwrap();
}

/// By hand, with the real model in a folder of its own:
/// `THOUGHTS_MEANING_MODEL_HOME=<dir that holds meaning/model/…> cargo test -p thoughtsd real_model -- --ignored`
#[test]
#[ignore]
fn real_model_an_arabic_question_finds_the_english_note() {
    let home = PathBuf::from(std::env::var("THOUGHTS_MEANING_MODEL_HOME").expect("THOUGHTS_MEANING_MODEL_HOME"));
    let e5 = model::E5::load(&home).unwrap();
    let root = tmp();
    put(&root, "farm/orchard.md", "# The orchard\n\nThis spring we planted forty apple trees and two pear trees by the river.\n");
    put(&root, "work/taxes.md", "# Paying tax\n\nThe accountant files the company's VAT return every quarter.\n");
    put(&root, "car/repairs.md", "# Car repairs\n\nThe garage replaced the brake pads and changed the engine oil.\n");
    let ix = fresh_index(&root.join(".ix"));
    pass(&ix, &root, &[], &e5);
    let ix = ix.read().unwrap();
    let ask = |q: &str| -> Vec<(String, f32)> {
        let v = e5.embed(&[model::query_text(q)]).unwrap().pop().unwrap();
        ix.top(&v, 3, 0.0).into_iter().map(|(i, s)| (ix.chunks[i].path.clone(), s)).collect()
    };
    for (q, want) in [("أشجار التفاح في المزرعة", "farm/orchard.md"), ("ضريبة القيمة المضافة", "work/taxes.md"), ("تغيير زيت المحرك", "car/repairs.md"),
        ("fruit trees", "farm/orchard.md"), ("fixing the vehicle", "car/repairs.md")] {
        let got = ask(q);
        eprintln!("{q}: {got:?}");
        assert_eq!(got[0].0, want, "{q}");
        assert!(got[0].1 >= THRESHOLD, "{q}: {} is under the threshold", got[0].1);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_run_is_closed_to_the_model_and_the_index() {
    let (home, cwd) = (tmp(), tmp());
    // Before it exists too, so no run can plant a model or an index there.
    let deny = crate::claude::protective_deny(&cwd, &[], &home, "chat-1", false);
    for tool in ["Read", "Edit"] {
        assert!(deny.iter().any(|r| r.starts_with(&format!("{tool}(")) && r.ends_with("/meaning/**)")), "{tool}: {deny:?}");
    }
    std::fs::remove_dir_all(home).unwrap();
    std::fs::remove_dir_all(cwd).unwrap();
}
