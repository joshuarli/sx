use std::sync::Arc;

use sx_kernel::json::Json;
use sx_kernel::types::{BOOL, EnumType, FLOAT, INT, RecordType, STRING, Type, encodable_problem};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Input,
    Output,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skip(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct Mapped {
    pub ty: Type,
    pub enums: Vec<Arc<EnumType>>,
}

const NESTING_LIMIT: usize = 64;

const ANNOTATIONS: &[&str] = &[
    "title",
    "description",
    "default",
    "examples",
    "deprecated",
    "readOnly",
    "writeOnly",
    "$comment",
    "$schema",
    "$id",
    "$defs",
    "definitions",
    "minLength",
    "maxLength",
    "pattern",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "minItems",
    "maxItems",
    "uniqueItems",
    "minProperties",
    "maxProperties",
];

const STRUCTURE: &[&str] =
    &["type", "properties", "required", "additionalProperties", "items", "prefixItems", "enum", "anyOf", "$ref"];

fn skip<T>(reason: impl Into<String>) -> Result<T, Skip> {
    Err(Skip(reason.into()))
}

pub fn pascal(text: &str) -> String {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect()
}

fn is_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn check_keyword(key: &str, value: &Json) -> Result<(), Skip> {
    if ANNOTATIONS.contains(&key) || STRUCTURE.contains(&key) {
        return Ok(());
    }
    if key == "format" {
        return match value.as_str() {
            Some(format @ ("byte" | "binary")) => skip(format!("format {format} changes how a string is read")),
            _ => Ok(()),
        };
    }
    skip(format!("uses {key}, which has no Sx type"))
}

fn is_null_schema(schema: &Json) -> bool {
    let Json::Object(entries) = schema else {
        return false;
    };
    schema.get("type").and_then(Json::as_str) == Some("null")
        && entries.iter().all(|(key, _)| key == "type" || ANNOTATIONS.contains(&key.as_str()))
}

enum Additional<'j> {
    Open,
    Closed,
    Schema(&'j Json),
}

struct Mapper<'j> {
    root: &'j Json,
    side: Side,
    prefix: String,
    enums: Vec<Arc<EnumType>>,
    refs: Vec<String>,
    path: Vec<String>,
    depth: usize,
}

pub fn map(root: &Json, side: Side, prefix: &str) -> Result<Mapped, Skip> {
    let mut mapper =
        Mapper { root, side, prefix: prefix.to_string(), enums: Vec::new(), refs: Vec::new(), path: Vec::new(), depth: 0 };
    let ty = mapper.node(root)?;
    if !matches!(ty, Type::Record(_) | Type::Map(_)) {
        return skip("the schema does not describe an object");
    }
    if let Some(problem) = encodable_problem(&ty) {
        return skip(problem);
    }
    let mut seen: Vec<Arc<EnumType>> = Vec::new();
    for found in &mapper.enums {
        match seen.iter().find(|held| held.name == found.name) {
            Some(held) if **held != **found => {
                return skip(format!("two different enums would both be named {}", found.name));
            }
            Some(_) => {}
            None => seen.push(found.clone()),
        }
    }
    Ok(Mapped { ty, enums: seen })
}

impl<'j> Mapper<'j> {
    fn node(&mut self, schema: &'j Json) -> Result<Type, Skip> {
        if self.depth >= NESTING_LIMIT {
            return skip(format!("nests deeper than {NESTING_LIMIT} levels"));
        }
        self.depth += 1;
        let mapped = self.schema(schema);
        self.depth -= 1;
        mapped
    }

    fn child(&mut self, segment: String, schema: &'j Json) -> Result<Type, Skip> {
        self.path.push(segment);
        let mapped = self.node(schema);
        self.path.pop();
        mapped
    }

