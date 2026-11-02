use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use sx_kernel::json::{Json, Layout, dumps, parse};

const MISCOUNTED_AT_END: &str = "(let [a 1\n      b 2]\n  (+ a b)))\n";
const MISSING_AT_END: &str = "(let [a 1\n      b 2]\n  (+ a b\n";
const TYPE_ERROR_AFTER_INFERENCE: &str = "(let [a 1\n      b \"x\"]\n  (+ a b)))\n";
const AMBIGUOUS: &str = "(let [f (fn [x]\n    (let [y x]\n      (if y\n        (g y)\n        x))]\n  (f 2))\n";
const BALANCED: &str = "(let [a 1\n      b 2]\n  (+ a b))\n";

fn program(name: &str, text: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("closer-inference-{name}.sx"));
    fs::write(&path, text).expect("scratch file is writable");
    path
}

fn sx(args: &[&str], file: &PathBuf) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sx"))
        .args(args)
        .arg(file)
        .stdin(Stdio::null())
        .output()
        .expect("sx runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("utf-8 output")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("utf-8 output")
}

fn edits(output: &Output) -> Json {
    let report = parse(&stdout(output)).expect("JSON output");
    report.get("inferred_closers").cloned().expect("inferred_closers present")
}

fn assert_json(actual: &Json, expected: &Json) {
    let shown = |json: &Json| dumps(json, Layout::Spaced).expect("serializable");
    assert_eq!(shown(actual), shown(expected));
}

fn edit(line: i64, column: i64, written: &str, used: &str) -> Json {
    Json::object([
        ("line", Json::Int(line)),
        ("column", Json::Int(column)),
        ("written", Json::string(written)),
        ("used", Json::string(used)),
    ])
}

#[test]
fn check_runs_the_inferred_program_and_notes_it_on_stderr() {
    let output = sx(&["check"], &program("extra", MISCOUNTED_AT_END));
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "ok: Int\n");
    assert_eq!(
        stderr(&output),
        "sx: the brackets did not balance, so closers were inferred from indentation: line 3 wrote \")))\" used \"))\"\n"
    );
}

#[test]
fn check_json_lists_each_inferred_edit() {
    let output = sx(&["check", "--format", "json"], &program("extra-json", MISCOUNTED_AT_END));
    assert_eq!(output.status.code(), Some(0));
    assert_json(&edits(&output), &Json::Array(vec![edit(3, 9, ")))", "))")]));
    assert_eq!(stderr(&output), "");
}

#[test]
fn closers_that_were_missing_are_added_before_the_end_of_the_line() {
    let output = sx(&["check", "--format", "json"], &program("missing", MISSING_AT_END));
    assert_eq!(output.status.code(), Some(0));
    assert_json(&edits(&output), &Json::Array(vec![edit(3, 9, "", "))")]));
}

#[test]
fn a_balanced_program_is_reported_without_an_inference() {
    let file = program("balanced", BALANCED);
    let text = sx(&["check"], &file);
    assert_eq!(stderr(&text), "");
    let json = sx(&["check", "--format", "json"], &file);
    assert!(parse(&stdout(&json)).expect("JSON output").get("inferred_closers").is_none());
}

#[test]
fn emitted_ir_is_the_ir_of_the_inferred_program() {
    let inferred = sx(&["check", "--emit", "ir"], &program("ir-inferred", MISCOUNTED_AT_END));
    let balanced = sx(&["check", "--emit", "ir"], &program("ir-balanced", BALANCED));
    assert_eq!(stdout(&inferred), stdout(&balanced));
    assert!(stderr(&inferred).contains("line 3 wrote"));
}

#[test]
fn run_and_explain_carry_the_inference() {
    let file = program("run", MISCOUNTED_AT_END);
    let run = sx(&["run", "--format", "json"], &file);
    assert_eq!(run.status.code(), Some(0));
    let report = parse(&stdout(&run)).expect("JSON output");
    assert_json(report.get("result").expect("result"), &Json::Int(3));
    assert_json(&edits(&run), &Json::Array(vec![edit(3, 9, ")))", "))")]));
    let explain = sx(&["explain", "--format", "json"], &file);
    assert_eq!(explain.status.code(), Some(0));
    assert_json(&edits(&explain), &Json::Array(vec![edit(3, 9, ")))", "))")]));
    let text = sx(&["run"], &file);
    assert_eq!(stdout(&text), "3\n");
    assert!(stderr(&text).starts_with("sx: the brackets did not balance"));
}

#[test]
fn a_program_the_checker_rejects_after_inference_is_reported_against_the_inferred_text() {
    let file = program("type-error", TYPE_ERROR_AFTER_INFERENCE);
    let text = sx(&["check"], &file);
    assert_eq!(text.status.code(), Some(1));
    assert!(stdout(&text).contains("error[type-mismatch]"));
    assert!(stdout(&text).contains("  (+ a b))\n"));
    assert!(!stdout(&text).contains("(+ a b)))"));
    assert!(stderr(&text).contains("line 3 wrote \")))\" used \"))\""));
    let json = sx(&["check", "--format", "json"], &file);
    assert_eq!(json.status.code(), Some(1));
    assert_json(&edits(&json), &Json::Array(vec![edit(3, 9, ")))", "))")]));
}

