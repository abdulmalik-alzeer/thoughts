//! Cleaning a screen mock-up before the window may show it.
//!
//! A mock-up is HTML and CSS an assistant wrote, so it is hostile until proven otherwise. It is
//! parsed the way a browser parses a page (html5ever, the parser already built for the desktop
//! shell) and written out again from an ALLOWLIST: only elements and attributes that draw a static
//! layout survive, every piece of text is escaped again, and nothing that names an address
//! survives except pictures embedded in the file itself (`data:image/…`) and links within the page
//! (`#…`). What is not on the list is not copied: there is no list of bad things to keep complete.
//!
//! - Elements that run or load something (`script`, `iframe`, `object`, `embed`, `link`, `base`,
//!   `meta`, media, `template`, MathML…) go with everything inside them. Unknown but harmless
//!   elements are opened up: their text and children stay.
//! - `style` attributes and `<style>` blocks stay, read token by token as the CSS standard says
//!   (so `\75rl(` is `url(`): a declaration or rule that holds `@import`, a `url()` that is not an
//!   embedded picture, `image-set()`, `expression()`, `-moz-binding`, `behavior` or a font to load
//!   is left out whole.
//! - A small part of SVG stays for icons (shapes, text, gradients); `use`, `image`, `a`,
//!   `foreignObject`, `script`, `style` and animation do not.
//!
//! The result is `<style>` blocks first, then one `<body>`: the window puts its own first lines
//! (the content policy and the design's variables) before it and shows it only inside a sealed
//! frame. The frame and its policy would hold even a raw mock-up; this is the first of three walls.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use html5ever::interface::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{ns, parse_document, Attribute, ExpandedName, ParseOpts, QualName};

/// Elements nested deeper than this are left out with what they hold.
const MAX_DEPTH: usize = 200;
/// At most this many elements are written; the rest of the page is left out.
const MAX_ELEMENTS: usize = 20_000;
/// The longest ordinary attribute value kept.
const MAX_ATTR: usize = 2_000;
/// The longest `style` attribute kept.
const MAX_STYLE_ATTR: usize = 8_000;
/// The longest embedded picture kept (the address, as written).
const MAX_DATA_URL: usize = 300_000;
/// Rules nested deeper than this in a style sheet are left out.
const MAX_CSS_DEPTH: usize = 12;

// ---------- a small tree for the parser to build ----------

enum Data {
    Document,
    Text(RefCell<StrTendril>),
    Element { name: QualName, attrs: RefCell<Vec<Attribute>>, template: RefCell<Option<Handle>>, mathml_ip: bool },
    Other,
}

struct Node { parent: Cell<Option<Weak<Node>>>, children: RefCell<Vec<Handle>>, data: Data, depth: Cell<usize> }
type Handle = Rc<Node>;

impl Node {
    fn new(data: Data) -> Handle { Rc::new(Node { parent: Cell::new(None), children: RefCell::new(vec![]), data, depth: Cell::new(0) }) }
}

impl Drop for Node {
    /// Without recursion: a page of 80 000 nested `<div>`s must not overflow the stack when dropped.
    fn drop(&mut self) {
        let mut stack: Vec<Handle> = std::mem::take(&mut *self.children.borrow_mut());
        if let Data::Element { template, .. } = &self.data { stack.extend(template.borrow_mut().take()); }
        while let Some(n) = stack.pop() {
            stack.extend(std::mem::take(&mut *n.children.borrow_mut()));
            if let Data::Element { template, .. } = &n.data { stack.extend(template.borrow_mut().take()); }
        }
    }
}

fn parent_of(n: &Handle) -> Option<Handle> {
    let w = n.parent.take();
    let p = w.as_ref().and_then(Weak::upgrade);
    n.parent.set(w);
    p
}

fn detach(n: &Handle) {
    if let Some(p) = parent_of(n) {
        p.children.borrow_mut().retain(|c| !Rc::ptr_eq(c, n));
    }
    n.parent.set(None);
}

fn append_to(parent: &Handle, child: Handle) {
    detach(&child);
    child.parent.set(Some(Rc::downgrade(parent)));
    parent.children.borrow_mut().push(child);
}

struct Sink { document: Handle, stop: Rc<Cell<Option<Refused>>>, created: Cell<usize> }

impl Sink {
    /// A node took its place under `parent`: how deep it lies is the measure of how many elements
    /// are open at once. (A template's contents continue its depth, see `get_template_contents`.)
    fn placed(&self, parent: &Handle, child: &Handle) {
        let depth = parent.depth.get() + 1;
        child.depth.set(depth);
        if depth > MAX_OPEN && self.stop.get().is_none() { self.stop.set(Some(Refused::TooDeep)); }
    }

    /// Text joins the text node before it, so `<scr` + `ipt>` style splits never matter later.
    fn text_or_node(&self, child: NodeOrText<Handle>, before: Option<&Handle>) -> Option<Handle> {
        match child {
            NodeOrText::AppendNode(n) => Some(n),
            NodeOrText::AppendText(t) => {
                if let Some(Data::Text(prev)) = before.map(|b| &b.data) { prev.borrow_mut().push_tendril(&t); return None; }
                Some(Node::new(Data::Text(RefCell::new(t))))
            }
        }
    }
}

impl TreeSink for Sink {
    type Handle = Handle;
    type Output = Handle;
    type ElemName<'a> = ExpandedName<'a> where Self: 'a;

