use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::json::repr_str;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Primitive {
    Bool,
    Int,
    Float,
    Bytes,
    String,
    Unit,
}

impl Primitive {
    pub const ALL: [Primitive; 6] = [
        Primitive::Bool,
        Primitive::Int,
        Primitive::Float,
        Primitive::Bytes,
        Primitive::String,
        Primitive::Unit,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Primitive::Bool => "Bool",
            Primitive::Int => "Int",
            Primitive::Float => "Float",
            Primitive::Bytes => "Bytes",
            Primitive::String => "String",
            Primitive::Unit => "Unit",
        }
    }

    pub fn from_name(name: &str) -> Option<Primitive> {
        Primitive::ALL.into_iter().find(|p| p.name() == name)
    }
}

#[derive(Clone, Debug)]
pub struct RecordType {
    pub fields: Vec<(String, Type)>,
    pub name: Option<String>,
    pub open: bool,
}

impl PartialEq for RecordType {
    fn eq(&self, other: &RecordType) -> bool {
        self.fields == other.fields && self.open == other.open
    }
}

impl RecordType {
    pub fn plain(fields: Vec<(String, Type)>) -> RecordType {
        RecordType { fields, name: None, open: false }
    }

    pub fn field(&self, name: &str) -> Option<&Type> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumType {
    pub name: String,
    pub variants: Vec<(String, Option<Type>)>,
}

impl EnumType {
    pub fn variant(&self, name: &str) -> Option<&Option<Type>> {
        self.variants.iter().find(|(n, _)| n == name).map(|(_, p)| p)
    }

