use std::collections::BTreeSet;

use sx_kernel::authority::Grants;
use sx_kernel::help::effect_label;
use sx_kernel::json::Json;
use sx_kernel::registry::{Capability, Registry};
use sx_kernel::types::{EnumType, Primitive, RecordType, Type};

use crate::commands::{Stop, load_registry};
use crate::mcp::scan;
use crate::mcp_serve::grants_of;
use crate::{EXIT_FAILED, EXIT_OK, EXIT_USAGE, Exit};

const GUIDE_HELP: &str = r#"usage: sx guide --declarations FILE [--grants FILE] [--allow ID ...] [--notes]

Print the guide for writing one program against these declarations: the
kernel guide, how TypeScript habits map to Sx, a section for each pack and
core in use, a call template for each granted tool, and how the host treats
the program it is given.

options:
  --declarations FILE  Declarations of the capabilities, packs, and cores
  --grants FILE        JSON object granting capabilities, with their scopes
  --allow ID           Grant one capability that declares no scope
  --notes              Print only the notes on how the granted tools behave,
                       which hold for a program in any language
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    Fs,
    Process,
    Http,
    Sqlite,
    Mcp,
}

impl Family {
    fn sections(self) -> (&'static str, &'static str) {
        match self {
            Family::Fs => (sx_packs::guide::FS, sx_packs::guide::FS_NOTES),
            Family::Process => (sx_packs::guide::PROCESS, sx_packs::guide::PROCESS_NOTES),
            Family::Http => (sx_packs::guide::HTTP, sx_packs::guide::HTTP_NOTES),
            Family::Sqlite => (sx_packs::guide::SQLITE, sx_packs::guide::SQLITE_NOTES),
            Family::Mcp => (sx_packs::guide::MCP, sx_packs::guide::MCP_NOTES),
        }
    }
}

struct Tool<'a> {
    capability: &'a Capability,
    family: Option<Family>,
    endpoint: Option<String>,
}

pub(crate) fn run(args: &[String], new_registry: &dyn Fn() -> Registry) -> Exit {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Exit { code: EXIT_OK, stdout: GUIDE_HELP.to_string(), stderr: String::new() };
    }
    let notes_only = args.iter().any(|arg| arg == "--notes");
    let rest: Vec<String> = args.iter().filter(|arg| *arg != "--notes").cloned().collect();
    let options = match scan(&rest, &["--declarations", "--grants", "--allow"]) {
        Ok(options) => options,
        Err(message) => return guide_usage(&message),
    };
    if let Some(extra) = options.positional.first() {
        return guide_usage(&format!("unrecognized argument {extra}"));
    }
    let Some(path) = options.last("--declarations") else {
        return guide_usage("sx guide needs --declarations FILE");
    };
    let loaded = load_registry(Some(path), new_registry())
        .and_then(|(registry, declarations, _)| grants_of(&options, &registry).map(|grants| (registry, declarations, grants)));
    let (registry, declarations, grants) = match loaded {
        Ok(loaded) => loaded,
        Err(Stop::Usage(message)) => return guide_usage(&message),
        Err(Stop::Internal(message)) => {
            return Exit { code: EXIT_FAILED, stdout: String::new(), stderr: format!("sx guide: internal error: {message}\n") };
        }
    };
    let text = if notes_only {
        let tools = granted_tools(&registry, &declarations, &grants);
        notes(&tools.iter().filter_map(|tool| tool.family).collect())
    } else {
        guide_text(&registry, &declarations, &grants)
    };
    Exit { code: EXIT_OK, stdout: text, stderr: String::new() }
}

pub(crate) fn guide_text(registry: &Registry, declarations: &Json, grants: &Grants) -> String {
    let tools = granted_tools(registry, declarations, grants);
    let families: BTreeSet<Family> = tools.iter().filter_map(|tool| tool.family).collect();
    tailored(registry, declarations, &tools, &families)
}

fn guide_usage(message: &str) -> Exit {
    Exit { code: EXIT_USAGE, stdout: String::new(), stderr: format!("sx guide: {message}\n") }
}

fn declared_entry<'a>(declarations: &'a Json, id: &str) -> Option<&'a Json> {
    declarations
        .get("capabilities")
        .and_then(Json::as_array)
        .and_then(|entries| entries.iter().find(|entry| entry.get("id").and_then(Json::as_str) == Some(id)))
}

