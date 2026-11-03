use std::collections::BTreeMap;

use sx_cores::js::{JsError, float_text, index_of, length, order_key, pad_end, pad_start, parse_int, replace_first, slice, stringify, trim};
use sx_cores::kernel::{Cores, js_core};
use sx_kernel::authority::{Authority, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::{Dialect, compile_source};
use sx_kernel::machine::{Machine, Outcome};
use sx_kernel::registry::Registry;
use sx_kernel::values::{Value, show_value};

#[test]
fn lengths_and_indexes_count_utf16_units() {
    assert_eq!(length("a😀"), 3);
    assert_eq!(length("é"), 1);
    assert_eq!(index_of("😀b😀b", "b"), 2);
    assert_eq!(index_of("😀b😀b", "😀b"), 0);
    assert_eq!(index_of("abc", ""), 0);
    assert_eq!(index_of("abc", "d"), -1);
}

#[test]
fn slices_follow_javascript_indexes_and_trap_inside_a_pair() {
    assert_eq!(slice("hello", -3, 5), Ok("llo".to_string()));
    assert_eq!(slice("hello", 3, 1), Ok(String::new()));
    assert_eq!(slice("hello", -99, 99), Ok("hello".to_string()));
    assert_eq!(slice("a😀b", 1, 3), Ok("😀".to_string()));
    assert!(matches!(slice("a😀b", 0, 2), Err(JsError::Trap(_))));
    assert!(matches!(slice("a😀b", 2, 4), Err(JsError::Trap(_))));
}

#[test]
fn trimming_removes_exactly_ecmascript_whitespace() {
    assert_eq!(trim("\u{feff}\u{a0}\u{2028} x\u{3000}\t"), "x");
    assert_eq!(trim("\u{200b}x\u{200b}"), "\u{200b}x\u{200b}");
    assert_eq!(trim("\u{85}x"), "\u{85}x");
}

#[test]
fn padding_cuts_the_filler_by_utf16_units() {
    assert_eq!(pad_start("5", 4, "ab"), Ok("aba5".to_string()));
    assert_eq!(pad_end("5", 4, "ab"), Ok("5aba".to_string()));
    assert_eq!(pad_start("abc", 2, "x"), Ok("abc".to_string()));
    assert_eq!(pad_start("a", 4, ""), Ok("a".to_string()));
    assert!(matches!(pad_start("a", 4, "😀"), Err(JsError::Trap(_))));
    assert_eq!(pad_start("a", 3, "😀"), Ok("😀a".to_string()));
}

#[test]
fn replace_first_refuses_dollar_patterns_and_replaces_once() {
    assert_eq!(replace_first("a.a", "a", "b"), Ok("b.a".to_string()));
    assert_eq!(replace_first("abc", "", "-"), Ok("-abc".to_string()));
    assert!(matches!(replace_first("abc", "b", "$1"), Err(JsError::Refused(_))));
}

#[test]
fn parse_int_reads_the_digit_prefix_and_traps_without_one() {
    assert_eq!(parse_int("\u{a0} -12abc"), Ok(-12));
    assert_eq!(parse_int("+0"), Ok(0));
    assert!(matches!(parse_int("abc"), Err(JsError::Trap(_))));
    assert!(matches!(parse_int("-"), Err(JsError::Trap(_))));
    assert!(matches!(parse_int("9007199254740993"), Err(JsError::Trap(_))));
    assert_eq!(parse_int("9007199254740992"), Ok(9_007_199_254_740_992));
}

#[test]
fn floats_are_written_as_ecmascript_writes_them() {
    let cases = [
        (1.0, "1"),
        (-0.0, "0"),
        (1.5, "1.5"),
        (0.1 + 0.2, "0.30000000000000004"),
        (123456789012345680000.0, "123456789012345680000"),
        (1e21, "1e+21"),
        (1.5e21, "1.5e+21"),
        (0.000001, "0.000001"),
        (1e-7, "1e-7"),
        (-1.25e-7, "-1.25e-7"),
        (5e-324, "5e-324"),
        (1.7976931348623157e308, "1.7976931348623157e+308"),
    ];
    for (value, text) in cases {
        assert_eq!(float_text(value), text, "{value:e}");
    }
}

#[test]
fn order_keys_sort_by_utf16_unit() {
    let mut strings = vec!["\u{ff5e}", "\u{1f600}", "a", "\u{e000}", "\u{10000}"];
    strings.sort_by_key(|s| order_key(s));
    assert_eq!(strings, vec!["a", "\u{10000}", "\u{1f600}", "\u{e000}", "\u{ff5e}"]);
    let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let mut by_units = strings.clone();
    by_units.sort_by_key(|s| units(s));
    assert_eq!(by_units, strings);
}

#[test]
fn stringify_keeps_the_order_a_record_holds() {
    let record = Value::Record(sx_kernel::values::RecordValue::new(vec![
        ("z".into(), Value::Int(1)),
        ("a".into(), Value::list(vec![Value::string("q\"\u{1}"), Value::None, Value::some(Value::float(2.0))])),
    ]));
    assert_eq!(stringify(&record, 0), Ok("{\"z\":1,\"a\":[\"q\\\"\\u0001\",null,2]}".to_string()));
    assert_eq!(
        stringify(&record, 2),
        Ok("{\n  \"z\": 1,\n  \"a\": [\n    \"q\\\"\\u0001\",\n    null,\n    2\n  ]\n}".to_string())
    );
    assert_eq!(stringify(&Value::list(Vec::new()), 4), Ok("[]".to_string()));
}

fn run(source: &str) -> Outcome {
    let mut registry = Registry::new();
    registry.add_core(js_core()).expect("the js core registers");
    let grants = Grants::new();
    let checked =
        compile_source(source, &registry, Authority::Granted(&grants), &BTreeMap::new(), Budgets::default(), Dialect::Sx);
    let program = checked.program.unwrap_or_else(|| panic!("{source}: {:?}", checked.diagnostics));
    Machine::new(&program, &[], Budgets::default(), &Cores).start()
}

#[test]
fn every_js_example_produces_its_documented_result() {
    let mut registry = Registry::new();
    registry.add_core(js_core()).expect("the js core registers");
    for function in registry.functions() {
        let example = function.example.as_ref().expect("every function has an example");
        match run(&example.program) {
            Outcome::Completed { value, .. } => assert_eq!(show_value(&value), example.result, "{}", function.name),
            other => panic!("{}: {other:?}", function.name),
        }
    }
}

#[test]
fn a_trap_is_an_arithmetic_failure_and_a_refusal_is_an_invalid_input() {
    match run("(js.slice \"a😀\" 0 2)") {
        Outcome::Failed(failure) => assert_eq!(failure.kind.to_string(), "arithmetic"),
        other => panic!("expected a failure, got {other:?}"),
    }
    match run("(js.replace-first \"a\" \"a\" \"$\")") {
        Outcome::Failed(failure) => assert_eq!(failure.kind.to_string(), "invalid-input"),
        other => panic!("expected a failure, got {other:?}"),
    }
}
