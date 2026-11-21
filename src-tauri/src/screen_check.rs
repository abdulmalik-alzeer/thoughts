//! Looking at the screens of a project MADO built (docs/mado-spec.md, round 0.4.0, "Screen check").
//!
//! After a build the daemon starts the project on this computer (`http://localhost:<port>` or
//! `http://127.0.0.1:<port>`) and asks the window to look at each screen the way a browser would:
//! the first real build shipped screens that were broken without any JavaScript error (an edit form
//! that opened empty, a select with no options, the word "null" on a page). The window calls
//! `mado_screen_check`; this module opens every address in a window of its own and hands back what
//! the screen showed. Everything that comes back is untrusted text.
//!
//! The page is the user's own freshly built project, but it is treated as hostile (the daemon may be
//! building a project downloaded from anywhere). How it is kept away from the app:
//!
//! * **No IPC.** Tauri injects its IPC script into every webview, but every call is checked in
//!   `Webview::on_message` (tauri 2.12.5, `src/webview/mod.rs`): a call from an origin that is not the
//!   app's own (`is_local_url`: `tauri://localhost`, the dev server, a protocol the app registered) is
//!   refused unless a capability with a matching `remote` section *and* a matching window label grants
//!   it. No capability in `capabilities/` has a `remote` section, and none names a label starting with
//!   [`PREFIX`]; `no_capability_reaches_a_screen_window` checks both against the real files, and the
//!   check refuses the app's own dev server and the daemon's port up front, so the page is never "local".
//!   Events reach only webviews that listened (`plugin:event|listen`, refused the same way), and channel
//!   data is kept per webview label.
//! * **Not the app's origin.** The window may only load the project's one origin ([`may_load`], checked
//!   for every navigation of every frame by `on_navigation`; the app's own `nav-guard` plugin yields
//!   for these windows only because this check runs first and is stricter). Requests to the app's own
//!   `tauri://` protocol, which Tauri registers in every webview, are answered with an empty 403 with
//!   no CORS header (`on_web_resource_request`), so the page cannot read the app's files.
//! * **Never shown, never kept.** The window is built hidden, unfocusable, out of the taskbar, with
//!   devtools off and a private (incognito) data store, so nothing it stores outlives it. The page has no
//!   call that could show it (window commands are IPC, refused above). It is destroyed after its screen
//!   whatever happens ([`Hidden`] destroys it on drop), after at most [`PER_SCREEN`] plus a short grace,
//!   and the run stops when the app window closes. One check at a time.
//! * **Nothing it asks for.** New windows (`window.open`) are refused, downloads are refused, every
//!   permission (camera, microphone, location, notifications…) is denied natively, and a small script in
//!   every frame stubs `alert`/`confirm`/`prompt`/`print` and the file pickers (a hidden window never gets
//!   the user's click a picker needs; the stubs are a second lock).
//!
//! How the data gets out, with no channel into the app: an initialisation script (main frame only)
//! notes errors and failed requests from the start, waits until the page has loaded and no request has
//! started for [`QUIET_MS`] (at most [`SETTLE_CAP_MS`]; the shell asks it every [`POLL`], because WebKit
//! slows a hidden page's own timers to once a second after about a second, measured on macOS 26), reads
//! the screen, and then *navigates* to a
//! reserved address on the project's own origin, `REPORT_PATH<nonce>?d=<json>`. That navigation never
//! happens: `on_navigation` sees it first, takes the query, and cancels it (no request is ever sent).
//! The navigation handler exists on all three engines (WKWebView `decidePolicyForNavigationAction`,
//! WebView2 `NavigationStarting`, WebKitGTK `decide-policy`), and a navigation is something any page can
//! already do, so the page gains nothing it did not have. The shell only reads a string: it is size-limited
//! ([`MOST_REPORT`]), parsed as JSON, every piece cut to its limit and cleaned of control and direction
//! characters ([`clean_line`], [`clean_text`]), and never interpreted. A page can of course forge its own
//! report; that is no worse than a page showing whatever it likes.
//!
//! What a hostile page can still do: everything a web page in a browser on this computer can. It has the
//! network (it can fetch any address, including other local servers; the daemon's API refuses it without
//! the token, which the page cannot read), it can use CPU and memory for the seconds it lives, and it can
//! read Tauri's IPC script, including the invoke key, which only guards against content without the
//! injected script and is not what refuses its calls (the origin and label checks above are).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::webview::{NewWindowResponse, PermissionResponse};
use tauri::{AppHandle, Url, WebviewUrl, WebviewWindowBuilder};

/// Labels of the hidden windows: a namespace of their own that no capability names.
pub(crate) const PREFIX: &str = "screencheck-";
/// Screens looked at in one check; more are answered "not checked".
const MOST_SCREENS: usize = 40;
/// Screens that may be asked for at all (the rest of the request is refused).
const MOST_ASKED: usize = 200;
/// The longest a screen may take, from opening the window to its report.
const PER_SCREEN: Duration = Duration::from_secs(10);
/// After `PER_SCREEN` the shell asks the page for what it has, and waits this much more.
const GRACE: Duration = Duration::from_millis(1500);
/// The whole check, so the answer reaches the daemon within its 120 s wait.
const BUDGET: Duration = Duration::from_secs(100);
/// A screen has settled when no request has started for this long after it loaded…
const QUIET_MS: u64 = 700;
/// …or this long after its document started, whichever comes first.
const SETTLE_CAP_MS: u64 = 8000;
const MOST_TEXT: usize = 4000;
const MOST_FIELDS: usize = 60;
const MOST_ERRORS: usize = 20;
const MOST_ERROR: usize = 300;
const MOST_LABEL: usize = 120;
const MOST_VALUE: usize = 300;
const MOST_ID: usize = 100;
const MOST_PATH: usize = 512;
/// The longest report address read (the page cuts its own report well below this).
const MOST_REPORT: usize = 512 * 1024;
/// The reserved path the report is sent to; it never reaches the project's server.
const REPORT_PATH: &str = "/__thoughts_screen_check__/";

