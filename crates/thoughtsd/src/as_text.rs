//! A plain-text version of an office file, for the assistant to read.
//!
//! The assistant opens attached files with its Read tool, which reads text, pictures and PDFs but
//! not spreadsheets, Word documents or slide decks (zip or binary containers). `run_files` therefore
//! writes, beside the copy of each such file, what this module makes of it: every sheet as CSV,
//! a document's headings, paragraphs, lists and tables in reading order, a deck's text slide by
//! slide with its notes.
//!
//! Every file is treated as hostile. Nothing in it is run and no name in it is used as a path (names
//! inside a zip are only looked up in that zip). What is unpacked is counted while unpacking and
//! stops at a limit, whatever sizes the file declares; the number of entries, the size of the text
//! made and the time spent are limited too; a panic in a parser is caught. Whatever goes wrong, the
//! answer is an `Err` with a plain reason: the message is still sent, and the prompt says the file
//! could not be turned into text.
//!
//! Nothing is cut silently: where a limit stops the text, a line starting `[truncated:` says so.

use std::io::{Cursor, Read};
use std::time::{Duration, Instant};

/// Largest file looked at.
pub const IN_MAX: u64 = 64 * 1024 * 1024;
/// Most that one zip may unpack to, counted while unpacking.
const UNZIP_MAX: u64 = 256 * 1024 * 1024;
/// Most that one part of a zip may unpack to.
const ENTRY_MAX: u64 = 128 * 1024 * 1024;
/// Most entries a zip may have.
const ENTRIES_MAX: usize = 20_000;
/// Longest text version, in bytes.
const OUT_MAX: usize = 3 * 1024 * 1024;
/// Most rows written for one sheet, and most columns.
const ROWS_MAX: usize = 20_000;
const COLS_MAX: usize = 256;
/// Longest text kept from one cell, in characters.
const CELL_MAX: usize = 500;
/// Longest line written: the assistant's Read tool cuts lines after 2000 characters.
const LINE_MAX: usize = 1500;
/// Most slides read from a deck.
const SLIDES_MAX: usize = 2_000;
/// Most cells (rows × columns of the used area, all sheets together) an old .xls may span: the
/// reader used for it keeps the whole used area in memory, filled or not.
const XLS_AREA_MAX: u64 = 3_000_000;
/// Longest one file may take.
pub const TIME_MAX: Duration = Duration::from_secs(20);

type Res<T> = Result<T, String>;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind { Xlsx, Xlsb, Xls, Ods, Docx, Pptx, Odt, Odp, Rtf, Doc, Ppt }

fn ext_of(name: &str) -> String {
    match name.rsplit_once('.') { Some((s, e)) if !s.is_empty() => e.to_ascii_lowercase(), _ => String::new() }
}

fn kind_of(name: &str) -> Option<Kind> {
    Some(match ext_of(name).as_str() {
        "xlsx" | "xlsm" | "xltx" | "xltm" => Kind::Xlsx,
        "xlsb" => Kind::Xlsb,
        "xls" | "xlt" => Kind::Xls,
        "ods" | "ots" => Kind::Ods,
        "docx" | "docm" | "dotx" | "dotm" => Kind::Docx,
        "pptx" | "pptm" | "ppsx" | "ppsm" | "potx" | "potm" => Kind::Pptx,
        "odt" | "ott" => Kind::Odt,
        "odp" | "otp" => Kind::Odp,
        "rtf" => Kind::Rtf,
        "doc" | "dot" => Kind::Doc,
        "ppt" | "pps" | "pot" => Kind::Ppt,
        _ => return None,
    })
}

/// Is this a file the Read tool cannot open but a text version can be made of?
pub fn convertible(name: &str) -> bool { kind_of(name).is_some() }

/// A file neither the Read tool nor this module can read (Apple's own office formats and the like).
pub fn opaque(name: &str) -> bool { matches!(ext_of(name).as_str(), "numbers" | "pages" | "key" | "one" | "pub" | "vsd" | "vsdx" | "mpp" | "accdb" | "mdb") }

/// What the file is, in the prompt's words.
pub fn what(name: &str) -> &'static str {
    match kind_of(name) {
        Some(Kind::Xlsx | Kind::Xlsb | Kind::Xls | Kind::Ods) => "a spreadsheet",
        Some(Kind::Pptx | Kind::Odp | Kind::Ppt) => "a slide deck",
        Some(_) => "a document",
        None => "a file",
    }
}

/// The text version of file `name` holding `bytes`, or why there is none (a plain reason, in
/// English: it goes into the prompt). Never panics; returns within about `TIME_MAX`.
pub fn convert(name: &str, bytes: &[u8]) -> Res<String> { convert_by(name, bytes, Instant::now() + TIME_MAX) }

pub fn convert_by(name: &str, bytes: &[u8], until: Instant) -> Res<String> {
    let kind = kind_of(name).ok_or("not an office file")?;
    if bytes.len() as u64 > IN_MAX { return Err("the file is too big".into()); }
    if bytes.is_empty() { return Err("the file is empty".into()); }
    let clock = Clock(until);
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut out = Out::new();
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
        out.s.push_str(&format!("Text version of \"{base}\", made for reading: formatting, pictures and charts are left out.\n"));
        match kind {
            Kind::Xlsx | Kind::Xlsb | Kind::Xls | Kind::Ods => out.s.push_str("One section per sheet, as CSV: the first column is the row number, the first line names the columns.\n\n"),
            _ => out.s.push('\n'),
        }
        let zipped = !matches!(kind, Kind::Rtf | Kind::Doc | Kind::Ppt | Kind::Xls);
        if zipped && bytes.starts_with(&CFB_MAGIC) { return Err("the file is protected with a password".to_string()); }
        if zipped && !bytes.starts_with(b"PK") { return Err("the file is not what its name says".to_string()); }
        if !zipped && kind != Kind::Rtf && !bytes.starts_with(&CFB_MAGIC) { return Err("the file is not what its name says".to_string()); }
        match kind {
            Kind::Xlsx => xlsx(bytes, false, &mut out, &clock)?,
            Kind::Xlsb => xlsx(bytes, true, &mut out, &clock)?,
            Kind::Xls => xls(bytes, &mut out, &clock)?,
            Kind::Ods => ods(bytes, &mut out, &clock)?,
            Kind::Docx => docx(bytes, &mut out, &clock)?,
            Kind::Pptx => pptx(bytes, &mut out, &clock)?,
            Kind::Odt | Kind::Odp => odf_text(bytes, &mut out, &clock)?,
            Kind::Rtf => rtf(bytes, &mut out, &clock)?,
            Kind::Doc => doc(bytes, &mut out, &clock)?,
            Kind::Ppt => ppt(bytes, &mut out, &clock)?,
        }
        Ok(out.finish())
    }));
    match made {
        Ok(r) => r,
        Err(_) => Err("the file could not be read (it may be damaged)".into()),
    }
}

const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

struct Clock(Instant);
impl Clock {
    fn over(&self) -> bool { Instant::now() >= self.0 }
    fn check(&self) -> Res<()> { if self.over() { Err("reading the file took too long".into()) } else { Ok(()) } }
}

// ---------- the text being made ----------

struct Out { s: String, cut: bool, body: usize }

impl Out {
    fn new() -> Self { Out { s: String::new(), cut: false, body: 0 } }
    fn room(&self) -> usize { OUT_MAX.saturating_sub(self.s.len()) }
    /// One line (or several: a long one is wrapped at spaces). Once the limit is reached nothing
    /// more is written and `cut` is set.
    fn line(&mut self, l: &str) {
        for part in l.split('\n') {
            for piece in wrap(part.trim_end_matches('\r')) {
                if self.cut || piece.len() + 1 > self.room() { self.cut = true; return; }
                self.s.push_str(piece);
                self.s.push('\n');
                if !piece.trim().is_empty() { self.body += 1; }
            }
        }
    }
    fn blank(&mut self) { if !self.s.ends_with("\n\n") { self.line(""); } }
    fn finish(mut self) -> String {
        if self.cut { self.s.push_str(&format!("\n[truncated: this text version stops here, at its limit of {} MB; the file holds more]\n", OUT_MAX / (1024 * 1024))); }
        else if self.body == 0 { self.s.push_str("(No text was found in the file.)\n"); }
        self.s
    }
}

/// A line in pieces of at most `LINE_MAX` bytes, broken at spaces where there are any.
fn wrap(l: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut rest = l;
    while rest.len() > LINE_MAX {
        let mut at = LINE_MAX;
        while !rest.is_char_boundary(at) { at -= 1; }
        let cut = rest[..at].rfind(' ').filter(|i| *i > LINE_MAX / 2).map_or(at, |i| i + 1);
        out.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    out.push(rest);
    out
}

/// Text without the characters that would break a line-based text file.
fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect()
}

