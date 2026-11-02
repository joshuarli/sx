use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use sx_kernel::json::{Json, python_repr, repr_str};
use sx_kernel::registry::{Effect, Registry};
use sx_kernel::types::{CallErrorKind, STRING, Type};
use sx_kernel::values::{Value, decode};

use super::client::{Connection, RpcError, STANDARD_TIMEOUT};
use super::export::{Shape, WRAPPED_KEY};
use super::import::ResultKind;
use crate::Failure;

const INVALID_PARAMS: i64 = -32602;
const MESSAGE_LIMIT: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub startup: Duration,
    pub call: Duration,
}

impl Limits {
    pub const STANDARD: Limits = Limits { startup: STANDARD_TIMEOUT, call: STANDARD_TIMEOUT };
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub tool: String,
    pub effect: Effect,
    pub input: Type,
    pub output: Type,
    pub result: ResultKind,
}

#[derive(Clone, Debug, Default)]
pub struct Bindings {
    server: String,
    by_id: BTreeMap<String, Binding>,
}

struct Declared<'j> {
    id: &'j str,
    server: &'j str,
    tool: &'j str,
    result: ResultKind,
}

fn declared(entry: &Json) -> Result<Option<Declared<'_>>, String> {
    let Some(mcp) = entry.get("mcp") else {
        return Ok(None);
    };
    let id = entry.get("id").and_then(Json::as_str).ok_or("a capability with an mcp key needs a string id")?;
    let named = |message: String| format!("capability {}: {message}", repr_str(id));
    let Json::Object(fields) = mcp else {
        return Err(named(format!("mcp must be an object, not {}", python_repr(mcp))));
    };
    if let Some((key, _)) = fields.iter().find(|(key, _)| !["server", "tool", "result"].contains(&key.as_str())) {
        return Err(named(format!("mcp has an unknown key {}", repr_str(key))));
    }
    let text = |key: &str| {
        mcp.get(key).and_then(Json::as_str).ok_or_else(|| named(format!("mcp needs a string {}", repr_str(key))))
    };
    let result = ResultKind::from_name(text("result")?)
        .ok_or_else(|| named("mcp result must be \"text\", \"structured\", or \"wrapped\"".to_string()))?;
    Ok(Some(Declared { id, server: text("server")?, tool: text("tool")?, result }))
}

impl Bindings {
    pub fn from_declarations(registry: &Registry, declarations: &Json, server: Option<&str>) -> Result<Bindings, String> {
        let entries = declarations.get("capabilities").and_then(Json::as_array).unwrap_or(&[]);
        let mut all = Vec::new();
        for entry in entries {
            if let Some(found) = declared(entry)? {
                all.push(found);
            }
        }
        let servers: BTreeSet<&str> = all.iter().map(|found| found.server).collect();
        let chosen = match (server, servers.len()) {
            (Some(name), _) if servers.contains(name) => name,
            (Some(name), _) => return Err(format!("the declarations define no tools of the MCP server {}", repr_str(name))),
            (None, 0) => return Err("the declarations define no MCP tools".to_string()),
            (None, 1) => servers.iter().next().copied().unwrap_or(""),
            (None, _) => {
                let listed: Vec<&str> = servers.iter().copied().collect();
                return Err(format!(
                    "the declarations define tools of several MCP servers ({}); choose one with --name",
                    listed.join(", ")
                ));
            }
        };
        let mut by_id = BTreeMap::new();
        for found in all.into_iter().filter(|found| found.server == chosen) {
            let capability = registry
                .capability(found.id)
                .ok_or_else(|| format!("capability {} is not registered", repr_str(found.id)))?;
            let fits = match found.result {
                ResultKind::Text => capability.output == STRING,
                ResultKind::Structured => matches!(capability.output, Type::Record(_) | Type::Map(_)),
                ResultKind::Wrapped => Shape::of(&capability.output) == Shape::Wrapped,
            };
            if !fits {
                return Err(format!(
                    "capability {} declares a {} result that its output type does not fit",
                    repr_str(found.id),
                    found.result.name()
                ));
            }
            by_id.insert(
                found.id.to_string(),
                Binding {
                    tool: found.tool.to_string(),
                    effect: capability.effect,
                    input: capability.input.clone(),
                    output: capability.output.clone(),
                    result: found.result,
                },
            );
        }
        Ok(Bindings { server: chosen.to_string(), by_id })
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    pub fn get(&self, id: &str) -> Option<&Binding> {
        self.by_id.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.by_id.keys().map(String::as_str)
    }
}

pub fn arguments(input: &Json, ty: &Type) -> Json {
    match (input, ty) {
        (Json::Object(entries), Type::Record(record)) => Json::Object(
            entries
                .iter()
                .filter_map(|(name, value)| {
                    let field = record.field(name);
                    if value.is_null() && matches!(field, Some(Type::Option(_))) {
                        return None;
                    }
                    Some((name.clone(), field.map_or_else(|| value.clone(), |field| arguments(value, field))))
                })
                .collect(),
        ),
        (Json::Object(entries), Type::Map(value_type)) => {
            Json::Object(entries.iter().map(|(name, value)| (name.clone(), arguments(value, value_type))).collect())
        }
        (Json::Array(items), Type::List(element)) => Json::Array(items.iter().map(|item| arguments(item, element)).collect()),
        (Json::Array(items), Type::Tuple(types)) => {
            Json::Array(items.iter().zip(types).map(|(item, ty)| arguments(item, ty)).collect())
        }
        (value, Type::Option(inner)) if !value.is_null() => arguments(value, inner),
        (value, _) => value.clone(),
    }
}

pub fn filled(output: &Json, ty: &Type) -> Json {
    match (output, ty) {
        (Json::Object(entries), Type::Record(record)) => {
            let mut fields: Vec<(String, Json)> = entries
                .iter()
                .map(|(name, value)| (name.clone(), record.field(name).map_or_else(|| value.clone(), |field| filled(value, field))))
                .collect();
            for (name, field) in &record.fields {
                if matches!(field, Type::Option(_)) && !entries.iter().any(|(held, _)| held == name) {
                    fields.push((name.clone(), Json::Null));
                }
            }
            Json::Object(fields)
        }
        (Json::Object(entries), Type::Map(value_type)) => {
            Json::Object(entries.iter().map(|(name, value)| (name.clone(), filled(value, value_type))).collect())
        }
        (Json::Array(items), Type::List(element)) => Json::Array(items.iter().map(|item| filled(item, element)).collect()),
        (Json::Array(items), Type::Tuple(types)) => {
            Json::Array(items.iter().zip(types).map(|(item, ty)| filled(item, ty)).collect())
        }
        (value, Type::Option(inner)) if !value.is_null() => filled(value, inner),
        (value, _) => value.clone(),
    }
}

fn shortened(text: &str) -> String {
    match text.char_indices().nth(MESSAGE_LIMIT) {
        Some((end, _)) => format!("{}...", text.get(..end).unwrap_or(text)),
        None => text.to_string(),
    }
}

fn texts(result: &Json) -> Vec<&str> {
    result
        .get("content")
        .and_then(Json::as_array)
        .unwrap_or(&[])
        .iter()
        .filter(|block| block.get("type").and_then(Json::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Json::as_str))
        .collect()
}

fn other_block(result: &Json) -> Option<&str> {
    result
        .get("content")
        .and_then(Json::as_array)
        .unwrap_or(&[])
        .iter()
        .map(|block| block.get("type").and_then(Json::as_str).unwrap_or("untyped"))
        .find(|kind| *kind != "text")
}

fn unmatched(effect: Effect) -> CallErrorKind {
    match effect {
        Effect::Read => CallErrorKind::Invalid,
        Effect::Write => CallErrorKind::Failed,
    }
}

pub struct Session {
    bindings: Bindings,
    connection: Connection,
    limits: Limits,
}

impl Session {
    pub fn open(bindings: Bindings, command: &[String], limits: Limits) -> Result<Session, String> {
        let connection = Connection::open(command, limits.startup)?;
        Ok(Session { bindings, connection, limits })
    }

