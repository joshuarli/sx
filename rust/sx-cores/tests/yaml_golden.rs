use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use sx_cores::kernel::{Cores, yaml_core};
use sx_cores::yaml::{get, keys, set};
use sx_kernel::authority::{Authority, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::compile_source;
use sx_kernel::json::{Json, parse};
use sx_kernel::machine::{Machine, Outcome};
use sx_kernel::registry::Registry;
use sx_kernel::values::show_value;

fn golden(kind: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/yaml_golden").join(kind)
}

fn files(kind: &str, extension: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(golden(kind))
        .expect("golden directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.to_str().is_some_and(|name| name.ends_with(extension)))
        .collect();
    found.sort();
    found
}

fn strings(json: &Json) -> Vec<String> {
    json.as_array().expect("an array").iter().map(|item| item.as_str().expect("a string").to_string()).collect()
}

fn run_cases(document: &str, cases: &Json, label: &str, convert: &dyn Fn(&str) -> String) -> usize {
    let mut counted = 0;
    for case in cases.as_array().expect("cases") {
        counted += 1;
        let context = format!("{label}: {}", sx_kernel::json::dumps(case, sx_kernel::json::Layout::Spaced).expect("json"));
        let field = |name: &str| strings(case.get(name).unwrap_or_else(|| panic!("{context}: no {name}")));
        if let Some(operation) = case.get("error").and_then(Json::as_str) {
            let path = field("path");
            let path: Vec<&str> = path.iter().map(String::as_str).collect();
            let expected = case.get("contains").and_then(Json::as_str).expect("contains");
            let message = match operation {
                "get" => get(document, &path).map(|_| ()).expect_err(&context).to_string(),
                "keys" => keys(document, &path).map(|_| ()).expect_err(&context).to_string(),
                "set" => {
                    let value = case.get("value").and_then(Json::as_str).expect("value");
                    set(document, &path, value).map(|_| ()).expect_err(&context).to_string()
                }
                other => panic!("{context}: unknown operation {other}"),
            };
            assert!(message.contains(expected), "{context}: {message}");
        } else if case.get("get").is_some() {
            let path = field("get");
            let path: Vec<&str> = path.iter().map(String::as_str).collect();
            let expected = case.get("is").and_then(Json::as_str).map(convert);
            assert_eq!(get(document, &path).expect(&context), expected, "{context}");
        } else if case.get("keys").is_some() {
            let path = field("keys");
            let path: Vec<&str> = path.iter().map(String::as_str).collect();
            assert_eq!(keys(document, &path).expect(&context), strings(case.get("is").expect("is")), "{context}");
        } else {
            let path = field("set");
            let path: Vec<&str> = path.iter().map(String::as_str).collect();
            let value = case.get("value").and_then(Json::as_str).expect("value");
            let replace = field("replace");
            let (old, new) = (convert(&replace[0]), convert(&replace[1]));
            assert_eq!(document.matches(&old).count(), 1, "{context}: the pattern must occur once");
            let expected = document.replacen(&old, &new, 1);
            let edited = set(document, &path, value).expect(&context);
            assert_eq!(edited, expected, "{context}");
            assert_eq!(get(&edited, &path).expect(&context).as_deref(), Some(value), "{context}: reads back");
        }
    }
    counted
}

#[test]
fn every_supported_golden_case_reads_and_edits_as_expected() {
    let mut total = 0;
    for path in files("supported", ".yaml") {
        let name = path.file_name().and_then(|n| n.to_str()).expect("name").to_string();
        let document = fs::read_to_string(&path).expect("document");
        let cases_path = path.with_extension("cases.json");
        let cases = parse(&fs::read_to_string(&cases_path).expect("cases")).expect("cases JSON");
        total += run_cases(&document, &cases, &name, &|text| text.to_string());
        let crlf = document.replace('\n', "\r\n");
        total += run_cases(&crlf, &cases, &format!("{name} (CRLF)"), &|text| text.replace('\n', "\r\n"));
    }
    assert!(total >= 150, "only {total} cases ran");
}

#[test]
fn every_refused_golden_case_fails_with_its_reason() {
    let mut total = 0;
    for path in files("refused", ".yaml") {
        let name = path.file_name().and_then(|n| n.to_str()).expect("name").to_string();
        let document = fs::read_to_string(&path).expect("document");
        let reason = fs::read_to_string(path.with_extension("reason")).expect("reason");
        let reason = reason.trim_end();
        for target in [&[][..], &["a"][..]] {
            let outcomes = [
                get(&document, target).map(|_| ()).expect_err(&name).to_string(),
                keys(&document, target).map(|_| ()).expect_err(&name).to_string(),
                set(&document, target, "x").map(|_| ()).expect_err(&name).to_string(),
            ];
            for message in outcomes {
                assert!(message.contains(reason), "{name}: {message:?} lacks {reason:?}");
            }
        }
        total += 1;
    }
    assert!(total >= 35, "only {total} refused documents");
}

#[test]
fn a_refused_document_makes_the_core_fail_as_an_arithmetic_stop() {
    let mut registry = Registry::new();
    registry.add_core(yaml_core()).expect("the yaml core registers");
    let grants = Grants::new();
    let checked = compile_source(
        "(yaml.get \"a: [1]\" [\"a\"])",
        &registry,
        Authority::Granted(&grants),
        &BTreeMap::new(),
        Budgets::default(),
    );
    let program = checked.program.expect("the program checks");
    let mut machine = Machine::new(&program, &[], Budgets::default(), &Cores);
    match machine.start() {
        Outcome::Failed(failure) => {
            assert_eq!(failure.kind.to_string(), "arithmetic");
            assert!(failure.message.starts_with("yaml.get: line 1: flow collections are outside the supported YAML subset"), "{}", failure.message);
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn every_yaml_example_produces_its_documented_result() {
    let mut registry = Registry::new();
    registry.add_core(yaml_core()).expect("the yaml core registers");
    let grants = Grants::new();
    let mut checked_functions = 0;
    for function in registry.functions() {
        let example = function.example.as_ref().expect("every function has an example");
        let compiled = compile_source(
            &example.program,
            &registry,
            Authority::Granted(&grants),
            &BTreeMap::new(),
            Budgets::default(),
        );
        let program = compiled.program.unwrap_or_else(|| panic!("{}: {:?}", function.name, compiled.diagnostics));
        let mut machine = Machine::new(&program, &[], Budgets::default(), &Cores);
        match machine.start() {
            Outcome::Completed { value, .. } => assert_eq!(show_value(&value), example.result, "{}", function.name),
            other => panic!("{}: {other:?}", function.name),
        }
        checked_functions += 1;
    }
    assert_eq!(checked_functions, 3);
}
