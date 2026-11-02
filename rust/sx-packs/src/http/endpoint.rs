use std::collections::BTreeMap;
use std::sync::Arc;

use sx_kernel::authority::ScopeCheck;
use sx_kernel::json::{Json, Layout, dumps, parse, python_repr, repr_str};
use sx_kernel::registry::{Capability, Effect, EndpointBinder, Registry};
use sx_kernel::types::{BOOL, CallErrorKind, INT, RecordType, STRING, Type, UNIT, show_type};
use sx_kernel::values::{RecordValue, Value, decode, encode};

use super::origin::{Origin, Url, encode_component};
use super::scope::{self, Scope, check_endpoint, prepare, scope_type};
use super::transport::{Headers, Limits, Outgoing, send, status_kind};
use crate::Failure;
use crate::declaration::{required, string_of, unknown_keys};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scalar {
    String,
    Int,
    Bool,
}

impl Scalar {
    fn of(t: &Type) -> Option<Scalar> {
        if *t == STRING {
            Some(Scalar::String)
        } else if *t == INT {
            Some(Scalar::Int)
        } else if *t == BOOL {
            Some(Scalar::Bool)
        } else {
            None
        }
    }

    fn text(self, value: &Value) -> Option<String> {
        match (self, value) {
            (Scalar::String, Value::String(s)) => Some(s.to_string()),
            (Scalar::Int, Value::Int(n)) => Some(n.to_string()),
            (Scalar::Bool, Value::Bool(b)) => Some(b.to_string()),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    One,
    Maybe,
    Many,
}

#[derive(Clone, Debug)]
enum Part {
    Literal(String),
    Field { name: String, scalar: Scalar },
}

#[derive(Clone, Debug)]
struct Param {
    field: String,
    scalar: Scalar,
    shape: Shape,
}

#[derive(Clone, Debug)]
pub struct Endpoint {
    id: String,
    method: String,
    origin: Origin,
    path: Vec<Part>,
    query: Vec<Param>,
    headers: Headers,
    body: Vec<(String, Type)>,
    output: Type,
    effect: Effect,
}

#[derive(Clone, Debug, Default)]
pub struct Endpoints {
    by_id: BTreeMap<String, Arc<Endpoint>>,
}

impl Endpoints {
    pub fn get(&self, id: &str) -> Option<&Arc<Endpoint>> {
        self.by_id.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.by_id.keys().map(String::as_str)
    }
}

#[derive(Debug, Default)]
pub struct EndpointLoader {
    endpoints: Endpoints,
}

impl EndpointLoader {
    pub fn new() -> EndpointLoader {
        EndpointLoader::default()
    }

    pub fn finish(self) -> Endpoints {
        self.endpoints
    }
}

impl EndpointBinder for EndpointLoader {
    fn bind(&mut self, registry: &mut Registry, entry: &Json) -> Result<(), String> {
        let declared = Declared::of(registry, entry)?;
        let endpoint = declared.endpoint;
        let origin = endpoint.origin.clone();
        let method = endpoint.method.clone();
        let check = ScopeCheck::new(prepare, move |scope, _input| check_endpoint(scope, &origin, &method));
        let id = endpoint.id.clone();
        registry
            .add_checked_capability(declared.capability, check)
            .map_err(|e| format!("endpoint {}: {}", repr_str(&id), e.0))?;
        self.endpoints.by_id.insert(id, Arc::new(endpoint));
        Ok(())
    }
}

struct Declared {
    endpoint: Endpoint,
    capability: Capability,
}

impl Declared {
    fn of(registry: &Registry, entry: &Json) -> Result<Declared, String> {
        let id = string_of(required(entry, "id", "an endpoint")?, "an endpoint id")?.to_string();
        let named = |message: String| format!("endpoint {}: {message}", repr_str(&id));
        Declared::build(registry, entry, &id).map_err(named)
    }

    fn build(registry: &Registry, entry: &Json, id: &str) -> Result<Declared, String> {
        if entry.get("scope").is_some() {
            return Err("an endpoint's scope is implied, so it declares none".to_string());
        }
        let http = required(entry, "http", "an endpoint")?;
        unknown_keys(http, &["method", "url", "query", "headers"], "http")?;
        let effect_json = required(entry, "effect", "an endpoint")?;
        let effect = effect_json
            .as_str()
            .and_then(Effect::from_name)
            .ok_or_else(|| format!("{} is not a valid Effect", python_repr(effect_json)))?;
        let input = registry
            .parse(string_of(required(entry, "input", "an endpoint")?, "input")?)
            .map_err(|e| e.0)?;
        let output = registry
            .parse(string_of(required(entry, "output", "an endpoint")?, "output")?)
            .map_err(|e| e.0)?;
        let Type::Record(input_record) = &input else {
            return Err(format!("the input must be a record, not {}", show_type(&input)));
        };
        let method = string_of(required(http, "method", "http")?, "the method")?.to_string();
        if !scope::is_method(&method) {
            return Err(format!("method {} is not an uppercase HTTP method token", repr_str(&method)));
        }
        let template = string_of(required(http, "url", "http")?, "the url")?;
        let (origin, path) = parse_template(template, input_record)?;
        let mut used: Vec<String> = path
            .iter()
            .filter_map(|part| match part {
                Part::Field { name, .. } => Some(name.clone()),
                Part::Literal(_) => None,
            })
            .collect();
        let query = parse_query(http, input_record, &mut used)?;
        let body: Vec<(String, Type)> =
            input_record.fields.iter().filter(|(name, _)| !used.contains(name)).cloned().collect();
        let carries_body = matches!(method.as_str(), "POST" | "PUT" | "PATCH");
        if !body.is_empty() && !carries_body {
            let names: Vec<&str> = body.iter().map(|(name, _)| name.as_str()).collect();
            return Err(format!(
                "input fields {} are not used in the path or query, and {method} sends no body",
                names.join(", ")
            ));
        }
        let mut headers = Headers::new();
        if let Some(declared) = http.get("headers") {
            let Json::Object(entries) = declared else {
                return Err(format!("headers must map names to values, not {}", python_repr(declared)));
            };
            for (name, value) in entries {
                headers.insert(name, string_of(value, "a header value")?).map_err(|f| f.message)?;
            }
        }
        if !body.is_empty() && !headers.has("content-type") {
            headers.insert("content-type", "application/json").map_err(|f| f.message)?;
        }
        let idempotent = entry.get("idempotent").and_then(Json::as_bool).unwrap_or(false);
        let description = match entry.get("description") {
            Some(text) => string_of(text, "the description")?.to_string(),
            None => format!("{method} {template}"),
        };
        let endpoint = Endpoint {
            id: id.to_string(),
            method,
            origin,
            path,
            query,
            headers,
            body,
            output: output.clone(),
            effect,
        };
        let capability =
            Capability { id: id.to_string(), effect, input, output, description, idempotent, scope: Some(scope_type()) };
        Ok(Declared { endpoint, capability })
    }
}

fn scalar_field(record: &RecordType, name: &str, what: &str) -> Result<(Scalar, Shape), String> {
    let t = record.field(name).ok_or_else(|| format!("{what} {} is not a field of the input", repr_str(name)))?;
    let (inner, shape) = match t {
        Type::Option(inner) => (&**inner, Shape::Maybe),
        Type::List(inner) => (&**inner, Shape::Many),
        other => (other, Shape::One),
    };
    let scalar = Scalar::of(inner).ok_or_else(|| {
        format!("{what} {} has type {}, which is not a String, Int, or Bool", repr_str(name), show_type(t))
    })?;
    Ok((scalar, shape))
}

fn parse_template(template: &str, input: &RecordType) -> Result<(Origin, Vec<Part>), String> {
    let (scheme, rest) = template.split_once("://").ok_or_else(|| format!("url {} is not absolute", repr_str(template)))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    if authority.contains(['{', '}']) {
        return Err(format!("the origin of url {} must be literal, without a {{field}}", repr_str(template)));
    }
    if path.contains(['?', '#']) {
        return Err("a url template has no query or fragment; list query parameters under \"query\"".to_string());
    }
    let origin = Origin::parse(&format!("{scheme}://{authority}"))?;
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = path.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '}' {
                        closed = true;
                        break;
                    }
                    name.push(inner);
                }
                if !closed || name.is_empty() || name.contains('{') {
                    return Err(format!("url {} has a malformed {{field}}", repr_str(template)));
                }
                let (scalar, shape) = scalar_field(input, &name, "path variable")?;
                if shape != Shape::One {
                    return Err(format!("path variable {} must be a String, Int, or Bool, not optional or a list", repr_str(&name)));
                }
                if parts.iter().any(|part| matches!(part, Part::Field { name: used, .. } if *used == name)) {
                    return Err(format!("path variable {} appears twice", repr_str(&name)));
                }
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                parts.push(Part::Field { name, scalar });
            }
            '}' => return Err(format!("url {} has a }} without a {{", repr_str(template))),
            other => literal.push(other),
        }
    }
    if !literal.is_empty() {
        parts.push(Part::Literal(literal));
    }
    let probe: String = parts
        .iter()
        .map(|part| match part {
            Part::Literal(text) => text.as_str(),
            Part::Field { .. } => "x",
        })
        .collect();
    Url::parse(&format!("{origin}{probe}")).map_err(|e| format!("url {}: {e}", repr_str(template)))?;
    Ok((origin, parts))
}

