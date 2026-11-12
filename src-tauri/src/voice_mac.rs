// macOS side of voice in (see voice.rs): AVFoundation for the microphone, the Speech framework for
// turning speech into text, always with `requiresOnDeviceRecognition` (audio never leaves the Mac).
//
// Dictation: AVAudioEngine's input feeds an SFSpeechAudioBufferRecognitionRequest; the words come
// back as `partial` events and Stop answers with the whole text.
// Meetings: AVAudioRecorder writes AAC (mono, speech quality, about 15 MB an hour) straight into
// the item's meetings folder. A recording is transcribed in pieces of about three minutes, each cut
// at the quietest moment near its end and read from the file straight into a recognition request
// (no copies on disk): memory stays small whatever the length, progress is exact, Cancel takes
// effect within a piece, and no single recognition request has to cope with an hour.
//
// All Objective-C calls are made with plain `msg_send!` on classes looked up at run time (no
// framework crates); calls that can raise an Objective-C exception go through `catch`. Results from
// the recognizer arrive on its own serial queue and are only copied into Rust data there; everything
// else happens on threads of this module.

use super::{emit, fail};
use block2::RcBlock;
use objc2::rc::{autoreleasepool, Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{class, msg_send, sel};
use serde_json::{json, Value};
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::SeqCst};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use thoughtsd::voice::{self as v, Merger, Piece, Word};

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeAudio: *const AnyObject;
    static AVFormatIDKey: *const AnyObject;
    static AVSampleRateKey: *const AnyObject;
    static AVNumberOfChannelsKey: *const AnyObject;
    static AVEncoderAudioQualityKey: *const AnyObject;
    static AVEncoderBitRateKey: *const AnyObject;
}
#[link(name = "Speech", kind = "framework")]
extern "C" {}
extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
}

/// A recording is transcribed in pieces of about this many seconds.
const CHUNK_SECS: f64 = 180.0;
/// No word from the recognizer for this long: it is stuck.
const STUCK: Duration = Duration::from_secs(300);

// ---------- small Objective-C helpers ----------

/// An Objective-C object handed between this module's threads. Only objects whose methods are
/// called from one place at a time, or are documented as thread-safe, are kept in one.
#[derive(Clone)]
struct Obj(Retained<AnyObject>);
unsafe impl Send for Obj {}
unsafe impl Sync for Obj {}
impl std::ops::Deref for Obj {
    type Target = AnyObject;
    fn deref(&self) -> &AnyObject { &self.0 }
}

fn cls(name: &CStr) -> Result<&'static AnyClass, String> {
    if let Some(c) = AnyClass::get(name) { return Ok(c); }
    static LOAD: std::sync::Once = std::sync::Once::new();
    LOAD.call_once(|| unsafe {
        for p in [c"/System/Library/Frameworks/Speech.framework/Speech", c"/System/Library/Frameworks/AVFoundation.framework/AVFoundation"] {
            dlopen(p.as_ptr(), 1);
        }
    });
    AnyClass::get(name).ok_or_else(|| fail("unsupported", Some(&name.to_string_lossy())))
}

/// Run Objective-C calls that may raise an exception (they would otherwise end the app).
fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    objc2::exception::catch(std::panic::AssertUnwindSafe(f)).map_err(|e| fail("failed", Some(&e.map(|e| format!("{e:?}")).unwrap_or_else(|| "exception".into()))))
}

fn key(k: *const AnyObject) -> &'static AnyObject { unsafe { &*k } }
fn media_audio() -> &'static AnyObject { key(unsafe { AVMediaTypeAudio }) }

fn nsstring(s: &str) -> Retained<AnyObject> {
    let c = CString::new(s.replace('\0', "")).unwrap_or_default();
    unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
}

fn string(o: &AnyObject) -> String {
    let p: *const c_char = unsafe { msg_send![o, UTF8String] };
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}
fn opt_string(o: Option<Retained<AnyObject>>) -> String { o.map(|s| string(&s)).unwrap_or_default() }

fn array(a: Option<Retained<AnyObject>>) -> Vec<Retained<AnyObject>> {
    let mut out = Vec::new();
    let Some(a) = a else { return out };
    let n: usize = unsafe { msg_send![&*a, count] };
    for i in 0..n.min(1_000_000) {
        let o: Option<Retained<AnyObject>> = unsafe { msg_send![&*a, objectAtIndex: i] };
        if let Some(o) = o { out.push(o); }
    }
    out
}

fn num_f64(x: f64) -> Retained<AnyObject> { unsafe { msg_send![class!(NSNumber), numberWithDouble: x] } }
fn num_u32(x: u32) -> Retained<AnyObject> { unsafe { msg_send![class!(NSNumber), numberWithUnsignedInt: x] } }

fn file_url(p: &Path) -> Retained<AnyObject> {
    let s = nsstring(&p.to_string_lossy());
    unsafe { msg_send![class!(NSURL), fileURLWithPath: &*s] }
}

fn dict(items: &[(&AnyObject, Retained<AnyObject>)]) -> Retained<AnyObject> {
    let d: Retained<AnyObject> = unsafe { msg_send![class!(NSMutableDictionary), new] };
    for (k, val) in items {
        let _: () = unsafe { msg_send![&*d, setObject: &**val, forKey: *k] };
    }
    d
}

/// An `NSError` as plain data.
#[derive(Clone, Debug)]
struct NsErr { domain: String, code: isize, text: String }
impl NsErr {
    fn read(e: *mut AnyObject) -> Option<NsErr> {
        let e = unsafe { e.as_ref() }?;
        let domain: Option<Retained<AnyObject>> = unsafe { msg_send![e, domain] };
        let code: isize = unsafe { msg_send![e, code] };
        let text: Option<Retained<AnyObject>> = unsafe { msg_send![e, localizedDescription] };
        Some(NsErr { domain: opt_string(domain), code, text: opt_string(text) })
    }
    /// "No speech detected": silence, not a failure.
    fn no_speech(&self) -> bool { self.code == 1110 || self.text.to_lowercase().contains("no speech") }
    fn detail(&self) -> String { format!("{} ({} {})", self.text, self.domain, self.code) }
}

// ---------- availability and permission ----------

