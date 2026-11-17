import { beforeAll, describe, expect, it } from 'vitest';
import { zip, type ZipEntry } from '../../content/zip';
import { decodeCsv, guessSeparator, parseDelimited } from './csv';
import { decodeXml, scanXml } from './xmlscan';
import { CAPS, OfficeError, OfficeZip, resolvePart } from './safezip';
import { cellRef, colIndex, colName, formatNumber, isDateFormat, openBook, readCellFormats, readCsv, readOdsContent, readSharedStrings, readWorksheet, serialDate } from './sheet';
import { readOdp, readPptx } from './slides';
import { odtToDom } from './odf';
import { parseRtf, rtfToDom } from './rtf';
import { docxToDom } from '../../content/docxview';

// jsdom's Blob has no arrayBuffer(); its FileReader reads one.
const read = (b: Blob) => new Promise<ArrayBuffer>((ok, no) => { const r = new FileReader(); r.onload = () => ok(r.result as ArrayBuffer); r.onerror = no; r.readAsArrayBuffer(b); });
const bytesOf = async (entries: ZipEntry[]) => new Uint8Array(await read(await zip(entries)));
const enc = (s: string) => new TextEncoder().encode(s);
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0]);

let made: string[] = [];
beforeAll(() => {
  // jsdom has no object URLs; the parsers only need a unique string back.
  (URL as any).createObjectURL = (b: Blob) => { const u = `blob:test/${made.length}-${b.type}`; made.push(u); return u; };
  (URL as any).revokeObjectURL = () => {};
});

describe('xmlscan', () => {
  it('decodes the built-in entities and character references only', () => {
    expect(decodeXml('a &amp; b &lt;c&gt; &#1575;&#x644; &quot;&apos; &custom;')).toBe('a & b <c> ال "\' &custom;');
  });
  it('reads tags, attributes with > inside, CDATA, and never expands a declared entity', () => {
    const xml = '<?xml version="1.0"?><!DOCTYPE r [<!ENTITY x SYSTEM "file:///etc/passwd"><!ENTITY y "boom">]><r a="1 > 0" b=\'q\'><t>&x;&y;</t><![CDATA[<raw>]]><e/></r>';
    const ev: string[] = [];
    scanXml(xml, { open: (n, a, self) => { ev.push(`<${n}${a.a ? ` a=${a.a}` : ''}${self ? '/' : ''}>`); }, close: (n) => { ev.push(`</${n}>`); }, text: (t) => { ev.push(t); } });
    expect(ev).toEqual(['<r a=1 > 0>', '<t>', '&x;&y;', '</t>', '<raw>', '<e/>', '</e>', '</r>']);
  });
});