fn parse_query(http: &Json, input: &RecordType, used: &mut Vec<String>) -> Result<Vec<Param>, String> {
    let Some(listed) = http.get("query") else {
        return Ok(Vec::new());
    };
    let Json::Array(names) = listed else {
        return Err(format!("query must be a list of input field names, not {}", python_repr(listed)));
    };
    let mut params = Vec::new();
    for name in names {
        let field = string_of(name, "a query parameter")?;
        if used.iter().any(|u| u == field) {
            return Err(format!("input field {} is used twice in the path and query", repr_str(field)));
        }
        let (scalar, shape) = scalar_field(input, field, "query parameter")?;
        used.push(field.to_string());
        params.push(Param { field: field.to_string(), scalar, shape });
    }
    Ok(params)
}

fn invalid(message: String) -> Failure {
    Failure::new(CallErrorKind::Invalid, message)
}

fn field<'v>(input: &'v Value, name: &str) -> Option<&'v Value> {
    match input {
        Value::Record(record) => record.get(name),
        _ => None,
    }
}

impl Endpoint {
    pub fn id(&self) -> &str {
        &self.id
    }

    fn mismatch(&self) -> Failure {
        Failure::new(CallErrorKind::Failed, format!("{} received an input that does not match its type", self.id))
    }

    fn target(&self, input: &Value) -> Result<String, Failure> {
        let mut target = String::new();
        for part in &self.path {
            match part {
                Part::Literal(text) => target.push_str(text),
                Part::Field { name, scalar } => {
                    let value = field(input, name).and_then(|v| scalar.text(v)).ok_or_else(|| self.mismatch())?;
                    if value.is_empty() || value == "." || value == ".." {
                        return Err(invalid(format!("path variable {name} is {value:?}, which names no segment")));
                    }
                    target.push_str(&encode_component(&value));
                }
            }
        }
        if target.is_empty() {
            target.push('/');
        }
        let mut pairs: Vec<String> = Vec::new();
        for param in &self.query {
            let value = field(input, &param.field).ok_or_else(|| self.mismatch())?;
            let values: Vec<&Value> = match (param.shape, value) {
                (Shape::One, value) => vec![value],
                (Shape::Maybe, Value::None) => Vec::new(),
                (Shape::Maybe, Value::Some(inner)) => vec![&**inner],
                (Shape::Many, Value::List(items)) => items.iter().collect(),
                _ => return Err(self.mismatch()),
            };
            for value in values {
                let text = param.scalar.text(value).ok_or_else(|| self.mismatch())?;
                pairs.push(format!("{}={}", encode_component(&param.field), encode_component(&text)));
            }
        }
        if !pairs.is_empty() {
            target.push('?');
            target.push_str(&pairs.join("&"));
        }
        Ok(target)
    }

