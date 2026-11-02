use std::fs;
use std::path::{Path, PathBuf};

use sx_kernel::diagnostics::render;
use sx_kernel::reader::read;
use sx_kernel::syntax::show;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/read")
}

fn outcome(source: &str) -> String {
    let result = read(source);
    match &result.program {
        Some(program) => format!("ok: {}\n", show(program)),
        None => render(source, &result.diagnostics),
    }
}

fn programs() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(fixtures())
        .expect("fixtures/read exists")
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "sx"))
        .collect();
    found.sort();
    found
}

#[test]
fn every_read_fixture_is_byte_identical() {
    let programs = programs();
    assert!(!programs.is_empty());
    let mut failures = Vec::new();
    for program in &programs {
        let source = fs::read_to_string(program).expect("readable program");
        let expected = fs::read_to_string(program.with_extension("expected")).expect("expected output");
        let actual = outcome(&source);
        if actual != expected {
            failures.push(format!("{}\n--- expected\n{expected}--- actual\n{actual}", program.display()));
        }
    }
    assert!(failures.is_empty(), "{} of {} differ:\n{}", failures.len(), programs.len(), failures.join("\n"));
}

#[test]
fn canonical_form_reads_back_to_itself() {
    for program in programs() {
        let source = fs::read_to_string(&program).expect("readable program");
        let Some(node) = read(&source).program else {
            continue;
        };
        let canonical = show(&node);
        let again = read(&canonical).program.expect("canonical form reads");
        assert_eq!(show(&again), canonical, "{}", program.display());
    }
}

#[test]
fn reports_more_errors_than_the_limit_as_a_count() {
    let source = vec!["1."; 23].join(" ");
    let text = render(&source, &read(&format!("[{source}]")).diagnostics);
    assert_eq!(text.matches("error[bad-literal]").count(), 20);
    assert!(text.ends_with("3 more errors not shown.\n"));
}

#[test]
fn error_after_an_unclosed_bracket_is_still_reported() {
    let result = read("(let [x 1.]\n  (f \"a\\q\"");
    let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["bad-literal", "bad-escape", "unclosed", "unclosed"]);
}

#[test]
fn deep_nesting_reads_and_drops_without_overflow() {
    let depth = 70_000;
    let source = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    let result = read(&source);
    let node = result.program.expect("deep program reads");
    assert_eq!(show(&node).len(), source.len());
    let record = format!("{}1{}", "{:a ".repeat(depth), "}".repeat(depth));
    assert!(read(&record).program.is_some());
}
