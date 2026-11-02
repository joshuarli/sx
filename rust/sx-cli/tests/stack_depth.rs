use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sx_kernel::authority::{Authority, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::compile_source;
use sx_kernel::export::{export_program, read_program_text};
use sx_kernel::json::{Json, Layout, dumps};
use sx_kernel::registry::Registry;
use sx_kernel::stack::{is_sized, on_sized_stack, on_stack_of};

fn scratch() -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("stack-depth");
    fs::create_dir_all(&directory).expect("scratch directory");
    directory
}

fn write(name: &str, contents: &str) -> String {
    let path = scratch().join(name);
    fs::write(&path, contents).expect("scratch file");
    path.to_string_lossy().into_owned()
}

fn run(args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_sx")).args(args).output().expect("the sx binary runs");
    let code = output.status.code().expect("the binary exits instead of dying on a signal");
    (code, String::from_utf8_lossy(&output.stdout).into_owned())
}

fn nested_lambdas(levels: usize) -> String {
    format!("{}x{}", "(map (fn [x] ".repeat(levels), ") [1])".repeat(levels))
}

#[test]
fn the_deepest_programs_the_checker_follows_never_overflow_the_stack() {
    for (name, source) in [
        ("lambdas-at-the-nesting-limit", nested_lambdas(250)),
        ("lambdas-past-the-nesting-limit", nested_lambdas(300)),
        ("lists-at-the-source-limit", format!("{}{}", "[".repeat(32768), "]".repeat(32768))),
        ("forms-at-the-source-limit", format!("{}{}", "(".repeat(32768), ")".repeat(32768))),
        ("records-at-the-source-limit", format!("{}{}", "{".repeat(32768), "}".repeat(32768))),
        ("unclosed-forms", "(".repeat(65536)),
    ] {
        let file = write(&format!("{name}.sx"), &source);
        for command in [&["check", &file][..], &["check", &file, "--emit", "ir"], &["explain", &file], &["run", &file]] {
            let (code, _) = run(command);
            assert!(code == 0 || code == 1, "{name} {command:?} exited {code}");
        }
    }
}

#[test]
fn the_deepest_accepted_program_exports_and_runs() {
    let accepted = format!("{}1{}", "(attempt ".repeat(127), ")".repeat(127));
    let file = write("attempts.sx", &accepted);
    assert_eq!(run(&["check", &file]).0, 0);
    let (code, ir) = run(&["check", &file, "--emit", "ir"]);
    assert_eq!(code, 0);
    assert!(ir.starts_with("{\"body\""));
    assert_eq!(run(&["run", &file]).0, 0);
}

#[test]
fn json_nested_to_the_limit_round_trips_through_run() {
    let levels = 998;
    let declarations = write(
        "deep-input.json",
        &format!("{{\"inputs\": {{\"x\": \"{}Int{}\"}}}}", "List<".repeat(levels), ">".repeat(levels)),
    );
    let program = write("deep-input.sx", "x");
    let input = format!("x={}1{}", "[".repeat(levels), "]".repeat(levels));
    for format in ["text", "json"] {
        let (code, _) = run(&["run", &program, "--declarations", &declarations, "--input", &input, "--format", format]);
        assert_eq!(code, 0, "{format}");
    }
}

#[test]
fn declared_types_are_bounded_instead_of_recursing_without_limit() {
    let chain: Vec<String> = (1..2000).map(|i| format!("\"T{i}\": \"List<T{}>\"", i - 1)).collect();
    let declarations = write("chain.json", &format!("{{\"types\": {{\"T0\": \"Int\", {}}}}}", chain.join(", ")));
    let (code, _) = run(&["help", "T5", "--declarations", &declarations]);
    assert_eq!(code, 64);
    let written = write(
        "written.json",
        &format!("{{\"types\": {{\"T\": \"{}Int{}\"}}}}", "List<".repeat(200_000), ">".repeat(200_000)),
    );
    assert_eq!(run(&["help", "T", "--declarations", &written]).0, 64);
}

fn deep_attempt_ir(levels: usize) -> String {
    let mut body = Json::object([
        ("node", Json::string("Literal")),
        ("value", Json::object([("int", Json::Int(1))])),
        ("type", Json::string("Int")),
        ("span", Json::Array(vec![Json::Int(1), Json::Int(1), Json::Int(1), Json::Int(2)])),
    ]);
    for _ in 0..levels {
        body = Json::object([("node", Json::string("Attempt")), ("body", body), ("span", Json::Array(vec![Json::Int(1); 4]))]);
    }
    let program = Json::object([
        ("body", body),
        ("result", Json::string("Int")),
        ("capabilities", Json::Array(Vec::new())),
        ("inputs", Json::object::<&str>([])),
        ("enums", Json::object::<&str>([])),
        ("signatures", Json::object([("capabilities", Json::object::<&str>([])), ("functions", Json::object::<&str>([]))])),
    ]);
    dumps(&program, Layout::Spaced).expect("serializable")
}

#[test]
fn the_ir_reader_and_exporter_follow_the_deepest_ir_on_any_thread() {
    assert!(!is_sized());
    let text = deep_attempt_ir(480);
    let program = read_program_text(&text).expect("the reader sizes its own stack");
    let again = on_sized_stack(|| dumps(&export_program(&program), Layout::Spaced)).expect("thread").expect("json");
    assert_eq!(again.matches("\"Attempt\"").count(), 480);
}

#[test]
fn a_stack_too_small_for_the_nesting_is_reported_instead_of_overflowing() {
    let registry = Registry::new();
    let source = nested_lambdas(249);
    let checked = on_stack_of(24 * 1024 * 1024, || {
        compile_source(&source, &registry, Authority::Granted(&Grants::new()), &BTreeMap::new(), Budgets::default())
    })
    .expect("thread");
    match checked.program {
        Some(_) => assert!(checked.diagnostics.is_empty()),
        None => assert!(checked.diagnostics.iter().all(|d| d.code == "too-large")),
    }
}
