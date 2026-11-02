use std::fmt;
use std::fmt::Write as _;

use deser::Context;
use deser::de::DuplicateKeys;
use deser::ext::{Decimal, Number};
use deser_json::{DeserializerConfig, Indent, SerializerConfig};
use deser_value::{Kind, Map, Value as DeserValue};

use crate::unicode;

pub const MAX_DEPTH: usize = 1000;

#[derive(Clone, Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    BigInt(String),
    Float(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError(pub String);

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JsonError {}

impl Json {
    pub fn object<K: Into<String>>(entries: impl IntoIterator<Item = (K, Json)>) -> Json {
        Json::Object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    pub fn string(text: impl Into<String>) -> Json {
        Json::String(text.into())
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(entries) => Some(entries),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }
}

pub fn parse(text: &str) -> Result<Json, JsonError> {
    let (substituted, beyond) = substitute_beyond_f64(text)?;
    let config = DeserializerConfig::builder().context(Context::with(DuplicateKeys::Last)).build();
    let value: DeserValue = config.from_str(&substituted).map_err(|e| JsonError(e.to_string()))?;
    from_deser(&value, &beyond, 0)
}

const SUBSTITUTE_ESCAPE: &str = "\\u0000";

fn is_json_number(token: &str) -> bool {
    let bytes = token.as_bytes();
    let mut at = usize::from(bytes.first() == Some(&b'-'));
    let digits = |from: usize| bytes[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    match bytes.get(at) {
        Some(b'0') => at += 1,
        Some(b'1'..=b'9') => at += digits(at),
        _ => return false,
    }
    if bytes.get(at) == Some(&b'.') {
        let count = digits(at + 1);
        if count == 0 {
            return false;
        }
        at += 1 + count;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let count = digits(at);
        if count == 0 {
            return false;
        }
        at += count;
    }
    at == bytes.len()
}

fn beyond_f64(token: &str) -> Option<Json> {
    match token {
        "NaN" => return Some(Json::Float(f64::NAN)),
        "Infinity" => return Some(Json::Float(f64::INFINITY)),
        "-Infinity" => return Some(Json::Float(f64::NEG_INFINITY)),
        _ => {}
    }
    if !is_json_number(token) || token.parse::<f64>().is_ok_and(f64::is_finite) {
        return None;
    }
    if token.contains(['.', 'e', 'E']) {
        let negative = token.starts_with('-');
        return Some(Json::Float(if negative { f64::NEG_INFINITY } else { f64::INFINITY }));
    }
    Some(Json::BigInt(token.to_string()))
}

fn substitute_beyond_f64(text: &str) -> Result<(std::borrow::Cow<'_, str>, Vec<Json>), JsonError> {
    let bytes = text.as_bytes();
    let mut replacements: Vec<(usize, usize, Json)> = Vec::new();
    let mut escapes_nul = false;
    let mut at = 0;
    while at < bytes.len() {
        let b = bytes[at];
        if b == b'"' {
            at += 1;
            while at < bytes.len() && bytes[at] != b'"' {
                if bytes[at] == b'\\' {
                    if text[at..].starts_with(SUBSTITUTE_ESCAPE) {
                        escapes_nul = true;
                    }
                    at += 1;
                }
                at += 1;
            }
            at += 1;
            continue;
        }
        let starts_word = matches!(b, b'N' | b'I') || (b == b'-' && bytes.get(at + 1) == Some(&b'I'));
        if b == b'-' || b.is_ascii_digit() || starts_word {
            let start = at;
            at += 1;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'+' | b'-' | b'.')) {
                at += 1;
            }
            if let Some(json) = beyond_f64(&text[start..at]) {
                replacements.push((start, at, json));
            }
            continue;
        }
        at += 1;
    }
    if replacements.is_empty() {
        return Ok((std::borrow::Cow::Borrowed(text), Vec::new()));
    }
    if escapes_nul {
        return Err(JsonError(
            "JSON that holds both \\u0000 and numbers beyond the 64-bit Float range is not supported".to_string(),
        ));
    }
    let mut out = String::with_capacity(text.len() + replacements.len() * 12);
    let mut last = 0;
    let mut values = Vec::with_capacity(replacements.len());
    for (index, (start, end, json)) in replacements.into_iter().enumerate() {
        out.push_str(&text[last..start]);
        out.push('"');
        out.push_str(SUBSTITUTE_ESCAPE);
        out.push_str(&index.to_string());
        out.push('"');
        last = end;
        values.push(json);
    }
    out.push_str(&text[last..]);
    Ok((std::borrow::Cow::Owned(out), values))
}

fn substituted(text: &str, beyond: &[Json]) -> Option<Json> {
    let index = text.strip_prefix('\u{0}')?.parse::<usize>().ok()?;
    beyond.get(index).cloned()
}

fn from_deser(value: &DeserValue, beyond: &[Json], depth: usize) -> Result<Json, JsonError> {
    if depth > MAX_DEPTH {
        return Err(JsonError(format!("JSON nests deeper than {MAX_DEPTH} levels")));
    }
    Ok(match value.kind() {
        Kind::Null => Json::Null,
        Kind::Bool(b) => Json::Bool(*b),
        Kind::U64(n) => integer_text(&n.to_string()),
        Kind::I64(n) => Json::Int(*n),
        Kind::F64(f) => Json::Float(*f),
        Kind::F32(f) => Json::Float(f64::from(*f)),
        Kind::Str(s) | Kind::Lexical(s) => substituted(s, beyond).unwrap_or_else(|| Json::String(s.clone())),
        Kind::Ext(ext) => {
            if let Some(n) = ext.downcast_ref::<u128>() {
                integer_text(&n.to_string())
            } else if let Some(n) = ext.downcast_ref::<i128>() {
                integer_text(&n.to_string())
            } else if let Some(number) = ext.downcast_value_ref::<Number>() {
                if number.is_integer() {
                    integer_text(number.as_str())
                } else {
                    Json::Float(number.value())
                }
            } else {
                return Err(JsonError("JSON value of an unsupported kind".to_string()));
            }
        }
        Kind::Seq(seq) => {
            Json::Array(seq.into_iter().map(|item| from_deser(item, beyond, depth + 1)).collect::<Result<_, _>>()?)
        }
        Kind::Map(map) => {
            let mut entries = Vec::with_capacity(map.len());
            for (key, item) in map {
                let Some(key) = key.as_str() else {
                    return Err(JsonError("JSON object key is not a string".to_string()));
                };
                entries.push((key.to_string(), from_deser(item, beyond, depth + 1)?));
            }
            Json::Object(entries)
        }
        _ => return Err(JsonError("JSON value of an unsupported kind".to_string())),
    })
}

fn integer_text(text: &str) -> Json {
    match text.parse::<i64>() {
        Ok(n) => Json::Int(n),
        Err(_) => Json::BigInt(text.to_string()),
    }
}

fn to_deser(json: &Json) -> DeserValue {
    match json {
        Json::Null => DeserValue::from(()),
        Json::Bool(b) => DeserValue::from(*b),
        Json::Int(n) => DeserValue::from(*n),
        Json::BigInt(text) => decimal(text),
        Json::Float(f) if f.is_finite() => decimal(&python_float_repr(*f)),
        Json::Float(f) => DeserValue::from(*f),
        Json::String(s) => DeserValue::from(s.as_str()),
        Json::Array(items) => DeserValue::from(items.iter().map(to_deser).collect::<Vec<_>>()),
        Json::Object(entries) => {
            let mut map = Map::with_capacity(entries.len());
            for (key, item) in entries {
                map.insert(key.as_str(), to_deser(item));
            }
            DeserValue::from(map)
        }
    }
}

fn decimal(text: &str) -> DeserValue {
    match Decimal::new(text) {
        Ok(d) => DeserValue::ext(d),
        Err(_) => DeserValue::from(text),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Spaced,
    Indented,
}

pub fn dumps(json: &Json, layout: Layout) -> Result<String, JsonError> {
    let config = match layout {
        Layout::Spaced => SerializerConfig::builder().compact(false).non_finite_floats(true).build(),
        Layout::Indented => {
            SerializerConfig::builder().pretty(Indent::Spaces(2)).non_finite_floats(true).build()
        }
    };
    let written = config.to_string(&to_deser(json)).map_err(|e| JsonError(e.to_string()))?;
    Ok(ensure_ascii(&written))
}

fn ensure_ascii(text: &str) -> String {
    if text.bytes().all(|b| b < 0x7f) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        if (c as u32) < 0x7f {
            out.push(c);
            continue;
        }
        let mut units = [0u16; 2];
        for unit in c.encode_utf16(&mut units) {
            let _ = write!(out, "\\u{unit:04x}");
        }
    }
    out
}

pub fn dumps_str(text: &str) -> Result<String, JsonError> {
    dumps(&Json::String(text.to_string()), Layout::Spaced)
}

pub fn shortest_digits(value: f64) -> (String, i32) {
    let formatted = format!("{:e}", value.abs());
    let (mantissa, exponent) = formatted.split_once('e').unwrap_or((formatted.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().unwrap_or(0);
    (digits, exponent + 1)
}

pub fn python_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf".to_string() } else { "-inf".to_string() };
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0.0");
    }
    let (digits, point) = shortest_digits(value);
    let count = digits.len() as i32;
    let body = if point <= -4 || point > 16 {
        let exponent = point - 1;
        let mantissa = if count > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits.clone() };
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        format!("{mantissa}e{exponent_sign}{:02}", exponent.abs())
    } else if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point >= count {
        format!("{digits}{}.0", "0".repeat((point - count) as usize))
    } else {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    };
    format!("{sign}{body}")
}

