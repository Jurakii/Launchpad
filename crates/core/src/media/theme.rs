//! Media's colours: every colour the page shows is one of these tokens on
//! :root (media.css). The user's overrides are a map of token -> #rrggbb.
//! Media's accent is its own token, so Media can be themed apart from the
//! rest of Launchpad; with no override it follows the main window's accent.
//! Ported from renderer/media/theme.js; its localStorage and DOM parts
//! (load, save, apply, current) stay with the UI.
use serde_json::{Map, Value};

pub const STORAGE_KEY: &str = "launchpad-media-colors";

pub struct Token {
    pub token: &'static str,
    pub label: &'static str,
    pub dark: &'static str,
    pub light: &'static str,
}

const fn token(token: &'static str, label: &'static str, dark: &'static str, light: &'static str) -> Token {
    Token { token, label, dark, light }
}

/// token, label, and the defaults media.css uses for dark and light.
pub const TOKENS: &[Token] = &[
    token("--bg", "Background", "#16181d", "#f2f3f6"),
    token("--panel", "Panels and tiles", "#1e2128", "#ffffff"),
    token("--raised", "Buttons and fields", "#262a33", "#e6e9ef"),
    token("--border", "Borders", "#2b2f38", "#dde1e8"),
    token("--text", "Text", "#e8e9ec", "#1b1d22"),
    token("--muted", "Secondary text", "#a3a9b5", "#5d6573"),
    token("--accent", "Accent", "#5b8cff", "#3b6fe0"),
    token("--on-accent", "Text on accent", "#0b1020", "#ffffff"),
    token("--warn", "Warnings", "#f5c26b", "#8a5a00"),
    token("--danger", "Delete and errors", "#ff5b6b", "#c42a3f"),
    token("--shadow", "Shadows", "#000000", "#000000"),
];

/// Pairs checked for contrast (text, background, what to call it).
pub const CONTRAST_PAIRS: [(&str, &str, &str); 4] = [
    ("--text", "--panel", "Text on panels"),
    ("--text", "--bg", "Text on the background"),
    ("--muted", "--panel", "Secondary text on panels"),
    ("--on-accent", "--accent", "Text on accent"),
];

// /^#[0-9a-f]{6}$/i
fn is_hex(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7 && b[0] == b'#' && b[1..].iter().all(u8::is_ascii_hexdigit)
}

