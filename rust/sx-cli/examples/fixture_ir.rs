use std::collections::BTreeMap;
use std::fs;
use std::process::ExitCode;

use sx_cores::kernel::{Cores, declared_registry};
use sx_kernel::authority::{Authority, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::{Dialect, compile_source};
use sx_kernel::diagnostics::render;
use sx_kernel::export::export_program;
use sx_kernel::host::{FixtureHost, run};
use sx_kernel::json::{Json, Layout, dumps, parse};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [mode, path] = args.as_slice() else {
        eprintln!("usage: fixture_ir (ir|run) FIXTURE.json");
        return ExitCode::from(2);
    };
    match work(mode, path) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn work(mode: &str, path: &str) -> Result<String, String> {
    let read = |path: &std::path::Path| -> Result<Json, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    };
    let fixture_path = std::path::Path::new(path);
    let fixture = read(fixture_path)?;
    let directory = fixture_path.parent().ok_or("the fixture has no directory")?;
    let source = fixture.get("program").and_then(Json::as_str).ok_or("no program")?;
    let declarations = fixture.get("declarations").and_then(Json::as_str).ok_or("no declarations")?;
    let registry = declared_registry(&read(&directory.join(declarations))?).map_err(|e| e.to_string())?;
    let dialect = match fixture.get("dialect").and_then(Json::as_str) {
        Some("js") => Dialect::JavaScript,
        _ => Dialect::Sx,
    };
    let mut grants = Grants::new();
    for id in fixture.get("allow").and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(Json::as_str) {
        grants.grant_unscoped(&registry, id).map_err(|e| e.to_string())?;
    }
    let checked =
        compile_source(source, &registry, Authority::Granted(&grants), &BTreeMap::new(), Budgets::default(), dialect);
    let Some(program) = checked.program else {
        return Err(render(source, &checked.diagnostics));
    };
    if mode == "ir" {
        return dumps(&export_program(&program), Layout::Spaced).map_err(|e| e.to_string());
    }
    let responses = fixture.get("responses").cloned().unwrap_or(Json::Array(Vec::new()));
    let mut host =
        FixtureHost::from_responses(&program.signatures.capabilities, &responses).map_err(|e| e.to_string())?;
    let finished = run(&program, &grants, &mut host, &[], Budgets::default(), &Cores).map_err(|e| e.to_string())?;
    let outcome = match &finished.outcome {
        sx_kernel::machine::Outcome::Completed { json, .. } => format!("completed {}", dumps(json, Layout::Spaced).unwrap_or_default()),
        sx_kernel::machine::Outcome::Failed(failure) => format!("failed {} {}", failure.kind.name(), failure.message),
        sx_kernel::machine::Outcome::Suspended(_) => "suspended".to_string(),
    };
    let trace = dumps(&Json::Array(finished.machine.trace().to_vec()), Layout::Spaced).unwrap_or_default();
    Ok(format!("{outcome}\nlogs {:?}\ntrace {trace}", finished.machine.logs()))
}