fn granted_tools<'a>(registry: &'a Registry, declarations: &Json, grants: &Grants) -> Vec<Tool<'a>> {
    registry
        .capabilities()
        .filter(|capability| grants.contains(&capability.id))
        .map(|capability| {
            let entry = declared_entry(declarations, &capability.id);
            let has = |key: &str| entry.is_some_and(|entry| entry.get(key).is_some());
            let family = match capability.id.split_once('.').map(|(namespace, _)| namespace) {
                Some("fs") => Some(Family::Fs),
                Some("process") => Some(Family::Process),
                Some("http") => Some(Family::Http),
                Some("sqlite") => Some(Family::Sqlite),
                _ if has("http") => Some(Family::Http),
                _ if has("sqlite") => Some(Family::Sqlite),
                _ if has("mcp") => Some(Family::Mcp),
                _ => None,
            };
            let endpoint = entry.and_then(|entry| entry.get("http")).and_then(|http| {
                let method = http.get("method").and_then(Json::as_str)?;
                let url = http.get("url").and_then(Json::as_str)?;
                Some(format!("{method} {url}"))
            });
            Tool { capability, family, endpoint }
        })
        .collect()
}

fn notes(families: &BTreeSet<Family>) -> String {
    let mut parts: Vec<&str> = families.iter().map(|family| family.sections().1.trim_end()).collect();
    parts.retain(|part| !part.is_empty());
    if parts.is_empty() {
        return String::new();
    }
    format!("{}\n", parts.join("\n\n"))
}

