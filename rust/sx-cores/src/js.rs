use std::collections::BTreeMap;
use std::fmt::Write as _;

use sx_kernel::json::{Json, parse, shortest_digits};
use sx_kernel::types::parse_type;
use sx_kernel::values::{Value, decode};

pub const SAFE_INT_LIMIT: i64 = 9_007_199_254_740_992;
const PAD_LIMIT: i64 = 1 << 22;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsError {
    Trap(String),
    Refused(String),
}

fn trap<T>(message: &str) -> Result<T, JsError> {
    Err(JsError::Trap(message.to_string()))
}

fn refused<T>(message: impl Into<String>) -> Result<T, JsError> {
    Err(JsError::Refused(message.into()))
}

fn units(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

fn count(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

pub fn length(text: &str) -> i64 {
    count(units(text))
}

fn byte_at(text: &str, unit: usize) -> Result<usize, JsError> {
    let mut passed = 0;
    for (byte, c) in text.char_indices() {
        if passed == unit {
            return Ok(byte);
        }
        if passed > unit {
            return trap("A string index falls inside a surrogate pair.");
        }
        passed += c.len_utf16();
    }
    if passed == unit {
        return Ok(text.len());
    }
    if passed > unit { trap("A string index falls inside a surrogate pair.") } else { Ok(text.len()) }
}

fn relative(index: i64, length: i64) -> i64 {
    if index < 0 { (length + index).max(0) } else { index.min(length) }
}

pub fn slice(text: &str, start: i64, end: i64) -> Result<String, JsError> {
    let size = length(text);
    let (from, to) = (relative(start, size), relative(end, size));
    if from >= to {
        return Ok(String::new());
    }
    let from = byte_at(text, usize::try_from(from).unwrap_or(0))?;
    let to = byte_at(text, usize::try_from(to).unwrap_or(0))?;
    Ok(text.get(from..to).unwrap_or_default().to_string())
}

pub fn index_of(text: &str, needle: &str) -> i64 {
    match text.find(needle) {
        Some(byte) => count(units(text.get(..byte).unwrap_or_default())),
        None => -1,
    }
}

fn is_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

pub fn trim(text: &str) -> String {
    text.trim_matches(is_space).to_string()
}

pub fn to_lower(text: &str) -> String {
    text.to_lowercase()
}

pub fn to_upper(text: &str) -> String {
    text.to_uppercase()
}

pub fn join(separator: &str, parts: &[&str]) -> String {
    parts.join(separator)
}

fn padding(text: &str, target: i64, filler: &str) -> Result<String, JsError> {
    let size = length(text);
    if target <= size || filler.is_empty() {
        return Ok(String::new());
    }
    let wanted = target - size;
    if wanted > PAD_LIMIT {
        return refused("The padded length is more than the pad limit.");
    }
    let width = count(units(filler));
    let mut fill = filler.repeat(usize::try_from(wanted / width).unwrap_or(0));
    let rest = wanted % width;
    if rest > 0 {
        fill.push_str(&slice(filler, 0, rest)?);
    }
    Ok(fill)
}

pub fn pad_start(text: &str, target: i64, filler: &str) -> Result<String, JsError> {
    Ok(padding(text, target, filler)? + text)
}

pub fn pad_end(text: &str, target: i64, filler: &str) -> Result<String, JsError> {
    Ok(text.to_string() + &padding(text, target, filler)?)
}

pub fn replace_first(text: &str, old: &str, new: &str) -> Result<String, JsError> {
    if new.contains('$') {
        return refused("The replacement contains $, which JavaScript reads as a pattern.");
    }
    Ok(text.replacen(old, new, 1))
}

pub fn parse_int(text: &str) -> Result<i64, JsError> {
    let trimmed = text.trim_start_matches(is_space);
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    let digits = digits.get(..end).unwrap_or_default();
    if digits.is_empty() {
        return trap("parseInt found no digits.");
    }
    let Ok(magnitude) = digits.parse::<i64>() else {
        return trap("parseInt read an integer beyond plus or minus 2^53.");
    };
    if magnitude > SAFE_INT_LIMIT {
        return trap("parseInt read an integer beyond plus or minus 2^53.");
    }
    Ok(if negative { -magnitude } else { magnitude })
}

pub fn is_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

pub fn float_text(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let (digits, point) = shortest_digits(value);
    let sign = if value < 0.0 { "-" } else { "" };
    let count = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let body = if count <= point && point <= 21 {
        format!("{digits}{}", "0".repeat(usize::try_from(point - count).unwrap_or(0)))
    } else if 0 < point && point <= 21 {
        let at = usize::try_from(point).unwrap_or(0);
        format!("{}.{}", digits.get(..at).unwrap_or_default(), digits.get(at..).unwrap_or_default())
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat(usize::try_from(-point).unwrap_or(0)))
    } else {
        let exponent = point - 1;
        let sign_of_exponent = if exponent < 0 { '-' } else { '+' };
        let mantissa = match digits.split_at_checked(1) {
            Some((first, rest)) if !rest.is_empty() => format!("{first}.{rest}"),
            _ => digits.clone(),
        };
        format!("{mantissa}e{sign_of_exponent}{}", exponent.abs())
    };
    format!("{sign}{body}")
}

pub fn order_key(text: &str) -> String {
    text.chars()
        .map(|c| match u32::from(c) {
            0x10000..=0x10ffff => char::from_u32(u32::from(c) - 0x2000).unwrap_or(c),
            0xe000..=0xffff => char::from_u32(u32::from(c) + 0x100000).unwrap_or(c),
            _ => c,
        })
        .collect()
}

fn write_string(text: &str, out: &mut String) {
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
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Writer {
    indent: usize,
    out: String,
}

impl Writer {
    fn line(&mut self, depth: usize) {
        if self.indent > 0 {
            self.out.push('\n');
            self.out.push_str(&" ".repeat(self.indent * depth));
        }
    }

    fn sequence<I: IntoIterator>(
        &mut self,
        depth: usize,
        brackets: (char, char),
        items: I,
        mut write: impl FnMut(&mut Writer, I::Item, usize) -> Result<(), JsError>,
    ) -> Result<(), JsError> {
        let mut any = false;
        self.out.push(brackets.0);
        for item in items {
            if any {
                self.out.push(',');
            }
            any = true;
            self.line(depth + 1);
            write(self, item, depth + 1)?;
        }
        if any {
            self.line(depth);
        }
        self.out.push(brackets.1);
        Ok(())
    }

    fn entries(&mut self, depth: usize, pairs: Vec<(&str, &Value)>) -> Result<(), JsError> {
        self.sequence(depth, ('{', '}'), pairs, |writer, (key, value), depth| {
            write_string(key, &mut writer.out);
            writer.out.push(':');
            if writer.indent > 0 {
                writer.out.push(' ');
            }
            writer.value(value, depth)
        })
    }

    fn tagged(&mut self, depth: usize, pairs: Vec<(&str, Value)>) -> Result<(), JsError> {
        let borrowed: Vec<(&str, &Value)> = pairs.iter().map(|(key, value)| (*key, value)).collect();
        self.entries(depth, borrowed)
    }

    fn value(&mut self, value: &Value, depth: usize) -> Result<(), JsError> {
        match value {
            Value::Bool(b) => self.out.push_str(if *b { "true" } else { "false" }),
            Value::Int(n) => self.out.push_str(&n.to_string()),
            Value::Float(f) => self.out.push_str(&float_text(f.0)),
            Value::String(s) => write_string(s, &mut self.out),
            Value::Unit | Value::None => self.out.push_str("null"),
            Value::Some(inner) => self.value(inner, depth)?,
            Value::List(items) | Value::Tuple(items) => {
                self.sequence(depth, ('[', ']'), items.iter(), |writer, item, depth| writer.value(item, depth))?;
            }
            Value::Record(record) => {
                let pairs = record.fields().iter().map(|(name, field)| (&**name, field)).collect();
                self.entries(depth, pairs)?;
            }
            Value::Map(map) => {
                let pairs = map.entries().iter().map(|(name, field)| (&**name, field)).collect();
                self.entries(depth, pairs)?;
            }
            Value::Ok(inner) => {
                self.tagged(depth, vec![("status", Value::string("fulfilled")), ("value", (**inner).clone())])?;
            }
            Value::Err(inner) => {
                self.tagged(depth, vec![("status", Value::string("rejected")), ("reason", (**inner).clone())])?;
            }
            Value::Variant(variant) if variant.enum_name.starts_with(sx_kernel::values::SHAPED_PREFIX) => match &variant.payload {
                Some(payload) => self.value(payload, depth)?,
                None => return refused("A js.Shaped value holds a record."),
            },
            Value::Variant(variant) => match &variant.payload {
                None => write_string(&variant.variant, &mut self.out),
                Some(payload) => {
                    self.tagged(depth, vec![("tag", Value::string(&variant.variant)), ("value", payload.clone())])?;
                }
            },
            Value::Bytes(_) => return refused("JSON.stringify met Bytes, which have no JavaScript counterpart."),
        }
        Ok(())
    }
}

pub fn stringify(value: &Value, indent: i64) -> Result<String, JsError> {
    let mut writer = Writer { indent: usize::try_from(indent.clamp(0, 10)).unwrap_or(0), out: String::new() };
    writer.value(value, 0)?;
    Ok(writer.out)
}

fn within_safe_range(value: &Value) -> Result<(), JsError> {
    match value {
        Value::Int(n) if n.abs() > SAFE_INT_LIMIT => trap("A decoded integer is beyond plus or minus 2^53."),
        Value::List(items) | Value::Tuple(items) => items.iter().try_for_each(within_safe_range),
        Value::Record(record) => record.fields().iter().try_for_each(|(_, field)| within_safe_range(field)),
        Value::Map(map) => map.entries().iter().try_for_each(|(_, field)| within_safe_range(field)),
        Value::Some(inner) | Value::Ok(inner) | Value::Err(inner) => within_safe_range(inner),
        Value::Variant(variant) => variant.payload.as_ref().map_or(Ok(()), within_safe_range),
        _ => Ok(()),
    }
}

pub fn parse_document(type_text: &str, text: &str) -> Result<Value, JsError> {
    let target = parse_type(type_text, &BTreeMap::new()).or_else(|e| refused(e.to_string()))?;
    let document: Json = parse(text).or_else(|e| refused(e.to_string()))?;
    let decoded = decode(&document, &target, "$").or_else(|e| refused(e.to_string()))?;
    within_safe_range(&decoded)?;
    Ok(decoded)
}