    pub fn has(&self, name: &str) -> bool {
        self.variant(name).is_some()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Primitive(Primitive),
    List(Box<Type>),
    Map(Box<Type>),
    Tuple(Vec<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    Record(RecordType),
    Enum(Arc<EnumType>),
    Nominal { name: String, base: Box<Type> },
    Variable(u64),
    Never,
}

pub const BOOL: Type = Type::Primitive(Primitive::Bool);
pub const INT: Type = Type::Primitive(Primitive::Int);
pub const FLOAT: Type = Type::Primitive(Primitive::Float);
pub const BYTES: Type = Type::Primitive(Primitive::Bytes);
pub const STRING: Type = Type::Primitive(Primitive::String);
pub const UNIT: Type = Type::Primitive(Primitive::Unit);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CallErrorKind {
    Timeout,
    Unavailable,
    RateLimited,
    NotFound,
    PermissionDenied,
    Invalid,
    Conflict,
    Failed,
}

impl CallErrorKind {
    pub const ALL: [CallErrorKind; 8] = [
        CallErrorKind::Timeout,
        CallErrorKind::Unavailable,
        CallErrorKind::RateLimited,
        CallErrorKind::NotFound,
        CallErrorKind::PermissionDenied,
        CallErrorKind::Invalid,
        CallErrorKind::Conflict,
        CallErrorKind::Failed,
    ];
    pub const TRANSIENT: [CallErrorKind; 3] =
        [CallErrorKind::Timeout, CallErrorKind::Unavailable, CallErrorKind::RateLimited];
    pub const REFUSAL: [CallErrorKind; 5] = [
        CallErrorKind::RateLimited,
        CallErrorKind::NotFound,
        CallErrorKind::PermissionDenied,
        CallErrorKind::Invalid,
        CallErrorKind::Conflict,
    ];

    pub fn name(self) -> &'static str {
        match self {
            CallErrorKind::Timeout => "Timeout",
            CallErrorKind::Unavailable => "Unavailable",
            CallErrorKind::RateLimited => "RateLimited",
            CallErrorKind::NotFound => "NotFound",
            CallErrorKind::PermissionDenied => "PermissionDenied",
            CallErrorKind::Invalid => "Invalid",
            CallErrorKind::Conflict => "Conflict",
            CallErrorKind::Failed => "Failed",
        }
    }

    pub fn from_name(name: &str) -> Option<CallErrorKind> {
        CallErrorKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

pub const CALL_ERROR_KIND_NAME: &str = "CallErrorKind";

pub fn call_error_kind() -> Arc<EnumType> {
    Arc::new(EnumType {
        name: CALL_ERROR_KIND_NAME.to_string(),
        variants: CallErrorKind::ALL.iter().map(|k| (k.name().to_string(), None)).collect(),
    })
}

pub fn call_error() -> Type {
    Type::Record(RecordType::plain(vec![
        ("kind".to_string(), Type::Enum(call_error_kind())),
        ("message".to_string(), STRING),
    ]))
}

impl Type {
    pub fn is_call_error(&self) -> bool {
        match self {
            Type::Record(record) => {
                !record.open
                    && record.fields.len() == 2
                    && record.fields[0].0 == "kind"
                    && record.fields[1].0 == "message"
                    && record.fields[1].1 == STRING
                    && matches!(&record.fields[0].1, Type::Enum(e) if **e == *call_error_kind())
            }
            _ => false,
        }
    }
}

pub fn show_type(t: &Type) -> String {
    match t {
        Type::Primitive(p) => p.name().to_string(),
        Type::List(element) => format!("List<{}>", show_type(element)),
        Type::Map(value) => format!("Map<String, {}>", show_type(value)),
        Type::Tuple(items) => format!("Tuple<{}>", items.iter().map(show_type).collect::<Vec<_>>().join(", ")),
        Type::Option(inner) => format!("Option<{}>", show_type(inner)),
        Type::Result(ok, err) => {
            let err_text = if err.is_call_error() { "CallError".to_string() } else { show_type(err) };
            format!("Result<{}, {}>", show_type(ok), err_text)
        }
        Type::Record(record) => {
            if t.is_call_error() {
                return "CallError".to_string();
            }
            match &record.name {
                Some(name) => name.clone(),
                None => show_definition(t),
            }
        }
        Type::Enum(e) => e.name.clone(),
        Type::Nominal { name, .. } => name.clone(),
        Type::Variable(_) => "?".to_string(),
        Type::Never => "Never".to_string(),
    }
}

pub fn show_definition(t: &Type) -> String {
    match t {
        Type::Record(record) => {
            let marker = if record.open { "open " } else { "" };
            let fields: Vec<String> = record.fields.iter().map(|(n, ft)| format!("{n}: {}", show_type(ft))).collect();
            format!("Record {marker}{{{}}}", fields.join(", "))
        }
        _ => show_type(t),
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&show_type(self))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeSyntaxError(pub String);

impl fmt::Display for TypeSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TypeSyntaxError {}

fn tokenize(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        while at < chars.len() && crate::unicode::is_space(chars[at]) {
            at += 1;
        }
        if at >= chars.len() {
            break;
        }
        let c = chars[at];
        if c.is_ascii_alphabetic() {
            let start = at;
            at += 1;
            while at < chars.len()
                && (chars[at].is_ascii_alphanumeric() || matches!(chars[at], '_' | '.' | '-'))
            {
                at += 1;
            }
            tokens.push(chars[start..at].iter().collect());
        } else {
            tokens.push(c.to_string());
            at += 1;
        }
    }
    tokens
}

pub const TYPE_NESTING_LIMIT: usize = 1000;

pub fn type_depth(t: &Type) -> usize {
    1 + match t {
        Type::List(inner) | Type::Map(inner) | Type::Option(inner) | Type::Nominal { base: inner, .. } => type_depth(inner),
        Type::Tuple(items) => items.iter().map(type_depth).max().unwrap_or(0),
        Type::Result(ok, err) => type_depth(ok).max(type_depth(err)),
        Type::Record(record) => record.fields.iter().map(|(_, ft)| type_depth(ft)).max().unwrap_or(0),
        Type::Enum(_) | Type::Primitive(_) | Type::Variable(_) | Type::Never => 0,
    }
}

pub fn parse_type(text: &str, named: &BTreeMap<String, Type>) -> Result<Type, TypeSyntaxError> {
    let tokens = tokenize(text);
    let mut parser = TypeParser { tokens: &tokens, named, text, at: 0, depth: 0 };
    let result = parser.parse()?;
    if parser.at < tokens.len() {
        return Err(TypeSyntaxError(format!(
            "unexpected {} in type {}",
            repr_str(&tokens[parser.at]),
            repr_str(text)
        )));
    }
    Ok(result)
}

struct TypeParser<'a> {
    tokens: &'a [String],
    named: &'a BTreeMap<String, Type>,
    text: &'a str,
    at: usize,
    depth: usize,
}

impl TypeParser<'_> {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.at).map(String::as_str)
    }

    fn take(&mut self, expected: Option<&str>) -> Result<String, TypeSyntaxError> {
        let Some(token) = self.peek() else {
            return Err(TypeSyntaxError(format!("type {} ends early", repr_str(self.text))));
        };
        if let Some(expected) = expected
            && token != expected
        {
            return Err(TypeSyntaxError(format!(
                "expected {}, found {} in type {}",
                repr_str(expected),
                repr_str(token),
                repr_str(self.text)
            )));
        }
        let token = token.to_string();
        self.at += 1;
        Ok(token)
    }

