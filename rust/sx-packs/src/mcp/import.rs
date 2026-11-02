use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use sx_kernel::json::Json;
use sx_kernel::registry::{Effect, Registry, load_declarations};
use sx_kernel::types::{EnumType, STRING, show_type};

use super::schema::{Side, map, pascal};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub tool: String,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct Imported {
    pub declarations: Json,
    pub skipped: Vec<Skipped>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    ServerName(String),
    Malformed(String),
    NameCollision { id: String, first: String, second: String },
    TypeCollision(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::ServerName(message) | ImportError::Malformed(message) | ImportError::TypeCollision(message) => {
                f.write_str(message)
            }
            ImportError::NameCollision { id, first, second } => {
                write!(f, "tools {first:?} and {second:?} would both be the capability {id}")
            }
        }
    }
}

impl std::error::Error for ImportError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultKind {
    Text,
    Structured,
}

impl ResultKind {
    pub fn name(self) -> &'static str {
        match self {
            ResultKind::Text => "text",
            ResultKind::Structured => "structured",
        }
    }

    pub fn from_name(name: &str) -> Option<ResultKind> {
        match name {
            "text" => Some(ResultKind::Text),
            "structured" => Some(ResultKind::Structured),
            _ => None,
        }
    }
}

pub fn server_name_problem(name: &str) -> Option<String> {
    let mut parts = name.split('-');
    let word = |part: &str, first: bool| {
        let mut chars = part.chars();
        let head = chars.next().is_some_and(|c| if first { c.is_ascii_lowercase() } else { c.is_ascii_lowercase() || c.is_ascii_digit() });
        head && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    };
    let first = parts.next().is_some_and(|part| word(part, true));
    if first && parts.all(|part| word(part, false)) {
        None
    } else {
        Some(format!(
            "the server name {name:?} must be lowercase letters and digits joined by -, starting with a letter"
        ))
    }
}

pub fn member_name(tool: &str) -> Result<String, String> {
    let mut member = String::new();
    let mut previous: Option<char> = None;
    for c in tool.chars() {
        match c {
            'A'..='Z' => {
                if previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) {
                    member.push('-');
                }
                member.push(c.to_ascii_lowercase());
            }
            'a'..='z' | '0'..='9' => member.push(c),
            '_' | '-' | '.' => member.push('-'),
            _ => return Err(format!("the character {c:?} has no place in an Sx name")),
        }
        previous = Some(c);
    }
    let starts_with_letter = member.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    if !starts_with_letter || member.split('-').any(str::is_empty) {
        return Err("the name does not start with a letter, or has an empty word".to_string());
    }
    Ok(member)
}

pub fn capability_id(server: &str, member: &str) -> String {
    format!("mcp-{server}.{member}")
}

fn effect_of(tool: &Json) -> (Effect, bool) {
    let hint = |name: &str| tool.get("annotations").and_then(|annotations| annotations.get(name)).and_then(Json::as_bool);
    if hint("readOnlyHint") == Some(true) {
        (Effect::Read, false)
    } else if hint("destructiveHint") == Some(false) && hint("idempotentHint") == Some(true) {
        (Effect::Write, true)
    } else {
        (Effect::Write, false)
    }
}

fn describe(tool: &Json, name: &str, server: &str) -> String {
    ["description", "title"]
        .iter()
        .filter_map(|key| tool.get(key).and_then(Json::as_str))
        .find_map(|text| text.lines().map(str::trim).find(|line| !line.is_empty()))
        .map_or_else(|| format!("The MCP tool {name} of the server {server}."), str::to_string)
}

struct Built {
    capability: Json,
    enums: Vec<Arc<EnumType>>,
}