fn tailored(registry: &Registry, declarations: &Json, tools: &[Tool<'_>], families: &BTreeSet<Family>) -> String {
    let mut parts = vec![sx_kernel::help::KERNEL.trim_end().to_string(), sx_kernel::help::TYPESCRIPT.trim_end().to_string()];
    for family in families {
        let (section, behavior) = family.sections();
        parts.push(format!("{}\n\n{}", section.trim_end(), behavior.trim_end()));
    }
    for core in declared_cores(declarations) {
        if let Some(section) = core_section(registry, &core) {
            parts.push(section);
        }
    }
    parts.push(tools_section(tools));
    parts.push(sx_kernel::help::WRITING.trim_end().to_string());
    format!("{}\n", parts.join("\n\n"))
}

fn declared_cores(declarations: &Json) -> Vec<String> {
    declarations
        .get("cores")
        .and_then(Json::as_array)
        .map(|cores| cores.iter().filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn core_section(registry: &Registry, id: &str) -> Option<String> {
    let core = registry.core(id)?;
    let mut lines = vec![format!("## The {id} core"), String::new()];
    if let Some(section) = sx_cores::guide::section(id) {
        lines.push(section.trim_end().to_string());
        lines.push(String::new());
    }
    lines.push("Functions:".to_string());
    for function in &core.functions {
        let params = function.params.iter().map(shape).collect::<Vec<_>>().join(" ");
        lines.push(format!("- `({} {params})` returns `{}`. {}", function.name, shape(&function.output), function.description));
    }
    Some(lines.join("\n"))
}

fn tools_section(tools: &[Tool<'_>]) -> String {
    let mut lines = vec!["## Your tools".to_string(), String::new()];
    if tools.is_empty() {
        lines.push("The host grants no tools, so the program calls none.".to_string());
        return lines.join("\n");
    }
    lines.push(
        "These tools are granted, and a call to any other tool is rejected before the program runs. Call one with \
         `(call ID {:field value})`. A field whose type is `Option<...>` may be left out, and is then `none`. Read \
         the result with `get`."
            .to_string(),
    );
    for tool in tools {
        let capability = tool.capability;
        lines.push(String::new());
        lines.push(format!("### {}, {}", capability.id, effect_label(capability.effect, capability.idempotent)));
        lines.push(String::new());
        lines.push(capability.description.trim().to_string());
        if let Some(endpoint) = &tool.endpoint {
            lines.push(format!("It sends `{endpoint}`."));
        }
        lines.push(String::new());
        lines.push("```sx".to_string());
        lines.push(format!("(call {} {})", capability.id, template(&capability.input)));
        lines.push("```".to_string());
        lines.push(String::new());
        lines.push(format!("- Input: `{}`", shape(&capability.input)));
        lines.push(format!("- Result: `{}`", shape(&capability.output)));
        for line in enums_used(&[&capability.input, &capability.output]) {
            lines.push(format!("- {line}"));
        }
    }
    lines.join("\n")
}

fn template(t: &Type) -> String {
    match t {
        Type::Primitive(Primitive::String) => "\"...\"".to_string(),
        Type::Primitive(Primitive::Int) => "0".to_string(),
        Type::Primitive(Primitive::Float) => "0.0".to_string(),
        Type::Primitive(Primitive::Bool) => "false".to_string(),
        Type::Primitive(Primitive::Unit) => "{}".to_string(),
        Type::Primitive(Primitive::Bytes) => "...".to_string(),
        Type::List(element) => format!("[{}]", template(element)),
        Type::Map(_) => "{}".to_string(),
        Type::Option(inner) => template(inner),
        Type::Tuple(items) => format!("[{}]", items.iter().map(template).collect::<Vec<_>>().join(" ")),
        Type::Record(record) => record_template(record),
        Type::Enum(found) => variant_template(found),
        Type::Nominal(nominal) => format!("({}.parse \"...\")", nominal.name.split('.').next().unwrap_or(&nominal.name)),
        Type::Result(..) | Type::Variable(_) | Type::Never => "...".to_string(),
    }
}

fn record_template(record: &RecordType) -> String {
    let fields: Vec<String> = record
        .fields
        .iter()
        .filter(|(_, t)| !matches!(t, Type::Option(_)))
        .map(|(name, t)| format!(":{name} {}", template(t)))
        .collect();
    format!("{{{}}}", fields.join(" "))
}

fn variant_template(found: &EnumType) -> String {
    match found.variants.first() {
        Some((name, None)) if name.starts_with(|c: char| c.is_ascii_uppercase()) => name.clone(),
        Some((name, None)) => format!("\"{name}\""),
        Some((name, Some(_))) => format!("({name} ...)"),
        None => "...".to_string(),
    }
}

fn shape(t: &Type) -> String {
    match t {
        Type::Record(record) if !t.is_call_error() => {
            let fields: Vec<String> = record.fields.iter().map(|(name, field)| format!(":{name} {}", shape(field))).collect();
            let open = if record.open { " ..." } else { "" };
            format!("{{{}{open}}}", fields.join(" "))
        }
        Type::List(element) => format!("List<{}>", shape(element)),
        Type::Map(value) => format!("Map<String, {}>", shape(value)),
        Type::Option(inner) => format!("Option<{}>", shape(inner)),
        Type::Tuple(items) => format!("[{}]", items.iter().map(shape).collect::<Vec<_>>().join(" ")),
        Type::Result(ok, err) => format!("Result<{}, {}>", shape(ok), shape(err)),
        other => sx_kernel::types::show_type(other),
    }
}

fn enums_used(types: &[&Type]) -> Vec<String> {
    let mut found: Vec<std::sync::Arc<EnumType>> = Vec::new();
    for t in types {
        collect_enums(t, &mut found);
    }
    found
        .iter()
        .map(|e| {
            let variants: Vec<String> = e
                .variants
                .iter()
                .map(|(name, payload)| match payload {
                    Some(payload) => format!("({name} {})", shape(payload)),
                    None if name.starts_with(|c: char| c.is_ascii_uppercase()) => name.clone(),
                    None => format!("\"{name}\""),
                })
                .collect();
            format!("`{}` is one of {}", e.name, variants.join(", "))
        })
        .collect()
}

fn collect_enums(t: &Type, found: &mut Vec<std::sync::Arc<EnumType>>) {
    match t {
        Type::Enum(e) => {
            if !found.iter().any(|held| held.name == e.name) {
                found.push(e.clone());
            }
            for (_, payload) in &e.variants {
                if let Some(payload) = payload {
                    collect_enums(payload, found);
                }
            }
        }
        Type::List(inner) | Type::Map(inner) | Type::Option(inner) => collect_enums(inner, found),
        Type::Tuple(items) => items.iter().for_each(|item| collect_enums(item, found)),
        Type::Result(ok, err) => {
            collect_enums(ok, found);
            collect_enums(err, found);
        }
        Type::Record(record) => record.fields.iter().for_each(|(_, field)| collect_enums(field, found)),
        Type::Primitive(_) | Type::Nominal(_) | Type::Variable(_) | Type::Never => {}
    }
}
