use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use sx_kernel::budgets::Budgets;
use sx_kernel::export::{read_program_text, read_type};
use sx_kernel::host::{FixtureHost, parse_result, run};
use sx_kernel::json::{Json, Layout, dumps, parse, python_equal};
use sx_kernel::machine::{Machine, NoCores, Outcome, Response};
use sx_kernel::values::{decode, encode, show_value};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn exported() -> &'static Path {
    static EXPORTED: OnceLock<PathBuf> = OnceLock::new();
    EXPORTED.get_or_init(|| {
        let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fixture-ir");
        let status = Command::new("uv")
            .args(["run", "--quiet", "python", "rust/scripts/export_fixtures.py"])
            .arg(&target)
            .current_dir(root())
            .status()
            .expect("uv runs the Python exporter");
        assert!(status.success(), "rust/scripts/export_fixtures.py failed");
        target
    })
}

fn read_json(path: &Path) -> Json {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn cases() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(root().join("fixtures/run"))
        .expect("fixtures/run exists")
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "json") && !path.ends_with("declarations.json"))
        .collect();
    found.sort();
    found
}

fn budgets_of(case: &Json) -> Budgets {
    let mut budgets = Budgets::default();
    if let Some(overrides) = case.get("budgets").and_then(Json::as_object) {
        for (name, value) in overrides {
            let value = value.as_i64().and_then(|v| u64::try_from(v).ok()).expect("budget is a count");
            budgets.set(name, value).expect("known budget");
        }
    }
    budgets
}

fn expectation(outcome: &Outcome) -> Json {
    match outcome {
        Outcome::Completed { json, .. } => Json::object([("completed", json.clone())]),
        Outcome::Failed(failure) => Json::object([("failed", Json::string(failure.kind.name()))]),
        Outcome::Suspended(requests) => {
            let ids: Vec<Json> = requests.iter().map(|r| Json::string(r.id())).collect();
            Json::object([("suspended", Json::Array(ids))])
        }
    }
}

fn detailed(outcome: &Outcome) -> Json {
    match outcome {
        Outcome::Completed { json, value } => {
            Json::object([("completed", json.clone()), ("shown", Json::string(show_value(value)))])
        }
        Outcome::Failed(failure) => {
            let span = failure.span.map_or(Json::Null, |s| {
                let n = |v: usize| Json::Int(i64::try_from(v).expect("span fits"));
                Json::Array(vec![n(s.start.line), n(s.start.column), n(s.end.line), n(s.end.column)])
            });
            Json::object([
                ("failed", Json::string(failure.kind.name())),
                ("message", Json::string(failure.message.as_str())),
                ("span", span),
                ("request", failure.request.as_deref().map_or(Json::Null, Json::string)),
            ])
        }
        Outcome::Suspended(_) => expectation(outcome),
    }
}

fn shown(json: &Json) -> String {
    dumps(json, Layout::Spaced).expect("serializable")
}

