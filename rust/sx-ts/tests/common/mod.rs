#![allow(dead_code)]

use sx_kernel::registry::{Capability, Effect, Registry};
use sx_ts::lower_source;
use sx_ts::pretty::pretty;

fn add(registry: &mut Registry, id: &str, effect: Effect, input: &str, output: &str, idempotent: bool) {
    let input = registry.parse(input).expect("input type");
    let output = registry.parse(output).expect("output type");
    registry
        .add_capability(Capability { id: id.to_string(), effect, input, output, description: "A tool.".to_string(), idempotent, scope: None })
        .expect("capability");
}

pub fn registry() -> Registry {
    let mut registry = Registry::new();
    add(&mut registry, "fs.read-text", Effect::Read, "Record {path: String}", "String", false);
    add(&mut registry, "fs.list", Effect::Read, "Record {path: String}", "List<Record {name: String, size: Int}>", false);
    add(&mut registry, "fs.write-text", Effect::Write, "Record {path: String, text: String}", "Unit", true);
    add(&mut registry, "jobs.status", Effect::Read, "Record {id: String}", "Record {state: String, pct: Int}", false);
    add(
        &mut registry,
        "mcp-github.list-issues",
        Effect::Read,
        "Record {repo: String, cursor: Option<String>}",
        "Record {items: List<Record {id: Int, title: String}>, next: Option<String>}",
        false,
    );
    registry
}

pub fn lowered(source: &str) -> Result<String, Vec<sx_kernel::diagnostics::Diagnostic>> {
    lower_source(source, &registry()).map(|node| pretty(&node))
}

pub fn codes(source: &str) -> Vec<&'static str> {
    match lower_source(source, &registry()) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics.iter().map(|d| d.code).collect(),
    }
}