describe('csv', () => {
  it('splits quoted fields with separators, doubled quotes and line breaks inside', () => {
    const p = parseDelimited('name,note\r\n"Smith, J","said ""hi""\nthen left"\r\nأحمد,نعم\n', ',');
    expect(p.rows).toEqual([['name', 'note'], ['Smith, J', 'said "hi"\nthen left'], ['أحمد', 'نعم']]);
    expect(p.totalRows).toBe(3);
  });
  it('keeps at most the cap of rows and columns but counts all of them', () => {
    const text = Array.from({ length: 50 }, (_, i) => `${i},a,b,c,d`).join('\n');
    const p = parseDelimited(text, ',', 10, 3);
    expect(p.rows.length).toBe(10);
    expect(p.rows[9]).toEqual(['9', 'a', 'b']);
    expect(p.totalRows).toBe(50);
    expect(p.totalCols).toBe(5);
    expect(p.cols).toBe(3);
  });
  it('guesses the separator', () => {
    expect(guessSeparator('a;b;c\n1;2;3\n4;5,5;6')).toBe(';');
    expect(guessSeparator('a\tb\n1\t2')).toBe('\t');
    expect(guessSeparator('"x,y";z\n1;2', 'f.csv')).toBe(';');
    expect(guessSeparator('a,b', 'data.tsv')).toBe('\t');
    expect(guessSeparator('a,b,c\n1,2,3')).toBe(',');
  });
  it('works out the encoding: BOM, UTF-16, UTF-8, Arabic Windows code page', () => {
    expect(decodeCsv(new Uint8Array([0xef, 0xbb, 0xbf, ...enc('a,ب')]))).toBe('a,ب');
    const u16 = new Uint8Array([0xff, 0xfe, 0x61, 0, 0x2c, 0, 0x28, 0x06]);
    expect(decodeCsv(u16)).toBe('a,ب');
    expect(decodeCsv(enc('café,1'))).toBe('café,1');
    // "مرحبا,1" in windows-1256
    expect(decodeCsv(new Uint8Array([0xe3, 0xd1, 0xcd, 0xc8, 0xc7, 0x2c, 0x31]))).toBe('مرحبا,1');
    // "café" in windows-1252
    expect(decodeCsv(new Uint8Array([0x63, 0x61, 0x66, 0xe9]))).toBe('café');
  });
  it('becomes a sheet whose numbers line up and whose first line is the header', () => {
    const s = readCsv(enc('item,price\nTea,"1,250.50"\nCoffee,12%\n'), 'x.csv');
    expect(s.header).toBe(true);
    expect(s.rows[1]).toEqual([{ v: 'Tea' }, { v: '1,250.50', num: true }]);
    expect(s.rows[2][1]).toEqual({ v: '12%', num: true });
  });
});

describe('cell references and number formats', () => {
  it('converts column letters and references', () => {
    expect(colIndex('A')).toBe(0); expect(colIndex('Z')).toBe(25); expect(colIndex('AA')).toBe(26); expect(colIndex('XFD')).toBe(16383);
    expect(colName(0)).toBe('A'); expect(colName(27)).toBe('AB'); expect(colName(16383)).toBe('XFD');
    expect(cellRef('$C$12')).toEqual({ r: 11, c: 2 });
    expect(cellRef('nope')).toBeNull();
  });
  it('shows numbers the way their format says', () => {
    expect(formatNumber(0.1 + 0.2, 'General')).toBe('0.3');
    expect(formatNumber(3.14159, '0.00')).toBe('3.14');
    expect(formatNumber(1234567, '#,##0')).toBe('1,234,567');
    expect(formatNumber(0.256, '0%')).toBe('26%');
    expect(formatNumber(0.256, '0.00%')).toBe('25.60%');
    expect(formatNumber(1500, '"SAR "#,##0.00')).toBe('SAR 1,500.00');
    expect(formatNumber(-5, '#,##0.00;(#,##0.00)')).toBe('(5.00)');
    expect(formatNumber(-5, '#,##0.00')).toBe('-5.00');
    expect(formatNumber(12345, '0.00E+00')).toBe('1.23E+04');
    expect(formatNumber(1, '[$€-407] #,##0.00')).toBe('€ 1.00');
    expect(formatNumber(2500000, '#,##0,')).toBe('2,500');
  });
  it('shows dates and times from serial numbers', () => {
    expect(isDateFormat('yyyy-mm-dd')).toBe(true);
    expect(isDateFormat('#,##0.00')).toBe(false);
    expect(isDateFormat('"m"0')).toBe(false);
    expect(formatNumber(45356, 'm/d/yyyy')).toBe('2024-03-05');
    expect(formatNumber(45356.5, 'yyyy-mm-dd h:mm')).toBe('2024-03-05 12:00');
    expect(formatNumber(0.75, 'h:mm:ss')).toBe('18:00:00');
    expect(serialDate(1, 'yyyy-mm-dd')).toBe('1900-01-01');
    expect(serialDate(0, 'yyyy-mm-dd', true)).toBe('1904-01-01');
    expect(serialDate(1.5, '[h]:mm:ss')).toBe('36:00:00');
  });
});