pub fn repr_str(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c if c.is_ascii() || unicode::is_printable(c) => out.push(c),
            c if (c as u32) < 0x100 => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c if (c as u32) < 0x10000 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => {
                let _ = write!(out, "\\U{:08x}", c as u32);
            }
        }
    }
    out.push(quote);
    out
}

pub fn python_repr(json: &Json) -> String {
    let mut out = String::new();
    write_repr(json, &mut out);
    out
}

fn write_repr(json: &Json, out: &mut String) {
    match json {
        Json::Null => out.push_str("None"),
        Json::Bool(true) => out.push_str("True"),
        Json::Bool(false) => out.push_str("False"),
        Json::Int(n) => {
            let _ = write!(out, "{n}");
        }
        Json::BigInt(text) => out.push_str(text),
        Json::Float(f) => out.push_str(&python_float_repr(*f)),
        Json::String(s) => out.push_str(&repr_str(s)),
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_repr(item, out);
            }
            out.push(']');
        }
        Json::Object(entries) => {
            out.push('{');
            for (i, (key, item)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&repr_str(key));
                out.push_str(": ");
                write_repr(item, out);
            }
            out.push('}');
        }
    }
}

enum Numeric<'a> {
    Int(i64),
    Big(&'a str),
    Float(f64),
}