// ---------- sheets ----------

/// A column's letters: 0 = A, 25 = Z, 26 = AA.
fn col_name(mut i: usize) -> String {
    let mut s = vec![];
    loop { s.push(b'A' + (i % 26) as u8); if i < 26 { break; } i = i / 26 - 1; }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// A cell's text as kept: at most `CELL_MAX` characters, then a note of how many more there were
/// (`more`: characters already dropped by the caller). Done once, before the text is stored or
/// copied, so that no cell held in memory is ever long.
fn cut_cell(v: &str, more: usize) -> String {
    let cut = v.char_indices().nth(CELL_MAX).map_or(v.len(), |(i, _)| i);
    let rest = &v[cut..];
    if rest.is_empty() && more == 0 { return v.to_string(); }
    // What is left is counted exactly when that is quick, and from its size when it is very long
    // (so that a huge text named by a million cells is not read through a million times).
    let (about, over) = if rest.len() <= 16_384 { ("", rest.chars().count()) } else { ("about ", rest.len()) };
    format!("{}… [truncated: {about}{} more characters in this cell]", &v[..cut], over + more)
}

fn csv_cell(v: &str) -> String {
    let v: String = clean(v).replace("\r\n", "\n").replace('\n', " \u{23ce} ").replace('\t', " ");
    if v.contains([',', '"']) || v.starts_with(' ') || v.ends_with(' ') { format!("\"{}\"", v.replace('"', "\"\"")) } else { v }
}

/// One sheet, written row by row as cells arrive (in row order).
struct Sheet { body: String, room: usize, rows: usize, more_rows: u64, widest: usize, row: Option<u32>, cells: Vec<(usize, String)> }

impl Sheet {
    fn new(room: usize) -> Self { Sheet { body: String::new(), room, rows: 0, more_rows: 0, widest: 0, row: None, cells: vec![] } }
    fn cell(&mut self, row: u32, col: u32, text: String) {
        if self.row != Some(row) { self.flush(); self.row = Some(row); }
        self.cell_cut(row, col, cut_cell(&text, 0));
    }
    /// `cell`, for text already cut to length (`cut_cell`).
    fn cell_cut(&mut self, row: u32, col: u32, text: String) {
        if self.row != Some(row) { self.flush(); self.row = Some(row); }
        if text.is_empty() { return; }
        let col = col as usize;
        self.widest = self.widest.max(col + 1);
        // (A file may name the same cell a million times: a row keeps no more cells than are shown.)
        if col < COLS_MAX && self.cells.len() < COLS_MAX { self.cells.push((col, text)); }
    }
    /// Has the sheet taken all the rows it will show? (Further rows are only counted.)
    fn full(&self) -> bool { self.rows >= ROWS_MAX || self.body.len() >= self.room }
    fn flush(&mut self) {
        let Some(row) = self.row.take() else { return };
        if self.cells.is_empty() { return; }
        let mut cells = std::mem::take(&mut self.cells);
        cells.sort_by_key(|c| c.0);
        let mut line = (row as u64 + 1).to_string();
        let mut at = 0;
        for (col, text) in cells {
            for _ in at..=col { line.push(','); }
            at = col + 1;
            line.push_str(&csv_cell(&text));
        }
        if self.rows >= ROWS_MAX || self.body.len() + line.len() + 1 > self.room { self.more_rows += 1; self.room = self.room.min(self.body.len()); return; }
        self.body.push_str(&line);
        self.body.push('\n');
        self.rows += 1;
    }
    /// Write the sheet. `stopped`: why reading ended before the sheet did, if it did.
    fn finish(mut self, out: &mut Out, n: usize, name: &str, stopped: Option<&str>) {
        self.flush();
        out.blank();
        out.line(&format!("## Sheet {n}: {}", clean(name).replace('\n', " ")));
        if self.rows == 0 && self.more_rows == 0 && stopped.is_none() { out.line("(empty)"); return; }
        let shown = self.widest.min(COLS_MAX);
        let head = std::iter::once("row".to_string()).chain((0..shown).map(col_name)).collect::<Vec<_>>().join(",");
        if out.cut || head.len() + self.body.len() + 2 > out.room() { out.cut = true; return; }
        out.s.push_str(&head);
        out.s.push('\n');
        out.s.push_str(&self.body);
        out.body += self.rows;
        if self.more_rows > 0 { out.line(&format!("[truncated: {} more rows of this sheet are not shown]", self.more_rows)); }
        if self.widest > COLS_MAX { out.line(&format!("[truncated: {} more columns of this sheet are not shown (it reaches column {})]", self.widest - COLS_MAX, col_name(self.widest - 1))); }
        if let Some(why) = stopped { out.line(&format!("[truncated: {why}; this sheet has more rows than are shown]")); }
    }
}

fn number(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 { format!("{}", f as i64) } else { format!("{f}") }
}

fn date(d: &calamine::ExcelDateTime) -> String {
    if d.is_duration() {
        let secs = (d.as_f64() * 86_400.0).round() as i64;
        return format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
    }
    let (y, mo, da, h, mi, s, _) = d.to_ymd_hms_milli();
    let whole = d.as_f64().fract() == 0.0;
    if whole { format!("{y:04}-{mo:02}-{da:02}") }
    else if d.as_f64() < 1.0 { format!("{h:02}:{mi:02}:{s:02}") }
    else { format!("{y:04}-{mo:02}-{da:02} {h:02}:{mi:02}:{s:02}") }
}

/// A cell's value as text, already cut to a cell's length (`cut_cell`).
fn cell_ref(v: &calamine::DataRef) -> String {
    use calamine::DataRef as D;
    match v {
        D::Int(i) => i.to_string(),
        D::Float(f) => number(*f),
        D::String(s) => cut_cell(s, 0),
        D::SharedString(s) => cut_cell(s, 0),
        D::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
        D::DateTime(d) => date(d),
        D::DateTimeIso(s) | D::DurationIso(s) => s.clone(),
        D::Error(e) => e.to_string(),
        D::Empty => String::new(),
    }
}

fn cell_data(v: &calamine::Data) -> String {
    use calamine::Data as D;
    match v {
        D::Int(i) => i.to_string(),
        D::Float(f) => number(*f),
        D::String(s) => cut_cell(s, 0),
        D::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
        D::DateTime(d) => date(d),
        D::DateTimeIso(s) | D::DurationIso(s) => s.clone(),
        D::Error(e) => e.to_string(),
        D::Empty => String::new(),
    }
}

const SLOW: &str = "reading stopped after the time allowed";

/// Excel workbooks (.xlsx and its kin; `binary`: .xlsb), read cell by cell: the sheet is never
/// held as a whole. The zip is unpacked once first, only to count what it unpacks to.
fn xlsx(bytes: &[u8], binary: bool, out: &mut Out, clock: &Clock) -> Res<()> {
    use calamine::Reader;
    unzip_count(bytes, clock)?;
    let bad = |e: &dyn std::fmt::Display| format!("the workbook could not be read ({e})");
    macro_rules! sheets {
        ($book:expr) => {{
            let mut book = $book;
            let names = book.sheet_names();
            if names.is_empty() { return Err("the workbook has no sheets".into()); }
            for (i, name) in names.iter().enumerate() {
                if out.cut { break; }
                let mut sheet = Sheet::new(out.room());
                let mut stopped = None;
                match book.worksheet_cells_reader(name) {
                    Ok(mut cells) => {
                        let mut n = 0u32;
                        loop {
                            match cells.next_cell() {
                                Ok(Some(c)) => { let (r, col) = c.get_position(); sheet.cell_cut(r, col, cell_ref(c.get_value())); }
                                Ok(None) => break,
                                Err(_) => { stopped = Some("the rest of this sheet could not be read"); break; }
                            }
                            n = n.wrapping_add(1);
                            if n % 4096 == 0 && clock.over() { stopped = Some(SLOW); break; }
                        }
                    }
                    // A chart sheet, a macro sheet: named, with nothing to read.
                    Err(_) => { out.blank(); out.line(&format!("## Sheet {}: {}", i + 1, clean(name))); out.line("(not a sheet of cells: nothing to read)"); continue; }
                }
                sheet.finish(out, i + 1, name, stopped);
                if stopped == Some(SLOW) { break; }
            }
        }};
    }
    if binary { sheets!(calamine::Xlsb::new(Cursor::new(bytes)).map_err(|e| bad(&e))?) }
    else { sheets!(calamine::Xlsx::new(Cursor::new(bytes)).map_err(|e| bad(&e))?) }
    Ok(())
}

/// The workbook's stream of records in an old Excel file.
fn xls_stream(bytes: &[u8], clock: &Clock) -> Res<Vec<u8>> {
    Ole::open(bytes, clock).and_then(|f| ["Workbook", "Book"].into_iter().find_map(|s| f.stream(s))).filter(|b| !b.is_empty()).ok_or_else(|| "the workbook could not be read".to_string())
}

/// Old Excel workbooks (.xls). The reader keeps each sheet's whole used area in memory, so the
/// area is measured first, from the cell records themselves, and a workbook spanning too much is
/// refused.
fn xls(bytes: &[u8], out: &mut Out, clock: &Clock) -> Res<()> {
    use calamine::Reader;
    let book = xls_stream(bytes, clock)?;
    let mut area = 0u64;
    let mut span: Option<(u16, u16, u16, u16)> = None;
    let mut close = |span: &mut Option<(u16, u16, u16, u16)>| if let Some((r0, r1, c0, c1)) = span.take() { area += (r1 - r0) as u64 * (c1 - c0) as u64 + (r1 - r0) as u64 + (c1 - c0) as u64 + 1; };
    let mut at = 0usize;
    while at + 4 <= book.len() {
        let id = u16::from_le_bytes([book[at], book[at + 1]]);
        let len = u16::from_le_bytes([book[at + 2], book[at + 3]]) as usize;
        let rec = book.get(at + 4..at + 4 + len).unwrap_or(&[]);
        match id {
            0x0809 | 0x0409 | 0x0209 | 0x0009 => close(&mut span),
            0x0006 | 0x0201 | 0x0203 | 0x0204 | 0x0205 | 0x027E | 0x00FD | 0x00BD | 0x00BE | 0x00D6 | 0x0406 | 0x0206 | 0x0001..=0x0005 if rec.len() >= 4 => {
                let row = u16::from_le_bytes([rec[0], rec[1]]);
                let col = u16::from_le_bytes([rec[2], rec[3]]);
                let last = if matches!(id, 0x00BD | 0x00BE) && rec.len() >= 6 { u16::from_le_bytes([rec[rec.len() - 2], rec[rec.len() - 1]]).max(col) } else { col };
                span = Some(match span { None => (row, row, col, last), Some((r0, r1, c0, c1)) => (r0.min(row), r1.max(row), c0.min(col), c1.max(last)) });
            }
            _ => {}
        }
        at += 4 + len;
    }
    close(&mut span);
    if area > XLS_AREA_MAX { return Err("the workbook is too large to turn into text here".into()); }
    clock.check()?;
    // The reader gets a container made here around the very stream that was measured: in the file
    // itself it might find another stream (a second "Workbook", a "Book" beside it) than the one looked at.
    let only = ole_single("Workbook", &book);
    drop(book);
    let mut book = calamine::Xls::new(Cursor::new(&only[..])).map_err(|e| format!("the workbook could not be read ({e})"))?;
    let names = book.sheet_names();
    if names.is_empty() { return Err("the workbook has no sheets".into()); }
    for (i, name) in names.iter().enumerate() {
        if out.cut { break; }
        let mut sheet = Sheet::new(out.room());
        let Ok(range) = book.worksheet_range(name) else { out.blank(); out.line(&format!("## Sheet {}: {}", i + 1, clean(name))); out.line("(not a sheet of cells: nothing to read)"); continue };
        let (r0, c0) = range.start().unwrap_or((0, 0));
        let mut stopped = None;
        for (r, row) in range.rows().enumerate() {
            for (c, v) in row.iter().enumerate() { sheet.cell_cut(r0 + r as u32, c0 + c as u32, cell_data(v)); }
            if r % 512 == 0 && clock.over() { stopped = Some(SLOW); break; }
        }
        sheet.finish(out, i + 1, name, stopped);
        if stopped.is_some() { break; }
    }
    Ok(())
}

// ---------- zips ----------

/// A zip whose parts are read by name, with everything unpacked counted against the limits.
struct Pack<'a> { zip: zip::ZipArchive<Cursor<&'a [u8]>>, left: u64 }