/// Asking the system for the microphone or for speech recognition ends the app at once unless its
/// Info.plist says why it asks. A build without those words (a plain `cargo build` run outside the
/// app bundle on an old toolchain) reports voice as unavailable instead.
fn bundle_ready() -> bool {
    let b: Option<Retained<AnyObject>> = unsafe { msg_send![class!(NSBundle), mainBundle] };
    let Some(b) = b else { return false };
    ["NSMicrophoneUsageDescription", "NSSpeechRecognitionUsageDescription"].iter().all(|k| {
        let k = nsstring(k);
        let v: Option<Retained<AnyObject>> = unsafe { msg_send![&*b, objectForInfoDictionaryKey: &*k] };
        v.is_some()
    })
}

fn ready() -> Result<(), String> {
    if !bundle_ready() { return Err(fail("unsupported", Some("build"))); }
    cls(c"SFSpeechRecognizer")?;
    cls(c"AVAudioEngine")?;
    Ok(())
}

fn mic_status() -> &'static str {
    let Ok(c) = cls(c"AVCaptureDevice") else { return "restricted" };
    let s: isize = unsafe { msg_send![c, authorizationStatusForMediaType: media_audio()] };
    // AVAuthorizationStatus: 0 not asked, 1 restricted, 2 denied, 3 allowed.
    match s { 3 => "granted", 2 => "denied", 1 => "restricted", _ => "ask" }
}

fn speech_status() -> &'static str {
    let Ok(c) = cls(c"SFSpeechRecognizer") else { return "restricted" };
    let s: isize = unsafe { msg_send![c, authorizationStatus] };
    // SFSpeechRecognizerAuthorizationStatus: 0 not asked, 1 denied, 2 restricted, 3 allowed.
    match s { 3 => "granted", 1 => "denied", 2 => "restricted", _ => "ask" }
}

fn preferred_languages() -> Vec<String> {
    let a: Option<Retained<AnyObject>> = unsafe { msg_send![class!(NSLocale), preferredLanguages] };
    array(a).iter().map(|s| string(s)).collect()
}

fn recognizer(locale: &str) -> Option<Obj> {
    let c = cls(c"SFSpeechRecognizer").ok()?;
    let id = nsstring(locale);
    let loc: Option<Retained<AnyObject>> = unsafe { msg_send![class!(NSLocale), localeWithLocaleIdentifier: &*id] };
    let loc = loc?;
    let a: Allocated<AnyObject> = unsafe { msg_send![c, alloc] };
    let r: Option<Retained<AnyObject>> = unsafe { msg_send![a, initWithLocale: &*loc] };
    r.map(Obj)
}

fn on_device(r: &AnyObject) -> bool {
    r.class().responds_to(sel!(supportsOnDeviceRecognition)) && unsafe { msg_send![r, supportsOnDeviceRecognition] }
}
fn available(r: &AnyObject) -> bool { unsafe { msg_send![r, isAvailable] } }

struct Lang { locale: String, on_device: bool, available: bool, recognizer: Option<Obj> }

/// The recognizer for a language: the first locale that runs on this Mac; otherwise what there is
/// (`on_device: false`, no recognizer: it is never used).
fn pick(lang: &str) -> Option<Lang> {
    let mut first: Option<Lang> = None;
    for id in v::locale_candidates(lang, &preferred_languages()) {
        let Some(r) = recognizer(&id) else { continue };
        let (on, av) = (on_device(&r), available(&r));
        if on && av { return Some(Lang { locale: id, on_device: true, available: true, recognizer: Some(r) }); }
        if first.is_none() { first = Some(Lang { locale: id, on_device: on, available: av, recognizer: None }); }
    }
    first
}

pub fn status() -> Value {
    autoreleasepool(|_| {
        if let Err(e) = ready() {
            let reason = if e.ends_with("::build") { "build" } else { "os" };
            return json!({"supported": false, "reason": reason});
        }
        let lang = |l: &str| match pick(l) {
            Some(x) => json!({"locale": x.locale, "onDevice": x.on_device, "available": x.available}),
            None => json!({"locale": null, "onDevice": false, "available": false}),
        };
        let mut s = state();
        s["supported"] = json!(true);
        s["mic"] = json!(mic_status());
        s["speech"] = json!(speech_status());
        s["langs"] = json!({"en": lang("en"), "ar": lang("ar")});
        s
    })
}

/// Wait for a completion handler to be called (the user answering the system's question).
fn wait_for(f: impl FnOnce(RcBlock<dyn Fn()>)) {
    let (tx, rx) = mpsc::channel::<()>();
    let tx = Mutex::new(Some(tx));
    let done = RcBlock::new(move || { if let Some(t) = tx.lock().ok().and_then(|mut g| g.take()) { let _ = t.send(()); } });
    f(done);
    let _ = rx.recv_timeout(Duration::from_secs(600));
}

pub fn permit(mic: bool) -> Value {
    if ready().is_err() { return status(); }
    autoreleasepool(|_| {
        if speech_status() == "ask" {
            if let Ok(c) = cls(c"SFSpeechRecognizer") {
                wait_for(|done| {
                    let h = RcBlock::new(move |_status: isize| done.call(()));
                    let _: () = unsafe { msg_send![c, requestAuthorization: &*h] };
                });
            }
        }
        if mic && mic_status() == "ask" {
            if let Ok(c) = cls(c"AVCaptureDevice") {
                wait_for(|done| {
                    let h = RcBlock::new(move |_granted: Bool| done.call(()));
                    let _: () = unsafe { msg_send![c, requestAccessForMediaType: media_audio(), completionHandler: &*h] };
                });
            }
        }
    });
    status()
}

fn need(mic: bool) -> Result<(), String> {
    ready()?;
    if speech_status() != "granted" { return Err(fail("speech-permission", Some(speech_status()))); }
    if mic && mic_status() != "granted" { return Err(fail("mic-permission", Some(mic_status()))); }
    Ok(())
}

/// The recognizer to use for `lang`, or why there is none that stays on this Mac.
fn on_device_recognizer(lang: &str) -> Result<(Obj, String), String> {
    let l = pick(lang).ok_or_else(|| fail("not-on-device", Some(v::lang(lang))))?;
    match l.recognizer {
        Some(r) => Ok((r, l.locale)),
        None => Err(fail("not-on-device", Some(&l.locale))),
    }
}

/// Results are delivered on a queue of their own, never on the main thread.
fn use_own_queue(r: &AnyObject) {
    let q: Retained<AnyObject> = unsafe { msg_send![class!(NSOperationQueue), new] };
    let _: () = unsafe { msg_send![&*q, setMaxConcurrentOperationCount: 1isize] };
    let _: () = unsafe { msg_send![r, setQueue: &*q] };
}