fn build(server: &str, tool: &Json, name: &str, member: &str) -> Result<Built, String> {
    let id = capability_id(server, member);
    let Some(schema @ Json::Object(_)) = tool.get("inputSchema") else {
        return Err("it has no inputSchema object".to_string());
    };
    let words = pascal(member);
    let input = map(schema, Side::Input, &format!("mcp-{server}.{words}Input")).map_err(|e| format!("inputSchema: {}", e.0))?;
    let (output, kind, output_enums) = match tool.get("outputSchema") {
        None | Some(Json::Null) => (STRING, ResultKind::Text, Vec::new()),
        Some(schema) => {
            let mapped = map(schema, Side::Output, &format!("mcp-{server}.{words}Output"))
                .map_err(|e| format!("outputSchema: {}", e.0))?;
            (mapped.ty, ResultKind::Structured, mapped.enums)
        }
    };
    let (effect, idempotent) = effect_of(tool);
    let mut entries = vec![
        ("id", Json::string(id)),
        ("effect", Json::string(effect.name())),
    ];
    if effect == Effect::Write {
        entries.push(("idempotent", Json::Bool(idempotent)));
    }
    entries.push(("description", Json::string(describe(tool, name, server))));
    entries.push(("input", Json::string(show_type(&input.ty))));
    entries.push(("output", Json::string(show_type(&output))));
    entries.push((
        "mcp",
        Json::object([("server", Json::string(server)), ("tool", Json::string(name)), ("result", Json::string(kind.name()))]),
    ));
    let mut enums = input.enums;
    enums.extend(output_enums);
    for (at, found) in enums.iter().enumerate() {
        if enums.iter().take(at).any(|held| held.name == found.name && **held != **found) {
            return Err(format!("two different enums would both be named {}", found.name));
        }
    }
    Ok(Built { capability: Json::object(entries), enums })
}

fn enum_declaration(found: &EnumType) -> Json {
    Json::Object(
        found
            .variants
            .iter()
            .map(|(variant, payload)| (variant.clone(), payload.as_ref().map_or(Json::Null, |payload| Json::string(show_type(payload)))))
            .collect(),
    )
}

pub fn import(server: &str, tools: &[Json]) -> Result<Imported, ImportError> {
    if let Some(problem) = server_name_problem(server) {
        return Err(ImportError::ServerName(problem));
    }
    let mut claimed: BTreeMap<String, String> = BTreeMap::new();
    let mut named: Vec<(&Json, &str, Result<String, String>)> = Vec::new();
    for (at, tool) in tools.iter().enumerate() {
        let Some(name) = tool.get("name").and_then(Json::as_str) else {
            return Err(ImportError::Malformed(format!("tool {at} of the list has no string name")));
        };
        let member = member_name(name);
        if let Ok(member) = &member
            && let Some(first) = claimed.insert(member.clone(), name.to_string())
        {
            return Err(ImportError::NameCollision {
                id: capability_id(server, member),
                first,
                second: name.to_string(),
            });
        }
        named.push((tool, name, member));
    }
    let mut registry = Registry::new();
    let mut committed: BTreeMap<String, Arc<EnumType>> = BTreeMap::new();
    let mut enums: Vec<(String, Json)> = Vec::new();
    let mut capabilities = Vec::new();
    let mut skipped = Vec::new();
    for (tool, name, member) in named {
        let outcome = member.and_then(|member| build(server, tool, name, &member));
        let built = match outcome {
            Ok(built) => built,
            Err(reason) => {
                skipped.push(Skipped { tool: name.to_string(), reason });
                continue;
            }
        };
        for found in &built.enums {
            if committed.get(&found.name).is_some_and(|held| **held != **found) {
                return Err(ImportError::TypeCollision(format!(
                    "the type {} of tool {name:?} is already named by another tool",
                    found.name
                )));
            }
        }
        let fresh: Vec<&Arc<EnumType>> = built.enums.iter().filter(|found| !committed.contains_key(&found.name)).collect();
        let scratch_declarations = Json::object([
            ("enums", Json::Object(fresh.iter().map(|found| (found.name.clone(), enum_declaration(found))).collect())),
            ("capabilities", Json::Array(vec![built.capability.clone()])),
        ]);
        let mut scratch = registry.clone();
        if let Err(error) = load_declarations(&mut scratch, &scratch_declarations) {
            skipped.push(Skipped { tool: name.to_string(), reason: format!("the declaration is invalid: {error}") });
            continue;
        }
        registry = scratch;
        for found in fresh {
            enums.push((found.name.clone(), enum_declaration(found)));
        }
        for found in &built.enums {
            committed.insert(found.name.clone(), found.clone());
        }
        capabilities.push(built.capability);
    }
    let declarations =
        Json::object([("enums", Json::Object(enums)), ("capabilities", Json::Array(capabilities))]);
    Ok(Imported { declarations, skipped })
}