describe('xlsx', () => {
  it('reads shared strings with rich runs, leaving out phonetic guides', () => {
    expect(readSharedStrings('<sst><si><t>Plain</t></si><si><r><t>Bo</t></r><r><rPr><b/></rPr><t xml:space="preserve">ld &amp; more</t></r><rPh><t>X</t></rPh></si><si><t/></si></sst>'))
      .toEqual(['Plain', 'Bold & more', '']);
  });
  it('maps cell styles to number formats, custom and built-in', () => {
    expect(readCellFormats('<styleSheet><numFmts><numFmt numFmtId="164" formatCode="0.0%"/></numFmts><cellStyleXfs><xf numFmtId="3"/></cellStyleXfs><cellXfs><xf numFmtId="0"/><xf numFmtId="164"/><xf numFmtId="14"/></cellXfs></styleSheet>'))
      .toEqual(['General', '0.0%', 'yyyy-mm-dd']);
  });
  it('reads a worksheet: strings, numbers, formulas by their saved value, booleans, merges, direction', () => {
    const xml = `<worksheet><dimension ref="A1:D3"/><sheetViews><sheetView rightToLeft="1"/></sheetViews><sheetData>
      <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
      <row r="2"><c r="A2"><v>42</v></c><c r="B2" s="1"><f>A2/100</f><v>0.42</v></c><c r="C2" t="b"><v>1</v></c><c r="D2" t="inlineStr"><is><t>inline</t></is></c></row>
      <row r="3"><c r="A3" t="str"><f>"x"&amp;"y"</f><v>xy</v></c><c r="C3" t="e"><v>#DIV/0!</v></c></row>
      </sheetData><mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells></worksheet>`;
    const s = readWorksheet(xml, 'S', { strings: ['اسم', 'Total'], formats: ['General', '0%'], date1904: false });
    expect(s.rtl).toBe(true);
    expect(s.cols).toBe(4);
    expect(s.rows[0].map((c) => c?.v ?? null)).toEqual(['اسم', 'Total', null, null]);
    expect(s.rows[1].map((c) => c?.v)).toEqual(['42', '42%', 'TRUE', 'inline']);
    expect(s.rows[1][0]?.num).toBe(true);
    expect(s.rows[2].map((c) => c?.v ?? null)).toEqual(['xy', null, '#DIV/0!', null]);
    expect(s.merges).toEqual([{ r: 0, c: 0, rs: 1, cs: 2 }]);
  });
  it('cuts a huge sheet to the caps and still says how big it is', () => {
    const rows = Array.from({ length: 30 }, (_, i) => `<row r="${i + 1}">${Array.from({ length: 8 }, (_, c) => `<c r="${colName(c)}${i + 1}"><v>${i * c}</v></c>`).join('')}</row>`).join('');
    const s = readWorksheet(`<worksheet><dimension ref="A1:H30"/><sheetData>${rows}</sheetData></worksheet>`, 'Big', { strings: [], formats: [], date1904: false }, 10, 5);
    expect(s.rows.length).toBe(10);
    expect(s.cols).toBe(5);
    expect(s.totalRows).toBe(30);
    expect(s.totalCols).toBe(8);
  });
  it('opens a whole workbook: sheet names in order, hidden sheets left out', async () => {
    const b = await bytesOf([
      { name: '[Content_Types].xml', data: '<Types/>' },
      { name: 'xl/workbook.xml', data: '<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Summary" sheetId="1" r:id="rId1"/><sheet name="Secret" sheetId="2" state="hidden" r:id="rId2"/><sheet name="البيانات" sheetId="3" r:id="rId3"/></sheets></workbook>' },
      { name: 'xl/_rels/workbook.xml.rels', data: '<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="/xl/worksheets/sheet3.xml"/><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>' },
      { name: 'xl/sharedStrings.xml', data: '<sst><si><t>مرحبا</t></si></sst>' },
      { name: 'xl/worksheets/sheet1.xml', data: '<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>' },
      { name: 'xl/worksheets/sheet3.xml', data: '<worksheet><sheetData><row r="2"><c r="B2" t="s"><v>0</v></c></row></sheetData></worksheet>' },
    ]);
    const book = await openBook('xlsx', b, 'w.xlsx');
    expect(book.sheets.map((s) => s.name)).toEqual(['Summary', 'البيانات']);
    const s = await book.sheet(1);
    expect(s.rows[1][1]?.v).toBe('مرحبا');
    expect(s.rows[0]).toEqual([null, null]);
  });
});

