use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use crate::json::{Json, dumps_str, python_float_repr, shortest_digits};
use crate::types::{CALL_ERROR_KIND_NAME, CallErrorKind, Primitive, Type, show_type};

pub const INT_MIN: i64 = i64::MIN;
pub const INT_MAX: i64 = i64::MAX;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Variant {
    pub enum_name: Arc<str>,
    pub variant: Arc<str>,
    pub payload: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct RecordValue(Arc<[(Arc<str>, Value)]>);

impl RecordValue {
    pub fn new(fields: Vec<(Arc<str>, Value)>) -> RecordValue {
        RecordValue(fields.into())
    }

    pub fn fields(&self) -> &[(Arc<str>, Value)] {
        &self.0
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.iter().find(|(n, _)| &**n == name).map(|(_, v)| v)
    }
}

impl PartialEq for RecordValue {
    fn eq(&self, other: &RecordValue) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().all(|(name, value)| other.get(name).is_some_and(|o| o == value))
    }
}

impl Eq for RecordValue {}

impl Hash for RecordValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut combined: u64 = 0;
        for field in self.0.iter() {
            let mut hasher = DefaultHasher::new();
            field.hash(&mut hasher);
            combined = combined.wrapping_add(hasher.finish());
        }
        state.write_usize(self.0.len());
        state.write_u64(combined);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MapValue(Arc<[(Arc<str>, Value)]>);

