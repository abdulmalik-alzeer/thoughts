//! The design's settings (`DesignTokens`), the fonts a project may use, and the style examples
//! the app ships.
//!
//! Fonts are names from one fixed list, each standing for a stack of fonts that are already on
//! people's computers: nothing is ever downloaded. `ui/src/lib/mado/design/fonts.ts` holds the
//! same list for the window (a test here keeps the two in step).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A font a design may name: what the user sees, the stack it stands for, and its kind.
pub struct Font { pub name: &'static str, pub kind: &'static str, pub stack: &'static str }

pub const FONTS: &[Font] = &[
    Font { name: "System", kind: "sans", stack: "system-ui, -apple-system, \"Segoe UI\", Roboto, \"Helvetica Neue\", Arial, sans-serif" },
    Font { name: "Humanist", kind: "sans", stack: "Seravek, \"Gill Sans Nova\", Ubuntu, Calibri, \"DejaVu Sans\", sans-serif" },
    Font { name: "Geometric", kind: "sans", stack: "Avenir, Montserrat, Corbel, \"URW Gothic\", sans-serif" },
    Font { name: "Grotesque", kind: "sans", stack: "Inter, Roboto, \"Helvetica Neue\", \"Arial Nova\", \"Nimbus Sans\", Arial, sans-serif" },
    Font { name: "Classical", kind: "sans", stack: "Optima, Candara, \"Noto Sans\", sans-serif" },
    Font { name: "Rounded", kind: "sans", stack: "ui-rounded, \"Hiragino Maru Gothic ProN\", Quicksand, Comfortaa, \"Arial Rounded MT Bold\", Calibri, sans-serif" },
    Font { name: "Condensed", kind: "sans", stack: "Bahnschrift, \"DIN Alternate\", \"Franklin Gothic Medium\", \"Nimbus Sans Narrow\", sans-serif" },
    Font { name: "Old Style", kind: "serif", stack: "\"Iowan Old Style\", \"Palatino Linotype\", \"URW Palladio L\", P052, serif" },
    Font { name: "Transitional", kind: "serif", stack: "Charter, \"Bitstream Charter\", \"Sitka Text\", Cambria, serif" },
    Font { name: "Didone", kind: "serif", stack: "Didot, \"Bodoni MT\", \"Noto Serif Display\", \"URW Palladio L\", P052, Sylfaen, serif" },
    Font { name: "Slab", kind: "serif", stack: "Rockwell, \"Rockwell Nova\", \"Roboto Slab\", \"DejaVu Serif\", \"Sitka Small\", serif" },
    Font { name: "Mono", kind: "mono", stack: "ui-monospace, \"SF Mono\", \"Cascadia Code\", \"Source Code Pro\", Menlo, Consolas, \"DejaVu Sans Mono\", monospace" },
    Font { name: "Typewriter", kind: "mono", stack: "\"Nimbus Mono PS\", \"Courier New\", Courier, monospace" },
];

/// The names an assistant may use for fonts (for the prompt of the design step).
pub fn font_names() -> Vec<&'static str> { FONTS.iter().map(|f| f.name).collect() }
/// The fixed-width fonts (the only ones `font.mono` may name).
pub fn mono_font_names() -> Vec<&'static str> { FONTS.iter().filter(|f| f.kind == "mono").map(|f| f.name).collect() }

/// The CSS variables the window sets on a mock-up from the picked style (`tokens.ts`, `tokenVars`),
/// in their order: what the design prompt names. One spelling each (`onPrimary` is `--mado-color-on-primary`).
pub const CSS_VARS: [&str; 18] = [
    "--mado-color-bg", "--mado-color-surface", "--mado-color-text", "--mado-color-muted", "--mado-color-primary",
    "--mado-color-on-primary", "--mado-color-accent", "--mado-color-border", "--mado-color-success", "--mado-color-warning",
    "--mado-color-danger", "--mado-font-heading", "--mado-font-body", "--mado-font-mono", "--mado-radius", "--mado-spacing",
    "--mado-shadow", "--mado-density",
];

/// Corner radius, in pixels.
pub const RADIUS: (f64, f64) = (0.0, 32.0);
/// The base step of spacing, in pixels.
pub const SPACING: (f64, f64) = (2.0, 16.0);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Colors {
    pub bg: String, pub surface: String, pub text: String, pub muted: String, pub primary: String, pub on_primary: String,
    pub accent: String, pub border: String, pub success: String, pub warning: String, pub danger: String,
}

impl Colors {
    pub fn all(&self) -> [(&'static str, &str); 11] {
        [("bg", &self.bg), ("surface", &self.surface), ("text", &self.text), ("muted", &self.muted), ("primary", &self.primary),
         ("onPrimary", &self.on_primary), ("accent", &self.accent), ("border", &self.border), ("success", &self.success),
         ("warning", &self.warning), ("danger", &self.danger)]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fonts { pub heading: String, pub body: String, pub mono: String }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tokens { pub color: Colors, pub font: Fonts, pub radius: f64, pub spacing: f64, pub density: String, pub shadow: String }

/// `#rrggbb`, nothing else (no names, no alpha, no three-digit form).
pub fn is_hex(c: &str) -> bool {
    let b = c.as_bytes();
    b.len() == 7 && b[0] == b'#' && b[1..].iter().all(u8::is_ascii_hexdigit)
}

impl Tokens {
    /// Read and check a `DesignTokens` value. The error names the first thing that is wrong, for
    /// "Show details"; the person is told in plain words by the caller.
    pub fn parse(v: &Value) -> Result<Tokens, String> {
        let mut t: Tokens = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        for (name, c) in t.color.all() {
            if !is_hex(c) { return Err(format!("color.{name} is not #rrggbb")); }
        }
        // One spelling, so two equal designs are equal as text.
        let lower = |s: &mut String| *s = s.to_ascii_lowercase();
        let c = &mut t.color;
        for s in [&mut c.bg, &mut c.surface, &mut c.text, &mut c.muted, &mut c.primary, &mut c.on_primary, &mut c.accent, &mut c.border, &mut c.success, &mut c.warning, &mut c.danger] { lower(s); }
        for (role, f) in [("heading", &t.font.heading), ("body", &t.font.body), ("mono", &t.font.mono)] {
            let Some(font) = FONTS.iter().find(|x| x.name == f) else { return Err(format!("font.{role} is not one of the app's fonts")) };
            if role == "mono" && font.kind != "mono" { return Err("font.mono is not a fixed-width font".into()); }
        }
        if !t.radius.is_finite() || t.radius < RADIUS.0 || t.radius > RADIUS.1 { return Err(format!("radius is outside {}–{}", RADIUS.0, RADIUS.1)); }
        if !t.spacing.is_finite() || t.spacing < SPACING.0 || t.spacing > SPACING.1 { return Err(format!("spacing is outside {}–{}", SPACING.0, SPACING.1)); }
        if !matches!(t.density.as_str(), "compact" | "regular" | "roomy") { return Err("density is not compact, regular or roomy".into()); }
        if !matches!(t.shadow.as_str(), "none" | "soft" | "strong") { return Err("shadow is not none, soft or strong".into()); }
        Ok(t)
    }

    pub fn json(&self) -> Value { serde_json::to_value(self).unwrap_or(Value::Null) }
}

// ---------- contrast (WCAG 2) ----------

fn channel(v: u8) -> f64 {
    let s = v as f64 / 255.0;
    if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
}

/// Relative luminance of `#rrggbb` (0 for anything else).
pub fn luminance(hex: &str) -> f64 {
    if !is_hex(hex) { return 0.0; }
    let p = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
    0.2126 * channel(p(1)) + 0.7152 * channel(p(3)) + 0.0722 * channel(p(5))
}

/// The contrast ratio of two colours, 1 to 21.
pub fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

// ---------- the app's own examples ----------

#[allow(clippy::too_many_arguments)]
fn style(id: &str, colors: [&str; 11], fonts: [&str; 3], radius: f64, spacing: f64, density: &str, shadow: &str) -> (String, Tokens) {
    let [bg, surface, text, muted, primary, on_primary, accent, border, success, warning, danger] = colors.map(String::from);
    let [heading, body, mono] = fonts.map(String::from);
    (id.to_string(), Tokens {
        color: Colors { bg, surface, text, muted, primary, on_primary, accent, border, success, warning, danger },
        font: Fonts { heading, body, mono }, radius, spacing, density: density.into(), shadow: shadow.into(),
    })
}

/// The eight examples, in the order shown. Ids start with `app-`, which no assistant's style may.
/// Colours: bg, surface, text, muted, primary, onPrimary, accent, border, success, warning, danger.
pub fn app_tokens() -> Vec<(String, Tokens)> {
    vec![
        style("app-calm", ["#f7f7f5", "#ffffff", "#1f2328", "#5c6370", "#3b5bdb", "#ffffff", "#0b7285", "#dedfe3", "#2b8a3e", "#a85a00", "#c92a2a"],
            ["System", "System", "Mono"], 8.0, 8.0, "regular", "soft"),
        style("app-editorial", ["#fffdf8", "#ffffff", "#111111", "#555555", "#111111", "#ffffff", "#d12b2b", "#111111", "#1b7a3a", "#9a5200", "#c81e1e"],
            ["Didone", "Transitional", "Typewriter"], 0.0, 10.0, "roomy", "none"),
        style("app-warm", ["#fff8f0", "#ffffff", "#3b2a20", "#75594a", "#c2410c", "#ffffff", "#0f766e", "#f0dccb", "#2f7d32", "#9a5b00", "#b91c1c"],
            ["Rounded", "Humanist", "Mono"], 16.0, 9.0, "roomy", "soft"),
        style("app-dashboard", ["#f3f5f8", "#ffffff", "#17212e", "#536175", "#1d4ed8", "#ffffff", "#7c3aed", "#d5dbe5", "#15803d", "#b45309", "#b91c1c"],
            ["Grotesque", "Grotesque", "Mono"], 4.0, 6.0, "compact", "none"),
        style("app-terminal", ["#0d1117", "#161b22", "#e6edf3", "#9da7b3", "#3fb950", "#04210c", "#58a6ff", "#30363d", "#3fb950", "#d29922", "#f85149"],
            ["Grotesque", "System", "Mono"], 6.0, 8.0, "regular", "none"),
        style("app-pastel", ["#f6f3ff", "#ffffff", "#2e2a45", "#615c7d", "#6d4fc4", "#ffffff", "#c2417b", "#e3dcf7", "#2c8a5a", "#a8650a", "#c0365a"],
            ["Rounded", "Geometric", "Mono"], 20.0, 8.0, "regular", "soft"),
        style("app-contrast", ["#ffffff", "#ffffff", "#000000", "#333333", "#0033a0", "#ffffff", "#7a0060", "#000000", "#005a2b", "#6b3d00", "#a00000"],
            ["System", "System", "Mono"], 4.0, 10.0, "roomy", "none"),
        style("app-luxury", ["#12100e", "#1c1915", "#f3ece0", "#b3a892", "#c9a45c", "#1a1408", "#8fb5a3", "#3a332a", "#7fb88f", "#d9a441", "#e07a6a"],
            ["Didone", "Classical", "Mono"], 2.0, 10.0, "roomy", "strong"),
    ]
}

/// The app's examples as `StyleCandidate`s, named and described in the UI language.
pub fn app_styles() -> Vec<Value> {
    app_tokens().into_iter().map(|(id, tokens)| json!({
        "id": id,
        "name": crate::tr::text(&format!("mado.style.{id}.name"), &[]),
        "description": crate::tr::text(&format!("mado.style.{id}.desc"), &[]),
        "tokens": tokens.json(),
        "source": "app",
    })).collect()
}

/// The assistant's candidates from the text of `design/styles.json`: every one that is whole and
/// valid, and how many were left out. Nothing of an invalid candidate is shown.
pub fn assistant_styles(text: &str) -> (Vec<Value>, usize) {
    /// More than a handful is not a choice any more (the step asks for 3 or 4).
    const MOST: usize = 8;
    let Ok(doc) = serde_json::from_str::<Value>(text) else { return (vec![], 1) };
    let Some(list) = doc.get("styles").and_then(Value::as_array) else { return (vec![], 1) };
    let mut out: Vec<Value> = vec![];
    let mut dropped = 0;
    let mut seen = std::collections::HashSet::new();
    let plain = |v: Option<&Value>, max: usize| -> Option<String> {
        let s: String = v?.as_str()?.chars().filter(|c| !c.is_control()).collect();
        let s = s.trim().to_string();
        (!s.is_empty() && s.chars().count() <= max).then_some(s)
    };
    for c in list {
        let id = c.get("id").and_then(Value::as_str).filter(|id| crate::mado_design::valid_id(id) && !id.starts_with("app-"));
        let name = plain(c.get("name"), 80);
        let description = c.get("description").map_or(Some(String::new()), |d| plain(Some(d), 400).or_else(|| d.as_str().filter(|s| s.trim().is_empty()).map(|_| String::new())));
        let tokens = c.get("tokens").and_then(|t| Tokens::parse(t).ok());
        match (id, name, description, tokens) {
            (Some(id), Some(name), Some(description), Some(tokens)) if out.len() < MOST && seen.insert(id.to_string()) => {
                out.push(json!({"id": id, "name": name, "description": description, "tokens": tokens.json(), "source": "assistant"}));
            }
            _ => dropped += 1,
        }
    }
    (out, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_is_the_wcag_ratio() {
        assert!((contrast("#000000", "#ffffff") - 21.0).abs() < 1e-9);
        assert!((contrast("#ffffff", "#ffffff") - 1.0).abs() < 1e-9);
        // A well-known pair: #767676 on white is the lightest grey that passes AA.
        let c = contrast("#767676", "#ffffff");
        assert!((4.5..4.6).contains(&c), "{c}");
        assert_eq!(contrast("#123456", "#abcdef"), contrast("#abcdef", "#123456"));
    }

    #[test]
    fn every_shipped_style_is_valid_and_readable() {
        let styles = app_tokens();
        assert_eq!(styles.len(), 8);
        let mut ids = std::collections::HashSet::new();
        for (id, t) in &styles {
            assert!(crate::mado_design::valid_id(id) && id.starts_with("app-") && ids.insert(id.clone()), "{id}");
            assert_eq!(&Tokens::parse(&t.json()).unwrap_or_else(|e| panic!("{id}: {e}")), t, "{id}");
            let c = &t.color;
            // WCAG AA for running text: 4.5 to 1.
            for (what, fg, bg) in [("text on bg", &c.text, &c.bg), ("text on surface", &c.text, &c.surface), ("muted on bg", &c.muted, &c.bg),
                ("muted on surface", &c.muted, &c.surface), ("onPrimary on primary", &c.on_primary, &c.primary)] {
                assert!(contrast(fg, bg) >= 4.5, "{id}: {what} is {:.2}", contrast(fg, bg));
            }
            // Colours that carry meaning as text or as the edge of a control: 3 to 1 at least on both grounds…
            for (what, fg) in [("primary", &c.primary), ("accent", &c.accent), ("success", &c.success), ("warning", &c.warning), ("danger", &c.danger)] {
                for (ground, bg) in [("bg", &c.bg), ("surface", &c.surface)] {
                    assert!(contrast(fg, bg) >= 3.0, "{id}: {what} on {ground} is {:.2}", contrast(fg, bg));
                }
            }
            // The names exist in both languages.
            for part in ["name", "desc"] {
                let key = format!("mado.style.{id}.{part}");
                assert_ne!(crate::tr::text_in(false, &key, &[]), crate::tr::text_in(false, "unexpected", &[]), "{key}");
                assert_ne!(crate::tr::text_in(true, &key, &[]), crate::tr::text_in(true, "unexpected", &[]), "{key}");
            }
        }
        // The high-contrast example passes the stricter AAA (7 to 1) for all of its text.
        let hc = &styles.iter().find(|(id, _)| id == "app-contrast").unwrap().1.color;
        for (fg, bg) in [(&hc.text, &hc.bg), (&hc.muted, &hc.bg), (&hc.on_primary, &hc.primary), (&hc.danger, &hc.bg), (&hc.success, &hc.bg), (&hc.warning, &hc.bg), (&hc.accent, &hc.bg)] {
            assert!(contrast(fg, bg) >= 7.0, "{fg} on {bg}: {:.2}", contrast(fg, bg));
        }
        // Eight different looks, not one in eight colours.
        let distinct: std::collections::HashSet<String> = styles.iter().map(|(_, t)| format!("{}|{}|{}|{}", t.font.heading, t.radius, t.density, t.color.bg)).collect();
        assert_eq!(distinct.len(), 8);
    }

    #[test]
    fn tokens_are_checked_field_by_field() {
        let good = app_tokens()[0].1.json();
        assert!(Tokens::parse(&good).is_ok());
        let with = |path: &str, v: Value| { let mut t = good.clone(); *t.pointer_mut(path).unwrap() = v; Tokens::parse(&t) };
        for bad in ["red", "#fff", "#12345", "#1234567", "#12345g", "rgb(0,0,0)", "#ffffff;background:url(x)", " #ffffff", "#FFFFFF\n"] {
            assert!(with("/color/primary", json!(bad)).is_err(), "{bad}");
        }
        // Upper case is taken and written lower case.
        assert_eq!(with("/color/primary", json!("#AABBCC")).unwrap().color.primary, "#aabbcc");
        assert!(with("/font/body", json!("Comic Sans MS")).is_err());
        assert!(with("/font/body", json!("system-ui; } body { background: url(http://x) ")).is_err());
        assert!(with("/font/mono", json!("System")).is_err(), "the fixed-width role takes a fixed-width font");
        assert!(with("/font/heading", json!("Mono")).is_ok());
        for bad in [json!(-1), json!(33), json!("8"), json!(null), json!(1e99)] { assert!(with("/radius", bad.clone()).is_err(), "{bad}"); }
        for bad in [json!(1.9), json!(16.5), json!(true)] { assert!(with("/spacing", bad.clone()).is_err(), "{bad}"); }
        assert!(with("/density", json!("huge")).is_err());
        assert!(with("/shadow", json!("0 0 9px red")).is_err());
        // Nothing beyond the shape: an extra field anywhere is refused, a missing one too.
        let mut extra = good.clone();
        extra["color"]["link"] = json!("#000000");
        assert!(Tokens::parse(&extra).is_err());
        let mut extra = good.clone();
        extra["css"] = json!("body{}");
        assert!(Tokens::parse(&extra).is_err());
        let mut missing = good.clone();
        missing["color"].as_object_mut().unwrap().remove("danger");
        assert!(Tokens::parse(&missing).is_err());
        assert!(Tokens::parse(&json!("tokens")).is_err());
    }

    #[test]
    fn invalid_candidates_of_the_assistant_are_dropped_and_counted() {
        let good = app_tokens()[2].1.json();
        let mut bad = good.clone();
        bad["color"]["bg"] = json!("javascript:alert(1)");
        let doc = json!({"version": 1, "styles": [
            {"id": "sea", "name": "Sea", "description": "Cool and quiet.", "tokens": good, "source": "app"},
            {"id": "broken", "name": "Broken", "description": "", "tokens": bad},
            {"id": "sea", "name": "Sea again", "description": "", "tokens": good},
            {"id": "app-calm", "name": "Pretender", "description": "", "tokens": good},
            {"id": "../x", "name": "Path", "description": "", "tokens": good},
            {"id": "no-name", "name": "  ", "description": "", "tokens": good},
            {"id": "no-desc", "name": "No description", "tokens": good},
            "not an object",
        ]});
        let (styles, dropped) = assistant_styles(&doc.to_string());
        assert_eq!(styles.iter().map(|s| s["id"].as_str().unwrap()).collect::<Vec<_>>(), ["sea", "no-desc"]);
        assert_eq!(dropped, 6);
        // Whatever the file says, these are the assistant's.
        assert!(styles.iter().all(|s| s["source"] == "assistant"));
        assert_eq!(assistant_styles("{ not json"), (vec![], 1));
        assert_eq!(assistant_styles("{\"styles\": 3}"), (vec![], 1));
        // More than eight is not a choice: the rest are counted as left out.
        let many: Vec<Value> = (0..12).map(|i| json!({"id": format!("s{i}"), "name": format!("S{i}"), "description": "", "tokens": good})).collect();
        let (styles, dropped) = assistant_styles(&json!({"styles": many}).to_string());
        assert_eq!((styles.len(), dropped), (8, 4));
    }

    #[test]
    fn the_window_sets_exactly_the_variables_the_prompt_names() {
        let ts = include_str!("../../../ui/src/lib/mado/design/tokens.ts");
        let start = ts.find("export const CSS_VARS = [").expect("tokens.ts has CSS_VARS");
        let list = &ts[start..start + ts[start..].find("] as const").unwrap()];
        let names: Vec<&str> = list.split('\'').skip(1).step_by(2).collect();
        assert_eq!(names, CSS_VARS, "tokens.ts CSS_VARS and the daemon's CSS_VARS differ");
        let prompt = crate::mado_prompts::system("design", &crate::mado_prompts::Context::default());
        for v in CSS_VARS { assert!(prompt.contains(&format!("`{v}`")), "the design prompt does not name {v}"); }
        assert!(!prompt.contains("onPrimary`") && !prompt.contains("--mado-color-onPrimary"));
        for f in FONTS { assert!(prompt.contains(&format!("\"{}\"", f.name)), "the design prompt does not name the font {}", f.name); }
    }

    #[test]
    fn the_window_has_the_same_fonts() {
        let ts = include_str!("../../../ui/src/lib/mado/design/fonts.ts");
        for f in FONTS {
            let line = format!("{{ name: '{}', kind: '{}', stack: '{}' }}", f.name, f.kind, f.stack);
            assert!(ts.contains(&line), "fonts.ts is missing: {line}");
        }
        assert_eq!(ts.matches("{ name: '").count(), FONTS.len(), "fonts.ts has a font the daemon does not know");
        assert!(FONTS.iter().all(|f| !f.stack.contains('\'') && !f.stack.contains(';') && !f.stack.contains('<')));
    }
}