describe('ods', () => {
  it('reads sheets with repeated cells and rows, spans and comments left out', () => {
    const xml = `<office:document-content xmlns:office="o" xmlns:table="t" xmlns:text="x"><office:body><office:spreadsheet>
      <table:table table:name="One">
        <table:table-row><table:table-cell office:value-type="string"><text:p>Name</text:p></table:table-cell><table:table-cell table:number-columns-spanned="2" office:value-type="string"><text:p>Both</text:p></table:table-cell><table:covered-table-cell/></table:table-row>
        <table:table-row table:number-rows-repeated="2"><table:table-cell office:value-type="float" office:value="3"><text:p>3.00</text:p><office:annotation><text:p>a note</text:p></office:annotation></table:table-cell><table:table-cell table:number-columns-repeated="2" office:value-type="string"><text:p>x<text:s text:c="2"/>y</text:p></table:table-cell></table:table-row>
        <table:table-row table:number-rows-repeated="1048570"><table:table-cell table:number-columns-repeated="16384"/></table:table-row>
      </table:table>
      <table:table table:name="Two"><table:table-row><table:table-cell/></table:table-row></table:table>
      </office:spreadsheet></office:body></office:document-content>`;
    const [one, two] = readOdsContent(xml);
    expect(one.name).toBe('One');
    expect(one.rows.length).toBe(3);
    expect(one.cols).toBe(3);
    expect(one.rows[0].map((c) => c?.v ?? null)).toEqual(['Name', 'Both', null]);
    expect(one.merges).toEqual([{ r: 0, c: 1, rs: 1, cs: 2 }]);
    expect(one.rows[1].map((c) => c?.v)).toEqual(['3.00', 'x  y', 'x  y']);
    expect(one.rows[2][0]).toEqual({ v: '3.00', num: true });
    expect(one.totalRows).toBe(3); // the empty run at the end is not content
    expect(two.name).toBe('Two');
    expect(two.rows.length).toBe(0);
  });
});

describe('zip limits', () => {
  it('refuses a file with too many parts', async () => {
    const b = await bytesOf(Array.from({ length: 30 }, (_, i) => ({ name: `p${i}.xml`, data: 'x' })));
    expect(() => new OfficeZip(b, { entries: 20, total: CAPS.total })).toThrow(OfficeError);
    try { new OfficeZip(b, { entries: 20, total: CAPS.total }); } catch (e) { expect((e as OfficeError).problem).toBe('bomb'); }
  });
  it('refuses a file whose parts claim to unpack too large', async () => {
    const b = await bytesOf([{ name: 'a.xml', data: 'x' }]);
    // Patch the size in the table of contents to 2 GB.
    const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
    for (let i = 0; i < b.length - 4; i++) if (v.getUint32(i, true) === 0x02014b50) v.setUint32(i + 24, 0x80000000, true);
    expect(() => new OfficeZip(b)).toThrow(/unpacks too large/);
  });
  it('stops once what was really unpacked passes the total, whatever the sizes say', async () => {
    const big = 'x'.repeat(5000);
    const b = await bytesOf([{ name: 'a.xml', data: big }, { name: 'b.xml', data: big }]);
    const z = new OfficeZip(b, { entries: 10, total: 12000 });
    await z.text('a.xml');
    await z.text('a.xml');
    await expect(z.text('b.xml')).rejects.toMatchObject({ problem: 'bomb' });
  });
  it('says a password-protected (or old binary) file is protected, and a non-zip is damaged', () => {
    expect(() => new OfficeZip(new Uint8Array([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]))).toThrow(expect.objectContaining({ problem: 'protected' }));
    expect(() => new OfficeZip(enc('hello'))).toThrow(expect.objectContaining({ problem: 'damaged' }));
  });
  it('resolves relationship targets', () => {
    expect(resolvePart('ppt/slides/slide1.xml', '../media/image1.png')).toBe('ppt/media/image1.png');
    expect(resolvePart('word/document.xml', 'media/a.png')).toBe('word/media/a.png');
    expect(resolvePart('word/document.xml', '/word/media/a.png')).toBe('word/media/a.png');
    expect(resolvePart('a/b.xml', '../../../../etc/x')).toBe('etc/x');
  });
});