impl MapValue {
    pub fn from_pairs(pairs: impl IntoIterator<Item = (Arc<str>, Value)>) -> MapValue {
        let mut entries: Vec<(Arc<str>, Value)> = Vec::new();
        for (key, value) in pairs {
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => entries.push((key, value)),
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        MapValue(entries.into())
    }

    fn from_sorted(entries: Vec<(Arc<str>, Value)>) -> MapValue {
        MapValue(entries.into())
    }

    pub fn entries(&self) -> &[(Arc<str>, Value)] {
        &self.0
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.binary_search_by(|(k, _)| (**k).cmp(key)).ok().map(|at| &self.0[at].1)
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn assoc(&self, key: Arc<str>, value: Value) -> MapValue {
        let mut entries = self.0.to_vec();
        match entries.binary_search_by(|(k, _)| k.cmp(&key)) {
            Ok(at) => entries[at].1 = value,
            Err(at) => entries.insert(at, (key, value)),
        }
        MapValue(entries.into())
    }

    pub fn dissoc(&self, key: &str) -> MapValue {
        MapValue(self.0.iter().filter(|(k, _)| &**k != key).cloned().collect())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Float(pub f64);

impl PartialEq for Float {
    fn eq(&self, other: &Float) -> bool {
        self.0 == other.0
    }
}

impl Eq for Float {}

impl Hash for Float {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let normalized = if self.0 == 0.0 { 0.0f64 } else { self.0 };
        state.write_u64(normalized.to_bits());
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(Float),
    String(Arc<str>),
    Bytes(Arc<[u8]>),
    Unit,
    List(Arc<[Value]>),
    Tuple(Arc<[Value]>),
    Record(RecordValue),
    Map(MapValue),
    Some(Arc<Value>),
    None,
    Ok(Arc<Value>),
    Err(Arc<Value>),
    Variant(Arc<Variant>),
}

impl Value {
    pub fn float(value: f64) -> Value {
        Value::Float(Float(value))
    }

    pub fn string(text: &str) -> Value {
        Value::String(Arc::from(text))
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(items.into())
    }

    pub fn tuple(items: Vec<Value>) -> Value {
        Value::Tuple(items.into())
    }

    pub fn some(value: Value) -> Value {
        Value::Some(Arc::new(value))
    }

    pub fn variant(enum_name: &str, variant: &str, payload: Option<Value>) -> Value {
        Value::Variant(Arc::new(Variant { enum_name: Arc::from(enum_name), variant: Arc::from(variant), payload }))
    }
}

pub fn call_error_value(kind: CallErrorKind, message: &str) -> RecordValue {
    RecordValue::new(vec![
        (Arc::from("kind"), Value::variant(CALL_ERROR_KIND_NAME, kind.name(), None)),
        (Arc::from("message"), Value::string(message)),
    ])
}

pub fn size(value: &Value) -> u64 {
    match value {
        Value::String(s) => 1 + s.chars().count() as u64 / 32,
        Value::Bytes(b) => 1 + b.len() as u64 / 32,
        Value::List(items) | Value::Tuple(items) => 1 + items.iter().map(size).sum::<u64>(),
        Value::Record(record) => 1 + record.fields().iter().map(|(_, v)| size(v)).sum::<u64>(),
        Value::Map(map) => 1 + map.entries().iter().map(|(_, v)| size(v)).sum::<u64>(),
        Value::Some(inner) | Value::Ok(inner) | Value::Err(inner) => 1 + size(inner),
        Value::Variant(variant) => 1 + variant.payload.as_ref().map_or(0, size),
        _ => 1,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError {
    pub path: String,
    pub expected: String,
    pub found: String,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: expected {}, found {}", self.path, self.expected, self.found)
    }
}

impl std::error::Error for DecodeError {}

fn json_kind(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "a boolean",
        Json::Int(_) | Json::BigInt(_) => "an integer",
        Json::Float(_) => "a number with a fraction",
        Json::String(_) => "a string",
        Json::Array(_) => "an array",
        Json::Object(_) => "an object",
    }
}

fn error(path: &str, expected: impl Into<String>, found: impl Into<String>) -> DecodeError {
    DecodeError { path: path.to_string(), expected: expected.into(), found: found.into() }
}

fn only_key<'a>(entries: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    match entries {
        [(k, v)] if k == key => Some(v),
        _ => None,
    }
}

pub fn decode(value: &Json, t: &Type, path: &str) -> Result<Value, DecodeError> {
    let fail = |expected: String| error(path, expected, json_kind(value));
    match t {
        Type::Primitive(Primitive::Bool) => match value {
            Json::Bool(b) => Ok(Value::Bool(*b)),
            _ => Err(fail("Bool".to_string())),
        },
        Type::Primitive(Primitive::Int) => match value {
            Json::Int(n) => Ok(Value::Int(*n)),
            Json::BigInt(_) => Err(error(path, "Int", "an integer outside the signed 64-bit range")),
            _ => Err(fail("Int".to_string())),
        },
        Type::Primitive(Primitive::Float) => {
            let number = match value {
                Json::Int(n) => *n as f64,
                Json::BigInt(text) => text.parse::<f64>().unwrap_or(f64::INFINITY),
                Json::Float(f) => *f,
                _ => return Err(fail("Float".to_string())),
            };
            if !number.is_finite() {
                return Err(error(path, "Float", "a number outside the finite 64-bit Float range"));
            }
            Ok(Value::float(number))
        }
        Type::Primitive(Primitive::String) => match value {
            Json::String(s) => Ok(Value::string(s)),
            _ => Err(fail("String".to_string())),
        },
        Type::Primitive(Primitive::Bytes) => {
            let Json::String(text) = value else {
                return Err(fail("Bytes (a base64 string)".to_string()));
            };
            match base64_decode(text) {
                Some(raw) if base64_encode(&raw) == *text => Ok(Value::Bytes(raw.into())),
                _ => Err(error(path, "Bytes (standard base64 with padding)", "a string that is not valid base64")),
            }
        }
        Type::Primitive(Primitive::Unit) => match value {
            Json::Null => Ok(Value::Unit),
            _ => Err(fail("Unit (null)".to_string())),
        },
        Type::List(element) => {
            let Json::Array(items) = value else {
                return Err(fail(show_type(t)));
            };
            let decoded = items
                .iter()
                .enumerate()
                .map(|(i, item)| decode(item, element, &format!("{path}[{i}]")))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::list(decoded))
        }
        Type::Tuple(types) => match value {
            Json::Array(items) if items.len() == types.len() => {
                let decoded = items
                    .iter()
                    .zip(types)
                    .enumerate()
                    .map(|(i, (item, it))| decode(item, it, &format!("{path}[{i}]")))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::tuple(decoded))
            }
            _ => Err(fail(format!("{} as an array of {}", show_type(t), types.len()))),
        },
        Type::Option(inner) => match value {
            Json::Null => Ok(Value::None),
            _ => Ok(Value::some(decode(value, inner, path)?)),
        },
        Type::Result(ok, err) => {
            if let Json::Object(entries) = value {
                if let Some(inner) = only_key(entries, "ok") {
                    return Ok(Value::Ok(Arc::new(decode(inner, ok, &format!("{path}.ok"))?)));
                }
                if let Some(inner) = only_key(entries, "err") {
                    return Ok(Value::Err(Arc::new(decode(inner, err, &format!("{path}.err"))?)));
                }
            }
            Err(fail("{\"ok\": ...} or {\"err\": ...}".to_string()))
        }
        Type::Map(value_type) => {
            let Json::Object(entries) = value else {
                return Err(fail(show_type(t)));
            };
            let mut sorted: Vec<&(String, Json)> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let mut decoded = Vec::with_capacity(sorted.len());
            for (key, item) in sorted {
                let quoted = dumps_str(key).map_err(|e| error(path, "a JSON-encodable key", e.0))?;
                decoded.push((Arc::from(key.as_str()), decode(item, value_type, &format!("{path}[{quoted}]"))?));
            }
            Ok(Value::Map(MapValue::from_sorted(decoded)))
        }
        Type::Record(record) => {
            let Json::Object(entries) = value else {
                return Err(fail(show_type(t)));
            };
            let lookup = |name: &str| entries.iter().find(|(k, _)| k == name).map(|(_, v)| v);
            if let Some((name, field_type)) = record.fields.iter().find(|(name, _)| lookup(name).is_none()) {
                return Err(error(&format!("{path}.{name}"), show_type(field_type), "no field"));
            }
            if !record.open {
                let extra = entries
                    .iter()
                    .map(|(k, _)| k)
                    .filter(|k| record.field(k).is_none())
                    .min();
                if let Some(extra) = extra {
                    return Err(error(
                        &format!("{path}.{extra}"),
                        "no field, because the record type does not declare it",
                        "a field",
                    ));
                }
            }
            let mut fields = Vec::with_capacity(record.fields.len());
            for (name, field_type) in &record.fields {
                let Some(item) = lookup(name) else {
                    return Err(error(&format!("{path}.{name}"), show_type(field_type), "no field"));
                };
                fields.push((Arc::from(name.as_str()), decode(item, field_type, &format!("{path}.{name}"))?));
            }
            Ok(Value::Record(RecordValue::new(fields)))
        }
        Type::Enum(e) => {
            if let Json::String(text) = value
                && let Some((variant, None)) = e.variants.iter().find(|(v, _)| v == text)
            {
                return Ok(Value::variant(&e.name, variant, None));
            }
            if let Json::Object(entries) = value
                && entries.len() == 2
                && let (Some(tag), Some(inner)) = (
                    entries.iter().find(|(k, _)| k == "tag").map(|(_, v)| v),
                    entries.iter().find(|(k, _)| k == "value").map(|(_, v)| v),
                )
                && let Json::String(tag) = tag
                && let Some((variant, Some(payload))) = e.variants.iter().find(|(v, _)| v == tag)
            {
                let decoded = decode(inner, payload, &format!("{path}.value"))?;
                return Ok(Value::variant(&e.name, variant, Some(decoded)));
            }
            let choices: Vec<String> = e
                .variants
                .iter()
                .map(|(v, p)| if p.is_none() { v.clone() } else { format!("{{tag: {v}}}") })
                .collect();
            Err(fail(format!("{} ({})", e.name, choices.join(" | "))))
        }
        Type::Nominal(nominal) => match &nominal.validation {
            None => decode(value, &nominal.base, path),
            Some(validation) => {
                let Json::String(text) = value else {
                    return Err(fail(format!("{} (a string)", nominal.name)));
                };
                match (validation.canonicalize)(text) {
                    Ok(canonical) => Ok(Value::string(&canonical)),
                    Err(problem) => Err(error(path, nominal.name.as_str(), format!("a string that is not valid: {problem}"))),
                }
            }
        },
        Type::Variable(_) => Err(error(path, "a known type", "a type the checker could not resolve")),
        Type::Never => Err(fail(show_type(t))),
    }
}