    fn finish(self) -> Handle { self.document }
    fn parse_error(&self, _: Cow<'static, str>) {}
    fn get_document(&self) -> Handle { self.document.clone() }
    fn set_quirks_mode(&self, _: QuirksMode) {}
    fn same_node(&self, x: &Handle, y: &Handle) -> bool { Rc::ptr_eq(x, y) }

    fn elem_name<'a>(&'a self, target: &'a Handle) -> ExpandedName<'a> {
        match &target.data { Data::Element { name, .. } => name.expanded(), _ => panic!("not an element") }
    }

    fn get_template_contents(&self, target: &Handle) -> Handle {
        match &target.data {
            Data::Element { template, .. } => {
                let contents = template.borrow().clone().expect("not a template");
                contents.depth.set(target.depth.get());
                contents
            }
            _ => panic!("not a template"),
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, target: &Handle) -> bool {
        matches!(&target.data, Data::Element { mathml_ip: true, .. })
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        // The limits, counted where the parser itself does the work. Passing one only raises a
        // flag: `clean` looks at it after every piece of the page and stops there.
        self.created.set(self.created.get() + 1);
        if self.stop.get().is_none() {
            if attrs.len() > MAX_ATTRS { self.stop.set(Some(Refused::TooManyAttributes)); }
            else if self.created.get() > MAX_CREATED { self.stop.set(Some(Refused::TooMany)); }
        }
        Node::new(Data::Element {
            name, attrs: RefCell::new(attrs),
            template: RefCell::new(flags.template.then(|| Node::new(Data::Document))),
            mathml_ip: flags.mathml_annotation_xml_integration_point,
        })
    }
    fn create_comment(&self, _: StrTendril) -> Handle { Node::new(Data::Other) }
    fn create_pi(&self, _: StrTendril, _: StrTendril) -> Handle { Node::new(Data::Other) }

    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        let last = parent.children.borrow().last().cloned();
        if let Some(n) = self.text_or_node(child, last.as_ref()) { self.placed(parent, &n); append_to(parent, n); }
    }

    fn append_before_sibling(&self, sibling: &Handle, child: NodeOrText<Handle>) {
        let Some(parent) = parent_of(sibling) else { return };
        let at = parent.children.borrow().iter().position(|c| Rc::ptr_eq(c, sibling)).unwrap_or(0);
        let before = if at > 0 { parent.children.borrow().get(at - 1).cloned() } else { None };
        if let Some(n) = self.text_or_node(child, before.as_ref()) {
            detach(&n);
            // Detaching may have moved the sibling (the node came from the same parent).
            let at = parent.children.borrow().iter().position(|c| Rc::ptr_eq(c, sibling)).unwrap_or(0);
            n.parent.set(Some(Rc::downgrade(&parent)));
            self.placed(&parent, &n);
            parent.children.borrow_mut().insert(at, n);
        }
    }

    fn append_based_on_parent_node(&self, element: &Handle, prev_element: &Handle, child: NodeOrText<Handle>) {
        if parent_of(element).is_some() { self.append_before_sibling(element, child) } else { self.append(prev_element, child) }
    }

    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}

    fn add_attrs_if_missing(&self, target: &Handle, attrs: Vec<Attribute>) {
        if let Data::Element { attrs: have, .. } = &target.data {
            let mut have = have.borrow_mut();
            for a in attrs { if !have.iter().any(|h| h.name == a.name) { have.push(a); } }
        }
    }

    fn remove_from_parent(&self, target: &Handle) { detach(target) }

    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        let kids = std::mem::take(&mut *node.children.borrow_mut());
        for k in kids {
            self.placed(new_parent, &k);
            k.parent.set(Some(Rc::downgrade(new_parent)));
            new_parent.children.borrow_mut().push(k);
        }
    }
}

// ---------- the lists ----------

/// HTML elements copied, with their children.
const HTML_KEPT: &[&str] = &[
    "div", "span", "p", "h1", "h2", "h3", "h4", "h5", "h6", "header", "footer", "main", "nav", "section", "article", "aside",
    "address", "hgroup", "figure", "figcaption", "blockquote", "pre", "hr", "br", "wbr", "ul", "ol", "li", "dl", "dt", "dd",
    "a", "b", "strong", "i", "em", "u", "s", "small", "mark", "sub", "sup", "code", "kbd", "samp", "var", "abbr", "cite", "q",
    "time", "del", "ins", "bdi", "bdo", "ruby", "rt", "rp", "img", "table", "thead", "tbody", "tfoot", "tr", "td", "th",
    "caption", "colgroup", "col", "form", "input", "button", "select", "option", "optgroup", "textarea", "label", "fieldset",
    "legend", "progress", "meter", "details", "summary",
];
/// HTML elements that end without children.
const HTML_VOID: &[&str] = &["br", "hr", "img", "input", "col", "wbr"];
/// HTML elements left out with everything inside them. Anything not here and not kept is opened
/// up (its children stay), which is safe because children are judged the same way and text is escaped.
const HTML_GONE: &[&str] = &[
    "script", "style", "iframe", "frame", "frameset", "object", "embed", "applet", "link", "base", "meta", "title", "head",
    "template", "noscript", "noembed", "noframes", "xmp", "plaintext", "listing", "audio", "video", "source", "track",
    "canvas", "map", "area", "portal", "slot", "datalist", "param", "dialog", "picture", "marquee", "bgsound", "menu",
];
/// Attributes any kept HTML element may carry.
const HTML_ANY: &[&str] = &["class", "id", "style", "title", "lang", "dir", "hidden", "role"];
/// Attributes by element.
const HTML_OWN: &[(&str, &[&str])] = &[
    ("a", &["href"]),
    ("img", &["src", "alt", "width", "height"]),
    ("input", &["type", "placeholder", "value", "checked", "disabled", "readonly", "name", "min", "max", "step"]),
    ("button", &["disabled"]),
    ("select", &["disabled", "multiple"]),
    ("option", &["selected", "value", "disabled", "label"]),
    ("optgroup", &["label", "disabled"]),
    ("textarea", &["placeholder", "rows", "cols", "disabled", "readonly"]),
    ("label", &["for"]),
    ("td", &["colspan", "rowspan", "width", "height"]),
    ("th", &["colspan", "rowspan", "scope", "width", "height"]),
    ("table", &["width"]),
    ("col", &["span", "width"]),
    ("colgroup", &["span"]),
    ("ol", &["start", "reversed"]),
    ("li", &["value"]),
    ("progress", &["value", "max"]),
    ("meter", &["value", "min", "max", "low", "high", "optimum"]),
    ("details", &["open"]),
    ("time", &["datetime"]),
];
/// What an `<input>` may be; anything else becomes a text box.
const INPUT_TYPES: &[&str] = &["text", "email", "password", "search", "number", "tel", "url", "checkbox", "radio", "range", "date", "time", "datetime-local", "month", "week", "color", "button", "submit", "reset"];
/// Attributes whose value is a small number or size.
const NUMERIC: &[&str] = &["width", "height", "colspan", "rowspan", "rows", "cols", "start", "span", "min", "max", "step", "low", "high", "optimum"];