/// On this Mac only, with punctuation, as dictation. Refused if the recognizer would not promise
/// to keep the audio here.
fn configure(req: &AnyObject) -> Result<(), String> {
    let _: () = unsafe { msg_send![req, setShouldReportPartialResults: true] };
    if !req.class().responds_to(sel!(setRequiresOnDeviceRecognition:)) { return Err(fail("not-on-device", None)); }
    let _: () = unsafe { msg_send![req, setRequiresOnDeviceRecognition: true] };
    let on: bool = unsafe { msg_send![req, requiresOnDeviceRecognition] };
    if !on { return Err(fail("not-on-device", None)); }
    if req.class().responds_to(sel!(setAddsPunctuation:)) {
        let _: () = unsafe { msg_send![req, setAddsPunctuation: true] };
    }
    let _: () = unsafe { msg_send![req, setTaskHint: 1isize] }; // SFSpeechRecognitionTaskHintDictation
    Ok(())
}

// ---------- results from the recognizer ----------

enum Heard {
    /// `finished`: an utterance (or everything) is done; `last`: nothing more will come.
    Result { text: String, words: Vec<Word>, finished: bool, last: bool },
    Error(NsErr),
}

fn read_result(r: &AnyObject) -> Heard {
    let mut text = String::new();
    let mut words = Vec::new();
    let best: Option<Retained<AnyObject>> = unsafe { msg_send![r, bestTranscription] };
    if let Some(b) = &best {
        text = opt_string(unsafe { msg_send![&**b, formattedString] });
        let segs: Option<Retained<AnyObject>> = unsafe { msg_send![&**b, segments] };
        for s in array(segs) {
            let t: Option<Retained<AnyObject>> = unsafe { msg_send![&*s, substring] };
            let start: f64 = unsafe { msg_send![&*s, timestamp] };
            let dur: f64 = unsafe { msg_send![&*s, duration] };
            words.push(Word { text: opt_string(t), start, dur });
        }
    }
    let last: bool = unsafe { msg_send![r, isFinal] };
    // Newer systems close each utterance with its metadata (and then start the next one afresh).
    let utterance = r.class().responds_to(sel!(speechRecognitionMetadata)) && {
        let m: Option<Retained<AnyObject>> = unsafe { msg_send![r, speechRecognitionMetadata] };
        m.is_some()
    };
    Heard::Result { text, words, finished: last || utterance, last }
}

/// The recognizer's result handler: copies each result into plain data and passes it on.
fn result_handler(tx: mpsc::Sender<Heard>) -> RcBlock<dyn Fn(*mut AnyObject, *mut AnyObject)> {
    RcBlock::new(move |result: *mut AnyObject, error: *mut AnyObject| {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| autoreleasepool(|_| {
            if let Some(r) = unsafe { result.as_ref() } { let _ = tx.send(read_result(r)); }
            if let Some(e) = NsErr::read(error) { let _ = tx.send(Heard::Error(e)); }
        })));
    })
}

fn start_task(recognizer: &AnyObject, request: &AnyObject, tx: mpsc::Sender<Heard>) -> Result<Obj, String> {
    let handler = result_handler(tx);
    let task: Option<Retained<AnyObject>> = catch(|| unsafe { msg_send![recognizer, recognitionTaskWithRequest: request, resultHandler: &*handler] })?;
    task.map(Obj).ok_or_else(|| fail("failed", Some("no recognition task")))
}

/// Loudness of a buffer, 0..1 on a decibel scale (-50 dB and below is 0).
fn level_of(b: &AnyObject) -> f32 {
    let n: u32 = unsafe { msg_send![b, frameLength] };
    let data: *const *const f32 = unsafe { msg_send![b, floatChannelData] };
    if data.is_null() || n == 0 { return 0.0; }
    let ch = unsafe { *data };
    if ch.is_null() { return 0.0; }
    let s = unsafe { std::slice::from_raw_parts(ch, n as usize) };
    let rms = (s.iter().map(|x| x * x).sum::<f32>() / n as f32).sqrt();
    db_level(20.0 * rms.max(1e-6).log10())
}
fn db_level(db: f32) -> f32 { ((db + 50.0) / 50.0).clamp(0.0, 1.0) }

// ---------- state ----------

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
fn next_id() -> u64 { NEXT_ID.fetch_add(1, SeqCst) }

struct Dictation {
    id: u64,
    engine: Obj,
    input: Obj,
    request: Obj,
    task: Obj,
    _recognizer: Obj,
    stopping: Arc<AtomicBool>,
    outcome: Arc<(Mutex<Option<Result<String, String>>>, Condvar)>,
    latest: Arc<Mutex<String>>,
}

struct Recording {
    id: u64,
    recorder: Obj,
    abs: PathBuf,
    rel: String,
    item: String,
    title: String,
    lang: String,
    date: String,
    seconds: f64,
    paused: bool,
    level: f32,
    /// Why it stopped by itself ("limit", "interrupted"); then it only waits for `record_stop`.
    ended: Option<&'static str>,
    /// Checks in a row that found it not recording although it was not paused.
    misses: u8,
}

struct Job {
    id: u64,
    rel: String,
    meta: Value,
    cancel: Arc<AtomicBool>,
    task: Arc<Mutex<Option<Obj>>>,
    progress: Arc<Mutex<(f64, f64)>>,
}

static DICTATION: Mutex<Option<Dictation>> = Mutex::new(None);
static RECORDING: Mutex<Option<Recording>> = Mutex::new(None);
static JOB: Mutex<Option<Job>> = Mutex::new(None);
static RESULT: Mutex<Option<Value>> = Mutex::new(None);
static LEVEL: AtomicU32 = AtomicU32::new(0);
static TICKING: AtomicBool = AtomicBool::new(false);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> { m.lock().unwrap_or_else(|e| e.into_inner()) }

fn rec_json(r: &Recording) -> Value {
    json!({"path": r.rel, "item": r.item, "title": r.title, "lang": r.lang, "date": r.date,
           "seconds": r.seconds, "paused": r.paused, "level": r.level, "ended": r.ended})
}

fn state() -> Value {
    let rec = lock(&RECORDING).as_ref().map(rec_json);
    let job = lock(&JOB).as_ref().map(|j| { let (done, total) = *lock(&j.progress); json!({"path": j.rel, "meta": j.meta, "done": done, "total": total}) });
    let result = lock(&RESULT).as_ref().map(|r| r["path"].clone());
    json!({"dictating": lock(&DICTATION).is_some(), "recording": rec, "transcribing": job, "result": result})
}