#[test]
fn an_ambiguous_program_keeps_the_reader_diagnostics() {
    let file = program("ambiguous", AMBIGUOUS);
    let text = sx(&["check"], &file);
    assert_eq!(text.status.code(), Some(1));
    assert!(stdout(&text).contains("error[unexpected-close]"));
    assert_eq!(stderr(&text), "");
    let json = sx(&["check", "--format", "json"], &file);
    assert!(parse(&stdout(&json)).expect("JSON output").get("inferred_closers").is_none());
}

fn serve(messages: &[&str]) -> Vec<Json> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sx"))
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("sx serve starts");
    let mut stdin = child.stdin.take().expect("piped stdin");
    for message in messages {
        writeln!(stdin, "{message}").expect("kernel reads its input");
    }
    drop(stdin);
    let output = child.wait_with_output().expect("kernel exits");
    String::from_utf8(output.stdout)
        .expect("utf-8 output")
        .lines()
        .map(|line| parse(line).expect("reply is JSON"))
        .collect()
}

fn start(program: &str) -> String {
    let message = Json::object([
        ("version", Json::Int(1)),
        ("type", Json::string("start")),
        ("program", Json::string(program)),
        ("allow", Json::Array(Vec::new())),
    ]);
    dumps(&message, Layout::Spaced).expect("serializable")
}

fn explain(program: &str) -> String {
    let message = Json::object([
        ("version", Json::Int(1)),
        ("type", Json::string("explain")),
        ("program", Json::string(program)),
    ]);
    dumps(&message, Layout::Spaced).expect("serializable")
}

#[test]
fn the_host_protocol_reports_the_inference_on_every_reply_about_the_program() {
    let wanted = Json::Array(vec![edit(3, 9, ")))", "))")]);
    let replies = serve(&[&start(MISCOUNTED_AT_END), &explain(MISCOUNTED_AT_END)]);
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0].get("type").and_then(Json::as_str), Some("completed"));
    assert_json(replies[0].get("value").expect("value"), &Json::Int(3));
    assert_json(replies[0].get("inferred_closers").expect("inferred_closers"), &wanted);
    assert_eq!(replies[1].get("type").and_then(Json::as_str), Some("explained"));
    assert_json(replies[1].get("inferred_closers").expect("inferred_closers"), &wanted);
}

#[test]
fn the_host_protocol_marks_a_rejection_after_inference_and_leaves_other_replies_alone() {
    let replies = serve(&[&start(TYPE_ERROR_AFTER_INFERENCE), &start(BALANCED), &start(AMBIGUOUS)]);
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0].get("type").and_then(Json::as_str), Some("rejected"));
    assert_json(replies[0].get("inferred_closers").expect("inferred_closers"), &Json::Array(vec![edit(3, 9, ")))", "))")]));
    let rendered = replies[0].get("rendered").and_then(Json::as_str).expect("rendered text");
    assert!(rendered.contains("  (+ a b))\n"));
    assert_eq!(replies[1].get("type").and_then(Json::as_str), Some("completed"));
    assert!(replies[1].get("inferred_closers").is_none());
    assert_eq!(replies[2].get("type").and_then(Json::as_str), Some("rejected"));
    assert!(replies[2].get("inferred_closers").is_none());
}

#[test]
fn the_host_protocol_reports_the_inference_with_the_first_requests() {
    let message = Json::object([
        ("version", Json::Int(1)),
        ("type", Json::string("start")),
        ("program", Json::string("(let [a (call store.get {:id \"a\"})\n      b 1]\n  (get a :size)))\n")),
        ("allow", Json::Array(vec![Json::string("store.get")])),
        (
            "declarations",
            Json::object([(
                "capabilities",
                Json::Array(vec![Json::object([
                    ("id", Json::string("store.get")),
                    ("effect", Json::string("read")),
                    ("input", Json::string("Record {id: String}")),
                    ("output", Json::string("Record {id: String, size: Int}")),
                    ("description", Json::string("Reads one item.")),
                ])]),
            )]),
        ),
    ]);
    let replies = serve(&[&dumps(&message, Layout::Spaced).expect("serializable")]);
    assert_eq!(replies[0].get("type").and_then(Json::as_str), Some("requests"));
    assert_json(
        replies[0].get("inferred_closers").expect("inferred_closers"),
        &Json::Array(vec![edit(3, 15, ")))", "))")]),
    );
}

const DECIDED_BY_THE_CHECKER: &str = "(let [f (fn [x]\n    (let [y x]\n      (if (= y 1)\n        (+ y 1)\n        x))]\n  (f 2))\n";

#[test]
fn the_checker_decides_between_repairs_that_indentation_cannot_rank() {
    let file = program("decided", DECIDED_BY_THE_CHECKER);
    let output = sx(&["check", "--format", "json"], &file);
    assert_eq!(output.status.code(), Some(0));
    assert_json(&edits(&output), &Json::Array(vec![edit(5, 10, "))]", ")))]")]));
}
