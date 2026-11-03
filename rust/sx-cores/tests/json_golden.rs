use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use sx_cores::json::{Kind, get, keys, kind};
use sx_cores::kernel::{Cores, json_core};
use sx_kernel::authority::{Authority, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::{Dialect, compile_source};
use sx_kernel::json::{Json, MAX_DEPTH, parse};
use sx_kernel::machine::{Machine, Outcome};
use sx_kernel::registry::Registry;
use sx_kernel::values::show_value;

fn cases(name: &str) -> Vec<Json> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/json_golden").join(name);
    let text = fs::read_to_string(path).expect("golden file");
    parse(&text).expect("golden JSON").as_array().expect("an array of cases").to_vec()
}

fn strings(json: &Json) -> Vec<String> {
    json.as_array().expect("an array").iter().map(|item| item.as_str().expect("a string").to_string()).collect()
}

fn text_of(case: &Json) -> &str {
    case.get("text").and_then(Json::as_str).expect("text")
}

#[test]
fn every_supported_golden_case_reads_as_expected() {
    let mut total = 0;
    for case in cases("supported.json") {
        let name = case.get("name").and_then(Json::as_str).expect("name");
        let text = text_of(&case);
        for check in case.get("checks").and_then(Json::as_array).expect("checks") {
            total += 1;
            let context = format!("{name}: {}", sx_kernel::json::dumps(check, sx_kernel::json::Layout::Spaced).expect("json"));
            let expected = check.get("is").expect("is");
            if let Some(path) = check.get("get") {
                let path = strings(path);
                let path: Vec<&str> = path.iter().map(String::as_str).collect();
                assert_eq!(get(text, &path).expect(&context).as_deref(), expected.as_str(), "{context}");
            } else if let Some(path) = check.get("keys") {
                let path = strings(path);
                let path: Vec<&str> = path.iter().map(String::as_str).collect();
                assert_eq!(keys(text, &path).expect(&context), strings(expected), "{context}");
            } else {
                let path = strings(check.get("kind").expect("kind"));
                let path: Vec<&str> = path.iter().map(String::as_str).collect();
                let wanted = Kind::from_variant(expected.as_str().expect("a kind name")).expect("a known kind");
                assert_eq!(kind(text, &path).expect(&context), wanted, "{context}");
            }
        }
    }
    assert!(total >= 70, "only {total} checks ran");
}

#[test]
fn every_refused_golden_case_fails_with_its_reason() {
    let mut total = 0;
    for case in cases("refused.json") {
        let text = text_of(&case);
        let at = case.get("at").and_then(Json::as_str).expect("at");
        let reason = case.get("reason").and_then(Json::as_str).expect("reason");
        let expected = format!("{at}: {reason}");
        for target in [&[][..], &["a"][..]] {
            let outcomes = [
                get(text, target).map(|_| ()).expect_err(text).to_string(),
                keys(text, target).map(|_| ()).expect_err(text).to_string(),
                kind(text, target).map(|_| ()).expect_err(text).to_string(),
            ];
            for message in outcomes {
                assert_eq!(message, expected, "{text:?}");
            }
        }
        total += 1;
    }
    assert!(total >= 45, "only {total} refused documents");
}

#[test]
fn a_path_that_names_a_container_or_a_scalar_is_refused_for_the_wrong_function() {
    let text = "{\"o\": {\"k\": 1}, \"a\": [1], \"s\": \"x\", \"n\": 2, \"t\": true}";
    assert_eq!(get(text, &["o"]).expect_err("object").to_string(), "the path \"o\" is an object, not a scalar");
    assert_eq!(get(text, &["a"]).expect_err("array").to_string(), "the path \"a\" is an array, not a scalar");
    assert_eq!(get(text, &[]).expect_err("root").to_string(), "the document root is an object, not a scalar");
    for scalar in ["s", "n", "t"] {
        let message = keys(text, &[scalar]).expect_err("scalar").to_string();
        assert_eq!(message, format!("the path {scalar:?} is a scalar, so it has no keys"));
    }
}

#[test]
fn nesting_is_accepted_to_the_kernel_depth_limit_and_refused_beyond_it() {
    let nested = |levels: usize| format!("{}1{}", "[".repeat(levels), "]".repeat(levels));
    assert_eq!(kind(&nested(MAX_DEPTH), &[]), Ok(Kind::Array));
    let refused = kind(&nested(MAX_DEPTH + 1), &[]).expect_err("too deep").to_string();
    assert_eq!(refused, format!("line 1 column {}: the value nests deeper than {MAX_DEPTH} levels", MAX_DEPTH + 1));
    let objects = format!("{}1{}", "{\"a\":".repeat(MAX_DEPTH + 1), "}".repeat(MAX_DEPTH + 1));
    assert!(kind(&objects, &[]).expect_err("too deep").to_string().contains("nests deeper"));
}

fn run(source: &str) -> Outcome {
    let mut registry = Registry::new();
    registry.add_core(json_core()).expect("the json core registers");
    let grants = Grants::new();
    let checked =
        compile_source(source, &registry, Authority::Granted(&grants), &BTreeMap::new(), Budgets::default(), Dialect::Sx);
    let program = checked.program.unwrap_or_else(|| panic!("{source}: {:?}", checked.diagnostics));
    let mut machine = Machine::new(&program, &[], Budgets::default(), &Cores);
    machine.start()
}

#[test]
fn a_refused_document_stops_the_program_as_an_invalid_input_failure() {
    match run("(json.get \"{\\\"a\\\": [1,]}\" [\"a\"])") {
        Outcome::Failed(failure) => {
            assert_eq!(failure.kind.to_string(), "invalid-input");
            assert_eq!(failure.message, "json.get: line 1 column 9: a trailing comma is not allowed");
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn every_json_example_produces_its_documented_result() {
    let mut registry = Registry::new();
    registry.add_core(json_core()).expect("the json core registers");
    let mut checked_functions = 0;
    for function in registry.functions() {
        let example = function.example.as_ref().expect("every function has an example");
        match run(&example.program) {
            Outcome::Completed { value, .. } => assert_eq!(show_value(&value), example.result, "{}", function.name),
            other => panic!("{}: {other:?}", function.name),
        }
        checked_functions += 1;
    }
    assert_eq!(checked_functions, 3);
}