/// Ten times a second while dictating or recording: the level (and, for a recording, its time),
/// and the checks that end a recording by itself.
fn start_ticker() {
    if TICKING.swap(true, SeqCst) { return; }
    std::thread::spawn(|| {
        let mut n: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            n += 1;
            let dictating = lock(&DICTATION).is_some();
            if dictating { emit(json!({"type": "level", "level": f32::from_bits(LEVEL.load(SeqCst))})); }
            let recording = if n % 2 == 0 { tick_recording() } else { lock(&RECORDING).as_ref().is_some_and(|r| r.ended.is_none()) };
            if !dictating && !recording {
                TICKING.store(false, SeqCst);
                // Something started just now: keep going.
                let again = lock(&DICTATION).is_some() || lock(&RECORDING).as_ref().is_some_and(|r| r.ended.is_none());
                if !again || TICKING.swap(true, SeqCst) { break; }
            }
        }
    });
}

fn tick_recording() -> bool {
    let Some((rec, paused, id)) = lock(&RECORDING).as_ref().filter(|r| r.ended.is_none()).map(|r| (r.recorder.clone(), r.paused, r.id)) else { return false };
    let (secs, power, running) = autoreleasepool(|_| unsafe {
        let _: () = msg_send![&*rec, updateMeters];
        let power: f32 = msg_send![&*rec, averagePowerForChannel: 0usize];
        let secs: f64 = msg_send![&*rec, currentTime];
        let running: bool = msg_send![&*rec, isRecording];
        (secs, power, running)
    });
    let limit = secs >= v::MAX_RECORDING_SECS;
    if limit { let _: () = unsafe { msg_send![&*rec, stop] }; }
    let mut g = lock(&RECORDING);
    let Some(r) = g.as_mut().filter(|r| r.id == id) else { return false };
    // Not recording although not paused (and not just being paused or resumed): twice in a row means
    // the system stopped it (the microphone went away).
    r.misses = if !paused && !r.paused && !running { r.misses + 1 } else { 0 };
    let ended = if limit { Some("limit") } else if r.misses >= 2 { Some("interrupted") } else { None };
    if !paused { r.seconds = r.seconds.max(secs); }
    r.level = if r.paused { 0.0 } else { db_level(power) };
    r.ended = ended;
    let v = rec_json(r);
    drop(g);
    emit(json!({"type": "rec", "recording": v}));
    if let Some(reason) = ended { emit(json!({"type": "rec-ended", "reason": reason, "recording": v})); }
    ended.is_none()
}

// ---------- dictation ----------

pub fn dictate_start(lang: &str) -> Result<Value, String> {
    autoreleasepool(|_| {
        need(true)?;
        if lock(&DICTATION).is_some() { return Err(fail("busy", Some("dictating"))); }
        if lock(&RECORDING).is_some() { return Err(fail("busy", Some("recording"))); }
        let (recognizer, locale) = on_device_recognizer(lang)?;
        use_own_queue(&recognizer);
        let request: Retained<AnyObject> = unsafe { msg_send![cls(c"SFSpeechAudioBufferRecognitionRequest")?, new] };
        configure(&request)?;
        let engine: Retained<AnyObject> = unsafe { msg_send![cls(c"AVAudioEngine")?, new] };
        let input: Option<Retained<AnyObject>> = catch(|| unsafe { msg_send![&*engine, inputNode] })?;
        let input = input.ok_or_else(|| fail("no-input", None))?;
        let format: Option<Retained<AnyObject>> = unsafe { msg_send![&*input, outputFormatForBus: 0usize] };
        let format = format.ok_or_else(|| fail("no-input", None))?;
        let channels: u32 = unsafe { msg_send![&*format, channelCount] };
        let rate: f64 = unsafe { msg_send![&*format, sampleRate] };
        if channels == 0 || rate <= 0.0 { return Err(fail("no-input", None)); }

        let feed = Obj(request.clone());
        LEVEL.store(0f32.to_bits(), SeqCst);
        let tap = RcBlock::new(move |buffer: *mut AnyObject, _when: *mut AnyObject| {
            if let Some(b) = unsafe { buffer.as_ref() } {
                let _: () = unsafe { msg_send![&*feed, appendAudioPCMBuffer: b] };
                LEVEL.store(level_of(b).to_bits(), SeqCst);
            }
        });
        catch(|| unsafe { let _: () = msg_send![&*input, installTapOnBus: 0usize, bufferSize: 1024u32, format: &*format, block: &*tap]; })?;
        let stop_input = || unsafe {
            let _ = catch(|| { let _: () = msg_send![&*engine, stop]; let _: () = msg_send![&*input, removeTapOnBus: 0usize]; });
        };
        let _: () = unsafe { msg_send![&*engine, prepare] };
        let mut err: *mut AnyObject = null_mut();
        let started: Result<bool, String> = catch(|| unsafe { msg_send![&*engine, startAndReturnError: &mut err] });
        if started != Ok(true) {
            let detail = NsErr::read(err).map(|e| e.detail()).or(started.err()).unwrap_or_default();
            stop_input();
            return Err(fail("no-input", Some(&detail)));
        }
        let (tx, rx) = mpsc::channel();
        let task = match start_task(&recognizer, &request, tx) {
            Ok(t) => t,
            Err(e) => { stop_input(); return Err(e); }
        };

        let id = next_id();
        let stopping = Arc::new(AtomicBool::new(false));
        let outcome = Arc::new((Mutex::new(None), Condvar::new()));
        let latest = Arc::new(Mutex::new(String::new()));
        {
            let (stopping, outcome, latest) = (stopping.clone(), outcome.clone(), latest.clone());
            std::thread::spawn(move || dictation_loop(id, rx, stopping, outcome, latest));
        }
        *lock(&DICTATION) = Some(Dictation { id, engine: Obj(engine.clone()), input: Obj(input.clone()), request: Obj(request), task, _recognizer: recognizer, stopping, outcome, latest });
        start_ticker();
        Ok(json!({"locale": locale}))
    })
}