static BUSY: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
static NEXT: AtomicU32 = AtomicU32::new(0);

/// One screen to look at, as the daemon gave it.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Screen {
    pub id: String,
    pub path: String,
}

/// What a screen showed (`ScreenResult` in ui/src/lib/mado/types.ts).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct ScreenResult {
    pub id: String,
    pub path: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub errors: Vec<String>,
    pub text: String,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Field {
    pub label: String,
    pub value: String,
}

/// The one origin a check may load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Site {
    host: &'static str,
    port: u16,
}

impl Site {
    fn origin(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Is `label` one of the hidden screen windows?
pub(crate) fn is_label(label: &str) -> bool {
    label.starts_with(PREFIX)
}

/// The app window is closing (or was replaced): stop a check that is running.
pub(crate) fn main_window_event(ev: &tauri::WindowEvent) {
    if matches!(ev, tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed) && BUSY.load(Ordering::SeqCst) {
        STOP.store(true, Ordering::SeqCst);
    }
}

/// `url` as a check may use it: exactly `http://localhost:<port>` or `http://127.0.0.1:<port>`
/// (a trailing `/` allowed), nothing else, and none of the `refused` ports (the app's own).
pub(crate) fn site(raw: &str, refused: &[u16]) -> Result<Site, &'static str> {
    if raw.len() > 64 || raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("screen-check-bad-url");
    }
    let rest = raw.strip_prefix("http://").ok_or("screen-check-bad-url")?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let (host, port) = rest.split_once(':').ok_or("screen-check-bad-url")?;
    let host = match host {
        "localhost" => "localhost",
        "127.0.0.1" => "127.0.0.1",
        _ => return Err("screen-check-bad-url"),
    };
    if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) || (port.len() > 1 && port.starts_with('0')) {
        return Err("screen-check-bad-url");
    }
    let port: u16 = port.parse().map_err(|_| "screen-check-bad-url")?;
    if port == 0 || refused.contains(&port) {
        return Err("screen-check-bad-url");
    }
    // The same answer from a real URL parser, so the two never disagree.
    let parsed = Url::parse(raw).map_err(|_| "screen-check-bad-url")?;
    if parsed.scheme() != "http" || parsed.host_str() != Some(host) || parsed.port() != Some(port) || !parsed.username().is_empty() || parsed.password().is_some() || parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("screen-check-bad-url");
    }
    Ok(Site { host, port })
}

/// A plain path on the project's server: starts with one `/`, printable ASCII without spaces or
/// backslashes, no `.`/`..` segments, short. A query and a `#/…` (hash router) are allowed.
pub(crate) fn plain_path(p: &str) -> bool {
    if p.is_empty() || p.len() > MOST_PATH || !p.starts_with('/') || p.starts_with("//") {
        return false;
    }
    if !p.bytes().all(|b| (0x21..0x7f).contains(&b) && b != b'\\') {
        return false;
    }
    let path_part = p.split(['?', '#']).next().unwrap_or("");
    if path_part.split('/').any(|seg| seg == "." || seg == ".." || seg.eq_ignore_ascii_case("%2e") || seg.eq_ignore_ascii_case("%2e%2e") || seg.eq_ignore_ascii_case(".%2e") || seg.eq_ignore_ascii_case("%2e.")) {
        return false;
    }
    !path_part.starts_with(REPORT_PATH.trim_end_matches('/'))
}

/// The full address of `path` on `site`, checked once more after parsing.
pub(crate) fn address(site: &Site, path: &str) -> Option<Url> {
    if !plain_path(path) {
        return None;
    }
    let url = Url::parse(&format!("{}{}", site.origin(), path)).ok()?;
    may_load(&url, site).then_some(url)
}

/// May the hidden window (any of its frames) load `url`? Only the one origin, plus the empty
/// documents and same-origin blobs pages make for themselves. Never `data:`, another host or port,
/// another scheme, or an address with credentials.
pub(crate) fn may_load(url: &Url, site: &Site) -> bool {
    match url.scheme() {
        "about" => matches!(url.as_str(), "about:blank" | "about:srcdoc"),
        "blob" => Url::parse(url.path()).is_ok_and(|inner| inner.scheme() == "http" && same_server(&inner, site)),
        "http" => same_server(url, site),
        _ => false,
    }
}

fn same_server(url: &Url, site: &Site) -> bool {
    url.host_str() == Some(site.host) && url.port_or_known_default() == Some(site.port) && url.username().is_empty() && url.password().is_none()
}

/// Is this navigation the page's report? (Then it is never loaded, whatever it carries.)
fn is_report(url: &Url, site: &Site) -> bool {
    url.scheme() == "http" && same_server(url, site) && url.path().starts_with(REPORT_PATH)
}

/// The report of the screen whose nonce is `nonce`, if `url` is one and is not too long.
pub(crate) fn report_of(url: &Url, site: &Site, nonce: &str) -> Option<String> {
    if !is_report(url, site) || url.path() != format!("{REPORT_PATH}{nonce}") || url.as_str().len() > MOST_REPORT {
        return None;
    }
    url.query_pairs().find(|(k, _)| k == "d").map(|(_, v)| v.into_owned())
}

/// What a page reported, already cleaned and cut to the limits.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Page {
    pub status: Option<u16>,
    pub errors: Vec<String>,
    pub text: String,
    pub fields: Vec<Field>,
    /// Where the page ended up (path, query and hash), as it said.
    pub at: Option<String>,
}