fn numeric(json: &Json) -> Option<Numeric<'_>> {
    match json {
        Json::Bool(b) => Some(Numeric::Int(i64::from(*b))),
        Json::Int(n) => Some(Numeric::Int(*n)),
        Json::BigInt(text) => Some(Numeric::Big(text)),
        Json::Float(f) => Some(Numeric::Float(*f)),
        _ => None,
    }
}

fn integer_equals_float(text: &str, f: f64) -> bool {
    f.is_finite() && f.fract() == 0.0 && format!("{f:.0}") == text
}

fn numbers_equal(a: &Numeric<'_>, b: &Numeric<'_>) -> bool {
    match (a, b) {
        (Numeric::Int(x), Numeric::Int(y)) => x == y,
        (Numeric::Float(x), Numeric::Float(y)) => x == y,
        (Numeric::Big(x), Numeric::Big(y)) => x == y,
        (Numeric::Int(x), Numeric::Big(y)) | (Numeric::Big(y), Numeric::Int(x)) => x.to_string() == *y,
        (Numeric::Int(x), Numeric::Float(f)) | (Numeric::Float(f), Numeric::Int(x)) => {
            integer_equals_float(&x.to_string(), *f)
        }
        (Numeric::Big(x), Numeric::Float(f)) | (Numeric::Float(f), Numeric::Big(x)) => integer_equals_float(x, *f),
    }
}