/// SVG elements copied (names as the HTML parser writes them).
const SVG_KEPT: &[&str] = &[
    "svg", "g", "path", "circle", "ellipse", "rect", "line", "polyline", "polygon", "text", "tspan", "defs",
    "linearGradient", "radialGradient", "stop", "title", "desc", "clipPath",
];
/// SVG attributes copied (names as the HTML parser writes them).
const SVG_ATTRS: &[&str] = &[
    "class", "id", "style", "role", "viewBox", "width", "height", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r", "rx", "ry",
    "d", "points", "fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "stroke-dasharray",
    "stroke-dashoffset", "stroke-opacity", "stroke-miterlimit", "fill-opacity", "fill-rule", "clip-rule", "clip-path", "opacity",
    "transform", "offset", "stop-color", "stop-opacity", "gradientUnits", "gradientTransform", "text-anchor", "font-size",
    "font-weight", "font-family", "dominant-baseline", "preserveAspectRatio", "dx", "dy", "pathLength",
];

// ---------- writing the clean page ----------

fn esc_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c { '&' => out.push_str("&amp;"), '<' => out.push_str("&lt;"), '>' => out.push_str("&gt;"), '\0' => {}, c => out.push(c) }
    }
}

fn esc_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"), '<' => out.push_str("&lt;"), '>' => out.push_str("&gt;"), '"' => out.push_str("&quot;"),
            c if c.is_control() && c != '\n' && c != '\t' => {},
            c => out.push(c),
        }
    }
}

/// `data:image/…` of a kind a browser draws as a picture (and only as a picture: an SVG shown
/// this way runs nothing and loads nothing).
pub fn is_data_image(url: &str) -> bool {
    let u = url.trim();
    if u.len() > MAX_DATA_URL { return false; }
    let lower: String = u.chars().take(40).collect::<String>().to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("data:image/") else { return false };
    let kind: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '+').collect();
    if !matches!(kind.as_str(), "png" | "jpeg" | "gif" | "webp" | "svg+xml") { return false; }
    // Then parameters and the comma: `;base64,`, `;charset=utf-8,`, or `,` at once.
    let Some(comma) = u.find(',') else { return false };
    let head = &u["data:image/".len() + kind.len()..comma];
    head.is_empty() || (head.starts_with(';') && head.len() <= 40 && head.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ';' | '=' | '-')))
}