impl<'a> Pack<'a> {
    fn open(bytes: &'a [u8]) -> Res<Self> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "the file could not be opened (it may be damaged)".to_string())?;
        if zip.len() > ENTRIES_MAX { return Err("the file has too many parts".into()); }
        Ok(Pack { zip, left: UNZIP_MAX })
    }
    fn names(&self) -> Vec<String> { self.zip.file_names().flatten().map(|n| n.to_string()).collect() }
    /// The part called `name` (a name in the zip, never a path on disk), or `None` when there is none.
    fn read(&mut self, name: &str) -> Res<Option<Vec<u8>>> {
        let cap = ENTRY_MAX.min(self.left);
        let f = match self.zip.by_name(name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(_) => return Err("a part of the file could not be opened (it may be protected or damaged)".into()),
        };
        let mut buf = Vec::new();
        f.take(cap + 1).read_to_end(&mut buf).map_err(|_| "a part of the file could not be unpacked".to_string())?;
        if buf.len() as u64 > cap { return Err("the file unpacks to more than is read here".into()); }
        self.left -= buf.len() as u64;
        Ok(Some(buf))
    }
}

/// Unpack the whole zip into nothing, to know what it really unpacks to before a reader that
/// keeps parts in memory is let at it.
fn unzip_count(bytes: &[u8], clock: &Clock) -> Res<()> { unzip_count_to(bytes, clock, UNZIP_MAX) }

fn unzip_count_to(bytes: &[u8], clock: &Clock, max: u64) -> Res<()> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "the file could not be opened (it may be damaged)".to_string())?;
    if zip.len() > ENTRIES_MAX { return Err("the file has too many parts".into()); }
    let mut left = max;
    for i in 0..zip.len() {
        clock.check()?;
        let f = zip.by_index(i).map_err(|_| "a part of the file could not be opened (it may be protected or damaged)".to_string())?;
        let n = std::io::copy(&mut f.take(left + 1), &mut std::io::sink()).map_err(|_| "a part of the file could not be unpacked".to_string())?;
        if n > left { return Err("the file unpacks to more than is read here".into()); }
        left -= n;
    }
    Ok(())
}

// ---------- XML ----------

use quick_xml::events::{BytesStart, Event};

enum Node<'a> { Open(&'a [u8], &'a BytesStart<'a>), Close(&'a [u8]), Text(&'a str) }

/// The part of an element or attribute name after its prefix.
fn local(name: &[u8]) -> &[u8] { name.iter().rposition(|b| *b == b':').map_or(name, |i| &name[i + 1..]) }

fn attr(e: &BytesStart, key: &[u8]) -> Option<String> {
    e.attributes().with_checks(false).flatten().find(|a| local(a.key.0.as_bytes()) == key)
        .map(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map(|v| v.into_owned()).unwrap_or_else(|_| a.value.to_string()))
}

/// Walk an XML part: every element opening and closing (by local name) and every piece of text.
/// Entities a file defines itself are not expanded.
fn walk(xml: &[u8], clock: &Clock, mut f: impl FnMut(Node) -> Res<()>) -> Res<()> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().check_end_names = false;
    let mut n = 0u32;
    loop {
        n = n.wrapping_add(1);
        if n % 8192 == 0 { clock.check()?; }
        match reader.read_event() {
            Ok(Event::Start(e)) => f(Node::Open(local(e.name().0.as_bytes()), &e))?,
            Ok(Event::Empty(e)) => { let name = e.name(); let l = local(name.0.as_bytes()); f(Node::Open(l, &e))?; f(Node::Close(l))?; }
            Ok(Event::End(e)) => f(Node::Close(local(e.name().0.as_bytes())))?,
            Ok(Event::Text(t)) => f(Node::Text(&t))?,
            Ok(Event::CData(t)) => f(Node::Text(&t))?,
            Ok(Event::GeneralRef(r)) => {
                let name = r.to_string();
                let ch = r.resolve_char_ref().ok().flatten().map(String::from)
                    .or_else(|| quick_xml::escape::resolve_predefined_entity(&name).map(str::to_string));
                if let Some(s) = ch { f(Node::Text(&s))?; }
            }
            Ok(Event::Eof) => return Ok(()),
            Ok(_) => {}
            Err(_) => return Err("the file's contents could not be read (it may be damaged)".into()),
        }
    }
}

// ---------- running text: paragraphs, lists, tables ----------

struct Table { row: Vec<String>, cell: String, rows: usize }

/// Paragraphs and tables in reading order. A table becomes a markdown table; a table inside a
/// cell is written into that cell.
struct Flow<'o> { out: &'o mut Out, tables: Vec<Table> }