    fn body_of(&self, input: &Value) -> Result<Option<Vec<u8>>, Failure> {
        if self.body.is_empty() {
            return Ok(None);
        }
        let fields: Vec<(Arc<str>, Value)> = self
            .body
            .iter()
            .map(|(name, _)| field(input, name).map(|v| (Arc::from(name.as_str()), v.clone())))
            .collect::<Option<_>>()
            .ok_or_else(|| self.mismatch())?;
        let shape = Type::Record(RecordType::plain(self.body.clone()));
        let json = encode(&Value::Record(RecordValue::new(fields)), &shape);
        let text = dumps(&json, Layout::Spaced).map_err(|e| Failure::new(CallErrorKind::Failed, e.0))?;
        Ok(Some(text.into_bytes()))
    }

    pub fn perform_with(&self, limits: &Limits, scope_value: &Value, input: &Value) -> Result<Value, Failure> {
        let denied = |reason: String| Failure::new(CallErrorKind::PermissionDenied, reason);
        let scope = Scope::of(scope_value).map_err(denied)?;
        scope.permits(&self.origin, &self.method).map_err(denied)?;
        let url = Url { origin: self.origin.clone(), target: self.target(input)? };
        let body = self.body_of(input)?;
        let reply = send(
            limits,
            &scope,
            Outgoing { method: self.method.clone(), url, headers: self.headers.clone(), body },
        )?;
        if let Some(kind) = status_kind(reply.status) {
            let detail = reply
                .body
                .as_ref()
                .ok()
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .map(|text| text.trim().chars().take(200).collect::<String>())
                .filter(|text| !text.is_empty())
                .map(|text| format!(": {text}"))
                .unwrap_or_default();
            return Err(Failure::new(kind, format!("{} answered {}{detail}", reply.url.shown(), reply.status)));
        }
        let unreadable = if self.effect == Effect::Read { CallErrorKind::Invalid } else { CallErrorKind::Failed };
        let bytes = reply.body?;
        if self.output == UNIT {
            return Ok(Value::Unit);
        }
        let shown = reply.url.shown();
        let text = String::from_utf8(bytes)
            .map_err(|e| Failure::new(unreadable, format!("{shown}: the response body is not valid UTF-8: {e}")))?;
        let json = parse(&text).map_err(|e| Failure::new(unreadable, format!("{shown}: the response body is not JSON: {e}")))?;
        decode(&json, &self.output, "$")
            .map_err(|e| Failure::new(unreadable, format!("{shown}: the response does not match the declared output: {e}")))
    }

    pub fn perform(&self, scope: &Value, input: &Value) -> Result<Value, Failure> {
        self.perform_with(&Limits::DEFAULT, scope, input)
    }
}