/// Read a report. `None` when it is not a JSON object.
pub(crate) fn parse_report(raw: &str) -> Option<Page> {
    if raw.len() > MOST_REPORT {
        return None;
    }
    let v: Value = serde_json::from_str(raw).ok()?;
    let o = v.as_object()?;
    let s = |key: &str| o.get(key).and_then(Value::as_str);
    let status = o.get("status").and_then(Value::as_u64).filter(|n| (100..=599).contains(n)).map(|n| n as u16);
    let errors = o
        .get("errors")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter(|e| !app_noise(e)).map(|e| clean_line(e, MOST_ERROR)).filter(|e| !e.is_empty()).take(MOST_ERRORS).collect())
        .unwrap_or_default();
    let fields = o
        .get("fields")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_object)
                .take(MOST_FIELDS)
                .map(|f| Field {
                    label: or(clean_line(f.get("label").and_then(Value::as_str).unwrap_or(""), MOST_LABEL), "(no label)"),
                    value: or(clean_line(f.get("value").and_then(Value::as_str).unwrap_or(""), MOST_VALUE), "(empty)"),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Page {
        status,
        errors,
        text: clean_text(s("text").unwrap_or(""), MOST_TEXT),
        fields,
        at: s("at").map(|a| clean_line(a, MOST_PATH)).filter(|a| !a.is_empty()),
    })
}

/// The refusal of the app's own notification plugin, whose start-up script Tauri runs in every webview
/// (it asks `plugin:notification|is_permission_granted`, which a screen window may not): not the page's.
fn app_noise(e: &str) -> bool {
    e.starts_with("Unhandled promise rejection: ") && e.contains("is_permission_granted not allowed")
}

fn or(s: String, empty: &str) -> String {
    if s.is_empty() { empty.to_string() } else { s }
}

/// Characters that change how text around them is shown (direction overrides, invisible marks).
fn invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// One line of untrusted text: no control or direction characters, whitespace collapsed, at most
/// `max` characters.
pub(crate) fn clean_line(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut n = 0;
    let mut space = false;
    for c in s.chars() {
        if invisible(c) || (c.is_control() && !c.is_whitespace()) {
            continue;
        }
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            if n + 1 >= max {
                break;
            }
            out.push(' ');
            n += 1;
            space = false;
        }
        if n >= max {
            break;
        }
        out.push(c);
        n += 1;
    }
    out
}

/// What a screen shows: lines kept, each cleaned like [`clean_line`], blank runs folded into one
/// blank line, at most `max` characters in all.
pub(crate) fn clean_text(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in s.split('\n') {
        let line = clean_line(line, max);
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        let sep = if out.is_empty() { "" } else if blank { "\n\n" } else { "\n" };
        blank = false;
        let room = max.saturating_sub(out.chars().count());
        if room <= sep.len() {
            break;
        }
        out.push_str(sep);
        out.extend(line.chars().take(room - sep.len()));
    }
    out
}

/// How looking at one screen ended.
#[derive(Debug, PartialEq)]
pub(crate) enum Look {
    Report(String),
    TimedOut,
    Failed(String),
    Stopped,
}

fn not_checked(screen: &Screen, why: &str) -> ScreenResult {
    ScreenResult { id: clean_line(&screen.id, MOST_ID), path: clean_line(&screen.path, MOST_PATH), ok: false, status: None, errors: vec![why.to_string()], text: String::new(), fields: vec![] }
}

/// The path part of an address, without a trailing `/`, to tell whether a page went elsewhere.
fn path_only(p: &str) -> &str {
    let p = p.split(['?', '#']).next().unwrap_or("");
    let p = p.trim_end_matches('/');
    if p.is_empty() { "/" } else { p }
}

/// The answer for one screen.
pub(crate) fn result_of(screen: &Screen, look: Look) -> ScreenResult {
    let mut r = not_checked(screen, "");
    r.errors.clear();
    match look {
        Look::Report(raw) => match parse_report(&raw) {
            Some(page) => {
                r.errors = page.errors;
                if let Some(at) = &page.at {
                    if path_only(at) != path_only(&screen.path) && r.errors.len() < MOST_ERRORS {
                        r.errors.push(clean_line(&format!("The screen went to another address: {at}"), MOST_ERROR));
                    }
                }
                r.status = page.status;
                r.ok = r.errors.is_empty() && page.status.is_none_or(|s| (200..300).contains(&s));
                r.text = page.text;
                r.fields = page.fields;
            }
            None => r.errors.push("The screen's report could not be read.".into()),
        },
        Look::TimedOut => r.errors.push(format!("The screen did not finish within {} seconds: it did not load, or kept itself busy.", PER_SCREEN.as_secs())),
        Look::Failed(why) => r.errors.push(clean_line(&format!("The screen could not be opened: {why}"), MOST_ERROR)),
        Look::Stopped => r.errors.push("Not checked: the app window was closed.".into()),
    }
    r
}

/// A nonce for one screen, so reports of an earlier screen are never taken for this one. It is not a
/// secret (the page can read it); it only tells screens apart.
fn nonce() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(NEXT.load(Ordering::SeqCst));
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    format!("{:016x}", h.finish())
}

/// The script, in every frame, that takes away what a hidden page could ask the person for.
const GUARD_SCRIPT: &str = r#"(function(){try{
var w=window,nop=function(){};
w.alert=nop;w.print=nop;w.confirm=function(){return false};w.prompt=function(){return null};
var no=function(){return Promise.reject(new DOMException('Not allowed here','NotAllowedError'))};
['showOpenFilePicker','showSaveFilePicker','showDirectoryPicker'].forEach(function(k){if(w[k])w[k]=no});
if(w.HTMLInputElement){var P=HTMLInputElement.prototype,click=P.click;
if(P.showPicker)P.showPicker=nop;
P.click=function(){if(String(this.type).toLowerCase()==='file')return;return click.apply(this,arguments)};}
}catch(e){}})();"#;