fn dictation_loop(id: u64, rx: mpsc::Receiver<Heard>, stopping: Arc<AtomicBool>, outcome: Arc<(Mutex<Option<Result<String, String>>>, Condvar)>, latest: Arc<Mutex<String>>) {
    let mut m = Merger::new();
    let mut stop_seen: Option<Instant> = None;
    let result: Result<String, String> = loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Heard::Result { text, words, finished, last }) => {
                m.push(&text, &words, finished);
                let t = m.text();
                *lock(&latest) = t.clone();
                emit(json!({"type": "partial", "text": t}));
                if last { break Ok(t); }
            }
            Ok(Heard::Error(e)) => {
                let t = m.text();
                break if e.no_speech() || !t.is_empty() || stopping.load(SeqCst) { Ok(t) } else { Err(fail("failed", Some(&e.detail()))) };
            }
            Err(RecvTimeoutError::Timeout) => {
                // Stopped, and no last result after a while: what was heard is the answer.
                if stopping.load(SeqCst) && stop_seen.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(10) { break Ok(m.text()); }
            }
            Err(RecvTimeoutError::Disconnected) => break Ok(m.text()),
        }
    };
    {
        let (slot, cv) = &*outcome;
        *lock(slot) = Some(result.clone());
        cv.notify_all();
    }
    if stopping.load(SeqCst) { return; }
    // It ended by itself (the microphone went away, the recognizer gave up): tidy up and say so.
    let d = { let mut g = lock(&DICTATION); if g.as_ref().is_some_and(|d| d.id == id) { g.take() } else { None } };
    if let Some(d) = d { end_input(&d); }
    match result {
        Ok(text) => emit(json!({"type": "dictation-ended", "text": text, "error": null})),
        Err(e) => emit(json!({"type": "dictation-ended", "text": lock(&latest).clone(), "error": e})),
    }
}

fn end_input(d: &Dictation) {
    autoreleasepool(|_| {
        let _ = catch(|| unsafe {
            let _: () = msg_send![&*d.engine, stop];
            let _: () = msg_send![&*d.input, removeTapOnBus: 0usize];
            let _: () = msg_send![&*d.request, endAudio];
        });
    });
}

pub fn dictate_stop() -> Result<String, String> {
    let Some(d) = lock(&DICTATION).take() else { return Ok(String::new()) };
    d.stopping.store(true, SeqCst);
    end_input(&d);
    let (slot, cv) = &*d.outcome;
    let got = {
        let g = lock(slot);
        let (g, _) = cv.wait_timeout_while(g, Duration::from_secs(8), |o| o.is_none()).unwrap_or_else(|e| e.into_inner());
        g.clone()
    };
    let _ = catch(|| unsafe { let _: () = msg_send![&*d.task, cancel]; });
    got.unwrap_or_else(|| Ok(lock(&d.latest).clone()))
}

pub fn dictate_cancel() {
    let Some(d) = lock(&DICTATION).take() else { return };
    d.stopping.store(true, SeqCst);
    end_input(&d);
    let _ = catch(|| unsafe { let _: () = msg_send![&*d.task, cancel]; });
}

// ---------- meeting recordings ----------

fn library_root() -> Result<PathBuf, String> {
    thoughtsd::settings::Settings::load().library().ok_or_else(|| fail("library", Some("no library")))
}

fn make_recorder(abs: &Path) -> Result<Obj, String> {
    let url = file_url(abs);
    let recorder = cls(c"AVAudioRecorder")?;
    let mut last = String::new();
    // Speech quality first; the plainer settings if this Mac's encoder refuses them.
    for (rate, bitrate) in [(16000.0, Some(32000u32)), (16000.0, None), (44100.0, None)] {
        let mut items = vec![
            (key(unsafe { AVFormatIDKey }), num_u32(0x6161_6320)), // kAudioFormatMPEG4AAC ('aac ')
            (key(unsafe { AVSampleRateKey }), num_f64(rate)),
            (key(unsafe { AVNumberOfChannelsKey }), num_u32(1)),
            (key(unsafe { AVEncoderAudioQualityKey }), num_u32(0x60)), // AVAudioQualityHigh
        ];
        if let Some(b) = bitrate { items.push((key(unsafe { AVEncoderBitRateKey }), num_u32(b))); }
        let settings = dict(&items);
        let a: Allocated<AnyObject> = unsafe { msg_send![recorder, alloc] };
        let mut err: *mut AnyObject = null_mut();
        let r: Option<Retained<AnyObject>> = catch(|| unsafe { msg_send![a, initWithURL: &*url, settings: &*settings, error: &mut err] })?;
        let Some(r) = r else { last = NsErr::read(err).map(|e| e.detail()).unwrap_or_default(); continue };
        let ok = catch(|| unsafe {
            let _: () = msg_send![&*r, setMeteringEnabled: true];
            let prepared: bool = msg_send![&*r, prepareToRecord];
            prepared && { let started: bool = msg_send![&*r, record]; started }
        })?;
        if ok { return Ok(Obj(r)); }
        let _ = catch(|| unsafe { let _: () = msg_send![&*r, stop]; });
    }
    Err(fail("no-input", Some(&last)))
}

pub fn record_start(item: &str, title: &str, lang: &str) -> Result<Value, String> {
    autoreleasepool(|_| {
        ready()?;
        if mic_status() != "granted" { return Err(fail("mic-permission", Some(mic_status()))); }
        if lock(&RECORDING).is_some() { return Err(fail("busy", Some("recording"))); }
        if lock(&DICTATION).is_some() { return Err(fail("busy", Some("dictating"))); }
        let root = library_root()?;
        let lang = v::lang(lang);
        let today = v::today();
        let (rel, title) = v::recording_path(&root, item, title, today, lang == "ar").map_err(|e| fail("library", Some(&e.to_string())))?;
        let abs = thoughtsd::library::resolve(&root, &rel).map_err(|e| fail("library", Some(&e.to_string())))?;
        if let Some(dir) = abs.parent() { std::fs::create_dir_all(dir).map_err(|e| fail("library", Some(&e.to_string())))?; }
        // Checked again now that the folder exists: still inside the library, and the name still free.
        let abs = thoughtsd::library::resolve(&root, &rel).map_err(|e| fail("library", Some(&e.to_string())))?;
        std::fs::OpenOptions::new().write(true).create_new(true).open(&abs).map_err(|e| fail("library", Some(&e.to_string())))?;
        let recorder = match make_recorder(&abs) {
            Ok(r) => r,
            Err(e) => { let _ = std::fs::remove_file(&abs); return Err(e); }
        };
        let item = rel.split("/meetings/").next().unwrap_or("").to_string();
        let r = Recording { id: next_id(), recorder, abs, rel, item, title, lang: lang.into(), date: today.format("%Y-%m-%d").to_string(),
            seconds: 0.0, paused: false, level: 0.0, ended: None, misses: 0 };
        let v = rec_json(&r);
        *lock(&RECORDING) = Some(r);
        start_ticker();
        emit(json!({"type": "rec", "recording": v}));
        Ok(v)
    })
}

