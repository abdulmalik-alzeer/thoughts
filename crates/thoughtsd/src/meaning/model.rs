//! The one model search by meaning uses, how it is fetched, and how it turns text into vectors.
//!
//! - The model is preset: `intfloat/multilingual-e5-small` at a pinned commit, MIT, about 100
//!   languages (Arabic and English among them), 384 numbers per piece of text, at most 512 tokens.
//! - Nothing is fetched until the user presses the button in Settings (`POST /api/meaning/install`).
//!   Then the three files come one at a time, over https, from the one preset host; the Hub sends
//!   the large files from its CDN, so a redirect is followed only to `*.huggingface.co` / `*.hf.co`.
//! - A download that is cut off (pause, a dropped connection, the app closing) continues where it
//!   stopped (`Range` with `If-Range`); a file whose size or SHA-256 is not the pinned one is deleted.
//! - Texts get the `query: ` / `passage: ` prefixes the model was trained with, are mean-pooled over
//!   their tokens and L2-normalised, so a dot product is the cosine similarity.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const ID: &str = "multilingual-e5-small";
pub const NAME: &str = "Multilingual E5 small";
pub const REPO: &str = "intfloat/multilingual-e5-small";
/// The Hub commit every file is fetched from (checked 2026-11-23).
pub const COMMIT: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
pub const LICENSE: &str = "MIT";
pub const LANGUAGES: &str = "about 100, Arabic and English among them";
pub const HOST: &str = "huggingface.co";
pub const DIMS: usize = 384;
/// Longest input in tokens (the model's own limit).
pub const MAX_TOKENS: usize = 512;

pub struct File { pub name: &'static str, pub bytes: u64, pub sha256: &'static str }

/// Smallest first, so the bar moves early and a bad answer is seen before the large file.
pub const FILES: [File; 3] = [
    File { name: "config.json", bytes: 655, sha256: "69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959" },
    File { name: "tokenizer.json", bytes: 17_082_730, sha256: "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39" },
    File { name: "model.safetensors", bytes: 470_641_600, sha256: "1a55775f53449dac10a2bcbc312469fac40b96d53198c407081a831f81c98477" },
];

pub fn total_bytes() -> u64 { FILES.iter().map(|f| f.bytes).sum() }

/// What the index records, so an index made with another model is never mixed with this one.
pub fn tag() -> String { format!("{ID}@{COMMIT}") }

pub fn dir(home: &Path) -> PathBuf { home.join("meaning").join("model").join(ID) }

/// Written once every file has been checked: the hashes are not worked out again at every start.
const READY: &str = "ready.json";

/// Is the model here, complete and checked?
pub fn installed(home: &Path) -> bool {
    let d = dir(home);
    let ready = std::fs::read_to_string(d.join(READY)).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    ready.map_or(false, |r| r["model"] == tag()) && FILES.iter().all(|f| checked(&d, f))
}

/// Where the files come from: the Hub at the pinned commit. Tests point it at a local server.
pub fn base_url() -> String { format!("https://{HOST}/{REPO}/resolve/{COMMIT}") }

/// May a download go to `url`? https to the preset host or the Hub's CDN hosts only.
pub fn allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return test_host(url) };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.contains('@') || host.contains(':') || host.is_empty() { return false; }
    let host = host.to_ascii_lowercase();
    host == HOST || host == "hf.co" || host.ends_with(".huggingface.co") || host.ends_with(".hf.co")
}

/// Tests serve the files from a server on this computer, over plain http.
#[cfg(test)]
fn test_host(url: &str) -> bool { url.starts_with("http://127.0.0.1:") }
#[cfg(not(test))]
fn test_host(_url: &str) -> bool { false }

/// Why a download stopped early.
#[derive(Debug, PartialEq)]
pub enum Stopped { Cancelled }
impl std::fmt::Display for Stopped { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("cancelled") } }
impl std::error::Error for Stopped {}

