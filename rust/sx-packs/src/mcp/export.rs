use sx_kernel::json::Json;
use sx_kernel::registry::{Capability, Effect};
use sx_kernel::types::{Primitive, STRING, Type};

use super::schema::Side;
use super::server::NotServed;

pub const WRAPPED_KEY: &str = "result";
pub const RESULT_META: &str = "sx/result";
pub const WRAPPED_META: &str = "wrapped";
pub const ONE_OF: &str = "oneOf";
pub const TAG_KEY: &str = "tag";
pub const VALUE_KEY: &str = "value";
pub const OK_KEY: &str = "ok";
pub const ERR_KEY: &str = "err";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Text,
    Structured,
    Wrapped,
}

impl Shape {
    pub fn of(output: &Type) -> Shape {
        match output {
            Type::Record(_) | Type::Map(_) => Shape::Structured,
            other if *other == STRING => Shape::Text,
            _ => Shape::Wrapped,
        }
    }
}

impl Shape {
    pub fn name(self) -> &'static str {
        match self {
            Shape::Text => "text",
            Shape::Structured => "structured",
            Shape::Wrapped => "wrapped",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Exported {
    pub shape: Shape,
    pub tool: Json,
}

fn kind(name: &str) -> Json {
    Json::object([("type", Json::string(name))])
}

fn strings(names: impl IntoIterator<Item = impl Into<String>>) -> Json {
    Json::Array(names.into_iter().map(Json::string).collect())
}

fn nullable(inner: Json) -> Json {
    Json::object([("anyOf", Json::Array(vec![inner, kind("null")]))])
}

fn tagged(tag: &str, payload: Json) -> Json {
    Json::object([
        ("type", Json::string("object")),
        (
            "properties",
            Json::object([
                (TAG_KEY, Json::object([("type", Json::string("string")), ("enum", strings([tag]))])),
                (VALUE_KEY, payload),
            ]),
        ),
        ("required", strings([TAG_KEY, VALUE_KEY])),
        ("additionalProperties", Json::Bool(false)),
    ])
}

fn keyed(key: &str, payload: Json) -> Json {
    Json::object([
        ("type", Json::string("object")),
        ("properties", Json::object([(key, payload)])),
        ("required", strings([key])),
        ("additionalProperties", Json::Bool(false)),
    ])
}

fn is_closed_object(option: &Json, names: &[&str]) -> bool {
    let (Json::Object(entries), Some(Json::Object(properties))) = (option, option.get("properties")) else {
        return false;
    };
    let required: Vec<&str> = option.get("required").and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(Json::as_str).collect();
    entries.len() == 4
        && option.get("type").and_then(Json::as_str) == Some("object")
        && matches!(option.get("additionalProperties"), Some(Json::Bool(false)))
        && properties.len() == names.len()
        && names.iter().all(|name| properties.iter().any(|(held, _)| held == name))
        && required.len() == names.len()
        && names.iter().all(|name| required.contains(name))
}

pub fn untagged(option: &Json) -> Option<(&str, &Json)> {
    if !is_closed_object(option, &[TAG_KEY, VALUE_KEY]) {
        return None;
    }
    let marker = option.get("properties")?.get(TAG_KEY)?;
    let Json::Object(entries) = marker else { return None };
    let names = marker.get("enum")?.as_array()?;
    let [tag] = names else { return None };
    let tag = tag.as_str()?;
    let plain = entries.len() == 2 && marker.get("type").and_then(Json::as_str) == Some("string");
    (plain && !tag.is_empty()).then_some((tag, option.get("properties")?.get(VALUE_KEY)?))
}

pub fn unkeyed<'j>(option: &'j Json, key: &str) -> Option<&'j Json> {
    if !is_closed_object(option, &[key]) {
        return None;
    }
    option.get("properties")?.get(key)
}

pub fn plain_variants(option: &Json) -> Option<Vec<&str>> {
    let Json::Object(entries) = option else { return None };
    if entries.len() != 2 || option.get("type").and_then(Json::as_str) != Some("string") {
        return None;
    }
    option.get("enum")?.as_array()?.iter().map(Json::as_str).collect()
}

fn enumeration(variants: &[(String, Option<Type>)], side: Side) -> Result<Json, String> {
    let plain = variants.iter().filter(|(_, payload)| payload.is_none()).map(|(name, _)| name.as_str());
    let names = Json::object([("type", Json::string("string")), ("enum", strings(plain))]);
    if variants.iter().all(|(_, payload)| payload.is_none()) {
        return Ok(names);
    }
    let mut options = Vec::new();
    if variants.iter().any(|(_, payload)| payload.is_none()) {
        options.push(names);
    }
    for (name, payload) in variants {
        if let Some(payload) = payload {
            options.push(tagged(name, schema(payload, side)?));
        }
    }
    Ok(Json::object([(ONE_OF, Json::Array(options))]))
}

fn record(fields: &[(String, Type)], open: bool, side: Side) -> Result<Json, String> {
    let mut properties = Vec::new();
    let mut required = Vec::new();
    for (name, field) in fields {
        match (field, side) {
            (Type::Option(inner), Side::Input) => properties.push((name.clone(), schema(inner, side)?)),
            _ => {
                properties.push((name.clone(), schema(field, side)?));
                required.push(name.as_str());
            }
        }
    }
    let mut entries = vec![
        ("type".to_string(), Json::string("object")),
        ("properties".to_string(), Json::Object(properties)),
        ("required".to_string(), strings(required)),
    ];
    if !open {
        entries.push(("additionalProperties".to_string(), Json::Bool(false)));
    }
    Ok(Json::Object(entries))
}