impl<'o> Flow<'o> {
    fn new(out: &'o mut Out) -> Self { Flow { out, tables: vec![] } }
    fn para(&mut self, prefix: &str, text: &str) {
        let text = clean(text);
        match self.tables.last_mut() {
            Some(t) => {
                let text = text.trim();
                if text.is_empty() { return; }
                if !t.cell.is_empty() { t.cell.push_str("<br>"); }
                t.cell.push_str(&text.replace('\n', "<br>").replace('\t', " "));
            }
            None => {
                if text.trim().is_empty() { self.out.blank(); return; }
                if prefix.starts_with('#') { self.out.blank(); }
                self.out.line(&format!("{prefix}{}", text.trim_end()));
                if prefix.is_empty() || prefix.starts_with('#') { self.out.blank(); }
            }
        }
    }
    fn table(&mut self) { if self.tables.is_empty() { self.out.blank(); } self.tables.push(Table { row: vec![], cell: String::new(), rows: 0 }); }
    fn end_cell(&mut self) { if let Some(t) = self.tables.last_mut() { let c = std::mem::take(&mut t.cell); t.row.push(c); } }
    fn end_row(&mut self) {
        let Some(t) = self.tables.last_mut() else { return };
        if !t.cell.is_empty() { let c = std::mem::take(&mut t.cell); t.row.push(c); }
        let row = std::mem::take(&mut t.row);
        if row.is_empty() { return; }
        t.rows += 1;
        let first = t.rows == 1;
        let line = format!("| {} |", row.iter().map(|c| c.replace('|', "\\|")).collect::<Vec<_>>().join(" | "));
        if self.tables.len() > 1 {
            let n = self.tables.len();
            let outer = &mut self.tables[n - 2];
            if !outer.cell.is_empty() { outer.cell.push_str("<br>"); }
            outer.cell.push_str(&line);
        } else {
            self.out.line(&line);
            if first { self.out.line(&format!("|{}", " --- |".repeat(row.len()))); }
        }
    }
    fn end_table(&mut self) { self.end_row(); self.tables.pop(); if self.tables.is_empty() { self.out.blank(); } }
    fn end(&mut self) { while !self.tables.is_empty() { self.end_table(); } }
}

/// A Word heading style's level ("Heading1", "heading 2", "Title").
fn heading_level(style: &str) -> Option<usize> {
    let s = style.to_ascii_lowercase().replace([' ', '-', '_'], "");
    if s == "title" { return Some(1); }
    let n = s.strip_prefix("heading")?.parse::<usize>().ok()?;
    (1..=9).contains(&n).then_some(n.min(6))
}

/// One WordprocessingML part (the document itself, its footnotes, its comments).
fn word_part(xml: &[u8], out: &mut Out, clock: &Clock) -> Res<()> {
    let mut flow = Flow::new(out);
    let (mut para, mut in_t, mut skip) = (String::new(), false, 0usize);
    let (mut heading, mut list): (Option<usize>, Option<usize>) = (None, None);
    // Inside a paragraph's properties: its tab stops are settings, not tab characters.
    let mut props = 0usize;
    walk(xml, clock, |n| {
        if flow.out.cut { return Ok(()); }
        match n {
            // A text box is stored twice (new and old drawing): the second, `Fallback`, is skipped.
            Node::Open(b"Fallback", _) => skip += 1,
            Node::Close(b"Fallback") => skip = skip.saturating_sub(1),
            _ if skip > 0 => {}
            Node::Open(b"p", _) => { para.clear(); heading = None; list = None; }
            Node::Open(b"pPr" | b"rPr", _) => props += 1,
            Node::Close(b"pPr" | b"rPr") => props = props.saturating_sub(1),
            Node::Open(b"pStyle", e) => heading = attr(e, b"val").and_then(|v| heading_level(&v)),
            Node::Open(b"numPr", _) => list = Some(list.unwrap_or(0)),
            Node::Open(b"ilvl", e) => if list.is_some() { list = attr(e, b"val").and_then(|v| v.parse().ok()).map(|n: usize| n.min(8)); },
            Node::Open(b"t", _) => in_t = true,
            Node::Close(b"t") => in_t = false,
            Node::Text(s) if in_t => para.push_str(s),
            Node::Open(b"tab", _) if props == 0 => para.push('\t'),
            Node::Open(b"br" | b"cr", _) if props == 0 => para.push('\n'),
            Node::Open(b"noBreakHyphen", _) => para.push('-'),
            Node::Close(b"p") => {
                let prefix = match (heading, list) { (Some(h), _) => format!("{} ", "#".repeat(h)), (_, Some(l)) => format!("{}- ", "  ".repeat(l)), _ => String::new() };
                flow.para(&prefix, &para);
                para.clear();
            }
            Node::Open(b"tbl", _) => flow.table(),
            Node::Close(b"tc") => flow.end_cell(),
            Node::Close(b"tr") => flow.end_row(),
            Node::Close(b"tbl") => flow.end_table(),
            _ => {}
        }
        Ok(())
    })?;
    flow.end();
    Ok(())
}

fn docx(bytes: &[u8], out: &mut Out, clock: &Clock) -> Res<()> {
    let mut pack = Pack::open(bytes)?;
    // The main part is named by the package's relationships; almost always word/document.xml.
    let main = match pack.read("word/document.xml")? {
        Some(x) => x,
        None => {
            let name = pack.names().into_iter().find(|n| n.starts_with("word/document") && n.ends_with(".xml")).ok_or("the file is not a Word document")?;
            pack.read(&name)?.ok_or("the file is not a Word document")?
        }
    };
    word_part(&main, out, clock)?;
    drop(main);
    for (part, title) in [("word/footnotes.xml", "Footnotes"), ("word/endnotes.xml", "Endnotes"), ("word/comments.xml", "Comments")] {
        if out.cut { break; }
        let Some(xml) = pack.read(part)? else { continue };
        let mut sub = Out::new();
        word_part(&xml, &mut sub, clock)?;
        if sub.body == 0 { continue; }
        out.blank();
        out.line(&format!("## {title}"));
        out.blank();
        out.line(sub.s.trim_matches('\n'));
        if sub.cut { out.cut = true; }
    }
    Ok(())
}

// ---------- PowerPoint ----------

/// `target` of a relationship of the part `from`, as a name in the zip.
fn zip_target(from: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') { return abs.to_string(); }
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for step in target.split('/') {
        match step { "" | "." => {} ".." => { parts.pop(); } s => parts.push(s) }
    }
    parts.join("/")
}

/// The relationships of zip part `part`: (id, type, target as a name in the zip).
fn rels(pack: &mut Pack, part: &str, clock: &Clock) -> Res<Vec<(String, String, String)>> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let name = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
    let Some(xml) = pack.read(&name)? else { return Ok(vec![]) };
    let mut out = vec![];
    walk(&xml, clock, |n| {
        if let Node::Open(b"Relationship", e) = n {
            if attr(e, b"TargetMode").as_deref() != Some("External") && out.len() < 10_000 {
                out.push((attr(e, b"Id").unwrap_or_default(), attr(e, b"Type").unwrap_or_default(), zip_target(part, &attr(e, b"Target").unwrap_or_default())));
            }
        }
        Ok(())
    })?;
    Ok(out)
}

/// The text of one DrawingML part (a slide, a notes page): a line per paragraph, tables as rows.
fn slide_text(xml: &[u8], notes: bool, clock: &Clock) -> Res<Vec<String>> {
    let mut lines: Vec<String> = vec![];
    let (mut para, mut in_t, mut skip) = (String::new(), false, 0usize);
    let (mut row, mut cell, mut in_table): (Vec<String>, String, bool) = (vec![], String::new(), false);
    walk(xml, clock, |n| {
        match n {
            Node::Open(b"Fallback", _) => skip += 1,
            Node::Close(b"Fallback") => skip = skip.saturating_sub(1),
            // On a notes page the slide number is a field: not the speaker's words.
            Node::Open(b"fld", _) if notes => skip += 1,
            Node::Close(b"fld") if notes => skip = skip.saturating_sub(1),
            _ if skip > 0 => {}
            Node::Open(b"tbl", _) => in_table = true,
            Node::Close(b"tbl") => in_table = false,
            Node::Open(b"p", _) => para.clear(),
            Node::Open(b"t", _) => in_t = true,
            Node::Close(b"t") => in_t = false,
            Node::Text(s) if in_t => para.push_str(s),
            Node::Open(b"br", _) => para.push('\n'),
            Node::Close(b"p") => {
                let text = clean(para.trim());
                para.clear();
                if text.is_empty() { return Ok(()); }
                if in_table { if !cell.is_empty() { cell.push_str("<br>"); } cell.push_str(&text.replace('\n', "<br>")); }
                else if lines.len() < 50_000 { lines.push(text); }
            }
            Node::Close(b"tc") if in_table => row.push(std::mem::take(&mut cell).replace('|', "\\|")),
            Node::Close(b"tr") if in_table => { if !row.is_empty() { lines.push(format!("| {} |", row.join(" | "))); row.clear(); } }
            _ => {}
        }
        Ok(())
    })?;
    Ok(lines)
}