/// The script, in the main frame, that watches for errors and reports the screen once it settled.
fn report_script(site: &Site, nonce: &str) -> String {
    let report = serde_json::to_string(&format!("{}{REPORT_PATH}{nonce}", site.origin())).unwrap_or_else(|_| "\"\"".into());
    COLLECT_SCRIPT
        .replace("__REPORT__", &report)
        .replace("__QUIET__", &QUIET_MS.to_string())
        .replace("__CAP__", &SETTLE_CAP_MS.to_string())
        .replace("__TEXT__", &MOST_TEXT.to_string())
        .replace("__FIELDS__", &MOST_FIELDS.to_string())
        .replace("__ERRORS__", &MOST_ERRORS.to_string())
        .replace("__ERROR__", &MOST_ERROR.to_string())
        .replace("__LABEL__", &MOST_LABEL.to_string())
        .replace("__VALUE__", &MOST_VALUE.to_string())
}

const COLLECT_SCRIPT: &str = r#"(function(){'use strict';
if(window.top!==window||window.__thoughtsScreenCheck)return;
var REPORT=__REPORT__,QUIET=__QUIET__,CAP=__CAP__,st=window.setTimeout.bind(window),now=Date.now;
var started=now(),last=started,pending=0,sent=false,errors=[];
function clip(s,n){s=s==null?'':String(s);return s.length>n?s.slice(0,n):s}
function show(v){try{if(v instanceof Error)return v.name+': '+v.message;if(typeof v==='string')return v;var j=JSON.stringify(v);return j===undefined?String(v):j}catch(e){try{return String(v)}catch(e2){return '?'}}}
function note(s){try{if(errors.length<__ERRORS__)errors.push(clip(s,__ERROR__))}catch(e){}}
window.addEventListener('error',function(e){var t=e.target;
if(t&&t!==window&&t.tagName)note('Could not load '+String(t.tagName).toLowerCase()+': '+(t.src||t.href||''));
else note('Uncaught error: '+(e.message||show(e.error))+(e.filename?' ('+e.filename+':'+e.lineno+')':''))},true);
window.addEventListener('unhandledrejection',function(e){note('Unhandled promise rejection: '+show(e.reason))});
window.addEventListener('load',function(){last=Math.max(last,now())});
try{var ce=console.error;console.error=function(){var a=[];for(var i=0;i<arguments.length;i++)a.push(show(arguments[i]));note('console.error: '+a.join(' '));return ce.apply(this,arguments)}}catch(e){}
function began(){pending++;last=now()}
function ended(){pending=Math.max(0,pending-1)}
var of=window.fetch;
if(of)window.fetch=function(input,init){var m='GET',u='';
try{m=String((init&&init.method)||(input&&input.method)||'GET').toUpperCase();u=typeof input==='string'?input:((input&&input.url)||String(input))}catch(e){}
began();
return of.apply(this,arguments).then(function(r){ended();if(!r.ok)note('Request failed: '+m+' '+u+' answered '+r.status);return r},
function(err){ended();note('Request failed: '+m+' '+u+' ('+show(err)+')');throw err})};
var X=window.XMLHttpRequest;
if(X){var xo=X.prototype.open,xs=X.prototype.send;
X.prototype.open=function(m,u){try{this.__tsc=[String(m).toUpperCase(),String(u)]}catch(e){}return xo.apply(this,arguments)};
X.prototype.send=function(){var x=this,w=x.__tsc||['GET',''];began();
x.addEventListener('loadend',function(){ended();if(x.status<200||x.status>=300)note('Request failed: '+w[0]+' '+w[1]+(x.status?' answered '+x.status:' (no answer)'))});
return xs.apply(this,arguments)}}
function lastStart(){var t=last;try{var r=performance.getEntriesByType('resource');if(r.length){var s=performance.timeOrigin+r[r.length-1].startTime;if(s>t)t=s}}catch(e){}return t}
function txt(el){return el?String(el.innerText||el.textContent||''):''}
function labelOf(el){var l='';
try{if(el.labels&&el.labels.length)l=txt(el.labels[0])}catch(e){}
if(!l.trim()){var by=el.getAttribute('aria-labelledby');if(by)l=by.split(/\s+/).map(function(id){return txt(document.getElementById(id))}).join(' ')}
if(!l.trim())l=el.getAttribute('aria-label')||el.getAttribute('placeholder')||el.getAttribute('name')||el.id||el.getAttribute('title')||'';
return clip(String(l).replace(/\s+/g,' ').trim(),__LABEL__)}
function valueOf(el){var tag=String(el.tagName).toLowerCase(),type=String(el.getAttribute('type')||'').toLowerCase();
if(tag==='select'){var n=el.options.length;if(!n)return '(no options)';var c=[];
for(var i=0;i<n;i++)if(el.options[i].selected)c.push(txt(el.options[i]).trim()||el.options[i].value);
return clip((c.length?c.join(', '):'(nothing chosen)')+' ('+n+(n===1?' option)':' options)'),__VALUE__)}
if(type==='checkbox'||type==='radio')return el.checked?'checked':'not checked';
if(type==='password')return el.value?'(filled in)':'(empty)';
if(type==='file')return el.files&&el.files.length?'(file chosen)':'(no file)';
var v=el.value;return v===''||v==null?'(empty)':clip(v,__VALUE__)}
function collect(){var fields=[],list=document.querySelectorAll('input,select,textarea');
for(var i=0;i<list.length&&fields.length<__FIELDS__;i++){var el=list[i];
if(String(el.tagName).toLowerCase()==='input'&&String(el.getAttribute('type')||'').toLowerCase()==='hidden')continue;
fields.push({label:labelOf(el),value:valueOf(el)})}
var status;try{var nv=performance.getEntriesByType('navigation')[0];if(nv&&nv.responseStatus)status=nv.responseStatus}catch(e){}
return {text:clip(txt(document.body),__TEXT__),fields:fields,errors:errors.slice(0,__ERRORS__),status:status,at:clip(location.pathname+location.search+location.hash,512)}}
function report(){var d;
try{d=JSON.stringify(collect())}catch(e){d=JSON.stringify({text:'',fields:[],errors:['The screen could not be read: '+show(e)]})}
sent=true;location.replace(REPORT+'?d='+encodeURIComponent(d))}
function settled(){var t=now();return t-started>=CAP||(document.readyState==='complete'&&pending===0&&t-lastStart()>=QUIET)}
function poll(){if(!sent&&settled()){try{report()}catch(e){}}}
function tick(){poll();if(!sent)st(tick,100)}
window.__thoughtsScreenCheck={poll:poll,now:function(){try{report()}catch(e){}}};
st(tick,100);
})();"#;