/// A link within the page.
fn is_fragment(href: &str) -> bool {
    let h = href.trim();
    h.len() <= 200 && h.starts_with('#') && h[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A value of an SVG drawing attribute: plain characters only, and a reference only to something
/// in the same drawing (`url(#shade)`).
fn svg_value_ok(v: &str) -> bool {
    if v.len() > MAX_ATTR || !v.chars().all(|c| c.is_ascii_alphanumeric() || " #.,%()-+_/:'\n\t".contains(c)) { return false; }
    let lower = v.to_ascii_lowercase();
    if !lower.contains("url") { return true; }
    let t = lower.trim();
    t.starts_with("url(#") && t.ends_with(')') && t[5..t.len() - 1].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn free_name(n: &str) -> bool {
    // `aria-…` and `data-…`: words for readers and hooks for the page's own styles. Never `on…`.
    (n.starts_with("aria-") || n.starts_with("data-")) && n.len() <= 48
        && n[5..].chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && n.len() > 5
}

struct Writer<'c> { out: String, styles: Vec<String>, elements: usize, clock: &'c Clock }

impl Writer<'_> {
    fn attr(&mut self, name: &str, value: &str) {
        self.out.push(' ');
        self.out.push_str(name);
        self.out.push_str("=\"");
        esc_attr(value, &mut self.out);
        self.out.push('"');
    }

    fn html_attrs(&mut self, el: &str, attrs: &[Attribute]) {
        let own = HTML_OWN.iter().find(|(e, _)| *e == el).map(|(_, a)| *a).unwrap_or(&[]);
        let mut seen: Vec<&str> = vec![];
        for a in attrs {
            if a.name.ns != ns!() { continue; }
            let name: &str = &a.name.local;
            let value: &str = &a.value;
            if name.starts_with("on") || seen.contains(&name) { continue; }
            if !(HTML_ANY.contains(&name) || own.contains(&name) || free_name(name)) { continue; }
            let kept: Option<String> = match name {
                "style" => (value.len() <= MAX_STYLE_ATTR).then(|| clean_css_by(value, false, self.clock)).filter(|s| !s.trim().is_empty()),
                "href" => is_fragment(value).then(|| value.trim().to_string()),
                "src" => is_data_image(value).then(|| value.trim().to_string()),
                "dir" => matches!(value.to_ascii_lowercase().as_str(), "ltr" | "rtl" | "auto").then(|| value.to_ascii_lowercase()),
                "type" => Some(if INPUT_TYPES.contains(&value.to_ascii_lowercase().as_str()) { value.to_ascii_lowercase() } else { "text".into() }),
                n if NUMERIC.contains(&n) => (value.len() <= 20 && value.chars().all(|c| c.is_ascii_alphanumeric() || ".%-:".contains(c))).then(|| value.to_string()),
                _ => (value.len() <= MAX_ATTR).then(|| value.to_string()),
            };
            if let Some(v) = kept { seen.push(name); self.attr(name, &v); }
        }
        // A button in a mock-up never sends anything.
        if el == "button" { self.attr("type", "button"); }
    }

    fn svg_attrs(&mut self, attrs: &[Attribute]) {
        let mut seen: Vec<&str> = vec![];
        for a in attrs {
            if a.name.ns != ns!() { continue; }
            let name: &str = &a.name.local;
            let value: &str = &a.value;
            if name.to_ascii_lowercase().starts_with("on") || seen.contains(&name) { continue; }
            if !(SVG_ATTRS.contains(&name) || free_name(name)) { continue; }
            let kept = match name {
                "style" => (value.len() <= MAX_STYLE_ATTR).then(|| clean_css_by(value, false, self.clock)).filter(|s| !s.trim().is_empty()),
                "class" | "id" | "role" => (value.len() <= MAX_ATTR).then(|| value.to_string()),
                n if free_name(n) => (value.len() <= MAX_ATTR).then(|| value.to_string()),
                "d" | "points" => (value.len() <= 60_000 && svg_value_ok_long(value)).then(|| value.to_string()),
                _ => svg_value_ok(value).then(|| value.to_string()),
            };
            if let Some(v) = kept { seen.push(name); self.attr(name, &v); }
        }
    }

    fn children(&mut self, node: &Handle, depth: usize, in_svg: bool) {
        // The list is copied out so no borrow is held while walking.
        let kids: Vec<Handle> = node.children.borrow().clone();
        for k in &kids { self.node(k, depth, in_svg); }
    }

    fn node(&mut self, node: &Handle, depth: usize, in_svg: bool) {
        match &node.data {
            Data::Text(t) => esc_text(&t.borrow(), &mut self.out),
            Data::Element { name, attrs, .. } => {
                if depth > MAX_DEPTH || self.elements >= MAX_ELEMENTS || self.clock.late() { return; }
                let local: &str = &name.local;
                if name.ns == ns!(svg) {
                    // Only inside a drawing that starts with <svg>, and only the listed shapes.
                    if !(SVG_KEPT.contains(&local) && (in_svg || local == "svg")) { return; }
                    self.elements += 1;
                    self.out.push('<');
                    self.out.push_str(local);
                    self.svg_attrs(&attrs.borrow());
                    self.out.push('>');
                    self.children(node, depth + 1, true);
                    self.out.push_str("</");
                    self.out.push_str(local);
                    self.out.push('>');
                } else if name.ns == ns!(html) && !in_svg {
                    if local == "style" {
                        let css: String = node.children.borrow().iter().filter_map(|c| match &c.data { Data::Text(t) => Some(t.borrow().to_string()), _ => None }).collect();
                        let css = clean_css_by(&css, true, self.clock);
                        if !css.trim().is_empty() { self.styles.push(css); }
                        return;
                    }
                    if HTML_GONE.contains(&local) { return; }
                    if !HTML_KEPT.contains(&local) { self.children(node, depth + 1, false); return; }
                    self.elements += 1;
                    self.out.push('<');
                    self.out.push_str(local);
                    self.html_attrs(local, &attrs.borrow());
                    self.out.push('>');
                    if HTML_VOID.contains(&local) { return; }
                    self.children(node, depth + 1, false);
                    self.out.push_str("</");
                    self.out.push_str(local);
                    self.out.push('>');
                }
                // Anything else (MathML, HTML inside a drawing) is left out with what it holds.
            }
            Data::Document | Data::Other => {}
        }
    }
}

/// Path data and point lists: digits, letters of the path language, separators.
fn svg_value_ok_long(v: &str) -> bool {
    v.chars().all(|c| c.is_ascii_alphanumeric() || " .,-+\n\t".contains(c))
}

fn child_named(parent: &Handle, local: &str) -> Option<Handle> {
    parent.children.borrow().iter().find(|c| matches!(&c.data, Data::Element { name, .. } if name.ns == ns!(html) && &*name.local == local)).cloned()
}

/// How deep elements may lie in one another before reading is abandoned. The parser
/// looks back over every open element for each new one, so a page of nothing but opening tags
/// would cost the square of its length. Measured on the tree the parser builds, not predicted from the text.
const MAX_OPEN: usize = 1_000;
/// Elements a page may create before reading is abandoned.
const MAX_CREATED: usize = 30_000;
/// Attributes on one element before reading is abandoned.
const MAX_ATTRS: usize = 400;
/// The page is handed to the parser in pieces of this many bytes; the limits and the clock are
/// looked at between pieces.
const PIECE: usize = 2_048;
/// The longest the reading of one page may take.
const MAX_TIME: std::time::Duration = std::time::Duration::from_secs(2);

/// Why a mock-up was not cleaned: it is not shown at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Refused { TooDeep, TooMany, TooManyAttributes, TooSlow }

/// The mock-up, made safe to put in the sealed frame: `<style>` blocks, then one `<body>`.
/// `Err`: the page passed a limit while it was being read (see `Refused`); nothing of it is shown.
pub fn clean(html: &str) -> Result<String, Refused> { clean_within(html, MAX_TIME) }

fn clean_within(html: &str, time: std::time::Duration) -> Result<String, Refused> {
    let started = std::time::Instant::now();
    let opts = ParseOpts { tree_builder: html5ever::tree_builder::TreeBuilderOpts { scripting_enabled: false, ..Default::default() }, ..Default::default() };
    let stop: Rc<Cell<Option<Refused>>> = Rc::new(Cell::new(None));
    let mut parser = parse_document(Sink { document: Node::new(Data::Document), stop: stop.clone(), created: Cell::new(0) }, opts);
    let mut at = 0;
    while at < html.len() {
        let mut end = (at + PIECE).min(html.len());
        while !html.is_char_boundary(end) { end += 1; }
        parser.process(StrTendril::from_slice(&html[at..end]));
        at = end;
        if let Some(why) = stop.get() { return Err(why); }
        if started.elapsed() > time { return Err(Refused::TooSlow); }
    }
    let document = parser.finish();
    if let Some(why) = stop.get() { return Err(why); }
    // Writing the page out and reading its CSS run on the same clock as the parsing did.
    let clock = Clock::until(started + time);
    let mut w = Writer { out: String::new(), styles: vec![], elements: 0, clock: &clock };
    let root = child_named(&document, "html");
    let (head, body) = match &root { Some(r) => (child_named(r, "head"), child_named(r, "body")), None => (None, None) };

    // Of the head only the style sheets are read.
    if let Some(head) = &head {
        let kids: Vec<Handle> = head.children.borrow().clone();
        for k in &kids {
            if matches!(&k.data, Data::Element { name, .. } if name.ns == ns!(html) && &*name.local == "style") { w.node(k, 1, false); }
        }
    }

    let mut open = String::from("<body");
    if let Some(body) = &body {
        let mut bw = Writer { out: String::new(), styles: vec![], elements: 0, clock: &clock };
        let mut attrs: Vec<Attribute> = match &body.data { Data::Element { attrs, .. } => attrs.borrow().clone(), _ => vec![] };
        // The page's language and direction are usually written on <html>.
        if let Some(Data::Element { attrs: on_root, .. }) = root.as_ref().map(|r| &r.data) {
            for a in on_root.borrow().iter() {
                if matches!(&*a.name.local, "lang" | "dir") && !attrs.iter().any(|b| b.name == a.name) { attrs.push(a.clone()); }
            }
        }
        attrs.retain(|a| matches!(&*a.name.local, "class" | "id" | "style" | "lang" | "dir"));
        bw.html_attrs("body", &attrs);
        open.push_str(&bw.out);
        w.children(body, 1, false);
    }
    open.push('>');

    let mut out = String::with_capacity(w.out.len() + 64);
    for css in &w.styles {
        out.push_str("<style>");
        out.push_str(css);
        out.push_str("</style>");
    }
    out.push_str(&open);
    out.push_str(&w.out);
    out.push_str("</body>");
    // Out of time anywhere on the way: a page written half is not shown.
    if clock.late_now() { return Err(Refused::TooSlow); }
    Ok(out)
}

// ---------- CSS ----------
//
// A tokenizer of our own, written after the CSS Syntax standard (section 4, "Tokenization"):
// the well-known CSS parser for Rust is under a licence this project does not take in. Only what
// decides safety is kept exact: where strings, comments, addresses, functions and blocks begin and
// end, and what a name is once its escapes are undone.

/// Function names that load something or run something, in any letter case (escapes are already
/// undone). `url` is judged separately: an embedded picture may stay.
const CSS_FUNCTIONS_GONE: &[&str] = &["src", "image-set", "-webkit-image-set", "image", "cross-fade", "-webkit-cross-fade", "element", "-moz-element", "expression", "paint"];
/// At-rules that load something, or that a mock-up has no use for.
const CSS_AT_GONE: &[&str] = &["import", "font-face", "namespace", "charset", "document", "-moz-document"];
/// Names that bind behaviour to an element in old browsers.
const CSS_IDENTS_GONE: &[&str] = &["-moz-binding", "behavior"];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Space, Comment, Ident(String), Function(String), At(String), Hash, Str(String), BadStr, Url(String), BadUrl, Number,
    /// A comment the text ends inside of.
    Unclosed,
    Delim(char), Colon, Semi, Comma, Open(char), Close(char), Cdo, Cdc,
}
struct Token { tok: Tok, start: usize, end: usize }

