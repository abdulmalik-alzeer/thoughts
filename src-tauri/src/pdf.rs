//! "Save as PDF" for the app window: the page has laid the note out for print (its `#print-root`,
//! shown only on paper while `html.print-doc` is set), and the shell prints the app window's own
//! web view straight into a PDF file, page by page, without the system print dialog.
//!
//! The file goes only where the user said: the save dialog is opened here, in native code, and the
//! window never supplies a path. The path the dialog returns is still checked (`pdf_target`): an
//! absolute path to a `.pdf` file. A saved file can then be opened or shown in its folder through
//! `open_saved_pdf`, by the id `save_pdf` answered with, so the window can only ever open a PDF
//! this app has just written.
//!
//! macOS: WebKit's own print operation, with the job set to "save to file" and no panels
//! (`NSPrintOperation` with `NSPrintSaveJob`), which paginates like printing does. Windows and
//! Linux answer `{"unsupported": true}` before any dialog: the page then uses the system print
//! dialog, which offers saving as PDF itself (see the report of batch libf-pdf for what a native
//! path there would take).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// One PDF at a time: a second request while one is being written is refused.
static BUSY: AtomicBool = AtomicBool::new(false);
/// PDFs written in this run of the app, by id, newest last (only the last few are kept).
static SAVED: Mutex<Vec<(u64, PathBuf)>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
const KEEP: usize = 20;

/// The name the save dialog offers: the note's title with only characters every file system
/// takes (letters of any script, digits, spaces and a few marks), at most 100 characters, `.pdf`.
pub(crate) fn pdf_name(title: &str) -> String {
    let stem = title.strip_suffix(".pdf").or_else(|| title.strip_suffix(".PDF")).unwrap_or(title);
    let cleaned: String = stem
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' ' | '.' | '(' | ')' | ',' | '\'' | '&' | '+'))
        .collect();
    let squeezed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed: String = squeezed.trim_matches(|c: char| c == '.' || c == ' ').chars().take(100).collect();
    let trimmed = trimmed.trim_end_matches(['.', ' ']);
    format!("{}.pdf", if trimmed.is_empty() { "document" } else { trimmed })
}

/// Where the PDF is written: the path the save dialog returned, which must be absolute and name a
/// file (not a folder); it always ends in `.pdf`, whatever the user typed.
pub(crate) fn pdf_target(picked: &Path) -> Result<PathBuf, String> {
    if !picked.is_absolute() || picked.file_name().is_none() || picked.is_dir() {
        return Err("this place cannot be used for the PDF".into());
    }
    let mut p = picked.to_path_buf();
    if p.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() != Some("pdf") {
        let mut name = p.file_name().unwrap_or_default().to_os_string();
        name.push(".pdf");
        p.set_file_name(name);
    }
    Ok(p)
}

/// Whether `path` holds a PDF (what the print operation wrote).
fn is_pdf(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 5];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == b"%PDF-"
}

fn remember(path: PathBuf) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut saved = SAVED.lock().unwrap_or_else(|e| e.into_inner());
    saved.push((id, path));
    let extra = saved.len().saturating_sub(KEEP);
    saved.drain(..extra);
    id
}

fn saved_path(id: u64) -> Option<PathBuf> {
    SAVED.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(i, _)| *i == id).map(|(_, p)| p.clone())
}

struct Busy;
impl Busy {
    fn take() -> Option<Busy> {
        BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Busy)
    }
}
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

/// Save the page as it prints (the note the page laid out in `#print-root`) as a PDF.
/// `save_pdf {name}` → `null` when the user cancelled the dialog, `{"unsupported": true}` where
/// this computer has no way to write it directly (the page then prints instead), or
/// `{"id", "file"}` once written (`file` is the file's name, for the message).
#[tauri::command]
pub(crate) async fn save_pdf(app: tauri::AppHandle, window: tauri::Window, webview: tauri::Webview, name: String) -> Result<Value, String> {
    crate::main_only(&window)?;
    if !cfg!(target_os = "macos") {
        let _ = (app, webview, name);
        return Ok(json!({"unsupported": true}));
    }
    let Some(_busy) = Busy::take() else { return Err("busy".into()) };
    let file = pdf_name(&name);
    let picked = tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        app.dialog().file().set_file_name(&file).add_filter("PDF", &["pdf"]).blocking_save_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(picked) = picked.and_then(|p| p.into_path().ok()) else { return Ok(Value::Null) };
    let path = pdf_target(&picked)?;
    write_pdf(&webview, &path).await?;
    if !is_pdf(&path) {
        return Err("the PDF was not written".into());
    }
    let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(json!({"id": remember(path), "file": file}))
}

