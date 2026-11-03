use sx_kernel::help::effect_label;
use sx_kernel::registry::{Capability, Registry};
use sx_kernel::types::{Style, Type, show_definition, show_type, type_name, use_style};

use crate::lower::camel;

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

#[derive(Default)]
struct Used {
    named: Vec<String>,
    settled: bool,
}

fn collect(t: &Type, registry: &Registry, used: &mut Used) {
    match t {
        Type::List(inner) | Type::Map(inner) | Type::Option(inner) => collect(inner, registry, used),
        Type::Tuple(items) => items.iter().for_each(|item| collect(item, registry, used)),
        Type::Result(ok, err) => {
            used.settled = true;
            collect(ok, registry, used);
            collect(err, registry, used);
        }
        Type::Nominal(nominal) => collect(&nominal.base, registry, used),
        Type::Record(record) => {
            if let Some(name) = record.name.as_ref().filter(|name| registry.named_types().contains_key(*name)) {
                if used.named.contains(name) {
                    return;
                }
                used.named.push(name.clone());
            }
            record.fields.iter().for_each(|(_, field)| collect(field, registry, used));
        }
        Type::Enum(found) => {
            if registry.named_types().contains_key(&found.name) && !used.named.contains(&found.name) {
                used.named.push(found.name.clone());
                found.variants.iter().filter_map(|(_, payload)| payload.as_ref()).for_each(|payload| collect(payload, registry, used));
            }
        }
        Type::Primitive(_) | Type::Variable(_) | Type::Never => {}
    }
}

fn tool_line(capability: &Capability, name: &str) -> String {
    let input = {
        let _style = use_style(Style::TypeScriptInput);
        match &capability.input {
            Type::Record(record) if record.fields.is_empty() => "input?: Record<string, never>".to_string(),
            other => format!("input: {}", show_type(other)),
        }
    };
    let output = {
        let _style = use_style(Style::TypeScriptOutput);
        show_type(&capability.output)
    };
    format!(
        "    /** {} [{}] */\n    {}({input}): Promise<{output}>;",
        capability.description.split_whitespace().collect::<Vec<_>>().join(" ").replace("*/", "* /"),
        effect_label(capability.effect, capability.idempotent),
        camel(name)
    )
}

fn definition(name: &str, t: &Type) -> String {
    let body = match t {
        Type::Enum(found) => found
            .variants
            .iter()
            .map(|(variant, payload)| match payload {
                None => format!("\"{variant}\""),
                Some(payload) => format!("{{ tag: \"{variant}\"; value: {} }}", show_type(payload)),
            })
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Record(_) => show_definition(t),
        other => show_type(other),
    };
    format!("type {} = {body};", type_name(name))
}

pub fn declarations(registry: &Registry, granted: &dyn Fn(&str) -> bool) -> String {
    let mut used = Used::default();
    let mut namespaces: Vec<(String, Vec<String>)> = Vec::new();
    for capability in registry.capabilities().filter(|capability| granted(&capability.id)) {
        let Some((namespace, name)) = capability.id.split_once('.') else {
            continue;
        };
        collect(&capability.input, registry, &mut used);
        collect(&capability.output, registry, &mut used);
        let line = tool_line(capability, name);
        match namespaces.iter_mut().find(|(held, _)| held == namespace) {
            Some((_, lines)) => lines.push(line),
            None => namespaces.push((namespace.to_string(), vec![line])),
        }
    }
    let mut parts = vec!["type Int = number;\ntype Float = number;".to_string()];
    let _style = use_style(Style::TypeScriptOutput);
    let definitions: Vec<String> = used
        .named
        .iter()
        .filter_map(|name| registry.named_types().get(name).map(|t| definition(name, t)))
        .collect();
    if !definitions.is_empty() {
        parts.push(definitions.join("\n"));
    }
    if used.settled {
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