/// Fetch every missing file into `dir(home)`, calling `progress(got, total)` at most every 250 ms
/// (and once at the end). Returns early, with `Stopped::Cancelled`, when `cancel` is set.
pub fn download(home: &Path, base: &str, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> anyhow::Result<()> {
    download_files(home, base, &FILES, cancel, progress)?;
    std::fs::write(dir(home).join(READY), json!({"model": tag(), "checked": chrono::Utc::now().to_rfc3339()}).to_string())?;
    Ok(())
}

pub(crate) fn download_files(home: &Path, base: &str, files: &[File], cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> anyhow::Result<()> {
    let d = dir(home);
    crate::paths::create_private_dir(&d)?;
    let total: u64 = files.iter().map(|f| f.bytes).sum();
    let mut before = 0u64;
    for f in files {
        if !checked(&d, f) {
            let _ = std::fs::remove_file(d.join(f.name));
            fetch(&d, f, &format!("{base}/{}", f.name), cancel, &mut |got| progress(before + got, total))?;
        }
        before += f.bytes;
    }
    progress(total, total);
    Ok(())
}

/// A file already in place that was checked when it was fetched (its `.ok` mark holds the hash).
fn checked(d: &Path, f: &File) -> bool {
    std::fs::symlink_metadata(d.join(f.name)).map_or(false, |m| m.is_file() && m.len() == f.bytes)
        && std::fs::read_to_string(d.join(format!("{}.ok", f.name))).map_or(false, |s| s == f.sha256)
}

/// The partial file's companion: where it came from, the server's tag for it, and how much is in.
fn part_paths(d: &Path, f: &File) -> (PathBuf, PathBuf) { (d.join(format!("{}.part", f.name)), d.join(format!("{}.part.json", f.name))) }

fn forget_part(d: &Path, f: &File) {
    let (part, meta) = part_paths(d, f);
    let _ = std::fs::remove_file(part);
    let _ = std::fs::remove_file(meta);
}

const MAX_REDIRECTS: usize = 5;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        // Redirects are followed by hand, each one checked against `allowed`.
        .redirects(0)
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("thoughts/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Ask for `url` from byte `from` on, following allowed redirects by hand.
fn request(url: &str, from: u64, etag: Option<&str>) -> anyhow::Result<ureq::Response> {
    let a = agent();
    let mut at = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        if !allowed(&at) { return Err(crate::tr::err("meaning.bad_host", &[])); }
        let mut r = a.get(&at);
        if from > 0 {
            r = r.set("Range", &format!("bytes={from}-"));
            if let Some(t) = etag { r = r.set("If-Range", t); }
        }
        let res = match r.call() {
            Ok(res) => res,
            Err(ureq::Error::Status(code, res)) if (300..400).contains(&code) => res,
            Err(ureq::Error::Status(416, res)) => res,
            Err(ureq::Error::Status(code, _)) => return Err(crate::tr::err_detail("meaning.download_failed", &[], format!("the server answered {code}"))),
            Err(e) => return Err(crate::tr::err_detail("meaning.download_failed", &[], e)),
        };
        if (300..400).contains(&res.status()) {
            let Some(next) = res.header("location").map(str::to_string) else { return Err(crate::tr::err("meaning.download_failed", &[])) };
            // A relative address stays on the host that sent it.
            at = if next.starts_with('/') {
                let scheme_host: String = at.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
                format!("{scheme_host}{next}")
            } else { next };
            continue;
        }
        return Ok(res);
    }
    Err(crate::tr::err("meaning.download_failed", &[]))
}

/// Fetch one file, continuing a partial one when the server can. Checked before it takes its name.
pub(crate) fn fetch(d: &Path, f: &File, url: &str, cancel: &AtomicBool, progress: &mut dyn FnMut(u64)) -> anyhow::Result<()> {
    let (part, meta) = part_paths(d, f);
    let saved: Value = std::fs::read_to_string(&meta).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    let mut got = std::fs::symlink_metadata(&part).map_or(0, |m| if m.is_file() { m.len() } else { 0 });
    // Another source, or more than the file can hold: start over.
    if saved["url"].as_str() != Some(url) || got > f.bytes { forget_part(d, f); got = 0; }
    // The hash goes on from what is already in: read back once (a SHA-256 cannot be saved half-way).
    let mut hash = Sha256::new();
    if got > 0 {
        let mut r = std::fs::File::open(&part)?;
        let mut buf = vec![0u8; 1 << 16];
        let mut left = got;
        while left > 0 {
            let n = r.read(&mut buf[..(left.min(1 << 16)) as usize])?;
            if n == 0 { break; }
            hash.update(&buf[..n]);
            left -= n as u64;
        }
    }
    let etag = saved["etag"].as_str().map(str::to_string);
    let res = request(url, got, etag.as_deref())?;
    let status = res.status();
    if status == 416 && got == f.bytes {
        // Everything was in already.
    } else {
        if status == 200 && got > 0 {
            // The server sends the whole file (it changed, or cannot send a part): start over.
            got = 0;
            hash = Sha256::new();
        } else if status != 200 && status != 206 {
            forget_part(d, f);
            return Err(crate::tr::err_detail("meaning.download_failed", &[], format!("the server answered {status}")));
        }
        let new_tag = res.header("etag").map(str::to_string).or(etag);
        let mut out = std::fs::OpenOptions::new().create(true).write(true).append(got > 0).truncate(got == 0).open(&part)
            .with_context(|| format!("could not write {}", part.display()))?;
        let keep = |got: u64| { let _ = std::fs::write(&meta, json!({"url": url, "etag": new_tag, "got": got}).to_string()); };
        keep(got);
        let mut body = res.into_reader().take(f.bytes - got + 1);
        let mut buf = vec![0u8; 1 << 16];
        let mut last = Instant::now();
        progress(got);
        loop {
            if cancel.load(Ordering::Relaxed) {
                out.flush()?;
                keep(got);
                return Err(Stopped::Cancelled.into());
            }
            let n = match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => { out.flush()?; keep(got); return Err(crate::tr::err_detail("meaning.download_failed", &[], e)); }
            };
            out.write_all(&buf[..n])?;
            hash.update(&buf[..n]);
            got += n as u64;
            if got > f.bytes { break; }
            if last.elapsed() >= Duration::from_millis(250) { last = Instant::now(); keep(got); progress(got); }
        }
        out.sync_all()?;
        keep(got);
        if got < f.bytes {
            // The connection ended early: what came stays, and the next try goes on from there.
            return Err(crate::tr::err_detail("meaning.download_failed", &[], "the connection ended before the file was complete"));
        }
    }
    let sum = format!("{:x}", hash.finalize());
    if got != f.bytes || sum != f.sha256 {
        forget_part(d, f);
        return Err(crate::tr::err_detail("meaning.bad_download", &[], format!("{}: {got} bytes, sha256 {sum}", f.name)));
    }
    std::fs::rename(&part, d.join(f.name))?;
    let _ = std::fs::remove_file(&meta);
    let _ = std::fs::write(d.join(format!("{}.ok", f.name)), f.sha256);
    progress(f.bytes);
    Ok(())
}

/// How much is already in, for showing the bar before the download goes on.
pub fn got_so_far(home: &Path) -> u64 {
    let d = dir(home);
    FILES.iter().map(|f| {
        let done = std::fs::symlink_metadata(d.join(f.name)).map_or(0, |m| if m.is_file() { m.len() } else { 0 });
        if done == f.bytes { done } else { std::fs::symlink_metadata(part_paths(&d, f).0).map_or(0, |m| m.len().min(f.bytes)) }
    }).sum()
}

// ---------- turning text into vectors ----------

/// Anything that turns texts into unit vectors of `DIMS` numbers. The real model in the app; a
/// small stand-in in tests.
pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
}