fn pptx(bytes: &[u8], out: &mut Out, clock: &Clock) -> Res<()> {
    let mut pack = Pack::open(bytes)?;
    let main = "ppt/presentation.xml";
    let mut order: Vec<String> = vec![];
    if let Some(xml) = pack.read(main)? {
        let by_id = rels(&mut pack, main, clock)?;
        walk(&xml, clock, |n| {
            if let Node::Open(b"sldId", e) = n {
                // `r:id`, not the slide's own numeric `id`.
                let rid = e.attributes().with_checks(false).flatten().find(|a| a.key.0.ends_with(":id")).map(|a| a.value.to_string());
                if let Some(t) = rid.and_then(|rid| by_id.iter().find(|r| r.0 == rid)) { if order.len() < SLIDES_MAX { order.push(t.2.clone()); } }
            }
            Ok(())
        })?;
    }
    if order.is_empty() {
        // No readable list: the slides in the order of their numbers.
        let mut found: Vec<(u32, String)> = pack.names().into_iter().filter_map(|n| {
            let num = n.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse().ok()?;
            Some((num, n))
        }).collect();
        found.sort();
        order = found.into_iter().take(SLIDES_MAX).map(|f| f.1).collect();
    }
    if order.is_empty() { return Err("the file is not a slide deck".into()); }
    for (i, part) in order.iter().enumerate() {
        if out.cut { break; }
        clock.check()?;
        out.blank();
        out.line(&format!("## Slide {}", i + 1));
        let Some(xml) = pack.read(part)? else { out.line("(this slide is missing from the file)"); continue };
        let lines = slide_text(&xml, false, clock)?;
        if lines.is_empty() { out.line("(no text on this slide)"); }
        for l in &lines { out.line(l); }
        let notes = rels(&mut pack, part, clock)?.into_iter().find(|r| r.1.ends_with("/notesSlide"));
        if let Some(xml) = match notes { Some(r) => pack.read(&r.2)?, None => None } {
            let lines = slide_text(&xml, true, clock)?;
            if !lines.is_empty() { out.blank(); out.line("Notes:"); for l in &lines { out.line(l); } }
        }
    }
    Ok(())
}

// ---------- OpenDocument ----------

fn odf_content(bytes: &[u8]) -> Res<Vec<u8>> {
    Pack::open(bytes)?.read("content.xml")?.ok_or_else(|| "the file is not an OpenDocument file".to_string())
}

/// OpenDocument text and presentations: headings, paragraphs, lists, tables; a deck page by page
/// with its notes.
fn odf_text(bytes: &[u8], out: &mut Out, clock: &Clock) -> Res<()> {
    let xml = odf_content(bytes)?;
    let mut flow = Flow::new(out);
    // `depth`: how many paragraphs or headings are open (a footnote's paragraph lies inside another).
    let (mut para, mut depth, mut skip, mut page) = (String::new(), 0usize, 0usize, 0usize);
    let (mut heading, mut lists, mut item_new): (Option<usize>, usize, bool) = (None, 0, false);
    walk(&xml, clock, |n| {
        if flow.out.cut { return Ok(()); }
        match n {
            Node::Open(b"tracked-changes" | b"forms", _) => skip += 1,
            Node::Close(b"tracked-changes" | b"forms") => skip = skip.saturating_sub(1),
            _ if skip > 0 => {}
            Node::Open(b"page", e) => {
                flow.end();
                page += 1;
                let name = attr(e, b"name").filter(|n| !n.is_empty()).map(|n| format!(": {}", clean(&n))).unwrap_or_default();
                flow.para("## ", &format!("Slide {page}{name}"));
            }
            Node::Open(b"notes", _) if page > 0 => flow.para("", "Notes:"),
            Node::Open(b"p", _) => { if depth == 0 { para.clear(); heading = None; } else { para.push(' '); } depth += 1; }
            Node::Open(b"h", e) => {
                if depth == 0 { para.clear(); heading = Some(attr(e, b"outline-level").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 6)); }
                depth += 1;
            }
            Node::Open(b"list", _) => lists += 1,
            Node::Close(b"list") => lists = lists.saturating_sub(1),
            Node::Open(b"list-item" | b"list-header", _) => item_new = true,
            Node::Text(s) if depth > 0 => para.push_str(s),
            Node::Open(b"s", e) if depth > 0 => { let n = attr(e, b"c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(100); para.extend(std::iter::repeat(' ').take(n)); }
            Node::Open(b"tab", _) if depth > 0 => para.push('\t'),
            Node::Open(b"line-break", _) if depth > 0 => para.push('\n'),
            Node::Close(b"p" | b"h") if depth > 0 => {
                depth -= 1;
                if depth > 0 { return Ok(()); }
                let prefix = match heading {
                    // A deck's page titles are below the "Slide N" heading.
                    Some(h) => format!("{} ", "#".repeat(if page > 0 { (h + 2).min(6) } else { h })),
                    None if lists > 0 && item_new => format!("{}- ", "  ".repeat((lists - 1).min(8))),
                    None if lists > 0 => "  ".repeat(lists.min(9)),
                    None => String::new(),
                };
                if lists > 0 && !para.trim().is_empty() { item_new = false; }
                flow.para(&prefix, &para);
                para.clear();
                heading = None;
            }
            Node::Open(b"table", _) => flow.table(),
            Node::Close(b"table-cell" | b"covered-table-cell") => flow.end_cell(),
            Node::Close(b"table-row") => flow.end_row(),
            Node::Close(b"table") => flow.end_table(),
            _ => {}
        }
        Ok(())
    })?;
    flow.end();
    Ok(())
}

/// OpenDocument spreadsheets. Rows and cells may be said to repeat (a million empty rows are one
/// element): repeats are counted, never unrolled beyond what is written.
fn ods(bytes: &[u8], out: &mut Out, clock: &Clock) -> Res<()> {
    let xml = odf_content(bytes)?;
    struct Cur { sheet: Sheet, name: String, row: u32 }
    let mut cur: Option<Cur> = None;
    let mut count = 0usize;
    let (mut row_rep, mut col, mut cell_rep) = (1u32, 0u32, 1u32);
    let (mut row_cells, mut text, mut value): (Vec<(u32, String)>, String, Option<String>) = (vec![], String::new(), None);
    let (mut in_cell, mut depth, mut skip, mut nested) = (false, 0usize, 0usize, 0usize);
    let (mut row_widest, mut dropped) = (0usize, 0usize);
    let reps = |e: &BytesStart, key: &[u8]| attr(e, key).and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).clamp(1, 1 << 30);
    let mut stopped = false;
    walk(&xml, clock, |n| {
        if stopped { return Ok(()); }
        match n {
            Node::Open(b"annotation" | b"tracked-changes" | b"forms", _) => skip += 1,
            Node::Close(b"annotation" | b"tracked-changes" | b"forms") => skip = skip.saturating_sub(1),
            _ if skip > 0 => {}
            Node::Open(b"table", e) => {
                if cur.is_some() { nested += 1; return Ok(()); }
                if out.cut { stopped = true; return Ok(()); }
                cur = Some(Cur { sheet: Sheet::new(out.room()), name: attr(e, b"name").unwrap_or_default(), row: 0 });
            }
            Node::Close(b"table") => {
                if nested > 0 { nested -= 1; return Ok(()); }
                if let Some(c) = cur.take() { count += 1; c.sheet.finish(out, count, &c.name, None); }
            }
            _ if nested > 0 => {}
            Node::Open(b"table-row", e) => { row_rep = reps(e, b"number-rows-repeated"); col = 0; row_cells.clear(); row_widest = 0; }
            Node::Close(b"table-row") => {
                let Some(c) = cur.as_mut() else { return Ok(()) };
                if !row_cells.is_empty() {
                    c.sheet.widest = c.sheet.widest.max(row_widest);
                    // A row said to repeat is written until the sheet is full; the rest are counted.
                    let mut written = 0u32;
                    while written < row_rep && !c.sheet.full() {
                        if written % 256 == 255 { clock.check()?; }
                        for (col, t) in &row_cells { c.sheet.cell_cut(c.row.saturating_add(written), *col, t.clone()); }
                        c.sheet.flush();
                        written += 1;
                    }
                    c.sheet.more_rows += (row_rep - written) as u64;
                }
                c.row = c.row.saturating_add(row_rep);
            }
            Node::Open(b"table-cell" | b"covered-table-cell", e) => {
                in_cell = true;
                text.clear();
                dropped = 0;
                cell_rep = reps(e, b"number-columns-repeated");
                value = ["date-value", "time-value", "boolean-value", "value", "string-value"].iter().find_map(|k| attr(e, k.as_bytes()));
            }
            Node::Close(b"table-cell" | b"covered-table-cell") => {
                in_cell = false;
                // Cut to a cell's length before it is kept, and long before it is copied for repeats.
                let shown = if text.is_empty() { cut_cell(&value.take().unwrap_or_default(), 0) } else { cut_cell(&std::mem::take(&mut text), dropped) };
                if !shown.is_empty() {
                    row_widest = row_widest.max(col.saturating_add(cell_rep) as usize);
                    for i in 0..cell_rep.min(COLS_MAX as u32) {
                        if col.saturating_add(i) as usize >= COLS_MAX || row_cells.len() >= COLS_MAX { break; }
                        row_cells.push((col.saturating_add(i), shown.clone()));
                    }
                }
                col = col.saturating_add(cell_rep);
            }
            Node::Open(b"p", _) if in_cell => { if depth == 0 && !text.is_empty() { text.push('\n'); } depth += 1; }
            Node::Close(b"p") if in_cell => depth = depth.saturating_sub(1),
            // (No more of a cell's text is held than a few times what is kept of it.)
            Node::Text(s) if in_cell && depth > 0 => {
                let mut room = (CELL_MAX * 8).saturating_sub(text.len()).min(s.len());
                while !s.is_char_boundary(room) { room -= 1; }
                text.push_str(&s[..room]);
                dropped += s[room..].chars().count();
            }
            Node::Open(b"s", e) if in_cell && depth > 0 => { let n = attr(e, b"c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(100); text.extend(std::iter::repeat(' ').take(n)); }
            Node::Open(b"tab", _) if in_cell && depth > 0 => text.push(' '),
            Node::Open(b"line-break", _) if in_cell && depth > 0 => text.push('\n'),
            _ => {}
        }
        Ok(())
    })?;
    if count == 0 { return Err("the file has no sheets".into()); }
    Ok(())
}

// ---------- Rich Text ----------

fn code_page(n: i64) -> &'static encoding_rs::Encoding {
    let label = match n { 10000 => "macintosh".to_string(), 932 => "shift_jis".into(), 936 => "gbk".into(), 949 => "euc-kr".into(), 950 => "big5".into(), 65001 => "utf-8".into(), n => format!("windows-{n}") };
    encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::WINDOWS_1252)
}

