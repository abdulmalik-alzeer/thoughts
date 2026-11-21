use super::*;
use std::io::Write;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/office").join(name)).unwrap()
}

fn zip_of(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in parts {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn ole_of(streams: &[(&str, &[u8])]) -> Vec<u8> {
    let mut f = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    for (name, data) in streams { f.create_stream(format!("/{name}")).unwrap().write_all(data).unwrap(); }
    f.flush().unwrap();
    f.into_inner().into_inner()
}

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;

fn xlsx_of(sheets: &[(&str, String)], shared: &[&str]) -> Vec<u8> {
    let mut book = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#);
    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    for (i, (name, _)) in sheets.iter().enumerate() {
        book.push_str(&format!(r#"<sheet name="{name}" sheetId="{0}" r:id="rId{0}"/>"#, i + 1));
        rels.push_str(&format!(r#"<Relationship Id="rId{0}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{0}.xml"/>"#, i + 1));
    }
    book.push_str("</sheets></workbook>");
    rels.push_str(r#"<Relationship Id="rIdS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#);
    let sst = format!(r#"<?xml version="1.0" encoding="UTF-8"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{}</sst>"#, shared.iter().map(|s| format!("<si><t>{s}</t></si>")).collect::<String>());
    let names: Vec<String> = (1..=sheets.len()).map(|i| format!("xl/worksheets/sheet{i}.xml")).collect();
    let bodies: Vec<String> = sheets.iter().map(|(_, rows)| format!(r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{rows}</sheetData></worksheet>"#)).collect();
    let root = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let mut parts: Vec<(&str, &[u8])> = vec![("_rels/.rels", root.as_bytes()), ("xl/workbook.xml", book.as_bytes()), ("xl/_rels/workbook.xml.rels", rels.as_bytes()), ("xl/sharedStrings.xml", sst.as_bytes())];
    for (n, b) in names.iter().zip(&bodies) { parts.push((n, b.as_bytes())); }
    zip_of(&parts)
}

#[test]
fn which_files_get_a_text_version() {
    for n in ["a.xlsx", "a.XLSM", "a.xltx", "a.xls", "a.xlsb", "a.ods", "a.docx", "a.docm", "a.dotx", "a.pptx", "a.ppsx", "a.odt", "a.odp", "a.rtf", "a.doc", "a.ppt", "dir.v2/My Report.DOCX"] {
        assert!(convertible(n), "{n}");
    }
    for n in ["a.pdf", "a.txt", "a.png", "a.csv", "xlsx", ".xlsx", "a.numbers", "a.zip", "a"] { assert!(!convertible(n), "{n}"); }
    assert!(opaque("Budget.numbers") && opaque("a.pages") && opaque("a.key") && !opaque("a.xlsx") && !opaque("a.txt"));
    assert_eq!((what("a.xls"), what("a.odp"), what("a.rtf")), ("a spreadsheet", "a slide deck", "a document"));
}

#[test]
fn real_word_rtf_and_opendocument_files_come_out_in_reading_order() {
    // Written by macOS's own converter from one page: heading, text, Arabic, a list, a table.
    for f in ["sample.docx", "sample.doc", "sample.rtf", "sample.odt"] {
        let t = convert(f, &fixture(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        let at = |needle: &str| t.find(needle).unwrap_or_else(|| panic!("{f}: no {needle:?} in\n{t}"));
        let order = [at("Quarterly plan"), at("The budget for the launch is 4200 riyals."), at("مرحبا بالعالم"), at("Steps"), at("First step"), at("Second step"), at("Item"), at("Cost"), at("Paper"), at("12"), at("Closing line & thanks.")];
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{f}: out of order\n{t}");
        assert!(t.starts_with(&format!("Text version of \"{f}\"")), "{t}");
        assert!(!t.contains("[truncated") && !t.chars().any(|c| c.is_control() && c != '\n' && c != '\t'), "{f}:\n{t:?}");
        // Nothing of the container: no font names, style names or field codes.
        for junk in ["Times", "fonttbl", "HYPERLINK", "rsid", "<w:", "PK"] { assert!(!t.contains(junk), "{f}: {junk} in\n{t}"); }
    }
    // Where the format keeps the table as one, it stays one.
    for f in ["sample.doc", "sample.rtf", "sample.odt"] {
        let t = convert(f, &fixture(f)).unwrap();
        assert!(t.contains("| Item | Cost |") && t.contains("| Paper | 12 |"), "{f}:\n{t}");
    }
    assert!(convert("sample.odt", &fixture("sample.odt")).unwrap().contains("- First step\n- Second step"));
}

#[test]
fn a_word_document_keeps_headings_lists_and_tables() {
    let doc = format!(r#"<?xml version="1.0"?><w:document {W}><w:body>
      <w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Plan</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="Heading2"/><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr><w:r><w:t>Costs &amp; </w:t></w:r><w:r><w:t>dates</w:t></w:r></w:p>
      <w:p><w:r><w:t xml:space="preserve">One </w:t><w:tab/><w:t>two</w:t><w:br/><w:t>three &#x645;</w:t></w:r><w:del><w:r><w:delText>gone</w:delText></w:r></w:del></w:p>
      <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>first</w:t></w:r></w:p>
      <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>inner</w:t></w:r></w:p>
      <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>A|B</w:t></w:r></w:p></w:tc></w:tr>
        <w:tr><w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p><w:p><w:r><w:t>y</w:t></w:r></w:p></w:tc>
          <w:tc><w:tbl><w:tr><w:tc><w:p><w:r><w:t>deep</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/></w:tc></w:tr></w:tbl>
      <w:p><w:r><mc:AlternateContent><mc:Choice><w:t>box</w:t></mc:Choice><mc:Fallback><w:t>box again</w:t></mc:Fallback></mc:AlternateContent></w:r></w:p>
      <w:p><w:r><w:instrText>HYPERLINK "x"</w:instrText></w:r><w:r><w:t>after</w:t></w:r></w:p>
    </w:body></w:document>"#);
    let notes = format!(r#"<w:footnotes {W}><w:footnote><w:p><w:r><w:t>See page 4.</w:t></w:r></w:p></w:footnote></w:footnotes>"#);
    let t = convert("plan.docx", &zip_of(&[("word/document.xml", doc.as_bytes()), ("word/footnotes.xml", notes.as_bytes())])).unwrap();
    for want in ["\n# Plan\n", "\n## Costs & dates\n", "One \ttwo\nthree م\n", "\n- first\n  - inner\n", "| Name | A\\|B |\n| --- | --- |\n| x<br>y | \\| deep \\| |\n", "\nbox\n", "\nafter\n", "## Footnotes\n\nSee page 4.\n"] {
        assert!(t.contains(want), "no {want:?} in\n{t}");
    }
    for not in ["gone", "box again", "HYPERLINK"] { assert!(!t.contains(not), "{not} in\n{t}"); }
    // A document whose main part has another name is still found; one without any is refused.
    assert!(convert("a.docx", &zip_of(&[("word/document2.xml", doc.as_bytes())])).unwrap().contains("# Plan"));
    assert!(convert("a.docx", &zip_of(&[("other.xml", b"<a/>")])).is_err());
}

#[test]
fn a_workbook_gives_every_sheet_as_csv_with_row_numbers() {
    let rows = r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="D1" t="inlineStr"><is><t>in, "line"</t></is></c></row>
        <row r="3"><c r="A3"><v>1200</v></c><c r="B3"><v>2.5</v></c><c r="C3" t="b"><v>1</v></c></row>"#;
    let t = convert("b.xlsx", &xlsx_of(&[("Costs", rows.to_string()), ("Empty one", String::new()), ("أرقام", r#"<row r="2"><c r="B2" t="s"><v>2</v></c></row>"#.to_string())], &["Item", "Cost", "مرحبا"])).unwrap();
    assert!(t.contains("## Sheet 1: Costs\nrow,A,B,C,D\n1,Item,Cost,,\"in, \"\"line\"\"\"\n3,1200,2.5,TRUE\n"), "{t}");
    assert!(t.contains("## Sheet 2: Empty one\n(empty)\n"), "{t}");
    assert!(t.contains("## Sheet 3: أرقام\nrow,A,B\n2,,مرحبا\n"), "{t}");
    assert!(!t.contains("[truncated"));
    // The same reader reads macro-enabled workbooks and templates: nothing in them is run.
    assert!(convert("b.xlsm", &xlsx_of(&[("S", r#"<row r="1"><c r="A1"><v>7</v></c></row>"#.to_string())], &[])).unwrap().contains("row,A\n1,7\n"));
}

#[test]
fn a_long_or_wide_sheet_says_what_it_leaves_out() {
    let mut rows = String::new();
    for r in 1..=ROWS_MAX + 25 { rows.push_str(&format!(r#"<row r="{r}"><c r="A{r}"><v>{r}</v></c></row>"#)); }
    rows.push_str(&format!(r#"<row r="{0}"><c r="KN{0}"><v>9</v></c></row>"#, ROWS_MAX + 26));
    let t = convert("big.xlsx", &xlsx_of(&[("Long", rows), ("Next", r#"<row r="1"><c r="A1"><v>5</v></c></row>"#.to_string())], &[])).unwrap();
    assert!(t.contains(&format!("\n{0},{0}\n[truncated: 25 more rows of this sheet are not shown]\n", ROWS_MAX)), "{}", &t[t.len().saturating_sub(400)..]);
    assert!(t.contains("[truncated: 44 more columns of this sheet are not shown (it reaches column KN)]"), "{}", &t[t.len().saturating_sub(400)..]);
    assert!(t.contains("## Sheet 2: Next\nrow,A\n1,5\n"), "the sheet after a long one is lost");
    // A very long cell is cut with the count of what is missing; a line break in a cell stays on its row.
    let long = "x".repeat(CELL_MAX + 40);
    let t = convert("c.xlsx", &xlsx_of(&[("S", r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>"#.to_string())], &[&long, "two\nlines"])).unwrap();
    assert!(t.contains("… [truncated: 40 more characters in this cell],two \u{23ce} lines\n"), "{t}");
    assert_eq!((col_name(0), col_name(25), col_name(26), col_name(701), col_name(702), col_name(16383)), ("A".into(), "Z".into(), "AA".into(), "ZZ".into(), "AAA".into(), "XFD".into()));
}

#[test]
fn an_opendocument_sheet_with_a_million_repeated_rows_is_counted_not_unrolled() {
    let content = r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet>
      <table:table table:name="Stock">
        <table:table-row><table:table-cell office:value-type="string"><text:p>Item</text:p></table:table-cell><table:table-cell table:number-columns-repeated="2"/><table:table-cell office:value-type="float" office:value="3"><text:p>3.00</text:p><office:annotation><text:p>a note</text:p></office:annotation></table:table-cell><table:table-cell table:number-columns-repeated="16000"/></table:table-row>
        <table:table-row table:number-rows-repeated="1000000"><table:table-cell table:number-columns-repeated="16384"/></table:table-row>
        <table:table-row><table:table-cell office:value-type="date" office:date-value="2026-01-05"/><table:table-cell><text:p>a<text:s text:c="2"/>b</text:p><text:p>c</text:p></table:table-cell></table:table-row>
        <table:table-row table:number-rows-repeated="1048576"><table:table-cell table:number-columns-repeated="2"><text:p>same</text:p></table:table-cell></table:table-row>
      </table:table>
      <table:table table:name="Second"><table:table-row><table:table-cell><text:p>z</text:p></table:table-cell></table:table-row></table:table>
    </office:spreadsheet></office:body></office:document-content>"#;
    let started = Instant::now();
    let t = convert("s.ods", &zip_of(&[("content.xml", content.as_bytes())])).unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(t.contains("## Sheet 1: Stock\nrow,A,B,C,D\n1,Item,,,3.00\n1000002,2026-01-05,a  b \u{23ce} c\n1000003,same,same\n"), "{}", &t[..600.min(t.len())]);
    assert!(!t.contains("a note"));
    assert!(t.contains(&format!("[truncated: {} more rows of this sheet are not shown]", 1_048_576 + 2 - ROWS_MAX)), "{}", &t[t.len() - 300..]);
    assert!(t.contains("## Sheet 2: Second\nrow,A\n1,z\n"));
    assert!(t.len() < OUT_MAX);
}

fn slide(texts: &[&str]) -> String {
    format!(r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>{}</p:spTree></p:cSld></p:sld>"#, texts.iter().map(|t| format!("<p:sp><p:txBody><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:txBody></p:sp>")).collect::<String>())
}

#[test]
fn a_deck_gives_its_slides_in_showing_order_with_notes() {
    let pres = r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="256" r:id="rId7"/><p:sldId id="257" r:id="rId3"/></p:sldIdLst></p:presentation>"#;
    let pres_rels = r#"<Relationships><Relationship Id="rId3" Type="http://x/slide" Target="slides/slide1.xml"/><Relationship Id="rId7" Type="http://x/slide" Target="slides/slide2.xml"/><Relationship Id="rId9" Type="http://x/slide" Target="http://elsewhere/" TargetMode="External"/></Relationships>"#;
    let s2_rels = r#"<Relationships><Relationship Id="rId1" Type="http://x/relationships/notesSlide" Target="../notesSlides/notesSlide9.xml"/></Relationships>"#;
    let table = r#"<p:sld xmlns:p="p" xmlns:a="a"><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>Q1</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>40</a:t></a:r><a:br/><a:r><a:t>units</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></p:sld>"#;
    let notes = r#"<p:notes xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>Say hello first.</a:t></a:r></a:p><a:p><a:fld type="slidenum"><a:t>2</a:t></a:fld></a:p></p:notes>"#;
    let deck = zip_of(&[("ppt/presentation.xml", pres.as_bytes()), ("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
        ("ppt/slides/slide1.xml", table.as_bytes()), ("ppt/slides/slide2.xml", slide(&["Opening", "Why now"]).as_bytes()),
        ("ppt/slides/_rels/slide2.xml.rels", s2_rels.as_bytes()), ("ppt/notesSlides/notesSlide9.xml", notes.as_bytes())]);
    let t = convert("d.pptx", &deck).unwrap();
    assert!(t.contains("## Slide 1\nOpening\nWhy now\n\nNotes:\nSay hello first.\n\n## Slide 2\n| Q1 | 40<br>units |\n"), "{t}");
    // Without a readable list of slides: by their numbers. An empty slide says so.
    let t = convert("d.ppsx", &zip_of(&[("ppt/slides/slide10.xml", slide(&["ten"]).as_bytes()), ("ppt/slides/slide2.xml", slide(&[]).as_bytes())])).unwrap();
    assert!(t.contains("## Slide 1\n(no text on this slide)\n\n## Slide 2\nten\n"), "{t}");
    assert!(convert("d.pptx", &zip_of(&[("x", b"y")])).is_err());
    assert_eq!(zip_target("ppt/slides/slide2.xml", "../notesSlides/n.xml"), "ppt/notesSlides/n.xml");
    assert_eq!(zip_target("ppt/slides/slide2.xml", "../../../../etc/passwd"), "etc/passwd", "only ever a name inside the zip");
    assert_eq!(zip_target("ppt/presentation.xml", "/ppt/slides/s.xml"), "ppt/slides/s.xml");
}

#[test]
fn an_opendocument_deck_gives_its_pages_and_notes() {
    let content = r#"<office:document-content xmlns:office="o" xmlns:draw="d" xmlns:text="t" xmlns:presentation="p"><office:body><office:presentation>
      <draw:page draw:name="Intro"><draw:frame><draw:text-box><text:p>Welcome</text:p><text:list><text:list-item><text:p>point one</text:p></text:list-item></text:list></draw:text-box></draw:frame>
        <presentation:notes><draw:frame><draw:text-box><text:p>Smile.</text:p></draw:text-box></draw:frame></presentation:notes></draw:page>
      <draw:page><draw:frame><draw:text-box><text:p>End</text:p></draw:text-box></draw:frame></draw:page>
    </office:presentation></office:body></office:document-content>"#;
    let t = convert("d.odp", &zip_of(&[("content.xml", content.as_bytes())])).unwrap();
    let at = |n: &str| t.find(n).unwrap_or_else(|| panic!("no {n:?} in\n{t}"));
    let order = [at("## Slide 1: Intro"), at("Welcome"), at("- point one"), at("Notes:"), at("Smile."), at("## Slide 2\n"), at("End")];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{t}");
}

fn rec(id: u16, body: &[u8]) -> Vec<u8> { let mut v = id.to_le_bytes().to_vec(); v.extend((body.len() as u16).to_le_bytes()); v.extend(body); v }

/// A small Excel 97 workbook, record by record: one sheet with the given (row, column, number) cells
/// and one text cell at A1.
fn xls_of(cells: &[(u16, u16, f64)]) -> Vec<u8> {
    let bof = |kind: u16| rec(0x0809, &[&0x0600u16.to_le_bytes()[..], &kind.to_le_bytes(), &[0xBB, 0x0D, 0xCC, 0x07, 0, 0, 0, 0, 6, 0, 0, 0]].concat());
    let mut globals = bof(0x0005);
    globals.extend(rec(0x0042, &1200u16.to_le_bytes()));
    let name = b"Old";
    let sheet_at = globals.len() + 4 + 8 + name.len() + 4;
    globals.extend(rec(0x0085, &[&(sheet_at as u32).to_le_bytes()[..], &[0, 0, name.len() as u8, 0], name].concat()));
    globals.extend(rec(0x000A, &[]));
    assert_eq!(globals.len(), sheet_at);
    let mut sheet = bof(0x0010);
    sheet.extend(rec(0x0204, &[&0u16.to_le_bytes()[..], &0u16.to_le_bytes(), &0u16.to_le_bytes(), &5u16.to_le_bytes(), &[0], b"Label"].concat()));
    for (r, c, v) in cells { sheet.extend(rec(0x0203, &[&r.to_le_bytes()[..], &c.to_le_bytes(), &0u16.to_le_bytes(), &v.to_le_bytes()].concat())); }
    sheet.extend(rec(0x000A, &[]));
    globals.extend(sheet);
    ole_of(&[("Workbook", &globals)])
}

#[test]
fn an_old_excel_workbook_is_read_and_one_spanning_too_much_is_refused() {
    let t = convert("old.xls", &xls_of(&[(0, 1, 42.0), (2, 0, 1.5)])).unwrap();
    assert!(t.contains("## Sheet 1: Old\nrow,A,B\n1,Label,42\n3,1.5\n"), "{t}");
    // Two cells at opposite corners: the reader would hold 16 million cells for them.
    let e = convert("wide.xls", &xls_of(&[(0, 0, 1.0), (65_535, 255, 2.0)])).unwrap_err();
    assert!(e.contains("too large"), "{e}");
    assert!(convert("not.xls", b"just some text, not a workbook").is_err());
    assert!(convert("empty.xls", &ole_of(&[("Other", b"x")])).is_err());
}

#[test]
fn an_old_powerpoint_deck_gives_its_text() {
    let atom = |kind: u16, body: &[u8]| { let mut v = vec![0, 0]; v.extend(kind.to_le_bytes()); v.extend((body.len() as u32).to_le_bytes()); v.extend(body); v };
    let container = |kind: u16, instance: u16, body: &[u8]| { let mut v = ((instance << 4) | 0xF).to_le_bytes().to_vec(); v.extend(kind.to_le_bytes()); v.extend((body.len() as u32).to_le_bytes()); v.extend(body); v };
    let utf16 = |s: &str| s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>();
    let outline = container(0x0FF0, 0, &[atom(0x03F3, &[0; 20]), atom(0x0FA0, &utf16("Title one")), atom(0x0FA8, b"Body\rsecond line"), atom(0x03F3, &[0; 20]), atom(0x0FA0, &utf16("عنوان"))].concat());
    let master = container(0x0FF0, 1, &atom(0x0FA8, b"Click to edit Master title style"));
    let a_slide = container(0x03EE, 0, &container(0xF00D, 0, &atom(0x0FA8, b"A text box")));
    let a_note = container(0x03F0, 0, &container(0xF00D, 0, &atom(0x0FA8, b"Remember the demo")));
    let doc = container(0x03E8, 0, &[master, outline].concat());
    let t = convert("old.ppt", &ole_of(&[("PowerPoint Document", &[doc, a_slide, a_note].concat())])).unwrap();
    assert!(t.contains("## Slide 1\nTitle one\nBody\nsecond line\n\n## Slide 2\nعنوان\n\n## Text boxes\nA text box\n\n## Notes\nRemember the demo\n"), "{t}");
    assert!(!t.contains("Master title"));
    assert!(convert("none.ppt", &ole_of(&[("PowerPoint Document", &atom(0x0FA8, b""))])).is_err());
    assert!(convert("locked.ppt", &ole_of(&[("PowerPoint Document", b"x"), ("EncryptedSummary", b"y")])).unwrap_err().contains("password"));
}

#[test]
fn rich_text_in_other_alphabets_and_with_hidden_parts() {
    let src = b"{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fcharset0 Arial;}{\\f1\\fcharset178 Tahoma;}}{\\info{\\author Someone}}{\\*\\generator Word;}\
\\pard Caf\\'e9 \\u8364?5 \\{ok\\}\\par {\\f1 \\'e3\\'d1\\'cd\\'c8\\'c7}\\line \\u-10179\\'3f\\u-8704\\'3f\\par {\\pict\\bin5 }}}}{\\header Top}\
a\\cell b\\cell\\row\\par}";
    let t = convert("n.rtf", src).unwrap();
    assert!(t.contains("Café €5 {ok}\nمرحبا\n😀\n| a | b |\n"), "{t:?}");
    for not in ["Arial", "Someone", "Word", "Top"] { assert!(!t.contains(not), "{not} in {t}"); }
    assert!(convert("n.rtf", b"plain words").is_err());
    // Thousands of nested groups: refused, not a crash.
    let deep = [b"{\\rtf1 ".to_vec(), vec![b'{'; 100_000], b"x".to_vec()].concat();
    assert!(convert("deep.rtf", &deep).is_err());
}

#[test]
fn hostile_and_broken_files_are_refused_with_a_reason_never_a_crash() {
    // Not what the name says, empty, cut short, garbage.
    for (name, bytes) in [("a.xlsx", &b"hello"[..]), ("a.docx", b""), ("a.pptx", b"PK\x03\x04 nothing more"), ("a.ods", b"PK"), ("a.doc", b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1"), ("a.xlsb", b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0"), ("a.txt", b"x")] {
        let r = convert(name, bytes);
        assert!(r.as_ref().is_err_and(|e| !e.is_empty()), "{name}: {r:?}");
    }
    let whole = fixture("sample.docx");
    for cut in [10, whole.len() / 2, whole.len() - 5] { assert!(convert("cut.docx", &whole[..cut]).is_err(), "cut at {cut}"); }
    let whole = fixture("sample.doc");
    for cut in [600, 4000, whole.len() - 700] { let _ = convert("cut.doc", &whole[..cut]); }
    // Every byte of the old Word file damaged in turn: an answer either way, never a panic or a hang.
    for i in (0..whole.len()).step_by(37) { let mut b = whole.clone(); b[i] ^= 0xFF; let _ = convert("flip.doc", &b); }
    let rtf = fixture("sample.rtf");
    for i in (0..rtf.len()).step_by(11) { let mut b = rtf.clone(); b[i] = b'{'; let _ = convert("flip.rtf", &b); }
    // A password-protected Office file is an OLE container under its usual name.
    assert!(convert("locked.docx", &ole_of(&[("EncryptedPackage", b"x")])).unwrap_err().contains("password"));
    // Broken XML inside a good zip; an entity the file defines itself is not expanded.
    assert!(convert("a.docx", &zip_of(&[("word/document.xml", b"<w:document><w:body><w:p><w:r><w:t>half")])).map_or(true, |t| !t.contains("[truncated")));
    let lol = br#"<?xml version="1.0"?><!DOCTYPE d [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;">]><w:document xmlns:w="w"><w:body><w:p><w:r><w:t>x&b;y</w:t></w:r></w:p></w:body></w:document>"#;
    let t = convert("lol.docx", &zip_of(&[("word/document.xml", lol)]));
    assert!(t.map_or(true, |t| !t.contains("aaaa")));
    // A zip that unpacks to more than is allowed is refused while unpacking, whatever it declares.
    let big = zip_of(&[("a", &vec![0u8; 300_000]), ("b", &vec![0u8; 300_000])]);
    assert!(big.len() < 10_000);
    let clock = Clock(Instant::now() + TIME_MAX);
    assert!(unzip_count_to(&big, &clock, 600_000).is_ok());
    assert!(unzip_count_to(&big, &clock, 599_999).unwrap_err().contains("unpacks to more"));
    let mut pack = Pack::open(&big).unwrap();
    pack.left = 400_000;
    assert!(pack.read("a").unwrap().is_some() && pack.read("b").is_err() && pack.read("missing").unwrap().is_none());
    // Out of time: stops with a reason (or a marked cut), at once.
    let started = Instant::now();
    let mut rows = String::new();
    for r in 1..=30_000 { rows.push_str(&format!(r#"<row r="{r}"><c r="A{r}"><v>{r}</v></c></row>"#)); }
    match convert_by("slow.xlsx", &xlsx_of(&[("S", rows)], &[]), Instant::now()) {
        Ok(t) => assert!(t.contains("[truncated: reading stopped after the time allowed"), "{}", &t[t.len() - 200..]),
        Err(e) => assert!(e.contains("too long"), "{e}"),
    }
    assert!(convert_by("slow.docx", &fixture("sample.docx"), Instant::now()).is_ok(), "a small file is done before the clock is looked at");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn long_text_is_wrapped_and_a_huge_document_is_cut_with_a_line_saying_so() {
    let para = "word ".repeat(1000);
    let one = format!("<w:p><w:r><w:t>{para}</w:t></w:r></w:p>");
    let doc = format!("<w:document {W}><w:body>{}</w:body></w:document>", one.repeat(OUT_MAX / para.len() + 50));
    let t = convert("long.docx", &zip_of(&[("word/document.xml", doc.as_bytes())])).unwrap();
    assert!(t.lines().all(|l| l.len() <= LINE_MAX), "a line the Read tool would cut");
    assert!(t.len() <= OUT_MAX + 200 && t.trim_end().ends_with("the file holds more]") && t.contains("\n[truncated: this text version stops here"), "{}", &t[t.len() - 200..]);
    assert_eq!(wrap(&"é".repeat(LINE_MAX)).concat(), "é".repeat(LINE_MAX));
    // Nothing to read is said, not left blank.
    let empty = convert("e.docx", &zip_of(&[("word/document.xml", format!("<w:document {W}><w:body><w:p/></w:body></w:document>").as_bytes())])).unwrap();
    assert!(empty.contains("(No text was found in the file.)"), "{empty}");
}