const PNS = 'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"';
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';

describe('pptx', () => {
  it('reads slides in order: title, bullets with levels, pictures of its own, notes; leaves out footers and linked pictures', async () => {
    const slide = (body: string) => `<p:sld ${PNS}><p:cSld><p:spTree>${body}</p:spTree></p:cSld></p:sld>`;
    const sp = (ph: string, paras: string) => `<p:sp><p:nvSpPr><p:cNvPr id="1" name="x"/><p:cNvSpPr/><p:nvPr>${ph}</p:nvPr></p:nvSpPr><p:txBody>${paras}</p:txBody></p:sp>`;
    const b = await bytesOf([
      { name: '_rels/.rels', data: `<Relationships><Relationship Id="rId1" Type="${REL}/officeDocument" Target="ppt/presentation.xml"/></Relationships>` },
      { name: 'ppt/presentation.xml', data: `<p:presentation ${PNS}><p:sldIdLst><p:sldId id="257" r:id="rId3"/><p:sldId id="256" r:id="rId2"/></p:sldIdLst></p:presentation>` },
      { name: 'ppt/_rels/presentation.xml.rels', data: `<Relationships><Relationship Id="rId2" Type="${REL}/slide" Target="slides/slide1.xml"/><Relationship Id="rId3" Type="${REL}/slide" Target="slides/slide2.xml"/></Relationships>` },
      { name: 'ppt/slides/slide1.xml', data: slide(sp('<p:ph type="title"/>', '<a:p><a:r><a:t>Second</a:t></a:r></a:p>')) },
      { name: 'ppt/slides/slide2.xml', data: slide(
        sp('<p:ph idx="1"/>', '<a:p><a:r><a:rPr b="1"/><a:t>Point one</a:t></a:r></a:p><a:p><a:pPr lvl="1"/><a:r><a:t>Sub point</a:t></a:r></a:p><a:p><a:pPr rtl="1"><a:buNone/></a:pPr><a:r><a:t>نص عربي</a:t></a:r></a:p>')
        + sp('<p:ph type="ctrTitle"/>', '<a:p><a:r><a:t>First</a:t></a:r><a:br/><a:r><a:t>slide</a:t></a:r></a:p>')
        + sp('<p:ph type="sldNum" idx="12"/>', '<a:p><a:fld type="slidenum"><a:t>1</a:t></a:fld></a:p>')
        + `<p:pic><p:nvPicPr><p:cNvPr id="4" name="Picture 3" descr="A chart"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId5"/></p:blipFill></p:pic>`
        + `<p:pic><p:nvPicPr><p:cNvPr id="5" name="Linked"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId6"/></p:blipFill></p:pic>`) },
      { name: 'ppt/slides/_rels/slide2.xml.rels', data: `<Relationships><Relationship Id="rId5" Type="${REL}/image" Target="../media/image1.png"/><Relationship Id="rId6" Type="${REL}/image" Target="https://example.com/x.png" TargetMode="External"/><Relationship Id="rId7" Type="${REL}/notesSlide" Target="../notesSlides/notesSlide1.xml"/></Relationships>` },
      { name: 'ppt/notesSlides/notesSlide1.xml', data: `<p:notes ${PNS}><p:cSld><p:spTree>${sp('<p:ph type="sldImg"/>', '<a:p><a:r><a:t>image</a:t></a:r></a:p>')}${sp('<p:ph type="body" idx="1"/>', '<a:p><a:r><a:t>Say hello</a:t></a:r></a:p>')}</p:spTree></p:cSld></p:notes>` },
      { name: 'ppt/media/image1.png', data: PNG },
    ]);
    const d = await readPptx(b);
    expect(d.slides.map((s) => s.title)).toEqual(['First slide', 'Second']);
    const s = d.slides[0];
    expect(s.blocks[0]).toMatchObject({ kind: 'text', role: 'title' });
    const body = s.blocks[1];
    expect(body.kind === 'text' && body.paras.map((p) => [p.runs.map((r) => r.t).join(''), p.level, p.list, !!p.rtl])).toEqual([
      ['Point one', 0, 'bullet', false], ['Sub point', 1, 'bullet', false], ['نص عربي', 0, 'none', true],
    ]);
    expect(body.kind === 'text' && body.paras[0].runs[0].b).toBe(true);
    expect(s.blocks.filter((x) => x.kind === 'image')).toEqual([{ kind: 'image', src: expect.stringMatching(/^blob:.*image\/png$/), alt: 'A chart' }]);
    expect(s.skipped).toBe(true); // the linked picture
    expect(s.notes).toBe('Say hello');
    expect(JSON.stringify(s)).not.toContain('example.com');
    expect(d.urls.length).toBe(1);
  });
  it('says a file that is not a presentation is the wrong kind', async () => {
    const b = await bytesOf([{ name: 'word/document.xml', data: '<w:document/>' }]);
    await expect(readPptx(b)).rejects.toMatchObject({ problem: 'wrongKind' });
  });
});