fn charset(n: i64) -> Option<&'static encoding_rs::Encoding> {
    Some(code_page(match n { 0 | 1 => 1252, 77 => 10000, 128 => 932, 129 => 949, 134 => 936, 136 => 950, 161 => 1253, 162 => 1254, 163 => 1258, 177 => 1255, 178 => 1256, 186 => 1257, 204 => 1251, 222 => 874, 238 => 1250, _ => return None }))
}

/// Rich Text Format: the document's text, paragraph by paragraph, table rows as lines of cells.
/// Pictures, embedded objects, headers, footers and notes are left out.
fn rtf(bytes: &[u8], out: &mut Out, clock: &Clock) -> Res<()> {
    if !bytes.starts_with(b"{\\rtf") { return Err("the file is not what its name says".into()); }
    const SKIP: &[&str] = &["fonttbl", "colortbl", "stylesheet", "info", "pict", "object", "header", "headerl", "headerr", "headerf", "footer", "footerl", "footerr", "footerf",
        "footnote", "annotation", "atnid", "atnauthor", "xmlnstbl", "listtable", "listoverridetable", "rsidtbl", "generator", "themedata", "colorschememapping", "datastore",
        "latentstyles", "pgdsctbl", "revtbl", "fldinst", "filetbl", "nonshppict", "bkmkstart", "bkmkend", "template", "userprops", "xe", "tc", "operator", "author", "title", "company",
        "pntext", "listtext"];
    #[derive(Clone, Copy)]
    struct St { skip: bool, fonts: bool, font: i64, uc: usize }
    let mut st = St { skip: false, fonts: false, font: -1, uc: 1 };
    let mut stack: Vec<St> = vec![];
    let mut ansi = encoding_rs::WINDOWS_1252;
    let mut default_font = -1i64;
    let mut fonts: std::collections::HashMap<i64, &'static encoding_rs::Encoding> = Default::default();
    let (mut line, mut pending, mut to_skip) = (String::new(), Vec::<u8>::new(), 0usize);
    let mut in_row = false;
    let mut high: Option<u32> = None;
    let n = bytes.len();
    let mut i = 0usize;
    let mut steps = 0u32;
    macro_rules! flush { () => { if !pending.is_empty() {
        let enc = fonts.get(&(if st.font >= 0 { st.font } else { default_font })).copied().unwrap_or(ansi);
        line.push_str(&enc.decode_without_bom_handling(&pending).0);
        pending.clear();
    } }; }
    macro_rules! newline { () => {{ flush!(); let l = clean(&line); line.clear(); if in_row { out.line(&format!("| {}", l.trim())); in_row = false; } else { out.line(l.trim_end()); } }}; }
    while i < n {
        steps = steps.wrapping_add(1);
        if steps % 65_536 == 0 { clock.check()?; }
        if out.cut { break; }
        match bytes[i] {
            b'{' => { flush!(); if stack.len() >= 4096 { return Err("the file is nested too deeply".into()); } stack.push(st); i += 1; }
            b'}' => { flush!(); if let Some(s) = stack.pop() { st = s; } i += 1; }
            b'\\' => {
                i += 1;
                if i >= n { break; }
                let c = bytes[i];
                if c.is_ascii_alphabetic() {
                    let start = i;
                    while i < n && bytes[i].is_ascii_alphabetic() && i - start < 32 { i += 1; }
                    let word = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
                    let num_start = i;
                    if i < n && bytes[i] == b'-' { i += 1; }
                    while i < n && bytes[i].is_ascii_digit() && i - num_start < 11 { i += 1; }
                    let param: Option<i64> = std::str::from_utf8(&bytes[num_start..i]).ok().and_then(|s| s.parse().ok());
                    if i < n && bytes[i] == b' ' { i += 1; }
                    if word == "bin" { flush!(); i = i.saturating_add(param.unwrap_or(0).max(0) as usize).min(n); continue; }
                    if word == "fonttbl" { st.fonts = true; }
                    if SKIP.contains(&word) { st.skip = true; continue; }
                    if st.fonts {
                        match word { "f" => st.font = param.unwrap_or(0), "fcharset" => if let Some(e) = charset(param.unwrap_or(0)) { fonts.insert(st.font, e); }, _ => {} }
                        continue;
                    }
                    if st.skip { continue; }
                    match word {
                        "ansicpg" => ansi = code_page(param.unwrap_or(1252)),
                        "deff" => default_font = param.unwrap_or(0),
                        "f" => { flush!(); st.font = param.unwrap_or(0); }
                        "uc" => st.uc = param.unwrap_or(1).clamp(0, 16) as usize,
                        "u" => {
                            flush!();
                            let v = param.unwrap_or(0).rem_euclid(65_536) as u32;
                            // A character outside the first 65,536 arrives as two halves.
                            if (0xD800..0xDC00).contains(&v) { high = Some(v); }
                            else if (0xDC00..0xE000).contains(&v) { if let Some(ch) = high.take().and_then(|h| char::from_u32(0x10000 + ((h - 0xD800) << 10) + (v - 0xDC00))) { line.push(ch); } }
                            else if let Some(ch) = char::from_u32(v) { line.push(ch); }
                            to_skip = st.uc;
                        }
                        "par" | "sect" | "page" | "line" => newline!(),
                        "row" => { flush!(); in_row = true; newline!(); }
                        "cell" | "nestcell" => { flush!(); line.push_str(" | "); in_row = true; }
                        "tab" => { flush!(); line.push('\t'); }
                        "emdash" => { flush!(); line.push('—'); }
                        "endash" => { flush!(); line.push('–'); }
                        "bullet" => { flush!(); line.push('•'); }
                        "lquote" => { flush!(); line.push('‘'); }
                        "rquote" => { flush!(); line.push('’'); }
                        "ldblquote" => { flush!(); line.push('“'); }
                        "rdblquote" => { flush!(); line.push('”'); }
                        _ => {}
                    }
                } else {
                    i += 1;
                    match c {
                        b'\'' => {
                            let hex = bytes.get(i..i + 2).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
                            i = (i + 2).min(n);
                            if to_skip > 0 { to_skip -= 1; } else if let (Some(b), false) = (hex, st.skip || st.fonts) { pending.push(b); }
                        }
                        // `{\*\word …}`: a part readers that do not know `word` are told to skip.
                        b'*' => st.skip = true,
                        b'\\' | b'{' | b'}' => if to_skip > 0 { to_skip -= 1; } else if !st.skip && !st.fonts { flush!(); line.push(c as char); },
                        b'~' => if !st.skip && !st.fonts { flush!(); line.push('\u{a0}'); },
                        b'_' => if !st.skip && !st.fonts { flush!(); line.push('-'); },
                        b'\n' | b'\r' => if !st.skip && !st.fonts { newline!(); },
                        _ => {}
                    }
                }
            }
            b'\r' | b'\n' => i += 1,
            b => {
                i += 1;
                if to_skip > 0 { to_skip -= 1; } else if !st.skip && !st.fonts { pending.push(b); }
            }
        }
    }
    flush!();
    if !line.trim().is_empty() { newline!(); }
    let _ = in_row;
    Ok(())
}

