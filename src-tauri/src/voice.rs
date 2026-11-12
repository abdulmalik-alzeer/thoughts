// Voice in (FEAT-55): dictation into the message box and the note editor, and meeting recordings
// turned into text, all on this computer.
//
// Only macOS has a speech recognizer the app can require to stay on the computer: the Speech
// framework with `requiresOnDeviceRecognition`. It is always set; a language whose recognizer cannot
// run on this Mac is reported as such and is never sent to Apple instead (`voice_mac.rs`). On
// Windows and Linux there is no such recognizer the app can use, so the status says
// `supported: false, reason: "platform"` and the window shows no voice buttons there.
//
// The window calls the commands below. Progress comes back as `voice` events:
//   {type:"level", level}                 while dictating (0..1, ten times a second)
//   {type:"partial", text}                as words are recognised while dictating
//   {type:"dictation-ended", text, error} dictation stopped by itself (the microphone went away…)
//   {type:"rec", recording}               four times a second while recording (`recording: null` once over)
//   {type:"rec-ended", reason}            a recording stopped by itself: "limit" (four hours) or "interrupted"
//   {type:"progress", path, done, total}  while transcribing (seconds of the recording)
//   {type:"transcribed", path}            the transcript is ready: `voice_result`, kept until `voice_result_clear`
//   {type:"transcribe-failed", path, error} / {type:"transcribe-cancelled", path}
// Errors are `VOICE::<code>` or `VOICE::<code>::<detail>`.

use serde_json::{json, Value};
use std::sync::OnceLock;
use tauri::Emitter;

#[cfg(target_os = "macos")]
#[path = "voice_mac.rs"]
mod imp;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Send a `voice` event to the window.
pub(crate) fn emit(v: Value) {
    if let Some(app) = APP.get() {
        let _ = app.emit("voice", v);
    }
}

/// `VOICE::<code>` / `VOICE::<code>::<detail>`: what the window shows the user is chosen by `code`.
pub(crate) fn fail(code: &str, detail: Option<&str>) -> String {
    match detail.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => format!("VOICE::{code}::{}", d.chars().take(400).collect::<String>()),
        None => format!("VOICE::{code}"),
    }
}

/// Lets the module know the app (to send events) and, when the app quits, finishes a running
/// recording properly so the file is complete, and stops dictation and transcription.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("voice")
        .setup(|app, _| {
            let _ = APP.set(app.clone());
            Ok(())
        })
        .on_event(|_, ev| {
            if let tauri::RunEvent::Exit = ev {
                imp::shutdown();
            }
        })
        .build()
}

/// `{supported, reason?, platform, mic, speech, langs: {en|ar: {locale, onDevice, available}},
/// dictating, recording, transcribing, result}`. `mic`/`speech`: granted | denied | restricted | ask.
#[tauri::command]
pub fn voice_status() -> Value {
    let mut v = imp::status();
    v["platform"] = json!(std::env::consts::OS);
    v
}

/// Ask the system for speech recognition and (with `mic`, the default) microphone access where it
/// has not been asked yet. Answers with the status afterwards.
#[tauri::command]
pub async fn voice_permit(mic: Option<bool>) -> Value {
    let mic = mic.unwrap_or(true);
    let mut v = tauri::async_runtime::spawn_blocking(move || imp::permit(mic)).await.unwrap_or_else(|_| imp::status());
    v["platform"] = json!(std::env::consts::OS);
    v
}

/// Start listening in `lang` (`en` | `ar`): `{locale}`.
#[tauri::command]
pub fn voice_dictate_start(lang: String) -> Result<Value, String> {
    imp::dictate_start(&lang)
}

/// Stop listening and answer with what was said.
#[tauri::command]
pub async fn voice_dictate_stop() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(imp::dictate_stop).await.map_err(|e| fail("failed", Some(&e.to_string())))?
}

#[tauri::command]
pub fn voice_dictate_cancel() {
    imp::dictate_cancel()
}

/// Start recording a meeting of the item at `item` (`<area>/<item>`) into its meetings folder:
/// `{path, item, title, lang, date, seconds, paused}`.
#[tauri::command]
pub fn voice_record_start(item: String, title: String, lang: String) -> Result<Value, String> {
    imp::record_start(&item, &title, &lang)
}

#[tauri::command]
pub fn voice_record_pause(paused: bool) -> Result<Value, String> {
    imp::record_pause(paused)
}

/// Finish the recording; the file is complete. Same shape as `voice_record_start`, plus `ended`
/// (why it had already stopped by itself, if it had).
#[tauri::command]
pub fn voice_record_stop() -> Result<Value, String> {
    imp::record_stop()
}

/// Stop the recording and delete its file.
#[tauri::command]
pub fn voice_record_discard() -> Result<(), String> {
    imp::record_discard()
}

/// Start turning the recording at `path` (library-relative) into text, in `lang`. `meta` is kept
/// with the job and handed back with the result.
#[tauri::command]
pub fn voice_transcribe(path: String, lang: String, meta: Option<Value>) -> Result<Value, String> {
    imp::transcribe(&path, &lang, meta.unwrap_or(Value::Null))
}

#[tauri::command]
pub fn voice_transcribe_cancel() {
    imp::transcribe_cancel()
}

/// The finished transcript waiting to be filed, or null: `{path, meta, paragraphs: [{at, text}],
/// seconds, complete}`.
#[tauri::command]
pub fn voice_result() -> Value {
    imp::result()
}

/// The transcript of `path` was filed (or given up): forget it.
#[tauri::command]
pub fn voice_result_clear(path: String) {
    imp::result_clear(&path)
}

/// Windows and Linux: no recognizer the app can keep on the computer (see the top of this file).
#[cfg(not(target_os = "macos"))]
mod imp {
    use super::fail;
    use serde_json::{json, Value};

    pub fn status() -> Value { json!({"supported": false, "reason": "platform"}) }
    pub fn permit(_mic: bool) -> Value { status() }
    pub fn dictate_start(_lang: &str) -> Result<Value, String> { Err(fail("unsupported", None)) }
    pub fn dictate_stop() -> Result<String, String> { Err(fail("unsupported", None)) }
    pub fn dictate_cancel() {}
    pub fn record_start(_item: &str, _title: &str, _lang: &str) -> Result<Value, String> { Err(fail("unsupported", None)) }
    pub fn record_pause(_paused: bool) -> Result<Value, String> { Err(fail("unsupported", None)) }
    pub fn record_stop() -> Result<Value, String> { Err(fail("unsupported", None)) }
    pub fn record_discard() -> Result<(), String> { Err(fail("unsupported", None)) }
    pub fn transcribe(_path: &str, _lang: &str, _meta: Value) -> Result<Value, String> { Err(fail("unsupported", None)) }
    pub fn transcribe_cancel() {}
    pub fn result() -> Value { Value::Null }
    pub fn result_clear(_path: &str) {}
    pub fn shutdown() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_carry_a_code_the_window_can_word() {
        assert_eq!(fail("busy", None), "VOICE::busy");
        assert_eq!(fail("failed", Some("  ")), "VOICE::failed");
        assert_eq!(fail("not-on-device", Some("ar-SA")), "VOICE::not-on-device::ar-SA");
        assert_eq!(fail("failed", Some(&"x".repeat(1000))).len(), "VOICE::failed::".len() + 400);
    }
}