pub fn encode(value: &Value) -> Json {
    match value {
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(n) => Json::Int(*n),
        Value::Float(f) => Json::Float(f.0),
        Value::String(s) => Json::String(s.to_string()),
        Value::Bytes(b) => Json::String(base64_encode(b)),
        Value::Unit | Value::None => Json::Null,
        Value::List(items) | Value::Tuple(items) => Json::Array(items.iter().map(encode).collect()),
        Value::Some(inner) => encode(inner),
        Value::Ok(inner) => Json::object([("ok", encode(inner))]),
        Value::Err(inner) => Json::object([("err", encode(inner))]),
        Value::Map(map) => Json::Object(map.entries().iter().map(|(k, v)| (k.to_string(), encode(v))).collect()),
        Value::Record(record) => {
            Json::Object(record.fields().iter().map(|(name, v)| (name.to_string(), encode(v))).collect())
        }
        Value::Variant(variant) => match &variant.payload {
            None => Json::String(variant.variant.to_string()),
            Some(payload) if variant.enum_name.starts_with(SHAPED_PREFIX) => encode(payload),
            Some(payload) => {
                Json::object([("tag", Json::String(variant.variant.to_string())), ("value", encode(payload))])
            }
        },
    }
}

pub const SHAPED_PREFIX: &str = "js.Shaped<";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NonFiniteFloat(pub f64);