fn luminance(hex: &str) -> f64 {
    // Callers pass #rrggbb; anything else reads as black.
    let n = hex.get(1..).and_then(|h| u32::from_str_radix(h, 16).ok()).unwrap_or(0);
    let channel = |v: u32| {
        let c = f64::from(v) / 255.0;
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel((n >> 16) & 255) + 0.7152 * channel((n >> 8) & 255) + 0.0722 * channel(n & 255)
}

/// WCAG contrast ratio of two #rrggbb colours (1 to 21).
pub fn contrast(a: &str, b: &str) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Only known tokens with plain #rrggbb values survive, so a hand-edited
/// or stale entry can't inject anything into the page's styles. In TOKENS
/// order, lower-cased.
pub fn clean(map: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(map) = map.as_object() else { return out };
    for t in TOKENS {
        if let Some(v) = map.get(t.token).and_then(Value::as_str).filter(|v| is_hex(v)) {
            out.insert(t.token.into(), v.to_ascii_lowercase().into());
        }
    }
    out
}

/// The value each token has right now, in TOKENS order: the override, else
/// the default for the theme (with the main window's accent standing in
/// for Media's). `theme` is "light" or anything else for dark.
pub fn effective(overrides: &Map<String, Value>, theme: &str, shared_accent: Option<&str>) -> Vec<(&'static str, String)> {
    TOKENS
        .iter()
        .map(|t| {
            let mut value = (if theme == "light" { t.light } else { t.dark }).to_string();
            if let Some(accent) = shared_accent.filter(|a| t.token == "--accent" && is_hex(a)) {
                value = accent.to_ascii_lowercase();
            }
            match overrides.get(t.token).and_then(Value::as_str).filter(|v| !v.is_empty()) {
                Some(v) => (t.token, v.to_string()),
                None => (t.token, value),
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct LowContrast {
    pub label: &'static str,
    pub ratio: f64,
}

/// Every pair under WCAG AA (4.5:1).
pub fn low_contrast(values: &[(&'static str, String)]) -> Vec<LowContrast> {
    let get = |token: &str| values.iter().find(|(t, _)| *t == token).map_or("", |(_, v)| v.as_str());
    CONTRAST_PAIRS
        .iter()
        .map(|(fg, bg, label)| LowContrast { label, ratio: contrast(get(fg), get(bg)) })
        .filter(|p| p.ratio < 4.5)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn media_css() -> String {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../renderer/media/media.css");
        std::fs::read_to_string(file).unwrap()
    }

    // [...text.matchAll(/(--[a-z-]+):\s*(#[0-9a-f]{6})/gi)], lower-cased values
    fn read_tokens(text: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut rest = text;
        while let Some(at) = rest.find("--") {
            let after = &rest[at + 2..];
            let name_len = after.find(|c: char| !(c.is_ascii_alphabetic() || c == '-')).unwrap_or(after.len());
            let tail = after[name_len..].strip_prefix(':').map(|t| t.trim_start_matches(super::super::js_is_space));
            match tail.and_then(|t| t.get(..7)).filter(|v| is_hex(v)) {
                Some(value) if name_len > 0 => {
                    let name = format!("--{}", &after[..name_len]);
                    // Object.fromEntries: a later duplicate replaces the value in place.
                    match out.iter_mut().find(|(n, _)| *n == name) {
                        Some(slot) => slot.1 = value.to_ascii_lowercase(),
                        None => out.push((name, value.to_ascii_lowercase())),
                    }
                    rest = &after[name_len..];
                }
                _ => rest = &rest[at + 1..],
            }
        }
        out
    }

    fn block<'a>(css: &'a str, selector: &str) -> &'a str {
        let start = css.find(&format!("{selector} {{")).unwrap();
        &css[start..start + css[start..].find('}').unwrap()]
    }

    #[test]
    fn every_colour_token_in_media_css_is_editable_with_the_same_defaults_for_dark_and_light() {
        let css = media_css();
        let dark = read_tokens(block(&css, ":root"));
        let light = read_tokens(block(&css, ":root[data-theme='light']"));
        let mut names: Vec<&str> = dark.iter().map(|(n, _)| n.as_str()).collect();
        names.sort();
        let mut tokens: Vec<&str> = TOKENS.iter().map(|t| t.token).collect();
        tokens.sort();
        assert_eq!(names, tokens);
        let get = |list: &[(String, String)], token: &str| list.iter().find(|(n, _)| n == token).map(|(_, v)| v.clone());
        for t in TOKENS {
            assert_eq!(get(&dark, t.token).as_deref(), Some(t.dark), "{} dark", t.token);
            assert_eq!(get(&light, t.token).as_deref(), Some(t.light), "{} light", t.token);
        }
    }

    #[test]
    fn media_css_uses_no_colour_outside_the_tokens() {
        let mut rules = media_css();
        // .replace(/:root(\[data-theme='light'\])? \{[^}]*\}/g, '')
        for selector in [":root {", ":root[data-theme='light'] {"] {
            while let Some(start) = rules.find(selector) {
                let Some(len) = rules[start..].find('}') else { break };
                rules.replace_range(start..start + len + 1, "");
            }
        }
        // .replace(/\/\*[\s\S]*?\*\//g, '')
        while let Some(start) = rules.find("/*") {
            let Some(len) = rules[start + 2..].find("*/") else { break };
            rules.replace_range(start..start + 2 + len + 2, "");
        }
        // /#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/gi
        let b = rules.as_bytes();
        let hex_colours: Vec<&str> = (0..b.len())
            .filter(|&i| b[i] == b'#')
            .filter_map(|i| {
                let run = b[i + 1..].iter().take_while(|c| c.is_ascii_hexdigit()).count();
                let word = |c: &u8| c.is_ascii_alphanumeric() || *c == b'_';
                ((3..=8).contains(&run) && !b.get(i + 1 + run).is_some_and(word)).then(|| &rules[i..i + 1 + run])
            })
            .collect();
        assert_eq!(hex_colours, Vec::<&str>::new());
        let lower = rules.to_ascii_lowercase();
        for f in ["rgb(", "rgba(", "hsl(", "hsla("] {
            assert!(!lower.contains(f), "{f}");
        }
    }

    #[test]
    fn clean_keeps_only_known_tokens_with_rrggbb_values() {
        let out = clean(&json!({ "--text": "#ABCDEF", "--bg": "red", "--evil": "#000000", "--panel": "#123; color: x" }));
        assert_eq!(Value::Object(out), json!({ "--text": "#abcdef" }));
        assert!(clean(&Value::Null).is_empty());
        assert!(clean(&json!(["#000000"])).is_empty());
    }

    fn value(values: &[(&'static str, String)], token: &str) -> String {
        values.iter().find(|(t, _)| *t == token).unwrap().1.clone()
    }

    #[test]
    fn effective_overrides_win_the_main_window_accent_stands_in_for_an_unset_media_accent() {
        let none = Map::new();
        assert_eq!(value(&effective(&none, "dark", None), "--bg"), "#16181d");
        assert_eq!(value(&effective(&none, "light", None), "--bg"), "#f2f3f6");
        assert_eq!(value(&effective(&none, "dark", Some("#ff0000")), "--accent"), "#ff0000");
        let own = clean(&json!({ "--accent": "#00ff00" }));
        assert_eq!(value(&effective(&own, "dark", Some("#ff0000")), "--accent"), "#00ff00");
    }

    #[test]
    fn contrast_follows_wcag_and_the_defaults_pass_aa_a_bad_choice_is_reported() {
        assert_eq!(contrast("#000000", "#ffffff").round(), 21.0);
        assert_eq!(contrast("#777777", "#777777"), 1.0);
        let none = Map::new();
        assert!(low_contrast(&effective(&none, "dark", None)).is_empty());
        assert!(low_contrast(&effective(&none, "light", None)).is_empty());
        let low = low_contrast(&effective(&clean(&json!({ "--text": "#20232a" })), "dark", None));
        assert_eq!(low.iter().map(|p| p.label).collect::<Vec<_>>(), ["Text on panels", "Text on the background"]);
    }
}