pub fn record_pause(paused: bool) -> Result<Value, String> {
    let mut g = lock(&RECORDING);
    let r = g.as_mut().ok_or_else(|| fail("not-recording", None))?;
    if r.ended.is_some() || r.paused == paused { return Ok(rec_json(r)); }
    let ok = autoreleasepool(|_| catch(|| unsafe {
        if paused { let _: () = msg_send![&*r.recorder, pause]; true } else { let b: bool = msg_send![&*r.recorder, record]; b }
    }))?;
    if !ok { return Err(fail("no-input", None)); }
    r.paused = paused;
    if paused { r.level = 0.0; }
    let v = rec_json(r);
    drop(g);
    emit(json!({"type": "rec", "recording": v}));
    start_ticker();
    Ok(v)
}

fn finish_recording(r: &Recording) -> f64 {
    autoreleasepool(|_| {
        let secs: f64 = unsafe { msg_send![&*r.recorder, currentTime] };
        let _ = catch(|| unsafe { let _: () = msg_send![&*r.recorder, stop]; });
        if r.paused || r.ended.is_some() { r.seconds } else { r.seconds.max(secs) }
    })
}

pub fn record_stop() -> Result<Value, String> {
    let r = lock(&RECORDING).take().ok_or_else(|| fail("not-recording", None))?;
    let seconds = finish_recording(&r);
    let mut v = rec_json(&r);
    v["seconds"] = json!(seconds);
    let size = std::fs::metadata(&r.abs).map(|m| m.len()).unwrap_or(0);
    drop(r.recorder);
    emit(json!({"type": "rec", "recording": null}));
    if size == 0 {
        let _ = std::fs::remove_file(&r.abs);
        return Err(fail("empty", None));
    }
    Ok(v)
}

pub fn record_discard() -> Result<(), String> {
    let r = lock(&RECORDING).take().ok_or_else(|| fail("not-recording", None))?;
    finish_recording(&r);
    drop(r.recorder);
    emit(json!({"type": "rec", "recording": null}));
    std::fs::remove_file(&r.abs).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }).map_err(|e| fail("library", Some(&e.to_string())))
}

// ---------- transcribing a recording ----------

enum Stop { Cancelled, Failed(String) }

pub fn transcribe(rel: &str, lang: &str, meta: Value) -> Result<Value, String> {
    autoreleasepool(|_| {
        need(false)?;
        if lock(&JOB).is_some() { return Err(fail("busy", Some("transcribing"))); }
        let rel = rel.trim().trim_start_matches("./").replace('\\', "/");
        if lock(&RECORDING).as_ref().is_some_and(|r| r.rel == rel) { return Err(fail("busy", Some("recording"))); }
        if !v::is_audio(&rel) { return Err(fail("library", Some("not a recording"))); }
        let root = library_root()?;
        let abs = thoughtsd::library::resolve(&root, &rel).map_err(|e| fail("library", Some(&e.to_string())))?;
        if !abs.is_file() { return Err(fail("library", Some("not a file"))); }
        let lang = v::lang(lang);
        let (_, locale) = on_device_recognizer(lang)?;
        let id = next_id();
        let cancel = Arc::new(AtomicBool::new(false));
        let task = Arc::new(Mutex::new(None));
        let progress = Arc::new(Mutex::new((0.0, 0.0)));
        *lock(&JOB) = Some(Job { id, rel: rel.clone(), meta: meta.clone(), cancel: cancel.clone(), task: task.clone(), progress: progress.clone() });
        let loc = locale.clone();
        std::thread::spawn(move || transcribe_run(id, abs, rel, loc, meta, cancel, task, progress));
        Ok(json!({"locale": locale}))
    })
}

#[allow(clippy::too_many_arguments)]
fn transcribe_run(id: u64, abs: PathBuf, rel: String, locale: String, meta: Value, cancel: Arc<AtomicBool>, task: Arc<Mutex<Option<Obj>>>, progress: Arc<Mutex<(f64, f64)>>) {
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut report = |done: f64, total: f64, force: bool| {
        *lock(&progress) = (done, total);
        if force || last_emit.elapsed() >= Duration::from_millis(400) {
            last_emit = Instant::now();
            emit(json!({"type": "progress", "path": rel, "done": done, "total": total}));
        }
    };
    let out = autoreleasepool(|_| {
        let r = recognizer(&locale).filter(|r| on_device(r)).ok_or_else(|| Stop::Failed(fail("not-on-device", Some(&locale))))?;
        use_own_queue(&r);
        transcribe_file(&abs, &r, &cancel, &task, &mut report)
    });
    match out {
        Ok((paragraphs, seconds, complete)) => {
            *lock(&RESULT) = Some(json!({"path": rel, "meta": meta, "paragraphs": paragraphs, "seconds": seconds, "complete": complete}));
            emit(json!({"type": "transcribed", "path": rel}));
        }
        Err(Stop::Cancelled) => emit(json!({"type": "transcribe-cancelled", "path": rel})),
        Err(Stop::Failed(e)) => emit(json!({"type": "transcribe-failed", "path": rel, "error": e})),
    }
    let mut g = lock(&JOB);
    if g.as_ref().is_some_and(|j| j.id == id) { *g = None; }
}

fn open_audio(abs: &Path) -> Result<(Obj, Obj, f64, i64), Stop> {
    let url = file_url(abs);
    let a: Allocated<AnyObject> = unsafe { msg_send![cls(c"AVAudioFile").map_err(Stop::Failed)?, alloc] };
    let mut err: *mut AnyObject = null_mut();
    let f: Option<Retained<AnyObject>> = catch(|| unsafe { msg_send![a, initForReading: &*url, error: &mut err] }).map_err(Stop::Failed)?;
    let f = f.ok_or_else(|| Stop::Failed(fail("unreadable", NsErr::read(err).map(|e| e.detail()).as_deref())))?;
    let fmt: Option<Retained<AnyObject>> = unsafe { msg_send![&*f, processingFormat] };
    let fmt = fmt.ok_or_else(|| Stop::Failed(fail("unreadable", None)))?;
    let rate: f64 = unsafe { msg_send![&*fmt, sampleRate] };
    let frames: i64 = unsafe { msg_send![&*f, length] };
    Ok((Obj(f), Obj(fmt), rate, frames))
}