struct Lexer { c: Vec<char>, at: Vec<usize>, i: usize }

fn css_space(c: char) -> bool { matches!(c, ' ' | '\n' | '\t') }
fn name_start(c: char) -> bool { c.is_ascii_alphabetic() || c == '_' || !c.is_ascii() }
fn name_char(c: char) -> bool { name_start(c) || c.is_ascii_digit() || c == '-' }

impl Lexer {
    fn peek(&self, k: usize) -> Option<char> { self.c.get(self.i + k).copied() }
    fn escape_at(&self, k: usize) -> bool { self.peek(k) == Some('\\') && self.peek(k + 1) != Some('\n') }
    fn ident_at(&self, k: usize) -> bool {
        match self.peek(k) {
            Some('-') => self.peek(k + 1).is_some_and(|c| name_start(c) || c == '-') || self.escape_at(k + 1),
            Some('\\') => self.escape_at(k),
            Some(c) => name_start(c),
            None => false,
        }
    }
    fn number_at(&self, k: usize) -> bool {
        let digit = |n: usize| self.peek(n).is_some_and(|c| c.is_ascii_digit());
        match self.peek(k) {
            Some('+') | Some('-') => digit(k + 1) || (self.peek(k + 1) == Some('.') && digit(k + 2)),
            Some('.') => digit(k + 1),
            Some(c) => c.is_ascii_digit(),
            None => false,
        }
    }
    /// The backslash is already passed: the character it stands for.
    fn escaped(&mut self) -> char {
        let mut hex = String::new();
        while hex.len() < 6 && self.peek(0).is_some_and(|c| c.is_ascii_hexdigit()) { hex.push(self.c[self.i]); self.i += 1; }
        if hex.is_empty() {
            let Some(c) = self.peek(0) else { return '\u{fffd}' };
            self.i += 1;
            return c;
        }
        if self.peek(0).is_some_and(css_space) { self.i += 1; }
        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) { Some('\0') | None => '\u{fffd}', Some(c) => c }
    }
    fn name(&mut self) -> String {
        let mut out = String::new();
        loop {
            match self.peek(0) {
                Some(c) if name_char(c) => { out.push(c); self.i += 1; }
                Some('\\') if self.escape_at(0) => { self.i += 1; let c = self.escaped(); out.push(c); }
                _ => return out,
            }
        }
    }
    fn skip_space(&mut self) { while self.peek(0).is_some_and(css_space) { self.i += 1; } }

    fn string(&mut self, quote: char) -> Tok {
        let mut out = String::new();
        loop {
            match self.peek(0) {
                // The text ends inside the string: a browser would close it there, but then nothing
                // may be written after it, so the item is not kept.
                None => return Tok::BadStr,
                Some(c) if c == quote => { self.i += 1; return Tok::Str(out); }
                Some('\n') => return Tok::BadStr,
                Some('\\') => {
                    self.i += 1;
                    match self.peek(0) { None => {}, Some('\n') => self.i += 1, Some(_) => { let c = self.escaped(); out.push(c); } }
                }
                Some(c) => { out.push(c); self.i += 1; }
            }
        }
    }

    /// After `url(` when no quote follows: the address up to `)`.
    fn url(&mut self) -> Tok {
        self.skip_space();
        let mut out = String::new();
        loop {
            match self.peek(0) {
                None => return Tok::BadUrl,
                Some(')') => { self.i += 1; return Tok::Url(out); }
                Some(c) if css_space(c) => {
                    self.skip_space();
                    match self.peek(0) { None => return Tok::BadUrl, Some(')') => { self.i += 1; return Tok::Url(out); } _ => return self.bad_url() }
                }
                Some('"') | Some('\'') | Some('(') => return self.bad_url(),
                Some(c) if c.is_control() => return self.bad_url(),
                Some('\\') => {
                    if !self.escape_at(0) { return self.bad_url(); }
                    self.i += 1;
                    let c = self.escaped();
                    out.push(c);
                }
                Some(c) => { out.push(c); self.i += 1; }
            }
        }
    }
    fn bad_url(&mut self) -> Tok {
        loop {
            match self.peek(0) {
                None => return Tok::BadUrl,
                Some(')') => { self.i += 1; return Tok::BadUrl; }
                Some('\\') if self.escape_at(0) => { self.i += 1; self.escaped(); }
                Some(_) => self.i += 1,
            }
        }
    }

    fn ident_like(&mut self) -> Tok {
        let name = self.name();
        if self.peek(0) != Some('(') { return Tok::Ident(name); }
        self.i += 1;
        if name.eq_ignore_ascii_case("url") {
            let mut k = 0;
            while self.peek(k).is_some_and(css_space) { k += 1; }
            if !matches!(self.peek(k), Some('"') | Some('\'')) { return self.url(); }
        }
        Tok::Function(name)
    }

    fn number(&mut self) -> Tok {
        if matches!(self.peek(0), Some('+') | Some('-')) { self.i += 1; }
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) { self.i += 1; }
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
            while self.peek(0).is_some_and(|c| c.is_ascii_digit()) { self.i += 1; }
        }
        if matches!(self.peek(0), Some('e') | Some('E')) {
            let sign = matches!(self.peek(1), Some('+') | Some('-')) as usize;
            if self.peek(1 + sign).is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1 + sign;
                while self.peek(0).is_some_and(|c| c.is_ascii_digit()) { self.i += 1; }
            }
        }
        if self.ident_at(0) { self.name(); } else if self.peek(0) == Some('%') { self.i += 1; }
        Tok::Number
    }

    fn next(&mut self) -> Option<Token> {
        let c = self.peek(0)?;
        let from = self.i;
        let tok = match c {
            c if css_space(c) => { self.skip_space(); Tok::Space }
            '/' if self.peek(1) == Some('*') => {
                self.i += 2;
                while self.i < self.c.len() && !(self.c[self.i] == '*' && self.peek(1) == Some('/')) { self.i += 1; }
                if self.i >= self.c.len() { Tok::Unclosed } else { self.i += 2; Tok::Comment }
            }
            '"' | '\'' => { self.i += 1; self.string(c) }
            '#' if self.peek(1).is_some_and(name_char) || self.escape_at(1) => { self.i += 1; self.name(); Tok::Hash }
            '(' | '[' | '{' => { self.i += 1; Tok::Open(c) }
            ')' | ']' | '}' => { self.i += 1; Tok::Close(c) }
            ',' => { self.i += 1; Tok::Comma }
            ':' => { self.i += 1; Tok::Colon }
            ';' => { self.i += 1; Tok::Semi }
            '+' | '.' if self.number_at(0) => self.number(),
            '-' if self.number_at(0) => self.number(),
            '-' if self.peek(1) == Some('-') && self.peek(2) == Some('>') => { self.i += 3; Tok::Cdc }
            '-' if self.ident_at(0) => self.ident_like(),
            '<' if self.peek(1) == Some('!') && self.peek(2) == Some('-') && self.peek(3) == Some('-') => { self.i += 4; Tok::Cdo }
            '@' if self.ident_at(1) => { self.i += 1; Tok::At(self.name()) }
            '\\' if self.escape_at(0) => self.ident_like(),
            c if c.is_ascii_digit() => self.number(),
            c if name_start(c) => self.ident_like(),
            c => { self.i += 1; Tok::Delim(c) }
        };
        Some(Token { tok, start: self.at[from], end: self.at[self.i] })
    }
}