pub fn json_text(value: &Value) -> Result<String, NonFiniteFloat> {
    let mut out = String::new();
    write_json_text(&encode(value), &mut out)?;
    Ok(out)
}

fn write_json_text(json: &Json, out: &mut String) -> Result<(), NonFiniteFloat> {
    match json {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(n) => out.push_str(&n.to_string()),
        Json::BigInt(digits) => out.push_str(digits),
        Json::Float(f) if f.is_finite() => out.push_str(&float_text(*f)),
        Json::Float(f) => return Err(NonFiniteFloat(*f)),
        Json::String(text) => write_json_string(text, out),
        Json::Array(items) => {
            out.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                write_json_text(item, out)?;
            }
            out.push(']');
        }
        Json::Object(entries) => {
            let mut ordered: Vec<&(String, Json)> = entries.iter().collect();
            ordered.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (at, (key, item)) in ordered.into_iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                write_json_string(key, out);
                out.push(':');
                write_json_text(item, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn write_json_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn show_value(value: &Value) -> String {
    match value {
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => python_float_repr(f.0),
        Value::String(s) => {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        Value::Bytes(b) => format!("<bytes \"{}\">", base64_encode(b)),
        Value::Unit => "unit".to_string(),
        Value::List(items) | Value::Tuple(items) => {
            format!("[{}]", items.iter().map(show_value).collect::<Vec<_>>().join(" "))
        }
        Value::Record(record) => format!(
            "{{{}}}",
            record.fields().iter().map(|(n, v)| format!(":{n} {}", show_value(v))).collect::<Vec<_>>().join(" ")
        ),
        Value::Map(map) => {
            let pairs: Vec<String> = map
                .entries()
                .iter()
                .map(|(k, v)| format!("[{} {}]", show_value(&Value::String(k.clone())), show_value(v)))
                .collect();
            format!("(into-map [{}])", pairs.join(" "))
        }
        Value::Some(inner) => format!("(some {})", show_value(inner)),
        Value::None => "none".to_string(),
        Value::Ok(inner) => format!("(ok {})", show_value(inner)),
        Value::Err(inner) => format!("(err {})", show_value(inner)),
        Value::Variant(variant) => match &variant.payload {
            None => variant.variant.to_string(),
            Some(payload) => format!("({} {})", variant.variant, show_value(payload)),
        },
    }
}

pub fn float_text(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let (digits, point) = shortest_digits(value);
    let coefficient = digits.trim_end_matches('0');
    let coefficient = if coefficient.is_empty() { "0" } else { coefficient };
    let count = coefficient.len() as i32;
    let sign = if value < 0.0 { "-" } else { "" };
    let body = if count <= point && point <= 21 {
        format!("{coefficient}{}", "0".repeat((point - count) as usize))
    } else if 0 < point && point <= 21 {
        format!("{}.{}", &coefficient[..point as usize], &coefficient[point as usize..])
    } else if -6 < point && point <= 0 {
        format!("0.{}{coefficient}", "0".repeat((-point) as usize))
    } else {
        let shown = point - 1;
        let mantissa =
            if count > 1 { format!("{}.{}", &coefficient[..1], &coefficient[1..]) } else { coefficient.to_string() };
        format!("{mantissa}e{}{}", if shown >= 0 { '+' } else { '-' }, shown.abs())
    };
    format!("{sign}{body}")
}

const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let symbols = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        for (i, symbol) in symbols.iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(BASE64_ALPHABET[*symbol as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base64_value(byte: u8) -> Option<u32> {
    BASE64_ALPHABET.iter().position(|&a| a == byte).map(|p| p as u32)
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let quads = bytes.len() / 4;
    for (index, quad) in bytes.chunks(4).enumerate() {
        let padding = quad.iter().rev().take_while(|&&b| b == b'=').count();
        if padding > 2 || (padding > 0 && index + 1 != quads) {
            return None;
        }
        let mut n: u32 = 0;
        for &b in &quad[..4 - padding] {
            n = (n << 6) | base64_value(b)?;
        }
        n <<= 6 * padding as u32;
        let decoded = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&decoded[..3 - padding]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_number_text() {
        let cases = [
            (1.0, "1"),
            (-0.0, "0"),
            (1.5, "1.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (123.456, "123.456"),
            (-2.5e-10, "-2.5e-10"),
        ];
        for (value, text) in cases {
            assert_eq!(float_text(value), text);
        }
    }

    #[test]
    fn json_text_writes_a_payload_variant_and_a_failed_result_in_key_order() {
        let payload = Value::variant("Branch", "OnBranch", Some(Value::string("main")));
        assert_eq!(json_text(&payload).as_deref(), Ok("{\"tag\":\"OnBranch\",\"value\":\"main\"}"));
        let failure = Value::Err(Arc::new(Value::Record(call_error_value(CallErrorKind::Timeout, "slow"))));
        assert_eq!(json_text(&failure).as_deref(), Ok("{\"err\":{\"kind\":\"Timeout\",\"message\":\"slow\"}}"));
        let unordered = Value::Record(RecordValue::new(vec![(Arc::from("b"), Value::Int(1)), (Arc::from("a"), Value::Int(2))]));
        assert_eq!(json_text(&unordered).as_deref(), Ok("{\"a\":2,\"b\":1}"));
    }

    #[test]
    fn json_text_refuses_a_float_that_is_not_finite() {
        assert_eq!(json_text(&Value::float(f64::INFINITY)), Err(NonFiniteFloat(f64::INFINITY)));
    }

    #[test]
    fn base64_is_canonical() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", b"\xff\x00"] {
            assert_eq!(base64_decode(&base64_encode(data)).as_deref(), Some(data));
        }
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert!(base64_decode("Zm8").is_none());
        assert!(base64_decode("Zm9=").is_some_and(|raw| base64_encode(&raw) != "Zm9="));
        assert!(base64_decode("Zm-=").is_none());
    }
}