const ONS = 'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"';

describe('odt', () => {
  it('builds a page: headings, styled runs, numbered lists, tables with spans, own pictures only, right-to-left paragraphs', async () => {
    const content = `<office:document-content ${ONS}><office:automatic-styles>
        <style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
        <style:style style:name="P1" style:family="paragraph"><style:paragraph-properties style:writing-mode="rl-tb" fo:text-align="center"/></style:style>
        <text:list-style style:name="L1"><text:list-level-style-number text:level="1"/></text:list-style>
      </office:automatic-styles><office:body><office:text>
        <text:h text:outline-level="2">Heading</text:h>
        <text:p>Plain <text:span text:style-name="T1">bold</text:span><text:note><text:note-body><text:p>foot</text:p></text:note-body></text:note> <text:a xlink:href="https://example.org/">site</text:a> <text:a xlink:href="javascript:alert(1)">bad</text:a></text:p>
        <text:p text:style-name="P1">مرحبا</text:p>
        <text:list text:style-name="L1"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list>
        <table:table><table:table-header-rows><table:table-row><table:table-cell table:number-columns-spanned="2"><text:p>Head</text:p></table:table-cell><table:covered-table-cell/></table:table-row></table:table-header-rows>
          <table:table-row><table:table-cell><text:p>a</text:p></table:table-cell><table:table-cell><text:p>b</text:p></table:table-cell></table:table-row></table:table>
        <text:p><draw:frame><draw:image xlink:href="Pictures/p.png"/><svg:title>Logo</svg:title></draw:frame><draw:frame><draw:image xlink:href="http://example.com/x.png"/></draw:frame></text:p>
      </office:text></office:body></office:document-content>`;
    const b = await bytesOf([{ name: 'mimetype', data: 'application/vnd.oasis.opendocument.text' }, { name: 'content.xml', data: content }, { name: 'Pictures/p.png', data: PNG }]);
    const v = await odtToDom(b);
    const div = document.createElement('div');
    div.appendChild(v.node);
    expect(div.querySelector('h2')?.textContent).toBe('Heading');
    expect(div.querySelector('strong')?.textContent).toBe('bold');
    expect(Array.from(div.querySelectorAll('a')).map((a) => a.getAttribute('href'))).toEqual(['https://example.org/']);
    expect(div.textContent).not.toContain('foot');
    const rtl = Array.from(div.querySelectorAll('p')).find((p) => p.textContent === 'مرحبا')!;
    expect(rtl.getAttribute('dir')).toBe('rtl');
    expect(rtl.getAttribute('align')).toBe('center');
    expect(Array.from(div.querySelectorAll('ol > li')).map((li) => li.textContent)).toEqual(['one', 'two']);
    const th = div.querySelector('thead th') as HTMLTableCellElement;
    expect(th.textContent).toBe('Head');
    expect(th.colSpan).toBe(2);
    expect(th.getAttribute('scope')).toBe('col');
    const imgs = Array.from(div.querySelectorAll('img'));
    expect(imgs.length).toBe(1);
    expect(imgs[0].alt).toBe('Logo');
    expect(imgs[0].getAttribute('src')).toMatch(/^blob:/);
    expect(v.skipped).toBe(true);
    expect(div.innerHTML).not.toContain('example.com');
  });
});