fn run_case(path: &Path, case: &Json, ir: &Path, python: &Json) -> Result<(), String> {
    let text = fs::read_to_string(ir).map_err(|e| format!("no exported IR: {e}"))?;
    let program = read_program_text(&text).map_err(|e| e.to_string())?;
    let budgets = budgets_of(case);
    let cores = NoCores;
    let (outcome, trace) = if let Some(steps) = case.get("steps").and_then(Json::as_array) {
        let mut machine = Machine::new(&program, &[], budgets, &cores);
        let mut outcome = machine.start();
        for step in steps {
            let Outcome::Suspended(requests) = &outcome else {
                return Err(format!("expected a suspension, got {}", shown(&expectation(&outcome))));
            };
            let ids: Vec<&str> = requests.iter().map(|r| r.id()).collect();
            let wanted: Vec<&str> =
                step.get("requests").and_then(Json::as_array).expect("step requests").iter().filter_map(Json::as_str).collect();
            if ids != wanted {
                return Err(format!("suspended with {ids:?}, expected {wanted:?}"));
            }
            let mut responses = Vec::new();
            for pair in step.get("respond").and_then(Json::as_array).expect("step responses") {
                let [id, entry] = pair.as_array().expect("response pair") else {
                    return Err("a response is not a pair".to_string());
                };
                let result = parse_result(entry).map_err(|e| e.to_string())?;
                let response: Response = result.response();
                responses.push((id.as_str().expect("response id").to_string(), response));
            }
            outcome = machine.resume(responses);
        }
        (outcome, machine.trace().to_vec())
    } else {
        let responses = case.get("responses").cloned().unwrap_or(Json::Array(Vec::new()));
        let mut host =
            FixtureHost::from_responses(&program.signatures.capabilities, &responses).map_err(|e| e.to_string())?;
        let finished = run(&program, &mut host, &[], budgets, &cores).map_err(|e| format!("unexpected call: {e}"))?;
        let trace = finished.machine.trace().to_vec();
        (finished.outcome, trace)
    };
    let expected = case.get("expect").expect("expect");
    let actual = expectation(&outcome);
    if !python_equal(&actual, expected) {
        return Err(format!("{}: outcome {} != {}", path.display(), shown(&actual), shown(expected)));
    }
    let actual_detail = detailed(&outcome);
    if !python_equal(&actual_detail, python) || shown(&actual_detail) != shown(python) {
        return Err(format!("outcome {} != Python's {}", shown(&actual_detail), shown(python)));
    }
    let wanted_trace = case.get("trace").cloned().unwrap_or(Json::Array(Vec::new()));
    let actual_trace = Json::Array(trace);
    if !python_equal(&actual_trace, &wanted_trace) {
        return Err(format!("trace {} != {}", shown(&actual_trace), shown(&wanted_trace)));
    }
    Ok(())
}

#[test]
fn every_accepted_run_fixture_matches_outcome_and_trace() {
    let target = exported();
    let mut compared = 0;
    let mut rejected = 0;
    let mut failures = Vec::new();
    for path in cases() {
        let case = read_json(&path);
        if case.get("python").and_then(Json::as_bool) == Some(false) {
            continue;
        }
        if case.get("expect").and_then(|e| e.get("rejected")).is_some() {
            rejected += 1;
            continue;
        }
        compared += 1;
        let stem = path.file_stem().and_then(|s| s.to_str()).expect("fixture name");
        let python = read_json(&target.join(format!("{stem}.outcome.json")));
        if let Err(problem) = run_case(&path, &case, &target.join(format!("{stem}.ir.json")), &python) {
            failures.push(format!("{stem}: {problem}"));
        }
    }
    eprintln!("compared {compared} accepted run fixtures, {rejected} rejected ones are elaborator behavior");
    assert!(compared > 0);
    assert!(failures.is_empty(), "{} of {compared} disagree:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn every_fixture_value_decodes_and_encodes_as_python_does() {
    let values = read_json(&exported().join("values.json"));
    let cases = values.as_array().expect("value cases");
    assert!(!cases.is_empty());
    let mut failures = Vec::new();
    for case in cases {
        let get = |key: &str| case.get(key).expect("value case field");
        let t = read_type(get("type"), get("enums")).expect("exported type reads");
        let value = get("value");
        let actual = match decode(value, &t, "$") {
            Ok(decoded) => Ok(shown(&encode(&decoded, &t))),
            Err(error) => Err(error.to_string()),
        };
        let expected = match (get("decoded"), get("error")) {
            (Json::String(text), Json::Null) => Ok(text.clone()),
            (Json::Null, Json::String(error)) => Err(error.clone()),
            _ => panic!("value case has neither a result nor an error"),
        };
        if actual != expected {
            failures.push(format!("{}: {actual:?} != {expected:?}", shown(value)));
        }
    }
    assert!(failures.is_empty(), "{} of {} values differ:\n{}", failures.len(), cases.len(), failures.join("\n"));
}