// ---------- old binary Word and PowerPoint ----------

/// An OLE compound file (the container of old Office files), read leniently: real files from
/// other programs break rules a strict reader insists on. Streams are found by name.
///
/// Nothing in the file is trusted to end a read: a chain of sectors is followed for at most as
/// many steps as the file really has sectors and stops at a sector it has already taken, so a
/// chain that loops or repeats ends; a stream is never longer than the file, whatever size it
/// declares; a sector listed twice as part of the allocation table counts once; and the clock is
/// looked at along the way.
struct Ole<'a> { bytes: &'a [u8], shift: u32, sectors: usize, fat: Vec<u32>, mini_fat: Vec<u32>, mini: Vec<u8>, cutoff: u64, dir: Vec<(String, u8, u32, u64)>, clock: &'a Clock }

impl<'a> Ole<'a> {
    fn open(bytes: &'a [u8], clock: &'a Clock) -> Option<Self> {
        if !bytes.starts_with(&CFB_MAGIC) || bytes.len() < 512 { return None; }
        let shift = le16(bytes, 30)? as u32;
        if shift != 9 && shift != 12 { return None; }
        let size = 1usize << shift;
        // Sectors the file really has (the header takes the place of one).
        let sectors = (bytes.len() >> shift).saturating_sub(1);
        let sector = |n: u32| -> Option<&'a [u8]> { let at = (n as usize).checked_add(1)?.checked_mul(size)?; bytes.get(at..at.checked_add(size)?) };
        // The sectors holding the allocation table: 109 named in the header, the rest in a chain
        // of lists. Each real sector is taken once, so the table is never bigger than the file.
        let mut seen = vec![false; sectors];
        let mut fat_at: Vec<u32> = vec![];
        let mut take = |v: u32, fat_at: &mut Vec<u32>| if let Some(s) = seen.get_mut(v as usize) { if !*s { *s = true; fat_at.push(v); } };
        for i in 0..109 { take(le32(bytes, 76 + i * 4)?, &mut fat_at); }
        let mut lists = vec![false; sectors];
        let mut next = le32(bytes, 68)?;
        while let Some(visited) = lists.get_mut(next as usize) {
            if *visited || clock.over() { break; }
            *visited = true;
            let s = sector(next)?;
            for i in 0..size / 4 - 1 { take(le32(s, i * 4)?, &mut fat_at); }
            next = le32(s, size - 4)?;
        }
        let mut fat = Vec::with_capacity(fat_at.len() * (size / 4));
        for s in fat_at { let s = sector(s)?; fat.extend(s.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))); }
        let mut ole = Ole { bytes, shift, sectors, fat, mini_fat: vec![], mini: vec![], cutoff: le32(bytes, 56)? as u64, dir: vec![], clock };
        let dir = ole.chain(le32(bytes, 48)?, u64::MAX);
        for e in dir.chunks_exact(128).take(100_000) {
            let len = (le16(e, 64)? as usize).min(64);
            let name = String::from_utf16_lossy(&e[..len].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>());
            let size = if shift == 9 { le32(e, 120)? as u64 } else { u64::from_le_bytes(e.get(120..128)?.try_into().ok()?) };
            ole.dir.push((name.trim_end_matches('\0').to_string(), e[66], le32(e, 116)?, size));
        }
        let mini_fat = ole.chain(le32(bytes, 60)?, u64::MAX);
        ole.mini_fat = mini_fat.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        if let Some(root) = ole.dir.iter().find(|d| d.1 == 5).cloned() { ole.mini = ole.chain(root.2, root.3); }
        if clock.over() { return None; }
        Some(ole)
    }
    /// The sectors chained from `start`, at most `size` bytes of them and never more than the file holds.
    fn chain(&self, start: u32, size: u64) -> Vec<u8> {
        let sector = 1usize << self.shift;
        let size = size.min(self.bytes.len() as u64);
        let mut out = Vec::new();
        let mut taken = vec![false; self.sectors];
        let (mut at, mut steps) = (start, 0usize);
        while (out.len() as u64) < size && steps < self.sectors {
            // A sector outside the file, an end mark, or one this chain already took: the end.
            match taken.get_mut(at as usize) { Some(t) if !*t => *t = true, _ => break }
            let from = (at as usize + 1) << self.shift;
            let Some(s) = self.bytes.get(from..(from + sector).min(self.bytes.len())) else { break };
            out.extend_from_slice(s);
            at = self.fat.get(at as usize).copied().unwrap_or(0xFFFF_FFFE);
            steps += 1;
            if steps % 4096 == 0 && self.clock.over() { break; }
        }
        out.truncate(size.min(out.len() as u64) as usize);
        out
    }
    fn has(&self, name: &str) -> bool { self.dir.iter().any(|d| d.1 == 2 && d.0 == name) }
    /// The stream called `name` (the first of that name, wherever it is in the file).
    fn stream(&self, name: &str) -> Option<Vec<u8>> {
        let (_, _, start, size) = self.dir.iter().find(|d| d.1 == 2 && d.0 == name)?.clone();
        if size >= self.cutoff { return Some(self.chain(start, size)); }
        // A small stream lies in 64-byte pieces of the root's own stream, with a table of its own.
        let pieces = self.mini.len() / 64 + 1;
        let size = size.min(self.mini.len() as u64);
        let mut out = Vec::new();
        let mut taken = vec![false; pieces];
        let (mut at, mut steps) = (start, 0usize);
        while (out.len() as u64) < size && steps < pieces {
            match taken.get_mut(at as usize) { Some(t) if !*t => *t = true, _ => break }
            let from = at as usize * 64;
            let Some(s) = self.mini.get(from..(from + 64).min(self.mini.len())) else { break };
            out.extend_from_slice(s);
            at = self.mini_fat.get(at as usize).copied().unwrap_or(0xFFFF_FFFE);
            steps += 1;
        }
        out.truncate(size.min(out.len() as u64) as usize);
        Some(out)
    }
}

/// A new container holding `data` as its one stream, `name`: what the workbook reader is given, so
/// that it reads exactly the bytes that were measured and nothing else of the file it came from.
/// (4096-byte sectors; the stream is padded with zeros to a whole sector, which no reader takes for a record.)
fn ole_single(name: &str, data: &[u8]) -> Vec<u8> {
    const S: usize = 4096;
    let (end, fat_mark, free) = (0xFFFF_FFFEu32, 0xFFFF_FFFDu32, 0xFFFF_FFFFu32);
    let nd = data.len().div_ceil(S).max(1);
    // Sectors: the allocation table's (nf), the directory's one, one (empty) for the root's own
    // stream — the workbook reader wants the root to have a place — then the stream's (nd).
    let mut nf = 1;
    while nf * (S / 4) < nf + 2 + nd { nf += 1; }
    let (dir_at, root_at, data_at) = (nf, nf + 1, nf + 2);
    let mut out = vec![0u8; S * (1 + data_at + nd)];
    let put16 = |o: &mut [u8], at: usize, v: u16| o[at..at + 2].copy_from_slice(&v.to_le_bytes());
    let put32 = |o: &mut [u8], at: usize, v: u32| o[at..at + 4].copy_from_slice(&v.to_le_bytes());
    out[..8].copy_from_slice(&CFB_MAGIC);
    put16(&mut out, 24, 0x3E); put16(&mut out, 26, 4); put16(&mut out, 28, 0xFFFE); put16(&mut out, 30, 12); put16(&mut out, 32, 6);
    put32(&mut out, 40, 1); put32(&mut out, 44, nf as u32); put32(&mut out, 48, dir_at as u32); put32(&mut out, 56, 4096);
    put32(&mut out, 60, end); put32(&mut out, 68, end);
    for i in 0..109 { put32(&mut out, 76 + i * 4, if i < nf { i as u32 } else { free }); }
    for i in 0..nf * (S / 4) {
        let v = if i < nf { fat_mark } else if i == dir_at || i == root_at || i == data_at + nd - 1 { end } else if i < data_at + nd { i as u32 + 1 } else { free };
        put32(&mut out, S + i * 4, v);
    }
    let dir = S * (1 + dir_at);
    let mut entry = |n: usize, name: &str, kind: u8, child: u32, start: u32, size: u64| {
        let at = dir + n * 128;
        let units: Vec<u16> = name.encode_utf16().take(31).collect();
        for (i, u) in units.iter().enumerate() { put16(&mut out, at + i * 2, *u); }
        put16(&mut out, at + 64, if kind == 0 { 0 } else { (units.len() as u16 + 1) * 2 });
        out[at + 66] = kind;
        out[at + 67] = 1;
        put32(&mut out, at + 68, free); put32(&mut out, at + 72, free); put32(&mut out, at + 76, child);
        put32(&mut out, at + 116, start);
        out[at + 120..at + 128].copy_from_slice(&size.to_le_bytes());
    };
    entry(0, "Root Entry", 5, 1, root_at as u32, 0);
    entry(1, name, 2, free, data_at as u32, (nd * S) as u64);
    for n in 2..S / 128 { entry(n, "", 0, free, 0, 0); }
    let at = S * (1 + data_at);
    out[at..at + data.len()].copy_from_slice(data);
    out
}