    pub fn handles(&self, id: &str) -> bool {
        self.bindings.get(id).is_some()
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn call(&self, id: &str, input: &Json) -> Result<Value, Failure> {
        let Some(binding) = self.bindings.get(id) else {
            return Err(Failure::new(CallErrorKind::Failed, format!("{id} is not a tool of the MCP server {}", self.bindings.server())));
        };
        let params = Json::object([
            ("name", Json::string(binding.tool.as_str())),
            ("arguments", arguments(input, &binding.input)),
        ]);
        let result = self
            .connection
            .request("tools/call", params, self.limits.call)
            .map_err(|error| self.transport_failure(id, binding, error))?;
        Self::interpret(id, binding, &result)
    }

    fn transport_failure(&self, id: &str, binding: &Binding, error: RpcError) -> Failure {
        match error {
            RpcError::Timeout => Failure::new(
                CallErrorKind::Timeout,
                format!("{id}: the server did not answer within {} ms", self.limits.call.as_millis()),
            ),
            RpcError::Closed(reason) => {
                let kind = match binding.effect {
                    Effect::Read => CallErrorKind::Unavailable,
                    Effect::Write => CallErrorKind::Failed,
                };
                Failure::new(kind, format!("{id}: {reason}"))
            }
            RpcError::Remote { code, message } => {
                let kind = match (code, binding.effect) {
                    (INVALID_PARAMS, Effect::Read) => CallErrorKind::Invalid,
                    _ => CallErrorKind::Failed,
                };
                Failure::new(kind, format!("{id}: the server answered with error {code}: {}", shortened(&message)))
            }
        }
    }

    fn interpret(id: &str, binding: &Binding, result: &Json) -> Result<Value, Failure> {
        if result.get("isError").and_then(Json::as_bool) == Some(true) {
            let message = texts(result).join("\n");
            let message = if message.trim().is_empty() { "the tool reported an error without a message".to_string() } else { message };
            return Err(Failure::new(CallErrorKind::Failed, format!("{id}: {}", shortened(&message))));
        }
        let mismatch = |detail: String| Failure::new(unmatched(binding.effect), format!("{id}: {}", shortened(&detail)));
        match binding.result {
            ResultKind::Text => {
                if let Some(kind) = other_block(result) {
                    return Err(mismatch(format!(
                        "the result has a {kind} content block, and a tool without an outputSchema returns only text"
                    )));
                }
                Ok(Value::string(&texts(result).concat()))
            }
            ResultKind::Structured => {
                let Some(structured) = result.get("structuredContent") else {
                    return Err(mismatch("the result has no structuredContent for the declared outputSchema".to_string()));
                };
                decode(&filled(structured, &binding.output), &binding.output, "$")
                    .map_err(|error| mismatch(format!("structuredContent does not match the declared output: {error}")))
            }
            ResultKind::Wrapped => {
                let Some(wrapped) = result.get("structuredContent").and_then(|structured| structured.get(WRAPPED_KEY)) else {
                    return Err(mismatch(format!("the result has no structuredContent.{WRAPPED_KEY} for the wrapped output")));
                };
                decode(&filled(wrapped, &binding.output), &binding.output, "$")
                    .map_err(|error| mismatch(format!("structuredContent.{WRAPPED_KEY} does not match the declared output: {error}")))
            }
        }
    }
}