fn css_tokens(src: &str) -> Vec<Token> {
    let mut at: Vec<usize> = src.char_indices().map(|(i, _)| i).collect();
    at.push(src.len());
    let mut lx = Lexer { c: src.chars().collect(), at, i: 0 };
    let mut out = vec![];
    while let Some(t) = lx.next() { out.push(t); }
    out
}

fn closer(open: char) -> char { match open { '(' => ')', '[' => ']', _ => '}' } }

/// Is this token forbidden where it stands? (`url("…")` looks at what follows it.)
fn css_token_bad(toks: &[Token], i: usize) -> bool {
    match &toks[i].tok {
        Tok::Url(u) => !is_data_image(u),
        Tok::BadUrl | Tok::BadStr | Tok::Unclosed => true,
        Tok::At(name) => CSS_AT_GONE.iter().any(|n| name.eq_ignore_ascii_case(n)),
        Tok::Ident(name) => CSS_IDENTS_GONE.iter().any(|n| name.eq_ignore_ascii_case(n)),
        Tok::Function(name) => {
            let name = name.to_ascii_lowercase();
            if name != "url" { return CSS_FUNCTIONS_GONE.contains(&name.as_str()); }
            // url("…"): exactly one quoted address, an embedded picture, then the bracket closes.
            let mut rest = toks[i + 1..].iter().map(|t| &t.tok).filter(|t| !matches!(t, Tok::Space | Tok::Comment));
            !(matches!(rest.next(), Some(Tok::Str(s)) if is_data_image(s)) && matches!(rest.next(), Some(Tok::Close(')'))))
        }
        _ => false,
    }
}

