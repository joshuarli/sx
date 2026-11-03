use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use sx_cores::kernel::{Cores, decimal_core};
use sx_kernel::authority::{Authority, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::{Dialect, compile_source};
use sx_kernel::json::{Json, Layout, dumps, parse, python_equal};
use sx_kernel::machine::{CoreError, CoreImplementations, Machine, Outcome};
use sx_kernel::registry::Registry;
use sx_kernel::values::{decode, encode, show_value};

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.add_core(decimal_core()).expect("the decimal core registers");
    registry
}

#[test]
fn every_decimal_vector_holds_through_the_kernel() {
    let registry = registry();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cores/decimal.json");
    let file = parse(&fs::read_to_string(path).expect("vector file")).expect("vector JSON");
    assert_eq!(file.get("core").and_then(Json::as_str), Some("decimal"));
    let core = registry.core("decimal").expect("registered core");
    assert_eq!(file.get("version").and_then(Json::as_str), Some(core.version.as_str()));
    let vectors = file.get("vectors").and_then(Json::as_array).expect("vectors");
    let named: std::collections::BTreeSet<&str> =
        vectors.iter().filter_map(|vector| vector.get("function").and_then(Json::as_str)).collect();
    let declared: std::collections::BTreeSet<&str> = core.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(named, declared, "every function has vectors");
    let mut failures = Vec::new();
    for vector in vectors {
        let name = vector.get("function").and_then(Json::as_str).expect("function name");
        let function = registry.function(name).expect("registered function");
        let shown = || dumps(vector, Layout::Spaced).unwrap_or_default();
        let args = vector.get("args").and_then(Json::as_array).expect("args");
        let decoded: Result<Vec<_>, _> =
            args.iter().zip(&function.params).map(|(arg, t)| decode(arg, t, "$")).collect();
        let Ok(values) = decoded else {
            failures.push(format!("{}: an argument does not decode", shown()));
            continue;
        };
        match (vector.get("result"), vector.get("fails"), Cores.apply(name, &values)) {
            (Some(expected), None, Ok(value)) => {
                let actual = encode(&value);
                if !python_equal(&actual, expected) {
                    failures.push(format!("{}: got {}", shown(), dumps(&actual, Layout::Spaced).unwrap_or_default()));
                }
            }
            (None, Some(Json::String(kind)), Err(CoreError::Arithmetic(_))) if kind == "arithmetic" => {}
            (_, _, other) => failures.push(format!("{}: got {other:?}", shown())),
        }
    }
    eprintln!("{} decimal vectors hold through the kernel", vectors.len());
    assert!(vectors.len() > 5000);
    assert!(failures.is_empty(), "{} of {} vectors differ:\n{}", failures.len(), vectors.len(), failures.join("\n"));
}

#[test]
fn every_decimal_example_produces_its_documented_result() {
    let registry = registry();
    let grants = Grants::new();
    for function in registry.functions() {
        let example = function.example.as_ref().expect("every function has an example");
        let checked = compile_source(
            &example.program,
            &registry,
            Authority::Granted(&grants),
            &BTreeMap::new(),
            Budgets::default(), Dialect::Sx,
        );
        let program = checked.program.unwrap_or_else(|| panic!("{}: {:?}", function.name, checked.diagnostics));
        let mut machine = Machine::new(&program, &[], Budgets::default(), &Cores);
        match machine.start() {
            Outcome::Completed { value, .. } => assert_eq!(show_value(&value), example.result, "{}", function.name),
            other => panic!("{}: {other:?}", function.name),
        }
    }
}

#[test]
fn a_decimal_json_number_and_a_malformed_string_are_decode_errors_with_their_path() {
    let registry = registry();
    let t = registry.parse("List<decimal.Decimal>").expect("the type parses");
    let number = parse("[\"1.5\", 2]").expect("json");
    let error = decode(&number, &t, "$").expect_err("a number is not a decimal");
    assert!(error.to_string().starts_with("$[1]: expected decimal.Decimal (a string), found an integer"), "{error}");
    let malformed = parse("[\"1.5\", \"1e3\"]").expect("json");
    let error = decode(&malformed, &t, "$").expect_err("an exponent is not a decimal literal");
    assert!(error.to_string().starts_with("$[1]: expected decimal.Decimal, found a string that is not valid"), "{error}");
    let canonical = decode(&parse("[\"1.50\", \"-0\"]").expect("json"), &t, "$").expect("both decode");
    assert_eq!(dumps(&encode(&canonical), Layout::Spaced).expect("json"), "[\"1.5\", \"0\"]");
}