    fn schema(&mut self, schema: &'j Json) -> Result<Type, Skip> {
        let Json::Object(entries) = schema else {
            return skip(match schema {
                Json::Bool(_) => "a boolean schema has no Sx type",
                _ => "a schema is not an object",
            });
        };
        for (key, value) in entries {
            check_keyword(key, value)?;
        }
        if let Some(target) = schema.get("$ref") {
            if let Some((key, _)) = entries.iter().find(|(key, _)| key != "$ref" && STRUCTURE.contains(&key.as_str())) {
                return skip(format!("$ref beside {key}"));
            }
            return self.reference(target);
        }
        if let Some(options) = schema.get("anyOf") {
            if let Some((key, _)) = entries.iter().find(|(key, _)| key != "anyOf" && STRUCTURE.contains(&key.as_str())) {
                return skip(format!("anyOf beside {key}"));
            }
            return self.nullable(options);
        }
        let (kind, nullable) = kind_of(schema)?;
        if kind != "string" && schema.get("enum").is_some() {
            return skip("enum on a type other than string");
        }
        let ty = match kind {
            "string" => match schema.get("enum") {
                None => STRING,
                Some(values) => self.enumeration(values)?,
            },
            "integer" => INT,
            "number" => FLOAT,
            "boolean" => BOOL,
            "array" => self.array(schema)?,
            "object" => self.object(schema)?,
            other => return skip(format!("type {other} has no Sx type")),
        };
        Ok(if nullable { Type::Option(Box::new(ty)) } else { ty })
    }

    fn reference(&mut self, target: &Json) -> Result<Type, Skip> {
        let Some(pointer) = target.as_str() else {
            return skip("$ref is not a string");
        };
        let local = pointer
            .strip_prefix("#/$defs/")
            .map(|name| ("$defs", name))
            .or_else(|| pointer.strip_prefix("#/definitions/").map(|name| ("definitions", name)));
        let Some((section, name)) = local.filter(|(_, name)| !name.is_empty() && !name.contains(['/', '~', '%'])) else {
            return skip(format!("$ref {pointer} is not a local definition"));
        };
        if self.refs.iter().any(|held| held == pointer) {
            return skip(format!("$ref {pointer} is recursive"));
        }
        let Some(definition) = self.root.get(section).and_then(|defs| defs.get(name)) else {
            return skip(format!("$ref {pointer} names no definition"));
        };
        self.refs.push(pointer.to_string());
        let mapped = self.schema(definition);
        self.refs.pop();
        mapped
    }

    fn nullable(&mut self, options: &'j Json) -> Result<Type, Skip> {
        let Some([first, second]) = options.as_array() else {
            return skip("anyOf is not the nullable pattern of one schema and null");
        };
        let inner = match (is_null_schema(first), is_null_schema(second)) {
            (true, false) => second,
            (false, true) => first,
            _ => return skip("anyOf is not the nullable pattern of one schema and null"),
        };
        match self.schema(inner)? {
            Type::Option(_) => skip("a nullable schema is nullable twice"),
            ty => Ok(Type::Option(Box::new(ty))),
        }
    }

    fn enumeration(&mut self, values: &Json) -> Result<Type, Skip> {
        let Some(values) = values.as_array().filter(|values| !values.is_empty()) else {
            return skip("enum is not a non-empty list");
        };
        let mut variants: Vec<(String, Option<Type>)> = Vec::new();
        for value in values {
            let Some(text) = value.as_str().filter(|text| !text.is_empty()) else {
                return skip("enum holds a value that is not a non-empty string");
            };
            if variants.iter().any(|(held, _)| held == text) {
                return skip(format!("enum lists {text} twice"));
            }
            variants.push((text.to_string(), None));
        }
        let name = format!("{}{}", self.prefix, self.path.concat());
        let found = Arc::new(EnumType { name, variants });
        self.enums.push(found.clone());
        Ok(Type::Enum(found))
    }