fn pcm_buffer(fmt: &AnyObject, frames: u32) -> Result<Obj, Stop> {
    let a: Allocated<AnyObject> = unsafe { msg_send![cls(c"AVAudioPCMBuffer").map_err(Stop::Failed)?, alloc] };
    let b: Option<Retained<AnyObject>> = unsafe { msg_send![a, initWithPCMFormat: fmt, frameCapacity: frames] };
    b.map(Obj).ok_or_else(|| Stop::Failed(fail("failed", Some("no memory for audio"))))
}

/// Read up to `frames` frames from `pos` into `buf`; how many were read.
fn read_frames(file: &AnyObject, buf: &AnyObject, pos: i64, frames: u32) -> Result<u32, Stop> {
    let mut err: *mut AnyObject = null_mut();
    let ok: bool = catch(|| unsafe {
        let _: () = msg_send![file, setFramePosition: pos];
        msg_send![file, readIntoBuffer: buf, frameCount: frames, error: &mut err]
    }).map_err(Stop::Failed)?;
    if !ok { return Err(Stop::Failed(fail("unreadable", NsErr::read(err).map(|e| e.detail()).as_deref()))); }
    Ok(unsafe { msg_send![buf, frameLength] })
}

/// The quietest twentieth of a second in the 15 seconds before `end`: where to cut between two pieces.
fn quiet_point(file: &AnyObject, fmt: &AnyObject, rate: f64, end: i64) -> Option<i64> {
    let start = (end - (15.0 * rate) as i64).max(0);
    let n = u32::try_from(end - start).ok()?;
    let buf = pcm_buffer(fmt, n).ok()?;
    let got = read_frames(file, &buf, start, n).ok()? as usize;
    let data: *const *const f32 = unsafe { msg_send![&*buf, floatChannelData] };
    if data.is_null() || got == 0 { return None; }
    let ch = unsafe { *data };
    if ch.is_null() { return None; }
    let s = unsafe { std::slice::from_raw_parts(ch, got) };
    let block = ((0.05 * rate) as usize).max(1);
    let (best, _) = s.chunks_exact(block).enumerate()
        .map(|(i, c)| (i, c.iter().map(|x| x * x).sum::<f32>()))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;
    Some(start + (best * block + block / 2) as i64)
}

/// Recognise frames `from..to` of `file`: read a second at a time and handed to a buffer request
/// (each in a buffer of its own, which the request keeps until it has used it), then the end of the
/// audio is marked and the words are collected. `heard(seconds)` reports how far into the piece the
/// words have come. Answers the piece's words and whether all of it was recognised.
#[allow(clippy::too_many_arguments)]
fn recognize_piece(file: &AnyObject, fmt: &AnyObject, rate: f64, from: i64, to: i64, recognizer: &AnyObject, cancel: &AtomicBool,
    slot: &Mutex<Option<Obj>>, heard: &mut dyn FnMut(f64)) -> Result<(Vec<Piece>, bool), Stop> {
    let req: Retained<AnyObject> = unsafe { msg_send![cls(c"SFSpeechAudioBufferRecognitionRequest").map_err(Stop::Failed)?, new] };
    configure(&req).map_err(Stop::Failed)?;
    let (tx, rx) = mpsc::channel();
    let task = start_task(recognizer, &req, tx).map_err(Stop::Failed)?;
    *lock(slot) = Some(task.clone());
    let cancel_task = || { let _ = catch(|| unsafe { let _: () = msg_send![&*task, cancel]; }); };
    let cap = (rate as u32).max(4096);
    let mut pos = from;
    let mut fed: Result<(), Stop> = Ok(());
    while pos < to {
        if cancel.load(SeqCst) { fed = Err(Stop::Cancelled); break; }
        let n = (to - pos).min(cap as i64) as u32;
        let got = autoreleasepool(|_| -> Result<u32, Stop> {
            let buf = pcm_buffer(fmt, n)?;
            let got = read_frames(file, &buf, pos, n)?;
            if got > 0 { let _: () = unsafe { msg_send![&*req, appendAudioPCMBuffer: &*buf] }; }
            Ok(got)
        });
        match got {
            Ok(0) => break,
            Ok(g) => pos += g as i64,
            // The file ends a little before its stated length (compressed sound can): what was read is the piece.
            Err(_) if pos > from => break,
            Err(e) => { fed = Err(e); break; }
        }
    }
    let _: () = unsafe { msg_send![&*req, endAudio] };
    if let Err(e) = fed {
        cancel_task();
        *lock(slot) = None;
        return Err(e);
    }
    let mut m = Merger::new();
    let mut quiet = Instant::now();
    let out = loop {
        if cancel.load(SeqCst) { cancel_task(); break Err(Stop::Cancelled); }
        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(Heard::Result { text, words, finished, last }) => {
                quiet = Instant::now();
                m.push(&text, &words, finished);
                heard(m.heard_until());
                if last { break Ok((m.pieces(), true)); }
            }
            Ok(Heard::Error(e)) => {
                if cancel.load(SeqCst) { break Err(Stop::Cancelled); }
                let p = m.pieces();
                break if e.no_speech() { Ok((p, true)) } else if !p.is_empty() { Ok((p, false)) } else { Err(Stop::Failed(fail("failed", Some(&e.detail())))) };
            }
            Err(RecvTimeoutError::Timeout) => {
                if quiet.elapsed() > STUCK {
                    cancel_task();
                    let p = m.pieces();
                    break if p.is_empty() { Err(Stop::Failed(fail("failed", Some("the recognizer stopped answering")))) } else { Ok((p, false)) };
                }
            }
            Err(RecvTimeoutError::Disconnected) => break Ok((m.pieces(), true)),
        }
    };
    *lock(slot) = None;
    out
}

fn transcribe_file(abs: &Path, recognizer: &AnyObject, cancel: &AtomicBool, slot: &Mutex<Option<Obj>>, report: &mut dyn FnMut(f64, f64, bool)) -> Result<(Vec<v::Para>, f64, bool), Stop> {
    let (file, fmt, rate, frames) = open_audio(abs)?;
    if rate <= 0.0 || frames <= 0 { return Ok((vec![], 0.0, true)); }
    let total = frames as f64 / rate;
    report(0.0, total, true);
    let chunk = (CHUNK_SECS * rate) as i64;
    let mut pieces: Vec<Piece> = Vec::new();
    let (mut pos, mut complete, mut any) = (0i64, true, false);
    let mut last_err = String::new();
    while pos < frames {
        if cancel.load(SeqCst) { return Err(Stop::Cancelled); }
        let mut end = (pos + chunk).min(frames);
        if end < frames { end = quiet_point(&file, &fmt, rate, end).filter(|e| *e > pos).unwrap_or(end); }
        // No tiny last piece.
        if frames - end < (20.0 * rate) as i64 { end = frames; }
        let offset = pos as f64 / rate;
        let piece = recognize_piece(&file, &fmt, rate, pos, end, recognizer, cancel, slot, &mut |heard| report((offset + heard).min(total), total, false));
        match piece {
            Ok((p, whole)) => {
                any = true;
                complete &= whole;
                pieces.extend(p.into_iter().map(|x| Piece { start: x.start + offset, end: x.end + offset, text: x.text }));
            }
            Err(Stop::Cancelled) => return Err(Stop::Cancelled),
            Err(Stop::Failed(e)) => { complete = false; last_err = e; }
        }
        pos = end;
        report(pos as f64 / rate, total, true);
    }
    if !any { return Err(Stop::Failed(last_err)); }
    Ok((v::paragraphs(&pieces), total, complete))
}