pub fn python_equal(a: &Json, b: &Json) -> bool {
    if let (Some(x), Some(y)) = (numeric(a), numeric(b)) {
        return numbers_equal(&x, &y);
    }
    match (a, b) {
        (Json::Null, Json::Null) => true,
        (Json::String(x), Json::String(y)) => x == y,
        (Json::Array(x), Json::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| python_equal(p, q)),
        (Json::Object(x), Json::Object(y)) => {
            x.len() == y.len()
                && x.iter().all(|(key, value)| {
                    y.iter().find(|(other, _)| other == key).is_some_and(|(_, other)| python_equal(value, other))
                })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_python() {
        let cases = [
            (1.0, "1.0"),
            (0.1, "0.1"),
            (-0.0, "-0.0"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (1.5e16, "1.5e+16"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1e-7, "1e-07"),
            (123.456, "123.456"),
            (2.5e-300, "2.5e-300"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (0.30000000000000004, "0.30000000000000004"),
            (1234567890123456.7, "1234567890123456.8"),
        ];
        for (value, text) in cases {
            assert_eq!(python_float_repr(value), text, "{value:e}");
        }
    }

    #[test]
    fn dumps_matches_python_layouts() {
        let value = Json::object([
            ("a", Json::Array(vec![Json::Int(1), Json::Float(2.5), Json::Object(vec![]), Json::Array(vec![])])),
            ("b", Json::object([("c", Json::string("x\u{e9}\u{7f}\u{1F600}\"\n\u{1}"))])),
            ("big", Json::BigInt("123456789012345678901234567890".to_string())),
            ("e", Json::Float(1e16)),
        ]);
        assert_eq!(
            dumps(&value, Layout::Spaced).unwrap(),
            r#"{"a": [1, 2.5, {}, []], "b": {"c": "x\u00e9\u007f\ud83d\ude00\"\n\u0001"}, "big": 123456789012345678901234567890, "e": 1e+16}"#
        );
        assert_eq!(
            dumps(&Json::object([("a", Json::Array(vec![Json::Int(1)])), ("b", Json::Null)]), Layout::Indented).unwrap(),
            "{\n  \"a\": [\n    1\n  ],\n  \"b\": null\n}"
        );
    }

    #[test]
    fn parse_keeps_python_number_kinds() {
        let parsed = parse(r#"{"a": 1, "b": 1.0, "c": 1e5, "d": 18446744073709551616, "a": 3}"#).unwrap();
        let entries = parsed.as_object().unwrap();
        assert_eq!(entries[0].0, "a");
        assert!(matches!(entries[0].1, Json::Int(3)));
        assert!(matches!(entries[1].1, Json::Float(f) if f == 1.0));
        assert!(matches!(entries[2].1, Json::Float(f) if f == 100000.0));
        assert!(matches!(&entries[3].1, Json::BigInt(t) if t == "18446744073709551616"));
    }

    #[test]
    fn parse_accepts_what_python_accepts_beyond_f64() {
        let digits = format!("1{}", "0".repeat(400));
        let parsed = parse(&format!("[1e999, -1e999, {digits}, NaN, Infinity, -Infinity, \"1e999\"]")).unwrap();
        let items = parsed.as_array().unwrap();
        assert!(matches!(items[0], Json::Float(f) if f == f64::INFINITY));
        assert!(matches!(items[1], Json::Float(f) if f == f64::NEG_INFINITY));
        assert!(matches!(&items[2], Json::BigInt(t) if *t == digits));
        assert!(matches!(items[3], Json::Float(f) if f.is_nan()));
        assert!(matches!(items[5], Json::Float(f) if f == f64::NEG_INFINITY));
        assert_eq!(items[6].as_str(), Some("1e999"));
        assert!(parse("[01e999]").is_err());
        assert_eq!(dumps(&Json::Array(items[..2].to_vec()), Layout::Spaced).unwrap(), "[Infinity, -Infinity]");
    }

    #[test]
    fn equality_follows_python() {
        assert!(python_equal(&Json::Int(1), &Json::Float(1.0)));
        assert!(python_equal(&Json::Bool(true), &Json::Int(1)));
        assert!(!python_equal(&Json::Int(1), &Json::Float(1.5)));
        assert!(python_equal(&Json::BigInt("18446744073709551616".to_string()), &Json::Float(18446744073709551616.0)));
        assert!(python_equal(
            &Json::object([("a", Json::Int(1)), ("b", Json::Null)]),
            &Json::object([("b", Json::Null), ("a", Json::Int(1))])
        ));
    }

    #[test]
    fn repr_matches_python() {
        assert_eq!(repr_str("it's"), "\"it's\"");
        assert_eq!(repr_str("a'\"b"), "'a\\'\"b'");
        assert_eq!(repr_str("\u{e9}\u{a0}\u{1}\u{200b}\u{1F600}"), "'\u{e9}\\xa0\\x01\\u200b\u{1F600}'");
        let value = Json::object([("id", Json::string("a")), ("n", Json::Array(vec![Json::Bool(true), Json::Null]))]);
        assert_eq!(python_repr(&value), "{'id': 'a', 'n': [True, None]}");
    }
}