/// Asks the page whether it has settled. The shell asks every [`POLL`]: WebKit slows the timers of a
/// hidden page to about once a second after its first second, so the page's own timer is only a
/// fallback, and a page that polls its server would otherwise look quiet between two slowed timers.
const POLL_SCRIPT: &str = "try{window.__thoughtsScreenCheck&&window.__thoughtsScreenCheck.poll()}catch(e){}";
/// How often the shell asks.
const POLL: Duration = Duration::from_millis(150);
/// Asks a page that has not reported yet for what it has now.
const FORCE_SCRIPT: &str = "try{window.__thoughtsScreenCheck&&window.__thoughtsScreenCheck.now()}catch(e){}";

/// A hidden window that is destroyed when this goes, whatever happened.
struct Hidden(tauri::WebviewWindow);

impl Drop for Hidden {
    fn drop(&mut self) {
        let _ = self.0.destroy();
    }
}

/// The empty, unreadable answer to a request for the app's own files from a screen window.
fn refused_response() -> tauri::http::Response<Cow<'static, [u8]>> {
    let mut r = tauri::http::Response::new(Cow::Borrowed(&[][..]));
    *r.status_mut() = tauri::http::StatusCode::FORBIDDEN;
    r
}

/// Open `addr` in a hidden window of its own and wait for its report.
fn look(app: &AppHandle, site: &Site, addr: Url) -> Look {
    let label = format!("{PREFIX}{}", NEXT.fetch_add(1, Ordering::SeqCst));
    let nonce = nonce();
    let (tx, rx) = mpsc::sync_channel::<String>(1);
    let nav_site = site.clone();
    let nav_nonce = nonce.clone();
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(addr))
        .title("")
        .visible(false)
        .focused(false)
        .focusable(false)
        .skip_taskbar(true)
        .decorations(false)
        .inner_size(1280.0, 800.0)
        .incognito(true)
        .devtools(false)
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
        .initialization_script_for_all_frames(GUARD_SCRIPT)
        .initialization_script(report_script(site, &nonce))
        .on_navigation(move |u| {
            if is_report(u, &nav_site) {
                if let Some(d) = report_of(u, &nav_site, &nav_nonce) {
                    let _ = tx.try_send(d);
                }
                return false;
            }
            may_load(u, &nav_site)
        })
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .on_permission_request(|_, _| PermissionResponse::Deny)
        .on_web_resource_request(|_, response| *response = refused_response())
        .build();
    let win = match built {
        Ok(w) => Hidden(w),
        Err(e) => return Look::Failed(e.to_string()),
    };
    let mut deadline = Instant::now() + PER_SCREEN;
    let mut forced = false;
    loop {
        match rx.recv_timeout(POLL) {
            Ok(d) => return Look::Report(d),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Look::Failed("the window went away".into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if STOP.load(Ordering::SeqCst) {
            return Look::Stopped;
        }
        if !forced {
            let _ = win.0.eval(POLL_SCRIPT);
        }
        if Instant::now() >= deadline {
            if forced {
                return Look::TimedOut;
            }
            forced = true;
            let _ = win.0.eval(FORCE_SCRIPT);
            deadline = Instant::now() + GRACE;
        }
    }
}

/// Look at every screen in turn (the first `MOST_SCREENS`, within `BUDGET`).
fn run(app: &AppHandle, site: &Site, screens: &[Screen]) -> Vec<ScreenResult> {
    let started = Instant::now();
    screens
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if i >= MOST_SCREENS {
                return not_checked(s, "Not checked: only the first 40 screens are looked at.");
            }
            if STOP.load(Ordering::SeqCst) {
                return result_of(s, Look::Stopped);
            }
            if started.elapsed() + PER_SCREEN + GRACE > BUDGET {
                return not_checked(s, "Not checked: the screen check ran out of time.");
            }
            match address(site, &s.path) {
                Some(addr) => result_of(s, look(app, site, addr)),
                None => not_checked(s, "Not checked: the path is not a plain address on the project's server."),
            }
        })
        .collect()
}

/// Ports the check never loads: the daemon's, and (in a developer build) the app's own dev server.
/// Only a developer build counts `devUrl` as the app's own origin (`get_app_url` in Tauri reads it
/// under `cfg(dev)` alone), so a release build does not refuse a project that happens to use its port.
fn own_ports(app: &AppHandle) -> Vec<u16> {
    let mut ports: Vec<u16> = thoughtsd::paths::read_daemon_info().map(|i| i.port).into_iter().collect();
    if let Some(dev) = app.config().build.dev_url.as_ref().filter(|_| tauri::is_dev()) {
        if let Some(p) = dev.port_or_known_default() {
            ports.push(p);
        }
    }
    ports
}