fn le16(b: &[u8], at: usize) -> Option<u16> { b.get(at..at + 2).map(|x| u16::from_le_bytes([x[0], x[1]])) }
fn le32(b: &[u8], at: usize) -> Option<u32> { b.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])) }

/// Word 97–2003 (.doc): the main text, found through the file's own table of text pieces.
/// Headers, footnotes, text boxes and all formatting are left out.
fn doc(bytes: &[u8], out: &mut Out, clock: &Clock) -> Res<()> {
    let not_word = || "the file could not be read as an old Word document".to_string();
    let file = Ole::open(bytes, clock).ok_or_else(not_word)?;
    let main = file.stream("WordDocument").ok_or_else(not_word)?;
    if le16(&main, 0) != Some(0xA5EC) { return Err(not_word()); }
    let flags = le16(&main, 0x0A).ok_or_else(not_word)?;
    if flags & 0x0100 != 0 { return Err("the file is protected with a password".into()); }
    let table = file.stream(if flags & 0x0200 != 0 { "1Table" } else { "0Table" }).ok_or_else(not_word)?;
    let text_len = le32(&main, 0x4C).ok_or_else(not_word)? as usize;
    let (clx_at, clx_len) = (le32(&main, 0x01A2).ok_or_else(not_word)? as usize, le32(&main, 0x01A6).ok_or_else(not_word)? as usize);
    let clx = table.get(clx_at..clx_at.checked_add(clx_len).ok_or_else(not_word)?).ok_or_else(not_word)?;
    // Skip the property parts (type 1) to the piece table (type 2).
    let mut at = 0usize;
    let pieces = loop {
        match clx.get(at) {
            Some(1) => at += 3 + le16(clx, at + 1).ok_or_else(not_word)? as usize,
            Some(2) => { let len = le32(clx, at + 1).ok_or_else(not_word)? as usize; break clx.get(at + 5..at + 5 + len).ok_or_else(not_word)?; }
            _ => return Err(not_word()),
        }
    };
    if pieces.len() < 16 { return Err(not_word()); }
    let count = (pieces.len() - 4) / 12;
    let mut text = String::new();
    for k in 0..count {
        clock.check()?;
        let (from, to) = (le32(pieces, k * 4).ok_or_else(not_word)? as usize, le32(pieces, (k + 1) * 4).ok_or_else(not_word)? as usize);
        if from >= text_len || to <= from { continue; }
        let chars = to.min(text_len) - from;
        if text.len() > OUT_MAX * 2 { break; }
        let fc = le32(pieces, (count + 1) * 4 + k * 8 + 2).ok_or_else(not_word)?;
        let start = (fc & 0x3FFF_FFFF) as usize;
        if fc & 0x4000_0000 != 0 {
            let raw = main.get(start / 2..(start / 2).saturating_add(chars)).ok_or_else(not_word)?;
            text.push_str(&encoding_rs::WINDOWS_1252.decode_without_bom_handling(raw).0);
        } else {
            let raw = main.get(start..start.saturating_add(chars.saturating_mul(2))).ok_or_else(not_word)?;
            let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            text.push_str(&String::from_utf16_lossy(&units));
        }
    }
    // Word's own marks: paragraph and cell ends, fields (their code is dropped, their result kept).
    let mut line = String::new();
    let mut field_code = 0usize;
    let mut cell_end = false;
    for c in text.chars() {
        if out.cut { break; }
        match c {
            '\u{13}' => field_code += 1,
            '\u{14}' => field_code = field_code.saturating_sub(1),
            '\u{15}' => {}
            _ if field_code > 0 => {}
            '\r' | '\u{b}' | '\u{c}' => { out.line(line.trim_end()); line.clear(); cell_end = false; }
            '\u{7}' => {
                // A cell's end; twice in a row, the row's.
                if cell_end { out.line(&format!("| {} |", line.trim_end().trim_end_matches('|').trim())); line.clear(); cell_end = false; }
                else { line.push_str(" | "); cell_end = true; }
            }
            '\u{1e}' => { line.push('-'); cell_end = false; }
            '\t' => { line.push('\t'); cell_end = false; }
            c if c.is_control() => {}
            c => { line.push(c); cell_end = false; }
        }
    }
    if !line.trim().is_empty() { out.line(line.trim_end()); }
    Ok(())
}

/// PowerPoint 97–2003 (.ppt): the text the file keeps for its outline (titles and body text, slide
/// by slide), then the text of text boxes and of the notes pages, in the order they are stored.
fn ppt(bytes: &[u8], out: &mut Out, clock: &Clock) -> Res<()> {
    let not_ppt = || "the file could not be read as an old PowerPoint deck".to_string();
    let file = Ole::open(bytes, clock).ok_or_else(not_ppt)?;
    if file.has("EncryptedSummary") { return Err("the file is protected with a password".into()); }
    let s = file.stream("PowerPoint Document").ok_or_else(not_ppt)?;
    #[derive(Clone, Copy, PartialEq)]
    enum In { Outline, MasterList, Slide, Notes, Other }
    let mut stack: Vec<(usize, In)> = vec![];
    let (mut slides, mut boxes, mut notes): (Vec<Vec<String>>, Vec<String>, Vec<String>) = (vec![], vec![], vec![]);
    let (mut at, mut steps) = (0usize, 0u32);
    while at + 8 <= s.len() {
        steps = steps.wrapping_add(1);
        if steps % 16_384 == 0 { clock.check()?; }
        while stack.last().is_some_and(|t| at >= t.0) { stack.pop(); }
        let (ver, kind, len) = (le16(&s, at).unwrap_or(0), le16(&s, at + 2).unwrap_or(0), le32(&s, at + 4).unwrap_or(0) as usize);
        let end = at.saturating_add(8).saturating_add(len).min(s.len());
        let within = stack.iter().rev().map(|t| t.1).find(|k| *k != In::Other).unwrap_or(In::Other);
        if ver & 0x000F == 0x000F {
            if stack.len() >= 256 { return Err(not_ppt()); }
            let what = match kind {
                0x0FF0 => match ver >> 4 { 0 => In::Outline, _ => In::MasterList },
                0x03EE => In::Slide,
                0x03F0 => In::Notes,
                _ => In::Other,
            };
            stack.push((end, what));
            at += 8;
            continue;
        }
        let body = &s[at + 8..end];
        let text = match kind {
            0x0FA0 => Some(String::from_utf16_lossy(&body.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>())),
            0x0FA8 => Some(body.iter().map(|b| *b as char).collect::<String>()),
            _ => None,
        };
        match (within, kind, text) {
            (In::Outline, 0x03F3, _) => if slides.len() < SLIDES_MAX { slides.push(vec![]); },
            (In::Outline, _, Some(t)) => { if slides.is_empty() { slides.push(vec![]); } if let Some(last) = slides.last_mut() { if last.len() < 10_000 { last.push(t); } } }
            (In::Slide, _, Some(t)) => if boxes.len() < 100_000 { boxes.push(t); },
            (In::Notes, _, Some(t)) => if notes.len() < 100_000 { notes.push(t); },
            _ => {}
        }
        at = end;
    }
    let tidy = |t: &str| clean(&t.replace(['\r', '\u{b}'], "\n"));
    out.line("(An old PowerPoint file: titles and body text are given slide by slide; text boxes and notes follow in the order the file stores them, which may differ from the order of the slides.)");
    for (i, lines) in slides.iter().enumerate() {
        out.blank();
        out.line(&format!("## Slide {}", i + 1));
        if lines.iter().all(|l| l.trim().is_empty()) { out.line("(no title or body text)"); }
        for l in lines { out.line(tidy(l).trim()); }
    }
    for (title, list) in [("Text boxes", &boxes), ("Notes", &notes)] {
        let list: Vec<String> = list.iter().map(|t| tidy(t).trim().to_string()).filter(|t| !t.is_empty()).collect();
        if list.is_empty() { continue; }
        out.blank();
        out.line(&format!("## {title}"));
        for l in &list { out.line(l); }
    }
    if out.body <= 1 { return Err("no text was found in the old PowerPoint deck".into()); }
    Ok(())
}

#[cfg(test)]
#[path = "as_text_tests.rs"]
mod tests;