/// Open a PDF `save_pdf` wrote (in its usual app), or show it in its folder (`reveal`).
#[tauri::command]
pub(crate) async fn open_saved_pdf(app: tauri::AppHandle, window: tauri::Window, id: u64, reveal: Option<bool>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    crate::main_only(&window)?;
    let path = saved_path(id).ok_or("unknown file")?;
    if !path.is_file() {
        return Err("gone".into());
    }
    if reveal.unwrap_or(false) {
        app.opener().reveal_item_in_dir(&path).map_err(|e| e.to_string())
    } else {
        app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "macos")]
async fn write_pdf(webview: &tauri::Webview, path: &Path) -> Result<(), String> {
    use std::sync::mpsc::RecvTimeoutError;
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let target = path.to_path_buf();
    webview
        .with_webview(move |wv| mac::print_to_file(wv.inner(), wv.ns_window(), &target, tx))
        .map_err(|e| e.to_string())?;
    let answer = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(180)))
        .await
        .map_err(|e| e.to_string())?;
    match answer {
        Ok(r) => r,
        Err(RecvTimeoutError::Disconnected) => Err("the PDF was not written".into()),
        Err(RecvTimeoutError::Timeout) => Err("writing the PDF took too long".into()),
    }
}

#[cfg(not(target_os = "macos"))]
async fn write_pdf(_webview: &tauri::Webview, _path: &Path) -> Result<(), String> {
    Err("unsupported".into())
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject};
    use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
    use objc2_app_kit::{NSPrintInfo, NSPrintJobSavingURL, NSPrintOperation, NSPrintSaveJob, NSPrintingPaginationMode, NSWindow};
    use objc2_foundation::{MainThreadMarker, NSRect, NSURL};
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::path::Path;
    use std::sync::mpsc::Sender;

    type Done = Sender<Result<(), String>>;

    pub(super) struct Ivars {
        done: RefCell<Option<Done>>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and this class does not implement Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = Ivars]
        struct PrintDone;

        impl PrintDone {
            /// `-[NSPrintOperation runOperationModalForWindow:delegate:didRunSelector:contextInfo:]`'s callback.
            #[unsafe(method(printOperationDidRun:success:contextInfo:))]
            fn did_run(&self, _op: *mut AnyObject, success: Bool, _ctx: *mut c_void) {
                if let Some(tx) = self.ivars().done.borrow_mut().take() {
                    let _ = tx.send(if success.as_bool() { Ok(()) } else { Err("printing to the file failed".into()) });
                }
            }
        }
    );

    thread_local! {
        /// The delegate of the print operation running now (kept alive until it has answered).
        static RUNNING: RefCell<Option<Retained<PrintDone>>> = const { RefCell::new(None) };
    }

    /// Print the WKWebView `view` (in window `window`) into the PDF file `path`, paginated, without
    /// any panel. Runs on the main thread (inside `with_webview`); `done` is answered when finished.
    pub(super) fn print_to_file(view: *mut c_void, window: *mut c_void, path: &Path, done: Done) {
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = done.send(Err("not on the main thread".into()));
            return;
        };
        if view.is_null() || window.is_null() {
            let _ = done.send(Err("no window".into()));
            return;
        }
        let view = view as *mut AnyObject;
        // SAFETY: tauri hands us the window's live NSWindow and WKWebView, on the main thread.
        let window: &NSWindow = unsafe { &*(window as *const NSWindow) };
        let Some(url) = NSURL::from_file_path(path) else {
            let _ = done.send(Err("this place cannot be used for the PDF".into()));
            return;
        };
        let info = NSPrintInfo::sharedPrintInfo();
        // A copy: the shared settings (the user's own print choices) are left as they were.
        let info: Retained<NSPrintInfo> = unsafe { msg_send![&*info, copy] };
        unsafe {
            info.setJobDisposition(NSPrintSaveJob);
            info.dictionary().setObject_forKey(&url, objc2::runtime::ProtocolObject::from_ref(NSPrintJobSavingURL));
        }
        // The margins come from the page's own `@page` rule; the width fits the paper.
        info.setTopMargin(0.0);
        info.setBottomMargin(0.0);
        info.setLeftMargin(0.0);
        info.setRightMargin(0.0);
        info.setHorizontalPagination(NSPrintingPaginationMode::Fit);
        info.setVerticalPagination(NSPrintingPaginationMode::Automatic);
        info.setHorizontallyCentered(false);
        info.setVerticallyCentered(false);
        let op: Option<Retained<NSPrintOperation>> = unsafe { msg_send![view, printOperationWithPrintInfo: &*info] };
        let Some(op) = op else {
            let _ = done.send(Err("printing is not available".into()));
            return;
        };
        op.setShowsPrintPanel(false);
        op.setShowsProgressPanel(false);
        // WebKit lays the pages out only when the operation's view has a size (otherwise: blank pages).
        let bounds: NSRect = unsafe { msg_send![view, bounds] };
        if let Some(v) = op.view() {
            v.setFrame(bounds);
        }
        let delegate = PrintDone::alloc(mtm).set_ivars(Ivars { done: RefCell::new(Some(done)) });
        let delegate: Retained<PrintDone> = unsafe { msg_send![super(delegate), init] };
        RUNNING.with(|r| *r.borrow_mut() = Some(delegate.clone()));
        // The modal run (unlike `runOperation`) lets WebKit's web process deliver the pages; with no
        // panels nothing is shown, and the delegate is called when the file is written.
        unsafe {
            op.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
                window,
                Some(&delegate),
                Some(sel!(printOperationDidRun:success:contextInfo:)),
                std::ptr::null_mut(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_from_the_title() {
        assert_eq!(pdf_name("Weekly plan"), "Weekly plan.pdf");
        assert_eq!(pdf_name("خطة الأسبوع"), "خطة الأسبوع.pdf");
        assert_eq!(pdf_name("a/b\\c:d*e?f\"g<h>i|j"), "abcdefghij.pdf");
        assert_eq!(pdf_name("../../etc/passwd"), "etcpasswd.pdf");
        assert_eq!(pdf_name("  .hidden.  "), "hidden.pdf");
        assert_eq!(pdf_name("report.pdf"), "report.pdf");
        assert_eq!(pdf_name("line\nbreak\ttab"), "line break tab.pdf");
        assert_eq!(pdf_name("\u{202e}evil\u{200f}"), "evil.pdf");
        assert_eq!(pdf_name(""), "document.pdf");
        assert_eq!(pdf_name("///"), "document.pdf");
        assert_eq!(pdf_name(&"x".repeat(300)).chars().count(), 104);
    }

    #[test]
    fn the_target_is_an_absolute_pdf_path() {
        let dir = std::env::temp_dir();
        assert_eq!(pdf_target(&dir.join("a.pdf")).unwrap(), dir.join("a.pdf"));
        assert_eq!(pdf_target(&dir.join("a.PDF")).unwrap(), dir.join("a.PDF"));
        assert_eq!(pdf_target(&dir.join("a.html")).unwrap(), dir.join("a.html.pdf"));
        assert_eq!(pdf_target(&dir.join("notes")).unwrap(), dir.join("notes.pdf"));
        assert!(pdf_target(Path::new("relative.pdf")).is_err());
        assert!(pdf_target(Path::new("")).is_err());
        // A folder is never written over.
        assert!(pdf_target(&dir).is_err());
    }

    #[test]
    fn only_files_this_app_saved_can_be_opened() {
        let a = remember(PathBuf::from("/tmp/one.pdf"));
        assert_eq!(saved_path(a), Some(PathBuf::from("/tmp/one.pdf")));
        assert_eq!(saved_path(a + 1000), None);
        for i in 0..KEEP {
            remember(PathBuf::from(format!("/tmp/{i}.pdf")));
        }
        // Old ids are forgotten; the list stays short.
        assert_eq!(saved_path(a), None);
        assert!(SAVED.lock().unwrap().len() <= KEEP);
    }

    #[test]
    fn a_pdf_is_recognised_by_its_header() {
        let dir = std::env::temp_dir().join(format!("thoughts-pdf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.pdf");
        let bad = dir.join("bad.pdf");
        std::fs::write(&good, b"%PDF-1.3\n...").unwrap();
        std::fs::write(&bad, b"<html>").unwrap();
        assert!(is_pdf(&good));
        assert!(!is_pdf(&bad));
        assert!(!is_pdf(&dir.join("missing.pdf")));
        std::fs::remove_file(&good).unwrap();
        std::fs::remove_file(&bad).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