/// Clears `BUSY` when the check ends, also when it panics.
struct Busy;

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// The whole check, shared by the command and the probe build.
pub(crate) async fn check(app: AppHandle, url: String, screens: Vec<Screen>) -> Result<Vec<ScreenResult>, String> {
    if screens.len() > MOST_ASKED {
        return Err("screen-check-too-many".into());
    }
    let site = site(&url, &own_ports(&app)).map_err(String::from)?;
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("screen-check-busy".into());
    }
    let busy = Busy;
    STOP.store(false, Ordering::SeqCst);
    let out = tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        run(&app, &site, &screens)
    })
    .await;
    out.map_err(|e| e.to_string())
}

/// Look at the screens of the project MADO just built: `url` is its local address, each screen a
/// path on it. Only the app window may ask. Errors are codes the window translates:
/// `screen-check-bad-url`, `screen-check-busy`, `screen-check-too-many`.
#[tauri::command]
pub(crate) async fn mado_screen_check(app: AppHandle, window: tauri::Window, url: String, screens: Vec<Screen>) -> Result<Vec<ScreenResult>, String> {
    crate::main_only(&window)?;
    check(app, url, screens).await
}

/// A developer build only (`--features screen-check-probe`): runs one check at start-up from the
/// JSON file named by `THOUGHTS_SCREEN_CHECK_PROBE` (`{url, screens}`), writes the results and what
/// the hidden windows did to `THOUGHTS_SCREEN_CHECK_PROBE_OUT`, and quits. No release has it.
#[cfg(feature = "screen-check-probe")]
pub(crate) mod probe {
    use super::*;
    use tauri::Manager;