describe('odp', () => {
  it('reads pages: title, outline with levels, notes, footers left out', async () => {
    const content = `<office:document-content ${ONS}><office:body><office:presentation>
      <draw:page draw:name="p1">
        <draw:frame presentation:class="title"><draw:text-box><text:p>Hello</text:p></draw:text-box></draw:frame>
        <draw:frame presentation:class="outline"><draw:text-box><text:list><text:list-item><text:p>Top</text:p><text:list><text:list-item><text:p>Inner</text:p></text:list-item></text:list></text:list-item></text:list></draw:text-box></draw:frame>
        <draw:frame presentation:class="footer"><draw:text-box><text:p>Footer</text:p></draw:text-box></draw:frame>
        <presentation:notes><draw:frame presentation:class="notes"><draw:text-box><text:p>Remember</text:p></draw:text-box></draw:frame></presentation:notes>
      </draw:page>
      <draw:page draw:name="p2"><draw:custom-shape><text:p>Shape text</text:p></draw:custom-shape></draw:page>
      </office:presentation></office:body></office:document-content>`;
    const d = await readOdp(await bytesOf([{ name: 'content.xml', data: content }]));
    expect(d.slides.length).toBe(2);
    expect(d.slides[0].title).toBe('Hello');
    const body = d.slides[0].blocks[1];
    expect(body.kind === 'text' && body.paras.map((p) => [p.runs.map((r) => r.t).join(''), p.level, p.list])).toEqual([['Top', 0, 'bullet'], ['Inner', 1, 'bullet']]);
    expect(JSON.stringify(d.slides[0])).not.toContain('Footer');
    expect(d.slides[0].notes).toBe('Remember');
    expect(d.slides[1].blocks[0]).toMatchObject({ kind: 'text', role: 'body' });
  });
});

describe('rtf', () => {
  const rtf = (s: string) => new Uint8Array(Array.from(s, (c) => c.charCodeAt(0)));
  it('reads paragraphs and bold, skipping tables of fonts, colours, pictures and optional parts', () => {
    const r = parseRtf(rtf('{\\rtf1\\ansi{\\fonttbl{\\f0 Arial;}}{\\colortbl;\\red0\\green0\\blue0;}{\\*\\generator Word;}\\f0 Hello {\\b bold}\\b0  end.\\par Second{\\pict\\pngblip 89504e47}\\line line\\par}'));
    expect(r.paras.map((p) => p.runs.map((x) => x.t).join(''))).toEqual(['Hello bold end.', 'Second\nline']);
    expect(r.paras[0].runs.find((x) => x.t === 'bold')?.b).toBe(true);
    expect(r.skipped).toBe(true);
  });
  it('decodes Arabic from the code page, the font’s character set and Unicode escapes, and right-to-left paragraphs', () => {
    // \'e3\'d1\'cd\'c8\'c7 = مرحبا in windows-1256
    const r = parseRtf(rtf("{\\rtf1\\ansi\\ansicpg1256{\\fonttbl{\\f1\\fcharset178 Arial;}}\\pard\\rtlpar\\qr \\'e3\\'d1\\'cd\\'c8\\'c7\\par\\pard\\uc1\\u1587?\\u1604?\\u1575?\\u1605? ok\\par}"));
    expect(r.paras[0]).toMatchObject({ rtl: true, align: 'right' });
    expect(r.paras[0].runs[0].t).toBe('مرحبا');
    expect(r.paras[1].runs[0].t).toBe('سلام ok');
    expect(r.paras[1].rtl).toBe(false);
    const latin = parseRtf(rtf("{\\rtf1\\ansi\\ansicpg1252{\\fonttbl{\\f0 Times;}{\\f1\\fcharset178 Arial;}}\\f0 caf\\'e9 {\\f1 \\'c7}\\par}"));
    expect(latin.paras[0].runs.map((x) => x.t).join('')).toBe('café ا');
  });
  it('never shows field codes, only their result, and refuses what is not RTF', () => {
    const r = parseRtf(rtf('{\\rtf1{\\field{\\*\\fldinst HYPERLINK "http://x"}{\\fldrslt link text}}\\par}'));
    expect(r.paras[0].runs.map((x) => x.t).join('')).toBe('link text');
    expect(() => parseRtf(rtf('hello'))).toThrow(OfficeError);
  });
  it('builds a page of text nodes only', () => {
    const v = rtfToDom(rtf('{\\rtf1 <img src=x onerror=alert(1)>\\par}'));
    const div = document.createElement('div');
    div.appendChild(v.node);
    expect(div.querySelector('img')).toBeNull();
    expect(div.textContent).toContain('<img');
  });
});

