use std::collections::BTreeSet;

use sx_kernel::help::effect_label;
use sx_kernel::registry::{Capability, Registry};
use sx_kernel::types::{EnumType, Primitive, RecordType, Type};

use crate::lower::camel;
use crate::terms::type_name;

const CALL_ERROR: &str = r#"interface CallError extends Error { kind: "Timeout" | "Unavailable" | "RateLimited" | "NotFound" | "PermissionDenied" | "Invalid" | "Conflict" | "Failed"; message: string }"#;
const SETTLED: &str = r#"type Settled<T, E> = { status: "fulfilled"; value: T } | { status: "rejected"; reason: E };"#;
const BUILTINS: &str = r#"declare const tools: Tools;
declare const console: { log(...values: unknown[]): void };
declare function retry<T>(options: { delaysMs?: number[]; backoffMs?: [number, number]; withinMs?: number; on?: CallError["kind"][] }, body: () => Promise<T>): Promise<T>;
declare function waitUntil<T>(options: { everyMs?: number; backoffMs?: [number, number]; withinMs: number }, poll: () => Promise<T>, done: (value: T) => boolean): Promise<T>;
declare function parMap<T, U>(options: { limit: number }, items: T[], body: (item: T) => Promise<U>): Promise<U[]>;
interface Step<S, R> { _next?: S; _done?: R }
declare function loop<S extends object, R>(options: { max: number }, state: S, body: (state: S) => Step<S, R>): Promise<R>;
declare function next<S>(state: S): Step<S, never>;
declare function done<R>(value: R): Step<never, R>;
declare function sleep(ms: number): Promise<void>;"#;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Input,
    Output,
}

struct Writer<'a> {
    registry: &'a Registry,
    named: Vec<String>,
    settled: bool,
}

fn key(name: &str) -> String {
    let plain = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain { name.to_string() } else { format!("\"{name}\"") }
}

fn comment(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace("*/", "* /")
}

impl Writer<'_> {
    fn name_of(&mut self, name: &str) -> String {
        if !self.named.iter().any(|held| held == name) {
            self.named.push(name.to_string());
        }
        type_name(name)
    }

    fn ty(&mut self, t: &Type, position: Position) -> String {
        match t {
            Type::Primitive(Primitive::Bool) => "boolean".to_string(),
            Type::Primitive(Primitive::Int) => "Int".to_string(),
            Type::Primitive(Primitive::Float) => "Float".to_string(),
            Type::Primitive(Primitive::String | Primitive::Bytes) => "string".to_string(),
            Type::Primitive(Primitive::Unit) => "null".to_string(),
            Type::List(element) => {
                let element = self.ty(element, position);
                if element.contains(" | ") || element.contains("=>") { format!("({element})[]") } else { format!("{element}[]") }
            }
            Type::Map(value) => format!("Record<string, {}>", self.ty(value, position)),
            Type::Tuple(items) => format!("[{}]", items.iter().map(|item| self.ty(item, position)).collect::<Vec<_>>().join(", ")),
            Type::Option(inner) => format!("{} | null", self.ty(inner, position)),
            Type::Result(ok, err) => {
                self.settled = true;
                format!("Settled<{}, {}>", self.ty(ok, position), self.ty(err, position))
            }
            Type::Record(_) if t.is_call_error() => "CallError".to_string(),
            Type::Record(record) => match &record.name {
                Some(name) if self.registry.named_types().contains_key(name) => self.name_of(name),
                _ => self.record(record, position),
            },
            Type::Enum(found) if self.registry.named_types().contains_key(&found.name) => self.name_of(&found.name),
            Type::Enum(found) => self.variants(found),
            Type::Nominal(nominal) => self.ty(&nominal.base, position),
            Type::Variable(_) => "unknown".to_string(),
            Type::Never => "never".to_string(),
        }
    }

    fn record(&mut self, record: &RecordType, position: Position) -> String {
        if record.fields.is_empty() {
            return "Record<string, never>".to_string();
        }
        let fields: Vec<String> = record
            .fields
            .iter()
            .map(|(name, field)| match (field, position) {
                (Type::Option(inner), Position::Input) => format!("{}?: {} | null", key(name), self.ty(inner, position)),
                _ => format!("{}: {}", key(name), self.ty(field, position)),
            })
            .collect();
        format!("{{ {} }}", fields.join("; "))
    }

    fn variants(&mut self, found: &EnumType) -> String {
        let shapes: Vec<String> = found
            .variants
            .iter()
            .map(|(name, payload)| match payload {
                None => format!("\"{name}\""),
                Some(payload) => format!("{{ tag: \"{name}\"; value: {} }}", self.ty(payload, Position::Output)),
            })
            .collect();
        shapes.join(" | ")
    }

    fn definition(&mut self, name: &str) -> Option<String> {
        let t = self.registry.named_types().get(name)?.clone();
        let body = match &t {
            Type::Enum(found) => self.variants(found),
            Type::Record(record) => self.record(record, Position::Output),
            other => self.ty(other, Position::Output),
        };
        Some(format!("type {} = {body};", type_name(name)))
    }
}

fn tool_line(writer: &mut Writer<'_>, capability: &Capability, name: &str) -> String {
    let input = match &capability.input {
        Type::Record(record) if record.fields.is_empty() => "input?: Record<string, never>".to_string(),
        other => format!("input: {}", writer.ty(other, Position::Input)),
    };
    let output = writer.ty(&capability.output, Position::Output);
    format!(
        "    /** {} [{}] */\n    {}({input}): Promise<{output}>;",
        comment(&capability.description),
        effect_label(capability.effect, capability.idempotent),
        camel(name)
    )
}

pub fn declarations(registry: &Registry, granted: &dyn Fn(&str) -> bool) -> String {
    let mut writer = Writer { registry, named: Vec::new(), settled: false };
    let mut namespaces: Vec<(String, Vec<String>)> = Vec::new();
    for capability in registry.capabilities().filter(|capability| granted(&capability.id)) {
        let Some((namespace, name)) = capability.id.split_once('.') else {
            continue;
        };
        let line = tool_line(&mut writer, capability, name);
        match namespaces.iter_mut().find(|(held, _)| held == namespace) {
            Some((_, lines)) => lines.push(line),
            None => namespaces.push((namespace.to_string(), vec![line])),
        }
    }
    let mut definitions = Vec::new();
    let mut done: BTreeSet<String> = BTreeSet::new();
    while let Some(name) = writer.named.iter().find(|name| !done.contains(*name)).cloned() {
        done.insert(name.clone());
        definitions.extend(writer.definition(&name));
    }
    let mut parts = vec!["type Int = number;\ntype Float = number;".to_string()];
    if !definitions.is_empty() {
        parts.push(definitions.join("\n"));
    }
    if writer.settled {
        parts.push(SETTLED.to_string());
    }
    parts.push(CALL_ERROR.to_string());
    let mut tools = vec!["interface Tools {".to_string()];
    for (namespace, lines) in &namespaces {
        tools.push(format!("  {}: {{", camel(namespace)));
        tools.extend(lines.iter().cloned());
        tools.push("  };".to_string());
    }
    tools.push("}".to_string());
    parts.push(tools.join("\n"));
    parts.push(BUILTINS.to_string());
    parts.join("\n")
}
