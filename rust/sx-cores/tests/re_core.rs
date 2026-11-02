use std::collections::BTreeMap;

use sx_cores::kernel::{Cores, re_core};
use sx_kernel::authority::{Authority, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::compile_source;
use sx_kernel::machine::{Machine, Outcome};
use sx_kernel::registry::Registry;
use sx_kernel::values::show_value;

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.add_core(re_core()).expect("the re core registers");
    registry
}

fn run(source: &str) -> Result<String, String> {
    let registry = registry();
    let grants = Grants::new();
    let compiled = compile_source(source, &registry, Authority::Granted(&grants), &BTreeMap::new(), Budgets::default());
    let program = compiled
        .program
        .ok_or_else(|| compiled.diagnostics.iter().map(|d| format!("{}: {}", d.code, d.message)).collect::<Vec<_>>().join("\n"))?;
    let mut machine = Machine::new(&program, &[], Budgets::default(), &Cores);
    match machine.start() {
        Outcome::Completed { value, .. } => Ok(show_value(&value)),
        Outcome::Failed(failure) => Err(format!("{}: {}", failure.kind.name(), failure.message)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_re_function_yields_its_documented_result_on_a_table_of_inputs() {
    let table: &[(&str, &str)] = &[
        (r#"(re.find #"(\d+)-(\d+)" "ab 12-345 x")"#, r#"(some ["12-345" "12" "345"])"#),
        (r#"(re.find #"\d+" "no digits")"#, "none"),
        (r#"(re.find #"a(b)?(c)" "ac")"#, r#"(some ["ac" "" "c"])"#),
        (r#"(re.find #"(?P<key>\w+)=(?P<value>\w+)" "a=1")"#, r#"(some ["a=1" "a" "1"])"#),
        (r#"(re.find #"x*" "abc")"#, r#"(some [""])"#),
        (r#"(re.find #"(本)(.)" "日本語")"#, r#"(some ["本語" "本" "語"])"#),
        (r#"(re.find #"(.)é" "café")"#, r#"(some ["fé" "f"])"#),
        (r#"(re.find #"(?i)ERROR" "an error")"#, r#"(some ["error"])"#),
        (r#"(re.find #"main\.c:(\d+):(\d+): error: (.*)" "main.c:9:18: error: expected ';'")"#, r#"(some ["main.c:9:18: error: expected ';'" "9" "18" "expected ';'"])"#),
        (r#"(re.find-all #"\d+" "a1b22c333")"#, r#"[["1"] ["22"] ["333"]]"#),
        (r#"(re.find-all #"(\w)=(\d)" "a=1 b=2 c=x")"#, r#"[["a=1" "a" "1"] ["b=2" "b" "2"]]"#),
        (r#"(re.find-all #"z" "abc")"#, "[]"),
        (r#"(re.find-all #"x*" "axb")"#, r#"[[""] ["x"] [""]]"#),
        (r#"(re.find-all #"" "ab")"#, r#"[[""] [""] [""]]"#),
        (r#"(re.find-all #"" "é")"#, r#"[[""] [""]]"#),
        (r#"(re.find-all #"." "aé😀")"#, r#"[["a"] ["é"] ["😀"]]"#),
        (r#"(re.find-all #"(a)|(b)" "ab")"#, r#"[["a" "a" ""] ["b" "" "b"]]"#),
        (r#"(re.matches #"\d+" "123")"#, r#"(some ["123"])"#),
        (r#"(re.matches #"\d+" "123a")"#, "none"),
        (r#"(re.matches #"\d+" "a123")"#, "none"),
        (r#"(re.matches #"a|ab" "ab")"#, r#"(some ["ab"])"#),
        (r#"(re.matches #"(\d+)-(\d+)" "1-2")"#, r#"(some ["1-2" "1" "2"])"#),
        (r#"(re.matches #"(?i)ab" "AB")"#, r#"(some ["AB"])"#),
        (r#"(re.matches #"" "")"#, r#"(some [""])"#),
        (r#"(re.matches #"." "😀")"#, r#"(some ["😀"])"#),
        (r#"(re.test #"^v\d+$" "v12")"#, "true"),
        (r#"(re.test #"^v\d+$" "v12\n")"#, "false"),
        (r#"(re.test #"warn" "no")"#, "false"),
        (r#"(re.replace #"(\d+)-(\d+)" "10-20 30-40" "${2}-${1}")"#, r#""20-10 40-30""#),
        (r#"(re.replace #"\s+" "a  b \n c" " ")"#, r#""a b c""#),
        (r#"(re.replace #"é" "café été" "e")"#, r#""cafe ete""#),
        (r#"(re.replace #"x*" "abc" "-")"#, r#""-a-b-c-""#),
        (r#"(re.replace #"a" "banana" "$$")"#, r#""b$n$n$""#),
        (r#"(re.replace #"(?P<n>\d+)" "v12" "<${n}>")"#, r#""v<12>""#),
        (r#"(re.replace #"(a)" "a" "${2}")"#, r#""""#),
        (r#"(re.replace #"\." "a.b.c" "\\")"#, r#""a\\b\\c""#),
        (r#"(re.split #"\s*,\s*" "a , b,c")"#, r#"["a" "b" "c"]"#),
        (r#"(re.split #"," "")"#, r#"[""]"#),
        (r#"(re.split #"," "a,,b,")"#, r#"["a" "" "b" ""]"#),
        (r#"(re.split #"" "ab")"#, r#"["" "a" "b" ""]"#),
        (r#"(re.split #"、" "日、本")"#, r#"["日" "本"]"#),
        (r#"(re.split #"(,)" "a,b")"#, r#"["a" "b"]"#),
        (r#"(re.find (str "a" "+") "caab")"#, r#"(some ["aa"])"#),
    ];
    for (source, expected) in table {
        assert_eq!(run(source).as_deref(), Ok(*expected), "{source}");
    }
}

#[test]
fn a_literal_pattern_that_does_not_compile_is_rejected_at_check_time_at_its_position() {
    let registry = registry();
    let grants = Grants::new();
    let source = "(re.test #\"(a\" \"x\")";
    let compiled = compile_source(source, &registry, Authority::Granted(&grants), &BTreeMap::new(), Budgets::default());
    assert!(compiled.program.is_none());
    let diagnostic = compiled.diagnostics.first().expect("a diagnostic");
    assert_eq!(diagnostic.code, "bad-literal");
    assert_eq!((diagnostic.span.start.line, diagnostic.span.start.column), (1, 10));
    assert!(diagnostic.message.contains("re.test"), "{}", diagnostic.message);
    assert!(diagnostic.message.contains("open group without closing"), "{}", diagnostic.message);
}

#[test]
fn only_the_pattern_argument_is_checked_as_a_pattern() {
    assert_eq!(run("(re.replace #\"a\" \"(a\" \"(\")").as_deref(), Ok("\"((\""));
    assert_eq!(run("(re.test #\"a\" \"(\")").as_deref(), Ok("false"));
}

#[test]
fn a_computed_pattern_that_does_not_compile_fails_with_invalid_input() {
    let failure = run("(re.test (str \"(\" \"a\") \"x\")").expect_err("fails");
    assert!(failure.starts_with("invalid-input: re.test: "), "{failure}");
    assert!(failure.contains("open group without closing"), "{failure}");
}

#[test]
fn a_pattern_wrapped_for_whole_string_matching_cannot_hide_an_invalid_one() {
    let failure = run("(re.matches (str \"a)\" \"(b\") \"ab\")").expect_err("fails");
    assert!(failure.starts_with("invalid-input: re.matches: "), "{failure}");
}

#[test]
fn a_pattern_whose_compiled_size_exceeds_the_limit_is_refused() {
    let failure = run("(re.test (str \"((((a{100}){100}){100}){100})\") \"x\")").expect_err("fails");
    assert!(failure.starts_with("invalid-input: re.test: "), "{failure}");
    let rejected = run("(re.test #\"((((a{100}){100}){100}){100})\" \"x\")").expect_err("rejected");
    assert!(rejected.starts_with("bad-literal: "), "{rejected}");
}

#[test]
fn every_re_example_produces_its_documented_result() {
    let registry = registry();
    let grants = Grants::new();
    let mut checked = 0;
    for function in registry.functions() {
        let example = function.example.as_ref().expect("every function has an example");
        let compiled = compile_source(&example.program, &registry, Authority::Granted(&grants), &BTreeMap::new(), Budgets::default());
        let program = compiled.program.unwrap_or_else(|| panic!("{}: {:?}", function.name, compiled.diagnostics));
        let mut machine = Machine::new(&program, &[], Budgets::default(), &Cores);
        match machine.start() {
            Outcome::Completed { value, .. } => assert_eq!(show_value(&value), example.result, "{}", function.name),
            other => panic!("{}: {other:?}", function.name),
        }
        checked += 1;
    }
    assert_eq!(checked, 6);
}