describe('docx', () => {
  const WNS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
  it('merged table cells get row spans, header rows become column headers, right-to-left comes from styles too', async () => {
    const cell = (t: string, pr = '') => `<w:tc><w:tcPr>${pr}</w:tcPr><w:p><w:r><w:t>${t}</w:t></w:r></w:p></w:tc>`;
    const doc = `<w:document ${WNS}><w:body>
      <w:p><w:pPr><w:pStyle w:val="Arabic"/></w:pPr><w:r><w:t>Styled</w:t></w:r></w:p>
      <w:p><w:r><w:t>Default</w:t></w:r></w:p>
      <w:tbl><w:tr><w:trPr><w:tblHeader/></w:trPr>${cell('H1')}${cell('H2')}</w:tr>
        <w:tr>${cell('tall', '<w:vMerge w:val="restart"/>')}${cell('b1')}</w:tr>
        <w:tr>${cell('', '<w:vMerge/>')}${cell('b2')}</w:tr>
        <w:tr>${cell('wide', '<w:gridSpan w:val="2"/>')}</w:tr></w:tbl>
      </w:body></w:document>`;
    const styles = `<w:styles ${WNS}><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Arabic"><w:name w:val="Arabic"/><w:basedOn w:val="Normal"/><w:pPr><w:bidi/></w:pPr></w:style></w:styles>`;
    const v = await docxToDom(await bytesOf([{ name: 'word/document.xml', data: doc }, { name: 'word/styles.xml', data: styles }]));
    const div = document.createElement('div');
    div.appendChild(v.node);
    const ps = Array.from(div.querySelectorAll(':scope > p'));
    expect(ps.map((p) => p.getAttribute('dir'))).toEqual(['rtl', 'auto']);
    expect(Array.from(div.querySelectorAll('thead th')).map((th) => [th.textContent, th.getAttribute('scope')])).toEqual([['H1', 'col'], ['H2', 'col']]);
    const tall = Array.from(div.querySelectorAll('td')).find((td) => td.textContent === 'tall') as HTMLTableCellElement;
    expect(tall.rowSpan).toBe(2);
    expect(div.querySelectorAll('tbody tr')[1].children.length).toBe(1);
    expect((Array.from(div.querySelectorAll('td')).find((td) => td.textContent === 'wide') as HTMLTableCellElement).colSpan).toBe(2);
  });
  it('a password-protected Word file says so', async () => {
    await expect(docxToDom(new Uint8Array([0xd0, 0xcf, 0x11, 0xe0, 0, 0, 0, 0]))).rejects.toMatchObject({ problem: 'protected' });
  });
});

void made;