    fn array(&mut self, schema: &'j Json) -> Result<Type, Skip> {
        match (schema.get("prefixItems"), schema.get("items")) {
            (Some(Json::Array(prefix)), Some(Json::Bool(false))) => {
                let length = i64::try_from(prefix.len()).unwrap_or(i64::MAX);
                if prefix.is_empty() {
                    return skip("prefixItems is empty");
                }
                if schema.get("minItems").and_then(Json::as_i64) != Some(length)
                    || schema.get("maxItems").is_some_and(|max| max.as_i64() != Some(length))
                {
                    return skip("prefixItems without minItems equal to its length does not fix the length");
                }
                let mut items = Vec::new();
                for (at, item) in prefix.iter().enumerate() {
                    items.push(self.child(format!("Item{at}"), item)?);
                }
                Ok(Type::Tuple(items))
            }
            (Some(_), _) => skip("prefixItems needs items false"),
            (None, Some(item @ Json::Object(_))) => Ok(Type::List(Box::new(self.child("Item".to_string(), item)?))),
            (None, Some(_)) => skip("items is not a single schema"),
            (None, None) => skip("an array without items has no element type"),
        }
    }

    fn object(&mut self, schema: &'j Json) -> Result<Type, Skip> {
        let properties = match schema.get("properties") {
            None => &[][..],
            Some(Json::Object(properties)) => properties.as_slice(),
            Some(_) => return skip("properties is not an object"),
        };
        let additional = match schema.get("additionalProperties") {
            None | Some(Json::Bool(true)) => Additional::Open,
            Some(Json::Bool(false)) => Additional::Closed,
            Some(found @ Json::Object(_)) => Additional::Schema(found),
            Some(_) => return skip("additionalProperties is not a boolean or a schema"),
        };
        let required: Vec<&str> = match schema.get("required") {
            None => Vec::new(),
            Some(Json::Array(names)) => {
                let mut listed = Vec::new();
                for name in names {
                    match name.as_str() {
                        Some(name) => listed.push(name),
                        None => return skip("required lists a name that is not a string"),
                    }
                }
                listed
            }
            Some(_) => return skip("required is not a list"),
        };
        if let Some(missing) = required.iter().find(|name| !properties.iter().any(|(held, _)| held == **name)) {
            return skip(format!("required property {missing} is not declared"));
        }
        if properties.is_empty() {
            return match additional {
                Additional::Schema(value) => {
                    let value = self.child("Value".to_string(), value)?;
                    Ok(Type::Map(Box::new(value)))
                }
                Additional::Closed => Ok(Type::Record(RecordType::plain(Vec::new()))),
                Additional::Open if self.depth == 1 && self.side == Side::Input => {
                    Ok(Type::Record(RecordType::plain(Vec::new())))
                }
                Additional::Open => skip("an object with no declared properties has no Sx type"),
            };
        }
        if matches!(additional, Additional::Schema(_)) {
            return skip("properties beside an additionalProperties schema have no Sx type");
        }
        let open = self.side == Side::Output && matches!(additional, Additional::Open);
        let mut fields = Vec::new();
        for (name, property) in properties {
            if !is_field_name(name) {
                return skip(format!("property {name:?} cannot be a record field name"));
            }
            let ty = self.child(pascal(name), property)?;
            let field = if required.contains(&name.as_str()) {
                if self.side == Side::Input && matches!(ty, Type::Option(_)) {
                    return skip(format!("required property {name} accepts null, which an Option input omits"));
                }
                ty
            } else if matches!(ty, Type::Option(_)) {
                ty
            } else {
                Type::Option(Box::new(ty))
            };
            fields.push((name.clone(), field));
        }
        Ok(Type::Record(RecordType { fields, name: None, open }))
    }
}

fn kind_of(schema: &Json) -> Result<(&str, bool), Skip> {
    match schema.get("type") {
        Some(Json::String(kind)) if kind == "null" => skip("type null alone has no Sx type"),
        Some(Json::String(kind)) => Ok((kind, false)),
        Some(Json::Array(kinds)) => {
            let names: Vec<&str> = kinds.iter().filter_map(Json::as_str).collect();
            match (names.as_slice(), names.len() == kinds.len()) {
                ([kind, "null"] | ["null", kind], true) if *kind != "null" => Ok((*kind, true)),
                _ => skip("a type list is not the nullable pattern of one type and null"),
            }
        }
        Some(_) => skip("type is not a string"),
        None if schema.get("enum").is_some() => Ok(("string", false)),
        None => skip("the schema has no type"),
    }
}