    pub(crate) fn start(app: &AppHandle) {
        let (Some(input), Some(output)) = (std::env::var_os("THOUGHTS_SCREEN_CHECK_PROBE"), std::env::var_os("THOUGHTS_SCREEN_CHECK_PROBE_OUT")) else { return };
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            let ask: Value = std::fs::read_to_string(&input).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
            let url = ask["url"].as_str().unwrap_or("").to_string();
            let screens: Vec<Screen> = serde_json::from_value(ask["screens"].clone()).unwrap_or_default();
            // Watch the hidden windows while the check runs: were any ever visible or focused?
            let watching = std::sync::Arc::new(AtomicBool::new(true));
            let seen = std::sync::Arc::new(std::sync::Mutex::new((0usize, false, false)));
            {
                let (app, watching, seen) = (app.clone(), watching.clone(), seen.clone());
                std::thread::spawn(move || {
                    while watching.load(Ordering::SeqCst) {
                        for (label, w) in app.webview_windows() {
                            if is_label(&label) {
                                let mut s = seen.lock().unwrap();
                                s.0 += 1;
                                s.1 |= w.is_visible().unwrap_or(false);
                                s.2 |= w.is_focused().unwrap_or(false);
                            }
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                });
            }
            let started = Instant::now();
            let results = tauri::async_runtime::block_on(check(app.clone(), url.clone(), screens));
            let busy_again = tauri::async_runtime::block_on(check(app.clone(), url, vec![]));
            watching.store(false, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(500));
            let left: Vec<String> = app.webview_windows().keys().filter(|l| is_label(l)).cloned().collect();
            let seen = *seen.lock().unwrap();
            let refused: Vec<Value> = ["http://example.com:80", "http://localhost", "https://localhost:5173", "http://user:pw@localhost:5173", "http://localhost:5173/x"]
                .iter()
                .map(|u| serde_json::json!({"url": u, "answer": tauri::async_runtime::block_on(check(app.clone(), u.to_string(), vec![])).err()}))
                .collect();
            let out = serde_json::json!({
                "ms": started.elapsed().as_millis() as u64,
                "results": results,
                "secondCheckAfterwards": busy_again.map(|r| r.len()),
                "refused": refused,
                "windowSamples": seen.0, "everVisible": seen.1, "everFocused": seen.2,
                "windowsLeft": left,
            });
            let _ = std::fs::write(&output, serde_json::to_string_pretty(&out).unwrap_or_default());
            app.exit(0);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: &str, path: &str) -> Screen {
        Screen { id: id.into(), path: path.into() }
    }

    #[test]
    fn only_a_local_origin_with_a_port_is_a_site() {
        assert_eq!(site("http://localhost:5173", &[]), Ok(Site { host: "localhost", port: 5173 }));
        assert_eq!(site("http://127.0.0.1:8000/", &[]), Ok(Site { host: "127.0.0.1", port: 8000 }));
        for bad in [
            "", "localhost:5173", "http://localhost", "http://localhost:", "http://localhost:0", "http://localhost:65536",
            "http://localhost:05173", "https://localhost:5173", "ws://localhost:5173", "file:///etc/passwd",
            "http://example.com:5173", "http://localhost.example.com:5173", "http://127.0.0.2:5173", "http://[::1]:5173",
            "http://0.0.0.0:5173", "http://LOCALHOST:5173", "http://user@localhost:5173", "http://user:pw@localhost:5173",
            "http://localhost:5173/app", "http://localhost:5173/?x=1", "http://localhost:5173/#x", "http://localhost:5173//",
            " http://localhost:5173", "http://localhost:5173\n", "http://localhost:5173@evil.com", "http://evil.com#@localhost:5173",
            "http://localhost:5173:80", "http://2130706433:5173", "http://127.1:5173", "javascript:alert(1)",
        ] {
            assert!(site(bad, &[]).is_err(), "{bad:?} must be refused");
        }
        // The app's own ports (its daemon, its dev server) are never a project's.
        assert!(site("http://127.0.0.1:4321", &[4321]).is_err());
        assert!(site("http://localhost:1420", &[4321, 1420]).is_err());
    }

    #[test]
    fn only_plain_paths_are_looked_at() {
        for good in ["/", "/customers", "/customers/1/edit", "/search?q=acme&page=2", "/#/customers/1", "/a-b_c.d~e/%20x"] {
            assert!(plain_path(good), "{good:?}");
        }
        for bad in [
            "", "customers", "//evil.com/x", "/a b", "/a\tb", "/a\nb", "/a\\b", "/../etc/passwd", "/a/../b", "/./a", "/a/%2e%2e/b",
            "/a/%2E/b", "/a/.%2e/b", "/ü", "/__thoughts_screen_check__/x", "/__thoughts_screen_check__",
        ] {
            assert!(!plain_path(bad), "{bad:?}");
        }
        assert!(!plain_path(&format!("/{}", "a".repeat(MOST_PATH))));
        let site = site("http://localhost:5173", &[]).unwrap();
        assert_eq!(address(&site, "/customers/1/edit").unwrap().as_str(), "http://localhost:5173/customers/1/edit");
        assert!(address(&site, "//evil.com/").is_none());
        assert!(address(&site, "@evil.com/").is_none());
    }

    #[test]
    fn a_screen_window_may_load_only_its_own_origin() {
        let site = site("http://localhost:5173", &[]).unwrap();
        let ok = |u: &str| may_load(&Url::parse(u).unwrap(), &site);
        assert!(ok("http://localhost:5173/"));
        assert!(ok("http://localhost:5173/customers?x=1#top"));
        assert!(ok("about:blank"));
        assert!(ok("about:srcdoc"));
        assert!(ok("blob:http://localhost:5173/6f1c-uuid"));
        for bad in [
            "http://127.0.0.1:5173/", "http://localhost:5174/", "http://localhost/", "https://localhost:5173/", "http://user:pw@localhost:5173/",
            "http://example.com/", "tauri://localhost/", "http://tauri.localhost/", "ipc://localhost/", "data:text/html,<p>x",
            "blob:http://example.com/uuid", "blob:tauri://localhost/uuid", "file:///etc/passwd", "javascript:alert(1)", "about:config",
            "thoughts://open", "ws://localhost:5173/",
        ] {
            assert!(!ok(bad), "{bad:?} must be refused");
        }
    }

    #[test]
    fn the_report_is_taken_only_from_its_own_reserved_address() {
        let site = site("http://localhost:5173", &[]).unwrap();
        let u = |s: &str| Url::parse(s).unwrap();
        let good = u("http://localhost:5173/__thoughts_screen_check__/abc?d=%7B%22text%22%3A%22hi%22%7D");
        assert!(is_report(&good, &site));
        assert_eq!(report_of(&good, &site, "abc").as_deref(), Some(r#"{"text":"hi"}"#));
        // Another screen's nonce, another origin, or no data: not this screen's report.
        assert_eq!(report_of(&good, &site, "abd"), None);
        assert!(is_report(&u("http://localhost:5173/__thoughts_screen_check__/zzz"), &site));
        assert_eq!(report_of(&u("http://localhost:5173/__thoughts_screen_check__/abc"), &site, "abc"), None);
        assert!(!is_report(&u("http://127.0.0.1:5173/__thoughts_screen_check__/abc?d=1"), &site));
        assert!(!is_report(&u("http://localhost:5173/customers"), &site));
        // Far too long: blocked like every report, but not read.
        let huge = u(&format!("http://localhost:5173/__thoughts_screen_check__/abc?d={}", "a".repeat(MOST_REPORT)));
        assert!(is_report(&huge, &site));
        assert_eq!(report_of(&huge, &site, "abc"), None);
    }

    #[test]
    fn a_report_is_cut_to_its_limits_and_cleaned() {
        let long = "x".repeat(1_000);
        let fields: Vec<Value> = (0..100).map(|i| serde_json::json!({"label": format!("Field {i}\u{202E}"), "value": long})).collect();
        let errors: Vec<Value> = (0..50).map(|i| Value::String(format!("error {i}\u{0007}\n{long}"))).collect();
        let raw = serde_json::json!({"text": format!("Hello\u{0000}\u{001B}[31m world\r\n\n\n\n  Next   line\u{200B}\n{}", long.repeat(5)), "fields": fields, "errors": errors, "status": 200, "at": "/customers"}).to_string();
        let page = parse_report(&raw).unwrap();
        assert_eq!(page.text.chars().count(), MOST_TEXT);
        assert!(page.text.starts_with("Hello[31m world\n\nNext line\nxxx"), "{:?}", &page.text[..40]);
        assert!(!page.text.chars().any(|c| c.is_control() && c != '\n'));
        assert_eq!(page.fields.len(), MOST_FIELDS);
        assert_eq!(page.fields[0].label, "Field 0");
        assert_eq!(page.fields[0].value.chars().count(), MOST_VALUE);
        assert_eq!(page.errors.len(), MOST_ERRORS);
        assert!(page.errors[0].starts_with("error 0 xxx"));
        assert_eq!(page.errors[0].chars().count(), MOST_ERROR);
        assert_eq!(page.status, Some(200));
        // Odd shapes are tolerated, never trusted.
        let odd = parse_report(r#"{"text":5,"fields":[1,{"label":7},{"label":"","value":""}],"errors":[null,"","ok"],"status":99999}"#).unwrap();
        assert_eq!(odd.text, "");
        assert_eq!(odd.fields, vec![Field { label: "(no label)".into(), value: "(empty)".into() }, Field { label: "(no label)".into(), value: "(empty)".into() }]);
        assert_eq!(odd.errors, vec!["ok".to_string()]);
        assert_eq!(odd.status, None);
        // The app's own plugin script being refused is not the page's error (debug and release wording).
        let noise = parse_report(r#"{"errors":["Unhandled promise rejection: notification.is_permission_granted not allowed on window \"screencheck-0\"","Unhandled promise rejection: Command plugin:notification|is_permission_granted not allowed by ACL","Uncaught error: is_permission_granted not allowed"]}"#).unwrap();
        assert_eq!(noise.errors, vec!["Uncaught error: is_permission_granted not allowed".to_string()]);
        assert!(parse_report("[1,2]").is_none());
        assert!(parse_report("not json").is_none());
        assert!(parse_report(&"x".repeat(MOST_REPORT + 1)).is_none());
    }

    #[test]
    fn text_keeps_its_lines_and_multibyte_characters() {
        assert_eq!(clean_text("  شاشة العملاء \n\n\nعميل   ١ ", 100), "شاشة العملاء\n\nعميل ١");
        assert_eq!(clean_text("abc\ndef", 5), "abc\nd");
        assert_eq!(clean_line("\u{2066}a\u{2069}  b\u{FEFF}", 10), "a b");
        assert_eq!(clean_line("ab cd", 3), "ab");
        assert_eq!(clean_line("   ", 3), "");
    }

    #[test]
    fn each_screen_gets_a_plain_answer() {
        let edit = s("customer-edit", "/customers/1/edit");
        let good = r#"{"text":"Edit customer","fields":[{"label":"Name","value":"Acme"}],"errors":[],"status":200,"at":"/customers/1/edit"}"#;
        let r = result_of(&edit, Look::Report(good.into()));
        assert!(r.ok, "{r:?}");
        assert_eq!(r.fields, vec![Field { label: "Name".into(), value: "Acme".into() }]);
        let failed = result_of(&edit, Look::Report(r#"{"text":"null","errors":["Request failed: GET /api/customers/1 answered 500"]}"#.into()));
        assert!(!failed.ok);
        assert_eq!(failed.text, "null");
        let moved = result_of(&edit, Look::Report(r#"{"text":"Sign in","at":"/login?next=1"}"#.into()));
        assert!(!moved.ok);
        assert_eq!(moved.errors, vec!["The screen went to another address: /login?next=1".to_string()]);
        // A trailing slash or a query is not "another address".
        assert!(result_of(&edit, Look::Report(r#"{"at":"/customers/1/edit/?tab=2"}"#.into())).ok);
        assert!(!result_of(&edit, Look::Report(r#"{"status":404,"at":"/customers/1/edit"}"#.into())).ok);
        for look in [Look::TimedOut, Look::Stopped, Look::Failed("no\u{0007} way".into()), Look::Report("garbage".into())] {
            let r = result_of(&edit, look);
            assert!(!r.ok && r.errors.len() == 1 && !r.errors[0].chars().any(char::is_control), "{r:?}");
            assert_eq!((r.id.as_str(), r.path.as_str()), ("customer-edit", "/customers/1/edit"));
        }
        // The answer has the shape of `ScreenResult` in ui/src/lib/mado/types.ts.
        let v = serde_json::to_value(&r).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["errors", "fields", "id", "ok", "path", "status", "text"]);
        assert!(serde_json::to_value(result_of(&edit, Look::TimedOut)).unwrap().get("status").is_none());
    }

    #[test]
    fn the_scripts_carry_their_limits_and_report_address() {
        let site = site("http://127.0.0.1:8000", &[]).unwrap();
        let js = report_script(&site, "n0nce");
        assert!(js.contains(r#"REPORT="http://127.0.0.1:8000/__thoughts_screen_check__/n0nce""#));
        assert!(!js.contains("__REPORT__") && !js.contains("__QUIET__") && !js.contains("__CAP__") && !js.contains("__TEXT__"));
        assert!(!js.contains("__FIELDS__") && !js.contains("__ERRORS__") && !js.contains("__ERROR__") && !js.contains("__LABEL__") && !js.contains("__VALUE__"));
        assert!(js.contains("QUIET=700,CAP=8000"));
        assert_ne!(nonce(), nonce());
        assert!(nonce().bytes().all(|b| b.is_ascii_hexdigit()));
    }

    /// Does the capability glob `pattern` (only `*` is special) match `label`?
    fn glob(pattern: &str, label: &str) -> bool {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 1 {
            return pattern == label;
        }
        let mut rest = label;
        for (i, part) in parts.iter().enumerate() {
            if i == 0 {
                let Some(r) = rest.strip_prefix(part) else { return false };
                rest = r;
            } else if i == parts.len() - 1 {
                return rest.ends_with(part);
            } else {
                let Some(at) = rest.find(part) else { return false };
                rest = &rest[at + part.len()..];
            }
        }
        true
    }

    /// The written check behind "no access to the app": Tauri refuses every IPC call from a remote
    /// origin unless a capability with a `remote` section matching it also names the window's label
    /// (tauri 2.12.5 `Webview::on_message` and `RuntimeAuthority::resolve_access`). Neither exists here.
    #[test]
    fn no_capability_reaches_a_screen_window() {
        assert!(glob("main-*", "main-2") && glob("view-*", "view-1") && !glob("main-*", "screencheck-1") && glob("*", "screencheck-1"));
        let mut files = 0;
        for entry in std::fs::read_dir("capabilities").unwrap().flatten() {
            if entry.path().extension().is_none_or(|e| e != "json") {
                continue;
            }
            files += 1;
            let cap: Value = serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap()).unwrap();
            assert!(cap.get("remote").is_none(), "{:?} grants remote content", entry.path());
            for pattern in cap["windows"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                for label in ["screencheck-0", "screencheck-17", "screencheck-"] {
                    assert!(!glob(pattern, label), "{:?}: {pattern} matches {label}", entry.path());
                }
            }
            assert!(cap.get("webviews").is_none(), "{:?} names webviews", entry.path());
        }
        assert!(files >= 3);
        // The label namespace is the screen windows' own.
        assert!(is_label("screencheck-3") && !crate::windows::is_app_label("screencheck-3") && !crate::is_main_label("screencheck-3"));
        assert_ne!(crate::capture::LABEL, "screencheck-0");
    }
}