/// From the token after an opening bracket to just past the bracket that closes it (brackets
/// inside are stepped over without recursion). Returns where to go on and whether anything in
/// between is forbidden.
fn css_skip_block(toks: &[Token], mut i: usize, open: char) -> (usize, bool) {
    let mut want = vec![closer(open)];
    let mut bad = false;
    while i < toks.len() {
        bad |= css_token_bad(toks, i);
        match &toks[i].tok {
            Tok::Open(c) => want.push(closer(*c)),
            Tok::Function(_) => want.push(')'),
            Tok::Close(c) if want.last() == Some(c) => { want.pop(); if want.is_empty() { return (i + 1, bad); } }
            _ => {}
        }
        i += 1;
    }
    (i, bad)
}

/// One list of rules or declarations, from token `i` to the `}` that ends the list (or the end):
/// each item runs to its `;` or to the end of its `{…}`. Returns the index after the list.
fn css_items(src: &str, toks: &[Token], mut i: usize, out: &mut String, depth: usize, blocks: bool, clock: &Clock) -> usize {
    loop {
        // Out of time: nothing more is read (the caller then shows none of it).
        if clock.late() { return toks.len(); }
        let first = i;
        let mut bad = false;
        loop {
            let Some(t) = toks.get(i) else {
                // The end of the text: a last declaration may come without its `;`.
                if i > first && !bad { out.push_str(&src[toks[first].start..toks[i - 1].end]); }
                return i;
            };
            match &t.tok {
                Tok::Semi => {
                    if !bad { out.push_str(&src[toks[first].start..t.end]); }
                    i += 1;
                    break;
                }
                Tok::Close('}') if depth > 0 => {
                    if i > first && !bad { out.push_str(&src[toks[first].start..toks[i - 1].end]); }
                    return i + 1;
                }
                // A closing bracket nothing opened: the item is not copied.
                Tok::Close(_) => { bad = true; i += 1; }
                Tok::Open('{') => {
                    if bad || !blocks || depth >= MAX_CSS_DEPTH {
                        i = css_skip_block(toks, i + 1, '{').0;
                    } else {
                        out.push_str(&src[toks[first].start..t.start]);
                        out.push('{');
                        i = css_items(src, toks, i + 1, out, depth + 1, blocks, clock);
                        out.push('}');
                    }
                    break;
                }
                Tok::Open(c) => { bad |= css_token_bad(toks, i); let (next, b) = css_skip_block(toks, i + 1, *c); bad |= b; i = next; }
                Tok::Function(_) => { bad |= css_token_bad(toks, i); let (next, b) = css_skip_block(toks, i + 1, '('); bad |= b; i = next; }
                _ => { bad |= css_token_bad(toks, i); i += 1; }
            }
        }
    }
}