pub fn query_text(q: &str) -> String { format!("query: {q}") }
pub fn passage_text(title: &str, body: &str) -> String {
    if title.is_empty() { format!("passage: {body}") } else { format!("passage: {title}\n{body}") }
}

/// The model itself, on the CPU (Apple's Accelerate framework on a Mac), weights as f32.
pub struct E5 {
    model: candle_transformers::models::bert::BertModel,
    tok: tokenizers::Tokenizer,
    device: candle_core::Device,
}

impl E5 {
    pub fn load(home: &Path) -> anyhow::Result<Self> {
        use candle_transformers::models::bert::{BertModel, Config};
        let d = dir(home);
        if !installed(home) { anyhow::bail!("the model is not installed"); }
        let device = candle_core::Device::Cpu;
        let config: Config = serde_json::from_str(&std::fs::read_to_string(d.join("config.json"))?)?;
        let mut tok = tokenizers::Tokenizer::from_file(d.join("tokenizer.json")).map_err(|e| anyhow::anyhow!("tokenizer: {e}"))?;
        tok.with_padding(Some(tokenizers::PaddingParams { strategy: tokenizers::PaddingStrategy::BatchLongest, ..Default::default() }));
        tok.with_truncation(Some(tokenizers::TruncationParams { max_length: MAX_TOKENS, ..Default::default() })).map_err(|e| anyhow::anyhow!("tokenizer: {e}"))?;
        // Safety: the file is ours (checked against its pinned hash), and it is not changed while mapped.
        let vb = unsafe { candle_nn::VarBuilder::from_mmaped_safetensors(&[d.join("model.safetensors")], candle_core::DType::F32, &device)? };
        let model = BertModel::load(vb, &config)?;
        Ok(E5 { model, tok, device })
    }
}

impl Embedder for E5 {
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        use candle_core::{DType, Tensor, D};
        if texts.is_empty() { return Ok(vec![]); }
        let enc = self.tok.encode_batch(texts.to_vec(), true).map_err(|e| anyhow::anyhow!("tokenizer: {e}"))?;
        let ids: Vec<Tensor> = enc.iter().map(|e| Tensor::new(e.get_ids(), &self.device)).collect::<Result<_, _>>()?;
        let mask: Vec<Tensor> = enc.iter().map(|e| Tensor::new(e.get_attention_mask(), &self.device)).collect::<Result<_, _>>()?;
        let ids = Tensor::stack(&ids, 0)?;
        let mask = Tensor::stack(&mask, 0)?;
        let types = ids.zeros_like()?;
        let out = self.model.forward(&ids, &types, Some(&mask))?;
        // Mean over the real tokens, then unit length.
        let m = mask.to_dtype(DType::F32)?.unsqueeze(2)?;
        let sum = out.broadcast_mul(&m)?.sum(1)?;
        let n = m.sum(1)?;
        let mean = sum.broadcast_div(&n)?;
        let norm = mean.sqr()?.sum_keepdim(D::Minus1)?.sqrt()?;
        let unit = mean.broadcast_div(&norm)?;
        Ok(unit.to_vec2::<f32>()?)
    }
}
