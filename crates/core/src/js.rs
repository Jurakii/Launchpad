//! JavaScript semantics the 1.2.2 data files depend on, where Rust's own
//! differ: truthiness, `trim`'s whitespace set, `Math.round`, string lengths
//! in UTF-16 units, and how `JSON.stringify` writes numbers.
use serde_json::Value;
use std::path::Path;
use std::{fs, io};

/// JS truthiness of a JSON value, `None` standing for undefined.
pub(crate) fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// What JS `\s` and `String.prototype.trim` count as white space. Not the
/// same set as Rust's `char::is_whitespace` (U+FEFF is in, U+0085 is out).
pub(crate) fn js_is_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{D}' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}'
            | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// `String.prototype.trim`.
pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(js_is_space)
}

/// `Math.round`: halves go toward +infinity (-2.5 -> -2), where `f64::round`
/// goes away from zero. Monitors left of the primary have negative x.
pub(crate) fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `s.length`: UTF-16 units, so limits mean what they did in the JS build.
pub(crate) fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.slice(0, max)` in UTF-16 units. JS would cut a character outside the
/// BMP in half at the limit; here it is left out whole.
pub(crate) fn utf16_prefix(s: &str, max: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > max {
            return &s[..i];
        }
    }
    s
}

/// A number as `JSON.stringify` writes it: whole numbers without ".0".
pub(crate) fn js_num(n: f64) -> Value {
    // 2^53: every whole number below it is exact in both forms.
    if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
        Value::from(n as i64)
    } else {
        Value::from(n) // NaN and Infinity become null, as in JSON.stringify
    }
}

/// `Number.isInteger` on a JSON value: 2 and 2.0 are, "2" is not.
pub(crate) fn whole_number(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|n| n.fract() == 0.0)
}

/// JS `Number(value)` for a value read from JSON.
// shortcut: arrays and objects are NaN here; JS makes [] 0 and [x] Number(x).
// Only matters for a hand-edited settings.json.
pub(crate) fn js_to_number(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => string_to_number(s),
        _ => f64::NAN,
    }
}

// JS `Number(string)`: trimmed, "" is 0, 0x/0o/0b prefixes, else a decimal
// literal ("Infinity" spelled out; Rust's "inf" and "nan" are not numbers).
pub(crate) fn string_to_number(s: &str) -> f64 {
    let t = js_trim(s);
    if t.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if t.len() > 2 && t.is_char_boundary(2) && t[..2].eq_ignore_ascii_case(prefix) {
            let digits = &t[2..];
            if !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN; // from_str_radix would also take a sign
            }
            return u64::from_str_radix(digits, radix).map_or(f64::NAN, |n| n as f64);
        }
    }
    let unsigned = t.strip_prefix(['+', '-']).unwrap_or(t);
    if unsigned == "Infinity" {
        return if t.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    if !unsigned.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return f64::NAN; // "inf", "nan" and the like
    }
    t.parse().unwrap_or(f64::NAN)
}

/// `fs.readFileSync(file, 'utf-8')`: bytes that aren't UTF-8 are replaced,
/// not refused.
pub(crate) fn read_text(file: &Path) -> io::Result<String> {
    Ok(String::from_utf8_lossy(&fs::read(file)?).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_round_sends_halves_up_also_below_zero() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(-2.51), -3.0);
        assert_eq!(js_round(-0.5), 0.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
        assert_eq!(js_round(-1920.0), -1920.0);
    }

    #[test]
    fn js_trim_and_utf16_limits_match_js() {
        assert_eq!(js_trim("\u{FEFF} a\u{A0}\n"), "a");
        assert_eq!(js_trim("\u{85}a"), "\u{85}a");
        assert_eq!(utf16_len("a😀"), 3);
        assert_eq!(utf16_prefix("ab😀c", 3), "ab");
        assert_eq!(utf16_prefix("ab😀c", 4), "ab😀");
        assert_eq!(utf16_prefix("abc", 40), "abc");
    }

    #[test]
    fn js_num_writes_whole_numbers_without_a_fraction() {
        assert_eq!(js_num(1.0).to_string(), "1");
        assert_eq!(js_num(-1920.0).to_string(), "-1920");
        assert_eq!(js_num(0.25).to_string(), "0.25");
    }

    #[test]
    fn truthy_matches_js() {
        use serde_json::json;
        for (v, t) in [(json!(0), false), (json!(""), false), (json!(null), false), (json!("0"), true), (json!([]), true), (json!(-1), true)] {
            assert_eq!(truthy(Some(&v)), t, "{v}");
        }
        assert!(!truthy(None));
    }
}