/// The longest style sheet or `style` attribute read at all.
const MAX_CSS: usize = 200_000;

/// The moment by which a page must be done, asked now and then (asking costs a system call).
struct Clock { end: std::time::Instant, asked: Cell<u32>, over: Cell<bool> }
impl Clock {
    fn until(end: std::time::Instant) -> Clock { Clock { end, asked: Cell::new(0), over: Cell::new(false) } }
    fn late(&self) -> bool {
        if self.over.get() { return true; }
        self.asked.set(self.asked.get().wrapping_add(1));
        if self.asked.get().is_multiple_of(64) && std::time::Instant::now() > self.end { self.over.set(true); }
        self.over.get()
    }
    fn late_now(&self) -> bool {
        if std::time::Instant::now() > self.end { self.over.set(true); }
        self.over.get()
    }
}

/// CSS with everything that loads or runs something left out. `blocks`: a style sheet (rules with
/// `{…}`); otherwise the declarations of a `style` attribute, where a block is not expected and is
/// left out. A declaration or rule is kept exactly as written, or not at all.
pub fn clean_css(css: &str, blocks: bool) -> String {
    clean_css_by(css, blocks, &Clock::until(std::time::Instant::now() + MAX_TIME))
}

/// The same, within a page's time. Nothing here changes the text it keeps: every step is one
/// pass over it, and whatever cannot be shown as written is not shown.
fn clean_css_by(css: &str, blocks: bool, clock: &Clock) -> String {
    if css.len() > MAX_CSS || clock.late_now() { return String::new(); }
    // What the standard does before reading: one kind of line end, no NUL. The browser is then
    // given this very text, so both read the same characters.
    let src = css.replace("\r\n", "\n").replace(['\r', '\u{c}'], "\n").replace('\0', "\u{fffd}");
    // A backslash as the very last character would escape whatever is written after it.
    let src = src.trim_end_matches('\\').to_string();
    let toks = css_tokens(&src);
    if clock.late_now() { return String::new(); }
    let mut out = String::new();
    css_items(&src, &toks, 0, &mut out, 0, blocks, clock);
    if clock.late_now() { return String::new(); }
    // The text sits inside <style>…</style>: it must never hold the end of that element, nor the
    // start or end of a comment or of any tag. Nothing is taken out or rewritten to make it so (a
    // deletion can form the very thing it removes): a sheet with `<` before `/`, `!`, `?` or a
    // letter, or with `-->`, is not shown at all. A mock-up's styles have no use for them.
    let b = out.as_bytes();
    if b.windows(2).any(|w| w[0] == b'<' && (matches!(w[1], b'/' | b'!' | b'?') || w[1].is_ascii_alphabetic())) || out.contains("-->") { return String::new(); }
    // A second, blunt look at the result, in case this reading and the browser's ever disagree:
    // if anything forbidden can still be read out of it, none of it is shown.
    if css_still_bad(&out) { return String::new(); }
    out
}

/// CSS escapes undone, comments and blank space removed, lower case: what a browser would read.
fn css_flat(css: &str) -> String {
    let chars: Vec<char> = css.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) { i += 1; }
            i += 2;
            continue;
        }
        if c == '\\' {
            let mut j = i + 1;
            let mut hex = String::new();
            while j < chars.len() && hex.len() < 6 && chars[j].is_ascii_hexdigit() { hex.push(chars[j]); j += 1; }
            if !hex.is_empty() {
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) { out.extend(ch.to_lowercase()); }
                if j < chars.len() && chars[j].is_whitespace() { j += 1; }
                i = j;
            } else {
                if let Some(n) = chars.get(i + 1) { if !n.is_whitespace() { out.extend(n.to_lowercase()); } }
                i += 2;
            }
            continue;
        }
        if !c.is_whitespace() && c != '\0' { out.extend(c.to_lowercase()); }
        i += 1;
    }
    out
}

fn css_still_bad(css: &str) -> bool {
    let flat = css_flat(css);
    if flat.contains("@import") || flat.contains("expression(") || flat.contains("-moz-binding") || flat.contains("image-set(") { return true; }
    // `behavior:` as a property of its own (not `scroll-behavior:`).
    let mut from = 0;
    while let Some(at) = flat[from..].find("behavior:") {
        let at = from + at;
        let prev = flat[..at].chars().last();
        if !prev.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') { return true; }
        from = at + 1;
    }
    let mut from = 0;
    while let Some(at) = flat[from..].find("url(") {
        let rest = &flat[from + at + 4..];
        let rest = rest.strip_prefix('"').or_else(|| rest.strip_prefix('\'')).unwrap_or(rest);
        if !rest.starts_with("data:image/") { return true; }
        from += at + 4;
    }
    false
}

#[cfg(test)]
#[path = "mado_design_clean_tests.rs"]
mod tests;
