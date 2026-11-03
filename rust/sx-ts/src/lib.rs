#![forbid(unsafe_code)]

pub mod analysis;
pub mod ast;
pub mod build;
pub mod calls;
pub mod exprs;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod pretty;
pub mod stmts;

use std::collections::BTreeMap;

use sx_kernel::authority::Authority;
use sx_kernel::budgets::Budgets;
pub use sx_kernel::checker::Dialect;
use sx_kernel::checker::{CheckResult, check};
use sx_kernel::diagnostics::Diagnostic;
use sx_kernel::registry::Registry;
use sx_kernel::syntax::{Node, Position, Span};
use sx_kernel::types::Type;

pub(crate) fn diag(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, span, message).help("typescript")
}


pub fn lower_source(source: &str, registry: &Registry) -> Result<Node, Vec<Diagnostic>> {
    let lowered = sx_kernel::stack::on_sized_stack(|| {
        let program = parser::parse(source)?;
        lower::lower(&program, source, registry)
    });
    match lowered {
        Ok(result) => result,
        Err(error) => {
            let start = Position { line: 1, column: 1 };
            let message = format!("The front end could not start a thread for this program: {error}.");
            Err(vec![Diagnostic::new("too-large", Span::new(start, start), message)])
        }
    }
}

pub fn check_lowered(
    program: &Node,
    registry: &Registry,
    authority: Authority<'_>,
    inputs: &BTreeMap<String, Type>,
    budgets: Budgets,
    dialect: Dialect,
) -> CheckResult {
    check(program, registry, authority, inputs, budgets, dialect)
}

pub fn compile_source(
    source: &str,
    registry: &Registry,
    authority: Authority<'_>,
    inputs: &BTreeMap<String, Type>,
    budgets: Budgets,
) -> CheckResult {
    let size = source.len() as u64;
    if size > budgets.source_bytes {
        let start = Position { line: 1, column: 1 };
        let message = format!("The program is {size} bytes, more than the source-bytes budget of {}.", budgets.source_bytes);
        return rejected(vec![Diagnostic::new("too-large", Span::new(start, start), message)]);
    }
    match lower_source(source, registry) {
        Err(diagnostics) => rejected(diagnostics),
        Ok(program) => check_lowered(&program, registry, authority, inputs, budgets, Dialect::JavaScript),
    }
}

fn rejected(diagnostics: Vec<Diagnostic>) -> CheckResult {
    CheckResult { program: None, diagnostics, header_inputs: None }
}