pub fn transcribe_cancel() {
    let g = lock(&JOB);
    let Some(j) = g.as_ref() else { return };
    j.cancel.store(true, SeqCst);
    let t = lock(&j.task).clone();
    drop(g);
    if let Some(t) = t { let _ = catch(|| unsafe { let _: () = msg_send![&*t, cancel]; }); }
}

pub fn result() -> Value { lock(&RESULT).clone().unwrap_or(Value::Null) }

pub fn result_clear(path: &str) {
    let mut g = lock(&RESULT);
    if g.as_ref().is_some_and(|r| r["path"] == path) { *g = None; }
}

/// The app is quitting: a recording is finished properly (the file stays complete and can be
/// transcribed next time), dictation and transcription are stopped.
pub fn shutdown() {
    if let Some(r) = lock(&RECORDING).take() { finish_recording(&r); }
    dictate_cancel();
    transcribe_cancel();
}

#[cfg(test)]
mod tests {
    // Only what needs no permission: nothing here listens, records or asks the system anything.
    use super::*;

    /// A 16 kHz mono WAV: a second of tone, `quiet` seconds of silence, a second of tone.
    fn wav(path: &Path, quiet: u32) {
        let rate = 16000u32;
        let mut samples: Vec<i16> = Vec::new();
        let tone = |n: u32| (0..n).map(|i| ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12000.0) as i16).collect::<Vec<_>>();
        samples.extend(tone(rate));
        samples.extend(std::iter::repeat(0).take((rate * quiet) as usize));
        samples.extend(tone(rate));
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = Vec::new();
        out.extend(b"RIFF"); out.extend((36 + data.len() as u32).to_le_bytes()); out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes()); out.extend(1u16.to_le_bytes()); out.extend(1u16.to_le_bytes());
        out.extend(rate.to_le_bytes()); out.extend((rate * 2).to_le_bytes()); out.extend(2u16.to_le_bytes()); out.extend(16u16.to_le_bytes());
        out.extend(b"data"); out.extend((data.len() as u32).to_le_bytes()); out.extend(data);
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn strings_and_settings_cross_into_objective_c_and_back() {
        autoreleasepool(|_| {
            for s in ["Weekly sync", "مراجعة الحسابات", ""] { assert_eq!(string(&nsstring(s)), s); }
            let d = dict(&[(key(unsafe { AVSampleRateKey }), num_f64(16000.0)), (key(unsafe { AVFormatIDKey }), num_u32(0x6161_6320))]);
            let n: usize = unsafe { msg_send![&*d, count] };
            assert_eq!(n, 2);
            assert_eq!(string(media_audio()), "soun");
            assert_eq!(string(key(unsafe { AVFormatIDKey })), "AVFormatIDKey");
        });
    }

    #[test]
    fn languages_and_requests_are_set_up_on_this_mac_only() {
        autoreleasepool(|_| {
            assert!(cls(c"SFSpeechRecognizer").is_ok() && cls(c"AVAudioEngine").is_ok() && cls(c"AVAudioRecorder").is_ok());
            assert!(cls(c"NoSuchClassAnywhere").unwrap_err().starts_with("VOICE::unsupported"));
            for l in ["en", "ar"] {
                let x = pick(l).expect("macOS knows the language");
                assert!(x.locale.starts_with(l), "{}", x.locale);
                // A recognizer is only handed out when it runs on this Mac.
                assert_eq!(x.recognizer.is_some(), x.on_device && x.available);
                eprintln!("{l}: {} on this Mac: {}", x.locale, x.on_device);
            }
            let req: Retained<AnyObject> = unsafe { msg_send![cls(c"SFSpeechAudioBufferRecognitionRequest").unwrap(), new] };
            configure(&req).unwrap();
            let on: bool = unsafe { msg_send![&*req, requiresOnDeviceRecognition] };
            assert!(on);
            if let Some(r) = pick("en").and_then(|x| x.recognizer) { use_own_queue(&r); }
            // Reading the permission state asks nothing.
            assert!(["granted", "denied", "restricted", "ask"].contains(&mic_status()));
            assert!(["granted", "denied", "restricted", "ask"].contains(&speech_status()));
            assert!(!preferred_languages().is_empty());
        });
    }

    #[test]
    fn a_recording_is_read_in_pieces_cut_where_it_is_quiet() {
        let dir = std::env::temp_dir().join(format!("thoughts-voice-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("call.wav");
        wav(&path, 3);
        autoreleasepool(|_| {
            let Ok((file, fmt, rate, frames)) = open_audio(&path) else { panic!("unreadable") };
            assert_eq!((rate, frames), (16000.0, 5 * 16000));
            // The quiet stretch is seconds 1 to 4.
            let cut = quiet_point(&file, &fmt, rate, frames).unwrap();
            assert!((16000..4 * 16000).contains(&cut), "{cut}");
            let buf = pcm_buffer(&fmt, 8000).ok().unwrap();
            assert_eq!(read_frames(&file, &buf, 0, 8000).ok(), Some(8000));
            assert!(level_of(&buf) > 0.5, "a tone is loud");
            assert_eq!(read_frames(&file, &buf, 2 * 16000, 8000).ok(), Some(8000));
            assert_eq!(level_of(&buf), 0.0, "silence is silent");
            // Past the end: nothing more (macOS answers with nothing or with an error).
            assert!(matches!(read_frames(&file, &buf, frames, 8000), Ok(0) | Err(_)));
        });
        assert!(open_audio(&dir.join("none.m4a")).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
        assert_eq!(db_level(0.0), 1.0);
        assert_eq!(db_level(-80.0), 0.0);
    }
}