pub fn schema(ty: &Type, side: Side) -> Result<Json, String> {
    match ty {
        Type::Primitive(primitive) => Ok(match primitive {
            Primitive::Bool => kind("boolean"),
            Primitive::Int => kind("integer"),
            Primitive::Float => kind("number"),
            Primitive::Bytes => Json::object([("type", Json::string("string")), ("contentEncoding", Json::string("base64"))]),
            Primitive::String => kind("string"),
            Primitive::Unit => kind("null"),
        }),
        Type::List(element) => Ok(Json::object([("type", Json::string("array")), ("items", schema(element, side)?)])),
        Type::Map(value) => {
            Ok(Json::object([("type", Json::string("object")), ("additionalProperties", schema(value, side)?)]))
        }
        Type::Tuple(items) => {
            let length = Json::Int(i64::try_from(items.len()).unwrap_or(i64::MAX));
            let prefix = items.iter().map(|item| schema(item, side)).collect::<Result<Vec<_>, _>>()?;
            Ok(Json::object([
                ("type", Json::string("array")),
                ("prefixItems", Json::Array(prefix)),
                ("items", Json::Bool(false)),
                ("minItems", length.clone()),
                ("maxItems", length),
            ]))
        }
        Type::Option(inner) => Ok(nullable(schema(inner, side)?)),
        Type::Result(ok, err) => Ok(Json::object([(
            ONE_OF,
            Json::Array(vec![keyed(OK_KEY, schema(ok, side)?), keyed(ERR_KEY, schema(err, side)?)]),
        )])),
        Type::Record(found) => record(&found.fields, found.open, side),
        Type::Enum(found) => enumeration(&found.variants, side),
        Type::Nominal(found) => {
            let base = schema(&found.base, side)?;
            match (&found.validation, base) {
                (Some(validation), Json::Object(mut entries)) => {
                    entries.push(("description".to_string(), Json::string(validation.summary)));
                    Ok(Json::Object(entries))
                }
                (_, base) => Ok(base),
            }
        }
        Type::Variable(_) | Type::Never => Err(format!("the type {ty} has no JSON Schema")),
    }
}

fn input_schema(capability: &Capability) -> Result<Json, String> {
    match &capability.input {
        Type::Record(_) | Type::Map(_) => schema(&capability.input, Side::Input),
        other => Err(format!("its input type {other} is not a record or a map")),
    }
}

fn annotations(effect: Effect, idempotent: bool) -> Json {
    let hints = match (effect, idempotent) {
        (Effect::Read, _) => [true, false, true],
        (Effect::Write, true) => [false, false, true],
        (Effect::Write, false) => [false, true, false],
    };
    Json::object([
        ("readOnlyHint", Json::Bool(hints[0])),
        ("destructiveHint", Json::Bool(hints[1])),
        ("idempotentHint", Json::Bool(hints[2])),
    ])
}

pub fn tool_name_problem(name: &str) -> Option<String> {
    let valid = !name.is_empty() && name.len() <= 128 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    (!valid).then(|| format!("the capability id {name:?} is not a valid MCP tool name"))
}

pub fn export(capability: &Capability) -> Result<Exported, String> {
    if let Some(problem) = tool_name_problem(&capability.id) {
        return Err(problem);
    }
    let input = input_schema(capability)?;
    let shape = Shape::of(&capability.output);
    let mut entries = vec![
        ("name".to_string(), Json::string(capability.id.as_str())),
        ("description".to_string(), Json::string(capability.description.as_str())),
        ("inputSchema".to_string(), input),
    ];
    match shape {
        Shape::Text => {}
        Shape::Structured => {
            entries.push(("outputSchema".to_string(), schema(&capability.output, Side::Output)?));
        }
        Shape::Wrapped => {
            let wrapped = keyed(WRAPPED_KEY, schema(&capability.output, Side::Output)?);
            entries.push(("outputSchema".to_string(), wrapped));
            entries.push((
                "_meta".to_string(),
                Json::object([(RESULT_META, Json::string(WRAPPED_META))]),
            ));
        }
    }
    entries.push(("annotations".to_string(), annotations(capability.effect, capability.idempotent)));
    Ok(Exported { shape, tool: Json::Object(entries) })
}

#[derive(Clone, Debug)]
pub struct Listing {
    pub tools: Vec<Json>,
    pub shapes: Vec<(String, Shape)>,
    pub skipped: Vec<NotServed>,
}

pub fn listing<'c>(capabilities: impl IntoIterator<Item = &'c Capability>) -> Listing {
    let mut ordered: Vec<&Capability> = capabilities.into_iter().collect();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    let mut listing = Listing { tools: Vec::new(), shapes: Vec::new(), skipped: Vec::new() };
    for capability in ordered {
        match export(capability) {
            Ok(exported) => {
                listing.tools.push(exported.tool);
                listing.shapes.push((capability.id.clone(), exported.shape));
            }
            Err(reason) => listing.skipped.push(NotServed { id: capability.id.clone(), reason }),
        }
    }
    listing
}