    fn arguments(&mut self, count: Option<usize>, name: &str) -> Result<Vec<Type>, TypeSyntaxError> {
        self.take(Some("<"))?;
        let mut args = vec![self.parse()?];
        while self.peek() == Some(",") {
            self.take(Some(","))?;
            args.push(self.parse()?);
        }
        self.take(Some(">"))?;
        if let Some(count) = count
            && args.len() != count
        {
            let plural = if count != 1 { "s" } else { "" };
            return Err(TypeSyntaxError(format!(
                "{name} takes {count} type argument{plural} in {}",
                repr_str(self.text)
            )));
        }
        Ok(args)
    }

    fn parse(&mut self) -> Result<Type, TypeSyntaxError> {
        if self.depth >= TYPE_NESTING_LIMIT {
            return Err(TypeSyntaxError(format!("a type nests more than {TYPE_NESTING_LIMIT} levels deep")));
        }
        self.depth += 1;
        let parsed = self.parse_one();
        self.depth -= 1;
        parsed
    }

    fn parse_one(&mut self) -> Result<Type, TypeSyntaxError> {
        let name = self.take(None)?;
        if let Some(p) = Primitive::from_name(&name) {
            return Ok(Type::Primitive(p));
        }
        match name.as_str() {
            "CallError" => return Ok(call_error()),
            "List" => {
                let mut args = self.arguments(Some(1), &name)?;
                return Ok(Type::List(Box::new(args.remove(0))));
            }
            "Map" => {
                let mut args = self.arguments(Some(2), &name)?;
                let value = args.remove(1);
                if args[0] != STRING {
                    return Err(TypeSyntaxError(format!(
                        "Map keys must be String, because JSON object keys are strings: {}",
                        repr_str(self.text)
                    )));
                }
                return Ok(Type::Map(Box::new(value)));
            }
            "Option" => {
                let mut args = self.arguments(Some(1), &name)?;
                return Ok(Type::Option(Box::new(args.remove(0))));
            }
            "Result" => {
                let mut args = self.arguments(Some(2), &name)?;
                let err = args.remove(1);
                let ok = args.remove(0);
                return Ok(Type::Result(Box::new(ok), Box::new(err)));
            }
            "Tuple" => return Ok(Type::Tuple(self.arguments(None, &name)?)),
            "Record" => return self.record(),
            _ => {}
        }
        if let Some(t) = self.named.get(&name) {
            return Ok(t.clone());
        }
        Err(TypeSyntaxError(format!("unknown type {} in {}", repr_str(&name), repr_str(self.text))))
    }

    fn record(&mut self) -> Result<Type, TypeSyntaxError> {
        let open = self.peek() == Some("open");
        if open {
            self.take(Some("open"))?;
        }
        self.take(Some("{"))?;
        let mut fields: Vec<(String, Type)> = Vec::new();
        while self.peek() != Some("}") {
            let field_name = self.take(None)?;
            self.take(Some(":"))?;
            if fields.iter().any(|(existing, _)| *existing == field_name) {
                return Err(TypeSyntaxError(format!(
                    "field {} appears twice in {}",
                    repr_str(&field_name),
                    repr_str(self.text)
                )));
            }
            let field_type = self.parse()?;
            fields.push((field_name, field_type));
            if self.peek() == Some(",") {
                self.take(Some(","))?;
            }
        }
        self.take(Some("}"))?;
        Ok(Type::Record(RecordType { fields, name: None, open }))
    }
}

pub fn encodable_problem(t: &Type) -> Option<String> {
    match t {
        Type::Option(inner) => {
            if matches!(**inner, Type::Option(_)) || **inner == UNIT {
                return Some(format!(
                    "{} has no JSON encoding, because none and (some x) would both be null",
                    show_type(t)
                ));
            }
            encodable_problem(inner)
        }
        Type::List(element) => encodable_problem(element),
        Type::Map(value) => {
            if matches!(**value, Type::Option(_)) {
                return Some(format!(
                    "{} has no JSON encoding, because a null value and a missing key would collide",
                    show_type(t)
                ));
            }
            encodable_problem(value)
        }
        Type::Tuple(items) => items.iter().find_map(encodable_problem),
        Type::Result(ok, err) => encodable_problem(ok).or_else(|| encodable_problem(err)),
        Type::Record(record) => record.fields.iter().find_map(|(_, ft)| encodable_problem(ft)),
        Type::Enum(e) => e.variants.iter().find_map(|(_, payload)| payload.as_ref().and_then(encodable_problem)),
        Type::Nominal { base, .. } => encodable_problem(base),
        _ => None,
    }
}
