use std::collections::BTreeMap;

use crate::diagnostics::Diagnostic;
use crate::registry::Registry;
use crate::syntax::{Node, NodeKind, Span};
use crate::types::{Type, encodable_problem, show_type};

use super::{closest, form_items, is_reserved, suggested_name, symbol};

pub const SCRIPT: &str = "script";
pub const HEADER_KEYS: [&str; 1] = ["inputs"];
const SHAPE: &str = "(script {:inputs {:name \"Type\"}} body)";

pub struct Script<'a> {
    pub body: Option<&'a Node>,
    pub inputs: Vec<(String, Type)>,
    pub unusable: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn is_script(program: &Node) -> bool {
    form_items(program).and_then(|items| items.first()).and_then(symbol) == Some(SCRIPT)
}

pub fn read<'a>(program: &'a Node, registry: &Registry) -> Script<'a> {
    let mut script = Script { body: None, inputs: Vec::new(), unusable: Vec::new(), diagnostics: Vec::new() };
    let items = form_items(program).unwrap_or_default();
    if items.len() != 3 {
        script.diagnostics.push(
            Diagnostic::new("arity", program.span, format!("script takes a header record and a body: {SHAPE}."))
                .detail(format!("Found: {}", plural(items.len().saturating_sub(1), "argument")))
                .help(SCRIPT),
        );
        return script;
    }
    script.body = items.get(2);
    let Some(header) = items.get(1) else {
        return script;
    };
    let NodeKind::Record(fields) = &header.kind else {
        script.diagnostics.push(
            Diagnostic::new("script-header", header.span, "The header of a script is a record literal.")
                .detail(format!("Write: {SHAPE}"))
                .help(SCRIPT),
        );
        return script;
    };
    for field in fields.iter() {
        if field.key.name != HEADER_KEYS[0] {
            let suggestion = closest(&field.key.name, &HEADER_KEYS);
            script.diagnostics.push(
                Diagnostic::new(
                    "script-key",
                    field.key.span,
                    format!("A script header has no key :{}.", field.key.name),
                )
                .detail(format!("Keys: {}", HEADER_KEYS.map(|key| format!(":{key}")).join(", ")))
                .maybe_fix(suggestion.map(|key| format!(":{key}")))
                .help(SCRIPT),
            );
            continue;
        }
        read_inputs(&field.value, registry, &mut script);
    }
    script
}

fn read_inputs(declared: &Node, registry: &Registry, script: &mut Script<'_>) {
    let NodeKind::Record(fields) = &declared.kind else {
        script.diagnostics.push(
            Diagnostic::new("script-header", declared.span, "The value of :inputs is a record of input names and types.")
                .detail("Write: {:owners \"List<String>\" :limit \"Int\"}")
                .help(SCRIPT),
        );
        return;
    };
    for field in fields.iter() {
        let name = field.key.name.as_str();
        if is_reserved(name) || name == SCRIPT {
            script.diagnostics.push(
                Diagnostic::new(
                    "reserved-name",
                    field.key.span,
                    format!("{name} is the name of a form, so it cannot name an input."),
                )
                .fix(format!(":{}", suggested_name(name)))
                .help(SCRIPT),
            );
            script.unusable.push(name.to_string());
            continue;
        }
        match input_type(name, &field.value, registry) {
            Ok(declared) => script.inputs.push((name.to_string(), declared)),
            Err(diagnostic) => {
                script.diagnostics.push(*diagnostic);
                script.unusable.push(name.to_string());
            }
        }
    }
}

fn input_type(name: &str, node: &Node, registry: &Registry) -> Result<Type, Box<Diagnostic>> {
    let NodeKind::String(written) = &node.kind else {
        return Err(Box::new(
            Diagnostic::new(
                "input-type",
                node.span,
                format!("The type of input :{name} must be a string literal, such as \"List<String>\"."),
            )
            .detail("A header states each type in the declaration type language, as text.")
            .help(SCRIPT),
        ));
    };
    let parsed = registry.parse(written).map_err(|error| {
        Box::new(
            Diagnostic::new("input-type", node.span, format!("The type of input :{name}, {written:?}, does not parse."))
                .detail(format!("Parser: {error}"))
                .help(SCRIPT),
        )
    })?;
    if let Some(problem) = encodable_problem(&parsed) {
        let reason = problem.split_once(" has no JSON encoding").map_or("", |(_, reason)| reason);
        return Err(Box::new(
            Diagnostic::new(
                "no-encoding",
                node.span,
                format!("Input :{name} is {}, which has no JSON encoding{reason}.", show_type(&parsed)),
            )
            .help(SCRIPT),
        ));
    }
    Ok(parsed)
}

pub fn position_diagnostic(span: Span) -> Diagnostic {
    Diagnostic::new("script-position", span, "script is the header of a whole program, so only the outermost form can be one.")
        .detail(format!("Write: {SHAPE}"))
        .help(SCRIPT)
}

pub fn disagreement(header: &BTreeMap<String, Type>, host: &BTreeMap<String, Type>) -> Option<String> {
    if host.is_empty() || header == host {
        return None;
    }
    let mut differences = Vec::new();
    for (name, declared) in header {
        match host.get(name) {
            None => differences.push(format!(
                "the script's header declares input {name} ({}), which the declarations do not",
                show_type(declared)
            )),
            Some(other) if other != declared => differences.push(format!(
                "input {name} is {} in the script's header and {} in the declarations",
                show_type(declared),
                show_type(other)
            )),
            Some(_) => {}
        }
    }
    for (name, declared) in host {
        if !header.contains_key(name) {
            differences.push(format!(
                "the declarations declare input {name} ({}), which the script's header does not",
                show_type(declared)
            ));
        }
    }
    Some(format!("the script's inputs and the declarations' inputs disagree: {}", differences.join("; ")))
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("{count} {noun}") } else { format!("{count} {noun}s") }
}
